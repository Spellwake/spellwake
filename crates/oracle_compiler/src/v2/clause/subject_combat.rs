//! Combat restrictions, `can't` predicates, compound damage targets, and trailing duration
//! helpers for subject-headed effect clauses in `v2`.

use crate::ast::{CardTypeMask, Rounding};
use crate::v2::condition::parse_condition;
use crate::v2::filter::parse_subject_ref;
use crate::v2::ir::{
    Duration, Effect, ObjectMutation, ObjectStateAction, PlayerRef, PlayerRestrictionKind,
    RuleRestrictionKind, StaticAbility, SubjectRef, ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

use super::subject_verbs::parse_subject_verb_frame;
use super::{
    apply_continuous, compile_quoted_face, eat_rounding, eat_type_color_subtype_run, mutate_obj,
    simple_zone_op,
};

/// Parse `<subj> can't ... [duration]` (handles both player-level and object-level restrictions).
pub(super) fn parse_subject_cant(
    ts: &mut TokenStream<'_>,
    subject: SubjectRef,
) -> PResult<Vec<Effect>> {
    // `<subj> can't attack/block [<tgt>] unless their/its controller pays <cost> [for each <expr>]` (`War Cadence`, `Forbidding Spirit`, `Sivitri`, `War Tax`)
    {
        let tax_cp = ts.checkpoint();
        if ts.eat_word("attack") || ts.eat_word("block") {
            let target = if !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("unless"))
            {
                let t1 = opt(ts, parse_subject_ref).unwrap_or_else(|| subject.clone());
                let ocp = ts.checkpoint();
                if ts.eat_word("or")
                    && let Ok(t2) = try_parse(ts, parse_subject_ref)
                {
                    SubjectRef::Compound(vec![t1, t2])
                } else {
                    ts.restore(ocp);
                    t1
                }
            } else {
                subject.clone()
            };
            if ts.eat_word("unless")
                && ts.eat_any_phrase(&[
                    &["their", "controller", "pays"],
                    &["its", "controller", "pays"],
                ])
                && let Ok(cost) = try_parse(ts, crate::v2::cost::parse_cost_atom)
            {
                let per_creature = ts.eat_phrase(&["for", "each", "of", "those", "creatures"])
                    || opt(ts, parse_for_each_expr).is_some();
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![apply_continuous(
                    StaticAbility::AttackTax {
                        target,
                        cost,
                        per_creature,
                    },
                    duration,
                )]);
            }
        }
        ts.restore(tax_cp);
    }
    let player_opt = match subject {
        SubjectRef::Player(p) => Some(p),
        SubjectRef::TargetPlayer { opponent_only, .. } => Some(if opponent_only {
            PlayerRef::TargetOpponent
        } else {
            PlayerRef::TargetPlayer
        }),
        _ => None,
    };
    if let Some(player) = player_opt {
        if ts.eat_word("attack")
            && let Ok(atk_tgt) = try_parse(ts, parse_subject_ref)
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![apply_continuous(
                StaticAbility::RuleRestriction {
                    subject: Some(atk_tgt),
                    rule: RuleRestrictionKind::CantAttack,
                    condition: None,
                },
                duration,
            )]);
        }
        const PLAYER_CANT_TABLE: &[(&[&str], PlayerRestrictionKind)] = &[
            (
                &["cast", "additional", "spells"],
                PlayerRestrictionKind::CantCastMoreThanOnePerTurn,
            ),
            (&["gain", "life"], PlayerRestrictionKind::CantGainLife),
            (
                &["activate", "abilities"],
                PlayerRestrictionKind::CantActivateAbilities,
            ),
            (
                &["search", "libraries"],
                PlayerRestrictionKind::CantSearchLibraries,
            ),
        ];
        if let Some(restriction) = PLAYER_CANT_TABLE
            .iter()
            .find_map(|&(p, r)| ts.eat_phrase(p).then_some(r))
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![apply_continuous(
                StaticAbility::PlayerRestriction {
                    player,
                    restriction,
                    condition: None,
                },
                duration,
            )]);
        }
        if ts.eat_phrase(&["play", "lands"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![apply_continuous(
                StaticAbility::CantCastSpellsMatching {
                    player,
                    filter: crate::v2::ir::ObjectFilter {
                        any_types: CardTypeMask::LAND,
                        ..Default::default()
                    },
                    condition: None,
                },
                duration,
            )]);
        }
        let is_win = ts.eat_phrase(&["win", "the", "game"]);
        if is_win || ts.eat_phrase(&["lose", "the", "game"]) {
            let also_other = if is_win {
                ts.eat_phrase(&["or", "lose", "the", "game"])
            } else {
                ts.eat_phrase(&["or", "win", "the", "game"])
            };
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            let (first_r, second_r) = if is_win {
                (
                    PlayerRestrictionKind::CantWinGame,
                    PlayerRestrictionKind::CantLoseGame,
                )
            } else {
                (
                    PlayerRestrictionKind::CantLoseGame,
                    PlayerRestrictionKind::CantWinGame,
                )
            };
            let mut out = vec![apply_continuous(
                StaticAbility::PlayerRestriction {
                    player,
                    restriction: first_r,
                    condition: None,
                },
                duration.clone(),
            )];
            if also_other {
                out.push(apply_continuous(
                    StaticAbility::PlayerRestriction {
                        player,
                        restriction: second_r,
                        condition: None,
                    },
                    duration,
                ));
            }
            let acp = ts.checkpoint();
            if ts.eat_word("and")
                && let Ok(more) = try_parse(ts, parse_subject_verb_frame)
            {
                out.extend(more);
            } else {
                ts.restore(acp);
            }
            return Ok(out);
        }
        if ts.eat_word("cast")
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            let duration = if ts.eat_any_phrase(&[
                &["during", "that", "player's", "next", "turn"],
                &["during", "their", "next", "turn"],
            ]) {
                Duration::UntilEndOfNextTurn
            } else {
                parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn)
            };
            let mut out = vec![apply_continuous(
                StaticAbility::RuleRestriction {
                    subject: Some(SubjectRef::Group(Box::new(flt))),
                    rule: RuleRestrictionKind::CantBeCast,
                    condition: None,
                },
                duration,
            )];
            let acp = ts.checkpoint();
            let _ = ts.eat_comma();
            if ts.eat_word("and")
                && let Ok(more) = try_parse(ts, parse_subject_verb_frame)
            {
                out.extend(more);
            } else {
                ts.restore(acp);
            }
            return Ok(out);
        }
    }
    let mutation = if ts.eat_phrase(&["become", "untapped"]) {
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantBecomeUntapped)
    } else if ts.eat_phrase(&["be", "the", "targets", "of"])
        || ts.eat_phrase(&["be", "the", "target", "of"])
    {
        let flt = crate::v2::filter::parse_object_filter(ts)?;
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeTargetedBy(Box::new(flt)))
    } else if ts.eat_phrase(&["attack", "or", "block"]) {
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttackOrBlock)
    } else if ts.eat_word("attack") {
        let tcp = ts.checkpoint();
        if let Ok(tgt) = try_parse(ts, parse_subject_ref) {
            let ocp = ts.checkpoint();
            if ts.eat_word("or")
                && let Ok(tgt2) = try_parse(ts, parse_subject_ref)
            {
                ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttackTarget(Box::new(
                    SubjectRef::Compound(vec![tgt, tgt2]),
                )))
            } else {
                ts.restore(ocp);
                ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttackTarget(Box::new(tgt)))
            }
        } else {
            ts.restore(tcp);
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttack)
        }
    } else if ts.eat_word("block") {
        if let Ok(blk_subj) = try_parse(ts, parse_subject_ref) {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBlockSubject(Box::new(blk_subj)))
        } else {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBlock)
        }
    } else if ts.eat_phrase(&["be", "blocked"]) {
        let _ = ts.eat_phrase(&["this", "turn"]);
        if ts.eat_phrase(&["except", "by"])
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlockedExceptBy(Box::new(flt)))
        } else if ts.eat_word("by")
            && let Ok(flt) = try_parse(ts, crate::v2::filter::parse_object_filter)
        {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlockedBy(Box::new(flt)))
        } else {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeBlocked)
        }
    } else if ts.eat_phrase(&["be", "prevented"]) {
        ObjectMutation::ApplyRule(RuleRestrictionKind::DamageCantBePrevented)
    } else if ts.eat_phrase(&["be", "regenerated"]) {
        ObjectMutation::CantBeRegenerated
    } else if ts.eat_phrase(&["be", "countered"]) {
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeCountered)
    } else if ts.eat_phrase(&["phase", "out"]) {
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantPhaseOut)
    } else if ts.eat_phrase(&["be", "cast"]) || ts.eat_phrase(&["be", "played"]) {
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeCast)
    } else {
        return Err(ts.err("expected combat restriction after `can't`"));
    };
    let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
    let mut out = vec![mutate_obj(subject.clone(), mutation, duration)];
    // Optional `and has base power and toughness P/T [dur]` (`Atomic Microsizer`)
    {
        let hcp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["and", "has", "base", "power", "and", "toughness"],
            &["and", "have", "base", "power", "and", "toughness"],
        ]) && let Some((power, toughness)) = ts.next_base_pt()
        {
            let d2 = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            out.push(mutate_obj(
                subject,
                ObjectMutation::SetDynamicBasePT {
                    power: ValueExpr::Const(power),
                    toughness: ValueExpr::Const(toughness),
                    add_types: CardTypeMask::EMPTY,
                    keep_existing_types: true,
                },
                d2,
            ));
        } else {
            ts.restore(hcp);
        }
    }
    Ok(out)
}

pub(super) fn parse_damage_target(ts: &mut TokenStream<'_>) -> PResult<SubjectRef> {
    let first_tgt = parse_subject_ref(ts)?;
    let mut tgts = vec![first_tgt];
    loop {
        let tcp = ts.checkpoint();
        if ts.eat_word("and") {
            let acp = ts.checkpoint();
            if opt(ts, parse_value_expr).is_some() && ts.eat_word("damage") {
                ts.restore(tcp);
                break;
            }
            ts.restore(acp);
            if let Ok(next_t) = try_parse(ts, parse_subject_ref) {
                let after_is_boundary = ts.is_eof()
                    || matches!(
                        ts.peek().map(|t| &t.kind),
                        Some(TokenKind::Comma | TokenKind::Period)
                    )
                    || ts.peek_word().is_some_and(|w| {
                        matches!(
                            w.to_ascii_lowercase().as_str(),
                            "and" | "for" | "until" | "if" | "unless" | "this" | "at" | "equal"
                        )
                    });
                if after_is_boundary {
                    tgts.push(next_t);
                    continue;
                }
            }
        }
        ts.restore(tcp);
        break;
    }
    Ok(if tgts.len() == 1 {
        tgts.remove(0)
    } else {
        SubjectRef::Compound(tgts)
    })
}

pub(super) fn parse_trailing_duration(ts: &mut TokenStream<'_>) -> Option<Duration> {
    if ts.eat_any_phrase(&[&["until", "end", "of", "turn"], &["this", "turn"]]) {
        return Some(Duration::UntilEndOfTurn);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_word("until") && ts.eat_tilde() {
            if ts.eat_phrase(&["leaves", "the", "battlefield"]) {
                return Some(Duration::UntilSourceLeaves);
            }
            if ts.eat_phrase(&["is", "cast", "from", "exile"]) {
                return Some(Duration::ForAsLongAsExiled);
            }
        }
        ts.restore(cp);
    }
    const FIXED_DURATIONS: &[(&[&str], Duration)] = &[
        (
            &["until", "end", "of", "combat"],
            Duration::UntilEndOfCombat,
        ),
        (&["at", "end", "of", "combat"], Duration::UntilEndOfCombat),
        (&["this", "combat"], Duration::UntilEndOfCombat),
        (&["that", "combat"], Duration::UntilEndOfCombat),
        (
            &["until", "your", "next", "end", "step"],
            Duration::UntilYourNextEndStep,
        ),
        (
            &["until", "the", "next", "end", "step"],
            Duration::UntilYourNextEndStep,
        ),
        (
            &["until", "your", "next", "upkeep"],
            Duration::UntilYourNextUpkeep,
        ),
        (
            &["until", "its", "controller's", "next", "untap", "step"],
            Duration::UntilYourNextUpkeep,
        ),
        (
            &["until", "their", "controller's", "next", "untap", "step"],
            Duration::UntilYourNextUpkeep,
        ),
        (
            &["until", "their", "next", "untap", "step"],
            Duration::UntilYourNextUpkeep,
        ),
        (
            &["until", "your", "next", "turn"],
            Duration::UntilYourNextTurn,
        ),
        (
            &["until", "the", "end", "of", "your", "next", "turn"],
            Duration::UntilEndOfNextTurn,
        ),
        (
            &["until", "the", "end", "of", "their", "next", "turn"],
            Duration::UntilEndOfNextTurn,
        ),
        (
            &[
                "until", "the", "end", "of", "that", "player's", "next", "turn",
            ],
            Duration::UntilEndOfNextTurn,
        ),
        (
            &[
                "until",
                "the",
                "end",
                "of",
                "its",
                "controller's",
                "next",
                "turn",
            ],
            Duration::UntilEndOfNextTurn,
        ),
        (
            &["during", "that", "player's", "next", "turn"],
            Duration::UntilEndOfNextTurn,
        ),
        (
            &["during", "their", "next", "turn"],
            Duration::UntilEndOfNextTurn,
        ),
        (
            &["during", "its", "controller's", "next", "turn"],
            Duration::UntilEndOfNextTurn,
        ),
    ];
    if let Some(d) = FIXED_DURATIONS
        .iter()
        .find_map(|(p, d)| ts.eat_phrase(p).then(|| d.clone()))
    {
        return Some(d);
    }
    {
        let lcp = ts.checkpoint();
        if ts.eat_word("until")
            && (ts.eat_phrase(&["it", "leaves", "the", "battlefield"])
                || (try_parse(ts, parse_subject_ref).is_ok()
                    && ts.eat_phrase(&["leaves", "the", "battlefield"])))
        {
            return Some(Duration::UntilSourceLeaves);
        }
        ts.restore(lcp);
    }
    if ts.eat_phrase(&["for", "the", "rest", "of", "the", "game"]) {
        return Some(Duration::ForTheRestOfTheGame);
    }
    if ts.eat_phrase(&["until", "an", "opponent", "becomes", "the", "monarch"]) {
        return Some(Duration::ForAsLongAs(Box::new(
            crate::v2::ir::ConditionExpr::Not(Box::new(
                crate::v2::ir::ConditionExpr::PlayerCheck {
                    player: PlayerRef::AnyOpponent,
                    kind: crate::v2::ir::PlayerConditionKind::IsMonarch,
                },
            )),
        )));
    }
    if ts.eat_any_phrase(&[
        &["for", "as", "long", "as", "it", "remains", "exiled"],
        &["for", "as", "long", "as", "they", "remain", "exiled"],
    ]) {
        return Some(Duration::ForAsLongAsExiled);
    }
    {
        let uecp = ts.checkpoint();
        if ts.eat_phrase(&["until", "you", "exile", "another", "card", "with"]) && ts.eat_tilde() {
            return Some(Duration::ForAsLongAsExiled);
        }
        ts.restore(uecp);
    }
    if ts.eat_any_phrase(&[&["for", "as", "long", "as"], &["as", "long", "as"]]) {
        let cond = parse_condition(ts).ok()?;
        return Some(Duration::ForAsLongAs(Box::new(cond)));
    }
    None
}

/// Extended subject-headed predicates delegated from `subject_verbs.rs`.
pub(super) fn parse_extra_subject_predicates(
    ts: &mut TokenStream<'_>,
    subject: SubjectRef,
) -> PResult<Vec<Effect>> {
    // 1. `<subj> assigns/assign combat damage equal to its/their toughness rather than its/their power [dur]`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["assigns", "assign"])
            && ts.eat_phrase(&["combat", "damage", "equal", "to"])
            && ts.eat_any_word(&["its", "their"])
            && ts.eat_phrase(&["toughness", "rather", "than"])
            && ts.eat_any_word(&["its", "their"])
            && ts.eat_word("power")
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![apply_continuous(
                StaticAbility::SubjectRule {
                    subject,
                    rule: crate::v2::ir::SubjectStaticRule::AssignDamageByToughness,
                    condition: None,
                },
                duration,
            )]);
        }
        ts.restore(cp);
    }

    // 2. `<subj> attacks/attack ... if able`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["attacks", "attack"]) {
            if ts.eat_any_phrase(&[
                &["each", "combat", "if", "able"],
                &["a", "player", "other", "than", "you", "if", "able"],
            ]) {
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
                    Duration::UntilYourNextTurn,
                )]);
            }
            if ts.eat_any_phrase(&[
                &[
                    "during",
                    "its",
                    "controller's",
                    "next",
                    "combat",
                    "phase",
                    "if",
                    "able",
                ],
                &[
                    "during",
                    "their",
                    "controller's",
                    "next",
                    "combat",
                    "phase",
                    "if",
                    "able",
                ],
            ]) {
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
                    Duration::UntilEndOfNextTurn,
                )]);
            }
            if ts.eat_any_phrase(&[&["or", "blocks"], &["or", "block"]])
                && ts.eat_any_phrase(&[
                    &["this", "combat", "if", "able"],
                    &["this", "turn", "if", "able"],
                    &["if", "able"],
                ])
            {
                return Ok(vec![
                    mutate_obj(
                        subject.clone(),
                        ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
                        Duration::UntilEndOfTurn,
                    ),
                    mutate_obj(
                        subject,
                        ObjectMutation::MustBlockThisTurn(None),
                        Duration::UntilEndOfTurn,
                    ),
                ]);
            }
            if ts.eat_phrase(&["if", "able"])
                || (try_parse(ts, parse_subject_ref).is_ok() && ts.eat_phrase(&["if", "able"]))
            {
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
                    Duration::UntilEndOfTurn,
                )]);
            }
        }
        ts.restore(cp);
    }

    // 3. `<subj> isn't/aren't [a/an] <card_type> [dur]` (`RemoveTypes`)
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["isn't", "aren't"]) {
            let _ = ts.eat_any_word(&["a", "an"]);
            if let Some(w) = ts.peek_word()
                && let Some(ct) = crate::vocab::lookup_card_type(&w.to_ascii_lowercase())
            {
                ts.next_word();
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![apply_continuous(
                    StaticAbility::SubjectRule {
                        subject,
                        rule: crate::v2::ir::SubjectStaticRule::RemoveTypes(ct),
                        condition: None,
                    },
                    duration,
                )]);
            }
        }
        ts.restore(cp);
    }

    // 4. `<subj> loses/lose all other card types and abilities [dur]`
    if ts.eat_any_phrase(&[
        &["loses", "all", "other", "card", "types", "and", "abilities"],
        &["lose", "all", "other", "card", "types", "and", "abilities"],
    ]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
        return Ok(vec![
            mutate_obj(
                subject.clone(),
                ObjectMutation::BecomeCreatureTypes {
                    subtypes: Vec::new(),
                    add_types: CardTypeMask::EMPTY,
                    keep_existing_types: false,
                    base_pt: None,
                    keywords: Vec::new(),
                },
                duration.clone(),
            ),
            mutate_obj(subject, ObjectMutation::RemoveAllAbilities, duration),
        ]);
    }

    // 5. `<subj> is/are a/an <P>/<T> [<colors>] <Subtypes/Types> [with <kws>] [in addition to its/their other [colors and] types] [dur]`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["is", "are"]) {
            let _ = ts.eat_any_word(&["a", "an"]);
            if let Some(base_pt) = ts.next_base_pt() {
                let (add_types, colors, subtypes) =
                    eat_type_color_subtype_run(ts, true, false, false);
                let mut keywords = Vec::new();
                if ts.eat_word("with")
                    && let Ok((kws, _)) = try_parse(ts, crate::v2::filter::parse_keyword_list)
                {
                    keywords = kws;
                }
                let keep_existing_types = ts.eat_in_addition_to_other_types();
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                let mut out = Vec::new();
                if !colors.is_empty() {
                    out.push(mutate_obj(
                        subject.clone(),
                        ObjectMutation::BecomeColors {
                            colors,
                            keep_existing_colors: keep_existing_types,
                        },
                        duration.clone(),
                    ));
                }
                out.push(mutate_obj(
                    subject,
                    ObjectMutation::BecomeCreatureTypes {
                        subtypes,
                        add_types,
                        keep_existing_types,
                        base_pt: Some(base_pt),
                        keywords,
                    },
                    duration,
                ));
                return Ok(out);
            }
        }
        ts.restore(cp);
    }

    // 6. Extended `<subj> doesn't/don't untap [during <owner> next [<N>] untap step(s)]`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["doesn't", "untap"], &["don't", "untap"]]) {
            if ts.eat_word("during") {
                if ts.eat_any_phrase(&[
                    &["their", "next"],
                    &["the", "player's", "next"],
                    &["target", "player's", "next"],
                    &["target", "opponent's", "next"],
                    &["its", "controller's", "next"],
                    &["their", "controller's", "next"],
                    &["their", "controllers'", "next"],
                    &["that", "player's", "next"],
                    &["your", "next"],
                ]) {
                    let _ = ts.next_number(false);
                    if ts.eat_any_phrase(&[&["untap", "step"], &["untap", "steps"]]) {
                        let duration =
                            parse_trailing_duration(ts).unwrap_or(Duration::UntilYourNextTurn);
                        return Ok(vec![mutate_obj(
                            subject,
                            ObjectMutation::StateAction(ObjectStateAction::DoesntUntapNextUntap),
                            duration,
                        )]);
                    }
                }
            } else if ts.is_eof() {
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::StateAction(ObjectStateAction::DoesntUntapNextUntap),
                    Duration::UntilYourNextTurn,
                )]);
            }
        }
        ts.restore(cp);
    }

    // 7. `<subj> becomes/become (all colors | legendary ...)`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["becomes", "become"]) {
            if ts.eat_phrase(&["all", "colors"]) {
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![mutate_obj(
                    subject,
                    ObjectMutation::BecomeColors {
                        colors: crate::ast::ColorMask::WHITE
                            .union(crate::ast::ColorMask::BLUE)
                            .union(crate::ast::ColorMask::BLACK)
                            .union(crate::ast::ColorMask::RED)
                            .union(crate::ast::ColorMask::GREEN),
                        keep_existing_colors: false,
                    },
                    duration,
                )]);
            }
            let _ = ts.eat_any_word(&["a", "an"]);
            if ts.eat_word("legendary") {
                let base_pt = ts.next_base_pt();
                let (add_types, colors, subtypes) =
                    eat_type_color_subtype_run(ts, true, false, false);
                if ts.eat_word("named") {
                    super::player_verbs_ext::eat_proper_name_words(ts);
                }
                let mut keywords = Vec::new();
                if ts.eat_word("with")
                    && let Ok((kws, _)) = try_parse(ts, crate::v2::filter::parse_keyword_list)
                {
                    keywords = kws;
                }
                if base_pt.is_some() || !add_types.is_empty() || !subtypes.is_empty() {
                    let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                    let mut out = Vec::new();
                    if !colors.is_empty() {
                        out.push(mutate_obj(
                            subject.clone(),
                            ObjectMutation::BecomeColors {
                                colors,
                                keep_existing_colors: false,
                            },
                            duration.clone(),
                        ));
                    }
                    out.push(mutate_obj(
                        subject,
                        ObjectMutation::BecomeCreatureTypes {
                            subtypes,
                            add_types,
                            keep_existing_types: false,
                            base_pt,
                            keywords,
                        },
                        duration,
                    ));
                    return Ok(out);
                }
            }
        }
        ts.restore(cp);
    }

    // 10. `<subj> assign(s) its/their combat damage [this turn] as though it/they weren't blocked`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["assigns", "assign"])
            && ts.eat_any_word(&["its", "their"])
            && ts.eat_phrase(&["combat", "damage"])
        {
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_phrase(&["as", "though"])
                && ts.eat_any_word(&["it", "they"])
                && ts.eat_phrase(&["weren't", "blocked"])
            {
                return Ok(vec![apply_continuous(
                    StaticAbility::SubjectRule {
                        subject,
                        rule: crate::v2::ir::SubjectStaticRule::AssignDamageAsUnblocked,
                        condition: None,
                    },
                    Duration::UntilEndOfTurn,
                )]);
            }
        }
        ts.restore(cp);
    }

    // 11. `<subj> must be blocked each combat [this turn] if able`
    if ts.eat_any_phrase(&[
        &[
            "must", "be", "blocked", "each", "combat", "this", "turn", "if", "able",
        ],
        &["must", "be", "blocked", "each", "combat", "if", "able"],
    ]) {
        return Ok(vec![apply_continuous(
            StaticAbility::RuleRestriction {
                subject: Some(subject),
                rule: RuleRestrictionKind::MustBeBlocked,
                condition: None,
            },
            Duration::UntilEndOfTurn,
        )]);
    }

    // 12. `<subj> attacks that combat if able`
    if ts.eat_any_phrase(&[
        &["attacks", "that", "combat", "if", "able"],
        &["attack", "that", "combat", "if", "able"],
    ]) {
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::StateAction(ObjectStateAction::MustAttackEachCombat),
            Duration::UntilEndOfCombat,
        )]);
    }

    // 13. `<subj> enters tapped and attacking`
    if ts.eat_any_phrase(&[
        &["enters", "tapped", "and", "attacking"],
        &["enter", "tapped", "and", "attacking"],
        &["enters", "tapped"],
        &["enter", "tapped"],
    ]) {
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::Tap,
            Duration::Instant,
        )]);
    }

    // 14. `<subj> loses/lose all other abilities [dur]`
    if ts.eat_any_phrase(&[
        &["loses", "all", "other", "abilities"],
        &["lose", "all", "other", "abilities"],
    ]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::RemoveAllAbilities,
            duration,
        )]);
    }

    // 15. Extended `loses`/`lose` predicates
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["loses", "lose"]) {
            let player = match subject {
                SubjectRef::Player(p) => p,
                _ => PlayerRef::ThatPlayer,
            };
            if ts.eat_any_phrase(&[
                &["all", "land", "types", "and", "abilities"],
                &["all", "other", "land", "types", "and", "abilities"],
            ]) {
                let mut out = vec![
                    mutate_obj(
                        subject.clone(),
                        ObjectMutation::BecomeCreatureTypes {
                            subtypes: Vec::new(),
                            add_types: CardTypeMask::EMPTY,
                            keep_existing_types: false,
                            base_pt: None,
                            keywords: Vec::new(),
                        },
                        Duration::Permanent,
                    ),
                    mutate_obj(
                        subject.clone(),
                        ObjectMutation::RemoveAllAbilities,
                        Duration::Permanent,
                    ),
                ];
                let hcp = ts.checkpoint();
                if ts.eat_any_phrase(&[&["and", "has"], &["and", "have"]])
                    && let Some(quoted) = ts.next_quoted()
                {
                    if let Ok(compiled) = compile_quoted_face(ts, "~", "Land", quoted) {
                        out.push(mutate_obj(
                            subject,
                            ObjectMutation::GrantAbility(Box::new(compiled)),
                            Duration::Permanent,
                        ));
                    } else {
                        ts.restore(hcp);
                    }
                } else {
                    ts.restore(hcp);
                }
                return Ok(out);
            }
            if ts.eat_word("all") {
                if ts.eat_word("but")
                    && let Ok(keep) = try_parse(ts, parse_value_expr)
                    && ts.eat_word("life")
                {
                    return Ok(vec![Effect::AdjustLife {
                        player,
                        amount: keep,
                        op: crate::v2::ir::LifeOp::Set,
                    }]);
                }
                let kind = opt(ts, crate::v2::filter::parse_counter_kind)
                    .unwrap_or(crate::v2::ir::CounterKind::Any);
                if ts.eat_any_word(&["counters", "counter"]) {
                    return Ok(vec![mutate_obj(
                        subject,
                        ObjectMutation::RemoveCounters {
                            kind,
                            count: None,
                            all: true,
                        },
                        Duration::Instant,
                    )]);
                }
            }
            if ts.eat_any_phrase(&[
                &["a", "third", "of", "their", "life"],
                &["a", "third", "of", "your", "life"],
            ]) {
                let rounding = eat_rounding(ts, Rounding::Up);
                return Ok(vec![Effect::AdjustLife {
                    player,
                    amount: ValueExpr::Div {
                        expr: Box::new(ValueExpr::PlayerStat {
                            player,
                            stat: crate::ast::PlayerStatKind::LifeTotal,
                        }),
                        denom: 3,
                        rounding,
                    },
                    op: crate::v2::ir::LifeOp::Lose,
                }]);
            }
        }
        ts.restore(cp);
    }

    // 16. `is colorless`, `is sacrificed by its controller`, `can cast only one more spell`
    if ts.eat_any_phrase(&[&["is", "colorless"], &["are", "colorless"]]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
        return Ok(vec![mutate_obj(
            subject,
            ObjectMutation::BecomeColors {
                colors: crate::ast::ColorMask::EMPTY,
                keep_existing_colors: false,
            },
            duration,
        )]);
    }
    if ts.eat_any_phrase(&[
        &["is", "sacrificed", "by", "its", "controller"],
        &["are", "sacrificed", "by", "their", "controllers"],
    ]) {
        return Ok(vec![simple_zone_op(
            PlayerRef::EachPlayer,
            crate::v2::ir::ZoneOpKind::Sacrifice,
            subject,
            Vec::new(),
            None,
        )]);
    }
    if ts.eat_phrase(&["can", "cast", "only", "one", "more", "spell"]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        let player = match subject {
            SubjectRef::Player(p) => p,
            _ => PlayerRef::EachPlayer,
        };
        return Ok(vec![apply_continuous(
            StaticAbility::PlayerRestriction {
                player,
                restriction: PlayerRestrictionKind::CantCastMoreThanOnePerTurn,
                condition: None,
            },
            duration,
        )]);
    }

    // 17. `[this turn] that are <colors> cost(s) {M} less/more to cast` + inline token/mana replacement predicates
    {
        let cp = ts.checkpoint();
        let _ = ts.eat_phrase(&["this", "turn"]);
        if ts.eat_phrase(&["that", "are"]) {
            let mut saw_c = false;
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if crate::vocab::lookup_color(&low).is_some()
                    || matches!(low.as_str(), "and/or" | "or" | "and")
                {
                    saw_c |= crate::vocab::lookup_color(&low).is_some();
                    ts.next_word();
                } else {
                    break;
                }
            }
            if saw_c
                && ts.eat_any_word(&["cost", "costs"])
                && let Some(delta) = ts.eat_mana_cost()
            {
                let less = if ts.eat_phrase(&["less", "to", "cast"]) {
                    Some(true)
                } else if ts.eat_phrase(&["more", "to", "cast"]) {
                    Some(false)
                } else {
                    None
                };
                if let Some(less) = less {
                    let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                    return Ok(vec![apply_continuous(
                        StaticAbility::CostModifier {
                            subject: crate::v2::ir::ObjectFilter {
                                any_spell: true,
                                ..Default::default()
                            },
                            delta,
                            less,
                            scale: None,
                            min_mana: None,
                            condition: None,
                        },
                        duration,
                    )]);
                }
            }
        }
        ts.restore(cp);
    }

    Err(ts.err("expected verb predicate after subject"))
}
