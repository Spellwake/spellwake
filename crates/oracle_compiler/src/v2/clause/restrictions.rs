//! Activation/cast restriction, additional-combat, and temporary-static clause helpers for `v2`.

use crate::v2::clause::find_phrase_index;
use crate::v2::condition::parse_condition;
use crate::v2::filter::parse_object_filter;
use crate::v2::ir::{
    ActivationRestriction, Duration, Effect, ObjectFilter, ParseStage, PlayerMutation, PlayerRef,
    RuleRestrictionKind, StaticAbility, SubjectRef,
};
use crate::v2::parser::{PResult, TokenStream};
use crate::v2::static_ability::parse_static_ability;

pub(super) fn parse_additional_combat(ts: &mut TokenStream<'_>) -> PResult<Effect> {
    let _ = ts.eat_phrase(&["after", "this", "phase"])
        || ts.eat_phrase(&["after", "this", "main", "phase"])
        || ts.eat_phrase(&["after", "this", "combat", "phase"]);
    let _ = ts.eat_comma();
    if ts.eat_phrase(&["there", "is", "an", "additional", "beginning", "phase"])
        || ts.eat_phrase(&["there's", "an", "additional", "beginning", "phase"])
    {
        let _ = ts.eat_phrase(&["after", "this", "phase"]);
        return Ok(Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::Action(crate::v2::ir::PlayerActionKind::ExtraTurn),
            duration: Duration::UntilEndOfTurn,
        });
    }
    if ts.eat_phrase(&["there", "are"]) {
        let cp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["additional", "combat", "phases"])
        {
            let _ = ts.eat_phrase(&["after", "this", "phase"]);
            return Ok(Effect::ForEach {
                over: crate::v2::ir::ForEachDomain::Count(crate::v2::ir::ValueExpr::Const(n)),
                body: vec![Effect::MutatePlayer {
                    player: PlayerRef::You,
                    mutation: PlayerMutation::AdditionalCombat {
                        followed_by_main: false,
                    },
                    duration: Duration::UntilEndOfTurn,
                }],
            });
        }
        ts.restore(cp);
    }
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

/// Parse extended mana-spend purposes inside `Spend this mana only to ...` and
/// `This mana can't be spent to ...` (`Adarkar Unicorn`, `Overgrown Zealot`,
/// `Unblinking Observer`, `Niko Defies Destiny`, `Quinjet Technician`, `Ice Cauldron`,
/// `Gallifrey Council Chamber`, `Jegantha, the Wellspring`, `Qarsi Deceiver`).
pub(super) fn try_parse_extra_mana_spend_purpose(
    ts: &mut TokenStream<'_>,
) -> Option<crate::v2::ir::ManaSpendPurpose> {
    use crate::v2::ir::ManaSpendPurpose;
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["foretell", "cards"]) || ts.eat_phrase(&["foretell", "a", "card"]) {
        let _ = ts.eat_phrase(&["from", "your", "hand"]);
        return Some(ManaSpendPurpose::Foretell);
    }
    if ts.eat_phrase(&["pay", "cumulative", "upkeep", "costs"])
        || ts.eat_phrase(&["pay", "a", "disturb", "cost"])
        || ts.eat_phrase(&["pay", "a", "morph", "cost"])
        || ts.eat_phrase(&["pay", "generic", "mana", "costs"])
        || ts.eat_phrase(&[
            "pay",
            "a",
            "mana",
            "cost",
            "to",
            "turn",
            "a",
            "manifested",
            "creature",
            "face",
            "up",
        ])
        || ts.eat_phrase(&["turn", "permanents", "face", "up"])
        || ts.eat_phrase(&["turn", "a", "permanent", "face", "up"])
        || ts.eat_phrase(&["activate", "power-up", "abilities"])
        || ts.eat_phrase(&["activate", "equip", "abilities"])
        || ts.eat_phrase(&["activate", "an", "equip", "ability"])
    {
        return Some(ManaSpendPurpose::ActivateAbilities(None));
    }
    if ts.eat_phrase(&["cast", "the", "last", "card", "exiled", "with"]) && ts.eat_tilde() {
        return Some(ManaSpendPurpose::CastFromZone(
            crate::v2::ir::ZoneKind::Exile,
        ));
    }
    ts.restore(cp);
    // `cast a Time Lord or Alien spell` / `activate an ability of a Time Lord or Alien`
    if ts.eat_phrase(&["cast", "a", "time", "lord", "or", "alien", "spell"]) {
        let mut flt = ObjectFilter {
            any_spell: true,
            subtypes_match_any: true,
            ..Default::default()
        };
        flt.subtypes.push("Time Lord".to_string());
        flt.subtypes.push("Alien".to_string());
        return Some(ManaSpendPurpose::CastSpells(Some(flt)));
    }
    if ts.eat_phrase(&[
        "activate", "an", "ability", "of", "a", "time", "lord", "or", "alien",
    ]) || ts.eat_phrase(&[
        "activate",
        "abilities",
        "of",
        "a",
        "time",
        "lord",
        "or",
        "alien",
    ]) {
        let mut flt = ObjectFilter {
            subtypes_match_any: true,
            ..Default::default()
        };
        flt.subtypes.push("Time Lord".to_string());
        flt.subtypes.push("Alien".to_string());
        return Some(ManaSpendPurpose::ActivateAbilities(Some(flt)));
    }
    ts.restore(cp);
    None
}

pub(super) fn parse_temporary_static_clause(ts: &mut TokenStream<'_>) -> PResult<Effect> {
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

pub(super) fn parse_activation_restrictions(
    ts: &mut TokenStream<'_>,
) -> PResult<Vec<ActivationRestriction>> {
    // `Activate no more than [twice | <N> times] each turn` / `Activate no more times each turn than <val>`
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["activate", "no", "more", "than"])
            && (ts.eat_phrase(&["twice", "each", "turn"])
                || (ts.next_number(false).is_some() && ts.eat_phrase(&["times", "each", "turn"])))
        {
            return Ok(vec![ActivationRestriction::OnceEachTurn]);
        }
        ts.restore(ncp);
        if ts.eat_phrase(&["activate", "no", "more", "times", "each", "turn", "than"])
            && crate::v2::parser::try_parse(ts, crate::v2::value::parse_value_expr).is_ok()
        {
            return Ok(vec![ActivationRestriction::OnceEachTurn]);
        }
        ts.restore(ncp);
    }
    let valid_head = ts.eat_phrase(&["activate", "only"])
        || ts.eat_phrase(&["cast", "this", "spell", "only"])
        || {
            let cp = ts.checkpoint();
            if ts.eat_word("cast") && ts.eat_tilde() && ts.eat_word("only") {
                true
            } else {
                ts.restore(cp);
                false
            }
        };
    if !valid_head {
        return Err(ts.err("expected `Activate only` or `Cast ... only`"));
    }
    let mut out = Vec::new();
    loop {
        if ts.eat_phrase(&["as", "a", "sorcery"]) {
            out.push(ActivationRestriction::AsSorcery);
        } else if ts.eat_phrase(&["as", "an", "instant"]) {
            out.push(ActivationRestriction::AsInstant);
        } else if ts.eat_phrase(&["during", "your", "upkeep"]) {
            out.push(ActivationRestriction::DuringYourUpkeep);
        } else if ts.eat_phrase(&["during", "any", "upkeep", "step"])
            || ts.eat_phrase(&["during", "any", "upkeep"])
        {
            out.push(ActivationRestriction::OnlyIf(Box::new(
                crate::v2::ir::ConditionExpr::EventCheck(
                    crate::v2::ir::EventCheckKind::DuringStep(crate::ast::TurnStepAst::Upkeep),
                ),
            )));
        } else if ts.eat_phrase(&["during", "an", "opponent's", "upkeep"])
            || ts.eat_phrase(&["during", "each", "opponent's", "upkeep"])
        {
            out.push(ActivationRestriction::OnlyIf(Box::new(
                crate::v2::ir::ConditionExpr::And(
                    Box::new(crate::v2::ir::ConditionExpr::Not(Box::new(
                        crate::v2::ir::ConditionExpr::Flag(
                            crate::v2::ir::ConditionFlag::IsYourTurn,
                        ),
                    ))),
                    Box::new(crate::v2::ir::ConditionExpr::EventCheck(
                        crate::v2::ir::EventCheckKind::DuringStep(crate::ast::TurnStepAst::Upkeep),
                    )),
                ),
            )));
        } else if ts.eat_phrase(&["during", "your", "turn"])
            || ts.eat_phrase(&["on", "your", "turn"])
        {
            out.push(ActivationRestriction::DuringYourTurn);
            if ts.eat_comma() && ts.eat_phrase(&["before", "attackers", "are", "declared"]) {
                out.push(ActivationRestriction::BeforeAttackers);
            }
        } else if ts.eat_phrase(&["during", "an", "opponent's", "turn"])
            || ts.eat_phrase(&["on", "an", "opponent's", "turn"])
            || ts.eat_phrase(&["during", "opponents'", "turns"])
        {
            out.push(ActivationRestriction::OnlyIf(Box::new(
                crate::v2::ir::ConditionExpr::Not(Box::new(crate::v2::ir::ConditionExpr::Flag(
                    crate::v2::ir::ConditionFlag::IsYourTurn,
                ))),
            )));
        } else if ts.eat_phrase(&["during", "combat", "on", "an", "opponent's", "turn"])
            || ts.eat_phrase(&["during", "combat", "on", "opponents'", "turns"])
        {
            out.push(ActivationRestriction::DuringCombat);
            out.push(ActivationRestriction::OnlyIf(Box::new(
                crate::v2::ir::ConditionExpr::Not(Box::new(crate::v2::ir::ConditionExpr::Flag(
                    crate::v2::ir::ConditionFlag::IsYourTurn,
                ))),
            )));
        } else if ts.eat_phrase(&["during", "combat", "on", "your", "turn"]) {
            out.push(ActivationRestriction::DuringCombat);
            out.push(ActivationRestriction::DuringYourTurn);
        } else if ts.eat_phrase(&["during", "the", "declare", "attackers", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareAttackers,
            ));
        } else if ts.eat_phrase(&["during", "the", "declare", "blockers", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareBlockers,
            ));
        } else if ts.eat_phrase(&["during", "the", "combat", "damage", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::CombatDamage,
            ));
        } else if ts.eat_phrase(&["during", "the", "end", "of", "combat", "step"]) {
            out.push(ActivationRestriction::DuringEndOfCombat);
        } else if ts.eat_phrase(&["during", "combat", "before", "blockers", "are", "declared"]) {
            out.push(ActivationRestriction::DuringCombat);
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareAttackers,
            ));
        } else if ts.eat_phrase(&["during", "combat", "after", "blockers", "are", "declared"]) {
            out.push(ActivationRestriction::DuringCombat);
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareBlockers,
            ));
        } else if ts.eat_phrase(&["before", "the", "combat", "damage", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareBlockers,
            ));
        } else if ts.eat_phrase(&["after", "combat"]) {
            out.push(ActivationRestriction::OnlyIf(Box::new(
                crate::v2::ir::ConditionExpr::EventCheck(
                    crate::v2::ir::EventCheckKind::DuringStep(
                        crate::ast::TurnStepAst::PostcombatMain,
                    ),
                ),
            )));
        } else if ts.eat_phrase(&["before", "the", "end", "of", "combat", "step"])
            || ts.eat_phrase(&["before", "combat"])
        {
            out.push(ActivationRestriction::BeforeAttackers);
        } else if ts.eat_phrase(&["during", "combat"]) {
            out.push(ActivationRestriction::DuringCombat);
        } else if ts.eat_phrase(&["before", "blockers", "are", "declared"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                crate::v2::ir::CombatStepKind::DeclareAttackers,
            ));
        } else if ts.eat_phrase(&["before", "attackers", "are", "declared"]) {
            out.push(ActivationRestriction::BeforeAttackers);
        } else if ts.eat_phrase(&["once", "each", "turn"]) {
            out.push(ActivationRestriction::OnceEachTurn);
        } else if ts.eat_word("once") {
            out.push(ActivationRestriction::Once);
        } else if ts.eat_phrase(&["if", "you've", "controlled"])
            && ts.eat_tilde()
            && ts.eat_phrase(&[
                "continuously",
                "since",
                "the",
                "beginning",
                "of",
                "your",
                "most",
                "recent",
                "turn",
            ])
        {
            out.push(ActivationRestriction::ControlledContinuously);
        } else if ts.eat_word("if") {
            // Split before trailing `and only ...` (`Activate only if <cond> and only once each turn`)
            if let Some(ao_idx) = find_phrase_index(ts.tokens, ts.pos, &["and", "only"])
                && ao_idx > ts.pos
            {
                let cond_slice = &ts.tokens[ts.pos..ao_idx];
                let mut cond_ts = ts.sub_stream(cond_slice, ParseStage::Effect);
                let cond = parse_condition(&mut cond_ts).or_else(|_| {
                    cond_ts.pos = 0;
                    super::sentence_ext::parse_extra_clause_condition(&mut cond_ts)
                })?;
                cond_ts.expect_eof()?;
                ts.pos = ao_idx;
                out.push(ActivationRestriction::OnlyIf(Box::new(cond)));
            } else {
                let ccp = ts.checkpoint();
                let mut cond = match parse_condition(ts) {
                    Ok(c) => c,
                    Err(_) => {
                        ts.restore(ccp);
                        super::sentence_ext::parse_extra_clause_condition(ts)?
                    }
                };
                if ts.eat_phrase(&["or", "if"]) {
                    let c2_cp = ts.checkpoint();
                    let c2 = match parse_condition(ts) {
                        Ok(c) => c,
                        Err(_) => {
                            ts.restore(c2_cp);
                            super::sentence_ext::parse_extra_clause_condition(ts)?
                        }
                    };
                    cond = crate::v2::ir::ConditionExpr::Or(Box::new(cond), Box::new(c2));
                }
                out.push(ActivationRestriction::OnlyIf(Box::new(cond)));
            }
        } else {
            return Err(ts.err("expected timing restriction after `Activate only`"));
        }
        if ts.eat_phrase(&["and", "only"]) || ts.eat_word("and") {
            continue;
        }
        break;
    }
    Ok(out)
}

/// Parse extended restriction, ability-rule, and additional-cost sentences in effect bodies.
pub(super) fn parse_extra_restriction_or_cost_sentence(
    ts: &mut TokenStream<'_>,
) -> PResult<Vec<Effect>> {
    use crate::v2::parser::try_parse;
    let parse_cond = |s: &mut TokenStream<'_>| {
        let ccp = s.checkpoint();
        parse_condition(s).or_else(|_| {
            s.restore(ccp);
            super::sentence_ext::parse_extra_clause_condition(s)
        })
    };

    // 1. `This ability ...` (triggers only once/if, can't be copied, still resolves, costs {M} less/more if)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["this", "ability"]) {
            if ts.eat_phrase(&["triggers", "only", "once"]) && ts.is_eof() {
                return Ok(vec![Effect::Restriction(vec![ActivationRestriction::Once])]);
            }
            if ts.eat_phrase(&["triggers", "only", "if"])
                && let Ok(mut cond) = parse_cond(ts)
            {
                if ts.eat_phrase(&["or", "if"])
                    && let Ok(c2) = parse_cond(ts)
                {
                    cond = crate::v2::ir::ConditionExpr::Or(Box::new(cond), Box::new(c2));
                }
                if ts.is_eof() {
                    return Ok(vec![Effect::Restriction(vec![
                        ActivationRestriction::OnlyIf(Box::new(cond)),
                    ])]);
                }
            }
            ts.restore(cp);
            if ts.eat_phrase(&["this", "ability", "can't", "be", "copied"]) {
                let mut out = vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::RuleRestriction {
                        subject: None,
                        rule: RuleRestrictionKind::CantBeCountered,
                        condition: None,
                    }),
                    duration: Duration::Instant,
                }];
                if ts.eat_phrase(&["and", "x", "can't", "be"]) && ts.next_number(false) == Some(0) {
                    out.push(Effect::MutatePlayer {
                        player: PlayerRef::You,
                        mutation: PlayerMutation::Action(
                            crate::v2::ir::PlayerActionKind::XCantBeZero,
                        ),
                        duration: Duration::Instant,
                    });
                }
                if ts.is_eof() {
                    return Ok(out);
                }
            }
            ts.restore(cp);
            if ts.eat_phrase(&[
                "this", "ability", "still", "resolves", "if", "its", "target", "becomes", "illegal",
            ]) && ts.is_eof()
            {
                return Ok(vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::GlobalFlag(
                        crate::v2::ir::StaticFlagKind::AppliesInAllZones,
                    )),
                    duration: Duration::Instant,
                }]);
            }
            ts.restore(cp);
            if ts.eat_phrase(&["this", "ability", "costs"])
                && let Some(delta) = ts.eat_mana_cost()
            {
                let less = if ts.eat_phrase(&["less", "to", "activate"]) {
                    Some(true)
                } else if ts.eat_phrase(&["more", "to", "activate"]) {
                    Some(false)
                } else {
                    None
                };
                if let Some(less) = less
                    && ts.eat_word("if")
                    && let Ok(cond) = parse_cond(ts)
                    && ts.is_eof()
                {
                    return Ok(vec![Effect::ApplyContinuous {
                        effect: Box::new(StaticAbility::ActivationCostModifier {
                            delta,
                            less,
                            scale: None,
                            condition: Some(cond),
                        }),
                        duration: Duration::Permanent,
                    }]);
                }
            }
        }
        ts.restore(cp);
    }

    // 2. `Only the player ~ is attacking may activate ...`, `Spend only mana of the chosen color ...`, `For each mana in this ability's activation cost ...`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["only", "the", "player"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["is", "attacking", "may", "activate", "this", "ability"])
        {
            let mut r = vec![ActivationRestriction::AllowedPlayer(
                PlayerRef::DefendingPlayer,
            )];
            if ts.eat_phrase(&[
                "and",
                "only",
                "during",
                "the",
                "declare",
                "attackers",
                "step",
            ]) {
                r.push(ActivationRestriction::DuringCombatStep(
                    crate::v2::ir::CombatStepKind::DeclareAttackers,
                ));
            }
            if ts.is_eof() {
                return Ok(vec![Effect::Restriction(r)]);
            }
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&[
        "spend", "only", "mana", "of", "the", "chosen", "color", "to", "activate", "this",
        "ability",
    ]) && ts.is_eof()
    {
        return Ok(vec![Effect::MutatePlayer {
            player: PlayerRef::You,
            mutation: PlayerMutation::ManaRule(crate::v2::ir::ManaRuleKind::SpendOnlyColorOnX(
                crate::ast::ColorMask::EMPTY,
            )),
            duration: Duration::Permanent,
        }]);
    }
    if ts.eat_phrase(&[
        "for",
        "each",
        "mana",
        "in",
        "this",
        "ability's",
        "activation",
        "cost",
    ]) && ts.eat_comma()
        && ts.eat_phrase(&[
            "you", "may", "tap", "an", "untapped", "creature", "you", "control", "rather", "than",
            "pay", "that", "mana",
        ])
        && ts.is_eof()
    {
        return Ok(vec![Effect::ApplyContinuous {
            effect: Box::new(StaticAbility::GlobalFlag(
                crate::v2::ir::StaticFlagKind::CostReductionRule,
            )),
            duration: Duration::Permanent,
        }]);
    }

    // 3. `As an additional cost to cast ~, <cost_or_effect>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["as", "an", "additional", "cost", "to", "cast"])
            && (ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
            && ts.eat_comma()
        {
            let body_cp = ts.checkpoint();
            let _ = ts.eat_phrase(&["you", "may"]);
            if let Ok(costs) = try_parse(ts, crate::v2::cost::parse_cost_list)
                && ts.is_eof()
            {
                return Ok(vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::AdditionalCost(costs)),
                    duration: Duration::Instant,
                }]);
            }
            ts.restore(body_cp);
            if let Ok(effs) = try_parse(ts, super::parse_sentence_top)
                && ts.is_eof()
            {
                return Ok(effs);
            }
        }
        ts.restore(cp);
    }

    Err(ts.err("not an extended restriction or cost sentence"))
}
