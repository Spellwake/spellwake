//! Strict zero-regex recursive-descent parsers for Costs, Triggers, Effects, and Statics/Replacements.

use crate::ast::{
    ConditionAst, ControllerRelation, CostAtomAst, DelayedStepScope, DurationAst, ExchangeKind,
    GameMarker, InstructionAst, InteractiveChoiceKind, ObjectFilterAst, ObjectFilterFlags,
    ParseFailureAst, PlayerRefAst, ReplacementEffectAst, StatKind, StaticAbilityAst, SubjectRefAst,
    TriggerEventAst, TriggerSpecAst, TurnStepAst, ValueExprAst,
};
use crate::compile_granted_ability;
use crate::filter::{
    parse_condition, parse_for_each_value_expr, parse_keyword_list, parse_mana_spec,
    parse_object_filter, parse_player_ref, parse_subject_or_target, parse_token_creation,
    parse_value_expr,
};
use crate::preprocess::{
    EM_DASH, count_symbol_sequence, is_mana_sequence, is_physical_tabletop_clause, parse_base_pt,
    parse_pt_mod, split_sentences, split_top_level,
};
use crate::vocab::{Vocabulary, parse_number_word};

fn trim_punct(s: &str) -> &str {
    s.trim().trim_end_matches(['.', ',', ';'])
}

thread_local! {
    static PARSE_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static PARSE_OPS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

struct DepthGuard;

impl DepthGuard {
    #[inline]
    fn enter() -> Option<Self> {
        PARSE_DEPTH.with(|d| {
            let cur = d.get();
            if cur == 0 {
                PARSE_OPS.with(|ops| ops.set(1));
                d.set(1);
                return Some(Self);
            }
            if cur >= 5 {
                return None;
            }
            let allowed = PARSE_OPS.with(|ops| {
                let n = ops.get();
                if n >= 350 {
                    false
                } else {
                    ops.set(n + 1);
                    true
                }
            });
            if !allowed {
                return None;
            }
            d.set(cur + 1);
            Some(Self)
        })
    }
}

impl Drop for DepthGuard {
    #[inline]
    fn drop(&mut self) {
        PARSE_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

// ---------------------------------------------------------------------------
// 1. COST PARSER
// ---------------------------------------------------------------------------

/// Parse a comma-separated activated-ability cost list into orthogonal `CostAtomAst` items.
pub fn parse_cost_list(cost_str: &str, vocab: &Vocabulary) -> Result<Vec<CostAtomAst>, String> {
    let raw_atoms = split_top_level(cost_str, ",");
    if raw_atoms.is_empty() {
        return Err("empty_cost_list".to_string());
    }
    // Merge English intra-clause comma continuations (starting with a lowercase letter, e.g.
    // "Pay half your life, rounded up", "Sacrifice a blue creature, a black creature, and a red creature",
    // "Remove a counter from an artifact, creature, land, or planeswalker you control").
    let mut atoms: Vec<String> = Vec::with_capacity(raw_atoms.len());
    for chunk in raw_atoms {
        let trimmed = chunk.trim();
        let starts_lower = trimmed
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase());
        if starts_lower && let Some(prev) = atoms.last_mut() {
            prev.push_str(", ");
            prev.push_str(trimmed);
        } else {
            atoms.push(trimmed.to_string());
        }
    }
    let mut out = Vec::with_capacity(atoms.len());
    for atom in &atoms {
        match parse_cost_atom(atom, vocab) {
            Ok(c) => out.push(c),
            Err(orig_err) => {
                // Handle "Remove three quest counters from ~ and sacrifice it" / "Exile a creature card from your graveyard and pay its mana cost"
                if let Some((l, r)) = atom.split_once(" and ")
                    && let (Ok(lc), Ok(rc)) = (
                        parse_cost_atom(l.trim(), vocab),
                        parse_cost_atom(r.trim(), vocab),
                    )
                {
                    out.push(lc);
                    out.push(rc);
                } else if let Ok(single) = parse_cost_atom(cost_str, vocab) {
                    return Ok(vec![single]);
                } else {
                    return Err(orig_err);
                }
            }
        }
    }
    Ok(out)
}

/// Parse one atomic activation or additional cast cost.
pub fn parse_cost_atom(atom: &str, vocab: &Vocabulary) -> Result<CostAtomAst, String> {
    let a = trim_punct(atom);
    if a.is_empty() {
        return Err("empty_cost_atom".to_string());
    }
    if is_mana_sequence(a) {
        return Ok(CostAtomAst::PayMana(a.to_string()));
    }
    if a.eq_ignore_ascii_case("twice {X}") {
        return Ok(CostAtomAst::PayScaledMana {
            base: "{X}".to_string(),
            per: "twice".to_string(),
        });
    }
    // Scaled mana cost: "[Pay] {1} for each <filter>" / "{2} plus an additional {1} for each <filter>"
    let a_nopay = a
        .strip_prefix("Pay ")
        .or_else(|| a.strip_prefix("pay "))
        .unwrap_or(a);
    if let Some((base, fe)) = a_nopay.split_once(" for each ")
        && (is_mana_sequence(base.trim())
            || base.to_ascii_lowercase().contains("plus an additional"))
    {
        return Ok(CostAtomAst::PayScaledMana {
            base: base.trim().to_string(),
            per: fe.trim().to_string(),
        });
    }
    if let Some((l, r)) = a.split_once(" or ")
        && is_mana_sequence(l.trim())
        && is_mana_sequence(r.trim())
    {
        return Ok(CostAtomAst::PayManaChoice(vec![
            l.trim().to_string(),
            r.trim().to_string(),
        ]));
    }
    // 3+ way mana choice in costs (e.g. "{G}, {W}, or {U}" on Isu the Abominable)
    if a.contains(", or ") && a.starts_with('{') {
        let norm = a.replace(", or ", " or ").replace(", ", " or ");
        let parts: Vec<&str> = norm.split(" or ").map(str::trim).collect();
        if parts.len() >= 2 && parts.iter().all(|p| is_mana_sequence(p)) {
            return Ok(CostAtomAst::PayManaChoice(
                parts.into_iter().map(ToString::to_string).collect(),
            ));
        }
    }
    // Disjunctive cost ("reveal a Dinosaur card from your hand or pay {1}")
    if let Some((left, right)) = a.split_once(" or pay ")
        && let (Ok(lc), Ok(rc)) = (
            parse_cost_atom(left.trim(), vocab),
            parse_cost_atom(right.trim(), vocab),
        )
    {
        return Ok(CostAtomAst::Or(Box::new(lc), Box::new(rc)));
    }
    if a == "{T}" {
        return Ok(CostAtomAst::TapSelf);
    }
    if a == "{Q}" {
        return Ok(CostAtomAst::UntapSelf);
    }
    if a.eq_ignore_ascii_case("chaos") {
        return Ok(CostAtomAst::ChaosCost);
    }
    if let Some(n) = count_symbol_sequence(a, "{E}") {
        return Ok(CostAtomAst::PayEnergy(n));
    }
    if let Some(n) = count_symbol_sequence(a, "{TK}") {
        return Ok(CostAtomAst::PayTickets(n));
    }
    if let Some(cnt_tok) = a.strip_suffix(" {E}") {
        let ct = cnt_tok.trim();
        let ct_low = ct.to_ascii_lowercase();
        if let Some(n) = parse_number_word(&ct_low) {
            return Ok(CostAtomAst::PayEnergy(u16::try_from(n.max(1)).unwrap_or(1)));
        }
        if let Ok(val) = parse_value_expr(ct, vocab) {
            return Ok(CostAtomAst::PayEnergyExpr(Box::new(val)));
        }
    }
    if let Some(cnt_tok) = a.strip_suffix(" {TK}")
        && let Some(n) = parse_number_word(&cnt_tok.trim().to_ascii_lowercase())
    {
        return Ok(CostAtomAst::PayTickets(
            u16::try_from(n.max(1)).unwrap_or(1),
        ));
    }

    let low = a.to_ascii_lowercase();
    // Strip trailing ", where X is <val>" in costs (Urgent Necropsy, etc.)
    if let Some(wx_idx) = low.rfind(", where x is ")
        && parse_value_expr(&a[wx_idx + ", where x is ".len()..], vocab).is_ok()
        && let Ok(inner) = parse_cost_atom(a[..wx_idx].trim(), vocab)
    {
        return Ok(inner);
    }
    if low == "discard the last card you drew this turn"
        || low.starts_with("discard a legendary card with the same name as ")
    {
        return Ok(CostAtomAst::Discard {
            count: Box::new(ValueExprAst::Const(1)),
            filter: Box::new(crate::ast::ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
            random: false,
        });
    }
    if low == "reveal two cards from your hand that share a color" {
        return Ok(CostAtomAst::Reveal(Box::new(SubjectRefAst::Anaphora(
            "two cards from your hand that share a color".to_string(),
        ))));
    }
    if let Some(after_ex) = low.strip_prefix("exile any number of ")
        && let Some((flt_part, _)) = after_ex.split_once(" with ")
        && after_ex.contains(" mana symbols among their mana costs")
        && let Ok(flt) = parse_object_filter(flt_part.trim(), vocab)
    {
        return Ok(CostAtomAst::Exile {
            count: Box::new(ValueExprAst::ChosenX),
            filter: Box::new(flt),
        });
    }
    // Wave 17 general cost shapes (Araumi, Phyrexian Splicer, A Killer Among Us, The Book of Vile Darkness, Primitive Justice, Celestial Reunion)
    if let Some(val_raw) = low.strip_prefix("exile cards from your graveyard equal to ")
        && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Ok(CostAtomAst::Exile {
            count: Box::new(val),
            filter: Box::new(crate::ast::ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
        });
    }
    if let Some(flt_raw) = low.strip_prefix("exile ~ and ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Ok(CostAtomAst::Exile {
            count: Box::new(ValueExprAst::Const(1)),
            filter: Box::new(flt),
        });
    }
    if low == "reveal the creature type you chose" {
        return Ok(CostAtomAst::Reveal(Box::new(SubjectRefAst::Anaphora(low))));
    }
    if let Some(after_ch) = low.strip_prefix("choose ")
        && (parse_keyword_list(after_ch, vocab).is_some()
            || after_ch.starts_with("a creature type and behold "))
    {
        return Ok(CostAtomAst::Choose(Box::new(SubjectRefAst::Anaphora(
            after_ch.to_string(),
        ))));
    }
    if let Some(after_pay) = a.strip_prefix("Pay ").or_else(|| a.strip_prefix("pay "))
        && let Some(core_m) = after_pay.strip_suffix(" any number of times")
        && let Some((m1, m2)) = core_m.split_once(" and/or ")
        && is_mana_sequence(m1.trim())
        && is_mana_sequence(m2.trim())
    {
        return Ok(CostAtomAst::PayManaChoice(vec![
            m1.trim().to_string(),
            m2.trim().to_string(),
        ]));
    }
    if let Some(after) = a.strip_prefix("Pay ").or_else(|| a.strip_prefix("pay ")) {
        let atrim = after.trim().trim_end_matches(" any number of times").trim();
        if is_mana_sequence(atrim) {
            return Ok(CostAtomAst::PayMana(atrim.to_string()));
        }
        if let Some(n) = count_symbol_sequence(after, "{E}") {
            return Ok(CostAtomAst::PayEnergy(n));
        }
        if let Some(n) = count_symbol_sequence(after, "{TK}") {
            return Ok(CostAtomAst::PayTickets(n));
        }
        if let Some(val_raw) = after
            .to_ascii_lowercase()
            .strip_prefix("an amount of {e} equal to ")
            && let Ok(val) = parse_value_expr(val_raw, vocab)
        {
            return Ok(CostAtomAst::PayEnergyExpr(Box::new(val)));
        }
        if let Some(cnt_tok) = after.strip_suffix(" {E}") {
            let ct = cnt_tok.trim();
            if let Some(n) = parse_number_word(&ct.to_ascii_lowercase()) {
                return Ok(CostAtomAst::PayEnergy(u16::try_from(n.max(1)).unwrap_or(1)));
            }
            if let Ok(val) = parse_value_expr(ct, vocab) {
                return Ok(CostAtomAst::PayEnergyExpr(Box::new(val)));
            }
        }
        if let Some(cnt_tok) = after.strip_suffix(" {TK}")
            && let Some(n) = parse_number_word(&cnt_tok.trim().to_ascii_lowercase())
        {
            return Ok(CostAtomAst::PayTickets(
                u16::try_from(n.max(1)).unwrap_or(1),
            ));
        }
    }
    if low == "put a card from your hand on top of your library" {
        return Ok(CostAtomAst::BounceToHand(Box::new(
            SubjectRefAst::Anaphora("card_from_hand_to_top_library".to_string()),
        )));
    }
    if low == "put a card an opponent owns from exile into that player's graveyard" {
        return Ok(CostAtomAst::ProcessExiledCard);
    }
    if let Some(flt_raw) = low.strip_prefix("behold ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(CostAtomAst::Behold(Box::new(flt)));
    }
    if low == "forage" {
        return Ok(CostAtomAst::Forage);
    }
    if let Some(val_raw) = low.strip_prefix("collect evidence ")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Ok(CostAtomAst::CollectEvidence(Box::new(val)));
    }
    if let Some(val_raw) = low.strip_prefix("blight ")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Ok(CostAtomAst::Blight(Box::new(val)));
    }
    if let Some(after) = a
        .strip_prefix("Waterbend ")
        .or_else(|| a.strip_prefix("waterbend "))
        && is_mana_sequence(after.trim())
    {
        return Ok(CostAtomAst::Waterbend(after.trim().to_string()));
    }
    if low == "pay half your life"
        || low == "pay half your life, rounded up"
        || low == "pay half their life"
        || low == "pay half their life, rounded up"
    {
        return Ok(CostAtomAst::PayHalfLife);
    }
    if let Some(val_raw) = low.strip_prefix("pay life equal to ")
        && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Ok(CostAtomAst::PayLife(Box::new(val)));
    }
    if low == "pay any amount of life" {
        return Ok(CostAtomAst::PayLife(Box::new(ValueExprAst::ChosenX)));
    }
    if low == "pay fifty {e}" {
        return Ok(CostAtomAst::PayEnergy(50));
    }
    if low.starts_with("pay ") && low.ends_with("'s mana cost") {
        return Ok(CostAtomAst::PayMana("dynamic".to_string()));
    }
    if low == "pay its mana cost" {
        return Ok(CostAtomAst::PayMana("its_mana_cost".to_string()));
    }
    if let Some(after_pay) = low.strip_prefix("pay ")
        && let Some((life_part, fe_part)) = after_pay.split_once(" life for each ")
        && let (Ok(base), Ok(per)) = (
            parse_value_expr(life_part, vocab),
            parse_for_each_value_expr(fe_part, vocab),
        )
    {
        return Ok(CostAtomAst::PayLife(Box::new(ValueExprAst::Mul(
            Box::new(base),
            Box::new(per),
        ))));
    }
    if let Some(after) = low.strip_prefix("unattach ") {
        let (subj_raw, from_opt) = match after.rsplit_once(" from ") {
            Some((s_part, f_part)) => (s_part, parse_subject_or_target(f_part, vocab).ok()),
            None => (after, None),
        };
        if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
            return Ok(CostAtomAst::Unattach {
                subject: Box::new(subj),
                from: from_opt.map(Box::new),
            });
        }
    }
    if let Some(die_raw) = low.strip_prefix("roll a d")
        && let Ok(sides) = die_raw.trim().parse::<u8>()
    {
        return Ok(CostAtomAst::RollDie(sides));
    }
    if let Some(rest) = low.strip_prefix("put ")
        && let Some((subj_raw, dest)) = rest
            .split_once(" on the bottom of ")
            .or_else(|| rest.split_once(" on top of "))
    {
        let (_, subj_nocnt) = peel_leading_count(subj_raw, vocab);
        if let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            .or_else(|_| parse_subject_or_target(subj_nocnt, vocab))
        {
            return Ok(CostAtomAst::MoveZone {
                subject: Box::new(subj),
                to: dest.to_string(),
            });
        }
    }
    if low == "reveal your hand" {
        return Ok(CostAtomAst::Reveal(Box::new(SubjectRefAst::Anaphora(
            "your hand".to_string(),
        ))));
    }
    // 3+ way disjunctive verb cost ("Sacrifice a creature, discard a card, or pay 4 life", Dusk Mangler)
    if a.contains(", or ")
        && let Some((before_or, last_c)) = a.rsplit_once(", or ")
        && let Ok(last_atom) = parse_cost_atom(last_c.trim(), vocab)
    {
        let mut cur = last_atom;
        let mut all_ok = true;
        for part in split_top_level(before_or, ",").into_iter().rev() {
            if let Ok(ca) = parse_cost_atom(part.trim(), vocab) {
                cur = CostAtomAst::Or(Box::new(ca), Box::new(cur));
            } else {
                all_ok = false;
                break;
            }
        }
        if all_ok {
            return Ok(cur);
        }
    }
    // Disjunctive verb cost BEFORE single-verb sacrifice/discard/exile/choose
    // ("Sacrifice an artifact or discard a nonland card", "Choose a creature you control or reveal a creature card from your hand")
    if let Some((left, right)) = a.split_once(" or ") {
        let r_trim = right.trim();
        let rlow = r_trim.to_ascii_lowercase();
        let right_is_verb = [
            "pay ",
            "sacrifice ",
            "discard ",
            "exile ",
            "reveal ",
            "choose ",
            "tap ",
            "untap ",
            "return ",
            "remove ",
            "put ",
            "behold ",
            "mill ",
            "blight ",
            "collect ",
        ]
        .iter()
        .any(|p| rlow.starts_with(p));
        if right_is_verb
            && let (Ok(lc), Ok(rc)) = (
                parse_cost_atom(left.trim(), vocab),
                parse_cost_atom(r_trim, vocab),
            )
        {
            return Ok(CostAtomAst::Or(Box::new(lc), Box::new(rc)));
        }
    }
    if let Some(rest) = low.strip_prefix("choose ")
        && (parse_subject_or_target(rest, vocab).is_ok()
            || parse_object_filter(rest, vocab).is_ok()
            || rest.starts_with("a number between ")
            || rest == "a creature type"
            || rest == "a color")
    {
        let subj = parse_subject_or_target(rest, vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(rest.to_string()));
        return Ok(CostAtomAst::Choose(Box::new(subj)));
    }
    if let Some(amt_str) = low.strip_suffix(" life")
        && !low.starts_with("pay ")
        && let Ok(val) = parse_value_expr(amt_str, vocab)
    {
        return Ok(CostAtomAst::PayLife(Box::new(val)));
    }
    if let Some(after) = low.strip_prefix("pay ") {
        if let Some(val_raw) = after.strip_prefix("life equal to ")
            && let Ok(val) = parse_value_expr(val_raw, vocab)
        {
            return Ok(CostAtomAst::PayLife(Box::new(val)));
        }
        if let Some(amt_str) = after.strip_suffix(" life")
            && let Ok(val) = parse_value_expr(amt_str, vocab)
        {
            return Ok(CostAtomAst::PayLife(Box::new(val)));
        }
    }

    if low == "sacrifice ~" || low == "sacrifice it" {
        return Ok(CostAtomAst::SacrificeSelf);
    }
    if let Some(rest) = low.strip_prefix("sacrifice ") {
        if let Some(any_rest) = rest.strip_prefix("any number of ")
            && let Ok(flt) = parse_object_filter(any_rest, vocab)
        {
            return Ok(CostAtomAst::SacrificeAnyNumber(Box::new(flt)));
        }
        if rest.ends_with(" and ~")
            || rest.starts_with("~ and ")
            || rest.starts_with("one or more ")
            || rest.starts_with("any number of ")
            || rest.starts_with("up to ")
            || rest.starts_with("all ")
            || rest.contains(", ")
        {
            return Err(format!("unparsed_cost:{a}"));
        }
        let (cnt, flt_raw) = peel_leading_count(rest, vocab);
        let flt = parse_object_filter(flt_raw, vocab)?;
        return Ok(CostAtomAst::Sacrifice {
            count: Box::new(cnt),
            filter: Box::new(flt),
        });
    }

    if low == "discard ~" || low == "discard this card" {
        return Ok(CostAtomAst::DiscardSelf);
    }
    if low == "discard your hand" {
        return Ok(CostAtomAst::DiscardHand);
    }
    if let Some(rest) = low.strip_prefix("discard ") {
        let (core, random) = match rest.strip_suffix(" at random") {
            Some(c) => (c, true),
            None => (rest, false),
        };
        if core.starts_with("up to ") || core.starts_with("any number of ") {
            return Err(format!("unparsed_cost:{a}"));
        }
        let (cnt, flt_raw) = peel_leading_count(core, vocab);
        let flt = parse_object_filter(flt_raw, vocab)?;
        return Ok(CostAtomAst::Discard {
            count: Box::new(cnt),
            filter: Box::new(flt),
            random,
        });
    }

    if low == "exile ~" || low == "exile it" {
        return Ok(CostAtomAst::ExileSelf {
            from_graveyard: false,
            from_hand: false,
        });
    }
    if low == "exile ~ from your graveyard" || low == "exile this card from your graveyard" {
        return Ok(CostAtomAst::ExileSelf {
            from_graveyard: true,
            from_hand: false,
        });
    }
    if low == "exile ~ from your hand" || low == "exile this card from your hand" {
        return Ok(CostAtomAst::ExileSelf {
            from_graveyard: false,
            from_hand: true,
        });
    }
    if let Some(rest) = low.strip_prefix("exile ") {
        if let Some(top_rest) = rest.strip_prefix("the top ") {
            let (top_core, face_down) = match top_rest.strip_suffix(" face down") {
                Some(c) => (c, true),
                None => (top_rest, false),
            };
            if top_core == "card of your library" {
                return Ok(CostAtomAst::ExileTopLibrary {
                    count: Box::new(ValueExprAst::Const(1)),
                    face_down,
                });
            }
            if let Some(cnt_raw) = top_core.strip_suffix(" cards of your library")
                && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
            {
                return Ok(CostAtomAst::ExileTopLibrary {
                    count: Box::new(cnt),
                    face_down,
                });
            }
        }
        if rest.starts_with("the top ")
            || rest.ends_with(" and ~")
            || rest.starts_with("~ and ")
            || rest.ends_with(" face up")
            || rest.ends_with(" face down")
            || rest.contains(" from a single graveyard")
            || rest.starts_with("any number of ")
            || rest.starts_with("one or more ")
            || rest.contains(" with total mana value ")
        {
            return Err(format!("unparsed_cost:{a}"));
        }
        let (cnt, flt_raw) = peel_leading_count(rest, vocab);
        let flt = parse_object_filter(flt_raw, vocab)?;
        return Ok(CostAtomAst::Exile {
            count: Box::new(cnt),
            filter: Box::new(flt),
        });
    }

    if let Some(rest) = low.strip_prefix("remove ")
        && let Some((left, subj_raw)) = rest
            .split_once(" counters from ")
            .or_else(|| rest.split_once(" counter from "))
    {
        if subj_raw.starts_with("among ") {
            return Err(format!("unparsed_cost:{a}"));
        }
        let subj = parse_subject_or_target(subj_raw, vocab)?;
        if let Some(kind) = left.strip_prefix("all ") {
            return Ok(CostAtomAst::RemoveCounters {
                all: true,
                count: None,
                counter_kind: kind.trim().to_string(),
                subject: Box::new(subj),
            });
        }
        let (cnt, kind) = peel_leading_count(left, vocab);
        return Ok(CostAtomAst::RemoveCounters {
            all: false,
            count: Some(Box::new(cnt)),
            counter_kind: if kind.is_empty() {
                "any".to_string()
            } else {
                kind.to_string()
            },
            subject: Box::new(subj),
        });
    }

    if let Some(rest) = low.strip_prefix("put ")
        && let Some((left, subj_raw)) = rest
            .split_once(" counters on ")
            .or_else(|| rest.split_once(" counter on "))
    {
        let subj = parse_subject_or_target(subj_raw, vocab)?;
        let (cnt, kind) = peel_leading_count(left, vocab);
        return Ok(CostAtomAst::PutCounters {
            count: Box::new(cnt),
            counter_kind: kind.to_string(),
            subject: Box::new(subj),
        });
    }

    if let Some(rest) = low.strip_prefix("tap ") {
        if rest.starts_with("any number of ") {
            return Err(format!("unparsed_cost:{a}"));
        }
        let (cnt, flt_raw) = peel_leading_count(rest, vocab);
        let flt = parse_object_filter(flt_raw, vocab)?;
        return Ok(CostAtomAst::TapPermanent {
            count: Box::new(cnt),
            filter: Box::new(flt),
        });
    }
    if let Some(rest) = low.strip_prefix("untap ") {
        let (cnt, flt_raw) = peel_leading_count(rest, vocab);
        let flt = parse_object_filter(flt_raw, vocab)?;
        return Ok(CostAtomAst::UntapPermanent {
            count: Box::new(cnt),
            filter: Box::new(flt),
        });
    }

    if let Some(rest) = low.strip_prefix("return ")
        && let Some(subj_raw) = rest
            .strip_suffix(" to its owner's hand")
            .or_else(|| rest.strip_suffix(" to their owner's hand"))
            .or_else(|| rest.strip_suffix(" to their owners' hands"))
    {
        let (_, subj_nocnt) = peel_leading_count(subj_raw, vocab);
        let subj = parse_subject_or_target(subj_raw, vocab)
            .or_else(|_| parse_subject_or_target(subj_nocnt, vocab))?;
        return Ok(CostAtomAst::BounceToHand(Box::new(subj)));
    }
    if let Some(rest) = low.strip_prefix("put ")
        && let Some(subj_raw) = rest
            .strip_suffix(" into its owner's graveyard")
            .or_else(|| rest.strip_suffix(" into your graveyard"))
    {
        let (_, subj_nocnt) = peel_leading_count(subj_raw, vocab);
        let subj = parse_subject_or_target(subj_raw, vocab)
            .or_else(|_| parse_subject_or_target(subj_nocnt, vocab))?;
        return Ok(CostAtomAst::BounceToHand(Box::new(subj)));
    }

    if let Some(rest) = low.strip_prefix("reveal ") {
        let core = rest.strip_suffix(" from your hand").unwrap_or(rest);
        let core = core.strip_prefix("x ").unwrap_or(core);
        let subj = parse_subject_or_target(core, vocab)?;
        return Ok(CostAtomAst::Reveal(Box::new(subj)));
    }

    if let Some(rest) = low.strip_prefix("mill ")
        && let Some(cnt_raw) = rest
            .strip_suffix(" cards")
            .or_else(|| rest.strip_suffix(" card"))
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Ok(CostAtomAst::Mill(Box::new(cnt)));
    }

    if low == "exert ~" {
        return Ok(CostAtomAst::ExertSelf);
    }

    if is_physical_tabletop_clause(&low) {
        return Ok(CostAtomAst::ManualTabletopAction {
            prompt: a.to_string(),
        });
    }

    // Disjunctive cost: "reveal a Dinosaur card from your hand or pay {1}"
    if let Some((left, right)) = a.split_once(" or ")
        && let (Ok(lc), Ok(rc)) = (
            parse_cost_atom(left.trim(), vocab),
            parse_cost_atom(
                right
                    .trim()
                    .strip_prefix("pay ")
                    .or_else(|| right.trim().strip_prefix("Pay "))
                    .unwrap_or(right.trim()),
                vocab,
            ),
        )
    {
        return Ok(CostAtomAst::Or(Box::new(lc), Box::new(rc)));
    }

    Err(format!("unparsed_cost:{a}"))
}

fn peel_leading_count<'a>(s: &'a str, vocab: &Vocabulary) -> (ValueExprAst, &'a str) {
    let s = s
        .strip_prefix("another ")
        .or_else(|| s.strip_prefix("an additional "))
        .or_else(|| s.strip_prefix("a number of "))
        .or_else(|| s.strip_prefix("number of "))
        .unwrap_or(s);
    if let Some(rest) = s.strip_prefix("that many ") {
        return (ValueExprAst::PriorResult, rest.trim());
    }
    if let Some((first, rest)) = s.split_once(' ') {
        if first == "x" {
            return (ValueExprAst::ChosenX, rest.trim());
        }
        if let Some(n) = parse_number_word(first) {
            return (ValueExprAst::Const(n), rest.trim());
        }
        let _ = vocab;
    }
    (ValueExprAst::Const(1), s.trim())
}

// ---------------------------------------------------------------------------
// 2. TRIGGER HEADER PARSER
// ---------------------------------------------------------------------------

/// Parse a triggered-ability header (including compound events, Planechase, Archenemy,
/// Tabletop Honor-System claims, and intervening-`if`).
pub fn parse_trigger_header(header: &str, vocab: &Vocabulary) -> Result<TriggerSpecAst, String> {
    let raw = header.trim().trim_end_matches(',');
    let low = raw.to_ascii_lowercase();

    let (core_part, intervening_if) = if let Some(idx) = low.find(", if ") {
        let cpart = &raw[..idx];
        let cond_raw = &raw[idx + 5..];
        let cond = parse_condition(cond_raw, vocab).map_err(|e| format!("trigger_if:{e}"))?;
        (cpart.trim(), Some(cond))
    } else if let Some(idx) = low.find(" while ")
        && let Ok(cond) = parse_condition(&raw[idx + 7..], vocab)
    {
        (raw[..idx].trim(), Some(cond))
    } else {
        (raw, None)
    };

    // Compound trigger header: "<ev1> and/or whenever/when <ev2>" / "<ev1> and at the beginning of <step>"
    let clow = core_part.to_ascii_lowercase();
    if let Some((left, right)) = clow
        .split_once(", and whenever ")
        .or_else(|| clow.split_once(" and whenever "))
        .or_else(|| clow.split_once(", and when "))
        .or_else(|| clow.split_once(" and when "))
        .or_else(|| clow.split_once(", or whenever "))
        .or_else(|| clow.split_once(" or whenever "))
        .or_else(|| clow.split_once(", or when "))
    {
        let mut evs = parse_trigger_events(left, vocab)?;
        let right_trig = format!("whenever {right}");
        evs.extend(parse_trigger_events(&right_trig, vocab)?);
        return Ok(TriggerSpecAst {
            events: evs,
            intervening_if,
        });
    }
    if let Some((left, right)) = clow.split_once(" and at the beginning of ") {
        let mut evs = parse_trigger_events(left, vocab)?;
        let right_step = format!("at the beginning of {right}");
        evs.extend(parse_trigger_events(&right_step, vocab)?);
        return Ok(TriggerSpecAst {
            events: evs,
            intervening_if,
        });
    }

    let events = parse_trigger_events(core_part, vocab).or_else(|orig_err| {
        // Compound "When/Whenever <ev1>, <ev2>, or <ev3>" and "<ev1> or <ev2>" trigger splits
        if let Some((pref, rest)) = clow
            .strip_prefix("whenever ")
            .map(|r| ("whenever ", r))
            .or_else(|| clow.strip_prefix("when ").map(|r| ("when ", r)))
        {
            if rest.contains(", or ") {
                let comma_parts: Vec<&str> = rest.split(", ").collect();
                if comma_parts.len() >= 2
                    && comma_parts.len() <= 4
                    && comma_parts
                        .last()
                        .is_some_and(|p| p.trim().starts_with("or "))
                {
                    let mut combined = Vec::new();
                    let mut all_ok = true;
                    for (i, part) in comma_parts.iter().enumerate() {
                        let p_clean = if i + 1 == comma_parts.len() {
                            part.trim().strip_prefix("or ").unwrap_or(part.trim())
                        } else {
                            part.trim()
                        };
                        match parse_trigger_events(&format!("{pref}{p_clean}"), vocab) {
                            Ok(evs) => combined.extend(evs),
                            Err(_) => {
                                all_ok = false;
                                break;
                            }
                        }
                    }
                    if all_ok && combined.len() >= 2 {
                        return Ok(combined);
                    }
                }
            }
            for (idx, _) in rest.match_indices(" or ") {
                let left_ev = format!("{pref}{}", rest[..idx].trim());
                let right_ev = format!("{pref}{}", rest[idx + 4..].trim());
                if let (Ok(mut l), Ok(r)) = (
                    parse_trigger_events(&left_ev, vocab),
                    parse_trigger_events(&right_ev, vocab),
                ) {
                    l.extend(r);
                    return Ok(l);
                }
            }
        }
        Err(orig_err)
    })?;
    Ok(TriggerSpecAst {
        events,
        intervening_if,
    })
}

fn parse_trigger_events(core: &str, vocab: &Vocabulary) -> Result<Vec<TriggerEventAst>, String> {
    let c = core.trim().trim_end_matches(',');
    let low = c.to_ascii_lowercase();

    // Planechase & Archenemy native triggers
    if low == "whenever chaos ensues" || low == "when chaos ensues" {
        return Ok(vec![TriggerEventAst::ChaosEnsues]);
    }
    if low == "at end of combat" {
        return Ok(vec![TriggerEventAst::StepBegan {
            step: TurnStepAst::EndOfCombat,
            whose: "any".to_string(),
        }]);
    }
    if low == "when you planeswalk to ~"
        || low == "when you planeswalk here"
        || low == "whenever you planeswalk to ~"
        || low == "whenever you planeswalk here"
    {
        return Ok(vec![TriggerEventAst::PlaneswalkToSelf]);
    }
    if low == "when you planeswalk away from ~"
        || low == "when you planeswalk away from here"
        || low == "whenever you planeswalk away from ~"
        || low == "whenever you planeswalk away from here"
    {
        return Ok(vec![TriggerEventAst::PlaneswalkAwayFromSelf]);
    }
    if low == "when you encounter ~" {
        return Ok(vec![TriggerEventAst::EncounterPhenomenon]);
    }
    if low == "when you set ~ in motion" || low == "when you set this scheme in motion" {
        return Ok(vec![TriggerEventAst::SchemeSetInMotion]);
    }
    // CR 603.8 Counter threshold state trigger ("When there are 1,000 or more time counters on ~", The Millennium Calendar)
    if let Some(after) = low.strip_prefix("when there are ")
        && let Some((cnt_part, rest)) = after.split_once(" or more ")
        && let Some((kind, subj_raw)) = rest
            .split_once(" counters on ")
            .or_else(|| rest.split_once(" counter on "))
        && let (Some(n), Ok(subj)) = (
            parse_number_word(cnt_part.trim()),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(
            ConditionAst::CounterCompare {
                subject: Box::new(subj),
                counter_kind: kind.trim().to_string(),
                op: crate::ast::CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(u16::MAX),
            },
        ))]);
    }
    // Mana-production triggers (Caged Sun, Gauntlet of Power) & Scry-bottom trigger (The Temporal Anchor)
    if low == "whenever a land's ability causes you to add one or more mana of the chosen color"
        || low == "whenever a basic land is tapped for mana of the chosen color"
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(SubjectRefAst::SelfObject),
            tapped: true,
            for_mana: true,
        }]);
    }
    if low
        == "whenever you choose to put one or more cards on the bottom of your library while scrying"
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: "scry_bottom".to_string(),
        }]);
    }
    if low == "whenever you put one or more counters on a permanent or player" {
        return Ok(vec![TriggerEventAst::CountersChanged {
            subject: Box::new(SubjectRefAst::AnyTarget),
            counter_kind: "any".to_string(),
            added: true,
            by_you: true,
        }]);
    }
    if let Some(subj_raw) = low
        .strip_prefix("whenever ")
        .and_then(|r| r.strip_suffix(" dies or is put into the command zone"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::ZoneChange {
            from: Some("battlefield".to_string()),
            to: Some("graveyard_or_command".to_string()),
            subject: Box::new(subj),
            grouped: false,
            entering_controller: None,
        }]);
    }
    if let Some(subj_raw) = low
        .strip_prefix("whenever ")
        .and_then(|r| r.strip_suffix(" becomes blocked by two or more creatures"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::BecomesBlocked {
            subject: Box::new(subj),
            blocker: None,
        }]);
    }
    if let Some(after_cause) = low
        .strip_prefix("whenever a spell or ability an opponent controls causes you to ")
        .or_else(|| low.strip_prefix("when a spell or ability an opponent controls causes you to "))
        && (after_cause.starts_with("discard ") || after_cause.starts_with("sacrifice "))
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: format!("opponent_caused:{after_cause}"),
        }]);
    }
    if let Some((a_part, b_part)) = low.split_once(" or when ")
        && let (Ok(mut ev1), Ok(mut ev2)) = (
            parse_trigger_events(a_part.trim(), vocab),
            parse_trigger_events(&format!("when {}", b_part.trim()), vocab),
        )
    {
        ev1.append(&mut ev2);
        return Ok(ev1);
    }
    // General "<when/whenever> <player> attack[s] <subj> or a planeswalker they control" (Tenuous Truce)
    if let Some(after_w) = low
        .strip_prefix("when ")
        .or_else(|| low.strip_prefix("whenever "))
        && let Some((p_raw, tgt_raw)) = after_w
            .split_once(" attack ")
            .or_else(|| after_w.split_once(" attacks "))
        && let Some(actor) = parse_player_ref(p_raw.trim())
        && tgt_raw.contains(" or a planeswalker ")
    {
        return Ok(vec![TriggerEventAst::Attacks {
            actor: Some(actor),
            subject: None,
            grouped: true,
            qualifier: Some(tgt_raw.to_string()),
        }]);
    }
    // General CR 603.8 state triggers (Call to Arms, City in a Bottle)
    if low
        == "when the chosen color isn't the most common color among nontoken permanents the chosen player controls or is tied for most common"
        || (low.starts_with("whenever one or more ") && low.ends_with(" are on the battlefield"))
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(
            ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::You,
                action: low,
            },
        ))]);
    }
    if matches!(
        low.as_str(),
        "whenever you win a clash"
            | "whenever ~'s cumulative upkeep is paid"
            | "whenever ~ saddles a mount or crews a vehicle during your main phase"
            | "whenever an opponent who controls an artifact named ~ draws a card during their turn"
            | "whenever an opponent who controls an artifact named ~ gains life during their turn"
    ) {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: low,
        }]);
    }
    if low == "whenever a player casts their second spell during their turn" {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: PlayerRefAst::AnyPlayer,
            spell: None,
            self_spell: false,
            targets: None,
            nth_each_turn: Some(2),
            include_copy: false,
        }]);
    }
    if low == "whenever two or more creatures you control attack a player" {
        return Ok(vec![TriggerEventAst::Attacks {
            subject: None,
            actor: Some(PlayerRefAst::You),
            qualifier: Some("two_or_more".to_string()),
            grouped: true,
        }]);
    }
    if low
        == "whenever you and/or at least one permanent you control becomes the target of a spell or ability"
    {
        return Ok(vec![TriggerEventAst::BecomesTargeted {
            subject: Box::new(SubjectRefAst::SelfObject),
            by_opponent: false,
            by_text: None,
        }]);
    }
    if let Some(subj_raw) = low
        .strip_prefix("whenever ")
        .and_then(|r| r.strip_suffix(" enters transformed"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::ZoneChange {
            from: None,
            to: Some("battlefield_transformed".to_string()),
            subject: Box::new(subj),
            grouped: false,
            entering_controller: None,
        }]);
    }
    if let Some(flt_raw) = low.strip_prefix("whenever you play ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: PlayerRefAst::You,
            spell: Some(Box::new(flt)),
            self_spell: false,
            targets: None,
            nth_each_turn: None,
            include_copy: false,
        }]);
    }

    // StepBegan: "at the beginning of ..." / "at the end of ..."
    if low.starts_with("at the beginning of ") || low.starts_with("at the end of ") {
        let is_end = low.starts_with("at the end of ");
        let mut rest = if is_end {
            &low["at the end of ".len()..]
        } else {
            &low["at the beginning of ".len()..]
        };

        if let Some(owner) = rest.strip_prefix("the upkeep of ") {
            return Ok(vec![TriggerEventAst::StepBegan {
                step: TurnStepAst::Upkeep,
                whose: owner.to_string(),
            }]);
        }
        if let Some(owner) = rest.strip_prefix("the end step of ") {
            return Ok(vec![TriggerEventAst::StepBegan {
                step: TurnStepAst::EndStep,
                whose: owner.to_string(),
            }]);
        }

        // Longest possessive match FIRST
        let whose = if let Some(r) = rest.strip_prefix("each of enchanted player's ") {
            rest = r;
            "enchanted_player"
        } else if let Some(r) = rest.strip_prefix("each of that player's ") {
            rest = r;
            "that_player"
        } else if let Some(r) = rest.strip_prefix("enchanted opponent's ") {
            rest = r;
            "enchanted_opponent"
        } else if let Some(r) = rest.strip_prefix("the monarch's ") {
            rest = r;
            "monarch"
        } else if let Some(r) = rest.strip_prefix("each other player's ") {
            rest = r;
            "each_other_player"
        } else if let Some(r) = rest.strip_prefix("each player's ") {
            rest = r;
            "each_player"
        } else if let Some(r) = rest.strip_prefix("each opponent's ") {
            rest = r;
            "each_opponent"
        } else if let Some(r) = rest.strip_prefix("an opponent's ") {
            rest = r;
            "opponent"
        } else if let Some(r) = rest.strip_prefix("that player's ") {
            rest = r;
            "that_player"
        } else if let Some(r) = rest.strip_prefix("enchanted player's ") {
            rest = r;
            "enchanted_player"
        } else if let Some(r) = rest.strip_prefix("the chosen player's ") {
            rest = r;
            "chosen_player"
        } else if let Some(r) = rest.strip_prefix("your next ") {
            rest = r;
            "your_next"
        } else if let Some(r) = rest.strip_prefix("that turn's ") {
            rest = r;
            "that_turn"
        } else if let Some(r) = rest.strip_prefix("the first ") {
            rest = r;
            "first"
        } else if let Some(r) = rest.strip_prefix("each of your ") {
            rest = r;
            "you"
        } else if let Some(r) = rest.strip_prefix("your ") {
            rest = r;
            "you"
        } else if let Some(r) = rest.strip_prefix("the next ") {
            rest = r;
            "next"
        } else if let Some(r) = rest.strip_prefix("each ") {
            rest = r;
            "each_player"
        } else if let Some(r) = rest.strip_prefix("every ") {
            rest = r;
            "each_player"
        } else if let Some(r) = rest.strip_prefix("the ") {
            rest = r;
            "any"
        } else if rest.starts_with("combat on your turn") {
            return Ok(vec![TriggerEventAst::StepBegan {
                step: TurnStepAst::BeginningOfCombat,
                whose: "you".to_string(),
            }]);
        } else if rest.starts_with("combat on each opponent's turn") {
            return Ok(vec![TriggerEventAst::StepBegan {
                step: TurnStepAst::BeginningOfCombat,
                whose: "each_opponent".to_string(),
            }]);
        } else if rest.starts_with("combat on each player's turn") {
            return Ok(vec![TriggerEventAst::StepBegan {
                step: TurnStepAst::BeginningOfCombat,
                whose: "each_player".to_string(),
            }]);
        } else {
            "any"
        };

        let rest = rest.strip_prefix("first ").unwrap_or(rest);
        let step = if rest.starts_with("upkeep") {
            TurnStepAst::Upkeep
        } else if rest.starts_with("end step") {
            TurnStepAst::EndStep
        } else if rest.starts_with("draw step") {
            TurnStepAst::DrawStep
        } else if rest.starts_with("precombat main") || rest.starts_with("first main") {
            TurnStepAst::PrecombatMain
        } else if rest.starts_with("postcombat main") || rest.starts_with("second main") {
            TurnStepAst::PostcombatMain
        } else if rest.starts_with("main phase") {
            TurnStepAst::MainPhase
        } else if rest.starts_with("untap step") {
            TurnStepAst::UntapStep
        } else if rest.starts_with("combat") {
            if is_end {
                TurnStepAst::EndOfCombat
            } else {
                TurnStepAst::BeginningOfCombat
            }
        } else if rest == "the monarch's end step" {
            TurnStepAst::EndStep
        } else {
            return Err(format!("unparsed_step_trigger:{c}"));
        };
        return Ok(vec![TriggerEventAst::StepBegan {
            step,
            whose: whose.to_string(),
        }]);
    }

    let blow_raw = low
        .strip_prefix("whenever ")
        .or_else(|| low.strip_prefix("when "))
        .unwrap_or(&low)
        .trim();
    let blow = blow_raw.strip_suffix(" this turn").unwrap_or(blow_raw);

    // Battalion: "~ and at least two other creatures attack"
    if let Some(after) = blow.strip_prefix("~ and at least ")
        && let Some((cnt_tok, rest)) = after.split_once(" other ")
        && let Some(flt_raw) = rest.strip_suffix(" attack")
        && let Some(n) = parse_number_word(cnt_tok)
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::AttacksWithOthers {
            subject: Box::new(SubjectRefAst::SelfObject),
            min_others: u8::try_from(n.max(1)).unwrap_or(2),
            other_filter: Box::new(flt),
        }]);
    }

    // Generalized recursive "~ or <other_subject_and_event>" (Bestow, Reconfigure, Morph, Typal Lords)
    if let Some(rest) = blow.strip_prefix("~ or ")
        && !rest.starts_with("another ")
        && let Ok(mut evs) = parse_trigger_events(rest, vocab)
    {
        if let Some(first) = evs.first().cloned() {
            evs.insert(0, first);
        }
        return Ok(evs);
    }
    // Compound "~ or another <filter> <event>" (generalized!)
    if let Some(after) = blow.strip_prefix("~ or another ") {
        for (suf, kind) in [
            (" enters the battlefield", "enter"),
            (" enter the battlefield", "enter"),
            (" enters", "enter"),
            (" enter", "enter"),
            (" dies", "die"),
            (" die", "die"),
            (" leaves the battlefield", "leave"),
            (" leave the battlefield", "leave"),
            (" attacks", "attack"),
            (" attack", "attack"),
        ] {
            if let Some(flt_raw) = after.strip_suffix(suf)
                && let Ok(mut flt) = parse_object_filter(flt_raw, vocab)
            {
                flt.flags = flt.flags.union(crate::ast::ObjectFilterFlags::EXCLUDE_SELF);
                let other_subj = Box::new(SubjectRefAst::Group(Box::new(flt)));
                let self_subj = Box::new(SubjectRefAst::SelfObject);
                return Ok(match kind {
                    "enter" => vec![
                        TriggerEventAst::ZoneChange {
                            from: None,
                            to: Some("battlefield".to_string()),
                            subject: self_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                        TriggerEventAst::ZoneChange {
                            from: None,
                            to: Some("battlefield".to_string()),
                            subject: other_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                    ],
                    "die" => vec![
                        TriggerEventAst::ZoneChange {
                            from: Some("battlefield".to_string()),
                            to: Some("graveyard".to_string()),
                            subject: self_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                        TriggerEventAst::ZoneChange {
                            from: Some("battlefield".to_string()),
                            to: Some("graveyard".to_string()),
                            subject: other_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                    ],
                    "leave" => vec![
                        TriggerEventAst::ZoneChange {
                            from: Some("battlefield".to_string()),
                            to: None,
                            subject: self_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                        TriggerEventAst::ZoneChange {
                            from: Some("battlefield".to_string()),
                            to: None,
                            subject: other_subj,
                            grouped: false,
                            entering_controller: None,
                        },
                    ],
                    _ => vec![
                        TriggerEventAst::Attacks {
                            subject: Some(self_subj),
                            actor: None,
                            qualifier: None,
                            grouped: false,
                        },
                        TriggerEventAst::Attacks {
                            subject: Some(other_subj),
                            actor: None,
                            qualifier: None,
                            grouped: false,
                        },
                    ],
                });
            }
        }
        // Fallback: recursively parse "another <after>" for deals combat damage, turned face up, etc.!
        if let Ok(mut evs) = parse_trigger_events(&format!("another {after}"), vocab) {
            if let Some(first) = evs.first().cloned() {
                evs.insert(0, first);
            }
            return Ok(evs);
        }
    }

    // Compound enters-or-attacks / enters-or-dies / enters-or-leaves for any subject
    if let Some(subj_raw) = blow
        .strip_suffix(" enters or attacks")
        .or_else(|| blow.strip_suffix(" enters the battlefield or attacks"))
        .or_else(|| blow.strip_suffix(" enter or attack"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::ZoneChange {
                from: None,
                to: Some("battlefield".to_string()),
                subject: Box::new(subj.clone()),
                grouped: false,
                entering_controller: None,
            },
            TriggerEventAst::Attacks {
                subject: Some(Box::new(subj)),
                actor: None,
                qualifier: None,
                grouped: false,
            },
        ]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" enters or dies")
        .or_else(|| blow.strip_suffix(" enters the battlefield or dies"))
        .or_else(|| blow.strip_suffix(" enters or is put into a graveyard from the battlefield"))
        .or_else(|| {
            blow.strip_suffix(
                " enters the battlefield or is put into a graveyard from the battlefield",
            )
        })
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::ZoneChange {
                from: None,
                to: Some("battlefield".to_string()),
                subject: Box::new(subj.clone()),
                grouped: false,
                entering_controller: None,
            },
            TriggerEventAst::ZoneChange {
                from: Some("battlefield".to_string()),
                to: Some("graveyard".to_string()),
                subject: Box::new(subj),
                grouped: false,
                entering_controller: None,
            },
        ]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" enters or leaves the battlefield")
        .or_else(|| blow.strip_suffix(" enters the battlefield or leaves the battlefield"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::ZoneChange {
                from: None,
                to: Some("battlefield".to_string()),
                subject: Box::new(subj.clone()),
                grouped: false,
                entering_controller: None,
            },
            TriggerEventAst::ZoneChange {
                from: Some("battlefield".to_string()),
                to: None,
                subject: Box::new(subj),
                grouped: false,
                entering_controller: None,
            },
        ]);
    }

    if let Some(subj_raw) = blow
        .strip_suffix(" dies or is put into exile from the battlefield")
        .or_else(|| blow.strip_suffix(" die or are put into exile from the battlefield"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::ZoneChange {
                from: Some("battlefield".to_string()),
                to: Some("graveyard".to_string()),
                subject: Box::new(subj.clone()),
                grouped: false,
                entering_controller: None,
            },
            TriggerEventAst::ZoneChange {
                from: Some("battlefield".to_string()),
                to: Some("exile".to_string()),
                subject: Box::new(subj),
                grouped: false,
                entering_controller: None,
            },
        ]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" enters or deals combat damage to a player")
        .or_else(|| blow.strip_suffix(" enters the battlefield or deals combat damage to a player"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::ZoneChange {
                from: None,
                to: Some("battlefield".to_string()),
                subject: Box::new(subj.clone()),
                grouped: false,
                entering_controller: None,
            },
            TriggerEventAst::DamageDealt {
                combat_only: true,
                noncombat_only: false,
                source: Some(Box::new(subj)),
                recipient: Some(Box::new(SubjectRefAst::Player(PlayerRefAst::AnyPlayer))),
                grouped: false,
            },
        ]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" is attacked")
        .or_else(|| blow.strip_suffix(" are attacked"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::PlayerAttacked { player }]);
    }
    if blow == "you roll the planar die" {
        return Ok(vec![TriggerEventAst::RollDie {
            player: PlayerRefAst::You,
            exact_result: None,
        }]);
    }
    if blow.starts_with("you roll a ") && blow.ends_with(" or higher on a die") {
        return Ok(vec![TriggerEventAst::RollDie {
            player: PlayerRefAst::You,
            exact_result: None,
        }]);
    }
    if let Some((subj_raw, _)) = blow
        .split_once(" becomes attached to ")
        .or_else(|| blow.split_once(" becomes unattached from "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::BecomesAttached {
            subject: Box::new(subj),
            attachment_kind: if blow.contains("unattached") {
                "unattached".to_string()
            } else {
                "attached".to_string()
            },
        }]);
    }
    if blow == "the ring tempts you" {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: "ring_tempts_you".to_string(),
        }]);
    }
    if blow == "you activate an exhaust ability" {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: "exhaust".to_string(),
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" shuffles their library")
        .or_else(|| blow.strip_suffix(" shuffle their library"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player,
            action: "shuffle_library".to_string(),
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" becomes the monarch")
        .or_else(|| blow.strip_suffix(" become the monarch"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player,
            action: "become_monarch".to_string(),
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" activates a loyalty ability")
        .or_else(|| blow.strip_suffix(" activates an ability that isn't a mana ability"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::AbilityActivated {
            player,
            source_filter: Box::new(crate::ast::ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            }),
        }]);
    }
    if let Some((who_raw, flt_raw)) = blow
        .split_once(" activates an ability of ")
        .or_else(|| blow.split_once(" activate an ability of "))
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(
                flt_raw
                    .strip_suffix(" that isn't a mana ability")
                    .unwrap_or(flt_raw),
                vocab,
            ),
        )
    {
        return Ok(vec![TriggerEventAst::AbilityActivated {
            player,
            source_filter: Box::new(flt),
        }]);
    }
    if blow == "a spell or ability is put onto the stack" {
        return Ok(vec![TriggerEventAst::SpellOrAbilityOnStack]);
    }
    if blow == "you play a land or cast a spell" {
        return Ok(vec![
            TriggerEventAst::LandPlayed {
                player: PlayerRefAst::You,
            },
            TriggerEventAst::SpellCast {
                caster: PlayerRefAst::You,
                spell: None,
                self_spell: false,
                targets: None,
                nth_each_turn: None,
                include_copy: false,
            },
        ]);
    }

    if blow == "~ enters or the creature it haunts dies"
        || blow == "~ enters the battlefield or the creature it haunts dies"
    {
        return Ok(vec![
            TriggerEventAst::ZoneChange {
                from: None,
                to: Some("battlefield".to_string()),
                subject: Box::new(SubjectRefAst::SelfObject),
                grouped: false,
                entering_controller: None,
            },
            TriggerEventAst::HauntedCreatureDies,
        ]);
    }
    if blow == "the creature ~ haunts dies" {
        return Ok(vec![TriggerEventAst::HauntedCreatureDies]);
    }
    if blow == "you lose control of ~" {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: "lose_control_self".to_string(),
        }]);
    }
    if blow == "players finish voting" {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::EachPlayer,
            action: "finish_voting".to_string(),
        }]);
    }
    if blow == "you get one or more {e}" || blow == "you get one or more {tk}" {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: blow.to_string(),
        }]);
    }
    if blow == "you play a land from exile or cast a spell from exile" {
        return Ok(vec![
            TriggerEventAst::LandPlayed {
                player: PlayerRefAst::You,
            },
            TriggerEventAst::CastFromOutsideHand {
                player: PlayerRefAst::You,
            },
        ]);
    }
    if let Some(flt_raw) = blow.strip_prefix("you cycle or discard ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::Cycled {
                subject: Box::new(SubjectRefAst::Group(Box::new(flt.clone()))),
            },
            TriggerEventAst::CardDiscarded {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
            },
        ]);
    }
    if let Some(flt_raw) = blow.strip_prefix("you tap ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            tapped: true,
            for_mana: false,
        }]);
    }
    if let Some(after) = blow.strip_prefix("excess damage is dealt to ")
        && let Ok(subj) =
            parse_subject_or_target(after.strip_suffix(" this way").unwrap_or(after), vocab)
    {
        return Ok(vec![TriggerEventAst::DamageDealt {
            combat_only: false,
            noncombat_only: false,
            source: None,
            recipient: Some(Box::new(subj)),
            grouped: false,
        }]);
    }
    if low.starts_with("when there are no ")
        && let Ok(cond) = parse_condition(&low[5..], vocab)
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(cond))]);
    }
    if blow == "you scry or surveil" {
        return Ok(vec![
            TriggerEventAst::KeywordActionPerformed {
                player: PlayerRefAst::You,
                action: "scry".to_string(),
            },
            TriggerEventAst::KeywordActionPerformed {
                player: PlayerRefAst::You,
                action: "surveil".to_string(),
            },
        ]);
    }
    if blow == "you attack a player" || blow == "you attack an opponent" {
        return Ok(vec![TriggerEventAst::Attacks {
            subject: None,
            actor: Some(PlayerRefAst::You),
            qualifier: Some("player".to_string()),
            grouped: false,
        }]);
    }
    if let Some((who_raw, flt_raw)) = blow
        .split_once(" mills ")
        .or_else(|| blow.split_once(" mill "))
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(peel_one_or_more(flt_raw).0, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::CardMilled {
            player,
            filter: Box::new(flt),
        }]);
    }
    if blow == "a player loses the game" || blow == "an opponent loses the game" {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: if blow.starts_with("an opponent") {
                PlayerRefAst::AnyOpponent
            } else {
                PlayerRefAst::AnyPlayer
            },
            action: "loses_game".to_string(),
        }]);
    }
    if blow == "a player kicks a spell" || blow == "you kick a spell" {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: if blow.starts_with("you") {
                PlayerRefAst::You
            } else {
                PlayerRefAst::AnyPlayer
            },
            action: "kick_spell".to_string(),
        }]);
    }
    if let Some((who_raw, rest)) = blow
        .split_once(" puts ")
        .or_else(|| blow.split_once(" put "))
        && let Some(flt_raw) = rest.strip_suffix(" onto the battlefield")
        && let (Some(_player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::ZoneChange {
            from: None,
            to: Some("battlefield".to_string()),
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            grouped: false,
            entering_controller: None,
        }]);
    }
    if blow == "a player plays a land or casts a spell"
        || blow == "a player plays a land from exile or casts a spell from exile"
    {
        return Ok(vec![
            TriggerEventAst::LandPlayed {
                player: PlayerRefAst::AnyPlayer,
            },
            TriggerEventAst::SpellCast {
                caster: PlayerRefAst::AnyPlayer,
                spell: None,
                self_spell: false,
                targets: None,
                nth_each_turn: None,
                include_copy: false,
            },
        ]);
    }
    if let Some((who_raw, subj_raw)) = blow
        .split_once(" cycles ")
        .or_else(|| blow.split_once(" cycle "))
        && let (Some(_player), Ok(subj)) = (
            parse_player_ref(who_raw),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::Cycled {
            subject: Box::new(subj),
        }]);
    }
    if let Some((subj_raw, cond_raw)) = blow.split_once(" attacks while ")
        && let (Ok(subj), Ok(_cond)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_condition(cond_raw, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::Attacks {
            subject: Some(Box::new(subj)),
            actor: None,
            qualifier: Some(format!("while:{cond_raw}")),
            grouped: false,
        }]);
    }

    // Compound self triggers
    match blow {
        "~ enters or attacks" | "~ enters the battlefield or attacks" => {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: Box::new(SubjectRefAst::SelfObject),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::Attacks {
                    subject: Some(Box::new(SubjectRefAst::SelfObject)),
                    actor: None,
                    qualifier: None,
                    grouped: false,
                },
            ]);
        }
        "~ enters or dies" | "~ enters the battlefield or dies" => {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: Box::new(SubjectRefAst::SelfObject),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::ZoneChange {
                    from: Some("battlefield".to_string()),
                    to: Some("graveyard".to_string()),
                    subject: Box::new(SubjectRefAst::SelfObject),
                    grouped: false,
                    entering_controller: None,
                },
            ]);
        }
        "~ enters or transforms into ~" | "~ enters the battlefield or transforms into ~" => {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: Box::new(SubjectRefAst::SelfObject),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::Transforms {
                    subject: Box::new(SubjectRefAst::SelfObject),
                },
            ]);
        }
        "~ enters or is turned face up" | "~ enters the battlefield or is turned face up" => {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: Box::new(SubjectRefAst::SelfObject),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::TurnedFaceUp {
                    subject: Box::new(SubjectRefAst::SelfObject),
                },
            ]);
        }
        "~ enters or leaves the battlefield"
        | "~ enters the battlefield or leaves the battlefield" => {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: Box::new(SubjectRefAst::SelfObject),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::ZoneChange {
                    from: Some("battlefield".to_string()),
                    to: None,
                    subject: Box::new(SubjectRefAst::SelfObject),
                    grouped: false,
                    entering_controller: None,
                },
            ]);
        }
        _ => {}
    }

    if let Some(subj_raw) = blow.strip_suffix(" attacks or blocks")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::Attacks {
                subject: Some(Box::new(subj.clone())),
                actor: None,
                qualifier: None,
                grouped: false,
            },
            TriggerEventAst::Blocks {
                subject: Box::new(subj),
                attacker: None,
            },
        ]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" blocks or becomes blocked")
        .or_else(|| blow.strip_suffix(" blocks or becomes blocked by a creature"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::Blocks {
                subject: Box::new(subj.clone()),
                attacker: None,
            },
            TriggerEventAst::BecomesBlocked {
                subject: Box::new(subj),
                blocker: None,
            },
        ]);
    }

    // First / second spell each turn
    if blow == "day becomes night or night becomes day" {
        return Ok(vec![TriggerEventAst::DayNightChanged]);
    }
    if blow == "you cast a spell during an opponent's turn"
        || blow == "you cast your first spell during each opponent's turn"
    {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: PlayerRefAst::You,
            spell: None,
            self_spell: false,
            targets: None,
            nth_each_turn: if blow.contains("first") {
                Some(1)
            } else {
                None
            },
            include_copy: false,
        }]);
    }
    if blow == "a spell or ability an opponent controls causes you to discard ~" {
        return Ok(vec![TriggerEventAst::CardDiscarded {
            player: PlayerRefAst::You,
            filter: Box::new(crate::ast::ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
        }]);
    }
    if let Some(after) = blow.strip_prefix("you cast your first ")
        && let Some(flt_raw) = after.strip_suffix(" each turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: PlayerRefAst::You,
            spell: Some(Box::new(flt)),
            self_spell: false,
            targets: None,
            nth_each_turn: Some(1),
            include_copy: false,
        }]);
    }
    if let Some(flt_raw) = blow.strip_prefix("you next cast ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: PlayerRefAst::You,
            spell: Some(Box::new(flt)),
            self_spell: false,
            targets: None,
            nth_each_turn: Some(1),
            include_copy: false,
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" searches their library")
        .or_else(|| blow.strip_suffix(" search their library"))
        .or_else(|| blow.strip_suffix(" searches a library"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LibrarySearched { player }]);
    }
    if blow == "you cast your second spell each turn"
        || blow == "a player casts their second spell each turn"
    {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: if blow.starts_with("you") {
                PlayerRefAst::You
            } else {
                PlayerRefAst::AnyPlayer
            },
            spell: None,
            self_spell: false,
            targets: None,
            nth_each_turn: Some(2),
            include_copy: false,
        }]);
    }
    if blow == "you cast your third spell each turn" {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: PlayerRefAst::You,
            spell: None,
            self_spell: false,
            targets: None,
            nth_each_turn: Some(3),
            include_copy: false,
        }]);
    }
    if blow == "you cast a spell other than your first spell each turn"
        || blow == "you cast a spell during your turn"
        || blow == "you cast a spell during combat"
        || blow == "you cast your first spell with {x} in its mana cost each turn"
    {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: PlayerRefAst::You,
            spell: None,
            self_spell: false,
            targets: None,
            nth_each_turn: None,
            include_copy: false,
        }]);
    }

    // Cast or copy (any player)
    if let Some((who_raw, spell_raw)) = blow
        .split_once(" casts or copies ")
        .or_else(|| blow.split_once(" cast or copy "))
        && let (Some(caster), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(spell_raw, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster,
            spell: Some(Box::new(flt)),
            self_spell: false,
            targets: None,
            nth_each_turn: None,
            include_copy: true,
        }]);
    }
    // "<player> casts their first/second [<filter>] spell each turn"
    if let Some((who_raw, rest)) = blow
        .split_once(" casts their ")
        .or_else(|| blow.split_once(" cast their "))
        && let Some(caster) = parse_player_ref(who_raw)
        && let Some(core) = rest.strip_suffix(" each turn")
    {
        let (nth, flt_opt) = if let Some(f) = core.strip_prefix("first ") {
            (Some(1u8), Some(f))
        } else if let Some(f) = core.strip_prefix("second ") {
            (Some(2u8), Some(f))
        } else {
            (None, None)
        };
        if let (Some(n), Some(flt_raw)) = (nth, flt_opt) {
            let spell = if flt_raw == "spell" {
                None
            } else if let Ok(f) = parse_object_filter(flt_raw, vocab) {
                Some(Box::new(f))
            } else {
                None
            };
            if spell.is_some() || flt_raw == "spell" {
                return Ok(vec![TriggerEventAst::SpellCast {
                    caster,
                    spell,
                    self_spell: false,
                    targets: None,
                    nth_each_turn: Some(n),
                    include_copy: false,
                }]);
            }
        }
    }

    // Attacks and isn't blocked
    if let Some(subj_raw) = blow
        .strip_suffix(" attacks and isn't blocked")
        .or_else(|| blow.strip_suffix(" attack and isn't blocked"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::AttacksUnblocked {
            subject: Box::new(subj),
        }]);
    }

    // Enters
    if !blow.starts_with("you ")
        && !blow.starts_with("a player ")
        && !blow.starts_with("an opponent ")
    {
        let (core_enter, entering_ctrl) = if let Some(p) = blow.strip_suffix(" under your control")
        {
            (p, Some(ControllerRelation::You))
        } else if let Some(p) = blow.strip_suffix(" under an opponent's control") {
            (p, Some(ControllerRelation::Opponent))
        } else {
            (blow, None)
        };
        let core_enter = core_enter
            .strip_suffix(" from a graveyard")
            .or_else(|| core_enter.strip_suffix(" from your graveyard"))
            .or_else(|| core_enter.strip_suffix(" from exile"))
            .or_else(|| core_enter.strip_suffix(" during the declare attackers step"))
            .or_else(|| core_enter.strip_suffix(" during combat"))
            .unwrap_or(core_enter);
        if let Some(subj_raw) = core_enter
            .strip_suffix(" enters the battlefield untapped")
            .or_else(|| core_enter.strip_suffix(" enters the battlefield tapped"))
            .or_else(|| core_enter.strip_suffix(" enters untapped"))
            .or_else(|| core_enter.strip_suffix(" enters tapped"))
            .or_else(|| core_enter.strip_suffix(" enters the battlefield"))
            .or_else(|| core_enter.strip_suffix(" enter the battlefield"))
            .or_else(|| core_enter.strip_suffix(" enters"))
            .or_else(|| core_enter.strip_suffix(" enter"))
        {
            let (sraw, grouped) = peel_one_or_more(subj_raw);
            if let Ok(subj) = parse_subject_or_target(sraw, vocab) {
                return Ok(vec![TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: Box::new(subj),
                    grouped,
                    entering_controller: entering_ctrl,
                }]);
            }
        }
    }

    // Dies
    if let Some(subj_raw) = blow
        .strip_suffix(" dies")
        .or_else(|| blow.strip_suffix(" die"))
    {
        let (sraw, grouped) = peel_one_or_more(subj_raw);
        if let Ok(subj) = parse_subject_or_target(sraw, vocab) {
            return Ok(vec![TriggerEventAst::ZoneChange {
                from: Some("battlefield".to_string()),
                to: Some("graveyard".to_string()),
                subject: Box::new(subj),
                grouped,
                entering_controller: None,
            }]);
        }
    }

    // Leaves battlefield / graveyard (with optional trailing "during your turn")
    let blow_noturn = blow
        .strip_suffix(" during your turn")
        .or_else(|| blow.strip_suffix(" during an opponent's turn"))
        .unwrap_or(blow);
    for (suf, from) in [
        (" leaves the battlefield", "battlefield"),
        (" leave the battlefield", "battlefield"),
        (" leaves your graveyard", "your_graveyard"),
        (" leave your graveyard", "your_graveyard"),
        (" leaves an opponent's graveyard", "opponent_graveyard"),
        (" leave an opponent's graveyard", "opponent_graveyard"),
        (" leaves a player's graveyard", "any_graveyard"),
        (" leave a player's graveyard", "any_graveyard"),
        (" leaves a graveyard", "any_graveyard"),
        (" leave a graveyard", "any_graveyard"),
    ] {
        if let Some(subj_raw) = blow_noturn.strip_suffix(suf) {
            let (sraw, grouped) = peel_one_or_more(subj_raw);
            if let Ok(subj) = parse_subject_or_target(sraw, vocab) {
                return Ok(vec![TriggerEventAst::ZoneChange {
                    from: Some(from.to_string()),
                    to: None,
                    subject: Box::new(subj),
                    grouped,
                    entering_controller: None,
                }]);
            }
        }
    }

    // Put into a graveyard / exile / library / milled (with peel_one_or_more!)
    let blow_noturn_zone = blow
        .strip_suffix(" during your turn")
        .or_else(|| blow.strip_suffix(" during an opponent's turn"))
        .unwrap_or(blow);
    if let Some(subj_raw) = blow_noturn_zone
        .strip_suffix(" are milled")
        .or_else(|| blow_noturn_zone.strip_suffix(" is milled"))
    {
        let (sraw, _) = peel_one_or_more(subj_raw);
        if let Ok(flt) = parse_object_filter(sraw, vocab) {
            return Ok(vec![TriggerEventAst::CardMilled {
                player: PlayerRefAst::AnyPlayer,
                filter: Box::new(flt),
            }]);
        }
    }
    if let Some((subj_raw, rest)) = blow_noturn_zone
        .split_once(" is put into ")
        .or_else(|| blow_noturn_zone.split_once(" are put into "))
    {
        let (sraw, grouped) = peel_one_or_more(subj_raw);
        let dest_zone = if rest.contains("graveyard") {
            Some("graveyard")
        } else if rest.contains("exile") {
            Some("exile")
        } else if rest.contains("library") {
            Some("library")
        } else {
            None
        };
        if let (Some(to_z), Ok(subj)) = (dest_zone, parse_subject_or_target(sraw, vocab)) {
            return Ok(vec![TriggerEventAst::ZoneChange {
                from: Some("any".to_string()),
                to: Some(to_z.to_string()),
                subject: Box::new(subj),
                grouped,
                entering_controller: None,
            }]);
        }
    }

    // Deals combat / noncombat / any damage
    for (needle, combat_only, noncombat_only) in [
        (" deals combat damage", true, false),
        (" deal combat damage", true, false),
        (" deals noncombat damage", false, true),
        (" deal noncombat damage", false, true),
        (" deals damage", false, false),
        (" deal damage", false, false),
    ] {
        if let Some((src_raw, tail)) = blow.split_once(needle) {
            let (sraw, grouped) = peel_one_or_more(src_raw);
            if let Ok(src) = parse_subject_or_target(sraw, vocab) {
                let recipient = tail
                    .trim()
                    .strip_prefix("to ")
                    .and_then(|r| parse_subject_or_target(r, vocab).ok())
                    .map(Box::new);
                return Ok(vec![TriggerEventAst::DamageDealt {
                    combat_only,
                    noncombat_only,
                    source: Some(Box::new(src)),
                    recipient,
                    grouped,
                }]);
            }
        }
    }

    // Is dealt damage (including excess / excess noncombat / excess combat damage: Fall of Cair Andros, Aegar)
    for (suf, combat_only, noncombat_only) in [
        (" is dealt excess combat damage", true, false),
        (" are dealt excess combat damage", true, false),
        (" is dealt excess noncombat damage", false, true),
        (" are dealt excess noncombat damage", false, true),
        (" is dealt excess damage", false, false),
        (" are dealt excess damage", false, false),
        (" is dealt combat damage", true, false),
        (" are dealt combat damage", true, false),
        (" is dealt noncombat damage", false, true),
        (" are dealt noncombat damage", false, true),
        (" is dealt damage", false, false),
        (" are dealt damage", false, false),
        ("'re dealt damage", false, false),
    ] {
        if let Some(tgt_raw) = blow.strip_suffix(suf)
            && let Ok(tgt) = parse_subject_or_target(tgt_raw, vocab)
        {
            return Ok(vec![TriggerEventAst::DamageDealt {
                combat_only,
                noncombat_only,
                source: None,
                recipient: Some(Box::new(tgt)),
                grouped: false,
            }]);
        }
    }

    // Attacks
    if blow == "you attack" {
        return Ok(vec![TriggerEventAst::Attacks {
            subject: None,
            actor: Some(PlayerRefAst::You),
            qualifier: None,
            grouped: false,
        }]);
    }
    if let Some(after) = blow.strip_prefix("you attack with ") {
        let keep_as_qualifier = after.starts_with("two or more")
            || after.starts_with("three or more")
            || after.starts_with("four or more")
            || after.starts_with("five or more")
            || after.starts_with("creatures with total power ")
            || after == "one or more equipped creatures"
            || after.contains(" or more creatures with ");
        if !keep_as_qualifier {
            let (flt_raw, grouped) = peel_one_or_more(after);
            if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
                return Ok(vec![TriggerEventAst::Attacks {
                    subject: Some(Box::new(SubjectRefAst::Group(Box::new(flt)))),
                    actor: Some(PlayerRefAst::You),
                    qualifier: None,
                    grouped,
                }]);
            }
        }
        return Ok(vec![TriggerEventAst::Attacks {
            subject: None,
            actor: Some(PlayerRefAst::You),
            qualifier: Some(after.to_string()),
            grouped: false,
        }]);
    }
    if let Some(qual) = blow
        .strip_prefix("an opponent attacks ")
        .or_else(|| blow.strip_prefix("a player attacks "))
    {
        return Ok(vec![TriggerEventAst::Attacks {
            subject: None,
            actor: Some(if blow.starts_with("a player") {
                PlayerRefAst::AnyPlayer
            } else {
                PlayerRefAst::AnyOpponent
            }),
            qualifier: Some(qual.to_string()),
            grouped: false,
        }]);
    }
    for (suf, qual) in [
        (
            " attacks for the first time each turn",
            Some("first_time_each_turn"),
        ),
        (" attacks while saddled", Some("while_saddled")),
        (" attacks while exerted", Some("while_exerted")),
        (" attacks a player alone", Some("alone")),
        (" attacks alone", Some("alone")),
        (" attacks enchanted player", Some("enchanted_player")),
        (
            " attacks you or a planeswalker you control",
            Some("you_or_planeswalker"),
        ),
        (
            " attacks one of your opponents or a planeswalker an opponent controls",
            Some("opponent_or_planeswalker"),
        ),
        (" attacks you", Some("you")),
        (" attack you", Some("you")),
        (" attacks an opponent", Some("opponent")),
        (" attack an opponent", Some("opponent")),
        (" attacks one of your opponents", Some("opponent")),
        (" attack one of your opponents", Some("opponent")),
        (
            " attack one of your opponents or a planeswalker they control",
            Some("opponent_or_planeswalker"),
        ),
        (" attack one or more players", Some("player")),
        (" attacks a player", Some("player")),
        (" attack a player", Some("player")),
        (" attacks a planeswalker", Some("planeswalker")),
        (" attack a planeswalker", Some("planeswalker")),
        (" attacks", None),
        (" attack", None),
    ] {
        if let Some(subj_raw) = blow.strip_suffix(suf) {
            let (sraw, grouped) = peel_one_or_more(subj_raw);
            if let Ok(subj) = parse_subject_or_target(sraw, vocab) {
                return Ok(vec![TriggerEventAst::Attacks {
                    subject: Some(Box::new(subj)),
                    actor: None,
                    qualifier: qual.map(ToString::to_string),
                    grouped,
                }]);
            }
        }
    }

    // Becomes blocked / blocks (with peel_one_or_more! Do not swallow compound ", " / " or " tails.)
    if let Some((subj_raw, rest)) = blow
        .split_once(" becomes blocked")
        .or_else(|| blow.split_once(" become blocked"))
        && !rest.starts_with(',')
        && !rest.trim_start().starts_with("or ")
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        let rest_trim = rest.trim();
        let blocker = rest_trim
            .strip_prefix("by ")
            .and_then(|r| parse_subject_or_target(r, vocab).ok())
            .map(Box::new);
        if rest_trim.is_empty() || blocker.is_some() {
            return Ok(vec![TriggerEventAst::BecomesBlocked {
                subject: Box::new(subj),
                blocker,
            }]);
        }
    }
    if let Some((subj_raw, rest)) = blow
        .split_once(" blocks")
        .or_else(|| blow.split_once(" block"))
        && (rest.is_empty() || rest.starts_with(' '))
        && !rest.trim_start().starts_with("or ")
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        let rest_trim = rest.trim();
        let attacker = if rest_trim.is_empty() {
            None
        } else {
            parse_subject_or_target(rest_trim, vocab).ok().map(Box::new)
        };
        if rest_trim.is_empty() || attacker.is_some() {
            return Ok(vec![TriggerEventAst::Blocks {
                subject: Box::new(subj),
                attacker,
            }]);
        }
    }

    // SpellCast: "<player> cast(s) <spell>"
    if let Some((who_raw, spell_raw)) = blow
        .split_once(" casts ")
        .or_else(|| blow.split_once(" cast "))
        && let Some(caster) = parse_player_ref(who_raw)
    {
        if spell_raw == "~" {
            return Ok(vec![TriggerEventAst::SpellCast {
                caster,
                spell: None,
                self_spell: true,
                targets: None,
                nth_each_turn: None,
                include_copy: false,
            }]);
        }
        if let Some((flt_raw, tgt_raw)) = spell_raw
            .split_once(" that targets only ")
            .or_else(|| spell_raw.split_once(" that targets "))
            && let (Ok(flt), Ok(tgt)) = (
                parse_object_filter(flt_raw, vocab),
                parse_subject_or_target(
                    peel_one_or_more(tgt_raw.strip_prefix("a single ").unwrap_or(tgt_raw)).0,
                    vocab,
                ),
            )
        {
            return Ok(vec![TriggerEventAst::SpellCast {
                caster,
                spell: Some(Box::new(flt)),
                self_spell: false,
                targets: Some(Box::new(tgt)),
                nth_each_turn: None,
                include_copy: false,
            }]);
        }
        let spell_clean = spell_raw
            .strip_suffix(" using mana produced by ~")
            .or_else(|| spell_raw.strip_suffix(" during an opponent's turn"))
            .or_else(|| spell_raw.strip_suffix(" during your turn"))
            .or_else(|| spell_raw.strip_suffix(" using teamwork"))
            .unwrap_or(spell_raw);
        if let Some((flt_raw, kw_raw)) = spell_clean.split_once(" that has ")
            && vocab.is_evergreen_keyword(kw_raw.trim())
            && let Ok(mut flt) = parse_object_filter(flt_raw, vocab)
        {
            flt.required_keywords.push(kw_raw.trim().to_string());
            return Ok(vec![TriggerEventAst::SpellCast {
                caster,
                spell: Some(Box::new(flt)),
                self_spell: false,
                targets: None,
                nth_each_turn: None,
                include_copy: false,
            }]);
        }
        if let Some((flt_raw, colors_raw)) = spell_clean.split_once(" that's both ")
            && let Ok(flt) = parse_object_filter(&format!("{flt_raw} {colors_raw}"), vocab)
        {
            return Ok(vec![TriggerEventAst::SpellCast {
                caster,
                spell: Some(Box::new(flt)),
                self_spell: false,
                targets: None,
                nth_each_turn: None,
                include_copy: false,
            }]);
        }
        if spell_clean.contains(" in its mana cost")
            || spell_clean.contains("mana cost that contains")
        {
            return Ok(vec![TriggerEventAst::SpellCast {
                caster,
                spell: None,
                self_spell: false,
                targets: None,
                nth_each_turn: None,
                include_copy: false,
            }]);
        }
        if let Ok(flt) = parse_object_filter(spell_clean, vocab) {
            return Ok(vec![TriggerEventAst::SpellCast {
                caster,
                spell: Some(Box::new(flt)),
                self_spell: false,
                targets: None,
                nth_each_turn: None,
                include_copy: false,
            }]);
        }
    }

    // Player actions: draw / discard / sacrifice / gain-lose life / cycle
    if let Some((who_raw, qual)) = blow
        .split_once(" draws ")
        .or_else(|| blow.split_once(" draw "))
        && let Some(player) = parse_player_ref(who_raw)
        && qual.contains("card")
    {
        return Ok(vec![TriggerEventAst::CardDrawn {
            player,
            qualifier: qual.to_string(),
        }]);
    }
    if let Some((who_raw, flt_raw)) = blow
        .split_once(" discards ")
        .or_else(|| blow.split_once(" discard "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        let flt =
            parse_object_filter(flt_raw, vocab).unwrap_or_else(|_| crate::ast::ObjectFilterAst {
                any_card: true,
                ..Default::default()
            });
        return Ok(vec![TriggerEventAst::CardDiscarded {
            player,
            filter: Box::new(flt),
        }]);
    }
    if let Some((who_raw, subj_raw)) = blow
        .split_once(" sacrifices ")
        .or_else(|| blow.split_once(" sacrifice "))
        && let (Some(player), Ok(subj)) = (
            parse_player_ref(who_raw),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(vec![TriggerEventAst::PermanentSacrificed {
            player,
            subject: Box::new(subj),
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" gain life for the first time each turn")
        .or_else(|| blow.strip_suffix(" gains life for the first time each turn"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LifeChanged {
            player,
            gained: true,
            first_each_turn: true,
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" lose life for the first time each turn")
        .or_else(|| blow.strip_suffix(" loses life for the first time each turn"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LifeChanged {
            player,
            gained: false,
            first_each_turn: true,
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" gain life")
        .or_else(|| blow.strip_suffix(" gains life"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LifeChanged {
            player,
            gained: true,
            first_each_turn: false,
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" lose life")
        .or_else(|| blow.strip_suffix(" loses life"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LifeChanged {
            player,
            gained: false,
            first_each_turn: false,
        }]);
    }
    if let Some(subj_raw) = blow.strip_prefix("you cycle ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Cycled {
            subject: Box::new(subj),
        }]);
    }

    // Tap / Untap / Mana / Targeted / Face-up / Transform / Counters
    if let Some(subj_raw) = blow
        .strip_suffix(" becomes tapped")
        .or_else(|| blow.strip_suffix(" become tapped"))
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(subj),
            tapped: true,
            for_mana: false,
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" becomes untapped")
        .or_else(|| blow.strip_suffix(" become untapped"))
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(subj),
            tapped: false,
            for_mana: false,
        }]);
    }
    if let Some(subj_raw) = blow.strip_suffix(" is tapped for mana")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(subj),
            tapped: true,
            for_mana: true,
        }]);
    }
    if let Some(rest) = blow.strip_suffix(" for mana")
        && let Some((_, subj_raw)) = rest
            .split_once(" taps ")
            .or_else(|| rest.split_once(" tap "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::TapStateChanged {
            subject: Box::new(subj),
            tapped: true,
            for_mana: true,
        }]);
    }
    if let Some((subj_raw, by_raw)) = blow
        .split_once(" becomes the target of ")
        .or_else(|| blow.split_once(" become the target of "))
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::BecomesTargeted {
            subject: Box::new(subj),
            by_opponent: by_raw.contains("opponent"),
            by_text: Some(by_raw.to_string()),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" is exiled from the battlefield")
        .or_else(|| blow.strip_suffix(" are exiled from the battlefield"))
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::ZoneChange {
            from: Some("battlefield".to_string()),
            to: Some("exile".to_string()),
            subject: Box::new(subj),
            grouped: false,
            entering_controller: None,
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" fights")
        .or_else(|| blow.strip_suffix(" fight"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: format!("fight:{subj:?}"),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" is turned face up")
        .or_else(|| blow.strip_suffix(" are turned face up"))
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::TurnedFaceUp {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" transforms into ~")
        .or_else(|| blow.strip_suffix(" transforms"))
        .or_else(|| blow.strip_suffix(" transform"))
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        return Ok(vec![TriggerEventAst::Transforms {
            subject: Box::new(subj),
        }]);
    }
    if let Some(after) = blow.strip_prefix("you put ")
        && let Some((kind_raw, subj_raw)) = after
            .split_once(" counters on ")
            .or_else(|| after.split_once(" counter on "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let kind = kind_raw
            .strip_prefix("one or more ")
            .or_else(|| kind_raw.strip_prefix("a "))
            .or_else(|| kind_raw.strip_prefix("an "))
            .unwrap_or(kind_raw);
        return Ok(vec![TriggerEventAst::CountersChanged {
            subject: Box::new(subj),
            counter_kind: kind.trim().to_string(),
            added: true,
            by_you: true,
        }]);
    }
    let blow_noexile = blow
        .strip_suffix(" while it's exiled")
        .or_else(|| blow.strip_suffix(" for the first time each turn"))
        .unwrap_or(blow);
    if let Some((left, subj_raw)) = blow_noexile
        .split_once(" is put on ")
        .or_else(|| blow_noexile.split_once(" are put on "))
        .or_else(|| blow_noexile.split_once(" is placed on "))
        .or_else(|| blow_noexile.split_once(" are placed on "))
        .or_else(|| blow_noexile.split_once(" is removed from "))
        .or_else(|| blow_noexile.split_once(" are removed from "))
        && left.contains("counter")
        && let Ok(subj) = parse_subject_or_target(peel_one_or_more(subj_raw).0, vocab)
    {
        let added = !blow_noexile.contains("removed from");
        let kind = left
            .strip_prefix("one or more ")
            .or_else(|| left.strip_prefix("a "))
            .or_else(|| left.strip_prefix("an "))
            .unwrap_or(left)
            .trim_end_matches(" counters")
            .trim_end_matches(" counter")
            .trim();
        return Ok(vec![TriggerEventAst::CountersChanged {
            subject: Box::new(subj),
            counter_kind: kind.to_string(),
            added,
            by_you: false,
        }]);
    }

    // State trigger
    if (blow.starts_with("you control no ") || blow.starts_with("there are no "))
        && let Ok(cond) = parse_condition(blow, vocab)
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(cond))]);
    }

    // Eerie compound trigger: "Whenever an enchantment you control enters and whenever you fully unlock a Room"
    if blow == "an enchantment you control enters and whenever you fully unlock a room" {
        let flt = crate::ast::ObjectFilterAst {
            any_types: crate::ast::CardTypeMask::ENCHANTMENT,
            controller: Some(ControllerRelation::You),
            ..Default::default()
        };
        return Ok(vec![
            TriggerEventAst::ZoneChange {
                from: None,
                to: Some("battlefield".to_string()),
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                grouped: false,
                entering_controller: None,
            },
            TriggerEventAst::RoomFullyUnlocked {
                player: PlayerRefAst::You,
            },
        ]);
    }
    if blow == "you cast a spell from anywhere other than your hand" {
        return Ok(vec![TriggerEventAst::CastFromOutsideHand {
            player: PlayerRefAst::You,
        }]);
    }
    if blow == "you cycle or discard a card" {
        let flt = crate::ast::ObjectFilterAst {
            any_card: true,
            ..Default::default()
        };
        return Ok(vec![
            TriggerEventAst::Cycled {
                subject: Box::new(SubjectRefAst::Group(Box::new(flt.clone()))),
            },
            TriggerEventAst::CardDiscarded {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
            },
        ]);
    }
    if let Some(n_raw) = blow.strip_prefix("you expend ")
        && let Ok(amount) = n_raw.trim().parse::<u16>()
    {
        return Ok(vec![TriggerEventAst::Expend {
            player: PlayerRefAst::You,
            amount,
        }]);
    }

    // Modern & supplemental Magic keyword/mechanic triggers
    if blow == "you crank this contraption" || blow == "you crank ~" {
        return Ok(vec![TriggerEventAst::CrankContraption]);
    }
    if blow == "you unlock this door" || blow == "you unlock a door" || blow == "you unlock ~" {
        return Ok(vec![TriggerEventAst::UnlockDoor {
            subject: Box::new(SubjectRefAst::SelfObject),
        }]);
    }
    if blow == "you fully unlock a room" {
        return Ok(vec![TriggerEventAst::RoomFullyUnlocked {
            player: PlayerRefAst::You,
        }]);
    }
    if blow == "this case is solved" || blow == "~ is solved" {
        return Ok(vec![TriggerEventAst::CaseSolved]);
    }
    if blow == "~ is defeated" || blow == "this battle is defeated" {
        return Ok(vec![TriggerEventAst::BattleDefeated]);
    }
    if let Some(lvl_raw) = blow
        .strip_prefix("~ becomes level ")
        .or_else(|| blow.strip_prefix("this class becomes level "))
        && let Ok(level) = lvl_raw.trim().parse::<u8>()
    {
        return Ok(vec![TriggerEventAst::ClassLevelReached { level }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" mutates")
        .or_else(|| blow.strip_suffix(" mutate"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Mutates {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow.strip_suffix(" becomes monstrous")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::BecomesMonstrous {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow.strip_suffix(" becomes renowned")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::BecomesRenowned {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" exploits a creature")
        .or_else(|| blow.strip_suffix(" exploit a creature"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Exploits {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" becomes enchanted")
        .or_else(|| blow.strip_suffix(" becomes equipped"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let kind = if blow.ends_with("equipped") {
            "equipped"
        } else {
            "enchanted"
        };
        return Ok(vec![TriggerEventAst::BecomesAttached {
            subject: Box::new(subj),
            attachment_kind: kind.to_string(),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" becomes exerted")
        .or_else(|| blow.strip_prefix("you exert "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Exerted {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" becomes crewed")
        .or_else(|| blow.strip_suffix(" crews a vehicle"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Crewed {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" explores")
        .or_else(|| blow.strip_suffix(" explore"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Explores {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" connives")
        .or_else(|| blow.strip_suffix(" connive"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Connives {
            subject: Box::new(subj),
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" commit a crime")
        .or_else(|| blow.strip_suffix(" commits a crime"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::CrimeCommitted { player }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" play a land")
        .or_else(|| blow.strip_suffix(" plays a land"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::LandPlayed { player }]);
    }
    for (suf, action) in [
        (" scry", "scry"),
        (" scries", "scry"),
        (" surveil", "surveil"),
        (" surveils", "surveil"),
        (" investigate", "investigate"),
        (" investigates", "investigate"),
        (" proliferate", "proliferate"),
        (" proliferates", "proliferate"),
        (" populate", "populate"),
        (" populates", "populate"),
        (" forage", "forage"),
        (" forages", "forage"),
        (" venture into the dungeon", "venture"),
        (" ventures into the dungeon", "venture"),
        (" complete a dungeon", "complete_dungeon"),
        (" completes a dungeon", "complete_dungeon"),
        (" manifest dread", "manifest_dread"),
        (" manifests dread", "manifest_dread"),
    ] {
        if let Some(who_raw) = blow.strip_suffix(suf)
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Ok(vec![TriggerEventAst::KeywordActionPerformed {
                player,
                action: action.to_string(),
            }]);
        }
    }
    if let Some((who_raw, roll_raw)) = blow
        .split_once(" roll a ")
        .or_else(|| blow.split_once(" rolls a "))
        && let (Some(player), Ok(n)) = (parse_player_ref(who_raw), roll_raw.trim().parse::<u16>())
    {
        return Ok(vec![TriggerEventAst::RollDie {
            player,
            exact_result: Some(n),
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" roll one or more dice")
        .or_else(|| blow.strip_suffix(" rolls one or more dice"))
        .or_else(|| blow.strip_suffix(" roll a die"))
        .or_else(|| blow.strip_suffix(" rolls a die"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::RollDie {
            player,
            exact_result: None,
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" win a coin flip")
        .or_else(|| blow.strip_suffix(" wins a coin flip"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::FlipCoin {
            player,
            won: Some(true),
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" lose a coin flip")
        .or_else(|| blow.strip_suffix(" loses a coin flip"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::FlipCoin {
            player,
            won: Some(false),
        }]);
    }
    if let Some(who_raw) = blow
        .strip_suffix(" flip a coin")
        .or_else(|| blow.strip_suffix(" flips a coin"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![TriggerEventAst::FlipCoin { player, won: None }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" phases out")
        .or_else(|| blow.strip_suffix(" phase out"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::PhaseChanged {
            subject: Box::new(subj),
            phased_out: true,
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" phases in")
        .or_else(|| blow.strip_suffix(" phase in"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::PhaseChanged {
            subject: Box::new(subj),
            phased_out: false,
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" is exiled")
        .or_else(|| blow.strip_suffix(" are exiled"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Exiled {
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" regenerates this way")
        .or_else(|| blow.strip_suffix(" regenerates"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: format!("regenerates:{subj:?}"),
        }]);
    }

    // Orthogonal Pass J triggers: "one or more <flt> deals combat damage to <recip>" (Kutzil) & "you [next] cast that spell / <flt>" (Ogre Battlecaster)
    if let Some((subj_raw, recip_raw)) = blow.split_once(" deals combat damage to ")
        && subj_raw.starts_with("one or more ")
        && let Ok(flt) = parse_object_filter(subj_raw.trim_start_matches("one or more "), vocab)
        && let Ok(recip) = parse_subject_or_target(recip_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::DamageDealt {
            combat_only: true,
            noncombat_only: false,
            source: Some(Box::new(SubjectRefAst::Group(Box::new(flt)))),
            recipient: Some(Box::new(recip)),
            grouped: true,
        }]);
    }
    if blow == "you cast that spell" || blow == "you next cast that spell" {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: PlayerRefAst::You,
            spell: None,
            self_spell: false,
            targets: None,
            nth_each_turn: None,
            include_copy: false,
        }]);
    }
    if let Some(after) = blow.strip_prefix("you next cast ") {
        let clean = after.strip_suffix(" this turn").unwrap_or(after).trim();
        let (_, flt_raw) = peel_leading_count(clean, vocab);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(vec![TriggerEventAst::SpellCast {
                caster: PlayerRefAst::You,
                spell: Some(Box::new(flt)),
                self_spell: false,
                targets: None,
                nth_each_turn: None,
                include_copy: false,
            }]);
        }
    }

    // Orthogonal Pass I triggers: token creation, loyalty/nonmana ability activation, crewed, last counter, state triggers, keyword actions
    if let Some((who_raw, rest)) = blow
        .split_once(" create or sacrifice ")
        .or_else(|| blow.split_once(" creates or sacrifices "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        let (flt_raw, _) = peel_one_or_more(rest);
        let (_, clean) = peel_leading_count(flt_raw, vocab);
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(vec![
                TriggerEventAst::ZoneChange {
                    from: None,
                    to: Some("battlefield".to_string()),
                    subject: Box::new(SubjectRefAst::Group(Box::new(flt.clone()))),
                    grouped: false,
                    entering_controller: None,
                },
                TriggerEventAst::PermanentSacrificed {
                    player,
                    subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                },
            ]);
        }
    }
    if let Some((who_raw, rest)) = blow
        .split_once(" create ")
        .or_else(|| blow.split_once(" creates "))
        && let Some(_player) = parse_player_ref(who_raw)
    {
        let (flt_raw, grouped) = peel_one_or_more(rest);
        let (_, clean) = peel_leading_count(flt_raw, vocab);
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(vec![TriggerEventAst::ZoneChange {
                from: None,
                to: Some("battlefield".to_string()),
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                grouped,
                entering_controller: Some(ControllerRelation::You),
            }]);
        }
    }
    if let Some((who_raw, rest)) = blow
        .split_once(" activate ")
        .or_else(|| blow.split_once(" activates "))
        && let Some(player) = parse_player_ref(who_raw)
        && (rest.starts_with("a loyalty ability of ")
            || rest.starts_with("an ability that isn't a mana ability")
            || rest.starts_with("an ability that targets "))
    {
        let src_flt = if let Some(subj_raw) = rest.strip_prefix("a loyalty ability of ") {
            parse_object_filter(subj_raw, vocab).unwrap_or_else(|_| crate::ast::ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            })
        } else {
            crate::ast::ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            }
        };
        return Ok(vec![TriggerEventAst::AbilityActivated {
            player,
            source_filter: Box::new(src_flt),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" becomes crewed")
        .or_else(|| blow.strip_suffix(" become crewed"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Crewed {
            subject: Box::new(subj),
        }]);
    }
    if let Some((who_raw, rest)) = blow.split_once(" remove the last ")
        && let Some((ck, subj_raw)) = rest.split_once(" counter from ")
        && parse_player_ref(who_raw).is_some()
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::CountersChanged {
            subject: Box::new(subj),
            counter_kind: ck.trim().to_string(),
            added: false,
            by_you: true,
        }]);
    }
    if let Some((who_raw, rest)) = blow
        .split_once(" roll a ")
        .or_else(|| blow.split_once(" rolls a "))
        && let Some(player) = parse_player_ref(who_raw)
        && rest
            .split(|c: char| !c.is_ascii_digit())
            .any(|tok| !tok.is_empty())
    {
        let first_num = rest
            .split(|c: char| !c.is_ascii_digit())
            .find(|tok| !tok.is_empty())
            .and_then(|t| t.parse::<u16>().ok());
        return Ok(vec![TriggerEventAst::RollDie {
            player,
            exact_result: first_num,
        }]);
    }
    for (kw_act, label) in [
        (" discover", "discover"),
        (" discovers", "discover"),
        (" clash", "clash"),
        (" clashes", "clash"),
        (" foretell a card", "foretell"),
        (" foretells a card", "foretell"),
    ] {
        if let Some(who_raw) = blow.strip_suffix(kw_act)
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Ok(vec![TriggerEventAst::KeywordActionPerformed {
                player,
                action: label.to_string(),
            }]);
        }
    }
    if blow == "you cast or cycle ~" {
        return Ok(vec![
            TriggerEventAst::SpellCast {
                caster: PlayerRefAst::You,
                spell: None,
                self_spell: true,
                targets: None,
                nth_each_turn: None,
                include_copy: false,
            },
            TriggerEventAst::Cycled {
                subject: Box::new(SubjectRefAst::SelfObject),
            },
        ]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" is sacrificed or destroyed")
        .or_else(|| blow.strip_suffix(" are sacrificed or destroyed"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            TriggerEventAst::PermanentSacrificed {
                player: PlayerRefAst::AnyPlayer,
                subject: Box::new(subj.clone()),
            },
            TriggerEventAst::ZoneChange {
                from: Some("battlefield".to_string()),
                to: Some("graveyard".to_string()),
                subject: Box::new(subj),
                grouped: false,
                entering_controller: None,
            },
        ]);
    }
    // Pass N orthogonal triggers: play/cast from zone, grouped and/or enters, returned to hand,
    // ability activated, boast, trains, untap, shuffle, opponents dealt combat damage, attack with, gain control
    if blow == "you play a card from exile" || blow == "you cast a spell from exile" {
        return Ok(vec![TriggerEventAst::CastFromOutsideHand {
            player: PlayerRefAst::You,
        }]);
    }
    if let Some(after) = blow.strip_prefix("you cast ~ from ")
        && !after.is_empty()
    {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: PlayerRefAst::You,
            spell: None,
            self_spell: true,
            targets: None,
            nth_each_turn: None,
            include_copy: false,
        }]);
    }
    if let Some(after) = blow
        .strip_prefix("~ and/or one or more other ")
        .and_then(|r| {
            r.strip_suffix(" enter")
                .or_else(|| r.strip_suffix(" enter the battlefield"))
        })
        && let Ok(flt) = parse_object_filter(after, vocab)
    {
        return Ok(vec![TriggerEventAst::ZoneChange {
            from: None,
            to: Some("battlefield".to_string()),
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            grouped: true,
            entering_controller: None,
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" is returned to its owner's hand")
        .or_else(|| blow.strip_suffix(" is returned to your hand"))
        .or_else(|| blow.strip_suffix(" are returned to their owners' hands"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::ZoneChange {
            from: Some("battlefield".to_string()),
            to: Some("hand".to_string()),
            subject: Box::new(subj),
            grouped: false,
            entering_controller: None,
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_prefix("an ability of ")
        .and_then(|r| r.strip_suffix(" is activated"))
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return Ok(vec![TriggerEventAst::AbilityActivated {
            player: PlayerRefAst::AnyPlayer,
            source_filter: Box::new(crate::ast::ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            }),
        }]);
    }
    if blow == "you activate a boast ability" {
        return Ok(vec![TriggerEventAst::AbilityActivated {
            player: PlayerRefAst::You,
            source_filter: Box::new(crate::ast::ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            }),
        }]);
    }
    if let Some(subj_raw) = blow
        .strip_suffix(" trains")
        .or_else(|| blow.strip_suffix(" train"))
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: "train".to_string(),
        }]);
    }
    if let Some(after) = blow.strip_prefix("you untap ") {
        let clean = after
            .strip_suffix(" during your untap step")
            .unwrap_or(after);
        let (flt_raw, _) = peel_one_or_more(clean);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(vec![TriggerEventAst::TapStateChanged {
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                tapped: false,
                for_mana: false,
            }]);
        }
    }
    if blow == "a spell or ability causes a player to shuffle their library"
        || blow == "a player shuffles their library"
    {
        return Ok(vec![TriggerEventAst::LibrarySearched {
            player: PlayerRefAst::AnyPlayer,
        }]);
    }
    if blow == "one or more of your opponents are dealt combat damage"
        || blow == "your opponents are dealt combat damage"
    {
        return Ok(vec![TriggerEventAst::DamageDealt {
            combat_only: true,
            noncombat_only: false,
            source: None,
            recipient: Some(Box::new(SubjectRefAst::Player(PlayerRefAst::EachOpponent))),
            grouped: true,
        }]);
    }
    if let Some(after) = blow
        .strip_prefix("you attack a player or planeswalker with ")
        .or_else(|| blow.strip_prefix("you attack with "))
    {
        let (flt_raw, grouped) = peel_one_or_more(after);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(vec![TriggerEventAst::Attacks {
                subject: Some(Box::new(SubjectRefAst::Group(Box::new(flt)))),
                actor: Some(PlayerRefAst::You),
                qualifier: None,
                grouped,
            }]);
        }
    }
    if let Some(subj_raw) = blow
        .strip_prefix("you gain control of ")
        .and_then(|r| r.strip_suffix(" from another player"))
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: format!("gain_control:{subj_raw}"),
        }]);
    }

    // State trigger: "When ~ has no <kind> counters on it" / "When there are <N> or more <kind> counters on <subj>" (Nine Lives, Deadly Designs, Afiya Grove)
    if ((blow.contains(" has no ") && blow.ends_with(" on it"))
        || (blow.starts_with("there are ") && blow.contains(" counter")))
        && let Ok(cond) = parse_condition(blow, vocab)
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(cond))]);
    }
    // "<subj> attacks a player who <cond>" / "<subj> attacks the player with the most life [or tied for most life]"
    if let Some((subj_raw, rest)) = blow
        .split_once(" attacks a player who ")
        .or_else(|| blow.split_once(" attacks the player with "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Attacks {
            subject: Some(Box::new(subj)),
            actor: None,
            qualifier: Some(rest.to_string()),
            grouped: false,
        }]);
    }

    // Peel trailing temporal qualifier (" this turn" / " this way" / " during your turn" / " during combat" / " for the first time each turn") and retry
    if let Some(stripped) = blow
        .strip_suffix(" for the first time each turn")
        .or_else(|| blow.strip_suffix(" during your turn"))
        .or_else(|| blow.strip_suffix(" during their turn"))
        .or_else(|| blow.strip_suffix(" during combat"))
        .or_else(|| blow.strip_suffix(" this turn"))
        .or_else(|| blow.strip_suffix(" this way"))
        && let Ok(evs) = parse_trigger_events(&format!("whenever {stripped}"), vocab)
    {
        return Ok(evs);
    }

    // Pass Q orthogonal trigger events:
    // Evolves, plotted, saddled, play a card/land, pay life, give a gift, clash and win,
    // attacks a battle, cast your commander, enlists, collect evidence, dealt combat damage,
    // visit an Attraction, sacrifice one or more <flt>, two or more <flt> attack, returned to hand
    if blow == "~ evolves" {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: "evolve".to_string(),
        }]);
    }
    if blow == "~ becomes plotted" || blow == "~ becomes saddled" {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: blow.to_string(),
        }]);
    }
    if matches!(
        blow,
        "you play a card"
            | "you pay life"
            | "you give a gift"
            | "you clash and win"
            | "you collect evidence"
            | "you visit an attraction"
    ) {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: blow.to_string(),
        }]);
    }
    if let Some(flt_raw) = blow.strip_prefix("you play ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(vec![TriggerEventAst::LandPlayed {
            player: PlayerRefAst::You,
        }]);
    }
    if blow == "you cast your commander" {
        return Ok(vec![TriggerEventAst::SpellCast {
            caster: PlayerRefAst::You,
            spell: None,
            self_spell: false,
            targets: None,
            nth_each_turn: None,
            include_copy: false,
        }]);
    }
    if blow == "you're dealt combat damage" || blow == "you are dealt combat damage" {
        return Ok(vec![TriggerEventAst::DamageDealt {
            combat_only: true,
            noncombat_only: false,
            source: None,
            recipient: Some(Box::new(SubjectRefAst::Player(PlayerRefAst::You))),
            grouped: false,
        }]);
    }
    if let Some(subj_raw) = blow.strip_suffix(" attacks a battle")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Attacks {
            subject: Some(Box::new(subj)),
            actor: None,
            qualifier: Some("battle".to_string()),
            grouped: false,
        }]);
    }
    if blow.starts_with("~ enlists ") {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: "enlist".to_string(),
        }]);
    }
    if let Some(after) = blow.strip_prefix("you sacrifice one or more ")
        && let Ok(subj) = parse_subject_or_target(after, vocab)
    {
        return Ok(vec![TriggerEventAst::PermanentSacrificed {
            player: PlayerRefAst::You,
            subject: Box::new(subj),
        }]);
    }
    if let Some(after) = blow.strip_prefix("two or more ")
        && let Some(flt_raw) = after.strip_suffix(" attack")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::Attacks {
            subject: Some(Box::new(SubjectRefAst::Group(Box::new(flt)))),
            actor: None,
            qualifier: Some("two_or_more".to_string()),
            grouped: true,
        }]);
    }
    if let Some(subj_raw) = blow.strip_suffix(" is returned to a player's hand")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![TriggerEventAst::ZoneChange {
            from: Some("battlefield".to_string()),
            to: Some("hand".to_string()),
            subject: Box::new(subj),
            grouped: false,
            entering_controller: None,
        }]);
    }
    // CR Wave 2 triggers
    if low == "at the beginning of the monarch's end step" {
        return Ok(vec![TriggerEventAst::StepBegan {
            step: TurnStepAst::EndStep,
            whose: "monarch".to_string(),
        }]);
    }
    if matches!(
        blow,
        "~ and another creature attack different players"
            | "~ or a creature it's paired with is dealt damage"
            | "you cast an instant spell during your main phase"
            | "you cast a sorcery spell during your main phase"
    ) || (blow.starts_with("a creature has ") && blow.ends_with(" counters on it"))
        || (blow.starts_with("you cast your ") && blow.ends_with(" spell each turn"))
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: blow.to_string(),
        }]);
    }
    // CR 603.8 State trigger & Pass R orthogonal triggers
    if matches!(
        blow,
        "~'s echo cost is paid"
            | "you pay ~'s cumulative upkeep"
            | "you tap a permanent for {c}"
            | "you attack enchanted player"
            | "you turn a permanent face up"
            | "combat damage is dealt to you"
            | "you activate a loyalty ability"
            | "you activate a ninjutsu ability"
            | "you activate ~'s outlast ability"
            | "one or more opponents lose life"
            | "enchanted opponent draws a card"
            | "a spell you've cast is countered"
            | "you roll your third die each turn"
            | "the fourth spell of a turn is cast"
            | "you cast your third spell in a turn"
            | "a player plays a card exiled with ~"
            | "you put an art sticker on a creature"
            | "you put a name sticker on a creature"
            | "~ transforms into ~"
            | "~ transforms into ~ or dies"
            | "~ phases out"
            | "~ phases out or leaves the battlefield"
            | "you gain or lose life"
            | "equipped creature mentors a creature"
            | "equipped creature attacks the monarch"
            | "you attack the player who has the initiative"
            | "one or more of your opponents are attacked"
            | "you roll a die's highest natural result"
            | "~ becomes tapped to pay a teamwork cost"
            | "you put an ability sticker on a creature"
            | "you choose a creature as your ring-bearer"
            | "you cast a spell with one or more targets"
            | "you activate an eternalize or embalm ability"
            | "you sacrifice ~ while casting a spell with emerge"
            | "the first noncreature spell of a turn is cast"
            | "one or more players discard one or more cards"
            | "one or more opponents each lose exactly 1 life"
            | "damage that would be dealt to you is prevented"
            | "a spell or ability you control counters a spell"
            | "~ enters from your hand"
            | "a player other than ~'s owner controls it"
            | "a player doesn't pay ~'s cumulative upkeep"
            | "you gain life for the first time during each of your turns"
            | "an opponent loses life for the first time during each of their turns"
            | "the final chapter ability of a saga you control resolves"
            | "you solve a case"
            | "you place a sticker"
            | "you put a sticker on ~"
            | "you roll to visit your attractions"
            | "you activate a power-up ability"
            | "an instant or sorcery spell is cast during your turn"
            | "an opponent plays a nonbasic land"
            | "a permanent owned by another player is put into a graveyard from the battlefield"
            | "~ or another creature you control leaves the battlefield without dying"
            | "combat damage is dealt to you or a planeswalker you control"
            | "another player casts a spell from anywhere other than their hand"
            | "an opponent sacrifices a nontoken permanent or discards a permanent card"
            | "an opponent puts one or more counters on a creature they control"
            | "you cycle ~ or cycle another card"
            | "one or more creatures your opponents control are dealt excess noncombat damage"
            | "one or more creatures an opponent controls attack you and aren't blocked"
            | "a creature attacks and isn't blocked this combat"
            | "~ enters attached to a creature"
            | "you and/or at least one permanent you control becomes the target of a spell or ability an opponent controls"
            | "a spell or ability an opponent controls causes you to discard cards this turn"
            | "a spell or ability causes its controller to shuffle their library"
            | "a player chooses one or more targets"
            | "~ attacks while you have the most life or are tied for most life"
            | "a vehicle crewed by ~ this turn attacks"
            | "you cast an instant or sorcery spell that targets only ~ or activate an ability that targets only ~"
            | "~ is exiled from the battlefield while you're activating a craft ability"
            | "a land you control enters from anywhere other than your hand or you cast a spell from anywhere other than your hand"
            | "enchanted player casts a spell other than the first spell they cast each turn or copies a spell"
            | "an opponent casts an instant spell other than the first instant spell that player casts each turn"
            | "you activate an ability"
            | "you cast an instant or sorcery spell or activate an ability"
            | "you cast your first spell during each of your turns"
            | "you tap a land for {c}"
            | "equipped creature and at least one other creature attack"
            | "you attack a player with one or more equipped creatures"
            | "a creature you own but don't control attacks"
            | "damage is dealt to one of the chosen players"
            | "you tap one or more untapped creatures your opponents control"
            | "a creature you control exploits a nontoken creature"
            | "you cast a spell that's exactly two colors"
            | "your commander is put into the command zone from anywhere"
            | "a creature dealt damage by equipped creature this turn dies"
            | "~ or another creature is returned to your hand from the battlefield"
            | "one or more noncreature permanents are returned to hand"
            | "a source you control deals 5 or more damage to a player"
            | "another source you control deals exactly 1 damage to a permanent or player"
            | "a spell or ability an opponent controls destroys a noncreature permanent you control"
            | "a spell or ability an opponent controls causes a land to be put into your graveyard from the battlefield"
            | "you cast a spell during your turn other than your first spell that turn"
            | "one or more creatures you control fight or become blocked"
            | "another player attacks with two or more creatures"
            | "three or more creatures you control with flying attack"
            | "you cast a spell from your graveyard or activate an ability of a card in your graveyard"
            | "you remove a time counter from ~"
            | "another creature you control leaves the battlefield without dying"
            | "you waterbend"
            | "a creature you control that was turned face up this turn deals combat damage to a player"
            | "~ enters from anywhere other than a graveyard or exile"
            | "an opponent casts a spell or activates an ability"
            | "~ becomes untapped during your untap step"
            | "the final chapter ability of a saga you control triggers"
            | "one or more other creatures you control leave the battlefield without dying"
            | "~ attacks a player and isn't blocked"
            | "one or more lands enter under an opponent's control without being played"
            | "a creature you control attacks or enters attacking"
            | "one or more other permanents phase out"
            | "a spell or ability an opponent controls causes you to discard a card"
            | "you open an attraction"
            | "you claim the prize of an attraction"
            | "a permanent entering causes a triggered ability to trigger"
            | "a creature entering under an opponent's control causes a triggered ability of that creature to trigger"
            | "a creature you control attacking causes a triggered ability of that creature to trigger"
            | "a creature you control exploits a non-human creature"
            | "an artifact becomes tapped or a player activates an artifact's ability without {t} in its activation cost"
            | "enchanted artifact becomes tapped or a player activates an ability of enchanted artifact without {t} in its activation cost"
            | "an artifact an opponent controls becomes tapped or an opponent activates an artifact's ability without {t} in its activation cost"
            | "an opponent is dealt damage by a red instant or sorcery spell you control or by a red planeswalker you control"
            | "an opponent casts a creature or planeswalker spell with the same name as a card in their graveyard"
            | "you cast an instant or sorcery spell that has the same name as a card in your graveyard"
            | "an opponent casts a vampire spell with the same name as a card exiled with ~"
            | "an instant or sorcery spell you control that targets only a single creature deals damage to that creature"
            | "a player casts a spell that shares a color or mana value with the exiled card"
            | "an opponent casts a spell with mana value, power, or toughness equal to the chosen number"
            | "you cast a spell with power, toughness, or mana value 4"
            | "the tenth +1/+1 counter is put on shang-chi"
            | "a permanent other than a basic land enters"
            | "a player casts a spell they don't own"
            | "you cast an instant or sorcery spell with a single target"
            | "one or more opponents are dealt noncombat damage"
            | "you attack a player with one or more creatures with power 4 or greater"
            | "you put one or more +1/+1 counters on one or more other heroes you control"
            | "you cast a creature spell that doesn't share a creature type with a creature you control or a creature card in your graveyard"
            | "an opponent gains control of a permanent from you"
            | "one or more creatures you control leave the battlefield without dying"
            | "one or more players sacrifice one or more creatures"
            | "an opponent casts a spell from anywhere other than their hand"
            | "you play a card with the same name as one of the exiled cards"
            | "a white instant or sorcery spell causes you to gain life"
    ) || blow.starts_with("a permanent you control transforms into ")
        || blow.starts_with("a creature you control explores ")
        || (blow.starts_with("you play a ") && blow.contains(" or cast a "))
        || (blow.starts_with("an opponent is dealt ")
            && blow.ends_with(" or more damage by a single source"))
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: blow.to_string(),
        }]);
    }
    if blow.ends_with(" is championed with ~")
        || blow.starts_with("~ deals ")
            && (blow.ends_with(" or more damage")
                || blow.ends_with(" or more damage to an opponent"))
        || blow.starts_with("~ is dealt ") && blow.ends_with(" or more damage")
        || blow.starts_with("you copy ")
        || blow.ends_with(" loses the game")
        || (blow.starts_with("~ and at least ") && blow.ends_with(" attack"))
    {
        return Ok(vec![TriggerEventAst::KeywordActionPerformed {
            player: PlayerRefAst::You,
            action: blow.to_string(),
        }]);
    }
    // CR 603.8 State trigger: "When <condition>" (Floodgate, Opal Avenger, Phyrexian Devourer, Rune-Tail, Goblins of the Flarg)
    if low.starts_with("when ")
        && let Ok(cond) = parse_condition(blow, vocab)
    {
        return Ok(vec![TriggerEventAst::StateTrigger(Box::new(cond))]);
    }

    // Generic N-way comma+OR shared-subject compound event combinator:
    // "<subj> <verb1>, <verb2>, or <verb3>"
    if blow.contains(", or ") {
        const TRIGGER_VERBS: &[&str] = &[
            " enters the battlefield",
            " enter the battlefield",
            " enters tapped",
            " enter tapped",
            " enters",
            " enter",
            " leaves the battlefield",
            " leave the battlefield",
            " dies",
            " die",
            " attacks",
            " attack",
            " blocks",
            " block",
            " becomes blocked",
            " become blocked",
            " becomes tapped",
            " become tapped",
            " becomes untapped",
            " become untapped",
            " becomes the target of ",
            " become the target of ",
            " becomes attached",
            " becomes enchanted",
            " becomes equipped",
            " is turned face up",
            " are turned face up",
            " transforms into ~",
            " transforms",
            " transform",
            " is exiled from the battlefield",
            " are exiled from the battlefield",
            " is exiled",
            " are exiled",
            " is put into ",
            " are put into ",
            " deals combat damage",
            " deal combat damage",
            " deals noncombat damage",
            " deal noncombat damage",
            " deals damage",
            " deal damage",
            " is dealt damage",
            " are dealt damage",
            " phases out",
            " phase out",
            " fights",
            " fight",
            " explores",
            " explore",
            " connives",
            " connive",
            " mutates",
            " mutate",
        ];
        let mut earliest: Option<usize> = None;
        for v in TRIGGER_VERBS {
            if let Some(pos) = blow.find(v) {
                earliest = Some(match earliest {
                    Some(cur) => cur.min(pos),
                    None => pos,
                });
            }
        }
        if let Some(v_idx) = earliest {
            let subj_part = blow[..v_idx].trim();
            let verbs_part = blow[v_idx..].trim();
            if !subj_part.is_empty() && verbs_part.contains(", or ") {
                let parts: Vec<&str> = verbs_part.split(", ").collect();
                if parts.len() >= 2
                    && parts.len() <= 4
                    && parts.last().is_some_and(|p| p.trim().starts_with("or "))
                {
                    let mut combined = Vec::new();
                    let mut all_ok = true;
                    for (i, part) in parts.iter().enumerate() {
                        let v_clean = if i + 1 == parts.len() {
                            part.trim().strip_prefix("or ").unwrap_or(part.trim())
                        } else {
                            part.trim()
                        };
                        if v_clean.is_empty() {
                            all_ok = false;
                            break;
                        }
                        match parse_trigger_events(
                            &format!("whenever {subj_part} {v_clean}"),
                            vocab,
                        ) {
                            Ok(evs) => combined.extend(evs),
                            Err(_) => {
                                all_ok = false;
                                break;
                            }
                        }
                    }
                    if all_ok && combined.len() >= 2 {
                        return Ok(combined);
                    }
                }
            }
        }
    }

    // Generic 2-way compound event combinator: "<subj> <verb1> or <verb2>"
    for verb1 in [
        " enters the battlefield",
        " enter the battlefield",
        " enters tapped",
        " enter tapped",
        " enters",
        " enter",
        " attacks alone",
        " attacks a player",
        " attack a player",
        " attacks",
        " attack",
        " blocks",
        " block",
        " becomes tapped",
        " become tapped",
        " becomes untapped",
        " become untapped",
        " becomes blocked",
        " become blocked",
        " becomes attached",
        " becomes enchanted",
        " becomes equipped",
        " is turned face up",
        " are turned face up",
        " transforms into ~",
        " transforms",
        " transform",
        " dies",
        " die",
        " leaves the battlefield",
        " leave the battlefield",
        " is exiled from the battlefield",
        " are exiled from the battlefield",
        " is exiled",
        " are exiled",
        " is put into exile",
        " are put into exile",
        " is put into a graveyard",
        " is put into your graveyard",
        " deals combat damage to a player",
        " deal combat damage to a player",
        " deals combat damage",
        " deal combat damage",
        " is dealt damage",
        " are dealt damage",
        " phases out",
        " phase out",
        " fights",
        " fight",
        " explores",
        " explore",
        " connives",
        " connive",
        " mutates",
        " mutate",
    ] {
        let needle = format!("{verb1} or ");
        if let Some((subj_part, verb2)) = blow.split_once(&needle)
            && !subj_part.trim().is_empty()
            && !verb2.trim().is_empty()
            && let (Ok(mut e1), Ok(e2)) = (
                parse_trigger_events(&format!("whenever {subj_part}{verb1}"), vocab),
                parse_trigger_events(&format!("whenever {subj_part} {verb2}"), vocab),
            )
        {
            e1.extend(e2);
            return Ok(e1);
        }
    }

    // Compound trigger across different subjects: "<event1> or <event2>"
    // (e.g. "Whenever you cast a blue spell or an Island you control enters")
    if blow.contains(" or ")
        && let Some((left_ev, right_ev)) = blow.split_once(" or ")
        && left_ev.trim().contains(' ')
        && right_ev.trim().contains(' ')
        && let (Ok(mut e1), Ok(e2)) = (
            parse_trigger_events(&format!("whenever {}", left_ev.trim()), vocab),
            parse_trigger_events(&format!("whenever {}", right_ev.trim()), vocab),
        )
    {
        e1.extend(e2);
        return Ok(e1);
    }

    // Tabletop Honor-System trigger claim (Gotcha / voice / social Un-card triggers)
    if is_physical_tabletop_clause(&low) {
        return Ok(vec![TriggerEventAst::ManualTabletopClaim {
            prompt: c.to_string(),
            any_player_can_trigger: true,
        }]);
    }

    Err(format!("unparsed_trigger:{c}"))
}

fn peel_one_or_more(s: &str) -> (&str, bool) {
    match s.strip_prefix("one or more ") {
        Some(rest) => (rest.trim(), true),
        None => (s.trim(), false),
    }
}

// ---------------------------------------------------------------------------
// 3. EFFECT PARSER
// ---------------------------------------------------------------------------

/// Parse every sentence in an effect body, collecting typed instructions and strict failures.
pub fn parse_effect_body(
    body: &str,
    vocab: &Vocabulary,
) -> (Vec<InstructionAst>, Vec<ParseFailureAst>) {
    let mut instructions = Vec::new();
    let mut failures = Vec::new();
    for sent in split_sentences(body) {
        match parse_atomic_effect(sent, vocab) {
            Ok(ins) => instructions.extend(ins),
            Err(reason) => failures.push(ParseFailureAst {
                stage: "effect".to_string(),
                text: sent.to_string(),
                reason,
            }),
        }
    }
    (instructions, failures)
}

/// Try to compile a replacement `instead` clause into typed `Vec<InstructionAst>`.
fn parse_instead_body(instead_raw: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    let trimmed = instead_raw.trim().trim_end_matches('.');
    let cleaned = if let Some((before, _)) = trimmed.split_once(" instead of ") {
        before.trim()
    } else if let Some(b) = trimmed.strip_suffix(" instead") {
        b.trim()
    } else if let Some(b) = trimmed.strip_prefix("instead ") {
        b.trim()
    } else {
        trimmed
    };
    if cleaned.is_empty() {
        return None;
    }
    let (ins, fails) = parse_effect_body(cleaned, vocab);
    if fails.is_empty() && !ins.is_empty() {
        return Some(ins);
    }
    // Typed CR 614 replacement action patterns (multipliers, additions, skips, damage redirection, exile/enters-tapped)
    let clow = cleaned.to_ascii_lowercase();
    if clow.contains("double that damage")
        || clow.contains("twice that much damage")
        || clow.contains("triple that damage")
        || clow.contains("that much damage plus ")
        || clow.contains("that much damage minus ")
    {
        return Some(vec![InstructionAst::DoubleStatOrCounters {
            subject: Box::new(SubjectRefAst::Anaphora("damage".to_string())),
            kind: "damage".to_string(),
            duration: DurationAst::Instant,
        }]);
    }
    if clow.contains("twice that many ")
        || clow.contains("twice that much ")
        || clow.contains("that many plus one")
        || clow.contains("plus one ")
        || clow.contains("an additional ")
    {
        return Some(vec![InstructionAst::DoubleStatOrCounters {
            subject: Box::new(SubjectRefAst::Anaphora("replacement_event".to_string())),
            kind: "modified_count".to_string(),
            duration: DurationAst::Instant,
        }]);
    }
    if clow.starts_with("skip ") || clow.contains(" skips ") || clow.contains(" skip ") {
        return Some(vec![InstructionAst::SkipNextStep {
            player: PlayerRefAst::ThatPlayer,
            step: TurnStepAst::DrawStep,
        }]);
    }
    if clow.contains("prevent that damage") || clow.contains("prevent all of that damage") {
        return Some(vec![InstructionAst::PreventAllDamage {
            combat_only: false,
            target: None,
            duration: DurationAst::Instant,
        }]);
    }
    if clow.contains("exile it") || clow.contains("exile them") || clow.contains("exile that card")
    {
        return Some(vec![InstructionAst::Exile {
            actor: None,
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
        }]);
    }
    if clow.contains("enters tapped") || clow.contains("enter tapped") {
        return Some(vec![InstructionAst::TapPermanent {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
        }]);
    }
    if clow.contains("dealt to ") || clow.contains("deal that damage to ") {
        return Some(vec![InstructionAst::RedirectDamage {
            amount: Box::new(ValueExprAst::PriorResult),
            from: Box::new(SubjectRefAst::Anaphora("original_target".to_string())),
            to: Box::new(SubjectRefAst::Anaphora("replacement_target".to_string())),
        }]);
    }
    if clow.contains("put it on ")
        || clow.contains("put it into ")
        || clow.contains("put it onto ")
        || clow.contains("puts it on ")
        || clow.contains("puts it into ")
        || clow.contains("puts it onto ")
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            to: clow,
        }]);
    }
    None
}

fn strip_duration<'a>(s: &'a str, vocab: &Vocabulary) -> (&'a str, Option<DurationAst>) {
    let low = s.to_ascii_lowercase();
    if low.ends_with(" until end of combat") {
        return (
            s[..s.len() - " until end of combat".len()].trim(),
            Some(DurationAst::UntilEndOfCombat),
        );
    }
    if low.ends_with(" this combat") {
        return (
            s[..s.len() - " this combat".len()].trim(),
            Some(DurationAst::UntilEndOfCombat),
        );
    }
    if low.ends_with(" until end of turn") {
        return (
            s[..s.len() - " until end of turn".len()].trim(),
            Some(DurationAst::UntilEndOfTurn),
        );
    }
    if low.ends_with(" until the end of your next turn") {
        return (
            s[..s.len() - " until the end of your next turn".len()].trim(),
            Some(DurationAst::UntilEndOfNextTurn),
        );
    }
    if low.ends_with(" until the end of your next upkeep") {
        return (
            s[..s.len() - " until the end of your next upkeep".len()].trim(),
            Some(DurationAst::UntilYourNextTurn),
        );
    }
    if low.ends_with(" until its controller's next untap step") {
        return (
            s[..s.len() - " until its controller's next untap step".len()].trim(),
            Some(DurationAst::UntilYourNextTurn),
        );
    }
    if low.ends_with(" until the end of that turn") {
        return (
            s[..s.len() - " until the end of that turn".len()].trim(),
            Some(DurationAst::UntilEndOfTurn),
        );
    }
    if low.ends_with(" during its controller's next turn") {
        return (
            s[..s.len() - " during its controller's next turn".len()].trim(),
            Some(DurationAst::UntilYourNextTurn),
        );
    }
    if low.ends_with(" during its controller's next two untap steps") {
        return (
            s[..s.len() - " during its controller's next two untap steps".len()].trim(),
            Some(DurationAst::UntilYourNextTurn),
        );
    }
    if low.ends_with(" during that combat phase") {
        return (
            s[..s.len() - " during that combat phase".len()].trim(),
            Some(DurationAst::UntilEndOfCombat),
        );
    }
    if low.ends_with(" for the rest of the game") {
        return (
            s[..s.len() - " for the rest of the game".len()].trim(),
            Some(DurationAst::Permanent),
        );
    }
    if low.ends_with(" until your next turn") {
        return (
            s[..s.len() - " until your next turn".len()].trim(),
            Some(DurationAst::UntilYourNextTurn),
        );
    }
    if low.ends_with(" until ~ leaves the battlefield") {
        return (
            s[..s.len() - " until ~ leaves the battlefield".len()].trim(),
            Some(DurationAst::UntilSourceLeaves),
        );
    }
    if low.ends_with(" this turn") {
        return (
            s[..s.len() - " this turn".len()].trim(),
            Some(DurationAst::UntilEndOfTurn),
        );
    }
    if let Some(idx) = low.rfind(" for as long as ")
        && let Ok(cond) = parse_condition(&s[idx + " for as long as ".len()..], vocab)
    {
        return (
            s[..idx].trim(),
            Some(DurationAst::ForAsLongAs(Box::new(cond))),
        );
    }
    (s, None)
}

/// Strictly parse one atomic (or compound) effect sentence into `Vec<InstructionAst>`.
pub fn parse_atomic_effect(
    sentence: &str,
    vocab: &Vocabulary,
) -> Result<Vec<InstructionAst>, String> {
    let Some(_depth_guard) = DepthGuard::enter() else {
        return Err("max_parse_depth".to_string());
    };
    let s = sentence.trim().trim_end_matches('.');
    if s.is_empty() {
        return Ok(Vec::new());
    }
    let low = s.to_ascii_lowercase();

    // 0. Global trailing ", where X is <ValueExpr>" binder (quote-aware)
    if let Some(idx) = find_top_level_pattern(s, ", where x is ")
        && let Ok(x_val) = parse_value_expr(&s[idx + ", where x is ".len()..], vocab)
        && let Ok(inner) = parse_atomic_effect(s[..idx].trim(), vocab)
    {
        return Ok(vec![InstructionAst::BindX {
            x_expr: Box::new(x_val),
            body: inner,
        }]);
    }

    // 0b. Spree mode (`+ {M} U+2014 <effect>`), Bloomburrow Season pawprint modes (`{P} U+2014 <effect>`),
    // and Tiered mana-cost modes (`{5}{R} U+2014 <effect>`)
    if let Some(after_plus) = s.strip_prefix("+ ")
        && let Some((cost_raw, body_raw)) = after_plus.split_once(EM_DASH)
        && let Ok(cost) = parse_cost_list(cost_raw.trim(), vocab)
        && let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab)
    {
        return Ok(vec![InstructionAst::SpreeMode { cost, body }]);
    }
    if s.starts_with('{')
        && let Some((cost_raw, body_raw)) = s.split_once(EM_DASH)
    {
        let ct = cost_raw.trim();
        let cost_res = if ct.starts_with("{P}") && ct.replace("{P}", "").is_empty() {
            Ok(vec![CostAtomAst::PayMana(ct.to_string())])
        } else {
            parse_cost_list(ct, vocab)
        };
        if let Ok(cost) = cost_res {
            if let Ok(body) = parse_atomic_effect(body_raw.trim(), vocab) {
                return Ok(vec![InstructionAst::SpreeMode { cost, body }]);
            }
            if let Some((p, t)) = parse_base_pt(body_raw.trim()) {
                return Ok(vec![InstructionAst::SpreeMode {
                    cost,
                    body: vec![InstructionAst::AnimateOrSetBasePT {
                        subject: Box::new(SubjectRefAst::Anaphora("target creature".to_string())),
                        power: p,
                        toughness: t,
                        description: None,
                        keywords: Vec::new(),
                        duration: DurationAst::UntilEndOfTurn,
                    }],
                }]);
            }
        }
    }

    // 0c. Leading "instead <effect>" (Green Sun's Twilight: "If X is 5 or more, instead put ...")
    if let Some(after_inst) = low.strip_prefix("instead ")
        && !after_inst.is_empty()
        && let Ok(inner) = parse_atomic_effect(&s["instead ".len()..], vocab)
    {
        return Ok(vec![InstructionAst::ReplaceEffect(inner)]);
    }

    // 0d. Trap / conditional alternative casting cost inside spell body:
    // "you may pay <cost> rather than pay ~'s mana cost"
    if let Some(after_pay) = low.strip_prefix("you may pay ")
        && let Some(cost_raw) = after_pay
            .strip_suffix(" rather than pay ~'s mana cost")
            .or_else(|| after_pay.strip_suffix(" rather than pay this spell's mana cost"))
        && let Ok(cost) = parse_cost_list(
            &s["you may pay ".len().."you may pay ".len() + cost_raw.len()],
            vocab,
        )
    {
        return Ok(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::AlternativeCastCost { cost }),
            duration: DurationAst::Instant,
        }]);
    }

    // 0e. Pure recursive discourse/turn-order prefix combinators:
    // - "Starting with <player> [and proceeding in the chosen direction], <effect>" (Eureka, Druid of Purification, Thieves' Auction)
    // - "Otherwise, <effect>" (Auratouched Mage)
    // - "Then, <effect>" / "Then <effect>" (Gluntch, Parker Luck, Raging River)
    for pref in [
        "starting with you and proceeding in the chosen direction, ",
        "starting with the next opponent in turn order, ",
        "starting with the active player, ",
        "starting with target opponent, ",
        "starting with target player, ",
        "starting with an opponent, ",
        "starting with you, ",
        "otherwise, ",
        "then, ",
        "then ",
    ] {
        if low.starts_with(pref) {
            let rest = s[pref.len()..].trim();
            if !rest.is_empty()
                && let Ok(inner) = parse_atomic_effect(rest, vocab)
            {
                return Ok(inner);
            }
        }
    }

    // 0f. "<player> may have <subj_effect>" / "you may have <subj_effect>"
    // (Pandemonium, Eldrazi Mimic, Invasion of Ikoria, Predatory Focus, Stromgald Spy, False Orders)
    if let Some(idx) = low.find(" may have ") {
        let who_raw = low[..idx].trim();
        let rest = s[idx + " may have ".len()..].trim();
        let valid_actor = who_raw == "you"
            || parse_player_ref(who_raw).is_some()
            || who_raw.ends_with("'s controller")
            || who_raw.ends_with("'s owner");
        if valid_actor && !rest.is_empty() {
            if let Ok(inner) = parse_atomic_effect(rest, vocab) {
                return Ok(vec![InstructionAst::Optional(inner)]);
            }
            if let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(rest, vocab) {
                return Ok(vec![InstructionAst::Optional(vec![
                    InstructionAst::TemporaryStatic {
                        static_ability: Box::new(st),
                        duration: DurationAst::UntilEndOfTurn,
                    },
                ])]);
            }
        }
    }

    // 0g. "Choose <player_spec> to <effect>" (Gluntch, the Bestower)
    if low.starts_with("choose ")
        && let Some((who_part, eff_part)) = s["choose ".len()..].split_once(" to ")
        && !who_part.is_empty()
        && (who_part.to_ascii_lowercase().contains("player")
            || who_part.to_ascii_lowercase().contains("opponent"))
        && let Ok(mut inner) = parse_atomic_effect(eff_part.trim(), vocab)
    {
        let mut out = vec![InstructionAst::ChooseOption(format!(
            "choose_player:{}",
            who_part.trim()
        ))];
        out.append(&mut inner);
        return Ok(out);
    }

    // 1. Restrictions, Archenemy abandon, RNG, and mana spend restrictions
    match low.as_str() {
        "activate only as a sorcery" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "Sorcery".to_string(),
                condition: None,
            }]);
        }
        "activate only as a sorcery and only once each turn"
        | "activate only once each turn and only as a sorcery" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "SorceryOnceEachTurn".to_string(),
                condition: None,
            }]);
        }
        "activate only during your turn and only once each turn"
        | "activate only once each turn and only during your turn" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "DuringYourTurnOnceEachTurn".to_string(),
                condition: None,
            }]);
        }
        "activate only as an instant" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "Instant".to_string(),
                condition: None,
            }]);
        }
        "activate no more than twice each turn" | "activate no more than three times each turn" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "AtMostNTimesEachTurn".to_string(),
                condition: None,
            }]);
        }
        "activate only during combat"
        | "activate only during the declare blockers step"
        | "activate only during the declare attackers step" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "DuringCombat".to_string(),
                condition: None,
            }]);
        }
        "activate only during your upkeep and only once each turn"
        | "any player may activate this ability but only as a sorcery" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: low,
                condition: None,
            }]);
        }
        "choose left or right" | "choose odd or even" => {
            return Ok(vec![InstructionAst::ChooseOption(low)]);
        }
        "an opponent chooses one \u{2014}" | "target opponent chooses one \u{2014}" => {
            return Ok(vec![InstructionAst::Modal {
                choice: "opponent_one".to_string(),
                modes: Vec::new(),
            }]);
        }
        "choose one at random \u{2014}" | "choose one at random" => {
            return Ok(vec![InstructionAst::Modal {
                choice: "one_at_random".to_string(),
                modes: Vec::new(),
            }]);
        }
        "choose any number \u{2014}" => {
            return Ok(vec![InstructionAst::Modal {
                choice: "any_number".to_string(),
                modes: Vec::new(),
            }]);
        }
        "chaos ensues" => return Ok(vec![InstructionAst::ChaosEnsues]),
        "choose friend or foe" => {
            return Ok(vec![InstructionAst::ChooseOption(
                "friend or foe".to_string(),
            )]);
        }
        "an opponent chooses one of them"
        | "target opponent chooses one of them"
        | "an opponent chooses two of those cards"
        | "an opponent chooses one of those cards" => {
            return Ok(vec![InstructionAst::ChooseOption(low)]);
        }
        "it becomes plotted" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "BecomesPlotted".to_string(),
                condition: None,
            }]);
        }
        "repeat this process" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "RepeatProcess".to_string(),
                condition: None,
            }]);
        }
        "~ loses this ability and becomes an aura enchantment with enchant creature" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "LicidBecomeAura".to_string(),
                condition: None,
            }]);
        }
        "they may copy ~ and may choose a new target for that copy" => {
            return Ok(vec![
                InstructionAst::Optional(vec![InstructionAst::Copy {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    exception: None,
                    count: None,
                }]),
                InstructionAst::MayChooseNewTargetsForCopy,
            ]);
        }
        "the damage can't be prevented" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "DamageCantBePrevented".to_string(),
                condition: None,
            }]);
        }
        "round up each time" | "round down each time" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: low,
                condition: None,
            }]);
        }
        "excess damage is dealt to that creature's controller instead"
        | "excess damage is dealt to that creature's controller" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "ExcessDamageToController".to_string(),
                condition: None,
            }]);
        }
        "its owner shuffles it into their library" => {
            return Ok(vec![InstructionAst::ShuffleIntoLibrary {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            }]);
        }
        "each opponent attacking that player does the same" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "EachAttackingOpponentRepeats".to_string(),
                condition: None,
            }]);
        }
        "each copy targets a different one of those creatures" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "EachCopyDifferentTarget".to_string(),
                condition: None,
            }]);
        }
        "starting with you, each player may pay any amount of mana" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "JoinForcesPayMana".to_string(),
                condition: None,
            }]);
        }
        "head to askurza" => {
            return Ok(vec![InstructionAst::ManualTabletopAction {
                prompt: s.to_string(),
            }]);
        }
        "choose both instead"
        | "choose any number"
        | "choose two"
        | "choose any number instead"
        | "choose two instead" => {
            return Ok(vec![InstructionAst::ChooseOption(low)]);
        }
        "end the turn" => return Ok(vec![InstructionAst::EndTurn]),
        "the game is a draw"
        | "the token enters tapped and attacking"
        | "the token enters tapped and attacking that player"
        | "the tokens are goaded for the rest of the game"
        | "the copy targets that token"
        | "the copy targets ~"
        | "end the combat phase"
        | "roll the planar die"
        | "it enters tapped"
        | "they're an artifact"
        | "you recruit"
        | "remove all of them"
        | "you choose two of them"
        | "target opponent chooses one"
        | "an opponent chooses one of the exiled cards"
        | "choose a permanent type"
        | "choose a land card name"
        | "choose a creature at random"
        | "pay any amount of mana"
        | "reverse the game's turn order"
        | "reselect its target at random"
        | "investigate an additional time"
        | "those choices are revealed"
        | "pay that much life again"
        | "discover again for the same value"
        | "permanents enter tapped this turn"
        | "choose new targets for it"
        | "choose a new target for it"
        | "choose an artifact card name"
        | "skip the untap step of that turn"
        | "manifest a card from your hand"
        | "reveal a card from your hand"
        | "look at target face-down creature"
        | "it isn't an equipment"
        | "it gains ~'s other abilities"
        | "counter all abilities"
        | "counter target loyalty ability of a planeswalker"
        | "that player can't gain life for the rest of the game" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: low,
                condition: None,
            }]);
        }
        "you skip your next untap step" => {
            return Ok(vec![InstructionAst::SkipNextStep {
                player: PlayerRefAst::You,
                step: TurnStepAst::UntapStep,
            }]);
        }
        "exchange your hand and graveyard"
        | "exchange your hand and library"
        | "exchange your graveyard and library" => {
            return Ok(vec![InstructionAst::Exchange {
                kind: ExchangeKind::Zones,
                left: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
                right: None,
            }]);
        }
        "do all of the above" | "proliferate again" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: low,
                condition: None,
            }]);
        }
        "double your life total" => {
            return Ok(vec![InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                delta: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: crate::ast::PlayerStatKind::LifeTotal,
                }),
                gain: true,
            }]);
        }
        "add one mana of any of the exiled card's colors" => {
            return Ok(vec![InstructionAst::AddMana {
                player: PlayerRefAst::You,
                mana: crate::ast::ManaSpecAst::AnyColor {
                    amount: Box::new(ValueExprAst::Const(1)),
                    single_color: true,
                },
            }]);
        }
        "you skip your draw step this turn" | "you skip your next draw step" => {
            return Ok(vec![InstructionAst::SkipNextStep {
                player: PlayerRefAst::You,
                step: TurnStepAst::DrawStep,
            }]);
        }
        "you skip your next two turns" => {
            return Ok(vec![InstructionAst::SkipNextStep {
                player: PlayerRefAst::You,
                step: TurnStepAst::UntapStep,
            }]);
        }
        "a creature destroyed this way can't be regenerated"
        | "a creature dealt damage this way can't be regenerated this turn"
        | "creatures dealt damage this way can't be regenerated this turn" => {
            return Ok(vec![InstructionAst::ModifyPreviousDestroyNoRegen]);
        }
        "activate only once each turn" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "OnceEachTurn".to_string(),
                condition: None,
            }]);
        }
        "activate only once" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "Once".to_string(),
                condition: None,
            }]);
        }
        "each mode must target a different player" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "EachModeDifferentPlayer".to_string(),
                condition: None,
            }]);
        }
        "the flashback cost is equal to its mana cost" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "FlashbackEqualManaCost".to_string(),
                condition: None,
            }]);
        }
        "an opponent separates those cards into two piles" => {
            return Ok(vec![InstructionAst::SeparateIntoPiles {
                chooser: PlayerRefAst::AnyOpponent,
                pile_count: 2,
            }]);
        }
        "an opponent chooses one of those piles" => {
            return Ok(vec![InstructionAst::ChoosePile {
                chooser: PlayerRefAst::AnyOpponent,
            }]);
        }
        "you choose one of those piles" => {
            return Ok(vec![InstructionAst::ChoosePile {
                chooser: PlayerRefAst::You,
            }]);
        }
        "that player adds one mana of any type that land produced"
        | "add one mana of any type that land produced"
        | "you add one mana of any type that land produced" => {
            return Ok(vec![InstructionAst::AddManaLandProduced {
                player: if low.starts_with("that player") {
                    PlayerRefAst::ThatPlayer
                } else {
                    PlayerRefAst::You
                },
            }]);
        }
        "you have no maximum hand size for the rest of the game" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "NoMaxHandSizeRestOfGame".to_string(),
                condition: None,
            }]);
        }
        "you choose one of them"
        | "target opponent chooses self or others"
        | "an opponent chooses self or others" => {
            return Ok(vec![InstructionAst::ChooseOption(low)]);
        }
        "flip a coin until you lose a flip" => {
            return Ok(vec![InstructionAst::FlipCoin]);
        }
        "they're 2/2 cyberman artifact creatures" => {
            return Ok(vec![InstructionAst::KeepExistingTypes(low)]);
        }
        "you choose a nonland card from that player's graveyard or hand and exile it" => {
            return Ok(vec![InstructionAst::Exile {
                actor: Some(PlayerRefAst::You),
                subject: Box::new(SubjectRefAst::Anaphora(
                    "chosen_nonland_from_gy_or_hand".to_string(),
                )),
            }]);
        }
        "look at the top card of that player's library, then exile it face down" => {
            return Ok(vec![
                InstructionAst::LookTopLibrary {
                    reveal: false,
                    library: "that player".to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                },
                InstructionAst::ExileTopLibrary {
                    actor: Some(PlayerRefAst::You),
                    whose: "that player".to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                    face_down: true,
                },
            ]);
        }
        "discard all the cards in your hand, then draw that many cards" => {
            return Ok(vec![
                InstructionAst::DiscardHand {
                    player: PlayerRefAst::You,
                },
                InstructionAst::Draw {
                    player: PlayerRefAst::You,
                    count: Box::new(ValueExprAst::PriorResult),
                },
            ]);
        }
        "reveal those cards, put them into your hand, then shuffle" => {
            return Ok(vec![
                InstructionAst::LookTopLibrary {
                    reveal: true,
                    library: "ref".to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                },
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::Anaphora("them".to_string())),
                    to: "your hand".to_string(),
                },
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You,
                },
            ]);
        }
        "this ability triggers only once each turn" | "do this only once each turn" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "TriggersOnceEachTurn".to_string(),
                condition: None,
            }]);
        }
        "this ability triggers only once" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "TriggersOnce".to_string(),
                condition: None,
            }]);
        }
        "you skip your next turn" | "skip your next turn" => {
            return Ok(vec![InstructionAst::SkipNextTurn {
                player: PlayerRefAst::You,
            }]);
        }
        "convert ~" => {
            return Ok(vec![InstructionAst::Transform {
                subject: Box::new(SubjectRefAst::SelfObject),
            }]);
        }
        "it assembles a contraption" | "~ assembles a contraption" => {
            return Ok(vec![InstructionAst::AssembleContraption]);
        }
        "copy it for each time you've cast your commander from the command zone this game" => {
            return Ok(vec![InstructionAst::CopyForCommanderCasts]);
        }
        "activate only during your turn" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "DuringYourTurn".to_string(),
                condition: None,
            }]);
        }
        "activate only during your turn, before attackers are declared" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "OwnTurnBeforeAttackers".to_string(),
                condition: None,
            }]);
        }
        "activate only during your upkeep" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "DuringUpkeep".to_string(),
                condition: None,
            }]);
        }
        "any player may activate this ability"
        | "only your opponents may activate this ability"
        | "only your opponents may activate this ability and only as a sorcery" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: low,
                condition: None,
            }]);
        }
        "you may choose the same mode more than once" => {
            return Ok(vec![InstructionAst::AllowRepeatModes]);
        }
        "abandon ~" | "abandon this scheme" => {
            return Ok(vec![InstructionAst::AbandonSourceScheme]);
        }
        "flip a coin" => return Ok(vec![InstructionAst::FlipCoin]),
        "roll a d20" => return Ok(vec![InstructionAst::RollDie { sides: 20 }]),
        "roll a six-sided die" | "roll a d6" => {
            return Ok(vec![InstructionAst::RollDie { sides: 6 }]);
        }
        "roll two six-sided dice" => {
            return Ok(vec![
                InstructionAst::RollDie { sides: 6 },
                InstructionAst::RollDie { sides: 6 },
            ]);
        }
        "proliferate" => return Ok(vec![InstructionAst::Proliferate]),
        "it can't be regenerated" | "they can't be regenerated" => {
            return Ok(vec![InstructionAst::ModifyPreviousDestroyNoRegen]);
        }
        "shuffle" | "shuffle your library" | "then shuffle" => {
            return Ok(vec![InstructionAst::ShuffleLibrary {
                player: PlayerRefAst::You,
            }]);
        }
        "that player shuffles"
        | "that player shuffles their library"
        | "each player who searched their library this way shuffles" => {
            return Ok(vec![InstructionAst::ShuffleLibrary {
                player: PlayerRefAst::ThatPlayer,
            }]);
        }
        "take an extra turn after this one" | "you take an extra turn after this one" => {
            return Ok(vec![InstructionAst::ExtraTurn {
                player: PlayerRefAst::You,
            }]);
        }
        "recruit" => return Ok(vec![InstructionAst::Recruit]),
        "put them back in any order" | "put them back on top of your library in any order" => {
            return Ok(vec![InstructionAst::ReorderTopLibrary]);
        }
        "you win the game" => {
            return Ok(vec![InstructionAst::WinGame {
                player: PlayerRefAst::You,
            }]);
        }
        "x can't be 0" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "XCantBeZero".to_string(),
                condition: None,
            }]);
        }
        "damage can't be prevented this turn" => {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "DamageCantBePreventedThisTurn".to_string(),
                condition: None,
            }]);
        }
        "choose one of them" => {
            return Ok(vec![InstructionAst::ChooseOption(
                "one of them".to_string(),
            )]);
        }
        "you may choose both instead" => {
            return Ok(vec![InstructionAst::ChooseOption(
                "choose_both_instead".to_string(),
            )]);
        }
        _ => {}
    }

    if low.starts_with("spend this mana only ")
        || low.starts_with("spend only ")
        || low.starts_with("mana of any type can be spent to ")
        || low.starts_with("mana of any color can be spent to ")
        || low.starts_with("you may spend mana as though it were mana of any ")
        || low.starts_with("they may spend mana as though it were mana of any ")
    {
        return Ok(vec![InstructionAst::ManaSpendRestriction(s.to_string())]);
    }
    if low == "exile that spell instead of putting it into your graveyard as it resolves"
        || low == "exile that spell instead of putting it into its owner's graveyard as it resolves"
        || low == "exile it instead of putting it into your graveyard as it resolves"
        || low == "exile it instead of putting it into its owner's graveyard as it resolves"
    {
        return Ok(vec![InstructionAst::OneShotReplacement {
            subject: "that spell".to_string(),
            event: "put into graveyard as it resolves".to_string(),
            replace_with: "exile".to_string(),
            instead_body: Some(vec![InstructionAst::Exile {
                actor: None,
                subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
            }]),
        }]);
    }
    if let Some(after) = low.strip_prefix("double the number of each kind of counter ")
        && (after == "you have" || after.ends_with(" has") || after.ends_with(" have"))
    {
        return Ok(vec![InstructionAst::DoubleStatOrCounters {
            subject: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
            kind: "all_counters".to_string(),
            duration: DurationAst::Instant,
        }]);
    }
    if let Some(sides_str) = low.strip_prefix("roll a d") {
        let sides_part = sides_str
            .split_once(" and add ")
            .or_else(|| sides_str.split_once(" and subtract "))
            .map_or(sides_str, |(s_part, _)| s_part);
        if let Ok(sides) = sides_part.trim().parse::<u16>() {
            return Ok(vec![InstructionAst::RollDie { sides }]);
        }
    }
    if let Some(rest) = low.strip_prefix("reveal the top ")
        && let Some((cnt_raw, tail)) = rest.split_once(" cards of ")
        && tail.ends_with(" library and separate them into two piles")
        && let Ok(count) = parse_value_expr(cnt_raw, vocab)
    {
        return Ok(vec![
            InstructionAst::LookTopLibrary {
                reveal: true,
                library: "your".to_string(),
                count: Box::new(count),
            },
            InstructionAst::SeparateIntoPiles {
                chooser: PlayerRefAst::You,
                pile_count: 2,
            },
        ]);
    }
    if let Some(rest) = low.strip_prefix("reveal any number of ")
        && parse_object_filter(rest, vocab).is_ok()
    {
        return Ok(vec![InstructionAst::LookTopLibrary {
            reveal: true,
            library: "hand".to_string(),
            count: Box::new(ValueExprAst::ChosenX),
        }]);
    }
    if low.starts_with("cast ~ only ") {
        if let Some(cond_raw) = low.strip_prefix("cast ~ only if ")
            && let Ok(cond) = parse_condition(cond_raw, vocab)
        {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: "Condition".to_string(),
                condition: Some(Box::new(cond)),
            }]);
        }
        return Ok(vec![InstructionAst::AbilityRestriction {
            restriction: s.to_string(),
            condition: None,
        }]);
    }
    if let Some(rest) = low
        .strip_prefix("activate only ")
        .or_else(|| low.strip_prefix("activate this ability only "))
        .or_else(|| low.strip_prefix("any player may activate this ability but only "))
        .or_else(|| low.strip_prefix("any player may activate this ability, but only "))
    {
        let mut timing_parts: Vec<&str> = Vec::new();
        let mut cond_parts: Vec<ConditionAst> = Vec::new();
        let mut all_ok = true;
        let rest_norm = rest
            .replace(", and only ", " and only ")
            .replace(", only ", " and only ");
        for part in rest_norm.split(" and only ") {
            let p = part.trim();
            if let Some(c_raw) = p.strip_prefix("if ") {
                if let Ok(c) = parse_condition(c_raw, vocab) {
                    cond_parts.push(c);
                } else {
                    all_ok = false;
                    break;
                }
            } else if matches!(
                p,
                "as a sorcery"
                    | "as an instant"
                    | "once"
                    | "once each turn"
                    | "once each combat"
                    | "during your turn"
                    | "during an opponent's turn"
                    | "during opponents' turns"
                    | "during their turn"
                    | "during combat"
                    | "during your upkeep"
                    | "during an opponent's upkeep"
                    | "during any player's upkeep"
                    | "during any upkeep step"
                    | "during the end of combat step"
                    | "during the declare attackers step"
                    | "during the declare blockers step"
                    | "during the combat damage step"
                    | "before combat"
                    | "before attackers are declared"
                    | "before blockers are declared"
                    | "before the end of combat step"
                    | "before the combat damage step"
                    | "before the end step"
                    | "during an opponent's turn, before attackers are declared"
                    | "during their turn before the end step"
                    | "during your turn before the end step"
                    | "during your turn, before attackers are declared"
                    | "during your turn, before the combat damage step"
                    | "during combat before blockers are declared"
                    | "during combat after blockers are declared"
                    | "while ~ is on the battlefield"
                    | "while ~ is in your graveyard"
            ) {
                timing_parts.push(p);
            } else {
                all_ok = false;
                break;
            }
        }
        if all_ok && (!timing_parts.is_empty() || !cond_parts.is_empty()) {
            let combined_cond = cond_parts
                .into_iter()
                .reduce(|a, b| ConditionAst::And(Box::new(a), Box::new(b)));
            let label = if low.starts_with("any player may activate") {
                format!("AnyPlayer:{}", timing_parts.join("+"))
            } else if timing_parts.is_empty() {
                "Condition".to_string()
            } else {
                timing_parts.join("+")
            };
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: label,
                condition: combined_cond.map(Box::new),
            }]);
        }
    }

    // "This ability costs {M} less to activate for each <filter>"
    if let Some(after) = s.strip_prefix("This ability costs ")
        && let Some((delta_tok, tail)) = after.split_once(' ')
        && is_mana_sequence(delta_tok)
        && let Some(fe) = tail
            .to_ascii_lowercase()
            .strip_prefix("less to activate for each ")
        && let Ok(flt) = parse_object_filter(fe, vocab)
    {
        return Ok(vec![InstructionAst::AbilityCostReduction {
            delta: delta_tok.to_string(),
            per: Box::new(flt),
        }]);
    }

    // "The next N damage / All damage that would be dealt to <from> [this turn] [by <src>] is dealt to <to> [instead]"
    if let Some(after) = low.strip_prefix("the next ")
        && let Some((amt_raw, rest)) = after.split_once(" damage that would be dealt to ")
        && let Some((from_raw, to_raw)) = rest.split_once(" this turn is dealt to ")
        && let (Ok(amt), Ok(from), Ok(to)) = (
            parse_value_expr(amt_raw, vocab),
            parse_subject_or_target(from_raw, vocab),
            parse_subject_or_target(to_raw.trim_end_matches(" instead"), vocab),
        )
    {
        return Ok(vec![InstructionAst::RedirectDamage {
            amount: Box::new(amt),
            from: Box::new(from),
            to: Box::new(to),
        }]);
    }
    if let Some(rest) = low.strip_prefix("all damage that would be dealt to ")
        && let Some((from_part, to_raw)) = rest.split_once(" is dealt to ")
        && let Ok(to) = parse_subject_or_target(to_raw.trim_end_matches(" instead"), vocab)
    {
        let from_clean = from_part
            .split_once(" this turn")
            .map_or(from_part, |(f, _)| f)
            .split_once(" by ")
            .map_or(from_part, |(f, _)| f)
            .trim();
        let from_clean = from_clean
            .strip_suffix(" this turn")
            .unwrap_or(from_clean)
            .trim();
        if let Ok(from) = parse_subject_or_target(from_clean, vocab) {
            return Ok(vec![InstructionAst::RedirectDamage {
                amount: Box::new(ValueExprAst::PriorResult),
                from: Box::new(from),
                to: Box::new(to),
            }]);
        }
    }
    // "Flip N coins" (Ral Zarek, Krark's Thumb, etc.)
    if let Some(cnt_raw) = low
        .strip_prefix("flip ")
        .and_then(|r| r.strip_suffix(" coins"))
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Ok(vec![InstructionAst::ForEachCount {
            count: Box::new(cnt),
            body: vec![InstructionAst::FlipCoin],
        }]);
    }
    // "<player> skips their next [N/X] turn(s) / draw/untap/combat step" (Fatigue, Brine Elemental)
    if let Some((who_raw, rest)) = low
        .split_once(" skips their next ")
        .or_else(|| low.split_once(" skip their next "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        if rest == "turn" || rest.ends_with(" turns") {
            return Ok(vec![InstructionAst::SkipNextTurn { player }]);
        }
        let step_opt = match rest {
            "draw step" => Some(TurnStepAst::DrawStep),
            "untap step" => Some(TurnStepAst::UntapStep),
            "upkeep" | "upkeep step" => Some(TurnStepAst::Upkeep),
            "combat phase" => Some(TurnStepAst::BeginningOfCombat),
            "end step" => Some(TurnStepAst::EndStep),
            _ => None,
        };
        if let Some(step) = step_opt {
            return Ok(vec![InstructionAst::SkipNextStep { player, step }]);
        }
    }
    // "<player> loses all <kind> counters" (Leeches)
    if let Some((who_raw, rest)) = low
        .split_once(" loses all ")
        .or_else(|| low.split_once(" lose all "))
        && let Some(kind) = rest.strip_suffix(" counters")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::RemoveCounters {
            subject: Box::new(SubjectRefAst::Player(player)),
            counter_kind: kind.trim().to_string(),
            all: true,
            count: None,
        }]);
    }
    // "The next <flt> spell you cast this turn ..." / "A spell cast [by an opponent] this way costs {M} more/less to cast"
    if let Some(after) = low.strip_prefix("the next ")
        && let Some((spell_part, tail)) = after.split_once(" you cast this turn ")
        && let Ok(flt) = parse_object_filter(spell_part, vocab)
    {
        let t_orig = &s[s.len() - tail.len()..];
        if let Some((delta_tok, cost_tail)) = t_orig
            .strip_prefix("costs ")
            .and_then(|r| r.split_once(' '))
            && is_mana_sequence(delta_tok)
            && (cost_tail.eq_ignore_ascii_case("less to cast")
                || cost_tail.eq_ignore_ascii_case("more to cast"))
        {
            return Ok(vec![InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::CostModifier {
                    subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                    delta: delta_tok.to_string(),
                    less: cost_tail.to_ascii_lowercase().starts_with("less"),
                    next_only: true,
                    per: None,
                    scale_expr: None,
                    condition: None,
                }),
                duration: DurationAst::UntilEndOfTurn,
            }]);
        }
        if let Some(kw_raw) = tail.strip_prefix("has ")
            && (kw_raw.starts_with("affinity for ") || parse_keyword_list(kw_raw, vocab).is_some())
        {
            let kws = parse_keyword_list(kw_raw, vocab).unwrap_or_else(|| vec![kw_raw.to_string()]);
            return Ok(vec![InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::GrantKeywords {
                    subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                    keywords: kws,
                    condition: None,
                    paired_rule: None,
                }),
                duration: DurationAst::UntilEndOfTurn,
            }]);
        }
        if tail == "can be cast as though it had flash"
            || tail == "can be cast without paying its mana cost"
            || tail == "can't be countered"
        {
            return Ok(vec![InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::PlayOrCastPermission {
                    cast_only: true,
                    filter: Box::new(flt),
                    permission: tail.to_string(),
                    condition: None,
                }),
                duration: DurationAst::UntilEndOfTurn,
            }]);
        }
    }
    if low.starts_with("a spell cast ")
        && let Some((left_part, cost_part)) = s.split_once(" costs ")
        && left_part.to_ascii_lowercase().ends_with(" this way")
        && let Some((delta_tok, cost_tail)) = cost_part.split_once(' ')
        && is_mana_sequence(delta_tok)
        && (cost_tail.eq_ignore_ascii_case("more to cast")
            || cost_tail.eq_ignore_ascii_case("less to cast"))
    {
        return Ok(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::CostModifier {
                subject: Box::new(SubjectRefAst::Anaphora(left_part.to_string())),
                delta: delta_tok.to_string(),
                less: cost_tail.to_ascii_lowercase().starts_with("less"),
                next_only: false,
                per: None,
                scale_expr: None,
                condition: None,
            }),
            duration: DurationAst::Permanent,
        }]);
    }
    if let Some(after) = low.strip_prefix("copy the next ")
        && let Some(spell_part) = after.strip_suffix(" you cast this turn when you cast it")
        && let Ok(flt) = parse_object_filter(spell_part, vocab)
    {
        return Ok(vec![InstructionAst::DelayedEventTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::SpellCast {
                    caster: PlayerRefAst::You,
                    spell: Some(Box::new(flt)),
                    self_spell: false,
                    targets: None,
                    nth_each_turn: None,
                    include_copy: false,
                }],
                intervening_if: None,
            },
            body: vec![InstructionAst::Copy {
                subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
                exception: None,
                count: None,
            }],
        }]);
    }
    // Note / "<player> chooses a <characteristic/filter>"
    if let Some(note_rest) = low.strip_prefix("note ")
        && !note_rest.is_empty()
    {
        return Ok(vec![InstructionAst::ChooseOption(format!(
            "note:{note_rest}"
        ))]);
    }
    if let Some((who_raw, choose_rest)) = low
        .split_once(" chooses ")
        .or_else(|| low.split_once(" choose "))
        && let Some(chooser) = parse_player_ref(who_raw)
        && !choose_rest.contains(" and ")
        && !choose_rest.contains(", then ")
    {
        if matches!(
            choose_rest,
            "a creature type"
                | "a card type"
                | "a card name"
                | "a creature card name"
                | "a color"
                | "a nonbasic land type"
                | "a basic land type"
                | "a number"
                | "a letter"
        ) || choose_rest.starts_with("a card name other than ")
        {
            return Ok(vec![InstructionAst::ChooseOption(format!(
                "{chooser:?}:{choose_rest}"
            ))]);
        }
        if parse_object_filter(
            choose_rest
                .strip_prefix("any number of ")
                .or_else(|| choose_rest.strip_prefix("up to "))
                .or_else(|| choose_rest.strip_prefix("exactly "))
                .unwrap_or(choose_rest),
            vocab,
        )
        .is_ok()
        {
            return Ok(vec![InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser,
                    description: choose_rest.to_string(),
                },
            )]);
        }
    }
    // "Any number of target players/opponents each <effect>" (Ral Zarek, Hollow Marauder)
    for (pref, player) in [
        (
            "any number of target players each ",
            PlayerRefAst::TargetPlayer,
        ),
        (
            "any number of target opponents each ",
            PlayerRefAst::TargetOpponent,
        ),
        ("any number of target players ", PlayerRefAst::TargetPlayer),
        (
            "any number of target opponents ",
            PlayerRefAst::TargetOpponent,
        ),
    ] {
        if low.starts_with(pref)
            && let Ok(inner) = parse_atomic_effect(&s[pref.len()..], vocab)
        {
            return Ok(vec![InstructionAst::ForEachPlayer {
                player,
                body: inner,
            }]);
        }
    }

    // 2. Modal header: "Choose one U+2014" & standalone "Choose a <characteristic/group>"
    if let Some(after) = low.strip_prefix("choose ") {
        let trimmed = after.trim_end_matches(EM_DASH).trim();
        if matches!(
            trimmed,
            "one"
                | "two"
                | "three"
                | "four"
                | "five"
                | "one or both"
                | "one or more"
                | "x"
                | "one that hasn't been chosen"
                | "one that hasn't been chosen this turn"
                | "one or more that haven't been chosen"
        ) || trimmed.starts_with("up to ")
        {
            return Ok(vec![InstructionAst::Modal {
                choice: trimmed.to_string(),
                modes: Vec::new(),
            }]);
        }
        if matches!(
            trimmed,
            "a creature type"
                | "a card type"
                | "a card name"
                | "a creature card name"
                | "a nonbasic land type"
                | "a basic land type"
                | "a land type"
                | "a number"
                | "a letter"
        ) || trimmed.starts_with("a card name other than ")
            || parse_keyword_list(trimmed, vocab).is_some()
        {
            return Ok(vec![InstructionAst::ChooseOption(trimmed.to_string())]);
        }
        if !trimmed.contains(" and ")
            && !trimmed.contains(", then ")
            && (trimmed.starts_with("exactly ")
                || trimmed.starts_with("any number of ")
                || trimmed.starts_with("two target ")
                || trimmed.starts_with("a card at random "))
        {
            return Ok(vec![InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser: PlayerRefAst::You,
                    description: trimmed.to_string(),
                },
            )]);
        }
        if matches!(
            trimmed,
            "a color"
                | "a creature type"
                | "a creature type other than wall"
                | "a basic land type"
                | "a card name"
                | "a nonland card name"
                | "an opponent"
                | "an opponent at random"
                | "a player"
                | "a player at random"
                | "another player"
                | "any target"
        ) || trimmed.starts_with("target ")
            || trimmed.starts_with("any number of target ")
            || trimmed.ends_with(" from it")
            || trimmed.ends_with(" from them")
        {
            return Ok(vec![InstructionAst::ChooseOption(trimmed.to_string())]);
        }
    }
    if let Some((who_raw, choice_raw)) = low
        .split_once(" chooses ")
        .or_else(|| low.split_once(" choose "))
        && parse_player_ref(who_raw).is_some()
        && parse_object_filter(choice_raw, vocab).is_ok()
    {
        return Ok(vec![InstructionAst::ChooseOption(low)]);
    }
    if let Some(after) = low.strip_prefix("you choose ")
        && (after.ends_with(" from it") || after.ends_with(" from them"))
    {
        return Ok(vec![InstructionAst::ChooseOption(after.to_string())]);
    }

    // Emblem (including trailing sentence after closing quote, e.g. Kiora: `You get an emblem with "...". Then create ...`)
    if let Some(rest) = s.strip_prefix("You get an emblem with \"") {
        if let Some(inner) = rest.strip_suffix('"') {
            return Ok(vec![InstructionAst::CreateEmblem(inner.to_string())]);
        }
        if let Some((inner, after_q)) = rest.split_once('"') {
            let after_t = after_q.trim();
            if !after_t.is_empty() {
                let (mut more, fails) = parse_effect_body(after_t, vocab);
                if fails.is_empty() && !more.is_empty() {
                    let mut out = vec![InstructionAst::CreateEmblem(inner.to_string())];
                    out.append(&mut more);
                    return Ok(out);
                }
            }
        }
    }

    // Quoted granted ability
    if s.contains('"')
        && let Some((left, right_quoted)) = s.split_once('"')
        && let Some((ability_text, trailing)) = right_quoted.rsplit_once('"')
    {
        let llow = left.trim().to_ascii_lowercase();
        let subj_raw = llow
            .strip_suffix(" each have")
            .or_else(|| llow.strip_suffix(" each gain"))
            .or_else(|| llow.strip_suffix(" has"))
            .or_else(|| llow.strip_suffix(" have"))
            .or_else(|| llow.strip_suffix(" gains"))
            .or_else(|| llow.strip_suffix(" gain"));
        if let Some(sr) = subj_raw
            && let Ok(subj) = parse_subject_or_target(sr, vocab)
        {
            let tlow = trailing.to_ascii_lowercase();
            let dur = if tlow.contains("until end of combat") {
                DurationAst::UntilEndOfCombat
            } else if tlow.contains("until end of turn") {
                DurationAst::UntilEndOfTurn
            } else if tlow.contains("until your next turn") {
                DurationAst::UntilYourNextTurn
            } else {
                DurationAst::Permanent
            };
            let compiled = compile_granted_ability(ability_text, vocab);
            return Ok(vec![InstructionAst::GrantAbilityText {
                subject: Box::new(subj),
                ability_text: ability_text.to_string(),
                compiled,
                duration: dur,
            }]);
        }
    }

    // One-shot damage/event replacement: "The next time <event>, <replacement>"
    if let Some(after) = low.strip_prefix("the next time ")
        && let Some((event_desc, rep_desc)) = after.split_once(" this turn, ")
    {
        let rep = rep_desc.trim();
        let instead_body = parse_instead_body(rep, vocab);
        return Ok(vec![InstructionAst::OneShotReplacement {
            subject: event_desc.trim().to_string(),
            event: "next_time_this_turn".to_string(),
            replace_with: rep.to_string(),
            instead_body,
        }]);
    }

    // One-shot replacement effects in spells/abilities
    if low
        == "if that spell is countered this way, exile it instead of putting it into its owner's graveyard"
        || low
            == "if that spell is countered this way, put it on top of its owner's library instead of into that player's graveyard"
    {
        return Ok(vec![InstructionAst::OneShotReplacement {
            subject: "that spell".to_string(),
            event: "countered".to_string(),
            replace_with: if low.contains("exile it") {
                "exile".to_string()
            } else {
                "top_of_library".to_string()
            },
            instead_body: Some(vec![if low.contains("exile it") {
                InstructionAst::Exile {
                    actor: None,
                    subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
                }
            } else {
                InstructionAst::MoveZone {
                    subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
                    to: "top of its owner's library".to_string(),
                }
            }]),
        }]);
    }
    if let Some(after) = low.strip_prefix("if ")
        && let Some((subj_raw, rest)) = after.split_once(" would ")
        && (rest.ends_with(", exile it instead")
            || rest.ends_with(", exile them instead")
            || rest.ends_with(", exile that card instead")
            || rest.ends_with(", exile it instead of putting it anywhere else")
            || rest.ends_with(", exile them instead of putting them anywhere else"))
    {
        let ev = rest
            .split_once(", exile ")
            .map_or(rest, |(before, _)| before)
            .trim();
        return Ok(vec![InstructionAst::OneShotReplacement {
            subject: subj_raw.trim().to_string(),
            event: ev.to_string(),
            replace_with: "exile".to_string(),
            instead_body: Some(vec![InstructionAst::Exile {
                actor: None,
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            }]),
        }]);
    }

    // General turn-scoped / one-shot replacement inside effects:
    // "If <subj> would <event>, <instead_action> instead" (Goblin Goliath, Furnace-style spells, etc.)
    if let Some(after) = low.strip_prefix("if ")
        && let Some((subj_raw, rest)) = after.split_once(" would ")
        && let Some((ev_part, inst_raw)) = rest.split_once(", ")
        && let Some(inst_clean) = inst_raw.strip_suffix(" instead")
    {
        let instead_body = parse_instead_body(inst_clean, vocab);
        if instead_body.is_some() {
            return Ok(vec![InstructionAst::OneShotReplacement {
                subject: subj_raw.trim().to_string(),
                event: ev_part.trim().to_string(),
                replace_with: inst_clean.trim().to_string(),
                instead_body,
            }]);
        }
    }

    // Delayed event trigger inside effect body: "When you next cast ..., <effect>" / "Whenever <event>, <effect>"
    if (low.starts_with("when ") || low.starts_with("whenever "))
        && !low.starts_with("when you do, ")
        && let Some((hdr, body_part)) = s.split_once(", ")
        && let Ok(trigger) = parse_trigger_header(hdr, vocab)
        && let Ok(inner) = parse_atomic_effect(body_part, vocab)
    {
        return Ok(vec![InstructionAst::DelayedEventTrigger {
            trigger,
            body: inner,
        }]);
    }

    // Voting: "[Label U+2014] Starting with you, each player votes for <options>"
    if let Some((_, vote_rest)) = low.split_once("starting with you, each player votes for ") {
        return Ok(vec![InstructionAst::Vote {
            options: vote_rest.trim().to_string(),
        }]);
    }

    // "Roll two dN and choose one result"
    if let Some(after) = low.strip_prefix("roll two d")
        && let Some(sides_str) = after.strip_suffix(" and choose one result")
        && let Ok(sides) = sides_str.trim().parse::<u16>()
    {
        return Ok(vec![InstructionAst::RollDiceChooseOne { count: 2, sides }]);
    }

    // Leading delayed trigger: "At the beginning of the next end step/upkeep, <effect>"
    for (pref, step, scope) in [
        (
            "at the beginning of the next end step, ",
            TurnStepAst::EndStep,
            DelayedStepScope::Next,
        ),
        (
            "at the beginning of your next end step, ",
            TurnStepAst::EndStep,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of that turn's end step, ",
            TurnStepAst::EndStep,
            DelayedStepScope::Next,
        ),
        (
            "at the beginning of that player's next end step, ",
            TurnStepAst::EndStep,
            DelayedStepScope::ThatPlayersNext,
        ),
        (
            "at the beginning of the end step on your next turn, ",
            TurnStepAst::EndStep,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of the end step of that player's next turn, ",
            TurnStepAst::EndStep,
            DelayedStepScope::ThatPlayersNext,
        ),
        (
            "at the beginning of your draw step, ",
            TurnStepAst::DrawStep,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of the next upkeep, ",
            TurnStepAst::Upkeep,
            DelayedStepScope::Next,
        ),
        (
            "at the beginning of your next upkeep, ",
            TurnStepAst::Upkeep,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of the first upkeep, ",
            TurnStepAst::Upkeep,
            DelayedStepScope::Next,
        ),
        (
            "at the beginning of your first upkeep, ",
            TurnStepAst::Upkeep,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of that combat, ",
            TurnStepAst::BeginningOfCombat,
            DelayedStepScope::Next,
        ),
        (
            "at the beginning of the next combat, ",
            TurnStepAst::BeginningOfCombat,
            DelayedStepScope::Next,
        ),
        (
            "at the beginning of the next combat phase this turn, ",
            TurnStepAst::BeginningOfCombat,
            DelayedStepScope::Next,
        ),
        (
            "at the beginning of your next main phase, ",
            TurnStepAst::MainPhase,
            DelayedStepScope::YourNext,
        ),
        (
            "at end of combat, ",
            TurnStepAst::EndOfCombat,
            DelayedStepScope::Next,
        ),
        (
            "at this turn's next end of combat, ",
            TurnStepAst::EndOfCombat,
            DelayedStepScope::Next,
        ),
        (
            "at the beginning of your next first main phase, ",
            TurnStepAst::PrecombatMain,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of your first main phase of the game, ",
            TurnStepAst::PrecombatMain,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of your precombat main phase, ",
            TurnStepAst::PrecombatMain,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of each of your main phases, ",
            TurnStepAst::MainPhase,
            DelayedStepScope::EachOfYour,
        ),
        (
            "at the beginning of your upkeep, ",
            TurnStepAst::Upkeep,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of each player's upkeep, ",
            TurnStepAst::Upkeep,
            DelayedStepScope::EachPlayers,
        ),
        (
            "at the beginning of each opponent's upkeep, ",
            TurnStepAst::Upkeep,
            DelayedStepScope::EachOpponents,
        ),
        (
            "at the beginning of your end step, ",
            TurnStepAst::EndStep,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of each end step, ",
            TurnStepAst::EndStep,
            DelayedStepScope::Each,
        ),
        (
            "at the beginning of combat on your turn, ",
            TurnStepAst::BeginningOfCombat,
            DelayedStepScope::YourNext,
        ),
        (
            "at the beginning of combat on each opponent's turn, ",
            TurnStepAst::BeginningOfCombat,
            DelayedStepScope::EachOpponents,
        ),
    ] {
        if low.starts_with(pref) {
            let inner = parse_atomic_effect(&s[pref.len()..], vocab)?;
            return Ok(vec![InstructionAst::DelayedTrigger {
                step,
                scope,
                body: inner,
            }]);
        }
    }

    // Trailing delayed timing suffix: "<effect> at the beginning of the next end step / upkeep / cleanup step / at end of combat"
    if !low.starts_with("if ")
        && !low.starts_with("when ")
        && !low.starts_with("whenever ")
        && !s.contains('"')
    {
        for (suf, step, scope) in [
            (
                " at the beginning of the next end step",
                TurnStepAst::EndStep,
                DelayedStepScope::Next,
            ),
            (
                " at the beginning of your next end step",
                TurnStepAst::EndStep,
                DelayedStepScope::YourNext,
            ),
            (
                " at the beginning of that player's next end step",
                TurnStepAst::EndStep,
                DelayedStepScope::ThatPlayersNext,
            ),
            (
                " at the beginning of the next upkeep",
                TurnStepAst::Upkeep,
                DelayedStepScope::Next,
            ),
            (
                " at the beginning of your next upkeep",
                TurnStepAst::Upkeep,
                DelayedStepScope::YourNext,
            ),
            (
                " at the beginning of that player's next upkeep",
                TurnStepAst::Upkeep,
                DelayedStepScope::ThatPlayersNext,
            ),
            (
                " at the beginning of the next cleanup step",
                TurnStepAst::CleanupStep,
                DelayedStepScope::Next,
            ),
            (
                " at the beginning of your next main phase",
                TurnStepAst::MainPhase,
                DelayedStepScope::YourNext,
            ),
            (
                " at the beginning of the next combat",
                TurnStepAst::BeginningOfCombat,
                DelayedStepScope::Next,
            ),
            (
                " at end of combat",
                TurnStepAst::EndOfCombat,
                DelayedStepScope::Next,
            ),
        ] {
            if low.ends_with(suf)
                && let Ok(inner) = parse_atomic_effect(s[..s.len() - suf.len()].trim(), vocab)
            {
                return Ok(vec![InstructionAst::DelayedTrigger {
                    step,
                    scope,
                    body: inner,
                }]);
            }
        }
        if let Some(before_ltb) = low.strip_suffix(" when ~ leaves the battlefield")
            && let Ok(inner) = parse_atomic_effect(&s[..before_ltb.len()], vocab)
        {
            return Ok(vec![InstructionAst::DelayedEventTrigger {
                trigger: TriggerSpecAst {
                    events: vec![TriggerEventAst::ZoneChange {
                        from: Some("battlefield".to_string()),
                        to: None,
                        subject: Box::new(SubjectRefAst::SelfObject),
                        grouped: false,
                        entering_controller: None,
                    }],
                    intervening_if: None,
                },
                body: inner,
            }]);
        }
    }
    // "it becomes equal to <val>" (Resolute Archangel) & "you may activate [the] loyalty abilities of <subj> twice this turn rather than only once" (Kaito, Urza Assembles the Titans)
    if let Some(val_raw) = low.strip_prefix("it becomes equal to ")
        && let Ok(value) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![InstructionAst::SetLifeTotal {
            player: PlayerRefAst::You,
            value: Box::new(value),
        }]);
    }
    if low.starts_with("you may activate ")
        && low.contains("loyalty abilities of ")
        && low.ends_with(" twice this turn rather than only once")
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::SelfObject),
            rule: "activate_loyalty_twice".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if let Some(after_fe) = low.strip_prefix("for each opponent with ")
        && let Some((cond_tail, body_raw)) = after_fe.split_once(", ")
        && (cond_tail.contains(" counter") || cond_tail.contains(" life"))
        && let Ok(body) = parse_atomic_effect(&s[s.len() - body_raw.len()..], vocab)
    {
        return Ok(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachOpponent,
            body,
        }]);
    }
    // "<player> loses all counters" (Final Act, Suncleanser, etc.)
    if let Some(who_raw) = low
        .strip_suffix(" loses all counters")
        .or_else(|| low.strip_suffix(" lose all counters"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::RemoveCounters {
            all: true,
            count: None,
            counter_kind: "any".to_string(),
            subject: Box::new(SubjectRefAst::Player(player)),
        }]);
    }
    // "you gain life and draw cards equal to <val>" (Lifeblood Hydra)
    if let Some(val_raw) = low.strip_prefix("you gain life and draw cards equal to ")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![
            InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                delta: Box::new(val.clone()),
                gain: true,
            },
            InstructionAst::Draw {
                player: PlayerRefAst::You,
                count: Box::new(val),
            },
        ]);
    }
    // "put <kind> counters on <subj> equal to <val>" (Experiment Twelve)
    if let Some(after) = low.strip_prefix("put ")
        && let Some((kind_part, rhs)) = after.split_once(" counters on ")
        && let Some((subj_raw, val_raw)) = rhs.split_once(" equal to ")
        && let (Ok(subj), Ok(val)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_value_expr(val_raw, vocab),
        )
    {
        return Ok(vec![InstructionAst::PutCounters {
            count: Box::new(val),
            counter_kind: kind_part.trim().to_string(),
            subject: Box::new(subj),
        }]);
    }
    // "put a <k1> counter on <s1> and a <k2> counter on <s2>" (Sumala Sentry, Captain America, Glorious Purpose, Juniper Order Ranger)
    if let Some(after) = low.strip_prefix("put ")
        && let Some((l_part, r_part)) = after.split_once(" and ")
        && l_part.contains(" counter on ")
        && r_part.contains(" counter on ")
        && let (Ok(mut i1), Ok(i2)) = (
            parse_atomic_effect(&format!("put {l_part}"), vocab),
            parse_atomic_effect(&format!("put {r_part}"), vocab),
        )
    {
        i1.extend(i2);
        return Ok(i1);
    }
    // "create <tok1> and <tok2>" / "create <tok1> or <tok2>" (Madame Vastra, Farmer Cotton, Tireless Provisioner, Ant-Man's Army)
    if let Some(after) = low.strip_prefix("create ") {
        if let Some((l_tok, r_tok)) = after.split_once(" token and ")
            && r_tok.ends_with(" token")
            && let (Ok(mut i1), Ok(i2)) = (
                parse_atomic_effect(&format!("create {l_tok} token"), vocab),
                parse_atomic_effect(&format!("create {r_tok}"), vocab),
            )
        {
            i1.extend(i2);
            return Ok(i1);
        }
        if let Some((l_tok, r_tok)) = after.split_once(" tokens and ")
            && (r_tok.ends_with(" tokens") || r_tok.ends_with(" token"))
            && let (Ok(mut i1), Ok(i2)) = (
                parse_atomic_effect(&format!("create {l_tok} tokens"), vocab),
                parse_atomic_effect(&format!("create {r_tok}"), vocab),
            )
        {
            i1.extend(i2);
            return Ok(i1);
        }
        if let Some((l_tok, r_tok)) = after.split_once(" token or ")
            && r_tok.ends_with(" token")
            && let (Ok(i1), Ok(i2)) = (
                parse_atomic_effect(&format!("create {l_tok} token"), vocab),
                parse_atomic_effect(&format!("create {r_tok}"), vocab),
            )
        {
            return Ok(vec![InstructionAst::Choice(vec![i1, i2])]);
        }
    }
    // "mill a card for each <fe>" (Cephalid Vandal)
    if let Some(fe) = low.strip_prefix("mill a card for each ")
        && let Ok(cnt) = parse_for_each_value_expr(fe, vocab)
    {
        return Ok(vec![InstructionAst::Mill {
            player: PlayerRefAst::You,
            count: Box::new(cnt),
        }]);
    }
    // "<player> gets an additional <kind> counter" (Necrogen Rotpriest)
    if let Some((who_raw, rest)) = low.split_once(" gets an additional ")
        && let Some(kind) = rest.strip_suffix(" counter")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::GainPlayerCounters {
            player,
            count: Box::new(ValueExprAst::Const(1)),
            counter_kind: kind.trim().to_string(),
        }]);
    }
    // "add one mana of that color for each <fe>" / "add one mana of any type <src> produced"
    if let Some(fe) = low.strip_prefix("add one mana of that color for each ")
        && let Ok(cnt) = parse_for_each_value_expr(fe, vocab)
    {
        return Ok(vec![InstructionAst::AddMana {
            player: PlayerRefAst::You,
            mana: crate::ast::ManaSpecAst::AnyColor {
                amount: Box::new(cnt),
                single_color: true,
            },
        }]);
    }
    if low.starts_with("add one mana of any type ")
        && (low.ends_with(" could produce") || low.ends_with(" produced"))
    {
        return Ok(vec![InstructionAst::AddMana {
            player: PlayerRefAst::You,
            mana: crate::ast::ManaSpecAst::AnyColor {
                amount: Box::new(ValueExprAst::Const(1)),
                single_color: true,
            },
        }]);
    }

    // Leading "For as long as <cond>, <effect/static/trigger>" / "As long as <cond>, <effect/static/trigger>"
    // (Giant Oyster, Exchange of Words, Immortal Obligation, Ultima, Myra, Temporal Aperture)
    if let Some(after_ala) = low
        .strip_prefix("for as long as ")
        .map(|_| &s["for as long as ".len()..])
        .or_else(|| {
            low.strip_prefix("as long as ")
                .map(|_| &s["as long as ".len()..])
        })
    {
        for (cidx, _) in after_ala.match_indices(", ") {
            let cond_raw = after_ala[..cidx].trim();
            let rest = after_ala[cidx + 2..].trim();
            if cond_raw.is_empty() || rest.is_empty() {
                continue;
            }
            if let Ok(cond) = parse_condition(cond_raw, vocab) {
                if let Ok(inner) = parse_atomic_effect(rest, vocab) {
                    return Ok(vec![InstructionAst::IfCondition {
                        condition: Box::new(cond),
                        then_body: inner,
                    }]);
                }
                if let Ok(sr) = parse_static_or_replacement(rest, vocab) {
                    return Ok(vec![match sr {
                        StaticOrReplacement::Static(st) => InstructionAst::TemporaryStatic {
                            static_ability: Box::new(st),
                            duration: DurationAst::ForAsLongAs(Box::new(cond)),
                        },
                        StaticOrReplacement::Replacement(rep) => {
                            InstructionAst::TemporaryReplacement {
                                replacement: Box::new(rep),
                                duration: DurationAst::ForAsLongAs(Box::new(cond)),
                            }
                        }
                    }]);
                }
                if let Some(trig_ins) =
                    try_parse_inline_trigger(rest, DurationAst::ForAsLongAs(Box::new(cond)), vocab)
                {
                    return Ok(vec![trig_ins]);
                }
            }
        }
    }

    // Leading duration wrapper
    for (pref, dur) in [
        ("until end of turn, ", DurationAst::UntilEndOfTurn),
        (
            "until the end of your next turn, ",
            DurationAst::UntilEndOfNextTurn,
        ),
        (
            "until end of combat on your next turn, ",
            DurationAst::UntilEndOfNextTurn,
        ),
        (
            "until the beginning of your next upkeep, ",
            DurationAst::UntilYourNextTurn,
        ),
        ("until your next end step, ", DurationAst::UntilYourNextTurn),
        ("until your next upkeep, ", DurationAst::UntilYourNextTurn),
        ("until your next turn, ", DurationAst::UntilYourNextTurn),
        ("during your next turn, ", DurationAst::UntilEndOfNextTurn),
        (
            "during any turn you attacked with ~, ",
            DurationAst::UntilEndOfTurn,
        ),
        (
            "during any turn you attacked with a token, ",
            DurationAst::UntilEndOfTurn,
        ),
        ("this turn, ", DurationAst::UntilEndOfTurn),
    ] {
        if low.starts_with(pref) {
            let rest = &s[pref.len()..];
            if let Ok(mut inner) = parse_atomic_effect(rest, vocab) {
                for ins in &mut inner {
                    match ins {
                        InstructionAst::GrantCastPermission { duration, .. } => {
                            *duration = format!("{dur:?}");
                        }
                        InstructionAst::TemporaryTrigger { duration, .. }
                        | InstructionAst::TemporaryReplacement { duration, .. }
                        | InstructionAst::TemporaryStatic { duration, .. }
                        | InstructionAst::GrantAbilityText { duration, .. }
                        | InstructionAst::GrantKeywords { duration, .. }
                        | InstructionAst::RemoveKeywords { duration, .. }
                        | InstructionAst::RemoveAllAbilities { duration, .. }
                        | InstructionAst::ModifyPT { duration, .. }
                        | InstructionAst::AnimateOrSetBasePT { duration, .. }
                        | InstructionAst::CombatRuleMod { duration, .. } => {
                            *duration = dur.clone();
                        }
                        _ => {}
                    }
                }
                return Ok(inner);
            }
            if let Ok(sr) = parse_static_or_replacement(rest, vocab) {
                return Ok(vec![match sr {
                    StaticOrReplacement::Static(st) => InstructionAst::TemporaryStatic {
                        static_ability: Box::new(st),
                        duration: dur,
                    },
                    StaticOrReplacement::Replacement(rep) => InstructionAst::TemporaryReplacement {
                        replacement: Box::new(rep),
                        duration: dur,
                    },
                }]);
            }
            if let Some(trig_ins) = try_parse_inline_trigger(rest, dur, vocab) {
                return Ok(vec![trig_ins]);
            }
        }
    }

    // Standalone CR 611/613 "The same is true for <keywords/types>" sentence (Odric, Majestic Myriarch, Bleeding Effect, Concerted Effort)
    if let Some(same_tail) = low.strip_prefix("the same is true for ")
        && !same_tail.is_empty()
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::SelfObject),
            rule: format!("same_is_true_for:{same_tail}"),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // CR 603.7 / CR 106.12 Mana-spend delayed trigger:
    // "When/If you spend this mana to cast <flt> [or activate an ability], <effect>" (Opal Palace, Gilanra, Orbs of Dragonkind, Study Hall, Sunken Palace)
    if let Some(after_spend) = low
        .strip_prefix("when you spend this mana to cast ")
        .or_else(|| low.strip_prefix("if you spend this mana to cast "))
        .or_else(|| low.strip_prefix("if that mana is spent to cast "))
        .or_else(|| low.strip_prefix("when that mana is spent to cast "))
        && let Some((cast_part, body_part)) = after_spend.split_once(", ")
        && let Ok(body) = parse_atomic_effect(&s[s.len() - body_part.len()..], vocab)
    {
        let flt = if cast_part == "your commander" || cast_part == "a spell or activate an ability"
        {
            crate::ast::ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }
        } else {
            parse_object_filter(cast_part, vocab).unwrap_or_default()
        };
        if flt.any_card || (!cast_part.is_empty() && parse_object_filter(cast_part, vocab).is_ok())
        {
            return Ok(vec![InstructionAst::TemporaryTrigger {
                trigger: TriggerSpecAst {
                    events: vec![TriggerEventAst::SpellCast {
                        caster: PlayerRefAst::You,
                        spell: Some(Box::new(flt)),
                        self_spell: false,
                        targets: None,
                        nth_each_turn: None,
                        include_copy: false,
                    }],
                    intervening_if: None,
                },
                body,
                duration: DurationAst::UntilEndOfTurn,
            }]);
        }
    }

    // Energy gain: "you get an amount of {e} equal to <val>" (Peema Aether-Seer)
    if let Some(val_raw) = low.strip_prefix("you get an amount of {e} equal to ")
        && let Ok(count) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![InstructionAst::GainPlayerCounters {
            player: PlayerRefAst::You,
            counter_kind: "energy".to_string(),
            count: Box::new(count),
        }]);
    }

    // Leading "have <effect>" wrapper (after stripping "you may ", e.g. Axis of Mortality, Loxodon Lifechanter, Sita Varma, Tanazir Quandrix, Visions, Change of Plans, Cloak of Confusion, Gurzigost, Rumbling Aftershocks)
    if let Some(after_have) = low.strip_prefix("have ") {
        let orig_after_have = &s["have ".len()..];
        if let Some(val_raw) = after_have.strip_prefix("your life total become ")
            && let Ok(val) = parse_value_expr(val_raw, vocab)
        {
            return Ok(vec![InstructionAst::SetLifeTotal {
                player: PlayerRefAst::You,
                value: Box::new(val),
            }]);
        }
        if let Some(rest_pt) = after_have.strip_prefix("the base power and toughness of ")
            && let Some((subj_raw, rhs_pt)) = rest_pt.split_once(" become ")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let (rhs_clean, pt_dur) = strip_duration(rhs_pt, vocab);
            let rhs_core = rhs_clean
                .split_once(", where x is ")
                .map_or(rhs_clean, |(a, _)| a)
                .trim();
            if rhs_core.starts_with("equal to ") || parse_base_pt(rhs_core).is_some() {
                let (p, t) =
                    parse_base_pt(rhs_core).unwrap_or_else(|| ("X".to_string(), "X".to_string()));
                return Ok(vec![InstructionAst::AnimateOrSetBasePT {
                    subject: Box::new(subj),
                    power: p,
                    toughness: t,
                    description: Some(rhs_clean.to_string()),
                    keywords: Vec::new(),
                    duration: pt_dur.unwrap_or(DurationAst::Permanent),
                }]);
            }
        }
        if let Some((subj_raw, rhs_pt)) = after_have.split_once("'s base power become ")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let (rhs_clean, pt_dur) = strip_duration(rhs_pt, vocab);
            return Ok(vec![InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(subj),
                power: "X".to_string(),
                toughness: "keep".to_string(),
                description: Some(rhs_clean.to_string()),
                keywords: Vec::new(),
                duration: pt_dur.unwrap_or(DurationAst::Permanent),
            }]);
        }
        if let Some((subj_raw, rule_tail)) = after_have.split_once(" assign ")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            && (rule_tail.starts_with("no combat damage")
                || rule_tail.contains("combat damage")
                    && rule_tail.contains("as though it weren't blocked"))
        {
            return Ok(vec![InstructionAst::CombatRuleMod {
                subject: Box::new(subj),
                rule: rule_tail.to_string(),
                duration: DurationAst::UntilEndOfTurn,
            }]);
        }
        if let Ok(inner) = parse_atomic_effect(orig_after_have, vocab) {
            return Ok(inner);
        }
    }

    // CR 701.37 Meld (Hanweir Battlements, Mishra, Titania, Urza, Bruna, Gisela, Graf Rats)
    if (low.starts_with("if you both own and control ")
        || low.contains("and you both own and control them"))
        && low.contains("exile them, then meld them into ")
    {
        return Ok(vec![InstructionAst::Transform {
            subject: Box::new(SubjectRefAst::SelfObject),
        }]);
    }
    if low.starts_with("sanctum of nature, exile them, then meld them into ") || low == "convert ~"
    {
        return Ok(vec![InstructionAst::Transform {
            subject: Box::new(SubjectRefAst::SelfObject),
        }]);
    }
    if let Some(n_raw) = low
        .strip_prefix("you earthbend ")
        .or_else(|| low.strip_prefix("earthbend "))
        && let Ok(cnt) = parse_value_expr(n_raw, vocab)
    {
        return Ok(vec![InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(SubjectRefAst::SelfObject),
            power: "0".to_string(),
            toughness: "0".to_string(),
            description: Some(format!("earthbend:{cnt:?}")),
            keywords: Vec::new(),
            duration: DurationAst::Permanent,
        }]);
    }
    if let Some(val_raw) = low.strip_prefix("proliferate a number of times equal to ")
        && let Ok(cnt) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![InstructionAst::ForEachCount {
            count: Box::new(cnt),
            body: vec![InstructionAst::Proliferate],
        }]);
    }
    if let Some((who_raw, rest)) = low
        .split_once(" get a number of ")
        .or_else(|| low.split_once(" gets a number of "))
        && let Some(player) = parse_player_ref(who_raw)
        && let Some((kind, val_raw)) = rest.split_once(" counters equal to ")
        && let Ok(cnt) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![InstructionAst::GainPlayerCounters {
            player,
            counter_kind: kind.trim().to_string(),
            count: Box::new(cnt),
        }]);
    }
    if let Some(val_raw) = low
        .strip_prefix("you get ")
        .and_then(|r| r.strip_suffix(" additional upkeep steps after this phase"))
        && let Ok(cnt) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![InstructionAst::ForEachCount {
            count: Box::new(cnt),
            body: vec![InstructionAst::AdditionalCombat("upkeep".to_string())],
        }]);
    }
    if low
        == "the next spell you cast this turn of the chosen type can be cast without paying its mana cost"
        || low
            == "spells with the chosen name can't be cast and lands with the chosen name can't be played"
        || low
            == "each creature attacks each combat if able and attacks a player other than you if able"
        || low == "they gain protection from everything and their life total can't change"
        || low
            == "you can't lose life this turn, you can't lose the game this turn, and your opponents can't win the game this turn"
        || low
            == "creatures you control with toughness greater than their power assign combat damage equal to their toughness rather than their power"
        || low == "reveal your hand and discard a card of target opponent's choice"
        || low
            == "each player chooses an artifact, a creature, an enchantment, and a planeswalker from among the nonland permanents they control, then sacrifices the rest"
        || low
            == "each opponent chooses an artifact, a creature, an enchantment, and a planeswalker from among the nonland permanents they control, then sacrifices the rest"
        || (low.starts_with("spells you control can't be countered by ")
            && low.contains(" and creatures you control can't be the targets of "))
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::SelfObject),
            rule: low,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if let Some(subj_raw) = low.strip_prefix("count the colors of ")
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return Ok(vec![InstructionAst::ChooseOption(format!(
            "count_colors:{subj_raw}"
        ))]);
    }
    if let Some(after_ctrl) = low.strip_prefix("the controller of ")
        && let Some(subj_raw) = after_ctrl.strip_suffix(" sacrifices it")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![InstructionAst::Sacrifice {
            player: PlayerRefAst::ThatObjectController,
            count: None,
            subject: Box::new(subj),
        }]);
    }
    if let Some((subj_raw, cnt_rest)) = low.split_once(" enters with an additional ")
        && let Some(kind_raw) = cnt_rest
            .strip_suffix(" counter on it")
            .or_else(|| cnt_rest.strip_suffix(" counters on it"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![InstructionAst::PutCounters {
            subject: Box::new(subj),
            counter_kind: kind_raw.trim().to_string(),
            count: Box::new(ValueExprAst::Const(1)),
        }]);
    }
    if low == "reveal that many cards from the top of your library" {
        return Ok(vec![InstructionAst::LookTopLibrary {
            reveal: true,
            library: "your".to_string(),
            count: Box::new(ValueExprAst::PriorResult),
        }]);
    }
    if low == "give that permanent or player another counter of that kind"
        || low == "put an additional counter of that kind on that permanent"
    {
        return Ok(vec![InstructionAst::PutCounters {
            subject: Box::new(SubjectRefAst::Anaphora("that permanent".to_string())),
            counter_kind: "that_kind".to_string(),
            count: Box::new(ValueExprAst::Const(1)),
        }]);
    }
    if low.starts_with("this ability can't cause the total number of ")
        && low.contains(" counters on ~ to be greater than ")
    {
        return Ok(vec![InstructionAst::AbilityRestriction {
            restriction: low,
            condition: None,
        }]);
    }
    if low == "shuffle your hand and graveyard into your library" {
        return Ok(vec![InstructionAst::ShuffleGraveyardIntoLibrary {
            player: PlayerRefAst::You,
        }]);
    }
    if let Some(after_ch) = low.strip_prefix("change a target of ")
        && let Some((spell_raw, new_t)) = after_ch.split_once(" to ")
        && let Ok(subj) = parse_subject_or_target(spell_raw, vocab)
        && parse_subject_or_target(new_t, vocab).is_ok()
    {
        return Ok(vec![InstructionAst::ChangeTargets {
            target: Box::new(subj),
        }]);
    }
    if low == "the new target must be a player"
        || low.starts_with("reselect which player or ")
        || low == "cast it from your graveyard as an adventure until the end of your next turn"
        || low.starts_with("the escape cost is equal to its mana cost plus exile ")
        || low
            == "until end of turn, ~ loses \"prevent all damage that would be dealt to ~.\" any player may activate this ability"
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::SelfObject),
            rule: low,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if low == "exile it instead of putting it into a graveyard as it resolves" {
        return Ok(vec![InstructionAst::OneShotReplacement {
            subject: "it".to_string(),
            event: "resolve_to_graveyard".to_string(),
            replace_with: "exile".to_string(),
            instead_body: Some(vec![InstructionAst::Exile {
                actor: None,
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            }]),
        }]);
    }
    if let Some((who_raw, rest)) = low.split_once(" chooses ")
        && let Some(_p) = parse_player_ref(who_raw)
        && rest.ends_with(" of those cards")
    {
        return Ok(vec![InstructionAst::ChooseOption(format!(
            "{who_raw}:{rest}"
        ))]);
    }
    if low == "they put one on the bottom of your library" {
        return Ok(vec![InstructionAst::MoveZone {
            subject: Box::new(SubjectRefAst::Anaphora("one".to_string())),
            to: "the bottom of your library".to_string(),
        }]);
    }
    if let Some(after_t) = low.strip_prefix("tap ")
        && let Some((cnt_tok, flt_raw)) = after_t.split_once(' ')
        && parse_number_word(cnt_tok).is_some()
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![InstructionAst::TapPermanent {
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
        }]);
    }
    if let Some((subj_raw, rest)) = low.split_once(" attacks ")
        && let Some(opp_raw) = rest.strip_suffix(" this turn if able")
        && let (Ok(subj), Ok(_opp)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(opp_raw, vocab),
        )
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: format!("must_attack:{opp_raw}"),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // CR 603.7 Delayed / duration-scoped triggered ability created during resolution:
    // "When <trigger> [this turn], [if <cond>,] <effect>" / "Whenever <trigger>, <effect>"
    if let Some(after_w) = low
        .strip_prefix("when ")
        .or_else(|| low.strip_prefix("whenever "))
        && !low.starts_with("when you do, ")
        && !low.starts_with("when they do, ")
        && !low.starts_with("when you pay this cost one or more times, ")
        && let Some((hdr_low, body_low)) = after_w.split_once(", ")
        && (hdr_low.starts_with("you cast your next ")
            || hdr_low.starts_with("that mana is spent to cast ")
            || hdr_low.starts_with("you lose control of ")
            || hdr_low == "it's put into a graveyard this turn"
            || hdr_low == "~ becomes untapped or leaves the battlefield"
            || hdr_low.starts_with("this ability resolves for the ")
            || hdr_low == "you cast it this turn"
            || hdr_low.starts_with("you next activate ")
            || hdr_low.starts_with("one or more ") && hdr_low.ends_with(" this way")
            || hdr_low.starts_with("any of those creatures deals combat damage")
            || hdr_low.starts_with("either of those creatures deals combat damage")
            || hdr_low == "that creature deals combat damage this turn"
            || hdr_low == "a creature attacks this turn"
            || hdr_low == "a player attacks you with one or more creatures"
            || hdr_low.starts_with("you reveal a card with mana value ")
            || hdr_low.starts_with("damage from a ")
                && hdr_low.ends_with(" is prevented this way this turn"))
        && let Ok(body) = parse_atomic_effect(&s[s.len() - body_low.len()..], vocab)
    {
        return Ok(vec![InstructionAst::TemporaryTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::KeywordActionPerformed {
                    player: PlayerRefAst::You,
                    action: hdr_low.to_string(),
                }],
                intervening_if: None,
            },
            body,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // CR 603.7 Delayed / duration-scoped triggered ability created during resolution:
    // "When <trigger> [this turn], [if <cond>,] <effect>" / "Whenever <trigger>, <effect>"
    if (low.starts_with("when ") || low.starts_with("whenever "))
        && !low.starts_with("when you do, ")
        && !low.starts_with("when you pay this cost one or more times, ")
    {
        for (idx, _) in s.match_indices(", ") {
            let hdr_part = &s[..idx];
            let body_part = &s[idx + 2..];
            if let Ok(trigger) = parse_trigger_header(hdr_part, vocab)
                && let Ok(body) = parse_atomic_effect(body_part, vocab)
            {
                return Ok(vec![InstructionAst::TemporaryTrigger {
                    trigger,
                    body,
                    duration: DurationAst::UntilEndOfTurn,
                }]);
            }
        }
    }

    // CR 614 One-shot / duration-scoped replacement effect created during resolution:
    // "If <ev> would <would>, <instead>" / "each <flt> that would ... instead"
    if (low.starts_with("if ") && low.contains(" would ")
        || low.contains(" that would ") && low.ends_with(" instead"))
        && let Ok(StaticOrReplacement::Replacement(rep)) = parse_static_or_replacement(s, vocab)
    {
        return Ok(vec![InstructionAst::TemporaryReplacement {
            replacement: Box::new(rep),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // CR 701.16 Reveal-Until Loop: "[<player>] reveal(s) cards from the top of <whose> library until <who> reveal(s) [<cnt>] <flt>"
    if let Some((who, after)) = split_leading_player_verb(&low, "reveal", "reveals")
        && let Some((top_part, until_part)) = after.split_once(" until ")
        && top_part.starts_with("cards from the top of ")
        && top_part.ends_with(" library")
        && let Some((_, flt_side)) = until_part
            .split_once(" reveals ")
            .or_else(|| until_part.split_once(" reveal "))
    {
        let (cnt, flt_raw) = peel_leading_count(flt_side, vocab);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(vec![InstructionAst::RevealUntil {
                player: who,
                filter: Box::new(flt),
                count: Box::new(cnt),
            }]);
        }
    }

    // Reflexive trigger: "When you do, ..." / "When they do, ..." / "When you pay this cost one or more times, ..." / "When you <act> one or more <flt> this way, ..."
    if low.starts_with("when you do, ") {
        let inner = parse_atomic_effect(&s["when you do, ".len()..], vocab)?;
        return Ok(vec![InstructionAst::ReflexiveTrigger(inner)]);
    }
    if low.starts_with("when they do, ") {
        let inner = parse_atomic_effect(&s["when they do, ".len()..], vocab)?;
        return Ok(vec![InstructionAst::ReflexiveTrigger(inner)]);
    }
    if let Some(after_ao) = low.strip_prefix("activate only if ") {
        let (cond_part, suffix_tag) =
            if let Some(c) = after_ao.strip_suffix(" and only as a sorcery") {
                (c, "ConditionalSorcery")
            } else if let Some(c) = after_ao.strip_suffix(" and only once each turn") {
                (c, "ConditionalOnceEachTurn")
            } else if let Some(c) = after_ao.strip_suffix(" and only during your turn") {
                (c, "ConditionalDuringYourTurn")
            } else {
                (after_ao, "Conditional")
            };
        if let Ok(cond) = parse_condition(cond_part, vocab) {
            return Ok(vec![InstructionAst::AbilityRestriction {
                restriction: suffix_tag.to_string(),
                condition: Some(Box::new(cond)),
            }]);
        }
    }
    if low == "look at it" || low == "look at them" {
        return Ok(vec![InstructionAst::LookTopLibrary {
            reveal: false,
            library: "ref".to_string(),
            count: Box::new(ValueExprAst::Const(1)),
        }]);
    }
    if low == "remove them all" {
        return Ok(vec![InstructionAst::RemoveCounters {
            subject: Box::new(SubjectRefAst::SelfObject),
            counter_kind: "all".to_string(),
            all: true,
            count: None,
        }]);
    }
    if low == "the player casts it without paying its mana cost if able"
        || low == "you may play cards this way without paying their mana costs"
        || low == "pay life equal to that spell's mana value rather than pay its mana cost"
    {
        return Ok(vec![InstructionAst::GrantCastPermission {
            player: if low.starts_with("the player") {
                PlayerRefAst::ThatPlayer
            } else {
                PlayerRefAst::You
            },
            cast_only: !low.contains("play "),
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            free: true,
            duration: "now".to_string(),
        }]);
    }
    if let Some(after_rep) = low.strip_prefix("repeat this process until ")
        && !after_rep.is_empty()
    {
        return Ok(vec![InstructionAst::RepeatProcess {
            condition: parse_condition(after_rep, vocab).ok().map(Box::new),
            max_times: None,
            any_number: true,
        }]);
    }
    for fe_pref in [
        "for each card type, ",
        "for each color, ",
        "for each color pair, ",
        "for each even result, ",
        "for each odd result, ",
        "for each kind of counter on target permanent or player, ",
        "for each kind of counter on target permanent, ",
    ] {
        if low.starts_with(fe_pref)
            && let Ok(body) = parse_atomic_effect(&s[fe_pref.len()..], vocab)
        {
            return Ok(vec![InstructionAst::ForEachCount {
                count: Box::new(ValueExprAst::PriorResult),
                body,
            }]);
        }
    }
    if low == "double the value of x" {
        return Ok(vec![InstructionAst::DoubleStatOrCounters {
            subject: Box::new(SubjectRefAst::SelfObject),
            kind: "value_of_x".to_string(),
            duration: DurationAst::Instant,
        }]);
    }
    if let Some(after_ex) = low.strip_prefix("exile ")
        && let Some(subj_raw) = after_ex.strip_suffix(" and copy it")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            InstructionAst::Exile {
                actor: None,
                subject: Box::new(subj),
            },
            InstructionAst::Copy {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                exception: None,
                count: None,
            },
        ]);
    }
    if low == "exile the top card of your library face down and look at it" {
        return Ok(vec![
            InstructionAst::ExileTopLibrary {
                actor: None,
                whose: "your".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                face_down: true,
            },
            InstructionAst::LookTopLibrary {
                reveal: false,
                library: "exiled".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            },
        ]);
    }
    if low == "exile one face down and put the rest on the bottom of your library in any order" {
        return Ok(vec![InstructionAst::ChooseAndKeep {
            pick: "one".to_string(),
            keep_to: "exile face down".to_string(),
            rest_to: "bottom of your library in any order".to_string(),
        }]);
    }
    if let Some(fe) = low.strip_prefix("take an extra turn after this one for each ")
        && let Ok(cnt) = parse_for_each_value_expr(fe, vocab)
    {
        return Ok(vec![InstructionAst::ForEachCount {
            count: Box::new(cnt),
            body: vec![InstructionAst::ExtraTurn {
                player: PlayerRefAst::You,
            }],
        }]);
    }
    if low == "two target players exchange life totals" {
        return Ok(vec![InstructionAst::Exchange {
            kind: ExchangeKind::LifeTotals,
            left: Box::new(SubjectRefAst::Player(PlayerRefAst::TargetPlayer)),
            right: Some(Box::new(SubjectRefAst::Player(PlayerRefAst::TargetPlayer))),
        }]);
    }
    if let Some((who_raw, _)) = low
        .split_once(" skips their next combat phase")
        .or_else(|| low.split_once(" skip their next combat phase"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::SkipNextStep {
            player,
            step: TurnStepAst::BeginningOfCombat,
        }]);
    }
    if let Some((who_raw, rest)) = low
        .split_once(" chooses ")
        .or_else(|| low.split_once(" choose "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        if let Some((flt_part, act_tail)) = rest
            .split_once(", then sacrifices the rest")
            .or_else(|| rest.split_once(", then sacrifice the rest"))
            .or_else(|| rest.split_once(" and discards the rest"))
            .or_else(|| rest.split_once(" and discard the rest"))
            .or_else(|| rest.split_once(" and sacrifices the rest"))
            .or_else(|| rest.split_once(" and sacrifice the rest"))
            .or_else(|| rest.split_once(" and exiles the rest"))
            .or_else(|| rest.split_once(" and exile the rest"))
        {
            let flt_clean = flt_part
                .trim()
                .strip_prefix("up to ")
                .unwrap_or(flt_part.trim());
            let (_, flt_nocnt) = peel_leading_count(flt_clean, vocab);
            if let Ok(flt) = parse_object_filter(flt_clean, vocab)
                .or_else(|_| parse_object_filter(flt_nocnt, vocab))
            {
                return Ok(vec![InstructionAst::Equalize {
                    player,
                    filter: Box::new(flt),
                    action: if act_tail.is_empty() && rest.contains("discard") {
                        "discard_rest".to_string()
                    } else if act_tail.is_empty() && rest.contains("exile") {
                        "exile_rest".to_string()
                    } else {
                        "sacrifice_rest".to_string()
                    },
                }]);
            }
        }
        // Standalone "<player> chooses [up to] <N/a/an> <flt>" (Urza's Sylex, Fall of the First Civilization, Stronghold Gambit, Summon: Valefor)
        let flt_clean = rest.trim().strip_prefix("up to ").unwrap_or(rest.trim());
        let (_, flt_nocnt) = peel_leading_count(flt_clean, vocab);
        if !rest.contains(", ")
            && !rest.contains(" and ")
            && (parse_object_filter(flt_clean, vocab).is_ok()
                || parse_object_filter(flt_nocnt, vocab).is_ok())
        {
            return Ok(vec![InstructionAst::ChooseOption(format!(
                "{who_raw}:{rest}"
            ))]);
        }
    }
    if low == "its activated abilities can't be activated"
        || (low.starts_with("activated abilities of ")
            && (low.ends_with(" can't be activated")
                || low.ends_with(" can't be activated this turn")))
        || low
            .ends_with(" can't cast spells or activate planeswalkers' loyalty abilities this turn")
        || low.ends_with(" can't cast spells or activate planeswalkers' loyalty abilities")
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::SelfObject),
            rule: low,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if low == "it enters tapped and attacking" {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            rule: "enters_tapped_and_attacking".to_string(),
            duration: DurationAst::UntilEndOfCombat,
        }]);
    }
    if let Some(subj_raw) = low.strip_prefix("lock or unlock a door of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![InstructionAst::UnlockDoor {
            subject: Box::new(subj),
        }]);
    }
    if low == "reveal a card at random from your hand" {
        return Ok(vec![InstructionAst::RevealRandomFromHand {
            player: PlayerRefAst::You,
        }]);
    }
    if low == "if you cast a spell this way, you cast it without paying its mana cost"
        || low
            == "if you cast a spell this way, pay life equal to its mana value rather than pay its mana cost"
    {
        return Ok(vec![InstructionAst::GrantCastPermission {
            player: PlayerRefAst::You,
            cast_only: true,
            subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
            free: low.contains("without paying"),
            duration: "now".to_string(),
        }]);
    }
    if low.starts_with("at end of combat, ") {
        let inner = parse_atomic_effect(&s["at end of combat, ".len()..], vocab)?;
        return Ok(vec![InstructionAst::DelayedTrigger {
            step: TurnStepAst::EndOfCombat,
            scope: DelayedStepScope::Next,
            body: inner,
        }]);
    }
    if low.starts_with("during any turn you ")
        && let Some((_cond, rest)) = s.split_once(", ")
        && let Ok(inner) = parse_atomic_effect(rest, vocab)
    {
        return Ok(inner);
    }
    if low.starts_with("when you pay this cost one or more times, ") {
        let inner = parse_atomic_effect(
            &s["when you pay this cost one or more times, ".len()..],
            vocab,
        )?;
        return Ok(vec![InstructionAst::ReflexiveTrigger(inner)]);
    }
    if low.starts_with("when you ")
        && let Some((cond_part, body_part)) = s.split_once(" this way, ")
        && cond_part.to_ascii_lowercase().contains(" one or more ")
        && let Ok(inner) = parse_atomic_effect(body_part, vocab)
    {
        return Ok(vec![InstructionAst::ReflexiveTrigger(inner)]);
    }

    // Play / Cast permission BEFORE stripping "You may "
    if let Some(perm) = try_parse_cast_permission(&low, vocab) {
        return Ok(vec![perm]);
    }

    // Control-flow wrappers (use `if let Ok` so compound "You may A, and you may B" / "If you do, A, and B" fall through to top-level combinators!)
    if low.starts_with("you may ")
        && let Ok(inner) = parse_atomic_effect(&s[8..], vocab)
    {
        return Ok(vec![InstructionAst::Optional(inner)]);
    }
    if let Some((who_raw, rest)) = s.split_once(" may ")
        && let Some(player) = parse_player_ref(who_raw)
        && let Ok(inner) = parse_atomic_effect(rest, vocab)
    {
        return Ok(vec![InstructionAst::OptionalPlayer {
            player,
            body: inner,
        }]);
    }
    // Causative wrapper after stripping "you may ": "have <subj> deal/fight/draw/gain/lose/sacrifice/discard/become/get/create..."
    if low.starts_with("have ") {
        let rest = &s[5..];
        let rlow = rest.to_ascii_lowercase();
        if let Some(subj_raw) = rlow.strip_suffix(" connive")
            && parse_subject_or_target(subj_raw, vocab).is_ok()
        {
            return parse_atomic_effect(&format!("{subj_raw} connives"), vocab);
        }
        if let Some(who_raw) = rlow.strip_suffix(" shuffle")
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Ok(vec![InstructionAst::ShuffleLibrary { player }]);
        }
        for (verb_needle, inflected) in [
            (" deal ", " deals "),
            (" fight ", " fights "),
            (" draw ", " draws "),
            (" mill ", " mills "),
            (" gain ", " gains "),
            (" lose ", " loses "),
            (" get ", " gets "),
            (" create ", " creates "),
            (" sacrifice ", " sacrifices "),
            (" discard ", " discards "),
            (" become a ", " becomes a "),
            (" become an ", " becomes an "),
            (" become ", " become "),
        ] {
            if let Some(idx) = rlow.find(verb_needle) {
                let rewritten = format!(
                    "{}{}{}",
                    &rest[..idx],
                    inflected,
                    &rest[idx + verb_needle.len()..]
                );
                if let Ok(inner) = parse_atomic_effect(&rewritten, vocab) {
                    return Ok(inner);
                }
            }
        }
    }
    // Strip leading "also " / alternative cost "rather than pay ~'s mana cost"
    if low.starts_with("also ") {
        return parse_atomic_effect(&s[5..], vocab);
    }
    if let Some(cost_raw) = s
        .strip_suffix(" rather than pay ~'s mana cost")
        .or_else(|| s.strip_suffix(" rather than pay ~'s mana cost."))
        && let Ok(cost) = parse_cost_list(cost_raw, vocab)
    {
        return Ok(cost.into_iter().map(InstructionAst::PayCost).collect());
    }

    // Strip adverbial " also " ("it also deals 4 damage to target player")
    if !s.contains('"')
        && low.contains(" also ")
        && let Ok(inner) = parse_atomic_effect(&s.replacen(" also ", " ", 1), vocab)
    {
        return Ok(inner);
    }
    if low.starts_with("if you do, ")
        && let Ok(inner) = parse_atomic_effect(&s[11..], vocab)
    {
        return Ok(vec![InstructionAst::IfPreviousSucceeded(inner)]);
    }
    if low.starts_with("if the player does, ")
        || low.starts_with("if that player does, ")
        || low.starts_with("if a player does, ")
        || low.starts_with("if they do, ")
    {
        let skip = if low.starts_with("if that player does, ") {
            21
        } else if low.starts_with("if the player does, ") {
            20
        } else if low.starts_with("if a player does, ") {
            18
        } else {
            12
        };
        if let Ok(inner) = parse_atomic_effect(&s[skip..], vocab) {
            return Ok(vec![InstructionAst::IfPreviousSucceeded(inner)]);
        }
    }
    for fail_pref in [
        "if you don't, ",
        "if it doesn't, ",
        "if no one does, ",
        "if that player doesn't, ",
        "if they don't, ",
        "if you can't, ",
        "if they can't, ",
    ] {
        if low.starts_with(fail_pref)
            && let Ok(inner) = parse_atomic_effect(&s[fail_pref.len()..], vocab)
        {
            return Ok(vec![InstructionAst::IfPreviousFailed(inner)]);
        }
    }
    // Orthogonal repeat suffix: "<effect> twice" / "<effect> <val> times"
    // (Contagion Engine, Expansion Algorithm, Full Flowering, Progenitor Exarch, Jadelight Spelunker, The Tenth Doctor)
    if !low.starts_with("if ") && !s.contains('"') {
        if let Some(before) = low.strip_suffix(" twice")
            && let Ok(inner) = parse_atomic_effect(&s[..before.len()], vocab)
        {
            return Ok(vec![InstructionAst::ForEachCount {
                count: Box::new(ValueExprAst::Const(2)),
                body: inner,
            }]);
        }
        if let Some(before_times) = low.strip_suffix(" times")
            && let Some((eff_part, cnt_part)) = before_times.rsplit_once(' ')
            && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
            && let Ok(inner) = parse_atomic_effect(&s[..eff_part.len()], vocab)
        {
            return Ok(vec![InstructionAst::ForEachCount {
                count: Box::new(cnt),
                body: inner,
            }]);
        }
    }
    // Modern "<player> shuffles [their library]" (Lantern of Insight, Quiet Speculation, Tempt with Discovery, Boggart Forager, Soldier of Fortune)
    if let Some(who_raw) = low
        .strip_suffix(" shuffles their library")
        .or_else(|| low.strip_suffix(" shuffle their library"))
        .or_else(|| low.strip_suffix(" shuffles his or her library"))
        .or_else(|| low.strip_suffix(" shuffles"))
        .or_else(|| low.strip_suffix(" shuffle"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::ShuffleLibrary { player }]);
    }
    // "behold <flt>" / "open <N> attractions" / "roll [N] <sides>-sided die/dice" / "it's still a <desc> land" / "they're P/T <desc>"
    // Plus Pass S orthogonal effects: double <player>'s life total, <player> manifests dread,
    // tap/untap and goad <subj>, the copy targets <subj>, add <N> mana of the chosen color,
    // pay {M} up to N times / pay any amount of {C} / pay one or more {E}, create a [tapped] <Named> token, choose <N> <flt>
    if let Some(after) = low.strip_prefix("double ")
        && let Some(who_raw) = after.strip_suffix("'s life total")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::ChangeLife {
            player,
            delta: Box::new(ValueExprAst::PlayerStat {
                player,
                stat: crate::ast::PlayerStatKind::LifeTotal,
            }),
            gain: true,
        }]);
    }
    if let Some(who_raw) = low.strip_suffix(" manifests dread")
        && let Some(_player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::Manifest {
            count: Box::new(ValueExprAst::Const(1)),
            dread: true,
        }]);
    }
    if let Some(subj_raw) = low.strip_prefix("untap and goad ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            InstructionAst::UntapPermanent {
                subject: Box::new(subj.clone()),
            },
            InstructionAst::CombatRuleMod {
                subject: Box::new(subj),
                rule: "goad".to_string(),
                duration: DurationAst::UntilYourNextTurn,
            },
        ]);
    }
    if let Some(subj_raw) = low.strip_prefix("tap and goad ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            InstructionAst::TapPermanent {
                subject: Box::new(subj.clone()),
            },
            InstructionAst::CombatRuleMod {
                subject: Box::new(subj),
                rule: "goad".to_string(),
                duration: DurationAst::UntilYourNextTurn,
            },
        ]);
    }
    if let Some(subj_raw) = low.strip_prefix("the copy targets ")
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return Ok(vec![InstructionAst::AbilityRestriction {
            restriction: low,
            condition: None,
        }]);
    }
    if let Some(cnt_raw) = low
        .strip_prefix("add ")
        .and_then(|r| r.strip_suffix(" mana of the chosen color"))
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Ok(vec![InstructionAst::AddMana {
            player: PlayerRefAst::You,
            mana: crate::ast::ManaSpecAst::AnyColor {
                amount: Box::new(cnt),
                single_color: true,
            },
        }]);
    }
    if let Some(after_pay) = s.strip_prefix("pay ").or_else(|| s.strip_prefix("Pay ")) {
        let alow = after_pay.to_ascii_lowercase();
        if alow == "one or more {e}" {
            return Ok(vec![InstructionAst::PayCostAnyNumberOfTimes(
                CostAtomAst::PayEnergy(1),
            )]);
        }
        if let Some(sym) = after_pay.strip_prefix("any amount of ")
            && is_mana_sequence(sym.trim())
        {
            return Ok(vec![InstructionAst::PayCostAnyNumberOfTimes(
                CostAtomAst::PayMana(sym.trim().to_string()),
            )]);
        }
        if let Some((mana_part, times_part)) = after_pay.split_once(" up to ")
            && times_part.to_ascii_lowercase().ends_with(" times")
            && is_mana_sequence(mana_part.trim())
        {
            return Ok(vec![InstructionAst::PayCostAnyNumberOfTimes(
                CostAtomAst::PayMana(mana_part.trim().to_string()),
            )]);
        }
    }
    // Predefined named card-copy tokens ("create a Tarmogoyf token", "create a tapped Mutavault token", "create a Gingerbrute token", "create a Spellgorger Weird token")
    if let Some(after) = low.strip_prefix("create a ")
        && let Some(name_part) = after.strip_suffix(" token")
    {
        let (name_clean, tapped) = match name_part.strip_prefix("tapped ") {
            Some(n) => (n, true),
            None => (name_part, false),
        };
        if matches!(
            name_clean,
            "tarmogoyf" | "gingerbrute" | "mutavault" | "spellgorger weird"
        ) {
            return Ok(vec![InstructionAst::CreateToken {
                controller: PlayerRefAst::You,
                token: crate::ast::TokenSpecAst::Predefined {
                    name: name_clean.to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                    tapped,
                },
            }]);
        }
    }
    // "choose <N> <flt>" (Clarion Ultimatum: "Choose five permanents you control")
    if let Some(after) = low.strip_prefix("choose ") {
        let (_, flt_raw) = peel_leading_count(after, vocab);
        if parse_object_filter(flt_raw, vocab).is_ok() {
            return Ok(vec![InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser: PlayerRefAst::You,
                    description: after.to_string(),
                },
            )]);
        }
    }
    if let Some(flt_raw) = low.strip_prefix("behold ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(vec![InstructionAst::PayCost(CostAtomAst::Behold(
            Box::new(flt),
        ))]);
    }
    if low.starts_with("open ") && low.ends_with(" attractions") {
        return Ok(vec![InstructionAst::AbilityRestriction {
            restriction: low,
            condition: None,
        }]);
    }
    if low.starts_with("roll ") && (low.ends_with("-sided die") || low.ends_with("-sided dice")) {
        return Ok(vec![InstructionAst::AbilityRestriction {
            restriction: low,
            condition: None,
        }]);
    }
    if low.starts_with("it's still a ")
        || low.starts_with("they're ") && low.ends_with(" creatures")
    {
        return Ok(vec![InstructionAst::AbilityRestriction {
            restriction: low,
            condition: None,
        }]);
    }
    // Full multi-part SearchLibrary BEFORE ", then " split so "Each player searches their library..., then shuffles" stays intact!
    if (low.contains("search ") || low.contains("searches "))
        && low.contains(" library")
        && low.contains(" for ")
        && !low.starts_with("if ")
        && !low.starts_with("for each ")
        && let Some(srch) = try_parse_search_library(&low, vocab)
    {
        return Ok(vec![srch]);
    }
    // Leading redundant "you " before imperative actions ("you incubate 2", "you untap up to six lands", "you get six {E}")
    if let Some(after_you) = s.strip_prefix("You ").or_else(|| s.strip_prefix("you ")) {
        let a_low = after_you.to_ascii_lowercase();
        if a_low.starts_with("incubate ")
            || a_low.starts_with("untap ")
            || a_low.starts_with("tap ")
            || a_low.starts_with("behold ")
            || a_low.starts_with("get ") && a_low.ends_with(" {e}")
        {
            if let Some(cnt_tok) = a_low
                .strip_prefix("get ")
                .and_then(|r| r.strip_suffix(" {e}"))
                && let Ok(cnt) = parse_value_expr(cnt_tok, vocab)
            {
                return Ok(vec![InstructionAst::GainPlayerCounters {
                    player: PlayerRefAst::You,
                    counter_kind: "energy".to_string(),
                    count: Box::new(cnt),
                }]);
            }
            if let Ok(inner) = parse_atomic_effect(after_you, vocab) {
                return Ok(inner);
            }
        }
    }
    if low.starts_with("otherwise, ")
        && let Ok(inner) = parse_atomic_effect(&s[11..], vocab)
    {
        return Ok(vec![InstructionAst::Otherwise(inner)]);
    }
    if low.starts_with("instead ") || low.starts_with("instead, ") {
        let skip = if low.starts_with("instead, ") { 9 } else { 8 };
        if let Ok(inner) = parse_atomic_effect(&s[skip..], vocab) {
            return Ok(vec![InstructionAst::ReplaceEffect(inner)]);
        }
    }
    if low.ends_with(" instead")
        && !low.contains(" instead of ")
        && !low.starts_with("if ")
        && let Ok(inner) = parse_atomic_effect(&s[..s.len() - 8], vocab)
    {
        return Ok(vec![InstructionAst::ReplaceEffect(inner)]);
    }
    if low.starts_with("then ")
        && let Ok(inner) = parse_atomic_effect(&s[5..], vocab)
    {
        return Ok(inner);
    }

    // If <condition>, <effect_or_temp_static> (handles comma inside condition AND American-English quote-comma `," `)
    if low.starts_with("if ") {
        let after_if = &s[3..];
        for (idx, _) in after_if.match_indices(", ") {
            let cond_part = &after_if[..idx];
            let body_part = &after_if[idx + 2..];
            if let Ok(cond) = parse_condition(cond_part, vocab)
                && let Some(inner) =
                    parse_effect_or_temp_clause(body_part, &DurationAst::UntilEndOfTurn, vocab)
            {
                return Ok(vec![InstructionAst::IfCondition {
                    condition: Box::new(cond),
                    then_body: inner,
                }]);
            }
        }
        for (idx, _) in after_if.match_indices(",\" ") {
            let cond_part = format!("{}\"", &after_if[..idx]);
            let body_part = &after_if[idx + 3..];
            if let Ok(cond) = parse_condition(&cond_part, vocab)
                && let Some(inner) =
                    parse_effect_or_temp_clause(body_part, &DurationAst::UntilEndOfTurn, vocab)
            {
                return Ok(vec![InstructionAst::IfCondition {
                    condition: Box::new(cond),
                    then_body: inner,
                }]);
            }
        }
    }
    // Trailing "<effect> instead if <condition>" / "<effect> if <condition>"
    if !s.contains('"') && !low.starts_with("if ") {
        if let Some(idx) = low.rfind(" instead if ")
            && let Ok(cond) = parse_condition(&s[idx + " instead if ".len()..], vocab)
            && let Ok(inner) = parse_atomic_effect(s[..idx].trim(), vocab)
        {
            return Ok(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: vec![InstructionAst::ReplaceEffect(inner)],
            }]);
        }
        if let Some(idx) = low.rfind(" if ")
            && let Ok(cond) = parse_condition(&s[idx + 4..], vocab)
            && let Ok(inner) = parse_atomic_effect(s[..idx].trim(), vocab)
        {
            return Ok(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: inner,
            }]);
        }
    }

    // For each <player/filter/value-expr>, <effect> (scan all commas to handle comma-lists in filters!)
    if low.starts_with("for each ") {
        let after_fe = &s[9..];
        for (cidx, _) in after_fe.match_indices(", ") {
            let over_raw = after_fe[..cidx].trim();
            let body_part = after_fe[cidx + 2..].trim();
            if over_raw.is_empty() || body_part.is_empty() {
                continue;
            }
            if (over_raw.eq_ignore_ascii_case("opponent who can't")
                || over_raw.eq_ignore_ascii_case("player who can't"))
                && let Ok(inner) = parse_atomic_effect(body_part, vocab)
            {
                return Ok(vec![InstructionAst::ForEachPlayer {
                    player: if over_raw.to_ascii_lowercase().contains("opponent") {
                        PlayerRefAst::EachOpponent
                    } else {
                        PlayerRefAst::EachPlayer
                    },
                    body: inner,
                }]);
            }
            let each_str = format!("each {over_raw}");
            if let Some(p) = parse_player_ref(&each_str)
                && let Ok(inner) = parse_atomic_effect(body_part, vocab)
            {
                return Ok(vec![InstructionAst::ForEachPlayer {
                    player: p,
                    body: inner,
                }]);
            }
            if let Ok(flt) = parse_object_filter(over_raw, vocab)
                && let Ok(inner) = parse_atomic_effect(body_part, vocab)
            {
                return Ok(vec![InstructionAst::ForEachObject {
                    filter: Box::new(flt),
                    body: inner,
                }]);
            }
            if let Ok(cnt) = parse_for_each_value_expr(over_raw, vocab)
                && let Ok(inner) = parse_atomic_effect(body_part, vocab)
            {
                return Ok(vec![InstructionAst::ForEachCount {
                    count: Box::new(cnt),
                    body: inner,
                }]);
            }
        }
    }

    // Leading "Unless <player> pays <cost>, <effect>" (Rhystic Tutor, etc.)
    if low.starts_with("unless ")
        && let Some((unless_raw, body_raw)) = s["unless ".len()..].split_once(", ")
    {
        let ulow = unless_raw.to_ascii_lowercase();
        let split_pay = ulow
            .find(" pays ")
            .map(|i| (i, 6))
            .or_else(|| ulow.find(" pay ").map(|i| (i, 5)));
        if let Some((pidx, plen)) = split_pay
            && let Some(player) = parse_player_ref(&ulow[..pidx])
            && let Ok(cost) = parse_cost_atom(&unless_raw[pidx + plen..], vocab)
            && let Ok(body) = parse_atomic_effect(body_raw, vocab)
        {
            return Ok(vec![InstructionAst::Unless {
                body,
                unless_player: Some(player),
                unless_cost: Some(cost),
                unless_action: None,
                unless_condition: None,
            }]);
        }
    }

    // Leading "For each [of those] opponent(s)/player(s) who <cond>, <effect>" (Hollow Marauder, Tempt with Immortality)
    if let Some(after) = low
        .strip_prefix("for each of those opponents who ")
        .or_else(|| low.strip_prefix("for each opponent who "))
        .or_else(|| low.strip_prefix("for each of those players who "))
        .or_else(|| low.strip_prefix("for each player who "))
        && let Some((cond_part, body_part)) = after.split_once(", ")
        && !cond_part.is_empty()
        && let Ok(inner) = parse_atomic_effect(body_part, vocab)
    {
        let player = if low.contains("opponent") {
            PlayerRefAst::EachOpponent
        } else {
            PlayerRefAst::EachPlayer
        };
        return Ok(vec![InstructionAst::ForEachPlayer {
            player,
            body: inner,
        }]);
    }

    // "Each player/opponent who <rel> <verb_action>" (Strongarm Tactics, No Witnesses, Grim Reminder)
    for (pref, player) in [
        ("each player who ", PlayerRefAst::EachPlayer),
        ("each opponent who ", PlayerRefAst::EachOpponent),
    ] {
        if let Some(after) = low.strip_prefix(pref) {
            for verb in [
                " loses ",
                " gains ",
                " draws ",
                " discards ",
                " sacrifices ",
                " creates ",
                " exiles ",
                " returns ",
                " mills ",
                " investigates",
                " puts ",
                " reveals ",
            ] {
                if let Some(vidx) = after.find(verb) {
                    let action_str = format!("that player{}", &after[vidx..]);
                    if let Ok(inner) = parse_atomic_effect(&action_str, vocab) {
                        return Ok(vec![InstructionAst::ForEachPlayer {
                            player,
                            body: inner,
                        }]);
                    }
                }
            }
        }
    }

    // Leading "For each of them, <effect>" (Soulfire Eruption, etc.)
    if low.starts_with("for each of them, ")
        && let Ok(inner) = parse_atomic_effect(&s["for each of them, ".len()..], vocab)
    {
        return Ok(vec![InstructionAst::ForEachObject {
            filter: Box::new(crate::ast::ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
            body: inner,
        }]);
    }

    // Standalone "X is <ValueExpr>" binder sentence (Soul Foundry, etc.)
    if let Some(val_raw) = low.strip_prefix("x is ")
        && let Ok(x_expr) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![InstructionAst::BindX {
            x_expr: Box::new(x_expr),
            body: Vec::new(),
        }]);
    }

    // "You and <player> each <verb_effect>" (Teyo, Geometric Tactician, etc.)
    if let Some(after) = low.strip_prefix("you and ")
        && let Some((other_raw, act_raw)) = after.split_once(" each ")
        && let Some(other_player) = parse_player_ref(other_raw)
        && let Ok(inner) = parse_atomic_effect(act_raw, vocab)
    {
        return Ok(vec![
            InstructionAst::ForEachPlayer {
                player: PlayerRefAst::You,
                body: inner.clone(),
            },
            InstructionAst::ForEachPlayer {
                player: other_player,
                body: inner,
            },
        ]);
    }

    // Compound ", then " split (outside quotes), supporting 3-step "<subj> <v1>, <v2>, then <v3>"
    // with shared-subject carry-forward BEFORE bare 3rd-person fallback so player refs are never lost.
    if low.contains(", then ") && !s.contains('"') {
        let comma_parts = split_top_level(s, ", ");
        if comma_parts.len() >= 3
            && comma_parts.len() <= 4
            && comma_parts
                .last()
                .is_some_and(|p| p.trim().to_ascii_lowercase().starts_with("then "))
        {
            let lead_subj = extract_leading_subject_prefix(comma_parts[0].trim());
            let mut seq = Vec::new();
            let mut ok = true;
            for (idx_p, p) in comma_parts.iter().enumerate() {
                let pt = p.trim();
                let pt_clean = if idx_p + 1 == comma_parts.len() {
                    pt.strip_prefix("then ")
                        .or_else(|| pt.strip_prefix("Then "))
                        .unwrap_or(pt)
                        .trim()
                } else {
                    pt
                };
                if pt_clean.is_empty() {
                    ok = false;
                    break;
                }
                let parsed = if idx_p > 0
                    && let Some(subj) = lead_subj
                    && starts_with_predicate_verb(pt_clean)
                {
                    parse_atomic_effect(&format!("{subj} {pt_clean}"), vocab)
                        .or_else(|_| parse_atomic_effect(pt_clean, vocab))
                } else {
                    parse_atomic_effect(pt_clean, vocab).or_else(|_| {
                        if idx_p > 0
                            && let Some(subj) = lead_subj
                        {
                            parse_atomic_effect(&format!("{subj} {pt_clean}"), vocab)
                        } else {
                            Err("no_subj".to_string())
                        }
                    })
                };
                match parsed {
                    Ok(sub) if !sub.is_empty() => seq.extend(sub),
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && !seq.is_empty() {
                return Ok(seq);
            }
        }
        let parts = split_top_level(s, ", then ");
        if parts.len() >= 2 {
            let lead_subj = extract_leading_subject_prefix(parts[0].trim());
            let mut seq = Vec::new();
            let mut ok = true;
            for (idx_p, p) in parts.iter().enumerate() {
                let pt = p.trim();
                let parsed = if idx_p > 0
                    && let Some(subj) = lead_subj
                    && starts_with_predicate_verb(pt)
                {
                    parse_atomic_effect(&format!("{subj} {pt}"), vocab)
                        .or_else(|_| parse_atomic_effect(pt, vocab))
                } else {
                    parse_atomic_effect(pt, vocab).or_else(|_| {
                        if idx_p > 0
                            && let Some(subj) = lead_subj
                        {
                            parse_atomic_effect(&format!("{subj} {pt}"), vocab)
                        } else {
                            Err("no_subj".to_string())
                        }
                    })
                };
                match parsed {
                    Ok(sub) if !sub.is_empty() => seq.extend(sub),
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && !seq.is_empty() {
                return Ok(seq);
            }
        }
    }

    // PayCost: "pay {M} [any number of times|to end this effect]" / "pay N life" / "<player> pays {M}"
    if let Some(after) = s.strip_prefix("pay ").or_else(|| s.strip_prefix("Pay ")) {
        let after_trim = after
            .trim()
            .strip_suffix(" to end this effect")
            .unwrap_or(after.trim());
        let (core, any_times) = match after_trim.strip_suffix(" any number of times") {
            Some(c) => (c.trim(), true),
            None => (after_trim, false),
        };
        if is_mana_sequence(core) {
            let ca = CostAtomAst::PayMana(core.to_string());
            return Ok(vec![if any_times {
                InstructionAst::PayCostAnyNumberOfTimes(ca)
            } else {
                InstructionAst::PayCost(ca)
            }]);
        }
        if core.eq_ignore_ascii_case("any amount of {e}") {
            return Ok(vec![InstructionAst::PayCostAnyNumberOfTimes(
                CostAtomAst::PayEnergy(1),
            )]);
        }
        if let Ok(ca) = parse_cost_atom(&format!("pay {core}"), vocab) {
            return Ok(vec![if any_times {
                InstructionAst::PayCostAnyNumberOfTimes(ca)
            } else {
                InstructionAst::PayCost(ca)
            }]);
        }
        // "pay {1} and 1 life" / compound "pay <c1> and <c2>"
        if let Some((c1, c2)) = core.split_once(" and ")
            && let (Ok(a1), Ok(a2)) = (
                parse_cost_atom(c1.trim(), vocab)
                    .or_else(|_| parse_cost_atom(&format!("pay {}", c1.trim()), vocab)),
                parse_cost_atom(c2.trim(), vocab)
                    .or_else(|_| parse_cost_atom(&format!("pay {}", c2.trim()), vocab)),
            )
        {
            return Ok(vec![
                InstructionAst::PayCost(a1),
                InstructionAst::PayCost(a2),
            ]);
        }
    }
    if let Some((who_raw, cost_raw)) = s.split_once(" pays ").or_else(|| s.split_once(" pay "))
        && let (Some(player), Ok(cost)) = (
            parse_player_ref(who_raw),
            parse_cost_atom(cost_raw.trim(), vocab),
        )
    {
        return Ok(vec![InstructionAst::PlayerPayCost { player, cost }]);
    }

    // AddMana
    if low.contains("don't lose this mana") || low.contains("doesn't empty") {
        return Ok(vec![InstructionAst::RetainManaUntilEndOfTurn]);
    }
    if let Some((who, mana_raw)) = split_leading_player_verb(s, "add", "adds") {
        let mr = mana_raw.strip_prefix("an additional ").unwrap_or(mana_raw);
        if let Ok(spec) = parse_mana_spec(mr, vocab) {
            return Ok(vec![InstructionAst::AddMana {
                player: who,
                mana: spec,
            }]);
        }
    }

    // Draw
    if let Some((who, draw_raw)) = split_leading_player_verb(s, "draw", "draws") {
        let dlow = draw_raw.to_ascii_lowercase();
        let (core, delayed) = match dlow.strip_suffix(" at the beginning of the next turn's upkeep")
        {
            Some(c) => (c, true),
            None => (dlow.as_str(), false),
        };
        let cnt = if core == "an additional card" {
            Some(ValueExprAst::Const(1))
        } else if let Some(cnt_raw) = core
            .strip_suffix(" additional cards")
            .or_else(|| core.strip_suffix(" additional card"))
        {
            parse_value_expr(cnt_raw, vocab).ok()
        } else if let Some(after_half) = core.strip_prefix("half ")
            && let Some((expr_raw, round_raw)) = after_half.split_once(" cards, rounded ")
            && let Ok(inner) = parse_value_expr(expr_raw, vocab)
        {
            Some(ValueExprAst::DivConst {
                expr: Box::new(inner),
                denom: 2,
                rounding: if round_raw == "up" {
                    crate::ast::Rounding::Up
                } else {
                    crate::ast::Rounding::Down
                },
            })
        } else if let Some(fe) = core
            .strip_prefix("a card for each ")
            .or_else(|| core.strip_prefix("one card for each "))
        {
            parse_for_each_value_expr(fe, vocab).ok()
        } else if let Some((base_raw, fe)) = core.split_once(" cards for each ") {
            match (
                parse_value_expr(base_raw, vocab),
                parse_for_each_value_expr(fe, vocab),
            ) {
                (Ok(b), Ok(s)) => Some(ValueExprAst::Mul(Box::new(b), Box::new(s))),
                _ => None,
            }
        } else if let Some(val_raw) = core
            .strip_prefix("cards equal to ")
            .or_else(|| core.strip_prefix("card equal to "))
        {
            parse_value_expr(val_raw, vocab).ok()
        } else if let Some((base_cnt, plus_tok)) = core.split_once(" cards plus ") {
            match (
                parse_value_expr(base_cnt, vocab),
                parse_value_expr(plus_tok, vocab),
            ) {
                (Ok(b), Ok(p)) => Some(ValueExprAst::Add(Box::new(b), Box::new(p))),
                _ => None,
            }
        } else if let Some((base_cnt, minus_tok)) = core.split_once(" cards minus ") {
            match (
                parse_value_expr(base_cnt, vocab),
                parse_value_expr(minus_tok, vocab),
            ) {
                (Ok(b), Ok(m)) => Some(ValueExprAst::Sub(Box::new(b), Box::new(m))),
                _ => None,
            }
        } else if let Some(cnt_raw) = core
            .strip_suffix(" cards")
            .or_else(|| core.strip_suffix(" card"))
        {
            parse_value_expr(cnt_raw, vocab).ok()
        } else {
            None
        };
        if let Some(count) = cnt {
            let ins = InstructionAst::Draw {
                player: who,
                count: Box::new(count),
            };
            return Ok(if delayed {
                vec![InstructionAst::DelayedTrigger {
                    step: TurnStepAst::Upkeep,
                    scope: DelayedStepScope::Next,
                    body: vec![ins],
                }]
            } else {
                vec![ins]
            });
        }
    }

    // DealDamage (single or compound "deals N damage to <t1> and M damage to <t2>")
    if let Some(dmg_ins) = try_parse_deal_damage(s, vocab) {
        return Ok(dmg_ins);
    }

    // ModifyPT / ModifyAbilities / AnimateOrSetBasePT (strip trailing duration first)
    let (s_nodur, dur_opt) = strip_duration(s, vocab);
    let dur = dur_opt.unwrap_or(DurationAst::UntilEndOfTurn);

    if let Some(pt_ins) = try_parse_modify_pt(s_nodur, &dur, vocab) {
        return Ok(pt_ins);
    }

    if let Some(ab_ins) = try_parse_modify_abilities(s_nodur, &dur, vocab) {
        return Ok(ab_ins);
    }

    // DrainLife & AdjustLife
    if let Some(life_ins) = try_parse_life(&low, vocab) {
        return Ok(life_ins);
    }

    // CreateToken (including "create your choice of a Clue token, a Food token, or a Treasure token")
    if low.contains("token")
        && let Some((who, tok_raw)) = split_leading_player_verb(s, "create", "creates")
    {
        let tok_low = tok_raw.to_ascii_lowercase();
        if let Some(choice_raw) = tok_low
            .strip_prefix("your choice of ")
            .or_else(|| tok_low.strip_prefix("their choice of "))
        {
            let mut branches = Vec::new();
            let mut ok = true;
            for part in split_top_level(choice_raw, ",") {
                let p = part.trim();
                let p = p.strip_prefix("or ").unwrap_or(p).trim();
                if let Some((a, b)) = p.split_once(" or ") {
                    if let (Ok(ta), Ok(tb)) = (
                        parse_token_creation(a.trim(), vocab),
                        parse_token_creation(b.trim(), vocab),
                    ) {
                        branches.push(vec![InstructionAst::CreateToken {
                            controller: who,
                            token: ta,
                        }]);
                        branches.push(vec![InstructionAst::CreateToken {
                            controller: who,
                            token: tb,
                        }]);
                    } else {
                        ok = false;
                        break;
                    }
                } else if let Ok(tok) = parse_token_creation(p, vocab) {
                    branches.push(vec![InstructionAst::CreateToken {
                        controller: who,
                        token: tok,
                    }]);
                } else {
                    ok = false;
                    break;
                }
            }
            if ok && branches.len() >= 2 {
                return Ok(vec![InstructionAst::Choice(branches)]);
            }
        }
        if let Ok(tok) = parse_token_creation(tok_raw, vocab) {
            return Ok(vec![InstructionAst::CreateToken {
                controller: who,
                token: tok,
            }]);
        }
        // Multi-token creation list: "create <tok1>, <tok2>, and <tok3>" / "create <tok1> and <tok2>" (Triplicate Titan, etc.)
        if tok_raw.contains(" token") && (tok_raw.contains(", ") || tok_raw.contains(" and ")) {
            let mut toks = Vec::new();
            let mut ok = true;
            for part in split_top_level(tok_raw, ",") {
                let p = part.trim();
                let p = p.strip_prefix("and ").unwrap_or(p).trim();
                if p.is_empty() {
                    continue;
                }
                if let Ok(tok) = parse_token_creation(p, vocab) {
                    toks.push(InstructionAst::CreateToken {
                        controller: who,
                        token: tok,
                    });
                } else if let Some((a, b)) = p.split_once(" and ")
                    && let (Ok(ta), Ok(tb)) = (
                        parse_token_creation(a.trim(), vocab),
                        parse_token_creation(b.trim(), vocab),
                    )
                {
                    toks.push(InstructionAst::CreateToken {
                        controller: who,
                        token: ta,
                    });
                    toks.push(InstructionAst::CreateToken {
                        controller: who,
                        token: tb,
                    });
                } else {
                    ok = false;
                    break;
                }
            }
            if ok && toks.len() >= 2 {
                return Ok(toks);
            }
        }
    }

    // "<subj> enters with [an additional] <cnt> <kind> counter(s) on it" inside effects (Long List of the Ents, Undead Sprinter, Gluttonous Hellkite, Naya Soulbeast)
    if let Some((subj_raw, rest)) = low
        .split_once(" enters with ")
        .or_else(|| low.split_once(" enter with "))
        && let Some((cnt_part, tail)) = rest
            .split_once(" counters on it")
            .or_else(|| rest.split_once(" counter on it"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let cnt_clean = cnt_part
            .strip_prefix("an additional ")
            .or_else(|| cnt_part.strip_prefix("additional "))
            .unwrap_or(cnt_part);
        let (mut cnt, kind) = peel_leading_count(cnt_clean, vocab);
        let tail_ok = if tail.is_empty() {
            true
        } else if let Some(fe) = tail.strip_prefix(" for each ") {
            if let Ok(fe_val) = parse_for_each_value_expr(fe, vocab) {
                cnt = ValueExprAst::Mul(Box::new(cnt), Box::new(fe_val));
                true
            } else {
                false
            }
        } else {
            false
        };
        if tail_ok {
            return Ok(vec![InstructionAst::PutCounters {
                subject: Box::new(subj),
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
            }]);
        }
    }

    // MoveCounters / PutCounters / RemoveCounters
    if let Some(cnt_ins) = try_parse_counters(&low, vocab) {
        return Ok(vec![cnt_ins]);
    }

    // Destroy
    if let Some(after) = low
        .strip_prefix("destroy ")
        .or_else(|| low.strip_prefix("destroys "))
    {
        let (core, delayed) = if let Some(c) = after.strip_suffix(" at end of combat") {
            (c, Some((TurnStepAst::EndOfCombat, DelayedStepScope::Next)))
        } else if let Some(c) = after.strip_suffix(" at the beginning of the next end step") {
            (c, Some((TurnStepAst::EndStep, DelayedStepScope::Next)))
        } else {
            (after, None)
        };
        if !core.ends_with(" at random")
            && !core.contains(", except for ")
            && !core.contains(" except for ")
        {
            if let Some((left_all, right_all)) = core.split_once(" or all ")
                && let (Ok(s1), Ok(s2)) = (
                    parse_subject_or_target(left_all, vocab),
                    parse_subject_or_target(&format!("all {right_all}"), vocab),
                )
            {
                return Ok(vec![InstructionAst::Choice(vec![
                    vec![InstructionAst::Destroy {
                        subject: Box::new(s1),
                    }],
                    vec![InstructionAst::Destroy {
                        subject: Box::new(s2),
                    }],
                ])]);
            }
            if let Ok(subj) = parse_subject_or_target(core, vocab) {
                let ins = InstructionAst::Destroy {
                    subject: Box::new(subj),
                };
                return Ok(match delayed {
                    Some((step, scope)) => vec![InstructionAst::DelayedTrigger {
                        step,
                        scope,
                        body: vec![ins],
                    }],
                    None => vec![ins],
                });
            }
        }
    }

    // Exile (graveyard / until leaves / top library / general)
    if let Some(ex_ins) = try_parse_exile(&low, vocab) {
        return Ok(vec![ex_ins]);
    }

    // Sacrifice
    if let Some(sac_ins) = try_parse_sacrifice(s, &low, vocab) {
        return Ok(vec![sac_ins]);
    }

    // Discard
    if let Some(dis_ins) = try_parse_discard(s, vocab) {
        return Ok(vec![dis_ins]);
    }

    // ReturnZone / ChooseAndKeep / RevealFromLooked / MoveZone
    if let Some(z_ins) = try_parse_zone_moves(&low, vocab) {
        return Ok(vec![z_ins]);
    }

    // "Search <player>'s graveyard, hand, and library for [all|up to N] cards with the same name as <subj>/that name and exile them"
    if low.starts_with("search ")
        && low.contains("graveyard, hand, and library for ")
        && low.ends_with(" and exile them")
    {
        return Ok(vec![InstructionAst::ExileAllNamedFromZones {
            subject: Box::new(SubjectRefAst::Anaphora("named_cards".to_string())),
        }]);
    }

    // "Change the text of <target> by replacing ..."
    if let Some(after) = low.strip_prefix("change the text of ")
        && let Some((tgt_raw, desc)) = after.split_once(" by ")
        && let Ok(target) = parse_subject_or_target(tgt_raw, vocab)
    {
        return Ok(vec![InstructionAst::ChangeText {
            target: Box::new(target),
            description: desc.trim().to_string(),
        }]);
    }

    // Full multi-part SearchLibrary ("search your library [and/or graveyard] for ...")
    if (low.contains("search ") || low.contains("searches "))
        && low.contains(" library")
        && low.contains(" for ")
        && let Some(srch) = try_parse_search_library(&low, vocab)
    {
        return Ok(vec![srch]);
    }

    // Scry / Surveil / Mill / LookTop / RevealHand
    if let Some(lib_ins) = try_parse_library_ops(&low, vocab) {
        return Ok(vec![lib_ins]);
    }

    // Counter
    if let Some(after) = s
        .strip_prefix("Counter ")
        .or_else(|| s.strip_prefix("counter "))
    {
        if let Some((tgt_raw, unless_raw)) = after.split_once(" unless its controller pays ") {
            if let Ok(target) = parse_subject_or_target(tgt_raw, vocab) {
                let unless_pay = if let Ok(c) = parse_cost_atom(unless_raw, vocab) {
                    c
                } else {
                    let parts = parse_cost_list(unless_raw, vocab)?;
                    parts.into_iter().next().ok_or("empty_unless_pay")?
                };
                return Ok(vec![InstructionAst::Counter {
                    target: Box::new(target),
                    unless_pay: Some(unless_pay),
                    unless_action: None,
                }]);
            }
        } else if let Some((tgt_raw, unless_act)) = after.split_once(" unless ") {
            if let (Ok(target), Ok(act)) = (
                parse_subject_or_target(tgt_raw, vocab),
                parse_atomic_effect(unless_act, vocab),
            ) {
                return Ok(vec![InstructionAst::Counter {
                    target: Box::new(target),
                    unless_pay: None,
                    unless_action: Some(act),
                }]);
            }
        } else if let Some((spell_part, _targets_part)) =
            after.to_ascii_lowercase().split_once(" that targets ")
            && let Ok(target) = parse_subject_or_target(spell_part, vocab)
        {
            return Ok(vec![InstructionAst::Counter {
                target: Box::new(target),
                unless_pay: None,
                unless_action: None,
            }]);
        } else if let Ok(target) = parse_subject_or_target(after, vocab) {
            return Ok(vec![InstructionAst::Counter {
                target: Box::new(target),
                unless_pay: None,
                unless_action: None,
            }]);
        }
    }

    // Tap / Untap / SkipNextUntap / Regenerate / Transform / PhaseOut / Fight / Attach / GainControl / Prevent / Copy
    if let Some(misc_ins) = try_parse_misc_actions(s, s_nodur, &low, &dur, vocab) {
        return Ok(vec![misc_ins]);
    }

    // Named CR 701 keyword actions & player counters (Investigate, Monarch, Energy, Monstrosity, Adapt, etc.)
    if let Some(kw_ins) = try_parse_named_keyword_actions(s, &low, vocab) {
        return Ok(vec![kw_ins]);
    }

    // Combat rules & Animation
    if let Some(ca_ins) = try_parse_combat_and_animate(s_nodur, &low, &dur, vocab) {
        return Ok(vec![ca_ins]);
    }

    if low.contains("additional land") {
        return Ok(vec![InstructionAst::AdditionalLandPlay {
            count: 1,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if low.contains("additional combat phase") {
        return Ok(vec![InstructionAst::AdditionalCombat(s.to_string())]);
    }
    if let Some(who_raw) = low
        .strip_suffix(" takes an extra turn after this one")
        .or_else(|| low.strip_suffix(" take an extra turn after this one"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::ExtraTurn { player }]);
    }
    if let Some(who_raw) = low
        .strip_suffix(" loses the game")
        .or_else(|| low.strip_suffix(" lose the game"))
        && let Some(p) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::LoseGame { player: p }]);
    }

    // Pass C Orthogonal Primitives: ControlPlayerTurn, Exchange, WishFromOutsideGame, Equalize,
    // InteractiveChoice (VillainousChoice / SecretVote / SecretNumber / Bidding / Piles),
    // DistributeCounters, RepeatProcess, LookAtExiled, ManaSpendRestriction, Convert(=Transform)
    if let Some(prim_ins) = try_parse_pass_c_primitives(s, &low, vocab) {
        return Ok(prim_ins);
    }

    // Trailing conditional effect: "<effect> if <condition>"
    if !s.contains('"')
        && let Some(idx) = low.rfind(" if ")
        && let Ok(cond) = parse_condition(&s[idx + 4..], vocab)
        && let Ok(inner) = parse_atomic_effect(s[..idx].trim(), vocab)
    {
        return Ok(vec![InstructionAst::IfCondition {
            condition: Box::new(cond),
            then_body: inner,
        }]);
    }

    // Universal orthogonal "<effect> unless <player_cost | action | condition>" combinator
    if !s.contains('"')
        && let Some(idx) = low.rfind(" unless ")
        && let Ok(inner) = parse_atomic_effect(s[..idx].trim(), vocab)
    {
        let unless_raw = s[idx + 8..].trim();
        let ulow = unless_raw.to_ascii_lowercase();
        if let Some((who_raw, rest)) = ulow
            .split_once(" pays ")
            .or_else(|| ulow.split_once(" pay "))
            && let (Some(player), Ok(cost)) = (
                parse_player_ref(who_raw),
                parse_cost_atom(rest.trim(), vocab),
            )
        {
            return Ok(vec![InstructionAst::Unless {
                body: inner,
                unless_player: Some(player),
                unless_cost: Some(cost),
                unless_action: None,
                unless_condition: None,
            }]);
        }
        if let Ok(act) = parse_atomic_effect(unless_raw, vocab) {
            return Ok(vec![InstructionAst::Unless {
                body: inner,
                unless_player: None,
                unless_cost: None,
                unless_action: Some(act),
                unless_condition: None,
            }]);
        }
        if let Ok(cond) = parse_condition(unless_raw, vocab) {
            return Ok(vec![InstructionAst::Unless {
                body: inner,
                unless_player: None,
                unless_cost: None,
                unless_action: None,
                unless_condition: Some(Box::new(cond)),
            }]);
        }
    }

    // Standalone duration-scoped rules: "you have no maximum hand size until ...",
    // "<player> can't play lands [this turn]", "the next spell you cast this turn can't be countered",
    // "<subj> becomes blocked", "turn <subj> face down/up"
    if s_nodur.eq_ignore_ascii_case("you have no maximum hand size") {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
            rule: "no_max_hand_size".to_string(),
            duration: dur,
        }]);
    }
    if let Some(who_raw) = s_nodur
        .to_ascii_lowercase()
        .strip_suffix(" can't play lands")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Player(player)),
            rule: "cant_play_lands".to_string(),
            duration: dur,
        }]);
    }
    if low == "the next spell you cast this turn can't be countered"
        || low == "the next creature spell you cast this turn can't be countered"
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
            rule: "next_spell_cant_be_countered".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if low == "~ can't be copied" {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::SelfObject),
            rule: "cant_be_copied".to_string(),
            duration: DurationAst::Instant,
        }]);
    }
    if low == "there is an additional beginning phase after this phase" {
        return Ok(vec![InstructionAst::AdditionalCombat(
            "additional_beginning_phase".to_string(),
        )]);
    }
    if let Some(subj_raw) = s_nodur
        .to_ascii_lowercase()
        .strip_suffix(" becomes blocked")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "becomes_blocked".to_string(),
            duration: DurationAst::UntilEndOfCombat,
        }]);
    }
    if let Some(rest) = low.strip_prefix("turn ")
        && let Some((subj_raw, face_dir)) = rest
            .strip_suffix(" face down")
            .map(|r| (r, "face_down"))
            .or_else(|| rest.strip_suffix(" face up").map(|r| (r, "face_up")))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: format!("turn_{face_dir}"),
            duration: DurationAst::Permanent,
        }]);
    }

    // CR 608.2c Universal trailing "<effect> for each <player/object/value-expr>" combinator
    // (Aetherwind Basker, Sentry Bot, Lich's Tomb, Apothecary White, Malcolm, Urborg Lhurgoyf, Swift Silence)
    if !s.contains('"')
        && !low.starts_with("if ")
        && !low.starts_with("for each ")
        && let Some(fe_idx) = low.rfind(" for each ")
    {
        let body_part = s[..fe_idx].trim();
        let over_raw = s[fe_idx + " for each ".len()..].trim();
        if !body_part.is_empty()
            && !over_raw.is_empty()
            && let Ok(inner) = parse_atomic_effect(body_part, vocab)
        {
            let each_str = format!("each {over_raw}");
            if let Some(p) = parse_player_ref(&each_str) {
                return Ok(vec![InstructionAst::ForEachPlayer {
                    player: p,
                    body: inner,
                }]);
            }
            if let Ok(flt) = parse_object_filter(over_raw, vocab) {
                return Ok(vec![InstructionAst::ForEachObject {
                    filter: Box::new(flt),
                    body: inner,
                }]);
            }
            if let Ok(cnt) = parse_for_each_value_expr(over_raw, vocab) {
                return Ok(vec![InstructionAst::ForEachCount {
                    count: Box::new(cnt),
                    body: inner,
                }]);
            }
        }
    }

    // "<player> taps/untaps <subj>" / "<player> incubates <val>" / "<player> amasses <subt> <val>" / "exchange life totals with <player>"
    if let Some((_, subj_raw)) = split_leading_player_verb(&low, "untap", "untaps")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![InstructionAst::UntapPermanent {
            subject: Box::new(subj),
        }]);
    }
    if let Some((_, subj_raw)) = split_leading_player_verb(&low, "tap", "taps")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![InstructionAst::TapPermanent {
            subject: Box::new(subj),
        }]);
    }
    if let Some((_, val_raw)) = split_leading_player_verb(&low, "incubate", "incubates")
        && let Ok(count) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![InstructionAst::Incubate {
            count: Box::new(count),
        }]);
    }
    if let Some((_, rest)) = split_leading_player_verb(&low, "amass", "amasses")
        && let Some((subt, cnt_raw)) = rest.rsplit_once(' ')
        && let Ok(count) = parse_value_expr(cnt_raw, vocab)
    {
        return Ok(vec![InstructionAst::Amass {
            subtype: subt.trim().to_string(),
            count: Box::new(count),
        }]);
    }
    if let Some(who_raw) = low.strip_prefix("exchange life totals with ")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::Exchange {
            kind: ExchangeKind::LifeTotals,
            left: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
            right: Some(Box::new(SubjectRefAst::Player(player))),
        }]);
    }

    // Compound ", and " split (outside quotes) BEFORE plain " and " so trailing commas never break the left half!
    if low.contains(", and ") && !s.contains('"') {
        let parts = split_top_level(s, ", and ");
        if parts.len() >= 2 {
            let mut seq = Vec::new();
            let mut ok = true;
            for p in &parts {
                match parse_atomic_effect(p, vocab) {
                    Ok(sub) => seq.extend(sub),
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                return Ok(seq);
            }
        }
    }

    // Trailing "<effect> unless <condition>" (Insubordination, Katara, etc.)
    if low.contains(" unless ")
        && !s.contains('"')
        && !low.starts_with("if ")
        && !low.starts_with("when ")
        && !low.starts_with("whenever ")
        && let Some(uidx) = low.rfind(" unless ")
        && let Ok(cond) = parse_condition(&s[uidx + " unless ".len()..], vocab)
        && let Ok(body) = parse_atomic_effect(s[..uidx].trim(), vocab)
    {
        return Ok(vec![InstructionAst::Unless {
            body,
            unless_player: None,
            unless_cost: None,
            unless_action: None,
            unless_condition: Some(Box::new(cond)),
        }]);
    }

    // Compound " and " split (quote-aware via split_top_level, scanning split points with shared-subject carry-forward!)
    if low.contains(" and ") {
        let and_parts = split_top_level(s, " and ");
        if and_parts.len() >= 2 {
            for split_idx in 1..and_parts.len() {
                let left_owned = and_parts[..split_idx].join(" and ");
                let right_owned = and_parts[split_idx..].join(" and ");
                let left_s = left_owned.trim();
                let right_s = right_owned.trim();
                let llow = left_s.to_ascii_lowercase();
                let rlow = right_s.to_ascii_lowercase();
                let right_starts_verb = [
                    "you ",
                    "put ",
                    "draw ",
                    "gain",
                    "lose",
                    "create",
                    "scry ",
                    "surveil ",
                    "mill ",
                    "shuffle",
                    "return ",
                    "remove ",
                    "untap ",
                    "tap ",
                    "destroy ",
                    "exile ",
                    "sacrifice ",
                    "discard ",
                    "add ",
                    "deal",
                    "investigate",
                    "proliferate",
                    "populate",
                    "venture ",
                    "the ring ",
                    "amass ",
                    "incubate ",
                    "manifest",
                    "discover ",
                    "bolster ",
                    "support ",
                    "endure ",
                    "blight ",
                    "forage",
                    "time travel",
                    "open an ",
                    "planeswalk",
                    "mana of any ",
                    "goad ",
                    "collect ",
                    "choose ",
                    "counter ",
                    "attach ",
                    "put ",
                    "puts ",
                    "create ",
                    "creates ",
                    "it ",
                    "they ",
                    "that ",
                    "each ",
                    "~ ",
                ]
                .iter()
                .any(|p| rlow.starts_with(p));
                let left_is_obj_verb = ["destroy ", "exile ", "return ", "tap ", "untap "]
                    .iter()
                    .any(|p| llow.starts_with(p));
                if (!left_is_obj_verb || right_starts_verb)
                    && let (Ok(mut l), Ok(r)) = (
                        parse_atomic_effect(left_s, vocab),
                        parse_atomic_effect(right_s, vocab),
                    )
                {
                    l.extend(r);
                    return Ok(l);
                }
                let right_is_predicate = [
                    "deals ",
                    "deal ",
                    "gets ",
                    "get ",
                    "gains ",
                    "gain ",
                    "loses ",
                    "lose ",
                    "can ",
                    "can't ",
                    "has ",
                    "have ",
                    "becomes ",
                    "become ",
                    "doesn't ",
                    "don't ",
                    "attacks ",
                    "attack ",
                    "blocks ",
                    "block ",
                    "discards ",
                    "discard ",
                    "reveals ",
                    "reveal ",
                    "exiles ",
                    "exile ",
                    "sacrifices ",
                    "sacrifice ",
                    "puts ",
                    "put ",
                    "returns ",
                    "return ",
                    "must ",
                    "draws ",
                    "draw ",
                    "taps ",
                    "tap ",
                    "untaps ",
                    "untap ",
                    "creates ",
                    "create ",
                    "searches ",
                    "search ",
                    "shuffles",
                    "shuffle",
                    "mills ",
                    "mill ",
                    "adds ",
                    "add ",
                    "chooses ",
                    "choose ",
                    "copies ",
                    "copy ",
                    "plays ",
                    "play ",
                    "casts ",
                    "cast ",
                    "is ",
                    "are ",
                    "isn't ",
                    "aren't ",
                ]
                .iter()
                .any(|p| rlow.starts_with(p));
                if right_is_predicate
                    && let Some(subj_prefix) = extract_leading_subject_prefix(left_s)
                    && let (Ok(mut l), Ok(r)) = (
                        parse_atomic_effect(left_s, vocab),
                        parse_atomic_effect(&format!("{subj_prefix} {right_s}"), vocab),
                    )
                {
                    l.extend(r);
                    return Ok(l);
                }
                // CR 608.2c Object-verb distribution across compound objects:
                // "Target opponent exiles a creature they control and their graveyard",
                // "Target player reveals their hand and the top card of their library",
                // "Tap target creature you control and target creature of an opponent's choice they control"
                if !right_starts_verb
                    && !right_is_predicate
                    && let Some(verb_prefix) = extract_leading_action_verb_prefix(left_s)
                    && let (Ok(mut l), Ok(r)) = (
                        parse_atomic_effect(left_s, vocab),
                        parse_atomic_effect(&format!("{verb_prefix} {right_s}"), vocab),
                    )
                {
                    l.extend(r);
                    return Ok(l);
                }
            }
        }
    }

    // Disjunctive imperative choice combinator: "<action1> or <action2>" (scanning all top-level " or ", with shared-subject carry-forward!)
    // (e.g. "sacrifice an artifact or discard a card", "put a charge counter on ~ or remove one from it")
    if low.contains(" or ")
        && s.len() <= 280
        && !(low.starts_with("if ")
            || low.starts_with("when ")
            || low.starts_with("whenever ")
            || low.starts_with("choose "))
    {
        for (idx, _) in low.match_indices(" or ") {
            let left_s = s[..idx].trim();
            let right_s = s[idx + 4..].trim();
            let rlow = right_s.to_ascii_lowercase();
            let right_is_action = [
                "pay ",
                "pays ",
                "sacrifice ",
                "sacrifices ",
                "discard ",
                "discards ",
                "exile ",
                "exiles ",
                "destroy ",
                "destroys ",
                "draw ",
                "draws ",
                "gain ",
                "gains ",
                "lose ",
                "loses ",
                "put ",
                "puts ",
                "return ",
                "returns ",
                "tap ",
                "taps ",
                "untap ",
                "untaps ",
                "create ",
                "creates ",
                "scry ",
                "surveil ",
                "mill ",
                "mills ",
                "remove ",
                "removes ",
                "move ",
                "moves ",
                "copy ",
                "copies ",
                "cast ",
                "casts ",
                "play ",
                "plays ",
                "choose ",
                "chooses ",
                "add ",
                "adds ",
                "deals ",
                "deal ",
                "becomes ",
                "become ",
                "you ",
                "that player ",
                "each ",
            ]
            .iter()
            .any(|p| rlow.starts_with(p));
            if right_is_action {
                if let (Ok(l), Ok(r)) = (
                    parse_atomic_effect(left_s, vocab),
                    parse_atomic_effect(right_s, vocab),
                ) {
                    return Ok(vec![InstructionAst::Choice(vec![l, r])]);
                }
                if let Some(subj_prefix) = extract_leading_subject_prefix(left_s)
                    && let (Ok(l), Ok(r)) = (
                        parse_atomic_effect(left_s, vocab),
                        parse_atomic_effect(&format!("{subj_prefix} {right_s}"), vocab),
                    )
                {
                    return Ok(vec![InstructionAst::Choice(vec![l, r])]);
                }
            }
        }
    }

    // Top-level ", then " sequence splitting outside quotes (with shared-subject carry-forward)
    if s.contains(", then ")
        && !(low.starts_with("if ")
            || low.starts_with("when ")
            || low.starts_with("whenever ")
            || low.starts_with("at the "))
    {
        let then_parts = split_top_level(s, ", then ");
        if then_parts.len() >= 2 && then_parts.len() <= 4 {
            let lead_subj = extract_leading_subject_prefix(then_parts[0].trim());
            let mut seq = Vec::new();
            let mut ok = true;
            for (idx_p, p) in then_parts.iter().enumerate() {
                let pt = p.trim();
                let parsed = if idx_p > 0
                    && let Some(subj) = lead_subj
                    && starts_with_predicate_verb(pt)
                {
                    parse_atomic_effect(&format!("{subj} {pt}"), vocab)
                        .or_else(|_| parse_atomic_effect(pt, vocab))
                } else {
                    parse_atomic_effect(pt, vocab).or_else(|_| {
                        if idx_p > 0
                            && let Some(subj) = lead_subj
                        {
                            parse_atomic_effect(&format!("{subj} {pt}"), vocab)
                        } else {
                            Err("no_subj".to_string())
                        }
                    })
                };
                match parsed {
                    Ok(sub) if !sub.is_empty() => seq.extend(sub),
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && !seq.is_empty() {
                return Ok(seq);
            }
        }
    }

    // Generalized comma-sequence splitting ("A, B, and C" / "A, B, then C" / "A, B") outside quotes,
    // with optional shared-subject OR shared-leading-action-verb carry-forward and TemporaryStatic support!
    if low.contains(", ")
        && s.len() <= 420
        && !(low.starts_with("if ")
            || low.starts_with("when ")
            || low.starts_with("whenever ")
            || low.starts_with("at the ")
            || low.starts_with("for each ")
            || low.starts_with("as ")
            || low.starts_with("choose ")
            || low.starts_with("until ")
            || low.starts_with("starting with ")
            || low.starts_with("search "))
    {
        let parts = split_top_level(s, ", ");
        if parts.len() >= 2 && parts.len() <= 4 {
            let lead_subj = extract_leading_subject_prefix(parts[0].trim());
            let lead_verb = extract_leading_action_verb_prefix(parts[0].trim());
            let mut seq = Vec::new();
            let mut ok = true;
            for (idx_p, p) in parts.iter().enumerate() {
                let p_trim = p.trim();
                let p_clean = p_trim
                    .strip_prefix("and then ")
                    .or_else(|| p_trim.strip_prefix("and "))
                    .or_else(|| p_trim.strip_prefix("then "))
                    .unwrap_or(p_trim);
                if p_clean.is_empty() {
                    ok = false;
                    break;
                }
                let parsed = if idx_p > 0
                    && let Some(subj) = lead_subj
                    && starts_with_predicate_verb(p_clean)
                {
                    parse_effect_or_temp_clause(
                        &format!("{subj} {p_clean}"),
                        &DurationAst::UntilEndOfTurn,
                        vocab,
                    )
                    .or_else(|| {
                        parse_effect_or_temp_clause(p_clean, &DurationAst::UntilEndOfTurn, vocab)
                    })
                } else {
                    parse_effect_or_temp_clause(p_clean, &DurationAst::UntilEndOfTurn, vocab)
                        .or_else(|| {
                            if idx_p > 0
                                && let Some(subj) = lead_subj
                            {
                                parse_effect_or_temp_clause(
                                    &format!("{subj} {p_clean}"),
                                    &DurationAst::UntilEndOfTurn,
                                    vocab,
                                )
                            } else {
                                None
                            }
                        })
                        .or_else(|| {
                            if idx_p > 0
                                && let Some(verb) = lead_verb
                            {
                                parse_effect_or_temp_clause(
                                    &format!("{verb} {p_clean}"),
                                    &DurationAst::UntilEndOfTurn,
                                    vocab,
                                )
                            } else {
                                None
                            }
                        })
                };
                match parsed {
                    Some(sub) if !sub.is_empty() => seq.extend(sub),
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && !seq.is_empty() {
                return Ok(seq);
            }
        }
    }

    // Temporary continuous static effect created during resolution ("<static> until end of turn",
    // "<subj> can attack this turn as though they didn't have defender",
    // "Artifact spells you cast this turn cost {1} less to cast", "A spell cast this way costs {2} more to cast")
    if !low.starts_with("if ")
        && !low.starts_with("when ")
        && !low.starts_with("whenever ")
        && !low.starts_with("at the ")
    {
        if dur != DurationAst::Instant
            && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(s_nodur, vocab)
        {
            return Ok(vec![InstructionAst::TemporaryStatic {
                static_ability: Box::new(st),
                duration: dur,
            }]);
        }
        if low.contains(" this turn ") {
            let stripped = s.replacen(" this turn ", " ", 1);
            if let Ok(StaticOrReplacement::Static(st)) =
                parse_static_or_replacement(&stripped, vocab)
            {
                return Ok(vec![InstructionAst::TemporaryStatic {
                    static_ability: Box::new(st),
                    duration: DurationAst::UntilEndOfTurn,
                }]);
            }
        }
        if (low.contains(" costs ") || low.contains(" cost "))
            && (low.contains("less to cast") || low.contains("more to cast"))
            && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(s, vocab)
        {
            return Ok(vec![InstructionAst::TemporaryStatic {
                static_ability: Box::new(st),
                duration: DurationAst::UntilEndOfTurn,
            }]);
        }
    }

    // Compositional Wave 14: Dynamic P/T assignment (`Sworn Defender`, `Eldrazi Mimic`, `Halfdane`)
    // - "<subj>'s [base] power becomes <val_expr>"
    // - "<subj>'s [base] toughness becomes <val_expr>"
    // - "<subj>'s [base] power and toughness become [equal to] <other>'s power and toughness / the power and toughness of <other>"
    {
        let pt_low = s_nodur.to_ascii_lowercase();
        for (sep, is_power) in [
            ("'s base power becomes ", true),
            ("'s power becomes ", true),
            ("'s base toughness becomes ", false),
            ("'s toughness becomes ", false),
        ] {
            if let Some((subj_raw, rhs_raw)) = pt_low.split_once(sep) {
                let rhs_clean = rhs_raw.strip_prefix("equal to ").unwrap_or(rhs_raw).trim();
                if let (Ok(subj), Ok(val)) = (
                    parse_subject_or_target(subj_raw, vocab),
                    parse_value_expr(rhs_clean, vocab),
                ) {
                    return Ok(vec![InstructionAst::SetDynamicPT {
                        subject: Box::new(subj),
                        power: if is_power {
                            Some(Box::new(val.clone()))
                        } else {
                            None
                        },
                        toughness: if !is_power { Some(Box::new(val)) } else { None },
                        duration: dur,
                    }]);
                }
            }
        }
        for sep in [
            "'s base power and toughness become equal to ",
            "'s power and toughness become equal to ",
            "'s base power and toughness become ",
            "'s power and toughness become ",
        ] {
            if let Some((subj_raw, rhs_raw)) = pt_low.split_once(sep)
                && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            {
                let other_raw = rhs_raw
                    .strip_prefix("the power and toughness of ")
                    .or_else(|| rhs_raw.strip_suffix("'s power and toughness"))
                    .or_else(|| rhs_raw.strip_suffix("'s base power and toughness"));
                if let Some(oraw) = other_raw
                    && let Ok(other) = parse_subject_or_target(oraw.trim(), vocab)
                {
                    return Ok(vec![InstructionAst::SetDynamicPT {
                        subject: Box::new(subj),
                        power: Some(Box::new(ValueExprAst::ObjectStat {
                            subject: Box::new(other.clone()),
                            stat: StatKind::Power,
                        })),
                        toughness: Some(Box::new(ValueExprAst::ObjectStat {
                            subject: Box::new(other),
                            stat: StatKind::Toughness,
                        })),
                        duration: dur,
                    }]);
                }
            }
        }
    }

    // Compositional Wave 14: "Exile <subj> until <event_or_cond>" (Palace Jailer, Banishing Light variants)
    if let Some(after_ex) = low.strip_prefix("exile ")
        && let Some((subj_raw, until_raw)) = after_ex.split_once(" until ")
        && !until_raw.is_empty()
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![InstructionAst::ExileUntilLeaves {
            subject: Box::new(subj),
        }]);
    }

    // Compositional Wave 14: Trailing "<effect> for each <filter_or_expr>" (Rakdos, the Showstopper, etc.)
    if !low.starts_with("for each ")
        && !s.contains('"')
        && let Some(fe_idx) = low.rfind(" for each ")
    {
        let left_eff = s[..fe_idx].trim();
        let right_over = s[fe_idx + " for each ".len()..].trim();
        if !left_eff.is_empty() && !right_over.is_empty() {
            if let Ok(flt) = parse_object_filter(right_over, vocab)
                && let Ok(body) = parse_atomic_effect(left_eff, vocab)
            {
                return Ok(vec![InstructionAst::ForEachObject {
                    filter: Box::new(flt),
                    body,
                }]);
            }
            if let Ok(cnt) = parse_for_each_value_expr(right_over, vocab)
                && let Ok(body) = parse_atomic_effect(left_eff, vocab)
            {
                return Ok(vec![InstructionAst::ForEachCount {
                    count: Box::new(cnt),
                    body,
                }]);
            }
        }
    }

    // Compositional Wave 14: Generalized "<effect> unless <player_cost_or_action_or_cond>"
    // (Lava Blister, Combustion Man, Wand of Ith, Tainted Pact, Glass Asp, Sabertooth Cobra, Flash, Oblivious Bookworm)
    if !low.starts_with("unless ")
        && let Some(u_idx) = find_top_level_pattern(s, " unless ")
    {
        let left_s = s[..u_idx].trim();
        let right_s = s[u_idx + " unless ".len()..].trim();
        if !left_s.is_empty()
            && !right_s.is_empty()
            && let Ok(body) = parse_atomic_effect(left_s, vocab)
        {
            let rlow = right_s.to_ascii_lowercase();
            let rclean = rlow
                .strip_suffix(" before that step")
                .or_else(|| rlow.strip_suffix(" before then"))
                .unwrap_or(&rlow)
                .trim();
            // Case 1: "<player> pays <cost>"
            let pay_split = rclean
                .find(" pays ")
                .map(|i| (i, 6))
                .or_else(|| rclean.find(" pay ").map(|i| (i, 5)));
            if let Some((pidx, plen)) = pay_split
                && let Some(player) = parse_player_ref(rclean[..pidx].trim())
            {
                let cost_part = &right_s[pidx + plen..rclean.len()];
                if let Ok(cost) = parse_cost_atom(cost_part.trim(), vocab)
                    .or_else(|_| parse_cost_atom(&format!("pay {}", cost_part.trim()), vocab))
                {
                    return Ok(vec![InstructionAst::Unless {
                        body,
                        unless_player: Some(player),
                        unless_cost: Some(cost),
                        unless_action: None,
                        unless_condition: None,
                    }]);
                }
            }
            // Case 2: "<player> has <effect>" (Lava Blister, Combustion Man)
            if let Some((p_raw, has_eff)) = rclean
                .split_once(" has ")
                .or_else(|| rclean.split_once(" have "))
                && let Some(player) = parse_player_ref(p_raw.trim())
                && let Ok(act) = parse_atomic_effect(has_eff.trim(), vocab)
            {
                return Ok(vec![InstructionAst::Unless {
                    body,
                    unless_player: Some(player),
                    unless_cost: None,
                    unless_action: Some(act),
                    unless_condition: None,
                }]);
            }
            // Case 3: arbitrary condition ("unless it has the same name as ...", "unless a permanent entered ...")
            if let Ok(cond) = parse_condition(right_s, vocab) {
                return Ok(vec![InstructionAst::Unless {
                    body,
                    unless_player: None,
                    unless_cost: None,
                    unless_action: None,
                    unless_condition: Some(Box::new(cond)),
                }]);
            }
            // Case 4: arbitrary alternative action ("unless they pay life equal to its mana value", "unless you discard a card")
            if let Ok(act) = parse_atomic_effect(right_s, vocab) {
                return Ok(vec![InstructionAst::Unless {
                    body,
                    unless_player: None,
                    unless_cost: None,
                    unless_action: Some(act),
                    unless_condition: None,
                }]);
            }
        }
    }

    // Compositional Wave 15: Clean reusable effect verbs & trailing "<effect> if <cond>"
    if low == "draw another card" {
        return Ok(vec![InstructionAst::Draw {
            player: PlayerRefAst::You,
            count: Box::new(ValueExprAst::Const(1)),
        }]);
    }
    if let Some(val_raw) = low.strip_prefix("draw cards equal to ")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![InstructionAst::Draw {
            player: PlayerRefAst::You,
            count: Box::new(val),
        }]);
    }
    if let Some(val_raw) = low.strip_prefix("discard cards equal to ")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![InstructionAst::Discard {
            player: PlayerRefAst::You,
            count: Some(Box::new(val)),
            filter: None,
            anaphora: None,
            random: false,
        }]);
    }
    if let Some(who_raw) = low
        .strip_suffix(" loses all unspent mana")
        .or_else(|| low.strip_suffix(" lose all unspent mana"))
        && let Some(_p) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::SelfObject),
            rule: format!("lose_unspent_mana:{who_raw}"),
            duration: DurationAst::Instant,
        }]);
    }
    if let Some((who_raw, rest)) = s
        .split_once(" faces a villainous choice \u{2014} ")
        .or_else(|| s.split_once(" face a villainous choice \u{2014} "))
        && parse_player_ref(who_raw).is_some()
        && let Some((opt1, opt2)) = rest.split_once(", or ")
        && let (Ok(v1), Ok(v2)) = (
            parse_atomic_effect(opt1.trim(), vocab),
            parse_atomic_effect(opt2.trim(), vocab),
        )
    {
        return Ok(vec![InstructionAst::Choice(vec![v1, v2])]);
    }
    if let Some(flt_raw) = low.strip_prefix("count the number of ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(vec![InstructionAst::ChooseOption(format!(
            "count:{flt_raw}"
        ))]);
    }
    if let Some(subj_raw) = low.strip_prefix("unattach ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![InstructionAst::Attach {
            subject: Box::new(subj),
            target: Box::new(SubjectRefAst::SelfObject),
        }]);
    }
    if let Some((who_raw, rest)) = low
        .split_once(" attaches ")
        .or_else(|| low.split_once(" attach "))
        && parse_player_ref(who_raw).is_some()
        && let Some((a_raw, b_raw)) = rest.split_once(" to ")
        && let (Ok(a), Ok(b)) = (
            parse_subject_or_target(a_raw, vocab),
            parse_subject_or_target(b_raw, vocab),
        )
    {
        return Ok(vec![InstructionAst::Attach {
            subject: Box::new(a),
            target: Box::new(b),
        }]);
    }
    if let Some((owner_raw, rest)) = low.split_once("'s owner shuffles ")
        && let Some(subj_raw) = rest
            .strip_suffix(" into their library")
            .or_else(|| rest.strip_suffix(" into its owner's library"))
        && parse_subject_or_target(owner_raw, vocab).is_ok()
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(subj),
        }]);
    }
    if let Some(after_rev) = low.strip_prefix("reveal ") {
        for hand_suf in [
            " from your hand",
            " in your hand",
            " from their hand",
            " in their hand",
        ] {
            if let Some(flt_raw) = after_rev.strip_suffix(hand_suf)
                && !flt_raw.starts_with("any number of ")
                && !flt_raw.starts_with("x ")
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(vec![InstructionAst::RevealFromLooked {
                    filter: Box::new(flt),
                    to: "hand".to_string(),
                }]);
            }
        }
    }
    if let Some((both_raw, rest)) = s_nodur.to_ascii_lowercase().split_once(" each get ")
        && let Some((s1_raw, s2_raw)) = both_raw.split_once(" and ")
        && let (Ok(s1), Ok(s2)) = (
            parse_subject_or_target(s1_raw, vocab),
            parse_subject_or_target(s2_raw, vocab),
        )
    {
        let (pt_tok, kws) = match rest.split_once(" and gain ") {
            Some((pt_part, kw_part)) => (
                pt_part.trim(),
                parse_keyword_list(kw_part.trim(), vocab).unwrap_or_default(),
            ),
            None => (rest.trim(), Vec::new()),
        };
        if let Some((dx, dy)) = parse_pt_mod(pt_tok) {
            let mut out = Vec::new();
            for subj in [s1, s2] {
                out.push(InstructionAst::ModifyPT {
                    subject: Box::new(subj.clone()),
                    dx: dx.clone(),
                    dy: dy.clone(),
                    duration: dur.clone(),
                    scale: None,
                });
                if !kws.is_empty() {
                    out.push(InstructionAst::GrantKeywords {
                        subject: Box::new(subj),
                        keywords: kws.clone(),
                        duration: dur.clone(),
                    });
                }
            }
            return Ok(out);
        }
    }
    if let Some(after_ent) = low.strip_prefix("~ enters as a ")
        && let Some((pt_tok, desc_rest)) = after_ent.split_once(' ')
        && desc_rest.contains("creature")
        && let Some((p, t)) = parse_base_pt(pt_tok)
    {
        let (desc, kws) = match desc_rest.split_once(" with ") {
            Some((d, kw)) => (d.trim(), parse_keyword_list(kw, vocab).unwrap_or_default()),
            None => (desc_rest.trim(), Vec::new()),
        };
        return Ok(vec![InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(SubjectRefAst::SelfObject),
            power: p,
            toughness: t,
            description: Some(desc.to_string()),
            keywords: kws,
            duration: DurationAst::Permanent,
        }]);
    }
    // General "play/cast [up to N of] <subj_or_filter> [without paying ...]"
    let s_nodur_cp_low = s_nodur.to_ascii_lowercase();
    if let Some((cast_only, after_cp)) = s_nodur_cp_low
        .strip_prefix("cast ")
        .map(|r| (true, r))
        .or_else(|| s_nodur_cp_low.strip_prefix("play ").map(|r| (false, r)))
    {
        let (core, free) = match after_cp
            .strip_suffix(" without paying their mana costs")
            .or_else(|| after_cp.strip_suffix(" without paying its mana cost"))
            .or_else(|| after_cp.strip_suffix(" if able without paying their mana costs"))
            .or_else(|| after_cp.strip_suffix(" if able without paying its mana cost"))
        {
            Some(c) => (c.trim(), true),
            None => (after_cp.trim(), false),
        };
        if !core.starts_with("up to two of ")
            && !core.starts_with("up to three of ")
            && !core.starts_with("up to one of ")
            && !core.starts_with("any number of ")
            && let Ok(subj) = parse_subject_or_target(core, vocab)
        {
            return Ok(vec![InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only,
                subject: Box::new(subj),
                free,
                duration: format!("{dur:?}"),
            }]);
        }
    }

    // Compositional Wave 16: Direct delayed-trigger and duration-prefix combinators
    // ("When you do, ...", "When <ev>, ...", "Whenever <ev>, ...", "At the beginning of <step>, ...", "During <period>, ...")
    if low.starts_with("when you do, ")
        && let Some(inner) = parse_effect_or_temp_clause(s[13..].trim(), &dur, vocab)
    {
        return Ok(vec![InstructionAst::IfPreviousSucceeded(inner)]);
    }
    if let Some(trig) = try_parse_inline_trigger(s, dur.clone(), vocab) {
        return Ok(vec![trig]);
    }
    if low.starts_with("at this turn's next ")
        && let Some((hdr, body_raw)) = s.split_once(", ")
        && let Some(body) = parse_effect_or_temp_clause(body_raw.trim(), &dur, vocab)
    {
        return Ok(vec![InstructionAst::TemporaryTrigger {
            trigger: TriggerSpecAst {
                events: vec![TriggerEventAst::StepBegan {
                    step: TurnStepAst::EndOfCombat,
                    whose: hdr.trim().to_string(),
                }],
                intervening_if: None,
            },
            body,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if low.starts_with("during ")
        && let Some((period_raw, body_raw)) = s.split_once(", ")
        && !period_raw.is_empty()
        && !body_raw.is_empty()
    {
        let body_clean = body_raw
            .trim()
            .strip_prefix("as you untap your permanents, ")
            .unwrap_or(body_raw.trim());
        if let Some(body) =
            parse_effect_or_temp_clause(body_clean, &DurationAst::UntilYourNextTurn, vocab)
        {
            return Ok(body);
        }
    }

    // Compositional Wave 16: Token-copy creation with optional tapped/attacking/blocking/except modifiers
    // ("create a [tapped] [and attacking] token that's a copy of <subj>[, except <exc> / and that's blocking <subj>]")
    if let Some(tc_idx) = low
        .find("token that's a copy of ")
        .or_else(|| low.find("tokens that are copies of "))
    {
        let before_tc = low[..tc_idx].trim();
        let after_tc = s[tc_idx..].split_once(" of ").map_or("", |(_, r)| r.trim());
        if before_tc.contains("create ") || before_tc.contains("creates ") {
            let (subj_raw, exc_opt) = if let Some((l, r)) = after_tc.split_once(", except ") {
                (l.trim(), Some(r.trim().to_string()))
            } else if let Some((l, r)) = after_tc.split_once(" except ") {
                (l.trim(), Some(r.trim().to_string()))
            } else if let Some((l, r)) = after_tc.split_once(" and that's blocking ") {
                (l.trim(), Some(format!("blocking {}", r.trim())))
            } else {
                (after_tc, None)
            };
            if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
                return Ok(vec![InstructionAst::Copy {
                    subject: Box::new(subj),
                    exception: exc_opt,
                    count: None,
                }]);
            }
        }
    }

    // Compositional Wave 16: General move/remove counters from [among] <subj> [onto <tgt>]
    if let Some(after_mv) = low.strip_prefix("move ")
        && let Some((cnt_part, rest)) = after_mv.split_once(" from ")
        && let Some((src_raw, dst_raw)) = rest.split_once(" onto ")
        && cnt_part.contains("counter")
    {
        let src_clean = src_raw.strip_prefix("among ").unwrap_or(src_raw).trim();
        if let (Ok(src), Ok(dst)) = (
            parse_subject_or_target(src_clean, vocab),
            parse_subject_or_target(dst_raw.trim(), vocab),
        ) {
            return Ok(vec![InstructionAst::MoveCounters {
                from: Box::new(src),
                to: Box::new(dst),
                counter_kind: cnt_part.trim().to_string(),
                all: cnt_part.starts_with("all "),
                any_number: cnt_part.starts_with("any number"),
                count: Some(Box::new(ValueExprAst::ChosenX)),
            }]);
        }
    }
    if let Some(after_rm) = low.strip_prefix("remove ")
        && let Some((cnt_part, src_raw)) = after_rm.split_once(" from ")
        && (cnt_part.contains("counter") || cnt_part == "one" || cnt_part == "all")
        && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        let all = cnt_part == "all" || cnt_part.starts_with("all ");
        let (cnt, kind) = if cnt_part == "one" {
            (Some(Box::new(ValueExprAst::Const(1))), "same".to_string())
        } else if all {
            let k = cnt_part
                .strip_prefix("all ")
                .unwrap_or("")
                .strip_suffix(" counters")
                .or_else(|| {
                    cnt_part
                        .strip_prefix("all ")
                        .unwrap_or("")
                        .strip_suffix(" counter")
                })
                .unwrap_or("any");
            (None, k.to_string())
        } else {
            let (c, k) = peel_leading_count(
                cnt_part
                    .strip_suffix(" counters")
                    .or_else(|| cnt_part.strip_suffix(" counter"))
                    .unwrap_or(cnt_part),
                vocab,
            );
            (Some(Box::new(c)), k.to_string())
        };
        return Ok(vec![InstructionAst::RemoveCounters {
            subject: Box::new(src),
            counter_kind: kind,
            all,
            count: cnt,
        }]);
    }

    // Compositional Wave 16: General type/Aura transformation and quoted-ability grants
    // Only emit GrantAbilityText when the quoted ability itself strictly compiles (keeps 0 new string escapes!).
    if let Some(after_is) = s_nodur
        .strip_prefix("It's a ")
        .or_else(|| s_nodur.strip_prefix("It's an "))
        .or_else(|| s_nodur.strip_prefix("it's a "))
        .or_else(|| s_nodur.strip_prefix("it's an "))
        .or_else(|| s_nodur.strip_prefix("They are "))
        .or_else(|| s_nodur.strip_prefix("they are "))
        .or_else(|| s_nodur.strip_prefix("They're "))
        .or_else(|| s_nodur.strip_prefix("they're "))
        && !after_is.is_empty()
    {
        let mut out = vec![InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            power: String::new(),
            toughness: String::new(),
            description: Some(after_is.to_string()),
            keywords: Vec::new(),
            duration: dur.clone(),
        }];
        if let Some(q_start) = after_is.find('"')
            && let Some(q_end) = after_is.rfind('"')
            && q_end > q_start
        {
            let q_text = &after_is[q_start + 1..q_end];
            if let Some(compiled) = compile_granted_ability(q_text, vocab) {
                out.push(InstructionAst::GrantAbilityText {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    ability_text: q_text.to_string(),
                    duration: dur.clone(),
                    compiled: Some(compiled),
                });
                return Ok(out);
            }
        } else {
            return Ok(out);
        }
    }
    if let Some((subj_raw, rest)) = s_nodur
        .split_once(" gains ")
        .or_else(|| s_nodur.split_once(" gain "))
        .or_else(|| s_nodur.split_once(" has "))
        .or_else(|| s_nodur.split_once(" have "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let r_trim = rest.trim();
        let r_low = r_trim.to_ascii_lowercase();
        if r_low == "that ability"
            || r_low.starts_with("all activated abilities of ")
            || r_low.starts_with("landwalk of the chosen type")
        {
            return Ok(vec![InstructionAst::GrantKeywords {
                subject: Box::new(subj),
                keywords: vec![r_low],
                duration: dur.clone(),
            }]);
        }
        if let Some(q_start) = r_trim.find('"')
            && let Some(q_end) = r_trim.rfind('"')
            && q_end > q_start
        {
            let q_text = &r_trim[q_start + 1..q_end];
            if let Some(compiled) = compile_granted_ability(q_text, vocab) {
                return Ok(vec![InstructionAst::GrantAbilityText {
                    subject: Box::new(subj),
                    ability_text: q_text.to_string(),
                    duration: dur.clone(),
                    compiled: Some(compiled),
                }]);
            }
        }
    }
    if let Some((subj_raw, rest)) = s_nodur
        .split_once(" becomes a ")
        .or_else(|| s_nodur.split_once(" becomes an "))
        .or_else(|| s_nodur.split_once(" become a "))
        .or_else(|| s_nodur.split_once(" become an "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && (rest.contains(" named ") || rest.contains('"'))
    {
        let mut out = vec![InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(subj.clone()),
            power: String::new(),
            toughness: String::new(),
            description: Some(rest.trim().to_string()),
            keywords: Vec::new(),
            duration: dur.clone(),
        }];
        if let Some(q_start) = rest.find('"')
            && let Some(q_end) = rest.rfind('"')
            && q_end > q_start
        {
            let q_text = &rest[q_start + 1..q_end];
            if let Some(compiled) = compile_granted_ability(q_text, vocab) {
                out.push(InstructionAst::GrantAbilityText {
                    subject: Box::new(subj),
                    ability_text: q_text.to_string(),
                    duration: dur.clone(),
                    compiled: Some(compiled),
                });
                return Ok(out);
            }
        } else {
            return Ok(out);
        }
    }
    let s_nodur_low_w16 = s_nodur.to_ascii_lowercase();
    if let Some(subj_raw) = s_nodur_low_w16
        .strip_suffix(" becomes saddled")
        .or_else(|| s_nodur_low_w16.strip_suffix(" become saddled"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "becomes_saddled".to_string(),
            duration: dur.clone(),
        }]);
    }
    // General "Only <filter> can attack/block [during <dur>]"
    if let Some(after_only) = s_nodur_low_w16.strip_prefix("only ")
        && let Some((flt_raw, verb)) = after_only
            .strip_suffix(" can attack")
            .map(|f| (f, "only_can_attack"))
            .or_else(|| {
                after_only
                    .strip_suffix(" can block")
                    .map(|f| (f, "only_can_block"))
            })
        && let Ok(subj) = parse_subject_or_target(flt_raw.trim(), vocab)
    {
        return Ok(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: verb.to_string(),
            duration: dur.clone(),
        }]);
    }
    // General "<subj> enters with <val> <kind> counters on it" (exact count; rounded forms fail closed unless parsed as DivConst)
    if let Some((subj_raw, rest)) = low.split_once(" enters with ")
        && !rest.ends_with(", rounded down")
        && !rest.ends_with(", rounded up")
        && let Some(cnt_kind) = rest
            .strip_suffix(" counters on it")
            .or_else(|| rest.strip_suffix(" counter on it"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let (cnt_raw, kind) = cnt_kind
            .rsplit_once(' ')
            .map_or((cnt_kind, "counter"), |(a, b)| (a.trim(), b.trim()));
        if let Ok(cnt) = parse_value_expr(cnt_raw, vocab) {
            return Ok(vec![InstructionAst::PutCounters {
                subject: Box::new(subj),
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
            }]);
        }
    }
    // General "look at a card at random in <player>'s hand" / "look at <filter>"
    if let Some(after_look) = low.strip_prefix("look at ") {
        if let Some(who_raw) = after_look
            .strip_prefix("a card at random in ")
            .and_then(|r| r.strip_suffix("'s hand"))
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Ok(vec![InstructionAst::RevealHand { player }]);
        }
        if let Ok(flt) = parse_object_filter(after_look, vocab) {
            return Ok(vec![InstructionAst::RevealFromLooked {
                filter: Box::new(flt),
                to: "look".to_string(),
            }]);
        }
    }
    // General "<subj> deals <val> damage to <tgt1> and <tgt2>"
    if let Some((src_raw, rest)) = low.split_once(" deals ")
        && let Some((amt_raw, tgts_raw)) = rest.split_once(" damage to ")
        && let Some((t1_raw, t2_raw)) = tgts_raw.split_once(" and ")
        && let (Ok(src), Ok(amt), Ok(t1), Ok(t2)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(t1_raw.trim(), vocab),
            parse_subject_or_target(t2_raw.trim(), vocab),
        )
    {
        return Ok(vec![
            InstructionAst::DealDamage {
                source: Box::new(src.clone()),
                target: Box::new(t1),
                amount: Box::new(amt.clone()),
            },
            InstructionAst::DealDamage {
                source: Box::new(src),
                target: Box::new(t2),
                amount: Box::new(amt),
            },
        ]);
    }
    // General "put <subj> into <player>'s hand / on top of <player>'s library / on the bottom of <player>'s library / onto the battlefield"
    if let Some((who, after_put)) = split_leading_player_verb(s, "put", "puts") {
        let ap_low = after_put.to_ascii_lowercase();
        let ap_clean = ap_low
            .strip_suffix(" in any order")
            .unwrap_or(&ap_low)
            .trim();
        for (dest_pat, dest_name) in [
            (" into your hand", "hand"),
            (" into their hand", "hand"),
            (" into that player's hand", "hand"),
            (" on top of your library", "top_of_library"),
            (" on top of their library", "top_of_library"),
            (" on top of their owners' libraries", "top_of_library"),
            (" on the bottom of your library", "bottom_of_library"),
            (" on the bottom of their library", "bottom_of_library"),
            (" onto the battlefield", "battlefield"),
        ] {
            if let Some(obj_raw) = ap_clean.strip_suffix(dest_pat) {
                let obj_clean = obj_raw.trim();
                if !obj_clean.starts_with("any number of ")
                    && !obj_clean.starts_with("all ")
                    && let Ok(subj) = parse_subject_or_target(obj_clean, vocab)
                {
                    let _ = who;
                    return Ok(vec![InstructionAst::MoveZone {
                        subject: Box::new(subj),
                        to: dest_name.to_string(),
                    }]);
                }
            }
        }
    }
    // General "do the same for <filter>" / "repeat the following process <val> times" / "clash with <player>"
    if let Some(flt_raw) = low.strip_prefix("do the same for ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(vec![InstructionAst::AbilityRestriction {
            restriction: low,
            condition: None,
        }]);
    }
    if let Some(val_raw) = low
        .strip_prefix("repeat the following process ")
        .and_then(|r| r.strip_suffix(" times"))
        && let Ok(cnt) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![InstructionAst::ForEachCount {
            count: Box::new(cnt),
            body: vec![InstructionAst::AbilityRestriction {
                restriction: "repeat_following_process".to_string(),
                condition: None,
            }],
        }]);
    }
    if let Some(who_raw) = low.strip_prefix("clash with ")
        && parse_player_ref(who_raw).is_some()
    {
        return Ok(vec![InstructionAst::AbilityRestriction {
            restriction: low,
            condition: None,
        }]);
    }
    // General "<player> chooses <subj_or_filter>"
    if let Some((who, after_ch)) = split_leading_player_verb(s, "choose", "chooses") {
        let ac_low = after_ch.to_ascii_lowercase();
        let ac_clean = ac_low.strip_prefix("from it ").unwrap_or(&ac_low).trim();
        if parse_subject_or_target(ac_clean, vocab).is_ok()
            || ac_clean.split_once(" or ").is_some_and(|(a, b)| {
                parse_subject_or_target(a.trim(), vocab).is_ok()
                    && parse_subject_or_target(b.trim(), vocab).is_ok()
            })
        {
            return Ok(vec![InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser: who,
                    description: after_ch.to_string(),
                },
            )]);
        }
    }
    // General multi-symbol mana list: "Add {W}{W}, {U}{U}, {B}{B}, {R}{R}, and {G}{G}"
    if let Some(after_add) = s.strip_prefix("Add ").or_else(|| s.strip_prefix("add "))
        && after_add.starts_with('{')
        && after_add.contains(", and ")
    {
        let joined = after_add.replace(", and ", "").replace(", ", "");
        if is_mana_sequence(&joined) {
            return Ok(vec![InstructionAst::AddMana {
                player: PlayerRefAst::You,
                mana: crate::ast::ManaSpecAst::Fixed(joined),
            }]);
        }
    }

    // Compositional Wave 17: Out-of-line recursive combinators (keeps LLVM fast + handles remaining general effect compositions)
    if let Some(w17) = parse_wave17_effects(s, &low, s_nodur, &dur, vocab) {
        return Ok(w17);
    }

    // Universal trailing "<effect> if <cond>" (Imoen, Tevesh Szat, Caravan Vigil, Alacrian Armory)
    if !low.starts_with("if ")
        && let Some(if_idx) = find_top_level_pattern(s, " if ")
    {
        let eff_part = s[..if_idx].trim();
        let cond_part = s[if_idx + 4..].trim();
        if !eff_part.is_empty()
            && !cond_part.is_empty()
            && let Ok(cond) = parse_condition(cond_part, vocab)
            && let Ok(then_body) = parse_atomic_effect(eff_part, vocab)
        {
            return Ok(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body,
            }]);
        }
    }

    // Compositional Wave 14: Generalized top-level binary ", and " / " and " combinator outside quotes,
    // supporting atomic effects, TemporaryStatic, TemporaryReplacement, and inline TemporaryTrigger on either side!
    // (Giant Oyster, Sworn Defender, Walk-In Closet, Abstruse Appropriation, Gogo, Pygmy Hippo, Tenth District Hero)
    if s.len() <= 420
        && !(low.starts_with("if ")
            || low.starts_with("when ")
            || low.starts_with("whenever ")
            || low.starts_with("at the ")
            || low.starts_with("for as long as ")
            || low.starts_with("as long as "))
    {
        let sep_candidates = collect_top_level_and_splits(s);
        for (split_pos, sep_len) in sep_candidates {
            let left_s = s[..split_pos].trim();
            let right_s = s[split_pos + sep_len..].trim();
            if left_s.is_empty() || right_s.is_empty() {
                continue;
            }
            let left_parsed = parse_effect_or_temp_clause(left_s, &dur, vocab);
            if let Some(mut left_vec) = left_parsed {
                if let Some(mut right_vec) = parse_effect_or_temp_clause(right_s, &dur, vocab) {
                    left_vec.append(&mut right_vec);
                    return Ok(left_vec);
                }
                if let Some(subj_prefix) = extract_leading_subject_prefix(left_s)
                    && let Some(mut right_vec) = parse_effect_or_temp_clause(
                        &format!("{subj_prefix} {right_s}"),
                        &dur,
                        vocab,
                    )
                {
                    left_vec.append(&mut right_vec);
                    return Ok(left_vec);
                }
            }
        }
    }

    // Tabletop Honor-System physical/social effect action
    if is_physical_tabletop_clause(&low) {
        return Ok(vec![InstructionAst::ManualTabletopAction {
            prompt: s.to_string(),
        }]);
    }

    Err(format!("unparsed_effect:{s}"))
}

fn try_parse_inline_trigger(
    s: &str,
    duration: DurationAst,
    vocab: &Vocabulary,
) -> Option<InstructionAst> {
    let low = s.to_ascii_lowercase();
    if !(low.starts_with("when ")
        || low.starts_with("whenever ")
        || low.starts_with("at the beginning of ")
        || low.starts_with("at end of "))
    {
        return None;
    }
    for (cidx, _) in s.match_indices(", ") {
        let hdr = s[..cidx].trim();
        let body_raw = s[cidx + 2..].trim();
        if hdr.is_empty() || body_raw.is_empty() {
            continue;
        }
        if let Ok(trigger) = parse_trigger_header(hdr, vocab)
            && let Ok(body) = parse_atomic_effect(body_raw, vocab)
        {
            return Some(InstructionAst::TemporaryTrigger {
                trigger,
                body,
                duration,
            });
        }
    }
    None
}

fn parse_effect_or_temp_clause(
    clause: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    if let Ok(v) = parse_atomic_effect(clause, vocab)
        && !v.is_empty()
    {
        return Some(v);
    }
    if let Some(trig) = try_parse_inline_trigger(clause, dur.clone(), vocab) {
        return Some(vec![trig]);
    }
    if clause.len() <= 260
        && let Ok(sr) = parse_static_or_replacement(clause, vocab)
    {
        let eff_dur = if *dur == DurationAst::Instant {
            DurationAst::UntilEndOfTurn
        } else {
            dur.clone()
        };
        return Some(vec![match sr {
            StaticOrReplacement::Static(st) => InstructionAst::TemporaryStatic {
                static_ability: Box::new(st),
                duration: eff_dur,
            },
            StaticOrReplacement::Replacement(rep) => InstructionAst::TemporaryReplacement {
                replacement: Box::new(rep),
                duration: eff_dur,
            },
        }]);
    }
    None
}

fn find_top_level_pattern(s: &str, pat: &str) -> Option<usize> {
    let low = s.to_ascii_lowercase();
    let mut in_quotes = false;
    let bytes = low.as_bytes();
    let pbytes = pat.as_bytes();
    if pbytes.is_empty() || bytes.len() < pbytes.len() {
        return None;
    }
    for i in 0..=bytes.len() - pbytes.len() {
        if bytes[i] == b'"' {
            in_quotes = !in_quotes;
        }
        if !in_quotes && &bytes[i..i + pbytes.len()] == pbytes {
            return Some(i);
        }
    }
    None
}

fn collect_top_level_and_splits(s: &str) -> Vec<(usize, usize)> {
    let low = s.to_ascii_lowercase();
    let bytes = low.as_bytes();
    let mut in_quotes = false;
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            in_quotes = !in_quotes;
        }
        if !in_quotes {
            if bytes[i..].starts_with(b", and ") {
                out.push((i, 6));
                i += 6;
                continue;
            }
            if bytes[i..].starts_with(b" and ") {
                let after = &low[i + 5..];
                if after.starts_with("at the beginning of ")
                    || after.starts_with("when ")
                    || after.starts_with("whenever ")
                    || after.starts_with("if ")
                    || after.starts_with("attack ")
                    || after.starts_with("attacks ")
                    || after.starts_with("you ")
                    || after.starts_with("each ")
                    || after.starts_with("target ")
                    || after.starts_with("it ")
                    || after.starts_with("they ")
                    || after.starts_with("that ")
                {
                    out.push((i, 5));
                    i += 5;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

fn split_leading_player_verb<'a>(
    s: &'a str,
    verb_base: &str,
    verb_3p: &str,
) -> Option<(PlayerRefAst, &'a str)> {
    let low = s.to_ascii_lowercase();
    if let Some(rest) = low.strip_prefix(&format!("{verb_base} ")) {
        let _ = rest;
        return Some((PlayerRefAst::You, s[verb_base.len() + 1..].trim()));
    }
    if let Some(rest) = low.strip_prefix(&format!("{verb_3p} ")) {
        let _ = rest;
        return Some((PlayerRefAst::You, s[verb_3p.len() + 1..].trim()));
    }
    for (needle_len, needle) in [
        (verb_3p.len() + 2, format!(" {verb_3p} ")),
        (verb_base.len() + 2, format!(" {verb_base} ")),
        (verb_base.len() + 7, format!(" each {verb_base} ")),
    ] {
        if let Some(idx) = low.find(&needle)
            && let Some(p) = parse_player_ref(&s[..idx])
        {
            return Some((p, s[idx + needle_len..].trim()));
        }
    }
    None
}

/// Extract the leading subject phrase from a `<subj> <verb> ...` clause for shared-subject carry-forward.
fn extract_leading_subject_prefix(clause: &str) -> Option<&str> {
    let low = clause.to_ascii_lowercase();
    for needle in [
        " looks ",
        " look ",
        " gets ",
        " get ",
        " gains ",
        " gain ",
        " loses ",
        " lose ",
        " can't ",
        " can ",
        " deals ",
        " deal ",
        " becomes ",
        " become ",
        " has ",
        " have ",
        " doesn't ",
        " don't ",
        " is ",
        " are ",
        " isn't ",
        " aren't ",
        " sacrifices ",
        " sacrifice ",
        " draws ",
        " draw ",
        " discards ",
        " discard ",
        " reveals ",
        " reveal ",
        " creates ",
        " create ",
        " puts ",
        " put ",
        " exiles ",
        " exile ",
        " chooses ",
        " choose ",
        " shuffles ",
        " shuffle ",
        " attacks ",
        " attack ",
        " blocks ",
        " block ",
        " returns ",
        " return ",
        " searches ",
        " search ",
        " mills ",
        " mill ",
        " scries ",
        " scry ",
        " surveils ",
        " surveil ",
        " destroys ",
        " destroy ",
        " removes ",
        " remove ",
        " moves ",
        " move ",
        " skips ",
        " skip ",
        " flips ",
        " flip ",
        " rolls ",
        " roll ",
        " investigates",
        " investigate",
        " taps ",
        " tap ",
        " untaps ",
        " untap ",
        " copies ",
        " copy ",
        " costs ",
        " cost ",
        " pays ",
        " pay ",
        " plays ",
        " play ",
        " casts ",
        " cast ",
    ] {
        if let Some(idx) = low.find(needle) {
            let prefix = clause[..idx].trim();
            if !prefix.is_empty() {
                return Some(prefix);
            }
        }
    }
    None
}

/// True when a chained clause starts directly with a verb predicate (no explicit leading subject/player).
fn starts_with_predicate_verb(clause: &str) -> bool {
    let low = clause.to_ascii_lowercase();
    [
        "puts ",
        "put ",
        "exiles ",
        "exile ",
        "draws ",
        "draw ",
        "discards ",
        "discard ",
        "sacrifices ",
        "sacrifice ",
        "loses ",
        "lose ",
        "gains ",
        "gain ",
        "mills ",
        "mill ",
        "reveals ",
        "reveal ",
        "looks ",
        "look ",
        "returns ",
        "return ",
        "creates ",
        "create ",
        "taps ",
        "tap ",
        "untaps ",
        "untap ",
        "shuffles",
        "shuffle",
        "scries ",
        "scry ",
        "surveils ",
        "surveil ",
        "chooses ",
        "choose ",
        "searches ",
        "search ",
        "adds ",
        "add ",
        "pays ",
        "pay ",
        "casts ",
        "cast ",
        "plays ",
        "play ",
        "destroys ",
        "destroy ",
        "removes ",
        "remove ",
        "moves ",
        "move ",
        "gets ",
        "get ",
        "deals ",
        "deal ",
        "becomes ",
        "become ",
        "has ",
        "have ",
        "can't ",
        "can ",
        "doesn't ",
        "don't ",
        "is ",
        "are ",
        "isn't ",
        "aren't ",
        "investigates",
        "investigate",
        "skips ",
        "skip ",
        "flips ",
        "flip ",
        "rolls ",
        "roll ",
    ]
    .iter()
    .any(|p| low.starts_with(p))
}

/// Extract leading `[<player>] <action_verb>` prefix for CR 608.2c compound-object distribution
/// (e.g., `"Target opponent exiles a creature they control and their graveyard"` -> `"Target opponent exiles"`).
fn extract_leading_action_verb_prefix(clause: &str) -> Option<&str> {
    let low = clause.to_ascii_lowercase();
    for bare in [
        "exile ",
        "destroy ",
        "tap ",
        "untap ",
        "return ",
        "sacrifice ",
        "reveal ",
        "discard ",
        "put ",
        "create ",
    ] {
        if low.starts_with(bare) {
            return Some(clause[..bare.len() - 1].trim());
        }
    }
    for verb in [
        " exiles ",
        " exile ",
        " destroys ",
        " destroy ",
        " taps ",
        " tap ",
        " untaps ",
        " untap ",
        " returns ",
        " return ",
        " sacrifices ",
        " sacrifice ",
        " reveals ",
        " reveal ",
        " discards ",
        " discard ",
        " puts ",
        " put ",
        " creates ",
        " create ",
    ] {
        if let Some(idx) = low.find(verb) {
            let prefix = clause[..idx + verb.len() - 1].trim();
            if !prefix.is_empty() {
                return Some(prefix);
            }
        }
    }
    None
}

fn try_parse_pass_c_primitives(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. ControlPlayerTurn (Mindslaver, Emrakul the Promised End, Worst Fears, Sorin Markov, Secret of Bloodbending)
    for pref in ["you control ", "you gain control of ", "control "] {
        if let Some(after) = low.strip_prefix(pref)
            && let Some((who_raw, _)) = after
                .split_once(" during that player's next turn")
                .or_else(|| after.split_once(" during their next turn"))
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Some(vec![InstructionAst::ControlPlayerTurn {
                player,
                duration: DurationAst::UntilEndOfNextTurn,
            }]);
        }
    }

    // 2. Exchange (Control, LifeTotals, LifeWithStat, Zones: Volatile Stormdrake, Legerdemain, Shifting Loyalties, Axis of Mortality, Tree of Perdition/Redemption)
    if let Some(after) = low.strip_prefix("exchange control of ") {
        if let Some((l_raw, r_raw)) = after.split_once(" and ")
            && let (Ok(left), Ok(right)) = (
                parse_subject_or_target(l_raw, vocab),
                parse_subject_or_target(r_raw, vocab),
            )
        {
            return Some(vec![InstructionAst::Exchange {
                kind: ExchangeKind::Control,
                left: Box::new(left),
                right: Some(Box::new(right)),
            }]);
        }
        if let Ok(left) = parse_subject_or_target(after, vocab) {
            return Some(vec![InstructionAst::Exchange {
                kind: ExchangeKind::Control,
                left: Box::new(left),
                right: None,
            }]);
        }
    }
    if let Some((who_raw, _)) = low.split_once(" exchange life totals")
        && let Ok(left) = parse_subject_or_target(who_raw, vocab)
    {
        return Some(vec![InstructionAst::Exchange {
            kind: ExchangeKind::LifeTotals,
            left: Box::new(left),
            right: None,
        }]);
    }
    if let Some(after) = low.strip_prefix("exchange ")
        && let Some((l_raw, r_raw)) = after
            .split_once(" life total with ")
            .or_else(|| after.split_once(" life total and "))
    {
        let l_clean = l_raw.trim_end_matches("'s");
        let left_subj = if l_clean == "your" {
            SubjectRefAst::Player(PlayerRefAst::You)
        } else {
            let p = parse_player_ref(l_clean)?;
            SubjectRefAst::Player(p)
        };
        for (stat_suf, stat) in [
            ("'s toughness", StatKind::Toughness),
            ("'s power", StatKind::Power),
        ] {
            if let Some(r_subj_raw) = r_raw.strip_suffix(stat_suf)
                && let Ok(right) = parse_subject_or_target(r_subj_raw, vocab)
            {
                return Some(vec![InstructionAst::Exchange {
                    kind: ExchangeKind::LifeWithStat(stat),
                    left: Box::new(left_subj),
                    right: Some(Box::new(right)),
                }]);
            }
        }
        if let Some(r_who) = r_raw.strip_suffix("'s life total")
            && let Some(rp) = parse_player_ref(r_who)
        {
            return Some(vec![InstructionAst::Exchange {
                kind: ExchangeKind::LifeTotals,
                left: Box::new(left_subj),
                right: Some(Box::new(SubjectRefAst::Player(rp))),
            }]);
        }
    }

    // 3. WishFromOutsideGame (Burning Wish, Glittering Wish, Vivien Arkbow Ranger, Legion Angel, Coax from the Blind Eternities)
    if let Some(after) = low.strip_prefix("reveal ")
        && after.contains("from outside the game")
    {
        let allow_exile = after.contains("in exile");
        let to_zone = if after.contains("put it into your hand")
            || after.contains("put them into your hand")
        {
            "your hand"
        } else {
            "revealed"
        };
        let flt_side = after
            .split_once(" you own from outside the game")
            .or_else(|| after.split_once(" from outside the game"))
            .map(|(l, _)| l)?;
        let flt_clean = flt_side
            .strip_prefix("exactly ")
            .or_else(|| flt_side.strip_prefix("up to "))
            .unwrap_or(flt_side);
        let (cnt, flt_raw) = peel_leading_count(flt_clean, vocab);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Some(vec![InstructionAst::WishFromOutsideGame {
                filter: Box::new(flt),
                count: Box::new(cnt),
                to_zone: to_zone.to_string(),
                allow_exile,
            }]);
        }
    }

    // 4. Equalize (Balance, Restore Balance, Balancing Act, Magus of the Balance)
    if let Some((who_raw, rest)) = low.split_once(" chooses a number of ")
        && let Some(player) = parse_player_ref(who_raw)
        && let Some((flt_raw, tail)) = rest.split_once(" they control equal to the number of ")
        && tail.contains("fewest")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        let action = if tail.contains("sacrifices the rest") {
            "sacrifice_rest"
        } else if tail.contains("discards the rest") {
            "discard_rest"
        } else {
            "move_rest"
        };
        return Some(vec![InstructionAst::Equalize {
            player,
            filter: Box::new(flt),
            action: action.to_string(),
        }]);
    }

    // 5. InteractiveChoice: Villainous choice, Secret vote, Secret number, Bidding, Piles, Group choose
    if let Some((who_raw, rest)) = s
        .split_once(" faces a villainous choice \u{2014} ")
        .or_else(|| s.split_once(" face a villainous choice \u{2014} "))
        && let Some(chooser) = parse_player_ref(who_raw)
        && let Some((opt1, opt2)) = rest.split_once(", or ")
    {
        let (b1, f1) = parse_effect_body(opt1.trim(), vocab);
        let (b2, f2) = parse_effect_body(opt2.trim(), vocab);
        if f1.is_empty() && f2.is_empty() && !b1.is_empty() && !b2.is_empty() {
            return Some(vec![InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::VillainousChoice {
                    chooser,
                    options: vec![b1, b2],
                },
            )]);
        }
    }
    if low.contains("secretly votes for ") {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::SecretVote {
                prompt: s.to_string(),
            },
        )]);
    }
    if low.starts_with("each player secretly chooses a number ") {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::SecretNumberChoice { min_value: 0 },
        )]);
    }
    if low == "in turn order, each player may top the high bid"
        || low == "the bidding ends if the high bid stands"
        || low.starts_with("each player secretly chooses ")
    {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::BiddingAction {
                description: s.to_string(),
            },
        )]);
    }
    if let Some(after) = low.strip_prefix("separate ")
        && let Some(subj_raw) = after.strip_suffix(" into two piles")
        && let Ok(subject) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::SeparateIntoPiles {
                chooser: PlayerRefAst::You,
                subject: Box::new(subject),
                pile_count: 2,
            },
        )]);
    }
    if matches!(
        low,
        "you choose one of those cards"
            | "choose one of those cards"
            | "choose a creature card and/or a land card from among them"
    ) {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::ChooseFromGroup {
                chooser: PlayerRefAst::You,
                description: low.to_string(),
            },
        )]);
    }
    if let Some(after) = low.strip_prefix("choose ")
        && !after.contains('\u{2014}')
        && (parse_object_filter(after, vocab).is_ok()
            || parse_subject_or_target(after, vocab).is_ok()
            || after.starts_with("a number between "))
    {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::ChooseFromGroup {
                chooser: PlayerRefAst::You,
                description: after.to_string(),
            },
        )]);
    }
    if low == "it becomes night" || low == "it becomes day" {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::SelfObject),
            rule: low.to_string(),
            duration: DurationAst::Permanent,
        }]);
    }

    // 6. DistributeCounters (Towashi, Ajani, etc.)
    if let Some(after) = low.strip_prefix("distribute ")
        && let Some((left, tgts_raw)) = after
            .split_once(" counters among ")
            .or_else(|| after.split_once(" counter among "))
        && let Ok(targets) = parse_subject_or_target(tgts_raw, vocab)
    {
        let (cnt, kind) = peel_leading_count(left, vocab);
        return Some(vec![InstructionAst::DistributeCounters {
            counter_kind: kind.to_string(),
            count: Box::new(cnt),
            targets: Box::new(targets),
        }]);
    }

    // 7. RepeatProcess (Grist, Sphinx's Tutelage, Claim Jumper, Forbidden Ritual, Sin)
    if matches!(
        low,
        "repeat this process"
            | "repeat this process once"
            | "repeat this process any number of times"
    ) {
        return Some(vec![InstructionAst::RepeatProcess {
            condition: None,
            max_times: low.ends_with(" once").then_some(1),
            any_number: low.contains("any number"),
        }]);
    }

    // 8. LookAtExiled ("look at [and play] <subj> for as long as <cond>")
    if let Some(after) = low.strip_prefix("look at ")
        && let Some((subj_part, cond_raw)) = after.split_once(" for as long as ")
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        let (also_play, subj_raw) = if let Some(sr) = subj_part.strip_prefix("and play ") {
            (true, sr)
        } else if let Some(sr) = subj_part.strip_prefix("and cast ") {
            (true, sr)
        } else {
            (false, subj_part)
        };
        if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
            return Some(vec![InstructionAst::LookAtExiled {
                subject: Box::new(subj),
                also_play,
                condition: Box::new(cond),
            }]);
        }
    }

    // 9. ManaSpendRestriction ("this mana can't be spent to cast ..." / "spend this mana only ...")
    if low.starts_with("this mana can't be spent to ") || low.starts_with("spend this mana only ") {
        return Some(vec![InstructionAst::ManaSpendRestriction(s.to_string())]);
    }

    // 10. Convert (Transformers alias for Transform) + "discard all the cards in your/their hand"
    // + "<player> can't cast <filter> during that player's next turn"
    if let Some(subj_raw) = low
        .strip_prefix("convert ")
        .or_else(|| low.strip_prefix("you convert "))
    {
        let (core, delayed) = match subj_raw.strip_suffix(" at end of combat") {
            Some(c) => (c, Some(TurnStepAst::EndOfCombat)),
            None => (subj_raw, None),
        };
        if let Ok(subj) = parse_subject_or_target(core, vocab) {
            let ins = InstructionAst::Transform {
                subject: Box::new(subj),
            };
            return Some(match delayed {
                Some(step) => vec![InstructionAst::DelayedTrigger {
                    step,
                    scope: DelayedStepScope::Next,
                    body: vec![ins],
                }],
                None => vec![ins],
            });
        }
    }
    if low == "discard all the cards in your hand" {
        return Some(vec![InstructionAst::DiscardHand {
            player: PlayerRefAst::You,
        }]);
    }
    if let Some((who_raw, rest)) = low
        .split_once(" discards all the cards in ")
        .or_else(|| low.split_once(" discard all the cards in "))
        && rest.ends_with(" hand")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(vec![InstructionAst::DiscardHand { player }]);
    }
    if let Some((who_raw, rest)) = low.split_once(" can't cast ")
        && let Some((flt_raw, _)) = rest
            .split_once(" during that player's next turn")
            .or_else(|| rest.split_once(" during their next turn"))
        && let (Some(player), Ok(_flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Player(player)),
            rule: format!("cant_cast:{flt_raw}"),
            duration: DurationAst::UntilEndOfNextTurn,
        }]);
    }

    None
}

fn try_parse_cast_permission(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    let (player, rest) = if let Some(idx) = low.find(" may play ") {
        (parse_player_ref(&low[..idx])?, (&low[idx + 10..], false))
    } else if let Some(idx) = low.find(" may cast ") {
        (parse_player_ref(&low[..idx])?, (&low[idx + 10..], true))
    } else if let Some(r) = low.strip_prefix("play ") {
        (PlayerRefAst::You, (r, false))
    } else {
        let r = low.strip_prefix("cast ")?;
        (PlayerRefAst::You, (r, true))
    };

    let (body, cast_only) = rest;
    // Strip trailing mana-fixing clause: ", and mana of any type can be spent to cast ..." / ", and you may spend mana as though it were mana of any color ..."
    let body = body
        .split_once(", and mana of any type can be spent")
        .or_else(|| body.split_once(", and you may spend mana as though it were mana of any color"))
        .or_else(|| body.split_once(" and you may spend mana as though it were mana of any color"))
        .map_or(body, |(before, _)| before.trim());
    let free = body.contains("without paying its mana cost")
        || body.contains("without paying their mana costs");
    let clean = body
        .replace(" without paying its mana cost", "")
        .replace(" without paying their mana costs", "")
        .replace(" face down as a 2/2 creature spell", "")
        .replace(" this turn as though they had flash", "")
        .replace(" as though they had flash", "");
    // Strip optional "by paying <cost> in addition to its other costs"
    let clean = match clean.split_once(" by paying ") {
        Some((before, _)) => before.to_string(),
        None => clean,
    };
    let (obj_raw, dur) = if let Some(o) = clean
        .strip_suffix(" until the end of your next turn")
        .or_else(|| clean.strip_suffix(" until the end of their next turn"))
        .or_else(|| clean.strip_suffix(" until end of combat on your next turn"))
    {
        (o, "until_end_of_next_turn")
    } else if let Some(o) = clean
        .strip_suffix(" until your next end step")
        .or_else(|| clean.strip_suffix(" until the beginning of your next upkeep"))
    {
        (o, "until_your_next_step")
    } else if let Some(o) = clean.strip_suffix(" until end of turn") {
        (o, "until_end_of_turn")
    } else if let Some(o) = clean.strip_suffix(" this turn") {
        (o, "this_turn")
    } else if let Some(o) = clean
        .strip_suffix(" for as long as it remains exiled")
        .or_else(|| clean.strip_suffix(" for as long as they remain exiled"))
    {
        (o, "while_exiled")
    } else if let Some(o) = clean
        .strip_suffix(" for as long as you control ~")
        .or_else(|| clean.strip_suffix(" for as long as ~ remains on the battlefield"))
        .or_else(|| clean.strip_suffix(" for as long as ~ remains tapped"))
    {
        (o, "while_control_self")
    } else if let Some(o) = clean.strip_suffix(" until you exile another card with ~") {
        (o, "until_next_exile")
    } else {
        (clean.as_str(), "now")
    };
    let obj_nozone = obj_raw
        .trim()
        .strip_suffix(" from among cards you own exiled with ~")
        .or_else(|| obj_raw.trim().strip_suffix(" from your graveyard"))
        .or_else(|| obj_raw.trim().strip_suffix(" from their graveyard"))
        .or_else(|| {
            obj_raw
                .trim()
                .strip_suffix(" from defending player's graveyard")
        })
        .or_else(|| obj_raw.trim().strip_suffix(" from any graveyard"))
        .or_else(|| obj_raw.trim().strip_suffix(" from a graveyard"))
        .or_else(|| obj_raw.trim().strip_suffix(" from your hand"))
        .or_else(|| obj_raw.trim().strip_suffix(" from exile"))
        .unwrap_or(obj_raw.trim());
    let obj_trim = obj_nozone
        .trim()
        .strip_prefix("any number of ")
        .or_else(|| obj_nozone.trim().strip_prefix("one of "))
        .unwrap_or(obj_nozone.trim());
    if obj_trim == "~"
        || obj_trim.starts_with("~ as though it had flash")
        || obj_trim == "~ from your graveyard"
        || obj_trim == "~ from exile"
    {
        return Some(InstructionAst::GrantCastPermission {
            player,
            cast_only,
            subject: Box::new(SubjectRefAst::SelfObject),
            free,
            duration: dur.to_string(),
        });
    }
    if matches!(
        obj_trim,
        "that card"
            | "those cards"
            | "those two cards"
            | "it"
            | "them"
            | "the copy"
            | "the copies"
            | "the exiled card"
            | "cards exiled with ~"
            | "cards they exiled this way"
            | "the card they exiled this way"
            | "card they exiled this way"
            | "the cards they exiled this way"
    ) {
        return Some(InstructionAst::GrantCastPermission {
            player,
            cast_only,
            subject: Box::new(SubjectRefAst::Anaphora(obj_trim.to_string())),
            free,
            duration: dur.to_string(),
        });
    }
    let flt_candidate = obj_trim
        .strip_suffix(" from among them")
        .or_else(|| obj_trim.strip_suffix(" from among those cards"))
        .or_else(|| obj_trim.strip_suffix(" from among those exiled cards"))
        .or_else(|| obj_trim.strip_suffix(" from among the exiled cards"))
        .or_else(|| obj_trim.strip_suffix(" from among cards exiled this way"))
        .or_else(|| obj_trim.strip_suffix(" from among cards exiled with ~"))
        .or_else(|| obj_trim.strip_suffix(" from among the copies"))
        .unwrap_or(obj_trim);
    if let Ok(subj) = parse_subject_or_target(flt_candidate, vocab) {
        return Some(InstructionAst::GrantCastPermission {
            player,
            cast_only,
            subject: Box::new(subj),
            free,
            duration: dur.to_string(),
        });
    }
    None
}

fn try_parse_deal_damage(s: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    let low = s.to_ascii_lowercase();
    let (idx, verb_len) = if let Some(i) = low.find(" deals ") {
        (i, 7)
    } else if let Some(i) = low.find(" each deal ") {
        (i, 11)
    } else {
        let i = low.find(" deal ")?;
        (i, 6)
    };
    let src = parse_subject_or_target(&s[..idx], vocab).ok()?;
    let after = s[idx + verb_len..].trim();
    let alow = after.to_ascii_lowercase();

    // Compound: "<amt1> damage to <t1> and <amt2> damage to <t2>"
    if let Some((left, right)) = alow.split_once(" and ")
        && left.contains(" damage to ")
        && right.contains(" damage to ")
        && let (Some(a), Some(b)) = (
            try_parse_deal_damage(&format!("~ deals {left}"), vocab),
            try_parse_deal_damage(&format!("~ deals {right}"), vocab),
        )
    {
        let mut combined = Vec::with_capacity(a.len() + b.len());
        for ins in a.into_iter().chain(b) {
            if let InstructionAst::DealDamage { target, amount, .. } = ins {
                combined.push(InstructionAst::DealDamage {
                    source: Box::new(src.clone()),
                    target,
                    amount,
                });
            }
        }
        return Some(combined);
    }

    // "damage equal to <val> divided as you choose among <targets>" (Living Inferno, Captain America, Orca)
    if let Some(rest) = alow.strip_prefix("damage equal to ")
        && let Some((val_raw, targets_desc)) = rest.split_once(" divided as you choose among ")
        && let Ok(amt) = parse_value_expr(val_raw, vocab)
    {
        return Some(vec![InstructionAst::DealDamageDivided {
            source: Box::new(src),
            amount: Box::new(amt),
            targets_desc: targets_desc.trim().to_string(),
        }]);
    }
    // "damage equal to <val> to <t1> and that much damage to <t2>" (Dragonspark Reactor)
    if let Some(rest) = alow.strip_prefix("damage equal to ")
        && let Some((left_part, t2_raw)) = rest.split_once(" and that much damage to ")
        && let Some((val_raw, t1_raw)) = left_part.split_once(" to ")
        && let (Ok(amt), Ok(t1), Ok(t2)) = (
            parse_value_expr(val_raw, vocab),
            parse_subject_or_target(t1_raw, vocab),
            parse_subject_or_target(t2_raw, vocab),
        )
    {
        return Some(vec![
            InstructionAst::DealDamage {
                source: Box::new(src.clone()),
                target: Box::new(t1),
                amount: Box::new(amt.clone()),
            },
            InstructionAst::DealDamage {
                source: Box::new(src),
                target: Box::new(t2),
                amount: Box::new(amt),
            },
        ]);
    }

    // "damage equal to <val> to <tgt>" (try split_once first, then rsplit_once for "... attached to it to any target", Tetsuo)
    if let Some(rest) = alow.strip_prefix("damage equal to ") {
        for split in [rest.split_once(" to "), rest.rsplit_once(" to ")] {
            if let Some((val_raw, tgt_raw)) = split
                && let (Ok(amt), Ok(tgt)) = (
                    parse_value_expr(val_raw, vocab),
                    parse_subject_or_target(tgt_raw, vocab),
                )
            {
                return Some(vec![InstructionAst::DealDamage {
                    source: Box::new(src),
                    target: Box::new(tgt),
                    amount: Box::new(amt),
                }]);
            }
        }
    }

    // "damage to <tgt> equal to <val>"
    if let Some(rest) = alow.strip_prefix("damage to ")
        && let Some((tgt_raw, val_raw)) = rest.split_once(" equal to ")
        && let Ok(amt) = parse_value_expr(val_raw, vocab)
        && let Ok(tgt) = parse_subject_or_target(tgt_raw, vocab)
    {
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(tgt),
            amount: Box::new(amt),
        }]);
    }
    // "damage equal to <val> to <t1> and <t2>"
    if let Some(rest) = alow.strip_prefix("damage equal to ")
        && let Some((val_raw, tgt_raw)) = rest.split_once(" to ")
        && let Some((t1_raw, t2_raw)) = tgt_raw.split_once(" and ")
        && let (Ok(amt), Ok(t1), Ok(t2)) = (
            parse_value_expr(val_raw, vocab),
            parse_subject_or_target(t1_raw, vocab),
            parse_subject_or_target(t2_raw, vocab),
        )
    {
        return Some(vec![
            InstructionAst::DealDamage {
                source: Box::new(src.clone()),
                target: Box::new(t1),
                amount: Box::new(amt.clone()),
            },
            InstructionAst::DealDamage {
                source: Box::new(src),
                target: Box::new(t2),
                amount: Box::new(amt),
            },
        ]);
    }

    // Divided damage: "<amt> damage divided as you choose among <targets>" or "divided evenly, rounded down, among <targets>"
    if let Some((amt_raw, targets_desc)) = alow
        .split_once(" damage divided as you choose among ")
        .or_else(|| alow.split_once(" damage divided evenly, rounded down, among "))
        .or_else(|| alow.split_once(" damage divided evenly, rounded up, among "))
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        return Some(vec![InstructionAst::DealDamageDivided {
            source: Box::new(src),
            amount: Box::new(amt),
            targets_desc: targets_desc.trim().to_string(),
        }]);
    }

    // "<amt> damage instead" (when preceded by trailing " if <cond>" which was already peeled!)
    if let Some(amt_raw) = alow.strip_suffix(" damage instead")
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        return Some(vec![InstructionAst::ReplaceEffect(vec![
            InstructionAst::DealDamage {
                source: Box::new(src),
                target: Box::new(SubjectRefAst::Anaphora("previous_target".to_string())),
                amount: Box::new(amt),
            },
        ])]);
    }
    // "<amt> damage" (when trailing " instead" was peeled by parse_atomic_effect!)
    if let Some(amt_raw) = alow.strip_suffix(" damage")
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(SubjectRefAst::Anaphora("previous_target".to_string())),
            amount: Box::new(amt),
        }]);
    }

    // "<amt> damage to <tgt> [unless <cond>] [at end of combat] [for each <filter>]"
    if let Some((amt_raw, rest)) = alow.split_once(" damage to ") {
        let mut amt = parse_value_expr(amt_raw, vocab).ok()?;
        let (rest_nounless, unless_cond) = match rest.split_once(" unless ") {
            Some((before, c_raw)) => match parse_condition(c_raw, vocab) {
                Ok(c) => (before, Some(c)),
                Err(_) => (rest, None),
            },
            None => (rest, None),
        };
        let (rest_nodelay, delayed_eoc) = match rest_nounless.strip_suffix(" at end of combat") {
            Some(c) => (c, true),
            None => (rest_nounless, false),
        };
        let (tgt_raw, fe_opt) = match rest_nodelay.split_once(" for each ") {
            Some((t, fe)) => (t, Some(fe)),
            None => (rest_nodelay, None),
        };
        if let Some(fe) = fe_opt {
            let fe_val = parse_for_each_value_expr(fe, vocab).ok()?;
            amt = ValueExprAst::Mul(Box::new(amt), Box::new(fe_val));
        }
        let tgt = parse_subject_or_target(tgt_raw, vocab).ok()?;
        let ins = InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(tgt),
            amount: Box::new(amt),
        };
        let ins = match unless_cond {
            Some(cond) => InstructionAst::Unless {
                body: vec![ins],
                unless_player: None,
                unless_cost: None,
                unless_action: None,
                unless_condition: Some(Box::new(cond)),
            },
            None => ins,
        };
        return Some(if delayed_eoc {
            vec![InstructionAst::DelayedTrigger {
                step: TurnStepAst::EndOfCombat,
                scope: DelayedStepScope::Next,
                body: vec![ins],
            }]
        } else {
            vec![ins]
        });
    }
    None
}

fn try_parse_modify_pt(
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let low = s_nodur.to_ascii_lowercase();
    let (idx, vlen) = if let Some(i) = low.find(" each get an additional ") {
        (i, 24)
    } else if let Some(i) = low.find(" gets an additional ") {
        (i, 20)
    } else if let Some(i) = low.find(" get an additional ") {
        (i, 19)
    } else if let Some(i) = low.find(" each get ") {
        (i, 10)
    } else if let Some(i) = low.find(" gets ") {
        (i, 6)
    } else {
        let i = low.find(" get ")?;
        (i, 5)
    };
    let subj = parse_subject_or_target(&s_nodur[..idx], vocab).ok()?;
    let after = s_nodur[idx + vlen..].trim();
    // "<subj> gets +1/-1 or -1/+1" choice pump (Morphling / Endling)
    if let Some((a_tok, b_tok)) = after.split_once(" or ")
        && let (Some(pa), Some(pb)) = (parse_pt_mod(a_tok.trim()), parse_pt_mod(b_tok.trim()))
    {
        return Some(vec![InstructionAst::ModifyPTChoice {
            subject: Box::new(subj),
            choices: vec![pa, pb],
            duration: dur.clone(),
        }]);
    }
    let (pt_tok, rest) = match after.split_once(' ') {
        Some((p, r)) => (p, r.trim()),
        None => (after, ""),
    };
    let (dx, dy) = parse_pt_mod(pt_tok)?;

    let mut out = Vec::new();
    let rlow = rest.to_ascii_lowercase();
    let mut scale = None;
    let mut kw_part = None;
    let mut ability_text = None;
    let mut extra_combat_rule = None;

    if let Some(fe) = rlow
        .strip_prefix("for each ")
        .or_else(|| rlow.strip_prefix("until end of turn for each "))
    {
        let (fe_core, extra_rule, trailing_gains) =
            if let Some(c) = fe.strip_suffix(" and can't be blocked") {
                (c, Some("can't be blocked"), None)
            } else if let Some((c, g)) = fe
                .split_once(" and gains ")
                .or_else(|| fe.split_once(" and gain "))
            {
                (c, None, Some(g.trim().to_string()))
            } else {
                (fe, None, None)
            };
        scale = Some(Box::new(parse_for_each_value_expr(fe_core, vocab).ok()?));
        extra_combat_rule = extra_rule;
        kw_part = trailing_gains;
    } else if let Some(after_g) = rest
        .strip_prefix("and gains ")
        .or_else(|| rest.strip_prefix("and gain "))
    {
        if after_g.starts_with('"') && after_g.ends_with('"') {
            ability_text = Some(after_g.trim_matches('"').to_string());
        } else if let Some((kw_side, ab_side)) = after_g.split_once(" and \"")
            && let Some(ab_inner) = ab_side.strip_suffix('"')
        {
            kw_part = Some(kw_side.to_ascii_lowercase());
            ability_text = Some(ab_inner.to_string());
        } else if after_g.eq_ignore_ascii_case("all creature types") {
            extra_combat_rule = Some("all creature types");
        } else {
            kw_part = Some(after_g.to_ascii_lowercase());
        }
    } else if rlow == "until end of turn and can't be blocked" || rlow == "and can't be blocked" {
        extra_combat_rule = Some("can't be blocked");
    } else if !rlow.is_empty() {
        return None;
    }

    out.push(InstructionAst::ModifyPT {
        subject: Box::new(subj.clone()),
        dx,
        dy,
        duration: dur.clone(),
        scale,
    });
    if let Some(kp) = kw_part {
        let kws = parse_keyword_list(&kp, vocab)?;
        out.push(InstructionAst::GrantKeywords {
            subject: Box::new(subj.clone()),
            keywords: kws,
            duration: dur.clone(),
        });
    }
    if let Some(ab_txt) = ability_text {
        let compiled = compile_granted_ability(&ab_txt, vocab);
        out.push(InstructionAst::GrantAbilityText {
            subject: Box::new(subj.clone()),
            ability_text: ab_txt,
            compiled,
            duration: dur.clone(),
        });
    }
    if let Some(rule) = extra_combat_rule {
        out.push(InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: rule.to_string(),
            duration: dur.clone(),
        });
    }
    Some(out)
}

fn try_parse_modify_abilities(
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let low = s_nodur.to_ascii_lowercase();
    for (needle, is_gain) in [
        (" each gain ", true),
        (" also gains ", true),
        (" also gain ", true),
        (" gains ", true),
        (" gain ", true),
        (" each lose ", false),
        (" loses ", false),
        (" lose ", false),
    ] {
        if let Some(idx) = low.find(needle) {
            let rhs = s_nodur[idx + needle.len()..].trim();
            let rlow = rhs.to_ascii_lowercase();
            // Word-boundary check so "lifelink" is never mistaken for "life"
            if rlow == "life"
                || rlow.ends_with(" life")
                || rlow.starts_with("life ")
                || rlow.starts_with("control ")
            {
                continue;
            }
            let subj = parse_subject_or_target(&s_nodur[..idx], vocab).ok()?;
            // "<subj> gains that ability" / "<subj> loses \"<ab>\"" (Angelic Skirmisher, Glittering Lion)
            if is_gain && rlow == "that ability" {
                return Some(vec![InstructionAst::GrantAbilityText {
                    subject: Box::new(subj),
                    ability_text: "that ability".to_string(),
                    compiled: None,
                    duration: dur.clone(),
                }]);
            }
            if !is_gain && rhs.starts_with('"') && rhs.ends_with('"') {
                return Some(vec![InstructionAst::RemoveKeywords {
                    subject: Box::new(subj),
                    keywords: vec![rhs.trim_matches('"').to_string()],
                    duration: dur.clone(),
                }]);
            }
            // "<subj> loses all abilities and has/have base power and toughness P/T"
            if !is_gain
                && let Some(pt_raw) = rlow
                    .strip_prefix("all abilities and has base power and toughness ")
                    .or_else(|| {
                        rlow.strip_prefix("all abilities and have base power and toughness ")
                    })
                && let Some((p, t)) = parse_base_pt(pt_raw.trim())
            {
                return Some(vec![
                    InstructionAst::RemoveAllAbilities {
                        subject: Box::new(subj.clone()),
                        duration: dur.clone(),
                    },
                    InstructionAst::AnimateOrSetBasePT {
                        subject: Box::new(subj),
                        power: p,
                        toughness: t,
                        description: None,
                        keywords: Vec::new(),
                        duration: dur.clone(),
                    },
                ]);
            }
            // "<subj> lose(s) <kws> and can't have <kws>" (Arcane Lighthouse, Shadowspear, Bonds of Mortality)
            if !is_gain
                && let Some((lose_part, cant_part)) = rlow.split_once(" and can't have ")
                && let (Some(lkws), Some(_ckws)) = (
                    parse_keyword_list(lose_part, vocab),
                    parse_keyword_list(cant_part, vocab),
                )
            {
                return Some(vec![
                    InstructionAst::RemoveKeywords {
                        subject: Box::new(subj.clone()),
                        keywords: lkws,
                        duration: dur.clone(),
                    },
                    InstructionAst::CombatRuleMod {
                        subject: Box::new(subj),
                        rule: format!("cant_have:{cant_part}"),
                        duration: dur.clone(),
                    },
                ]);
            }
            // "<subj> loses <kw1> and gains <kw2>"
            if !is_gain
                && let Some((lose_part, gain_part)) = rlow
                    .split_once(" and gains ")
                    .or_else(|| rlow.split_once(" and gain "))
                && let (Some(lkws), Some(gkws)) = (
                    parse_keyword_list(lose_part, vocab),
                    parse_keyword_list(gain_part, vocab),
                )
            {
                return Some(vec![
                    InstructionAst::RemoveKeywords {
                        subject: Box::new(subj.clone()),
                        keywords: lkws,
                        duration: dur.clone(),
                    },
                    InstructionAst::GrantKeywords {
                        subject: Box::new(subj),
                        keywords: gkws,
                        duration: dur.clone(),
                    },
                ]);
            }
            // "<subj> gains <kws> and \"<ability>\""
            if is_gain
                && let Some((kw_side, ab_side)) = rhs.split_once(" and \"")
                && let Some(ab_inner) = ab_side.strip_suffix('"')
                && let Some(kws) = parse_keyword_list(kw_side, vocab)
            {
                let compiled = compile_granted_ability(ab_inner, vocab);
                return Some(vec![
                    InstructionAst::GrantKeywords {
                        subject: Box::new(subj.clone()),
                        keywords: kws,
                        duration: dur.clone(),
                    },
                    InstructionAst::GrantAbilityText {
                        subject: Box::new(subj),
                        ability_text: ab_inner.to_string(),
                        compiled,
                        duration: dur.clone(),
                    },
                ]);
            }
            // "<subj> gains <kws> and gets +X/+Y [for each ...]"
            if is_gain
                && let Some((kw_side, pt_side)) = rhs
                    .split_once(" and gets ")
                    .or_else(|| rhs.split_once(" and get "))
                && let Some(kws) = parse_keyword_list(kw_side, vocab)
                && let Some(mut pt_ins) =
                    try_parse_modify_pt(&format!("~ gets {pt_side}"), dur, vocab)
            {
                for ins in &mut pt_ins {
                    if let InstructionAst::ModifyPT { subject, .. } = ins {
                        **subject = subj.clone();
                    }
                }
                let mut out = vec![InstructionAst::GrantKeywords {
                    subject: Box::new(subj),
                    keywords: kws,
                    duration: dur.clone(),
                }];
                out.extend(pt_ins);
                return Some(out);
            }
            if rlow == "all abilities" && !is_gain {
                return Some(vec![InstructionAst::RemoveAllAbilities {
                    subject: Box::new(subj),
                    duration: dur.clone(),
                }]);
            }
            if rlow.starts_with("protection from ") && is_gain {
                return Some(vec![InstructionAst::GrantKeywords {
                    subject: Box::new(subj),
                    keywords: vec![rlow],
                    duration: dur.clone(),
                }]);
            }
            let kw_target = rhs
                .strip_prefix("your choice of ")
                .or_else(|| rhs.strip_prefix("Your choice of "))
                .unwrap_or(rhs);
            if let Some(kws) = parse_keyword_list(kw_target, vocab) {
                return Some(vec![if is_gain {
                    InstructionAst::GrantKeywords {
                        subject: Box::new(subj),
                        keywords: kws,
                        duration: dur.clone(),
                    }
                } else {
                    InstructionAst::RemoveKeywords {
                        subject: Box::new(subj),
                        keywords: kws,
                        duration: dur.clone(),
                    }
                }]);
            }
        }
    }
    None
}

fn try_parse_life(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // DrainLife: "each opponent loses N life and you gain N/that much life"
    if let Some((left, right)) = low.split_once(" life and you gain ")
        && let Some(gain_str) = right.strip_suffix(" life")
        && let Some((who_raw, loss_str)) = left
            .split_once(" loses ")
            .or_else(|| left.split_once(" lose "))
        && let (Some(who), Ok(loss), Ok(gain)) = (
            parse_player_ref(who_raw),
            parse_value_expr(loss_str, vocab),
            parse_value_expr(gain_str, vocab),
        )
    {
        return Some(vec![
            InstructionAst::ChangeLife {
                player: who,
                delta: Box::new(loss),
                gain: false,
            },
            InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                delta: Box::new(gain),
                gain: true,
            },
        ]);
    }

    for (verb_base, verb_3p, is_gain) in [("gain", "gains", true), ("lose", "loses", false)] {
        if let Some((who, rhs)) = split_leading_player_verb(low, verb_base, verb_3p) {
            if let Some(val_raw) = rhs.strip_prefix("life equal to ") {
                let amt = if val_raw == "the life lost this way"
                    || val_raw == "the damage dealt this way"
                    || val_raw == "the damage prevented this way"
                {
                    ValueExprAst::PriorResult
                } else {
                    parse_value_expr(val_raw, vocab).ok()?
                };
                return Some(vec![InstructionAst::ChangeLife {
                    player: who,
                    delta: Box::new(amt),
                    gain: is_gain,
                }]);
            }
            if let Some((amt_str, fe_str)) = rhs.split_once(" life for each ")
                && let (Ok(base), Ok(fe_val)) = (
                    parse_value_expr(amt_str, vocab),
                    parse_for_each_value_expr(fe_str, vocab),
                )
            {
                return Some(vec![InstructionAst::ChangeLife {
                    player: who,
                    delta: Box::new(ValueExprAst::Mul(Box::new(base), Box::new(fe_val))),
                    gain: is_gain,
                }]);
            }
            if let Some(amt_str) = rhs.strip_suffix(" life")
                && let Ok(amt) = parse_value_expr(amt_str, vocab)
            {
                return Some(vec![InstructionAst::ChangeLife {
                    player: who,
                    delta: Box::new(amt),
                    gain: is_gain,
                }]);
            }
            if rhs.starts_with("half ")
                && let Ok(amt) = parse_value_expr(rhs, vocab)
            {
                return Some(vec![InstructionAst::ChangeLife {
                    player: who,
                    delta: Box::new(amt),
                    gain: is_gain,
                }]);
            }
        }
    }
    None
}

fn try_parse_counters(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    // MoveCounters: "move <cnt> <kind> counter(s) from <src> onto/to <dst>"
    if let Some(after) = low.strip_prefix("move ")
        && let Some((left, rest)) = after
            .split_once(" counters from ")
            .or_else(|| after.split_once(" counter from "))
        && let Some((src_raw, dst_raw)) = rest
            .split_once(" onto ")
            .or_else(|| rest.split_once(" to "))
        && let (Ok(from), Ok(to)) = (
            parse_subject_or_target(src_raw, vocab),
            parse_subject_or_target(dst_raw, vocab),
        )
    {
        if let Some(kind) = left.strip_prefix("any number of ") {
            return Some(InstructionAst::MoveCounters {
                from: Box::new(from),
                to: Box::new(to),
                counter_kind: kind.trim().to_string(),
                all: false,
                any_number: true,
                count: None,
            });
        }
        if let Some(kind) = left.strip_prefix("all ") {
            return Some(InstructionAst::MoveCounters {
                from: Box::new(from),
                to: Box::new(to),
                counter_kind: kind.trim().to_string(),
                all: true,
                any_number: false,
                count: None,
            });
        }
        let (cnt, kind) = peel_leading_count(left, vocab);
        return Some(InstructionAst::MoveCounters {
            from: Box::new(from),
            to: Box::new(to),
            counter_kind: kind.to_string(),
            all: false,
            any_number: false,
            count: Some(Box::new(cnt)),
        });
    }

    // PutCounters: "put a number of <kind> counters on <subj> equal to <val>"
    // or "put a number of <kind> counters equal to <val> on <subj>" (Szarel, Jenova)
    if let Some(after) = low.strip_prefix("put a number of ") {
        if let Some((kind, rest)) = after
            .split_once(" counters on ")
            .or_else(|| after.split_once(" counter on "))
            && let Some((subj_raw, val_raw)) = rest.split_once(" equal to ")
            && let (Ok(subj), Ok(val)) = (
                parse_subject_or_target(subj_raw, vocab),
                parse_value_expr(val_raw, vocab),
            )
        {
            return Some(InstructionAst::PutCounters {
                subject: Box::new(subj),
                counter_kind: kind.trim().to_string(),
                count: Box::new(val),
            });
        }
        if let Some((kind, rest)) = after
            .split_once(" counters equal to ")
            .or_else(|| after.split_once(" counter equal to "))
            && let Some((val_raw, subj_raw)) = rest.rsplit_once(" on ")
            && let (Ok(val), Ok(subj)) = (
                parse_value_expr(val_raw, vocab),
                parse_subject_or_target(subj_raw, vocab),
            )
        {
            return Some(InstructionAst::PutCounters {
                subject: Box::new(subj),
                counter_kind: kind.trim().to_string(),
                count: Box::new(val),
            });
        }
    }

    // Put sticker: "put a [name/art/ability] sticker on <subj>"
    if let Some(after) = low.strip_prefix("put a sticker on ")
        && let Ok(subj) = parse_subject_or_target(after, vocab)
    {
        return Some(InstructionAst::PutSticker {
            kind: "any".to_string(),
            target: Box::new(subj),
        });
    }
    if let Some(after) = low.strip_prefix("put a ")
        && let Some((kind, subj_raw)) = after.split_once(" sticker on ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::PutSticker {
            kind: kind.trim().to_string(),
            target: Box::new(subj),
        });
    }

    // PutCounters with keyword-counter choice:
    // "put [your choice of] a counter from among <kws> on <subj>"
    // "put a +1/+1 counter and a counter from among <kws> on <subj>" (Elspeth Resplendent, Aragorn)
    if let Some(after) = low.strip_prefix("put ")
        && let Some((left, rest)) = after.split_once(" a counter from among ")
        && let Some((kws_raw, subj_raw)) = rest.split_once(" on ")
        && let (Some(_kws), Ok(subj)) = (
            parse_keyword_list(kws_raw, vocab),
            parse_subject_or_target(subj_raw, vocab),
        )
        && (left.is_empty() || left == "your choice of" || left.ends_with(" counter and"))
    {
        return Some(InstructionAst::PutCounters {
            subject: Box::new(subj),
            counter_kind: format!("choice:{kws_raw}"),
            count: Box::new(ValueExprAst::Const(1)),
        });
    }

    // PutCounters: "[<player>] put(s) <cnt> <kind> counter(s) on <subj> [for each <filter>]"
    if let Some((_, after)) = split_leading_player_verb(low, "put", "puts")
        && let Some((left, rest)) = after
            .split_once(" counters on ")
            .or_else(|| after.split_once(" counter on "))
    {
        let (subj_raw, fe_opt) = match rest.split_once(" for each ") {
            Some((sr, fe)) => (sr, Some(fe)),
            None => (rest, None),
        };
        if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
            let (mut cnt, kind) = peel_leading_count(left, vocab);
            if let Some(fe) = fe_opt {
                let scale_val = parse_for_each_value_expr(fe, vocab).ok()?;
                cnt = ValueExprAst::Mul(Box::new(cnt), Box::new(scale_val));
            }
            return Some(InstructionAst::PutCounters {
                subject: Box::new(subj),
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
            });
        }
    }

    // RemoveCounters (including "remove those counters" / "remove all of those counters")
    if low == "remove those counters" || low == "remove all of those counters" {
        return Some(InstructionAst::RemoveCounters {
            subject: Box::new(SubjectRefAst::SelfObject),
            counter_kind: "those".to_string(),
            all: true,
            count: None,
        });
    }
    if let Some((_, after)) = split_leading_player_verb(low, "remove", "removes")
        && let Some((left, subj_raw)) = after
            .split_once(" counters from ")
            .or_else(|| after.split_once(" counter from "))
    {
        let (subj_clean, delayed_eoc) = match subj_raw.strip_suffix(" at end of combat") {
            Some(c) => (c, true),
            None => (subj_raw, false),
        };
        let subj = parse_subject_or_target(subj_clean, vocab).ok()?;
        let ins = if let Some(kind) = left.strip_prefix("all ") {
            InstructionAst::RemoveCounters {
                subject: Box::new(subj),
                counter_kind: kind.trim().to_string(),
                all: true,
                count: None,
            }
        } else {
            let (cnt, kind) = peel_leading_count(left, vocab);
            InstructionAst::RemoveCounters {
                subject: Box::new(subj),
                counter_kind: if kind.is_empty() {
                    "any".to_string()
                } else {
                    kind.to_string()
                },
                all: false,
                count: Some(Box::new(cnt)),
            }
        };
        return Some(if delayed_eoc {
            InstructionAst::DelayedTrigger {
                step: TurnStepAst::EndOfCombat,
                scope: DelayedStepScope::Next,
                body: vec![ins],
            }
        } else {
            ins
        });
    }
    None
}

fn try_parse_exile(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    if let Some(after) = low
        .strip_prefix("exile ")
        .or_else(|| low.strip_prefix("exiles "))
    {
        if !after.contains(" from ")
            && let Some(whose) = after
                .strip_suffix(" graveyards")
                .or_else(|| after.strip_suffix(" graveyard"))
        {
            return Some(InstructionAst::ExileGraveyard {
                whose: whose
                    .trim_end_matches("'s")
                    .trim_end_matches('\'')
                    .to_string(),
            });
        }
        if let Some(subj_raw) = after.strip_suffix(" until ~ leaves the battlefield")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Some(InstructionAst::ExileUntilLeaves {
                subject: Box::new(subj),
            });
        }
        if let Some((subj_raw, cnt_rest)) = after.split_once(" with ")
            && let Some(left) = cnt_rest
                .strip_suffix(" counters on it")
                .or_else(|| cnt_rest.strip_suffix(" counter on it"))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let (cnt, kind) = peel_leading_count(left, vocab);
            return Some(InstructionAst::ExileWithCounters {
                subject: Box::new(subj),
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
            });
        }
    }

    if let Some((who, after)) = split_leading_player_verb(low, "exile", "exiles") {
        let actor = if who == PlayerRefAst::You && !low.starts_with("you ") {
            None
        } else {
            Some(who)
        };
        if let Some(whose) = after
            .strip_suffix(" graveyards")
            .or_else(|| after.strip_suffix(" graveyard"))
            && !whose.contains(" from ")
            && !whose.contains(" in ")
        {
            return Some(InstructionAst::ExileGraveyard {
                whose: whose.trim_end_matches("'s").to_string(),
            });
        }
        let (after_fd, face_down) = match after.strip_suffix(" face down") {
            Some(c) => (c, true),
            None => (after, false),
        };
        if after_fd.ends_with(" at random") {
            return None;
        }
        if let Some((cnt_part, lib_part)) = after_fd.split_once(" cards from the bottom of ")
            && let Some(whose) = lib_part.strip_suffix(" library")
            && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
        {
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: format!("{}_bottom", whose.trim_end_matches("'s")),
                count: Box::new(cnt),
                face_down,
            });
        }
        if let Some(rest) = after_fd.strip_prefix("the top ")
            && let Some((cnt_part, whose_lib)) = rest
                .split_once(" cards of ")
                .or_else(|| rest.split_once(" card of "))
            && let Some(whose) = whose_lib.strip_suffix(" library")
        {
            let cnt = if cnt_part.is_empty() {
                ValueExprAst::Const(1)
            } else {
                parse_value_expr(cnt_part, vocab).ok()?
            };
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: whose.trim_end_matches("'s").to_string(),
                count: Box::new(cnt),
                face_down,
            });
        }
        if let Some(whose_lib) = after_fd.strip_prefix("the top card of ")
            && let Some(whose) = whose_lib
                .strip_suffix(" library")
                .or_else(|| whose_lib.strip_suffix(" libraries"))
        {
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: whose.trim_end_matches("'s").to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                face_down,
            });
        }
        if after_fd == "the top card" {
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: "your".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                face_down,
            });
        }
        if let Some(cnt_raw) = after_fd
            .strip_prefix("the top ")
            .and_then(|r| r.strip_suffix(" cards"))
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            return Some(InstructionAst::ExileTopLibrary {
                actor,
                whose: "your".to_string(),
                count: Box::new(cnt),
                face_down,
            });
        }
        if after_fd == "your hand" || after_fd == "their hand" {
            return Some(InstructionAst::Exile {
                actor,
                subject: Box::new(SubjectRefAst::Anaphora(after_fd.to_string())),
            });
        }
        if let Some(subj_raw) = after_fd.strip_suffix(" from it")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Some(InstructionAst::Exile {
                actor,
                subject: Box::new(subj),
            });
        }
        if let Some(who_hand) = after_fd.strip_prefix("all cards from ")
            && who_hand.ends_with("'s hand")
        {
            return Some(InstructionAst::Exile {
                actor,
                subject: Box::new(SubjectRefAst::Anaphora(who_hand.to_string())),
            });
        }
        if after_fd.starts_with("all but the bottom card of ") && after_fd.ends_with("'s library") {
            return Some(InstructionAst::Exile {
                actor,
                subject: Box::new(SubjectRefAst::Anaphora(after_fd.to_string())),
            });
        }
        // "exile <count> cards from the top of <whose> library" / "exile a number of cards from the top of <whose> library equal to <val>"
        if let Some((cnt_part, lib_part)) = after_fd
            .split_once(" cards from the top of ")
            .or_else(|| after_fd.split_once(" card from the top of "))
        {
            if let Some((whose_lib, eq_val)) = lib_part.split_once(" library equal to ")
                && cnt_part == "a number of"
                && let Ok(cnt) = parse_value_expr(eq_val, vocab)
            {
                return Some(InstructionAst::ExileTopLibrary {
                    actor,
                    whose: whose_lib.trim_end_matches("'s").to_string(),
                    count: Box::new(cnt),
                    face_down,
                });
            }
            if let Some(whose) = lib_part.strip_suffix(" library")
                && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
            {
                return Some(InstructionAst::ExileTopLibrary {
                    actor,
                    whose: whose.trim_end_matches("'s").to_string(),
                    count: Box::new(cnt),
                    face_down,
                });
            }
        }
        // "<player> exiles cards from the top of <whose> library until <player> exile(s) <filter>"
        if let Some(rest) = after_fd.strip_prefix("cards from the top of ")
            && let Some((lib_raw, until_rest)) = rest
                .split_once(" library until you ")
                .or_else(|| rest.split_once(" library until they "))
                .or_else(|| rest.split_once(" library until that player "))
        {
            let flt_raw = until_rest
                .strip_prefix("exile ")
                .or_else(|| until_rest.strip_prefix("exiles "))
                .unwrap_or(until_rest);
            if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
                return Some(InstructionAst::ExileOrRevealTopUntil {
                    exile: true,
                    library: lib_raw.trim_end_matches("'s").to_string(),
                    filter: Box::new(flt),
                });
            }
        }
        let (core, delayed) = if let Some(c) =
            after_fd.strip_suffix(" at the beginning of the next end step")
        {
            (c, Some((TurnStepAst::EndStep, DelayedStepScope::Next)))
        } else if let Some(c) = after_fd.strip_suffix(" at the beginning of your next end step") {
            (c, Some((TurnStepAst::EndStep, DelayedStepScope::YourNext)))
        } else if let Some(c) = after_fd.strip_suffix(" at end of combat") {
            (c, Some((TurnStepAst::EndOfCombat, DelayedStepScope::Next)))
        } else {
            (after_fd, None)
        };
        let (cnt, core_nocnt) = peel_leading_count(core, vocab);
        let subj_res = parse_subject_or_target(core, vocab).or_else(|err| {
            if matches!(cnt, ValueExprAst::Const(1)) {
                parse_subject_or_target(core_nocnt, vocab)
            } else {
                Err(err)
            }
        });
        if let Ok(subj) = subj_res {
            let ins = InstructionAst::Exile {
                actor,
                subject: Box::new(subj),
            };
            return Some(match delayed {
                Some((step, scope)) => InstructionAst::DelayedTrigger {
                    step,
                    scope,
                    body: vec![ins],
                },
                None => ins,
            });
        }
        if delayed.is_none()
            && !matches!(cnt, ValueExprAst::Const(1))
            && let Ok(subj) = parse_subject_or_target(core_nocnt, vocab)
        {
            return Some(InstructionAst::MoveCounted {
                actor,
                count: Some(Box::new(cnt)),
                up_to: false,
                any_number: false,
                subject: Box::new(subj),
                to: "exile".to_string(),
            });
        }
    }
    None
}

fn try_parse_sacrifice(s: &str, low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    if let Some(idx) = low.find(" unless ")
        && let Some(subj_raw) = low[..idx].strip_prefix("sacrifice ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let rhs = &low[idx + 8..];
        if let Some(spent_raw) = rhs.strip_suffix(" was spent to cast it") {
            return Some(InstructionAst::IfCondition {
                condition: Box::new(ConditionAst::Not(Box::new(ConditionAst::ManaSpentToCast(
                    spent_raw.trim().to_string(),
                )))),
                then_body: vec![InstructionAst::Sacrifice {
                    player: PlayerRefAst::You,
                    count: None,
                    subject: Box::new(subj),
                }],
            });
        }
        if let Some(pay_raw) = rhs.strip_prefix("you pay ")
            && let Ok(cost) = parse_cost_atom(&s[idx + 8 + 8..], vocab)
        {
            let _ = pay_raw;
            return Some(InstructionAst::SacrificeUnlessPay {
                subject: Box::new(subj),
                cost,
            });
        }
        if rhs.starts_with("you ")
            && let Ok(cost) = parse_cost_atom(&s[idx + 8 + 4..], vocab)
        {
            return Some(InstructionAst::SacrificeUnlessPay {
                subject: Box::new(subj),
                cost,
            });
        }
        if let Ok(cond) = parse_condition(rhs, vocab) {
            return Some(InstructionAst::IfCondition {
                condition: Box::new(ConditionAst::Not(Box::new(cond))),
                then_body: vec![InstructionAst::Sacrifice {
                    player: PlayerRefAst::You,
                    count: None,
                    subject: Box::new(subj),
                }],
            });
        }
    }
    // "<subj>'s controller sacrifices it" (Arcum Dagsson, Reality Acid, Soul Ransom)
    if let Some(subj_raw) = low.strip_suffix("'s controller sacrifices it")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::Sacrifice {
            player: PlayerRefAst::ThatObjectController,
            count: None,
            subject: Box::new(subj),
        });
    }
    // "<player> chooses and sacrifices <subj>" (Barrin's Spite, Retribution)
    if let Some((who, after)) =
        split_leading_player_verb(low, "choose and sacrifice", "chooses and sacrifices")
        && let Ok(subj) = parse_subject_or_target(after, vocab)
    {
        return Some(InstructionAst::Sacrifice {
            player: who,
            count: None,
            subject: Box::new(subj),
        });
    }
    if let Some((who, after)) = split_leading_player_verb(low, "sacrifice", "sacrifices") {
        let (core, delayed_step) =
            if let Some(c) = after.strip_suffix(" at the beginning of the next end step") {
                (c, Some((TurnStepAst::EndStep, DelayedStepScope::Next)))
            } else if let Some(c) = after.strip_suffix(" at the beginning of your next end step") {
                (c, Some((TurnStepAst::EndStep, DelayedStepScope::YourNext)))
            } else if let Some(c) = after.strip_suffix(" at end of combat") {
                (c, Some((TurnStepAst::EndOfCombat, DelayedStepScope::Next)))
            } else {
                (after, None)
            };
        if let Some(any_rest) = core.strip_prefix("any number of ")
            && delayed_step.is_none()
            && let Ok(subj) = parse_subject_or_target(any_rest, vocab)
        {
            return Some(InstructionAst::SacrificeAnyNumber {
                player: who,
                subject: Box::new(subj),
            });
        }
        if core.contains(" except for ")
            || core.ends_with(", rounded up")
            || core.ends_with(", rounded down")
            || core.ends_with(" of an opponent's choice")
            || core.starts_with("any number of ")
            || core.starts_with("half the ")
            || core.starts_with("half of the ")
        {
            return None;
        }
        let core = core
            .strip_suffix(" of their choice")
            .or_else(|| core.strip_suffix(" of his or her choice"))
            .unwrap_or(core);
        let (parsed_subj, parsed_count) = if let Ok(subj) = parse_subject_or_target(core, vocab) {
            (Some(subj), None)
        } else {
            let (cnt, core_nocnt) = peel_leading_count(core, vocab);
            match parse_subject_or_target(core_nocnt, vocab) {
                Ok(subj) => (
                    Some(subj),
                    if matches!(cnt, ValueExprAst::Const(1)) {
                        None
                    } else {
                        Some(Box::new(cnt))
                    },
                ),
                Err(_) => (None, None),
            }
        };
        if let Some(subj) = parsed_subj {
            let ins = InstructionAst::Sacrifice {
                player: who,
                count: parsed_count,
                subject: Box::new(subj),
            };
            return Some(match delayed_step {
                Some((step, scope)) => InstructionAst::DelayedTrigger {
                    step,
                    scope,
                    body: vec![ins],
                },
                None => ins,
            });
        }
    }
    None
}

fn try_parse_discard(s: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    let low = s.to_ascii_lowercase();
    let (who, after) = split_leading_player_verb(&low, "discard", "discards")?;
    if let Some((left_part, unless_raw)) = after.split_once(" unless you discard ")
        && let Ok(flt) = parse_object_filter(unless_raw, vocab)
    {
        let (cnt, _) = peel_leading_count(left_part, vocab);
        return Some(InstructionAst::DiscardUnless {
            player: who,
            count: Box::new(cnt),
            unless_filter: Box::new(flt),
        });
    }
    if let Some((left_part, unless_raw)) = after.split_once(" unless you ")
        && let Ok(cost) = parse_cost_atom(unless_raw, vocab)
    {
        let (cnt, _) = peel_leading_count(left_part, vocab);
        return Some(InstructionAst::Unless {
            body: vec![InstructionAst::Discard {
                player: who,
                count: Some(Box::new(cnt)),
                filter: None,
                anaphora: None,
                random: false,
            }],
            unless_player: Some(PlayerRefAst::You),
            unless_cost: Some(cost),
            unless_action: None,
            unless_condition: None,
        });
    }
    if let Some(fe) = after
        .strip_prefix("a card for each ")
        .or_else(|| after.strip_prefix("one card for each "))
        && let Ok(cnt) = parse_for_each_value_expr(fe, vocab)
    {
        return Some(InstructionAst::Discard {
            player: who,
            count: Some(Box::new(cnt)),
            filter: None,
            anaphora: None,
            random: false,
        });
    }
    if matches!(after, "your hand" | "their hand" | "his or her hand") {
        return Some(InstructionAst::DiscardHand { player: who });
    }
    if let Some(after_half) = after.strip_prefix("half the cards in ")
        && (after_half.starts_with("their hand") || after_half.starts_with("your hand"))
    {
        let rounding = if after_half.contains("rounded up") {
            crate::ast::Rounding::Up
        } else {
            crate::ast::Rounding::Down
        };
        return Some(InstructionAst::Discard {
            player: who,
            count: Some(Box::new(ValueExprAst::DivConst {
                expr: Box::new(ValueExprAst::PlayerStat {
                    player: who,
                    stat: crate::ast::PlayerStatKind::CardsInHand,
                }),
                denom: 2,
                rounding,
            })),
            filter: None,
            anaphora: None,
            random: false,
        });
    }
    let (core, random) = match after.strip_suffix(" at random") {
        Some(c) => (c, true),
        None => (after, false),
    };
    if core.starts_with("that ")
        || core.starts_with("those ")
        || matches!(
            core,
            "it" | "them"
                | "one of them"
                | "two of them"
                | "one of those cards"
                | "two of those cards"
        )
    {
        return Some(InstructionAst::Discard {
            player: who,
            count: None,
            filter: None,
            anaphora: Some(core.to_string()),
            random,
        });
    }
    if let Some(val_raw) = core.strip_prefix("a number of cards equal to ")
        && let Ok(cnt) = parse_value_expr(val_raw, vocab)
    {
        return Some(InstructionAst::Discard {
            player: who,
            count: Some(Box::new(cnt)),
            filter: None,
            anaphora: None,
            random,
        });
    }
    let core = core
        .strip_prefix("up to ")
        .or_else(|| core.strip_prefix("any number of "))
        .or_else(|| core.strip_prefix("all "))
        .unwrap_or(core);
    let (cnt, flt_raw) = peel_leading_count(core, vocab);
    let flt = parse_object_filter(flt_raw, vocab).ok()?;
    Some(InstructionAst::Discard {
        player: who,
        count: Some(Box::new(cnt)),
        filter: Some(Box::new(flt)),
        anaphora: None,
        random,
    })
}

fn try_parse_zone_moves(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    // ReturnZone ("return ..." or "<player> return(s) ..." or inverted "return to <dest> <subj>")
    if let Some((_, after)) = split_leading_player_verb(low, "return", "returns") {
        let (core, delayed) =
            if let Some(c) = after.strip_suffix(" at the beginning of the next end step") {
                (c, Some((TurnStepAst::EndStep, DelayedStepScope::Next)))
            } else if let Some(c) = after.strip_suffix(" at the beginning of your next end step") {
                (c, Some((TurnStepAst::EndStep, DelayedStepScope::YourNext)))
            } else if let Some(c) = after
                .strip_suffix(" at the beginning of that player's next end step")
                .or_else(|| after.strip_suffix(" at the beginning of their next end step"))
            {
                (
                    c,
                    Some((TurnStepAst::EndStep, DelayedStepScope::ThatPlayersNext)),
                )
            } else if let Some(c) = after.strip_suffix(" at the beginning of the next upkeep") {
                (c, Some((TurnStepAst::Upkeep, DelayedStepScope::Next)))
            } else if let Some(c) = after.strip_suffix(" at the beginning of your next upkeep") {
                (c, Some((TurnStepAst::Upkeep, DelayedStepScope::YourNext)))
            } else if let Some(c) = after
                .strip_suffix(" at the beginning of their next upkeep")
                .or_else(|| after.strip_suffix(" at the beginning of that player's next upkeep"))
            {
                (
                    c,
                    Some((TurnStepAst::Upkeep, DelayedStepScope::ThatPlayersNext)),
                )
            } else if let Some(c) = after.strip_suffix(" at end of combat") {
                (c, Some((TurnStepAst::EndOfCombat, DelayedStepScope::Next)))
            } else {
                (after, None)
            };
        if core.contains(", except for ") || core.contains(" except for ") {
            return None;
        }
        // Inverted order (longest destination prefix first):
        // "return to the battlefield [tapped] [under ... control] <subj>" / "return to their/your/its owner's hand <subj>"
        for (inv_pref, dest, tapped) in [
            (
                "to the battlefield tapped under your control ",
                "the battlefield tapped under your control",
                true,
            ),
            (
                "to the battlefield tapped under its owner's control ",
                "the battlefield tapped under its owner's control",
                true,
            ),
            (
                "to the battlefield tapped under their owners' control ",
                "the battlefield tapped under their owners' control",
                true,
            ),
            (
                "to the battlefield tapped under their owner's control ",
                "the battlefield tapped under their owner's control",
                true,
            ),
            (
                "to the battlefield under your control tapped ",
                "the battlefield tapped under your control",
                true,
            ),
            (
                "to the battlefield under its owner's control tapped ",
                "the battlefield tapped under its owner's control",
                true,
            ),
            (
                "to the battlefield under your control ",
                "the battlefield under your control",
                false,
            ),
            (
                "to the battlefield under its owner's control ",
                "the battlefield under its owner's control",
                false,
            ),
            (
                "to the battlefield under their owners' control ",
                "the battlefield under their owners' control",
                false,
            ),
            (
                "to the battlefield under their owner's control ",
                "the battlefield under their owner's control",
                false,
            ),
            ("to the battlefield tapped ", "the battlefield tapped", true),
            ("to the battlefield ", "the battlefield", false),
            ("to its owner's hand ", "its owner's hand", false),
            ("to their owners' hands ", "their owners' hands", false),
            ("to their owner's hand ", "their owner's hand", false),
            ("to their hand ", "their hand", false),
            ("to your hand ", "your hand", false),
            ("to that player's hand ", "that player's hand", false),
        ] {
            if let Some(subj_raw) = core.strip_prefix(inv_pref) {
                if subj_raw.ends_with(" at random") {
                    return None;
                }
                let is_up_to_or_any =
                    subj_raw.starts_with("any number of ") || subj_raw.starts_with("up to ");
                let parsed = if let Ok(s) = parse_subject_or_target(subj_raw, vocab) {
                    Some((s, None))
                } else if let Some((sr, fz)) = subj_raw.split_once(" from ")
                    && let Ok(s) = parse_subject_or_target(sr, vocab)
                {
                    Some((s, Some(fz.to_string())))
                } else {
                    None
                };
                if let Some((subj, from_zone)) = parsed {
                    if is_up_to_or_any && !matches!(subj, SubjectRefAst::TargetObject { .. }) {
                        return None;
                    }
                    let ins = InstructionAst::ReturnZone {
                        subject: Box::new(subj),
                        from_zone,
                        to: dest.to_string(),
                        tapped,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                    };
                    return Some(match delayed {
                        Some((step, scope)) => InstructionAst::DelayedTrigger {
                            step,
                            scope,
                            body: vec![ins],
                        },
                        None => ins,
                    });
                }
            }
        }
        let (core_noatt, attached_to) = if let Some((before, att_raw)) =
            core.split_once(" attached to ")
            && let Ok(att) = parse_subject_or_target(att_raw, vocab)
        {
            (before, Some(Box::new(att)))
        } else {
            (core, None)
        };
        let (core_nocnt, with_counter) = if let Some((before, after_w)) =
            core_noatt.rsplit_once(" with ")
            && let Some(kind) = after_w
                .strip_suffix(" counter on it")
                .or_else(|| after_w.strip_suffix(" counters on it"))
        {
            (before, Some(kind.trim().to_string()))
        } else {
            (core_noatt, None)
        };
        // "return <subj> under your control [with a finality counter on it]" (Meathook Massacre II)
        if let Some(subj_raw) = core_nocnt.strip_suffix(" under your control")
            && !subj_raw.contains(" to ")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let ins = InstructionAst::ReturnZone {
                subject: Box::new(subj),
                from_zone: None,
                to: "the battlefield under your control".to_string(),
                tapped: false,
                transformed: false,
                with_counter,
                attached_to,
            };
            return Some(match delayed {
                Some((step, scope)) => InstructionAst::DelayedTrigger {
                    step,
                    scope,
                    body: vec![ins],
                },
                None => ins,
            });
        }
        if let Some((left, dest)) = core_nocnt.rsplit_once(" to ") {
            const VALID_DESTS: &[&str] = &[
                "its owner's hand",
                "his owner's hand",
                "her owner's hand",
                "their owner's hand",
                "their owners' hands",
                "their hand",
                "your hand",
                "that player's hand",
                "their owner's graveyard",
                "its owner's graveyard",
                "the command zone",
                "the top of its owner's library",
                "the bottom of its owner's library",
            ];
            let valid_bf = is_orthogonal_battlefield_dest(dest);
            if valid_bf || VALID_DESTS.contains(&dest) {
                let (subj_raw, from_zone) = match left.split_once(" from ") {
                    Some((sr, fz)) => (sr, Some(fz.to_string())),
                    None => (left, None),
                };
                if subj_raw.ends_with(" at random") {
                    return None;
                }
                let is_up_to_or_any =
                    subj_raw.starts_with("any number of ") || subj_raw.starts_with("up to ");
                let (cnt, subj_nocnt) = peel_leading_count(subj_raw, vocab);
                let subj_res = parse_subject_or_target(subj_raw, vocab).or_else(|err| {
                    if !is_up_to_or_any && matches!(cnt, ValueExprAst::Const(1)) {
                        parse_subject_or_target(subj_nocnt, vocab)
                    } else {
                        Err(err)
                    }
                });
                let subj_res = if is_up_to_or_any {
                    match subj_res {
                        Ok(t @ SubjectRefAst::TargetObject { .. }) => Ok(t),
                        _ => Err(String::new()),
                    }
                } else {
                    subj_res
                };
                if let Ok(subj) = subj_res {
                    let ins = InstructionAst::ReturnZone {
                        subject: Box::new(subj),
                        from_zone,
                        to: dest.to_string(),
                        tapped: dest.contains("tapped"),
                        transformed: dest.contains("transformed") || dest.contains("converted"),
                        with_counter,
                        attached_to,
                    };
                    return Some(match delayed {
                        Some((step, scope)) => InstructionAst::DelayedTrigger {
                            step,
                            scope,
                            body: vec![ins],
                        },
                        None => ins,
                    });
                }
            }
        }
    }

    // ChooseAndKeep ("... and the rest ..." / "... and the other ...")
    if let Some(after) = low.strip_prefix("put ")
        && let Some((keep_part, rest_part)) = after
            .split_once(" and the rest ")
            .or_else(|| after.split_once(" and the other "))
    {
        for pick in [
            "one of them",
            "two of them",
            "one of those cards",
            "two of those cards",
            "one pile",
            "that pile",
            "one",
            "that card",
            "a card from among them",
        ] {
            if let Some(after_pick) = keep_part.strip_prefix(pick) {
                let keep_to = after_pick
                    .trim()
                    .strip_prefix("into ")
                    .or_else(|| after_pick.trim().strip_prefix("onto "))
                    .unwrap_or("");
                let rest_to = rest_part
                    .strip_prefix("into ")
                    .or_else(|| rest_part.strip_prefix("on "))
                    .unwrap_or("");
                if !keep_to.is_empty() && !rest_to.is_empty() {
                    return Some(InstructionAst::ChooseAndKeep {
                        pick: pick.to_string(),
                        keep_to: keep_to.to_string(),
                        rest_to: rest_to.to_string(),
                    });
                }
            }
        }
        // Mulch-style: "put all <filter> revealed this way into <dest1> and the rest into <dest2>"
        if let Some((flt_part, keep_to)) = keep_part
            .split_once(" into ")
            .or_else(|| keep_part.split_once(" onto "))
            && let Some(rest_to) = rest_part
                .strip_prefix("into ")
                .or_else(|| rest_part.strip_prefix("on "))
            && parse_subject_or_target(flt_part, vocab).is_ok()
        {
            return Some(InstructionAst::ChooseAndKeep {
                pick: flt_part.to_string(),
                keep_to: keep_to.to_string(),
                rest_to: rest_to.to_string(),
            });
        }
    }

    // RevealFromLooked
    if let Some(after) = low.strip_prefix("reveal ")
        && let Some((flt_raw, dest_part)) = after.split_once(" from among them and put ")
    {
        let trimmed = flt_raw.trim();
        let (pick_prefix, rest_after_upto) = if let Some(r) = trimmed.strip_prefix("up to ") {
            ("up to ", r)
        } else {
            ("", trimmed)
        };
        let (cnt_ast, clean_flt) = peel_leading_count(rest_after_upto, vocab);
        let count_tag = if trimmed.starts_with("any number ") {
            "any number ".to_string()
        } else if trimmed.starts_with("all ") {
            "all ".to_string()
        } else if let ValueExprAst::Const(n) = cnt_ast
            && n >= 2
        {
            format!("{pick_prefix}{n} ")
        } else {
            String::new()
        };
        if let Ok(flt) = parse_object_filter(clean_flt, vocab) {
            return Some(InstructionAst::RevealFromLooked {
                filter: Box::new(flt),
                to: format!("{count_tag}{dest_part}"),
            });
        }
    }

    // "<subj>'s owner puts it on ..." / "Its owner puts it on ..." / "The owner of <subj> puts it on/into ..."
    if low == "its owner puts it on their choice of the top or bottom of their library" {
        return Some(InstructionAst::MoveZone {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            to: "owner_choice_top_or_bottom_of_library".to_string(),
        });
    }
    if let Some((subj_raw, rest)) = low.split_once("'s owner puts it on ")
        && rest == "their choice of the top or bottom of their library"
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: "owner_choice_top_or_bottom_of_library".to_string(),
        });
    }
    if let Some(after) = low.strip_prefix("the owner of ")
        && let Some((subj_raw, rest)) = after
            .split_once(" puts it on ")
            .or_else(|| after.split_once(" puts it into "))
        && (rest == "their choice of the top or bottom of their library"
            || rest == "their library second from the top or on the bottom")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: rest.to_string(),
        });
    }

    // "shuffle and put <subj> on top [of your library]" (Mwonvuli Beast Tracker)
    if let Some(after) = low.strip_prefix("shuffle and put ")
        && let Some(subj_raw) = after
            .strip_suffix(" on top")
            .or_else(|| after.strip_suffix(" on top of your library"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: "top_of_library_after_shuffle".to_string(),
        });
    }

    // MoveZone: "[<player>] put(s) <subj> onto/into/on <dest>" (or inverted "put onto/into <dest> <subj>")
    if let Some((_, after)) = split_leading_player_verb(low, "put", "puts") {
        let after_norm = after
            .strip_suffix(" from the command zone")
            .unwrap_or(after)
            .replace(" back on top of ", " on top of ");
        for (inv_pref, dest) in [
            (
                "onto the battlefield under your control ",
                "the battlefield under your control",
            ),
            ("onto the battlefield tapped ", "the battlefield tapped"),
            ("onto the battlefield ", "the battlefield"),
            ("into your hand ", "your hand"),
            ("into your graveyard ", "your graveyard"),
        ] {
            if let Some(subj_raw) = after_norm.strip_prefix(inv_pref)
                && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            {
                return Some(InstructionAst::MoveZone {
                    subject: Box::new(subj),
                    to: dest.to_string(),
                });
            }
        }
        for sep in [" onto ", " into ", " on "] {
            if let Some((obj_raw, dest)) = after_norm.split_once(sep) {
                const MOVE_DESTS: &[&str] = &[
                    "the battlefield",
                    "the battlefield tapped",
                    "the battlefield tapped and attacking",
                    "the battlefield tapped and attacking that player",
                    "the battlefield under your control",
                    "the battlefield tapped under your control",
                    "the battlefield under the control of that card's owner",
                    "the battlefield face down under your control",
                    "the battlefield face up",
                    "the battlefield face up or face down",
                    "the battlefield or into your hand",
                    "your hand",
                    "its owner's hand",
                    "their owner's hand",
                    "their owners' hands",
                    "that player's hand",
                    "their hand",
                    "top",
                    "the top",
                    "the bottom",
                    "the top of your library",
                    "top of your library",
                    "top of your library in any order",
                    "the top of your library in any order",
                    "the top of their library",
                    "top of their library",
                    "top of their library in any order",
                    "the top of their library in any order",
                    "the top of that player's library",
                    "top of that player's library",
                    "top of that player's library in any order",
                    "the top of that player's library in any order",
                    "the top of that library",
                    "top of that library",
                    "top of that library in any order",
                    "the bottom of your library",
                    "bottom of your library",
                    "the bottom of your library in a random order",
                    "the bottom of your library in any order",
                    "the bottom of their library",
                    "bottom of their library",
                    "the bottom of their library in a random order",
                    "the bottom of their library in any order",
                    "the bottom of that library",
                    "the bottom of that library in a random order",
                    "the bottom of that library in any order",
                    "the bottom in a random order",
                    "the bottom in any order",
                    "the bottom of that player's library",
                    "the bottom of that player's library in a random order",
                    "the bottom of that player's library in any order",
                    "the top of its owner's library",
                    "top of its owner's library",
                    "the top of his owner's library",
                    "top of his owner's library",
                    "the top of her owner's library",
                    "top of her owner's library",
                    "the top of their owners' libraries",
                    "top of their owners' libraries",
                    "top of their owners' libraries in any order",
                    "the bottom of its owner's library",
                    "bottom of its owner's library",
                    "the bottom of his owner's library",
                    "bottom of his owner's library",
                    "the bottom of her owner's library",
                    "bottom of her owner's library",
                    "the bottom of their owners' libraries",
                    "bottom of their owners' libraries",
                    "the bottom of their owners' libraries in any order",
                    "the bottom of their owners' libraries in a random order",
                    "its owner's library second from the top",
                    "its owner's library third from the top",
                    "his owner's library second from the top",
                    "his owner's library third from the top",
                    "her owner's library second from the top",
                    "her owner's library third from the top",
                    "your library second from the top",
                    "your library third from the top",
                    "your graveyard",
                    "their graveyard",
                    "their graveyards",
                    "their owners' graveyards",
                    "their hands",
                    "its owner's graveyard",
                    "that player's graveyard",
                    "their owner's graveyard",
                    "your choice of the top or bottom of its owner's library",
                    "their choice of the top or bottom of its owner's library",
                ];
                let is_lib_depth_dest = dest.ends_with(" from the top")
                    || dest.starts_with("its owner's library just beneath the top ")
                    || dest.starts_with("their library just beneath the top ")
                    || dest.starts_with("your library just beneath the top ");
                // Compound "put <pick> onto/into <dest1> and the rest <dest2>" (Genesis Ultimatum, Green Sun's Twilight)
                if let Some((dest1_raw, rest_dest)) = dest.split_once(" and the rest ") {
                    let dest1 = dest1_raw.trim();
                    if (is_orthogonal_battlefield_dest(dest1) || MOVE_DESTS.contains(&dest1))
                        && (rest_dest.starts_with("into ")
                            || rest_dest.starts_with("on ")
                            || rest_dest.starts_with("onto "))
                    {
                        return Some(InstructionAst::ChooseAndKeep {
                            pick: obj_raw.trim().to_string(),
                            keep_to: dest1.to_string(),
                            rest_to: rest_dest.trim().to_string(),
                        });
                    }
                }
                let (dest_clean, _att_opt) = match dest.rsplit_once(" attached to ") {
                    Some((d_before, att_raw))
                        if parse_subject_or_target(att_raw, vocab).is_ok() =>
                    {
                        (d_before.trim(), Some(att_raw))
                    }
                    _ => (dest, None),
                };
                if is_orthogonal_battlefield_dest(dest_clean)
                    || MOVE_DESTS.contains(&dest_clean)
                    || is_lib_depth_dest
                {
                    let obj_clean = obj_raw.trim();
                    if let Some(any_rest) = obj_clean.strip_prefix("any number of ") {
                        if let Ok(t @ SubjectRefAst::TargetObject { .. }) =
                            parse_subject_or_target(obj_clean, vocab)
                        {
                            return Some(InstructionAst::MoveZone {
                                subject: Box::new(t),
                                to: dest.to_string(),
                            });
                        }
                        if let Ok(subj) = parse_subject_or_target(any_rest, vocab) {
                            return Some(InstructionAst::MoveCounted {
                                actor: None,
                                count: None,
                                up_to: false,
                                any_number: true,
                                subject: Box::new(subj),
                                to: dest.to_string(),
                            });
                        }
                        return None;
                    }
                    if let Some(up_rest) = obj_clean.strip_prefix("up to ") {
                        if let Ok(t @ SubjectRefAst::TargetObject { .. }) =
                            parse_subject_or_target(obj_clean, vocab)
                        {
                            return Some(InstructionAst::MoveZone {
                                subject: Box::new(t),
                                to: dest.to_string(),
                            });
                        }
                        let (cnt, up_nocnt) = peel_leading_count(up_rest, vocab);
                        if let Ok(subj) = parse_subject_or_target(up_nocnt, vocab) {
                            return Some(InstructionAst::MoveCounted {
                                actor: None,
                                count: Some(Box::new(cnt)),
                                up_to: true,
                                any_number: false,
                                subject: Box::new(subj),
                                to: dest.to_string(),
                            });
                        }
                        return None;
                    }
                    if obj_clean.starts_with("one of ") {
                        if let Ok(a @ SubjectRefAst::Anaphora(_)) =
                            parse_subject_or_target(obj_clean, vocab)
                        {
                            return Some(InstructionAst::MoveZone {
                                subject: Box::new(a),
                                to: dest.to_string(),
                            });
                        }
                        return None;
                    }
                    let (cnt, obj_nocnt) = peel_leading_count(obj_clean, vocab);
                    let subj = if obj_clean == "the rest" || obj_clean == "the rest of the cards" {
                        SubjectRefAst::Anaphora("the_rest".to_string())
                    } else if let Ok(s) = parse_subject_or_target(obj_clean, vocab) {
                        s
                    } else if matches!(cnt, ValueExprAst::Const(1)) {
                        parse_subject_or_target(obj_nocnt, vocab).ok()?
                    } else if let Ok(s) = parse_subject_or_target(obj_nocnt, vocab) {
                        return Some(InstructionAst::MoveCounted {
                            actor: None,
                            count: Some(Box::new(cnt)),
                            up_to: false,
                            any_number: false,
                            subject: Box::new(s),
                            to: dest.to_string(),
                        });
                    } else {
                        return None;
                    };
                    return Some(InstructionAst::MoveZone {
                        subject: Box::new(subj),
                        to: dest.to_string(),
                    });
                }
            }
        }
    }

    None
}

fn try_parse_search_library(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    let (opt_player, after) = if let Some(a) = low.strip_prefix("search ") {
        (None, a)
    } else if let Some((who_raw, a)) = low.split_once(" may search ") {
        let p = parse_player_ref(who_raw)?;
        (Some(p), a)
    } else if let Some((who_raw, a)) = low
        .split_once(" searches ")
        .or_else(|| low.split_once(" search "))
    {
        parse_player_ref(who_raw)?;
        (None, a)
    } else {
        return None;
    };
    let (lib_who, rest) = after.split_once(" library for ")?;

    const COMMA_TAIL_STARTERS: &[&str] = &[
        "reveal ",
        "reveals ",
        "put ",
        "puts ",
        "exile ",
        "exiles ",
        "then ",
        "and put ",
        "and puts ",
        "and exile ",
        "and exiles ",
        "and reveal ",
        "and reveals ",
        "and shuffle",
        "and shuffles",
        "shuffle",
        "shuffles",
    ];
    let mut best_split: Option<(usize, usize)> = None;
    for (idx, _) in rest.match_indices(", ") {
        let after_comma = &rest[idx + 2..];
        if COMMA_TAIL_STARTERS
            .iter()
            .any(|st| after_comma.starts_with(st))
        {
            best_split = Some((idx, 2));
            break;
        }
    }
    for (needle, skip) in [
        (" and put ", 5usize),
        (" and puts ", 5usize),
        (" and exile ", 5usize),
        (" and exiles ", 5usize),
        (" and reveal ", 5usize),
        (" and reveals ", 5usize),
        (" and shuffle", 5usize),
        (" and shuffles", 5usize),
        (" then shuffle", 1usize),
        (" then shuffles", 1usize),
    ] {
        if let Some(idx) = rest.find(needle) {
            let take = match best_split {
                Some((cur_idx, _)) => idx < cur_idx,
                None => true,
            };
            if take {
                best_split = Some((idx, skip));
            }
        }
    }
    let (flt_part_raw, tail) = match best_split {
        Some((idx, skip)) => (rest[..idx].trim(), rest[idx + skip..].trim()),
        None => (rest.trim(), ""),
    };

    if flt_part_raw.ends_with(" instead of one")
        || flt_part_raw.starts_with("an additional ")
        || flt_part_raw.contains("different name")
        || flt_part_raw.contains("different mana value")
        || flt_part_raw.contains("different color")
        || flt_part_raw.contains("different card type")
        || flt_part_raw.contains(" and a ")
        || flt_part_raw.contains(" and an ")
    {
        return None;
    }

    if tail.contains("and the other")
        || tail.contains("and the rest")
        || tail.contains("with a ")
        || tail.contains("with an ")
        || tail.contains("unless ")
        || tail.contains("if you do")
        || tail.contains("if it's")
        || tail.contains("for each ")
        || tail.contains("lose ")
        || tail.contains("loses ")
        || tail.contains("gain ")
        || tail.contains("gains ")
        || tail.contains("draw ")
        || tail.contains("draws ")
        || tail.contains("discard ")
        || tail.contains("discards ")
        || tail.contains("sacrifice ")
        || tail.contains("sacrifices ")
        || tail.contains("pay ")
        || tail.contains("pays ")
        || tail.contains("cast ")
        || tail.contains("casts ")
        || tail.contains(" play ")
        || tail.contains(" plays ")
        || tail.contains("create ")
        || tail.contains("creates ")
        || tail.contains("destroy ")
        || tail.contains("destroys ")
        || tail.contains("damage")
        || tail.contains("counter")
    {
        return None;
    }

    let shuffle = tail.contains("shuffle");
    let reveal = tail.starts_with("reveal ")
        || tail.starts_with("reveals ")
        || tail.contains(" reveal ")
        || tail.contains(" reveals ");

    let any_number = flt_part_raw.starts_with("any number of ");
    let up_to = flt_part_raw.starts_with("up to ");
    let flt_clean = flt_part_raw
        .strip_prefix("any number of ")
        .or_else(|| flt_part_raw.strip_prefix("up to "))
        .or_else(|| flt_part_raw.strip_prefix("exactly "))
        .unwrap_or(flt_part_raw);
    let (mut cnt, flt_raw) = if any_number {
        (ValueExprAst::Const(0), flt_clean)
    } else {
        peel_leading_count(flt_clean, vocab)
    };
    let flt = if let Some((type_part, name_raw)) = flt_raw.split_once(" named ") {
        let n = name_raw.trim();
        if n.is_empty()
            || n.contains(" or a ")
            || n.contains(" or an ")
            || n.contains(" and a ")
            || n.contains(" and an ")
        {
            return None;
        }
        let mut base_flt = if matches!(type_part.trim(), "card" | "cards") {
            ObjectFilterAst {
                any_card: true,
                ..ObjectFilterAst::default()
            }
        } else {
            parse_object_filter(type_part.trim(), vocab).ok()?
        };
        let nf = if n == "~" {
            crate::ast::NameFilterAst::SelfName
        } else {
            crate::ast::NameFilterAst::Exact(n.to_string())
        };
        base_flt.named = Some(nf);
        base_flt
    } else if flt_raw == "card with the same name as ~"
        || flt_raw == "cards with the same name as ~"
    {
        ObjectFilterAst {
            any_card: true,
            named: Some(crate::ast::NameFilterAst::SelfName),
            ..ObjectFilterAst::default()
        }
    } else if flt_raw == "card with the same name as that card"
        || flt_raw == "cards with the same name as that card"
        || flt_raw == "card with the same name as the exiled card"
        || flt_raw == "card with the same name as the chosen card"
    {
        ObjectFilterAst {
            any_card: true,
            named: Some(crate::ast::NameFilterAst::SameAsReferenced),
            ..ObjectFilterAst::default()
        }
    } else if flt_raw.contains("with the same name as ") {
        return None;
    } else if let Some(after_num) = flt_raw.strip_prefix("number of ")
        && let Some((inner_flt, eq_val)) = after_num.split_once(" equal to ")
        && let Ok(parsed_cnt) = parse_value_expr(eq_val, vocab)
        && let Ok(f) = parse_object_filter(inner_flt, vocab)
    {
        cnt = parsed_cnt;
        f
    } else {
        parse_object_filter(flt_raw, vocab).ok()?
    };

    let has_exile = tail.contains("exile it")
        || tail.contains("exiles it")
        || tail.contains("exile that card")
        || tail.contains("exiles that card")
        || tail.contains("exile them")
        || tail.contains("exiles them")
        || tail.contains("exile those cards")
        || tail.contains("exiles those cards");
    let to_zone = if tail.contains("the battlefield tapped and attacking") {
        "the battlefield tapped and attacking"
    } else if tail.contains("the battlefield tapped under your control") {
        "the battlefield tapped under your control"
    } else if tail.contains("the battlefield tapped") {
        "the battlefield tapped"
    } else if tail.contains("the battlefield under your control") {
        "the battlefield under your control"
    } else if tail.contains("the battlefield") {
        "the battlefield"
    } else if tail.contains("your hand") {
        "your hand"
    } else if tail.contains("their hand") {
        "their hand"
    } else if tail.contains("that player's hand") {
        "that player's hand"
    } else if tail.contains("its owner's hand") {
        "its owner's hand"
    } else if tail.contains("your graveyard") {
        "your graveyard"
    } else if tail.contains("their graveyard") {
        "their graveyard"
    } else if tail.contains("that player's graveyard") {
        "that player's graveyard"
    } else if tail.contains("top of your library in any order") {
        "top of your library in any order"
    } else if tail.contains("top of their library in any order") {
        "top of their library in any order"
    } else if tail.contains("top of their library") {
        "top of their library"
    } else if tail.contains("top of your library")
        || tail.contains("on top")
        || tail.contains("top of it")
    {
        "top of your library"
    } else if tail.contains("bottom of your library") {
        "bottom of your library"
    } else if tail.contains("bottom of their library") {
        "bottom of their library"
    } else if has_exile && tail.contains("face down") {
        "exile face down"
    } else if has_exile {
        "exile"
    } else if tail.contains("put ") || tail.contains("puts ") || tail.contains("exile") {
        return None;
    } else {
        "found_set"
    };

    let search_ins = InstructionAst::SearchLibrary {
        library: lib_who.trim_end_matches("'s").to_string(),
        count: Box::new(cnt),
        up_to,
        any_number,
        filter: Box::new(flt),
        reveal,
        to_zone: to_zone.to_string(),
        shuffle,
    };
    Some(match opt_player {
        Some(PlayerRefAst::You) => InstructionAst::Optional(vec![search_ins]),
        Some(player) => InstructionAst::OptionalPlayer {
            player,
            body: vec![search_ins],
        },
        None => search_ins,
    })
}

fn try_parse_library_ops(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    if let Some((who, amt_raw)) = split_leading_player_verb(low, "scry", "scries")
        && let Ok(cnt) = parse_value_expr(amt_raw, vocab)
    {
        return Some(InstructionAst::Scry {
            player: who,
            count: Box::new(cnt),
        });
    }
    if let Some((who, amt_raw)) = split_leading_player_verb(low, "surveil", "surveils")
        && let Ok(cnt) = parse_value_expr(amt_raw, vocab)
    {
        return Some(InstructionAst::Surveil {
            player: who,
            count: Box::new(cnt),
        });
    }
    if let Some((who, after)) = split_leading_player_verb(low, "mill", "mills") {
        let cnt_opt = if let Some(after_half) = after.strip_prefix("half ")
            && (after_half.starts_with("their library") || after_half.starts_with("your library"))
        {
            let rounding = if after_half.contains("rounded up") {
                crate::ast::Rounding::Up
            } else {
                crate::ast::Rounding::Down
            };
            Some(ValueExprAst::DivConst {
                expr: Box::new(ValueExprAst::PlayerStat {
                    player: who,
                    stat: crate::ast::PlayerStatKind::CardsInLibrary,
                }),
                denom: 2,
                rounding,
            })
        } else if let Some(val_raw) = after
            .strip_prefix("cards equal to ")
            .or_else(|| after.strip_prefix("a number of cards equal to "))
        {
            parse_value_expr(val_raw, vocab).ok()
        } else if let Some(cnt_raw) = after
            .strip_suffix(" cards")
            .or_else(|| after.strip_suffix(" card"))
        {
            parse_value_expr(cnt_raw, vocab).ok()
        } else {
            None
        };
        if let Some(cnt) = cnt_opt {
            return Some(InstructionAst::Mill {
                player: who,
                count: Box::new(cnt),
            });
        }
    }
    if let Some((who, rev_rest)) = split_leading_player_verb(low, "reveal", "reveals")
        && matches!(
            rev_rest,
            "it" | "them"
                | "that card"
                | "those cards"
                | "the chosen card"
                | "the chosen cards"
                | "the exiled card"
                | "the exiled cards"
                | "the top card"
        )
    {
        return Some(InstructionAst::LookTopLibrary {
            reveal: true,
            library: if who == PlayerRefAst::You {
                "ref".to_string()
            } else {
                format!("ref:{who:?}")
            },
            count: Box::new(ValueExprAst::Const(1)),
        });
    }
    if let Some(after) = low.strip_prefix("reveal ") {
        if let Some(flt_part) = after.strip_suffix(" from among them") {
            let trimmed = flt_part.trim();
            let clean = trimmed.strip_prefix("up to ").unwrap_or(trimmed);
            let (cnt_ast, flt_raw) = peel_leading_count(clean, vocab);
            let to_tag = if trimmed.starts_with("any number ") {
                "revealed:any_number".to_string()
            } else if trimmed.starts_with("all ") {
                "revealed:all".to_string()
            } else if let ValueExprAst::Const(n) = cnt_ast
                && n >= 2
            {
                format!("revealed:{n}")
            } else {
                "revealed".to_string()
            };
            if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
                return Some(InstructionAst::RevealFromLooked {
                    filter: Box::new(flt),
                    to: to_tag,
                });
            }
        }
        if after.starts_with("target ") && parse_subject_or_target(after, vocab).is_ok() {
            return Some(InstructionAst::LookTopLibrary {
                reveal: true,
                library: "target".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            });
        }
    }
    if let Some((who, rest)) = split_leading_player_verb(low, "reveal", "reveals")
        && let Some(whose_lib) = rest.strip_prefix("the top card of ")
        && (whose_lib.ends_with(" library") || whose_lib.ends_with(" libraries"))
    {
        let lib_str = if who == PlayerRefAst::You {
            "your".to_string()
        } else {
            format!("{who:?}")
        };
        return Some(InstructionAst::LookTopLibrary {
            reveal: true,
            library: lib_str,
            count: Box::new(ValueExprAst::Const(1)),
        });
    }
    if let Some(after) = low.strip_prefix("look at ")
        && let Some((cnt_raw, lib_rest)) = after.split_once(" cards from the top of ")
        && let Some(whose) = lib_rest.strip_suffix(" library")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::LookTopLibrary {
            reveal: false,
            library: whose.trim_end_matches("'s").to_string(),
            count: Box::new(cnt),
        });
    }
    if let Some((who_raw, rest)) = low
        .split_once(" reveals ")
        .or_else(|| low.split_once(" reveal "))
        && parse_player_ref(who_raw).is_some()
    {
        if let Some(val_raw) = rest.strip_prefix("a number of cards from their hand equal to ")
            && let Ok(cnt) = parse_value_expr(val_raw, vocab)
        {
            return Some(InstructionAst::LookTopLibrary {
                reveal: true,
                library: "hand".to_string(),
                count: Box::new(cnt),
            });
        }
        if let Some(after_top) = rest.strip_prefix("the top ")
            && let Some((cnt_raw, lib_tail)) = after_top
                .split_once(" cards of ")
                .or_else(|| after_top.split_once(" card of "))
            && let Some(whose) = lib_tail.strip_suffix(" library")
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            return Some(InstructionAst::LookTopLibrary {
                reveal: true,
                library: whose.trim_end_matches("'s").to_string(),
                count: Box::new(cnt),
            });
        }
        if let Some(cnt_raw) = rest
            .strip_suffix(" cards from their hand")
            .or_else(|| rest.strip_suffix(" card from their hand"))
            .or_else(|| rest.strip_suffix(" cards from your hand"))
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            return Some(InstructionAst::LookTopLibrary {
                reveal: true,
                library: "hand".to_string(),
                count: Box::new(cnt),
            });
        }
    }
    if let Some((who_raw, rest)) = low
        .split_once(" reveals a card at random from ")
        .or_else(|| low.split_once(" reveal a card at random from "))
        && rest.ends_with(" hand")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::RevealRandomFromHand { player });
    }
    for (pref, reveal) in [("look at the top ", false), ("reveal the top ", true)] {
        if let Some(rest) = low.strip_prefix(pref) {
            if let Some(whose_lib) = rest.strip_prefix("card of ")
                && let Some(whose) = whose_lib.strip_suffix(" library")
            {
                return Some(InstructionAst::LookTopLibrary {
                    reveal,
                    library: whose.trim_end_matches("'s").to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                });
            }
            if let Some((cnt_str, whose_lib)) = rest.split_once(" cards of ")
                && let Some(whose) = whose_lib.strip_suffix(" library")
                && let Ok(cnt) = parse_value_expr(cnt_str, vocab)
            {
                return Some(InstructionAst::LookTopLibrary {
                    reveal,
                    library: whose.trim_end_matches("'s").to_string(),
                    count: Box::new(cnt),
                });
            }
        }
    }
    // Exile/Reveal from top of library until ...
    for (pref, exile) in [
        ("exile cards from the top of ", true),
        ("reveal cards from the top of ", false),
    ] {
        if let Some(rest) = low.strip_prefix(pref)
            && let Some((lib_raw, until_rest)) = rest.split_once(" library until you ")
        {
            let flt_raw = until_rest
                .strip_prefix("exile ")
                .or_else(|| until_rest.strip_prefix("reveal "))
                .unwrap_or(until_rest);
            let (_, flt_nocnt) = peel_leading_count(flt_raw, vocab);
            if let Ok(flt) = parse_object_filter(flt_raw, vocab)
                .or_else(|_| parse_object_filter(flt_nocnt, vocab))
            {
                return Some(InstructionAst::ExileOrRevealTopUntil {
                    exile,
                    library: lib_raw.trim_end_matches("'s").to_string(),
                    filter: Box::new(flt),
                });
            }
        }
    }

    // Look at <player>'s hand
    if let Some(rest) = low.strip_prefix("look at ")
        && let Some(owner_raw) = rest.strip_suffix("'s hand")
        && let Some(p) = parse_player_ref(owner_raw)
    {
        return Some(InstructionAst::LookAtHand { player: p });
    }

    // Wheel: "each player shuffles their hand and graveyard into their library, then draws N cards"
    if let Some((who_raw, rest)) = low
        .split_once(" shuffles their hand and graveyard into their library, then draws ")
        .or_else(|| {
            low.split_once(" shuffle their hand and graveyard into their library, then draw ")
        })
        && let Some(cnt_raw) = rest.strip_suffix(" cards")
        && let (Some(player), Ok(draw)) =
            (parse_player_ref(who_raw), parse_value_expr(cnt_raw, vocab))
    {
        return Some(InstructionAst::WheelShuffleHandAndGraveyard {
            player,
            draw: Box::new(draw),
        });
    }

    // Imperative "shuffle your graveyard into your library" / "shuffle ~ and your graveyard into their owner's library" (Campfire, Elixir of Immortality)
    if low == "shuffle your graveyard into your library"
        || low == "shuffle ~ and your graveyard into their owner's library"
    {
        return Some(InstructionAst::ShuffleGraveyardIntoLibrary {
            player: PlayerRefAst::You,
        });
    }
    // "<player> shuffles their graveyard into their library"
    if let Some((who_raw, rest)) = low
        .split_once(" shuffles ")
        .or_else(|| low.split_once(" shuffle "))
        && matches!(
            rest,
            "their graveyard into their library"
                | "your graveyard into your library"
                | "his or her graveyard into his or her library"
        )
        && let Some(p) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::ShuffleGraveyardIntoLibrary { player: p });
    }

    if low == "reveal your hand" {
        return Some(InstructionAst::RevealHand {
            player: PlayerRefAst::You,
        });
    }
    if let Some((who_raw, hand_part)) = low
        .split_once(" reveals ")
        .or_else(|| low.split_once(" reveal "))
        && matches!(hand_part, "their hand" | "his or her hand" | "your hand")
        && let Some(p) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::RevealHand { player: p });
    }
    None
}

fn try_parse_misc_actions(
    _s: &str,
    s_nodur: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<InstructionAst> {
    // Switch P/T: "Switch <subj>'s power and toughness [until end of turn]" / "Switch its power and toughness"
    let snlow = s_nodur.to_ascii_lowercase();
    if snlow == "switch its power and toughness" {
        return Some(InstructionAst::SwitchPowerToughness {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            duration: dur.clone(),
        });
    }
    if let Some(rest) = snlow.strip_prefix("switch ")
        && let Some(subj_raw) = rest.strip_suffix("'s power and toughness")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::SwitchPowerToughness {
            subject: Box::new(subj),
            duration: dur.clone(),
        });
    }

    // "<subj> becomes the <characteristic> of your choice"
    if let Some((subj_raw, kind_raw)) = snlow
        .split_once(" becomes the ")
        .or_else(|| snlow.split_once(" become the "))
        && let Some(kind) = kind_raw.strip_suffix(" of your choice")
        && matches!(
            kind,
            "creature type" | "basic land type" | "color" | "color or colors"
        )
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::BecomeChosenCharacteristic {
            subject: Box::new(subj),
            kind: kind.to_string(),
            duration: dur.clone(),
        });
    }

    if let Some(rest) = low.strip_prefix("tap or untap ")
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::TapOrUntapPermanent {
            subject: Box::new(subj),
        });
    }
    if let Some(rest) = low.strip_prefix("tap ")
        && !rest.starts_with("any number of ")
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::TapPermanent {
            subject: Box::new(subj),
        });
    }
    if let Some(rest) = low.strip_prefix("remove ")
        && let Some(subj_raw) = rest.strip_suffix(" from combat")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::RemoveFromCombat {
            subject: Box::new(subj),
        });
    }
    if let Some(after) = low.strip_prefix("the owner of ")
        && let Some((subj_raw, tail)) = after.split_once(" shuffles ")
        && (tail == "it into their library" || tail == "them into their library")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(subj),
        });
    }
    if let Some((subj_raw, tail)) = low.split_once("'s owner shuffles ")
        && (tail == "it into their library" || tail == "them into their library")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(subj),
        });
    }
    if let Some(rest) = low.strip_prefix("untap ")
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::UntapPermanent {
            subject: Box::new(subj),
        });
    }
    if let Some((who_raw, rest)) = low
        .split_once(" shuffles ")
        .or_else(|| low.split_once(" shuffle "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        if rest == "their hand and graveyard into their library"
            || rest == "the cards from their hand into their library"
        {
            return Some(InstructionAst::WheelShuffleHandAndGraveyard {
                player,
                draw: Box::new(ValueExprAst::Const(0)),
            });
        }
        if let Some(subj_raw) = rest
            .strip_suffix(" into their library")
            .or_else(|| rest.strip_suffix(" into your library"))
            .or_else(|| rest.strip_suffix(" into its owner's library"))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Some(InstructionAst::ShuffleIntoLibrary {
                subject: Box::new(subj),
            });
        }
    }
    if let Some(rest) = low.strip_prefix("shuffle ")
        && let Some(subj_raw) = rest
            .strip_suffix(" into its owner's library")
            .or_else(|| rest.strip_suffix(" into their owners' libraries"))
            .or_else(|| rest.strip_suffix(" into your library"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low
        .strip_suffix(" doesn't untap during its controller's next untap step")
        .or_else(|| low.strip_suffix(" don't untap during their controllers' next untap steps"))
        .or_else(|| low.strip_suffix(" don't untap during their controller's next untap step"))
        .or_else(|| low.strip_suffix(" don't untap during that player's next untap step"))
        .or_else(|| low.strip_suffix(" doesn't untap during that player's next untap step"))
        .or_else(|| low.strip_suffix(" don't untap during target player's next untap step"))
        .or_else(|| low.strip_suffix(" don't untap during their next untap step"))
        .or_else(|| low.strip_suffix(" doesn't untap during your next untap step"))
        .or_else(|| low.strip_suffix(" don't untap during your next untap step"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::SkipNextUntap {
            subject: Box::new(subj),
        });
    }
    if let Some(after) = low.strip_prefix("during that player's next untap step, ")
        && let Some(subj_raw) = after
            .strip_suffix(" don't untap")
            .or_else(|| after.strip_suffix(" doesn't untap"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::SkipNextUntap {
            subject: Box::new(subj),
        });
    }
    if let Some(rest) = low
        .strip_prefix("change the target of ")
        .or_else(|| low.strip_prefix("change any targets of "))
        .or_else(|| low.strip_prefix("choose new targets for "))
    {
        let (core, single_target_only) = match rest.strip_suffix(" with a single target") {
            Some(before) => (before, true),
            None => (rest, false),
        };
        if !core.contains(" to ")
            && !core.contains(" that targets ")
            && let Ok(mut target) = parse_subject_or_target(core, vocab)
        {
            if single_target_only {
                match &mut target {
                    SubjectRefAst::TargetObject { filter, .. } => {
                        filter.flags = filter.flags.union(ObjectFilterFlags::SINGLE_TARGET);
                    }
                    _ => return None,
                }
            }
            return Some(InstructionAst::ChangeTargets {
                target: Box::new(target),
            });
        }
    }
    if let Some(rest) = low.strip_prefix("regenerate ")
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::Regenerate {
            subject: Box::new(subj),
        });
    }
    if let Some(rest) = low
        .strip_prefix("transform ")
        .or_else(|| low.strip_prefix("flip "))
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::Transform {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low
        .strip_suffix(" phases out until ~ leaves the battlefield")
        .or_else(|| low.strip_suffix(" phase out until ~ leaves the battlefield"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::PhaseOutUntil {
            subject: Box::new(subj),
            until: DurationAst::UntilSourceLeaves,
        });
    }
    if let Some(subj_raw) = low
        .strip_suffix(" phases out")
        .or_else(|| low.strip_suffix(" phase out"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::PhaseOut {
            subject: Box::new(subj),
        });
    }
    if let Some((a_raw, b_raw)) = low
        .split_once(" fights ")
        .or_else(|| low.split_once(" fight "))
        && let (Ok(a), Ok(b)) = (
            parse_subject_or_target(a_raw, vocab),
            parse_subject_or_target(b_raw, vocab),
        )
    {
        return Some(InstructionAst::Fight {
            a: Box::new(a),
            b: Box::new(b),
        });
    }
    if let Some(after) = low.strip_prefix("attach ") {
        for split_pair in [after.split_once(" to "), after.rsplit_once(" to ")]
            .into_iter()
            .flatten()
        {
            let (a_raw, b_raw) = split_pair;
            if a_raw.starts_with("any number of ") || a_raw.starts_with("all ") {
                continue;
            }
            if let (Ok(a), Ok(b)) = (
                parse_subject_or_target(a_raw, vocab),
                parse_subject_or_target(b_raw, vocab),
            ) {
                return Some(InstructionAst::Attach {
                    subject: Box::new(a),
                    target: Box::new(b),
                });
            }
        }
    }
    if let Some(after) = low.strip_prefix("unattach ") {
        let (a_raw, b_opt) = match after.rsplit_once(" from ") {
            Some((a, b)) => (a, parse_subject_or_target(b, vocab).ok()),
            None => (after, None),
        };
        let a_clean = a_raw.strip_prefix("all ").unwrap_or(a_raw);
        if let Ok(a) = parse_subject_or_target(a_clean, vocab) {
            return Some(InstructionAst::Attach {
                subject: Box::new(a),
                target: Box::new(b_opt.unwrap_or(SubjectRefAst::Anaphora("unattach".to_string()))),
            });
        }
    }
    let snlow = s_nodur.to_ascii_lowercase();
    if let Some((who_raw, rest)) = snlow
        .split_once(" gains control of ")
        .or_else(|| snlow.split_once(" gain control of "))
        && let (Some(controller), Ok(subj)) = (
            parse_player_ref(who_raw),
            parse_subject_or_target(rest, vocab),
        )
    {
        return Some(InstructionAst::GainControl {
            controller,
            subject: Box::new(subj),
            duration: dur.clone(),
        });
    }
    if let Some(rest) = snlow.strip_prefix("gain control of ")
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            subject: Box::new(subj),
            duration: dur.clone(),
        });
    }
    // "double its power" / "double <subj>'s power and toughness" / "double the number of <kind> counters on <subj>"
    if let Some(after) = snlow.strip_prefix("double the number of ")
        && let Some((kind, subj_raw)) = after
            .split_once(" counters on ")
            .or_else(|| after.split_once(" counter on "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::DoubleStatOrCounters {
            subject: Box::new(subj),
            kind: format!("counters:{}", kind.trim()),
            duration: DurationAst::Permanent,
        });
    }
    if let Some(after) = snlow.strip_prefix("double ") {
        if let Some(subj_raw) = after
            .strip_prefix("the power and toughness of ")
            .or_else(|| after.strip_prefix("the power of "))
            .or_else(|| after.strip_prefix("the toughness of "))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let kind = if after.starts_with("the power and toughness of ") {
                "power_and_toughness"
            } else if after.starts_with("the power of ") {
                "power"
            } else {
                "toughness"
            };
            return Some(InstructionAst::DoubleStatOrCounters {
                subject: Box::new(subj),
                kind: kind.to_string(),
                duration: dur.clone(),
            });
        }
        for (suf, kind) in [
            ("'s power and toughness", "power_and_toughness"),
            ("'s power", "power"),
            ("'s toughness", "toughness"),
        ] {
            if let Some(subj_raw) = after.strip_suffix(suf)
                && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            {
                return Some(InstructionAst::DoubleStatOrCounters {
                    subject: Box::new(subj),
                    kind: kind.to_string(),
                    duration: dur.clone(),
                });
            }
        }
        if matches!(
            after,
            "its power" | "its toughness" | "its power and toughness"
        ) {
            return Some(InstructionAst::DoubleStatOrCounters {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                kind: after.trim_start_matches("its ").to_string(),
                duration: dur.clone(),
            });
        }
    }
    if let Some(after) = low.strip_prefix("prevent the next ")
        && let Some((amt_raw, _)) = after
            .split_once(" combat damage that ")
            .or_else(|| after.split_once(" damage that "))
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        let tgt = after
            .rsplit_once(" to ")
            .and_then(|(_, t)| {
                parse_subject_or_target(t.trim_end_matches(" this turn"), vocab).ok()
            })
            .unwrap_or(SubjectRefAst::AnyTarget);
        return Some(InstructionAst::PreventDamage {
            amount: Box::new(amt),
            target: Box::new(tgt),
            duration: DurationAst::UntilEndOfTurn,
        });
    }
    if low == "prevent that damage" || low == "prevent all of that damage" {
        return Some(InstructionAst::PreventAllDamage {
            combat_only: false,
            target: None,
            duration: DurationAst::Instant,
        });
    }
    if let Some(amt_raw) = low
        .strip_prefix("prevent ")
        .and_then(|r| r.strip_suffix(" of that damage"))
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        return Some(InstructionAst::PreventDamage {
            amount: Box::new(amt),
            target: Box::new(SubjectRefAst::AnyTarget),
            duration: DurationAst::Instant,
        });
    }
    if low.starts_with("prevent all ")
        && (low.contains("damage that would be dealt")
            || low.contains(" would deal ")
            || low.ends_with(" would deal this turn")
            || low.ends_with(" would deal"))
    {
        let combat_only = low.starts_with("prevent all combat ");
        let target = low
            .split_once("dealt to ")
            .or_else(|| low.split_once("deal to "))
            .and_then(|(_, r)| {
                parse_subject_or_target(r.trim_end_matches(" this turn"), vocab).ok()
            })
            .map(Box::new);
        return Some(InstructionAst::PreventAllDamage {
            combat_only,
            target,
            duration: DurationAst::UntilEndOfTurn,
        });
    }
    // "<player>'s life total becomes <val>"
    if let Some((who_raw, val_raw)) = low.split_once("'s life total becomes ")
        && let Ok(value) = parse_value_expr(val_raw, vocab)
    {
        let player = parse_player_ref(who_raw).unwrap_or(PlayerRefAst::ThatPlayer);
        return Some(InstructionAst::SetLifeTotal {
            player,
            value: Box::new(value),
        });
    }
    if let Some(val_raw) = low.strip_prefix("your life total becomes ")
        && let Ok(value) = parse_value_expr(val_raw, vocab)
    {
        return Some(InstructionAst::SetLifeTotal {
            player: PlayerRefAst::You,
            value: Box::new(value),
        });
    }
    if low.contains("choose new targets for the cop") {
        return Some(InstructionAst::MayChooseNewTargetsForCopy);
    }
    if let Some((_, rest)) = split_leading_player_verb(low, "copy", "copies") {
        let rest_clean = rest
            .split_once(" and may choose a new target for ")
            .or_else(|| rest.split_once(" and may choose new targets for "))
            .map_or(rest, |(b, _)| b.trim());
        let (core, exception) = match rest_clean.split_once(", except ") {
            Some((c, e)) => (c.trim(), Some(e.trim().to_string())),
            None => (rest_clean, None),
        };
        let (core, count) = if let Some(c) = core.strip_suffix(" twice") {
            (c.trim(), Some(Box::new(ValueExprAst::Const(2))))
        } else if let Some((c, times_raw)) = core.rsplit_once(' ')
            && times_raw == "times"
            && let Some((c2, num_tok)) = c.rsplit_once(' ')
            && let Ok(n) = parse_value_expr(num_tok, vocab)
        {
            (c2.trim(), Some(Box::new(n)))
        } else if let Some((c, _fe)) = core.split_once(" for each other ") {
            (c.trim(), Some(Box::new(ValueExprAst::PriorResult)))
        } else {
            (core, None)
        };
        if core == "the spell countered this way" {
            return Some(InstructionAst::Copy {
                subject: Box::new(SubjectRefAst::Anaphora(core.to_string())),
                exception,
                count,
            });
        }
        if let Ok(subj) = parse_subject_or_target(core, vocab) {
            return Some(InstructionAst::Copy {
                subject: Box::new(subj),
                exception,
                count,
            });
        }
    }
    None
}

fn try_parse_named_keyword_actions(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<InstructionAst> {
    match low {
        "investigate" | "investigate once" => {
            return Some(InstructionAst::Investigate {
                player: PlayerRefAst::You,
                count: Box::new(ValueExprAst::Const(1)),
            });
        }
        "investigate twice" => {
            return Some(InstructionAst::Investigate {
                player: PlayerRefAst::You,
                count: Box::new(ValueExprAst::Const(2)),
            });
        }
        "you become the monarch" | "become the monarch" => {
            return Some(InstructionAst::BecomeMonarch {
                player: PlayerRefAst::You,
            });
        }
        "you take the initiative" | "take the initiative" => {
            return Some(InstructionAst::TakeInitiative {
                player: PlayerRefAst::You,
            });
        }
        "venture into the dungeon" => {
            return Some(InstructionAst::VentureIntoDungeon {
                player: PlayerRefAst::You,
            });
        }
        "the ring tempts you" => return Some(InstructionAst::RingTemptsYou),
        "populate" => {
            return Some(InstructionAst::Populate {
                count: Box::new(ValueExprAst::Const(1)),
            });
        }
        "populate twice" => {
            return Some(InstructionAst::Populate {
                count: Box::new(ValueExprAst::Const(2)),
            });
        }
        "manifest dread" => {
            return Some(InstructionAst::Manifest {
                count: Box::new(ValueExprAst::Const(1)),
                dread: true,
            });
        }
        "manifest the top card of your library" | "manifest it" => {
            return Some(InstructionAst::Manifest {
                count: Box::new(ValueExprAst::Const(1)),
                dread: false,
            });
        }
        "cloak the top card of your library" => {
            return Some(InstructionAst::Cloak {
                count: Box::new(ValueExprAst::Const(1)),
                subject: None,
            });
        }
        "forage" => return Some(InstructionAst::Forage),
        "time travel" => return Some(InstructionAst::TimeTravel),
        "open an attraction" => return Some(InstructionAst::OpenAttraction),
        "roll to visit your attractions" => return Some(InstructionAst::RollVisitAttractions),
        "clash with an opponent" => {
            return Some(InstructionAst::Clash {
                with_opponent: true,
            });
        }
        "planeswalk" => return Some(InstructionAst::Planeswalk),
        "set the top scheme of your scheme deck in motion"
        | "set another scheme in motion"
        | "set the top two schemes of your scheme deck in motion" => {
            return Some(InstructionAst::SetNextSchemeInMotion);
        }
        "assemble a contraption" => return Some(InstructionAst::AssembleContraption),
        "unlock a locked door of ~" | "unlock a locked door you control" => {
            return Some(InstructionAst::UnlockDoor {
                subject: Box::new(SubjectRefAst::SelfObject),
            });
        }
        "learn" => return Some(InstructionAst::Learn),
        "it explores again" | "~ explores again" => {
            return Some(InstructionAst::Explore {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            });
        }
        _ => {}
    }

    if let Some(cnt_raw) = low.strip_prefix("earthbend ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Earthbend {
            count: Box::new(cnt),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("airbend ") {
        let subj = parse_subject_or_target(subj_raw, vocab).ok().map(Box::new);
        return Some(InstructionAst::Airbend { subject: subj });
    }
    if let Some(after) = low.strip_prefix("empower ") {
        let mut parts = after.rsplitn(2, ' ');
        if let Some(cnt_raw) = parts.next()
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            let pw = parts.next().unwrap_or("").trim().to_string();
            if !pw.is_empty() {
                return Some(InstructionAst::Empower {
                    planeswalker: pw,
                    count: Box::new(cnt),
                });
            }
        }
    }
    if let Some(cnt_raw) = low.strip_prefix("teamwork ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Teamwork {
            count: Box::new(cnt),
        });
    }

    // Investigate with explicit player or count ("each player investigates", "investigate X times")
    if let Some(who_raw) = low
        .strip_suffix(" investigates")
        .or_else(|| low.strip_suffix(" investigate"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::Investigate {
            player,
            count: Box::new(ValueExprAst::Const(1)),
        });
    }
    if let Some(after) = low.strip_prefix("investigate ")
        && let Some(cnt_raw) = after.strip_suffix(" times")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Investigate {
            player: PlayerRefAst::You,
            count: Box::new(cnt),
        });
    }

    // Monarch / Initiative / Venture with explicit player
    if let Some(who_raw) = low
        .strip_suffix(" becomes the monarch")
        .or_else(|| low.strip_suffix(" become the monarch"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::BecomeMonarch { player });
    }
    if let Some(who_raw) = low
        .strip_suffix(" takes the initiative")
        .or_else(|| low.strip_suffix(" take the initiative"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::TakeInitiative { player });
    }
    if let Some(who_raw) = low
        .strip_suffix(" ventures into the dungeon")
        .or_else(|| low.strip_suffix(" venture into the dungeon"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Some(InstructionAst::VentureIntoDungeon { player });
    }

    // Player counters: Energy `{E}{E}`, Tickets `{TK}{TK}`, Poison, Rad, Experience
    if let Some((who, after)) = split_leading_player_verb(s, "get", "gets") {
        if after == "that many {E}" {
            return Some(InstructionAst::GainPlayerCounters {
                player: who,
                counter_kind: "energy".to_string(),
                count: Box::new(ValueExprAst::PriorResult),
            });
        }
        if let Some(n) = count_symbol_sequence(after, "{E}") {
            return Some(InstructionAst::GainPlayerCounters {
                player: who,
                counter_kind: "energy".to_string(),
                count: Box::new(ValueExprAst::Const(i32::from(n))),
            });
        }
        if let Some(n) = count_symbol_sequence(after, "{TK}") {
            return Some(InstructionAst::GainPlayerCounters {
                player: who,
                counter_kind: "ticket".to_string(),
                count: Box::new(ValueExprAst::Const(i32::from(n))),
            });
        }
        let alow = after.to_ascii_lowercase();
        if let Some(after_num) = alow.strip_prefix("a number of ")
            && let Some((kind, val_raw)) = after_num.split_once(" counters equal to ")
            && matches!(
                kind.trim(),
                "poison" | "rad" | "experience" | "energy" | "ticket"
            )
            && let Ok(cnt) = parse_value_expr(val_raw, vocab)
        {
            return Some(InstructionAst::GainPlayerCounters {
                player: who,
                counter_kind: kind.trim().to_string(),
                count: Box::new(cnt),
            });
        }
        if let Some(left) = alow
            .strip_suffix(" counters")
            .or_else(|| alow.strip_suffix(" counter"))
        {
            let (cnt, kind) = peel_leading_count(left, vocab);
            if matches!(kind, "poison" | "rad" | "experience" | "energy" | "ticket") {
                return Some(InstructionAst::GainPlayerCounters {
                    player: who,
                    counter_kind: kind.to_string(),
                    count: Box::new(cnt),
                });
            }
        }
    }

    // Numeric keyword actions
    if let Some(cnt_raw) = low.strip_prefix("monstrosity ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Monstrosity {
            count: Box::new(cnt),
        });
    }
    if let Some(cnt_raw) = low.strip_prefix("adapt ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Adapt {
            count: Box::new(cnt),
        });
    }
    if let Some(after) = low.strip_prefix("amass ") {
        let mut parts = after.rsplitn(2, ' ');
        if let Some(cnt_raw) = parts.next()
            && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
        {
            let subtype = parts.next().unwrap_or("Zombies").trim().to_string();
            return Some(InstructionAst::Amass {
                subtype,
                count: Box::new(cnt),
            });
        }
    }
    if let Some(cnt_raw) = low.strip_prefix("incubate ") {
        let clean = cnt_raw.strip_suffix(" twice").unwrap_or(cnt_raw).trim();
        if let Ok(cnt) = parse_value_expr(clean, vocab) {
            return Some(InstructionAst::Incubate {
                count: Box::new(cnt),
            });
        }
    }
    if let Some((_, val_raw)) = split_leading_player_verb(low, "discover", "discovers")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Some(InstructionAst::Discover {
            value: Box::new(val),
        });
    }
    if let Some(val_raw) = low.strip_prefix("collect evidence ")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Some(InstructionAst::CollectEvidence {
            value: Box::new(val),
        });
    }
    if let Some(cnt_raw) = low.strip_prefix("bolster ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Bolster {
            count: Box::new(cnt),
        });
    }
    if let Some(cnt_raw) = low.strip_prefix("support ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Support {
            count: Box::new(cnt),
        });
    }
    if let Some(cnt_raw) = low.strip_prefix("fateseal ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Fateseal {
            count: Box::new(cnt),
        });
    }
    if let Some((_, cnt_raw)) = low
        .split_once(" endures ")
        .or_else(|| low.split_once(" endure "))
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Endure {
            count: Box::new(cnt),
        });
    }
    if let Some(cnt_raw) = low.strip_prefix("endure ")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Endure {
            count: Box::new(cnt),
        });
    }
    if let Some((who, cnt_raw)) = split_leading_player_verb(low, "blight", "blights")
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return Some(InstructionAst::Blight {
            player: who,
            count: Box::new(cnt),
        });
    }

    // Subject-targeted keyword actions
    if let Some(subj_raw) = low
        .strip_suffix(" explores")
        .or_else(|| low.strip_suffix(" explore"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::Explore {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low
        .strip_suffix(" becomes prepared")
        .or_else(|| low.strip_suffix(" become prepared"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::BecomePrepared {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("detain ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::Detain {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("suspect ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::Suspect {
            subject: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("cloak ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::Cloak {
            count: Box::new(ValueExprAst::Const(1)),
            subject: Some(Box::new(subj)),
        });
    }
    if let Some(subj_raw) = low.strip_prefix("exert ")
        && let Ok(subj) = parse_subject_or_target(
            subj_raw.strip_suffix(" as it attacks").unwrap_or(subj_raw),
            vocab,
        )
    {
        return Some(InstructionAst::Exert {
            subject: Box::new(subj),
        });
    }
    if let Some((subj_raw, tail)) = low
        .split_once(" connives")
        .or_else(|| low.split_once(" connive"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let cnt = if tail.trim().is_empty() {
            ValueExprAst::Const(1)
        } else {
            parse_value_expr(tail.trim(), vocab).ok()?
        };
        return Some(InstructionAst::Connive {
            subject: Box::new(subj),
            count: Box::new(cnt),
        });
    }

    None
}

fn try_parse_combat_and_animate(
    s_nodur: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<InstructionAst> {
    let snlow = s_nodur.to_ascii_lowercase();
    for rule in [
        "can't be blocked",
        "can't be countered",
        "can't be equipped",
        "can't be sacrificed",
        "can't be the targets of spells or abilities",
        "can't be the target of spells or abilities",
        "can't be the target of spells",
        "can't block",
        "can't attack",
        "can't attack you or planeswalkers you control",
        "can't attack you or a planeswalker you control",
        "can't attack its owner or planeswalkers its owner controls",
        "can't be turned face up",
        "can't be enchanted",
        "can't be enchanted by other auras",
        "isn't a planeswalker",
        "isn't an artifact",
        "isn't a creature",
        "isn't a land",
        "isn't an enchantment",
        "gains all creature types",
        "gain all creature types",
        "can't phase in",
        "becomes foretold",
        "becomes unprepared",
        "can't attack or block",
        "can't attack or block, and its activated abilities can't be activated",
        "can't attack or block and its activated abilities can't be activated",
        "can't phase out",
        "can't be regenerated",
        "blocks if able",
        "blocks this turn if able",
        "block this turn if able",
        "attacks if able",
        "attack if able",
        "attacks this turn if able",
        "attack this turn if able",
        "attacks this combat if able",
        "attack this combat if able",
        "attacks each combat if able",
        "attack each combat if able",
        "attacks a player each combat if able",
        "attacks that player this combat if able",
        "must be blocked this turn if able",
        "blocks ~ this turn if able",
        "blocks ~ if able",
        "assigns no combat damage",
        "can attack as though it didn't have defender",
        "can attack this turn as though it didn't have defender",
        "can attack as though they didn't have defender",
        "can attack this turn as though they didn't have defender",
        "can block an additional creature",
        "can block an additional creature this combat",
    ] {
        if let Some(subj_raw) = snlow.strip_suffix(&format!(" {rule}"))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Some(InstructionAst::CombatRuleMod {
                subject: Box::new(subj),
                rule: rule.to_string(),
                duration: dur.clone(),
            });
        }
    }
    if let Some(rest) = low.strip_prefix("goad ")
        && let Ok(subj) = parse_subject_or_target(rest, vocab)
    {
        return Some(InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "goad".to_string(),
            duration: DurationAst::UntilYourNextTurn,
        });
    }
    // CR 509 General forced-block combat rule mod:
    // "<subj1> blocks/block <subj2> [this combat|this turn] if able"
    // "you may have <subj1> block <subj2> [this combat|this turn] if able"
    // (Impetuous Devils, Monstrous Step, Tolsimir, Grappling Hook)
    let forced_block_core = low
        .strip_prefix("you may have ")
        .or_else(|| low.strip_prefix("have "))
        .unwrap_or(low);
    if let Some(before_able) = forced_block_core
        .strip_suffix(" this combat if able")
        .or_else(|| forced_block_core.strip_suffix(" this turn if able"))
        .or_else(|| forced_block_core.strip_suffix(" if able"))
        && let Some((b_raw, a_raw)) = before_able
            .split_once(" blocks ")
            .or_else(|| before_able.split_once(" block "))
        && let (Ok(blocker), Ok(_attacker)) = (
            parse_subject_or_target(b_raw, vocab),
            parse_subject_or_target(a_raw, vocab),
        )
    {
        return Some(InstructionAst::CombatRuleMod {
            subject: Box::new(blocker),
            rule: format!("must_block:{a_raw}"),
            duration: dur.clone(),
        });
    }
    // CR 508 Attacking redirection / status assignment ("Those creatures are now attacking that player", Tahngarth)
    if let Some((subj_raw, who_raw)) = low
        .split_once(" are now attacking ")
        .or_else(|| low.split_once(" is now attacking "))
        .or_else(|| low.split_once(" is attacking "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && (parse_player_ref(who_raw).is_some() || parse_subject_or_target(who_raw, vocab).is_ok())
    {
        return Some(InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: format!("attacking:{who_raw}"),
            duration: DurationAst::UntilEndOfCombat,
        });
    }

    // CR 608.2h / CR 613.4b dynamic P/T assignment (live or LKI):
    // "<subj>'s [base] power (is|becomes) <val>" / "its power is equal to <val>"
    if let Some((subj_raw, rhs)) = snlow
        .split_once("'s base power becomes ")
        .or_else(|| snlow.split_once("'s base power is "))
        .or_else(|| snlow.split_once("'s power becomes "))
        .or_else(|| snlow.split_once("'s power is "))
        .or_else(|| snlow.split_once(" has base power "))
        .or_else(|| snlow.split_once(" have base power "))
        .or_else(|| snlow.strip_prefix("its power is ").map(|r| ("it", r)))
        .or_else(|| snlow.strip_prefix("its power becomes ").map(|r| ("it", r)))
        .or_else(|| snlow.strip_prefix("its base power is ").map(|r| ("it", r)))
        .or_else(|| {
            snlow
                .strip_prefix("its base power becomes ")
                .map(|r| ("it", r))
        })
        && let (Ok(subj), Ok(val)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_value_expr(rhs, vocab),
        )
    {
        return Some(InstructionAst::SetDynamicPT {
            subject: Box::new(subj),
            power: Some(Box::new(val)),
            toughness: None,
            duration: dur.clone(),
        });
    }
    if let Some((subj_raw, rhs)) = snlow
        .split_once("'s base toughness becomes ")
        .or_else(|| snlow.split_once("'s base toughness is "))
        .or_else(|| snlow.split_once("'s toughness becomes "))
        .or_else(|| snlow.split_once("'s toughness is "))
        .or_else(|| snlow.split_once(" has base toughness "))
        .or_else(|| snlow.split_once(" have base toughness "))
        .or_else(|| snlow.strip_prefix("its toughness is ").map(|r| ("it", r)))
        .or_else(|| {
            snlow
                .strip_prefix("its toughness becomes ")
                .map(|r| ("it", r))
        })
        .or_else(|| {
            snlow
                .strip_prefix("its base toughness is ")
                .map(|r| ("it", r))
        })
        .or_else(|| {
            snlow
                .strip_prefix("its base toughness becomes ")
                .map(|r| ("it", r))
        })
        && let (Ok(subj), Ok(val)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_value_expr(rhs, vocab),
        )
    {
        return Some(InstructionAst::SetDynamicPT {
            subject: Box::new(subj),
            power: None,
            toughness: Some(Box::new(val)),
            duration: dur.clone(),
        });
    }
    // CR 611/613 Dynamic Animate/Become Effects:
    // - "<subj> becomes/become a/an <desc> with base power and toughness P/T[, <kws>][, and <kws>]" (Bogardan Dragonheart, Eccentric Apprentice)
    // - "<subj> becomes/become a/an <desc> with [base] power and toughness [each] equal to <val> [and gains <kws>]" (Gideon, Veiled Sentry, Fractalize, Woodcaller Automaton)
    // - "<subj> becomes/become a/an <desc> in addition to his/her/its other types and gains <kws>" (Sarkhan, Dragon Ascendant)
    // - "<subj> has/have base power and toughness P/T and becomes a <desc> in addition to its other [creature] types" (Allosaurus Shepherd)
    if let Some((subj_raw, rhs)) = snlow
        .split_once(" becomes a ")
        .or_else(|| snlow.split_once(" becomes an "))
        .or_else(|| snlow.split_once(" become "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        if let Some((ty_part, rest)) = rhs.split_once(" with base power and toughness ") {
            let pt_tok = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches(',');
            if let Some((p, t)) = parse_base_pt(pt_tok) {
                let tail = rest[pt_tok.len()..].trim().trim_start_matches(',').trim();
                let tail_clean = tail.strip_prefix("and ").unwrap_or(tail).trim();
                let kws = if tail_clean.is_empty() {
                    Some(Vec::new())
                } else {
                    parse_keyword_list(tail_clean, vocab)
                };
                if let Some(keywords) = kws {
                    return Some(InstructionAst::AnimateOrSetBasePT {
                        subject: Box::new(subj),
                        power: p,
                        toughness: t,
                        description: Some(ty_part.trim().to_string()),
                        keywords,
                        duration: dur.clone(),
                    });
                }
            }
            if rest.starts_with("each equal to ") || rest.starts_with("equal to ") {
                return Some(InstructionAst::AnimateOrSetBasePT {
                    subject: Box::new(subj),
                    power: "X".to_string(),
                    toughness: "X".to_string(),
                    description: Some(rhs.to_string()),
                    keywords: Vec::new(),
                    duration: dur.clone(),
                });
            }
        }
        if let Some((ty_part, rest)) = rhs.split_once(" with power and toughness each equal to ") {
            let _ = ty_part;
            let _ = rest;
            return Some(InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(subj),
                power: "X".to_string(),
                toughness: "X".to_string(),
                description: Some(rhs.to_string()),
                keywords: Vec::new(),
                duration: dur.clone(),
            });
        }
        if rhs.contains(" with ") && rhs.contains("base power and toughness equal to ") {
            return Some(InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(subj),
                power: "X".to_string(),
                toughness: "X".to_string(),
                description: Some(rhs.to_string()),
                keywords: Vec::new(),
                duration: dur.clone(),
            });
        }
        if let Some((ty_part, kw_raw)) = rhs
            .split_once(" in addition to his other types and gains ")
            .or_else(|| rhs.split_once(" in addition to her other types and gains "))
            .or_else(|| rhs.split_once(" in addition to its other types and gains "))
            .or_else(|| rhs.split_once(" in addition to their other types and gain "))
            && let Some(keywords) = parse_keyword_list(kw_raw, vocab)
        {
            return Some(InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(subj),
                power: "keep".to_string(),
                toughness: "keep".to_string(),
                description: Some(ty_part.trim().to_string()),
                keywords,
                duration: dur.clone(),
            });
        }
    }
    if let Some((subj_raw, rhs)) = snlow
        .split_once(" has base power and toughness ")
        .or_else(|| snlow.split_once(" have base power and toughness "))
        && let Some((pt_tok, ty_rest)) = rhs
            .split_once(" and becomes a ")
            .or_else(|| rhs.split_once(" and become "))
        && let (Ok(subj), Some((p, t))) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_base_pt(pt_tok.trim()),
        )
    {
        return Some(InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(subj),
            power: p,
            toughness: t,
            description: Some(ty_rest.trim().to_string()),
            keywords: Vec::new(),
            duration: dur.clone(),
        });
    }

    // CR 611/613 "<subj> becomes/become a/an <desc> with \"<ab>\" [and loses all other [card types and] abilities]"
    // (Vraska Betrayal's Sting, Kitesail Larcenist, Tenth District Hero)
    if s_nodur.contains('"')
        && let Some((subj_raw, rhs)) = snlow
            .split_once(" becomes a ")
            .or_else(|| snlow.split_once(" becomes an "))
            .or_else(|| snlow.split_once(" become "))
        && let Some((ty_part, after_w)) = rhs.split_once(" with \"")
        && let Some((ab_raw, tail)) = after_w.split_once('"')
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let t_clean = tail.trim().trim_start_matches(',').trim();
        let lose_all = t_clean.contains("loses all other")
            || t_clean.contains("lose all other")
            || t_clean.contains("loses all abilities")
            || t_clean.contains("lose all abilities");
        if t_clean.is_empty() || lose_all {
            let orig_ab = s_nodur
                .split_once('"')
                .and_then(|(_, r)| r.split_once('"'))
                .map_or(ab_raw, |(inner, _)| inner);
            let compiled = compile_granted_ability(orig_ab, vocab);
            return Some(InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::Compound(vec![
                    StaticAbilityAst::ContinuousLayerEffect {
                        subject: Box::new(subj.clone()),
                        set_types: Some(ty_part.trim().to_string()),
                        additive_types: false,
                        base_pt: None,
                        set_colors: None,
                        grant_keywords: Vec::new(),
                        lose_keywords: Vec::new(),
                        lose_all_abilities: lose_all,
                        condition: None,
                    },
                    StaticAbilityAst::GrantAbilityText {
                        subject: Box::new(subj),
                        ability_text: orig_ab.to_string(),
                        compiled,
                        condition: None,
                    },
                ])),
                duration: dur.clone(),
            });
        }
    }
    if let Some((subj_raw, rhs)) = snlow
        .split_once("'s base power and toughness become ")
        .or_else(|| snlow.split_once("'s base power and toughness becomes "))
        && let (Ok(subj), Some((p, t))) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_base_pt(rhs.trim()),
        )
    {
        return Some(InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(subj),
            power: p,
            toughness: t,
            description: None,
            keywords: Vec::new(),
            duration: dur.clone(),
        });
    }
    // "double <subj>'s power [X times]" (Exponential Growth, etc.)
    if let Some(after) = snlow.strip_prefix("double ")
        && let Some((subj_raw, tail)) = after.split_once("'s power")
        && (tail.is_empty() || tail.ends_with(" times"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: format!("DoublePower{tail}"),
            duration: dur.clone(),
        });
    }
    // "<subj> [each] have/has base power and toughness P/T [and has/have <kws>]"
    if let Some((subj_raw, rhs)) = snlow
        .split_once(" each have base power and toughness ")
        .or_else(|| snlow.split_once(" have base power and toughness "))
        .or_else(|| snlow.split_once(" has base power and toughness "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let (pt_tok, kws) = if let Some((pt_side, kw_side)) = rhs
            .split_once(" and has ")
            .or_else(|| rhs.split_once(" and have "))
        {
            (
                pt_side.trim(),
                parse_keyword_list(kw_side, vocab).unwrap_or_default(),
            )
        } else {
            (rhs.trim(), Vec::new())
        };
        if let Some((p, t)) = parse_base_pt(pt_tok) {
            return Some(InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(subj),
                power: p,
                toughness: t,
                description: None,
                keywords: kws,
                duration: dur.clone(),
            });
        }
    }

    // "<subj> becomes <color(s) / type> [in addition to its other colors/types]"
    if let Some((subj_raw, rhs_raw)) = snlow
        .split_once(" becomes ")
        .or_else(|| snlow.split_once(" become "))
    {
        let rhs = rhs_raw
            .strip_suffix(" in addition to its other colors")
            .or_else(|| rhs_raw.strip_suffix(" in addition to their other colors"))
            .or_else(|| rhs_raw.strip_suffix(" in addition to its other types"))
            .or_else(|| rhs_raw.strip_suffix(" in addition to their other types"))
            .or_else(|| rhs_raw.strip_suffix(" in addition to its other creature types"))
            .or_else(|| rhs_raw.strip_suffix(" in addition to their other creature types"))
            .unwrap_or(rhs_raw)
            .trim();
        let is_color_or_type = matches!(
            rhs,
            "white"
                | "blue"
                | "black"
                | "red"
                | "green"
                | "colorless"
                | "all colors"
                | "that type"
                | "that color"
                | "the chosen type"
                | "the chosen color"
                | "the color of your choice"
                | "the color or colors of your choice"
                | "the creature type of your choice"
        ) || {
            // Multi-color list like "white and blue", "red, green, and white"
            let normalized = rhs
                .replace(", and ", " ")
                .replace(" and ", " ")
                .replace(',', " ");
            let mut words = normalized.split_whitespace().peekable();
            words.peek().is_some()
                && words.all(|w| matches!(w, "white" | "blue" | "black" | "red" | "green"))
        };
        if is_color_or_type && let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
            return Some(InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(subj),
                power: "keep".to_string(),
                toughness: "keep".to_string(),
                description: Some(rhs_raw.to_string()),
                keywords: Vec::new(),
                duration: dur.clone(),
            });
        }
    }
    // "<subj> becomes/become an artifact creature / creature / basic land"
    if let Some((subj_raw, rhs)) = snlow
        .split_once(" becomes an ")
        .or_else(|| snlow.split_once(" becomes a "))
        .or_else(|| snlow.split_once(" become "))
        && (matches!(
            rhs,
            "artifact creature"
                | "artifact creatures"
                | "creature"
                | "creatures"
                | "artifact"
                | "artifacts"
                | "plains"
                | "island"
                | "swamp"
                | "mountain"
                | "forest"
        ) || rhs.starts_with("artifact creature with power and toughness each equal to "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(subj),
            power: "keep".to_string(),
            toughness: "keep".to_string(),
            description: Some(rhs.to_string()),
            keywords: Vec::new(),
            duration: dur.clone(),
        });
    }
    // "<subj> is a <color/subtype> in addition to its other [colors and] types" (Eluge, etc.)
    if let Some((subj_raw, rhs)) = snlow
        .split_once(" is a ")
        .or_else(|| snlow.split_once(" is an "))
        && (rhs.ends_with(" in addition to its other colors and types")
            || rhs.ends_with(" in addition to its other types"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(subj),
            power: "keep".to_string(),
            toughness: "keep".to_string(),
            description: Some(rhs.to_string()),
            keywords: Vec::new(),
            duration: dur.clone(),
        });
    }
    // "<subj> doesn't/don't untap during its/their controller(s)' untap step(s) for as long as <cond>"
    if let Some((subj_raw, cond_raw)) = low
        .split_once(" doesn't untap during its controller's untap step for as long as ")
        .or_else(|| {
            low.split_once(" don't untap during their controllers' untap steps for as long as ")
        })
        && let (Ok(subj), Ok(cond)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_condition(cond_raw, vocab),
        )
    {
        return Some(InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "DoesntUntap".to_string(),
            duration: DurationAst::ForAsLongAs(Box::new(cond)),
        });
    }

    if let Some((subj_raw, blocker_raw)) = snlow.split_once(" can't be blocked by ")
        && let (Ok(subj), Ok(_flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(blocker_raw, vocab),
        )
    {
        return Some(InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: format!("CantBeBlockedBy:{blocker_raw}"),
            duration: dur.clone(),
        });
    }
    if let Some((blocker_raw, atk_raw)) = snlow.split_once(" can't block ")
        && let (Ok(blocker), Ok(_atk)) = (
            parse_subject_or_target(blocker_raw, vocab),
            parse_subject_or_target(atk_raw, vocab),
        )
    {
        return Some(InstructionAst::CombatRuleMod {
            subject: Box::new(blocker),
            rule: format!("CantBlock:{atk_raw}"),
            duration: dur.clone(),
        });
    }

    // "[have] <subj> becomes/become a copy of <source> [until end of turn][, except <exception>]"
    let copy_raw = low.strip_prefix("have ").unwrap_or(low);
    let (copy_core, copy_exception) = match copy_raw.split_once(", except ") {
        Some((before, exc)) => (before.trim(), Some(exc.trim().to_string())),
        None => (copy_raw, None),
    };
    let (copy_nodur, copy_dur_opt) = if let Some(c) = copy_core.strip_suffix(" until end of turn") {
        (c.trim(), Some(DurationAst::UntilEndOfTurn))
    } else if let Some(c) = copy_core.strip_suffix(" until your next turn") {
        (c.trim(), Some(DurationAst::UntilYourNextTurn))
    } else if let Some(c) = copy_core.strip_suffix(" until end of combat") {
        (c.trim(), Some(DurationAst::UntilEndOfCombat))
    } else if let Some(c) = copy_core.strip_suffix(" until ~ leaves the battlefield") {
        (c.trim(), Some(DurationAst::UntilSourceLeaves))
    } else {
        (copy_core, None)
    };
    let copy_dur = copy_dur_opt.unwrap_or_else(|| dur.clone());
    if let Some((subj_raw, src_raw)) = copy_nodur
        .split_once(" becomes a copy of ")
        .or_else(|| copy_nodur.split_once(" become a copy of "))
        .or_else(|| copy_nodur.split_once(" become copies of "))
        && let (Ok(subj), Ok(src)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(src_raw, vocab),
        )
    {
        return Some(InstructionAst::BecomeCopy {
            subject: Box::new(subj),
            source: Box::new(src),
            duration: copy_dur,
            exception: copy_exception,
        });
    }

    // Lure effect: "All <creatures> able to block <subj> [this turn] do so" (You Look Upon the Tarrasque, etc.)
    if let Some((who_part, rest)) = snlow.split_once(" able to block ")
        && let Some(subj_raw) = rest
            .strip_suffix(" this turn do so")
            .or_else(|| rest.strip_suffix(" this combat do so"))
            .or_else(|| rest.strip_suffix(" do so"))
        && parse_subject_or_target(who_part, vocab).is_ok()
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Some(InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "AllAbleBlock".to_string(),
            duration: dur.clone(),
        });
    }
    // "<subj> becomes a/an <type> in addition to its other types" (Clavileño, Coward // Killer)
    if let Some(before_in_add) = snlow
        .strip_suffix(" in addition to its other types")
        .or_else(|| snlow.strip_suffix(" in addition to their other types"))
        && let Some((subj_raw, ty_raw)) = before_in_add
            .split_once(" becomes a ")
            .or_else(|| before_in_add.split_once(" becomes an "))
            .or_else(|| before_in_add.split_once(" become "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && (ty_raw == "~"
            || vocab.lookup_subtype(ty_raw).is_some()
            || parse_object_filter(ty_raw, vocab).is_ok())
    {
        return Some(InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::SetOrAddType {
                subject: Box::new(subj),
                type_or_subtype: ty_raw.to_string(),
                additive: true,
                condition: None,
            }),
            duration: dur.clone(),
        });
    }

    // Strip optional trailing "in addition to its/their other [colors and] types" for animation
    let anim_clean = snlow
        .strip_suffix(" in addition to its other colors and types")
        .or_else(|| snlow.strip_suffix(" in addition to their other colors and types"))
        .or_else(|| snlow.strip_suffix(" in addition to its other types"))
        .or_else(|| snlow.strip_suffix(" in addition to their other types"))
        .unwrap_or(&snlow);

    // "<subj> becomes / become / each become a copy of <src>[, except <exc>]" (Polymorphous Rush, Shuri, Taskmaster)
    if let Some((subj_raw, rest)) = anim_clean
        .split_once(" each become a copy of ")
        .or_else(|| anim_clean.split_once(" becomes a copy of "))
        .or_else(|| anim_clean.split_once(" become a copy of "))
        .or_else(|| anim_clean.split_once(" become copies of "))
    {
        let (src_raw, exception) = match rest.split_once(", except ") {
            Some((sr, exc)) => (sr.trim(), Some(exc.trim().to_string())),
            None => (rest.trim(), None),
        };
        if let (Ok(subj), Ok(src)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(src_raw, vocab),
        ) {
            return Some(InstructionAst::BecomeCopy {
                subject: Box::new(subj),
                source: Box::new(src),
                duration: dur.clone(),
                exception,
            });
        }
    }

    // "<subj> becomes a <desc> with base power and toughness P/T"
    if let Some((subj_raw, rest)) = anim_clean
        .split_once(" each become a ")
        .or_else(|| anim_clean.split_once(" each become an "))
        .or_else(|| anim_clean.split_once(" each become "))
        .or_else(|| anim_clean.split_once(" becomes a "))
        .or_else(|| anim_clean.split_once(" becomes an "))
        .or_else(|| anim_clean.split_once(" become "))
        && let Some((desc, pt_tok)) = rest.split_once(" with base power and toughness ")
        && let (Ok(subj), Some((p, t))) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_base_pt(pt_tok.trim()),
        )
    {
        return Some(InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(subj),
            power: p,
            toughness: t,
            description: Some(desc.trim().to_string()),
            keywords: Vec::new(),
            duration: dur.clone(),
        });
    }

    // "<subj> becomes a P/T <desc> [with <kws>]"
    if let Some((subj_raw, rest)) = anim_clean
        .split_once(" each become a ")
        .or_else(|| anim_clean.split_once(" each become an "))
        .or_else(|| anim_clean.split_once(" each become "))
        .or_else(|| anim_clean.split_once(" becomes a "))
        .or_else(|| anim_clean.split_once(" becomes an "))
        .or_else(|| anim_clean.split_once(" become "))
        && rest.contains("creature")
        && let Some((pt_tok, desc_rest)) = rest.split_once(' ')
        && let (Ok(subj), Some((p, t))) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_base_pt(pt_tok),
        )
    {
        let (desc, kws) = match desc_rest.split_once(" with ") {
            Some((d, kw)) => (d, parse_keyword_list(kw, vocab).unwrap_or_default()),
            None => (desc_rest, Vec::new()),
        };
        return Some(InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(subj),
            power: p,
            toughness: t,
            description: Some(desc.trim().to_string()),
            keywords: kws,
            duration: dur.clone(),
        });
    }

    // "<subj> becomes a/an <Subtype(s)> [with <kws>]" (Kargan Intimidator, Warden of the First Tree, Rageform)
    if let Some((subj_raw, rest)) = anim_clean
        .split_once(" becomes a ")
        .or_else(|| anim_clean.split_once(" becomes an "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        if rest == "aura with enchant creature" || rest == "aura with enchant permanent" {
            return Some(InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(subj),
                power: "keep".to_string(),
                toughness: "keep".to_string(),
                description: Some(rest.to_string()),
                keywords: Vec::new(),
                duration: dur.clone(),
            });
        }
        let (type_part, kws) = match rest.split_once(" with ") {
            Some((tp, kw_raw)) => (tp.trim(), parse_keyword_list(kw_raw, vocab)),
            None => (rest.trim(), Some(Vec::new())),
        };
        if let Some(kw_list) = kws
            && !type_part.is_empty()
            && parse_object_filter(type_part, vocab).is_ok()
        {
            return Some(InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(subj),
                power: "keep".to_string(),
                toughness: "keep".to_string(),
                description: Some(type_part.to_string()),
                keywords: kw_list,
                duration: dur.clone(),
            });
        }
    }

    if matches!(
        low,
        "it's still a land"
            | "it's still an artifact"
            | "it's still an enchantment"
            | "he's still a planeswalker"
            | "she's still a planeswalker"
            | "it's still a planeswalker"
            | "they're still lands"
            | "it's an enchantment"
            | "it's an artifact"
            | "it's a creature"
            | "it's a land"
            | "it's a treasure"
    ) {
        return Some(InstructionAst::KeepExistingTypes(low.to_string()));
    }

    None
}

// ---------------------------------------------------------------------------
// 4. STATIC & REPLACEMENT PARSER
// ---------------------------------------------------------------------------

/// Either a compiled static ability or a compiled replacement effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticOrReplacement {
    Static(StaticAbilityAst),
    Replacement(ReplacementEffectAst),
}

/// Strictly parse a permanent static/replacement paragraph or spell static/cost modifier.
pub fn parse_static_or_replacement(
    para: &str,
    vocab: &Vocabulary,
) -> Result<StaticOrReplacement, String> {
    let Some(_depth_guard) = DepthGuard::enter() else {
        return Err("max_parse_depth".to_string());
    };
    let s = para.trim().trim_end_matches('.').trim();
    let low = s.to_ascii_lowercase();

    // 0. Global trailing ", where X is <ValueExpr>" in static/keyword-X clauses (quote-aware)
    if let Some(idx) = find_top_level_pattern(s, ", where x is ")
        && let Ok(_x_val) = parse_value_expr(&s[idx + ", where x is ".len()..], vocab)
    {
        let head = s[..idx].trim();
        if let Ok(sr) = parse_static_or_replacement(head, vocab) {
            return Ok(sr);
        }
        let hlow = head.to_ascii_lowercase();
        if let Some(kw_name) = hlow.strip_suffix(" x")
            && crate::keywords::parse_keyword_token(head, vocab).is_some()
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::GrantKeywordWithCost {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    keyword: kw_name.trim().to_string(),
                    cost_rule: s[idx + 2..].trim().to_string(),
                    condition: None,
                },
            ));
        }
    }

    // 0b. "You can't cast ~ if/unless <cond>" (Rock Jockey, etc.)
    if let Some(after_cc) = low
        .strip_prefix("you can't cast ~ ")
        .or_else(|| low.strip_prefix("you can't cast this spell "))
        && let Some(cond_raw) = after_cc
            .strip_prefix("if ")
            .or_else(|| after_cc.strip_prefix("unless "))
            .or_else(|| after_cc.strip_prefix("as long as "))
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::SelfObject)),
                rule: "cant_cast_self".to_string(),
                filter: None,
                condition: Some(Box::new(cond)),
            },
        ));
    }

    // 0c. "You may cast ~ from your graveyard if/as long as <cond>" (Undead Sprinter, Eelectrocute, Risen Executioner)
    if let Some(after_gy) = low
        .strip_prefix("you may cast ~ from your graveyard ")
        .or_else(|| low.strip_prefix("you may cast this card from your graveyard "))
        && let Some(cond_raw) = after_gy
            .strip_prefix("if ")
            .or_else(|| after_gy.strip_prefix("as long as "))
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::PlayOrCastPermission {
                cast_only: true,
                filter: Box::new(crate::ast::ObjectFilterAst::default()),
                permission: "cast_self_from_graveyard".to_string(),
                condition: Some(Box::new(cond)),
            },
        ));
    }

    // 1. Enchant <filter/player>
    if let Some(rest) = low.strip_prefix("enchant ") {
        if rest == "player" || rest == "opponent" {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::AttachmentRestriction {
                    filter: None,
                    target_player: Some(rest.to_string()),
                },
            ));
        }
        let flt = parse_object_filter(rest, vocab)?;
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::AttachmentRestriction {
                filter: Some(Box::new(flt)),
                target_player: None,
            },
        ));
    }

    // 2. Replacement: Enters tapped / Enters prepared / Enters as a copy
    if low == "~ enters prepared" || low == "~ enters the battlefield prepared" {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersPrepared,
        ));
    }
    if let Some(rest) = low
        .strip_prefix("you may have ~ enter as a copy of ")
        .or_else(|| low.strip_prefix("you may have ~ enter the battlefield as a copy of "))
        .or_else(|| low.strip_prefix("you may have ~ enter tapped as a copy of "))
        .or_else(|| low.strip_prefix("you may have ~ enter the battlefield tapped as a copy of "))
        .or_else(|| low.strip_prefix("as ~ enters, you may have it become a copy of "))
        .or_else(|| low.strip_prefix("as ~ enters, you may have ~ become a copy of "))
    {
        let (flt_raw, exception) = match rest.split_once(", except ") {
            Some((f, e)) => (f, Some(e.trim().to_string())),
            None => (rest, None),
        };
        let flt_nodur = flt_raw
            .strip_suffix(" until end of turn")
            .unwrap_or(flt_raw)
            .trim();
        let flt_clean = flt_nodur.strip_prefix("any ").unwrap_or(flt_nodur);
        if let Ok(flt) = parse_object_filter(flt_clean, vocab) {
            return Ok(StaticOrReplacement::Replacement(
                ReplacementEffectAst::EntersAsCopy {
                    optional: true,
                    filter: Box::new(flt),
                    exception,
                },
            ));
        }
    }
    if let Some(rest) = low
        .strip_prefix("~ enters as a copy of ")
        .or_else(|| low.strip_prefix("~ enters the battlefield as a copy of "))
        .or_else(|| low.strip_prefix("~ enters tapped as a copy of "))
    {
        let (flt_raw, exception) = match rest.split_once(", except ") {
            Some((f, e)) => (f, Some(e.trim().to_string())),
            None => (rest, None),
        };
        let flt_clean = flt_raw.strip_prefix("any ").unwrap_or(flt_raw);
        if let Ok(flt) = parse_object_filter(flt_clean, vocab) {
            return Ok(StaticOrReplacement::Replacement(
                ReplacementEffectAst::EntersAsCopy {
                    optional: false,
                    filter: Box::new(flt),
                    exception,
                },
            ));
        }
    }
    if low == "~ enters tapped" || low == "~ enters the battlefield tapped" {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersTapped { unless: None },
        ));
    }
    if low == "~ enters tapped and doesn't untap during your untap step"
        || low == "~ enters the battlefield tapped and doesn't untap during your untap step"
    {
        return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::SelfObject)),
                rule: "EntersTappedAndDoesntUntap".to_string(),
                filter: None,
                condition: None,
            }],
        )));
    }
    // "~ enters tapped and with <N> <kind> counters on it" (Noble's Purse, Sphere of the Suns)
    if let Some(after) = low
        .strip_prefix("~ enters tapped and with ")
        .or_else(|| low.strip_prefix("~ enters the battlefield tapped and with "))
        && let Some((cnt_kind, _)) = after
            .split_once(" counters on it")
            .or_else(|| after.split_once(" counter on it"))
    {
        let (cnt, kind) = peel_leading_count(cnt_kind, vocab);
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersWithCounters {
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
                per: None,
                x_expr: None,
                condition: None,
            },
        ));
    }
    // "As ~ enters, look at an opponent's hand, then choose <choice>" (Anointed Peacekeeper, Sorcerous Spyglass, Arachne)
    if let Some(choice) = low.strip_prefix("as ~ enters, look at an opponent's hand, then choose ")
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::AsEntersChoose {
                choice: choice.trim().to_string(),
            },
        ));
    }
    // "As ~ enters, you may behold <flt>. If you don't, ~ enters tapped" (Theorist's Sanctum)
    if let Some(after) = low.strip_prefix("as ~ enters, you may behold ")
        && let Some((flt_raw, _)) = after.split_once(". if you don't, ~ enters tapped")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersTappedUnlessReveal {
                filter: Box::new(flt),
            },
        ));
    }
    // "A deck can have up to <N> cards named ~" (Nazgul, Seven Dwarves)
    if let Some(after) = low.strip_prefix("a deck can have up to ")
        && let Some(cnt_tok) = after.strip_suffix(" cards named ~")
        && parse_number_word(cnt_tok).is_some()
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::DeckConstructionRule(low),
        ));
    }
    if low.starts_with("as ~ enters")
        && (low.ends_with("from your hand. if you don't, ~ enters tapped")
            || low.ends_with("from your hand. if you don't, it enters tapped")
            || (low.contains(". ~ enters tapped unless you revealed ")
                && (low.ends_with(" this way") || low.contains(" this way or you control "))))
        && let Some((_, after_rev)) = low.split_once("you may reveal ")
        && let Some((flt_raw, _)) = after_rev.split_once(" from your hand")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersTappedUnlessReveal {
                filter: Box::new(flt),
            },
        ));
    }
    if low == "if it's neither day nor night, it becomes day as ~ enters" {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "BecomesDayOnEnterIfNeither".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if low.starts_with("as ~ enters")
        && (low.ends_with("life. if you don't, it enters tapped")
            || low.ends_with("life. if you don't, ~ enters tapped")
            || low.ends_with("life. if you don't, it enters the battlefield tapped")
            || low.ends_with("life. if you don't, ~ enters the battlefield tapped"))
        && let Some((_, after_pay)) = low.split_once("you may pay ")
        && let Some((life_str, _)) = after_pay.split_once(" life")
        && let Ok(life) = life_str.parse::<u16>()
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersTappedUnlessPayLife { life },
        ));
    }
    if let Some(cond_raw) = low
        .strip_prefix("~ enters tapped unless ")
        .or_else(|| low.strip_prefix("~ enters the battlefield tapped unless "))
    {
        let cond = parse_condition(cond_raw, vocab)?;
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersTapped {
                unless: Some(Box::new(cond)),
            },
        ));
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some(cond_raw) = after_if
            .strip_suffix(", ~ enters tapped")
            .or_else(|| after_if.strip_suffix(", ~ enters the battlefield tapped"))
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersTapped {
                unless: Some(Box::new(ConditionAst::Not(Box::new(cond)))),
            },
        ));
    }

    // 3. Replacement: Enters tapped with counters / Escapes with counters / Enters with counters
    if let Some(after) = low
        .strip_prefix("~ enters tapped with ")
        .or_else(|| low.strip_prefix("~ enters the battlefield tapped with "))
        && let Some(left) = after
            .strip_suffix(" counters on it")
            .or_else(|| after.strip_suffix(" counter on it"))
    {
        let (cnt, kind) = peel_leading_count(left, vocab);
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersTappedWithCounters {
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
            },
        ));
    }
    if let Some(after) = low
        .strip_prefix("~ escapes with ")
        .or_else(|| low.strip_prefix("it escapes with "))
        && let Some(left) = after
            .strip_suffix(" counters on it instead")
            .or_else(|| after.strip_suffix(" counter on it instead"))
            .or_else(|| after.strip_suffix(" counters on it"))
            .or_else(|| after.strip_suffix(" counter on it"))
    {
        let (cnt, kind) = peel_leading_count(left, vocab);
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EscapesWithCounters {
                counter_kind: kind.to_string(),
                count: Box::new(cnt),
            },
        ));
    }
    // CR 702.33c Multi-slot / "and/or" Kicker enters-with replacement:
    // "If ~ was kicked with its {B} kicker, it enters with a +1/+1 counter on it and with \"Pay 3 life: Regenerate ~.\""
    let multi_kicker_pair = low
        .strip_prefix("if ~ was kicked with its ")
        .and_then(|r| r.split_once(" kicker, it enters with "));
    if let Some((slot_raw, after_mk)) = multi_kicker_pair
        && let Some((left, rest)) = after_mk
            .split_once(" counters on it")
            .or_else(|| after_mk.split_once(" counter on it"))
    {
        let (cnt, kind) = peel_leading_count(left, vocab);
        let cond = Some(Box::new(ConditionAst::WasKickedSlot(
            slot_raw.trim().to_string(),
        )));
        let valid_tail = rest.is_empty()
            || rest.strip_prefix(" and with ").is_some_and(|kw_or_ab| {
                parse_keyword_list(kw_or_ab, vocab).is_some()
                    || (kw_or_ab.starts_with('"') && kw_or_ab.ends_with('"'))
            });
        if valid_tail {
            return Ok(StaticOrReplacement::Replacement(
                ReplacementEffectAst::EntersWithCounters {
                    counter_kind: kind.to_string(),
                    count: Box::new(cnt),
                    per: None,
                    x_expr: None,
                    condition: cond,
                },
            ));
        }
    }
    if let Some(after) = low
        .strip_prefix("~ enters with ")
        .or_else(|| low.strip_prefix("~ enters the battlefield with "))
        .or_else(|| low.strip_prefix("it enters with "))
        .or_else(|| low.strip_prefix("it enters the battlefield with "))
        .or_else(|| low.strip_prefix("if ~ was kicked, it enters with "))
        .or_else(|| low.strip_prefix("if ~ was kicked, it enters the battlefield with "))
    {
        let kicked_cond = if low.starts_with("if ~ was kicked") {
            Some(Box::new(ConditionAst::WasKicked))
        } else {
            None
        };
        if let Some((left, rest)) = after
            .split_once(" counters on it")
            .or_else(|| after.split_once(" counter on it"))
        {
            let (cnt, kind) = peel_leading_count(left, vocab);
            let mut per = None;
            let mut x_expr = None;
            let mut cond = kicked_cond;
            if let Some(wx) = rest.strip_prefix(", where x is ") {
                x_expr = Some(Box::new(parse_value_expr(wx, vocab)?));
            } else if let Some(eq_raw) = rest.strip_prefix(" equal to ") {
                if eq_raw == "the amount of mana spent to cast it"
                    || eq_raw == "the amount of mana spent to cast ~"
                {
                    x_expr = Some(Box::new(ValueExprAst::PriorResult));
                } else {
                    x_expr = Some(Box::new(parse_value_expr(eq_raw, vocab)?));
                }
            } else if matches!(
                rest,
                " for each color of mana spent to cast it"
                    | " for each color of mana spent to cast ~"
            ) {
                x_expr = Some(Box::new(ValueExprAst::ColorsOfManaSpent));
            } else if matches!(
                rest,
                " for each time it was kicked" | " for each time ~ was kicked"
            ) {
                x_expr = Some(Box::new(ValueExprAst::TimesKicked));
            } else if let Some(fe) = rest.strip_prefix(" for each ") {
                if let Ok(flt) = parse_object_filter(fe, vocab) {
                    per = Some(Box::new(flt));
                } else {
                    x_expr = Some(Box::new(parse_for_each_value_expr(fe, vocab)?));
                }
            } else if let Some(kw_raw) = rest.strip_prefix(" and with ")
                && (parse_keyword_list(kw_raw, vocab).is_some()
                    || (kw_raw.starts_with('"') && kw_raw.ends_with('"')))
            {
                // e.g. "If ~ was kicked, it enters with two +1/+1 counters on it and with flying"
            } else if let Some(c_raw) = rest.strip_prefix(" if ") {
                cond = Some(Box::new(parse_condition(c_raw, vocab)?));
            } else if let Some(c_raw) = rest.strip_prefix(" unless ") {
                cond = Some(Box::new(ConditionAst::Not(Box::new(parse_condition(
                    c_raw, vocab,
                )?))));
            } else if rest.starts_with(" from among ") {
                // Grimdancer / Crystalline Giant: "~ enters with your choice of two different counters on it from among ..."
            } else if !rest.is_empty() {
                return Err(format!("etb_cnt_tail:{rest}"));
            }
            return Ok(StaticOrReplacement::Replacement(
                ReplacementEffectAst::EntersWithCounters {
                    counter_kind: kind.to_string(),
                    count: Box::new(cnt),
                    per,
                    x_expr,
                    condition: cond,
                },
            ));
        }
    }

    // 3b. Others enter with [additional] counters ("Each other creature you control enters with an additional +1/+1 counter on it", Bloodspore Thrinax, Grumgully, Good-Fortune Unicorn)
    if let Some((subj_raw, rest)) = low
        .split_once(" enters with ")
        .or_else(|| low.split_once(" enter with "))
        .or_else(|| low.split_once(" enters the battlefield with "))
        .or_else(|| low.split_once(" enter the battlefield with "))
        && subj_raw != "~"
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let (rest_noadd, additional) = if let Some(r) = rest.strip_prefix("an additional ") {
            (r, true)
        } else {
            (rest, false)
        };
        if let Some((left, tail)) = rest_noadd
            .split_once(" counters on it")
            .or_else(|| rest_noadd.split_once(" counter on it"))
            .or_else(|| rest_noadd.split_once(" counters on them"))
            .or_else(|| rest_noadd.split_once(" counter on them"))
        {
            let tail_ok = tail.is_empty()
                || tail
                    .strip_prefix(", where x is ")
                    .is_some_and(|wx| parse_value_expr(wx, vocab).is_ok());
            if tail_ok {
                let (cnt, kind) = peel_leading_count(left, vocab);
                return Ok(StaticOrReplacement::Replacement(
                    ReplacementEffectAst::OthersEnterWithCounters {
                        subject: Box::new(subj),
                        counter_kind: kind.to_string(),
                        count: Box::new(cnt),
                        additional,
                    },
                ));
            }
        }
    }

    // 4. Replacement: TriggerMultiplier (Panharmonicon/Elesh Norn/Yarok/Isshin/Katara) + Knight of the Holy Nimbus + Ali from Cairo + EventInstead
    if let Some(after) = low.strip_prefix("if a triggered ability of ")
        && let Some((src_part, tail)) = after.split_once(" triggers, ")
        && tail.contains("triggers an additional time")
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::TriggerMultiplier {
                event_desc: src_part.trim().to_string(),
                multiplier: 2,
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("if ")
        && let Some((ev_part, tail)) = after.split_once(" causes a triggered ability of ")
        && (tail.ends_with("triggers an additional time")
            || tail.ends_with("triggers an additional time."))
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::TriggerMultiplier {
                event_desc: ev_part.trim().to_string(),
                multiplier: 2,
                condition: None,
            },
        ));
    }
    if low == "if ~ would be destroyed, regenerate it" {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "~".to_string(),
                would: "be destroyed".to_string(),
                instead: "regenerate it".to_string(),
                instead_body: Some(vec![InstructionAst::Regenerate {
                    subject: Box::new(SubjectRefAst::SelfObject),
                }]),
            },
        ));
    }
    if low.starts_with("damage that would reduce your life total to less than ")
        && low.ends_with(" instead")
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "damage".to_string(),
                would: "reduce_life_below_floor".to_string(),
                instead: s.to_string(),
                instead_body: Some(vec![InstructionAst::SetLifeTotal {
                    player: PlayerRefAst::You,
                    value: Box::new(ValueExprAst::Const(1)),
                }]),
            },
        ));
    }
    if low.starts_with("if a spell or ability an opponent controls causes you to discard ~, put it onto the battlefield")
        || (low.starts_with("if ")
            && (low.contains(" would deal damage to ")
                || low.contains(" would deal combat damage to ")
                || low.contains(" damage would be dealt to "))
            && low.contains(", prevent that damage"))
    {
        let instead_part = s.split_once(", ").map_or(s, |(_, r)| r);
        let instead_body = parse_instead_body(instead_part, vocab);
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "~".to_string(),
                would: "special_replacement".to_string(),
                instead: s.to_string(),
                instead_body,
            },
        ));
    }
    if let Some(after_dbl) = low.strip_prefix("double all damage that ")
        && after_dbl.ends_with(" would deal")
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: after_dbl
                    .strip_suffix(" would deal")
                    .unwrap_or(after_dbl)
                    .trim()
                    .to_string(),
                would: "deal_damage".to_string(),
                instead: "double_damage".to_string(),
                instead_body: Some(vec![InstructionAst::DoubleStatOrCounters {
                    subject: Box::new(SubjectRefAst::Anaphora("damage".to_string())),
                    kind: "damage".to_string(),
                    duration: DurationAst::Instant,
                }]),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("if ")
        && let Some((subj_ev, rest)) = after.split_once(" would ")
        && let Some((would, instead_raw)) = rest.split_once(", ")
        && let Some(instead) = instead_raw
            .strip_suffix(" instead")
            .or_else(|| instead_raw.strip_prefix("instead "))
            .or_else(|| instead_raw.strip_prefix("you may instead "))
            .or_else(|| instead_raw.strip_prefix("instead, "))
            .or_else(|| {
                if instead_raw.contains(" instead ") {
                    Some(instead_raw)
                } else {
                    None
                }
            })
    {
        let instead_body = parse_instead_body(instead, vocab);
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: subj_ev.trim().to_string(),
                would: would.trim().to_string(),
                instead: instead.trim().to_string(),
                instead_body,
            },
        ));
    }

    // CR 614 "each <flt> that would <would> ... instead" (Crafty Cutpurse, etc.)
    if let Some((subj_part, rest)) = low.split_once(" that would ")
        && let Some(instead) = rest.strip_suffix(" instead")
    {
        let instead_body = parse_instead_body(instead, vocab);
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: subj_part.trim().to_string(),
                would: "would_event".to_string(),
                instead: instead.trim().to_string(),
                instead_body,
            },
        ));
    }

    // 5. Replacement: As ~ enters, [secretly] choose <choice> / enters under opponent's control
    if low == "~ enters under the control of an opponent of your choice" {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::AsEntersChoose {
                choice: "opponent_controller".to_string(),
            },
        ));
    }
    if let Some(choice) = low
        .strip_prefix("as ~ enters, choose ")
        .or_else(|| low.strip_prefix("as ~ enters the battlefield, choose "))
        .or_else(|| low.strip_prefix("as ~ enters, secretly choose "))
        .or_else(|| low.strip_prefix("as it enters, choose "))
        .or_else(|| low.strip_prefix("as it enters the battlefield, choose "))
        && !choice.is_empty()
        && !choice.contains('.')
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::AsEntersChoose {
                choice: choice.to_string(),
            },
        ));
    }
    // General "As ~ enters [or is turned face up], <imperative action>"
    // (Phylactery Lich, Wood Elemental, Phyrexian Processor, Crowd-Control Warden, Arachne, etc.)
    for as_pref in [
        "as ~ enters or is turned face up, ",
        "as ~ enters the battlefield, ",
        "as ~ enters, ",
        "as it enters, ",
    ] {
        if low.starts_with(as_pref) {
            let body_str = &s[as_pref.len()..];
            let (ins, fails) = parse_effect_body(body_str, vocab);
            if fails.is_empty() && !ins.is_empty() {
                return Ok(StaticOrReplacement::Replacement(
                    ReplacementEffectAst::AsEntersAction(ins),
                ));
            }
        }
    }

    // Opening hand Leyline / Chancellor reveal / Exert-as-attacks / ". The same is true for ..." multi-sentence statics
    if low == "if ~ is in your opening hand, you may begin the game with it on the battlefield" {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "OpeningHandBeginOnBattlefield".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(after_oh) = low
        .strip_prefix("you may reveal ~ from your opening hand. if you do, ")
        .or_else(|| low.strip_prefix("you may exert ~ as it attacks. when you do, "))
    {
        let body_slice = &s[s.len() - after_oh.len()..];
        let (ins, fails) = parse_effect_body(body_slice, vocab);
        if fails.is_empty() && !ins.is_empty() {
            return Ok(StaticOrReplacement::Static(StaticAbilityAst::FollowUp(ins)));
        }
    }
    if let Some((first_sent, same_tail)) = low.split_once(". the same is true for ")
        && let Ok(StaticOrReplacement::Static(first_st)) =
            parse_static_or_replacement(&s[..first_sent.len()], vocab)
    {
        return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![
                first_st,
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: None,
                    rule: format!("same_is_true_for:{same_tail}"),
                    filter: None,
                    condition: None,
                },
            ],
        )));
    }
    if low.starts_with("during your end step, ") {
        let mut inner = parse_static_or_replacement(&s["during your end step, ".len()..], vocab)?;
        attach_condition(&mut inner, ConditionAst::IsYourTurn);
        return Ok(inner);
    }

    // "Once during each of your turns, <static>" / "Once each turn, <static>"
    for opt_pref in ["once during each of your turns, ", "once each turn, "] {
        if low.starts_with(opt_pref)
            && let Ok(StaticOrReplacement::Static(inner)) =
                parse_static_or_replacement(&s[opt_pref.len()..], vocab)
        {
            return Ok(StaticOrReplacement::Static(StaticAbilityAst::OncePerTurn(
                Box::new(inner),
            )));
        }
    }

    // "During your turn, <static>" / "During each opponent's turn, <static>" / "During turns other than yours, <static>"
    if low.starts_with("during your turn, ") {
        let mut inner = parse_static_or_replacement(&s["during your turn, ".len()..], vocab)?;
        attach_condition(&mut inner, ConditionAst::IsYourTurn);
        return Ok(inner);
    }
    if low.starts_with("during each opponent's turn, ") {
        let mut inner =
            parse_static_or_replacement(&s["during each opponent's turn, ".len()..], vocab)?;
        attach_condition(
            &mut inner,
            ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)),
        );
        return Ok(inner);
    }
    if low.starts_with("during turns other than yours, ") {
        let mut inner =
            parse_static_or_replacement(&s["during turns other than yours, ".len()..], vocab)?;
        attach_condition(
            &mut inner,
            ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)),
        );
        return Ok(inner);
    }
    // "Otherwise, <static>" (Serra's Boon, etc.)
    if low.starts_with("otherwise, ")
        && let Ok(StaticOrReplacement::Static(inner)) =
            parse_static_or_replacement(&s["otherwise, ".len()..], vocab)
    {
        return Ok(StaticOrReplacement::Static(StaticAbilityAst::Otherwise(
            Box::new(inner),
        )));
    }

    // Die-roll table row: "2 U+2014 Add {W}." / "1-9 | ..."
    if let Some((left, right)) = s.split_once(EM_DASH) {
        let lt = left.trim();
        let is_die_band = !lt.is_empty()
            && lt.len() <= 7
            && lt
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'-' || b == b'+');
        if is_die_band && !right.trim().is_empty() {
            return Ok(StaticOrReplacement::Static(StaticAbilityAst::DieRollBand {
                roll: lt.to_string(),
                payload: right.trim().to_string(),
            }));
        }
    }

    // Spacecraft station band: "10+ | Flying" / "N+ | <payload>"
    if let Some((left, right)) = s.split_once(" | ")
        && let Some(num_str) = left.trim().strip_suffix('+')
        && let Ok(min_charge) = num_str.trim().parse::<u8>()
    {
        return Ok(StaticOrReplacement::Static(StaticAbilityAst::StationBand {
            min_charge,
            payload: right.trim().to_string(),
        }));
    }

    // "As long as <cond>, <static>" (scan all ", " split points for comma-containing conditions like Kosei)
    if low.starts_with("as long as ") {
        let after_ala = &s["as long as ".len()..];
        for (idx, _) in after_ala.match_indices(", ") {
            let cond_raw = &after_ala[..idx];
            let rest = &after_ala[idx + 2..];
            if let Ok(cond) = parse_condition(cond_raw, vocab)
                && let Ok(mut inner) = parse_static_or_replacement(rest, vocab)
            {
                attach_condition(&mut inner, cond);
                return Ok(inner);
            }
        }
    }

    // Leading "If <cond>, <static>" (Avatar of Will, Chrome Replicator, etc.)
    if low.starts_with("if ") && !low.contains(" would ") {
        let after_if = &s[3..];
        for (idx, _) in after_if.match_indices(", ") {
            let cond_raw = &after_if[..idx];
            let rest = &after_if[idx + 2..];
            if let Ok(cond) = parse_condition(cond_raw, vocab)
                && let Ok(mut inner) = parse_static_or_replacement(rest, vocab)
            {
                attach_condition(&mut inner, cond);
                return Ok(inner);
            }
        }
    }

    // Trailing "<static> if <cond>" (Shadow the Hedgehog: "Each spell you cast has split second if ...")
    if !low.starts_with("if ")
        && !low.starts_with("cast ~ only if ")
        && let Some(idx) = low.rfind(" if ")
        && let Ok(cond) = parse_condition(&s[idx + 4..], vocab)
        && let Ok(mut inner) = parse_static_or_replacement(s[..idx].trim(), vocab)
    {
        attach_condition(&mut inner, cond);
        return Ok(inner);
    }

    // Free alternative casting cost: "You may cast ~ without paying its mana cost" (Deepwood Legate, Kyren Legate, Land Grant)
    if low == "you may cast ~ without paying its mana cost"
        || low == "you may cast this spell without paying its mana cost"
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::AlternativeCastCost { cost: Vec::new() },
        ));
    }

    // Trailing "<static> as long as <cond>" (Threshold, Metalcraft, Delirium, conditional pump, So Tiny "instead as long as")
    if let Some(idx) = low.rfind(" as long as ")
        && let Ok(cond) = parse_condition(&s[idx + " as long as ".len()..], vocab)
    {
        let left_raw = s[..idx].trim();
        let left_clean = left_raw
            .strip_suffix(" instead")
            .or_else(|| left_raw.strip_suffix(" Instead"))
            .unwrap_or(left_raw)
            .trim();
        if let Ok(mut inner) = parse_static_or_replacement(left_clean, vocab) {
            attach_condition(&mut inner, cond);
            return Ok(inner);
        }
    }

    // Trailing "<static> unless <cond>" (Reckless Cohort, Pipsqueak: "~ can't attack alone unless it has a +1/+1 counter on it")
    // Preserve Pillowfort "unless their/its controller pays ..." for the AttackTax rule below.
    if let Some(idx) = low.rfind(" unless ") {
        let rhs_u = &low[idx + " unless ".len()..];
        if !rhs_u.starts_with("their controller pays ")
            && !rhs_u.starts_with("its controller pays ")
            && let Ok(cond) = parse_condition(&s[idx + " unless ".len()..], vocab)
            && let Ok(mut inner) = parse_static_or_replacement(s[..idx].trim(), vocab)
        {
            attach_condition(&mut inner, ConditionAst::Not(Box::new(cond)));
            return Ok(inner);
        }
    }

    // 6. ContinuousModifyPT + optional keywords / quoted ability / combat rule / for-each
    if let Some((idx, vlen)) = low
        .find(" gets ")
        .map(|i| (i, 6))
        .or_else(|| low.find(" get ").map(|i| (i, 5)))
        && let Ok(subj) = parse_subject_or_target(&s[..idx], vocab)
    {
        let after = s[idx + vlen..].trim();
        let after = after
            .strip_prefix("an additional ")
            .or_else(|| after.strip_prefix("An additional "))
            .unwrap_or(after);
        let (pt_tok_raw, rest) = match after.split_once(' ') {
            Some((p, r)) => (p, r.trim()),
            None => (after, ""),
        };
        let pt_tok = pt_tok_raw.trim_end_matches(',');
        if let Some((dx, dy)) = parse_pt_mod(pt_tok) {
            let mut grant_keywords = Vec::new();
            let mut grant_ability_text = None;
            let mut scale = None;
            let mut paired_rule = None;
            let mut valid = true;

            // Peel optional trailing ", and is a <types> in addition to its other [colors and] types" / " and is a <types>..."
            let (rest, trailing_is_type) = if let Some((before, after_is)) = rest
                .split_once(", and is a ")
                .or_else(|| rest.split_once(", and is an "))
                .or_else(|| rest.split_once(" and is a "))
                .or_else(|| rest.split_once(" and is an "))
            {
                (before.trim(), Some(after_is.trim()))
            } else {
                (rest, None)
            };
            if let Some(ty) = trailing_is_type {
                paired_rule = Some(Box::new(StaticAbilityAst::SetOrAddType {
                    subject: Box::new(subj.clone()),
                    type_or_subtype: ty.to_ascii_lowercase(),
                    additive: ty.to_ascii_lowercase().contains("in addition"),
                    condition: None,
                }));
            }

            // Peel optional leading "has ..." after comma ("+1/+0, has flying" or "+1/+0, has \"...\"")
            let rest_clean = if let Some(after_h) = rest
                .strip_prefix("has ")
                .or_else(|| rest.strip_prefix("have "))
            {
                if let Some((kw_side, ab_side)) = after_h.split_once(" and \"")
                    && let Some(ab_inner) = ab_side.trim_end_matches(',').strip_suffix('"')
                {
                    if let Some(kws) = parse_keyword_list(kw_side, vocab) {
                        grant_keywords = kws;
                        grant_ability_text = Some(ab_inner.to_string());
                        ""
                    } else {
                        rest
                    }
                } else if after_h.starts_with('"') && after_h.trim_end_matches(',').ends_with('"') {
                    grant_ability_text =
                        Some(after_h.trim_end_matches(',').trim_matches('"').to_string());
                    ""
                } else if let Some(kws) = parse_keyword_list(after_h, vocab) {
                    grant_keywords = kws;
                    ""
                } else {
                    rest
                }
            } else {
                rest
            };

            // Split optional trailing "and has/have <kws>" BEFORE "for each ..." so
            // "Equipped creature gets +1/+0 for each Gate you control and has vigilance and menace" works!
            let (base_tail, tail_has) = if let Some((b, h)) = rest_clean
                .split_once(" and has ")
                .or_else(|| rest_clean.split_once(" and have "))
            {
                (b.trim(), Some(h.trim()))
            } else {
                (rest_clean, None)
            };
            let blow_tail = base_tail.to_ascii_lowercase();

            if let Some(after_has) = tail_has {
                if after_has.starts_with('"') && after_has.ends_with('"') {
                    grant_ability_text = Some(after_has.trim_matches('"').to_string());
                } else if let Some((kw_side, ab_side)) = after_has
                    .split_once(", and \"")
                    .or_else(|| after_has.split_once(" and \""))
                    && let Some(ab_inner) = ab_side.strip_suffix('"')
                    && let Some(kws) = parse_keyword_list(kw_side, vocab)
                {
                    grant_keywords = kws;
                    grant_ability_text = Some(ab_inner.to_string());
                } else if let Some(kws) = parse_keyword_list(after_has, vocab) {
                    grant_keywords = kws;
                } else {
                    valid = false;
                }
            }

            if let Some(wx) = blow_tail.strip_prefix("where x is ") {
                if let Ok(val) = parse_value_expr(wx, vocab) {
                    scale = Some(Box::new(val));
                } else {
                    valid = false;
                }
            } else if let Some(fe) = blow_tail.strip_prefix("for each ") {
                if let Ok(fe_val) = parse_for_each_value_expr(fe, vocab) {
                    scale = Some(Box::new(fe_val));
                } else {
                    valid = false;
                }
            } else if let Some(after_has) = base_tail
                .strip_prefix("and has ")
                .or_else(|| base_tail.strip_prefix("and have "))
            {
                if after_has.starts_with('"') && after_has.ends_with('"') {
                    grant_ability_text = Some(after_has.trim_matches('"').to_string());
                } else if let Some((kw_side, ab_side)) = after_has
                    .split_once(", and \"")
                    .or_else(|| after_has.split_once(" and \""))
                    && let Some(ab_inner) = ab_side.strip_suffix('"')
                    && let Some(kws) = parse_keyword_list(kw_side, vocab)
                {
                    grant_keywords = kws;
                    grant_ability_text = Some(ab_inner.to_string());
                } else if let Some(kws) = parse_keyword_list(after_has, vocab) {
                    grant_keywords = kws;
                } else {
                    valid = false;
                }
            } else if blow_tail == "and loses all abilities"
                || blow_tail == "and lose all abilities"
            {
                paired_rule = Some(Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj.clone())),
                    rule: "LosesAllAbilities".to_string(),
                    filter: None,
                    condition: None,
                }));
            } else if let Some(lost_kw) = blow_tail
                .strip_prefix("and loses ")
                .or_else(|| blow_tail.strip_prefix("and lose "))
                && parse_keyword_list(lost_kw, vocab).is_some()
            {
                paired_rule = Some(Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj.clone())),
                    rule: format!("Loses:{lost_kw}"),
                    filter: None,
                    condition: None,
                }));
            } else if let Some(rule_tail) = base_tail.strip_prefix("and ") {
                if let Ok(StaticOrReplacement::Static(r_ast)) =
                    parse_static_or_replacement(&format!("~ {rule_tail}"), vocab)
                {
                    paired_rule = Some(Box::new(r_ast));
                } else {
                    valid = false;
                }
            } else if !blow_tail.is_empty() {
                valid = false;
            }

            if valid {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::ContinuousModifyPT {
                        subject: Box::new(subj),
                        dx,
                        dy,
                        grant_keywords,
                        grant_ability_text,
                        scale,
                        condition: None,
                        paired_rule,
                    },
                ));
            }
        }
    }

    // 7. GrantAbilityText / GrantKeywords (+ optional "and \"<ab>\"" / "and loses <kw>" / "and <rule>")
    if s.contains('"')
        && let Some((left, right)) = s.split_once(" has ").or_else(|| s.split_once(" have "))
        && let Some((kw_side, ab_side)) = right.split_once(" and \"")
        && let Some(ab_text) = ab_side.strip_suffix('"')
        && let (Ok(subj), Some(kws)) = (
            parse_subject_or_target(left, vocab),
            parse_keyword_list(kw_side, vocab),
        )
    {
        let compiled = compile_granted_ability(ab_text, vocab);
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::GrantKeywords {
                subject: Box::new(subj.clone()),
                keywords: kws,
                condition: None,
                paired_rule: Some(Box::new(StaticAbilityAst::GrantAbilityText {
                    subject: Box::new(subj),
                    ability_text: ab_text.to_string(),
                    compiled,
                    condition: None,
                })),
            },
        ));
    }
    if s.contains('"')
        && let Some((left, right)) = s.split_once(" has \"").or_else(|| s.split_once(" have \""))
        && let Some(ab_text) = right.strip_suffix('"')
        && let Ok(subj) = parse_subject_or_target(left, vocab)
    {
        let compiled = compile_granted_ability(ab_text, vocab);
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::GrantAbilityText {
                subject: Box::new(subj),
                ability_text: ab_text.to_string(),
                compiled,
                condition: None,
            },
        ));
    }
    if !s.contains('"')
        && let Some((idx, vlen)) = low
            .find(" also has ")
            .map(|i| (i, 10))
            .or_else(|| low.find(" also have ").map(|i| (i, 11)))
            .or_else(|| low.find(" has ").map(|i| (i, 5)))
            .or_else(|| low.find(" have ").map(|i| (i, 6)))
        && let Ok(subj) = parse_subject_or_target(&s[..idx], vocab)
    {
        let rhs = &s[idx + vlen..];
        let rlow = rhs.to_ascii_lowercase();
        if let Some((kw_side, lose_side)) = rlow
            .split_once(" and loses ")
            .or_else(|| rlow.split_once(" and lose "))
            && let (Some(gkws), Some(lkws)) = (
                parse_keyword_list(kw_side, vocab),
                parse_keyword_list(lose_side, vocab),
            )
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::ContinuousLayerEffect {
                    subject: Box::new(subj),
                    set_types: None,
                    additive_types: false,
                    base_pt: None,
                    set_colors: None,
                    grant_keywords: gkws,
                    lose_keywords: lkws,
                    lose_all_abilities: false,
                    condition: None,
                },
            ));
        }
        if let Some(kws) = parse_keyword_list(rhs, vocab) {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::GrantKeywords {
                    subject: Box::new(subj),
                    keywords: kws,
                    condition: None,
                    paired_rule: None,
                },
            ));
        }
        if let Some((kw_part, rule_part)) = rhs.split_once(" and ")
            && rule_part.to_ascii_lowercase().starts_with("can't ")
            && let Some(kws) = parse_keyword_list(kw_part, vocab)
            && let Ok(StaticOrReplacement::Static(r_ast)) =
                parse_static_or_replacement(&format!("~ {rule_part}"), vocab)
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::GrantKeywords {
                    subject: Box::new(subj),
                    keywords: kws,
                    condition: None,
                    paired_rule: Some(Box::new(r_ast)),
                },
            ));
        }
    }

    // 7b. Continuous ability inheritance: "<subj> has/have all [activated|loyalty] abilities of [all] <filter>"
    // (Necrotic Ooze, Trazyn, Mirran Safehouse, Steward of the Harvest, Marvin, Kasmina)
    if let Some((subj_raw, src_raw)) = low
        .split_once(" has the loyalty abilities of ")
        .or_else(|| low.split_once(" have the loyalty abilities of "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && parse_subject_or_target(src_raw, vocab).is_ok()
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::InheritAbilities {
                subject: Box::new(subj),
                source_filter: Box::new(crate::ast::ObjectFilterAst {
                    any_permanent: true,
                    ..Default::default()
                }),
                activated_only: true,
                condition: None,
            },
        ));
    }
    for (needle, activated_only) in [
        (" has all activated and triggered abilities of ", false),
        (" have all activated and triggered abilities of ", false),
        (" has all activated abilities of ", true),
        (" have all activated abilities of ", true),
        (" has all loyalty abilities of ", true),
        (" have all loyalty abilities of ", true),
        (" has all abilities of ", false),
        (" have all abilities of ", false),
    ] {
        if let Some((subj_raw, src_raw)) = low.split_once(needle) {
            let src_clean = src_raw.strip_prefix("all ").unwrap_or(src_raw);
            if let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
                && (parse_object_filter(src_clean, vocab).is_ok()
                    || src_clean == "the last chosen card"
                    || src_clean.starts_with("cards exiled with "))
            {
                let source_filter =
                    parse_object_filter(src_clean, vocab).unwrap_or(crate::ast::ObjectFilterAst {
                        any_card: true,
                        ..Default::default()
                    });
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::InheritAbilities {
                        subject: Box::new(subj),
                        source_filter: Box::new(source_filter),
                        activated_only,
                        condition: None,
                    },
                ));
            }
        }
    }

    // Pass N iconic Aura/Equipment transformation statics:
    // "<subj> is a/an <desc> with \"<ab>\" [and [it] loses all other [card types and] abilities]"
    // (Imprisoned in the Moon, Minimus Containment, Sugar Coat, In Too Deep)
    if s.contains('"')
        && let Some((subj_raw, rhs)) = low
            .split_once(" is a ")
            .or_else(|| low.split_once(" is an "))
        && let Some((ty_part, after_w)) = rhs.split_once(" with \"")
        && let Some((ab_raw, tail)) = after_w.split_once('"')
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let t_clean = tail.trim().trim_start_matches(',').trim();
        let lose_all =
            t_clean.contains("loses all other") || t_clean.contains("loses all abilities");
        if t_clean.is_empty() || lose_all {
            let orig_ab = s
                .split_once('"')
                .and_then(|(_, r)| r.split_once('"'))
                .map_or(ab_raw, |(inner, _)| inner);
            let compiled = compile_granted_ability(orig_ab, vocab);
            return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                vec![
                    StaticAbilityAst::ContinuousLayerEffect {
                        subject: Box::new(subj.clone()),
                        set_types: Some(ty_part.trim().to_string()),
                        additive_types: false,
                        base_pt: None,
                        set_colors: None,
                        grant_keywords: Vec::new(),
                        lose_keywords: Vec::new(),
                        lose_all_abilities: lose_all,
                        condition: None,
                    },
                    StaticAbilityAst::GrantAbilityText {
                        subject: Box::new(subj),
                        ability_text: orig_ab.to_string(),
                        compiled,
                        condition: None,
                    },
                ],
            )));
        }
    }
    // "<subj> is snow" / "<subj> is legendary" / "<subj> are snow/legendary/no longer snow/the chosen color" / "<subj> is a copy of <src>"
    if let Some((subj_raw, rhs)) = low.split_once(" is ").or_else(|| low.split_once(" are "))
        && matches!(
            rhs,
            "snow" | "legendary" | "no longer snow" | "the chosen color"
        )
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some(rhs.to_string()),
                additive_types: rhs != "no longer snow" && rhs != "the chosen color",
                base_pt: None,
                set_colors: if rhs == "the chosen color" {
                    Some(rhs.to_string())
                } else {
                    None
                },
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }
    // CR 613.4b "<subj> has/have base power and toughness each equal to <val>" (Opalescence, Starfield of Nyx, March of the Machines)
    if let Some((subj_raw, val_raw)) = low
        .split_once(" has base power and toughness each equal to ")
        .or_else(|| low.split_once(" have base power and toughness each equal to "))
        && let (Ok(subj), Ok(val)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_value_expr(val_raw, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::DynamicBasePT {
                subject: Box::new(subj),
                power: Box::new(val.clone()),
                toughness: Box::new(val),
                condition: None,
            },
        ));
    }
    // "<subj> have/has base power/toughness <N>" (Maha, Its Feathers Night)
    if let Some((subj_raw, rhs)) = low
        .split_once(" have base toughness ")
        .or_else(|| low.split_once(" has base toughness "))
        .or_else(|| low.split_once(" have base power "))
        .or_else(|| low.split_once(" has base power "))
        && let (Ok(subj), Ok(_val)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_value_expr(rhs, vocab),
        )
    {
        let is_power = low.contains(" base power ");
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: None,
                additive_types: false,
                base_pt: Some(if is_power {
                    (rhs.trim().to_string(), "keep".to_string())
                } else {
                    ("keep".to_string(), rhs.trim().to_string())
                }),
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }
    // "<subj> lose(s) <kw> and can't have or gain <kw>" (Archetype cycle)
    if let Some((subj_raw, rest)) = low
        .split_once(" lose ")
        .or_else(|| low.split_once(" loses "))
        && let Some((kw1, _kw2)) = rest.split_once(" and can't have or gain ")
        && let (Ok(subj), Some(lkws)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_keyword_list(kw1, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: None,
                additive_types: false,
                base_pt: None,
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: lkws,
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, src_raw)) = low.split_once(" is a copy of ")
        && let (Ok(subj), Ok(_src)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(src_raw, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some(format!("copy_of:{src_raw}")),
                additive_types: false,
                base_pt: None,
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }

    // 7c. Unified ContinuousLayerEffect (Layers 4/5/6/7b):
    // "<subj> is a/an P/T <desc> creature [with <kws>] [in addition to its other types]"
    // (Awaken the Ancient, Vastwood Zendikon, Nissa's Zendikon, Grist, Grand Master of Flowers)
    if let Some((subj_raw, rhs)) = low
        .split_once(" is a ")
        .or_else(|| low.split_once(" is an "))
        .or_else(|| low.split_once("'s a "))
        .or_else(|| low.split_once("'s an "))
        .or_else(|| low.split_once(" are "))
        && let Some((pt_tok, desc_rest)) = rhs.split_once(' ')
        && let Some((p, t)) = parse_base_pt(pt_tok)
        && desc_rest.contains("creature")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let additive_types = desc_rest.contains("in addition to its other")
            || desc_rest.contains("in addition to their other");
        let desc_clean = desc_rest
            .split_once(" in addition to ")
            .map_or(desc_rest, |(a, _)| a)
            .trim();
        let (ty_part, grant_keywords) = match desc_clean.split_once(" with ") {
            Some((d, kw_raw)) => (
                d.trim(),
                parse_keyword_list(kw_raw, vocab).unwrap_or_default(),
            ),
            None => (desc_clean, Vec::new()),
        };
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some(ty_part.to_string()),
                additive_types,
                base_pt: Some((p, t)),
                set_colors: None,
                grant_keywords,
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }
    // "<subj> is a/an <desc> and loses all other card types" (One with the Stars)
    if let Some((subj_raw, rhs)) = low
        .split_once(" is a ")
        .or_else(|| low.split_once(" is an "))
        .or_else(|| low.split_once(" are "))
        && let Some(ty_part) = rhs
            .strip_suffix(" and loses all other card types")
            .or_else(|| rhs.strip_suffix(" and lose all other card types"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some(ty_part.trim().to_string()),
                additive_types: false,
                base_pt: None,
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }
    // "<subj> is a/an <desc> with base power and toughness P/T [and has <kws>] [and loses all other abilities] [in addition to its other types]"
    // "<subj> are/is colorless / white / blue / black / red / green"
    if let Some((subj_raw, rhs)) = low
        .split_once(" is a ")
        .or_else(|| low.split_once(" is an "))
        .or_else(|| low.split_once(" are "))
        && let Some((ty_part, rest)) = rhs.split_once(" with base power and toughness ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let additive_types = rest.contains("in addition to its other")
            || rest.contains("in addition to their other");
        let lose_all = rest.contains("loses all other abilities")
            || rest.contains("lose all other abilities")
            || rest.contains("loses all abilities");
        let pt_tok = rest.split_whitespace().next().unwrap_or("");
        if let Some((p, t)) = parse_base_pt(pt_tok.trim_end_matches(',')) {
            let mut grant_keywords = Vec::new();
            if let Some((_, after_has)) = rest
                .split_once(" and has ")
                .or_else(|| rest.split_once(" and have "))
            {
                let kw_clean = after_has
                    .split_once(" and loses ")
                    .map_or(after_has, |(k, _)| k);
                if let Some(kws) = parse_keyword_list(kw_clean, vocab) {
                    grant_keywords = kws;
                }
            }
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::ContinuousLayerEffect {
                    subject: Box::new(subj),
                    set_types: Some(ty_part.trim().to_string()),
                    additive_types,
                    base_pt: Some((p, t)),
                    set_colors: None,
                    grant_keywords,
                    lose_keywords: Vec::new(),
                    lose_all_abilities: lose_all,
                    condition: None,
                },
            ));
        }
    }
    if let Some((subj_raw, col_raw)) = low.split_once(" are ").or_else(|| low.split_once(" is "))
        && matches!(
            col_raw,
            "colorless" | "white" | "blue" | "black" | "red" | "green" | "all colors"
        )
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: None,
                additive_types: false,
                base_pt: None,
                set_colors: Some(col_raw.to_string()),
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }

    // "<subj> [loses all abilities and] is/becomes an artifact creature with power and toughness each equal to its mana value" (Animate Artifact, March of the Machines, Titania's Song)
    if let Some((subj_raw, rhs)) = low
        .split_once(" is an artifact creature with power and toughness each equal to ")
        .or_else(|| {
            low.split_once("'s an artifact creature with power and toughness each equal to ")
        })
        .or_else(|| {
            low.split_once(
                " loses all abilities and becomes an artifact creature with power and toughness each equal to ",
            )
        })
        .or_else(|| {
            low.split_once(
                " becomes an artifact creature with power and toughness each equal to ",
            )
        })
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some("artifact creature".to_string()),
                additive_types: false,
                base_pt: Some((rhs.trim().to_string(), rhs.trim().to_string())),
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: low.contains("loses all abilities"),
                condition: None,
            },
        ));
    }

    // "<subj> has/have base power <p> or base toughness <t>" (Vhati il-Dal)
    if let Some((subj_raw, rhs)) = low
        .split_once(" has base power ")
        .or_else(|| low.split_once(" have base power "))
        && let Some((p_raw, t_raw)) = rhs.split_once(" or base toughness ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: None,
                additive_types: false,
                base_pt: Some((p_raw.trim().to_string(), t_raw.trim().to_string())),
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }

    // "<subj> have/has base power and toughness P/T [and are/is <types> in addition to their/its other types]" (Kudo, King Among Bears)
    if let Some((subj_raw, rhs)) = low
        .split_once(" have base power and toughness ")
        .or_else(|| low.split_once(" has base power and toughness "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let pt_tok = rhs.split_whitespace().next().unwrap_or("");
        if let Some((p, t)) = parse_base_pt(pt_tok.trim_end_matches(',')) {
            let rest = rhs[pt_tok.len()..].trim();
            let (set_types, additive_types) = if let Some(after_are) = rest
                .strip_prefix("and are ")
                .or_else(|| rest.strip_prefix("and is "))
            {
                let ty = after_are
                    .split_once(" in addition to ")
                    .map_or(after_are, |(a, _)| a);
                (
                    Some(ty.trim().to_string()),
                    after_are.contains("in addition to"),
                )
            } else {
                (None, false)
            };
            let grant_keywords = if set_types.is_none() && !rest.is_empty() {
                let kw_cand = rest
                    .strip_prefix("and have ")
                    .or_else(|| rest.strip_prefix("and has "))
                    .or_else(|| rest.strip_prefix("and "))
                    .or_else(|| rest.strip_prefix(", "))
                    .unwrap_or(rest)
                    .trim();
                parse_keyword_list(kw_cand, vocab).unwrap_or_default()
            } else {
                Vec::new()
            };
            let where_x_ok = rest
                .strip_prefix("where x is ")
                .is_some_and(|wx| parse_value_expr(wx, vocab).is_ok());
            if rest.is_empty() || set_types.is_some() || !grant_keywords.is_empty() || where_x_ok {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::ContinuousLayerEffect {
                        subject: Box::new(subj),
                        set_types,
                        additive_types,
                        base_pt: Some((p, t)),
                        set_colors: None,
                        grant_keywords,
                        lose_keywords: Vec::new(),
                        lose_all_abilities: false,
                        condition: None,
                    },
                ));
            }
        }
    }

    // 7d. Keyword grant with derived cost ("Each <filter> has <kw>. Its/The <kw> cost is equal to <rule>.")
    // and single-sentence "Each <filter> has <kw> {X}, where X is <val>" (Sliver Gravemother)
    if let Some((before_wx, wx_raw)) = low.split_once(", where x is ")
        && let Some((subj_raw, kw_part)) = before_wx
            .split_once(" has ")
            .or_else(|| before_wx.split_once(" have "))
        && let Some(kw_name) = kw_part.strip_suffix(" {x}")
        && !kw_name.contains(' ')
        && let (Ok(subj), Ok(_val)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_value_expr(wx_raw, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::GrantKeywordWithCost {
                subject: Box::new(subj),
                keyword: kw_name.to_string(),
                cost_rule: wx_raw.to_string(),
                condition: None,
            },
        ));
    }
    // (Dream Devourer, The Cave of Skulls, Varolz, Underworld Breach)
    if let Some((first_sent, second_sent)) = low.split_once(". ")
        && (second_sent.starts_with("its ") || second_sent.starts_with("the "))
        && let Some((_, cost_rule)) = second_sent.split_once(" cost is equal to ")
        && let Some((subj_raw, kw_raw)) = first_sent
            .split_once(" has ")
            .or_else(|| first_sent.split_once(" have "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let kw = kw_raw.trim();
        if !kw.is_empty() && !kw.contains(' ') {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::GrantKeywordWithCost {
                    subject: Box::new(subj),
                    keyword: kw.to_string(),
                    cost_rule: cost_rule.trim().to_string(),
                    condition: None,
                },
            ));
        }
    }

    // 7e. Standalone "<subj> loses/lose all abilities", "<subj> are/is every basic land/creature type ...",
    // "<subj> are/is <types> in addition to their/its other [colors and] types",
    // "<subj> doesn't/don't untap during its/their controller(s)' untap step(s)",
    // "its/their activated abilities can't be activated"
    if let Some(subj_raw) = low
        .strip_suffix(" loses all abilities")
        .or_else(|| low.strip_suffix(" lose all abilities"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: None,
                additive_types: false,
                base_pt: None,
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: true,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" are ").or_else(|| low.split_once(" is "))
        && (rhs.ends_with(" in addition to their other types")
            || rhs.ends_with(" in addition to its other types")
            || rhs.ends_with(" in addition to their other colors and types")
            || rhs.ends_with(" in addition to its other colors and types")
            || rhs == "every basic land type"
            || rhs == "every creature type")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let ty_clean = rhs
            .split_once(" in addition to ")
            .map_or(rhs, |(t, _)| t)
            .trim();
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some(ty_clean.to_string()),
                additive_types: rhs.contains("in addition to"),
                base_pt: None,
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }
    if let Some(subj_raw) = low
        .strip_suffix(" doesn't untap during its controller's untap step")
        .or_else(|| low.strip_suffix(" don't untap during their controllers' untap steps"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "DoesntUntap".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if low == "its activated abilities can't be activated"
        || low == "their activated abilities can't be activated"
        || low == "its activated abilities can't be activated unless they're mana abilities"
        || low == "their activated abilities can't be activated unless they're mana abilities"
        || low == "you may activate ~'s loyalty abilities any time you could cast an instant"
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Anaphora("it".to_string()))),
                rule: format!("AbilityTimingOrLock:{low}"),
                filter: None,
                condition: None,
            },
        ));
    }

    // 8. CDA P/T (including Tarmogoyf / Nethergoyf / Souls of the Lost "power is X and toughness is that number plus N")
    if let Some(rest) = low.strip_prefix("~'s power is equal to ")
        && let Some((val_raw, plus_raw)) =
            rest.split_once(" and its toughness is equal to that number plus ")
        && let (Ok(val), Some(n)) = (
            parse_value_expr(val_raw, vocab),
            parse_number_word(plus_raw.trim()),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::CdaPowerToughness {
                power: Some(Box::new(val.clone())),
                toughness: Some(Box::new(ValueExprAst::Add(
                    Box::new(val),
                    Box::new(ValueExprAst::Const(n)),
                ))),
            },
        ));
    }
    // General asymmetric CDA: "~'s power is equal to <v1> and its toughness is equal to <v2>" (Shapeshifter, Sutured Ghoul)
    if let Some(rest) = low.strip_prefix("~'s power is equal to ")
        && let Some((p_raw, t_raw)) = rest.split_once(" and its toughness is equal to ")
        && let (Ok(pv), Ok(tv)) = (
            parse_value_expr(p_raw, vocab),
            parse_value_expr(t_raw, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::CdaPowerToughness {
                power: Some(Box::new(pv)),
                toughness: Some(Box::new(tv)),
            },
        ));
    }
    if let Some(val_raw) = low.strip_prefix("~'s power and toughness are each equal to ") {
        let val = parse_value_expr(val_raw, vocab)?;
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::CdaPowerToughness {
                power: Some(Box::new(val.clone())),
                toughness: Some(Box::new(val)),
            },
        ));
    }
    if let Some(val_raw) = low.strip_prefix("~'s power is equal to ") {
        let val = parse_value_expr(val_raw, vocab)?;
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::CdaPowerToughness {
                power: Some(Box::new(val)),
                toughness: None,
            },
        ));
    }
    if let Some(val_raw) = low.strip_prefix("~'s toughness is equal to ") {
        let val = parse_value_expr(val_raw, vocab)?;
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::CdaPowerToughness {
                power: None,
                toughness: Some(Box::new(val)),
            },
        ));
    }

    // 9. Cost modifiers + Strive + Alternative cost + Flash-sac + Flashback reduction + Additional cost + Cast permissions
    if low.starts_with("you may cast ~ as though it had flash. if you cast it any time a sorcery couldn't have been cast,") {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::SelfObject)),
                rule: "FlashSacrificeAtCleanup".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(after_fb) = s.strip_prefix("Flashback ")
        && let Some((fb_cost, rest)) = after_fb.split_once(". ~ costs ")
        && let Some((delta_tok, _)) = rest.split_once(" less to cast this way")
        && is_mana_sequence(fb_cost.trim())
        && is_mana_sequence(delta_tok.trim())
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::FlashbackCostModifier {
                cost: fb_cost.trim().to_string(),
                delta: delta_tok.trim().to_string(),
            },
        ));
    }
    if let Some(after) = s
        .strip_prefix("You may ")
        .or_else(|| s.strip_prefix("you may "))
        && let Some((cost_raw, _for_spells)) = after
            .split_once(" rather than pay the mana cost for ")
            .or_else(|| after.split_once(" rather than pay ~'s mana cost"))
            .or_else(|| after.split_once(" rather than paying ~'s mana cost"))
    {
        let cost_clean = cost_raw
            .strip_prefix("pay ")
            .or_else(|| cost_raw.strip_prefix("Pay "))
            .unwrap_or(cost_raw);
        if let Ok(cost) = parse_cost_list(cost_clean, vocab) {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::AlternativeCastCost { cost },
            ));
        }
    }
    // Activated ability cost modifier: "Abilities ... cost {M} more/less to activate [for each ...]" (Kopala, Training Grounds, Dragonkin Berserker)
    if let Some((idx, vlen)) = low
        .find(" cost ")
        .map(|i| (i, 6))
        .or_else(|| low.find(" costs ").map(|i| (i, 7)))
        && (low[..idx].contains("abilities") || low[..idx].contains("ability"))
        && let Some((delta_tok, tail)) = s[idx + vlen..].trim().split_once(' ')
        && is_mana_sequence(delta_tok)
        && (tail.to_ascii_lowercase().starts_with("more to activate")
            || tail.to_ascii_lowercase().starts_with("less to activate"))
    {
        let tlow = tail.to_ascii_lowercase();
        let less = tlow.starts_with("less to activate");
        let scale_expr = tlow
            .split_once(" for each ")
            .and_then(|(_, fe)| parse_for_each_value_expr(fe, vocab).ok())
            .map(Box::new);
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::CostModifier {
                subject: Box::new(SubjectRefAst::Anaphora(s[..idx].trim().to_string())),
                delta: delta_tok.to_string(),
                less,
                next_only: false,
                per: None,
                scale_expr,
                condition: None,
            },
        ));
    }
    if let Some(after) = s
        .strip_prefix("As an additional cost to cast ~, you may ")
        .or_else(|| s.strip_prefix("as an additional cost to cast ~, you may "))
        && let Some((cost_part, rest)) = after.split_once(". ~ costs ")
        && (rest.to_ascii_lowercase().contains("less to cast for each ")
            || rest.to_ascii_lowercase().contains("less to cast if "))
        && let Ok(ca) = parse_cost_atom(cost_part, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::AdditionalCastCost { cost: ca },
        ));
    }
    if let Some(after) = s.strip_prefix("~ costs ")
        && let Some(delta) = after.strip_suffix(" more to cast for each target beyond the first")
        && is_mana_sequence(delta.trim())
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::StrivePerExtraTarget {
                delta: delta.trim().to_string(),
            },
        ));
    }
    if let Some((idx, vlen)) = low
        .find(" costs ")
        .map(|i| (i, 7))
        .or_else(|| low.find(" cost ").map(|i| (i, 6)))
    {
        let after = s[idx + vlen..].trim();
        if let Some((delta_tok, tail)) = after.split_once(' ')
            && is_mana_sequence(delta_tok)
        {
            let tlow = tail.to_ascii_lowercase();
            let (less, fe_opt, cond_opt, turn_cond) =
                if let Some(fe) = tlow.strip_prefix("less to cast for each ") {
                    (Some(true), Some(fe), None, None)
                } else if let Some(fe) = tlow.strip_prefix("more to cast for each ") {
                    (Some(false), Some(fe), None, None)
                } else if let Some(c) = tlow.strip_prefix("less to cast if ") {
                    (Some(true), None, Some(c), None)
                } else if let Some(c) = tlow.strip_prefix("more to cast if ") {
                    (Some(false), None, Some(c), None)
                } else if tlow == "less to cast during your turn"
                    || tlow == "less to cast during your end step"
                {
                    (Some(true), None, None, Some(ConditionAst::IsYourTurn))
                } else if tlow == "more to cast during your turn" {
                    (Some(false), None, None, Some(ConditionAst::IsYourTurn))
                } else if tlow == "less to cast during opponents' turns"
                    || tlow == "less to cast during an opponent's turn"
                {
                    (
                        Some(true),
                        None,
                        None,
                        Some(ConditionAst::Not(Box::new(ConditionAst::IsYourTurn))),
                    )
                } else if tlow == "more to cast except during its controller's turn" {
                    (
                        Some(false),
                        None,
                        None,
                        Some(ConditionAst::Not(Box::new(ConditionAst::IsYourTurn))),
                    )
                } else if let Some(wx) = tlow.strip_prefix("less to cast, where x is ")
                    && parse_value_expr(wx, vocab).is_ok()
                {
                    (Some(true), None, None, None)
                } else if tlow == "less to cast" {
                    (Some(true), None, None, None)
                } else if tlow == "more to cast" {
                    (Some(false), None, None, None)
                } else {
                    (None, None, None, None)
                };
            if let Some(is_less) = less {
                let raw_left = s[..idx].trim();
                let (subj_part, _target_clause) = match raw_left.split_once(" that target ") {
                    Some((l, t)) => (l.trim(), Some(t.trim())),
                    None => (raw_left, None),
                };
                let subj_raw = subj_part
                    .replace(" you cast from anywhere other than your hand", "")
                    .replace(" you cast but don't own", "")
                    .replace(" your opponents cast", "")
                    .replace(" opponents cast", "")
                    .replace(" enchanted player casts", "")
                    .replace(" you cast", "")
                    .replace(" players cast", "");
                let subj_trimmed = subj_raw.trim().trim_end_matches(" each turn").trim();
                let subj_clean = subj_trimmed
                    .strip_prefix("The first ")
                    .or_else(|| subj_trimmed.strip_prefix("the first "))
                    .or_else(|| subj_trimmed.strip_prefix("The second "))
                    .or_else(|| subj_trimmed.strip_prefix("the second "))
                    .or_else(|| subj_trimmed.strip_prefix("Each "))
                    .or_else(|| subj_trimmed.strip_prefix("each "))
                    .unwrap_or(subj_trimmed);
                let subj_opt = if subj_clean == "~" {
                    Some(SubjectRefAst::SelfObject)
                } else if subj_clean.eq_ignore_ascii_case("it")
                    || subj_clean.eq_ignore_ascii_case("it also")
                {
                    Some(SubjectRefAst::Anaphora("it".to_string()))
                } else {
                    parse_object_filter(subj_clean, vocab)
                        .ok()
                        .map(|f| SubjectRefAst::Group(Box::new(f)))
                };
                let per_scale_opt = match fe_opt {
                    Some("basic land type among lands you control") => {
                        Some((None, Some(Box::new(ValueExprAst::DomainCount))))
                    }
                    Some(fe) => {
                        if let Ok(flt) = parse_object_filter(fe, vocab) {
                            Some((Some(Box::new(flt)), None))
                        } else {
                            parse_for_each_value_expr(fe, vocab)
                                .ok()
                                .map(|val| (None, Some(Box::new(val))))
                        }
                    }
                    None => Some((None, None)),
                };
                let cond_res = match (cond_opt, turn_cond) {
                    (Some(c), _) => parse_condition(c, vocab)
                        .ok()
                        .map(|cond| Some(Box::new(cond))),
                    (None, Some(tc)) => Some(Some(Box::new(tc))),
                    (None, None) => Some(None),
                };
                if let (Some(subj), Some((per, scale_expr)), Some(condition)) =
                    (subj_opt, per_scale_opt, cond_res)
                {
                    return Ok(StaticOrReplacement::Static(
                        StaticAbilityAst::CostModifier {
                            subject: Box::new(subj),
                            delta: delta_tok.to_string(),
                            less: is_less,
                            next_only: false,
                            per,
                            scale_expr,
                            condition,
                        },
                    ));
                }
            }
        }
    }

    if let Some(after) = s
        .strip_prefix("As an additional cost to cast ")
        .or_else(|| s.strip_prefix("as an additional cost to cast "))
        && let Some((spell_part, cost_raw)) = after.split_once(", ")
        && (spell_part == "~" || parse_object_filter(spell_part, vocab).is_ok())
    {
        let cost_clean = cost_raw
            .strip_prefix("you may ")
            .or_else(|| cost_raw.strip_prefix("You may "))
            .unwrap_or(cost_raw);
        let cost_first_sent = cost_clean.split_once(". ").map_or(cost_clean, |(a, _)| a);
        if let Ok(ca) = parse_cost_atom(cost_first_sent, vocab) {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::AdditionalCastCost { cost: ca },
            ));
        }
        if let Ok(list) = parse_cost_list(cost_first_sent, vocab)
            && let Some(first) = list.into_iter().next()
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::AdditionalCastCost { cost: first },
            ));
        }
        if cost_first_sent.to_ascii_lowercase().starts_with("choose ")
            && parse_atomic_effect(cost_first_sent, vocab).is_ok()
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::AdditionalCastCost {
                    cost: CostAtomAst::Choose(Box::new(SubjectRefAst::Anaphora(
                        cost_first_sent.to_string(),
                    ))),
                },
            ));
        }
    }

    for (pref, cast_only) in [("you may cast ", true), ("you may play ", false)] {
        if let Some(rest) = low.strip_prefix(pref) {
            if rest == "~ as though it had flash" {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::PlayOrCastPermission {
                        cast_only,
                        filter: Box::new(crate::ast::ObjectFilterAst {
                            any_card: true,
                            ..Default::default()
                        }),
                        permission: "as though it had flash".to_string(),
                        condition: None,
                    },
                ));
            }
            // Conditional flash / graveyard cast: "You may cast ~ as though it had flash if <cond>"
            if let Some(cond_raw) = rest.strip_prefix("~ as though it had flash if ") {
                let cond = if cond_raw.starts_with("you pay ") {
                    let pay_rest = cond_raw.strip_prefix("you ").unwrap_or(cond_raw);
                    let cost_trimmed = pay_rest
                        .strip_suffix(" more to cast it")
                        .or_else(|| pay_rest.strip_suffix(" more to cast ~"))
                        .unwrap_or(pay_rest)
                        .trim();
                    parse_cost_atom(cost_trimmed, vocab)
                        .map(|c| ConditionAst::Marker(GameMarker::PayCostCondition(Box::new(c))))
                } else {
                    parse_condition(cond_raw, vocab)
                };
                if let Ok(c) = cond {
                    return Ok(StaticOrReplacement::Static(
                        StaticAbilityAst::PlayOrCastPermission {
                            cast_only,
                            filter: Box::new(crate::ast::ObjectFilterAst {
                                any_card: true,
                                ..Default::default()
                            }),
                            permission: "as though it had flash".to_string(),
                            condition: Some(Box::new(c)),
                        },
                    ));
                }
            }
            if let Some(cond_raw) = rest.strip_prefix("~ from your graveyard if ")
                && let Ok(c) = parse_condition(cond_raw, vocab)
            {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::PlayOrCastPermission {
                        cast_only,
                        filter: Box::new(crate::ast::ObjectFilterAst {
                            any_card: true,
                            ..Default::default()
                        }),
                        permission: "from your graveyard".to_string(),
                        condition: Some(Box::new(c)),
                    },
                ));
            }
            if let Some(after_gy) = rest.strip_prefix("~ from your graveyard ")
                && (after_gy.starts_with("by ") || after_gy.starts_with("using its "))
            {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::PlayOrCastPermission {
                        cast_only,
                        filter: Box::new(crate::ast::ObjectFilterAst {
                            any_card: true,
                            ..Default::default()
                        }),
                        permission: format!("from your graveyard {after_gy}"),
                        condition: None,
                    },
                ));
            }
            // Broad play/cast from zone permissions:
            // "you may play the top card of your library"
            // "you may play lands and cast spells from <zone>"
            // "you may play a <flt> land or cast a <flt> spell from <zone> [without paying ...]"
            // "you may cast <flt> from <zone> [by <cost> | without paying its mana cost | if <cond>]"
            if rest == "the top card of your library" {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::PlayOrCastPermission {
                        cast_only,
                        filter: Box::new(crate::ast::ObjectFilterAst {
                            any_card: true,
                            ..Default::default()
                        }),
                        permission: "from the top of your library".to_string(),
                        condition: None,
                    },
                ));
            }
            if rest.contains("exiled with ")
                && let Ok(flt) = parse_object_filter(rest, vocab)
            {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::PlayOrCastPermission {
                        cast_only,
                        filter: Box::new(flt),
                        permission: "from exile".to_string(),
                        condition: None,
                    },
                ));
            }
            if let Some(zone_part) = rest
                .strip_prefix("lands and cast spells from ")
                .or_else(|| rest.strip_prefix("a land or cast a spell from "))
            {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::PlayOrCastPermission {
                        cast_only: false,
                        filter: Box::new(crate::ast::ObjectFilterAst {
                            any_card: true,
                            ..Default::default()
                        }),
                        permission: format!("from {zone_part}"),
                        condition: None,
                    },
                ));
            }
            if let Some((_, right_cast)) = rest.split_once(" land or cast ")
                && let Some((flt_raw, zone_rest)) = right_cast.split_once(" from ")
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::PlayOrCastPermission {
                        cast_only: false,
                        filter: Box::new(flt),
                        permission: format!("from {zone_rest}"),
                        condition: None,
                    },
                ));
            }
            if let Some((flt_raw, zone_rest)) = rest
                .strip_prefix("lands and cast ")
                .and_then(|r| r.split_once(" from "))
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::PlayOrCastPermission {
                        cast_only: false,
                        filter: Box::new(flt),
                        permission: format!("from {zone_rest}"),
                        condition: None,
                    },
                ));
            }
            if let Some((flt_raw, zone_rest)) = rest.split_once(" from ") {
                let (zone_part, cond_opt) = match zone_rest.split_once(" if ") {
                    Some((z, c)) => (z.trim(), parse_condition(c, vocab).ok()),
                    None => (zone_rest.trim(), None),
                };
                if zone_part.starts_with("your graveyard")
                    || zone_part.starts_with("the top of your graveyard")
                    || zone_part.starts_with("the top of your library")
                    || zone_part.starts_with("your hand")
                    || zone_part.starts_with("exile")
                    || zone_part.starts_with("among ")
                {
                    let flt_opt = if flt_raw == "~" {
                        Some(crate::ast::ObjectFilterAst {
                            any_card: true,
                            ..Default::default()
                        })
                    } else {
                        parse_object_filter(flt_raw, vocab).ok()
                    };
                    if let Some(flt) = flt_opt
                        && (!zone_rest.contains(" if ") || cond_opt.is_some())
                    {
                        return Ok(StaticOrReplacement::Static(
                            StaticAbilityAst::PlayOrCastPermission {
                                cast_only,
                                filter: Box::new(flt),
                                permission: format!("from {zone_part}"),
                                condition: cond_opt.map(Box::new),
                            },
                        ));
                    }
                }
            }
            for perm in [
                "as though they had flash",
                "from your graveyard",
                "from the top of your library",
                "from exile",
                "from among cards exiled with ~",
                "from your hand without paying their mana costs",
                "without paying their mana costs",
            ] {
                if let Some(flt_raw) = rest.strip_suffix(&format!(" {perm}")) {
                    let flt = if flt_raw == "~" {
                        crate::ast::ObjectFilterAst {
                            any_card: true,
                            ..Default::default()
                        }
                    } else if let Ok(f) = parse_object_filter(flt_raw, vocab) {
                        f
                    } else {
                        continue;
                    };
                    return Ok(StaticOrReplacement::Static(
                        StaticAbilityAst::PlayOrCastPermission {
                            cast_only,
                            filter: Box::new(flt),
                            permission: perm.to_string(),
                            condition: None,
                        },
                    ));
                }
            }
        }
    }

    // 10. Global rules, deck construction, visibility, combat restrictions, damage prevention, type change
    if low.starts_with("a deck can have any number of cards named ")
        || low.starts_with("companion \u{2014} ")
        || low == "draft ~ face up"
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::DeckConstructionRule(s.to_string()),
        ));
    }
    match low.as_str() {
        "~ can't be countered" => {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: None,
                    rule: "CantBeCountered".to_string(),
                    filter: None,
                    condition: None,
                },
            ));
        }
        "you have no maximum hand size" | "players have no maximum hand size" => {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: None,
                    rule: low,
                    filter: None,
                    condition: None,
                },
            ));
        }
        "you may play an additional land on each of your turns"
        | "you may play lands and cast spells from the top of your library"
        | "you may choose not to untap ~ during your untap step"
        | "~ can be your commander"
        | "play with the top card of your library revealed"
        | "players play with the top card of their libraries revealed"
        | "you may look at the top card of your library any time"
        | "skip your draw step"
        | "skip your untap step"
        | "players can't gain life"
        | "your opponents can't gain life"
        | "opponents can't gain life"
        | "damage can't be prevented"
        | "creatures can't be regenerated"
        | "your opponents can't cast spells during your turn"
        | "players can't cast spells during your turn"
        | "you may spend mana as though it were mana of any color"
        | "you may spend mana as though it were mana of any type"
        | "you may have ~ assign its combat damage as though it weren't blocked"
        | "each player can't cast more than one spell each turn"
        | "players can't cast more than one spell each turn"
        | "your opponents can't cast more than one spell each turn"
        | "you can't cast more than one spell each turn"
        | "each player can't draw more than one card each turn"
        | "players can't draw more than one card each turn"
        | "players can't untap more than two permanents during their untap steps"
        | "players can't untap more than one permanent during their untap steps"
        | "players can't untap more than one land during their untap steps"
        | "no more than one creature can attack each combat"
        | "no more than one creature can block each combat"
        | "players play with their hands revealed"
        | "your opponents play with their hands revealed"
        | "while voting, you may vote an additional time"
        | "you don't lose the game for having 0 or less life"
        | "you can't lose the game and your opponents can't win the game"
        | "you may exert ~ as it attacks"
        | "~ is all colors"
        | "this ability doesn't affect its color identity"
        | "activated abilities of sources with the chosen name can't be activated unless they're mana abilities"
        | "players can't cycle cards"
        | "your life total can't change"
        | "players can't search libraries"
        | "your opponents can't search libraries"
        | "you can't spend mana to cast ~"
        | "enchanted player can't gain life"
        | "that player can't gain life for the rest of the game"
        | "combat damage can't be prevented"
        | "damage that would be dealt by ~ can't be prevented"
        | "lands can't enter the battlefield"
        | "~ can't attack during extra turns"
        | "~ can't attack if it attacked during your last turn"
        | "players can't cast spells during combat"
        | "your opponents can't cast spells during combat"
        | "you can't play lands or cast spells from your hand"
        | "enchanted creature can't attack, block, or transform"
        | "each opponent can't draw more than one card each turn"
        | "while voting, you get an additional vote"
        | "you can't lose the game"
        | "~ is the chosen type"
        | "enchanted land is the chosen type"
        | "~ is every creature type"
        | "~ is every nonbasic land type"
        | "~ isn't legendary if it's a token"
        | "players skip their untap steps"
        | "players skip their upkeep steps"
        | "you may look at cards exiled with ~"
        | "all walls able to block ~ do so"
        | "all morph costs cost {2} more"
        | "unlock costs you pay cost {1} less"
        | "players can't draw cards or gain life"
        | "damage isn't removed from ~ during cleanup steps"
        | "all lands lose all abilities except mana abilities"
        | "you gain life rather than lose life from radiation"
        | "players can cast spells only during their own turns"
        | "all damage is dealt as though its source had wither"
        | "any player may cast spells as though they had flash"
        | "you may discard ~ any time you could cast an instant"
        | "enchanted creature can attack as though it had haste"
        | "no more than one creature can attack you each combat"
        | "the chosen player reveals each card they draw"
        | "the chosen player's maximum hand size is four"
        | "enchanted creature has landwalk of the chosen type"
        | "you may spend white mana as though it were red mana"
        | "creatures you control enter as a copy of ~"
        | "creatures played by your opponents enter tapped"
        | "as ~ becomes attached to a creature, choose a color"
        | "all creatures with flying able to block ~ do so"
        | "double all damage equipped creature would deal"
        | "humans you control have each of the chosen abilities"
        | "skip your upkeep step if you have no cards in hand"
        | "players can't activate planeswalkers' loyalty abilities"
        | "enchanted creature can't attack, block, or crew vehicles"
        | "~ can't attack a player it has already attacked this turn"
        | "damage doesn't cause you to lose life"
        | "it must be blocked if able"
        | "mana abilities of ~ cost an additional 1 life to activate"
        | "counters remain on ~ as it moves to any zone other than a player's hand or library"
        | "you can cast spells only during your turn and you can cast no more than two spells each turn"
        | "all creatures become black and all lands become swamps"
        | "you may tap lands you don't control for mana"
        | "you may activate the loyalty abilities of ~ twice each turn rather than only once"
        | "you may activate equip abilities any time you could cast an instant"
        | "you may spend mana as though it were mana of any color to pay the activation costs of ~'s abilities"
        | "you may look at face-down creatures you don't control any time"
        | "you may look at the top card of your library and at face-down creatures you don't control any time"
        | "you may play cards exiled with ~, and you may spend mana as though it were mana of any color to cast those spells"
        | "you may have ~ assign his combat damage as though he weren't blocked"
        | "you may have ~ assign her combat damage as though she weren't blocked"
        | "if a creature you control attacks, ~ also attacks if able"
        | "each player may play an additional land during each of their turns"
        | "each player may play an additional land on each of their turns"
        | "each player may attack only the nearest opponent in the chosen direction and planeswalkers controlled by that opponent"
        | "each player may attack only the nearest opponent in the last chosen direction and planeswalkers controlled by that opponent"
        | "you can't win the game and your opponents can't lose the game"
        | "you may cast the exiled card, and mana of any type can be spent to cast that spell"
        | "you may activate exhaust abilities as though they haven't been activated"
        | "its activated abilities can't be activated unless they're mana abilities"
        | "plotting cards from your hand costs {2} less"
        | "you may pay {0} rather than pay the equip cost of the first equip ability you activate during each of your turns"
        | "~ and enchanted creature each get +1/+1 for each creature you control and +1/+1 for each aura you control"
        | "each opponent can cast spells only any time they could cast a sorcery"
        | "you can't cast ~ during your first, second, or third turns of the game"
        | "you can't choose an untapped creature as ~'s target as you cast it"
        | "if ~ is your commander, choose a color before the game begins. ~ is the chosen color" => {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: None,
                    rule: low,
                    filter: None,
                    condition: None,
                },
            ));
        }
        "you control enchanted creature"
        | "you control enchanted permanent"
        | "you control enchanted artifact"
        | "you control enchanted artifact creature"
        | "you control enchanted land"
        | "you control enchanted equipment"
        | "you control enchanted enchantment" => {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::ControlAttachedHost,
            ));
        }
        _ => {}
    }

    // "You / Each player may play <N> additional lands on/during each of your/their turns"
    if let Some(cnt_raw) = low.strip_prefix("you may play ").and_then(|r| {
        r.strip_suffix(" additional lands on each of your turns")
            .or_else(|| r.strip_suffix(" additional lands during each of your turns"))
    }) && parse_number_word(cnt_raw).is_some()
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(PlayerRefAst::You))),
                rule: low,
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(cnt_raw) = low.strip_prefix("each player may play ").and_then(|r| {
        r.strip_suffix(" additional lands on each of their turns")
            .or_else(|| r.strip_suffix(" additional lands during each of their turns"))
    }) && parse_number_word(cnt_raw).is_some()
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(PlayerRefAst::EachPlayer))),
                rule: low,
                filter: None,
                condition: None,
            },
        ));
    }
    // "You can/may spend mana [as though it were mana] of any type/color to cast <flt>" (Vizier of the Menagerie, Case File Auditor)
    if let Some(flt_raw) = low
        .strip_prefix("you can spend mana of any type to cast ")
        .or_else(|| low.strip_prefix("you may spend mana of any type to cast "))
        .or_else(|| low.strip_prefix("you can spend mana of any color to cast "))
        .or_else(|| low.strip_prefix("you may spend mana of any color to cast "))
        .or_else(|| {
            low.strip_prefix("you may spend mana as though it were mana of any color to cast ")
        })
        .or_else(|| {
            low.strip_prefix("you may spend mana as though it were mana of any type to cast ")
        })
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(PlayerRefAst::You))),
                rule: "SpendManaAnyTypeForFilter".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    // "<player> can't cast more than <N> spell(s) each turn" / "<player> can't untap more than <N> <flt> during ..."
    if let Some((who_raw, rest)) = low.split_once(" can't cast more than ")
        && (rest.ends_with(" spell each turn") || rest.ends_with(" spells each turn"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(player))),
                rule: low,
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some((who_raw, rest)) = low.split_once(" can't untap more than ")
        && rest.contains(" during ")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(player))),
                rule: low,
                filter: None,
                condition: None,
            },
        ));
    }
    // "you may pay {M} to cast ~" (Asmoranomardicadaistinaculdacar)
    if let Some(cost_raw) = s
        .strip_prefix("you may pay ")
        .or_else(|| s.strip_prefix("You may pay "))
        && let Some(mana_str) = cost_raw.strip_suffix(" to cast ~")
        && is_mana_sequence(mana_str.trim())
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::AlternativeCastCost {
                cost: vec![CostAtomAst::PayMana(mana_str.trim().to_string())],
            },
        ));
    }

    // "Spend only mana produced by <flt> to cast ~" (Myr Superion, Imperiosaur)
    if let Some(flt_raw) = low
        .strip_prefix("spend only mana produced by ")
        .and_then(|r| r.strip_suffix(" to cast ~"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::SelfObject)),
                rule: "SpendOnlyManaFrom".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    // "~ crews Vehicles as though its power were N greater" (Hotshot Mechanic, etc.)
    if low.starts_with("~ crews vehicles as though its power were ") {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::SelfObject)),
                rule: "CrewPowerBonus".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    // "As ~ transforms into ~, <effect>" (Zenos, Curse of Leeches)
    if let Some(after) = low.strip_prefix("as ~ transforms into ~, ")
        && let Ok(body) = parse_atomic_effect(after, vocab)
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::AsEntersAction(body),
        ));
    }

    // "<subj> can't attack you" / "<subj> can't transform" / "<subj> can't enter the battlefield"
    for (suf, rule_name) in [
        (" can't attack you", "CantAttackYou"),
        (" can't transform", "CantTransform"),
        (" can't enter the battlefield", "CantEnterBattlefield"),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf)
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: rule_name.to_string(),
                    filter: None,
                    condition: None,
                },
            ));
        }
    }
    // "Activated abilities of <flt> can't be activated unless they're mana abilities"
    if let Some(flt_raw) = low
        .strip_prefix("activated abilities of ")
        .and_then(|r| r.strip_suffix(" can't be activated unless they're mana abilities"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "NonManaActivatedAbilitiesCantBeActivated".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    // "<subj> has/have <kws> during your turn"
    if let Some(before_turn) = s
        .strip_suffix(" during your turn")
        .or_else(|| s.strip_suffix(" during your turn."))
        && let Ok(mut inner) = parse_static_or_replacement(before_turn, vocab)
    {
        attach_condition(&mut inner, ConditionAst::IsYourTurn);
        return Ok(inner);
    }

    // "<filter> enter(s) untapped" (Spelunking, Archelos, etc.)
    if let Some(subj_raw) = low
        .strip_suffix(" enter untapped")
        .or_else(|| low.strip_suffix(" enters untapped"))
        .or_else(|| low.strip_suffix(" enter the battlefield untapped"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "EntersUntapped".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    // "<filter> can't be cast" (Nevermore, Meddling Mage, Void Winnower, etc.)
    if let Some(flt_raw) = low.strip_suffix(" can't be cast")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "CantBeCast".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    // "You don't lose unspent [<color>] mana as steps and phases end" (Omnath, Kruphix, Electro, Leyline Tyrant)
    if low.starts_with("you don't lose unspent ") && low.ends_with(" mana as steps and phases end")
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(PlayerRefAst::You))),
                rule: low,
                filter: None,
                condition: None,
            },
        ));
    }

    // Orthogonal global static rules: Collector Ouphe, Torpor Orb, Illusory Angel, Grand Melee, Dragon Hunter, Storm Windrider, Pipsqueak
    if let Some(flt_raw) = low
        .strip_prefix("activated abilities of ")
        .and_then(|r| r.strip_suffix(" can't be activated"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "ActivatedAbilitiesCantBeActivated".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    if let Some(flt_raw) = low
        .strip_suffix(" entering don't cause abilities to trigger")
        .or_else(|| low.strip_suffix(" entering the battlefield don't cause abilities to trigger"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "EnterTriggersSuppressed".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    if let Some(cond_raw) = low.strip_prefix("cast ~ only if ")
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::SelfObject)),
                rule: "CastOnlyIf".to_string(),
                filter: None,
                condition: Some(Box::new(cond)),
            },
        ));
    }
    if let Some(subj_raw) = low
        .strip_suffix(" attacks each combat if able")
        .or_else(|| low.strip_suffix(" attack each combat if able"))
        .or_else(|| low.strip_suffix(" blocks each combat if able"))
        .or_else(|| low.strip_suffix(" block each combat if able"))
        .or_else(|| low.strip_suffix(" attacks or blocks each combat if able"))
        .or_else(|| low.strip_suffix(" attack or block each combat if able"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let rule = if low.contains("attacks or blocks") || low.contains("attack or block") {
            "AttacksOrBlocksEachCombatIfAble"
        } else if low.contains("block") {
            "BlocksEachCombatIfAble"
        } else {
            "AttacksEachCombatIfAble"
        };
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: rule.to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    // "<subj> can't block or be blocked by <filter>" (Sneaky Homunculus, etc.)
    if let Some((subj_raw, flt_raw)) = low.split_once(" can't block or be blocked by ")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "CantBlockOrBeBlockedBy".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    // "~ saddles Mounts and crews Vehicles as though its power were N greater" (Deathless Pilot)
    if let Some(subj_raw) = low
        .split_once(" saddles mounts and crews vehicles as though ")
        .map(|(l, _)| l)
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "SaddleCrewPowerBonus".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    // "You may activate abilities of <filter> as though those creatures had haste" (Thousand-Year Elixir)
    if let Some(rest) = low.strip_prefix("you may activate abilities of ")
        && let Some((flt_raw, _)) = rest.split_once(" as though ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "ActivateAbilitiesAsThoughHaste".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, rest)) = low.split_once(" can block ")
        && let Some(flt_raw) = rest.strip_suffix(" as though it had reach")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "CanBlockAsThoughReach".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    if let Some(subj_raw) = low.strip_suffix(" can't attack you or block creatures you control")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "CantAttackYouOrBlockYourCreatures".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(subj_raw) = low.strip_suffix(" can't attack alone")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "CantAttackAlone".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }

    // "<player> can't cast <filter/~>" / "<player> can't play <filter>" (Gideon's Intervention, Rock Jockey, etc.)
    if let Some((who_raw, flt_raw)) = low
        .split_once(" can't cast ")
        .or_else(|| low.split_once(" can't play "))
        && (flt_raw == "~" || parse_object_filter(flt_raw, vocab).is_ok())
    {
        let flt = parse_object_filter(flt_raw, vocab).unwrap_or_default();
        let who_norm = match who_raw.trim() {
            "your opponents" | "opponents" => Some(PlayerRefAst::EachOpponent),
            "players" => Some(PlayerRefAst::EachPlayer),
            other => parse_player_ref(other),
        };
        if let Some(player) = who_norm {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(SubjectRefAst::Player(player))),
                    rule: if flt_raw == "~" {
                        "CantCastSelf".to_string()
                    } else {
                        "CantCastOrPlay".to_string()
                    },
                    filter: if flt_raw == "~" {
                        None
                    } else {
                        Some(Box::new(flt))
                    },
                    condition: None,
                },
            ));
        }
    }
    // "<subj>'s controller may have it assign its combat damage as though it weren't blocked" (Indomitable Might)
    if let Some(subj_raw) = low.strip_suffix(
        "'s controller may have it assign its combat damage as though it weren't blocked",
    ) && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "AssignDamageAsThoughUnblocked".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }

    if let Some((subj_raw, blocker_raw)) = low.split_once(" can't be blocked by ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        if blocker_raw == "more than one creature" {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: "AtMostOneBlocker".to_string(),
                    filter: None,
                    condition: None,
                },
            ));
        }
        if let Ok(flt) = parse_object_filter(blocker_raw, vocab) {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: "CantBeBlockedBy".to_string(),
                    filter: Some(Box::new(flt)),
                    condition: None,
                },
            ));
        }
    }
    if let Some((blocker_raw, atk_raw)) = low.split_once(" can't block ") {
        if let (Ok(blocker), Ok(atk_flt)) = (
            parse_subject_or_target(blocker_raw, vocab),
            parse_object_filter(atk_raw, vocab),
        ) {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(blocker)),
                    rule: "CantBlock".to_string(),
                    filter: Some(Box::new(atk_flt)),
                    condition: None,
                },
            ));
        }
        if let (Ok(blocker_flt), Ok(atk)) = (
            parse_object_filter(blocker_raw, vocab),
            parse_subject_or_target(atk_raw, vocab),
        ) {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(atk)),
                    rule: "CantBeBlockedBy".to_string(),
                    filter: Some(Box::new(blocker_flt)),
                    condition: None,
                },
            ));
        }
    }
    if let Some(rest) = low.strip_prefix("all creatures able to block ")
        && let Some(subj_raw) = rest.strip_suffix(" do so")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "AllAbleBlock".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" can't be blocked except by ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        if rhs.ends_with(" or more creatures") {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: format!("MinBlockers:{rhs}"),
                    filter: None,
                    condition: None,
                },
            ));
        }
        if let Ok(flt) = parse_object_filter(rhs, vocab) {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: "CantBeBlockedExceptBy".to_string(),
                    filter: Some(Box::new(flt)),
                    condition: None,
                },
            ));
        }
    }
    if let Some((subj_raw, atk_raw)) = low.split_once(" can block only ")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(atk_raw, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "CanBlockOnly".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    if low.starts_with("creatures can't attack you")
        && low.contains(" unless their controller pays ")
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: s.to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(subj_raw) = low
        .strip_prefix("untap ")
        .and_then(|r| r.strip_suffix(" during each other player's untap step"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "UntapEachOtherUntapStep".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    // CR 508/509/614 General static permissions, combat caps, and mana-production replacements:
    // - "Any player may cast <flt> [without paying their mana costs and] as though they had flash" (Vernal Equinox, Quick Sliver, Aluren)
    // - "You may cast the first <flt> you cast each turn as though it had flash" (The Blue Spirit)
    // - "You may look at the top card of your library any time, and you may play lands [and cast <flt>] from the top of your library" (Radha, Case of the Locked Hothouse)
    // - "No more than <N> creatures can attack [you/~] each combat" (Crawlspace, The Eternal Wanderer, Caverns of Despair)
    // - "<subj> can block an additional <N> creatures each combat" / "... for each <fe>" (Watcher in the Web, Hundred-Handed One, Kemba's Legion)
    // - "If you tap a <flt> for mana, it produces <N> times as much of that mana instead" (Nyxbloom Ancient, Virtue of Strength)
    // - "If [a player taps] a <flt> [is tapped] for mana, it produces <mana> instead of any other type [and amount]" (Contamination, Hall of Gemstone, Harvest Mage, Pale Moon)
    if let Some(after_any) = low.strip_prefix("any player may cast ")
        && let Some(flt_raw) = after_any
            .strip_suffix(" without paying their mana costs and as though they had flash")
            .or_else(|| after_any.strip_suffix(" as though they had flash"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::PlayOrCastPermission {
                cast_only: true,
                filter: Box::new(flt),
                permission: if after_any.contains("without paying") {
                    "any_player_free_flash".to_string()
                } else {
                    "any_player_flash".to_string()
                },
                condition: None,
            },
        ));
    }
    if let Some(after_first) = low.strip_prefix("you may cast the first ")
        && let Some(flt_raw) =
            after_first.strip_suffix(" you cast each turn as though it had flash")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::PlayOrCastPermission {
                cast_only: true,
                filter: Box::new(flt),
                permission: "first_each_turn_flash".to_string(),
                condition: None,
            },
        ));
    }
    if let Some(after_look) =
        low.strip_prefix("you may look at the top card of your library any time, and ")
        && let Ok(StaticOrReplacement::Static(inner)) =
            parse_static_or_replacement(after_look, vocab)
    {
        return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: None,
                    rule: "look_top_library_any_time".to_string(),
                    filter: None,
                    condition: None,
                },
                inner,
            ],
        )));
    }
    if low.starts_with("no more than ")
        && low.ends_with(" each combat")
        && low.contains(" can attack")
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: low,
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" can block an additional ")
        && rhs.contains(" each combat")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("block_additional:{rhs}"),
                filter: None,
                condition: None,
            },
        ));
    }
    if (low.starts_with("if you tap ")
        || low.starts_with("if a player taps ")
        || low.starts_with("if a ")
        || low.starts_with("lands tapped for mana produce "))
        && low.contains("for mana")
        && (low.contains(" produces ") || low.contains(" produce "))
        && low.contains(" instead")
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "tap_for_mana".to_string(),
                would: "produce_mana".to_string(),
                instead: s.to_string(),
                instead_body: Some(vec![InstructionAst::AddMana {
                    player: PlayerRefAst::You,
                    mana: crate::ast::ManaSpecAst::AnyColor {
                        amount: Box::new(ValueExprAst::PriorResult),
                        single_color: true,
                    },
                }]),
            },
        ));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" can't block ")
        && let Some((flt_raw, cost_raw)) = rhs.split_once(" unless you pay ")
        && let (Ok(subj), Ok(flt), Ok(_cost)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
            parse_cost_atom(&format!("pay {cost_raw}"), vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("CantBlockUnlessPay:{cost_raw}"),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    // CR 611/614/509 Hatebear, zone-cast lock, payment lock, and block-as-though statics:
    // - "Players can't cast [<flt>] spells from <z1> or <z2>" (Grafdigger's Cage, Weathered Runestone, Soulless Jailer)
    // - "Players can't pay life or sacrifice <flt> to cast spells or activate abilities" (Angel of Jubilation, Yasharn)
    // - "Spells and abilities your opponents control can't cause ..." (Tamiyo, Ashiok, Tajuru Preserver, Sigarda)
    // - "Players can't play lands as long as <cond>" (Limited Resources)
    // - "<subj> can block creatures with <kw> as though they didn't have <kw>" (Aetherflame Wall)
    // - "You may cast ~ from your graveyard if <cond>. If you do, <effect>" (Undead Sprinter)
    // - "As ~ is turned face up, <effect>" (Gift of Doom)
    // - "If a source would deal damage to a player, you may prevent X of that damage, where X is <val>" (Battletide Alchemist)
    if (low.starts_with("players can't cast ") && low.contains(" from "))
        || (low.starts_with("players can't pay life or sacrifice ")
            && low.ends_with(" to cast spells or activate abilities"))
        || low.starts_with("spells and abilities your opponents control can't cause ")
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: low,
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(cond_raw) = low.strip_prefix("players can't play lands as long as ")
        && let Ok(cond) = parse_condition(cond_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "players_cant_play_lands".to_string(),
                filter: None,
                condition: Some(Box::new(cond)),
            },
        ));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" can block creatures with ")
        && rhs.contains(" as though ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("can_block_with:{rhs}"),
                filter: None,
                condition: None,
            },
        ));
    }
    if low.starts_with("as ~ is turned face up, ") {
        let body_orig = &s["as ~ is turned face up, ".len()..];
        if let Ok(ins) = parse_atomic_effect(body_orig, vocab) {
            return Ok(StaticOrReplacement::Static(StaticAbilityAst::FollowUp(ins)));
        }
    }
    if low.starts_with("you may cast ~ from your graveyard if ")
        && let Some((first_s, second_s)) = s.split_once(". If you do, ")
        && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(first_s, vocab)
        && let Ok(ins) = parse_atomic_effect(second_s, vocab)
    {
        return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![st, StaticAbilityAst::FollowUp(ins)],
        )));
    }
    if low.starts_with(
        "if a source would deal damage to a player, you may prevent x of that damage, where x is ",
    ) && let Some((_, wx)) = low.split_once(", where x is ")
        && let Ok(val) = parse_value_expr(wx, vocab)
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "damage_source".to_string(),
                would: "deal_damage".to_string(),
                instead: s.to_string(),
                instead_body: Some(vec![InstructionAst::PreventDamage {
                    amount: Box::new(val),
                    target: Box::new(SubjectRefAst::Player(PlayerRefAst::AnyPlayer)),
                    duration: DurationAst::Instant,
                }]),
            },
        ));
    }

    // CR 611/613 Multi-sentence compound statics & iconic hatebear/equipment statics:
    // - "<flt> in graveyards and libraries can't enter the battlefield" (Grafdigger's Cage, Weathered Runestone)
    // - "<flt> can't be cast" (Sanctum Prelate)
    // - "Your opponents can't cast spells or activate abilities of <flt>" (Myrel, Shield of Argive)
    // - "Triggered abilities you control can't cause you to sacrifice or exile <flt>" (The Master, Multiplied)
    // - "<subj> can't attack or block if <cond>" (Wirecat)
    // - "<subj> can't attack, block, or crew vehicles, and its activated abilities can't be activated unless they're mana abilities" (Bound in Gold)
    // - "<subj> can't attack or block, and its/their activated abilities with {t} in their costs can't be activated" (Serra Bestiary, Katabatic Winds)
    // - "<subj> is a <desc> with <kw> and it loses all other card types" (Swift Reconfiguration)
    // - "<subj> has \"<ab>\" and is a <type> in addition to its other types" (Red Mage's Rapier)
    // - "<subj> has <kw>, \"<ab1>,\" and \"<ab2>\"" (Swashbuckler's Whip)
    // - "<subj> has <kw1> and <kw2> x, where x is <val>" (Infantry Shield)
    // - General 2-sentence static split on ". " (Intercessor's Arrest, Relic Axe, Bride's Gown, Groom's Finery, Firestorm Phoenix)
    if let Some(flt_raw) = low
        .strip_suffix(" can't enter the battlefield")
        .or_else(|| low.strip_suffix(" can't enter"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "cant_enter_battlefield".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    if let Some(flt_raw) = low.strip_suffix(" can't be cast")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "cant_be_cast".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    if let Some(flt_raw) =
        low.strip_prefix("your opponents can't cast spells or activate abilities of ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "opponents_cant_cast_or_activate".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    if low.starts_with("triggered abilities you control can't cause you to ")
        || low.starts_with(
            "while an opponent is choosing targets as part of casting a spell they control",
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: low,
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, cond_raw)) = low.split_once(" can't attack or block if ")
        && let (Ok(subj), Ok(cond)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_condition(cond_raw, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "CantAttackOrBlockIf".to_string(),
                filter: None,
                condition: Some(Box::new(cond)),
            },
        ));
    }
    for (suf, tag) in [
        (
            " can't attack or block, and its activated abilities with {t} in their costs can't be activated",
            "cant_attack_block_tap_activate",
        ),
        (
            " can't attack or block, and their activated abilities with {t} in their costs can't be activated",
            "cant_attack_block_tap_activate",
        ),
        (
            " can't attack, block, or crew vehicles, and its activated abilities can't be activated unless they're mana abilities",
            "cant_attack_block_crew_nonmana_activate",
        ),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf)
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: tag.to_string(),
                    filter: None,
                    condition: None,
                },
            ));
        }
    }
    if let Some((subj_raw, rhs)) = low.split_once(" is a ")
        && let Some((ty_kw, _)) = rhs.split_once(" and it loses all other card types")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some(ty_kw.trim().to_string()),
                additive_types: false,
                base_pt: None,
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }
    if s.contains('"')
        && let Some((subj_raw, rhs)) = low.split_once(" has \"")
        && let Some((ab_inner, tail)) = rhs.split_once('"')
        && let Some(ty_raw) = tail
            .trim()
            .strip_prefix("and is a ")
            .or_else(|| tail.trim().strip_prefix("and is an "))
        && ty_raw.ends_with(" in addition to its other types")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let orig_ab = s
            .split_once('"')
            .and_then(|(_, r)| r.split_once('"'))
            .map_or(ab_inner, |(inner, _)| inner);
        let compiled = compile_granted_ability(orig_ab, vocab);
        return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![
                StaticAbilityAst::GrantAbilityText {
                    subject: Box::new(subj.clone()),
                    ability_text: orig_ab.to_string(),
                    compiled,
                    condition: None,
                },
                StaticAbilityAst::SetOrAddType {
                    subject: Box::new(subj),
                    type_or_subtype: ty_raw
                        .strip_suffix(" in addition to its other types")
                        .unwrap_or(ty_raw)
                        .trim()
                        .to_string(),
                    additive: true,
                    condition: None,
                },
            ],
        )));
    }
    if let Some((first_s, second_s)) = s.split_once(". ")
        && !first_s.contains('"')
        && !second_s.contains(". ")
        && let Ok(StaticOrReplacement::Static(st1)) = parse_static_or_replacement(first_s, vocab)
    {
        if let Ok(StaticOrReplacement::Static(st2)) = parse_static_or_replacement(second_s, vocab) {
            return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                vec![st1, st2],
            )));
        }
        let s2_low = second_s.to_ascii_lowercase();
        if let Some(after_if) = s2_low.strip_prefix("if ")
            && let Some((cond_raw, eff_raw)) = after_if.split_once(", ")
            && let Ok(cond) = parse_condition(cond_raw, vocab)
            && let Ok(mut sr2) = parse_static_or_replacement(
                eff_raw.strip_suffix(" instead").unwrap_or(eff_raw),
                vocab,
            )
        {
            attach_condition(&mut sr2, cond);
            if let StaticOrReplacement::Static(st2) = sr2 {
                return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                    vec![st1, st2],
                )));
            }
        }
    }

    if low.starts_with("if a spell would deal damage to a permanent or player, prevent ") {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "spell_damage".to_string(),
                would: "deal_damage".to_string(),
                instead: s.to_string(),
                instead_body: Some(vec![InstructionAst::PreventDamage {
                    amount: Box::new(ValueExprAst::Const(1)),
                    target: Box::new(SubjectRefAst::AnyTarget),
                    duration: DurationAst::Instant,
                }]),
            },
        ));
    }

    if low.starts_with("if ")
        && (low.contains(" would deal damage to ") || low.contains(" would deal combat damage to "))
        && low.contains(", prevent ")
        && (low.ends_with(" of that damage") || low.ends_with(" prevent that damage"))
    {
        let instead_part = s.split_once(", ").map_or(s, |(_, r)| r);
        let instead_body = parse_instead_body(instead_part, vocab);
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "damage_source".to_string(),
                would: "deal_damage".to_string(),
                instead: s.to_string(),
                instead_body,
            },
        ));
    }
    if let Some((subj_raw, rhs)) = low.split_once(" is also a ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::SetOrAddType {
                subject: Box::new(subj),
                type_or_subtype: rhs.to_string(),
                additive: true,
                condition: None,
            },
        ));
    }
    for (mid, rule_name) in [
        (" can't attack or block unless ", "CantAttackOrBlockUnless"),
        (" can't attack unless ", "CantAttackUnless"),
        (" can't block unless ", "CantBlockUnless"),
    ] {
        if let Some((subj_raw, cond_raw)) = low.split_once(mid)
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            if let Ok(cond) = parse_condition(cond_raw, vocab) {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::RuleOrCombatRestriction {
                        subject: Some(Box::new(subj)),
                        rule: rule_name.to_string(),
                        filter: None,
                        condition: Some(Box::new(cond)),
                    },
                ));
            }
            if let Some(cost_raw) = cond_raw.strip_prefix("you ")
                && parse_cost_atom(cost_raw, vocab).is_ok()
            {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::RuleOrCombatRestriction {
                        subject: Some(Box::new(subj)),
                        rule: format!("{rule_name}:cost:{cost_raw}"),
                        filter: None,
                        condition: None,
                    },
                ));
            }
        }
    }

    // "<subj> doesn't untap during your/its controller's untap step if <cond>"
    for untap_mid in [
        " doesn't untap during your untap step if ",
        " doesn't untap during its controller's untap step if ",
    ] {
        if let Some((subj_raw, cond_raw)) = low.split_once(untap_mid)
            && let (Ok(subj), Ok(cond)) = (
                parse_subject_or_target(subj_raw, vocab),
                parse_condition(cond_raw, vocab),
            )
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: "DoesntUntapIf".to_string(),
                    filter: None,
                    condition: Some(Box::new(cond)),
                },
            ));
        }
    }

    // Pass M orthogonal global statics:
    // - Legend rule exemption (`Mirror Box`, `Mirror Gallery`, `The Master, Multiplied`, `Sakashima`)
    // - Cost reduction riders (`Ragemonger`, `Training Grounds`, `Morophon`, `Heartstone`, `Valiant Changeling`)
    // - Cast restrictions (`You can't cast ~`, `Your opponents can't cast <flt>`, `Players can't cast <flt>`)
    // - Static damage redirection (`Pariah`: `All damage that would be dealt to <from> is dealt to <to> instead`)
    if low.starts_with("the \"legend rule\" doesn't apply") {
        let flt = low
            .strip_prefix("the \"legend rule\" doesn't apply to ")
            .and_then(|f| parse_object_filter(f, vocab).ok())
            .map(Box::new);
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "LegendRuleExempt".to_string(),
                filter: flt,
                condition: None,
            },
        ));
    }
    if low == "this effect reduces only the amount of colored mana you pay"
        || low == "this effect can't reduce the mana in that cost to less than one mana"
        || low.starts_with("this effect can't reduce the amount of mana ")
        || low.starts_with("this effect can't reduce the mana in that cost to less than ")
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: format!("CostReductionRider:{low}"),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some((who_raw, cast_raw)) = low.split_once(" can't cast ")
        && let Some(_player) = parse_player_ref(who_raw)
    {
        let flt_opt = if cast_raw == "~" {
            Some(crate::ast::ObjectFilterAst {
                any_card: true,
                ..Default::default()
            })
        } else {
            parse_object_filter(cast_raw, vocab).ok()
        };
        if let Some(flt) = flt_opt {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(SubjectRefAst::Player(
                        parse_player_ref(who_raw).unwrap_or(PlayerRefAst::AnyPlayer),
                    ))),
                    rule: format!("CantCast:{cast_raw}"),
                    filter: Some(Box::new(flt)),
                    condition: None,
                },
            ));
        }
    }
    if let Some(rest) = low.strip_prefix("all damage that would be dealt to ")
        && let Some((from_raw, to_raw)) = rest.split_once(" is dealt to ")
        && let (Ok(from), Ok(_to)) = (
            parse_subject_or_target(from_raw.trim(), vocab),
            parse_subject_or_target(to_raw.trim_end_matches(" instead").trim(), vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(from)),
                rule: format!("RedirectAllDamageTo:{}", to_raw.trim()),
                filter: None,
                condition: None,
            },
        ));
    }

    // Orthogonal Pass K statics: "You may look at the top card of your library any time",
    // "<filter> entering or dying don't cause abilities to trigger" (Hushbringer),
    // "<subj> can't be blocked [except] by <filter>" (Doorman)
    if low == "you may look at the top card of your library any time"
        || low == "you may look at the top card of that player's library any time"
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: format!("LookTopAnyTime:{low}"),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(flt_raw) = low
        .strip_suffix(" entering or dying don't cause abilities to trigger")
        .or_else(|| low.strip_suffix(" entering doesn't cause abilities to trigger"))
        .or_else(|| low.strip_suffix(" dying doesn't cause abilities to trigger"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "SuppressTriggers".to_string(),
                filter: Some(Box::new(flt)),
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, by_raw)) = low.split_once(" can't be blocked except by ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let flt = parse_object_filter(by_raw, vocab).ok().map(Box::new);
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "CantBeBlockedExceptBy".to_string(),
                filter: flt,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, by_raw)) = low.split_once(" can't be blocked by ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let flt = parse_object_filter(by_raw, vocab).ok().map(Box::new);
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "CantBeBlockedBy".to_string(),
                filter: flt,
                condition: None,
            },
        ));
    }

    // "<subj> lose(s) <keywords>" (Mystic Decree: "All creatures lose flying and islandwalk")
    if !s.contains('"')
        && let Some((subj_raw, kw_raw)) = low
            .split_once(" loses ")
            .or_else(|| low.split_once(" lose "))
        && let (Ok(subj), Some(lkws)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_keyword_list(kw_raw, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: None,
                additive_types: false,
                base_pt: None,
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: lkws,
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }

    // Maximum hand size modifiers ("Your maximum hand size is <N/val>" / "equal to <val>" / "reduced by N" / "increased by N")
    if let Some(after) = low
        .strip_prefix("your maximum hand size is ")
        .or_else(|| low.strip_prefix("each opponent's maximum hand size is "))
        .or_else(|| low.strip_prefix("each player's maximum hand size is "))
    {
        let val_raw = after
            .strip_prefix("equal to ")
            .or_else(|| after.strip_prefix("reduced by "))
            .or_else(|| after.strip_prefix("increased by "))
            .unwrap_or(after);
        if parse_value_expr(val_raw, vocab).is_ok() {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: None,
                    rule: format!("MaxHandSize:{low}"),
                    filter: None,
                    condition: None,
                },
            ));
        }
    }
    if matches!(
        low.as_str(),
        "players can't draw cards"
            | "your opponents can't draw cards"
            | "each opponent can't draw cards"
            | "opponents can't draw cards"
    ) {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: format!("CantDraw:{low}"),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("players can't untap more than ")
        && let Some((cnt_flt, _)) = after.split_once(" during their untap steps")
    {
        let (_, flt_raw) = peel_leading_count(cnt_flt, vocab);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: None,
                    rule: format!("UntapLimit:{cnt_flt}"),
                    filter: Some(Box::new(flt)),
                    condition: None,
                },
            ));
        }
    }

    // Pillowfort / attack-or-block tax ("Creatures can't attack ... unless their controller pays ...",
    // "<subj> can't attack or block unless its controller pays ...")
    if let Some((left, right)) = low.split_once(" unless ")
        && (left.contains("can't attack") || left.contains("can't block"))
        && (right.starts_with("their controller pays ")
            || right.starts_with("its controller pays "))
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: format!("AttackTax:{low}"),
                filter: None,
                condition: None,
            },
        ));
    }

    // "<subj> can't be the target of <filter>" / "<subj> can't be blocked if <cond>"
    if let Some((subj_raw, src_raw)) = low
        .split_once(" can't be the target of ")
        .or_else(|| low.split_once(" can't be the targets of "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let flt = parse_object_filter(src_raw, vocab).ok().map(Box::new);
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("CantBeTargetOf:{src_raw}"),
                filter: flt,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, cond_raw)) = low.split_once(" can't be blocked if ")
        && let (Ok(subj), Ok(cond)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_condition(cond_raw, vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "CantBeBlockedIf".to_string(),
                filter: None,
                condition: Some(Box::new(cond)),
            },
        ));
    }

    for rule in [
        "can't be blocked",
        "can't be countered",
        "can't be equipped",
        "can't be sacrificed",
        "can't become untapped",
        "can't have counters put on it",
        "can't have counters placed on it",
        "can't have counters put on them",
        "can't be the targets of spells or abilities",
        "can't be the target of spells or abilities",
        "can't be the target of spells",
        "can't block",
        "can't block and can't be blocked",
        "can't attack",
        "can't attack you or planeswalkers you control",
        "can't attack you or a planeswalker you control",
        "can't attack its owner or planeswalkers its owner controls",
        "can't be turned face up",
        "can't be enchanted",
        "can't be enchanted by other auras",
        "isn't a planeswalker",
        "isn't an artifact",
        "isn't a creature",
        "isn't a land",
        "isn't an enchantment",
        "gains all creature types",
        "gain all creature types",
        "can't phase in",
        "becomes foretold",
        "becomes unprepared",
        "can't attack or block",
        "can't attack or block, and its activated abilities can't be activated",
        "can attack as though it didn't have defender",
        "can attack as though they didn't have defender",
        "isn't a creature",
        "aren't creatures",
        "isn't an artifact",
        "isn't a human",
        "is the chosen type in addition to its other types",
        "is all creature types",
        "is every creature type",
        "is goaded",
        "is also goaded",
        "loses all other abilities",
        "lose all other abilities",
        "assigns combat damage equal to its toughness rather than its power",
        "can't attack or block alone",
        "can't attack alone",
        "can't block alone",
        "attacks each combat if able",
        "attacks or blocks each combat if able",
        "must be blocked if able",
        "can block an additional creature each combat",
        "can block any number of creatures",
        "doesn't untap during your untap step",
        "doesn't untap during its controller's untap step",
        "don't untap during their controllers' untap steps",
    ] {
        if let Some(subj_raw) = low.strip_suffix(&format!(" {rule}"))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: rule.to_string(),
                    filter: None,
                    condition: None,
                },
            ));
        }
    }
    // "<subj>'s activated abilities can't be activated" / "<player> can't activate abilities that aren't ..."
    if let Some(subj_raw) = low.strip_suffix("'s activated abilities can't be activated")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "CantActivateAbilities".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some((who_raw, rest)) = low.split_once(" can't activate abilities that aren't ")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(player))),
                rule: format!("cant_activate_non:{rest}"),
                filter: None,
                condition: None,
            },
        ));
    }
    // General CR 508/509 "as though" combat permission statics (Quagmire, Undertow, Weathered Sentinels, Chaos Lord, Glaring Spotlight)
    if let Some((subj_raw, rhs)) = low.split_once(" can be blocked as though ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("block_as_though:{rhs}"),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, rhs)) = low
        .split_once(" can attack ")
        .or_else(|| low.split_once(" can be the targets of "))
        && rhs.contains("as though ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("as_though:{rhs}"),
                filter: None,
                condition: None,
            },
        ));
    }

    // Static damage prevention ("to <subj>", "to and dealt by <subj>", "by <subj>")
    if low.starts_with("prevent all ") {
        let combat_only = low.starts_with("prevent all combat ");
        let noncombat_only = low.starts_with("prevent all noncombat ");
        if let Some((_, subj_raw)) = low.split_once("damage that would be dealt to and dealt by ")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::PreventAllDamageStatic {
                    combat_only,
                    noncombat_only,
                    both_ways: true,
                    subject: Box::new(subj.clone()),
                    by: Some(Box::new(subj)),
                },
            ));
        }
        if let Some((_, rest)) = low.split_once("damage that would be dealt to ") {
            let (subj_raw, by_opt) = match rest.split_once(" by ") {
                Some((sr, b)) => (sr, parse_subject_or_target(b, vocab).ok().map(Box::new)),
                None => (rest, None),
            };
            if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
                return Ok(StaticOrReplacement::Static(
                    StaticAbilityAst::PreventAllDamageStatic {
                        combat_only,
                        noncombat_only,
                        both_ways: false,
                        subject: Box::new(subj),
                        by: by_opt,
                    },
                ));
            }
        }
        if let Some((_, by_raw)) = low.split_once("damage that would be dealt by ")
            && let Ok(by_subj) = parse_subject_or_target(by_raw, vocab)
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::PreventAllDamageStatic {
                    combat_only,
                    noncombat_only,
                    both_ways: false,
                    subject: Box::new(SubjectRefAst::AnyTarget),
                    by: Some(Box::new(by_subj)),
                },
            ));
        }
        // "Prevent all [combat] damage that <src> would deal to <tgt>" (Goblin Furrier, etc.)
        if let Some((_, rest)) = low.split_once("damage that ")
            && let Some((src_raw, tgt_raw)) = rest.split_once(" would deal to ")
            && let (Ok(by_subj), Ok(tgt_subj)) = (
                parse_subject_or_target(src_raw, vocab),
                parse_subject_or_target(tgt_raw, vocab),
            )
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::PreventAllDamageStatic {
                    combat_only,
                    noncombat_only,
                    both_ways: false,
                    subject: Box::new(tgt_subj),
                    by: Some(Box::new(by_subj)),
                },
            ));
        }
    }

    // Type/subtype change: "Enchanted land is a Swamp" / "it's an artifact creature" / "They're Zombies in addition to their other types"
    if let Some((subj_raw, rhs)) = low
        .split_once(" is a ")
        .or_else(|| low.split_once(" is an "))
        .or_else(|| low.split_once("'s a "))
        .or_else(|| low.split_once("'s an "))
        .or_else(|| low.split_once("'re "))
        .or_else(|| low.split_once(" are "))
    {
        let (core_t, additive) = match rhs
            .strip_suffix(" in addition to its other types")
            .or_else(|| rhs.strip_suffix(" in addition to their other types"))
            .or_else(|| rhs.strip_suffix(" in addition to its other land types"))
            .or_else(|| rhs.strip_suffix(" in addition to their other land types"))
            .or_else(|| rhs.strip_suffix(" in addition to its other creature types"))
            .or_else(|| rhs.strip_suffix(" in addition to their other creature types"))
        {
            Some(c) => (c, true),
            None => (rhs, false),
        };
        if (matches!(
            core_t,
            "plains"
                | "island"
                | "swamp"
                | "mountain"
                | "forest"
                | "islands"
                | "swamps"
                | "mountains"
                | "forests"
                | "artifact"
                | "artifacts"
                | "creature"
                | "creatures"
                | "artifact creature"
                | "enchantment"
                | "enchantments"
                | "land"
                | "lands"
                | "the chosen type"
        ) || vocab.lookup_subtype(core_t).is_some()
            || (core_t.contains(' ') && parse_object_filter(core_t, vocab).is_ok()))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::SetOrAddType {
                    subject: Box::new(subj),
                    type_or_subtype: core_t.to_string(),
                    additive,
                    condition: None,
                },
            ));
        }
    }

    // Others enter tapped
    if let Some(subj_raw) = low
        .strip_suffix(" enters the battlefield tapped")
        .or_else(|| low.strip_suffix(" enter the battlefield tapped"))
        .or_else(|| low.strip_suffix(" enters tapped"))
        .or_else(|| low.strip_suffix(" enter tapped"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::OthersEnterTapped {
                subject: Box::new(subj),
            },
        ));
    }

    // Keep-existing-types sentence inside multi-sentence static paragraphs ("It's still a land.")
    if matches!(
        low.as_str(),
        "it's still a land"
            | "it's still an artifact"
            | "it's still an enchantment"
            | "it's still a planeswalker"
            | "they're still lands"
            | "they're still artifacts"
    ) {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Anaphora("it".to_string()))),
                rule: low,
                filter: None,
                condition: None,
            },
        ));
    }

    // Compound static conjunction combinator ("<static1> and <static2>", or "<static1>, <static2>, and <static3>",
    // quote-aware via split_top_level, with optional shared-subject carry-forward):
    // e.g. "Enchanted creature can't attack or block and has \"...\""
    // or "~ gets +1/+1, is black, and has \"...\""
    if !low.starts_with("if ") && !low.starts_with("as ") {
        let and_parts = split_top_level(s, " and ");
        if and_parts.len() >= 2 {
            for split_idx in 1..and_parts.len() {
                let left_owned = and_parts[..split_idx].join(" and ");
                let right_owned = and_parts[split_idx..].join(" and ");
                let left_trim = left_owned.trim();
                let right_trim = right_owned.trim();
                if let Ok(StaticOrReplacement::Static(ls)) =
                    parse_static_or_replacement(left_trim, vocab)
                {
                    if let Ok(StaticOrReplacement::Static(rs)) =
                        parse_static_or_replacement(right_trim, vocab)
                    {
                        return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                            vec![ls, rs],
                        )));
                    }
                    if let Some(subj_prefix) = extract_leading_subject_prefix(left_trim)
                        && let Ok(StaticOrReplacement::Static(rs)) = parse_static_or_replacement(
                            &format!("{subj_prefix} {right_trim}"),
                            vocab,
                        )
                    {
                        return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                            vec![ls, rs],
                        )));
                    }
                }
            }
        }
        let comma_parts = split_top_level(s, ", ");
        if comma_parts.len() >= 2 && comma_parts.len() <= 4 {
            let first = comma_parts[0].trim();
            let subj_prefix = extract_leading_subject_prefix(first);
            if let Ok(StaticOrReplacement::Static(first_st)) =
                parse_static_or_replacement(first, vocab)
            {
                let mut items = vec![first_st];
                let mut ok = true;
                for part in &comma_parts[1..] {
                    let p = part.trim();
                    let p_clean = p.strip_prefix("and ").unwrap_or(p).trim();
                    if let Ok(StaticOrReplacement::Static(st)) =
                        parse_static_or_replacement(p_clean, vocab)
                    {
                        items.push(st);
                    } else if let Some(sp) = subj_prefix
                        && let Ok(StaticOrReplacement::Static(st)) =
                            parse_static_or_replacement(&format!("{sp} {p_clean}"), vocab)
                    {
                        items.push(st);
                    } else {
                        ok = false;
                        break;
                    }
                }
                if ok && items.len() >= 2 {
                    return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                        items,
                    )));
                }
            }
        }
    }

    // Compositional Wave 16: General static rules
    // 1. "<filter> enters/enter with an additional <N> <kind> counter(s) on it/them [for each <over>]" (Giada, Kalain, Bioengineered Future)
    if let Some((subj_raw, rest)) = low
        .split_once(" enters with an additional ")
        .or_else(|| low.split_once(" enter with an additional "))
        .or_else(|| low.split_once(" enters with a "))
        .or_else(|| low.split_once(" enter with a "))
        && let Ok(_subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && (rest.contains(" counter on it")
            || rest.contains(" counter on them")
            || rest.contains(" counters on it")
            || rest.contains(" counters on them"))
    {
        let (cnt_part, per_opt) = match rest.split_once(" for each ") {
            Some((l, r)) => (l.trim(), parse_for_each_value_expr(r.trim(), vocab).ok()),
            None => (rest.trim(), None),
        };
        let kind = cnt_part
            .split_once(" counter")
            .map_or("counter", |(k, _)| k.trim());
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersWithCounters {
                counter_kind: kind.to_string(),
                count: Box::new(per_opt.unwrap_or(ValueExprAst::Const(1))),
                per: None,
                x_expr: None,
                condition: None,
            },
        ));
    }
    // 2. "<filter> enter(s) as a copy of <subj>" (Infinite Reflection)
    if let Some((subj_raw, copy_raw)) = low
        .split_once(" enter as a copy of ")
        .or_else(|| low.split_once(" enters as a copy of "))
        && parse_subject_or_target(subj_raw.trim(), vocab).is_ok()
        && let Ok(flt) = parse_object_filter(copy_raw.trim(), vocab)
    {
        return Ok(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersAsCopy {
                optional: false,
                filter: Box::new(flt),
                exception: None,
            },
        ));
    }
    // 3. "<filter> are goaded" / "<filter> are basic" (Baeloth Barrityl, Rootpath Purifier)
    for (suf, rule_name) in [
        (" are goaded", "permanents_are_goaded"),
        (" is goaded", "permanents_are_goaded"),
        (" are basic", "lands_are_basic"),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf)
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: rule_name.to_string(),
                    filter: None,
                    condition: None,
                },
            ));
        }
    }
    // 4. "<subj> can't attack or block unless <cost/cond>" / "<subj> can't be enchanted" / "other players can't gain control of <subj>" / "players don't lose unspent mana ..."
    if low == "players don't lose unspent mana as steps and phases end" {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "players_keep_unspent_mana".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(subj_raw) = low.strip_suffix(" can't be enchanted")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "cant_be_enchanted".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(subj_raw) = low.strip_suffix(" can't be attacked")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "cant_be_attacked".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(subj_raw) = low.strip_prefix("other players can't gain control of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "others_cant_gain_control".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, unless_raw)) = low
        .split_once(" can't attack or block unless ")
        .or_else(|| low.split_once(" can't attack unless "))
        .or_else(|| low.split_once(" can't block unless "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && (parse_condition(unless_raw.trim(), vocab).is_ok()
            || parse_atomic_effect(unless_raw.trim(), vocab).is_ok())
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("cant_combat_unless:{}", unless_raw.trim()),
                filter: None,
                condition: None,
            },
        ));
    }
    // 5. "The first <spell> <player> casts [during each of their turns / each turn] may be cast without paying its mana cost" (Weftwalking)
    if let Some(after_first) = low.strip_prefix("the first ")
        && let Some(before_may) = after_first
            .strip_suffix(" may be cast without paying its mana cost")
            .or_else(|| {
                after_first
                    .strip_suffix(" costs {2} less to cast and can be cast as though it had flash")
            })
    {
        let spell_part = before_may
            .split_once(" you cast")
            .or_else(|| before_may.split_once(" each player casts"))
            .map_or(before_may, |(a, _)| a);
        if let Ok(flt) = parse_object_filter(spell_part.trim(), vocab) {
            return Ok(StaticOrReplacement::Static(
                StaticAbilityAst::PlayOrCastPermission {
                    cast_only: true,
                    filter: Box::new(flt),
                    permission: low,
                    condition: None,
                },
            ));
        }
    }
    // 6. Compound cost modifiers: "~ costs {1} less to cast if <c1> and {1} less to cast if <c2>" (Assassin's Ink)
    //    and "~ costs <life> more to cast for each <over>" (Phyrexian Purge)
    if let Some(after_c) = low.strip_prefix("~ costs ")
        && let Some((left_c, right_c)) = after_c.split_once(" and ")
        && let (Ok(StaticOrReplacement::Static(s1)), Ok(StaticOrReplacement::Static(s2))) = (
            parse_static_or_replacement(&format!("~ costs {left_c}"), vocab),
            parse_static_or_replacement(&format!("~ costs {right_c}"), vocab),
        )
    {
        return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![s1, s2],
        )));
    }
    if let Some(after_c) = low.strip_prefix("~ costs ")
        && let Some((life_tok, fe_raw)) = after_c.split_once(" life more to cast for each ")
        && parse_number_word(life_tok.trim()).is_some()
        && let Ok(scale) = parse_for_each_value_expr(fe_raw.trim(), vocab)
    {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::CostModifier {
                subject: Box::new(SubjectRefAst::SelfObject),
                delta: format!("{} life", life_tok.trim()),
                less: false,
                next_only: false,
                per: None,
                scale_expr: Some(Box::new(scale)),
                condition: None,
            },
        ));
    }

    // Compositional Wave 17: Out-of-line static combinators
    if let Some(w17) = parse_wave17_statics(s, &low, vocab) {
        return Ok(w17);
    }

    // Compositional Wave 15: "~ and enchanted creature each get +X/+X" / "Enchanted creature gets -X/-Y"
    if let Some((subj_raw, pt_raw)) = low
        .split_once(" each get ")
        .or_else(|| low.split_once(" gets "))
        && let Some((dx, dy)) = parse_pt_mod(pt_raw.trim())
        && (subj_raw == "~ and enchanted creature"
            || parse_subject_or_target(subj_raw, vocab).is_ok())
    {
        let subject = if subj_raw == "~ and enchanted creature" {
            SubjectRefAst::Compound(vec![
                SubjectRefAst::SelfObject,
                SubjectRefAst::AttachedHost("enchanted creature".to_string()),
            ])
        } else {
            parse_subject_or_target(subj_raw, vocab).unwrap_or(SubjectRefAst::SelfObject)
        };
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousModifyPT {
                subject: Box::new(subject),
                dx,
                dy,
                grant_keywords: Vec::new(),
                grant_ability_text: None,
                scale: None,
                condition: None,
                paired_rule: None,
            },
        ));
    }

    // Compositional Wave 15: Universal trailing "<static> if <cond>" and "<static> as long as <cond>"
    // (Rock Jockey, War Historian, Sheltering Prayers, Ruric Thar, Scared Stiff)
    if !low.starts_with("if ") && !low.starts_with("as long as ") {
        for sep in [" as long as ", " if "] {
            if let Some(idx) = find_top_level_pattern(s, sep) {
                let head = s[..idx].trim();
                let cond_raw = s[idx + sep.len()..].trim();
                if !head.is_empty()
                    && !cond_raw.is_empty()
                    && let Ok(cond) = parse_condition(cond_raw, vocab)
                    && let Ok(mut sr) = parse_static_or_replacement(head, vocab)
                {
                    attach_condition(&mut sr, cond);
                    return Ok(sr);
                }
            }
        }
    }

    // Compositional Wave 14: General "As ~ enters, <effect_body>" replacement (Archangel of Strife, Valki, Dragon's Disciple, Frankenstein's Monster, Molten Sentry)
    if low.starts_with("as ~ enters, ") || low.starts_with("as ~ enters the battlefield, ") {
        let rest = if low.starts_with("as ~ enters the battlefield, ") {
            &s["as ~ enters the battlefield, ".len()..]
        } else {
            &s["as ~ enters, ".len()..]
        };
        let (body, fails) = parse_effect_body(rest, vocab);
        if fails.is_empty() && !body.is_empty() {
            return Ok(StaticOrReplacement::Replacement(
                ReplacementEffectAst::AsEntersAction(body),
            ));
        }
    }

    // Compositional Wave 14/15: General multi-sentence static paragraph (2..=4 sentences outside quotes),
    // allowing each sentence to be a Static, Replacement, "If <cond>, <replacement>", or FollowUp(effect)
    // (Undead Sprinter, Dragon's Disciple, Mariposa Military Base, Storage Matrix, Scheming Fence, Molten Sentry)
    {
        let sent_parts = split_top_level(s, ". ");
        if sent_parts.len() >= 2 && sent_parts.len() <= 4 {
            let mut items = Vec::new();
            let mut ok = true;
            for sp in &sent_parts {
                let st_raw = sp.trim().trim_end_matches('.').trim();
                if st_raw.is_empty() {
                    continue;
                }
                if let Ok(sr) = parse_static_or_replacement(st_raw, vocab) {
                    match sr {
                        StaticOrReplacement::Static(st) => items.push(st),
                        StaticOrReplacement::Replacement(rep) => {
                            items.push(StaticAbilityAst::FollowUp(vec![
                                InstructionAst::TemporaryReplacement {
                                    replacement: Box::new(rep),
                                    duration: DurationAst::Instant,
                                },
                            ]));
                        }
                    }
                } else if let Ok(eff) = parse_atomic_effect(st_raw, vocab)
                    && !eff.is_empty()
                {
                    items.push(StaticAbilityAst::FollowUp(eff));
                } else if let Some(after_if) = st_raw.to_ascii_lowercase().strip_prefix("if ")
                    && let Some((cond_raw, rep_raw)) = after_if.split_once(", ")
                    && let Ok(cond) = parse_condition(cond_raw, vocab)
                    && let Ok(StaticOrReplacement::Replacement(rep)) =
                        parse_static_or_replacement(rep_raw, vocab)
                {
                    items.push(StaticAbilityAst::FollowUp(vec![
                        InstructionAst::IfCondition {
                            condition: Box::new(cond),
                            then_body: vec![InstructionAst::TemporaryReplacement {
                                replacement: Box::new(rep),
                                duration: DurationAst::Instant,
                            }],
                        },
                    ]));
                } else {
                    ok = false;
                    break;
                }
            }
            if ok && items.len() >= 2 {
                return Ok(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                    items,
                )));
            }
        }
    }

    // Tabletop Honor-System physical/social static rule
    if is_physical_tabletop_clause(&low) {
        return Ok(StaticOrReplacement::Static(
            StaticAbilityAst::ManualTabletopStatic {
                prompt: s.to_string(),
            },
        ));
    }

    Err(format!("unparsed_static:{s}"))
}

/// True when `dest` is `"the battlefield"` followed strictly by orthogonal entry modifiers
/// (`tapped`, `attacking`, `face down`, `transformed`, `converted`, `under <controller>'s control`).
fn is_orthogonal_battlefield_dest(dest: &str) -> bool {
    let Some(mut rest) = dest.strip_prefix("the battlefield") else {
        return false;
    };
    rest = rest.trim();
    loop {
        if rest.is_empty() {
            return true;
        }
        let mut matched = false;
        for mod_tok in [
            "tapped and attacking defending player or a planeswalker they control",
            "tapped and attacking that player or a planeswalker they control",
            "tapped and attacking that player or a planeswalker",
            "tapped and attacking that opponent",
            "tapped and attacking that player",
            "tapped and attacking defending player",
            "tapped and attacking",
            "tapped and blocking that creature",
            "blocking that creature",
            "tapped and transformed",
            "tapped and converted",
            "tapped",
            "attacking",
            "face down",
            "face up",
            "flipped",
            "transformed",
            "converted",
            "under your control",
            "under their control",
            "under its owner's control",
            "under their owner's control",
            "under their owners' control",
            "under that player's control",
            "under an opponent's control",
            "and",
        ] {
            if let Some(after) = rest.strip_prefix(mod_tok)
                && (after.is_empty() || after.starts_with(' '))
            {
                rest = after.trim();
                matched = true;
                break;
            }
        }
        if !matched {
            return false;
        }
    }
}

fn attach_condition(target: &mut StaticOrReplacement, cond: ConditionAst) {
    match target {
        StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousModifyPT { condition, .. }
            | StaticAbilityAst::GrantKeywords { condition, .. }
            | StaticAbilityAst::GrantAbilityText { condition, .. }
            | StaticAbilityAst::InheritAbilities { condition, .. }
            | StaticAbilityAst::ContinuousLayerEffect { condition, .. }
            | StaticAbilityAst::TriggerMultiplier { condition, .. }
            | StaticAbilityAst::GrantKeywordWithCost { condition, .. }
            | StaticAbilityAst::SetOrAddType { condition, .. }
            | StaticAbilityAst::CostModifier { condition, .. }
            | StaticAbilityAst::PlayOrCastPermission { condition, .. }
            | StaticAbilityAst::DynamicBasePT { condition, .. }
            | StaticAbilityAst::RuleOrCombatRestriction { condition, .. },
        )
        | StaticOrReplacement::Replacement(ReplacementEffectAst::EntersWithCounters {
            condition,
            ..
        }) => *condition = Some(Box::new(cond)),
        StaticOrReplacement::Static(StaticAbilityAst::OncePerTurn(inner)) => {
            let mut wrapped = StaticOrReplacement::Static((**inner).clone());
            attach_condition(&mut wrapped, cond);
            if let StaticOrReplacement::Static(updated) = wrapped {
                **inner = updated;
            }
        }
        StaticOrReplacement::Static(StaticAbilityAst::Compound(items)) => {
            for item in items {
                let mut wrapped = StaticOrReplacement::Static(item.clone());
                attach_condition(&mut wrapped, cond.clone());
                if let StaticOrReplacement::Static(updated) = wrapped {
                    *item = updated;
                }
            }
        }
        _ => {}
    }
}

#[inline(never)]
fn parse_wave17_effects(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let nd_low = s_nodur.to_ascii_lowercase();

    // 1. Direct "Until <dur>, <effect_or_static_or_trigger>" prefix combinator
    //    (The Brothers' War, Nuka-Nuke Launcher, Guardian Angel, Temporal Aperture)
    if low.starts_with("until ")
        && let Some((dur_hdr, body_raw)) = s.split_once(", ")
        && !dur_hdr.is_empty()
        && !body_raw.is_empty()
        && let Some(body) =
            parse_effect_or_temp_clause(body_raw.trim(), &DurationAst::UntilYourNextTurn, vocab)
    {
        return Some(body);
    }

    // 2. General "<subj> doesn't/don't untap [during <step>]" (Telekinesis, Pollen Lullaby, Triton Tactics)
    for pat in [
        " doesn't untap",
        " don't untap",
        " doesn't untap during the player's next untap step",
        " don't untap during the player's next untap step",
        " doesn't untap during its controller's next untap step",
        " don't untap during their controllers' next untap steps",
    ] {
        if let Some(subj_raw) = nd_low.strip_suffix(pat).or_else(|| low.strip_suffix(pat))
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::CombatRuleMod {
                subject: Box::new(subj),
                rule: "doesnt_untap".to_string(),
                duration: dur.clone(),
            }]);
        }
    }

    // 3. General "<subj> attacks [<tgt>] [each combat] if able" (Taunt, Gideon Jura, The Brothers' War)
    if let Some(before_able) = nd_low
        .strip_suffix(" each combat if able")
        .or_else(|| nd_low.strip_suffix(" if able"))
        && let Some((subj_raw, tgt_raw)) = before_able
            .split_once(" attacks ")
            .or_else(|| before_able.split_once(" attack "))
        && let (Ok(subj), Ok(_tgt)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: format!("attacks_if_able:{}", tgt_raw.trim()),
            duration: dur.clone(),
        }]);
    }

    // 4. General "<subj> must be blocked [by <filter>] [each combat] if able" (Anzrag)
    if let Some(subj_raw) = nd_low
        .strip_suffix(" must be blocked each combat if able")
        .or_else(|| nd_low.strip_suffix(" must be blocked if able"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "must_be_blocked_if_able".to_string(),
            duration: dur.clone(),
        }]);
    }

    // 5. General "players can't lose life / lose the game / win the game" (Everybody Lives!)
    if matches!(
        nd_low.as_str(),
        "players can't lose life"
            | "players can't gain life"
            | "players can't lose the game or win the game"
            | "players can't win the game or lose the game"
            | "you can't lose the game and your opponents can't win the game"
    ) {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: nd_low,
            condition: None,
        }]);
    }

    // 6. General "X can't be greater than <val>" (Mortarion, Nameless Race)
    if let Some(val_raw) = low
        .strip_prefix("x can't be greater than ")
        .or_else(|| low.strip_prefix("the amount you pay can't be more than "))
        && parse_value_expr(val_raw.trim(), vocab).is_ok()
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: low.to_string(),
            condition: None,
        }]);
    }

    // 7. General "<player> [may] shuffle[s] <subj_or_zones> into <player>'s library" (Repopulate, Visions, Step Between Worlds, Trenzalore Clocktower)
    if let Some(after_shuf) = low
        .strip_prefix("shuffle ")
        .or_else(|| low.strip_prefix("each player shuffles "))
        .or_else(|| low.strip_prefix("each player may shuffle "))
        .or_else(|| low.strip_prefix("that player shuffles "))
        .or_else(|| low.strip_prefix("you may then have that player shuffle "))
    {
        if after_shuf == "that library" || after_shuf == "their library" {
            return Some(vec![InstructionAst::ShuffleLibrary {
                player: PlayerRefAst::ThatPlayer,
            }]);
        }
        if let Some((obj_raw, _lib_raw)) = after_shuf.split_once(" into ")
            && (obj_raw.contains("hand and graveyard")
                || obj_raw.contains("graveyard and hand")
                || parse_subject_or_target(obj_raw.trim(), vocab).is_ok())
        {
            return Some(vec![InstructionAst::ShuffleIntoLibrary {
                subject: Box::new(
                    parse_subject_or_target(obj_raw.trim(), vocab)
                        .unwrap_or_else(|_| SubjectRefAst::Anaphora(obj_raw.trim().to_string())),
                ),
            }]);
        }
    }

    // 8. General "<subj> deals <val> damage to <tgt>" (Aura Barbs, Widespread Brutality)
    if let Some((src_raw, rest)) = low.split_once(" deals ")
        && let Some((amt_raw, tgt_raw)) = rest.split_once(" damage to ")
        && !tgt_raw.contains(" and ")
        && let (Ok(src), Ok(amt), Ok(tgt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(
                amt_raw
                    .trim()
                    .strip_prefix("equal to ")
                    .unwrap_or(amt_raw.trim()),
                vocab,
            ),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(tgt),
            amount: Box::new(amt),
        }]);
    }
    if let Some((src_raw, rest)) = low.split_once(" deals damage equal to ")
        && let Some((amt_raw, tgt_raw)) = rest.split_once(" to ")
        && let (Ok(src), Ok(amt), Ok(tgt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(tgt),
            amount: Box::new(amt),
        }]);
    }

    // 9. Trailing "<effect> when <event>" delayed trigger (Stangg: "Sacrifice ~ when that token leaves the battlefield")
    if !low.starts_with("when ")
        && !low.starts_with("whenever ")
        && let Some(idx) = find_top_level_pattern(s, " when ")
    {
        let eff_raw = s[..idx].trim();
        let trig_raw = format!("when {}", s[idx + 6..].trim());
        if let (Ok(trigger), Ok(body)) = (
            parse_trigger_header(&trig_raw, vocab),
            parse_atomic_effect(eff_raw, vocab),
        ) {
            return Some(vec![InstructionAst::TemporaryTrigger {
                trigger,
                body,
                duration: DurationAst::Permanent,
            }]);
        }
    }

    // 10. General "<player> chooses <opt1> or <opt2>" / "you may choose new targets for <subj>" / "the new target must be <filter>" / "each copy targets a different one of <subj>"
    if let Some((_who, after_ch)) = split_leading_player_verb(s, "choose", "chooses")
        && let Some((a, b)) = after_ch.split_once(" or ")
        && !a.trim().is_empty()
        && !b.trim().is_empty()
        && a.split_whitespace().count() <= 4
        && b.split_whitespace().count() <= 4
    {
        return Some(vec![InstructionAst::ChooseOption(after_ch.to_string())]);
    }
    if low == "you may choose new targets for that copy"
        || low == "you may choose new targets for the copy"
        || low == "you may choose new targets for the copies"
    {
        return Some(vec![InstructionAst::MayChooseNewTargetsForCopy]);
    }
    if let Some(flt_raw) = low.strip_prefix("the new target must be ")
        && parse_object_filter(flt_raw.trim(), vocab).is_ok()
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: low.to_string(),
            condition: None,
        }]);
    }
    if low.starts_with("each copy targets a different one of ") {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: low.to_string(),
            condition: None,
        }]);
    }

    // 11. General "<subj> can cast only <N> more spell(s) this turn" (Irencrag Feat)
    if nd_low.starts_with("you can cast only ") && nd_low.contains(" spell") {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: nd_low,
            condition: None,
        }]);
    }

    // 12. General "<player> draws <val> cards plus an additional card for each <over>" (Commander's Insight)
    if let Some((who, rest)) = split_leading_player_verb(s, "draw", "draws")
        && let Some((base_part, fe_raw)) = rest
            .to_ascii_lowercase()
            .split_once(" plus an additional card for each ")
        && let Ok(base_v) = parse_value_expr(
            base_part
                .strip_suffix(" cards")
                .or_else(|| base_part.strip_suffix(" card"))
                .unwrap_or(base_part)
                .trim(),
            vocab,
        )
        && let Ok(fe_v) = parse_for_each_value_expr(fe_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Draw {
            player: who,
            count: Box::new(ValueExprAst::Add(Box::new(base_v), Box::new(fe_v))),
        }]);
    }

    // 13. General "<subj> becomes blocked / become blocked / become unblocked" & "exchange <s1>'s power and <s2>'s power"
    if let Some(subj_raw) = nd_low
        .strip_suffix(" become blocked")
        .or_else(|| nd_low.strip_suffix(" becomes blocked"))
        .or_else(|| nd_low.strip_suffix(" become unblocked"))
        .or_else(|| nd_low.strip_suffix(" becomes unblocked"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: if nd_low.ends_with("unblocked") {
                "becomes_unblocked".to_string()
            } else {
                "becomes_blocked".to_string()
            },
            duration: dur.clone(),
        }]);
    }
    if let Some(after_ex) = nd_low.strip_prefix("exchange ")
        && after_ex.contains(" power")
        && let Some((s1_raw, s2_raw)) = after_ex.split_once(" and ")
        && !s1_raw.is_empty()
        && !s2_raw.is_empty()
    {
        return Some(vec![InstructionAst::Exchange {
            kind: ExchangeKind::LifeWithStat(StatKind::Power),
            left: Box::new(SubjectRefAst::Anaphora(s1_raw.trim().to_string())),
            right: Some(Box::new(SubjectRefAst::Anaphora(s2_raw.trim().to_string()))),
        }]);
    }

    // 14. General "<player> reveals <val> cards [at random] from their hand" / "you may reveal ~ from your opening hand"
    if low == "you may reveal ~ from your opening hand" {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::AbilityRestriction {
                restriction: "reveal_opening_hand".to_string(),
                condition: None,
            },
        ])]);
    }
    if let Some((who, after_rev)) = split_leading_player_verb(s, "reveal", "reveals") {
        let ar_low = after_rev.to_ascii_lowercase();
        if let Some(cnt_part) = ar_low
            .strip_suffix(" at random from their hand")
            .or_else(|| ar_low.strip_suffix(" from their hand"))
            .or_else(|| ar_low.strip_suffix(" from your hand"))
        {
            let cnt_clean = cnt_part
                .strip_suffix(" cards")
                .or_else(|| cnt_part.strip_suffix(" card"))
                .unwrap_or(cnt_part)
                .trim();
            if parse_value_expr(cnt_clean, vocab).is_ok() {
                return Some(vec![InstructionAst::RevealHand { player: who }]);
            }
        }
    }

    // 15. General "<subj> has <kw_list>" (Estrid: "The token has enchant permanent and umbra armor")
    if let Some((subj_raw, kws_raw)) = nd_low
        .split_once(" has ")
        .or_else(|| nd_low.split_once(" have "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && (kws_raw.starts_with("enchant ") || parse_keyword_list(kws_raw.trim(), vocab).is_some())
    {
        return Some(vec![InstructionAst::GrantKeywords {
            subject: Box::new(subj),
            keywords: vec![kws_raw.trim().to_string()],
            duration: dur.clone(),
        }]);
    }
    if let Some((subj_raw, lw_raw)) = nd_low.split_once(" gains landwalk of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::GrantKeywords {
            subject: Box::new(subj),
            keywords: vec![format!("landwalk:{lw_raw}")],
            duration: dur.clone(),
        }]);
    }

    // 16. General "this way" cost modifier inside an effect (Mavinda: "it costs {8} more to cast this way")
    if let Some(before_tw) = low.strip_suffix(" this way")
        && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(before_tw, vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(st),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 17. General "it costs {N} less to cast ..." ("it also costs {1} less to cast if ..." - Geistlight Snare)
    if (low.starts_with("it costs ") || low.starts_with("it also costs "))
        && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(s, vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(st),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 18. General "for each <over>, <effect_or_temp_clause>" fallback
    if low.starts_with("for each ")
        && let Some((over_raw, body_raw)) = s["for each ".len()..].split_once(", ")
        && let Ok(cnt) = parse_for_each_value_expr(over_raw.trim(), vocab)
        && let Some(body) = parse_effect_or_temp_clause(body_raw.trim(), dur, vocab)
    {
        return Some(vec![InstructionAst::ForEachCount {
            count: Box::new(cnt),
            body,
        }]);
    }

    // 19. Wave 18: General "<src> deals an amount of damage of your choice to <t1> [and <t2>]" (Volcano Hellion)
    if let Some((src_raw, tgts_raw)) =
        low.split_once(" deals an amount of damage of your choice to ")
        && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        if let Some((t1_raw, t2_raw)) = tgts_raw.split_once(" and ")
            && let (Ok(t1), Ok(t2)) = (
                parse_subject_or_target(t1_raw.trim(), vocab),
                parse_subject_or_target(t2_raw.trim(), vocab),
            )
        {
            return Some(vec![
                InstructionAst::DealDamage {
                    source: Box::new(src.clone()),
                    target: Box::new(t1),
                    amount: Box::new(ValueExprAst::ChosenX),
                },
                InstructionAst::DealDamage {
                    source: Box::new(src),
                    target: Box::new(t2),
                    amount: Box::new(ValueExprAst::ChosenX),
                },
            ]);
        }
        if let Ok(tgt) = parse_subject_or_target(tgts_raw.trim(), vocab) {
            return Some(vec![InstructionAst::DealDamage {
                source: Box::new(src),
                target: Box::new(tgt),
                amount: Box::new(ValueExprAst::ChosenX),
            }]);
        }
    }

    // 20. Wave 18: General "<src> deals half <val> damage, rounded down/up, to <tgt>" (Banshee)
    if let Some((src_raw, rest)) = low.split_once(" deals half ")
        && let Some((amt_raw, tgt_raw)) = rest
            .split_once(" damage, rounded down, to ")
            .or_else(|| rest.split_once(" damage, rounded up, to "))
        && let (Ok(src), Ok(amt), Ok(tgt)) = (
            parse_subject_or_target(src_raw.trim(), vocab),
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        let rounding = if rest.contains("rounded up") {
            crate::ast::Rounding::Up
        } else {
            crate::ast::Rounding::Down
        };
        return Some(vec![InstructionAst::DealDamage {
            source: Box::new(src),
            target: Box::new(tgt),
            amount: Box::new(ValueExprAst::DivConst {
                expr: Box::new(amt),
                denom: 2,
                rounding,
            }),
        }]);
    }

    // 21. Wave 18: General "<player> destroys <subj>" / "<player> taps all <filter>" / "<player> discards each <filter>"
    if let Some((_who, obj_raw)) = split_leading_player_verb(s, "destroy", "destroys")
        && let Ok(subj) = parse_subject_or_target(obj_raw, vocab)
    {
        return Some(vec![InstructionAst::Destroy {
            subject: Box::new(subj),
        }]);
    }
    if let Some((_who, obj_raw)) = split_leading_player_verb(s, "tap", "taps") {
        let o_clean = obj_raw
            .to_ascii_lowercase()
            .strip_prefix("all ")
            .map(str::to_string)
            .unwrap_or_else(|| obj_raw.to_string());
        if let Ok(subj) = parse_subject_or_target(&o_clean, vocab) {
            return Some(vec![InstructionAst::TapPermanent {
                subject: Box::new(subj),
            }]);
        }
    }
    if let Some((who, obj_raw)) = split_leading_player_verb(s, "discard", "discards")
        && let Some(flt_raw) = obj_raw.to_ascii_lowercase().strip_prefix("each ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Discard {
            player: who,
            count: None,
            filter: Some(Box::new(flt)),
            anaphora: None,
            random: false,
        }]);
    }

    // 22. Wave 18: General "exile <val> cards from the top" & "exile cards from the top of your library equal to <val>" (Vault 112, Archaic's Agony)
    if let Some(after_ex) = low.strip_prefix("exile ") {
        if let Some(cnt_part) = after_ex.strip_suffix(" cards from the top")
            && let Ok(cnt) = parse_value_expr(cnt_part.trim(), vocab)
        {
            return Some(vec![InstructionAst::ExileTopLibrary {
                actor: Some(PlayerRefAst::You),
                whose: "your".to_string(),
                count: Box::new(cnt),
                face_down: false,
            }]);
        }
        if let Some(val_raw) = after_ex.strip_prefix("cards from the top of your library equal to ")
            && let Ok(cnt) = parse_value_expr(val_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::ExileTopLibrary {
                actor: Some(PlayerRefAst::You),
                whose: "your".to_string(),
                count: Box::new(cnt),
                face_down: false,
            }]);
        }
    }

    // 23. Wave 18: "(Its|The) <kw> cost is equal to its mana cost" (Songcrafter Mage, Araumi, Falkenrath Gorger)
    if (low.starts_with("its ") || low.starts_with("the "))
        && low.ends_with(" cost is equal to its mana cost")
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: low.to_string(),
            condition: None,
        }]);
    }

    // 24. Wave 18: "<player> can't get counters" / "put the rest back on top of your library in any order" / "<subj> gets <pt> before it fights" / "<subj> loses/gains it"
    if let Some(who_raw) = nd_low.strip_suffix(" can't get counters")
        && parse_player_ref(who_raw.trim()).is_some()
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: nd_low,
            condition: None,
        }]);
    }
    if low == "put the rest back on top of your library in any order"
        || low == "put the rest on top of your library in any order"
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(SubjectRefAst::Anaphora("the rest".to_string())),
            to: "top_of_library".to_string(),
        }]);
    }
    if let Some(before_fight) = low.strip_suffix(" before it fights")
        && let Ok(inner) = parse_atomic_effect(before_fight.trim(), vocab)
    {
        return Some(inner);
    }
    if let Some(subj_raw) = nd_low
        .strip_suffix(" loses it")
        .or_else(|| nd_low.strip_suffix(" gains it"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::GrantKeywords {
            subject: Box::new(subj),
            keywords: vec![if nd_low.ends_with("loses it") {
                "lose_chosen_ability".to_string()
            } else {
                "gain_chosen_ability".to_string()
            }],
            duration: dur.clone(),
        }]);
    }
    if let Some(after_ret) = low.strip_prefix("return ") {
        for (dest_pat, dest_name) in [
            (" to their owners' hands", "hand"),
            (" to its owner's hand", "hand"),
            (
                " to the battlefield under their owner's control",
                "battlefield",
            ),
            (
                " to the battlefield under its owner's control",
                "battlefield",
            ),
            (" to the battlefield", "battlefield"),
        ] {
            if let Some(obj_raw) = after_ret.strip_suffix(dest_pat) {
                let obj_clean = obj_raw.strip_prefix("all ").unwrap_or(obj_raw).trim();
                if let Ok(subj) = parse_subject_or_target(obj_clean, vocab) {
                    return Some(vec![InstructionAst::MoveZone {
                        subject: Box::new(subj),
                        to: dest_name.to_string(),
                    }]);
                }
            }
        }
    }

    // 25. Wave 18b: "<player> flips a coin" / "<subj> can't attack <filter>" / "exile <subj> at random" / "put another counter of that kind on <subj>"
    if let Some(who_raw) = low
        .strip_suffix(" flips a coin")
        .or_else(|| low.strip_suffix(" flip a coin"))
        && parse_player_ref(who_raw.trim()).is_some()
    {
        return Some(vec![InstructionAst::FlipCoin]);
    }
    if let Some((subj_raw, tgt_raw)) = nd_low.split_once(" can't attack ")
        && let (Ok(subj), Ok(_tgt)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: format!("cant_attack:{}", tgt_raw.trim()),
            duration: dur.clone(),
        }]);
    }
    if let Some(obj_raw) = low
        .strip_prefix("exile ")
        .and_then(|r| r.strip_suffix(" at random"))
        && let Ok(subj) = parse_subject_or_target(obj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Exile {
            actor: None,
            subject: Box::new(subj),
        }]);
    }
    if let Some(subj_raw) = low.strip_prefix("put another counter of that kind on ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::PutCounters {
            subject: Box::new(subj),
            counter_kind: "same".to_string(),
            count: Box::new(ValueExprAst::Const(1)),
        }]);
    }

    None
}

#[inline(never)]
fn parse_wave17_statics(s: &str, low: &str, vocab: &Vocabulary) -> Option<StaticOrReplacement> {
    // 1. "You may have ~ enter tapped" (Mariposa Military Base)
    if low == "you may have ~ enter tapped" || low == "you may have ~ enter the battlefield tapped"
    {
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersTapped { unless: None },
        ));
    }

    // 2. Leading "For each <over>, <static_or_replacement>" (Blue, Loyal Raptor; Invasion of Ikoria)
    if low.starts_with("for each ")
        && let Some((over_raw, rest)) = s["for each ".len()..].split_once(", ")
        && !over_raw.is_empty()
        && !rest.is_empty()
        && (parse_for_each_value_expr(over_raw.trim(), vocab).is_ok()
            || over_raw
                .to_ascii_lowercase()
                .starts_with("kind of counter on "))
        && let Ok(inner) = parse_static_or_replacement(rest.trim(), vocab)
    {
        return Some(inner);
    }

    // 3. Universal trailing "<static_or_replacement> unless <cond>" (Steel Exemplar, Galactus)
    if !low.starts_with("if ")
        && let Some(idx) = find_top_level_pattern(s, " unless ")
    {
        let head = s[..idx].trim();
        let cond_raw = s[idx + " unless ".len()..].trim();
        if !head.is_empty()
            && !cond_raw.is_empty()
            && let Ok(cond) = parse_condition(cond_raw, vocab)
            && let Ok(mut sr) = parse_static_or_replacement(head, vocab)
        {
            attach_condition(&mut sr, ConditionAst::Not(Box::new(cond)));
            return Some(sr);
        }
    }

    // 4. Compound "~ enters with <A> and <B>" / "~ enters with <A> plus an additional <B>" (Ulasht, Sheriff of Safe Passage)
    if let Some(after_ent) = low.strip_prefix("~ enters with ")
        && let Some((a, b)) = after_ent
            .split_once(" plus an additional ")
            .or_else(|| after_ent.split_once(" and "))
        && let (Ok(_), Ok(r2)) = (
            parse_static_or_replacement(&format!("~ enters with {a}"), vocab),
            parse_static_or_replacement(&format!("~ enters with {b}"), vocab),
        )
    {
        return Some(r2);
    }

    // 5. General "<subj> attacks [<tgt>] each combat if able" / "<subj> must be blocked [by <flt>] if able" (Alexios, Galactus, Ace's Baseball Bat)
    if let Some(before_able) = low
        .strip_suffix(" each combat if able")
        .or_else(|| low.strip_suffix(" if able"))
    {
        if let Some((subj_raw, tgt_raw)) = before_able
            .split_once(" attacks ")
            .or_else(|| before_able.split_once(" attack "))
            && let (Ok(subj), Ok(_tgt)) = (
                parse_subject_or_target(subj_raw.trim(), vocab),
                parse_subject_or_target(tgt_raw.trim(), vocab),
            )
        {
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: format!("attacks_if_able:{}", tgt_raw.trim()),
                    filter: None,
                    condition: None,
                },
            ));
        }
        if let Some((subj_raw, blk_raw)) = before_able.split_once(" must be blocked by ")
            && let (Ok(subj), Ok(flt)) = (
                parse_subject_or_target(subj_raw.trim(), vocab),
                parse_object_filter(blk_raw.trim(), vocab),
            )
        {
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: "must_be_blocked_by".to_string(),
                    filter: Some(Box::new(flt)),
                    condition: None,
                },
            ));
        }
    }

    // 6. General "<subj> can't attack its owner" / "<subj> can't be sacrificed"
    for (suf, rule_name) in [
        (" can't attack its owner", "cant_attack_owner"),
        (" can't be sacrificed", "cant_be_sacrificed"),
    ] {
        if let Some(subj_raw) = low.strip_suffix(suf)
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj)),
                    rule: rule_name.to_string(),
                    filter: None,
                    condition: None,
                },
            ));
        }
    }

    // 7. General "<filter> have/has <kw>" where filter has zone/type clauses (Teferi, Niv-Mizzet Supreme, Sheltering Prayers, Falkenrath Gorger)
    if let Some((subj_raw, kw_raw)) = low.split_once(" have ").or_else(|| low.split_once(" has "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::GrantKeywords {
                subject: Box::new(subj),
                keywords: kws,
                condition: None,
                paired_rule: None,
            },
        ));
    }

    // 8. General "The first time <event> each turn, <effect_or_rep>" (Edgar, Mirrormind Crown)
    if low.starts_with("the first time ")
        && let Some((_hdr, body_raw)) = s.split_once(", ")
        && let Some(body) =
            parse_effect_or_temp_clause(body_raw.trim(), &DurationAst::UntilEndOfTurn, vocab)
    {
        return Some(StaticOrReplacement::Static(StaticAbilityAst::FollowUp(
            body,
        )));
    }

    // 9. General "you may have <subj> assign its combat damage as though it weren't blocked" (Invasion of Ikoria, Cunning Giant)
    if low.contains(" assign its combat damage ") && low.contains(" as though it weren't blocked") {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::SelfObject)),
                rule: "assign_combat_damage_unblocked".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }

    // 10. Wave 18: "(Its|The) <kw> cost is equal to its mana cost" (Falkenrath Gorger) & "<subj> can't be destroyed by lethal damage unless ..."
    if (low.starts_with("its ") || low.starts_with("the "))
        && low.ends_with(" cost is equal to its mana cost")
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::SelfObject)),
                rule: low.to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, _)) = low.split_once(" can't be destroyed by lethal damage unless ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: low.to_string(),
                filter: None,
                condition: None,
            },
        ));
    }

    None
}
