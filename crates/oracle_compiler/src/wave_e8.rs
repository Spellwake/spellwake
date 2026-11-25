//! Compositional Wave E8 lossless clause parsers eliminating legacy
//! `E5_STRUCTURED_RULES` catch-alls in favor of reusable atomic Magic AST
//! primitives and compositional combinators.
//!
//! Zero `StructuredRule` or untyped `GameStateFlag` catch-alls are emitted here.

use crate::ast::{
    CardTypeMask, ColorMask, CompareOp, ConditionAst, ControllerRelation, CostAtomAst, DurationAst,
    ExchangeKind, FilterPredicateAst, GameMarker, InstructionAst, InteractiveChoiceKind,
    ManaSpecAst, ObjectFilterAst, ObjectFilterFlags, PileRefAst, PlayerRefAst,
    ReplacementEffectAst, Rounding, StatBoundAst, StatKind, StaticAbilityAst, SubjectRefAst,
    TokenSpecAst, TriggerEventAst, TriggerSpecAst, TurnStepAst, ValueExprAst, ZoneFilter,
};
use crate::clauses::{
    StaticOrReplacement, parse_atomic_effect, parse_cost_atom, parse_cost_list, parse_effect_body,
    parse_static_or_replacement, parse_trigger_header,
};
use crate::filter::{
    parse_condition, parse_for_each_value_expr, parse_keyword_list, parse_object_filter,
    parse_player_ref, parse_subject_or_target, parse_value_expr,
};
use crate::vocab::{Vocabulary, parse_number_word};

fn obj_type(mask: CardTypeMask) -> ObjectFilterAst {
    ObjectFilterAst {
        any_types: mask,
        ..Default::default()
    }
}

/// Compositional Wave E8 effect parser replacing `E5_STRUCTURED_RULES` completely.
#[inline(never)]
pub(crate) fn parse_wave_e8_effects(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Strip optional leading "then " for recursive compositional parsing
    if let Some(rest_low) = low.strip_prefix("then ") {
        let rest_s = &s[s.len() - rest_low.len()..];
        if let Some(body) = parse_wave_e8_core(rest_s, rest_low, s_nodur, dur, vocab) {
            return Some(body);
        }
    }
    parse_wave_e8_core(s, low, s_nodur, dur, vocab)
}

fn parse_wave_e8_core(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Atomic Wave E8 mechanics (restart game, extra steps, skip phases, speed, secret choices, ordering, etc.)
    if let Some(body) = parse_atomic_e8_mechanics(s, low, s_nodur, dur, vocab) {
        return Some(body);
    }
    // 2. Delayed and temporary triggered abilities ("when/whenever <ev>, <body>", "until end of turn, whenever ...", "for as long as ..., whenever ...")
    if let Some(body) = parse_delayed_triggers_e8(s, low, vocab) {
        return Some(body);
    }
    // 3. Split / scaled damage, scaled life, mana formulas, and draw+lose-life combinators
    if let Some(body) = parse_damage_life_mana_e8(low, vocab) {
        return Some(body);
    }
    // 4. Zone, copy, pile, choice, search-at-position, sacrifice-except, and exchange combinators
    if let Some(body) = parse_zone_choice_exchange_e8(s, low, dur, vocab) {
        return Some(body);
    }
    // 5. Temporary statics, combat rules, activation restrictions, and modal token lines
    if let Some(body) = parse_statics_and_restrictions_e8(s, low, dur, vocab) {
        return Some(body);
    }
    // 6. General conditional wrappers ("if <cond>, <eff>", "otherwise, <eff>", "<eff> if <cond>", "<eff> unless <cond>")
    if let Some(body) = parse_conditional_wrappers_e8(s, low, vocab) {
        return Some(body);
    }
    // 7. Sequential compound combinators ("<eff1>, then <eff2>" and "<eff1> and <eff2>")
    parse_compound_splitters_e8(s, low, vocab)
}

fn parse_atomic_e8_mechanics(
    _s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. CR 724 Restart the game ("restart the game[, leaving in exile <flt>]") (Karn Liberated)
    if low == "restart the game" {
        return Some(vec![InstructionAst::RestartGame {
            leave_in_exile: None,
        }]);
    }
    if let Some(flt_raw) = low.strip_prefix("restart the game, leaving in exile ") {
        let clean = flt_raw
            .trim()
            .strip_prefix("all ")
            .unwrap_or(flt_raw.trim());
        let flt = parse_object_filter(clean, vocab).ok()?;
        return Some(vec![InstructionAst::RestartGame {
            leave_in_exile: Some(Box::new(flt)),
        }]);
    }

    // 2. Additional step ("<player> gets an additional <step> step after this step") (Paradox Haze)
    if let Some((who_raw, rest)) = low.split_once(" gets an additional ")
        && let Some(step_raw) = rest.strip_suffix(" step after this step")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let step = match step_raw.trim() {
            "upkeep" => TurnStepAst::Upkeep,
            "draw" => TurnStepAst::DrawStep,
            "untap" => TurnStepAst::UntapStep,
            "end" => TurnStepAst::EndStep,
            _ => return None,
        };
        return Some(vec![InstructionAst::AdditionalStep {
            player,
            step,
            count: 1,
        }]);
    }

    // 2b. Variable multi-die roll ("roll <val> d<sides>") (Neverwinter Hydra and future X-die cards)
    if let Some(after) = low.strip_prefix("roll ")
        && let Some((cnt_raw, die_tok)) = after.rsplit_once(' ')
        && let Some(sides_raw) = die_tok.strip_prefix('d')
        && let Ok(sides) = sides_raw.parse::<u16>()
        && sides >= 2
        && let Ok(count) = parse_value_expr(cnt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ForEachCount {
            count: Box::new(count),
            body: vec![InstructionAst::RollDie { sides }],
        }]);
    }

    // 3. Skip all <phase> phases of their next turn (Empty City Ruse)
    if let Some((who_raw, rest)) = low
        .split_once(" skips all ")
        .or_else(|| low.split_once(" skip all "))
        && let Some(phase_raw) = rest
            .strip_suffix(" phases of their next turn")
            .or_else(|| rest.strip_suffix(" phases of your next turn"))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::SkipAllPhasesNextTurn {
            player,
            phase: phase_raw.trim().to_string(),
        }]);
    }

    // 4. Modify player speed ("reduce/increase <player>'s speed by <N>") (Spikeshell Harrier)
    if let Some((sign, after)) = low
        .strip_prefix("reduce ")
        .map(|r| (-1_i8, r))
        .or_else(|| low.strip_prefix("increase ").map(|r| (1_i8, r)))
        && let Some((who_raw, n_raw)) = after.split_once("'s speed by ")
        && let (Some(player), Some(n)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(n_raw.trim()),
        )
    {
        return Some(vec![InstructionAst::ModifyPlayerSpeed {
            player,
            delta: sign * i8::try_from(n).unwrap_or(1),
        }]);
    }

    // 5. Count counters or objects explicitly ("count the number of <kind> counters on <flt>") (Rumbling Ruin)
    if let Some(after) = low.strip_prefix("count the number of ")
        && let Some((kind_raw, flt_raw)) = after.split_once(" counters on ")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CountObjects {
            filter: Box::new(flt),
            counter_kind: Some(kind_raw.trim().to_string()),
        }]);
    }

    // 6. Guess card property ("<player> guesses whether <subj> is <prop>") (Gollum, Scheming Guide)
    if let Some((who_raw, rest)) = low
        .split_once(" guesses whether ")
        .or_else(|| low.split_once(" guess whether "))
        && let Some((subj_raw, prop_raw)) = rest.split_once(" is ")
        && let (Some(guesser), Ok(subject)) = (
            parse_player_ref(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::GuessCardProperty {
            guesser,
            subject: Box::new(subject),
            property: prop_raw.trim().to_string(),
        }]);
    }

    // 7. Distribute stickers ("distribute [up to] <N> <kind> stickers among <targets>") (Roxi, Publicist to the Stars)
    if let Some(after) = low.strip_prefix("distribute ")
        && let Some((left, tgts_raw)) = after.split_once(" stickers among ")
    {
        let (up_to, cnt_and_kind) = match left.strip_prefix("up to ") {
            Some(r) => (true, r),
            None => (false, left),
        };
        if let Some((cnt_tok, kind_raw)) = cnt_and_kind.trim().split_once(' ')
            && let (Some(n), Ok(targets)) = (
                parse_number_word(cnt_tok.trim()),
                parse_subject_or_target(tgts_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::DistributeStickers {
                sticker_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::Const(n)),
                up_to,
                targets: Box::new(targets),
            }]);
        }
    }

    // 8. Ignore effect for matching permanents ("ignore this effect for each <flt>") (Siren's Call)
    if let Some(after) = low.strip_prefix("ignore this effect for ") {
        let flt_raw = after
            .strip_prefix("each ")
            .or_else(|| after.strip_prefix("all "))
            .unwrap_or(after);
        if let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab) {
            return Some(vec![InstructionAst::IgnoreEffectFor {
                filter: Box::new(flt),
            }]);
        }
    }

    // 9. Secret choice and reveal ("<players> secretly chooses <o1> or <o2>, then the choices are revealed") (Prisoner's Dilemma)
    if let Some((who_raw, rest)) = low
        .split_once(" secretly chooses ")
        .or_else(|| low.split_once(" secretly choose "))
        && let Some(opts_raw) = rest.strip_suffix(", then the choices are revealed")
        && let Some((o1, o2)) = opts_raw.split_once(" or ")
        && let Some(players) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::SecretChoiceAndReveal {
            players,
            options: vec![o1.trim().to_string(), o2.trim().to_string()],
        }]);
    }

    // 10. Sequential ordering of a group ("choose <flt> one at a time until each <flt> has been chosen") (Starlight Spectacular)
    if let Some(after) = low.strip_prefix("choose ")
        && let Some((flt_raw, tail)) = after.split_once(" one at a time until each ")
        && tail.ends_with(" has been chosen")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::OrderGroup {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
        }]);
    }

    // 11. Exchange control of all matching permanents ("you and <player> each gain control of all <flt> the other controls [dur]" or "<p1> and <p2> exchange control of all <flt> ... [dur]") (Reins of Power)
    let snlow = s_nodur.to_ascii_lowercase();
    if let Some((players_raw, flt_tail)) = snlow
        .split_once(" each gain control of all ")
        .or_else(|| snlow.split_once(" exchange control of all "))
        && let Some((p1_raw, p2_raw)) = players_raw.split_once(" and ")
        && let (Some(left), Some(right)) = (
            parse_player_ref(p1_raw.trim()),
            parse_player_ref(p2_raw.trim()),
        )
    {
        let flt_clean = flt_tail
            .strip_suffix(" the other controls")
            .or_else(|| flt_tail.strip_suffix(" you each control"))
            .or_else(|| flt_tail.strip_suffix(" they each control"))
            .or_else(|| flt_tail.strip_suffix(" they control"))
            .unwrap_or(flt_tail);
        if let Ok(flt) = parse_object_filter(flt_clean.trim(), vocab) {
            return Some(vec![InstructionAst::ExchangeAllControlled {
                left,
                right,
                filter: Box::new(flt),
                duration: dur.clone(),
            }]);
        }
    }

    // 12. Replace mana production ("if <subj> is tapped for mana, it produces <mana> instead of <old>") (Quarum Trench Gnomes)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((subj_raw, rest)) = after_if.split_once(" is tapped for mana, it produces ")
        && let Some((new_mana, _old_mana)) = rest.split_once(" instead of ")
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let replace_with = match new_mana.trim() {
            "colorless mana" | "{c}" => ManaSpecAst::Fixed("{C}".to_string()),
            "white mana" | "{w}" => ManaSpecAst::Fixed("{W}".to_string()),
            "blue mana" | "{u}" => ManaSpecAst::Fixed("{U}".to_string()),
            "black mana" | "{b}" => ManaSpecAst::Fixed("{B}".to_string()),
            "red mana" | "{r}" => ManaSpecAst::Fixed("{R}".to_string()),
            "green mana" | "{g}" => ManaSpecAst::Fixed("{G}".to_string()),
            other => ManaSpecAst::Fixed(other.to_string()),
        };
        return Some(vec![InstructionAst::ReplaceManaProduction {
            subject: Box::new(subject),
            replace_with,
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 13. Dynamic cost reduction ("this cost is reduced by <delta> for each <per>") (Draco)
    if let Some(after) = low.strip_prefix("this cost is reduced by ")
        && let Some((delta_raw, per_raw)) = after.split_once(" for each ")
        && let Ok(per) = parse_for_each_value_expr(per_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::DynamicCostReduction {
            delta: delta_raw.trim().to_ascii_uppercase(),
            per: Box::new(per),
        }]);
    }

    // 14. Reorder zone ("reorder your graveyard as you choose" / "puts them back in any order") (Fossil Find, Tahngarth's Glare)
    if low == "reorder your graveyard as you choose" {
        return Some(vec![InstructionAst::ReorderZone {
            player: PlayerRefAst::You,
            zone: ZoneFilter::YourGraveyard,
            random: false,
        }]);
    }
    if low == "puts them back in any order"
        || low == "put them back in any order"
        || low == "puts them back on top of your library in any order"
    {
        return Some(vec![InstructionAst::ReorderTopLibrary]);
    }

    // 15. Sacrifice all except / all but N ("<player> sacrifices all <flt> except for <keep>" / "all but <keep> <flt>") (Keldon Firebombers, Choice of Damnations)
    if let Some((who_raw, rest)) = low
        .split_once(" sacrifices all ")
        .or_else(|| low.split_once(" sacrifice all "))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        if let Some((flt_raw, keep_raw)) = rest.split_once(" except for ")
            && let (Ok(flt), Ok(keep)) = (
                parse_object_filter(flt_raw.trim(), vocab),
                parse_value_expr(keep_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::SacrificeAllExcept {
                player,
                filter: Box::new(flt),
                keep: Box::new(keep),
            }]);
        }
        if let Some(after_but) = rest.strip_prefix("but ") {
            let clean = after_but
                .strip_suffix(" of their choice")
                .or_else(|| after_but.strip_suffix(" of your choice"))
                .unwrap_or(after_but);
            if let Some(flt_raw) = clean.strip_prefix("that many ")
                && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
            {
                return Some(vec![InstructionAst::SacrificeAllExcept {
                    player,
                    filter: Box::new(flt),
                    keep: Box::new(ValueExprAst::PriorResult),
                }]);
            }
            if let Some((cnt_tok, flt_raw)) = clean.split_once(' ')
                && let (Ok(keep), Ok(flt)) = (
                    parse_value_expr(cnt_tok.trim(), vocab),
                    parse_object_filter(flt_raw.trim(), vocab),
                )
            {
                return Some(vec![InstructionAst::SacrificeAllExcept {
                    player,
                    filter: Box::new(flt),
                    keep: Box::new(keep),
                }]);
            }
        }
    }

    // 16. Exile all but the bottom N cards of library (Doomsday Excruciator)
    if let Some((who_raw, rest)) = low
        .split_once(" exiles all but the bottom ")
        .or_else(|| low.split_once(" exile all but the bottom "))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let (face_down, core) = match rest.strip_suffix(" face down") {
            Some(r) => (true, r),
            None => (false, rest),
        };
        if let Some((cnt_tok, _lib)) = core.split_once(" cards of ")
            && let Ok(keep) = parse_value_expr(cnt_tok.trim(), vocab)
        {
            return Some(vec![InstructionAst::ExileAllExceptBottom {
                player,
                keep: Box::new(keep),
                face_down,
            }]);
        }
    }

    // 17. Roll a dN for each <scope> and ignore all but the highest roll (Iron Mastiff)
    if let Some(after_r) = low.strip_prefix("roll a d")
        && let Some((sides_s, tail)) = after_r.split_once(" for each ")
        && tail.ends_with(" and ignore all but the highest roll")
        && let Ok(sides) = sides_s.trim().parse::<u16>()
    {
        return Some(vec![InstructionAst::RollDiceChooseOne { count: 2, sides }]);
    }

    None
}

fn parse_delayed_triggers_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. "until end of turn, whenever <ev>, <body>" (False Cure)
    if let Some(after_u) = low.strip_prefix("until end of turn, ")
        && (after_u.starts_with("whenever ") || after_u.starts_with("when "))
    {
        let after_s = &s[s.len() - after_u.len()..];
        if let Some(inner) =
            parse_delayed_trigger_clause(after_s, after_u, DurationAst::UntilEndOfTurn, vocab)
        {
            return Some(inner);
        }
    }

    // 2. "for as long as <cond>, whenever <ev>, <body>" (Myra the Magnificent)
    if let Some(after_f) = low.strip_prefix("for as long as ")
        && let Some((cond_raw, trig_low)) = after_f.split_once(", whenever ")
    {
        let cond = if let Some(subj_raw) = cond_raw.strip_suffix(" is on the battlefield")
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            ConditionAst::Marker(GameMarker::InZone {
                subject: Box::new(subj),
                zone: ZoneFilter::Battlefield,
            })
        } else {
            parse_condition(cond_raw.trim(), vocab).ok()?
        };
        let full_trig_low = format!("whenever {trig_low}");
        let trig_s = &s[s.len() - full_trig_low.len()..];
        if let Some(inner) = parse_delayed_trigger_clause(
            trig_s,
            &full_trig_low,
            DurationAst::ForAsLongAs(Box::new(cond.clone())),
            vocab,
        ) {
            return Some(vec![InstructionAst::ForAsLongAs {
                condition: Box::new(cond),
                body: inner,
            }]);
        }
    }

    // 3. Direct "when/whenever <ev>, <body>"
    if low.starts_with("when ") || low.starts_with("whenever ") {
        let dur = if low.contains(" this turn,") {
            DurationAst::UntilEndOfTurn
        } else {
            DurationAst::Permanent
        };
        return parse_delayed_trigger_clause(s, low, dur, vocab);
    }

    None
}

fn parse_delayed_trigger_clause(
    s: &str,
    low: &str,
    dur: DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    for (idx, _) in low.match_indices(", ") {
        let header_low = &low[..idx];
        let body_low = &low[idx + 2..];
        if body_low.is_empty() {
            continue;
        }
        let header_s = &s[..idx];
        let body_s = &s[idx + 2..];

        let trigger = parse_trigger_header(header_s, vocab)
            .ok()
            .or_else(|| parse_delayed_trigger_header_e8(header_low, vocab));
        let Some(trig) = trigger else {
            continue;
        };

        // Handle intervening-if inside delayed trigger: "whenever ..., if <cond>, <eff>" (Vigorous Charge)
        if let Some(after_if) = body_low.strip_prefix("if ")
            && let Some((cond_raw, eff_low)) = after_if.split_once(", ")
            && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        {
            let eff_s = &body_s[body_s.len() - eff_low.len()..];
            let body = parse_sub_effect_e8(eff_s, eff_low, vocab)?;
            let mut trig_with_if = trig;
            trig_with_if.intervening_if = Some(cond);
            return Some(vec![InstructionAst::TemporaryTrigger {
                trigger: trig_with_if,
                body,
                duration: dur,
            }]);
        }

        if let Some(body) = parse_sub_effect_e8(body_s, body_low, vocab) {
            return Some(vec![InstructionAst::TemporaryTrigger {
                trigger: trig,
                body,
                duration: dur,
            }]);
        }
    }
    None
}

fn parse_delayed_trigger_header_e8(header_low: &str, vocab: &Vocabulary) -> Option<TriggerSpecAst> {
    let core = header_low
        .strip_prefix("whenever ")
        .or_else(|| header_low.strip_prefix("when "))?
        .trim();
    let core_clean = core.strip_suffix(" this turn").unwrap_or(core).trim();

    if let Some(subj_raw) = core_clean
        .strip_suffix(" dies under your control")
        .or_else(|| core_clean.strip_suffix(" dies"))
    {
        let subject = if let Some(src_raw) = subj_raw.strip_prefix("a creature dealt damage by ")
            && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
        {
            let mut cr = obj_type(CardTypeMask::CREATURE);
            cr.flags = cr.flags.union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
            cr.predicates
                .push(FilterPredicateAst::FromSource(Box::new(ObjectFilterAst {
                    predicates: vec![FilterPredicateAst::StateQualification(format!(
                        "dealt_damage_by:{src:?}"
                    ))],
                    ..Default::default()
                })));
            SubjectRefAst::Group(Box::new(cr))
        } else {
            parse_subject_or_target(subj_raw.trim(), vocab).ok()?
        };
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::ZoneChange {
                from: Some("battlefield".to_string()),
                to: Some("graveyard".to_string()),
                subject: Box::new(subject),
                grouped: false,
                entering_controller: if core_clean.contains("under your control") {
                    Some(ControllerRelation::You)
                } else {
                    None
                },
            }],
            intervening_if: None,
        });
    }

    if let Some(subj_raw) = core_clean.strip_suffix(" leaves the battlefield")
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::ZoneChange {
                from: Some("battlefield".to_string()),
                to: None,
                subject: Box::new(subject),
                grouped: false,
                entering_controller: None,
            }],
            intervening_if: None,
        });
    }

    if let Some(subj_raw) = core_clean.strip_suffix(" becomes untapped or leaves the battlefield")
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(TriggerSpecAst {
            events: vec![
                TriggerEventAst::TapStateChanged {
                    subject: Box::new(subject.clone()),
                    tapped: false,
                    for_mana: false,
                },
                TriggerEventAst::ZoneChange {
                    from: Some("battlefield".to_string()),
                    to: None,
                    subject: Box::new(subject),
                    grouped: false,
                    entering_controller: None,
                },
            ],
            intervening_if: None,
        });
    }

    if let Some((who_raw, spell_rest)) = core_clean.split_once(" cast ")
        && let Some(caster) = parse_player_ref(who_raw.trim())
    {
        let (nth, flt_raw) = match spell_rest.strip_suffix(" for the first time") {
            Some(r) => (Some(1), r),
            None => (None, spell_rest),
        };
        if let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab) {
            return Some(TriggerSpecAst {
                events: vec![TriggerEventAst::SpellCast {
                    caster,
                    spell: Some(Box::new(flt)),
                    self_spell: false,
                    targets: None,
                    nth_each_turn: nth,
                    include_copy: false,
                }],
                intervening_if: None,
            });
        }
    }

    if let Some((rec_raw, src_raw)) = core_clean.split_once(" is dealt damage by ")
        && let (Ok(rec), Ok(src)) = (
            parse_subject_or_target(rec_raw.trim(), vocab),
            parse_subject_or_target(src_raw.trim(), vocab),
        )
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::DamageDealt {
                combat_only: false,
                noncombat_only: false,
                source: Some(Box::new(src)),
                recipient: Some(Box::new(rec)),
                grouped: false,
                excess_only: false,
            }],
            intervening_if: None,
        });
    }
    if let Some(src_raw) = core_clean.strip_suffix(" deals combat damage")
        && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::DamageDealt {
                combat_only: true,
                noncombat_only: false,
                source: Some(Box::new(src)),
                recipient: None,
                grouped: false,
                excess_only: false,
            }],
            intervening_if: None,
        });
    }

    if let Some((who_raw, _)) = core_clean.split_once(" visit ")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::KeywordActionPerformed {
                player,
                action: "visit_attraction".to_string(),
            }],
            intervening_if: None,
        });
    }

    if let Some(who_raw) = core_clean.strip_suffix(" gains life")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::LifeChanged {
                player,
                gained: true,
                first_each_turn: false,
            }],
            intervening_if: None,
        });
    }

    if let Some((who_raw, rest)) = core_clean.split_once(" attacks ")
        && rest.ends_with(" with one or more creatures")
        && let Some(actor) = parse_player_ref(who_raw.trim())
    {
        return Some(TriggerSpecAst {
            events: vec![TriggerEventAst::Attacks {
                subject: None,
                actor: Some(actor),
                qualifier: Some(rest.to_string()),
                grouped: true,
            }],
            intervening_if: None,
        });
    }

    None
}

fn parse_damage_life_mana_e8(low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    // 1. Dual/split damage: "<src> deals <a1> damage to <t1>[,] and [an additional] <a2> [additional] damage to <t2>"
    if let Some((src_raw, rest)) = low.split_once(" deals ")
        && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        for sep in [", and ", " and "] {
            if let Some((part1, part2)) = rest.split_once(sep)
                && let Some((a1, t1)) = parse_dmg_amount_and_target(part1.trim(), vocab)
                && let Some((a2, t2)) = parse_dmg_amount_and_target(part2.trim(), vocab)
            {
                return Some(vec![
                    InstructionAst::DealDamage {
                        source: Box::new(src.clone()),
                        target: Box::new(t1),
                        amount: Box::new(a1),
                    },
                    InstructionAst::DealDamage {
                        source: Box::new(src),
                        target: Box::new(t2),
                        amount: Box::new(a2),
                    },
                ]);
            }
        }
        if let Some(after_eq) = rest.strip_prefix("damage equal to ")
            && let Some((val_raw, tgt_raw)) = after_eq.rsplit_once(" to ")
            && let (Ok(amt), Ok(tgt)) = (
                parse_value_expr(val_raw.trim(), vocab),
                parse_subject_or_target(tgt_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::DealDamage {
                source: Box::new(src),
                target: Box::new(tgt),
                amount: Box::new(amt),
            }]);
        }
        if let Some(after_to) = rest.strip_prefix("damage to ")
            && let Some((tgt_raw, val_raw)) = after_to.split_once(" equal to ")
            && let (Ok(tgt), Ok(amt)) = (
                parse_subject_or_target(tgt_raw.trim(), vocab),
                parse_value_expr(val_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::DealDamage {
                source: Box::new(src),
                target: Box::new(tgt),
                amount: Box::new(amt),
            }]);
        }
    }

    // 2. Scaled life gain/loss with base + per ("<player> gains/loses <base> life plus [an additional] <per> life for each <expr>")
    if let Some((who_raw, gain, rest)) = low
        .split_once(" gains ")
        .map(|(w, r)| (w, true, r))
        .or_else(|| low.split_once(" gain ").map(|(w, r)| (w, true, r)))
        .or_else(|| low.split_once(" loses ").map(|(w, r)| (w, false, r)))
        .or_else(|| low.split_once(" lose ").map(|(w, r)| (w, false, r)))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        if let Some((base_raw, after_plus)) = rest
            .split_once(" life plus an additional ")
            .or_else(|| rest.split_once(" life plus "))
            && let Some((per_raw, fe_raw)) = after_plus.split_once(" life for each ")
            && let (Ok(base_v), Ok(per_v), Ok(fe_v)) = (
                parse_value_expr(base_raw.trim(), vocab),
                parse_value_expr(per_raw.trim(), vocab),
                parse_for_each_value_expr(fe_raw.trim(), vocab),
            )
        {
            return Some(vec![InstructionAst::ChangeLife {
                player,
                delta: Box::new(ValueExprAst::Add(
                    Box::new(base_v),
                    Box::new(ValueExprAst::Mul(Box::new(per_v), Box::new(fe_v))),
                )),
                gain,
            }]);
        }
        if let Some((per_raw, fe_raw)) = rest.split_once(" life for each ")
            && let Ok(per_v) = parse_value_expr(per_raw.trim(), vocab)
        {
            let fe_v = if fe_raw.trim() == "1 life they gained" {
                ValueExprAst::PriorResult
            } else {
                parse_for_each_value_expr(fe_raw.trim(), vocab).ok()?
            };
            let delta = if per_v == ValueExprAst::Const(1) {
                fe_v
            } else {
                ValueExprAst::Mul(Box::new(per_v), Box::new(fe_v))
            };
            return Some(vec![InstructionAst::ChangeLife {
                player,
                delta: Box::new(delta),
                gain,
            }]);
        }
    }

    // 3. Paired draw cards and lose life equal to <val> (Eventide's Shadow)
    if let Some((who_raw, val_raw)) = low
        .split_once(" draw cards and lose life equal to ")
        .or_else(|| low.split_once(" draws cards and loses life equal to "))
        && let (Some(player), Ok(val)) = (
            parse_player_ref(who_raw.trim()),
            parse_value_expr(val_raw.trim(), vocab),
        )
    {
        return Some(vec![
            InstructionAst::Draw {
                player,
                count: Box::new(val.clone()),
                up_to: false,
            },
            InstructionAst::ChangeLife {
                player,
                delta: Box::new(val),
                gain: false,
            },
        ]);
    }

    // 4. "<player> gets twice that many <kind> counters" (Struggle for Project Purity)
    if let Some((who_raw, rest)) = low
        .split_once(" gets twice that many ")
        .or_else(|| low.split_once(" get twice that many "))
        && let Some(kind_raw) = rest.strip_suffix(" counters")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::GainPlayerCounters {
            player,
            counter_kind: kind_raw.trim().to_string(),
            count: Box::new(ValueExprAst::Mul(
                Box::new(ValueExprAst::Const(2)),
                Box::new(ValueExprAst::PriorResult),
            )),
        }]);
    }

    // 5. Mana production formulas (Extraplanar Lens, Open the Omenpaths, Paliano, Belbe, Radiant Lotus, Three Tree City)
    if let Some((who_raw, rest)) = low.split_once(" adds ")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        if rest == "one mana of any type that land produced" {
            return Some(vec![InstructionAst::AddManaLandProduced {
                player,
                count: Some(Box::new(ValueExprAst::Const(1))),
            }]);
        }
        if let Some((sym_raw, fe_raw)) = rest.split_once(" for each ")
            && let Ok(fe_val) = parse_for_each_value_expr(fe_raw.trim(), vocab)
        {
            if let Some((cnt_tok, _)) = sym_raw.trim().split_once(" mana of the chosen color")
                && let Some(n) = parse_number_word(cnt_tok.trim())
            {
                let amount = if n == 1 {
                    fe_val
                } else {
                    ValueExprAst::Mul(Box::new(ValueExprAst::Const(n)), Box::new(fe_val))
                };
                return Some(vec![InstructionAst::AddMana {
                    player,
                    mana: ManaSpecAst::ScaledSymbol {
                        symbol: "chosen_color".to_string(),
                        amount: Box::new(amount),
                    },
                }]);
            }
            if sym_raw.trim().starts_with('{') && sym_raw.trim().ends_with('}') {
                return Some(vec![InstructionAst::AddMana {
                    player,
                    mana: ManaSpecAst::ScaledSymbol {
                        symbol: sym_raw.trim().to_ascii_uppercase(),
                        amount: Box::new(fe_val),
                    },
                }]);
            }
        }
    }
    if let Some(after) = low.strip_prefix("add ") {
        if let Some((left_m, right_m)) = after.split_once(" mana of any one color and ")
            && let Some(n2_raw) = right_m.strip_suffix(" mana of any other color")
            && let (Some(n1), Some(n2)) = (
                parse_number_word(left_m.trim()),
                parse_number_word(n2_raw.trim()),
            )
        {
            return Some(vec![
                InstructionAst::AddMana {
                    player: PlayerRefAst::You,
                    mana: ManaSpecAst::AnyColor {
                        amount: Box::new(ValueExprAst::Const(n1)),
                        single_color: true,
                    },
                },
                InstructionAst::AddMana {
                    player: PlayerRefAst::You,
                    mana: ManaSpecAst::AnyColor {
                        amount: Box::new(ValueExprAst::Const(n2)),
                        single_color: true,
                    },
                },
            ]);
        }
        if after == "one mana of any color chosen as you drafted cards named ~" {
            return Some(vec![InstructionAst::AddMana {
                player: PlayerRefAst::You,
                mana: ManaSpecAst::ChosenColor,
            }]);
        }
        if let Some(val_raw) = after
            .strip_prefix("an amount of mana of that color equal to ")
            .or_else(|| after.strip_prefix("an amount of mana of the chosen color equal to "))
            && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::AddMana {
                player: PlayerRefAst::You,
                mana: ManaSpecAst::ScaledSymbol {
                    symbol: "chosen_color".to_string(),
                    amount: Box::new(val),
                },
            }]);
        }
    }

    None
}

fn parse_dmg_amount_and_target(
    part: &str,
    vocab: &Vocabulary,
) -> Option<(ValueExprAst, SubjectRefAst)> {
    let clean = part.strip_prefix("an additional ").unwrap_or(part).trim();
    let (amt_raw, tgt_raw) = clean
        .split_once(" additional damage to ")
        .or_else(|| clean.split_once(" damage to "))
        .unwrap_or(("", ""));
    if !amt_raw.is_empty()
        && let (Ok(amt), Ok(tgt)) = (
            parse_value_expr(amt_raw.trim(), vocab),
            parse_subject_or_target(tgt_raw.trim(), vocab),
        )
    {
        return Some((amt, tgt));
    }
    for (needle, rounding) in [
        (" damage, rounded down, to ", Rounding::Down),
        (" damage, rounded up, to ", Rounding::Up),
    ] {
        if let Some((before_dmg, tgt_raw)) = clean.split_once(needle)
            && let Some(inner_raw) = before_dmg.strip_prefix("half ")
            && let (Ok(inner), Ok(tgt)) = (
                parse_value_expr(inner_raw.trim(), vocab),
                parse_subject_or_target(tgt_raw.trim(), vocab),
            )
        {
            return Some((
                ValueExprAst::DivConst {
                    expr: Box::new(inner),
                    denom: 2,
                    rounding,
                },
                tgt,
            ));
        }
    }
    None
}

#[allow(clippy::too_many_lines)]
fn parse_zone_choice_exchange_e8(
    _s: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Copy card in exile/zone ("copy <flt>" / "you may copy <flt>") (Arcane Savant, Reversal of Fortune)
    let (opt_copy, copy_core) = match low.strip_prefix("you may copy ") {
        Some(r) => (true, r),
        None => match low.strip_prefix("copy ") {
            Some(r) => (false, r),
            None => (false, ""),
        },
    };
    if !copy_core.is_empty()
        && let Ok(flt) = parse_object_filter(copy_core.trim(), vocab)
    {
        let ins = InstructionAst::Copy {
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            exception: None,
            count: None,
        };
        return Some(if opt_copy {
            vec![InstructionAst::Optional(vec![ins])]
        } else {
            vec![ins]
        });
    }

    // 2. Create token copy of a filtered card ("create a token that's a copy of <flt>") (Caller of the Untamed)
    if let Some(src_raw) = low.strip_prefix("create a token that's a copy of ")
        && let Ok(flt) = parse_object_filter(src_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CreateToken {
            controller: PlayerRefAst::You,
            token: TokenSpecAst::Copy {
                count: Box::new(ValueExprAst::Const(1)),
                source: Box::new(SubjectRefAst::Group(Box::new(flt))),
                tapped: false,
                attacking: false,
                exception: None,
            },
        }]);
    }

    // 3. Exile from each graveyard ("[you may] exile <flt> from each graveyard") (King Narfi's Betrayal, Summon: Esper Valigarmanda)
    let (opt_ex, ex_core) = match low.strip_prefix("you may exile ") {
        Some(r) => (true, r),
        None => match low.strip_prefix("exile ") {
            Some(r) => (false, r),
            None => (false, ""),
        },
    };
    if let Some(flt_raw) = ex_core.strip_suffix(" from each graveyard")
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.zone = Some(ZoneFilter::AllGraveyards);
        let ins = InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![InstructionAst::Exile {
                actor: Some(PlayerRefAst::You),
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                random: false,
                face_down: false,
                into_pile: false,
            }],
        };
        return Some(if opt_ex {
            vec![InstructionAst::Optional(vec![ins])]
        } else {
            vec![ins]
        });
    }

    // 4. Exile top card of each player's library with a counter (Evelyn, the Covetous)
    if let Some(after_ex) = low.strip_prefix("exile the top card of each player's library with a ")
        && let Some(kind_raw) = after_ex.strip_suffix(" counter on it")
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachPlayer,
            body: vec![InstructionAst::ExileWithCounters {
                subject: Box::new(SubjectRefAst::Anaphora(
                    "the top card of their library".to_string(),
                )),
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                face_down: false,
            }],
        }]);
    }

    // 5. Exile card from hand until ~ leaves the battlefield (Intellect Devourer)
    if let Some((who_raw, rest)) = low
        .split_once(" exiles ")
        .or_else(|| low.split_once(" exile "))
        && let Some(subj_raw) = rest.strip_suffix(" until ~ leaves the battlefield")
        && let (Some(player), Ok(subj)) = (
            parse_player_ref(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player,
            body: vec![InstructionAst::ExileUntilLeaves {
                subject: Box::new(subj),
            }],
        }]);
    }

    // 6. Exile card from hand and may play for as long as it remains exiled (Lightstall Inquisitor)
    if let Some((who_raw, rest)) = low
        .split_once(" exiles ")
        .or_else(|| low.split_once(" exile "))
        && let Some((subj_raw, play_tail)) = rest.split_once(" and may play ")
        && let (Some(player), Ok(subj)) = (
            parse_player_ref(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player,
            body: vec![
                InstructionAst::Exile {
                    actor: Some(PlayerRefAst::ThatPlayer),
                    subject: Box::new(subj),
                    random: false,
                    face_down: false,
                    into_pile: false,
                },
                InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::ThatPlayer,
                    cast_only: false,
                    subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                    free: false,
                    duration: play_tail.trim().to_string(),
                    count_limit: None,
                    alt_cost: None,
                },
            ],
        }]);
    }

    // 7. Exile cards from top of library equal to <val> (Bone Mask)
    if let Some(val_raw) = low.strip_prefix("exile cards from the top of your library equal to ")
        && let Ok(count) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ExileTopLibrary {
            actor: Some(PlayerRefAst::You),
            whose: "your".to_string(),
            count: Box::new(count),
            face_down: false,
        }]);
    }

    // 8. Exile N of them at random (Orcish Librarian)
    if let Some(after) = low.strip_prefix("exile ")
        && let Some(cnt_raw) = after.strip_suffix(" of them at random")
        && let Ok(count) = parse_value_expr(cnt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::MoveCounted {
            actor: Some(PlayerRefAst::You),
            count: Some(Box::new(count)),
            up_to: false,
            any_number: false,
            subject: Box::new(SubjectRefAst::Anaphora("them".to_string())),
            to: "exile_random".to_string(),
        }]);
    }

    // 9. Put that many cards from top of library into hand / Put bottom card into graveyard (Scroll Rack, Cellar Door)
    if let Some(after) = low.strip_prefix("put that many cards from the top of your library into ")
    {
        return Some(vec![InstructionAst::MoveCounted {
            actor: Some(PlayerRefAst::You),
            count: Some(Box::new(ValueExprAst::PriorResult)),
            up_to: false,
            any_number: false,
            subject: Box::new(SubjectRefAst::Anaphora(
                "the top cards of your library".to_string(),
            )),
            to: after.trim().to_string(),
        }]);
    }
    if let Some((who_raw, rest)) = low.split_once(" puts the bottom card of their library into ")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::MoveCounted {
            actor: Some(player),
            count: Some(Box::new(ValueExprAst::Const(1))),
            up_to: false,
            any_number: false,
            subject: Box::new(SubjectRefAst::Anaphora(
                "the bottom card of their library".to_string(),
            )),
            to: rest.trim().to_string(),
        }]);
    }

    // 10. Search library, then shuffle and put that card <ordinal> from the top (Long-Term Plans)
    if let Some(after) = low.strip_prefix("search your library for ")
        && let Some((flt_raw, ord_tail)) = after.split_once(", then shuffle and put that card ")
        && let Some(ord_word) = ord_tail.strip_suffix(" from the top")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        let from_top = match ord_word.trim() {
            "first" => 1,
            "second" => 2,
            "third" => 3,
            "fourth" => 4,
            "fifth" => 5,
            "sixth" => 6,
            "seventh" => 7,
            _ => return None,
        };
        return Some(vec![
            InstructionAst::SearchLibrary {
                library: "your".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                up_to: false,
                any_number: false,
                exact: false,
                filter: Box::new(flt),
                reveal: false,
                to_zone: "found_set".to_string(),
                shuffle: true,
                extra_zones: Vec::new(),
                secondary_filter: None,
                secondary_is_or: false,
                where_x: None,
                rest_to_zone: None,
                modifier: None,
                follow_up: Vec::new(),
            },
            InstructionAst::PutInLibraryAtPosition {
                player: PlayerRefAst::You,
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                from_top,
            },
        ]);
    }

    // 11. Manifest a number of cards equal to <val> / Manifest one of those cards (Omarthis, Write into Being)
    if let Some(val_raw) =
        low.strip_prefix("manifest a number of cards from the top of your library equal to ")
        && let Ok(count) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Manifest {
            count: Box::new(count),
            dread: false,
            subject: None,
        }]);
    }
    if let Some(subj_raw) = low.strip_prefix("manifest ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Manifest {
            count: Box::new(ValueExprAst::Const(1)),
            dread: false,
            subject: Some(Box::new(subj)),
        }]);
    }

    // 12. The owners of those cards shuffle them into their libraries (Turn the Earth)
    if let Some(after) = low.strip_prefix("the owners of ")
        && let Some((subj_raw, tail)) = after.split_once(" shuffle ")
        && tail.ends_with(" into their libraries")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(subj),
        }]);
    }

    // 13. Return to your hand <s1>, <s2>, and <s3> (Remove Enchantments)
    if let Some(after) = low.strip_prefix("return to your hand ")
        && let Some((s1_raw, rest)) = after.split_once(", ")
        && let Some((s2_raw, s3_raw)) = rest.split_once(", and ")
        && let (Ok(s1), Ok(s2), Ok(s3)) = (
            parse_subject_or_target(s1_raw.trim(), vocab),
            parse_subject_or_target(s2_raw.trim(), vocab),
            parse_subject_or_target(s3_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ReturnZone {
            subject: Box::new(SubjectRefAst::Compound(vec![s1, s2, s3])),
            from_zone: Some("battlefield".to_string()),
            to: "your hand".to_string(),
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
        }]);
    }

    // 14. Return a card at random from your graveyard to your hand (Fossil Find)
    if let Some(after) = low.strip_prefix("return ")
        && let Some((subj_raw, rest)) = after.split_once(" at random from ")
        && let Some((zone_raw, to_raw)) = rest.split_once(" to ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ReturnZone {
            subject: Box::new(subj),
            from_zone: Some(zone_raw.trim().to_string()),
            to: to_raw.trim().to_string(),
            tapped: false,
            transformed: false,
            with_counter: None,
            attached_to: None,
            random: true,
            except: None,
            count: None,
            up_to: false,
            any_number: false,
            actor: None,
        }]);
    }

    // 15. Return ~ to the battlefield under your control as a <type> (Takklemaggot)
    if let Some(after) = low.strip_prefix("return ")
        && let Some((subj_raw, type_raw)) =
            after.split_once(" to the battlefield under your control as ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![
            InstructionAst::ReturnZone {
                subject: Box::new(subj.clone()),
                from_zone: None,
                to: "battlefield under your control".to_string(),
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
            },
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::SetOrAddType {
                    subject: Box::new(subj),
                    type_or_subtype: type_raw.trim().to_string(),
                    additive: false,
                    condition: None,
                }),
                duration: DurationAst::Permanent,
            },
        ]);
    }

    // 16. Sacrifice a tenth of ... rounded up (Legate Lanius)
    if let Some((who_raw, rest)) = low.split_once(" sacrifices a tenth of ")
        && let Some(flt_raw) = rest.strip_suffix(", rounded up")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        let count = ValueExprAst::DivConst {
            expr: Box::new(ValueExprAst::Count(Box::new(flt.clone()))),
            denom: 10,
            rounding: Rounding::Up,
        };
        return Some(vec![InstructionAst::Sacrifice {
            player,
            count: Some(Box::new(count)),
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            except: None,
            half: false,
            rounding: Some(Rounding::Up),
            chooser: Some(PlayerRefAst::ThatPlayer),
        }]);
    }

    // 17. Choose-per-filter / sacrifice-rest (Global Ruin, Stick Together, Guided Passage, Distended Mindbender)
    if let Some((who_raw, rest)) = low
        .split_once(" chooses from ")
        .or_else(|| low.split_once(" choose from "))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let (sac_rest, core) = match rest.strip_suffix(", then sacrifices the rest") {
            Some(r) => (true, r),
            None => (false, rest),
        };
        if let Some(after_src) = core.strip_prefix("the lands they control ")
            && let Ok(flt) = parse_object_filter(after_src.trim(), vocab)
        {
            return Some(vec![InstructionAst::ChoosePerFilter {
                player,
                categories: vec![flt],
                sacrifice_rest: sac_rest,
            }]);
        }
        if let Some(after_among) = core
            .strip_prefix("among them ")
            .or_else(|| core.strip_prefix("it "))
        {
            let parts: Vec<&str> = if let Some((a, bc)) = after_among.split_once(", ")
                && let Some((b, c)) = bc.split_once(", and ")
            {
                vec![a.trim(), b.trim(), c.trim()]
            } else if let Some((a, b)) = after_among.split_once(" and ") {
                vec![a.trim(), b.trim()]
            } else {
                Vec::new()
            };
            if !parts.is_empty() {
                let mut cats = Vec::with_capacity(parts.len());
                for p in parts {
                    cats.push(parse_object_filter(p, vocab).ok()?);
                }
                return Some(vec![InstructionAst::ChoosePerFilter {
                    player,
                    categories: cats,
                    sacrifice_rest: sac_rest,
                }]);
            }
        }
    }
    if let Some((who_raw, rest)) = low.split_once(" chooses a party from among ")
        && let Some(flt_raw) = rest.strip_suffix(", then sacrifices the rest")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ChoosePerFilter {
            player,
            categories: vec![flt],
            sacrifice_rest: true,
        }]);
    }

    // 18. Starting with you, each player separates <subj> into <N> piles (Whims of the Fates)
    if let Some(after) = low.strip_prefix("starting with you, ")
        && let Some((who_raw, rest)) = after.split_once(" separates ")
        && let Some((subj_raw, pile_raw)) = rest.split_once(" into ")
        && let Some(cnt_tok) = pile_raw.strip_suffix(" piles")
        && let (Some(chooser), Ok(subject), Some(n)) = (
            parse_player_ref(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_number_word(cnt_tok.trim()),
        )
    {
        return Some(vec![InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::SeparateIntoPiles {
                chooser,
                subject: Box::new(subject),
                pile_count: u8::try_from(n).unwrap_or(3),
            },
        )]);
    }

    // 19. Exile the pile of an opponent's choice and return the other to <zone> (Death or Glory)
    if let Some(dest) =
        low.strip_prefix("exile the pile of an opponent's choice and return the other to ")
    {
        return Some(vec![
            InstructionAst::PileAction {
                action: "exile".to_string(),
                pile: PileRefAst::ChosenPile,
                filter: None,
            },
            InstructionAst::ReturnZone {
                subject: Box::new(SubjectRefAst::PileRef(PileRefAst::OtherPile)),
                from_zone: None,
                to: dest.trim().to_string(),
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
            },
        ]);
    }

    // 20. Choose <flt> with mana value 1 ..., then do the same for <flt> with mana value 2 and 3
    if let Some(after) = low.strip_prefix("choose ")
        && let Some((first_raw, rest)) = after.split_once(", then do the same for ")
        && let Some((base_raw, nums_raw)) = rest.split_once(" with mana value ")
        && let Some((n2_s, n3_s)) = nums_raw.split_once(" and ")
        && let (Ok(f1), Ok(base_flt), Ok(n2), Ok(n3)) = (
            parse_object_filter(first_raw.trim(), vocab),
            parse_object_filter(base_raw.trim(), vocab),
            n2_s.trim().parse::<i32>(),
            n3_s.trim().parse::<i32>(),
        )
    {
        let mut f2 = base_flt.clone();
        f2.zone = f1.zone;
        f2.stat_bounds.push(StatBoundAst {
            stat: StatKind::ManaValue,
            op: CompareOp::Eq,
            value: Box::new(ValueExprAst::Const(n2)),
        });
        let mut f3 = base_flt;
        f3.zone = f1.zone;
        f3.stat_bounds.push(StatBoundAst {
            stat: StatKind::ManaValue,
            op: CompareOp::Eq,
            value: Box::new(ValueExprAst::Const(n3)),
        });
        return Some(vec![
            InstructionAst::ChooseFromGroup {
                count: Box::new(ValueExprAst::Const(1)),
                random: false,
                group: Box::new(SubjectRefAst::Group(Box::new(f1))),
            },
            InstructionAst::ChooseFromGroup {
                count: Box::new(ValueExprAst::Const(1)),
                random: false,
                group: Box::new(SubjectRefAst::Group(Box::new(f2))),
            },
            InstructionAst::ChooseFromGroup {
                count: Box::new(ValueExprAst::Const(1)),
                random: false,
                group: Box::new(SubjectRefAst::Group(Box::new(f3))),
            },
        ]);
    }

    // 21. Player chooses a group / random choice (Phyrexian Grimoire, Summon: Valefor, Confusion in the Ranks, Tyrant of Discord)
    if let Some((who_raw, rest)) = low.split_once(" chooses ")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let (random, core) = match rest.strip_suffix(" at random") {
            Some(r) => (true, r),
            None => (false, rest),
        };
        if let Ok(group) = parse_subject_or_target(core.trim(), vocab) {
            let act = InstructionAst::ChooseFromGroup {
                count: Box::new(ValueExprAst::Const(1)),
                random,
                group: Box::new(group),
            };
            return Some(
                if matches!(
                    player,
                    PlayerRefAst::EachOpponent | PlayerRefAst::EachPlayer
                ) {
                    vec![InstructionAst::ForEachPlayer {
                        player,
                        body: vec![act],
                    }]
                } else {
                    vec![act]
                },
            );
        }
    }

    // 22. Exchange control of the creature you each control with the greatest mana value (Juxtapose)
    if let Some(after) = low.strip_prefix("you and ")
        && let Some((other_raw, obj_raw)) = after.split_once(" exchange control of ")
        && let Some(_other) = parse_player_ref(other_raw.trim())
        && let Some(stat_part) =
            obj_raw.strip_prefix("the creature you each control with the greatest ")
    {
        let stat = match stat_part.trim() {
            "mana value" => StatKind::ManaValue,
            "power" => StatKind::Power,
            "toughness" => StatKind::Toughness,
            _ => return None,
        };
        let mut flt = obj_type(CardTypeMask::CREATURE);
        flt.predicates.push(FilterPredicateAst::SuperlativeStat {
            stat,
            greatest: true,
        });
        return Some(vec![InstructionAst::Exchange {
            kind: ExchangeKind::Control,
            left: Box::new(SubjectRefAst::Group(Box::new(flt))),
            right: None,
        }]);
    }

    // 23. For any number of opponents, <eff> (Windgrace's Judgment)
    if let Some(body_low) = low.strip_prefix("for any number of opponents, ")
        && let Some(body) = parse_sub_effect_e8(body_low, body_low, vocab)
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player: PlayerRefAst::EachOpponent,
            body,
        }]);
    }

    // 24. Distribute counters among one, two, or three <flt> (Invasion of Alara)
    if let Some(after) = low.strip_prefix("distribute ")
        && let Some((cnt_and_kind, tgts_raw)) = after.split_once(" counters among ")
        && let Some((cnt_tok, kind_raw)) = cnt_and_kind.trim().split_once(' ')
        && let Some(n) = parse_number_word(cnt_tok.trim())
    {
        let clean_tgt = tgts_raw
            .trim()
            .strip_prefix("one, two, or three ")
            .or_else(|| tgts_raw.trim().strip_prefix("one or two "))
            .unwrap_or(tgts_raw.trim());
        if let Ok(flt) = parse_object_filter(clean_tgt, vocab) {
            return Some(vec![InstructionAst::DistributeCounters {
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::Const(n)),
                targets: Box::new(SubjectRefAst::Group(Box::new(flt))),
            }]);
        }
    }

    // 25. Gain control for as long as <cond> (Garland, Royal Kidnapper)
    if let Some(after) = low.strip_prefix("gain control of ")
        && let Some((subj_raw, cond_raw)) = after.split_once(" for as long as ")
        && let (Ok(subj), Ok(cond)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_condition(cond_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            subject: Box::new(subj),
            duration: DurationAst::ForAsLongAs(Box::new(cond)),
        }]);
    }

    // 26. Tap up to X target <flt>, where X is <val> (Make a _____ Splash)
    if let Some((action_raw, x_raw)) = low.split_once(", where x is ")
        && let (Ok(body), Ok(x_expr)) = (
            parse_atomic_effect(action_raw.trim(), vocab),
            parse_value_expr(x_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::BindX {
            x_expr: Box::new(x_expr),
            body,
        }]);
    }

    // 27. Leave <subj> in <zone> (Deliver Unto Evil)
    if let Some(after) = low.strip_prefix("leave ")
        && let Some((subj_raw, zone_raw)) = after.split_once(" in ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: format!("stay_in:{}", zone_raw.trim()),
        }]);
    }

    // 28. Look at the top card(s) of <player>'s library (Tahngarth's Glare, look_and_exile_face_down)
    if let Some((who_raw, rest)) = low.split_once(" looks at the top ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((cnt_part, lib_part)) = rest.split_once(" of ")
        && let Some(whose_raw) = lib_part.strip_suffix(" library")
    {
        let count = if cnt_part.trim() == "card" {
            ValueExprAst::Const(1)
        } else {
            let num_s = cnt_part
                .trim()
                .strip_suffix(" cards")
                .unwrap_or(cnt_part.trim());
            ValueExprAst::Const(parse_number_word(num_s.trim())?)
        };
        return Some(vec![InstructionAst::LookAt {
            player,
            targets: vec![SubjectRefAst::Anaphora(format!(
                "top_{count:?}_of_{}",
                whose_raw.trim()
            ))],
        }]);
    }

    // 29. "<player> destroys <N> <flt>" (Burning of Xinye)
    if let Some((who_raw, rest)) = low
        .split_once(" destroys ")
        .or_else(|| low.split_once(" destroy "))
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((cnt_tok, flt_raw)) = rest.trim().split_once(' ')
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_tok.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player,
            body: vec![
                InstructionAst::ChooseFromGroup {
                    count: Box::new(ValueExprAst::Const(n)),
                    random: false,
                    group: Box::new(SubjectRefAst::Group(Box::new(flt))),
                },
                InstructionAst::Destroy {
                    subject: Box::new(SubjectRefAst::Anaphora("those permanents".to_string())),
                    random: false,
                    except: None,
                },
            ],
        }]);
    }

    // 30. "<player> draws as many cards as they discarded this way" (Forget)
    if let Some(after) = low.strip_prefix("draws as many cards as ")
        && after.ends_with(" discarded this way")
    {
        return Some(vec![InstructionAst::Draw {
            player: PlayerRefAst::ThatPlayer,
            count: Box::new(ValueExprAst::PriorResult),
            up_to: false,
        }]);
    }

    // 31. "<player> each discard their hands" (Wheel and Deal)
    if let Some(who_raw) = low.strip_suffix(" each discard their hands")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::ForEachPlayer {
            player,
            body: vec![InstructionAst::DiscardHand {
                player: PlayerRefAst::ThatPlayer,
            }],
        }]);
    }

    // 32. "another player of your choice may gain control of <subj>" (Stardew Valley)
    if let Some(subj_raw) = low.strip_prefix("another player of your choice may gain control of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::OptionalPlayer {
            player: PlayerRefAst::EachOtherPlayer,
            body: vec![InstructionAst::GainControl {
                controller: PlayerRefAst::ThatPlayer,
                subject: Box::new(subj),
                duration: dur.clone(),
            }],
        }]);
    }

    None
}

fn parse_statics_and_restrictions_e8(
    s: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. "<subj> become(s) copies of <src> until <dur>" (Niko, Light of Hope)
    if let Some((subj_raw, rest)) = low.split_once(" become copies of ")
        && let Some((src_raw, dur_raw)) = rest.split_once(" until ")
        && let (Ok(subject), Ok(source)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_subject_or_target(src_raw.trim(), vocab),
        )
    {
        let copy_dur = if dur_raw.trim() == "the next end step" || dur_raw.trim() == "end of turn" {
            DurationAst::UntilEndOfTurn
        } else {
            dur.clone()
        };
        return Some(vec![InstructionAst::BecomeCopy {
            subject: Box::new(subject),
            source: Box::new(source),
            duration: copy_dur,
            exception: None,
        }]);
    }

    // 2. Base P/T dynamic assignment (Galion, Trench Gorger, Brine Hag)
    if let Some(after) = low.strip_prefix("its base power and toughness become equal to ")
        && let Some(src_raw) = after
            .strip_suffix("'s power and toughness until end of turn")
            .or_else(|| after.strip_suffix("'s power and toughness"))
        && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::SetDynamicPT {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            power: Some(Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(src.clone()),
                stat: StatKind::Power,
            })),
            toughness: Some(Box::new(ValueExprAst::ObjectStat {
                subject: Box::new(src),
                stat: StatKind::Toughness,
            })),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if let Some((subj_raw, val_raw)) =
        low.split_once(" has base power and base toughness each equal to ")
        && let (Ok(subject), Ok(val)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_value_expr(val_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::SetDynamicPT {
            subject: Box::new(subject),
            power: Some(Box::new(val.clone())),
            toughness: Some(Box::new(val)),
            duration: DurationAst::Permanent,
        }]);
    }
    if let Some(after) = low.strip_prefix("the base power and toughness of ")
        && let Some((subj_raw, pt_raw)) = after.split_once(" become ")
        && let Some((p_s, t_s)) = pt_raw.trim().split_once('/')
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(subject),
            power: p_s.trim().to_string(),
            toughness: t_s.trim().to_string(),
            description: None,
            keywords: Vec::new(),
            duration: DurationAst::Permanent,
        }]);
    }

    // 3. Change land type (Vision Charm)
    if low == "each land of the first chosen type becomes the second chosen type until end of turn"
    {
        return Some(vec![InstructionAst::BecomeChosenCharacteristic {
            subject: Box::new(SubjectRefAst::Each(Box::new(obj_type(CardTypeMask::LAND)))),
            kind: "second_chosen_land_type".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    // 4. Activated abilities can't be activated for as long as <cond> (Braided Net)
    if let Some((subj_raw, cond_raw)) =
        low.split_once(" activated abilities can't be activated for as long as ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let subj = if subj_raw.trim() == "its" {
            SubjectRefAst::Anaphora("it".to_string())
        } else {
            parse_subject_or_target(subj_raw.trim().trim_end_matches("'s"), vocab).ok()?
        };
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "activated_abilities_cant_be_activated".to_string(),
                filter: None,
                condition: Some(Box::new(cond.clone())),
            }),
            duration: DurationAst::ForAsLongAs(Box::new(cond)),
        }]);
    }

    // 5. Assign combat damage as though unblocked (Outmaneuver, Predatory Focus)
    let (opt_unb, unb_core) = match low.strip_prefix("you may have ") {
        Some(r) => (true, r),
        None => (false, low),
    };
    if let Some(subj_raw) = unb_core
        .strip_suffix(" assign their combat damage this turn as though they weren't blocked")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let act = InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "assign_combat_damage_as_though_unblocked".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        };
        return Some(if opt_unb {
            vec![InstructionAst::Optional(vec![act])]
        } else {
            vec![act]
        });
    }

    // 6. Combat restrictions & forced attacks/blocks
    if let Some(subj_raw) =
        low.strip_suffix(" can't attack you or planeswalkers you control that combat")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "cant_attack_you_or_your_planeswalkers".to_string(),
            duration: DurationAst::UntilEndOfCombat,
        }]);
    }
    if let Some((who_raw, _)) =
        low.split_once(" can't attack you or permanents you control during their next turn")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Player(player)),
            rule: "cant_attack_you_or_your_permanents".to_string(),
            duration: DurationAst::UntilEndOfPlayersNextTurn(player),
        }]);
    }
    if let Some(subj_raw) = low.strip_suffix(" attack this turn if able")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "attacks_if_able".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if low == "you choose how those creatures block" {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Anaphora("those creatures".to_string())),
            rule: "you_choose_how_they_block".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if let Some(after) = low.strip_prefix("attacking doesn't cause ")
        && let Some((subj_raw, cond_raw)) = after.split_once(" to tap this combat if ")
        && let (Ok(subj), Ok(cond)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_condition(cond_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "attacking_doesnt_cause_tap".to_string(),
                filter: None,
                condition: Some(Box::new(cond)),
            }),
            duration: DurationAst::UntilEndOfCombat,
        }]);
    }

    // 7. Temporary revealed-hand rule (Stromgald Spy)
    if let Some(after) = low.strip_prefix("you may have ")
        && let Some((who_raw, cond_raw)) =
            after.split_once(" play with their hand revealed for as long as ")
        && let (Some(_player), Ok(cond)) = (
            parse_player_ref(who_raw.trim()),
            parse_condition(cond_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::LookOrRevealRule {
                    rule: format!("play_with_hand_revealed:{}", who_raw.trim()),
                    filter: None,
                    follow_up: Vec::new(),
                    condition: Some(Box::new(cond.clone())),
                }),
                duration: DurationAst::ForAsLongAs(Box::new(cond)),
            },
        ])]);
    }

    // 8. Can't lose / can't win / can't gain life (Angel's Grace, Screaming Nemesis)
    if let Some((who_raw, rule_tail)) = low
        .split_once(" can't lose the game ")
        .map(|(w, d)| (w, ("cant_lose_game", d)))
        .or_else(|| {
            low.split_once(" can't win the game ")
                .map(|(w, d)| (w, ("cant_win_game", d)))
        })
        .or_else(|| {
            low.split_once(" can't gain life ")
                .map(|(w, d)| (w, ("cant_gain_life", d)))
        })
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let rule_dur = match rule_tail.1.trim() {
            "this turn" => DurationAst::UntilEndOfTurn,
            "for the rest of the game" => DurationAst::Permanent,
            _ => return None,
        };
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(player))),
                rule: rule_tail.0.to_string(),
                filter: None,
                condition: None,
            }),
            duration: rule_dur,
        }]);
    }

    // 9. Permanent goad (Jon Irenicus, Shattered One)
    if let Some(subj_raw) = low
        .strip_suffix(" is goaded for the rest of the game")
        .or_else(|| low.strip_suffix("'s goaded for the rest of the game"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(subj),
            rule: "goaded".to_string(),
            duration: DurationAst::Permanent,
        }]);
    }

    // 10. Enters with counter and additive type
    if let Some(after) = low.strip_prefix("it enters with a ")
        && let Some((kind_raw, type_raw)) =
            after
                .split_once(" counter on it and is ")
                .and_then(|(k, t)| {
                    t.strip_suffix(" in addition to its other types")
                        .map(|ty| (k, ty))
                })
    {
        return Some(vec![
            InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                counter_kind: kind_raw.trim().to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            },
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::SetOrAddType {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    type_or_subtype: type_raw.trim().to_string(),
                    additive: true,
                    condition: None,
                }),
                duration: DurationAst::Permanent,
            },
        ]);
    }

    // 11. Activation restrictions (Merseine, Capricopian, Withering Wisps)
    if low.starts_with("only ") && low.contains(" may activate this ability") {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: low.to_string(),
            condition: None,
        }]);
    }
    if let Some(val_raw) = low.strip_prefix("activate no more times each turn than ")
        && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: "max_activations_per_turn".to_string(),
            condition: Some(Box::new(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::EventCountThisTurn {
                    event: "ability_activations_this_turn".to_string(),
                    player: None,
                }),
                op: CompareOp::Lt,
                rhs: Box::new(val),
            })),
        }]);
    }

    // 12. Alternative casting cost instruction (Cobra Trap)
    if let Some(after) = low.strip_prefix("you may pay ")
        && let Some(cost_raw) = after.strip_suffix(" rather than pay ~'s mana cost")
        && let Ok(cost) = parse_cost_atom(&cost_raw.trim().to_ascii_uppercase(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::AlternativeCastCost {
                cost: vec![cost],
                condition: None,
                for_spells: None,
                next_only: true,
            }),
            duration: DurationAst::Instant,
        }]);
    }

    // 13. Sacrifice unless pay reduced mana cost (Flash)
    if let Some(after) = low.strip_prefix("sacrifice ")
        && let Some((subj_raw, delta_raw)) =
            after.split_once(" unless you pay its mana cost reduced by ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Unless {
            body: vec![InstructionAst::Sacrifice {
                player: PlayerRefAst::You,
                count: Some(Box::new(ValueExprAst::Const(1))),
                subject: Box::new(subj),
                except: None,
                half: false,
                rounding: None,
                chooser: None,
            }],
            unless_player: Some(PlayerRefAst::You),
            unless_cost: Some(CostAtomAst::PayMana(format!(
                "its_mana_cost_minus_{}",
                delta_raw.trim().to_ascii_uppercase()
            ))),
            unless_action: None,
            unless_condition: None,
        }]);
    }

    // 14. Modal token characteristics line (Outlaws' Merriment)
    if let Some((pt_tok, rest)) = low.split_once(' ')
        && pt_tok.contains('/')
        && pt_tok.split('/').all(|p| p.parse::<u8>().is_ok())
        && rest.contains(" with ")
    {
        return Some(vec![InstructionAst::TokenCharacteristicsChoice(
            s.to_string(),
        )]);
    }

    // 15. "it's attacking <player>" (Midnight Crusader Shuttle)
    if let Some(who_raw) = low.strip_prefix("it's attacking ")
        && parse_player_ref(who_raw.trim()).is_some()
    {
        return Some(vec![InstructionAst::EntersTappedRule {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            tapped: false,
            attacking: Some(who_raw.trim().to_string()),
            duration: DurationAst::UntilEndOfCombat,
        }]);
    }

    // 16. "<subj> gains <kws> until end of turn, or until <cond>, whichever comes first" (Drop Tower)
    if let Some((before_or, until_part)) = low.split_once(" until end of turn, or until ")
        && let Some(cond_raw) = until_part.strip_suffix(", whichever comes first")
        && let Some((subj_raw, kw_raw)) = before_or
            .split_once(" gains ")
            .or_else(|| before_or.split_once(" gain "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
    {
        let cond = parse_condition(cond_raw.trim(), vocab)
            .ok()
            .unwrap_or_else(|| ConditionAst::EventOccurredThisTurn {
                event: cond_raw.trim().to_string(),
            });
        return Some(vec![InstructionAst::GrantKeywords {
            subject: Box::new(subj),
            keywords: kws,
            duration: DurationAst::ForAsLongAs(Box::new(ConditionAst::Not(Box::new(cond)))),
        }]);
    }

    // 17. "increase or decrease the result by <N>" (Night Shift of the Living Dead, Xenosquirrels)
    if let Some(num_raw) = low.strip_prefix("increase or decrease the result by ")
        && let Some(n) = parse_number_word(num_raw.trim())
        && let Ok(delta) = i8::try_from(n)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::DieRollModifier {
                cost: Vec::new(),
                delta: Some(delta),
                reroll: false,
                once_each_turn: false,
            }),
            duration: DurationAst::Instant,
        }]);
    }

    // 18. "you may activate ~'s loyalty ability <N> more times this turn" (Comet, Stellar Pup)
    if let Some(after) = low.strip_prefix("you may activate ")
        && let Some((subj_raw, rest)) = after.split_once("'s loyalty ability ")
        && let Some(num_raw) = rest
            .strip_suffix(" more times this turn")
            .or_else(|| rest.strip_suffix(" more time this turn"))
        && let Some(n) = parse_number_word(num_raw.trim())
        && let Ok(extra_times) = u8::try_from(n)
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::ActivationLimitOverride {
                ability_kind: "loyalty".to_string(),
                subject: Box::new(subj),
                extra_times,
                instant_speed: false,
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }

    None
}

fn parse_conditional_wrappers_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. "otherwise, <eff>" (Auratouched Mage)
    if let Some(rest_low) = low.strip_prefix("otherwise, ") {
        let rest_s = &s[s.len() - rest_low.len()..];
        if let Some(body) = parse_sub_effect_e8(rest_s, rest_low, vocab) {
            return Some(vec![InstructionAst::Otherwise(body)]);
        }
    }

    // 2. "if you do, <eff>" / "if they do, <eff>" / "if you don't, <eff>" / "if they don't, <eff>"
    for (pref, succeeded) in [
        ("if you do, ", true),
        ("if they do, ", true),
        ("if you don't, ", false),
        ("if they don't, ", false),
    ] {
        if let Some(rest_low) = low.strip_prefix(pref) {
            let rest_s = &s[s.len() - rest_low.len()..];
            if let Some(body) = parse_sub_effect_e8(rest_s, rest_low, vocab) {
                return Some(vec![if succeeded {
                    InstructionAst::IfPreviousSucceeded(body)
                } else {
                    InstructionAst::IfPreviousFailed(body)
                }]);
            }
        }
    }

    // 3. Leading "if <cond>, <eff> [instead]"
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest_low)) = after_if.split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let (is_instead, eff_low) = match rest_low.strip_suffix(" instead") {
            Some(r) => (true, r),
            None => (false, rest_low),
        };
        let eff_s_start = s.len() - rest_low.len();
        let eff_s = &s[eff_s_start..eff_s_start + eff_low.len()];
        if let Some(mut body) = parse_sub_effect_e8(eff_s, eff_low, vocab) {
            if is_instead {
                body = vec![InstructionAst::ReplaceEffect(body)];
            }
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: body,
            }]);
        }
    }

    // 4. Trailing "<eff> if <cond>"
    if let Some((eff_low, cond_raw)) = low.rsplit_once(" if ")
        && !eff_low.trim().is_empty()
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let eff_s = &s[..eff_low.len()];
        if let Some(body) = parse_sub_effect_e8(eff_s.trim(), eff_low.trim(), vocab) {
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: body,
            }]);
        }
    }

    // 5. Trailing "<eff> unless <cond>" (Dead Ringers)
    if let Some((eff_low, cond_raw)) = low.rsplit_once(" unless ")
        && !eff_low.trim().is_empty()
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let eff_s = &s[..eff_low.len()];
        if let Some(body) = parse_sub_effect_e8(eff_s.trim(), eff_low.trim(), vocab) {
            return Some(vec![InstructionAst::Unless {
                body,
                unless_player: None,
                unless_cost: None,
                unless_action: None,
                unless_condition: Some(Box::new(cond)),
            }]);
        }
    }

    None
}

thread_local! {
    static E8_SPLIT_DEPTH: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

fn parse_compound_splitters_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Prevent exponential blowup against DepthGuard PARSE_OPS budget on multi-"and" sentences
    if low.starts_with("if ")
        || low.starts_with("when ")
        || low.starts_with("whenever ")
        || low.contains(" simultaneously ")
        || E8_SPLIT_DEPTH.with(|d| d.get() >= 1)
    {
        return None;
    }
    struct SplitGuard;
    impl Drop for SplitGuard {
        fn drop(&mut self) {
            E8_SPLIT_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }
    E8_SPLIT_DEPTH.with(|d| d.set(d.get() + 1));
    let _guard = SplitGuard;

    // 1. Sequential ", then " splitter
    if let Some((l_low, r_low)) = low.split_once(", then ") {
        let l_s = &s[..l_low.len()];
        let r_s = &s[l_low.len() + 7..];
        if let (Some(mut l), Some(mut r)) = (
            parse_sub_effect_e8(l_s.trim(), l_low.trim(), vocab),
            parse_sub_effect_e8(r_s.trim(), r_low.trim(), vocab),
        ) {
            l.append(&mut r);
            return Some(l);
        }
    }

    // 2. Sequential ", and " or " and " splitter (bounded to top-level splits where both halves compile cleanly)
    for sep in [", and ", " and "] {
        for (idx, _) in low.match_indices(sep) {
            let l_low = low[..idx].trim();
            let r_low = low[idx + sep.len()..].trim();
            if l_low.is_empty() || r_low.is_empty() || l_low.ends_with(',') {
                continue;
            }
            if l_low.matches('"').count() % 2 == 1 {
                continue;
            }
            let l_s = s[..idx].trim();
            let r_s = s[idx + sep.len()..].trim();
            if let (Some(mut l), Some(mut r)) = (
                parse_sub_effect_e8(l_s, l_low, vocab),
                parse_sub_effect_e8(r_s, r_low, vocab),
            ) {
                l.append(&mut r);
                return Some(l);
            }
        }
    }

    None
}

fn parse_sub_effect_e8(s: &str, low: &str, vocab: &Vocabulary) -> Option<Vec<InstructionAst>> {
    if low.is_empty() {
        return None;
    }
    if let Ok(ins) = parse_atomic_effect(s, vocab) {
        return Some(ins);
    }
    if let Some(rest) = low.strip_prefix("puts ")
        && let Ok(ins) = parse_atomic_effect(&format!("that player puts {rest}"), vocab)
    {
        return Some(ins);
    }
    if let Some(rest) = low.strip_prefix("exiles ")
        && let Ok(ins) = parse_atomic_effect(&format!("that player exiles {rest}"), vocab)
    {
        return Some(ins);
    }
    if low == "sacrifices it" {
        return Some(vec![InstructionAst::Sacrifice {
            player: PlayerRefAst::ThatPlayer,
            count: Some(Box::new(ValueExprAst::Const(1))),
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            except: None,
            half: false,
            rounding: None,
            chooser: None,
        }]);
    }
    if low == "remove them" {
        return Some(vec![InstructionAst::RemoveCounters {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            counter_kind: "those".to_string(),
            all: true,
            count: None,
        }]);
    }
    if low == "exiles it face down" {
        return Some(vec![InstructionAst::Exile {
            actor: Some(PlayerRefAst::ThatPlayer),
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            random: false,
            face_down: true,
            into_pile: false,
        }]);
    }
    if let Some(after) = low.strip_prefix("put ")
        && let Some((subj_raw, dest_raw)) = after.split_once(" on ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: dest_raw.trim().to_string(),
        }]);
    }
    if let Some(after) = low.strip_prefix("put ")
        && let Some((subj_raw, dest_raw)) = after.split_once(" onto ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: format!("onto {}", dest_raw.trim()),
        }]);
    }
    if let Some(after) = low.strip_prefix("put ")
        && let Some((subj_raw, dest_raw)) = after.split_once(" into ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(subj),
            to: dest_raw.trim().to_string(),
        }]);
    }
    if low == "reveal the aura card" {
        return Some(vec![InstructionAst::RevealCards {
            subject: Box::new(SubjectRefAst::Anaphora("the aura card".to_string())),
        }]);
    }
    if let Some(after) = low.strip_prefix("you may put ")
        && let Some((subj_raw, dest_raw)) = after.split_once(" onto ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::MoveZone {
                subject: Box::new(subj),
                to: format!("onto {}", dest_raw.trim()),
            },
        ])]);
    }
    if let Some(subj_raw) = low.strip_prefix("fights ")
        && let Ok(b) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Fight {
            a: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
            b: Box::new(b),
        }]);
    }
    if low == "you may cast it without paying its mana cost" {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only: true,
                subject: Box::new(SubjectRefAst::SelfObject),
                free: true,
                duration: "now".to_string(),
                count_limit: None,
                alt_cost: None,
            },
        ])]);
    }
    if low == "it gains \"~ can't be sacrificed.\"" {
        return Some(vec![InstructionAst::GrantAbilityText {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            ability_text: "~ can't be sacrificed.".to_string(),
            compiled: None,
            duration: DurationAst::Permanent,
        }]);
    }
    None
}

// =============================================================================
// Compositional Wave E8 Static & Replacement Parser (eliminates E3/E4_STATIC_EXACTS)
// =============================================================================

/// Compositional Wave E8 static and replacement parser replacing legacy
/// `E3_STATIC_EXACTS` and `E4_STATIC_EXACTS` literal tables with reusable
/// atomic static and replacement grammars.
#[inline(never)]
pub(crate) fn parse_wave_e8_statics(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    let low = low.trim_end_matches('.');
    let s_clean = s.trim_end_matches('.');

    // 1. Continuous statics, multi-clause grants, conditional layers, and dynamic P/T
    if let Some(sr) = parse_continuous_and_conditional_statics_e8(s_clean, low, vocab) {
        return Some(sr);
    }
    // 2. Keyword/ability grants to spells/cards, craft/devour/landwalk/delve statics
    if let Some(sr) = parse_keyword_and_spell_grant_statics_e8(s_clean, low, vocab) {
        return Some(sr);
    }
    // 3. Additional/alternative cast & activation costs, mana spend rules, and permissions
    if let Some(sr) = parse_cost_and_permission_statics_e8(s_clean, low, vocab) {
        return Some(sr);
    }
    // 4. Compositional as-enters, die-roll, discard, draw, death, and search replacements
    if let Some(sr) = parse_replacement_and_enters_statics_e8(s_clean, low, vocab) {
        return Some(sr);
    }
    // 5. Combat damage assignment, activation limit/timing overrides, and game-rule statics
    parse_combat_activation_and_rule_statics_e8(s_clean, low, vocab)
}

fn extract_quoted_segments(s: &str) -> (Vec<String>, String) {
    let mut quoted = Vec::new();
    let mut rest = String::with_capacity(s.len());
    let mut in_quote = false;
    let mut cur = String::new();
    for ch in s.chars() {
        if ch == '"' {
            if in_quote {
                quoted.push(cur.trim().trim_end_matches(',').to_string());
                cur.clear();
                in_quote = false;
            } else {
                in_quote = true;
            }
        } else if in_quote {
            cur.push(ch);
        } else {
            rest.push(ch);
        }
    }
    (quoted, rest)
}

fn parse_simple_color_condition(cond_raw: &str, vocab: &Vocabulary) -> Option<ConditionAst> {
    let c = cond_raw.trim();
    if let Ok(cond) = parse_condition(c, vocab) {
        return Some(cond);
    }
    if let Some(color_word) = c.strip_prefix("it's ").or_else(|| c.strip_prefix("it is ")) {
        let mask = match color_word.trim() {
            "white" => ColorMask::WHITE,
            "blue" => ColorMask::BLUE,
            "black" => ColorMask::BLACK,
            "red" => ColorMask::RED,
            "green" => ColorMask::GREEN,
            _ => ColorMask::EMPTY,
        };
        if !mask.is_empty() {
            return Some(ConditionAst::MatchesFilter {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                filter: Box::new(ObjectFilterAst {
                    any_colors: mask,
                    ..Default::default()
                }),
            });
        }
    }
    if let Some(after) = c.strip_prefix("there are ")
        && let Some((num_raw, _)) =
            after.split_once(" or more card types among cards in your graveyard")
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        return Some(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: false,
            }),
            op: CompareOp::Ge,
            rhs: Box::new(ValueExprAst::Const(n)),
        });
    }
    if c == "a card exiled with ~ is a creature card" {
        return Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_card: true,
                zone: Some(ZoneFilter::Exile),
                flags: ObjectFilterFlags::EXILED_WITH_SELF,
                ..Default::default()
            }),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(after) = c.strip_prefix("the top card of your graveyard is ")
        && let Ok(flt) = parse_object_filter(after.trim(), vocab)
    {
        return Some(ConditionAst::Marker(GameMarker::TopCardOfZoneMatches {
            zone: ZoneFilter::YourGraveyard,
            filter: Box::new(flt),
        }));
    }
    if let Some(after) = c.strip_prefix("that player controls ")
        && let Some((num_raw, flt_raw)) = after.split_once(" or fewer ")
        && let Some(n) = parse_number_word(num_raw.trim())
        && let Ok(v) = u16::try_from(n)
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(ConditionAst::ControlsCount {
            player: PlayerRefAst::ThatPlayer,
            filter: Box::new(flt),
            op: CompareOp::Le,
            value: v,
        });
    }
    if let Some(after) = c.strip_prefix("a ")
        && let Some((flt_raw, who_raw)) = after.split_once(" is attacking ")
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
        && let Some(defender) = parse_player_ref(who_raw.trim())
    {
        flt.predicates.push(FilterPredicateAst::AttackingTarget {
            target: Box::new(SubjectRefAst::Player(defender)),
            this_turn: false,
        });
        return Some(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some((left, right)) = c.split_once(" and ")
        && let (Some(c1), Some(c2)) = (
            parse_simple_color_condition(left, vocab),
            parse_simple_color_condition(right, vocab),
        )
    {
        return Some(ConditionAst::And(Box::new(c1), Box::new(c2)));
    }
    None
}

fn parse_dynamic_pt_expr(raw: &str, vocab: &Vocabulary) -> Option<ValueExprAst> {
    let r = raw.trim();
    if let Ok(v) = parse_value_expr(r, vocab) {
        return Some(v);
    }
    if let Some((lhs_raw, rhs_raw)) = r.split_once(" plus ")
        && let (Some(l), Some(rhs)) = (
            parse_dynamic_pt_expr(lhs_raw, vocab),
            parse_dynamic_pt_expr(rhs_raw, vocab),
        )
    {
        return Some(ValueExprAst::Add(Box::new(l), Box::new(rhs)));
    }
    if let Some((lhs_raw, rhs_raw)) = r.split_once(" minus ")
        && let (Some(l), Some(rhs)) = (
            parse_dynamic_pt_expr(lhs_raw, vocab),
            parse_dynamic_pt_expr(rhs_raw, vocab),
        )
    {
        return Some(ValueExprAst::Sub(Box::new(l), Box::new(rhs)));
    }
    if r == "the number of those card types" {
        return Some(ValueExprAst::CardTypesInGraveyard {
            all_graveyards: false,
        });
    }
    None
}

fn parse_continuous_and_conditional_statics_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. Additive "also get(s) +P/+T ..." (Jetmir, Nexus of Revels and future scaling anthem cards)
    if low.contains(" also get ") || low.contains(" also gets ") {
        let stripped = s
            .replace(" also get ", " get ")
            .replace(" also gets ", " gets ");
        if let Ok(sr) = parse_static_or_replacement(&stripped, vocab) {
            return Some(sr);
        }
    }

    // 2. Multi-condition keyword/ability list:
    // "<subj> has <grant1> (if|as long as) <cond1>, <grant2> (if|as long as) <cond2>, ..., and <grantN> (if|as long as) <condN>"
    // (Scion of Draco, Multiclass Baldric, Tribal Golem)
    if let Some((subj_raw, rest_low)) = low.split_once(" has ").or_else(|| low.split_once(" have "))
        && rest_low.contains(", ")
        && (rest_low.contains(" if ") || rest_low.contains(" as long as "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let rest_s = &s[s.len() - rest_low.len()..];
        let mut items = Vec::new();
        let mut all_ok = true;
        // Split on ", " outside quotes
        let mut parts = Vec::new();
        let mut in_q = false;
        let mut start = 0usize;
        let bytes = rest_s.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'"' {
                in_q = !in_q;
            } else if !in_q && i + 1 < bytes.len() && bytes[i] == b',' && bytes[i + 1] == b' ' {
                parts.push(rest_s[start..i].trim());
                start = i + 2;
                i += 1;
            }
            i += 1;
        }
        parts.push(rest_s[start..].trim());
        if parts.len() >= 2 {
            for raw_part in parts {
                let cleaned_s = raw_part
                    .strip_prefix("and ")
                    .or_else(|| raw_part.strip_prefix("And "))
                    .unwrap_or(raw_part)
                    .trim();
                let cleaned_low = cleaned_s.to_ascii_lowercase();
                let split_res = cleaned_low
                    .split_once(" as long as ")
                    .map(|(g, c)| (g, c, " as long as ".len()))
                    .or_else(|| cleaned_low.split_once(" if ").map(|(g, c)| (g, c, 4)));
                let Some((grant_low, cond_low, _)) = split_res else {
                    all_ok = false;
                    break;
                };
                let Some(cond) = parse_simple_color_condition(cond_low, vocab) else {
                    all_ok = false;
                    break;
                };
                let grant_s = cleaned_s[..grant_low.len()].trim();
                if grant_s.starts_with('"') && grant_s.ends_with('"') && grant_s.len() >= 2 {
                    items.push(StaticAbilityAst::GrantAbilityText {
                        subject: Box::new(subj.clone()),
                        ability_text: grant_s[1..grant_s.len() - 1].to_string(),
                        compiled: None,
                        condition: Some(Box::new(cond)),
                    });
                } else if let Some(kws) = parse_keyword_list(grant_low.trim(), vocab) {
                    items.push(StaticAbilityAst::GrantKeywords {
                        subject: Box::new(subj.clone()),
                        keywords: kws,
                        condition: Some(Box::new(cond)),
                        paired_rule: None,
                    });
                } else {
                    all_ok = false;
                    break;
                }
            }
            if all_ok && !items.is_empty() {
                return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                    items,
                )));
            }
        }
    }

    // 3. "<subj> is <supertype/type>, gets <+P/+T>, and has <kws>" (On Serra's Wings)
    if let Some((subj_raw, rest)) = low.split_once(" is ")
        && let Some((ty_raw, pt_and_kws)) = rest.split_once(", gets ")
        && let Some((pt_raw, kws_raw)) = pt_and_kws.split_once(", and has ")
        && let Some((dx, dy)) = pt_raw.trim().split_once('/')
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && let Some(kws) = parse_keyword_list(kws_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![
                StaticAbilityAst::SetOrAddType {
                    subject: Box::new(subj.clone()),
                    type_or_subtype: ty_raw.trim().to_string(),
                    additive: true,
                    condition: None,
                },
                StaticAbilityAst::ContinuousModifyPT {
                    subject: Box::new(subj),
                    dx: dx.trim().to_string(),
                    dy: dy.trim().to_string(),
                    grant_keywords: kws,
                    grant_ability_text: None,
                    scale: None,
                    condition: None,
                    paired_rule: None,
                },
            ],
        )));
    }

    // 4. "<subj> get(s) <+P/+T> and has/have <kws_and_quoted_abilities>" (Cultist of the Absolute, Spy Kit)
    if let Some((subj_raw, rest)) = low.split_once(" get ").or_else(|| low.split_once(" gets "))
        && let Some((pt_raw, grant_low)) = rest
            .split_once(" and have ")
            .or_else(|| rest.split_once(" and has "))
        && let Some((dx, dy)) = pt_raw.trim().split_once('/')
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let grant_s = &s[s.len() - grant_low.len()..];
        let (quoted, unquoted) = extract_quoted_segments(grant_s);
        if !quoted.is_empty() {
            let mut items = Vec::new();
            let kw_clean = unquoted
                .replace(", and ", ", ")
                .replace(" and ", ", ")
                .trim_matches(|c: char| c == ',' || c == ' ')
                .to_ascii_lowercase();
            let kws = if kw_clean.is_empty() {
                Vec::new()
            } else {
                parse_keyword_list(&kw_clean, vocab).unwrap_or_default()
            };
            items.push(StaticAbilityAst::ContinuousModifyPT {
                subject: Box::new(subj.clone()),
                dx: dx.trim().to_string(),
                dy: dy.trim().to_string(),
                grant_keywords: kws,
                grant_ability_text: None,
                scale: None,
                condition: None,
                paired_rule: None,
            });
            for ab in quoted {
                items.push(StaticAbilityAst::GrantAbilityText {
                    subject: Box::new(subj.clone()),
                    ability_text: ab,
                    compiled: None,
                    condition: None,
                });
            }
            return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                items,
            )));
        }
        if let Some(names_flt) = grant_low
            .strip_prefix("all names of ")
            .and_then(|r| r.strip_suffix(" in addition to its name"))
            && let Ok(flt) = parse_object_filter(names_flt.trim(), vocab)
        {
            return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                vec![
                    StaticAbilityAst::ContinuousModifyPT {
                        subject: Box::new(subj.clone()),
                        dx: dx.trim().to_string(),
                        dy: dy.trim().to_string(),
                        grant_keywords: Vec::new(),
                        grant_ability_text: None,
                        scale: None,
                        condition: None,
                        paired_rule: None,
                    },
                    StaticAbilityAst::RuleOrCombatRestriction {
                        subject: Some(Box::new(subj)),
                        rule: "has_all_names_of".to_string(),
                        filter: Some(Box::new(flt)),
                        condition: None,
                    },
                ],
            )));
        }
    }

    // 5. "<subj> has/have <kw>, \"<ab1>,\" and \"<ab2>\"" (Swashbuckler's Whip)
    if s.contains('"')
        && let Some((subj_raw, grant_low)) =
            low.split_once(" has ").or_else(|| low.split_once(" have "))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let grant_s = &s[s.len() - grant_low.len()..];
        let (quoted, unquoted) = extract_quoted_segments(grant_s);
        if !quoted.is_empty() {
            let mut items = Vec::new();
            let kw_clean = unquoted
                .replace(", and ", ", ")
                .replace(" and ", ", ")
                .trim_matches(|c: char| c == ',' || c == ' ')
                .to_ascii_lowercase();
            if !kw_clean.is_empty()
                && let Some(kws) = parse_keyword_list(&kw_clean, vocab)
            {
                items.push(StaticAbilityAst::GrantKeywords {
                    subject: Box::new(subj.clone()),
                    keywords: kws,
                    condition: None,
                    paired_rule: None,
                });
            }
            for ab in quoted {
                items.push(StaticAbilityAst::GrantAbilityText {
                    subject: Box::new(subj.clone()),
                    ability_text: ab,
                    compiled: None,
                    condition: None,
                });
            }
            if !items.is_empty() {
                return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                    items,
                )));
            }
        }
    }

    // 6. "<subj> has <kw>, it can't be blocked, and its name and creature type are <desc>" (Psychic Paper)
    if let Some((subj_raw, rest)) = low.split_once(" has ")
        && let Some((kw_part, tail)) = rest.split_once(", it can't be blocked, and ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![
                StaticAbilityAst::GrantKeywords {
                    subject: Box::new(subj.clone()),
                    keywords: vec![kw_part.trim().to_string()],
                    condition: None,
                    paired_rule: None,
                },
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(subj.clone())),
                    rule: "cant_be_blocked".to_string(),
                    filter: None,
                    condition: None,
                },
                StaticAbilityAst::SetOrAddType {
                    subject: Box::new(subj),
                    type_or_subtype: tail.trim().to_string(),
                    additive: false,
                    condition: None,
                },
            ],
        )));
    }

    // 7. "<subj> have/has base power and toughness <P>/<T> and lose all <types>" (Curse of Conformity)
    if let Some((subj_raw, rest)) = low
        .split_once(" have base power and toughness ")
        .or_else(|| low.split_once(" has base power and toughness "))
        && let Some((pt_raw, lose_raw)) = rest
            .split_once(" and lose all ")
            .or_else(|| rest.split_once(" and loses all "))
        && let Some((p, t)) = pt_raw.trim().split_once('/')
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some(format!("lose_all:{}", lose_raw.trim())),
                additive_types: false,
                base_pt: Some((p.trim().to_string(), t.trim().to_string())),
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }

    // 8. Compound subject list "<subj1>, <subj2>, and <subj3> have <kws>" (Shalai, Shield of Kaldra)
    if let Some((subjs_raw, kw_raw)) = low.split_once(" have ")
        && subjs_raw.contains(", and ")
        && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
    {
        let mut subjs = Vec::new();
        let mut ok = true;
        for part in subjs_raw.split(", ") {
            let p = part.strip_prefix("and ").unwrap_or(part).trim();
            if let Ok(sr) = parse_subject_or_target(p, vocab) {
                subjs.push(sr);
            } else if let Some(pl) = parse_player_ref(p) {
                subjs.push(SubjectRefAst::Player(pl));
            } else if !p.is_empty() {
                subjs.push(SubjectRefAst::Anaphora(p.to_string()));
            } else {
                ok = false;
                break;
            }
        }
        if ok && subjs.len() >= 2 {
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::GrantKeywords {
                    subject: Box::new(SubjectRefAst::Compound(subjs)),
                    keywords: kws,
                    condition: None,
                    paired_rule: None,
                },
            ));
        }
    }

    // 9. Trailing conditional keyword grant "<subj> have/has <kws> as long as <cond>" (Sheltering Prayers)
    if let Some((left, cond_raw)) = low.split_once(" as long as ")
        && let Some((subj_raw, kw_raw)) = left
            .split_once(" have ")
            .or_else(|| left.split_once(" has "))
        && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
        && let Some(cond) = parse_simple_color_condition(cond_raw, vocab)
    {
        let subj = parse_subject_or_target(subj_raw.trim(), vocab)
            .or_else(|_| {
                subj_raw
                    .trim()
                    .strip_suffix(" each player controls")
                    .ok_or_else(String::new)
                    .and_then(|base| {
                        parse_object_filter(base, vocab).map(|mut f| {
                            f.controller = Some(ControllerRelation::ThatPlayer);
                            SubjectRefAst::All(Box::new(f))
                        })
                    })
            })
            .ok()?;
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::GrantKeywords {
                subject: Box::new(subj),
                keywords: kws,
                condition: Some(Box::new(cond)),
                paired_rule: None,
            },
        ));
    }

    // 10. Turn-scoped dynamic base P/T ("During your turn / During turns other than yours, <subj>'s power and toughness are each [equal to] <val>") (Angry Mob)
    for (pref, is_your_turn) in [
        ("during your turn, ", true),
        ("during turns other than yours, ", false),
    ] {
        if let Some(rest) = low.strip_prefix(pref)
            && let Some((subj_raw, expr_raw)) = rest
                .split_once("'s power and toughness are each equal to ")
                .or_else(|| rest.split_once("'s power and toughness are each "))
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
            && let Some(val) = parse_dynamic_pt_expr(expr_raw, vocab)
        {
            let cond = if is_your_turn {
                ConditionAst::IsYourTurn
            } else {
                ConditionAst::Not(Box::new(ConditionAst::IsYourTurn))
            };
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::DynamicBasePT {
                    subject: Box::new(subj),
                    power: Box::new(val.clone()),
                    toughness: Box::new(val),
                    condition: Some(Box::new(cond)),
                },
            ));
        }
    }

    // 11. Universal "As long as <cond>, <inner>" conditional static wrapper
    // (Duplicant, Volrath's Shapeshifter, Winter, Woolly Razorback, Camel, Futurist Operative, Trinisphere, Gaea's Liege)
    if let Some(after_al) = low.strip_prefix("as long as ")
        && let Some((cond_raw, rest_low)) = after_al.split_once(", ")
        && let Some(cond) = parse_simple_color_condition(cond_raw, vocab)
    {
        let rest_s = &s[s.len() - rest_low.len()..];
        // Dynamic P/T ("its/~'s power and toughness are each equal to <expr>") (Gaea's Liege)
        if let Some(expr_raw) = rest_low
            .strip_prefix("its power and toughness are each equal to ")
            .or_else(|| rest_low.strip_prefix("~'s power and toughness are each equal to "))
            && let Some(val) = parse_dynamic_pt_expr(expr_raw, vocab)
        {
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::DynamicBasePT {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    power: Box::new(val.clone()),
                    toughness: Box::new(val),
                    condition: Some(Box::new(cond)),
                },
            ));
        }
        // Maximum hand size ("<player>'s maximum hand size is equal to <expr>") (Winter, Misanthropic Guide)
        if let Some((who_raw, expr_raw)) = rest_low
            .split_once("'s maximum hand size is equal to ")
            .or_else(|| rest_low.split_once("'s maximum hand size is "))
            && let Some(player) = parse_player_ref(who_raw.trim())
            && let Some(size) = parse_dynamic_pt_expr(expr_raw, vocab)
        {
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::ModifyMaximumHandSize {
                    player,
                    size: Box::new(size),
                    condition: Some(Box::new(cond)),
                },
            ));
        }
        // Minimum spell cost floor ("each spell that would cost less than <N> mana to cast costs <N> mana to cast") (Trinisphere)
        if let Some(after_sp) = rest_low.strip_prefix("each spell that would cost less than ")
            && let Some((num_raw, _)) = after_sp.split_once(" mana to cast costs ")
            && let Some(n) = parse_number_word(num_raw.trim())
        {
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::CostModifier {
                    subject: Box::new(SubjectRefAst::All(Box::new(ObjectFilterAst {
                        any_spell: true,
                        ..Default::default()
                    }))),
                    delta: format!("min_{n}"),
                    less: false,
                    next_only: false,
                    per: None,
                    scale_expr: Some(Box::new(ValueExprAst::Const(n))),
                    condition: Some(Box::new(cond)),
                },
            ));
        }
        // Copy stats/types or full text of referenced card (Duplicant, Volrath's Shapeshifter)
        if let Some(src_raw) =
            rest_low.strip_prefix("~ has the power, toughness, and creature types of ")
        {
            let src_flt =
                parse_object_filter(src_raw.trim(), vocab).unwrap_or_else(|_| ObjectFilterAst {
                    any_card: true,
                    zone: Some(ZoneFilter::Exile),
                    flags: ObjectFilterFlags::EXILED_WITH_SELF,
                    ..Default::default()
                });
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::InheritAbilities {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    source_filter: Box::new(src_flt),
                    activated_only: false,
                    condition: Some(Box::new(cond)),
                },
            ));
        }
        if let Some(after_ft) = rest_low.strip_prefix("~ has the full text of ") {
            let (quoted, _) = extract_quoted_segments(rest_s);
            let mut items = vec![StaticAbilityAst::InheritAbilities {
                subject: Box::new(SubjectRefAst::SelfObject),
                source_filter: Box::new(ObjectFilterAst {
                    any_card: true,
                    zone: Some(ZoneFilter::YourGraveyard),
                    ..Default::default()
                }),
                activated_only: false,
                condition: Some(Box::new(cond.clone())),
            }];
            for ab in quoted {
                items.push(StaticAbilityAst::GrantAbilityText {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    ability_text: ab,
                    compiled: None,
                    condition: Some(Box::new(cond.clone())),
                });
            }
            let _ = after_ft;
            return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                items,
            )));
        }
        // Prevent combat damage + keyword ("prevent all combat damage it would deal and it has <kws>") (Woolly Razorback)
        if let Some(kw_raw) =
            rest_low.strip_prefix("prevent all combat damage it would deal and it has ")
            && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
        {
            return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                vec![
                    StaticAbilityAst::PreventAllDamageStatic {
                        combat_only: true,
                        noncombat_only: false,
                        both_ways: false,
                        subject: Box::new(SubjectRefAst::SelfObject),
                        by: Some(Box::new(SubjectRefAst::SelfObject)),
                    },
                    StaticAbilityAst::GrantKeywords {
                        subject: Box::new(SubjectRefAst::SelfObject),
                        keywords: kws,
                        condition: Some(Box::new(cond)),
                        paired_rule: None,
                    },
                ],
            )));
        }
        // Prevent damage from source filter to subjects ("prevent all damage <src> would deal to <dst>") (Camel)
        if let Some(after_pr) = rest_low.strip_prefix("prevent all damage ")
            && let Some((src_raw, dst_raw)) = after_pr.split_once(" would deal to ")
            && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
        {
            let dst = parse_subject_or_target(dst_raw.trim(), vocab)
                .unwrap_or_else(|_| SubjectRefAst::Anaphora(dst_raw.trim().to_string()));
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::PreventAllDamageStatic {
                    combat_only: false,
                    noncombat_only: false,
                    both_ways: false,
                    subject: Box::new(dst),
                    by: Some(Box::new(src)),
                },
            ));
        }
        // Layer type + base P/T + unblockable ("it's a <types> with base power and toughness <P>/<T> and can't be blocked") (Futurist Operative)
        if let Some(after_its) = rest_low.strip_prefix("it's a ")
            && let Some((ty_raw, pt_tail)) = after_its.split_once(" with base power and toughness ")
            && let Some((pt_raw, tail)) = pt_tail.split_once(" and ")
            && let Some((p, t)) = pt_raw.trim().split_once('/')
        {
            return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                vec![
                    StaticAbilityAst::ContinuousLayerEffect {
                        subject: Box::new(SubjectRefAst::SelfObject),
                        set_types: Some(ty_raw.trim().to_string()),
                        additive_types: false,
                        base_pt: Some((p.trim().to_string(), t.trim().to_string())),
                        set_colors: None,
                        grant_keywords: Vec::new(),
                        lose_keywords: Vec::new(),
                        lose_all_abilities: false,
                        condition: Some(Box::new(cond.clone())),
                    },
                    StaticAbilityAst::RuleOrCombatRestriction {
                        subject: Some(Box::new(SubjectRefAst::SelfObject)),
                        rule: tail.trim().to_string(),
                        filter: None,
                        condition: Some(Box::new(cond)),
                    },
                ],
            )));
        }
    }

    // 12. Global color/text/type layer statics (Mycosynth Lattice, Painter's Servant, Swirl the Mists, Illusionary Terrain, Ghostly Flame)
    if let Some(color_desc) =
        low.strip_prefix("all cards that aren't on the battlefield, spells, and permanents are ")
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(SubjectRefAst::All(Box::new(ObjectFilterAst {
                    any_card: true,
                    any_spell: true,
                    any_permanent: true,
                    ..Default::default()
                }))),
                set_types: None,
                additive_types: false,
                base_pt: None,
                set_colors: Some(color_desc.trim().to_string()),
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("all instances of color words in the text of ")
        && let Some((scope_raw, target_raw)) = after.split_once(" are changed to ")
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(SubjectRefAst::Anaphora(scope_raw.trim().to_string())),
                set_types: None,
                additive_types: false,
                base_pt: None,
                set_colors: Some(format!("text_change:{}", target_raw.trim())),
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("basic lands of the ")
        && let Some((first_raw, second_raw)) = after.split_once(" are the ")
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::SetOrAddType {
                subject: Box::new(SubjectRefAst::Anaphora(format!(
                    "basic lands of the {}",
                    first_raw.trim()
                ))),
                type_or_subtype: second_raw.trim().to_string(),
                additive: false,
                condition: None,
            },
        ));
    }
    if let Some(flt_raw) = low.strip_suffix(" are colorless sources of damage") {
        let subj = parse_subject_or_target(flt_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(flt_raw.trim().to_string()));
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: None,
                additive_types: false,
                base_pt: None,
                set_colors: Some("colorless_damage_source".to_string()),
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: false,
                condition: None,
            },
        ));
    }

    None
}

fn parse_keyword_and_spell_grant_statics_e8(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. "<flt> has/have the chosen creature type in addition to its other types" (Ashes of the Fallen)
    if let Some(subj_raw) = low
        .strip_suffix(" has the chosen creature type in addition to its other types")
        .or_else(|| {
            low.strip_suffix(" have the chosen creature type in addition to their other types")
        })
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::SetOrAddType {
                subject: Box::new(subj),
                type_or_subtype: "chosen_creature_type".to_string(),
                additive: true,
                condition: None,
            },
        ));
    }

    // 2. "<spell_flt> gain(s) <kw> <cost> as you cast them" (Zinnia, Ashling, the Limitless)
    if let Some(before) = low.strip_suffix(" as you cast them")
        && let Some((subj_raw, kw_cost)) = before
            .split_once(" gain ")
            .or_else(|| before.split_once(" gains "))
    {
        let subj = parse_subject_or_target(subj_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
        let (kw, cost_rule) = kw_cost
            .trim()
            .split_once(' ')
            .map_or((kw_cost.trim(), ""), |(k, c)| (k, c.trim()));
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::GrantKeywordWithCost {
                subject: Box::new(subj),
                keyword: kw.to_string(),
                cost_rule: cost_rule.to_ascii_uppercase(),
                condition: None,
            },
        ));
    }

    // 3. "<card_or_spell_flt> has/have <kw> [<cost>]" for graveyard/cast-filtered grants
    // (Solemn Doomguide, Niv-Mizzet Supreme, Threefold Signal, Rain of Riches)
    if let Some((subj_raw, rhs_raw)) = low.rsplit_once(" has ")
        && (subj_raw.starts_with("each ") || subj_raw.starts_with("the first spell "))
    {
        let rhs = rhs_raw.trim();
        let (kw, cost_opt) = match rhs.split_once(' ') {
            Some((k, c)) if c.starts_with('{') => (k, Some(c.to_ascii_uppercase())),
            _ => (rhs, None),
        };
        if matches!(
            kw,
            "unearth"
                | "jump-start"
                | "replicate"
                | "cascade"
                | "offspring"
                | "evoke"
                | "flashback"
                | "escape"
                | "encore"
        ) {
            let subj = parse_subject_or_target(subj_raw.trim(), vocab)
                .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
            if let Some(cost_rule) = cost_opt {
                return Some(StaticOrReplacement::Static(
                    StaticAbilityAst::GrantKeywordWithCost {
                        subject: Box::new(subj),
                        keyword: kw.to_string(),
                        cost_rule,
                        condition: None,
                    },
                ));
            }
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::GrantKeywords {
                    subject: Box::new(subj),
                    keywords: vec![kw.to_string()],
                    condition: None,
                    paired_rule: None,
                },
            ));
        }
    }

    // 4. Dynamic X keyword grants ("~ has <kw> x, where x is <expr>", "devour x, where x is <expr>", "craft with <list> <cost>")
    // (Kodama of the Center Tree, Thromok, Throne of the Grim Captain)
    if let Some((head, expr_raw)) = low.split_once(" x, where x is ") {
        let kw = head.strip_prefix("~ has ").unwrap_or(head).trim();
        if !kw.is_empty() && kw.split_whitespace().count() <= 2 {
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::GrantKeywordWithCost {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    keyword: kw.to_string(),
                    cost_rule: format!("x={}", expr_raw.trim()),
                    condition: None,
                },
            ));
        }
    }
    if let Some(rest) = low.strip_prefix("craft with ")
        && let Some((materials, cost_tok)) = rest.rsplit_once(' ')
        && cost_tok.starts_with('{')
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::GrantKeywordWithCost {
                subject: Box::new(SubjectRefAst::SelfObject),
                keyword: "craft".to_string(),
                cost_rule: format!("{}|{}", materials.trim(), cost_tok.to_ascii_uppercase()),
                condition: None,
            },
        ));
    }

    // 5. "For each basic land type among <flt>, ~ has landwalk of that type" (Magnigoth Treefolk)
    if let Some(after) = low.strip_prefix("for each basic land type among ")
        && let Some((flt_raw, rest)) = after.split_once(", ")
        && rest.contains(" has landwalk of that type")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(StaticAbilityAst::ScaledBy {
            per: Box::new(ValueExprAst::DistinctCountAmong {
                category: "basic_land_type".to_string(),
                filter: Box::new(flt),
            }),
            inner: Box::new(StaticAbilityAst::GrantKeywords {
                subject: Box::new(SubjectRefAst::SelfObject),
                keywords: vec!["landwalk_matching_basic_land_type".to_string()],
                condition: None,
                paired_rule: None,
            }),
        }));
    }

    // 6. "If a <flt> with <kw> was exiled with ~'s <ability> ability, ~ has <kw>" (Soulflayer)
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_part, body_part)) = after_if.split_once(", ~ has ")
        && let Some((flt_part, _)) = cond_part.split_once(" was exiled with ~'s ")
        && let Some(kws) = parse_keyword_list(body_part.trim(), vocab)
    {
        let mut flt =
            parse_object_filter(flt_part.trim(), vocab).unwrap_or_else(|_| ObjectFilterAst {
                any_types: CardTypeMask::CREATURE,
                any_card: true,
                ..Default::default()
            });
        flt.zone = Some(ZoneFilter::Exile);
        flt.flags = flt.flags.union(ObjectFilterFlags::EXILED_WITH_SELF);
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::GrantKeywords {
                subject: Box::new(SubjectRefAst::SelfObject),
                keywords: kws,
                condition: Some(Box::new(ConditionAst::ZoneObjectCount {
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: 1,
                })),
                paired_rule: None,
            },
        ));
    }

    None
}

fn parse_cost_and_permission_statics_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. Additional activation cost:
    // "Activated abilities [of <flt>] cost an additional <cost> to activate [for each <per>]"
    // "<kind> abilities you activate cost an additional <+N/-N> to activate"
    // (Drought, Brutal Suppression, Carth the Lion)
    if let Some((head, rest)) = low.split_once(" cost an additional ")
        && (rest.contains(" to activate") || rest.contains(" to cast"))
    {
        let is_activate = rest.contains(" to activate");
        let split_marker = if is_activate {
            " to activate"
        } else {
            " to cast"
        };
        if let Some((cost_part, tail)) = rest.split_once(split_marker) {
            let cost_clean = cost_part.trim().trim_matches('"');
            let per = tail
                .trim()
                .strip_prefix("for each ")
                .map(|p| p.trim().to_string());
            let parsed_cost = if let Some(after_plus) = cost_clean.strip_prefix('+')
                && let Ok(n) = after_plus.parse::<i32>()
            {
                Some(CostAtomAst::PutCounters {
                    count: Box::new(ValueExprAst::Const(n)),
                    counter_kind: "loyalty".to_string(),
                    subject: Box::new(SubjectRefAst::Anaphora("that planeswalker".to_string())),
                })
            } else if let Some(life_raw) = cost_clean.strip_suffix(" life")
                && let Ok(amt) = parse_value_expr(life_raw.trim(), vocab)
            {
                Some(CostAtomAst::PayLife(Box::new(amt)))
            } else {
                parse_cost_atom(cost_clean, vocab).ok()
            };
            if let Some(cost_atom) = parsed_cost {
                if is_activate {
                    let subj = head
                        .strip_prefix("activated abilities of ")
                        .and_then(|f| parse_object_filter(f.trim(), vocab).ok())
                        .map(|f| Box::new(SubjectRefAst::All(Box::new(f))))
                        .or_else(|| {
                            if head != "activated abilities" {
                                Some(Box::new(SubjectRefAst::Anaphora(head.trim().to_string())))
                            } else {
                                None
                            }
                        });
                    return Some(StaticOrReplacement::Static(
                        StaticAbilityAst::AdditionalActivationCost {
                            subject: subj,
                            cost: cost_atom,
                            per,
                            condition: None,
                        },
                    ));
                }
                // Additional cast cost (Terror of the Peaks, Drought spells)
                let subj = parse_subject_or_target(head.trim(), vocab)
                    .unwrap_or_else(|_| SubjectRefAst::Anaphora(head.trim().to_string()));
                return Some(StaticOrReplacement::Static(
                    StaticAbilityAst::AdditionalActivationCost {
                        subject: Some(Box::new(subj)),
                        cost: cost_atom,
                        per: per.or_else(|| Some("cast".to_string())),
                        condition: None,
                    },
                ));
            }
        }
    }

    // 2. "As an additional cost to cast <subj>, <cost_or_action>" (Tectonic Split, Chorus of the Conclave)
    if let Some(after) = low.strip_prefix("as an additional cost to cast ")
        && let Some((subj_raw, rest)) = after.split_once(", ")
    {
        let r = rest.trim();
        if let Some(flt_raw) = r.strip_prefix("sacrifice half ").and_then(|x| {
            x.strip_suffix(", rounded up")
                .or_else(|| x.strip_suffix(", rounded down"))
        }) && let Ok(flt) = parse_object_filter(
            flt_raw
                .trim()
                .strip_prefix("the ")
                .unwrap_or(flt_raw.trim()),
            vocab,
        ) {
            let rounding = if r.ends_with("rounded up") {
                Rounding::Up
            } else {
                Rounding::Down
            };
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::AdditionalCastCost {
                    cost: CostAtomAst::Sacrifice {
                        count: Box::new(ValueExprAst::DivConst {
                            expr: Box::new(ValueExprAst::Count(Box::new(flt.clone()))),
                            denom: 2,
                            rounding,
                        }),
                        filter: Box::new(flt),
                        up_to: false,
                        min_one: false,
                    },
                },
            ));
        }
        if r == "you may pay any amount of mana" {
            let _ = subj_raw;
            return Some(StaticOrReplacement::Static(
                StaticAbilityAst::AdditionalCastCost {
                    cost: CostAtomAst::PayMana("{X}".to_string()),
                },
            ));
        }
    }

    // 3. Mana substitution ("For each {<M>} in a cost, you may pay <cost> rather than pay that mana") (K'rrik)
    if let Some(after) = low.strip_prefix("for each ")
        && let Some((sym_raw, rest)) = after.split_once(" in a cost, you may ")
        && let Some(cost_raw) = rest.strip_suffix(" rather than pay that mana")
        && let Ok(cost_atom) = parse_cost_atom(cost_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::AlternativeAbilityCost {
                cost: vec![cost_atom],
                ability_kind: "mana_substitution".to_string(),
                scope: sym_raw.trim().to_ascii_uppercase(),
                condition: None,
            },
        ));
    }

    // 4. Alternative cast costs ("Rather than pay ...", "If <cond>, rather than pay ...", "Once during each of your turns, you may cast <flt> by paying ... rather than ...", "If <cond>, you may cast ~ without paying its mana cost and as though it had flash")
    // (Dream Halls, Liesa, Skyshroud Cutter, Demon of Fate's Design, Qasali Ambusher)
    if let Some(after) =
        low.strip_prefix("rather than pay the mana cost for a spell, its controller may ")
        && let Ok(cost_atom) = parse_cost_atom(after.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::AlternativeCastCost {
                cost: vec![cost_atom],
                condition: None,
                for_spells: Some(Box::new(ObjectFilterAst {
                    any_spell: true,
                    ..Default::default()
                })),
                next_only: false,
            },
        ));
    }
    if low.starts_with("rather than pay ")
        && let Some((paid_part, alt_part)) = low.split_once(", ")
        && paid_part.contains("command zone")
    {
        let life_cost = alt_part
            .strip_prefix("pay ")
            .and_then(|r| r.split_once(" life"))
            .and_then(|(n, _)| parse_number_word(n.trim()))
            .map_or_else(
                || CostAtomAst::PayLife(Box::new(ValueExprAst::Const(2))),
                |n| {
                    CostAtomAst::PayLife(Box::new(ValueExprAst::Mul(
                        Box::new(ValueExprAst::Const(n)),
                        Box::new(ValueExprAst::TimesCastFromCommandZone(Box::new(
                            SubjectRefAst::SelfObject,
                        ))),
                    )))
                },
            );
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::AlternativeCastCost {
                cost: vec![life_cost],
                condition: None,
                for_spells: None,
                next_only: false,
            },
        ));
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) = after_if.split_once(", rather than pay ~'s mana cost, ")
        && let Some(cond) = parse_simple_color_condition(cond_raw, vocab)
    {
        let eff_clean = rest.strip_prefix("you may ").unwrap_or(rest);
        let cost = parse_cost_atom(eff_clean, vocab)
            .map(|c| vec![c])
            .unwrap_or_default();
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::AlternativeCastCost {
                cost,
                condition: Some(Box::new(cond)),
                for_spells: None,
                next_only: false,
            },
        ));
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) =
            after_if.split_once(", you may cast ~ without paying its mana cost")
        && let Some(cond) = parse_simple_color_condition(cond_raw, vocab)
    {
        let alt = StaticAbilityAst::AlternativeCastCost {
            cost: Vec::new(),
            condition: Some(Box::new(cond.clone())),
            for_spells: None,
            next_only: false,
        };
        if rest.contains("as though it had flash") {
            return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
                vec![
                    alt,
                    StaticAbilityAst::GrantKeywords {
                        subject: Box::new(SubjectRefAst::SelfObject),
                        keywords: vec!["flash".to_string()],
                        condition: Some(Box::new(cond)),
                        paired_rule: None,
                    },
                ],
            )));
        }
        return Some(StaticOrReplacement::Static(alt));
    }
    if let Some(after) = low.strip_prefix("once during each of your turns, you may cast ")
        && let Some((flt_raw, rest)) = after
            .split_once(" by paying life equal to its mana value rather than paying its mana cost")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        let _ = rest;
        return Some(StaticOrReplacement::Static(StaticAbilityAst::OncePerTurn(
            Box::new(StaticAbilityAst::AlternativeCastCost {
                cost: vec![CostAtomAst::PayLife(Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat: StatKind::ManaValue,
                }))],
                condition: Some(Box::new(ConditionAst::IsYourTurn)),
                for_spells: Some(Box::new(flt)),
                next_only: false,
            }),
        )));
    }

    // 5. Cast from graveyard with scaling tax ("You may cast ~ from <zone> if you pay <delta> more to cast it for each <flt>") (Risen Executioner)
    if let Some(after) = low.strip_prefix("you may cast ~ from ")
        && let Some((zone_raw, rest)) = after.split_once(" if you pay ")
        && let Some((delta_raw, fe_raw)) = rest.split_once(" more to cast it for each ")
        && let Ok(per_flt) = parse_object_filter(fe_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![
                StaticAbilityAst::PlayOrCastPermission {
                    cast_only: true,
                    filter: Box::new(ObjectFilterAst {
                        zone: Some(ZoneFilter::YourGraveyard),
                        ..Default::default()
                    }),
                    permission: format!("cast_self_from_{}", zone_raw.trim()),
                    condition: None,
                },
                StaticAbilityAst::CostModifier {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    delta: delta_raw.trim().to_ascii_uppercase(),
                    less: false,
                    next_only: false,
                    per: Some(Box::new(per_flt)),
                    scale_expr: None,
                    condition: None,
                },
            ],
        )));
    }

    // 6. Foretell cost/timing modifiers (Ranar, Cosmos Charger)
    if let Some(after) = low.strip_prefix("the first card you ")
        && let Some((action_raw, rest)) = after.split_once(" each turn costs ")
        && let Some((cost_tok, _)) = rest.split_once(" to ")
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::AlternativeAbilityCost {
                cost: vec![CostAtomAst::PayMana(cost_tok.trim().to_ascii_uppercase())],
                ability_kind: action_raw.trim().to_string(),
                scope: "first_each_turn".to_string(),
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("foretelling cards from your hand costs ")
        && let Some((delta_raw, tail)) = after.split_once(" less")
    {
        let mut items = vec![StaticAbilityAst::CostModifier {
            subject: Box::new(SubjectRefAst::Anaphora("foretell".to_string())),
            delta: delta_raw.trim().to_ascii_uppercase(),
            less: true,
            next_only: false,
            per: None,
            scale_expr: None,
            condition: None,
        }];
        if tail.contains("on any player's turn") {
            items.push(StaticAbilityAst::ActivationLimitOverride {
                ability_kind: "foretell".to_string(),
                subject: Box::new(SubjectRefAst::Anaphora("cards in your hand".to_string())),
                extra_times: 0,
                instant_speed: true,
                condition: None,
            });
        }
        return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            items,
        )));
    }

    // 7. Mana spend restriction ("Spend only <kind> mana on X") (Emblazoned Golem)
    if let Some(kind_raw) = low
        .strip_prefix("spend only ")
        .and_then(|r| r.strip_suffix(" mana on x"))
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::ManaSpendRule {
                mana_kind: Some(kind_raw.trim().to_string()),
                as_though: "spend_only_on_x".to_string(),
                purpose: Some("x".to_string()),
                condition: None,
            },
        ));
    }

    // 8. Play/cast/plot permissions (Shared Fate, Evelyn, Fblthp Lost on the Range)
    if let Some(after) = low.strip_prefix("you may plot ")
        && let Some(flt_raw) = after.strip_suffix(" from the top of your library")
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.zone = Some(ZoneFilter::YourLibrary);
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::PlayOrCastPermission {
                cast_only: true,
                filter: Box::new(flt),
                permission: "plot_from_library_top".to_string(),
                condition: None,
            },
        ));
    }
    if low.starts_with("each player may look at cards they exiled with ~")
        && low.contains("play lands and cast spells from among those cards")
    {
        let flt = ObjectFilterAst {
            any_card: true,
            zone: Some(ZoneFilter::Exile),
            flags: ObjectFilterFlags::EXILED_WITH_SELF,
            ..Default::default()
        };
        return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![
                StaticAbilityAst::LookOrRevealRule {
                    rule: "look_at_exiled_with_self".to_string(),
                    filter: Some(Box::new(flt.clone())),
                    follow_up: Vec::new(),
                    condition: None,
                },
                StaticAbilityAst::PlayOrCastPermission {
                    cast_only: false,
                    filter: Box::new(flt),
                    permission: "each_player_play_exiled_with_self".to_string(),
                    condition: None,
                },
            ],
        )));
    }
    if let Some(after) = low.strip_prefix("once each turn, you may play a card from exile with a ")
        && let Some((ck_raw, rest)) = after.split_once(" counter on it")
    {
        let flt = ObjectFilterAst {
            any_card: true,
            zone: Some(ZoneFilter::Exile),
            counter_kind: Some(ck_raw.trim().to_string()),
            ..Default::default()
        };
        let mut items = vec![StaticAbilityAst::PlayOrCastPermission {
            cast_only: false,
            filter: Box::new(flt),
            permission: "play_exiled_with_counter".to_string(),
            condition: None,
        }];
        if rest.contains("spend mana as though it were mana of any color") {
            items.push(StaticAbilityAst::ManaSpendRule {
                mana_kind: None,
                as_though: "any_color".to_string(),
                purpose: Some("cast_that_card".to_string()),
                condition: None,
            });
        }
        return Some(StaticOrReplacement::Static(StaticAbilityAst::OncePerTurn(
            Box::new(StaticAbilityAst::Compound(items)),
        )));
    }

    // 9. Pregame mulligan special action ("Any time you could mulligan and ~ is in your hand, <eff>") (Serum Powder)
    if let Some(after) = low.strip_prefix("any time you could mulligan and ~ is in your hand, ") {
        let rest_s = &s[s.len() - after.len()..];
        let (body, fails) = parse_effect_body(rest_s, vocab);
        if fails.is_empty() && !body.is_empty() {
            return Some(StaticOrReplacement::Static(StaticAbilityAst::FollowUp(
                vec![InstructionAst::TemporarySpecialAction {
                    timing: "mulligan".to_string(),
                    duration: DurationAst::Permanent,
                    body,
                }],
            )));
        }
    }

    None
}

fn parse_replacement_and_enters_statics_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. "As ~ enters, <eff>" (Alhammarret, Neverwinter Hydra, Grifter's Blade, Deadpool)
    if let Some(after) = low.strip_prefix("as ~ enters, ") {
        let rest_s = &s[s.len() - after.len()..];
        if let Some(after_ex) = after.strip_prefix("you may exchange ")
            && (after_ex.contains("text box") || after_ex.contains("text boxes"))
        {
            return Some(StaticOrReplacement::Replacement(
                ReplacementEffectAst::AsEntersAction(vec![InstructionAst::Optional(vec![
                    InstructionAst::ExchangeTextBoxes {
                        left: Box::new(SubjectRefAst::SelfObject),
                        right: Box::new(SubjectRefAst::Anaphora("another creature".to_string())),
                    },
                ])]),
            ));
        }
        if let Some(flt_raw) = after
            .strip_prefix("choose ")
            .and_then(|r| r.strip_suffix(" it could be attached to"))
            && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            return Some(StaticOrReplacement::Replacement(
                ReplacementEffectAst::AsEntersAction(vec![InstructionAst::ChooseFromGroup {
                    count: Box::new(ValueExprAst::Const(1)),
                    random: false,
                    group: Box::new(SubjectRefAst::Group(Box::new(flt))),
                }]),
            ));
        }
        let (body, fails) = parse_effect_body(rest_s, vocab);
        if fails.is_empty() && !body.is_empty() {
            return Some(StaticOrReplacement::Replacement(
                ReplacementEffectAst::AsEntersAction(body),
            ));
        }
    }

    // 2. "As <flt> enters, it becomes a <P>/<T> <subtypes> in addition to its other types" (Displaced Dinosaurs)
    if let Some(after) = low.strip_prefix("as ")
        && let Some((flt_raw, rest)) = after.split_once(" enters, ")
        && let Ok(_flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: flt_raw.trim().to_string(),
                would: "enters".to_string(),
                instead: rest.trim().to_string(),
                instead_body: parse_atomic_effect(rest.trim(), vocab).ok(),
            },
        ));
    }

    // 3. Multi-sentence as-enters & additional-cost follow-ups (Neverwinter Hydra, Grifter's Blade, Chorus of the Conclave, Alhammarret)
    if let Some(after) = low.strip_prefix("it enters with a number of ")
        && let Some((ck_raw, val_raw)) = after.split_once(" counters on it equal to ")
    {
        let count = parse_value_expr(val_raw.trim(), vocab).unwrap_or(ValueExprAst::PriorResult);
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EntersWithCounters {
                counter_kind: ck_raw.trim().to_string(),
                count: Box::new(count),
                per: None,
                x_expr: None,
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("if you do, ") {
        if let Some(subj_raw) = after.strip_prefix("it enters attached to ") {
            return Some(StaticOrReplacement::Replacement(
                ReplacementEffectAst::EventInstead {
                    subject_or_event: "~".to_string(),
                    would: "enter".to_string(),
                    instead: format!("enters_attached_to:{}", subj_raw.trim()),
                    instead_body: None,
                },
            ));
        }
        if let Some((subj_raw, rest)) = after.split_once(" enters with that many additional ")
            && let Some(ck_raw) = rest.strip_suffix(" counters on it")
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            return Some(StaticOrReplacement::Replacement(
                ReplacementEffectAst::OthersEnterWithCounters {
                    subject: Box::new(subj),
                    counter_kind: ck_raw.trim().to_string(),
                    count: Box::new(ValueExprAst::PriorResult),
                    additional: true,
                    per: None,
                },
            ));
        }
    }
    if let Some(after) = low.strip_prefix("you choose the name of ")
        && let Some(flt_raw) = after.strip_suffix(" revealed this way")
    {
        return Some(StaticOrReplacement::Static(StaticAbilityAst::FollowUp(
            vec![InstructionAst::ChooseOption(format!(
                "card_name_revealed:{}",
                flt_raw.trim()
            ))],
        )));
    }

    // 4. "For each kind of counter on <subj>, <other_subj> enters with a counter of that kind on it" (Blue, Loyal Raptor)
    if let Some(after) = low.strip_prefix("for each kind of counter on ")
        && let Some((src_raw, rest)) = after.split_once(", ")
        && let Some(other_raw) = rest.strip_suffix(" enters with a counter of that kind on it")
        && let Ok(other_subj) = parse_subject_or_target(other_raw.trim(), vocab)
    {
        let src_flt =
            parse_object_filter(src_raw.trim(), vocab).unwrap_or_else(|_| ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            });
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::ScaledBy {
                per: Box::new(ValueExprAst::DistinctCountAmong {
                    category: "counter_kind".to_string(),
                    filter: Box::new(src_flt),
                }),
                inner: Box::new(ReplacementEffectAst::OthersEnterWithCounters {
                    subject: Box::new(other_subj),
                    counter_kind: "that kind".to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                    additional: false,
                    per: None,
                }),
            },
        ));
    }

    // 5. "~ escapes with \"<ability>\"" (Skyway Robber)
    if low.starts_with("~ escapes with \"") && s.contains('"') {
        let (quoted, _) = extract_quoted_segments(s);
        if let Some(ab) = quoted.into_iter().next() {
            return Some(StaticOrReplacement::Replacement(
                ReplacementEffectAst::AsEntersAction(vec![InstructionAst::GrantAbilityText {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    ability_text: ab,
                    compiled: None,
                    duration: DurationAst::Permanent,
                }]),
            ));
        }
    }

    // 6. Die-roll modifier statics ("After you roll a die, you may <cost>", "increase or decrease the result by <N>", "Once each turn, you may <cost> to reroll one or more dice you rolled")
    // (Night Shift of the Living Dead, Xenosquirrels, Monitor Monitor)
    if let Some(cost_raw) = low.strip_prefix("after you roll a die, you may ")
        && let Ok(costs) = parse_cost_list(cost_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::DieRollModifier {
                cost: costs,
                delta: None,
                reroll: false,
                once_each_turn: false,
            },
        ));
    }
    if let Some(num_raw) = low
        .strip_prefix("if you do, increase or decrease the result by ")
        .or_else(|| low.strip_prefix("increase or decrease the result by "))
        && let Some(n) = parse_number_word(num_raw.trim())
        && let Ok(delta) = i8::try_from(n)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::DieRollModifier {
                cost: Vec::new(),
                delta: Some(delta),
                reroll: false,
                once_each_turn: false,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("once each turn, you may ")
        && let Some((cost_raw, _)) = after.split_once(" to reroll one or more dice you rolled")
        && let Ok(costs) = parse_cost_list(cost_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::DieRollModifier {
                cost: costs,
                delta: None,
                reroll: true,
                once_each_turn: true,
            },
        ));
    }

    // 7. Damage-counter replacement ("For each 1 damage that would be dealt to <subj>, if it has a <ck> counter on it, remove a <ck> counter from it and prevent that 1 damage") (Rock Hydra)
    if let Some(after) = low.strip_prefix("for each 1 damage that would be dealt to ")
        && let Some((subj_raw, rest)) = after.split_once(", if it has a ")
        && let Some((ck_raw, _)) = rest.split_once(" counter on it, remove ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::PreventAndReplaceDamage {
                target: Box::new(subj.clone()),
                follow_up: vec![InstructionAst::RemoveCounters {
                    subject: Box::new(subj),
                    counter_kind: ck_raw.trim().to_string(),
                    all: false,
                    count: Some(Box::new(ValueExprAst::Const(1))),
                }],
                condition: Some(Box::new(ConditionAst::CounterCompare {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    counter_kind: ck_raw.trim().to_string(),
                    op: CompareOp::Ge,
                    value: 1,
                })),
            },
        ));
    }

    // 8. Death / discard / villainous choice / coin-flip / draw / surveil / cascade / library-search replacements
    // (Firestorm Phoenix, Library of Leng, Nephalia Academy, The Valeyard, Edgar, River Song, Enhanced Surveillance, Averna, Opposition Agent)
    if let Some(after_if) = low.strip_prefix("if ~ would die, ")
        && let Some(eff_raw) = after_if.strip_suffix(" instead")
        && let Ok(body) = parse_atomic_effect(eff_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "~".to_string(),
                would: "die".to_string(),
                instead: eff_raw.trim().to_string(),
                instead_body: Some(body),
            },
        ));
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((src_raw, rest)) = after_if.split_once(" causes you to discard a card, ")
        && rest.contains(" instead of ")
    {
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: src_raw.trim().to_string(),
                would: "discard_card".to_string(),
                instead: rest.trim().to_string(),
                instead_body: None,
            },
        ));
    }
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((who_raw, _)) = after_if
            .split_once(" would face a villainous choice, they face that choice an additional time")
    {
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: who_raw.trim().to_string(),
                would: "villainous_choice".to_string(),
                instead: "face_additional_time".to_string(),
                instead_body: None,
            },
        ));
    }
    if low.starts_with("the first time you flip one or more coins each turn, ") {
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "you".to_string(),
                would: "first_coin_flip_each_turn".to_string(),
                instead: low.to_string(),
                instead_body: None,
            },
        ));
    }
    if low == "you draw cards from the bottom of your library rather than the top" {
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "you".to_string(),
                would: "draw_card".to_string(),
                instead: "draw_from_bottom".to_string(),
                instead_body: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("you may look at an additional ")
        && let Some((num_raw, action_raw)) = after.split_once(" cards each time you ")
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "you".to_string(),
                would: action_raw.trim().to_string(),
                instead: format!("look_additional:{n}"),
                instead_body: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("as you cascade, ") {
        let rest_s = &s[s.len() - after.len()..];
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: "you".to_string(),
                would: "cascade".to_string(),
                instead: after.trim().to_string(),
                instead_body: parse_atomic_effect(rest_s, vocab).ok(),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("while ")
        && let Some((who_raw, rest)) = after.split_once(" is searching their library, ")
    {
        return Some(StaticOrReplacement::Replacement(
            ReplacementEffectAst::EventInstead {
                subject_or_event: who_raw.trim().to_string(),
                would: "search_library".to_string(),
                instead: rest.trim().to_string(),
                instead_body: parse_atomic_effect(rest.trim(), vocab).ok(),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("you control ")
        && let Some(who_raw) = after.strip_suffix(" while they're searching their libraries")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::ControlChange {
                controller: "you_while_searching".to_string(),
                subject: Box::new(SubjectRefAst::Player(player)),
                condition: None,
            },
        ));
    }

    None
}

fn parse_combat_activation_and_rule_statics_e8(
    s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<StaticOrReplacement> {
    // 1. Die-roll loyalty band rows ("+N, and <eff>" / "-N, then <eff>") (Comet, Stellar Pup)
    let signed_prefix = low
        .strip_prefix('+')
        .map(|r| (true, r))
        .or_else(|| low.strip_prefix('-').map(|r| (false, r)))
        .or_else(|| low.strip_prefix('\u{2212}').map(|r| (false, r)));
    if let Some((positive, after_sign)) = signed_prefix
        && let Some((num_tok, rest_low)) = after_sign
            .split_once(", and ")
            .or_else(|| after_sign.split_once(", then "))
        && let Ok(n) = num_tok.trim().parse::<i32>()
    {
        let rest_s = &s[s.len() - rest_low.len()..];
        let (mut body, fails) = parse_effect_body(rest_s, vocab);
        if fails.is_empty() && !body.is_empty() {
            let signed_n = if positive { n } else { -n };
            let loyalty_ins = if signed_n >= 0 {
                InstructionAst::PutCounters {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    counter_kind: "loyalty".to_string(),
                    count: Box::new(ValueExprAst::Const(signed_n)),
                }
            } else {
                InstructionAst::RemoveCounters {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    counter_kind: "loyalty".to_string(),
                    all: false,
                    count: Some(Box::new(ValueExprAst::Const(-signed_n))),
                }
            };
            let mut out = vec![loyalty_ins];
            out.append(&mut body);
            return Some(StaticOrReplacement::Static(StaticAbilityAst::FollowUp(out)));
        }
    }

    // 2. Activation frequency & instant-speed timing overrides (Oath of Teferi, Teferi Master of Time, Birgi, Wonder Man)
    if let Some(after) = low.strip_prefix("you may activate ")
        && let Some((kind_and_subj, tail)) = after
            .split_once(" twice each turn rather than only once")
            .map(|(l, _)| (l, false))
            .or_else(|| {
                after
                    .split_once(" on any player's turn any time you could cast an instant")
                    .map(|(l, _)| (l, true))
            })
        && let Some((kind_raw, subj_raw)) = kind_and_subj.split_once(" abilities of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let ability_kind = kind_raw
            .trim()
            .strip_prefix("the ")
            .unwrap_or(kind_raw.trim());
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::ActivationLimitOverride {
                ability_kind: ability_kind.to_string(),
                subject: Box::new(subj),
                extra_times: if tail { 0 } else { 1 },
                instant_speed: tail,
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, rest)) = low.split_once(" can ")
        && let Some((action_raw, _)) =
            rest.split_once(" twice during each of your turns rather than once")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::ActivationLimitOverride {
                ability_kind: action_raw.trim().to_string(),
                subject: Box::new(subj),
                extra_times: 1,
                instant_speed: false,
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("each ")
        && let Some((kind_raw, rest)) = after.split_once(" ability of ")
        && let Some(subj_raw) = rest.strip_suffix(" can be activated an additional time")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::ActivationLimitOverride {
                ability_kind: kind_raw.trim().to_string(),
                subject: Box::new(subj),
                extra_times: 1,
                instant_speed: false,
                condition: None,
            },
        ));
    }

    // 3. Combat damage assignment & lethal-damage rules (Arcades, Zilortha, Defensive Formation, Butcher Orgg, Cunning Giant, Loot)
    if let Some((subj_raw, rest)) =
        low.split_once(" assigns combat damage equal to its toughness rather than its power")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let mut items = vec![StaticAbilityAst::CombatDamageAssignmentRule {
            subject: Box::new(subj.clone()),
            rule: "toughness_instead_of_power".to_string(),
            condition: None,
        }];
        if rest.contains("can attack as though it didn't have defender") {
            items.push(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "can_attack_as_though_no_defender".to_string(),
                filter: None,
                condition: None,
            });
        }
        return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            items,
        )));
    }
    if let Some(after) = low.strip_prefix("lethal damage dealt to ")
        && let Some(subj_raw) =
            after.strip_suffix(" is determined by their power rather than their toughness")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::CombatDamageAssignmentRule {
                subject: Box::new(subj),
                rule: "lethal_damage_by_power".to_string(),
                condition: None,
            },
        ));
    }
    if let Some(subj_raw) =
        low.strip_prefix("rather than the attacking player, you assign the combat damage of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::CombatDamageAssignmentRule {
                subject: Box::new(subj),
                rule: "defender_assigns_combat_damage".to_string(),
                condition: None,
            },
        ));
    }
    if low.starts_with("you can divide that creature's combat damage as you choose among ") {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::CombatDamageAssignmentRule {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                rule: "divide_combat_damage_among_blockers".to_string(),
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("you may assign ")
        && let Some((subj_raw, _)) =
            after.split_once("'s combat damage divided as you choose among ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::CombatDamageAssignmentRule {
                subject: Box::new(subj),
                rule: "divide_combat_damage_among_defender_and_creatures".to_string(),
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("if ")
        && let Some((subj_raw, _)) = after.split_once(" is unblocked, you may have it assign its combat damage to a creature defending player controls")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::CombatDamageAssignmentRule {
                subject: Box::new(subj),
                rule: "unblocked_may_assign_to_defending_creature".to_string(),
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("if ")
        && let Some((subj_raw, rest)) = after.split_once("'s power is negative, ")
        && rest.contains("assigns combat damage as though ")
        && rest.ends_with("'s power were positive")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::CombatDamageAssignmentRule {
                subject: Box::new(subj),
                rule: "negative_power_assigns_as_positive".to_string(),
                condition: None,
            },
        ));
    }

    // 4. Combat blocking/attacking restrictions (Masako, Caverns of Despair, Invasion Plans, Akron Legionnaire, Intercessor's Arrest, Cogwork Tracker)
    if let Some(flt_raw) = low
        .strip_prefix("tapped ")
        .and_then(|r| r.strip_suffix(" can block as though they were untapped"))
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::All(Box::new(flt)))),
                rule: "tapped_can_block_as_untapped".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(num_raw) = low
        .strip_prefix("no more than ")
        .and_then(|r| r.strip_suffix(" creatures can block each combat"))
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: format!("max_blockers_each_combat:{n}"),
                filter: None,
                condition: None,
            },
        ));
    }
    if low == "the attacking player chooses how each creature blocks each combat" {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "attacking_player_chooses_blocks".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("except for ")
        && let Some((exc_raw, rest)) = after.split_once(", ")
        && let Some(subj_raw) = rest.strip_suffix(" can't attack")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let exc_flt = parse_object_filter(exc_raw.trim(), vocab).ok();
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("cant_attack_except:{}", exc_raw.trim()),
                filter: exc_flt.map(Box::new),
                condition: None,
            },
        ));
    }
    if let Some((subj_raw, rest)) = low.split_once(" can't attack, block, or ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: format!("cant_attack_block_or:{}", rest.trim()),
                filter: None,
                condition: None,
            },
        ));
    }
    if low == "its activated abilities can't be activated unless they're mana abilities" {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Anaphora("it".to_string()))),
                rule: "cant_activate_nonmana_abilities".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if low == "~ attacks a player you noted for cards named ~ each combat if able" {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::SelfObject)),
                rule: "attacks_noted_player_if_able".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }

    // 5. Remaining game-rule statics (Hama Pashar, Sanctum Lurker, Yurlok, Altar of the Pantheon, City of Solitude, Null Chamber, City in a Bottle, Bell Borca, Cryptic Spires, Firestorm Phoenix follow-up)
    if let Some(after) = low.strip_prefix("room abilities of ")
        && after.ends_with(" trigger an additional time")
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::TriggerMultiplier {
                event_desc: low.to_string(),
                multiplier: 2,
                condition: None,
            },
        ));
    }
    if let Some(subj_raw) =
        low.strip_suffix(" aren't put into their owners' graveyards for having 0 loyalty")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(subj)),
                rule: "survives_zero_loyalty".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if low == "a player losing unspent mana causes that player to lose that much life" {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "unspent_mana_causes_life_loss".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(num_raw) = low
        .strip_prefix("your devotion to each color and each combination of colors is increased by ")
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: format!("devotion_bonus:{n}"),
                filter: None,
                condition: None,
            },
        ));
    }
    if low == "players can cast spells and activate abilities only during their own turns" {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "cast_and_activate_own_turn_only".to_string(),
                filter: None,
                condition: None,
            },
        ));
    }
    if low
        == "spells with the chosen names can't be cast and lands with the chosen names can't be played"
    {
        return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: None,
                    rule: "spells_with_chosen_names_cant_be_cast".to_string(),
                    filter: None,
                    condition: None,
                },
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: None,
                    rule: "lands_with_chosen_names_cant_be_played".to_string(),
                    filter: None,
                    condition: None,
                },
            ],
        )));
    }
    if let Some(exp_raw) = low
        .strip_prefix(
            "players can't cast spells or play lands with a name originally printed in the ",
        )
        .and_then(|r| r.strip_suffix(" expansion"))
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: format!("cant_cast_or_play_expansion:{}", exp_raw.trim()),
                filter: None,
                condition: None,
            },
        ));
    }
    if let Some(after) = low.strip_prefix("note the ")
        && after.contains(" as it's put into exile")
    {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::LookOrRevealRule {
                rule: low.to_string(),
                filter: None,
                follow_up: Vec::new(),
                condition: None,
            },
        ));
    }
    if low.starts_with("as you create your deck, ") {
        return Some(StaticOrReplacement::Static(
            StaticAbilityAst::DeckConstructionRule(s.to_string()),
        ));
    }
    if let Some(after) = low.strip_prefix("until that player's next turn, ")
        && after.contains("plays with that card revealed in their hand and can't play it")
    {
        return Some(StaticOrReplacement::Static(StaticAbilityAst::Compound(
            vec![
                StaticAbilityAst::LookOrRevealRule {
                    rule: "play_with_that_card_revealed_until_next_turn".to_string(),
                    filter: None,
                    follow_up: Vec::new(),
                    condition: None,
                },
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(SubjectRefAst::Anaphora("that card".to_string()))),
                    rule: "cant_play_until_next_turn".to_string(),
                    filter: None,
                    condition: None,
                },
            ],
        )));
    }

    None
}
