//! Core effect-body and atomic-effect recursive-descent parser and submodule dispatch.

pub(super) mod cast_mana;
pub(super) mod choices_misc;
pub(super) mod combat_stats;
pub(super) mod zone_ops;

use cast_mana::try_parse_cast_permission;
use choices_misc::{
    parse_comparison_condition, parse_sub_effect_extended, try_parse_misc_actions,
    try_parse_named_keyword_actions, try_parse_pass_c_primitives,
};
use combat_stats::{
    try_parse_combat_and_animate, try_parse_counters, try_parse_deal_damage, try_parse_life,
    try_parse_modify_abilities, try_parse_modify_pt,
};
use zone_ops::{
    parse_named_or_quoted_animation_fields, try_parse_discard, try_parse_exile,
    try_parse_library_ops, try_parse_sacrifice, try_parse_search_library, try_parse_zone_moves,
};

use crate::ast::{
    AltCastCostAst, ConditionAst, CostAtomAst, CountLimitAst, DelayedStepScope, DurationAst,
    ExchangeKind, FilterPredicateAst, GameMarker, InstructionAst, InteractiveChoiceKind,
    ObjectFilterAst, ParseFailureAst, PlayerRefAst, QualifiedTokenCreationAst,
    ReplacementEffectAst, StatKind, StaticAbilityAst, SubjectRefAst, TriggerEventAst,
    TriggerSpecAst, TurnStepAst, ValueExprAst, ZoneFilter,
};
use crate::clauses::costs::{parse_cost_atom, parse_cost_list, peel_leading_count};
use crate::clauses::statics::{StaticOrReplacement, parse_static_or_replacement};
use crate::clauses::triggers::parse_trigger_header;
use crate::combinators::{
    DepthGuard, anaphora, any_card_flt, box_anaphora, cond_and, cond_status, extract_quoted_spans,
    grp, ins_ability_restr, ins_add_mana, ins_animate, ins_choose_group, ins_combat_rule, ins_copy,
    ins_counters, ins_create_token, ins_delayed, ins_delayed_ev, ins_discard, ins_dmg, ins_draw,
    ins_exchange, ins_exile, ins_exile_top, ins_fe_subj, ins_for_each_count, ins_for_each_player,
    ins_grant_ab, ins_grant_cast, ins_grant_kw, ins_if, ins_life, ins_look_top, ins_manifest,
    ins_mod_pt, ins_move, ins_opt_player, ins_opt1, ins_player_counters, ins_repeat,
    ins_rm_counters, ins_sac, ins_shuffle, ins_shuffle_into, ins_temp_rep, ins_temp_static,
    ins_temp_trig, ins_unless, ok1, self_obj, some1, split_once_any, strip_prefix_any,
    strip_suffix_any, val_div, val_obj_stat, val_player_stat, vconst,
};
use crate::compile_granted_ability;
use crate::filter::{
    RichTokenCreationAst, parse_condition, parse_for_each_value_expr, parse_keyword_list,
    parse_mana_spec, parse_object_filter, parse_player_ref, parse_subject_or_target,
    parse_token_creation_rich, parse_value_expr,
};
use crate::preprocess::{
    EM_DASH, is_mana_sequence, is_physical_tabletop_clause, parse_base_pt, parse_pt_mod,
    split_sentences, split_top_level,
};
use crate::vocab::{Vocabulary, parse_number_word};

/// Parse mixed keywords + one or more quoted granted abilities on a subject
/// (e.g. `gains haste, "When ~ leaves...", and "At the beginning of your end step, sacrifice ~."`).
pub(super) fn parse_mixed_keywords_and_quoted_abilities(
    subj: &SubjectRefAst,
    rhs: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    let (before_q, spans, after_q) = extract_quoted_spans(rhs)?;
    let mut out = Vec::new();
    let lead = before_q
        .trim()
        .trim_end_matches(',')
        .trim_end_matches(" and")
        .trim_end_matches(',')
        .trim();
    if !lead.is_empty() {
        let kws = parse_keyword_list(lead, vocab)?;
        out.push(ins_grant_kw(subj.clone(), kws, dur.clone()));
    }
    let trail_low = after_q.trim().to_ascii_lowercase();
    let effective_dur = if trail_low.contains("until end of combat") {
        DurationAst::UntilEndOfCombat
    } else if trail_low.contains("until end of turn") {
        DurationAst::UntilEndOfTurn
    } else if trail_low.contains("until your next turn") {
        DurationAst::UntilYourNextTurn
    } else {
        dur.clone()
    };
    for span in spans {
        let compiled = compile_granted_ability(span, vocab)?;
        out.push(ins_grant_ab(
            subj.clone(),
            span.to_string(),
            Some(compiled),
            effective_dur.clone(),
        ));
    }
    if out.is_empty() { None } else { Some(out) }
}

// ---------------------------------------------------------------------------
// 1. COST PARSER
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

fn extract_token_quoted_abilities(
    raw: &str,
    token_name: Option<&str>,
    vocab: &Vocabulary,
) -> Option<(Vec<String>, Box<crate::ast::FaceAst>)> {
    let mut bare_keywords = Vec::new();
    let mut quoted_abilities: Vec<String> = Vec::new();
    let mut rest = raw.trim();

    while let Some(open_idx) = rest.find('"') {
        let before = rest[..open_idx].trim();
        let before_clean = before
            .trim_end_matches(" and")
            .trim_end_matches(',')
            .trim_start_matches("and ")
            .trim();
        if !before_clean.is_empty() {
            bare_keywords.extend(parse_keyword_list(before_clean, vocab)?);
        }
        let after_open = &rest[open_idx + 1..];
        let close_idx = after_open.find('"')?;
        let mut ability_txt = after_open[..close_idx].trim().to_string();
        if let Some(name) = token_name {
            ability_txt = crate::preprocess::normalize_self(&ability_txt, name, "Creature");
        }
        if !ability_txt.is_empty() {
            quoted_abilities.push(ability_txt);
        }
        rest = after_open[close_idx + 1..].trim();
    }

    let tail_clean = rest
        .trim_start_matches("and ")
        .trim_start_matches(',')
        .trim_end_matches('.')
        .trim();
    if !tail_clean.is_empty() {
        if let Some(kws) = parse_keyword_list(tail_clean, vocab) {
            bare_keywords.extend(kws);
        } else if let Some((kw, _)) = crate::keywords::parse_keyword_token(tail_clean, vocab) {
            let kw_full = match kw.parameter {
                Some(p) => format!("{} {p}", kw.name),
                None => kw.name,
            };
            bare_keywords.push(kw_full);
        } else {
            return None;
        }
    }

    if quoted_abilities.is_empty() {
        return None;
    }
    let joined = quoted_abilities.join("\n");
    let compiled = crate::compile_granted_ability(&joined, vocab)?;
    Some((bare_keywords, compiled))
}

pub(crate) fn build_create_token_instruction(
    controller: PlayerRefAst,
    mut token: crate::ast::TokenSpecAst,
    vocab: &Vocabulary,
) -> InstructionAst {
    if let crate::ast::TokenSpecAst::Creature {
        ref mut keywords,
        ref named,
        ..
    } = token
        && keywords.iter().any(|k| k.contains('"'))
        && let Some((clean_keywords, compiled_abilities)) =
            extract_token_quoted_abilities(&keywords.join(", "), named.as_deref(), vocab)
    {
        *keywords = clean_keywords;
        return InstructionAst::CreateTokenWithAbilities {
            controller,
            token,
            compiled_abilities,
        };
    }
    ins_create_token(controller, token)
}

/// Try to compile a replacement `instead` clause into typed `Vec<InstructionAst>`.
pub(crate) fn parse_instead_body(
    instead_raw: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
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
    let clow = cleaned.to_ascii_lowercase();

    // Complete quantity-modification actions (Doubling Season, Embermaw Hellion,
    // Benevolent Hydra); clauses with unsupported riders remain opaque.
    if let Some(mq) = parse_replaced_quantity_instruction(&clow, vocab) {
        return Some(vec![mq]);
    }

    // Strictly lossless CR 614 replacement action patterns: never fabricate placeholder
    // instructions that alter multipliers, steps, amounts, plural targets, or drop riders.
    // Complex replacements remain faithfully in `EventInstead::instead` with `instead_body: None`.
    None
}

fn parse_replaced_quantity_instruction(text: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
    // Match the complete replacement action. Unrecognized recipients, conditions and
    // additional actions must stay opaque rather than disappear behind a quantity tag.
    let (kind, multiplier, add, qualifier) = match text {
        "it creates twice that many of those tokens" | "create twice that many of those tokens" => {
            ("tokens", 2, None, None)
        }
        "it creates three times that many of those tokens"
        | "create three times that many of those tokens" => ("tokens", 3, None, None),
        "it puts twice that many of those counters on that permanent" => {
            ("counters", 2, None, None)
        }
        "twice that many +1/+1 counters are put on that creature"
        | "twice that many +1/+1 counters are put on it" => ("counters", 2, None, Some("+1/+1")),
        "that many plus one +1/+1 counters are put on it" => {
            ("counters", 1, Some(ValueExprAst::Const(1)), Some("+1/+1"))
        }
        "it deals double that damage" | "it deals twice that much damage" => {
            ("damage", 2, None, None)
        }
        "it deals triple that damage" | "it deals three times that much damage" => {
            ("damage", 3, None, None)
        }
        "you gain twice that much life" | "that player gains twice that much life" => {
            ("life", 2, None, None)
        }
        _ => {
            let amount = text
                .strip_prefix("it deals that much damage plus ")?
                .strip_suffix(" to that permanent or player")?;
            (
                "damage",
                1,
                Some(parse_value_expr(amount, vocab).ok()?),
                None,
            )
        }
    };
    Some(InstructionAst::ModifyReplacedQuantity {
        quantity_kind: kind.to_string(),
        multiplier,
        add: add.map(Box::new),
        qualifier: qualifier.map(str::to_string),
    })
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
    let low = s.to_ascii_lowercase();

    // 0. Global trailing ", where X is <ValueExpr>" binder (quote-aware)
    if let Some(idx) = find_top_level_pattern(s, ", where x is ")
        && let Ok(x_val) = parse_value_expr(&s[idx + ", where x is ".len()..], vocab)
        && let Ok(inner) = parse_atomic_effect(s[..idx].trim(), vocab)
    {
        return ok1(InstructionAst::BindX {
            x_expr: Box::new(x_val),
            body: inner,
        });
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
                return ok1(InstructionAst::SpreeMode {
                    cost,
                    body: vec![InstructionAst::AnimateOrSetBasePT {
                        subject: box_anaphora("target creature"),
                        power: p,
                        toughness: t,
                        description: None,
                        keywords: Vec::new(),
                        duration: DurationAst::UntilEndOfTurn,
                    }],
                });
            }
        }
    }

    // 0c. Leading "instead <effect>" or trailing "<effect> instead" (Green Sun's Twilight, Gideon's Triumph)
    if let Some(after_inst) = low.strip_prefix("instead ")
        && !after_inst.is_empty()
        && let Ok(inner) = parse_atomic_effect(&s["instead ".len()..], vocab)
    {
        return Ok(vec![InstructionAst::ReplaceEffect(inner)]);
    }
    if let Some(before_inst) = low.strip_suffix(" instead")
        && !before_inst.is_empty()
        && !low.starts_with("if ")
        && !low.starts_with("when ")
        && !low.starts_with("whenever ")
        && !low.starts_with("choose ")
        && !low.contains('\u{2014}')
        && !before_inst.contains(" would ")
        && let Ok(inner) = parse_atomic_effect(&s[..before_inst.len()], vocab)
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
        return ok1(ins_temp_static(
            StaticAbilityAst::AlternativeCastCost {
                cost,
                condition: None,
                for_spells: None,
                next_only: false,
            },
            DurationAst::Instant,
        ));
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
    if low.starts_with("otherwise, ") {
        let rest = s["otherwise, ".len()..].trim();
        if !rest.is_empty()
            && let Ok(inner) = parse_atomic_effect(rest, vocab)
        {
            return Ok(vec![InstructionAst::Otherwise(inner)]);
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
                return ok1(ins_opt1(ins_temp_static(st, DurationAst::UntilEndOfTurn)));
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
    const EXACT_RESTR_MAP: &[(&str, &str)] = &[
        ("activate only as a sorcery", "Sorcery"),
        (
            "activate only as a sorcery and only once each turn",
            "SorceryOnceEachTurn",
        ),
        (
            "activate only once each turn and only as a sorcery",
            "SorceryOnceEachTurn",
        ),
        (
            "activate only during your turn and only once each turn",
            "DuringYourTurnOnceEachTurn",
        ),
        (
            "activate only once each turn and only during your turn",
            "DuringYourTurnOnceEachTurn",
        ),
        ("activate only as an instant", "Instant"),
        (
            "activate no more than twice each turn",
            "AtMost2TimesEachTurn",
        ),
        (
            "activate no more than three times each turn",
            "AtMost3TimesEachTurn",
        ),
        ("activate only during combat", "DuringCombat"),
        (
            "activate only during the declare attackers step",
            "DuringDeclareAttackersStep",
        ),
        (
            "activate only during the declare blockers step",
            "DuringDeclareBlockersStep",
        ),
        ("activate only once each turn", "OnceEachTurn"),
        ("activate only once", "Once"),
        ("activate only during your turn", "DuringYourTurn"),
        (
            "activate only during your turn, before attackers are declared",
            "OwnTurnBeforeAttackers",
        ),
        ("activate only during your upkeep", "DuringUpkeep"),
        ("any player may activate this ability", "AnyPlayer:"),
        (
            "this ability triggers only once each turn",
            "TriggersOnceEachTurn",
        ),
        ("do this only once each turn", "TriggersOnceEachTurn"),
        ("this ability triggers only once", "TriggersOnce"),
        ("it becomes plotted", "BecomesPlotted"),
        ("repeat this process", "RepeatProcess"),
        (
            "~ loses this ability and becomes an aura enchantment with enchant creature",
            "LicidBecomeAura",
        ),
        ("the damage can't be prevented", "DamageCantBePrevented"),
        (
            "damage can't be prevented this turn",
            "DamageCantBePreventedThisTurn",
        ),
        (
            "excess damage is dealt to that creature's controller instead",
            "ExcessDamageToController",
        ),
        (
            "excess damage is dealt to that creature's controller",
            "ExcessDamageToController",
        ),
        (
            "each opponent attacking that player does the same",
            "EachAttackingOpponentRepeats",
        ),
        (
            "each copy targets a different one of those creatures",
            "EachCopyDifferentTarget",
        ),
        (
            "starting with you, each player may pay any amount of mana",
            "JoinForcesPayMana",
        ),
        (
            "each mode must target a different player",
            "EachModeDifferentPlayer",
        ),
        (
            "the flashback cost is equal to its mana cost",
            "FlashbackEqualManaCost",
        ),
        (
            "you have no maximum hand size for the rest of the game",
            "NoMaxHandSizeRestOfGame",
        ),
        ("x can't be 0", "XCantBeZero"),
    ];
    if let Some((_, tag)) = EXACT_RESTR_MAP.iter().find(|&&(k, _)| k == low) {
        return ok1(ins_ability_restr(*tag));
    }
    const VERBATIM_RESTR_LIST: &[&str] = &[
        "activate only during your upkeep and only once each turn",
        "any player may activate this ability but only as a sorcery",
        "only your opponents may activate this ability",
        "only your opponents may activate this ability and only as a sorcery",
        "round up each time",
        "round down each time",
        "do all of the above",
        "proliferate again",
        "the game is a draw",
        "the token enters tapped and attacking",
        "the token enters tapped and attacking that player",
        "the tokens are goaded for the rest of the game",
        "the copy targets that token",
        "the copy targets ~",
        "end the combat phase",
        "roll the planar die",
        "it enters tapped",
        "they're an artifact",
        "you recruit",
        "remove all of them",
        "you choose two of them",
        "target opponent chooses one",
        "an opponent chooses one of the exiled cards",
        "choose a permanent type",
        "choose a land card name",
        "choose a creature at random",
        "reverse the game's turn order",
        "reselect its target at random",
        "investigate an additional time",
        "those choices are revealed",
        "pay that much life again",
        "discover again for the same value",
        "permanents enter tapped this turn",
        "choose new targets for it",
        "choose a new target for it",
        "choose an artifact card name",
        "skip the untap step of that turn",
        "manifest a card from your hand",
        "reveal a card from your hand",
        "look at target face-down creature",
        "it isn't an equipment",
        "it gains ~'s other abilities",
        "counter all abilities",
        "counter target loyalty ability of a planeswalker",
        "that player can't gain life for the rest of the game",
    ];
    if VERBATIM_RESTR_LIST.contains(&low.as_str()) {
        return ok1(ins_ability_restr(low));
    }
    if let Some(count_raw) = low
        .strip_prefix("activate no more than ")
        .and_then(|rest| rest.strip_suffix(" times each turn"))
        && let Some(count) = parse_number_word(count_raw.trim())
        && count >= 2
    {
        return ok1(ins_ability_restr(format!("AtMost{count}TimesEachTurn")));
    }
    match low.as_str() {
        "choose friend or foe" => return ok1(InstructionAst::ChooseOption("friend or foe".into())),
        "choose one of them" => return ok1(InstructionAst::ChooseOption("one of them".into())),
        "you may choose both instead" => {
            return ok1(InstructionAst::ChooseOption("choose_both_instead".into()));
        }
        "choose left or right"
        | "choose odd or even"
        | "an opponent chooses one of them"
        | "target opponent chooses one of them"
        | "an opponent chooses two of those cards"
        | "an opponent chooses one of those cards"
        | "choose both instead"
        | "choose any number"
        | "choose two"
        | "choose any number instead"
        | "choose two instead"
        | "you choose one of them"
        | "target opponent chooses self or others"
        | "an opponent chooses self or others" => return ok1(InstructionAst::ChooseOption(low)),
        "an opponent chooses one \u{2014}" | "target opponent chooses one \u{2014}" => {
            return ok1(InstructionAst::Modal {
                choice: "opponent_one".into(),
                modes: Vec::new(),
            });
        }
        "choose one at random \u{2014}" | "choose one at random" => {
            return ok1(InstructionAst::Modal {
                choice: "one_at_random".into(),
                modes: Vec::new(),
            });
        }
        "you choose one instead" => return ok1(InstructionAst::ModalModifier(low)),
        "you may choose two" | "you may choose two \u{2014}" => {
            return ok1(ins_opt1(InstructionAst::Modal {
                choice: "two".into(),
                modes: Vec::new(),
            }));
        }
        "when you remove a counter this way, choose one \u{2014}" => {
            return ok1(InstructionAst::ReflexiveTrigger(vec![
                InstructionAst::Modal {
                    choice: "one".into(),
                    modes: Vec::new(),
                },
            ]));
        }
        "choose any number \u{2014}" => {
            return ok1(InstructionAst::Modal {
                choice: "any_number".into(),
                modes: Vec::new(),
            });
        }
        "chaos ensues" => return ok1(InstructionAst::ChaosEnsues),
        "they may copy ~ and may choose a new target for that copy" => {
            return Ok(vec![
                ins_opt1(ins_copy(SubjectRefAst::SelfObject, None, None)),
                InstructionAst::MayChooseNewTargetsForCopy,
            ]);
        }
        "its owner shuffles it into their library" => return ok1(ins_shuffle_into(anaphora("it"))),
        "head to askurza" => {
            return ok1(InstructionAst::ManualTabletopAction {
                prompt: s.to_string(),
            });
        }
        "end the turn" => return ok1(InstructionAst::EndTurn),
        "you skip your next untap step" => {
            return ok1(InstructionAst::SkipNextStep {
                player: PlayerRefAst::You,
                step: TurnStepAst::UntapStep,
            });
        }
        "you skip your draw step this turn" | "you skip your next draw step" => {
            return ok1(InstructionAst::SkipNextStep {
                player: PlayerRefAst::You,
                step: TurnStepAst::DrawStep,
            });
        }
        "exchange your hand and graveyard"
        | "exchange your hand and library"
        | "exchange your graveyard and library" => {
            return ok1(ins_exchange(
                ExchangeKind::Zones,
                SubjectRefAst::Player(PlayerRefAst::You),
                None,
            ));
        }
        "double your life total" => {
            return ok1(ins_life(
                PlayerRefAst::You,
                val_player_stat(PlayerRefAst::You, crate::ast::PlayerStatKind::LifeTotal),
                true,
            ));
        }
        "add one mana of any of the exiled card's colors" => {
            return ok1(ins_add_mana(
                PlayerRefAst::You,
                crate::ast::ManaSpecAst::AnyColor {
                    amount: vconst(1),
                    single_color: true,
                },
            ));
        }
        "pay any amount of mana" => {
            return ok1(InstructionAst::PayAnyAmountOfMana {
                player: PlayerRefAst::You,
            });
        }
        "you skip your next two turns" => {
            return ok1(InstructionAst::SkipNextTurns {
                player: PlayerRefAst::You,
                count: 2,
            });
        }
        "you skip your next turn" | "skip your next turn" => {
            return ok1(InstructionAst::SkipNextTurn {
                player: PlayerRefAst::You,
            });
        }
        "a creature destroyed this way can't be regenerated"
        | "a creature dealt damage this way can't be regenerated this turn"
        | "creatures dealt damage this way can't be regenerated this turn"
        | "it can't be regenerated"
        | "they can't be regenerated" => {
            return ok1(InstructionAst::ModifyPreviousDestroyNoRegen);
        }
        "an opponent separates those cards into two piles" => {
            return ok1(InstructionAst::SeparateIntoPiles {
                chooser: PlayerRefAst::AnyOpponent,
                pile_count: 2,
                face_down_piles: 0,
            });
        }
        "an opponent chooses one of those piles" => {
            return ok1(InstructionAst::ChoosePile {
                chooser: PlayerRefAst::AnyOpponent,
            });
        }
        "you choose one of those piles" => {
            return ok1(InstructionAst::ChoosePile {
                chooser: PlayerRefAst::You,
            });
        }
        "that player adds one mana of any type that land produced"
        | "add one mana of any type that land produced"
        | "you add one mana of any type that land produced" => {
            return ok1(InstructionAst::AddManaLandProduced {
                player: if low.starts_with("that player") {
                    PlayerRefAst::ThatPlayer
                } else {
                    PlayerRefAst::You
                },
                count: None,
            });
        }
        "flip a coin until you lose a flip" | "flip a coin" => {
            return ok1(InstructionAst::FlipCoin);
        }
        "they're 2/2 cyberman artifact creatures" => {
            return ok1(InstructionAst::KeepExistingTypes(low));
        }
        "you choose a nonland card from that player's graveyard or hand and exile it" => {
            return ok1(ins_exile(
                Some(PlayerRefAst::You),
                anaphora("chosen_nonland_from_gy_or_hand"),
            ));
        }
        "look at the top card of that player's library, then exile it face down" => {
            return Ok(vec![
                ins_look_top(false, "that player", ValueExprAst::Const(1)),
                ins_exile_top(
                    Some(PlayerRefAst::You),
                    "that player",
                    ValueExprAst::Const(1),
                    true,
                ),
            ]);
        }
        "discard all the cards in your hand, then draw that many cards" => {
            return Ok(vec![
                InstructionAst::DiscardHand {
                    player: PlayerRefAst::You,
                },
                ins_draw(PlayerRefAst::You, ValueExprAst::PriorResult),
            ]);
        }
        "reveal those cards, put them into your hand, then shuffle" => {
            return Ok(vec![
                ins_look_top(true, "ref", ValueExprAst::Const(1)),
                ins_move(anaphora("them"), "your hand"),
                ins_shuffle(PlayerRefAst::You),
            ]);
        }
        "convert ~" => {
            return ok1(InstructionAst::Transform {
                subject: self_obj(),
            });
        }
        "it assembles a contraption" | "~ assembles a contraption" => {
            return ok1(InstructionAst::AssembleContraption);
        }
        "copy it for each time you've cast your commander from the command zone this game" => {
            return ok1(InstructionAst::CopyForCommanderCasts);
        }
        "you may choose the same mode more than once" => {
            return ok1(InstructionAst::AllowRepeatModes);
        }
        "abandon ~" | "abandon this scheme" => return ok1(InstructionAst::AbandonSourceScheme),
        "roll a d20" => return ok1(InstructionAst::RollDie { sides: 20 }),
        "roll a six-sided die" | "roll a d6" => return ok1(InstructionAst::RollDie { sides: 6 }),
        "roll two six-sided dice" => {
            return Ok(vec![
                InstructionAst::RollDie { sides: 6 },
                InstructionAst::RollDie { sides: 6 },
            ]);
        }
        "proliferate" => return ok1(InstructionAst::Proliferate),
        "shuffle" | "shuffle your library" | "then shuffle" => {
            return ok1(ins_shuffle(PlayerRefAst::You));
        }
        "that player shuffles" | "that player shuffles their library" => {
            return ok1(ins_shuffle(PlayerRefAst::ThatPlayer));
        }
        "each player who searched their library this way shuffles"
        | "each player who searched their library this way shuffles their library" => {
            return ok1(ins_shuffle(PlayerRefAst::EachPlayer));
        }
        "take an extra turn after this one" | "you take an extra turn after this one" => {
            return ok1(InstructionAst::ExtraTurn {
                player: PlayerRefAst::You,
            });
        }
        "recruit" => return ok1(InstructionAst::Recruit),
        "put them back in any order" | "put them back on top of your library in any order" => {
            return ok1(InstructionAst::ReorderTopLibrary);
        }
        "you win the game" => {
            return ok1(InstructionAst::WinGame {
                player: PlayerRefAst::You,
            });
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
    if let Some(who_raw) = strip_suffix_any(&low, &[" wins the game", " win the game"])
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Ok(vec![InstructionAst::WinGame { player }]);
    }
    if let Some(after) = low.strip_prefix("exile ")
        && let Some((subj_raw, rest)) = after.split_once(" instead of putting it into ")
        && rest.ends_with(" graveyard as it resolves")
        && matches!(subj_raw.trim(), "that spell" | "it" | "~")
    {
        return ok1(InstructionAst::OneShotReplacement {
            subject: "that spell".to_string(),
            event: "put into graveyard as it resolves".to_string(),
            replace_with: "exile".to_string(),
            instead_body: Some(vec![ins_exile(None, anaphora("that spell"))]),
        });
    }
    if let Some(after) = low.strip_prefix("double the number of each kind of counter ")
        && (after == "you have" || after.ends_with(" has") || after.ends_with(" have"))
    {
        return ok1(InstructionAst::DoubleStatOrCounters {
            subject: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
            kind: "all_counters".to_string(),
            duration: DurationAst::Instant,
        });
    }
    if let Some(sides_str) = low.strip_prefix("roll a d") {
        let sides_part = split_once_any(&sides_str, &[" and add ", " and subtract "])
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
            ins_look_top(true, "your", count),
            InstructionAst::SeparateIntoPiles {
                chooser: PlayerRefAst::You,
                pile_count: 2,
                face_down_piles: 0,
            },
        ]);
    }
    if let Some(rest) = low.strip_prefix("reveal any number of ")
        && parse_object_filter(rest, vocab).is_ok()
    {
        return Ok(vec![ins_look_top(true, "hand", ValueExprAst::ChosenX)]);
    }
    if low.starts_with("cast ~ only ") {
        if let Some(cond_raw) = low.strip_prefix("cast ~ only if ")
            && let Ok(cond) = parse_condition(cond_raw, vocab)
        {
            return ok1(InstructionAst::AbilityRestriction {
                restriction: "Condition".to_string(),
                condition: Some(Box::new(cond)),
            });
        }
        return Ok(vec![ins_ability_restr(s.to_string())]);
    }
    if let Some(rest) = strip_prefix_any(
        &low,
        &[
            "activate only ",
            "activate this ability only ",
            "any player may activate this ability but only ",
            "any player may activate this ability, but only ",
        ],
    ) {
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
            } else if let Some(canon) = match p {
                "as a sorcery" => Some("sorcery"),
                "as an instant" => Some("instant"),
                "once each turn" => Some("OnceEachTurn"),
                "during your turn" => Some("DuringYourTurn"),
                "during an opponent's turn" | "during opponents' turns" => {
                    Some("DuringOpponentsTurn")
                }
                "during your upkeep" => Some("DuringUpkeep"),
                "during your turn, before attackers are declared" => Some("OwnTurnBeforeAttackers"),
                "once"
                | "once each combat"
                | "during their turn"
                | "during combat"
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
                | "during your turn, before the combat damage step"
                | "during combat before blockers are declared"
                | "during combat after blockers are declared"
                | "while ~ is on the battlefield"
                | "while ~ is in your graveyard" => Some(p),
                _ => None,
            } {
                timing_parts.push(canon);
            } else {
                all_ok = false;
                break;
            }
        }
        if all_ok && (!timing_parts.is_empty() || !cond_parts.is_empty()) {
            let combined_cond = cond_parts.into_iter().reduce(cond_and);
            let label = if low.starts_with("any player may activate") {
                format!("AnyPlayer:{}", timing_parts.join("+"))
            } else if timing_parts.is_empty() {
                "Condition".to_string()
            } else {
                timing_parts.join("+")
            };
            return ok1(InstructionAst::AbilityRestriction {
                restriction: label,
                condition: combined_cond.map(Box::new),
            });
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
        return ok1(InstructionAst::AbilityCostReduction {
            delta: delta_tok.to_string(),
            per: Box::new(flt),
        });
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
        return ok1(InstructionAst::RedirectDamage {
            amount: Box::new(amt),
            from: Box::new(from),
            to: Box::new(to),
            source_choice: None,
            duration: None,
        });
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
            return ok1(InstructionAst::RedirectDamage {
                amount: Box::new(ValueExprAst::PriorResult),
                from: Box::new(from),
                to: Box::new(to),
                source_choice: None,
                duration: None,
            });
        }
    }
    // "Flip N coins" (Ral Zarek, Krark's Thumb, etc.)
    if let Some(cnt_raw) = low
        .strip_prefix("flip ")
        .and_then(|r| r.strip_suffix(" coins"))
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return ok1(ins_for_each_count(cnt, vec![InstructionAst::FlipCoin]));
    }
    // "<player> skips their next [N/X] turn(s) / draw/untap/combat step" (Fatigue, Brine Elemental)
    if let Some((who_raw, rest)) =
        split_once_any(&low, &[" skips their next ", " skip their next "])
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
    if let Some((who_raw, rest)) = split_once_any(&low, &[" loses all ", " lose all "])
        && let Some(kind) = rest.strip_suffix(" counters")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return ok1(ins_rm_counters(
            SubjectRefAst::Player(player),
            kind.trim().to_string(),
            true,
            None,
        ));
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
            return ok1(ins_temp_static(
                StaticAbilityAst::CostModifier {
                    subject: Box::new(grp(flt)),
                    delta: delta_tok.to_string(),
                    less: cost_tail.to_ascii_lowercase().starts_with("less"),
                    next_only: true,
                    per: None,
                    scale_expr: None,
                    condition: None,
                },
                DurationAst::UntilEndOfTurn,
            ));
        }
        if let Some(kw_raw) = tail.strip_prefix("has ")
            && (kw_raw.starts_with("affinity for ") || parse_keyword_list(kw_raw, vocab).is_some())
        {
            let kws = parse_keyword_list(kw_raw, vocab).unwrap_or_else(|| vec![kw_raw.to_string()]);
            return ok1(ins_temp_static(
                StaticAbilityAst::NextSpellModifier {
                    filter: Box::new(flt.clone()),
                    effect: Box::new(StaticAbilityAst::GrantKeywords {
                        subject: Box::new(grp(flt)),
                        keywords: kws,
                        condition: None,
                        paired_rule: None,
                    }),
                },
                DurationAst::UntilEndOfTurn,
            ));
        }
        if tail == "can be cast as though it had flash"
            || tail == "can be cast without paying its mana cost"
            || tail == "can't be countered"
        {
            return ok1(ins_temp_static(
                StaticAbilityAst::NextSpellModifier {
                    filter: Box::new(flt.clone()),
                    effect: Box::new(StaticAbilityAst::PlayOrCastPermission {
                        cast_only: true,
                        filter: Box::new(flt),
                        permission: tail.to_string(),
                        condition: None,
                    }),
                },
                DurationAst::UntilEndOfTurn,
            ));
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
        return ok1(ins_temp_static(
            StaticAbilityAst::CostModifier {
                subject: Box::new(SubjectRefAst::Anaphora(left_part.to_string())),
                delta: delta_tok.to_string(),
                less: cost_tail.to_ascii_lowercase().starts_with("less"),
                next_only: false,
                per: None,
                scale_expr: None,
                condition: None,
            },
            DurationAst::Permanent,
        ));
    }
    if let Some(after) = low.strip_prefix("copy the next ")
        && let Some(spell_part) = after.strip_suffix(" you cast this turn when you cast it")
        && let Ok(flt) = parse_object_filter(spell_part, vocab)
    {
        return ok1(ins_delayed_ev(
            TriggerSpecAst {
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
            vec![InstructionAst::Copy {
                subject: box_anaphora("that spell"),
                exception: None,
                count: None,
            }],
        ));
    }
    // Note / "<player> chooses a <characteristic/filter>"
    if let Some(note_rest) = low.strip_prefix("note ")
        && !note_rest.is_empty()
    {
        return ok1(InstructionAst::ChooseOption(format!("note:{note_rest}")));
    }
    if let Some((who_raw, choose_rest)) = split_once_any(&low, &[" chooses ", " choose "])
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
            return ok1(InstructionAst::PlayerChooseOption {
                chooser,
                options: vec![choose_rest.to_string()],
            });
        }
        if parse_object_filter(
            strip_prefix_any(&choose_rest, &["any number of ", "up to ", "exactly "])
                .unwrap_or(choose_rest),
            vocab,
        )
        .is_ok()
        {
            return ok1(InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser,
                    description: choose_rest.to_string(),
                },
            ));
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
            return ok1(InstructionAst::ForEachTargetPlayer {
                player,
                any_number: true,
                body: inner,
            });
        }
    }

    // 2. Modal header: "Choose one U+2014" & standalone "Choose a <characteristic/group>"
    if let Some(after) = low.strip_prefix("choose ") {
        let trimmed = after.trim_end_matches(EM_DASH).trim();
        if trimmed.ends_with(" instead")
            && (trimmed.starts_with("one")
                || trimmed.starts_with("two")
                || trimmed.starts_with("three")
                || trimmed.starts_with("four")
                || trimmed.starts_with("both")
                || trimmed.starts_with("all")
                || trimmed.starts_with("up to "))
        {
            return Ok(vec![InstructionAst::ModalModifier(low)]);
        }
        let has_modal_dash = after.trim_end().ends_with(EM_DASH);
        let modal_core = trimmed.strip_prefix("up to ").unwrap_or(trimmed);
        if has_modal_dash
            || modal_core.ends_with(" {p} worth of modes")
            || matches!(
                modal_core,
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
            )
        {
            return ok1(InstructionAst::Modal {
                choice: trimmed.to_string(),
                modes: Vec::new(),
            });
        }
        if !trimmed.contains(", then ")
            && let Some(up_to_rest) = trimmed.strip_prefix("up to ")
        {
            let (cnt, grp_raw) = peel_leading_count(up_to_rest, vocab);
            let grp_clean = grp_raw.trim();
            if !trimmed.contains("target ") {
                if let Ok(flt) = parse_object_filter(grp_clean, vocab) {
                    return ok1(ins_choose_group(cnt, false, grp(flt)));
                }
                if let Ok(subj) = parse_subject_or_target(grp_clean, vocab) {
                    return Ok(vec![ins_choose_group(cnt, false, subj)]);
                }
            }
            if let Ok(subj) = parse_subject_or_target(trimmed, vocab) {
                let derived_cnt = match &subj {
                    SubjectRefAst::TargetObject { count: tc, .. }
                    | SubjectRefAst::TargetPlayer { count: tc, .. }
                    | SubjectRefAst::TargetOneOf { count: tc, .. } => match tc {
                        crate::ast::TargetCountAst::Exact(n)
                        | crate::ast::TargetCountAst::UpTo(n) => ValueExprAst::Const(i32::from(*n)),
                        crate::ast::TargetCountAst::ChosenX | crate::ast::TargetCountAst::UpToX => {
                            ValueExprAst::ChosenX
                        }
                        crate::ast::TargetCountAst::AnyNumber => cnt,
                    },
                    _ => cnt,
                };
                return Ok(vec![ins_choose_group(derived_cnt, false, subj)]);
            }
            if let Ok(subj) = parse_subject_or_target(grp_clean, vocab) {
                return Ok(vec![ins_choose_group(cnt, false, subj)]);
            }
            return ok1(InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser: PlayerRefAst::You,
                    description: trimmed.to_string(),
                },
            ));
        }
        if matches!(
            trimmed,
            "a creature type"
                | "a planeswalker type"
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
            return ok1(InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser: PlayerRefAst::You,
                    description: trimmed.to_string(),
                },
            ));
        }
        if trimmed.starts_with("target ")
            && let Ok(subj) = parse_subject_or_target(trimmed, vocab)
            && matches!(
                &subj,
                SubjectRefAst::TargetObject { filter, .. }
                    if filter.predicates.iter().any(|p| matches!(
                        p,
                        crate::ast::FilterPredicateAst::PhasedOutBySource { .. }
                    ))
            )
        {
            return Ok(vec![ins_choose_group(ValueExprAst::Const(1), false, subj)]);
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
            if trimmed == "target opponent" {
                return ok1(InstructionAst::ChoosePlayerTarget {
                    player: PlayerRefAst::TargetOpponent,
                });
            }
            if trimmed == "target player" {
                return ok1(InstructionAst::ChoosePlayerTarget {
                    player: PlayerRefAst::TargetPlayer,
                });
            }
            return Ok(vec![InstructionAst::ChooseOption(trimmed.to_string())]);
        }
    }
    if let Some(after) = low.strip_prefix("choose ")
        && let Some((cnt_part, hand_rest)) = after.split_once(" in your hand drawn this turn")
        && hand_rest.trim().is_empty()
    {
        let (cnt, flt_raw) = peel_leading_count(cnt_part.trim(), vocab);
        if let Ok(mut flt) = parse_object_filter(flt_raw, vocab) {
            flt.zone = Some(ZoneFilter::YourHand);
            flt.predicates.push(FilterPredicateAst::StateQualification(
                "drawn_this_turn".to_string(),
            ));
            return ok1(ins_choose_group(cnt, false, grp(flt)));
        }
    }
    if let Some(after) = low.strip_prefix("you choose ")
        && (after.ends_with(" from it") || after.ends_with(" from them"))
    {
        return Ok(vec![InstructionAst::ChooseOption(after.to_string())]);
    }

    // Emblem (including trailing sentence after closing quote, e.g. Kiora, AND non-You / filtered player recipients like Chandra, Roaring Flame)
    if let Some(rest) = s.strip_prefix("You get an emblem with \"") {
        let mk_emblem = |inner: &str| {
            let compiled = compile_granted_ability(inner, vocab).or_else(|| {
                if inner.contains("\" and \"") {
                    compile_granted_ability(&inner.replace("\" and \"", "\n"), vocab)
                } else {
                    None
                }
            });
            InstructionAst::CreateEmblemFor {
                recipient: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
                text: inner.to_string(),
                compiled,
            }
        };
        if let Some(inner) = rest.strip_suffix('"') {
            return Ok(vec![mk_emblem(inner)]);
        }
        if let Some((inner, after_q)) = rest.split_once('"') {
            let after_t = after_q.trim();
            if !after_t.is_empty() {
                let (mut more, fails) = parse_effect_body(after_t, vocab);
                if fails.is_empty() && !more.is_empty() {
                    let mut out = vec![mk_emblem(inner)];
                    out.append(&mut more);
                    return Ok(out);
                }
            }
        }
    }
    if let Some((who_raw, rest)) =
        split_once_any(&s, &[" gets an emblem with \"", " get an emblem with \""])
        && let Ok(recipient) = parse_subject_or_target(who_raw.trim(), vocab)
    {
        let compiled_emblem = |inner: &str| InstructionAst::CreateEmblemFor {
            recipient: Box::new(recipient.clone()),
            text: inner.to_string(),
            compiled: compile_granted_ability(inner, vocab),
        };
        if let Some(inner) = rest.strip_suffix('"') {
            return Ok(vec![compiled_emblem(inner)]);
        }
        if let Some((inner, after_q)) = rest.split_once('"') {
            let after_t = after_q.trim();
            if !after_t.is_empty() {
                let (mut more, fails) = parse_effect_body(after_t, vocab);
                if fails.is_empty() && !more.is_empty() {
                    let mut out = vec![compiled_emblem(inner)];
                    out.append(&mut more);
                    return Ok(out);
                }
            }
        }
    }

    // Quoted granted ability
    if s.contains('"')
        && let Some((left, spans, trailing)) = extract_quoted_spans(s)
    {
        let llow = left.trim().to_ascii_lowercase();
        let subj_raw = strip_suffix_any(
            &llow,
            &[
                " each have",
                " each gain",
                " has",
                " have",
                " gains",
                " gain",
            ],
        );
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
            let mut out = Vec::with_capacity(spans.len());
            for ability_text in spans {
                let compiled = compile_granted_ability(ability_text, vocab);
                out.push(ins_grant_ab(
                    subj.clone(),
                    ability_text.to_string(),
                    compiled,
                    dur.clone(),
                ));
            }
            return Ok(out);
        }
    }

    // One-shot damage/event replacement: "The next time <event>, <replacement>"
    if let Some(after) = low.strip_prefix("the next time ")
        && let Some((event_desc, rep_desc)) = after.split_once(" this turn, ")
    {
        let rep = rep_desc.trim();
        let instead_body = parse_instead_body(rep, vocab);
        return ok1(InstructionAst::OneShotReplacement {
            subject: event_desc.trim().to_string(),
            event: "next_time_this_turn".to_string(),
            replace_with: rep.to_string(),
            instead_body,
        });
    }

    // One-shot replacement effects in spells/abilities
    if low
        == "if that spell is countered this way, exile it instead of putting it into its owner's graveyard"
        || low
            == "if that spell is countered this way, put it on top of its owner's library instead of into that player's graveyard"
    {
        return ok1(InstructionAst::OneShotReplacement {
            subject: "that spell".to_string(),
            event: "countered".to_string(),
            replace_with: if low.contains("exile it") {
                "exile".to_string()
            } else {
                "top_of_library".to_string()
            },
            instead_body: Some(vec![if low.contains("exile it") {
                ins_exile(None, anaphora("that spell"))
            } else {
                ins_move(anaphora("that spell"), "top of its owner's library")
            }]),
        });
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
        let exile_anaphora = if rest.contains(", exile them ") {
            "them"
        } else if rest.contains(", exile that card ") {
            "that card"
        } else {
            "it"
        };
        return ok1(InstructionAst::OneShotReplacement {
            subject: subj_raw.trim().to_string(),
            event: ev.to_string(),
            replace_with: "exile".to_string(),
            instead_body: Some(vec![ins_exile(
                None,
                SubjectRefAst::Anaphora(exile_anaphora.to_string()),
            )]),
        });
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
            return ok1(InstructionAst::OneShotReplacement {
                subject: subj_raw.trim().to_string(),
                event: ev_part.trim().to_string(),
                replace_with: inst_clean.trim().to_string(),
                instead_body,
            });
        }
    }

    // Delayed event trigger inside effect body: "When you next cast ..., <effect>" / "Whenever <event>, <effect>"
    if (low.starts_with("when ") || low.starts_with("whenever "))
        && !low.starts_with("when you do, ")
        && let Some((hdr, body_part)) = s.split_once(", ")
        && let Ok(trigger) = parse_trigger_header(hdr, vocab)
        && let Ok(inner) = parse_atomic_effect(body_part, vocab)
    {
        return Ok(vec![ins_delayed_ev(trigger, inner)]);
    }

    // Voting: "[Label U+2014] Starting with you, each player votes for <options>"
    if let Some((_, vote_rest)) = low.split_once("starting with you, each player votes for ") {
        return ok1(InstructionAst::Vote {
            options: vote_rest.trim().to_string(),
        });
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
            "at the beginning of each of your upkeeps for the rest of the game, ",
            TurnStepAst::Upkeep,
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
            return Ok(vec![ins_delayed(step, scope, inner)]);
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
                return Ok(vec![ins_delayed(step, scope, inner)]);
            }
        }
        if let Some(before_ltb) = low.strip_suffix(" when ~ leaves the battlefield")
            && let Ok(inner) = parse_atomic_effect(&s[..before_ltb.len()], vocab)
        {
            return ok1(ins_delayed_ev(
                TriggerSpecAst {
                    events: vec![TriggerEventAst::ZoneChange {
                        from: Some("battlefield".to_string()),
                        to: None,
                        subject: self_obj(),
                        grouped: false,
                        entering_controller: None,
                    }],
                    intervening_if: None,
                },
                inner,
            ));
        }
    }
    // "it becomes equal to <val>" (Resolute Archangel) & "you may activate [the] loyalty abilities of <subj> twice this turn rather than only once" (Kaito, Urza Assembles the Titans)
    if let Some(val_raw) = low.strip_prefix("it becomes equal to ")
        && let Ok(value) = parse_value_expr(val_raw, vocab)
    {
        return ok1(InstructionAst::SetLifeTotal {
            player: PlayerRefAst::You,
            value: Box::new(value),
        });
    }
    if low.starts_with("you may activate ")
        && let Some((_, after_lo)) = low.split_once("loyalty abilities of ")
        && let Some(subj_raw) = after_lo.strip_suffix(" twice this turn rather than only once")
    {
        let subject = parse_subject_or_target(subj_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
        return ok1(ins_combat_rule(
            subject,
            "activate_loyalty_twice",
            DurationAst::UntilEndOfTurn,
        ));
    }
    if let Some(after_fe) = low.strip_prefix("for each opponent with ")
        && let Some((cond_tail, body_raw)) = after_fe.split_once(", ")
        && (cond_tail.contains(" counter") || cond_tail.contains(" life"))
        && let Ok(body) = parse_atomic_effect(&s[s.len() - body_raw.len()..], vocab)
    {
        let parsed_cond = parse_condition(&format!("that opponent has {cond_tail}"), vocab)
            .ok()
            .map(Box::new);
        return ok1(InstructionAst::ForEachMatchingPlayer {
            player: PlayerRefAst::EachOpponent,
            condition_text: cond_tail.trim().to_string(),
            condition: parsed_cond,
            body,
        });
    }
    // "<player> loses all counters" (Final Act, Suncleanser, etc.)
    if let Some(who_raw) = strip_suffix_any(&low, &[" loses all counters", " lose all counters"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return ok1(ins_rm_counters(
            SubjectRefAst::Player(player),
            "any",
            true,
            None,
        ));
    }
    // "you gain life and draw cards equal to <val>" (Lifeblood Hydra)
    if let Some(val_raw) = low.strip_prefix("you gain life and draw cards equal to ")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![
            ins_life(PlayerRefAst::You, val.clone(), true),
            ins_draw(PlayerRefAst::You, val),
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
        return Ok(vec![ins_counters(subj, kind_part.trim().to_string(), val)]);
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
        return ok1(InstructionAst::Mill {
            player: PlayerRefAst::You,
            count: Box::new(cnt),
        });
    }
    // "<player> gets an additional <kind> counter" (Necrogen Rotpriest)
    if let Some((who_raw, rest)) = low.split_once(" gets an additional ")
        && let Some(kind) = rest.strip_suffix(" counter")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return ok1(ins_player_counters(
            player,
            kind.trim().to_string(),
            ValueExprAst::Const(1),
        ));
    }
    // "add one mana of that color for each <fe>" / "add one mana of any type <src> produced"
    if let Some(fe) = low.strip_prefix("add one mana of that color for each ")
        && let Ok(cnt) = parse_for_each_value_expr(fe, vocab)
    {
        return ok1(ins_add_mana(
            PlayerRefAst::You,
            crate::ast::ManaSpecAst::AnyColor {
                amount: Box::new(cnt),
                single_color: true,
            },
        ));
    }
    if let Some(after_type) = low.strip_prefix("add one mana of any type ") {
        if let Some(src_raw) = after_type
            .strip_suffix(" could produce")
            .map(|r| r.strip_prefix("that ").unwrap_or(r).trim())
        {
            if let Ok(flt) = parse_object_filter(src_raw, vocab) {
                return ok1(ins_add_mana(
                    PlayerRefAst::You,
                    crate::ast::ManaSpecAst::AnyObjectCouldProduce {
                        filter: Box::new(flt),
                        any_type: true,
                    },
                ));
            }
            let source = parse_subject_or_target(src_raw, vocab)
                .unwrap_or_else(|_| SubjectRefAst::Anaphora(src_raw.to_string()));
            return ok1(InstructionAst::AddQualifiedMana(Box::new(
                crate::ast::QualifiedManaAst {
                    player: PlayerRefAst::You,
                    kind: "any_type_source_could_produce".to_string(),
                    amount: Some(vconst(1)),
                    source: Some(Box::new(source)),
                    choices: Vec::new(),
                },
            )));
        }
        if let Some(src_raw) = after_type
            .strip_suffix(" produced")
            .map(|r| r.strip_prefix("that ").unwrap_or(r).trim())
        {
            let source = parse_subject_or_target(src_raw, vocab)
                .unwrap_or_else(|_| SubjectRefAst::Anaphora(src_raw.to_string()));
            return ok1(InstructionAst::AddQualifiedMana(Box::new(
                crate::ast::QualifiedManaAst {
                    player: PlayerRefAst::You,
                    kind: "any_type_source_produced".to_string(),
                    amount: Some(vconst(1)),
                    source: Some(Box::new(source)),
                    choices: Vec::new(),
                },
            )));
        }
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
                    return Ok(vec![ins_if(cond, inner)]);
                }
                if let Ok(sr) = parse_static_or_replacement(rest, vocab) {
                    return ok1(match sr {
                        StaticOrReplacement::Static(st) => {
                            ins_temp_static(st, DurationAst::ForAsLongAs(Box::new(cond)))
                        }
                        StaticOrReplacement::Replacement(rep) => {
                            ins_temp_rep(rep, DurationAst::ForAsLongAs(Box::new(cond)))
                        }
                    });
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
        (
            "until your next end step, ",
            DurationAst::ForAsLongAs(Box::new(ConditionAst::Marker(GameMarker::UntilStep {
                player: PlayerRefAst::You,
                step: TurnStepAst::EndStep,
            }))),
        ),
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
        ("this turn and next turn, ", DurationAst::UntilEndOfNextTurn),
        ("this turn, ", DurationAst::UntilEndOfTurn),
    ] {
        if low.starts_with(pref) {
            let rest = &s[pref.len()..];
            let parsed_inner = parse_atomic_effect(rest, vocab).or_else(|e| {
                if rest.contains(", and ") && rest.contains(", ") {
                    let mut acc = Vec::new();
                    let (head, tail) = rest.rsplit_once(", and ").ok_or_else(|| e.clone())?;
                    for part in head.split(", ").chain(std::iter::once(tail)) {
                        let p = part.trim();
                        if p.is_empty() {
                            return Err(e.clone());
                        }
                        if let Some(mut sub) = parse_effect_or_temp_clause(p, &dur, vocab) {
                            acc.append(&mut sub);
                        } else if let Ok(mut sub) = parse_atomic_effect(p, vocab) {
                            acc.append(&mut sub);
                        } else {
                            return Err(e.clone());
                        }
                    }
                    if acc.len() >= 2 {
                        return Ok(acc);
                    }
                }
                Err(e)
            });
            if let Ok(inner) = parsed_inner {
                let mut wrapped = Vec::with_capacity(inner.len());
                for mut ins in inner {
                    match &mut ins {
                        InstructionAst::GrantCastPermission { duration, .. } => {
                            *duration = duration_ast_to_slug(&dur);
                            wrapped.push(ins);
                        }
                        InstructionAst::GrantModifiedCastPermission(perm) => {
                            perm.duration = duration_ast_to_slug(&dur);
                            wrapped.push(ins);
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
                        | InstructionAst::CombatRuleMod { duration, .. }
                        | InstructionAst::ForcedBlockByFilter { duration, .. } => {
                            *duration = dur.clone();
                            wrapped.push(ins);
                        }
                        _ => {
                            wrapped.push(InstructionAst::ScopedDuration {
                                duration: dur.clone(),
                                body: vec![ins],
                            });
                        }
                    }
                }
                return Ok(wrapped);
            }
            if let Ok(sr) = parse_static_or_replacement(rest, vocab) {
                return ok1(match sr {
                    StaticOrReplacement::Static(st) => ins_temp_static(st, dur),
                    StaticOrReplacement::Replacement(rep) => ins_temp_rep(rep, dur),
                });
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
        return ok1(ins_combat_rule(
            SubjectRefAst::SelfObject,
            format!("same_is_true_for:{same_tail}"),
            DurationAst::UntilEndOfTurn,
        ));
    }

    // CR 603.7 / CR 106.12 Mana-spend delayed trigger:
    // "When/If you spend this mana to cast <flt> [or activate an ability], <effect>" (Opal Palace, Gilanra, Orbs of Dragonkind, Study Hall, Sunken Palace)
    if let Some(after_spend) = strip_prefix_any(
        &low,
        &[
            "when you spend this mana to cast ",
            "if you spend this mana to cast ",
            "if this mana is spent to cast ",
            "when this mana is spent to cast ",
            "if that mana is spent to cast ",
            "when that mana is spent to cast ",
        ],
    ) && let Some((cast_part, body_part)) = after_spend.split_once(", ")
        && let Ok(body) = parse_atomic_effect(&s[s.len() - body_part.len()..], vocab)
    {
        let flt = if cast_part == "your commander" || cast_part == "a spell or activate an ability"
        {
            any_card_flt()
        } else {
            parse_object_filter(cast_part, vocab).unwrap_or_default()
        };
        if flt.any_card || (!cast_part.is_empty() && parse_object_filter(cast_part, vocab).is_ok())
        {
            return ok1(ins_temp_trig(
                TriggerSpecAst {
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
                DurationAst::UntilEndOfTurn,
            ));
        }
    }

    // Energy gain: "you get an amount of {e} equal to <val>" (Peema Aether-Seer)
    if let Some(val_raw) = low.strip_prefix("you get an amount of {e} equal to ")
        && let Ok(count) = parse_value_expr(val_raw, vocab)
    {
        return ok1(ins_player_counters(PlayerRefAst::You, "energy", count));
    }

    // Leading "have <effect>" wrapper (after stripping "you may ", e.g. Axis of Mortality, Loxodon Lifechanter, Sita Varma, Tanazir Quandrix, Visions, Change of Plans, Cloak of Confusion, Gurzigost, Rumbling Aftershocks)
    if let Some(after_have) = low.strip_prefix("have ") {
        let orig_after_have = &s["have ".len()..];
        if let Some(val_raw) = after_have.strip_prefix("your life total become ")
            && let Ok(val) = parse_value_expr(val_raw, vocab)
        {
            return ok1(InstructionAst::SetLifeTotal {
                player: PlayerRefAst::You,
                value: Box::new(val),
            });
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
                return ok1(ins_animate(
                    subj,
                    p,
                    t,
                    Some(rhs_clean.to_string()),
                    Vec::new(),
                    pt_dur.unwrap_or(DurationAst::Permanent),
                ));
            }
        }
        if let Some((subj_raw, rhs_pt)) = after_have.split_once("'s base power become ")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let (rhs_clean, pt_dur) = strip_duration(rhs_pt, vocab);
            return ok1(ins_animate(
                subj,
                "X",
                "keep",
                Some(rhs_clean.to_string()),
                Vec::new(),
                pt_dur.unwrap_or(DurationAst::Permanent),
            ));
        }
        if let Some((subj_raw, rule_tail)) = after_have.split_once(" assign ")
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            && (rule_tail.starts_with("no combat damage")
                || rule_tail.contains("combat damage")
                    && rule_tail.contains("as though it weren't blocked"))
        {
            return ok1(ins_combat_rule(
                subj,
                rule_tail.to_string(),
                DurationAst::UntilEndOfTurn,
            ));
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
        return ok1(InstructionAst::Transform {
            subject: self_obj(),
        });
    }
    if low.starts_with("sanctum of nature, exile them, then meld them into ") || low == "convert ~"
    {
        return ok1(InstructionAst::Transform {
            subject: self_obj(),
        });
    }
    if let Some(n_raw) = strip_prefix_any(&low, &["you earthbend ", "earthbend "])
        && let Ok(cnt) = parse_value_expr(n_raw, vocab)
    {
        return ok1(InstructionAst::Earthbend {
            count: Box::new(cnt),
        });
    }
    if let Some(val_raw) = low.strip_prefix("proliferate a number of times equal to ")
        && let Ok(cnt) = parse_value_expr(val_raw, vocab)
    {
        return ok1(ins_for_each_count(cnt, vec![InstructionAst::Proliferate]));
    }
    if let Some((who_raw, rest)) =
        split_once_any(&low, &[" get a number of ", " gets a number of "])
        && let Some(player) = parse_player_ref(who_raw)
        && let Some((kind, val_raw)) = rest.split_once(" counters equal to ")
        && let Ok(cnt) = parse_value_expr(val_raw, vocab)
    {
        return ok1(ins_player_counters(player, kind.trim().to_string(), cnt));
    }
    if let Some(val_raw) = low
        .strip_prefix("you get ")
        .and_then(|r| r.strip_suffix(" additional upkeep steps after this phase"))
        && let Ok(cnt) = parse_value_expr(val_raw, vocab)
    {
        return ok1(ins_for_each_count(
            cnt,
            vec![InstructionAst::AdditionalCombat("upkeep".to_string())],
        ));
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
        return ok1(ins_combat_rule(
            SubjectRefAst::SelfObject,
            low,
            DurationAst::UntilEndOfTurn,
        ));
    }
    if let Some(subj_raw) = low.strip_prefix("count the colors of ")
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return ok1(InstructionAst::ChooseOption(format!(
            "count_colors:{subj_raw}"
        )));
    }
    if let Some(after_ctrl) = low.strip_prefix("the controller of ")
        && let Some(subj_raw) = after_ctrl.strip_suffix(" sacrifices it")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![ins_sac(PlayerRefAst::ThatObjectController, subj)]);
    }
    if let Some((subj_raw, cnt_rest)) = low.split_once(" enters with an additional ")
        && let Some(kind_raw) = strip_suffix_any(&cnt_rest, &[" counter on it", " counters on it"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return ok1(ins_counters(
            subj,
            kind_raw.trim().to_string(),
            ValueExprAst::Const(1),
        ));
    }
    if low == "reveal that many cards from the top of your library" {
        return Ok(vec![ins_look_top(true, "your", ValueExprAst::PriorResult)]);
    }
    if low == "give that permanent or player another counter of that kind"
        || low == "put an additional counter of that kind on that permanent"
    {
        return ok1(ins_counters(
            anaphora("that permanent"),
            "that_kind",
            ValueExprAst::Const(1),
        ));
    }
    if low.starts_with("this ability can't cause the total number of ")
        && low.contains(" counters on ~ to be greater than ")
    {
        return Ok(vec![ins_ability_restr(low)]);
    }
    if low == "shuffle your hand and graveyard into your library" {
        return ok1(InstructionAst::ShuffleGraveyardIntoLibrary {
            player: PlayerRefAst::You,
        });
    }
    if let Some(after_ch) = low.strip_prefix("change a target of ")
        && let Some((spell_raw, new_t)) = after_ch.split_once(" to ")
        && let Ok(subj) = parse_subject_or_target(spell_raw, vocab)
        && parse_subject_or_target(new_t, vocab).is_ok()
    {
        return ok1(InstructionAst::ChangeTargets {
            target: Box::new(subj),
            new_target: None,
            must_target: None,
            single_target_only: false,
        });
    }
    if low == "the new target must be a player"
        || low.starts_with("reselect which player or ")
        || low == "cast it from your graveyard as an adventure until the end of your next turn"
        || low.starts_with("the escape cost is equal to its mana cost plus exile ")
        || low
            == "until end of turn, ~ loses \"prevent all damage that would be dealt to ~.\" any player may activate this ability"
    {
        return ok1(ins_combat_rule(
            SubjectRefAst::SelfObject,
            low,
            DurationAst::UntilEndOfTurn,
        ));
    }
    if let Some((who_raw, rest)) = low.split_once(" chooses ")
        && let Some(_p) = parse_player_ref(who_raw)
        && rest.ends_with(" of those cards")
    {
        return ok1(InstructionAst::ChooseOption(format!("{who_raw}:{rest}")));
    }
    if low == "they put one on the bottom of your library" {
        return ok1(ins_move(anaphora("one"), "the bottom of your library"));
    }
    if let Some(after_t) = low.strip_prefix("tap ")
        && let Some((cnt_tok, flt_raw)) = after_t.split_once(' ')
        && parse_number_word(cnt_tok).is_some()
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return ok1(InstructionAst::TapPermanent {
            subject: Box::new(grp(flt)),
            any_number: false,
        });
    }
    if let Some((subj_raw, rest)) = low.split_once(" attacks ")
        && let Some(opp_raw) = rest.strip_suffix(" this turn if able")
        && let (Ok(subj), Ok(_opp)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_subject_or_target(opp_raw, vocab),
        )
    {
        return ok1(ins_combat_rule(
            subj,
            format!("must_attack:{opp_raw}"),
            DurationAst::UntilEndOfTurn,
        ));
    }

    // CR 603.7 Delayed / duration-scoped triggered ability created during resolution:
    // "When <trigger> [this turn], [if <cond>,] <effect>" / "Whenever <trigger>, <effect>"
    if let Some(after_w) = strip_prefix_any(&low, &["when ", "whenever "])
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
        return ok1(ins_temp_trig(
            TriggerSpecAst {
                events: vec![TriggerEventAst::KeywordActionPerformed {
                    player: PlayerRefAst::You,
                    action: hdr_low.to_string(),
                }],
                intervening_if: None,
            },
            body,
            DurationAst::UntilEndOfTurn,
        ));
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
                return ok1(ins_temp_trig(trigger, body, DurationAst::UntilEndOfTurn));
            }
        }
    }

    // CR 614 One-shot / duration-scoped replacement effect created during resolution:
    // "If <ev> would <would>, <instead>" / "each <flt> that would ... instead"
    if (low.starts_with("if ") && low.contains(" would ")
        || low.contains(" that would ") && low.ends_with(" instead"))
        && let Ok(StaticOrReplacement::Replacement(rep)) = parse_static_or_replacement(s, vocab)
    {
        return Ok(vec![ins_temp_rep(rep, DurationAst::UntilEndOfTurn)]);
    }

    // CR 701.16 Reveal-Until Loop: "[<player>] reveal(s) cards from the top of <whose> library until <who> reveal(s) [<cnt>] <flt>"
    // Also passive voice: "... until [<cnt>] <flt> are revealed" / "is revealed" (e.g. Mind Funeral).
    if let Some((who, after)) = split_leading_player_verb(&low, "reveal", "reveals")
        && let Some((top_part, until_part)) = after.split_once(" until ")
        && top_part.starts_with("cards from the top of ")
        && top_part.ends_with(" library")
        && let Some(flt_side) = split_once_any(&until_part, &[" reveals ", " reveal "])
            .map(|(_, rhs)| rhs)
            .or_else(|| strip_suffix_any(&until_part, &[" are revealed", " is revealed"]))
    {
        let (cnt, flt_raw) = peel_leading_count(flt_side.trim(), vocab);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return ok1(InstructionAst::RevealUntil {
                player: who,
                filter: Box::new(flt),
                count: Box::new(cnt),
            });
        }
    }

    // Reflexive trigger: "When you do, ..." / "When they do, ..." / "When you pay this cost one or more times, ..." / "When you <act> one or more <flt> this way, ..."
    if low.starts_with("when you do, ") {
        let inner = parse_atomic_effect(&s["when you do, ".len()..], vocab)?;
        return Ok(vec![InstructionAst::ReflexiveTrigger(inner)]);
    }
    if low == "look at it" || low == "look at them" {
        return Ok(vec![ins_look_top(false, "ref", ValueExprAst::Const(1))]);
    }
    if low == "remove them all" {
        return ok1(InstructionAst::RemoveCounters {
            subject: self_obj(),
            counter_kind: "all".to_string(),
            all: true,
            count: None,
        });
    }
    if low == "the player casts it without paying its mana cost if able"
        || low == "you may play cards this way without paying their mana costs"
    {
        return ok1(ins_grant_cast(
            if low.starts_with("the player") {
                PlayerRefAst::ThatPlayer
            } else {
                PlayerRefAst::You
            },
            !low.contains("play "),
            anaphora("it"),
            true,
            "now",
        ));
    }
    if low == "pay life equal to that spell's mana value rather than pay its mana cost"
        || low == "pay life equal to that spell's mana value rather than paying its mana cost"
        || low == "pay life equal to its mana value rather than pay its mana cost"
        || low == "pay life equal to its mana value rather than paying its mana cost"
    {
        return ok1(InstructionAst::GrantCastPermission {
            player: PlayerRefAst::You,
            cast_only: true,
            subject: box_anaphora("it"),
            free: false,
            duration: "now".to_string(),
            count_limit: None,
            alt_cost: Some(AltCastCostAst::PayLifeEqualToManaValue),
        });
    }
    if let Some(after_rep) = low.strip_prefix("repeat this process until ")
        && !after_rep.is_empty()
    {
        return ok1(InstructionAst::RepeatProcess {
            condition: parse_condition(after_rep, vocab).ok().map(Box::new),
            max_times: None,
            any_number: true,
        });
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
            return Ok(vec![ins_for_each_count(ValueExprAst::PriorResult, body)]);
        }
    }
    if low == "double the value of x" {
        return ok1(InstructionAst::DoubleStatOrCounters {
            subject: self_obj(),
            kind: "value_of_x".to_string(),
            duration: DurationAst::Instant,
        });
    }
    if let Some(after_ex) = low.strip_prefix("exile ")
        && let Some(subj_raw) = after_ex.strip_suffix(" and copy it")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            ins_exile(None, subj),
            InstructionAst::Copy {
                subject: box_anaphora("it"),
                exception: None,
                count: None,
            },
        ]);
    }
    if low == "exile the top card of your library face down and look at it" {
        return Ok(vec![
            ins_exile_top(None, "your", ValueExprAst::Const(1), true),
            ins_look_top(false, "exiled", ValueExprAst::Const(1)),
        ]);
    }
    if low == "exile one face down and put the rest on the bottom of your library in any order" {
        return ok1(InstructionAst::ChooseAndKeep {
            pick: "one".to_string(),
            keep_to: "exile face down".to_string(),
            rest_to: "bottom of your library in any order".to_string(),
        });
    }
    if let Some(fe) = low.strip_prefix("take an extra turn after this one for each ")
        && let Ok(cnt) = parse_for_each_value_expr(fe, vocab)
    {
        return ok1(ins_for_each_count(
            cnt,
            vec![InstructionAst::ExtraTurn {
                player: PlayerRefAst::You,
            }],
        ));
    }
    if low == "two target players exchange life totals" {
        return ok1(ins_exchange(
            ExchangeKind::LifeTotals,
            SubjectRefAst::Player(PlayerRefAst::TargetPlayer),
            Some(SubjectRefAst::Player(PlayerRefAst::TargetPlayer)),
        ));
    }
    if let Some((who_raw, _)) = split_once_any(
        &low,
        &[
            " skips their next combat phase",
            " skip their next combat phase",
        ],
    ) && let Some(player) = parse_player_ref(who_raw)
    {
        return ok1(InstructionAst::SkipNextStep {
            player,
            step: TurnStepAst::BeginningOfCombat,
        });
    }
    if let Some((who_raw, rest)) = split_once_any(&low, &[" chooses ", " choose "])
        && let Some(player) = parse_player_ref(who_raw)
    {
        if let Some((flt_part, act_tail)) = split_once_any(
            &rest,
            &[
                ", then sacrifices the rest",
                ", then sacrifice the rest",
                " and discards the rest",
                " and discard the rest",
                " and sacrifices the rest",
                " and sacrifice the rest",
                " and exiles the rest",
                " and exile the rest",
            ],
        ) {
            let flt_clean = flt_part
                .trim()
                .strip_prefix("up to ")
                .unwrap_or(flt_part.trim());
            let (_, flt_nocnt) = peel_leading_count(flt_clean, vocab);
            if let Ok(flt) = parse_object_filter(flt_clean, vocab)
                .or_else(|_| parse_object_filter(flt_nocnt, vocab))
            {
                return ok1(InstructionAst::Equalize {
                    player,
                    filter: Box::new(flt),
                    action: if act_tail.is_empty() && rest.contains("discard") {
                        "discard_rest".to_string()
                    } else if act_tail.is_empty() && rest.contains("exile") {
                        "exile_rest".to_string()
                    } else {
                        "sacrifice_rest".to_string()
                    },
                });
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
            return ok1(InstructionAst::ChooseOption(format!("{who_raw}:{rest}")));
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
        return ok1(ins_combat_rule(
            SubjectRefAst::SelfObject,
            low,
            DurationAst::UntilEndOfTurn,
        ));
    }
    if low == "it enters tapped and attacking" {
        return ok1(ins_combat_rule(
            anaphora("it"),
            "enters_tapped_and_attacking",
            DurationAst::UntilEndOfCombat,
        ));
    }
    if let Some(subj_raw) = low.strip_prefix("lock or unlock a door of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return ok1(InstructionAst::UnlockDoor {
            subject: Box::new(subj),
        });
    }
    if low == "reveal a card at random from your hand" {
        return ok1(InstructionAst::RevealRandomFromHand {
            player: PlayerRefAst::You,
        });
    }
    if low == "if you cast a spell this way, you cast it without paying its mana cost"
        || low
            == "if you cast a spell this way, pay life equal to its mana value rather than pay its mana cost"
    {
        return ok1(ins_grant_cast(
            PlayerRefAst::You,
            true,
            anaphora("that spell"),
            low.contains("without paying"),
            "now",
        ));
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

    // Compound play-lands-and-cast-spells permission BEFORE stripping "You may " (Sen Triplets, Yawgmoth's Will, etc.)
    if let Some(rest_pl) = low.strip_prefix("you may play lands and cast spells ")
        && let (Some(p1), Some(p2)) = (
            try_parse_cast_permission(&format!("you may play lands {rest_pl}"), vocab),
            try_parse_cast_permission(&format!("you may cast spells {rest_pl}"), vocab),
        )
    {
        return Ok(vec![p1, p2]);
    }

    // Play / Cast permission BEFORE stripping "You may "
    if let Some(perm) = try_parse_cast_permission(&low, vocab) {
        return Ok(vec![perm]);
    }

    // CR 118.12a: "you may <act> unless <player> pays <cost>" (Rhystic Study):
    // Opponent decides whether to pay FIRST, then you optionally perform <act> -> Unless(..., [Optional(<act>)]).
    if low.starts_with("you may ")
        && !s.contains('"')
        && let Some(idx) = low.rfind(" unless ")
        && idx > 8
        && let Some((who_raw, rest_cost)) = low[idx + 8..]
            .trim()
            .split_once(" pays ")
            .or_else(|| low[idx + 8..].trim().split_once(" pay "))
        && let (Some(player), Ok(cost), Ok(inner)) = (
            parse_player_ref(who_raw),
            parse_cost_atom(rest_cost.trim(), vocab),
            parse_atomic_effect(s[8..idx].trim(), vocab),
        )
    {
        return ok1(ins_unless(
            vec![InstructionAst::Optional(inner)],
            Some(player),
            Some(cost),
            None,
            None,
        ));
    }

    // Control-flow wrappers (use `if let Ok` so compound "You may A, and you may B" / "If you do, A, and B" fall through to top-level combinators!)
    if low.starts_with("you may ")
        && let Ok(inner) = parse_atomic_effect(&s[8..], vocab)
    {
        return Ok(vec![InstructionAst::Optional(inner)]);
    }
    if let Some((who_raw, rest)) = s.split_once(" may ")
        && let Some(player) = parse_player_ref(who_raw)
        && let Ok(mut inner) = parse_atomic_effect(rest, vocab)
    {
        if player != PlayerRefAst::You
            && !rest.trim_start().to_ascii_lowercase().starts_with("you ")
        {
            let bound_actor = match player {
                PlayerRefAst::TargetOpponent
                | PlayerRefAst::TargetPlayer
                | PlayerRefAst::AnyOpponent
                | PlayerRefAst::AnyPlayer
                | PlayerRefAst::EachPlayer
                | PlayerRefAst::EachOpponent => PlayerRefAst::ThatPlayer,
                other => other,
            };
            rebind_implicit_you_actor(&mut inner, bound_actor);
        }
        return Ok(vec![ins_opt_player(player, inner)]);
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
            return Ok(vec![ins_shuffle(player)]);
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
    if let Some(cost_raw) = strip_suffix_any(
        &s,
        &[
            " rather than pay ~'s mana cost",
            " rather than pay ~'s mana cost.",
        ],
    ) && let Ok(cost) = parse_cost_list(cost_raw, vocab)
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
    for succ_pref in [
        "if the first player does, ",
        "if a player does either, ",
        "if that player does, ",
        "if the player does, ",
        "if a player does, ",
        "if they do, ",
        "if you pay, ",
    ] {
        if low.starts_with(succ_pref)
            && let Ok(inner) = parse_atomic_effect(&s[succ_pref.len()..], vocab)
        {
            return Ok(vec![InstructionAst::IfPreviousSucceeded(inner)]);
        }
    }
    for fail_pref in [
        "if you don't, ",
        "if it doesn't, ",
        "if no one does, ",
        "if the player doesn't, ",
        "if that player doesn't, ",
        "if they don't, ",
        "if you can't, ",
        "if the player can't, ",
        "if that player can't, ",
        "if they can't, ",
        "if you can't sacrifice a creature, ",
        "if you can't choose an opponent this way, ",
        "if you don't sacrifice a forest, ",
        "if you don't reveal it, ",
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
            return Ok(vec![ins_for_each_count(ValueExprAst::Const(2), inner)]);
        }
        if let Some(before_times) = low.strip_suffix(" times")
            && let Some((eff_part, cnt_part)) = before_times.rsplit_once(' ')
            && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
            && let Ok(inner) = parse_atomic_effect(&s[..eff_part.len()], vocab)
        {
            return Ok(vec![ins_for_each_count(cnt, inner)]);
        }
    }
    // Modern "<player> shuffles [their library]" (Lantern of Insight, Quiet Speculation, Tempt with Discovery, Boggart Forager, Soldier of Fortune)
    if let Some(who_raw) = strip_suffix_any(
        &low,
        &[
            " shuffles their library",
            " shuffle their library",
            " shuffles his or her library",
            " shuffles",
            " shuffle",
        ],
    ) && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![ins_shuffle(player)]);
    }
    // "behold <flt>" / "open <N> attractions" / "roll [N] <sides>-sided die/dice" / "it's still a <desc> land" / "they're P/T <desc>"
    // Plus Pass S orthogonal effects: double <player>'s life total, <player> manifests dread,
    // tap/untap and goad <subj>, the copy targets <subj>, add <N> mana of the chosen color,
    // pay {M} up to N times / pay any amount of {C} / pay one or more {E}, create a [tapped] <Named> token, choose <N> <flt>
    if let Some(after) = low.strip_prefix("double ")
        && let Some(who_raw) = after.strip_suffix("'s life total")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return ok1(ins_life(
            player,
            val_player_stat(player, crate::ast::PlayerStatKind::LifeTotal),
            true,
        ));
    }
    if let Some(who_raw) = low.strip_suffix(" manifests dread")
        && let Some(player) = parse_player_ref(who_raw)
    {
        if player == PlayerRefAst::You {
            return Ok(vec![ins_manifest(ValueExprAst::Const(1), true, None)]);
        }
        return ok1(InstructionAst::ManifestByPlayer {
            player,
            count: vconst(1),
            dread: true,
            subject: None,
        });
    }
    if let Some(subj_raw) = low.strip_prefix("untap and goad ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            InstructionAst::UntapPermanent {
                subject: Box::new(subj.clone()),
                count: None,
                up_to: false,
            },
            ins_combat_rule(subj, "goad", DurationAst::UntilYourNextTurn),
        ]);
    }
    if let Some(subj_raw) = low.strip_prefix("tap and goad ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(vec![
            InstructionAst::TapPermanent {
                subject: Box::new(subj.clone()),
                any_number: false,
            },
            ins_combat_rule(subj, "goad", DurationAst::UntilYourNextTurn),
        ]);
    }
    if let Some(subj_raw) = low.strip_prefix("the copy targets ")
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return Ok(vec![ins_ability_restr(low)]);
    }
    if let Some(cnt_raw) = low
        .strip_prefix("add ")
        .and_then(|r| r.strip_suffix(" mana of the chosen color"))
        && let Ok(cnt) = parse_value_expr(cnt_raw, vocab)
    {
        return ok1(ins_add_mana(
            PlayerRefAst::You,
            crate::ast::ManaSpecAst::AnyColor {
                amount: Box::new(cnt),
                single_color: true,
            },
        ));
    }
    if let Some(after_pay) = s.strip_prefix("pay ").or_else(|| s.strip_prefix("Pay ")) {
        let alow = after_pay.to_ascii_lowercase();
        if alow == "one or more {e}" {
            return ok1(InstructionAst::PayCostAnyNumberOfTimes(
                CostAtomAst::PayEnergy(1),
            ));
        }
        if let Some(sym) = after_pay.strip_prefix("any amount of ")
            && is_mana_sequence(sym.trim())
        {
            return ok1(InstructionAst::PayCostAnyNumberOfTimes(
                CostAtomAst::PayMana(sym.trim().to_string()),
            ));
        }
        if let Some((mana_part, times_part)) = after_pay.split_once(" up to ")
            && times_part.to_ascii_lowercase().ends_with(" times")
            && is_mana_sequence(mana_part.trim())
        {
            return ok1(InstructionAst::PayCostAnyNumberOfTimes(
                CostAtomAst::PayMana(mana_part.trim().to_string()),
            ));
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
            return ok1(ins_create_token(
                PlayerRefAst::You,
                crate::ast::TokenSpecAst::Predefined {
                    name: name_clean.to_string(),
                    count: vconst(1),
                    tapped,
                },
            ));
        }
    }
    // Cluster 8: "choose <N> of them at random" / "choose one of those cards at random"
    if let Some(after_ch) = low.strip_prefix("choose ")
        && let Some(before_rand) = after_ch.strip_suffix(" at random")
        && let Some((cnt_tok, grp_raw)) = before_rand.split_once(" of ")
        && let (Ok(count), Ok(group)) = (
            parse_value_expr(cnt_tok.trim(), vocab),
            parse_subject_or_target(grp_raw.trim(), vocab),
        )
    {
        return Ok(vec![ins_choose_group(count, true, group)]);
    }
    // "choose <N> <flt>" (Clarion Ultimatum: "Choose five permanents you control")
    if let Some(after) = low.strip_prefix("choose ") {
        let (cnt, flt_raw) = peel_leading_count(after, vocab);
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            if flt.predicates.iter().any(|p| {
                matches!(
                    p,
                    crate::ast::FilterPredicateAst::ManaCostPayableBySpentMana { .. }
                )
            }) {
                return ok1(ins_choose_group(cnt, false, grp(flt)));
            }
            return ok1(InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser: PlayerRefAst::You,
                    description: after.to_string(),
                },
            ));
        }
    }
    if let Some(flt_raw) = low.strip_prefix("behold ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return ok1(InstructionAst::PayCost(CostAtomAst::Behold(Box::new(flt))));
    }
    if low.starts_with("open ") && low.ends_with(" attractions") {
        return Ok(vec![ins_ability_restr(low)]);
    }
    if low.starts_with("roll ") && (low.ends_with("-sided die") || low.ends_with("-sided dice")) {
        return Ok(vec![ins_ability_restr(low)]);
    }
    if low.starts_with("it's still a ") || low.starts_with("it's still an ") {
        return Ok(vec![InstructionAst::KeepExistingTypes(low)]);
    }
    if low.starts_with("they're ") && low.ends_with(" creatures") {
        return Ok(vec![ins_ability_restr(low)]);
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
            || a_low.starts_with("look ")
            || a_low.starts_with("get ") && a_low.ends_with(" {e}")
        {
            if let Some(cnt_tok) = a_low
                .strip_prefix("get ")
                .and_then(|r| r.strip_suffix(" {e}"))
                && let Ok(cnt) = parse_value_expr(cnt_tok, vocab)
            {
                return Ok(vec![ins_player_counters(PlayerRefAst::You, "energy", cnt)]);
            }
            if let Ok(inner) = parse_atomic_effect(after_you, vocab) {
                return Ok(inner);
            }
        }
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
                return Ok(vec![ins_if(cond, inner)]);
            }
        }
        for (idx, _) in after_if.match_indices(",\" ") {
            let cond_part = format!("{}\"", &after_if[..idx]);
            let body_part = &after_if[idx + 3..];
            if let Ok(cond) = parse_condition(&cond_part, vocab)
                && let Some(inner) =
                    parse_effect_or_temp_clause(body_part, &DurationAst::UntilEndOfTurn, vocab)
            {
                return Ok(vec![ins_if(cond, inner)]);
            }
        }
    }
    // Trailing "<effect> instead if <condition>" / "<effect> if <condition>"
    if !s.contains('"') && !low.starts_with("if ") {
        if let Some(idx) = low.rfind(" instead if ")
            && let Ok(cond) = parse_condition(&s[idx + " instead if ".len()..], vocab)
            && let Ok(inner) = parse_atomic_effect(s[..idx].trim(), vocab)
        {
            return ok1(ins_if(cond, vec![InstructionAst::ReplaceEffect(inner)]));
        }
        if let Some(idx) = low.rfind(" if ")
            && let Ok(cond) = parse_condition(&s[idx + 4..], vocab)
            && let Ok(inner) = parse_atomic_effect(s[..idx].trim(), vocab)
        {
            return Ok(vec![ins_if(cond, inner)]);
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
                return ok1(ins_for_each_player(
                    if over_raw.to_ascii_lowercase().contains("opponent") {
                        PlayerRefAst::EachOpponent
                    } else {
                        PlayerRefAst::EachPlayer
                    },
                    inner,
                ));
            }
            let each_str = format!("each {over_raw}");
            if let Some(p) = parse_player_ref(&each_str)
                && let Ok(inner) = parse_atomic_effect(body_part, vocab)
            {
                return Ok(vec![ins_for_each_player(p, inner)]);
            }
            if let Ok(flt) = parse_object_filter(over_raw, vocab)
                && let Ok(inner) = parse_atomic_effect(body_part, vocab)
            {
                return ok1(InstructionAst::ForEachObject {
                    filter: Box::new(flt),
                    body: inner,
                });
            }
            if let Some(src_raw) = over_raw.to_ascii_lowercase().strip_prefix("color among ")
                && let Ok(source) = parse_subject_or_target(src_raw.trim(), vocab)
                && let Ok(inner) = parse_atomic_effect(body_part, vocab)
            {
                return ok1(InstructionAst::ForEachColorAmong {
                    source: Box::new(source),
                    body: inner,
                });
            }
            if let Ok(cnt) = parse_for_each_value_expr(over_raw, vocab)
                && let Ok(inner) = parse_atomic_effect(body_part, vocab)
            {
                return Ok(vec![ins_for_each_count(cnt, inner)]);
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
            return Ok(vec![ins_unless(body, Some(player), Some(cost), None, None)]);
        }
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
                        return Ok(vec![ins_for_each_player(player, inner)]);
                    }
                }
            }
        }
    }

    // Standalone "X is <ValueExpr>" binder sentence (Soul Foundry, etc.)
    if let Some(val_raw) = low.strip_prefix("x is ")
        && let Ok(x_expr) = parse_value_expr(val_raw, vocab)
    {
        return ok1(InstructionAst::BindX {
            x_expr: Box::new(x_expr),
            body: Vec::new(),
        });
    }

    // "You and <player> each <verb_effect>" (Teyo, Geometric Tactician, etc.)
    if let Some(after) = low.strip_prefix("you and ")
        && let Some((other_raw, act_raw)) = after.split_once(" each ")
        && let Some(other_player) = parse_player_ref(other_raw)
        && let Ok(inner) = parse_atomic_effect(act_raw, vocab)
    {
        return Ok(vec![
            ins_for_each_player(PlayerRefAst::You, inner.clone()),
            ins_for_each_player(other_player, inner),
        ]);
    }

    // Compound ", then " split (outside quotes and villainous-choice frames), supporting 3-step "<subj> <v1>, <v2>, then <v3>"
    // with shared-subject carry-forward BEFORE bare 3rd-person fallback so player refs are never lost.
    if low.contains(", then ") && !s.contains('"') && !low.contains("villainous choice") {
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
                    strip_prefix_any(&pt, &["then ", "Then "])
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
            return ok1(if any_times {
                InstructionAst::PayCostAnyNumberOfTimes(ca)
            } else {
                InstructionAst::PayCost(ca)
            });
        }
        if core.eq_ignore_ascii_case("any amount of {e}") {
            return ok1(InstructionAst::PayCostAnyNumberOfTimes(
                CostAtomAst::PayEnergy(1),
            ));
        }
        if let Ok(ca) = parse_cost_atom(&format!("pay {core}"), vocab) {
            return ok1(if any_times {
                InstructionAst::PayCostAnyNumberOfTimes(ca)
            } else {
                InstructionAst::PayCost(ca)
            });
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
            return Ok(vec![ins_add_mana(who, spec)]);
        }
        let mr_norm = mr.replace(", or ", " or ").replace(", ", " or ");
        let mparts: Vec<&str> = mr_norm
            .split(" or ")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        if mparts.len() >= 2 {
            let mut parsed_choices = Vec::with_capacity(mparts.len());
            let mut all_ok = true;
            for mp in mparts {
                if let Ok(sp) = parse_mana_spec(mp, vocab) {
                    parsed_choices.push(sp);
                } else {
                    all_ok = false;
                    break;
                }
            }
            if all_ok {
                return ok1(InstructionAst::AddQualifiedMana(Box::new(
                    crate::ast::QualifiedManaAst {
                        player: who,
                        kind: "mana_choice".to_string(),
                        amount: None,
                        source: None,
                        choices: parsed_choices,
                    },
                )));
            }
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
        let (core, up_to) = match core.strip_prefix("up to ") {
            Some(c) => (c, true),
            None => (core, false),
        };
        let cnt = if core == "an additional card" {
            Some(ValueExprAst::Const(1))
        } else if let Some(cnt_raw) =
            strip_suffix_any(&core, &[" additional cards", " additional card"])
        {
            parse_value_expr(cnt_raw, vocab).ok()
        } else if let Some(after_half) = core.strip_prefix("half ")
            && let Some((expr_raw, round_raw)) = after_half.split_once(" cards, rounded ")
            && let Ok(inner) = parse_value_expr(expr_raw, vocab)
        {
            Some(val_div(
                inner,
                2,
                if round_raw == "up" {
                    crate::ast::Rounding::Up
                } else {
                    crate::ast::Rounding::Down
                },
            ))
        } else if let Some(fe) =
            strip_prefix_any(&core, &["a card for each ", "one card for each "])
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
            .strip_prefix("a number of cards equal to ")
            .or_else(|| core.strip_prefix("cards equal to "))
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
        } else if let Some(cnt_raw) = strip_suffix_any(&core, &[" cards", " card"]) {
            parse_value_expr(cnt_raw, vocab).ok()
        } else {
            None
        };
        if let Some(count) = cnt {
            let ins = InstructionAst::Draw {
                player: who,
                count: Box::new(count),
                up_to,
            };
            return Ok(if delayed {
                vec![ins_delayed(
                    TurnStepAst::Upkeep,
                    DelayedStepScope::Next,
                    vec![ins],
                )]
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
    let dur_perm = dur_opt.clone().unwrap_or(DurationAst::Permanent);
    let dur = dur_opt.unwrap_or(DurationAst::UntilEndOfTurn);

    if let Some(pt_ins) = try_parse_modify_pt(s_nodur, &dur, vocab) {
        return Ok(pt_ins);
    }

    if let Some(ab_ins) = try_parse_modify_abilities(s_nodur, &dur_perm, vocab) {
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
        let make_tok_ins = |controller: PlayerRefAst, rich: RichTokenCreationAst| {
            if rich.is_plain() {
                build_create_token_instruction(controller, rich.token, vocab)
            } else {
                InstructionAst::CreateQualifiedToken(Box::new(QualifiedTokenCreationAst {
                    controller,
                    token: rich.token,
                    blocking: rich.blocking,
                    dynamic_power: rich.dynamic_power.map(Box::new),
                    dynamic_toughness: rich.dynamic_toughness.map(Box::new),
                    dynamic_pt_text: rich.dynamic_pt_text,
                    chosen_color: rich.chosen_color,
                    chosen_type: rich.chosen_type,
                    attacking_target: rich.attacking_target.map(Box::new),
                }))
            }
        };
        if let Some(choice_raw) =
            strip_prefix_any(&tok_low, &["your choice of ", "their choice of "])
        {
            let mut branches = Vec::new();
            let mut ok = true;
            for part in split_top_level(choice_raw, ",") {
                let p = part.trim();
                let p = p.strip_prefix("or ").unwrap_or(p).trim();
                if let Some((a, b)) = p.split_once(" or ") {
                    if let (Ok(ta), Ok(tb)) = (
                        parse_token_creation_rich(a.trim(), vocab),
                        parse_token_creation_rich(b.trim(), vocab),
                    ) {
                        branches.push(vec![make_tok_ins(who, ta)]);
                        branches.push(vec![make_tok_ins(who, tb)]);
                    } else {
                        ok = false;
                        break;
                    }
                } else if let Ok(tok) = parse_token_creation_rich(p, vocab) {
                    branches.push(vec![make_tok_ins(who, tok)]);
                } else {
                    ok = false;
                    break;
                }
            }
            if ok && branches.len() >= 2 {
                return Ok(vec![InstructionAst::Choice(branches)]);
            }
        }
        if let Ok(tok) = parse_token_creation_rich(tok_raw, vocab) {
            return Ok(vec![make_tok_ins(who, tok)]);
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
                if let Ok(tok) = parse_token_creation_rich(p, vocab) {
                    toks.push(make_tok_ins(who, tok));
                } else if let Some((a, b)) = p.split_once(" and ")
                    && let (Ok(ta), Ok(tb)) = (
                        parse_token_creation_rich(a.trim(), vocab),
                        parse_token_creation_rich(b.trim(), vocab),
                    )
                {
                    toks.push(make_tok_ins(who, ta));
                    toks.push(make_tok_ins(who, tb));
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

    // "<subj> enters with a number of [additional] <kind> counters on it equal to <val>"
    // Emit EntersWithCounters for self-total riders ("equal to the total"), and
    // TemporaryReplacement(OthersEnterWithCounters) for dynamic battlefield expressions so 0/0 Hydras
    // survive SBAs.
    if let Some((subj_raw, rest)) = split_once_any(&low, &[" enters with ", " enter with "])
        && let Some((mid, val_raw)) = rest.split_once(" on it equal to ")
        && let Ok(val) = parse_value_expr(val_raw.trim(), vocab)
        && let Some(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
            .ok()
            .or_else(|| {
                subj_raw
                    .trim()
                    .strip_prefix("the next ")
                    .and_then(|r| r.strip_suffix(" you cast this turn"))
                    .and_then(|f| parse_object_filter(f.trim(), vocab).ok())
                    .map(grp)
            })
    {
        let m = mid
            .trim()
            .strip_prefix("a number of ")
            .unwrap_or(mid.trim());
        let (additional, k_tail) = match m.strip_prefix("additional ") {
            Some(r) => (true, r),
            None => (false, m),
        };
        if let Some(kind) = strip_suffix_any(&k_tail, &[" counters", " counter"])
            && !matches!(val, ValueExprAst::PriorResult)
        {
            return ok1(ins_temp_rep(
                ReplacementEffectAst::OthersEnterWithCounters {
                    subject: Box::new(subj),
                    counter_kind: kind.trim().to_string(),
                    count: Box::new(val),
                    additional,
                    per: None,
                },
                DurationAst::Instant,
            ));
        }
    }

    // "<subj> enters with [an additional] <cnt> <kind> counter(s) on it" inside effects (Long List of the Ents, Undead Sprinter, Gluttonous Hellkite, Naya Soulbeast)
    if let Some((subj_raw, rest)) = split_once_any(&low, &[" enters with ", " enter with "])
        && let Some((cnt_part, tail)) =
            split_once_any(&rest, &[" counters on it", " counter on it"])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let cnt_clean =
            strip_prefix_any(&cnt_part, &["an additional ", "additional "]).unwrap_or(cnt_part);
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
            return Ok(vec![ins_counters(subj, kind.to_string(), cnt)]);
        }
    }

    // MoveCounters / PutCounters / RemoveCounters
    if let Some(cnt_ins) = try_parse_counters(&low, vocab) {
        return Ok(vec![cnt_ins]);
    }

    // Destroy
    if let Some(after) = strip_prefix_any(&low, &["destroy ", "destroys "]) {
        let (core, delayed) = if let Some(c) = after.strip_suffix(" at end of combat") {
            (c, Some((TurnStepAst::EndOfCombat, DelayedStepScope::Next)))
        } else if let Some(c) = after.strip_suffix(" at the beginning of the next end step") {
            (c, Some((TurnStepAst::EndStep, DelayedStepScope::Next)))
        } else {
            (after, None)
        };
        let (core, random) = match core.strip_suffix(" at random") {
            Some(c) => (c.trim(), true),
            None => (core, false),
        };
        let (core, except) =
            if let Some((l, r)) = split_once_any(&core, &[", except for ", " except for "]) {
                match parse_subject_or_target(r.trim(), vocab) {
                    Ok(exc) => (l.trim(), Some(Box::new(exc))),
                    Err(_) => (core, None),
                }
            } else {
                (core, None)
            };
        if !core.contains(" except for ") {
            if !random
                && except.is_none()
                && let Some((left_all, right_all)) = core.split_once(" or all ")
                && let (Ok(s1), Ok(s2)) = (
                    parse_subject_or_target(left_all, vocab),
                    parse_subject_or_target(&format!("all {right_all}"), vocab),
                )
            {
                return ok1(InstructionAst::Choice(vec![
                    vec![InstructionAst::Destroy {
                        subject: Box::new(s1),
                        random: false,
                        except: None,
                    }],
                    vec![InstructionAst::Destroy {
                        subject: Box::new(s2),
                        random: false,
                        except: None,
                    }],
                ]));
            }
            if let Ok(subj) = parse_subject_or_target(core, vocab) {
                let ins = InstructionAst::Destroy {
                    subject: Box::new(subj),
                    random,
                    except,
                };
                return Ok(match delayed {
                    Some((step, scope)) => vec![ins_delayed(step, scope, vec![ins])],
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

    // "Change the text of <target> by replacing ..."
    if let Some(after) = low.strip_prefix("change the text of ")
        && let Some((tgt_raw, desc)) = after.split_once(" by ")
        && let Ok(target) = parse_subject_or_target(tgt_raw, vocab)
    {
        return ok1(InstructionAst::ChangeText {
            target: Box::new(target),
            description: desc.trim().to_string(),
        });
    }

    // Scry / Surveil / Mill / LookTop / RevealHand
    if let Some(lib_ins) = try_parse_library_ops(&low, vocab) {
        return Ok(vec![lib_ins]);
    }

    // Counter
    if let Some(after) = strip_prefix_any(&s, &["Counter ", "counter "]) {
        if let Some((tgt_raw, unless_raw)) = after.split_once(" unless its controller pays ") {
            if let Ok(target) = parse_subject_or_target(tgt_raw, vocab) {
                let unless_pay = if let Ok(c) = parse_cost_atom(unless_raw, vocab) {
                    Some(c)
                } else if let Ok(parts) = parse_cost_list(unless_raw, vocab) {
                    parts.into_iter().next()
                } else {
                    None
                };
                if let Some(unless_pay) = unless_pay {
                    return ok1(InstructionAst::Counter {
                        target: Box::new(target),
                        unless_pay: Some(unless_pay),
                        unless_action: None,
                    });
                }
            }
        } else if let Some((tgt_raw, unless_act)) = after.split_once(" unless ") {
            if let (Ok(target), Ok(act)) = (
                parse_subject_or_target(tgt_raw, vocab),
                parse_atomic_effect(unless_act, vocab),
            ) {
                return ok1(InstructionAst::Counter {
                    target: Box::new(target),
                    unless_pay: None,
                    unless_action: Some(act),
                });
            }
        } else if let Some((spell_part, _targets_part)) =
            after.to_ascii_lowercase().split_once(" that targets ")
            && let Ok(target) = parse_subject_or_target(spell_part, vocab)
        {
            return ok1(InstructionAst::Counter {
                target: Box::new(target),
                unless_pay: None,
                unless_action: None,
            });
        } else if let Ok(target) = parse_subject_or_target(after, vocab) {
            return ok1(InstructionAst::Counter {
                target: Box::new(target),
                unless_pay: None,
                unless_action: None,
            });
        }
    }

    // Tap / Untap / SkipNextUntap / Regenerate / Transform / PhaseOut / Fight / Attach / GainControl / Prevent / Copy
    if let Some(misc_ins) = try_parse_misc_actions(s, s_nodur, &low, &dur, &dur_perm, vocab) {
        return Ok(vec![misc_ins]);
    }

    // Named CR 701 keyword actions & player counters (Investigate, Monarch, Energy, Monstrosity, Adapt, etc.)
    if let Some(kw_ins) = try_parse_named_keyword_actions(s, &low, vocab) {
        return Ok(vec![kw_ins]);
    }

    // Combat rules & Animation
    if let Some(ca_ins) = try_parse_combat_and_animate(s_nodur, &low, &dur, &dur_perm, vocab) {
        return Ok(vec![ca_ins]);
    }

    if low.contains("additional land") {
        return ok1(InstructionAst::AdditionalLandPlay {
            count: 1,
            duration: DurationAst::UntilEndOfTurn,
        });
    }
    if is_standalone_additional_combat_clause(&low) {
        return Ok(vec![InstructionAst::AdditionalCombat(s.to_string())]);
    }
    if let Some(who_raw) = low
        .strip_suffix(" takes an extra turn after this one")
        .or_else(|| low.strip_suffix(" take an extra turn after this one"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::ExtraTurn { player }]);
    }
    if let Some(who_raw) = strip_suffix_any(&low, &[" loses the game", " lose the game"])
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
        return Ok(vec![ins_if(cond, inner)]);
    }

    // Universal orthogonal "<effect> unless <player_cost | action | condition>" combinator
    if !s.contains('"')
        && let Some(idx) = low.rfind(" unless ")
        && let Ok(inner) = parse_atomic_effect(s[..idx].trim(), vocab)
    {
        let unless_raw = s[idx + 8..].trim();
        let ulow = unless_raw.to_ascii_lowercase();
        if let Some((who_raw, rest)) = split_once_any(&ulow, &[" pays ", " pay "])
            && let (Some(player), Ok(cost)) = (
                parse_player_ref(who_raw),
                parse_cost_atom(rest.trim(), vocab),
            )
        {
            return Ok(vec![ins_unless(
                inner,
                Some(player),
                Some(cost),
                None,
                None,
            )]);
        }
        if let Ok(act) = parse_atomic_effect(unless_raw, vocab) {
            return Ok(vec![ins_unless(inner, None, None, Some(act), None)]);
        }
        if let Ok(cond) = parse_condition(unless_raw, vocab) {
            return Ok(vec![ins_unless(inner, None, None, None, Some(cond))]);
        }
    }

    // Standalone duration-scoped rules: "you have no maximum hand size until ...",
    // "<player> can't play lands [this turn]", "the next spell you cast this turn can't be countered",
    // "<subj> becomes blocked", "turn <subj> face down/up"
    if s_nodur.eq_ignore_ascii_case("you have no maximum hand size") {
        return ok1(ins_combat_rule(
            SubjectRefAst::Player(PlayerRefAst::You),
            "no_max_hand_size",
            dur,
        ));
    }
    if let Some(who_raw) = s_nodur
        .to_ascii_lowercase()
        .strip_suffix(" can't play lands")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return ok1(ins_combat_rule(
            SubjectRefAst::Player(player),
            "cant_play_lands",
            dur,
        ));
    }
    if low == "~ can't be copied" {
        return ok1(ins_combat_rule(
            SubjectRefAst::SelfObject,
            "cant_be_copied",
            DurationAst::Instant,
        ));
    }
    if low == "there is an additional beginning phase after this phase" {
        return ok1(InstructionAst::AdditionalCombat(
            "additional_beginning_phase".to_string(),
        ));
    }
    if let Some(subj_raw) = s_nodur
        .to_ascii_lowercase()
        .strip_suffix(" becomes blocked")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return ok1(ins_combat_rule(
            subj,
            "becomes_blocked",
            DurationAst::UntilEndOfCombat,
        ));
    }
    if let Some(rest) = low.strip_prefix("turn ")
        && let Some((subj_raw, face_dir)) = rest
            .strip_suffix(" face down")
            .map(|r| (r, "face_down"))
            .or_else(|| rest.strip_suffix(" face up").map(|r| (r, "face_up")))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return ok1(ins_combat_rule(
            subj,
            format!("turn_{face_dir}"),
            DurationAst::Permanent,
        ));
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
                return Ok(vec![ins_for_each_player(p, inner)]);
            }
            if let Ok(flt) = parse_object_filter(over_raw, vocab) {
                return ok1(InstructionAst::ForEachObject {
                    filter: Box::new(flt),
                    body: inner,
                });
            }
            if let Ok(cnt) = parse_for_each_value_expr(over_raw, vocab) {
                return Ok(vec![ins_for_each_count(cnt, inner)]);
            }
        }
    }

    // "<player> taps/untaps <subj>" / "<player> incubates <val>" / "<player> amasses <subt> <val>" / "exchange life totals with <player>"
    if let Some((_, subj_raw)) = split_leading_player_verb(&low, "untap", "untaps")
        && let Some(ins) = parse_untap_permanent_instruction(subj_raw, vocab)
    {
        return Ok(vec![ins]);
    }
    if let Some((_, subj_raw)) = split_leading_player_verb(&low, "tap", "taps")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return ok1(InstructionAst::TapPermanent {
            subject: Box::new(subj),
            any_number: false,
        });
    }
    if let Some((_, val_raw)) = split_leading_player_verb(&low, "incubate", "incubates")
        && let Ok(count) = parse_value_expr(val_raw, vocab)
    {
        return ok1(InstructionAst::Incubate {
            count: Box::new(count),
        });
    }
    if let Some((_, rest)) = split_leading_player_verb(&low, "amass", "amasses")
        && let Some((subt, cnt_raw)) = rest.rsplit_once(' ')
        && let Ok(count) = parse_value_expr(cnt_raw, vocab)
    {
        return ok1(InstructionAst::Amass {
            subtype: subt.trim().to_string(),
            count: Box::new(count),
        });
    }
    if let Some(who_raw) = low.strip_prefix("exchange life totals with ")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return ok1(ins_exchange(
            ExchangeKind::LifeTotals,
            SubjectRefAst::Player(PlayerRefAst::You),
            Some(SubjectRefAst::Player(player)),
        ));
    }

    // Simultaneous execution combinator before generic ", and "/" and " splitters so multi-type filters don't exhaust PARSE_OPS
    if (low.starts_with("simultaneously") || low.contains(" simultaneously "))
        && let Some(sim) = choices_misc::parse_simultaneous_effect(&low, vocab)
    {
        return Ok(sim);
    }

    // Compound ", and " split (outside quotes) BEFORE plain " and " so trailing commas never break the left half!
    if low.contains(", and ") && !s.contains('"') && !low.contains("villainous choice") {
        let parts = split_top_level(s, ", and ");
        if parts.len() >= 2 {
            let lead_subj = extract_leading_subject_prefix(parts[0].trim());
            let mut seq = Vec::new();
            let mut ok = true;
            for (idx_p, p) in parts.iter().enumerate() {
                let pt = p.trim();
                let parsed = if idx_p > 0
                    && let Some(subj) = lead_subj
                    && !subj.eq_ignore_ascii_case("you")
                    && starts_with_predicate_verb(pt)
                {
                    parse_atomic_effect(&format!("{subj} {pt}"), vocab)
                        .or_else(|_| parse_atomic_effect(pt, vocab))
                } else {
                    parse_atomic_effect(pt, vocab)
                };
                match parsed {
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

    // Compound " and " split (quote-aware via split_top_level, scanning split points with shared-subject carry-forward!)
    if low.contains(" and ") && !low.contains("villainous choice") {
        let and_parts = split_top_level(s, " and ");
        if and_parts.len() >= 2 {
            for split_idx in 1..and_parts.len() {
                let left_owned = and_parts[..split_idx].join(" and ");
                let right_owned = and_parts[split_idx..].join(" and ");
                let left_s = left_owned.trim();
                let right_s = right_owned.trim();
                let llow = left_s.to_ascii_lowercase();
                let rlow = right_s.to_ascii_lowercase();
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
                    "scries ",
                    "scry ",
                    "surveils ",
                    "surveil ",
                    "investigates",
                    "investigate",
                    "pays ",
                    "pay ",
                    "is ",
                    "are ",
                    "isn't ",
                    "aren't ",
                ]
                .iter()
                .any(|p| rlow.starts_with(p));
                // Carry forward an explicit non-"you" leading subject BEFORE trying bare
                // `parse_atomic_effect(right_s)`, otherwise player verbs like "draws a card" or
                // "lose 4 life" silently default to PlayerRefAst::You!
                if right_is_predicate
                    && let Some(subj_prefix) = extract_leading_subject_prefix(left_s)
                    && !subj_prefix.eq_ignore_ascii_case("you")
                    && let (Ok(mut l), Ok(r)) = (
                        parse_atomic_effect(left_s, vocab),
                        parse_atomic_effect(&format!("{subj_prefix} {right_s}"), vocab),
                    )
                {
                    l.extend(r);
                    return Ok(l);
                }
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
        && !low.contains("villainous choice")
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
                let p_clean =
                    strip_prefix_any(&p_trim, &["and then ", "and ", "then "]).unwrap_or(p_trim);
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
            return Ok(vec![ins_temp_static(st, dur)]);
        }
        if low.contains(" this turn ") {
            let stripped = s.replacen(" this turn ", " ", 1);
            if let Ok(StaticOrReplacement::Static(st)) =
                parse_static_or_replacement(&stripped, vocab)
            {
                return Ok(vec![ins_temp_static(st, DurationAst::UntilEndOfTurn)]);
            }
        }
        if (low.contains(" costs ") || low.contains(" cost "))
            && (low.contains("less to cast") || low.contains("more to cast"))
            && let Ok(StaticOrReplacement::Static(st)) = parse_static_or_replacement(s, vocab)
        {
            return Ok(vec![ins_temp_static(st, DurationAst::UntilEndOfTurn)]);
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
                    return ok1(InstructionAst::SetDynamicPT {
                        subject: Box::new(subj),
                        power: if is_power {
                            Some(Box::new(val.clone()))
                        } else {
                            None
                        },
                        toughness: if !is_power { Some(Box::new(val)) } else { None },
                        duration: dur,
                    });
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
                    return ok1(InstructionAst::SetDynamicPT {
                        subject: Box::new(subj),
                        power: Some(Box::new(val_obj_stat(other.clone(), StatKind::Power))),
                        toughness: Some(Box::new(val_obj_stat(other, StatKind::Toughness))),
                        duration: dur,
                    });
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
        return ok1(InstructionAst::ExileUntilLeaves {
            subject: Box::new(subj),
        });
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
            let rclean = strip_suffix_any(&rlow, &[" before that step", " before then"])
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
                    return Ok(vec![ins_unless(body, Some(player), Some(cost), None, None)]);
                }
            }
            // Case 2: "<player> has <effect>" (Lava Blister, Combustion Man)
            if let Some((p_raw, has_eff)) = split_once_any(&rclean, &[" has ", " have "])
                && let Some(player) = parse_player_ref(p_raw.trim())
                && let Ok(act) = parse_atomic_effect(has_eff.trim(), vocab)
            {
                return Ok(vec![ins_unless(body, Some(player), None, Some(act), None)]);
            }
            // Case 3: arbitrary condition ("unless it has the same name as ...", "unless a permanent entered ...")
            if let Ok(cond) = parse_condition(right_s, vocab) {
                return Ok(vec![ins_unless(body, None, None, None, Some(cond))]);
            }
            // Case 4: arbitrary alternative action ("unless they pay life equal to its mana value", "unless you discard a card")
            if let Ok(act) = parse_atomic_effect(right_s, vocab) {
                return Ok(vec![ins_unless(body, None, None, Some(act), None)]);
            }
        }
    }

    // Compositional Wave 15: Clean reusable effect verbs & trailing "<effect> if <cond>"
    if low == "draw another card" {
        return Ok(vec![ins_draw(PlayerRefAst::You, ValueExprAst::Const(1))]);
    }
    if let Some(val_raw) = low.strip_prefix("discard cards equal to ")
        && let Ok(val) = parse_value_expr(val_raw, vocab)
    {
        return Ok(vec![ins_discard(PlayerRefAst::You, Some(val))]);
    }
    if let Some(who_raw) =
        strip_suffix_any(&low, &[" loses all unspent mana", " lose all unspent mana"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(vec![InstructionAst::LoseUnspentMana { player }]);
    }
    if let Some(flt_raw) = low.strip_prefix("count the number of ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return ok1(InstructionAst::ChooseOption(format!("count:{flt_raw}")));
    }
    if let Some((who_raw, rest)) = split_once_any(&low, &[" attaches ", " attach "])
        && parse_player_ref(who_raw).is_some()
        && let Some((a_raw, b_raw)) = rest.split_once(" to ")
        && let (Ok(a), Ok(b)) = (
            parse_subject_or_target(a_raw, vocab),
            parse_subject_or_target(b_raw, vocab),
        )
    {
        return ok1(InstructionAst::Attach {
            subject: Box::new(a),
            target: Box::new(b),
            any_number: false,
            all: false,
        });
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
                return ok1(InstructionAst::RevealFromLooked {
                    filter: Box::new(flt),
                    to: "hand".to_string(),
                });
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
                out.push(ins_mod_pt(
                    subj.clone(),
                    dx.clone(),
                    dy.clone(),
                    dur.clone(),
                ));
                if !kws.is_empty() {
                    out.push(ins_grant_kw(subj, kws.clone(), dur.clone()));
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
        return ok1(InstructionAst::AnimateOrSetBasePT {
            subject: self_obj(),
            power: p,
            toughness: t,
            description: Some(desc.to_string()),
            keywords: kws,
            duration: DurationAst::Permanent,
        });
    }
    // General "play/cast [up to N of | any number of] <subj_or_filter> [without paying ...]"
    let s_nodur_cp_low = s_nodur.to_ascii_lowercase();
    if let Some((cast_only, after_cp)) = s_nodur_cp_low
        .strip_prefix("cast ")
        .map(|r| (true, r))
        .or_else(|| s_nodur_cp_low.strip_prefix("play ").map(|r| (false, r)))
    {
        let (core, free) = match strip_suffix_any(
            &after_cp,
            &[
                " without paying their mana costs",
                " without paying its mana cost",
                " if able without paying their mana costs",
                " if able without paying its mana cost",
            ],
        ) {
            Some(c) => (c.trim(), true),
            None => (after_cp.trim(), false),
        };
        let (core_clean, count_limit) = if let Some(rest) = core.strip_prefix("any number of ") {
            (rest.trim(), Some(CountLimitAst::AnyNumber))
        } else if let Some(rest) = core.strip_prefix("up to ")
            && let Some((cnt_tok, group_raw)) = rest.split_once(" of ")
            && let Ok(cnt) = parse_value_expr(cnt_tok.trim(), vocab)
        {
            (group_raw.trim(), Some(CountLimitAst::UpTo(Box::new(cnt))))
        } else {
            (core, None)
        };
        if let Ok(subj) = parse_subject_or_target(core_clean, vocab) {
            return ok1(InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only,
                subject: Box::new(subj),
                free,
                duration: duration_ast_to_slug(&dur),
                count_limit,
                alt_cost: None,
            });
        }
    }

    if let Some(trig) = try_parse_inline_trigger(s, dur.clone(), vocab) {
        return Ok(vec![trig]);
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
            let (subj_raw_0, exc_opt_0) = if let Some((l, r)) = after_tc.split_once(", except ") {
                (l.trim(), Some(r.trim().to_string()))
            } else if let Some((l, r)) = after_tc.split_once(" except ") {
                (l.trim(), Some(r.trim().to_string()))
            } else if let Some((l, r)) = after_tc.split_once(" and that's blocking ") {
                (l.trim(), Some(format!("blocking {}", r.trim())))
            } else {
                (after_tc, None)
            };
            let mut subj_raw = subj_raw_0;
            let mut extra_mods: Vec<String> = Vec::new();
            for combat_suf in [
                " tapped and attacking that player",
                " that are tapped and attacking that player",
                " and that are tapped and attacking that player",
                " that's tapped and attacking that player",
                " and that's tapped and attacking that player",
                " that are tapped and attacking",
                " and that are tapped and attacking",
                " that's tapped and attacking",
                " and that's tapped and attacking",
                " tapped and attacking",
                " tapped",
            ] {
                if let Some(prefix) = subj_raw
                    .to_ascii_lowercase()
                    .strip_suffix(combat_suf)
                    .map(|p| &subj_raw[..p.len()])
                {
                    subj_raw = prefix.trim();
                    extra_mods.push(combat_suf.trim().to_string());
                    break;
                }
            }
            let exc_opt = match (exc_opt_0, extra_mods.is_empty()) {
                (Some(e), true) => Some(e),
                (None, true) => None,
                (Some(e), false) => Some(format!("{}; {e}", extra_mods.join(", "))),
                (None, false) => Some(extra_mods.join(", ")),
            };
            if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
                return Ok(vec![ins_copy(subj, exc_opt, None)]);
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
            return ok1(InstructionAst::MoveCounters {
                from: Box::new(src),
                to: Box::new(dst),
                counter_kind: cnt_part.trim().to_string(),
                all: cnt_part.starts_with("all "),
                any_number: cnt_part.starts_with("any number"),
                count: Some(Box::new(ValueExprAst::ChosenX)),
            });
        }
    }
    if let Some(after_rm) = low.strip_prefix("remove ")
        && let Some((cnt_part, src_raw)) = after_rm.split_once(" from ")
        && (cnt_part.contains("counter") || cnt_part == "one" || cnt_part == "all")
        && let Ok(src) = parse_subject_or_target(src_raw.trim(), vocab)
    {
        let all = cnt_part == "all" || cnt_part.starts_with("all ");
        let (cnt, kind) = if cnt_part == "one" {
            (Some(vconst(1)), "same".to_string())
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
                strip_suffix_any(&cnt_part, &[" counters", " counter"]).unwrap_or(cnt_part),
                vocab,
            );
            (Some(Box::new(c)), k.to_string())
        };
        return ok1(InstructionAst::RemoveCounters {
            subject: Box::new(src),
            counter_kind: kind,
            all,
            count: cnt,
        });
    }

    // Compositional Wave 16: General type/Aura transformation and quoted-ability grants
    // Only emit GrantAbilityText when the quoted ability itself strictly compiles (keeps 0 new string escapes!).
    if let Some(after_is) = strip_prefix_any(
        &s_nodur,
        &[
            "It's a ",
            "It's an ",
            "it's a ",
            "it's an ",
            "They are ",
            "they are ",
            "They're ",
            "they're ",
        ],
    ) && !after_is.is_empty()
    {
        let mut out = vec![InstructionAst::AnimateOrSetBasePT {
            subject: box_anaphora("it"),
            power: String::new(),
            toughness: String::new(),
            description: Some(after_is.to_string()),
            keywords: Vec::new(),
            duration: dur.clone(),
        }];
        if let Some((_, spans, _)) = extract_quoted_spans(after_is) {
            let mut all_ok = true;
            for q_text in spans {
                if let Some(compiled) = compile_granted_ability(q_text, vocab) {
                    out.push(InstructionAst::GrantAbilityText {
                        subject: box_anaphora("it"),
                        ability_text: q_text.to_string(),
                        duration: dur.clone(),
                        compiled: Some(compiled),
                    });
                } else {
                    all_ok = false;
                    break;
                }
            }
            if all_ok {
                return Ok(out);
            }
        } else if !after_is.contains('"') {
            return Ok(out);
        }
    }
    if let Some((subj_raw, rest)) =
        split_once_any(&s_nodur, &[" gains ", " gain ", " has ", " have "])
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let r_trim = rest.trim();
        let r_low = r_trim.to_ascii_lowercase();
        if r_low == "that ability"
            || r_low.starts_with("all activated abilities of ")
            || r_low.starts_with("landwalk of the chosen type")
        {
            return Ok(vec![ins_grant_kw(subj, vec![r_low], dur.clone())]);
        }
        if r_trim.contains('"')
            && let Some(mixed) =
                parse_mixed_keywords_and_quoted_abilities(&subj, r_trim, &dur, vocab)
        {
            return Ok(mixed);
        }
    }
    if let Some((subj_raw, rest)) = split_once_any(
        &s_nodur,
        &[" becomes a ", " becomes an ", " become a ", " become an "],
    ) && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        && (rest.contains(" named ") || rest.contains('"'))
    {
        let (power, toughness, keywords) = parse_named_or_quoted_animation_fields(rest, vocab);
        let mut out = vec![InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(subj.clone()),
            power,
            toughness,
            description: Some(rest.trim().to_string()),
            keywords,
            duration: dur_perm.clone(),
        }];
        if let Some(q_start) = rest.find('"')
            && let Some(q_end) = rest.rfind('"')
            && q_end > q_start
        {
            let q_text = &rest[q_start + 1..q_end];
            if let Some(compiled) = compile_granted_ability(q_text, vocab) {
                out.push(ins_grant_ab(
                    subj,
                    q_text.to_string(),
                    Some(compiled),
                    dur_perm.clone(),
                ));
                return Ok(out);
            }
        } else {
            return Ok(out);
        }
    }
    let s_nodur_low_w16 = s_nodur.to_ascii_lowercase();
    if let Some(subj_raw) =
        strip_suffix_any(&s_nodur_low_w16, &[" becomes saddled", " become saddled"])
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(vec![ins_combat_rule(subj, "becomes_saddled", dur.clone())]);
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
        return Ok(vec![ins_combat_rule(subj, verb.to_string(), dur.clone())]);
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
            return ok1(InstructionAst::RevealFromLooked {
                filter: Box::new(flt),
                to: "look".to_string(),
            });
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
            ins_dmg(src.clone(), t1, amt.clone()),
            ins_dmg(src, t2, amt),
        ]);
    }
    // General "do the same for <filter>" / "repeat the following process <val> times" / "clash with <player>"
    if let Some(flt_raw) = low.strip_prefix("do the same for ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(vec![ins_ability_restr(low)]);
    }
    if let Some(val_raw) = low
        .strip_prefix("repeat the following process ")
        .and_then(|r| r.strip_suffix(" times"))
        && let Ok(cnt) = parse_value_expr(val_raw, vocab)
    {
        return ok1(ins_for_each_count(
            cnt,
            vec![ins_ability_restr("repeat_following_process")],
        ));
    }
    if let Some(who_raw) = low.strip_prefix("clash with ")
        && parse_player_ref(who_raw).is_some()
    {
        return Ok(vec![ins_ability_restr(low)]);
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
            return ok1(InstructionAst::InteractiveChoice(
                InteractiveChoiceKind::ChooseFromGroup {
                    chooser: who,
                    description: after_ch.to_string(),
                },
            ));
        }
    }
    // General multi-symbol mana list: "Add {W}{W}, {U}{U}, {B}{B}, {R}{R}, and {G}{G}"
    if let Some(after_add) = s.strip_prefix("Add ").or_else(|| s.strip_prefix("add "))
        && after_add.starts_with('{')
        && after_add.contains(", and ")
    {
        let joined = after_add.replace(", and ", "").replace(", ", "");
        if is_mana_sequence(&joined) {
            return ok1(ins_add_mana(
                PlayerRefAst::You,
                crate::ast::ManaSpecAst::Fixed(joined),
            ));
        }
    }

    // Compositional Wave 17: Out-of-line recursive combinators (keeps LLVM fast + handles remaining general effect compositions)
    // Cluster 4: Generalized subject-player action ("Players dealt damage this way can't cast...", "Target spell's controller loses 3 life")
    for verb in [
        " can't ",
        " loses ",
        " lose ",
        " gains ",
        " gain ",
        " draws ",
        " draw ",
        " discards ",
        " discard ",
        " sacrifices ",
        " sacrifice ",
        " exiles ",
        " exile ",
        " mills ",
        " mill ",
        " returns ",
        " return ",
        " puts ",
        " put ",
        " shuffles ",
        " shuffle ",
        " reveals ",
        " reveal ",
        " creates ",
        " create ",
        " may ",
    ] {
        if let Some(vidx) = low.find(verb) {
            let subj_part = s[..vidx].trim();
            if subj_part.len() >= 6
                && parse_player_ref(subj_part).is_none()
                && let Ok(
                    ps @ (SubjectRefAst::ControllerOf(_)
                    | SubjectRefAst::OwnerOf(_)
                    | SubjectRefAst::PlayerMatching { .. }),
                ) = parse_subject_or_target(subj_part, vocab)
            {
                let rewritten = format!("that player{}", &s[vidx..]);
                if let Ok(inner) = parse_atomic_effect(&rewritten, vocab) {
                    return Ok(vec![ins_fe_subj(ps, inner)]);
                }
            }
        }
    }

    if let Some(w17) = choices_misc::parse_extended_effect_fallbacks(s, &low, s_nodur, &dur, vocab)
    {
        return Ok(w17);
    }

    // Exposed multi-sentence trigger clauses (Pass E13-B: no longer swallowed by buggy period-crossing trigger scanner)
    if low == "each player rolls a d20" {
        return ok1(ins_for_each_player(
            PlayerRefAst::EachPlayer,
            vec![InstructionAst::RollDie { sides: 20 }],
        ));
    }
    if let Some(dest) = low
        .strip_prefix("put both cards onto ")
        .and_then(|r| r.strip_suffix(", then shuffle"))
    {
        return Ok(vec![
            ins_move(anaphora("both cards"), dest.trim().to_string()),
            ins_shuffle(PlayerRefAst::You),
        ]);
    }
    if let Some((highest, act_raw)) = low
        .strip_prefix("the player with the highest life total ")
        .map(|r| (true, r))
        .or_else(|| {
            low.strip_prefix("the player with the lowest life total ")
                .map(|r| (false, r))
        })
        && let Ok(acts) = parse_atomic_effect(&format!("that player {}", act_raw.trim()), vocab)
        && acts.len() == 1
    {
        return ok1(InstructionAst::ExtremeLifePlayerAction {
            highest,
            action: Box::new(acts.into_iter().next().expect("checked len == 1")),
        });
    }
    if let Some(after) = low.strip_prefix("repeat the following process for ") {
        return Ok(vec![ins_repeat(None, vec![after.trim().to_string()], None)]);
    }
    if let Some(after) = low.strip_prefix("you do the same with ") {
        return ok1(ins_repeat(
            Some(ValueExprAst::Const(1)),
            vec![after.trim().to_string()],
            None,
        ));
    }
    if low == "for each other result, it gains the indicated ability until end of turn" {
        return ok1(InstructionAst::GrantKeywords {
            subject: box_anaphora("it"),
            keywords: vec!["indicated_ability".to_string()],
            duration: DurationAst::UntilEndOfTurn,
        });
    }
    if let Some((players_raw, rest)) = low.split_once(" each manifest ")
        && let Some((cnt_raw, tail)) = rest.split_once(" card")
        && let (Ok(players), Ok(count)) = (
            parse_subject_or_target(players_raw.trim(), vocab),
            parse_value_expr(cnt_raw.trim(), vocab),
        )
    {
        let zone_desc = tail.trim_start_matches('s').trim();
        let subj_desc = if zone_desc.is_empty() {
            None
        } else {
            Some(Box::new(SubjectRefAst::Anaphora(format!(
                "cards {zone_desc}"
            ))))
        };
        return ok1(ins_fe_subj(
            players,
            vec![InstructionAst::Manifest {
                count: Box::new(count),
                dread: false,
                subject: subj_desc,
            }],
        ));
    }
    if let Some(after) = low.strip_prefix("that player chooses target player who controls more ")
        && after.ends_with(" than they do and is their opponent")
    {
        return ok1(InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::ChooseFromGroup {
                chooser: PlayerRefAst::ThatPlayer,
                description: low,
            },
        ));
    }
    if low.starts_with("an opponent guesses which ") && low.contains(", then you reveal ") {
        return ok1(InstructionAst::InteractiveChoice(
            InteractiveChoiceKind::SecretVote { prompt: low },
        ));
    }
    if let Some(rest_okoye) =
        low.strip_prefix("whenever that creature attacks the monarch this turn, ")
        && let Ok(body) = parse_atomic_effect(rest_okoye.trim(), vocab)
    {
        return ok1(ins_delayed_ev(
            TriggerSpecAst {
                events: vec![TriggerEventAst::Attacks {
                    subject: Some(box_anaphora("that creature")),
                    actor: None,
                    qualifier: Some("the monarch".to_string()),
                    grouped: false,
                }],
                intervening_if: None,
            },
            body,
        ));
    }
    if !s.contains('"')
        && !low.starts_with("if ")
        && !low.starts_with("when ")
        && !low.starts_with("whenever ")
        && let Some((head, tail)) = s.rsplit_once(", then ")
        && head.contains(", ")
    {
        let mut acc = Vec::new();
        let mut all_ok = true;
        for part in head.split(", ").chain(std::iter::once(tail)) {
            let p = part.trim();
            if p.is_empty() {
                all_ok = false;
                break;
            }
            if let Ok(mut sub) = parse_atomic_effect(p, vocab) {
                acc.append(&mut sub);
            } else {
                all_ok = false;
                break;
            }
        }
        if all_ok && acc.len() >= 3 {
            return Ok(acc);
        }
    }
    if let Some(who_raw) = low
        .strip_suffix(" mills a card for each 1 damage dealt to them")
        .or_else(|| low.strip_suffix(" mill a card for each 1 damage dealt to them"))
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return ok1(InstructionAst::Mill {
            player,
            count: Box::new(ValueExprAst::PriorResult),
        });
    }
    if let Some(after) = low.strip_prefix("you may reveal ")
        && let Some((subj_raw, cond_raw)) = after.split_once(" if it has ")
        && let Ok(subject) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let status = match cond_raw
            .trim()
            .strip_suffix(" in its mana cost")
            .unwrap_or(cond_raw.trim())
        {
            "a mana ability" => "has_mana_ability".to_string(),
            "cycling" => "cycling".to_string(),
            "flash" => "flash".to_string(),
            "flying" => "flying".to_string(),
            other => other.to_string(),
        };
        return ok1(ins_if(
            cond_status(subject.clone(), status),
            vec![ins_opt1(InstructionAst::RevealByPlayer {
                player: PlayerRefAst::You,
                subject: Box::new(subject),
            })],
        ));
    }
    if let Some(after) = low.strip_prefix("add ")
        && let Some((cnt_raw, tail)) =
            after.split_once(" mana in any combination of its colors and ")
        && let (Ok(amount), Ok(mut follow)) = (
            parse_value_expr(cnt_raw.trim(), vocab),
            parse_atomic_effect(tail.trim(), vocab),
        )
    {
        let mut out = vec![InstructionAst::AddQualifiedMana(Box::new(
            crate::ast::QualifiedManaAst {
                player: PlayerRefAst::You,
                kind: "any_combination_of_subject_colors".to_string(),
                amount: Some(Box::new(amount)),
                source: Some(box_anaphora("it")),
                choices: Vec::new(),
            },
        ))];
        out.append(&mut follow);
        return Ok(out);
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
            return Ok(vec![ins_if(cond, then_body)]);
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
        return ok1(InstructionAst::ManualTabletopAction {
            prompt: s.to_string(),
        });
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
            return Some(ins_temp_trig(trigger, body, duration));
        }
    }
    None
}

pub(crate) fn parse_effect_or_temp_clause(
    clause: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    if let Ok(v) = parse_atomic_effect(clause, vocab)
        && !v.is_empty()
    {
        return Some(v);
    }
    if clause.len() <= 260
        && let Ok(sr) = parse_static_or_replacement(clause, vocab)
    {
        let eff_dur = if *dur == DurationAst::Instant {
            DurationAst::UntilEndOfTurn
        } else {
            dur.clone()
        };
        return some1(match sr {
            StaticOrReplacement::Static(st) => ins_temp_static(st, eff_dur),
            StaticOrReplacement::Replacement(rep) => ins_temp_rep(rep, eff_dur),
        });
    }
    None
}

pub(crate) fn find_top_level_pattern(s: &str, pat: &str) -> Option<usize> {
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

/// Parse an untap clause body (after `untap ` / `<player> untap(s) `), preserving
/// non-targeted cardinality (`up to N/X/that many`, exact `N/X/that many`, `a number of ... equal to ...`)
/// losslessly on `InstructionAst::UntapPermanent` while keeping targeted phrases on `SubjectRefAst::TargetObject`.
pub(super) fn parse_untap_permanent_instruction(
    rest: &str,
    vocab: &Vocabulary,
) -> Option<InstructionAst> {
    let clean = rest.trim().trim_end_matches('.');
    if clean.is_empty() {
        return None;
    }
    let low = clean.to_ascii_lowercase();
    if !low.contains("target") {
        if let Some(after) = low.strip_prefix("up to ") {
            if let Some(flt_raw) = after.strip_prefix("that many ")
                && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
            {
                return Some(InstructionAst::UntapPermanent {
                    subject: Box::new(grp(flt)),
                    count: Some(Box::new(ValueExprAst::PriorResult)),
                    up_to: true,
                });
            }
            if let Some(after_num) = after.strip_prefix("a number of ")
                && let Some((flt_raw, val_raw)) = after_num.split_once(" equal to ")
                && let (Ok(flt), Ok(val)) = (
                    parse_object_filter(flt_raw.trim(), vocab),
                    parse_value_expr(val_raw.trim(), vocab),
                )
            {
                return Some(InstructionAst::UntapPermanent {
                    subject: Box::new(grp(flt)),
                    count: Some(Box::new(val)),
                    up_to: true,
                });
            }
            if let Some((cnt_tok, flt_raw)) = after.split_once(' ') {
                let count = if cnt_tok == "x" {
                    Some(ValueExprAst::ChosenX)
                } else if let Some(n) = parse_number_word(cnt_tok)
                    && n >= 0
                {
                    Some(ValueExprAst::Const(n))
                } else {
                    None
                };
                if let Some(val) = count
                    && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
                {
                    return Some(InstructionAst::UntapPermanent {
                        subject: Box::new(grp(flt)),
                        count: Some(Box::new(val)),
                        up_to: true,
                    });
                }
            }
        }
        if let Some(after_num) = low.strip_prefix("a number of ")
            && let Some((flt_raw, val_raw)) = after_num.split_once(" equal to ")
            && let (Ok(flt), Ok(val)) = (
                parse_object_filter(flt_raw.trim(), vocab),
                parse_value_expr(val_raw.trim(), vocab),
            )
        {
            return Some(InstructionAst::UntapPermanent {
                subject: Box::new(grp(flt)),
                count: Some(Box::new(val)),
                up_to: false,
            });
        }
        if let Some(flt_raw) = low.strip_prefix("that many ")
            && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            return Some(InstructionAst::UntapPermanent {
                subject: Box::new(grp(flt)),
                count: Some(Box::new(ValueExprAst::PriorResult)),
                up_to: false,
            });
        }
        if let Some((cnt_tok, flt_raw)) = low.split_once(' ') {
            let count = if cnt_tok == "x" {
                Some(ValueExprAst::ChosenX)
            } else if let Some(n) = parse_number_word(cnt_tok)
                && n >= 2
            {
                Some(ValueExprAst::Const(n))
            } else {
                None
            };
            if let Some(val) = count
                && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
            {
                return Some(InstructionAst::UntapPermanent {
                    subject: Box::new(grp(flt)),
                    count: Some(Box::new(val)),
                    up_to: false,
                });
            }
        }
    }
    let subj = parse_subject_or_target(clean, vocab).ok()?;
    Some(InstructionAst::UntapPermanent {
        subject: Box::new(subj),
        count: None,
        up_to: false,
    })
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

/// True when a sentence is a standalone additional-combat clause (not a compound sentence
/// carrying an `if` condition, an `untap` action, or a chained `, then ` / ` and ` action).
fn is_standalone_additional_combat_clause(low: &str) -> bool {
    if !low.contains("additional combat phase") {
        return false;
    }
    if low.starts_with("if ")
        || low.starts_with("untap ")
        || low.contains(" if ")
        || low.contains(", then ")
        || low.contains(" untap ")
        || low.contains(", and ")
    {
        return false;
    }
    if low.contains(" and ")
        && !low.contains("and after this")
        && !low.contains("and an additional main phase")
        && !low.contains("followed by an additional")
    {
        return false;
    }
    low.contains("there is an additional combat phase")
        || low.contains("there's an additional combat phase")
        || low.contains("get an additional combat phase")
        || low.contains("gets an additional combat phase")
        || low.contains("take an additional combat phase")
        || low.contains("takes an additional combat phase")
}

/// Extract the leading subject phrase from a `<subj> <verb> ...` clause for shared-subject carry-forward.
pub(crate) fn extract_leading_subject_prefix(clause: &str) -> Option<&str> {
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
        " enters ",
        " enter ",
    ] {
        if let Some(idx) = low.find(needle) {
            let prefix = clause[..idx].trim();
            let prefix = strip_suffix_any(prefix, &[" each", " Each"])
                .unwrap_or(prefix)
                .trim();
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

pub(crate) fn substitute_chosen_x(expr: &mut ValueExprAst, x_val: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ChosenX => {
            *expr = x_val.clone();
            true
        }
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::AbsDiff(a, b)
        | ValueExprAst::Max(a, b)
        | ValueExprAst::Min(a, b) => {
            let ca = substitute_chosen_x(a, x_val);
            let cb = substitute_chosen_x(b, x_val);
            ca || cb
        }
        ValueExprAst::DivConst { expr: inner, .. } => substitute_chosen_x(inner, x_val),
        ValueExprAst::Pow { base, exp } => {
            let cb = substitute_chosen_x(base, x_val);
            let ce = substitute_chosen_x(exp, x_val);
            cb || ce
        }
        ValueExprAst::Shortfall { threshold, actual } => {
            let ct = substitute_chosen_x(threshold, x_val);
            let ca = substitute_chosen_x(actual, x_val);
            ct || ca
        }
        ValueExprAst::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            let ct = substitute_chosen_x(then_expr, x_val);
            let ce = substitute_chosen_x(else_expr, x_val);
            ct || ce
        }
        _ => false,
    }
}

fn substitute_chosen_x_in_filter(flt: &mut ObjectFilterAst, x_val: &ValueExprAst) -> bool {
    let mut changed = false;
    for sb in &mut flt.stat_bounds {
        if substitute_chosen_x(&mut sb.value, x_val) {
            changed = true;
        }
    }
    for pred in &mut flt.predicates {
        match pred {
            FilterPredicateAst::StatRelationTo { expr, .. }
            | FilterPredicateAst::ColorCountCompare { expr, .. } => {
                if substitute_chosen_x(expr, x_val) {
                    changed = true;
                }
            }
            FilterPredicateAst::AnyOf(subs) => {
                for sub in subs {
                    if substitute_chosen_x_in_filter(sub, x_val) {
                        changed = true;
                    }
                }
            }
            _ => {}
        }
    }
    changed
}

fn substitute_chosen_x_in_subject(subj: &mut SubjectRefAst, x_val: &ValueExprAst) -> bool {
    match subj {
        SubjectRefAst::TargetObject { filter, .. }
        | SubjectRefAst::DistinctTarget { filter, .. }
        | SubjectRefAst::Each(filter)
        | SubjectRefAst::All(filter)
        | SubjectRefAst::Group(filter) => substitute_chosen_x_in_filter(filter, x_val),
        SubjectRefAst::InZone { subject, .. }
        | SubjectRefAst::ControllerOf(subject)
        | SubjectRefAst::OwnerOf(subject) => substitute_chosen_x_in_subject(subject, x_val),
        SubjectRefAst::Compound(items) | SubjectRefAst::TargetOneOf { options: items, .. } => {
            let mut changed = false;
            for item in items {
                if substitute_chosen_x_in_subject(item, x_val) {
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}

fn substitute_chosen_x_in_instructions(body: &mut [InstructionAst], x_val: &ValueExprAst) -> bool {
    let mut changed = false;
    for ins in body {
        match ins {
            InstructionAst::PreventDamage { amount, .. }
            | InstructionAst::DealDamage { amount, .. } => {
                if substitute_chosen_x(amount, x_val) {
                    changed = true;
                }
            }
            InstructionAst::Draw { count, .. }
            | InstructionAst::Mill { count, .. }
            | InstructionAst::Scry { count, .. }
            | InstructionAst::Surveil { count, .. }
            | InstructionAst::PutCounters { count, .. } => {
                if substitute_chosen_x(count, x_val) {
                    changed = true;
                }
            }
            InstructionAst::ChangeLife { delta, .. } => {
                if substitute_chosen_x(delta, x_val) {
                    changed = true;
                }
            }
            InstructionAst::Optional(inner)
            | InstructionAst::OptionalPlayer { body: inner, .. }
            | InstructionAst::IfCondition {
                then_body: inner, ..
            }
            | InstructionAst::ForEachPlayer { body: inner, .. }
            | InstructionAst::ForEachObject { body: inner, .. } => {
                if substitute_chosen_x_in_instructions(inner, x_val) {
                    changed = true;
                }
            }
            _ => {}
        }
    }
    changed
}

pub(crate) fn attach_where_x(target: &mut StaticOrReplacement, x_val: ValueExprAst) -> bool {
    match target {
        StaticOrReplacement::Static(StaticAbilityAst::ContinuousModifyPT {
            subject,
            dx,
            dy,
            scale,
            ..
        }) => {
            let mut changed = substitute_chosen_x_in_subject(subject, &x_val);
            if scale.is_none()
                && (dx.contains('X') || dx.contains('x') || dy.contains('X') || dy.contains('x'))
            {
                *scale = Some(Box::new(x_val));
                changed = true;
            }
            changed
        }
        StaticOrReplacement::Static(StaticAbilityAst::GrantKeywords { subject, .. }) => {
            substitute_chosen_x_in_subject(subject, &x_val)
        }
        StaticOrReplacement::Static(StaticAbilityAst::AlternativeCastCost {
            cost,
            for_spells,
            ..
        }) => {
            let mut changed = for_spells
                .as_deref_mut()
                .is_some_and(|f| substitute_chosen_x_in_filter(f, &x_val));
            for atom in cost {
                if matches!(atom, CostAtomAst::PayMana(m) if m.eq_ignore_ascii_case("{x}")) {
                    *atom = CostAtomAst::PayGenericManaExpr(Box::new(x_val.clone()));
                    changed = true;
                }
            }
            changed
        }
        StaticOrReplacement::Static(StaticAbilityAst::PreventAndReplaceDamage {
            follow_up,
            ..
        })
        | StaticOrReplacement::Static(StaticAbilityAst::FollowUp(follow_up)) => {
            substitute_chosen_x_in_instructions(follow_up, &x_val)
        }
        StaticOrReplacement::Static(
            StaticAbilityAst::CostModifier { scale_expr, .. }
            | StaticAbilityAst::CostModifierWithFloor { scale_expr, .. },
        ) => {
            if scale_expr.is_none() {
                *scale_expr = Some(Box::new(x_val));
                true
            } else {
                false
            }
        }
        StaticOrReplacement::Static(StaticAbilityAst::DynamicBasePT {
            power, toughness, ..
        }) => {
            let cp = substitute_chosen_x(power, &x_val);
            let ct = substitute_chosen_x(toughness, &x_val);
            cp || ct
        }
        StaticOrReplacement::Static(StaticAbilityAst::DynamicModifyPT {
            power_delta,
            toughness_delta,
            ..
        }) => {
            let cp = substitute_chosen_x(power_delta, &x_val);
            let ct = substitute_chosen_x(toughness_delta, &x_val);
            cp || ct
        }
        StaticOrReplacement::Static(StaticAbilityAst::CdaPowerToughness { power, toughness }) => {
            let cp = power
                .as_deref_mut()
                .is_some_and(|p| substitute_chosen_x(p, &x_val));
            let ct = toughness
                .as_deref_mut()
                .is_some_and(|t| substitute_chosen_x(t, &x_val));
            cp || ct
        }
        StaticOrReplacement::Static(StaticAbilityAst::PreventThresholdDamage {
            max_damage,
            ..
        }) => substitute_chosen_x(max_damage, &x_val),
        StaticOrReplacement::Static(StaticAbilityAst::ModifyMaximumHandSize { size, .. }) => {
            substitute_chosen_x(size, &x_val)
        }
        StaticOrReplacement::Static(StaticAbilityAst::CrewOrSaddlePowerBonus { bonus, .. }) => {
            substitute_chosen_x(bonus, &x_val)
        }
        StaticOrReplacement::Static(
            StaticAbilityAst::OncePerTurn(inner)
            | StaticAbilityAst::OnceDuringYourTurn(inner)
            | StaticAbilityAst::Otherwise(inner)
            | StaticAbilityAst::ScaledBy { inner, .. },
        ) => {
            let mut wrapped = StaticOrReplacement::Static((**inner).clone());
            if attach_where_x(&mut wrapped, x_val)
                && let StaticOrReplacement::Static(updated) = wrapped
            {
                **inner = updated;
                return true;
            }
            false
        }
        StaticOrReplacement::Static(StaticAbilityAst::Compound(items)) => {
            let mut any = false;
            for item in items {
                let mut wrapped = StaticOrReplacement::Static(item.clone());
                if attach_where_x(&mut wrapped, x_val.clone())
                    && let StaticOrReplacement::Static(updated) = wrapped
                {
                    *item = updated;
                    any = true;
                }
            }
            any
        }
        StaticOrReplacement::Replacement(ReplacementEffectAst::EntersWithCounters {
            x_expr,
            ..
        }) => {
            if x_expr.is_none() {
                *x_expr = Some(Box::new(x_val));
                true
            } else {
                false
            }
        }
        StaticOrReplacement::Replacement(ReplacementEffectAst::OthersEnterWithCounters {
            count,
            per,
            ..
        }) => {
            if substitute_chosen_x(count, &x_val) {
                true
            } else if per.is_none() {
                *per = Some(Box::new(x_val));
                true
            } else {
                false
            }
        }
        StaticOrReplacement::Replacement(ReplacementEffectAst::EventInstead {
            instead_body: Some(body),
            ..
        }) => substitute_chosen_x_in_instructions(body, &x_val),
        StaticOrReplacement::Replacement(ReplacementEffectAst::EntersTappedWithCounters {
            count,
            ..
        })
        | StaticOrReplacement::Replacement(ReplacementEffectAst::EntersTappedAndWithCounters {
            count,
            ..
        })
        | StaticOrReplacement::Replacement(ReplacementEffectAst::EscapesWithCounters {
            count,
            ..
        }) => substitute_chosen_x(count, &x_val),
        StaticOrReplacement::Replacement(ReplacementEffectAst::Compound(items)) => {
            let mut any = false;
            for item in items {
                let mut wrapped = StaticOrReplacement::Replacement(item.clone());
                if attach_where_x(&mut wrapped, x_val.clone())
                    && let StaticOrReplacement::Replacement(updated) = wrapped
                {
                    *item = updated;
                    any = true;
                }
            }
            any
        }
        _ => false,
    }
}

/// Pass E12: Parse both `TurnStepAst` and `DelayedStepScope` from a delayed-trigger header.
pub(super) fn parse_delayed_step_and_scope(hdr_low: &str) -> (TurnStepAst, DelayedStepScope) {
    let step = if hdr_low.contains("end of combat") {
        TurnStepAst::EndOfCombat
    } else if hdr_low.contains("upkeep") {
        TurnStepAst::Upkeep
    } else if hdr_low.contains("draw") {
        TurnStepAst::DrawStep
    } else if hdr_low.contains("combat") {
        TurnStepAst::BeginningOfCombat
    } else if hdr_low.contains("precombat main") {
        TurnStepAst::PrecombatMain
    } else if hdr_low.contains("postcombat main") {
        TurnStepAst::PostcombatMain
    } else if hdr_low.contains("main") {
        TurnStepAst::MainPhase
    } else if hdr_low.contains("untap") {
        TurnStepAst::UntapStep
    } else if hdr_low.contains("cleanup") {
        TurnStepAst::CleanupStep
    } else {
        TurnStepAst::EndStep
    };
    let scope = if hdr_low.contains("each of your") {
        DelayedStepScope::EachOfYour
    } else if hdr_low.contains("each opponent") {
        DelayedStepScope::EachOpponents
    } else if hdr_low.contains("each player") {
        DelayedStepScope::EachPlayers
    } else if hdr_low.contains("your next") || hdr_low.contains(" your ") {
        DelayedStepScope::YourNext
    } else if hdr_low.contains("that player's")
        || hdr_low.contains("their next")
        || hdr_low.contains("that opponent's")
        || hdr_low.contains("its controller's")
        || hdr_low.contains("defending player's")
    {
        DelayedStepScope::ThatPlayersNext
    } else if hdr_low.contains("each ") {
        DelayedStepScope::Each
    } else {
        DelayedStepScope::Next
    };
    (step, scope)
}

pub(super) fn duration_ast_to_slug(dur: &DurationAst) -> String {
    match dur {
        DurationAst::Instant => "now".to_string(),
        DurationAst::UntilEndOfCombat => "until_end_of_combat".to_string(),
        DurationAst::UntilEndOfTurn => "this_turn".to_string(),
        DurationAst::UntilYourNextTurn => "until_your_next_turn".to_string(),
        DurationAst::UntilEndOfNextTurn => "until_end_of_next_turn".to_string(),
        DurationAst::UntilEndOfPlayersNextTurn(_) => "until_end_of_players_next_turn".to_string(),
        DurationAst::Permanent => "permanent".to_string(),
        DurationAst::UntilSourceLeaves => "until_source_leaves".to_string(),
        DurationAst::ForAsLongAs(_) => "for_as_long_as".to_string(),
    }
}

pub(super) fn rebind_implicit_you_actor(instructions: &mut [InstructionAst], actor: PlayerRefAst) {
    for ins in instructions {
        match ins {
            InstructionAst::Sacrifice { player, .. }
            | InstructionAst::SacrificeAllExcept { player, .. }
            | InstructionAst::SacrificeAnyNumber { player, .. }
            | InstructionAst::Discard { player, .. }
            | InstructionAst::DiscardUpTo { player, .. }
            | InstructionAst::DiscardHand { player }
            | InstructionAst::Draw { player, .. }
            | InstructionAst::ChangeLife { player, .. }
            | InstructionAst::Mill { player, .. }
            | InstructionAst::ShuffleLibrary { player }
            | InstructionAst::Scry { player, .. }
            | InstructionAst::Surveil { player, .. }
            | InstructionAst::RevealHand { player }
            | InstructionAst::RevealByPlayer { player, .. } => {
                if *player == PlayerRefAst::You {
                    *player = actor;
                }
            }
            InstructionAst::PayCost(cost) => {
                let c = cost.clone();
                *ins = InstructionAst::PlayerPayCost {
                    player: actor,
                    cost: c,
                };
            }
            InstructionAst::Optional(body)
            | InstructionAst::IfPreviousSucceeded(body)
            | InstructionAst::IfPreviousFailed(body)
            | InstructionAst::Otherwise(body) => {
                rebind_implicit_you_actor(body, actor);
            }
            InstructionAst::IfCondition { then_body, .. } => {
                rebind_implicit_you_actor(then_body, actor);
            }
            InstructionAst::Choice(branches) => {
                for br in branches {
                    rebind_implicit_you_actor(br, actor);
                }
            }
            _ => {}
        }
    }
}
