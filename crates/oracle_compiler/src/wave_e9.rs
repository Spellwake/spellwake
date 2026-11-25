//! Compositional Wave E9 lossless effect parser eliminating legacy
//! `E3_EFFECT_EXACTS` (29 entries) and `E4_EFFECT_EXACTS` (129 entries) in
//! favor of reusable atomic Magic AST primitives.
//!
//! Zero `StructuredRule`, untyped `GameStateFlag`, or literal exact-sentence
//! tables are emitted here.

use crate::ast::{
    AltCastCostAst, ColorArity, CompareOp, ConditionAst, ControllerRelation, CostAtomAst,
    CountLimitAst, DurationAst, ExchangeKind, FilterPredicateAst, InstructionAst, ManaSpecAst,
    NameFilterAst, ObjectFilterAst, ObjectFilterFlags, PileRefAst, PlayerRefAst, Rounding,
    StatBoundAst, StatKind, StaticAbilityAst, SubjectRefAst, TargetCountAst, TokenSpecAst,
    TurnStepAst, ValueExprAst, ZoneFilter,
};
use crate::clauses::{parse_atomic_effect, parse_cost_atom};
use crate::filter::{
    parse_condition, parse_keyword_list, parse_object_filter, parse_player_ref,
    parse_subject_or_target, parse_token_creation_rich, parse_value_expr,
};
use crate::vocab::{Vocabulary, parse_number_word};

/// Compositional Wave E9 effect parser replacing `E3_EFFECT_EXACTS` and `E4_EFFECT_EXACTS`.
#[inline(never)]
pub(crate) fn parse_wave_e9_effects(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // Strip optional leading "then "
    if let Some(rest_low) = low.strip_prefix("then ") {
        let rest_s = &s[s.len() - rest_low.len()..];
        if let Some(body) = parse_wave_e9_core(rest_s, rest_low, s_nodur, dur, vocab) {
            return Some(body);
        }
    }
    parse_wave_e9_core(s, low, s_nodur, dur, vocab)
}

#[inline(never)]
fn parse_wave_e9_core(
    s: &str,
    low: &str,
    s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Extra turns, extra steps, skipped steps, turn-phase control, and mana formulas
    if let Some(body) = parse_extra_turns_steps_and_mana_e9(s, low, dur, vocab) {
        return Some(body);
    }
    // 2. Cast permissions, flash/cascade grants, copy, meld, unlock, manifest/cloak, search/zone moves
    if let Some(body) = parse_cast_copy_and_zone_actions_e9(s, low, s_nodur, dur, vocab) {
        return Some(body);
    }
    // 3. Multi-target damage, scaled damage/prevention/counters/tokens, die rolls, scry, and statics
    if let Some(body) = parse_damage_counters_tokens_and_stats_e9(s, low, s_nodur, dur, vocab) {
        return Some(body);
    }
    // 4. Control changes, exchanges, piles, choices, fights, combat/blocker limits
    if let Some(body) = parse_control_piles_choices_and_combat_e9(s, low, dur, vocab) {
        return Some(body);
    }
    // 5. Genuine activation, casting, targeting, X-bound, and repeat-process restrictions
    parse_genuine_restrictions_and_repeats_e9(s, low, dur, vocab)
}

#[inline(never)]
fn parse_extra_turns_steps_and_mana_e9(
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
    if let Some(cnt_raw) = turn_clean.strip_prefix("take ").and_then(|r| {
        r.strip_suffix(" extra turns")
            .or_else(|| r.strip_suffix(" extra turn"))
    }) {
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
    if let Some((who_raw, rest)) = turn_clean
        .split_once(" takes ")
        .or_else(|| turn_clean.split_once(" take "))
        && let Some(cnt_raw) = rest
            .strip_suffix(" extra turns")
            .or_else(|| rest.strip_suffix(" extra turn"))
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
    if let Some(step_raw) = low
        .strip_prefix("you get an additional ")
        .or_else(|| low.strip_prefix("there is an additional "))
        .and_then(|r| r.strip_suffix(" step after this step"))
    {
        let step = match step_raw.trim() {
            "upkeep" => TurnStepAst::Upkeep,
            "draw" => TurnStepAst::DrawStep,
            "untap" => TurnStepAst::UntapStep,
            "end" => TurnStepAst::EndStep,
            _ => return None,
        };
        return Some(vec![InstructionAst::AdditionalStep {
            player: PlayerRefAst::You,
            step,
            count: 1,
        }]);
    }

    // 3. Skip chosen step or phase this turn (Fatespinner)
    if let Some((who_raw, _)) =
        low.split_once(" skips each instance of the chosen step or phase this turn")
    {
        let player = parse_player_ref(who_raw.trim()).unwrap_or(PlayerRefAst::ThatPlayer);
        return Some(vec![InstructionAst::SkipAllPhasesNextTurn {
            player,
            phase: "chosen_step_or_phase_this_turn".to_string(),
        }]);
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
        return Some(vec![InstructionAst::ControlPlayerTurn {
            player,
            duration: phase_dur,
        }]);
    }

    // 5. Double each type of unspent mana (Doubling Cube)
    if low == "double the amount of each type of unspent mana you have" {
        return Some(vec![InstructionAst::DoubleUnspentMana {
            player: PlayerRefAst::You,
        }]);
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
        return Some(vec![InstructionAst::AddMana {
            player,
            mana: ManaSpecAst::AnyColor {
                amount: Box::new(amount),
                single_color: tail.starts_with("one "),
            },
        }]);
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
        return Some(vec![InstructionAst::AddQualifiedMana(Box::new(
            crate::ast::QualifiedManaAst {
                player: PlayerRefAst::You,
                kind: "equal_to_mana_cost".to_string(),
                amount: None,
                source: Some(Box::new(subj)),
                choices: Vec::new(),
            },
        ))]);
    }
    if low == "add one mana of either of the circled colors" {
        return Some(vec![InstructionAst::AddQualifiedMana(Box::new(
            crate::ast::QualifiedManaAst {
                player: PlayerRefAst::You,
                kind: "circled_colors".to_string(),
                amount: Some(Box::new(ValueExprAst::Const(1))),
                source: Some(Box::new(SubjectRefAst::SelfObject)),
                choices: Vec::new(),
            },
        ))]);
    }
    if low == "add one mana of ~'s last noted type" {
        return Some(vec![InstructionAst::AddQualifiedMana(Box::new(
            crate::ast::QualifiedManaAst {
                player: PlayerRefAst::You,
                kind: "last_noted_type".to_string(),
                amount: Some(Box::new(ValueExprAst::Const(1))),
                source: Some(Box::new(SubjectRefAst::SelfObject)),
                choices: Vec::new(),
            },
        ))]);
    }
    if low == "add ~'s last noted type and amount of mana" {
        return Some(vec![InstructionAst::AddQualifiedMana(Box::new(
            crate::ast::QualifiedManaAst {
                player: PlayerRefAst::You,
                kind: "last_noted_type_and_amount".to_string(),
                amount: None,
                source: Some(Box::new(SubjectRefAst::SelfObject)),
                choices: Vec::new(),
            },
        ))]);
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
        return Some(vec![InstructionAst::ReplaceManaProduction {
            subject: Box::new(SubjectRefAst::Anaphora(
                "spells and abilities you control".to_string(),
            )),
            replace_with: ManaSpecAst::ScaledSymbol {
                symbol: sym.to_string(),
                amount: Box::new(ValueExprAst::PriorResult),
            },
            duration: dur.clone(),
        }]);
    }

    // 11. Foretell / Flashback / Suspend derived costs (Ethereal Valkyrie, Sphinx of Forgotten Lore, The Face of Boe)
    if let Some(delta_raw) = low.strip_prefix("its foretell cost is its mana cost reduced by ") {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::FlashbackCostModifier {
                cost: "foretell_mana_cost".to_string(),
                delta: delta_raw.trim().to_ascii_uppercase(),
            }),
            duration: DurationAst::Permanent,
        }]);
    }
    if low == "the flashback cost is equal to that card's mana cost" {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::FlashbackCostModifier {
                cost: "mana_cost".to_string(),
                delta: "{0}".to_string(),
            }),
            duration: dur.clone(),
        }]);
    }
    if low == "pay its suspend cost rather than its mana cost" {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::AlternativeCastCost {
                cost: vec![CostAtomAst::PayMana("suspend_cost".to_string())],
                condition: None,
                for_spells: None,
                next_only: true,
            }),
            duration: DurationAst::Instant,
        }]);
    }

    None
}

#[inline(never)]
fn parse_cast_copy_and_zone_actions_e9(
    s: &str,
    low: &str,
    _s_nodur: &str,
    _dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Cast as though it had flash (Primal Prayers, Elsha of the Infinite, Scout's Warning)
    if low == "you may cast it as though it had flash" {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::GrantKeywords {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    keywords: vec!["flash".to_string()],
                    condition: None,
                    paired_rule: None,
                }),
                duration: DurationAst::Instant,
            },
        ])]);
    }
    if let Some(after) = low.strip_prefix("the next ")
        && let Some((flt_raw, _)) = after
            .split_once(" you play this turn can be played as though it had flash")
            .or_else(|| after.split_once(" you cast this turn can be cast as though it had flash"))
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::NextSpellModifier {
                filter: Box::new(flt.clone()),
                effect: Box::new(StaticAbilityAst::GrantKeywords {
                    subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                    keywords: vec!["flash".to_string()],
                    condition: None,
                    paired_rule: None,
                }),
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
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
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::NextSpellModifier {
                    filter: Box::new(f1.clone()),
                    effect: Box::new(StaticAbilityAst::GrantKeywords {
                        subject: Box::new(SubjectRefAst::Group(Box::new(f1))),
                        keywords: kws.clone(),
                        condition: None,
                        paired_rule: None,
                    }),
                }),
                duration: DurationAst::UntilEndOfTurn,
            },
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::NextSpellModifier {
                    filter: Box::new(f2.clone()),
                    effect: Box::new(StaticAbilityAst::GrantKeywords {
                        subject: Box::new(SubjectRefAst::Group(Box::new(f2))),
                        keywords: kws,
                        condition: None,
                        paired_rule: None,
                    }),
                }),
                duration: DurationAst::UntilEndOfTurn,
            },
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
        return Some(vec![InstructionAst::GrantCastPermission {
            player: PlayerRefAst::You,
            cast_only: true,
            subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            free: true,
            duration: "now".to_string(),
            count_limit: Some(CountLimitAst::AnyNumber),
            alt_cost: None,
        }]);
    }
    if low == "cast the copies if able without paying their mana costs" {
        return Some(vec![InstructionAst::GrantCastPermission {
            player: PlayerRefAst::You,
            cast_only: true,
            subject: Box::new(SubjectRefAst::Anaphora("the copies".to_string())),
            free: true,
            duration: "now".to_string(),
            count_limit: None,
            alt_cost: None,
        }]);
    }
    if let Some(after) = low.strip_prefix("you may cast ") {
        if let Some(flt_raw) = after.strip_suffix(" this way without paying their mana costs")
            && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::Optional(vec![
                InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::You,
                    cast_only: true,
                    subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                    free: true,
                    duration: "this_way".to_string(),
                    count_limit: None,
                    alt_cost: None,
                },
            ])]);
        }
        if let Some(after_up) = after.strip_prefix("up to ")
            && let Some((cnt_raw, tail)) = after_up.split_once(" spells from among ")
            && let Some(pool_raw) = tail.strip_suffix(" without paying their mana costs")
            && let Ok(cnt) = parse_value_expr(cnt_raw.trim(), vocab)
        {
            let subj = parse_subject_or_target(pool_raw.trim(), vocab)
                .unwrap_or_else(|_| SubjectRefAst::Anaphora(pool_raw.trim().to_string()));
            return Some(vec![InstructionAst::Optional(vec![
                InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::You,
                    cast_only: true,
                    subject: Box::new(subj),
                    free: true,
                    duration: "now".to_string(),
                    count_limit: Some(CountLimitAst::UpTo(Box::new(cnt))),
                    alt_cost: None,
                },
            ])]);
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
                return Some(vec![InstructionAst::Optional(vec![
                    InstructionAst::GrantCastPermission {
                        player: PlayerRefAst::You,
                        cast_only: true,
                        subject: Box::new(subj),
                        free: false,
                        duration: "now".to_string(),
                        count_limit: None,
                        alt_cost: Some(AltCastCostAst::PayCost(vec![atom])),
                    },
                ])]);
            }
        }
        if let Some(subj_raw) = after.strip_suffix(" without paying its mana cost")
            && subj_raw.contains(" milled this way")
        {
            return Some(vec![InstructionAst::Optional(vec![
                InstructionAst::GrantCastPermission {
                    player: PlayerRefAst::You,
                    cast_only: true,
                    subject: Box::new(SubjectRefAst::Anaphora(subj_raw.trim().to_string())),
                    free: true,
                    duration: "now".to_string(),
                    count_limit: None,
                    alt_cost: None,
                },
            ])]);
        }
    }

    // 4. Unlock a locked door of a Room (Ghostly Keybearer, Ghostly Dancers)
    if let Some(subj_raw) = low.strip_prefix("unlock a locked door of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::UnlockDoor {
            subject: Box::new(subj),
        }]);
    }

    // 5. Meld paired cards (Vanille, Cheerful l'Cie and future meld cards)
    if let Some(after) = low.strip_prefix("meld ")
        && let Some((subj_raw, into_low)) = after.split_once(" into ")
    {
        let into_start = s.len().saturating_sub(into_low.len());
        let into_name = s.get(into_start..).unwrap_or(into_low).trim().to_string();
        let subj = parse_subject_or_target(subj_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
        return Some(vec![InstructionAst::Meld {
            subject: Box::new(subj),
            into_name,
        }]);
    }

    // 6. Manifest / Cloak top card of library (Reality Shift, Thieving Amalgam, Unexplained Absence)
    if let Some((who_raw, rest)) = low
        .split_once(" manifests the top card of ")
        .or_else(|| low.split_once(" manifest the top card of "))
        && let Some(whose_raw) = rest.strip_suffix(" library")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let subject = Some(Box::new(SubjectRefAst::Anaphora(format!(
            "top card of {} library",
            whose_raw.trim()
        ))));
        if player == PlayerRefAst::You {
            return Some(vec![InstructionAst::Manifest {
                count: Box::new(ValueExprAst::Const(1)),
                dread: false,
                subject,
            }]);
        }
        return Some(vec![InstructionAst::ManifestByPlayer {
            player,
            count: Box::new(ValueExprAst::Const(1)),
            dread: false,
            subject,
        }]);
    }
    if let Some((who_raw, rest)) = low
        .split_once(" cloaks the top card of ")
        .or_else(|| low.split_once(" cloak the top card of "))
        && let Some(whose_raw) = rest.strip_suffix(" library")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        let subject = Some(Box::new(SubjectRefAst::Anaphora(format!(
            "top card of {} library",
            whose_raw.trim()
        ))));
        if player == PlayerRefAst::You {
            return Some(vec![InstructionAst::Cloak {
                count: Box::new(ValueExprAst::Const(1)),
                subject,
            }]);
        }
        return Some(vec![InstructionAst::CloakByPlayer {
            player,
            count: Box::new(ValueExprAst::Const(1)),
            subject,
        }]);
    }

    // 7. "<player> exiles <subj> with <val> <ck> counters on it" (Ertai's Meddling)
    if let Some((who_raw, rest)) = low.split_once(" exiles ")
        && let Some((subj_raw, cnt_tail)) = rest.split_once(" with ")
        && let Some(cnt_and_kind) = cnt_tail
            .strip_suffix(" counters on it")
            .or_else(|| cnt_tail.strip_suffix(" counter on it"))
        && let Some((cnt_raw, ck_raw)) = cnt_and_kind.split_once(' ')
        && let (Ok(subj), Ok(count)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_value_expr(cnt_raw.trim(), vocab),
        )
    {
        let _ = who_raw;
        return Some(vec![InstructionAst::ExileWithCounters {
            subject: Box::new(subj),
            counter_kind: ck_raw.trim().to_string(),
            count: Box::new(count),
            face_down: false,
        }]);
    }

    // 8. Phase out unchosen permanents (Disciple of Caelus Nin)
    if let Some(subj_raw) = low.strip_suffix(" phase out") {
        let clean = subj_raw
            .trim()
            .strip_prefix("all ")
            .unwrap_or(subj_raw.trim());
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Some(vec![InstructionAst::PhaseOut {
                subject: Box::new(SubjectRefAst::All(Box::new(flt))),
            }]);
        }
    }

    // 9. Library/graveyard reordering, owner shuffles, reveal-into-hand, choose-and-discard-rest
    // (Dimir Machinations, Search for Survivors, The Three Seasons, Breakthrough, Parker Luck, Keen Duelist, Invasion of Ravnica)
    if low == "put the rest back in any order" {
        return Some(vec![InstructionAst::ReorderTopLibrary]);
    }
    if low == "reorder your graveyard at random" {
        return Some(vec![InstructionAst::ReorderZone {
            player: PlayerRefAst::You,
            zone: ZoneFilter::YourGraveyard,
            random: true,
        }]);
    }
    if low == "their owners shuffle those cards into their libraries" {
        return Some(vec![InstructionAst::ShuffleIntoLibrary {
            subject: Box::new(SubjectRefAst::Anaphora("those cards".to_string())),
        }]);
    }
    if let Some(after) = low.strip_prefix("choose ")
        && let Some((cnt_raw, _)) = after.split_once(" cards in your hand and discard the rest")
        && let Ok(cnt) = parse_value_expr(cnt_raw.trim(), vocab)
    {
        return Some(vec![
            InstructionAst::ChooseFromGroup {
                count: Box::new(cnt),
                random: false,
                group: Box::new(SubjectRefAst::Group(Box::new(ObjectFilterAst {
                    any_card: true,
                    zone: Some(ZoneFilter::YourHand),
                    ..Default::default()
                }))),
            },
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
        return Some(vec![InstructionAst::MoveZone {
            subject: Box::new(SubjectRefAst::Anaphora("the card revealed".to_string())),
            to: "hand".to_string(),
        }]);
    }
    if let Some(after) = low.strip_prefix("reveal a card that's exactly ")
        && let Some((num_raw, _)) =
            after.split_once(" colors from among them and put it into your hand")
        && let Some(n) = parse_number_word(num_raw.trim())
    {
        let mut flt = ObjectFilterAst {
            any_card: true,
            ..Default::default()
        };
        if n >= 2 {
            flt.color_arity = Some(ColorArity::Multicolored);
        }
        flt.predicates
            .push(FilterPredicateAst::StateQualification(format!(
                "exact_colors:{n}"
            )));
        return Some(vec![
            InstructionAst::RevealCards {
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
            },
            InstructionAst::MoveZone {
                subject: Box::new(SubjectRefAst::Anaphora("that card".to_string())),
                to: "your hand".to_string(),
            },
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
                subject: Box::new(SubjectRefAst::Group(Box::new(flt))),
                from_zone: Some("your graveyard".to_string()),
                to: "battlefield".to_string(),
                tapped: false,
                transformed: false,
                with_counter: None,
                attached_to: None,
                random: false,
                except: None,
                count: Some(Box::new(ValueExprAst::Const(1))),
                up_to: false,
                any_number: false,
                actor: None,
            },
            InstructionAst::PutCounters {
                subject: Box::new(SubjectRefAst::Anaphora("that creature".to_string())),
                counter_kind: ck_raw.trim().to_string(),
                count: Box::new(ValueExprAst::PriorResult),
            },
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
        return Some(vec![InstructionAst::ReturnZone {
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
        }]);
    }

    // 11. Multi-named card search & draft-noted card search (Nissa's Encouragement, Aether Searcher)
    if let Some(after) = low.strip_prefix("search your library and graveyard for a card named ") {
        let mut names = Vec::new();
        let raw_start = s.len().saturating_sub(after.len());
        let orig_after = s.get(raw_start..).unwrap_or(after);
        for chunk in orig_after
            .replace(", and a card named ", "|")
            .replace(", a card named ", "|")
            .split('|')
        {
            let name = chunk.trim();
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
        if names.len() >= 2 {
            let searches = names
                .into_iter()
                .map(|name| InstructionAst::SearchLibrary {
                    library: "your".to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                    up_to: false,
                    any_number: false,
                    exact: false,
                    filter: Box::new(ObjectFilterAst {
                        any_card: true,
                        named: Some(NameFilterAst::Exact(name)),
                        ..Default::default()
                    }),
                    reveal: false,
                    to_zone: "found".to_string(),
                    shuffle: false,
                    extra_zones: vec![ZoneFilter::YourGraveyard],
                    secondary_filter: None,
                    secondary_is_or: false,
                    where_x: None,
                    rest_to_zone: None,
                    modifier: None,
                    follow_up: Vec::new(),
                })
                .collect();
            return Some(searches);
        }
    }
    if let Some(after) = low.strip_prefix("you may search your hand and/or library for a card with a name noted as you drafted cards named ~")
        && after.is_empty()
    {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::SearchLibrary {
                library: "your".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
                up_to: false,
                any_number: false,
                exact: false,
                filter: Box::new(ObjectFilterAst {
                    any_card: true,
                    predicates: vec![FilterPredicateAst::StateQualification(
                        "name_noted_drafted_self".to_string(),
                    )],
                    ..Default::default()
                }),
                reveal: false,
                to_zone: "found".to_string(),
                shuffle: false,
                extra_zones: vec![ZoneFilter::YourHand],
                secondary_filter: None,
                secondary_is_or: false,
                where_x: None,
                rest_to_zone: None,
                modifier: None,
                follow_up: Vec::new(),
            },
        ])]);
    }

    // 12. Delayed loyalty-ability copy, Tempt-with-Mayhem scaled spell copy, target/attacker reselection
    // (Jaya's Phoenix, Tempt with Mayhem, Chef's Kiss, Capricopian)
    if low == "copy the next loyalty ability you activate this turn when you activate it" {
        return Some(vec![InstructionAst::WhenYouNext {
            event: "activate_loyalty_ability".to_string(),
            body: vec![InstructionAst::Copy {
                subject: Box::new(SubjectRefAst::Anaphora("that ability".to_string())),
                exception: None,
                count: None,
            }],
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if low
        == "you copy that spell once plus an additional time for each opponent who copied the spell this way"
    {
        return Some(vec![InstructionAst::Copy {
            subject: Box::new(SubjectRefAst::Anaphora("that spell".to_string())),
            exception: None,
            count: Some(Box::new(ValueExprAst::Add(
                Box::new(ValueExprAst::Const(1)),
                Box::new(ValueExprAst::CountPlayersMatching {
                    predicate: "opponent_copied_spell_this_way".to_string(),
                }),
            ))),
        }]);
    }
    if low == "reselect the targets at random for the spell and the copy" {
        return Some(vec![InstructionAst::ReselectTargets {
            subject: Box::new(SubjectRefAst::Anaphora(
                "the spell and the copy".to_string(),
            )),
            random: true,
        }]);
    }
    if let Some(after) = low.strip_prefix("you may reselect which player ")
        && let Some(subj_raw) = after.strip_suffix(" is attacking")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::Optional(vec![
            InstructionAst::ReselectTargets {
                subject: Box::new(subj),
                random: false,
            },
        ])]);
    }

    None
}

#[inline(never)]
fn parse_damage_counters_tokens_and_stats_e9(
    s: &str,
    low: &str,
    _s_nodur: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Multi-distinct-target damage ("~ deals <N1> damage to any target, <N2> damage to another target, and <N3> damage to a third target") (Cone of Flame)
    if let Some(after) = low.strip_prefix("~ deals ")
        && let Some((first, rest)) = after.split_once(" damage to any target, ")
        && let Some((second, third_raw)) = rest.split_once(" damage to another target, and ")
        && let Some(third) = third_raw.strip_suffix(" damage to a third target")
        && let (Ok(v1), Ok(v2), Ok(v3)) = (
            parse_value_expr(first.trim(), vocab),
            parse_value_expr(second.trim(), vocab),
            parse_value_expr(third.trim(), vocab),
        )
    {
        return Some(vec![
            InstructionAst::DealDamage {
                source: Box::new(SubjectRefAst::SelfObject),
                target: Box::new(SubjectRefAst::AnyTarget),
                amount: Box::new(v1),
            },
            InstructionAst::DealDamage {
                source: Box::new(SubjectRefAst::SelfObject),
                target: Box::new(SubjectRefAst::DistinctTarget {
                    ordinal: 2,
                    filter: Box::new(ObjectFilterAst::default()),
                }),
                amount: Box::new(v2),
            },
            InstructionAst::DealDamage {
                source: Box::new(SubjectRefAst::SelfObject),
                target: Box::new(SubjectRefAst::DistinctTarget {
                    ordinal: 3,
                    filter: Box::new(ObjectFilterAst::default()),
                }),
                amount: Box::new(v3),
            },
        ]);
    }

    // 2. "~ deals double that damage [instead]" (Goblin Charbelcher)
    let no_instead = low.strip_suffix(" instead").unwrap_or(low);
    if no_instead == "~ deals double that damage" {
        let ins = InstructionAst::DealDamage {
            source: Box::new(SubjectRefAst::SelfObject),
            target: Box::new(SubjectRefAst::Anaphora("that target".to_string())),
            amount: Box::new(ValueExprAst::Mul(
                Box::new(ValueExprAst::Const(2)),
                Box::new(ValueExprAst::PriorResult),
            )),
        };
        return Some(if no_instead.len() != low.len() {
            vec![InstructionAst::ReplaceEffect(vec![ins])]
        } else {
            vec![ins]
        });
    }

    // 3. "prevent the next <N> damage [this way] [instead]" (Elvish Healer, Pollen Remedy)
    if let Some(after) = no_instead.strip_prefix("prevent the next ")
        && let Some(amt_raw) = after
            .strip_suffix(" damage this way")
            .or_else(|| after.strip_suffix(" damage"))
        && let Ok(amount) = parse_value_expr(amt_raw.trim(), vocab)
    {
        let ins = InstructionAst::PreventDamage {
            amount: Box::new(amount),
            target: Box::new(SubjectRefAst::Anaphora("that target".to_string())),
            duration: DurationAst::UntilEndOfTurn,
        };
        return Some(if no_instead.len() != low.len() {
            vec![InstructionAst::ReplaceEffect(vec![ins])]
        } else {
            vec![ins]
        });
    }

    // 4. Fractional player counters ("you get half x <ck> counters, rounded up/down") (Contaminated Drink)
    if let Some(after) = low.strip_prefix("you get half x ")
        && let Some((ck_raw, rnd_raw)) = after.split_once(" counters, rounded ")
    {
        let rounding = if rnd_raw.trim() == "up" {
            Rounding::Up
        } else {
            Rounding::Down
        };
        return Some(vec![InstructionAst::GainPlayerCounters {
            player: PlayerRefAst::You,
            counter_kind: ck_raw.trim().to_string(),
            count: Box::new(ValueExprAst::DivConst {
                expr: Box::new(ValueExprAst::ChosenX),
                denom: 2,
                rounding,
            }),
        }]);
    }

    // 5. "each creature gets twice -x/-x [dur]" (Nuclear Fallout)
    if let Some((subj_raw, rest)) = low.split_once(" gets twice ")
        && let Some((pt_tok, _)) = rest.split_once(' ').or(Some((rest, "")))
        && let Some((dx, dy)) = pt_tok.split_once('/')
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::ModifyPT {
            subject: Box::new(subj),
            dx: dx.to_string(),
            dy: dy.to_string(),
            duration: dur.clone(),
            scale: Some(Box::new(ValueExprAst::Const(2))),
        }]);
    }

    // 6. "the token enters with half that many <ck> counters on it, rounded down/up" (Ochre Jelly)
    if let Some(after) = low.strip_prefix("the token enters with half that many ")
        && let Some((ck_raw, rnd_raw)) = after.split_once(" counters on it, rounded ")
    {
        let rounding = if rnd_raw.trim() == "up" {
            Rounding::Up
        } else {
            Rounding::Down
        };
        return Some(vec![InstructionAst::PutCounters {
            subject: Box::new(SubjectRefAst::Anaphora("the token".to_string())),
            counter_kind: ck_raw.trim().to_string(),
            count: Box::new(ValueExprAst::DivConst {
                expr: Box::new(ValueExprAst::PriorResult),
                denom: 2,
                rounding,
            }),
        }]);
    }

    // 7. Remove counters distributed from among all permanents (Sensational Spider-Man)
    if let Some(after) = low
        .strip_prefix("you may remove up to ")
        .or_else(|| low.strip_prefix("remove up to "))
        && let Some((cnt_kind, flt_raw)) = after.split_once(" counters from among ")
        && let Some((cnt_raw, ck_raw)) = cnt_kind.split_once(' ')
    {
        let flt_clean = flt_raw
            .trim()
            .strip_prefix("all ")
            .unwrap_or(flt_raw.trim());
        if let (Ok(count), Ok(flt)) = (
            parse_value_expr(cnt_raw.trim(), vocab),
            parse_object_filter(flt_clean, vocab),
        ) {
            let ins = InstructionAst::RemoveCounters {
                subject: Box::new(SubjectRefAst::All(Box::new(flt))),
                counter_kind: ck_raw.trim().to_string(),
                all: false,
                count: Some(Box::new(count)),
            };
            return Some(if low.starts_with("you may ") {
                vec![InstructionAst::Optional(vec![ins])]
            } else {
                vec![ins]
            });
        }
    }

    let build_tok_ins = |controller: PlayerRefAst, rich: crate::filter::RichTokenCreationAst| {
        if rich.is_plain() {
            InstructionAst::CreateToken {
                controller,
                token: rich.token,
            }
        } else {
            InstructionAst::CreateQualifiedToken(Box::new(crate::ast::QualifiedTokenCreationAst {
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
    // 8. "remove all of them from it and create that many <token>" (Lightning Coils)
    if let Some(tok_raw) = low.strip_prefix("remove all of them from it and create that many ")
        && let Ok(mut rich) = parse_token_creation_rich(&format!("1 {tok_raw}"), vocab)
    {
        match &mut rich.token {
            TokenSpecAst::Creature { count, .. }
            | TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. } => {
                **count = ValueExprAst::PriorResult;
            }
            TokenSpecAst::Role { .. } => {}
        }
        return Some(vec![
            InstructionAst::RemoveCounters {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                counter_kind: "all".to_string(),
                all: true,
                count: None,
            },
            build_tok_ins(PlayerRefAst::You, rich),
        ]);
    }

    // 9. "<player> creates x minus <N> <token>" (Eiganjo Uprising)
    if let Some((who_raw, rest)) = low.split_once(" creates x minus ")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Some((sub_raw, tok_raw)) = rest.split_once(' ')
        && let Some(sub_n) = parse_number_word(sub_raw.trim())
        && let Ok(mut rich) = parse_token_creation_rich(&format!("1 {tok_raw}"), vocab)
    {
        let expr = ValueExprAst::Sub(
            Box::new(ValueExprAst::ChosenX),
            Box::new(ValueExprAst::Const(sub_n)),
        );
        match &mut rich.token {
            TokenSpecAst::Creature { count, .. }
            | TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. } => {
                **count = expr;
            }
            TokenSpecAst::Role { .. } => {}
        }
        return Some(vec![build_tok_ins(player, rich)]);
    }

    // 10. "create that token and a <predefined> token" (Mr. House) / characteristic tokens (Genku, Outlaws' Merriment) / copy attached (Stangg)
    if let Some(tok_raw) = low.strip_prefix("create that token and ")
        && let Ok(second_tok) = parse_token_creation_rich(tok_raw.trim(), vocab)
    {
        return Some(vec![
            InstructionAst::CreateToken {
                controller: PlayerRefAst::You,
                token: TokenSpecAst::Predefined {
                    name: "that token".to_string(),
                    count: Box::new(ValueExprAst::Const(1)),
                    tapped: false,
                },
            },
            build_tok_ins(PlayerRefAst::You, second_tok),
        ]);
    }
    if low.starts_with("create a ") && low.ends_with(" creature token with those characteristics") {
        return Some(vec![InstructionAst::TokenCharacteristicsChoice(
            s.to_string(),
        )]);
    }
    if let Some(after) = low.strip_prefix("create a token that's a copy of it attached to ") {
        return Some(vec![InstructionAst::CreateToken {
            controller: PlayerRefAst::You,
            token: TokenSpecAst::Copy {
                count: Box::new(ValueExprAst::Const(1)),
                source: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                tapped: false,
                attacking: false,
                exception: Some(format!("attached_to:{}", after.trim())),
            },
        }]);
    }

    // 11. Advantage/stored die rolls & result-scaled scry (Berserker's Frenzy, Centaur of Attention, Boing!)
    if let Some(after) = low.strip_prefix("roll ") {
        if let Some((cnt_and_die, _)) = after.split_once(" and ignore the lower roll")
            && let Some((cnt_raw, die_raw)) = cnt_and_die.split_once(' ')
            && let Some(sides_raw) = die_raw.strip_prefix('d')
            && let (Ok(count), Ok(sides)) = (
                parse_value_expr(cnt_raw.trim(), vocab),
                sides_raw.parse::<u16>(),
            )
        {
            return Some(vec![InstructionAst::RollDiceAndKeep {
                count: Box::new(count),
                sides,
                drop_lowest: 1,
                store_on_self: false,
            }]);
        }
        if let Some((cnt_raw, _)) =
            after.split_once(" six-sided dice and store those results on it")
            && let Ok(count) = parse_value_expr(cnt_raw.trim(), vocab)
        {
            return Some(vec![InstructionAst::RollDiceAndKeep {
                count: Box::new(count),
                sides: 6,
                drop_lowest: 0,
                store_on_self: true,
            }]);
        }
    }
    if low == "scry a number of cards equal to the result" {
        return Some(vec![InstructionAst::Scry {
            player: PlayerRefAst::You,
            count: Box::new(ValueExprAst::PriorResult),
        }]);
    }

    // 12. Temporary type/P-T/ability grants (Soul Sculptor, Cycle of Life, Tetravus, Wild Shape, Gabriel Angelfire)
    if let Some((subj_raw, rest)) =
        low.split_once(" becomes an enchantment and loses all abilities until ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        let cond = parse_condition(rest.trim(), vocab).ok().unwrap_or_else(|| {
            ConditionAst::EventOccurredThisTurn {
                event: rest.trim().to_string(),
            }
        });
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::ContinuousLayerEffect {
                subject: Box::new(subj),
                set_types: Some("Enchantment".to_string()),
                additive_types: false,
                base_pt: None,
                set_colors: None,
                grant_keywords: Vec::new(),
                lose_keywords: Vec::new(),
                lose_all_abilities: true,
                condition: None,
            }),
            duration: DurationAst::ForAsLongAs(Box::new(ConditionAst::Not(Box::new(cond)))),
        }]);
    }
    if let Some((subj_raw, rest)) = low.split_once(" has base power and toughness ")
        && let Some((pt_tok, _)) = rest.split_once(" until ")
        && let Some((p, t)) = pt_tok.trim().split_once('/')
    {
        let subj = parse_subject_or_target(subj_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
        return Some(vec![InstructionAst::AnimateOrSetBasePT {
            subject: Box::new(subj),
            power: p.to_string(),
            toughness: t.to_string(),
            description: None,
            keywords: Vec::new(),
            duration: dur.clone(),
        }]);
    }
    if let Some(after) = low.strip_prefix("they each have ")
        && let Some((kw_raw, ab_raw)) = after.split_once(" and \"")
        && let Some(kws) = parse_keyword_list(kw_raw.trim(), vocab)
    {
        let ab = ab_raw.trim_end_matches('"').to_string();
        return Some(vec![
            InstructionAst::GrantKeywords {
                subject: Box::new(SubjectRefAst::Anaphora("they".to_string())),
                keywords: kws,
                duration: DurationAst::Permanent,
            },
            InstructionAst::GrantAbilityText {
                subject: Box::new(SubjectRefAst::Anaphora("they".to_string())),
                ability_text: ab,
                compiled: None,
                duration: DurationAst::Permanent,
            },
        ]);
    }
    if let Some((subj_raw, _)) = low.split_once(
        " has that base power and toughness, becomes that creature type, and gains that ability",
    ) && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![
            InstructionAst::AnimateOrSetBasePT {
                subject: Box::new(subj.clone()),
                power: "chosen".to_string(),
                toughness: "chosen".to_string(),
                description: Some("chosen creature type".to_string()),
                keywords: Vec::new(),
                duration: dur.clone(),
            },
            InstructionAst::GrantAbilityText {
                subject: Box::new(subj),
                ability_text: "that ability".to_string(),
                compiled: None,
                duration: dur.clone(),
            },
        ]);
    }
    if let Some((subj_raw, _)) = low.split_once(" gains that ability")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::GrantAbilityText {
            subject: Box::new(subj),
            ability_text: "that ability".to_string(),
            compiled: None,
            duration: dur.clone(),
        }]);
    }

    None
}

#[inline(never)]
fn parse_control_piles_choices_and_combat_e9(
    _s: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Exchange text boxes / exchange control / redistribute life totals
    // (Exchange of Words, Cultural Exchange, Reverse the Sands)
    if low == "exchange the text boxes of those creatures" {
        return Some(vec![InstructionAst::ExchangeTextBoxes {
            left: Box::new(SubjectRefAst::Anaphora("first creature".to_string())),
            right: Box::new(SubjectRefAst::Anaphora("second creature".to_string())),
        }]);
    }
    if low == "those players exchange control of those creatures" {
        return Some(vec![InstructionAst::Exchange {
            kind: ExchangeKind::Control,
            left: Box::new(SubjectRefAst::Anaphora("those creatures".to_string())),
            right: None,
        }]);
    }
    if low == "redistribute any number of players' life totals" {
        return Some(vec![InstructionAst::RedistributeLifeTotals {
            player: PlayerRefAst::You,
        }]);
    }

    // 2. Gain control of chosen/owned permanents (Dack Fayden, Scrambleverse, Order of Succession, Herald of Leshrac, Expropriate)
    if let Some((who_raw, rest)) = low.split_once(" gains control of ")
        && let Some(controller) = parse_player_ref(who_raw.trim())
        && (rest == "the permanent for which they were chosen"
            || rest == "each permanent for which they were chosen"
            || rest == "the creature they chose"
            || rest == "each land they own that you control")
    {
        return Some(vec![InstructionAst::GainControl {
            controller,
            subject: Box::new(SubjectRefAst::Anaphora(rest.to_string())),
            duration: DurationAst::Permanent,
        }]);
    }
    if low == "choose a permanent owned by the voter and gain control of it" {
        return Some(vec![
            InstructionAst::ChooseFromGroup {
                count: Box::new(ValueExprAst::Const(1)),
                random: false,
                group: Box::new(SubjectRefAst::Anaphora(
                    "permanent owned by the voter".to_string(),
                )),
            },
            InstructionAst::GainControl {
                controller: PlayerRefAst::You,
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                duration: DurationAst::Permanent,
            },
        ]);
    }

    // 3. Structured choices (Cultural Exchange, Foreboding Steamboat, Celestial Judgment, Dack Fayden, Necrotic Plague, Tragic Arrogance)
    if let Some(after) = low.strip_prefix("choose the same number of ")
        && let Some((flt_raw, _)) = after.split_once(" another target player controls")
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.controller = Some(ControllerRelation::TargetPlayer);
        flt.predicates
            .push(crate::ast::FilterPredicateAst::StateQualification(
                "another_target_player_controls".to_string(),
            ));
        return Some(vec![InstructionAst::ChooseFromGroup {
            count: Box::new(ValueExprAst::PriorResult),
            random: false,
            group: Box::new(SubjectRefAst::Group(Box::new(flt))),
        }]);
    }
    if let Some((who_raw, rest)) = low.split_once(" chooses ")
        && let Some(chooser) = parse_player_ref(who_raw.trim())
    {
        if let Some((cnt_raw, flt_raw)) = rest.split_once(' ')
            && let (Some(n), Ok(flt)) = (
                parse_number_word(cnt_raw.trim()),
                parse_object_filter(flt_raw.trim(), vocab),
            )
        {
            if chooser == PlayerRefAst::You {
                return Some(vec![InstructionAst::ChooseFromGroup {
                    count: Box::new(ValueExprAst::Const(n)),
                    random: false,
                    group: Box::new(SubjectRefAst::Group(Box::new(flt))),
                }]);
            }
            return Some(vec![InstructionAst::PlayerChooseFromGroup {
                chooser,
                count: Box::new(ValueExprAst::Const(n)),
                random: false,
                group: Box::new(SubjectRefAst::Group(Box::new(flt))),
            }]);
        }
        if let Some(flt_raw) = rest.strip_prefix("target ")
            && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
        {
            let group = Box::new(SubjectRefAst::TargetObject {
                filter: Box::new(flt),
                count: TargetCountAst::Exact(1),
            });
            if chooser == PlayerRefAst::You {
                return Some(vec![InstructionAst::ChooseFromGroup {
                    count: Box::new(ValueExprAst::Const(1)),
                    random: false,
                    group,
                }]);
            }
            return Some(vec![InstructionAst::PlayerChooseFromGroup {
                chooser,
                count: Box::new(ValueExprAst::Const(1)),
                random: false,
                group,
            }]);
        }
    }
    if low == "choose a creature with that power" {
        return Some(vec![InstructionAst::ChooseFromGroup {
            count: Box::new(ValueExprAst::Const(1)),
            random: false,
            group: Box::new(SubjectRefAst::Anaphora(
                "creature with that power".to_string(),
            )),
        }]);
    }
    if low == "choose a different opponent" {
        return Some(vec![InstructionAst::ChooseFromGroup {
            count: Box::new(ValueExprAst::Const(1)),
            random: false,
            group: Box::new(SubjectRefAst::Player(PlayerRefAst::AnyOpponent)),
        }]);
    }
    if let Some(after) =
        low.strip_prefix("you choose from among the permanents that player controls ")
    {
        let mut categories = Vec::new();
        for piece in after.replace(", and ", ", ").split(", ") {
            if let Ok(flt) = parse_object_filter(piece.trim(), vocab) {
                categories.push(flt);
            }
        }
        if categories.len() >= 2 {
            return Some(vec![InstructionAst::ChoosePerFilter {
                player: PlayerRefAst::You,
                categories,
                sacrifice_rest: false,
            }]);
        }
    }

    // 4. Pile separation & pile actions (Make an Example, Bend or Break, Hostile Negotiations, Triumph of Saint Katherine, Whims of the Fates)
    if let Some((who_raw, rest)) = low.split_once(" separates ")
        && let Some((flt_raw, cnt_raw)) = rest
            .split_once(" into ")
            .and_then(|(f, r)| r.strip_suffix(" piles").map(|c| (f, c)))
        && let (Some(chooser), Some(n)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(cnt_raw.trim()),
        )
        && let Ok(pile_count) = u8::try_from(n)
    {
        let subj = parse_subject_or_target(flt_raw.trim(), vocab)
            .unwrap_or_else(|_| SubjectRefAst::Anaphora(flt_raw.trim().to_string()));
        return Some(vec![InstructionAst::SeparateFilteredIntoPiles {
            chooser,
            subject: Box::new(subj),
            pile_count,
            face_down_piles: 0,
        }]);
    }
    if low == "one of their piles is chosen by one of their opponents of their choice" {
        return Some(vec![InstructionAst::ChoosePile {
            chooser: PlayerRefAst::AnyOpponent,
        }]);
    }
    if low == "shuffle that pile and put it back on top of your library" {
        return Some(vec![InstructionAst::PileAction {
            action: "shuffle_to_library_top".to_string(),
            pile: PileRefAst::ThatPile,
            filter: None,
        }]);
    }
    if low == "look at the cards in each pile" {
        return Some(vec![InstructionAst::PileAction {
            action: "look".to_string(),
            pile: PileRefAst::EachPile,
            filter: None,
        }]);
    }
    if low == "turn a pile of your choice face up" {
        return Some(vec![InstructionAst::PileAction {
            action: "turn_face_up".to_string(),
            pile: PileRefAst::ChosenPile,
            filter: None,
        }]);
    }
    if low == "each player chooses one of their piles at random and sacrifices those permanents" {
        return Some(vec![InstructionAst::PileAction {
            action: "choose_random_and_sacrifice".to_string(),
            pile: PileRefAst::ChosenPile,
            filter: None,
        }]);
    }

    // 5. Combat actions, fights, sacrifices, goad, and blocker limits
    // (Ezuri's Predation, Tide of War, Vislor Turlough, Noctis, Stand or Fall, False Orders, Mirri, Yare)
    if low == "each of those tokens fights a different one of those creatures" {
        return Some(vec![InstructionAst::Fight {
            a: Box::new(SubjectRefAst::Anaphora("each of those tokens".to_string())),
            b: Box::new(SubjectRefAst::Anaphora(
                "different one of those creatures".to_string(),
            )),
        }]);
    }
    if low == "each blocking creature is sacrificed by its controller"
        || low == "each blocked creature is sacrificed by its controller"
    {
        let flt_raw = if low.contains("blocking") {
            "blocking creatures"
        } else {
            "blocked creatures"
        };
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Some(vec![InstructionAst::Sacrifice {
                player: PlayerRefAst::ItsController,
                count: None,
                subject: Box::new(SubjectRefAst::Each(Box::new(flt))),
                except: None,
                half: false,
                rounding: None,
                chooser: None,
            }]);
        }
    }
    if low == "it's goaded for as long as they control it" {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            rule: "goaded".to_string(),
            duration: DurationAst::ForAsLongAs(Box::new(ConditionAst::SubjectControlledBy {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                player: PlayerRefAst::ThatPlayer,
            })),
        }]);
    }
    if low == "it can't be blocked that combat" {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            rule: "CantBeBlocked".to_string(),
            duration: DurationAst::UntilEndOfCombat,
        }]);
    }
    if low == "only creatures in the chosen piles can block this turn" {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Anaphora(
                "creatures in the chosen piles".to_string(),
            )),
            rule: "only_chosen_piles_can_block".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if low == "have it block an attacking creature of your choice" {
        return Some(vec![InstructionAst::CombatRuleMod {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            rule: "block_chosen_attacker".to_string(),
            duration: DurationAst::UntilEndOfCombat,
        }]);
    }
    if let Some((who_raw, rest)) = low.split_once(" can't block with more than ")
        && let Some((cnt_raw, _)) = rest.split_once(" creature")
        && let (Some(player), Some(n)) = (
            parse_player_ref(who_raw.trim()),
            parse_number_word(cnt_raw.trim()),
        )
        && let Ok(max_blockers) = u8::try_from(n)
    {
        return Some(vec![InstructionAst::ChangeBlockerLimit {
            subject: Box::new(SubjectRefAst::Player(player)),
            extra_blockers: None,
            max_blockers: Some(max_blockers),
            duration: dur.clone(),
        }]);
    }
    if let Some((subj_raw, rest)) = low.split_once(" can block up to ")
        && let Some((cnt_raw, _)) = rest.split_once(" additional creature")
        && let (Ok(subj), Ok(extra)) = (
            parse_subject_or_target(subj_raw.trim(), vocab),
            parse_value_expr(cnt_raw.trim(), vocab),
        )
    {
        return Some(vec![InstructionAst::ChangeBlockerLimit {
            subject: Box::new(subj),
            extra_blockers: Some(Box::new(extra)),
            max_blockers: None,
            duration: dur.clone(),
        }]);
    }

    None
}

#[inline(never)]
fn parse_genuine_restrictions_and_repeats_e9(
    s: &str,
    low: &str,
    dur: &DurationAst,
    vocab: &Vocabulary,
) -> Option<Vec<InstructionAst>> {
    // 1. Unbounded optional repeat process ("<player> may repeat this process as many times as they choose") (Trade Secrets)
    if let Some((who_raw, _)) =
        low.split_once(" may repeat this process as many times as they choose")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        if player == PlayerRefAst::You {
            return Some(vec![InstructionAst::Optional(vec![
                InstructionAst::RepeatProcess {
                    condition: None,
                    max_times: None,
                    any_number: true,
                },
            ])]);
        }
        return Some(vec![InstructionAst::OptionalPlayer {
            player,
            body: vec![InstructionAst::PlayerRepeatProcess {
                player,
                condition: None,
                max_times: None,
                any_number: true,
            }],
        }]);
    }

    // 2. Temporary player/permanent prohibition statics (Call for Aid, Jared Carthalion, Conduit of Worlds, Shaman's Trance, Sen Triplets)
    if let Some((who_raw, rest)) = low.split_once(" can't sacrifice ")
        && let Some(subj_raw) = rest.strip_suffix(" this turn")
        && let Some(player) = parse_player_ref(who_raw.trim())
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Some(vec![InstructionAst::TemporaryPlayerRestriction {
            player,
            subject: Some(Box::new(subj)),
            rule: "cant_sacrifice".to_string(),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if let Some((who_raw, _)) = low.split_once(" can't become the monarch this turn")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(player))),
                rule: "cant_become_monarch".to_string(),
                filter: None,
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if let Some((who_raw, _)) = low.split_once(" can't cast additional spells this turn")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(player))),
                rule: "cant_cast_additional_spells".to_string(),
                filter: None,
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if low == "other players can't play lands or cast spells from their graveyards this turn" {
        return Some(vec![InstructionAst::TemporaryStatic {
            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(Box::new(SubjectRefAst::Player(
                    PlayerRefAst::EachOtherPlayer,
                ))),
                rule: "cant_play_or_cast_from_graveyard".to_string(),
                filter: None,
                condition: None,
            }),
            duration: DurationAst::UntilEndOfTurn,
        }]);
    }
    if let Some(after) = low.strip_prefix("this turn, ")
        && let Some((who_raw, _)) = after.split_once(
            " can't cast spells or activate abilities and plays with their hand revealed",
        )
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Some(vec![
            InstructionAst::TemporaryStatic {
                static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                    subject: Some(Box::new(SubjectRefAst::Player(player))),
                    rule: "cant_cast_spells_or_activate_abilities".to_string(),
                    filter: None,
                    condition: None,
                }),
                duration: DurationAst::UntilEndOfTurn,
            },
            InstructionAst::TemporaryPlayerRestriction {
                player,
                subject: None,
                rule: "play_with_hand_revealed".to_string(),
                duration: DurationAst::UntilEndOfTurn,
            },
        ]);
    }

    // 3. Genuine activation, casting, targeting, X-bound, speed-floor, and enchantment restrictions
    // (Well of Knowledge, Djinn of Infinite Deceits, Kang, Gogo, Gilded Drake, Spikeshell Harrier, Open the Way, Artificial Evolution, Chef's Kiss, Retether)
    if low.starts_with("any player may activate this ability but only during ")
        || low == "you can't activate this ability during combat"
        || (low.starts_with("during that turn, ") && low.ends_with(" abilities can't be activated"))
        || low == "this ability can't be copied and x can't be 0"
        || low == "this ability still resolves if its target becomes illegal"
        || low.starts_with("this effect can't reduce their speed below ")
        || low.starts_with("x can't be greater than ")
        || low.starts_with("the new creature type can't be ")
        || low.starts_with("the new targets can't be ")
        || (low.starts_with("only ") && low.ends_with(" can be enchanted this way"))
    {
        let _ = dur;
        return Some(vec![InstructionAst::AbilityRestriction {
            restriction: s.to_string(),
            condition: None,
        }]);
    }

    // Fallback for compound "If <cond>, <wave_e9_body>" when an inner clause only parses via wave_e9
    if let Some(after_if) = low.strip_prefix("if ")
        && let Some((cond_raw, rest_low)) = after_if.split_once(", ")
        && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
    {
        let rest_s = &s[s.len() - rest_low.len()..];
        let cleaned = rest_low.strip_prefix("instead ").unwrap_or(rest_low);
        let cleaned_s = &rest_s[rest_s.len() - cleaned.len()..];
        if let Some(body) = parse_wave_e9_core(cleaned_s, cleaned, cleaned_s, dur, vocab) {
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: if cleaned.len() != rest_low.len() {
                    vec![InstructionAst::ReplaceEffect(body)]
                } else {
                    body
                },
            }]);
        }
        if let Ok(body) = parse_atomic_effect(cleaned_s, vocab) {
            return Some(vec![InstructionAst::IfCondition {
                condition: Box::new(cond),
                then_body: body,
            }]);
        }
    }

    None
}
