//! Combat restrictions, `can't` predicates, compound damage targets, and trailing duration
//! helpers for subject-headed effect clauses in `v2`.

use crate::ast::CardTypeMask;
use crate::v2::condition::parse_condition;
use crate::v2::filter::parse_subject_ref;
use crate::v2::ir::{
    Duration, Effect, ObjectMutation, PlayerRef, RuleRestrictionKind, StaticAbility, SubjectRef,
    ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

use super::subject_verbs::parse_subject_verb_frame;

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
                && (ts.eat_phrase(&["their", "controller", "pays"])
                    || ts.eat_phrase(&["its", "controller", "pays"]))
                && let Ok(cost) = try_parse(ts, crate::v2::cost::parse_cost_atom)
            {
                let per_creature = ts.eat_phrase(&["for", "each", "of", "those", "creatures"])
                    || opt(ts, parse_for_each_expr).is_some();
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::AttackTax {
                        target,
                        cost,
                        per_creature,
                    }),
                    duration,
                }]);
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
        if ts.eat_phrase(&["gain", "life"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player,
                    restriction: crate::v2::ir::PlayerRestrictionKind::CantGainLife,
                    condition: None,
                }),
                duration,
            }]);
        }
        if ts.eat_phrase(&[
            "activate",
            "abilities",
            "that",
            "aren't",
            "mana",
            "abilities",
        ]) || ts.eat_phrase(&["activate", "abilities"])
        {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player,
                    restriction: crate::v2::ir::PlayerRestrictionKind::CantActivateAbilities,
                    condition: None,
                }),
                duration,
            }]);
        }
        if ts.eat_phrase(&["search", "libraries"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player,
                    restriction: crate::v2::ir::PlayerRestrictionKind::CantSearchLibraries,
                    condition: None,
                }),
                duration,
            }]);
        }
        if ts.eat_phrase(&["play", "lands"]) {
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            return Ok(vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::CantCastSpellsMatching {
                    player,
                    filter: crate::v2::ir::ObjectFilter {
                        any_types: CardTypeMask::LAND,
                        ..Default::default()
                    },
                    condition: None,
                }),
                duration,
            }]);
        }
        let is_win = ts.eat_phrase(&["win", "the", "game"]);
        if is_win || ts.eat_phrase(&["lose", "the", "game"]) {
            let also_other = if is_win {
                ts.eat_phrase(&["or", "lose", "the", "game"])
            } else {
                ts.eat_phrase(&["or", "win", "the", "game"])
            };
            let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            let mut out = vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::PlayerRestriction {
                    player,
                    restriction: if is_win {
                        crate::v2::ir::PlayerRestrictionKind::CantWinGame
                    } else {
                        crate::v2::ir::PlayerRestrictionKind::CantLoseGame
                    },
                    condition: None,
                }),
                duration: duration.clone(),
            }];
            if also_other {
                out.push(Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::PlayerRestriction {
                        player,
                        restriction: if is_win {
                            crate::v2::ir::PlayerRestrictionKind::CantLoseGame
                        } else {
                            crate::v2::ir::PlayerRestrictionKind::CantWinGame
                        },
                        condition: None,
                    }),
                    duration,
                });
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
            let duration = if ts.eat_phrase(&["during", "that", "player's", "next", "turn"])
                || ts.eat_phrase(&["during", "their", "next", "turn"])
            {
                Duration::UntilEndOfNextTurn
            } else {
                parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn)
            };
            let mut out = vec![Effect::ApplyContinuous {
                effect: Box::new(StaticAbility::RuleRestriction {
                    subject: Some(SubjectRef::Group(Box::new(flt))),
                    rule: RuleRestrictionKind::CantBeCast,
                    condition: None,
                }),
                duration,
            }];
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
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("you"))
            && let Ok(tgt) = try_parse(ts, parse_subject_ref)
        {
            ObjectMutation::ApplyRule(RuleRestrictionKind::CantAttackTarget(Box::new(tgt)))
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
    } else if ts.eat_phrase(&["be", "regenerated"]) {
        ObjectMutation::CantBeRegenerated
    } else if ts.eat_phrase(&["be", "countered"]) {
        // Optional `by <flt>` (`Autumn's Veil`: `can't be countered by blue or black spells this turn`)
        if ts.eat_word("by") {
            let _ = try_parse(ts, crate::v2::filter::parse_object_filter);
        }
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantBeCountered)
    } else if ts.eat_phrase(&["phase", "out"]) {
        ObjectMutation::ApplyRule(RuleRestrictionKind::CantPhaseOut)
    } else {
        return Err(ts.err("expected combat restriction after `can't`"));
    };
    let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
    let mut out = vec![Effect::MutateObject {
        subject: subject.clone(),
        mutation,
        duration,
    }];
    // Optional `and has base power and toughness P/T [dur]` (`Atomic Microsizer`)
    {
        let hcp = ts.checkpoint();
        if (ts.eat_phrase(&["and", "has", "base", "power", "and", "toughness"])
            || ts.eat_phrase(&["and", "have", "base", "power", "and", "toughness"]))
            && let Some((power, toughness)) = ts.next_base_pt()
        {
            let d2 = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
            out.push(Effect::MutateObject {
                subject,
                mutation: ObjectMutation::SetDynamicBasePT {
                    power: ValueExpr::Const(power),
                    toughness: ValueExpr::Const(toughness),
                    add_types: CardTypeMask::EMPTY,
                    keep_existing_types: true,
                },
                duration: d2,
            });
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
    if ts.eat_phrase(&["until", "end", "of", "turn"]) || ts.eat_phrase(&["this", "turn"]) {
        return Some(Duration::UntilEndOfTurn);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_word("until")
            && ts.eat_tilde()
            && ts.eat_phrase(&["leaves", "the", "battlefield"])
        {
            return Some(Duration::UntilSourceLeaves);
        }
        ts.restore(cp);
        if ts.eat_word("until") && ts.eat_tilde() && ts.eat_phrase(&["is", "cast", "from", "exile"])
        {
            return Some(Duration::ForAsLongAsExiled);
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["until", "end", "of", "combat"])
        || ts.eat_phrase(&["at", "end", "of", "combat"])
        || ts.eat_phrase(&["this", "combat"])
        || ts.eat_phrase(&["that", "combat"])
    {
        return Some(Duration::UntilEndOfCombat);
    }
    if ts.eat_phrase(&["until", "your", "next", "end", "step"])
        || ts.eat_phrase(&["until", "the", "next", "end", "step"])
    {
        return Some(Duration::UntilYourNextEndStep);
    }
    if ts.eat_phrase(&["until", "your", "next", "upkeep"])
        || ts.eat_phrase(&["until", "its", "controller's", "next", "untap", "step"])
        || ts.eat_phrase(&["until", "their", "controller's", "next", "untap", "step"])
        || ts.eat_phrase(&["until", "their", "next", "untap", "step"])
    {
        return Some(Duration::UntilYourNextUpkeep);
    }
    if ts.eat_phrase(&["until", "your", "next", "turn"]) {
        return Some(Duration::UntilYourNextTurn);
    }
    if ts.eat_phrase(&["until", "the", "end", "of", "your", "next", "turn"])
        || ts.eat_phrase(&["until", "the", "end", "of", "their", "next", "turn"])
        || ts.eat_phrase(&[
            "until", "the", "end", "of", "that", "player's", "next", "turn",
        ])
        || ts.eat_phrase(&[
            "until",
            "the",
            "end",
            "of",
            "its",
            "controller's",
            "next",
            "turn",
        ])
        || ts.eat_phrase(&["during", "that", "player's", "next", "turn"])
        || ts.eat_phrase(&["during", "their", "next", "turn"])
        || ts.eat_phrase(&["during", "its", "controller's", "next", "turn"])
    {
        return Some(Duration::UntilEndOfNextTurn);
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
    if ts.eat_phrase(&["for", "as", "long", "as", "it", "remains", "exiled"])
        || ts.eat_phrase(&["for", "as", "long", "as", "they", "remain", "exiled"])
    {
        return Some(Duration::ForAsLongAsExiled);
    }
    {
        let uecp = ts.checkpoint();
        if ts.eat_phrase(&["until", "you", "exile", "another", "card", "with"]) && ts.eat_tilde() {
            return Some(Duration::ForAsLongAsExiled);
        }
        ts.restore(uecp);
    }
    if ts.eat_phrase(&["for", "as", "long", "as"]) || ts.eat_phrase(&["as", "long", "as"]) {
        let cond = parse_condition(ts).ok()?;
        return Some(Duration::ForAsLongAs(Box::new(cond)));
    }
    if ts.eat_phrase(&["this", "turn"]) {
        return Some(Duration::UntilEndOfTurn);
    }
    None
}

/// Extended subject-headed predicates delegated from `subject_verbs.rs`.
pub(super) fn parse_extra_subject_predicates(
    ts: &mut TokenStream<'_>,
    subject: SubjectRef,
) -> PResult<Vec<Effect>> {
    // 1. `<subj> assigns/assign combat damage equal to its/their toughness rather than its/their power [dur]`
    if ts.eat_phrase(&[
        "assigns",
        "combat",
        "damage",
        "equal",
        "to",
        "its",
        "toughness",
        "rather",
        "than",
        "its",
        "power",
    ]) || ts.eat_phrase(&[
        "assign",
        "combat",
        "damage",
        "equal",
        "to",
        "their",
        "toughness",
        "rather",
        "than",
        "their",
        "power",
    ]) {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::SubjectRule {
                subject,
                rule: crate::v2::ir::SubjectStaticRule::AssignDamageByToughness,
                condition: None,
            }),
            duration,
        }]);
    }

    // 2. `<subj> attacks/attack each combat if able` / `during its/their controller's next combat phase if able` /
    //    `a player other than you if able` / `or blocks [this turn/combat] if able` / `[<tgt>] if able`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("attacks") || ts.eat_word("attack") {
            if ts.eat_phrase(&["each", "combat", "if", "able"]) {
                return Ok(vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(
                        crate::v2::ir::ObjectStateAction::MustAttackEachCombat,
                    ),
                    duration: Duration::UntilYourNextTurn,
                }]);
            }
            if ts.eat_phrase(&[
                "during",
                "its",
                "controller's",
                "next",
                "combat",
                "phase",
                "if",
                "able",
            ]) || ts.eat_phrase(&[
                "during",
                "their",
                "controller's",
                "next",
                "combat",
                "phase",
                "if",
                "able",
            ]) {
                return Ok(vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(
                        crate::v2::ir::ObjectStateAction::MustAttackEachCombat,
                    ),
                    duration: Duration::UntilEndOfNextTurn,
                }]);
            }
            if ts.eat_phrase(&["a", "player", "other", "than", "you", "if", "able"]) {
                return Ok(vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(
                        crate::v2::ir::ObjectStateAction::MustAttackEachCombat,
                    ),
                    duration: Duration::UntilYourNextTurn,
                }]);
            }
            if (ts.eat_phrase(&["or", "blocks"]) || ts.eat_phrase(&["or", "block"]))
                && (ts.eat_phrase(&["this", "combat", "if", "able"])
                    || ts.eat_phrase(&["this", "turn", "if", "able"])
                    || ts.eat_phrase(&["if", "able"]))
            {
                return Ok(vec![
                    Effect::MutateObject {
                        subject: subject.clone(),
                        mutation: ObjectMutation::StateAction(
                            crate::v2::ir::ObjectStateAction::MustAttackEachCombat,
                        ),
                        duration: Duration::UntilEndOfTurn,
                    },
                    Effect::MutateObject {
                        subject,
                        mutation: ObjectMutation::MustBlockThisTurn(None),
                        duration: Duration::UntilEndOfTurn,
                    },
                ]);
            }
            // Bare `attacks [<subj>] if able` (e.g. inside `During target player's next turn, ...`)
            if ts.eat_phrase(&["if", "able"]) {
                return Ok(vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(
                        crate::v2::ir::ObjectStateAction::MustAttackEachCombat,
                    ),
                    duration: Duration::UntilEndOfTurn,
                }]);
            }
            if try_parse(ts, parse_subject_ref).is_ok() && ts.eat_phrase(&["if", "able"]) {
                return Ok(vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(
                        crate::v2::ir::ObjectStateAction::MustAttackEachCombat,
                    ),
                    duration: Duration::UntilEndOfTurn,
                }]);
            }
        }
        ts.restore(cp);
    }

    // 3. `<subj> isn't/aren't [a/an] <card_type> [dur]` (`RemoveTypes`)
    {
        let cp = ts.checkpoint();
        if ts.eat_word("isn't") || ts.eat_word("aren't") {
            let _ = ts.eat_word("a") || ts.eat_word("an");
            if let Some(w) = ts.peek_word()
                && let Some(ct) = crate::vocab::lookup_card_type(&w.to_ascii_lowercase())
            {
                ts.next_word();
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
                return Ok(vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::SubjectRule {
                        subject,
                        rule: crate::v2::ir::SubjectStaticRule::RemoveTypes(ct),
                        condition: None,
                    }),
                    duration,
                }]);
            }
        }
        ts.restore(cp);
    }

    // 4. `<subj> loses/lose all other card types and abilities [dur]`
    if ts.eat_phrase(&["loses", "all", "other", "card", "types", "and", "abilities"])
        || ts.eat_phrase(&["lose", "all", "other", "card", "types", "and", "abilities"])
    {
        let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
        return Ok(vec![
            Effect::MutateObject {
                subject: subject.clone(),
                mutation: ObjectMutation::BecomeCreatureTypes {
                    subtypes: Vec::new(),
                    add_types: CardTypeMask::EMPTY,
                    keep_existing_types: false,
                    base_pt: None,
                    keywords: Vec::new(),
                },
                duration: duration.clone(),
            },
            Effect::MutateObject {
                subject,
                mutation: ObjectMutation::RemoveAllAbilities,
                duration,
            },
        ]);
    }

    // 5. `<subj> is/are a/an <P>/<T> [<colors>] <Subtypes/Types> [with <kws>] [in addition to its/their other [colors and] types] [dur]`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("is") || ts.eat_word("are") {
            let _ = ts.eat_word("a") || ts.eat_word("an");
            if let Some(base_pt) = ts.next_base_pt() {
                let mut add_types = CardTypeMask::EMPTY;
                let mut colors = crate::ast::ColorMask::EMPTY;
                let mut subtypes = Vec::new();
                while let Some(w) = ts.peek_word() {
                    let low = w.to_ascii_lowercase();
                    if let Some(ct) = crate::vocab::lookup_card_type(&low) {
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
                let mut keywords = Vec::new();
                if ts.eat_word("with")
                    && let Ok((kws, _)) = try_parse(ts, crate::v2::filter::parse_keyword_list)
                {
                    keywords = kws;
                }
                let keep_existing_types = ts.eat_phrase(&[
                    "in", "addition", "to", "its", "other", "colors", "and", "types",
                ]) || ts.eat_phrase(&[
                    "in", "addition", "to", "their", "other", "colors", "and", "types",
                ]) || ts
                    .eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
                let duration = parse_trailing_duration(ts).unwrap_or(Duration::Permanent);
                let mut out = Vec::new();
                if !colors.is_empty() {
                    out.push(Effect::MutateObject {
                        subject: subject.clone(),
                        mutation: ObjectMutation::BecomeColors {
                            colors,
                            keep_existing_colors: keep_existing_types,
                        },
                        duration: duration.clone(),
                    });
                }
                out.push(Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::BecomeCreatureTypes {
                        subtypes,
                        add_types,
                        keep_existing_types,
                        base_pt: Some(base_pt),
                        keywords,
                    },
                    duration,
                });
                return Ok(out);
            }
        }
        ts.restore(cp);
    }

    // 6. Extended `<subj> doesn't/don't untap [during <owner> next [<N>] untap step(s)]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["doesn't", "untap"]) || ts.eat_phrase(&["don't", "untap"]) {
            if ts.eat_word("during") {
                let owner_ok = ts.eat_phrase(&["their", "next"])
                    || ts.eat_phrase(&["the", "player's", "next"])
                    || ts.eat_phrase(&["target", "player's", "next"])
                    || ts.eat_phrase(&["target", "opponent's", "next"])
                    || ts.eat_phrase(&["its", "controller's", "next"])
                    || ts.eat_phrase(&["their", "controller's", "next"])
                    || ts.eat_phrase(&["their", "controllers'", "next"])
                    || ts.eat_phrase(&["that", "player's", "next"])
                    || ts.eat_phrase(&["your", "next"]);
                if owner_ok {
                    let _ = ts.next_number(false);
                    if ts.eat_phrase(&["untap", "step"]) || ts.eat_phrase(&["untap", "steps"]) {
                        let duration =
                            parse_trailing_duration(ts).unwrap_or(Duration::UntilYourNextTurn);
                        return Ok(vec![Effect::MutateObject {
                            subject,
                            mutation: ObjectMutation::StateAction(
                                crate::v2::ir::ObjectStateAction::DoesntUntapNextUntap,
                            ),
                            duration,
                        }]);
                    }
                }
            } else if ts.is_eof() {
                return Ok(vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(
                        crate::v2::ir::ObjectStateAction::DoesntUntapNextUntap,
                    ),
                    duration: Duration::UntilYourNextTurn,
                }]);
            }
        }
        ts.restore(cp);
    }

    Err(ts.err("expected verb predicate after subject"))
}
