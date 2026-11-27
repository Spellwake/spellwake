//! Activation/cast restriction, additional-combat, and temporary-static clause helpers for `v2`.

use crate::ast::{CardTypeMask, ColorMask, Rounding};
use crate::v2::clause::find_phrase_index;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_object_filter, parse_zone_ref};
use crate::v2::ir::{
    ActivationRestriction, ChoiceKind, CombatStepKind, CompiledFaceV2, CountSpec, CounterKind,
    Duration, Effect, EntryModifiers, LibraryPosition, ObjectFilter, ObjectMutation, ParseStage,
    PlayerActionKind, PlayerMutation, PlayerRef, Replacement, RuleRestrictionKind, StaticAbility,
    SubjectRef, ValueExpr, ZoneDest, ZoneKind, ZoneOpKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::static_ability::parse_static_ability;
use crate::vocab::{lookup_card_type, lookup_color};

pub(super) fn parse_additional_combat(ts: &mut TokenStream<'_>) -> PResult<Effect> {
    let _ = ts.eat_any_phrase(&[
        &["after", "this", "phase"],
        &["after", "this", "main", "phase"],
        &["after", "this", "combat", "phase"],
    ]);
    let _ = ts.eat_comma();
    if ts.eat_phrase(&["there", "are"]) {
        let cp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_phrase(&["additional", "combat", "phases"])
        {
            let _ = ts.eat_phrase(&["after", "this", "phase"]);
            return Ok(Effect::ForEach {
                over: crate::v2::ir::ForEachDomain::Count(crate::v2::ir::ValueExpr::Const(n)),
                body: vec![mutate_player(
                    PlayerRef::You,
                    PlayerMutation::AdditionalCombat {
                        followed_by_main: false,
                    },
                    Duration::UntilEndOfTurn,
                )],
            });
        }
        ts.restore(cp);
    }
    if ts.eat_any_phrase(&[
        &["there", "is", "an", "additional", "combat", "phase"],
        &["there's", "an", "additional", "combat", "phase"],
    ]) {
        let _ = ts.eat_phrase(&["after", "this", "phase"]);
        let followed_by_main =
            ts.eat_phrase(&["followed", "by", "an", "additional", "main", "phase"]);
        return Ok(mutate_player(
            PlayerRef::You,
            PlayerMutation::AdditionalCombat { followed_by_main },
            Duration::UntilEndOfTurn,
        ));
    }
    Err(ts.err("expected additional combat phase clause"))
}

/// Parse extended mana-spend purposes inside `Spend this mana only to ...` and
/// `This mana can't be spent to ...`.
pub(super) fn try_parse_extra_mana_spend_purpose(
    ts: &mut TokenStream<'_>,
) -> Option<crate::v2::ir::ManaSpendPurpose> {
    use crate::v2::ir::ManaSpendPurpose;
    let cp = ts.checkpoint();
    if ts.eat_any_phrase(&[&["foretell", "cards"], &["foretell", "a", "card"]]) {
        let _ = ts.eat_phrase(&["from", "your", "hand"]);
        return Some(ManaSpendPurpose::Foretell);
    }
    const ACTIVATE_PURPOSES: &[&[&str]] = &[
        &["pay", "cumulative", "upkeep", "costs"],
        &["pay", "a", "disturb", "cost"],
        &["pay", "a", "morph", "cost"],
        &["pay", "generic", "mana", "costs"],
        &[
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
        ],
        &["turn", "permanents", "face", "up"],
        &["turn", "a", "permanent", "face", "up"],
        &["activate", "power-up", "abilities"],
        &["activate", "equip", "abilities"],
        &["activate", "an", "equip", "ability"],
    ];
    if ts.eat_any_phrase(ACTIVATE_PURPOSES) {
        return Some(ManaSpendPurpose::ActivateAbilities(None));
    }
    if ts.eat_phrase(&["cast", "the", "last", "card", "exiled", "with"]) && ts.eat_tilde() {
        return Some(ManaSpendPurpose::CastFromZone(
            crate::v2::ir::ZoneKind::Exile,
        ));
    }
    ts.restore(cp);
    // `cast a Time Lord or Alien spell` / `activate an ability of a Time Lord or Alien`
    let time_lord_alien = |any_spell: bool| {
        let mut flt = ObjectFilter {
            any_spell,
            subtypes_match_any: true,
            ..Default::default()
        };
        flt.subtypes.push("Time Lord".to_string());
        flt.subtypes.push("Alien".to_string());
        flt
    };
    if ts.eat_phrase(&["cast", "a", "time", "lord", "or", "alien", "spell"]) {
        return Some(ManaSpendPurpose::CastSpells(Some(time_lord_alien(true))));
    }
    if ts.eat_any_phrase(&[
        &[
            "activate", "an", "ability", "of", "a", "time", "lord", "or", "alien",
        ],
        &[
            "activate",
            "abilities",
            "of",
            "a",
            "time",
            "lord",
            "or",
            "alien",
        ],
    ]) {
        return Some(ManaSpendPurpose::ActivateAbilities(Some(time_lord_alien(
            false,
        ))));
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
        return Ok(apply_continuous(
            StaticAbility::RuleRestriction {
                subject: Some(SubjectRef::Compound(vec![
                    SubjectRef::Player(PlayerRef::EachPlayer),
                    SubjectRef::All(Box::new(ObjectFilter {
                        any_permanent: true,
                        ..Default::default()
                    })),
                ])),
                rule: RuleRestrictionKind::CantBeTargetedBy(Box::new(src)),
                condition: None,
            },
            Duration::UntilEndOfTurn,
        ));
    }
    // `The next enchantment spell you cast this turn can't be countered`
    if ts.peek_phrase(&["the", "next"]) {
        return Ok(apply_continuous(
            parse_static_ability(ts)?,
            Duration::UntilEndOfTurn,
        ));
    }
    Err(ts.err("not a temporary static clause"))
}

pub(super) fn parse_activation_restrictions(
    ts: &mut TokenStream<'_>,
) -> PResult<Vec<ActivationRestriction>> {
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
                CombatStepKind::DeclareAttackers,
            ));
        } else if ts.eat_phrase(&["during", "the", "declare", "blockers", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                CombatStepKind::DeclareBlockers,
            ));
        } else if ts.eat_phrase(&["during", "the", "combat", "damage", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                CombatStepKind::CombatDamage,
            ));
        } else if ts.eat_phrase(&["during", "the", "end", "of", "combat", "step"]) {
            out.push(ActivationRestriction::DuringEndOfCombat);
        } else if ts.eat_phrase(&["during", "combat", "before", "blockers", "are", "declared"]) {
            out.push(ActivationRestriction::DuringCombat);
            out.push(ActivationRestriction::DuringCombatStep(
                CombatStepKind::DeclareAttackers,
            ));
        } else if ts.eat_phrase(&["during", "combat", "after", "blockers", "are", "declared"]) {
            out.push(ActivationRestriction::DuringCombat);
            out.push(ActivationRestriction::DuringCombatStep(
                CombatStepKind::DeclareBlockers,
            ));
        } else if ts.eat_phrase(&["before", "the", "combat", "damage", "step"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                CombatStepKind::DeclareBlockers,
            ));
        } else if ts.eat_phrase(&["after", "combat"]) {
            out.push(ActivationRestriction::OnlyIf(Box::new(
                crate::v2::ir::ConditionExpr::EventCheck(
                    crate::v2::ir::EventCheckKind::DuringStep(
                        crate::ast::TurnStepAst::PostcombatMain,
                    ),
                ),
            )));
        } else if ts.eat_any_phrase(&[
            &["before", "the", "end", "of", "combat", "step"],
            &["before", "combat"],
            &["before", "attackers", "are", "declared"],
        ]) {
            out.push(ActivationRestriction::BeforeAttackers);
        } else if ts.eat_phrase(&["during", "combat"]) {
            out.push(ActivationRestriction::DuringCombat);
        } else if ts.eat_phrase(&["before", "blockers", "are", "declared"]) {
            out.push(ActivationRestriction::DuringCombatStep(
                CombatStepKind::DeclareAttackers,
            ));
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
                let cond = parse_condition(&mut cond_ts)?;
                cond_ts.expect_eof()?;
                ts.commit_substream_eof(ao_idx, &cond_ts);
                out.push(ActivationRestriction::OnlyIf(Box::new(cond)));
            } else {
                let mut cond = parse_condition(ts)?;
                if ts.eat_phrase(&["or", "if"]) {
                    let c2 = parse_condition(ts)?;
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

    // 1. `This ability ...` (triggers only once/if, costs {M} less/more if)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["this", "ability"]) {
            if ts.eat_phrase(&["triggers", "only", "once"]) && ts.is_eof() {
                return Ok(vec![Effect::Restriction(vec![ActivationRestriction::Once])]);
            }
            if ts.eat_phrase(&["triggers", "only", "if"])
                && let Ok(mut cond) = try_parse(ts, parse_condition)
            {
                if ts.eat_phrase(&["or", "if"])
                    && let Ok(c2) = try_parse(ts, parse_condition)
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
                    && let Ok(cond) = try_parse(ts, parse_condition)
                    && ts.is_eof()
                {
                    return Ok(vec![apply_continuous(
                        StaticAbility::ActivationCostModifier {
                            delta,
                            less,
                            scale: None,
                            condition: Some(cond),
                        },
                        Duration::Permanent,
                    )]);
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
                    CombatStepKind::DeclareAttackers,
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
        return Ok(vec![mutate_player(
            PlayerRef::You,
            PlayerMutation::ManaRule(crate::v2::ir::ManaRuleKind::SpendOnlyColorOnX(
                crate::ast::ColorMask::EMPTY,
            )),
            Duration::Permanent,
        )]);
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
        return Ok(vec![apply_continuous(
            StaticAbility::GlobalFlag(crate::v2::ir::StaticFlagKind::CostReductionRule),
            Duration::Permanent,
        )]);
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
                return Ok(vec![apply_continuous(
                    StaticAbility::AdditionalCost(costs),
                    Duration::Instant,
                )]);
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

// ---------------------------------------------------------------------------
// Shared clause-submodule IR constructors and token helpers
// ---------------------------------------------------------------------------

pub(in crate::v2) fn simple_zone_op(
    actor: PlayerRef,
    op: ZoneOpKind,
    subject: SubjectRef,
    from: Vec<ZoneRef>,
    to: Option<ZoneDest>,
) -> Effect {
    counted_zone_op(actor, op, subject, from, to, None)
}

pub(in crate::v2) fn counted_zone_op(
    actor: PlayerRef,
    op: ZoneOpKind,
    subject: SubjectRef,
    from: Vec<ZoneRef>,
    to: Option<ZoneDest>,
    count: Option<CountSpec>,
) -> Effect {
    Effect::ZoneOp {
        actor,
        op,
        subject,
        from,
        to,
        modifiers: EntryModifiers::default(),
        except: None,
        count,
    }
}

pub(in crate::v2) fn move_zone_op(
    actor: PlayerRef,
    subject: SubjectRef,
    from: Vec<ZoneRef>,
    to: ZoneDest,
    modifiers: EntryModifiers,
    count: Option<CountSpec>,
) -> Effect {
    Effect::ZoneOp {
        actor,
        op: ZoneOpKind::Move,
        subject,
        from,
        to: Some(to),
        modifiers,
        except: None,
        count,
    }
}

pub(in crate::v2) fn exile_zone_op(
    actor: PlayerRef,
    subject: SubjectRef,
    from: Vec<ZoneRef>,
    face_down: bool,
    counters: Option<(CounterKind, ValueExpr)>,
    count: Option<CountSpec>,
) -> Effect {
    Effect::ZoneOp {
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
        count,
    }
}

pub(in crate::v2) fn exact_count(count: ValueExpr) -> CountSpec {
    CountSpec {
        count,
        up_to: false,
        any_number: false,
    }
}

pub(in crate::v2) fn simple_zone_dest(zone: ZoneKind, owner: ZoneOwner) -> ZoneDest {
    ZoneDest {
        zone,
        owner,
        library_position: None,
        library_order: None,
        under_control: None,
    }
}

pub(in crate::v2) fn shuffled_zone_dest(zone: ZoneKind, owner: ZoneOwner) -> ZoneDest {
    ZoneDest {
        zone,
        owner,
        library_position: Some(LibraryPosition::Shuffled),
        library_order: None,
        under_control: None,
    }
}

pub(in crate::v2) fn zone_cards_subject(zref: ZoneRef) -> SubjectRef {
    SubjectRef::Group(Box::new(ObjectFilter {
        any_card: true,
        zones: vec![zref],
        ..Default::default()
    }))
}

pub(in crate::v2) fn all_zone_cards_subject(zref: ZoneRef) -> SubjectRef {
    SubjectRef::All(Box::new(ObjectFilter {
        any_card: true,
        zones: vec![zref],
        ..Default::default()
    }))
}

pub(in crate::v2) fn mutate_obj(
    subject: SubjectRef,
    mutation: ObjectMutation,
    duration: Duration,
) -> Effect {
    Effect::MutateObject {
        subject,
        mutation,
        duration,
    }
}

pub(in crate::v2) fn mutate_player(
    player: PlayerRef,
    mutation: PlayerMutation,
    duration: Duration,
) -> Effect {
    Effect::MutatePlayer {
        player,
        mutation,
        duration,
    }
}

pub(in crate::v2) fn player_action(
    player: PlayerRef,
    action: PlayerActionKind,
    duration: Duration,
) -> Effect {
    mutate_player(player, PlayerMutation::Action(action), duration)
}

pub(in crate::v2) fn prompt_choice(chooser: PlayerRef, kind: ChoiceKind) -> Effect {
    Effect::PromptChoice { chooser, kind }
}

pub(in crate::v2) fn apply_continuous(effect: StaticAbility, duration: Duration) -> Effect {
    Effect::ApplyContinuous {
        effect: Box::new(effect),
        duration,
    }
}

pub(in crate::v2) fn apply_replacement(replacement: Replacement, duration: Duration) -> Effect {
    Effect::ApplyReplacement {
        replacement: Box::new(replacement),
        duration,
    }
}

pub(in crate::v2) fn compile_quoted_face(
    ts: &TokenStream<'_>,
    name: &str,
    type_line: &str,
    oracle_text: &str,
) -> PResult<CompiledFaceV2> {
    let compiled = crate::v2::card::compile_face_v2(
        crate::FaceInput {
            name,
            type_line,
            oracle_text,
        },
        ts.vocab,
    );
    if compiled.is_ok() {
        Ok(compiled)
    } else {
        Err(compiled
            .errors
            .into_iter()
            .next()
            .unwrap_or_else(|| ts.err("failed to compile quoted ability text")))
    }
}

pub(in crate::v2) fn compile_quoted_permanent(
    ts: &TokenStream<'_>,
    quoted: &str,
) -> PResult<CompiledFaceV2> {
    compile_quoted_face(ts, "~", "Permanent", quoted)
}

pub(in crate::v2) fn eat_rounding(ts: &mut TokenStream<'_>, default: Rounding) -> Rounding {
    match ts.eat_rounded_modifier() {
        Some(true) => Rounding::Up,
        Some(false) => Rounding::Down,
        None => default,
    }
}

pub(in crate::v2) fn eat_type_color_subtype_run(
    ts: &mut TokenStream<'_>,
    allow_colors: bool,
    allow_colorless: bool,
    allow_and: bool,
) -> (CardTypeMask, ColorMask, Vec<String>) {
    let mut add_types = CardTypeMask::EMPTY;
    let mut colors = ColorMask::EMPTY;
    let mut subtypes = Vec::new();
    while let Some(w) = ts.peek_word() {
        let low = w.to_ascii_lowercase();
        if let Some(ct) = lookup_card_type(&low) {
            ts.next_word();
            add_types = add_types.union(ct);
        } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
            ts.next_word();
            subtypes.push(sub.to_string());
        } else if allow_colors && let Some(c) = lookup_color(&low) {
            ts.next_word();
            colors = colors.union(c);
        } else if (allow_colorless && low == "colorless")
            || (allow_and
                && low == "and"
                && ts.peek_n(1).is_some_and(|t| {
                    matches!(t.kind, TokenKind::Word(nw) if {
                        let nl = nw.to_ascii_lowercase();
                        (allow_colors && lookup_color(&nl).is_some())
                            || lookup_card_type(&nl).is_some()
                            || ts.vocab.lookup_subtype(&nl).is_some()
                    })
                }))
        {
            ts.next_word();
        } else {
            break;
        }
    }
    (add_types, colors, subtypes)
}

pub(in crate::v2) fn parse_from_zone_list(ts: &mut TokenStream<'_>) -> PResult<Vec<ZoneRef>> {
    let mut from = Vec::new();
    if ts.eat_word("from") {
        let z1 = parse_zone_ref(ts)?;
        from.push(z1);
        loop {
            let zcp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_conj = ts.eat_any_word(&["and/or", "or", "and"]);
            if had_comma || had_conj {
                let _ = ts.eat_word("from");
                if let Ok(mut z2) = try_parse(ts, parse_zone_ref) {
                    if z2.owner == ZoneOwner::Default
                        && matches!(
                            z2.zone,
                            ZoneKind::Graveyard | ZoneKind::Hand | ZoneKind::Library
                        )
                    {
                        z2.owner = z1.owner;
                    }
                    from.push(z2);
                    continue;
                }
            }
            ts.restore(zcp);
            break;
        }
    }
    Ok(from)
}
