//! Cost list and atomic cost parser (`CostAtomAst`).

use crate::ast::{
    CompareOp, CostAtomAst, ObjectFilterFlags, SubjectRefAst, ValueExprAst, ZoneFilter,
};
use crate::combinators::{
    anaphora, split_once_any, strip_prefix_any, strip_suffix_any, trim_punct, vconst,
};
use crate::filter::{
    parse_for_each_value_expr, parse_keyword_list, parse_object_filter, parse_subject_or_target,
    parse_value_expr,
};
use crate::preprocess::{
    count_symbol_sequence, is_mana_sequence, is_physical_tabletop_clause, split_top_level,
};
use crate::vocab::{Vocabulary, parse_number_word};

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
                // Cluster 6: Compound sacrifice/exile costs: "Sacrifice a creature and ~", "Sacrifice ~ and a land",
                // "Sacrifice a blue creature, a black creature, and a red creature", "Exile a card and ~"
                let alow = atom.trim().to_ascii_lowercase();
                let mut split_compound: Option<Vec<CostAtomAst>> = None;
                for verb in ["sacrifice ", "exile "] {
                    if let Some(rest) = alow.strip_prefix(verb) {
                        let pieces: Vec<&str> = if rest.contains(", and ") {
                            rest.split(", ")
                                .map(|p| p.trim().strip_prefix("and ").unwrap_or(p.trim()))
                                .collect()
                        } else if let Some((l, r)) = rest.split_once(" and ") {
                            vec![l.trim(), r.trim()]
                        } else {
                            Vec::new()
                        };
                        if pieces.len() >= 2 {
                            let mut parsed = Vec::with_capacity(pieces.len());
                            let mut all_ok = true;
                            for p in pieces {
                                if let Ok(ca) = parse_cost_atom(&format!("{verb}{p}"), vocab) {
                                    parsed.push(ca);
                                } else {
                                    all_ok = false;
                                    break;
                                }
                            }
                            if all_ok {
                                split_compound = Some(parsed);
                                break;
                            }
                        }
                    }
                }
                if let Some(items) = split_compound {
                    out.extend(items);
                } else if let Some((l, r)) = atom.split_once(" and ")
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
    let a_nopay = strip_prefix_any(&a, &["Pay ", "pay "]).unwrap_or(a);
    if let Some(expr_raw) = a_nopay.to_ascii_lowercase().strip_prefix("mana equal to ")
        && let Ok(expr) = parse_value_expr(expr_raw.trim(), vocab)
    {
        return Ok(CostAtomAst::PayGenericManaExpr(Box::new(expr)));
    }
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
            count: vconst(1),
            filter: Box::new(crate::ast::ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
            random: false,
            up_to: false,
            any_number: false,
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
            face_down: false,
            face_up: false,
            single_graveyard: false,
            any_number: true,
            min_one: false,
            total_mv: None,
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
            face_down: false,
            face_up: false,
            single_graveyard: false,
            any_number: false,
            min_one: false,
            total_mv: None,
        });
    }
    if let Some(flt_raw) = low.strip_prefix("exile ~ and ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Ok(CostAtomAst::Exile {
            count: vconst(1),
            filter: Box::new(flt),
            face_down: false,
            face_up: false,
            single_graveyard: false,
            any_number: false,
            min_one: false,
            total_mv: None,
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
        return Ok(CostAtomAst::BounceToHand(Box::new(anaphora(
            "card_from_hand_to_top_library",
        ))));
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
    if let Some(after) = strip_prefix_any(&a, &["Waterbend ", "waterbend "])
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
        && let Some((subj_raw, pos_pref, dest)) = rest
            .split_once(" on the bottom of ")
            .map(|(l, r)| (l, "the bottom of ", r))
            .or_else(|| {
                rest.split_once(" on top of ")
                    .map(|(l, r)| (l, "top of ", r))
            })
    {
        let (_, subj_nocnt) = peel_leading_count(subj_raw, vocab);
        if let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            .or_else(|_| parse_subject_or_target(subj_nocnt, vocab))
        {
            return Ok(CostAtomAst::MoveZone {
                subject: Box::new(subj),
                to: format!("{pos_pref}{}", dest.trim()),
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
        if let Some(all_rest) = rest.strip_prefix("all ")
            && !all_rest.ends_with(" and ~")
            && !all_rest.contains(", ")
            && let Ok(flt) = parse_object_filter(all_rest, vocab)
        {
            return Ok(CostAtomAst::SacrificeAll(Box::new(flt)));
        }
        let has_conjoined_two_object_sacrifice = (rest.contains(" and a ")
            || rest.contains(" and an ")
            || rest.contains(" and another "))
            && !rest.contains(" with ")
            && !rest.contains(" without ");
        if rest.ends_with(" and ~")
            || rest.starts_with("~ and ")
            || has_conjoined_two_object_sacrifice
            || (rest.contains(", ") && !rest.contains(", or "))
        {
            return Err(format!("unparsed_cost:{a}"));
        }
        let (core, up_to, min_one) = if let Some(r) = rest.strip_prefix("up to ") {
            (r, true, false)
        } else if let Some(r) = rest.strip_prefix("one or more ") {
            (r, false, true)
        } else {
            (rest, false, false)
        };
        let exclude_self = core.starts_with("another ");
        let (cnt, flt_raw) = peel_leading_count(core, vocab);
        let mut flt = parse_object_filter(flt_raw, vocab)?;
        if exclude_self {
            flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Ok(CostAtomAst::Sacrifice {
            count: Box::new(cnt),
            filter: Box::new(flt),
            up_to,
            min_one,
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
        let (core, up_to, any_number) = if let Some(r) = core.strip_prefix("up to ") {
            (r, true, false)
        } else if let Some(r) = core.strip_prefix("any number of ") {
            (r, false, true)
        } else {
            (core, false, false)
        };
        let (cnt, flt_raw) = if any_number {
            (ValueExprAst::Const(0), core)
        } else {
            peel_leading_count(core, vocab)
        };
        let flt = parse_object_filter(flt_raw, vocab)?;
        return Ok(CostAtomAst::Discard {
            count: Box::new(cnt),
            filter: Box::new(flt),
            random,
            up_to,
            any_number,
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
                    count: vconst(1),
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
            // Cluster 6: Exile top [filtered] card(s) of your graveyard (Nature's Kiss, Alms, Zombie Scavengers, Necratog, Spinning Darkness)
            if let Some(gy_part) = top_core.strip_suffix(" of your graveyard") {
                if gy_part == "card" {
                    return Ok(CostAtomAst::ExileTopZone {
                        zone: ZoneFilter::YourGraveyard,
                        filter: None,
                        count: vconst(1),
                        face_down,
                    });
                }
                let (cnt, flt_raw) = peel_leading_count(gy_part, vocab);
                if matches!(flt_raw, "card" | "cards") {
                    return Ok(CostAtomAst::ExileTopZone {
                        zone: ZoneFilter::YourGraveyard,
                        filter: None,
                        count: Box::new(cnt),
                        face_down,
                    });
                }
                if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
                    return Ok(CostAtomAst::ExileTopZone {
                        zone: ZoneFilter::YourGraveyard,
                        filter: Some(Box::new(flt)),
                        count: Box::new(cnt),
                        face_down,
                    });
                }
            }
        }
        if rest.starts_with("the top ") || rest.ends_with(" and ~") || rest.starts_with("~ and ") {
            return Err(format!("unparsed_cost:{a}"));
        }
        let (core, face_down, face_up) = if let Some(c) = rest.strip_suffix(" face down") {
            (c, true, false)
        } else if let Some(c) = rest.strip_suffix(" face up") {
            (c, false, true)
        } else {
            (rest, false, false)
        };
        let (core, total_mv) =
            if let Some((before_mv, mv_raw)) = core.split_once(" with total mana value ") {
                let (mv_val_str, op) = if let Some(v) = mv_raw.strip_suffix(" or greater") {
                    (v.trim(), CompareOp::Ge)
                } else if let Some(v) = mv_raw.strip_suffix(" or more") {
                    (v.trim(), CompareOp::Ge)
                } else if let Some(v) = mv_raw.strip_suffix(" or less") {
                    (v.trim(), CompareOp::Le)
                } else {
                    (mv_raw.trim(), CompareOp::Eq)
                };
                let mv_expr = parse_value_expr(mv_val_str, vocab)?;
                (before_mv.trim(), Some((op, Box::new(mv_expr))))
            } else {
                (core, None)
            };
        let single_graveyard = core.contains(" from a single graveyard");
        let (core, any_number, min_one) = if let Some(r) = core.strip_prefix("any number of ") {
            (r, true, false)
        } else if let Some(r) = core.strip_prefix("one or more ") {
            (r, false, true)
        } else {
            (core, false, false)
        };
        let (cnt, flt_raw) = if any_number {
            (ValueExprAst::Const(0), core)
        } else {
            peel_leading_count(core, vocab)
        };
        let flt = parse_object_filter(flt_raw, vocab)?;
        return Ok(CostAtomAst::Exile {
            count: Box::new(cnt),
            filter: Box::new(flt),
            face_down,
            face_up,
            single_graveyard,
            any_number,
            min_one,
            total_mv,
        });
    }

    if let Some(rest) = low.strip_prefix("remove ")
        && let Some((left, subj_raw)) =
            split_once_any(&rest, &[" counters from ", " counter from "])
    {
        if let Some(among_raw) = subj_raw.strip_prefix("among ") {
            let flt = parse_object_filter(among_raw.trim(), vocab)?;
            if let Some(kind) = left.strip_prefix("all ") {
                return Ok(CostAtomAst::RemoveCountersFromAmong {
                    all: true,
                    min_one: false,
                    count: None,
                    counter_kind: kind.trim().to_string(),
                    filter: Box::new(flt),
                });
            }
            if let Some(kind) = left.strip_prefix("one or more ") {
                return Ok(CostAtomAst::RemoveCountersFromAmong {
                    all: false,
                    min_one: true,
                    count: None,
                    counter_kind: kind.trim().to_string(),
                    filter: Box::new(flt),
                });
            }
            let (cnt, kind) = peel_leading_count(left, vocab);
            return Ok(CostAtomAst::RemoveCountersFromAmong {
                all: false,
                min_one: false,
                count: Some(Box::new(cnt)),
                counter_kind: if kind.is_empty() {
                    "any".to_string()
                } else {
                    kind.to_string()
                },
                filter: Box::new(flt),
            });
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
        && let Some((left, subj_raw)) = split_once_any(&rest, &[" counters on ", " counter on "])
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
        let (core, any_number) = match rest.strip_prefix("any number of ") {
            Some(r) => (r, true),
            None => (rest, false),
        };
        let (cnt, flt_raw) = if any_number {
            (ValueExprAst::Const(0), core)
        } else {
            peel_leading_count(core, vocab)
        };
        let flt = parse_object_filter(flt_raw, vocab)?;
        return Ok(CostAtomAst::TapPermanent {
            count: Box::new(cnt),
            filter: Box::new(flt),
            any_number,
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
        && let Some(subj_raw) = strip_suffix_any(
            &rest,
            &[" into its owner's graveyard", " into your graveyard"],
        )
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
        && let Some(cnt_raw) = strip_suffix_any(&rest, &[" cards", " card"])
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

pub(crate) fn peel_leading_count<'a>(s: &'a str, vocab: &Vocabulary) -> (ValueExprAst, &'a str) {
    let s = strip_prefix_any(
        s,
        &["another ", "an additional ", "a number of ", "number of "],
    )
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
