//! Cast permissions, mana production and spend riders, alternative costs, extra turns/steps,
//! spell copying, and resolution-control effect parsers.

use super::parse_atomic_effect;
use crate::ast::{
    AltCastCostAst, ColorArity, CompareOp, ControllerRelation, CostAtomAst, CountLimitAst,
    DurationAst, FilterPredicateAst, InstructionAst, ManaSpecAst, ModifiedCastPermissionAst,
    ObjectFilterAst, ObjectFilterFlags, PlayerRefAst, StatBoundAst, StatKind, StaticAbilityAst,
    SubjectRefAst, TargetCountAst, TurnStepAst, ValueExprAst, ZoneFilter,
};
use crate::clauses::costs::parse_cost_atom;
use crate::combinators::{
    anaphora, any_card_flt, any_spell_flt, box_anaphora, grp, ins_add_mana, ins_choose_group,
    ins_counters, ins_dmg, ins_for_each_count, ins_grant_cast, ins_if, ins_move, ins_opt1,
    ins_temp_static, parse_player_ref_or_the_player, self_obj, some1, split_once_any,
    strip_prefix_any, strip_suffix_any, vconst,
};
use crate::filter::{
    parse_condition, parse_keyword_list, parse_object_filter, parse_player_ref,
    parse_subject_or_target, parse_value_expr,
};
use crate::vocab::{Vocabulary, parse_number_word};

pub(super) fn try_parse_cast_permission(low: &str, vocab: &Vocabulary) -> Option<InstructionAst> {
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
    // Capture trailing mana-fixing, flash, face-down, and additional-cost modifiers losslessly
    let spend_mana_as_any_color = body.contains(", and mana of any type can be spent")
        || body.contains("spend mana as though it were mana of any color");
    let body = body
        .split_once(", and mana of any type can be spent")
        .or_else(|| body.split_once(", and you may spend mana as though it were mana of any color"))
        .or_else(|| body.split_once(" and you may spend mana as though it were mana of any color"))
        .map_or(body, |(before, _)| before.trim());
    let free = body.contains("without paying its mana cost")
        || body.contains("without paying their mana costs");
    let face_down_2_2 = body.contains(" face down as a 2/2 creature spell");
    let as_though_flash =
        body.contains(" as though they had flash") || body.contains(" as though it had flash");
    let clean = body
        .replace(" without paying its mana cost", "")
        .replace(" without paying their mana costs", "")
        .replace(" face down as a 2/2 creature spell", "")
        .replace(" this turn as though they had flash", " this turn")
        .replace(" as though they had flash", "")
        .replace(" as though it had flash", "");
    // Capture optional additional cost / condition tails losslessly:
    // - " by paying <cost> ..."
    // - "~ by <cost>" (e.g. Tegwyll's Scouring)
    // - "~ if <cond_or_cost>" (e.g. Molten Exhale)
    // - " by <cost containing 'in addition to (paying )?its other costs'>"
    // - " if <cond containing 'as an additional cost'>"
    let (clean, additional_cost) = if let Some((before, pay_tail)) = clean.split_once(" by paying ")
    {
        (before.to_string(), Some(pay_tail.trim().to_string()))
    } else if let Some(by_tail) = clean.strip_prefix("~ by ") {
        ("~".to_string(), Some(by_tail.trim().to_string()))
    } else if let Some(if_tail) = clean.strip_prefix("~ if ") {
        ("~".to_string(), Some(format!("if {}", if_tail.trim())))
    } else if let Some((before, by_tail)) = clean.split_once(" by ") {
        if by_tail.contains("in addition to paying its other costs")
            || by_tail.contains("in addition to its other costs")
        {
            (before.to_string(), Some(by_tail.trim().to_string()))
        } else {
            (clean, None)
        }
    } else if let Some((before, if_tail)) = clean.split_once(" if ") {
        if if_tail.contains("as an additional cost") {
            (before.to_string(), Some(format!("if {}", if_tail.trim())))
        } else {
            (clean, None)
        }
    } else {
        (clean, None)
    };
    let has_cast_modifiers =
        as_though_flash || spend_mana_as_any_color || additional_cost.is_some();
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
    let (obj_trim, count_limit) =
        if let Some(rest) = obj_nozone.trim().strip_prefix("any number of ") {
            (rest, Some(crate::ast::CountLimitAst::AnyNumber))
        } else if let Some(rest) = obj_nozone.trim().strip_prefix("one of ") {
            (rest, None)
        } else {
            (obj_nozone.trim(), None)
        };
    let build_cast_perm = |subject: SubjectRefAst| {
        if face_down_2_2 && !has_cast_modifiers {
            InstructionAst::CastFaceDown {
                player,
                subject: Box::new(subject),
                power: 2,
                toughness: 2,
                without_paying_mana_cost: free,
            }
        } else if has_cast_modifiers || face_down_2_2 {
            InstructionAst::GrantModifiedCastPermission(Box::new(ModifiedCastPermissionAst {
                player,
                subject: Box::new(subject),
                cast_only,
                free,
                duration: dur.to_string(),
                as_though_flash,
                face_down_2_2,
                spend_mana_as_any_color,
                additional_cost: additional_cost.clone(),
            }))
        } else {
            InstructionAst::GrantCastPermission {
                player,
                cast_only,
                subject: Box::new(subject),
                free,
                duration: dur.to_string(),
                count_limit: count_limit.clone(),
                alt_cost: None,
            }
        }
    };
    if obj_trim == "~" || obj_trim == "~ from your graveyard" || obj_trim == "~ from exile" {
        return Some(build_cast_perm(SubjectRefAst::SelfObject));
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
        return Some(build_cast_perm(SubjectRefAst::Anaphora(
            obj_trim.to_string(),
        )));
    }
    let (flt_candidate, from_among_kind, exiled_with_self) =
        if let Some(p) = obj_trim.strip_suffix(" from among cards exiled with ~") {
            (p, Some("exiled_with_self"), true)
        } else if let Some(p) = obj_trim
            .strip_suffix(" from among those exiled cards")
            .or_else(|| obj_trim.strip_suffix(" from among the exiled cards"))
            .or_else(|| obj_trim.strip_suffix(" from among cards exiled this way"))
        {
            (p, Some("exiled_this_way"), false)
        } else if let Some(p) =
            strip_suffix_any(&obj_trim, &[" from among them", " from among those cards"])
        {
            (p, Some("from_among_those"), false)
        } else if let Some(p) = obj_trim.strip_suffix(" from among the copies") {
            (p, Some("from_among_copies"), false)
        } else {
            (obj_trim, None, false)
        };
    if let Ok(mut subj) = parse_subject_or_target(flt_candidate, vocab) {
        if let Some(kind) = from_among_kind {
            let attach = |flt: &mut ObjectFilterAst| {
                if exiled_with_self {
                    flt.flags = flt.flags.union(ObjectFilterFlags::EXILED_WITH_SELF);
                } else {
                    flt.predicates.push(FilterPredicateAst::PriorActionZoneRef {
                        kind: kind.to_string(),
                    });
                }
            };
            match &mut subj {
                SubjectRefAst::Group(flt) | SubjectRefAst::All(flt) => attach(flt),
                SubjectRefAst::TargetObject { filter, .. } => attach(filter),
                _ => {}
            }
        }
        return Some(build_cast_perm(subj));
    }
    None
}

pub(super) fn parse_mana_riders_and_alt_costs(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Compositional next-spell alternative cast cost:
    // "rather than pay the mana cost of the next <spell_filter> you cast this turn, you may <cost>"
    if let Some(after) = low.strip_prefix("rather than pay the mana cost of the next ")
        && let Some((spell_raw, cost_raw)) = after.split_once(" you cast this turn, you may ")
        && let Ok(cost_atom) = crate::clauses::parse_cost_atom(cost_raw.trim(), vocab)
    {
        let mut flt = parse_object_filter(spell_raw.trim(), vocab).unwrap_or(any_spell_flt());
        flt.controller = Some(ControllerRelation::You);
        return some1(ins_temp_static(
            StaticAbilityAst::AlternativeCastCost {
                cost: vec![cost_atom],
                condition: None,
                for_spells: Some(Box::new(flt)),
                next_only: true,
            },
            DurationAst::UntilEndOfTurn,
        ));
    }

    // Compositional temporary mana-as-any-type/color rule for one spell this turn
    if let Some(after) = low
        .strip_prefix("for one spell this turn, you may spend mana as though it were mana of any ")
        && let Some((kind_word, _)) = after.split_once(" to pay ")
        && matches!(kind_word.trim(), "type" | "color")
    {
        return some1(InstructionAst::TemporaryManaSpendRule {
            mana_kind: None,
            as_though: format!("mana_of_any_{}", kind_word.trim()),
            purpose: Some("pay_spell_mana_cost".to_string()),
            duration: DurationAst::UntilEndOfTurn,
            next_only: true,
        });
    }

    // Compositional bonus die-roll: "roll a <sides>-sided die plus an additional <sides>-sided die for each <expr>"
    if let Some(after) = low.strip_prefix("roll a ")
        && let Some((s1, rest)) = after.split_once("-sided die plus an additional ")
        && let Some((_s2, fe_raw)) = rest.split_once("-sided die for each ")
        && let Some(sides_n) = parse_number_word(s1.trim())
        && let Ok(sides) = u16::try_from(sides_n)
        && let Ok(count) = parse_value_expr(fe_raw.trim(), vocab)
    {
        return Some(vec![
            InstructionAst::RollDie { sides },
            ins_for_each_count(count, vec![InstructionAst::RollDie { sides }]),
        ]);
    }

    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest)) = after_if.split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
        && let Some(action_raw) = strip_suffix_any(
            &rest,
            &[" instead of putting it into your hand", " instead"],
        )
    {
        if let Ok(inner) = parse_atomic_effect(action_raw.trim(), vocab) {
            return some1(ins_if(cond, vec![InstructionAst::ReplaceEffect(inner)]));
        }
        // Implicit-target damage replacement: "<src> deals damage equal to <val>"
        if let Some((src_raw, val_raw)) = action_raw.split_once(" deals damage equal to ")
            && let (Ok(source), Ok(amount)) = (
                parse_subject_or_target(src_raw.trim(), vocab),
                parse_value_expr(val_raw.trim(), vocab),
            )
        {
            return some1(ins_if(
                cond,
                vec![InstructionAst::ReplaceEffect(vec![ins_dmg(
                    source,
                    anaphora("previous_target"),
                    amount,
                )])],
            ));
        }
    }

    None
}

pub(super) fn parse_forced_mana_abilities(
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // "[you may have ]<player> activate(s) a mana ability of each <filter> they control[ and lose(s) all unspent mana]"
    // Covers Pygmy Hippo, Drain Power, and future forced-mana-activation effects.
    let (optional, core) = match low.strip_prefix("you may have ") {
        Some(r) => (true, r),
        None => (false, low),
    };
    let (core_act, lose_unspent) = match strip_suffix_any(
        &core,
        &[" and lose all unspent mana", " and loses all unspent mana"],
    ) {
        Some(r) => (r, true),
        None => (core, false),
    };
    let (who_raw, flt_raw) = core_act
        .split_once(" activates a mana ability of each ")
        .or_else(|| core_act.split_once(" activate a mana ability of each "))?;
    let player = parse_player_ref(who_raw.trim())?;
    let flt_clean = flt_raw
        .trim()
        .strip_suffix(" they control")
        .unwrap_or(flt_raw.trim());
    let mut flt = parse_object_filter(flt_clean, vocab).ok()?;
    flt.controller = Some(match player {
        PlayerRefAst::DefendingPlayer => ControllerRelation::DefendingPlayer,
        PlayerRefAst::TargetPlayer => ControllerRelation::TargetPlayer,
        PlayerRefAst::TargetOpponent => ControllerRelation::TargetOpponent,
        PlayerRefAst::You => ControllerRelation::You,
        _ => ControllerRelation::ThatPlayer,
    });
    let mut seq = vec![InstructionAst::ActivateManaAbilities {
        player,
        filter: Box::new(flt),
    }];
    if lose_unspent {
        seq.push(InstructionAst::LoseUnspentMana { player });
    }
    Some(if optional {
        vec![InstructionAst::Optional(seq)]
    } else {
        seq
    })
}

#[inline(never)]
pub(super) fn parse_extra_turns_steps_and_mana(
    _s: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Extra turn(s): "[after that turn, ][<player>] take(s) [an|<N>] extra turn(s)[ after this one]"
    // (Emrakul, Time Stretch, Teferi Master of Time)
    let turn_clean = low
        .strip_prefix("after that turn, ")
        .unwrap_or(low)
        .strip_suffix(" after this one")
        .unwrap_or_else(|| low.strip_prefix("after that turn, ").unwrap_or(low));
    if let Some(cnt_raw) = turn_clean
        .strip_prefix("take ")
        .and_then(|r| strip_suffix_any(r, &[" extra turns", " extra turn"]))
    {
        let n = if cnt_raw == "an" {
            1
        } else {
            parse_number_word(cnt_raw.trim())?
        };
        if (1..=10).contains(&n) {
            return Some(
                (0..n)
                    .map(|_| InstructionAst::ExtraTurn {
                        player: PlayerRefAst::You,
                    })
                    .collect(),
            );
        }
    }
    if let Some((who_raw, rest)) = split_once_any(&turn_clean, &[" takes ", " take "])
        && let Some(cnt_raw) = strip_suffix_any(&rest, &[" extra turns", " extra turn"])
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let n = if cnt_raw == "an" {
            1
        } else {
            parse_number_word(cnt_raw.trim())?
        };
        if (1..=10).contains(&n) {
            return Some(
                (0..n)
                    .map(|_| InstructionAst::ExtraTurn { player })
                    .collect(),
            );
        }
    }

    // 2. Additional step ("you get an additional <step> step after this step" / "there is an additional <step> step after this step")
    // (The Ninth Doctor, Y'shtola Rhul)
    if let Some(step_raw) =
        strip_prefix_any(&low, &["you get an additional ", "there is an additional "])
            .and_then(|r| r.strip_suffix(" step after this step"))
    {
        let step = match step_raw.trim() {
            "upkeep" => TurnStepAst::Upkeep,
            "draw" => TurnStepAst::DrawStep,
            "untap" => TurnStepAst::UntapStep,
            "end" => TurnStepAst::EndStep,
            _ => return None,
        };
        return some1(InstructionAst::AdditionalStep {
            player: PlayerRefAst::You,
            step,
            count: 1,
        });
    }

    // 3. Skip chosen step or phase this turn (Fatespinner)
    if let Some((who_raw, _)) =
        low.split_once(" skips each instance of the chosen step or phase this turn")
    {
        let player = parse_player_ref(who_raw.trim()).unwrap_or(PlayerRefAst::ThatPlayer);
        return some1(InstructionAst::SkipAllPhasesNextTurn {
            player,
            phase: "chosen_step_or_phase_this_turn".to_string(),
        });
    }

    // 4. Control opponent during their next combat/turn phase (Secret of Bloodbending)
    if let Some(after) = low.strip_prefix("you control ")
        && let Some((who_raw, phase_raw)) = after.split_once(" during their next ")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let phase_dur = if phase_raw.contains("combat") {
            DurationAst::UntilEndOfCombat
        } else {
            DurationAst::UntilEndOfNextTurn
        };
        return some1(InstructionAst::ControlPlayerTurn {
            player,
            duration: phase_dur,
        });
    }

    // 5. Double each type of unspent mana (Doubling Cube)
    if low == "double the amount of each type of unspent mana you have" {
        return some1(InstructionAst::DoubleUnspentMana {
            player: PlayerRefAst::You,
        });
    }

    // 6. Drain unspent mana ("<player> loses all unspent mana and you add the mana lost this way") (Drain Power)
    if let Some((who_raw, _)) =
        low.split_once(" loses all unspent mana and you add the mana lost this way")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![
            InstructionAst::LoseUnspentMana { player },
            InstructionAst::AddQualifiedMana(Box::new(crate::ast::QualifiedManaAst {
                player: PlayerRefAst::You,
                kind: "mana_lost_this_way".to_string(),
                amount: Some(Box::new(ValueExprAst::PriorResult)),
                source: Some(Box::new(SubjectRefAst::Player(player))),
                choices: Vec::new(),
            })),
        ]);
    }

    // 7. Player adds mana of any [one] color they choose (Spectral Searchlight, Stadium Vendors)
    if let Some((who_raw, rest)) = low.split_once(" adds ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((amt_raw, tail)) = rest.split_once(" mana of any ")
        && (tail == "color they choose" || tail == "one color they choose")
        && let Ok(amount) = parse_value_expr(amt_raw.trim(), vocab)
    {
        return some1(ins_add_mana(
            player,
            ManaSpecAst::AnyColor {
                amount: Box::new(amount),
                single_color: tail.starts_with("one "),
            },
        ));
    }

    // 8. "add <N> mana of that type [instead]" (Incubation Druid)
    let no_instead = low.strip_suffix(" instead").unwrap_or(low);
    if let Some(after) = no_instead.strip_prefix("add ")
        && let Some(amt_raw) = after.strip_suffix(" mana of that type")
        && let Ok(amount) = parse_value_expr(amt_raw.trim(), vocab)
    {
        let ins = InstructionAst::AddQualifiedMana(Box::new(crate::ast::QualifiedManaAst {
            player: PlayerRefAst::You,
            kind: "mana_of_that_type".to_string(),
            amount: Some(Box::new(amount)),
            source: None,
            choices: Vec::new(),
        }));
        return Some(if no_instead.len() != low.len() {
            vec![InstructionAst::ReplaceEffect(vec![ins])]
        } else {
            vec![ins]
        });
    }

    // 9. Mana from noted type/amount, circled colors, or permanent's mana cost
    // (Elemental Resonance, Cryptic Spires, Jeweled Amulet, Ice Cauldron)
    if let Some(subj_raw) = low
        .strip_prefix("add mana equal to ")
        .and_then(|r| r.strip_suffix("'s mana cost"))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(InstructionAst::AddQualifiedMana(Box::new(
            crate::ast::QualifiedManaAst {
                player: PlayerRefAst::You,
                kind: "equal_to_mana_cost".to_string(),
                amount: None,
                source: Some(Box::new(subj)),
                choices: Vec::new(),
            },
        )));
    }
    if low == "add one mana of either of the circled colors" {
        return some1(InstructionAst::AddQualifiedMana(Box::new(
            crate::ast::QualifiedManaAst {
                player: PlayerRefAst::You,
                kind: "circled_colors".to_string(),
                amount: Some(vconst(1)),
                source: Some(self_obj()),
                choices: Vec::new(),
            },
        )));
    }
    if low == "add one mana of ~'s last noted type" {
        return some1(InstructionAst::AddQualifiedMana(Box::new(
            crate::ast::QualifiedManaAst {
                player: PlayerRefAst::You,
                kind: "last_noted_type".to_string(),
                amount: Some(vconst(1)),
                source: Some(self_obj()),
                choices: Vec::new(),
            },
        )));
    }
    if low == "add ~'s last noted type and amount of mana" {
        return some1(InstructionAst::AddQualifiedMana(Box::new(
            crate::ast::QualifiedManaAst {
                player: PlayerRefAst::You,
                kind: "last_noted_type_and_amount".to_string(),
                amount: None,
                source: Some(self_obj()),
                choices: Vec::new(),
            },
        )));
    }

    // 10. Temporary mana color replacement ("spells and abilities you control that would add colored mana instead add that much <color> mana") (False Dawn)
    if let Some(after) = low.strip_prefix(
        "spells and abilities you control that would add colored mana instead add that much ",
    ) && let Some(color_raw) = after.strip_suffix(" mana")
    {
        let sym = match color_raw.trim() {
            "white" => "{W}",
            "blue" => "{U}",
            "black" => "{B}",
            "red" => "{R}",
            "green" => "{G}",
            "colorless" => "{C}",
            _ => return None,
        };
        return some1(InstructionAst::ReplaceManaProduction {
            subject: box_anaphora("spells and abilities you control"),
            replace_with: ManaSpecAst::ScaledSymbol {
                symbol: sym.to_string(),
                amount: Box::new(ValueExprAst::PriorResult),
            },
            duration: dur.clone(),
        });
    }

    // 11. Foretell / Flashback / Suspend derived costs (Ethereal Valkyrie, Sphinx of Forgotten Lore, The Face of Boe)
    if let Some(delta_raw) = low.strip_prefix("its foretell cost is its mana cost reduced by ") {
        return some1(ins_temp_static(
            StaticAbilityAst::FlashbackCostModifier {
                cost: "foretell_mana_cost".to_string(),
                delta: delta_raw.trim().to_ascii_uppercase(),
            },
            DurationAst::Permanent,
        ));
    }
    if low == "the flashback cost is equal to that card's mana cost" {
        return some1(ins_temp_static(
            StaticAbilityAst::FlashbackCostModifier {
                cost: "mana_cost".to_string(),
                delta: "{0}".to_string(),
            },
            dur.clone(),
        ));
    }
    if low == "pay its suspend cost rather than its mana cost" {
        return some1(ins_temp_static(
            StaticAbilityAst::AlternativeCastCost {
                cost: vec![CostAtomAst::PayMana("suspend_cost".to_string())],
                condition: None,
                for_spells: None,
                next_only: true,
            },
            DurationAst::Instant,
        ));
    }

    None
}

#[inline(never)]
pub(super) fn parse_cast_copy_and_zone_actions(
    s: &str,
    low: &str,
    _s_nodur: &str,
    _dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    if let Some(after) = low.strip_prefix("the next ")
        && let Some((flt_raw, _)) = after
            .split_once(" you play this turn can be played as though it had flash")
            .or_else(|| after.split_once(" you cast this turn can be cast as though it had flash"))
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return some1(ins_temp_static(
            StaticAbilityAst::NextSpellModifier {
                filter: Box::new(flt.clone()),
                effect: Box::new(StaticAbilityAst::GrantKeywords {
                    subject: Box::new(grp(flt)),
                    keywords: vec!["flash".to_string()],
                    condition: None,
                    paired_rule: None,
                }),
            },
            DurationAst::UntilEndOfTurn,
        ));
    }

    // 2. "the next <flt1> and the next <flt2> you cast this turn each have <kw>" (Smoldering Stagecoach)
    if let Some(after) = low.strip_prefix("the next ")
        && let Some((pair_raw, kw_raw)) = after.split_once(" you cast this turn each have ")
        && let Some((f1_raw, f2_raw)) = pair_raw.split_once(" and the next ")
        && let (Ok(f1), Ok(f2)) = (
            parse_object_filter(f1_raw.trim(), vocab),
            parse_object_filter(f2_raw.trim(), vocab),
        )
        && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
    {
        return Some(vec![
            ins_temp_static(
                StaticAbilityAst::NextSpellModifier {
                    filter: Box::new(f1.clone()),
                    effect: Box::new(StaticAbilityAst::GrantKeywords {
                        subject: Box::new(grp(f1)),
                        keywords: kws.clone(),
                        condition: None,
                        paired_rule: None,
                    }),
                },
                DurationAst::UntilEndOfTurn,
            ),
            ins_temp_static(
                StaticAbilityAst::NextSpellModifier {
                    filter: Box::new(f2.clone()),
                    effect: Box::new(StaticAbilityAst::GrantKeywords {
                        subject: Box::new(grp(f2)),
                        keywords: kws,
                        condition: None,
                        paired_rule: None,
                    }),
                },
                DurationAst::UntilEndOfTurn,
            ),
        ]);
    }

    // 3. Free / alt-cost / outside-the-game / exiled / milled cast permissions
    // (Spawnsire of Ulamog, Spelltwine, Nahiri, Plargg and Nassari, The Infamous Cruelclaw, The Ur-Sphinx)
    if let Some(after) = low.strip_prefix("cast any number of ")
        && let Some((flt_raw, _)) = after.split_once(
            " from among cards you own outside the game without paying their mana costs",
        )
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.zone = Some(ZoneFilter::OutsideTheGame);
        flt.owner = Some(ControllerRelation::You);
        return some1(InstructionAst::GrantCastPermission {
            player: PlayerRefAst::You,
            cast_only: true,
            subject: Box::new(grp(flt)),
            free: true,
            duration: "now".to_string(),
            count_limit: Some(CountLimitAst::AnyNumber),
            alt_cost: None,
        });
    }
    if low == "cast the copies if able without paying their mana costs" {
        return some1(ins_grant_cast(
            PlayerRefAst::You,
            true,
            anaphora("the copies"),
            true,
            "now",
        ));
    }
    if let Some(after) = low.strip_prefix("you may cast ") {
        if let Some(flt_raw) = after.strip_suffix(" this way without paying their mana costs")
            && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            return some1(ins_opt1(ins_grant_cast(
                PlayerRefAst::You,
                true,
                grp(flt),
                true,
                "this_way",
            )));
        }
        if let Some(after_up) = after.strip_prefix("up to ")
            && let Some((cnt_raw, tail)) = after_up.split_once(" spells from among ")
            && let Some(pool_raw) = tail.strip_suffix(" without paying their mana costs")
            && let Ok(cnt) = parse_value_expr(cnt_raw.trim(), vocab)
        {
            let subj = parse_subject_or_target(pool_raw.trim(), vocab)
                .unwrap_or_else(|_| SubjectRefAst::Anaphora(pool_raw.trim().to_string()));
            return some1(ins_opt1(InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only: true,
                subject: Box::new(subj),
                free: true,
                duration: "now".to_string(),
                count_limit: Some(CountLimitAst::UpTo(Box::new(cnt))),
                alt_cost: None,
            }));
        }
        if let Some((subj_raw, cost_tail)) = after.split_once(" by ")
            && let Some(cost_raw) = cost_tail.strip_suffix(" rather than paying its mana cost")
            && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
        {
            let norm_cost = cost_raw
                .trim()
                .strip_prefix("discarding ")
                .map_or_else(|| cost_raw.trim().to_string(), |r| format!("discard {r}"));
            if let Ok(atom) = parse_cost_atom(&norm_cost, vocab) {
                return some1(ins_opt1(InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::You,
                    cast_only: true,
                    subject: Box::new(subj),
                    free: false,
                    duration: "now".to_string(),
                    count_limit: None,
                    alt_cost: Some(AltCastCostAst::PayCost(vec![atom])),
                }));
            }
        }
        if let Some(subj_raw) = after.strip_suffix(" without paying its mana cost")
            && subj_raw.contains(" milled this way")
        {
            return some1(ins_opt1(ins_grant_cast(
                PlayerRefAst::You,
                true,
                SubjectRefAst::Anaphora(subj_raw.trim().to_string()),
                true,
                "now",
            )));
        }
    }

    // 4. Unlock a locked door of a Room (Ghostly Keybearer, Ghostly Dancers)
    if let Some(subj_raw) = low.strip_prefix("unlock a locked door of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(InstructionAst::UnlockDoor {
            subject: Box::new(subj),
        });
    }

    // 5. Meld paired cards (Vanille, Cheerful l'Cie and future meld cards)
    if let Some(after) = low.strip_prefix("meld ")
        && let Some((subj_raw, into_low)) = after.split_once(" into ")
    {
        let into_start = s.len().saturating_sub(into_low.len());
        let into_name = s.get(into_start..).unwrap_or(into_low).trim().to_string();
        let subj = parse_subject_or_target(subj_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
        return some1(InstructionAst::Meld {
            subject: Box::new(subj),
            into_name,
        });
    }

    // 6. Manifest / Cloak top card of library (Reality Shift, Thieving Amalgam, Unexplained Absence)
    if let Some((who_raw, rest)) = split_once_any(
        &low,
        &[" manifests the top card of ", " manifest the top card of "],
    ) && let Some(whose_raw) = rest.strip_suffix(" library")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let subject = Some(Box::new(SubjectRefAst::Anaphora(format!(
            "top card of {} library",
            whose_raw.trim()
        ))));
        if player == PlayerRefAst::You {
            return some1(InstructionAst::Manifest {
                count: vconst(1),
                dread: false,
                subject,
            });
        }
        return some1(InstructionAst::ManifestByPlayer {
            player,
            count: vconst(1),
            dread: false,
            subject,
        });
    }
    if let Some((who_raw, rest)) = split_once_any(
        &low,
        &[" cloaks the top card of ", " cloak the top card of "],
    ) && let Some(whose_raw) = rest.strip_suffix(" library")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let subject = Some(Box::new(SubjectRefAst::Anaphora(format!(
            "top card of {} library",
            whose_raw.trim()
        ))));
        if player == PlayerRefAst::You {
            return some1(InstructionAst::Cloak {
                count: vconst(1),
                subject,
            });
        }
        return some1(InstructionAst::CloakByPlayer {
            player,
            count: vconst(1),
            subject,
        });
    }

    // 7. "<player> exiles <subj> with <val> <ck> counters on it" (Ertai's Meddling)
    if let Some((who_raw, rest)) = low.split_once(" exiles ")
        && let Some((subj_raw, cnt_tail)) = rest.split_once(" with ")
        && let Some(cnt_and_kind) =
            strip_suffix_any(&cnt_tail, &[" counters on it", " counter on it"])
        && let Some((cnt_raw, ck_raw)) = cnt_and_kind.split_once(' ')
        && let (Ok(subj), Ok(count)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_value_expr(cnt_raw.trim(), vocab),
        )
    {
        let _ = who_raw;
        return some1(InstructionAst::ExileWithCounters {
            subject: Box::new(subj),
            counter_kind: ck_raw.trim().to_string(),
            count: Box::new(count),
            face_down: false,
        });
    }

    // 9. Library/graveyard reordering, owner shuffles, reveal-into-hand, choose-and-discard-rest
    // (Dimir Machinations, Search for Survivors, The Three Seasons, Breakthrough, Parker Luck, Keen Duelist, Invasion of Ravnica)
    if low == "put the rest back in any order" {
        return Some(vec![InstructionAst::ReorderTopLibrary]);
    }
    if low == "reorder your graveyard at random" {
        return some1(InstructionAst::ReorderZone {
            player: PlayerRefAst::You,
            zone: ZoneFilter::YourGraveyard,
            random: true,
        });
    }
    if low == "their owners shuffle those cards into their libraries" {
        return some1(InstructionAst::ShuffleIntoLibrary {
            subject: box_anaphora("those cards"),
        });
    }
    if let Some(after) = low.strip_prefix("choose ")
        && let Some((cnt_raw, _)) = after.split_once(" cards in your hand and discard the rest")
        && let Ok(cnt) = parse_value_expr(cnt_raw.trim(), vocab)
    {
        return Some(vec![
            ins_choose_group(
                cnt,
                false,
                grp(ObjectFilterAst {
                    any_card: true,
                    zone: Some(ZoneFilter::YourHand),
                    ..Default::default()
                }),
            ),
            InstructionAst::Discard {
                player: PlayerRefAst::You,
                count: None,
                filter: None,
                anaphora: Some("the rest".to_string()),
                random: false,
            },
        ]);
    }
    if low == "you each put the card you revealed into your hand"
        || low == "they each put the card they revealed into their hand"
    {
        return some1(ins_move(anaphora("the card revealed"), "hand"));
    }
    if let Some(after) = low.strip_prefix("reveal a card that's exactly ")
        && let Some((num_raw, _)) =
            after.split_once(" colors from among them and put it into your hand")
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        let mut flt = any_card_flt();
        if n >= 2 {
            flt.color_arity = Some(ColorArity::Multicolored);
        }
        flt.predicates
            .push(FilterPredicateAst::StateQualification(format!(
                "exact_colors:{n}"
            )));
        return Some(vec![
            InstructionAst::RevealCards {
                subject: Box::new(grp(flt)),
            },
            ins_move(anaphora("that card"), "your hand"),
        ]);
    }

    // 10. Reanimate with counters equal to result / different mana values X or less (Grave Endeavor, Agadeem's Awakening)
    if let Some(after) = low.strip_prefix("return ")
        && let Some((flt_raw, rest)) =
            after.split_once(" from your graveyard to the battlefield with a number of ")
        && let Some((ck_raw, _)) = rest.split_once(" counters on it equal to that result")
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.zone = Some(ZoneFilter::YourGraveyard);
        return Some(vec![
            InstructionAst::ReturnZone {
                subject: Box::new(grp(flt)),
                from_zone: Some("your graveyard".to_string()),
                to: "battlefield".to_string(),
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: None,
                random: false,
                except: None,
                count: Some(vconst(1)),
                up_to: false,
                any_number: false,
                actor: None,
            },
            ins_counters(
                anaphora("that creature"),
                ck_raw.trim().to_string(),
                ValueExprAst::PriorResult,
            ),
        ]);
    }
    if let Some(after) =
        low.strip_prefix("return from your graveyard to the battlefield any number of target ")
        && let Some((flt_raw, mv_raw)) = after.split_once(" that each have a different mana value ")
        && let Some(bound_raw) = mv_raw.strip_suffix(" or less")
        && let (Ok(mut flt), Ok(val)) = (
            parse_object_filter(flt_raw.trim(), vocab),
            parse_value_expr(bound_raw.trim(), vocab),
        )
    {
        flt.zone = Some(ZoneFilter::YourGraveyard);
        flt.flags = flt.flags.union(ObjectFilterFlags::DIFFERENT_MANA_VALUES);
        flt.stat_bounds.push(StatBoundAst {
            stat: StatKind::ManaValue,
            op: CompareOp::Le,
            value: Box::new(val),
        });
        return some1(InstructionAst::ReturnZone {
            subject: Box::new(SubjectRefAst::TargetObject {
                filter: Box::new(flt),
                count: TargetCountAst::AnyNumber,
            }),
            from_zone: Some("your graveyard".to_string()),
            to: "battlefield".to_string(),
            tapped: false,
            transformed: false,
            with_counter: None,
            attached_to: None,
            random: false,
            except: None,
            count: None,
            up_to: false,
            any_number: true,
            actor: None,
        });
    }

    // 12. Delayed loyalty-ability copy, Tempt-with-Mayhem scaled spell copy, target/attacker reselection
    // (Jaya's Phoenix, Tempt with Mayhem, Chef's Kiss, Capricopian)
    if low == "copy the next loyalty ability you activate this turn when you activate it" {
        return some1(InstructionAst::WhenYouNext {
            event: "activate_loyalty_ability".to_string(),
            body: vec![InstructionAst::Copy {
                subject: box_anaphora("that ability"),
                exception: None,
                count: None,
            }],
            duration: DurationAst::UntilEndOfTurn,
        });
    }
    if low
        == "you copy that spell once plus an additional time for each opponent who copied the spell this way"
    {
        return some1(InstructionAst::Copy {
            subject: box_anaphora("that spell"),
            exception: None,
            count: Some(Box::new(ValueExprAst::Add(
                vconst(1),
                Box::new(ValueExprAst::CountPlayersMatching {
                    predicate: "opponent_copied_spell_this_way".to_string(),
                }),
            ))),
        });
    }
    if low == "reselect the targets at random for the spell and the copy" {
        return some1(InstructionAst::ReselectTargets {
            subject: box_anaphora("the spell and the copy"),
            random: true,
        });
    }
    if let Some(after) = low.strip_prefix("you may reselect which player ")
        && let Some(subj_raw) = after.strip_suffix(" is attacking")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return some1(ins_opt1(InstructionAst::ReselectTargets {
            subject: Box::new(subj),
            random: false,
        }));
    }

    None
}

/// Family 7: Mid-resolution opponent control, forced card play, and mana-ability restrictions
/// (`Word of Command`).
#[inline(never)]
pub(super) fn parse_resolution_control_and_forced_play(
    _s: &str,
    low: &str,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Word of Command S2: "you control <player> until ~ finishes resolving"
    if let Some(who_raw) = low
        .strip_prefix("you control ")
        .and_then(|r| r.strip_suffix(" until ~ finishes resolving"))
        && let Some(controlled) = parse_player_ref_or_the_player(who_raw.trim())
    {
        return some1(InstructionAst::ControlPlayerDuringResolution {
            controller: PlayerRefAst::You,
            controlled,
            during_chosen_spell: false,
        });
    }

    // 2. Word of Command S5 then-body: "you control <player> while that spell is resolving"
    if let Some(who_raw) = low
        .strip_prefix("you control ")
        .and_then(|r| r.strip_suffix(" while that spell is resolving"))
        && let Some(controlled) = parse_player_ref_or_the_player(who_raw.trim())
    {
        return some1(InstructionAst::ControlPlayerDuringResolution {
            controller: PlayerRefAst::You,
            controlled,
            during_chosen_spell: true,
        });
    }

    // 3. Word of Command S3: "<player> plays/casts <subj> if able"
    if let Some(before_able) = low.strip_suffix(" if able")
        && let Some((who_raw, subj_raw)) =
            split_once_any(&before_able, &[" plays ", " play ", " casts ", " cast "])
        && let (Some(player), Ok(subject)) = (
            parse_player_ref_or_the_player(who_raw.trim()),
            parse_subject_or_target(subj_raw.trim(), vocab),
        )
    {
        return some1(InstructionAst::ForcePlayCard {
            player,
            subject: Box::new(subject),
            if_able: true,
        });
    }

    // 4. Word of Command S4: "while doing so, <player> can activate mana abilities only if they're from <flt> and only if mana they produce is spent to activate other mana abilities of <flt> and/or to play that card"
    if let Some(after_while) = low.strip_prefix("while doing so, ")
        && let Some((who_raw, rest)) =
            after_while.split_once(" can activate mana abilities only if they're from ")
        && let Some((flt_raw, spend_tail)) =
            rest.split_once(" and only if mana they produce is spent ")
        && let (Some(player), Ok(allowed_source_filter)) = (
            parse_player_ref_or_the_player(who_raw.trim()),
            parse_object_filter(flt_raw.trim(), vocab),
        )
    {
        let spend_only_for_action_or_chained_mana = spend_tail.contains("other mana abilities")
            && (spend_tail.contains("to play that card") || spend_tail.contains("to cast that"));
        if spend_only_for_action_or_chained_mana {
            return some1(InstructionAst::RestrictManaAbilitiesDuringAction {
                player,
                allowed_source_filter: Box::new(allowed_source_filter),
                spend_only_for_action_or_chained_mana: true,
            });
        }
    }

    None
}
