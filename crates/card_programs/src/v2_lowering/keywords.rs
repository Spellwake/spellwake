//! Strictly-typed keyword ability lowering (`v2::KeywordSpec` -> `spellwake-engine::Program`).
//!
//! Zero string parsing, zero regex, zero raw `oracle_text` inspection.
//! Every keyword or keyword parameter not faithfully represented by `spellwake-engine` fails closed
//! with `LoweringError::Unsupported`.

use std::collections::BTreeSet;
use std::num::NonZeroU8;

use spellwake_contracts::names::EnglishCardName;
use spellwake_engine::{
    activated::{
        ActivatedAbility, ActivatedAbilityId, ActivationRestriction, ExtraActivationCost,
        HandDiscardKeyword, PowerAbility, SourceCost,
    },
    attachment::{AttachAbility, AttachmentKind, EnchantRestriction},
    characteristics::{
        AbilityChange, BlockingRestriction, CastPermissionRule, Color, ControllerSelector,
        HexproofQuality, KeywordAbility, PermanentSelector, PrintedStat, ProtectionFromQuality,
        ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, StaticContinuousAbility,
        StaticDamageKind, StaticDamageOp, StaticDamageRecipient, StaticDamageRule,
        StaticModifierRule, StaticPart, StaticParts, StaticSourceCondition, StaticTypeChange,
        SubtypeOperation, SubtypeProtection, TypeOperation,
    },
    effects::CounterKind as EngineCounterKind,
    face_down::FaceUpAbility,
    land::EntryTap,
    mana::{ManaCost, ManaType},
    program::{
        AdditionalCastCost, AffectedPlayers, AttackTriggerMode, BodyTarget, CardType,
        CastCopyKeyword, CastEntryKind, CastReduction, CombatTriggerKeyword, Comparison,
        ConditionalCastCost, ControllerFilter, CounterKeyword, DiesTriggerMode, EffectManaYield,
        EndStepCast, EntryCounterAbility, EventValueKind, ExecutableBody, ExileAction, GiftKind,
        GrowthKeyword, Instruction, LibrarySearch, LookTopKeepDest, LookTopRestDest, MiracleCost,
        MoveSubjectRef, ObjectFilter, ObjectStatKind, PermanentTypeFilter, PlayerRelation, Program,
        ProgramKind, ResolutionCondition, ResolvingPlayer, SearchBattlefieldPostEntry,
        SearchCardMatcher, SearchDestination, SearchManaValue, SpliceAbility, SpliceQuality,
        SubjectDestination, TokenKeyword, TransferKeyword, TriggerCondition, TriggerEvent,
        TriggerFromZone, TriggerGrouping, TriggerKeywordEvent, TriggerSpec, TriggerZone,
        TriggeredAbility, UpkeepKeyword, ValueExpr as EngineValueExpr, ValueObjectRef,
    },
    token::{AttackingDefenderScope, TokenDefinition, TokenName, TokenRecipients, TokenSubtype},
    toolbox::{
        ApplyContinuousSpec, ConditionExpr as EngineConditionExpr, CounterOp, DrawCardsSpec,
        MutateObjectSpec, MutatePlayerSpec, ObjectMutation, ObjectQuery, PlayerSelector,
        PromptChoiceSpec, StackOpSpec, TapStateOp, TokenCopySpec, ZoneDest, ZoneOpSpec,
    },
    turn::Step,
};
use spellwake_oracle_compiler::ast::{CardTypeMask as AstCardTypeMask, ColorArity, ColorMask};
use spellwake_oracle_compiler::v2::ir as v2;

use crate::v2_lowering::{
    atoms::lower_keyword_ability,
    cost::{
        is_battlefield_only_source_cost, lower_activation_cost_parts, lower_extra_cost_atom,
        lower_v2_fixed_mana_cost, lower_v2_mana_cost, merge_mana_cost,
    },
    ctx::{BodyAcc, LowerCtx},
    error::{LowerResult, LoweringError},
    filter::{lower_object_filter, lower_permanent_selector},
    mutation::lower_player_mutation,
};

/// Lowered layer-6 grants produced by granting one or more `v2::KeywordSpec` values
/// (both static `KeywordAbility` values and triggered/activated/parameterized keywords
/// such as `Crew N`, `Saddle N`, `Afterlife N`, `Prowess`, `Toxic N`, `Ward {M}`, `Exalted`).
#[derive(Debug, Clone, Default)]
pub(crate) struct GrantedKeywordParts {
    pub ability_change: AbilityChange,
    pub extra_triggers: Vec<TriggeredAbility>,
    pub extra_activated: Vec<(ActivatedAbility, Option<ActivationRestriction>)>,
}

impl GrantedKeywordParts {
    #[must_use]
    pub(crate) fn has_ability_change(&self) -> bool {
        !self.ability_change.granted_keywords.is_empty()
            || self.ability_change.granted_exalted > 0
            || self.ability_change.granted_toxic.is_some()
            || !self.ability_change.granted_combat_keywords.is_empty()
            || !self.ability_change.granted_triggers.is_empty()
    }
}

fn build_ward_triggered_ability(
    id: spellwake_engine::program::TriggeredAbilityId,
    instructions: Vec<Instruction>,
) -> TriggeredAbility {
    TriggeredAbility {
        id,
        condition: TriggerSpec::new(TriggerEvent::Targeted {
            subject: ObjectFilter::self_object(),
            by_opponent: true,
            spell_only: false,
            instant_or_sorcery_only: false,
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, instructions),
    }
}

fn lower_ward_trigger(costs: &[v2::Cost], ctx: &mut LowerCtx<'_>) -> LowerResult<TriggeredAbility> {
    if costs.is_empty() {
        return Err(LoweringError::Unsupported("v2:kw:ward-non-mana"));
    }
    let payer = ResolvingPlayer::UniqueRelatedPlayer;
    if let [v2::Cost::Mana(mc)] = costs {
        let (fixed, x_cnt, hybrids, snow) = lower_v2_mana_cost(mc)?;
        if hybrids.is_empty() && snow == 0 {
            if x_cnt == 0 {
                return Ok(TriggeredAbility::ward(ctx.alloc_triggered_id(), fixed));
            }
            let mut extra_generic = EngineValueExpr::EventValue(EventValueKind::ChosenX);
            if x_cnt > 1 {
                extra_generic = EngineValueExpr::Scale {
                    expr: Box::new(extra_generic),
                    factor: i64::from(x_cnt).into(),
                };
            }
            return Ok(build_ward_triggered_ability(
                ctx.alloc_triggered_id(),
                vec![
                    Instruction::PromptChoice(Box::new(
                        PromptChoiceSpec::offer_dynamic_mana_payment(
                            payer,
                            fixed,
                            extra_generic,
                            2,
                            1,
                        ),
                    )),
                    Instruction::StackOp(Box::new(StackOpSpec::counter_ward_source())),
                ],
            ));
        }
        return Err(LoweringError::Unsupported("v2:kw:ward-non-mana"));
    }

    let mut mana_cost: Option<ManaCost> = None;
    let mut non_mana_costs: Vec<&v2::Cost> = Vec::new();
    for c in costs {
        if let v2::Cost::Mana(mc) = c {
            let fixed = lower_v2_fixed_mana_cost(mc)
                .map_err(|_| LoweringError::Unsupported("v2:kw:ward-non-mana"))?;
            match &mut mana_cost {
                Some(existing) => merge_mana_cost(existing, &fixed),
                None => mana_cost = Some(fixed),
            }
        } else {
            non_mana_costs.push(c);
        }
    }

    if non_mana_costs.is_empty() {
        let mc = mana_cost.ok_or(LoweringError::Unsupported("v2:kw:ward-non-mana"))?;
        return Ok(TriggeredAbility::ward(ctx.alloc_triggered_id(), mc));
    }

    let prev_in_trigger = ctx.in_trigger;
    let prev_has_participant = ctx.trigger_has_participant;
    ctx.in_trigger = true;
    ctx.trigger_has_participant = true;
    let mut cost_acc = BodyAcc::new();
    let mut lower_res: LowerResult<()> = Ok(());
    for c in non_mana_costs {
        if let Err(err) = lower_player_mutation(
            v2::PlayerRef::ThatPlayer,
            &v2::PlayerMutation::PayCost(c.clone()),
            &v2::Duration::Instant,
            &mut cost_acc,
            ctx,
        ) {
            lower_res = Err(err);
            break;
        }
    }
    ctx.in_trigger = prev_in_trigger;
    ctx.trigger_has_participant = prev_has_participant;
    lower_res.map_err(|_| LoweringError::Unsupported("v2:kw:ward-non-mana"))?;

    if cost_acc.target_set
        || cost_acc.target != BodyTarget::None
        || cost_acc.instructions.is_empty()
        || cost_acc.instructions.iter().any(|ins| {
            matches!(
                ins,
                Instruction::BranchCondition { .. }
                    | Instruction::Jump { .. }
                    | Instruction::OptionalSequence { .. }
            )
        })
    {
        return Err(LoweringError::Unsupported("v2:kw:ward-non-mana"));
    }
    let non_mana_ops = cost_acc.instructions;

    let mut instructions = Vec::with_capacity(non_mana_ops.len() + 4);
    if let Some(mc) = mana_cost {
        let counter_idx = 1 + 1 + non_mana_ops.len() + 1;
        instructions.push(Instruction::PromptChoice(Box::new(
            PromptChoiceSpec::offer_dynamic_mana_payment(
                payer,
                mc,
                EngineValueExpr::Literal(0.into()),
                1,
                counter_idx,
            ),
        )));
    }
    instructions.push(Instruction::OptionalSequence {
        chooser: payer,
        instructions: non_mana_ops.len(),
    });
    instructions.extend(non_mana_ops);
    let branch_idx = instructions.len();
    instructions.push(Instruction::BranchCondition {
        condition: Box::new(EngineConditionExpr::Resolution(Box::new(
            ResolutionCondition {
                left: EngineValueExpr::EventValue(EventValueKind::LastActionSucceeded),
                comparison: Comparison::Less,
                right: EngineValueExpr::Literal(1.into()),
            },
        ))),
        destination: branch_idx + 2,
    });
    instructions.push(Instruction::StackOp(Box::new(
        StackOpSpec::counter_ward_source(),
    )));

    Ok(build_ward_triggered_ability(
        ctx.alloc_triggered_id(),
        instructions,
    ))
}

/// Lower a list of granted `v2::KeywordSpec` entries into layer-6 [`GrantedKeywordParts`].
///
/// Supports static keywords as well as triggered/activated/parameterized keywords faithfully
/// representable through `AbilityChange`, `GrantTrigger`, and `GrantActivated`.
pub(crate) fn lower_granted_keywords(
    keywords: &[v2::KeywordSpec],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<GrantedKeywordParts> {
    let mut out = GrantedKeywordParts::default();
    for kw in keywords {
        if let Ok(ka) = lower_keyword_ability(kw, ctx) {
            out.ability_change.granted_keywords.insert(ka);
            if ka == KeywordAbility::Decayed {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Decayed);
            }
            continue;
        }
        match (kw.kind, &kw.param) {
            (v2::KeywordKind::Exalted, None) => {
                out.ability_change.granted_exalted =
                    out.ability_change.granted_exalted.saturating_add(1);
            }
            (v2::KeywordKind::Toxic, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                let add = spellwake_contracts::RulesInteger::from(i64::from(*n));
                out.ability_change.granted_toxic = Some(match &out.ability_change.granted_toxic {
                    Some(prev) => prev.plus(&add),
                    None => add,
                });
            }
            (v2::KeywordKind::Prowess, None) => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Prowess);
            }
            (v2::KeywordKind::Flanking, None) => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Flanking);
            }
            (v2::KeywordKind::BattleCry, None) => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::BattleCry);
            }
            (v2::KeywordKind::Melee, None) => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Melee);
            }
            (v2::KeywordKind::Dethrone, None) => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Dethrone);
            }
            (v2::KeywordKind::Training, None) => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Training);
            }
            (v2::KeywordKind::Ingest, None) => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Ingest);
            }
            (v2::KeywordKind::Afflict, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Afflict(i64::from(*n).into()));
            }
            (v2::KeywordKind::Bushido, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Bushido(i64::from(*n).into()));
            }
            (v2::KeywordKind::Frenzy, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Frenzy(i64::from(*n).into()));
            }
            (v2::KeywordKind::Mobilize, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Mobilize(i64::from(*n).into()));
            }
            (v2::KeywordKind::Poisonous, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Poisonous(i64::from(*n).into()));
            }
            (v2::KeywordKind::Rampage, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Rampage(i64::from(*n).into()));
            }
            (v2::KeywordKind::Renown, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                out.ability_change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Renown(i64::from(*n).into()));
            }
            (v2::KeywordKind::Afterlife, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                let ab = TokenKeyword::Afterlife
                    .ability(ctx.alloc_triggered_id(), i64::from(*n).into(), ctx.vocab)
                    .map_err(|_| LoweringError::Unsupported("v2:kw:afterlife-vocab"))?;
                out.extra_triggers.push(ab);
            }
            (v2::KeywordKind::Fabricate, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                let ab = TokenKeyword::Fabricate
                    .ability(ctx.alloc_triggered_id(), i64::from(*n).into(), ctx.vocab)
                    .map_err(|_| LoweringError::Unsupported("v2:kw:fabricate-vocab"))?;
                out.extra_triggers.push(ab);
            }
            (v2::KeywordKind::Ward, Some(v2::KeywordParam::Cost(costs))) => {
                out.extra_triggers.push(lower_ward_trigger(costs, ctx)?);
            }
            (v2::KeywordKind::Annihilator, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                out.extra_triggers.push(TriggeredAbility::annihilator(
                    ctx.alloc_triggered_id(),
                    i64::from(*n).into(),
                ));
            }
            (v2::KeywordKind::Extort, None) => {
                out.extra_triggers
                    .push(TriggeredAbility::extort(ctx.alloc_triggered_id()));
            }
            (v2::KeywordKind::Undying, None) => {
                out.extra_triggers
                    .push(CounterKeyword::Undying.ability(ctx.alloc_triggered_id()));
            }
            (v2::KeywordKind::Persist, None) => {
                out.extra_triggers
                    .push(CounterKeyword::Persist.ability(ctx.alloc_triggered_id()));
            }
            (v2::KeywordKind::Evolve, None) => {
                out.extra_triggers
                    .push(GrowthKeyword::Evolve.ability(ctx.alloc_triggered_id()));
            }
            (v2::KeywordKind::Mentor, None) => {
                out.extra_triggers
                    .push(GrowthKeyword::Mentor.ability(ctx.alloc_triggered_id()));
            }
            (v2::KeywordKind::Increment, None) => {
                out.extra_triggers
                    .push(GrowthKeyword::Increment.ability(ctx.alloc_triggered_id()));
            }
            (v2::KeywordKind::Soulshift, Some(v2::KeywordParam::Number(n))) => {
                out.extra_triggers.push(
                    TransferKeyword::Soulshift(u32::from(*n)).ability(ctx.alloc_triggered_id()),
                );
            }
            (v2::KeywordKind::Myriad, None) => {
                out.extra_triggers.push(TriggeredAbility {
                    id: ctx.alloc_triggered_id(),
                    condition: TriggerSpec::attacks(
                        ObjectFilter::self_object(),
                        AttackTriggerMode::Individual {
                            capture_defender: true,
                        },
                    ),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![
                            Instruction::MutateObject(Box::new(MutateObjectSpec::myriad())),
                            Instruction::ScheduleDelayedStepTrigger {
                                step: Step::EndCombat,
                                own_step_only: false,
                                body: Box::new(ExecutableBody::new(
                                    BodyTarget::None,
                                    vec![Instruction::ZoneOp(Box::new(ZoneOpSpec::move_all(
                                        ObjectQuery::CreatedTokens,
                                        ZoneDest::exile(),
                                    )))],
                                )),
                            },
                        ],
                    ),
                });
            }
            (v2::KeywordKind::Enlist, None) => {
                out.extra_triggers.push(TriggeredAbility {
                    id: ctx.alloc_triggered_id(),
                    condition: TriggerSpec::self_attacks(),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::MutateObject(Box::new(
                            MutateObjectSpec::enlist(),
                        ))],
                    ),
                });
            }
            (v2::KeywordKind::Provoke, None) => {
                out.extra_triggers.push(TriggeredAbility {
                    id: ctx.alloc_triggered_id(),
                    condition: TriggerSpec::attacks(
                        ObjectFilter::self_object(),
                        AttackTriggerMode::Individual {
                            capture_defender: true,
                        },
                    ),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::filtered_permanent(ObjectFilter::creature().opposing()),
                        vec![
                            Instruction::OptionalSequence {
                                chooser: ResolvingPlayer::Controller,
                                instructions: 1,
                            },
                            Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                                ObjectQuery::Targets,
                                ObjectMutation::TapState(TapStateOp::Set(false)),
                            ))),
                            Instruction::ApplyContinuous(Box::new(
                                ApplyContinuousSpec::resolved_parts(
                                    ObjectQuery::Targets,
                                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                        BlockingRestriction::MustBlockSource,
                                    )]),
                                    ResolvedDuration::UntilEndOfTurn,
                                ),
                            )),
                        ],
                    ),
                });
            }
            (v2::KeywordKind::Exploit, None) => {
                out.extra_triggers.push(TriggeredAbility {
                    id: ctx.alloc_triggered_id(),
                    condition: TriggerSpec::self_enters(),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![
                            Instruction::OptionalSequence {
                                chooser: ResolvingPlayer::Controller,
                                instructions: 1,
                            },
                            Instruction::ZoneOp(Box::new(ZoneOpSpec::sacrifice(
                                AffectedPlayers::One(ResolvingPlayer::Controller),
                                EngineValueExpr::literal(1),
                                PermanentTypeFilter {
                                    any_types: BTreeSet::new(),
                                    required_types: BTreeSet::from([CardType::Creature]),
                                    forbidden_types: BTreeSet::new(),
                                    object_filter: None,
                                },
                            ))),
                        ],
                    ),
                });
            }
            (v2::KeywordKind::Firebending, Some(v2::KeywordParam::Number(n))) if *n > 0 => {
                out.extra_triggers.push(TriggeredAbility {
                    id: ctx.alloc_triggered_id(),
                    condition: TriggerSpec::attacks(
                        ObjectFilter::self_object(),
                        AttackTriggerMode::Individual {
                            capture_defender: false,
                        },
                    ),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::MutatePlayer(Box::new(
                            MutatePlayerSpec::add_mana(
                                ResolvingPlayer::Controller,
                                EffectManaYield::RetainedUntilEndOfCombat {
                                    kind: ManaType::Red,
                                    amount: EngineValueExpr::literal(i64::from(*n)),
                                },
                            ),
                        ))],
                    ),
                });
            }
            (v2::KeywordKind::Crew, Some(v2::KeywordParam::Number(n))) => {
                out.extra_activated.push((
                    ActivatedAbility::tap_for_power(
                        ActivatedAbilityId(0),
                        PowerAbility::Crew,
                        u64::from(*n),
                    ),
                    None,
                ));
            }
            (v2::KeywordKind::Saddle, Some(v2::KeywordParam::Number(n))) => {
                out.extra_activated.push((
                    ActivatedAbility::tap_for_power(
                        ActivatedAbilityId(0),
                        PowerAbility::Saddle,
                        u64::from(*n),
                    ),
                    Some(ActivationRestriction::Sorcery),
                ));
            }
            (v2::KeywordKind::Outlast, Some(v2::KeywordParam::Cost(costs))) => {
                let mana = extract_fixed_mana_from_costs(costs, "v2:kw:outlast-cost")?;
                out.extra_activated.push((
                    ActivatedAbility {
                        id: ActivatedAbilityId(0),
                        mana_cost: mana,
                        source_cost: SourceCost::Tap,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                                ObjectQuery::Source,
                                ObjectMutation::Counters(CounterOp::Add {
                                    kind: EngineCounterKind::PlusOne,
                                    amount: EngineValueExpr::literal(1),
                                }),
                            )))],
                        ),
                    },
                    Some(ActivationRestriction::Sorcery),
                ));
            }
            (v2::KeywordKind::Equip, Some(v2::KeywordParam::Cost(costs))) => {
                let mana = extract_fixed_mana_from_costs(costs, "v2:kw:equip-cost")?;
                out.extra_activated.push((
                    ActivatedAbility {
                        id: ActivatedAbilityId(0),
                        mana_cost: mana,
                        source_cost: SourceCost::None,
                        body: ExecutableBody::new(
                            BodyTarget::SINGLE_CREATURE,
                            vec![Instruction::MutateObject(Box::new(
                                MutateObjectSpec::attach_ability(AttachAbility::Equip),
                            ))],
                        ),
                    },
                    None,
                ));
            }
            _ => {
                let _ = lower_keyword_ability(kw, ctx)?;
            }
        }
    }
    Ok(out)
}

/// Map a bare, parameter-free `v2::KeywordKind` to its static engine [`KeywordAbility`], if any.
#[must_use]
pub(crate) fn simple_keyword_ability(kind: v2::KeywordKind) -> Option<KeywordAbility> {
    match kind {
        v2::KeywordKind::Flying => Some(KeywordAbility::Flying),
        v2::KeywordKind::FirstStrike => Some(KeywordAbility::FirstStrike),
        v2::KeywordKind::DoubleStrike => Some(KeywordAbility::DoubleStrike),
        v2::KeywordKind::Deathtouch => Some(KeywordAbility::Deathtouch),
        v2::KeywordKind::Haste => Some(KeywordAbility::Haste),
        v2::KeywordKind::Hexproof => Some(KeywordAbility::Hexproof),
        v2::KeywordKind::Indestructible => Some(KeywordAbility::Indestructible),
        v2::KeywordKind::Lifelink => Some(KeywordAbility::Lifelink),
        v2::KeywordKind::Menace => Some(KeywordAbility::Menace),
        v2::KeywordKind::Reach => Some(KeywordAbility::Reach),
        v2::KeywordKind::Trample => Some(KeywordAbility::Trample),
        v2::KeywordKind::Vigilance => Some(KeywordAbility::Vigilance),
        v2::KeywordKind::Defender => Some(KeywordAbility::Defender),
        v2::KeywordKind::Flash => Some(KeywordAbility::Flash),
        v2::KeywordKind::Shroud => Some(KeywordAbility::Shroud),
        v2::KeywordKind::Wither => Some(KeywordAbility::Wither),
        v2::KeywordKind::Infect => Some(KeywordAbility::Infect),
        v2::KeywordKind::Fear => Some(KeywordAbility::Fear),
        v2::KeywordKind::Intimidate => Some(KeywordAbility::Intimidate),
        v2::KeywordKind::Skulk => Some(KeywordAbility::Skulk),
        v2::KeywordKind::Shadow => Some(KeywordAbility::Shadow),
        v2::KeywordKind::Horsemanship => Some(KeywordAbility::Horsemanship),
        v2::KeywordKind::Banding => Some(KeywordAbility::Banding),
        v2::KeywordKind::Changeling => Some(KeywordAbility::Changeling),
        v2::KeywordKind::Devoid => Some(KeywordAbility::Devoid),
        v2::KeywordKind::SplitSecond => Some(KeywordAbility::SplitSecond),
        v2::KeywordKind::Rebound => Some(KeywordAbility::Rebound),
        v2::KeywordKind::Retrace => Some(KeywordAbility::Retrace),
        v2::KeywordKind::JumpStart => Some(KeywordAbility::JumpStart),
        v2::KeywordKind::Convoke => Some(KeywordAbility::Convoke),
        v2::KeywordKind::Delve => Some(KeywordAbility::Delve),
        v2::KeywordKind::Improvise => Some(KeywordAbility::Improvise),
        v2::KeywordKind::Bargain => Some(KeywordAbility::Bargain),
        v2::KeywordKind::Partner | v2::KeywordKind::FriendsForever => Some(KeywordAbility::Partner),
        v2::KeywordKind::ChooseABackground => Some(KeywordAbility::ChooseABackground),
        v2::KeywordKind::DoctorsCompanion => Some(KeywordAbility::DoctorsCompanion),
        v2::KeywordKind::Plainswalk => Some(KeywordAbility::Plainswalk),
        v2::KeywordKind::Islandwalk => Some(KeywordAbility::Islandwalk),
        v2::KeywordKind::Swampwalk => Some(KeywordAbility::Swampwalk),
        v2::KeywordKind::Mountainwalk => Some(KeywordAbility::Mountainwalk),
        v2::KeywordKind::Forestwalk => Some(KeywordAbility::Forestwalk),
        v2::KeywordKind::Phasing => Some(KeywordAbility::Phasing),
        v2::KeywordKind::UmbraArmor => Some(KeywordAbility::UmbraArmor),
        v2::KeywordKind::Daybound => Some(KeywordAbility::Daybound),
        v2::KeywordKind::Nightbound => Some(KeywordAbility::Nightbound),
        v2::KeywordKind::StartYourEngines => Some(KeywordAbility::StartYourEngines),
        v2::KeywordKind::Decayed => Some(KeywordAbility::Decayed),
        _ => None,
    }
}

/// Apply one typed `v2::KeywordSpec` onto `program`, failing closed on any unsupported keyword
/// or parameter shape.
pub(crate) fn apply_face_keyword(
    spec: &v2::KeywordSpec,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let is_spell = matches!(program.kind, ProgramKind::Spell(_));

    // Parameter-free static keyword abilities.
    if spec.param.is_none()
        && let Some(ka) = simple_keyword_ability(spec.kind)
    {
        if ka == KeywordAbility::Decayed {
            program.printed_keywords.insert(KeywordAbility::Decayed);
            program.triggered.push(TriggeredAbility::combat_keyword(
                ctx.alloc_triggered_id(),
                CombatTriggerKeyword::Decayed,
            ));
            return Ok(());
        }
        program.printed_keywords.insert(ka);
        return Ok(());
    }

    match spec.kind {
        // Hexproof from <quality>
        v2::KeywordKind::Hexproof => {
            let param = spec
                .param
                .as_ref()
                .ok_or(LoweringError::Unsupported("v2:kw:hexproof-param"))?;
            let quals = lower_hexproof_from_param(param, ctx)?;
            for q in quals {
                program
                    .printed_keywords
                    .insert(KeywordAbility::HexproofFrom(q));
            }
            Ok(())
        }

        // Protection from <quality>
        v2::KeywordKind::Protection => {
            let param = spec
                .param
                .as_ref()
                .ok_or(LoweringError::Unsupported("v2:kw:protection-no-param"))?;
            match param {
                v2::KeywordParam::ChosenColor => {
                    push_static(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ctx.alloc_static_id(),
                            selector: PermanentSelector {
                                controller: ControllerSelector::SourceItself,
                                ..PermanentSelector::default()
                            },
                            parts: StaticParts(vec![StaticPart::GrantProtectionChosenColor]),
                        },
                    );
                    Ok(())
                }
                v2::KeywordParam::ChosenPlayer => {
                    push_static(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ctx.alloc_static_id(),
                            selector: PermanentSelector {
                                controller: ControllerSelector::SourceItself,
                                ..PermanentSelector::default()
                            },
                            parts: StaticParts(vec![StaticPart::GrantProtectionChosenPlayer]),
                        },
                    );
                    Ok(())
                }
                other => {
                    let quals = lower_protection_param(other, ctx)?;
                    program.printed_keywords.extend(quals);
                    Ok(())
                }
            }
        }

        // Enchant <object or player>
        v2::KeywordKind::Enchant => {
            let restriction = match &spec.param {
                None => EnchantRestriction::Player,
                Some(v2::KeywordParam::Filter(flt)) => lower_enchant_filter(flt, ctx)?,
                Some(_) => {
                    return Err(LoweringError::Unsupported("v2:kw:enchant-param"));
                }
            };
            let ProgramKind::Permanent(def) = &mut program.kind else {
                return Err(LoweringError::Unsupported("v2:kw:enchant-on-spell"));
            };
            if def
                .attachment
                .replace(AttachmentKind::Aura(restriction))
                .is_some()
            {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-attachment"));
            }
            Ok(())
        }

        // Equip <cost> / Equip <filter> <cost>
        v2::KeywordKind::Equip => {
            let (target, costs) = match &spec.param {
                Some(v2::KeywordParam::Cost(costs)) => (BodyTarget::SINGLE_CREATURE, costs),
                Some(v2::KeywordParam::FilterAndCost(flt, costs)) => {
                    let mut obj = lower_object_filter(flt, ctx)?;
                    obj = ensure_controlled_creature_equip_filter(obj);
                    (BodyTarget::filtered_permanent(obj), costs)
                }
                _ => return Err(LoweringError::Unsupported("v2:kw:equip-param")),
            };
            let (mana_cost, source_cost, hybrids, extra_costs) =
                lower_activation_cost_parts(costs, false, ctx)?;
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if !extra_costs.is_empty() {
                program.extra_activation_costs.insert(id, extra_costs);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost,
                source_cost,
                body: ExecutableBody::new(
                    target,
                    vec![Instruction::MutateObject(Box::new(
                        MutateObjectSpec::attach_ability(AttachAbility::Equip),
                    ))],
                ),
            });
            Ok(())
        }

        // Reconfigure <cost>
        v2::KeywordKind::Reconfigure => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:reconfigure-cost")?;
            program.activated.push(ActivatedAbility {
                id: ctx.alloc_activated_id(),
                mana_cost: mana.clone(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SINGLE_CREATURE,
                    vec![Instruction::MutateObject(Box::new(
                        MutateObjectSpec::attach_ability(AttachAbility::Reconfigure),
                    ))],
                ),
            });
            program.activated.push(ActivatedAbility {
                id: ctx.alloc_activated_id(),
                mana_cost: mana,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                        ObjectQuery::Source,
                        ObjectMutation::Attach(spellwake_engine::toolbox::AttachOp::Unattach),
                    )))],
                ),
            });
            Ok(())
        }

        // Fortify <cost>
        v2::KeywordKind::Fortify => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:fortify-cost")?;
            program.activated.push(ActivatedAbility {
                id: ctx.alloc_activated_id(),
                mana_cost: mana,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SINGLE_PERMANENT,
                    vec![Instruction::MutateObject(Box::new(
                        MutateObjectSpec::attach_ability(AttachAbility::Fortify),
                    ))],
                ),
            });
            Ok(())
        }

        // Ward <cost>
        v2::KeywordKind::Ward => {
            let Some(v2::KeywordParam::Cost(costs)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:ward-param"));
            };
            program.triggered.push(lower_ward_trigger(costs, ctx)?);
            Ok(())
        }

        // Kicker <cost>
        v2::KeywordKind::Kicker => {
            let Some(v2::KeywordParam::Cost(costs)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:kicker-param"));
            };
            let add_cost = lower_kicker_cost(costs, ctx)?;
            program.additional_cast_costs.push(add_cost);
            Ok(())
        }

        // Multikicker <cost>
        v2::KeywordKind::Multikicker => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:multikicker-cost")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Multikicker(mana));
            Ok(())
        }

        // Buyback <cost>
        v2::KeywordKind::Buyback => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:buyback-cost")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Buyback(mana));
            Ok(())
        }

        // Entwine <cost>
        v2::KeywordKind::Entwine => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:entwine-cost")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Entwine(mana));
            Ok(())
        }

        // Replicate <cost>
        v2::KeywordKind::Replicate if !program.has_type(CardType::Land) => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:replicate-cost")?;
            let idx = program.additional_cast_costs.len();
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Replicate(mana));
            program.triggered.push(TriggeredAbility::spell_copies(
                ctx.alloc_triggered_id(),
                CastCopyKeyword::Replicate(idx),
            ));
            Ok(())
        }

        // Flashback <cost>
        v2::KeywordKind::Flashback => lower_flashback_keyword(&spec.param, program),

        // Cycling <cost>
        v2::KeywordKind::Cycling => {
            let (mana, hybrids, snow) =
                extract_mana_with_hybrids_snow(&spec.param, "v2:kw:cycling-cost")?;
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz)]);
            }
            program.activated.push(ActivatedAbility::cycling(id, mana));
            Ok(())
        }

        // Basic landcycling <cost>
        v2::KeywordKind::BasicLandcycling => {
            let (mana, hybrids, snow) =
                extract_mana_with_hybrids_snow(&spec.param, "v2:kw:basic-landcycling-cost")?;
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz)]);
            }
            program.activated.push(ActivatedAbility::typecycling(
                id,
                mana,
                LibrarySearch::to_hand(SearchCardMatcher::BasicLand),
            ));
            Ok(())
        }

        // Typecycling loses the land/creature prefix in v2::KeywordKind::Typecycling.
        v2::KeywordKind::Typecycling => Err(LoweringError::Unsupported(
            "v2:kw:typecycling-untyped-prefix",
        )),

        // Suspend N - <cost>
        v2::KeywordKind::Suspend => {
            let Some(v2::KeywordParam::NumberAndCost(counters, costs)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:suspend-param"));
            };
            if *counters == 0 {
                return Err(LoweringError::Unsupported("v2:kw:suspend-zero"));
            }
            let price = extract_fixed_mana_from_costs(costs, "v2:kw:suspend-cost")?;
            if program
                .exile_action
                .replace(ExileAction::Suspend {
                    counters: *counters,
                    price,
                })
                .is_some()
            {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-exile-action"));
            }
            Ok(())
        }

        // Foretell <cost>
        v2::KeywordKind::Foretell => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:foretell-cost")?;
            if program
                .exile_action
                .replace(ExileAction::Foretell(mana))
                .is_some()
            {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-exile-action"));
            }
            Ok(())
        }

        // Plot <cost>
        v2::KeywordKind::Plot => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:plot-cost")?;
            if program.plot.replace(mana).is_some() {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-plot"));
            }
            Ok(())
        }

        // Dash / Blitz / Warp <cost>
        v2::KeywordKind::Dash | v2::KeywordKind::Blitz | v2::KeywordKind::Warp => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:endstep-cast-cost")?;
            if program.conditional_cost.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:kw:duplicate-conditional-cost",
                ));
            }
            let kind = match spec.kind {
                v2::KeywordKind::Dash => EndStepCast::Dash,
                v2::KeywordKind::Blitz => EndStepCast::Blitz,
                _ => EndStepCast::Warp,
            };
            let tid = ctx.alloc_triggered_id();
            let owned = std::mem::replace(program, Program::enchantment(None));
            *program = owned.with_end_step_cast(kind, mana, tid);
            Ok(())
        }

        // Evoke <cost>
        v2::KeywordKind::Evoke => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:evoke-cost")?;
            if program.conditional_cost.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:kw:duplicate-conditional-cost",
                ));
            }
            let tid = ctx.alloc_triggered_id();
            let owned = std::mem::replace(program, Program::enchantment(None));
            *program = owned.with_evoke(mana, tid);
            Ok(())
        }

        // Escape - <mana>, Exile N other cards from your graveyard
        v2::KeywordKind::Escape => {
            let Some(v2::KeywordParam::Cost(costs)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:escape-param"));
            };
            let (mana, exile) = lower_escape_cost(costs)?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Escape {
                    mana,
                    exile,
                    entry_counters: 0,
                })
                .is_some()
            {
                return Err(LoweringError::Unsupported(
                    "v2:kw:duplicate-conditional-cost",
                ));
            }
            Ok(())
        }

        // Unearth <cost>
        v2::KeywordKind::Unearth => {
            let (mana, hybrids, snow) =
                extract_mana_with_hybrids_snow(&spec.param, "v2:kw:unearth-cost")?;
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz)]);
            }
            program.activated.push(ActivatedAbility::unearth(id, mana));
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }

        // Madness <cost>
        v2::KeywordKind::Madness => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:madness-cost")?;
            if program.madness.replace(mana).is_some() {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-madness"));
            }
            Ok(())
        }

        // Miracle <cost>
        v2::KeywordKind::Miracle => {
            let Some(v2::KeywordParam::Cost(costs)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:miracle-param"));
            };
            let [v2::Cost::Mana(mc)] = costs.as_slice() else {
                return Err(LoweringError::Unsupported("v2:kw:miracle-cost"));
            };
            let (mana, x_symbols, hybrids, snow) = lower_v2_mana_cost(mc)?;
            if !hybrids.is_empty() || snow != 0 {
                return Err(LoweringError::Unsupported("v2:kw:miracle-cost"));
            }
            if program
                .miracle
                .replace(MiracleCost { mana, x_symbols })
                .is_some()
            {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-miracle"));
            }
            Ok(())
        }

        // Bestow / Surge / Spectacle / Prowl / Freerunning / Sneak / Mayhem / Mutate / WebSlinging / Harmonize
        v2::KeywordKind::Bestow => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:bestow-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Bestow(mana))
        }
        v2::KeywordKind::Surge => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:surge-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Surge(mana))
        }
        v2::KeywordKind::Spectacle => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:spectacle-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Spectacle(mana))
        }
        v2::KeywordKind::Prowl => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:prowl-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Prowl(mana))
        }
        v2::KeywordKind::Freerunning => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:freerunning-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Freerunning(mana))
        }
        v2::KeywordKind::Sneak => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:sneak-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Sneak(mana))
        }
        v2::KeywordKind::Mayhem => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:mayhem-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Mayhem(mana))
        }
        v2::KeywordKind::Mutate => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:mutate-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Mutate(mana))
        }
        v2::KeywordKind::WebSlinging => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:webslinging-cost")?;
            set_conditional_cost(program, ConditionalCastCost::WebSlinging(mana))
        }
        v2::KeywordKind::Harmonize => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:harmonize-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Harmonize(mana))
        }

        // Emerge <cost> / Emerge from <type> <cost>
        v2::KeywordKind::Emerge => {
            let (from, mana) = match &spec.param {
                Some(v2::KeywordParam::Cost(costs)) => (
                    CardType::Creature,
                    extract_fixed_mana_from_costs(costs, "v2:kw:emerge-cost")?,
                ),
                Some(v2::KeywordParam::FilterAndCost(flt, costs)) => {
                    let ct = single_card_type_from_filter(flt)
                        .ok_or(LoweringError::Unsupported("v2:kw:emerge-type"))?;
                    let mana = extract_fixed_mana_from_costs(costs, "v2:kw:emerge-cost")?;
                    (ct, mana)
                }
                _ => return Err(LoweringError::Unsupported("v2:kw:emerge-param")),
            };
            set_conditional_cost(program, ConditionalCastCost::Emerge { mana, from })
        }

        // Awaken N - <cost>
        v2::KeywordKind::Awaken => {
            let Some(v2::KeywordParam::NumberAndCost(n, costs)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:awaken-param"));
            };
            let counters = u8::try_from(*n)
                .ok()
                .filter(|&v| v > 0)
                .ok_or(LoweringError::Unsupported("v2:kw:awaken-count"))?;
            let mana = extract_fixed_mana_from_costs(costs, "v2:kw:awaken-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Awaken { mana, counters })
        }

        // Impending N - <cost>
        v2::KeywordKind::Impending if !is_spell => {
            let Some(v2::KeywordParam::NumberAndCost(n, costs)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:impending-param"));
            };
            let counters = u8::try_from(*n)
                .ok()
                .filter(|&v| v > 0)
                .ok_or(LoweringError::Unsupported("v2:kw:impending-count"))?;
            let mana = extract_fixed_mana_from_costs(costs, "v2:kw:impending-cost")?;
            set_conditional_cost(program, ConditionalCastCost::Impending { mana, counters })?;
            push_static(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        source_condition: Some(StaticSourceCondition::SourceCounterAtLeast {
                            kind: EngineCounterKind::Time,
                            minimum: 1,
                        }),
                        ..PermanentSelector::default()
                    },
                    parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
                        subtypes: SubtypeOperation::Keep,
                    })]),
                },
            );
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::step_began(Step::End, PlayerRelation::Controller).only_if(
                    TriggerCondition::SourceCounterAtLeast {
                        kind: EngineCounterKind::Time,
                        minimum: 1,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                        ObjectQuery::Source,
                        ObjectMutation::Counters(CounterOp::Remove {
                            kind: Some(EngineCounterKind::Time),
                            amount: EngineValueExpr::literal(1),
                        }),
                    )))],
                ),
            });
            Ok(())
        }

        // Ninjutsu / Commander ninjutsu
        v2::KeywordKind::Ninjutsu | v2::KeywordKind::NinjutsuCommander => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:ninjutsu-cost")?;
            let id = ctx.alloc_activated_id();
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: mana,
                source_cost: SourceCost::Ninjutsu {
                    from_command: spec.kind == v2::KeywordKind::NinjutsuCommander,
                },
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        MutateObjectSpec::ninjutsu(),
                    ))],
                ),
            });
            Ok(())
        }

        // Morph / Megamorph / Disguise
        v2::KeywordKind::Morph => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:morph-cost")?;
            program.face_up_abilities.push(FaceUpAbility::Morph(mana));
            Ok(())
        }
        v2::KeywordKind::Megamorph => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:megamorph-cost")?;
            program
                .face_up_abilities
                .push(FaceUpAbility::Megamorph(mana));
            Ok(())
        }
        v2::KeywordKind::Disguise => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:disguise-cost")?;
            program
                .face_up_abilities
                .push(FaceUpAbility::Disguise(mana));
            Ok(())
        }

        // Crew / Saddle / Station
        v2::KeywordKind::Crew => {
            let n = extract_number(&spec.param, "v2:kw:crew-param")?;
            program.activated.push(ActivatedAbility::tap_for_power(
                ctx.alloc_activated_id(),
                PowerAbility::Crew,
                u64::from(n),
            ));
            Ok(())
        }
        v2::KeywordKind::Saddle => {
            let n = extract_number(&spec.param, "v2:kw:saddle-param")?;
            let id = ctx.alloc_activated_id();
            program.activated.push(ActivatedAbility::tap_for_power(
                id,
                PowerAbility::Saddle,
                u64::from(n),
            ));
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        v2::KeywordKind::Station => {
            let id = ctx.alloc_activated_id();
            program.activated.push(ActivatedAbility::tap_for_power(
                id,
                PowerAbility::Station,
                0,
            ));
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }

        // Cascade / Storm / Gravestorm / Affinity / Undaunted
        v2::KeywordKind::Cascade if spec.param.is_none() => {
            program
                .triggered
                .push(TriggeredAbility::cascade(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Storm if spec.param.is_none() && !program.has_type(CardType::Land) => {
            program.triggered.push(TriggeredAbility::spell_copies(
                ctx.alloc_triggered_id(),
                CastCopyKeyword::Storm,
            ));
            Ok(())
        }
        v2::KeywordKind::Gravestorm if spec.param.is_none() && is_spell => {
            program.triggered.push(TriggeredAbility::spell_copies(
                ctx.alloc_triggered_id(),
                CastCopyKeyword::Gravestorm,
            ));
            Ok(())
        }
        v2::KeywordKind::Affinity => {
            let Some(v2::KeywordParam::Filter(flt)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:affinity-param"));
            };
            if flt.any_card || flt.any_spell || flt.is_ability {
                return Err(LoweringError::Unsupported("v2:kw:affinity-non-permanent"));
            }
            let subj = v2::SubjectRef::Each(flt.clone());
            let mut sel = lower_permanent_selector(&subj, ctx)?;
            if sel.controller != ControllerSelector::Any || sel.owner.is_some() {
                return Err(LoweringError::Unsupported("v2:kw:affinity-controller"));
            }
            sel.controller = ControllerSelector::SourceController;
            program
                .cast_reductions
                .push(CastReduction::Affinity(Box::new(sel)));
            Ok(())
        }
        v2::KeywordKind::Undaunted if spec.param.is_none() => {
            program.cast_reductions.push(CastReduction::Undaunted);
            Ok(())
        }

        // Combat & spell-cast triggered keywords
        v2::KeywordKind::Prowess if spec.param.is_none() => {
            push_combat_kw(program, ctx, CombatTriggerKeyword::Prowess);
            Ok(())
        }
        v2::KeywordKind::Exalted if spec.param.is_none() => {
            program
                .triggered
                .push(TriggeredAbility::exalted(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Extort if spec.param.is_none() => {
            program
                .triggered
                .push(TriggeredAbility::extort(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Flanking if spec.param.is_none() => {
            push_combat_kw(program, ctx, CombatTriggerKeyword::Flanking);
            Ok(())
        }
        v2::KeywordKind::BattleCry if spec.param.is_none() => {
            push_combat_kw(program, ctx, CombatTriggerKeyword::BattleCry);
            Ok(())
        }
        v2::KeywordKind::Melee if spec.param.is_none() => {
            push_combat_kw(program, ctx, CombatTriggerKeyword::Melee);
            Ok(())
        }
        v2::KeywordKind::Dethrone if spec.param.is_none() => {
            push_combat_kw(program, ctx, CombatTriggerKeyword::Dethrone);
            Ok(())
        }
        v2::KeywordKind::Training if spec.param.is_none() => {
            push_combat_kw(program, ctx, CombatTriggerKeyword::Training);
            Ok(())
        }
        v2::KeywordKind::Ingest if spec.param.is_none() => {
            push_combat_kw(program, ctx, CombatTriggerKeyword::Ingest);
            Ok(())
        }
        v2::KeywordKind::Afflict => {
            let n = extract_number(&spec.param, "v2:kw:afflict-param")?;
            push_combat_kw(
                program,
                ctx,
                CombatTriggerKeyword::Afflict(i64::from(n).into()),
            );
            Ok(())
        }
        v2::KeywordKind::Annihilator => {
            let n = extract_number(&spec.param, "v2:kw:annihilator-param")?;
            program.triggered.push(TriggeredAbility::annihilator(
                ctx.alloc_triggered_id(),
                i64::from(n).into(),
            ));
            Ok(())
        }
        v2::KeywordKind::Bushido => {
            let n = extract_number(&spec.param, "v2:kw:bushido-param")?;
            push_combat_kw(
                program,
                ctx,
                CombatTriggerKeyword::Bushido(i64::from(n).into()),
            );
            Ok(())
        }
        v2::KeywordKind::Frenzy => {
            let n = extract_number(&spec.param, "v2:kw:frenzy-param")?;
            push_combat_kw(
                program,
                ctx,
                CombatTriggerKeyword::Frenzy(i64::from(n).into()),
            );
            Ok(())
        }
        v2::KeywordKind::Mobilize => {
            let n = extract_number(&spec.param, "v2:kw:mobilize-param")?;
            push_combat_kw(
                program,
                ctx,
                CombatTriggerKeyword::Mobilize(i64::from(n).into()),
            );
            Ok(())
        }
        v2::KeywordKind::Poisonous => {
            let n = extract_number(&spec.param, "v2:kw:poisonous-param")?;
            push_combat_kw(
                program,
                ctx,
                CombatTriggerKeyword::Poisonous(i64::from(n).into()),
            );
            Ok(())
        }
        v2::KeywordKind::Rampage => {
            let n = extract_number(&spec.param, "v2:kw:rampage-param")?;
            push_combat_kw(
                program,
                ctx,
                CombatTriggerKeyword::Rampage(i64::from(n).into()),
            );
            Ok(())
        }
        v2::KeywordKind::Renown => {
            let n = extract_number(&spec.param, "v2:kw:renown-param")?;
            push_combat_kw(
                program,
                ctx,
                CombatTriggerKeyword::Renown(i64::from(n).into()),
            );
            Ok(())
        }

        // Counter & growth & transfer keywords
        v2::KeywordKind::Undying if spec.param.is_none() => {
            program
                .triggered
                .push(CounterKeyword::Undying.ability(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Persist if spec.param.is_none() => {
            program
                .triggered
                .push(CounterKeyword::Persist.ability(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Vanishing => {
            let n = match &spec.param {
                None => None,
                Some(v2::KeywordParam::Number(v)) => Some(u32::from(*v)),
                _ => return Err(LoweringError::Unsupported("v2:kw:vanishing-param")),
            };
            program
                .triggered
                .push(CounterKeyword::Vanishing(n).ability(ctx.alloc_triggered_id()));
            program
                .triggered
                .push(CounterKeyword::VanishingLastCounter.ability(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Fading => {
            let n = extract_number(&spec.param, "v2:kw:fading-param")?;
            program
                .triggered
                .push(CounterKeyword::Fading(u32::from(n)).ability(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Evolve if spec.param.is_none() => {
            program
                .triggered
                .push(GrowthKeyword::Evolve.ability(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Mentor if spec.param.is_none() => {
            program
                .triggered
                .push(GrowthKeyword::Mentor.ability(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Increment if spec.param.is_none() => {
            program
                .triggered
                .push(GrowthKeyword::Increment.ability(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Modular => {
            let n = extract_number(&spec.param, "v2:kw:modular-param")?;
            let tk = TransferKeyword::Modular(u32::from(n));
            if let Some(entry) = tk.entry() {
                program.entry_counter_abilities.push(entry);
            }
            program.triggered.push(tk.ability(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Graft => {
            let n = extract_number(&spec.param, "v2:kw:graft-param")?;
            let tk = TransferKeyword::Graft(u32::from(n));
            if let Some(entry) = tk.entry() {
                program.entry_counter_abilities.push(entry);
            }
            program.triggered.push(tk.ability(ctx.alloc_triggered_id()));
            Ok(())
        }
        v2::KeywordKind::Soulshift => {
            let n = extract_number(&spec.param, "v2:kw:soulshift-param")?;
            program
                .triggered
                .push(TransferKeyword::Soulshift(u32::from(n)).ability(ctx.alloc_triggered_id()));
            Ok(())
        }

        // Token keywords (Afterlife, Fabricate, Living Weapon, For Mirrodin!, Job Select)
        v2::KeywordKind::Afterlife => {
            let n = extract_number(&spec.param, "v2:kw:afterlife-param")?;
            let ab = TokenKeyword::Afterlife
                .ability(ctx.alloc_triggered_id(), i64::from(n).into(), ctx.vocab)
                .map_err(|_| LoweringError::Unsupported("v2:kw:afterlife-vocab"))?;
            program.triggered.push(ab);
            Ok(())
        }
        v2::KeywordKind::Fabricate => {
            let n = extract_number(&spec.param, "v2:kw:fabricate-param")?;
            let ab = TokenKeyword::Fabricate
                .ability(ctx.alloc_triggered_id(), i64::from(n).into(), ctx.vocab)
                .map_err(|_| LoweringError::Unsupported("v2:kw:fabricate-vocab"))?;
            program.triggered.push(ab);
            Ok(())
        }
        v2::KeywordKind::LivingWeapon
        | v2::KeywordKind::ForMirrodin
        | v2::KeywordKind::JobSelect
            if spec.param.is_none() =>
        {
            let (pt, color, labels): (i64, Option<Color>, &[&str]) = match spec.kind {
                v2::KeywordKind::LivingWeapon => (0, Some(Color::Black), &["Phyrexian", "Germ"]),
                v2::KeywordKind::ForMirrodin => (2, Some(Color::Red), &["Rebel"]),
                _ => (1, None, &["Hero"]),
            };
            let types = labels
                .iter()
                .map(|word| ctx.vocab.resolve_label(word))
                .collect::<Option<Vec<_>>>()
                .ok_or(LoweringError::Unsupported("v2:kw:token-subtype-missing"))?;
            let mut token =
                Program::creature(None, pt.into(), pt.into(), types.iter().copied().collect());
            token.color_indicator = color.map(|c| BTreeSet::from([c]));
            let definition = TokenDefinition::new(
                token,
                TokenName::Default {
                    ordered_subtypes: types.into_iter().map(TokenSubtype::Creature).collect(),
                },
                ctx.vocab,
            )
            .map_err(|_| LoweringError::Unsupported("v2:kw:invalid-token-def"))?;
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::enters(ObjectFilter::self_object()),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        MutateObjectSpec::create_token_and_attach(Box::new(definition)),
                    ))],
                ),
            });
            Ok(())
        }

        // Toxic / Bloodthirst / Sunburst / Devour / Amplify / Ravenous / Tribute / Riot / Unleash
        v2::KeywordKind::Toxic => {
            let n = extract_number(&spec.param, "v2:kw:toxic-param")?;
            program.printed_toxic = Some(match &program.printed_toxic {
                Some(prev) => prev.plus(&i64::from(n).into()),
                None => i64::from(n).into(),
            });
            Ok(())
        }
        v2::KeywordKind::Bloodthirst => match &spec.param {
            Some(v2::KeywordParam::VariableX) => {
                program
                    .entry_counter_abilities
                    .push(EntryCounterAbility::BloodthirstX);
                Ok(())
            }
            Some(v2::KeywordParam::Number(n)) => {
                program
                    .entry_counter_abilities
                    .push(EntryCounterAbility::Bloodthirst(i64::from(*n).into()));
                Ok(())
            }
            _ => Err(LoweringError::Unsupported("v2:kw:bloodthirst-param")),
        },
        v2::KeywordKind::Sunburst if spec.param.is_none() => {
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::Sunburst);
            Ok(())
        }
        v2::KeywordKind::Devour => {
            let n = extract_positive_u8(&spec.param, "v2:kw:devour-param")?;
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::Devour(n));
            Ok(())
        }
        v2::KeywordKind::Amplify if !is_spell => {
            let n = extract_positive_u8(&spec.param, "v2:kw:amplify-param")?;
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::Amplify(n));
            Ok(())
        }
        v2::KeywordKind::Ravenous if spec.param.is_none() => {
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::ChosenXPlusOne);
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::self_enters(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::BranchCondition {
                            condition: Box::new(EngineConditionExpr::Resolution(Box::new(
                                ResolutionCondition {
                                    left: EngineValueExpr::EventValue(
                                        EventValueKind::EnteredKickerX,
                                    ),
                                    comparison: Comparison::GreaterOrEqual,
                                    right: EngineValueExpr::literal(5),
                                },
                            ))),
                            destination: 2,
                        },
                        Instruction::DrawCards(Box::new(DrawCardsSpec::simple(
                            PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            EngineValueExpr::literal(1),
                        ))),
                    ],
                ),
            });
            Ok(())
        }
        v2::KeywordKind::Tribute if !is_spell => {
            let n = extract_positive_u8(&spec.param, "v2:kw:tribute-param")?;
            let ProgramKind::Permanent(def) = &mut program.kind else {
                return Err(LoweringError::Unsupported("v2:kw:tribute-non-permanent"));
            };
            if def.entry_tap.replace(EntryTap::Tribute(n)).is_some() {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-entry-tap"));
            }
            Ok(())
        }
        v2::KeywordKind::Riot if spec.param.is_none() && !is_spell => {
            let ProgramKind::Permanent(def) = &mut program.kind else {
                return Err(LoweringError::Unsupported("v2:kw:riot-non-permanent"));
            };
            if def.entry_tap.replace(EntryTap::Riot).is_some() {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-entry-tap"));
            }
            Ok(())
        }
        v2::KeywordKind::Unleash if spec.param.is_none() && !is_spell => {
            let ProgramKind::Permanent(def) = &mut program.kind else {
                return Err(LoweringError::Unsupported("v2:kw:unleash-non-permanent"));
            };
            if def.entry_tap.replace(EntryTap::Unleash).is_some() {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-entry-tap"));
            }
            push_static(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        plus_one_counter: true,
                        ..PermanentSelector::default()
                    },
                    parts: StaticParts(vec![StaticPart::BlockingRestriction(
                        BlockingRestriction::CannotBlock,
                    )]),
                },
            );
            Ok(())
        }

        // Soulbond / Exploit / Myriad / Enlist / Provoke / Hideaway / Absorb / LivingMetal / Ascend
        v2::KeywordKind::Soulbond if spec.param.is_none() => {
            for entrant_only in [false, true] {
                program.triggered.push(TriggeredAbility {
                    id: ctx.alloc_triggered_id(),
                    condition: TriggerSpec::keyword(TriggerKeywordEvent::Soulbond { entrant_only }),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::MutateObject(Box::new(
                            MutateObjectSpec::soulbond_pair(entrant_only),
                        ))],
                    ),
                });
            }
            Ok(())
        }
        v2::KeywordKind::Exploit if spec.param.is_none() => {
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::self_enters(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 1,
                        },
                        Instruction::ZoneOp(Box::new(ZoneOpSpec::sacrifice(
                            AffectedPlayers::One(ResolvingPlayer::Controller),
                            EngineValueExpr::literal(1),
                            PermanentTypeFilter {
                                any_types: BTreeSet::new(),
                                required_types: BTreeSet::from([CardType::Creature]),
                                forbidden_types: BTreeSet::new(),
                                object_filter: None,
                            },
                        ))),
                    ],
                ),
            });
            Ok(())
        }
        v2::KeywordKind::Myriad if spec.param.is_none() => {
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::attacks(
                    ObjectFilter::self_object(),
                    AttackTriggerMode::Individual {
                        capture_defender: true,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::MutateObject(Box::new(MutateObjectSpec::myriad())),
                        Instruction::ScheduleDelayedStepTrigger {
                            step: Step::EndCombat,
                            own_step_only: false,
                            body: Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![Instruction::ZoneOp(Box::new(ZoneOpSpec::move_all(
                                    ObjectQuery::CreatedTokens,
                                    ZoneDest::exile(),
                                )))],
                            )),
                        },
                    ],
                ),
            });
            Ok(())
        }
        v2::KeywordKind::Enlist if spec.param.is_none() && !is_spell => {
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::self_attacks(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        MutateObjectSpec::enlist(),
                    ))],
                ),
            });
            Ok(())
        }
        v2::KeywordKind::Provoke if spec.param.is_none() && !is_spell => {
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::attacks(
                    ObjectFilter::self_object(),
                    AttackTriggerMode::Individual {
                        capture_defender: true,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::filtered_permanent(ObjectFilter::creature().opposing()),
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 1,
                        },
                        Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                            ObjectQuery::Targets,
                            ObjectMutation::TapState(TapStateOp::Set(false)),
                        ))),
                        Instruction::ApplyContinuous(Box::new(
                            ApplyContinuousSpec::resolved_parts(
                                ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                    BlockingRestriction::MustBlockSource,
                                )]),
                                ResolvedDuration::UntilEndOfTurn,
                            ),
                        )),
                    ],
                ),
            });
            Ok(())
        }
        v2::KeywordKind::Hideaway if !is_spell => {
            let n = extract_positive_u8(&spec.param, "v2:kw:hideaway-param")?;
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::self_enters(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    ZoneOpSpec::look_top_choose_instructions(
                        ResolvingPlayer::Controller,
                        ResolvingPlayer::Controller,
                        EngineValueExpr::literal(i64::from(n)),
                        None,
                        1,
                        false,
                        LookTopKeepDest::ExileLinked {
                            face_down: true,
                            playable: false,
                            free: false,
                            cast_only: false,
                            any_color: false,
                            duration: ResolvedDuration::NoStatedDuration,
                        },
                        LookTopRestDest::BottomRandom,
                        false,
                        false,
                        false,
                        None,
                    ),
                ),
            });
            push_static(
                program,
                StaticContinuousAbility::CastPermission {
                    id: ctx.alloc_static_id(),
                    permission: CastPermissionRule::LookAtSourceExiledCards,
                },
            );
            Ok(())
        }
        v2::KeywordKind::Absorb if !is_spell => {
            let n = extract_positive_u8(&spec.param, "v2:kw:absorb-param")?;
            push_static(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ctx.alloc_static_id(),
                    rule: StaticDamageRule::ModifyDamage {
                        source: None,
                        source_controller: PlayerRelation::AnyParticipant,
                        recipient: StaticDamageRecipient::SelfObject,
                        kind: StaticDamageKind::Any,
                        op: StaticDamageOp::PreventUpTo(n),
                    },
                },
            );
            Ok(())
        }
        v2::KeywordKind::LivingMetal if spec.param.is_none() && !is_spell => {
            push_static(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        source_condition: Some(StaticSourceCondition::ControllersTurn),
                        ..PermanentSelector::default()
                    },
                    parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                        subtypes: SubtypeOperation::Keep,
                    })]),
                },
            );
            Ok(())
        }
        v2::KeywordKind::Ascend if spec.param.is_none() => {
            if is_spell {
                let ProgramKind::Spell(spell) = &mut program.kind else {
                    return Err(LoweringError::Unsupported("v2:kw:ascend-not-spell"));
                };
                let ascend_op = Instruction::MutatePlayer(Box::new(MutatePlayerSpec::ascend()));
                if let Some(modes) = &mut spell.modes {
                    if let Some(prefix) = &mut modes.prefix {
                        prefix.instructions.insert(0, ascend_op);
                        for op in &mut prefix.instructions[1..] {
                            op.shift_jumps(1, 0);
                        }
                    } else {
                        modes.prefix = Some(Box::new(ExecutableBody::new(
                            BodyTarget::None,
                            vec![ascend_op],
                        )));
                    }
                } else {
                    spell.body.instructions.insert(0, ascend_op);
                    for op in &mut spell.body.instructions[1..] {
                        op.shift_jumps(1, 0);
                    }
                }
                Ok(())
            } else {
                push_static(
                    program,
                    StaticContinuousAbility::Modifier {
                        id: ctx.alloc_static_id(),
                        rule: StaticModifierRule::Ascend,
                    },
                );
                Ok(())
            }
        }

        // Outlast / LevelUp / Scavenge / Transmute / Transfigure / Reinforce / Encore / Embalm / Eternalize
        v2::KeywordKind::Outlast => {
            let (mana, hybrids, snow) =
                extract_mana_with_hybrids_snow(&spec.param, "v2:kw:outlast-cost")?;
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz)]);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: mana,
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                        ObjectQuery::Source,
                        ObjectMutation::Counters(CounterOp::Add {
                            kind: EngineCounterKind::PlusOne,
                            amount: EngineValueExpr::literal(1),
                        }),
                    )))],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        v2::KeywordKind::LevelUp => {
            let (mana, hybrids, snow) =
                extract_mana_with_hybrids_snow(&spec.param, "v2:kw:level-up-cost")?;
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz)]);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: mana,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                        ObjectQuery::Source,
                        ObjectMutation::Counters(CounterOp::Add {
                            kind: EngineCounterKind::Level,
                            amount: EngineValueExpr::literal(1),
                        }),
                    )))],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        v2::KeywordKind::Scavenge => {
            let (mana, hybrids, snow) =
                extract_mana_with_hybrids_snow(&spec.param, "v2:kw:scavenge-cost")?;
            let power = match program
                .permanent()
                .and_then(|p| p.printed_pt.as_ref())
                .map(|pt| &pt.power)
            {
                Some(PrintedStat::Number(n)) => n.clone(),
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:kw:scavenge-non-numeric-power",
                    ));
                }
            };
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz)]);
            }
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: mana,
                source_cost: SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: true,
                },
                body: ExecutableBody::new(
                    BodyTarget::SINGLE_CREATURE,
                    vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                        ObjectQuery::Targets,
                        ObjectMutation::Counters(CounterOp::Add {
                            kind: EngineCounterKind::PlusOne,
                            amount: EngineValueExpr::Literal(power),
                        }),
                    )))],
                ),
            });
            Ok(())
        }
        v2::KeywordKind::Transmute => {
            let (mana, hybrids, snow) =
                extract_mana_with_hybrids_snow(&spec.param, "v2:kw:transmute-cost")?;
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz)]);
            }
            program
                .activated
                .push(ActivatedAbility::transmute(id, mana, program.mana_value()));
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        v2::KeywordKind::Transfigure if !is_spell => {
            let (mana, hybrids, snow) =
                extract_mana_with_hybrids_snow(&spec.param, "v2:kw:transfigure-cost")?;
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if let Some(nz) = NonZeroU8::new(snow) {
                program
                    .extra_activation_costs
                    .insert(id, vec![ExtraActivationCost::PaySnowMana(nz)]);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: mana,
                source_cost: SourceCost::Sacrifice,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::SearchLibrary {
                        filter: Box::new(
                            LibrarySearch::to_battlefield(
                                SearchCardMatcher::CardTypes(BTreeSet::from([CardType::Creature])),
                                false,
                            )
                            .with_destination(SearchDestination::Battlefield {
                                tapped: false,
                                post_entry: SearchBattlefieldPostEntry::Transfigure,
                            })
                            .with_mana_value(SearchManaValue::Exact(program.mana_value())),
                        ),
                    }],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        v2::KeywordKind::Reinforce => {
            let Some(v2::KeywordParam::NumberAndCost(n, costs)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:reinforce-param"));
            };
            if *n == 0 {
                return Err(LoweringError::Unsupported("v2:kw:reinforce-zero"));
            }
            let mana = extract_fixed_mana_from_costs(costs, "v2:kw:reinforce-cost")?;
            program.activated.push(ActivatedAbility::reinforce(
                ctx.alloc_activated_id(),
                mana,
                i64::from(*n).into(),
            ));
            Ok(())
        }
        v2::KeywordKind::Encore => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:encore-cost")?;
            let id = ctx.alloc_activated_id();
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: mana,
                source_cost: SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: true,
                },
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::StackOp(Box::new(StackOpSpec::encore()))],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        v2::KeywordKind::Embalm | v2::KeywordKind::Eternalize if !is_spell => {
            let Some(v2::KeywordParam::Cost(costs)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:embalm-param"));
            };
            let (mana_cost, source_cost, hybrids, extra_costs) =
                lower_activation_cost_parts(costs, false, ctx)?;
            if source_cost != SourceCost::None {
                return Err(LoweringError::Unsupported("v2:kw:embalm-source-cost"));
            }
            let zombie = ctx
                .vocab
                .resolve_label("Zombie")
                .ok_or(LoweringError::Unsupported("v2:kw:zombie-subtype-missing"))?;
            let is_eternalize = spec.kind == v2::KeywordKind::Eternalize;
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if !extra_costs.is_empty() {
                program.extra_activation_costs.insert(id, extra_costs);
            }
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost,
                source_cost: SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: true,
                },
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        MutateObjectSpec::create_token_copy(
                            ObjectQuery::Source,
                            Box::new(TokenCopySpec {
                                count: EngineValueExpr::literal(1),
                                recipients: TokenRecipients::Controller,
                                enters_tapped: false,
                                enters_attacking: false,
                                haste: false,
                                sacrifice_at_end_step: false,
                                exile_at_end_step: false,
                                nonlegendary: false,
                                extra_types: BTreeSet::new(),
                                extra_subtypes: BTreeSet::from([zombie]),
                                extra_keywords: BTreeSet::new(),
                                override_pt: is_eternalize.then_some((4.into(), 4.into())),
                                extra_colors: BTreeSet::new(),
                                override_colors: Some(BTreeSet::from([if is_eternalize {
                                    Color::Black
                                } else {
                                    Color::White
                                }])),
                                strip_mana_cost: true,
                                defender_scope: AttackingDefenderScope::AnyLegal,
                                extra_activated: Vec::new(),
                                extra_triggered: Vec::new(),
                                extra_static: Vec::new(),
                                extra_mana: Vec::new(),
                                extra_counters: Vec::new(),
                                keep_name: false,
                            }),
                        ),
                    ))],
                ),
            });
            Ok(())
        }

        // Echo / CumulativeUpkeep / Recover / Ripple / Demonstrate
        v2::KeywordKind::Echo => {
            let mana = match &spec.param {
                None => program
                    .printed_mana_cost()
                    .cloned()
                    .ok_or(LoweringError::Unsupported("v2:kw:echo-no-cost"))?,
                Some(_) => extract_single_fixed_mana(&spec.param, "v2:kw:echo-cost")?,
            };
            program
                .triggered
                .push(UpkeepKeyword::Echo.ability(ctx.alloc_triggered_id(), mana));
            Ok(())
        }
        v2::KeywordKind::CumulativeUpkeep => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:cum-upkeep-non-mana")?;
            program
                .triggered
                .push(UpkeepKeyword::CumulativeUpkeep.ability(ctx.alloc_triggered_id(), mana));
            Ok(())
        }
        v2::KeywordKind::Recover => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:recover-cost")?;
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec {
                    event: TriggerEvent::Dies {
                        subject: ObjectFilter::creature().controlled(),
                        mode: DiesTriggerMode::Creature,
                    },
                    zone: TriggerZone::Graveyard,
                    intervening_if: None,
                },
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::PromptChoice(Box::new(
                            PromptChoiceSpec::offer_dynamic_mana_payment(
                                ResolvingPlayer::Controller,
                                mana,
                                EngineValueExpr::Literal(0.into()),
                                1,
                                3,
                            ),
                        )),
                        Instruction::ZoneOp(Box::new(ZoneOpSpec::move_all(
                            ObjectQuery::Source,
                            ZoneDest::Hand,
                        ))),
                        Instruction::Jump { destination: 4 },
                        Instruction::ZoneOp(Box::new(ZoneOpSpec::move_all(
                            ObjectQuery::Source,
                            ZoneDest::exile(),
                        ))),
                    ],
                ),
            });
            Ok(())
        }
        v2::KeywordKind::Ripple => {
            let n = extract_positive_u8(&spec.param, "v2:kw:ripple-param")?;
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::self_cast(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        MutatePlayerSpec::ripple(n),
                    ))],
                ),
            });
            Ok(())
        }
        v2::KeywordKind::Demonstrate if spec.param.is_none() => {
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::self_cast(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        MutatePlayerSpec::demonstrate(),
                    ))],
                ),
            });
            Ok(())
        }

        // Firebending / Offspring / Squad
        v2::KeywordKind::Firebending => {
            let amount = match &spec.param {
                Some(v2::KeywordParam::Number(n)) if *n > 0 => {
                    EngineValueExpr::literal(i64::from(*n))
                }
                Some(v2::KeywordParam::VariableX) => EngineValueExpr::ObjectStat {
                    subject: ValueObjectRef::AbilitySource,
                    stat: ObjectStatKind::Power,
                },
                Some(v2::KeywordParam::VariableXBound(expr)) => {
                    crate::v2_lowering::value::lower_value_expr(expr, ctx)?
                }
                _ => return Err(LoweringError::Unsupported("v2:kw:firebending-param")),
            };
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::attacks(
                    ObjectFilter::self_object(),
                    AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(
                        MutatePlayerSpec::add_mana(
                            ResolvingPlayer::Controller,
                            EffectManaYield::RetainedUntilEndOfCombat {
                                kind: ManaType::Red,
                                amount,
                            },
                        ),
                    ))],
                ),
            });
            Ok(())
        }
        v2::KeywordKind::Offspring if !is_spell => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:offspring-cost")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Kicker(mana));
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::new(TriggerEvent::Enters {
                    subject: ObjectFilter::self_object(),
                    from_zone: TriggerFromZone::Any,
                    cast_origin: Some(CastEntryKind::Kicked),
                    attached_to_creature: false,
                }),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        MutateObjectSpec::create_token_copy(
                            ObjectQuery::Source,
                            Box::new(TokenCopySpec {
                                count: EngineValueExpr::literal(1),
                                recipients: TokenRecipients::Controller,
                                enters_tapped: false,
                                enters_attacking: false,
                                haste: false,
                                sacrifice_at_end_step: false,
                                exile_at_end_step: false,
                                nonlegendary: false,
                                extra_types: BTreeSet::new(),
                                extra_subtypes: BTreeSet::new(),
                                extra_keywords: BTreeSet::new(),
                                override_pt: Some((1.into(), 1.into())),
                                extra_colors: BTreeSet::new(),
                                override_colors: None,
                                strip_mana_cost: false,
                                defender_scope: AttackingDefenderScope::AnyLegal,
                                extra_activated: Vec::new(),
                                extra_triggered: Vec::new(),
                                extra_static: Vec::new(),
                                extra_mana: Vec::new(),
                                extra_counters: Vec::new(),
                                keep_name: false,
                            }),
                        ),
                    ))],
                ),
            });
            Ok(())
        }
        v2::KeywordKind::Squad if !is_spell => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:squad-cost")?;
            let idx = program.additional_cast_costs.len();
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Multikicker(mana));
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::new(TriggerEvent::Enters {
                    subject: ObjectFilter::self_object(),
                    from_zone: TriggerFromZone::Any,
                    cast_origin: Some(CastEntryKind::Kicked),
                    attached_to_creature: false,
                }),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        MutateObjectSpec::create_token_copy(
                            ObjectQuery::Source,
                            Box::new(TokenCopySpec {
                                count: EngineValueExpr::event(EventValueKind::EnteredKickerCount(
                                    idx,
                                )),
                                recipients: TokenRecipients::Controller,
                                enters_tapped: false,
                                enters_attacking: false,
                                haste: false,
                                sacrifice_at_end_step: false,
                                exile_at_end_step: false,
                                nonlegendary: false,
                                extra_types: BTreeSet::new(),
                                extra_subtypes: BTreeSet::new(),
                                extra_keywords: BTreeSet::new(),
                                override_pt: None,
                                extra_colors: BTreeSet::new(),
                                override_colors: None,
                                strip_mana_cost: false,
                                defender_scope: AttackingDefenderScope::AnyLegal,
                                extra_activated: Vec::new(),
                                extra_triggered: Vec::new(),
                                extra_static: Vec::new(),
                                extra_mana: Vec::new(),
                                extra_counters: Vec::new(),
                                keep_name: false,
                            }),
                        ),
                    ))],
                ),
            });
            Ok(())
        }

        // Spell-flag keywords: Gift, Casualty, Conspire, Dredge, Assist, Cipher, Splice, Escalate, Spree, Tiered
        v2::KeywordKind::Gift => {
            let Some(v2::KeywordParam::Gift(g)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:gift-param"));
            };
            let kind = match g {
                v2::GiftKind::Card => GiftKind::Card,
                v2::GiftKind::Treasure => GiftKind::Treasure,
                v2::GiftKind::Food => GiftKind::Food,
                v2::GiftKind::TappedFish => GiftKind::TappedFish,
                v2::GiftKind::ExtraTurn => GiftKind::ExtraTurn,
                v2::GiftKind::Octopus => {
                    return Err(LoweringError::Unsupported("v2:kw:gift-octopus"));
                }
            };
            if program.gift.replace(kind).is_some() {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-gift"));
            }
            Ok(())
        }
        v2::KeywordKind::Casualty => {
            let n = extract_positive_u8(&spec.param, "v2:kw:casualty-param")?;
            if program.casualty.replace(n).is_some() {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-casualty"));
            }
            Ok(())
        }
        v2::KeywordKind::Conspire if spec.param.is_none() => {
            program.conspire = true;
            Ok(())
        }
        v2::KeywordKind::Dredge => {
            let n = extract_positive_u8(&spec.param, "v2:kw:dredge-param")?;
            if program.dredge.replace(n).is_some() {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-dredge"));
            }
            Ok(())
        }
        v2::KeywordKind::Assist if spec.param.is_none() => {
            program.assist = true;
            Ok(())
        }
        v2::KeywordKind::Cipher if spec.param.is_none() => {
            program.cipher = true;
            Ok(())
        }
        v2::KeywordKind::Splice if is_spell => {
            let Some(v2::KeywordParam::FilterAndCost(flt, costs)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:splice-param"));
            };
            let quality =
                if flt.subtypes.len() == 1 && flt.subtypes[0].eq_ignore_ascii_case("arcane") {
                    SpliceQuality::Arcane
                } else if flt.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY)
                    && flt.subtypes.is_empty()
                {
                    SpliceQuality::InstantOrSorcery
                } else {
                    return Err(LoweringError::Unsupported("v2:kw:splice-quality"));
                };
            let cost = extract_fixed_mana_from_costs(costs, "v2:kw:splice-cost")?;
            if !program.supports_splice_text() {
                return Err(LoweringError::Unsupported("v2:kw:splice-unsupported-text"));
            }
            if program
                .splice
                .replace(SpliceAbility { quality, cost })
                .is_some()
            {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-splice"));
            }
            Ok(())
        }
        v2::KeywordKind::Escalate if is_spell => {
            let mana = extract_single_fixed_mana(&spec.param, "v2:kw:escalate-cost")?;
            let ProgramKind::Spell(spell) = &mut program.kind else {
                return Err(LoweringError::Unsupported("v2:kw:escalate-non-spell"));
            };
            let Some(modes) = spell.modes.as_mut() else {
                return Err(LoweringError::Unsupported("v2:kw:escalate-no-modes"));
            };
            modes.escalate_mana = Some(mana);
            Ok(())
        }
        v2::KeywordKind::Spree | v2::KeywordKind::Tiered if is_spell => {
            if let ProgramKind::Spell(spell) = &mut program.kind
                && let Some(modes) = spell.modes.as_mut()
                && !modes.mode_costs.is_empty()
            {
                if spec.kind == v2::KeywordKind::Tiered {
                    modes.minimum = 1;
                    modes.maximum = 1;
                }
                Ok(())
            } else {
                Err(LoweringError::Unsupported("v2:kw:spree-without-modes"))
            }
        }

        // Partner with <name>
        v2::KeywordKind::Partner => {
            let Some(v2::KeywordParam::PartnerName(partner_raw)) = &spec.param else {
                return Err(LoweringError::Unsupported("v2:kw:partner-param"));
            };
            let partner = EnglishCardName::new(partner_raw.clone())
                .map_err(|_| LoweringError::Unsupported("v2:kw:partner-bad-name"))?;
            program.printed_keywords.insert(KeywordAbility::Partner);
            let search = LibrarySearch::to_hand(SearchCardMatcher::AnyOfNames(vec![partner]))
                .with_searcher(ResolvingPlayer::TargetPlayer);
            program.triggered.push(TriggeredAbility {
                id: ctx.alloc_triggered_id(),
                condition: TriggerSpec::self_enters(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::SINGLE_PLAYER,
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::TargetPlayer,
                            instructions: 1,
                        },
                        Instruction::SearchLibrary {
                            filter: Box::new(search),
                        },
                    ],
                ),
            });
            Ok(())
        }

        // Explicit fail-closed diagnostics for all remaining or context-dependent keywords.
        v2::KeywordKind::Landwalk => {
            Err(LoweringError::Unsupported("v2:kw:landwalk-untyped-prefix"))
        }
        v2::KeywordKind::Offering => {
            Err(LoweringError::Unsupported("v2:kw:offering-untyped-prefix"))
        }
        v2::KeywordKind::Backup => Err(LoweringError::Unsupported("v2:kw:backup-needs-face-ctx")),
        v2::KeywordKind::ReadAhead => {
            Err(LoweringError::Unsupported("v2:kw:read-ahead-needs-saga"))
        }
        v2::KeywordKind::Prototype => {
            Err(LoweringError::Unsupported("v2:kw:prototype-needs-face-ctx"))
        }
        v2::KeywordKind::Aftermath => Err(LoweringError::Unsupported("v2:kw:aftermath-non-split")),
        v2::KeywordKind::Fuse => Err(LoweringError::Unsupported("v2:kw:fuse-non-split")),
        v2::KeywordKind::Disturb => Err(LoweringError::Unsupported("v2:kw:disturb-non-transform")),
        v2::KeywordKind::Overload => Err(LoweringError::Unsupported("v2:kw:overload")),
        v2::KeywordKind::Cleave => Err(LoweringError::Unsupported("v2:kw:cleave")),
        v2::KeywordKind::Companion => Err(LoweringError::Unsupported("v2:kw:companion")),
        v2::KeywordKind::Boast => Err(LoweringError::Unsupported("v2:kw:boast")),
        v2::KeywordKind::Compleated => Err(LoweringError::Unsupported("v2:kw:compleated")),
        v2::KeywordKind::Craft => Err(LoweringError::Unsupported("v2:kw:craft")),
        v2::KeywordKind::AuraSwap => Err(LoweringError::Unsupported("v2:kw:aura-swap")),
        v2::KeywordKind::Champion => Err(LoweringError::Unsupported("v2:kw:champion")),
        v2::KeywordKind::HiddenAgenda => Err(LoweringError::Unsupported("v2:kw:hidden-agenda")),
        v2::KeywordKind::Meld => Err(LoweringError::Unsupported("v2:kw:meld")),
        v2::KeywordKind::MoreThanMeetsTheEye => {
            Err(LoweringError::Unsupported("v2:kw:more-than-meets-the-eye"))
        }
        v2::KeywordKind::Teamwork => Err(LoweringError::Unsupported("v2:kw:teamwork")),
        v2::KeywordKind::Paradigm => Err(LoweringError::Unsupported("v2:kw:paradigm")),
        v2::KeywordKind::Haunt => Err(LoweringError::Unsupported("v2:kw:haunt")),
        v2::KeywordKind::Storied => Err(LoweringError::Unsupported("v2:kw:storied")),
        v2::KeywordKind::Epic => Err(LoweringError::Unsupported("v2:kw:epic")),
        _ => Err(LoweringError::Unsupported("v2:kw:unsupported-keyword")),
    }
}

fn push_combat_kw(program: &mut Program, ctx: &mut LowerCtx<'_>, kw: CombatTriggerKeyword) {
    program.triggered.push(TriggeredAbility::combat_keyword(
        ctx.alloc_triggered_id(),
        kw,
    ));
}

fn push_static(program: &mut Program, ab: StaticContinuousAbility) {
    if program.continuous.is_none() {
        program.continuous = Some(ab);
    } else {
        program.additional_battlefield_abilities.push(ab);
    }
}

fn set_conditional_cost(program: &mut Program, cc: ConditionalCastCost) -> LowerResult<()> {
    if program.conditional_cost.replace(cc).is_some() {
        return Err(LoweringError::Unsupported(
            "v2:kw:duplicate-conditional-cost",
        ));
    }
    Ok(())
}

fn extract_number(param: &Option<v2::KeywordParam>, err: &'static str) -> LowerResult<u16> {
    match param {
        Some(v2::KeywordParam::Number(n)) => Ok(*n),
        _ => Err(LoweringError::Unsupported(err)),
    }
}

fn extract_positive_u8(param: &Option<v2::KeywordParam>, err: &'static str) -> LowerResult<u8> {
    match param {
        Some(v2::KeywordParam::Number(n)) if *n > 0 && *n <= u16::from(u8::MAX) => Ok(*n as u8),
        _ => Err(LoweringError::Unsupported(err)),
    }
}

fn extract_fixed_mana_from_costs(costs: &[v2::Cost], err: &'static str) -> LowerResult<ManaCost> {
    let [v2::Cost::Mana(mc)] = costs else {
        return Err(LoweringError::Unsupported(err));
    };
    lower_v2_fixed_mana_cost(mc).map_err(|_| LoweringError::Unsupported(err))
}

fn extract_single_fixed_mana(
    param: &Option<v2::KeywordParam>,
    err: &'static str,
) -> LowerResult<ManaCost> {
    let Some(v2::KeywordParam::Cost(costs)) = param else {
        return Err(LoweringError::Unsupported(err));
    };
    extract_fixed_mana_from_costs(costs, err)
}

fn extract_mana_with_hybrids_snow(
    param: &Option<v2::KeywordParam>,
    err: &'static str,
) -> LowerResult<(ManaCost, Vec<spellwake_engine::mana::HybridSymbol>, u8)> {
    let Some(v2::KeywordParam::Cost(costs)) = param else {
        return Err(LoweringError::Unsupported(err));
    };
    let [v2::Cost::Mana(mc)] = costs.as_slice() else {
        return Err(LoweringError::Unsupported(err));
    };
    let (mana, x, hybrids, snow) =
        lower_v2_mana_cost(mc).map_err(|_| LoweringError::Unsupported(err))?;
    if x != 0 {
        return Err(LoweringError::Unsupported(err));
    }
    Ok((mana, hybrids, snow))
}

fn lower_kicker_cost(
    costs: &[v2::Cost],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<AdditionalCastCost> {
    if costs.is_empty() {
        return Err(LoweringError::Unsupported("v2:kw:kicker-empty"));
    }
    if let [v2::Cost::Mana(mc)] = costs {
        let (mana, x, hybrids, snow) = lower_v2_mana_cost(mc)?;
        if !hybrids.is_empty() {
            return Err(LoweringError::Unsupported("v2:kw:kicker-hybrid"));
        }
        if snow == 0 {
            return match x {
                0 => Ok(AdditionalCastCost::Kicker(mana)),
                1 => Ok(AdditionalCastCost::KickerX(mana)),
                _ => Err(LoweringError::Unsupported("v2:kw:kicker-multi-x")),
            };
        }
        if x == 0
            && let Some(nz) = NonZeroU8::new(snow)
        {
            return Ok(AdditionalCastCost::KickerExtra {
                mana,
                extra: vec![ExtraActivationCost::PaySnowMana(nz)],
            });
        }
        return Err(LoweringError::Unsupported("v2:kw:kicker-snow-x"));
    }

    let mut mana = ManaCost::free();
    let mut snow_mana: u8 = 0;
    let mut extra = Vec::new();
    for atom in costs {
        if let v2::Cost::Mana(mc) = atom {
            let (part, x, hybrids, snow) = lower_v2_mana_cost(mc)?;
            if x != 0 || !hybrids.is_empty() {
                return Err(LoweringError::Unsupported("v2:kw:kicker-complex-mana"));
            }
            merge_mana_cost(&mut mana, &part);
            snow_mana = snow_mana
                .checked_add(snow)
                .ok_or(LoweringError::Unsupported("v2:kw:kicker-snow-overflow"))?;
        } else {
            let lowered = lower_extra_cost_atom(atom, ctx)?;
            if is_battlefield_only_source_cost(&lowered) {
                return Err(LoweringError::Unsupported("v2:kw:kicker-source-cost"));
            }
            extra.push(lowered);
        }
    }
    if let Some(nz) = NonZeroU8::new(snow_mana) {
        extra.push(ExtraActivationCost::PaySnowMana(nz));
    }
    if extra.is_empty() {
        Ok(AdditionalCastCost::Kicker(mana))
    } else {
        Ok(AdditionalCastCost::KickerExtra { mana, extra })
    }
}

fn lower_flashback_keyword(
    param: &Option<v2::KeywordParam>,
    program: &mut Program,
) -> LowerResult<()> {
    let Some(v2::KeywordParam::Cost(costs)) = param else {
        return Err(LoweringError::Unsupported("v2:kw:flashback-param"));
    };
    if costs.is_empty() {
        return Err(LoweringError::Unsupported("v2:kw:flashback-empty"));
    }
    let mut mana = ManaCost::free();
    let mut x_symbols: u8 = 0;
    let mut life: u16 = 0;
    let mut sac_creatures: u8 = 0;
    let mut tap_white_creatures: u8 = 0;

    for atom in costs {
        match atom {
            v2::Cost::Mana(mc) => {
                let (part, x, hybrids, snow) = lower_v2_mana_cost(mc)?;
                if !hybrids.is_empty() || snow != 0 {
                    return Err(LoweringError::Unsupported("v2:kw:flashback-complex-mana"));
                }
                merge_mana_cost(&mut mana, &part);
                x_symbols = x_symbols
                    .checked_add(x)
                    .ok_or(LoweringError::Unsupported("v2:kw:flashback-x-overflow"))?;
            }
            v2::Cost::PayLife(v2::ValueExpr::Const(n)) if *n > 0 && life == 0 => {
                life = u16::try_from(*n)
                    .map_err(|_| LoweringError::Unsupported("v2:kw:flashback-life"))?;
            }
            v2::Cost::Sacrifice {
                count: v2::ValueExpr::Const(n),
                filter,
            } if *n > 0
                && *n <= i32::from(u8::MAX)
                && sac_creatures == 0
                && filter.all_types.union(filter.any_types) == AstCardTypeMask::CREATURE
                && filter.forbidden_types.is_empty()
                && filter.subtypes.is_empty()
                && filter.any_colors.is_empty() =>
            {
                sac_creatures = *n as u8;
            }
            v2::Cost::TapUntapped {
                count: v2::ValueExpr::Const(n),
                filter,
            } if *n > 0
                && *n <= i32::from(u8::MAX)
                && tap_white_creatures == 0
                && filter.all_types.union(filter.any_types) == AstCardTypeMask::CREATURE
                && filter.any_colors == ColorMask::WHITE
                && filter.forbidden_types.is_empty()
                && filter.subtypes.is_empty() =>
            {
                tap_white_creatures = *n as u8;
            }
            _ => return Err(LoweringError::Unsupported("v2:kw:flashback-non-mana")),
        }
    }

    if program.flashback.replace(mana).is_some() {
        return Err(LoweringError::Unsupported("v2:kw:duplicate-flashback"));
    }
    program.flashback_x_mana_symbols = x_symbols;
    program.flashback_life = life;
    program.flashback_sacrifice_creatures = sac_creatures;
    program.flashback_tap_white_creatures = tap_white_creatures;
    Ok(())
}

fn lower_escape_cost(costs: &[v2::Cost]) -> LowerResult<(ManaCost, u16)> {
    let mut mana: Option<ManaCost> = None;
    let mut exile: Option<u16> = None;
    for atom in costs {
        match atom {
            v2::Cost::Mana(mc) if mana.is_none() => {
                mana = Some(lower_v2_fixed_mana_cost(mc)?);
            }
            v2::Cost::Exile {
                count: v2::ValueExpr::Const(n),
                filter,
                from,
            } if exile.is_none()
                && *n > 0
                && filter.flags.contains(v2::FilterFlags::EXCLUDE_SELF)
                && from
                    .first()
                    .or_else(|| filter.zones.first())
                    .is_some_and(|z| z.zone == v2::ZoneKind::Graveyard) =>
            {
                let qty = u16::try_from(*n)
                    .map_err(|_| LoweringError::Unsupported("v2:kw:escape-count"))?;
                exile = Some(qty);
            }
            _ => return Err(LoweringError::Unsupported("v2:kw:escape-cost")),
        }
    }
    match (mana, exile) {
        (Some(m), Some(e)) => Ok((m, e)),
        _ => Err(LoweringError::Unsupported("v2:kw:escape-incomplete")),
    }
}

fn lower_enchant_filter(
    flt: &v2::ObjectFilter,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EnchantRestriction> {
    if !flt.zones.is_empty() || flt.any_spell || flt.any_card || flt.is_ability {
        return Err(LoweringError::Unsupported("v2:kw:enchant-non-permanent"));
    }
    let obj = lower_object_filter(flt, ctx)?;
    if obj == ObjectFilter::any() {
        return Ok(EnchantRestriction::Permanent);
    }
    if obj == ObjectFilter::creature() {
        return Ok(EnchantRestriction::Creature);
    }
    if obj == ObjectFilter::creature().controlled() {
        return Ok(EnchantRestriction::CreatureYouControl);
    }
    if obj == ObjectFilter::creature().opposing() {
        return Ok(EnchantRestriction::CreatureAnOpponentControls);
    }
    if obj == ObjectFilter::artifact() {
        return Ok(EnchantRestriction::Artifact);
    }
    if obj == ObjectFilter::enchantment() {
        return Ok(EnchantRestriction::Enchantment);
    }
    if obj == ObjectFilter::land() {
        return Ok(EnchantRestriction::Land);
    }
    Ok(EnchantRestriction::Matching(obj))
}

fn ensure_controlled_creature_equip_filter(filter: ObjectFilter) -> ObjectFilter {
    let apply_clause = |mut c: spellwake_engine::program::ObjectClause| {
        if c.controller == ControllerFilter::Any {
            c.controller = ControllerFilter::You;
        }
        if c.required_types.is_empty() && c.any_types.is_empty() {
            c.required_types = spellwake_engine::program::CardTypeMask::CREATURE;
        }
        c
    };
    match filter {
        ObjectFilter::Clause(c) => ObjectFilter::Clause(apply_clause(c)),
        ObjectFilter::Either(a, b) => ObjectFilter::Either(apply_clause(a), apply_clause(b)),
    }
}

fn lower_protection_param(
    param: &v2::KeywordParam,
    ctx: &LowerCtx<'_>,
) -> LowerResult<Vec<KeywordAbility>> {
    match param {
        v2::KeywordParam::Everything => Ok(vec![KeywordAbility::ProtectionFrom(
            ProtectionFromQuality::Everything,
        )]),
        v2::KeywordParam::Color(mask) => lower_color_mask_to_protections(*mask),
        v2::KeywordParam::Filter(flt) => lower_protection_filter(flt, ctx),
        _ => Err(LoweringError::Unsupported("v2:kw:unsupported-protection")),
    }
}

fn lower_color_mask_to_protections(mask: ColorMask) -> LowerResult<Vec<KeywordAbility>> {
    let mut out = Vec::new();
    if (mask.0 & (ColorMask::WHITE).0) != 0 {
        out.push(KeywordAbility::ProtectionFromWhite);
    }
    if (mask.0 & (ColorMask::BLUE).0) != 0 {
        out.push(KeywordAbility::ProtectionFromBlue);
    }
    if (mask.0 & (ColorMask::BLACK).0) != 0 {
        out.push(KeywordAbility::ProtectionFromBlack);
    }
    if (mask.0 & (ColorMask::RED).0) != 0 {
        out.push(KeywordAbility::ProtectionFromRed);
    }
    if (mask.0 & (ColorMask::GREEN).0) != 0 {
        out.push(KeywordAbility::ProtectionFromGreen);
    }
    if out.is_empty() {
        return Err(LoweringError::Unsupported("v2:kw:empty-protection-color"));
    }
    Ok(out)
}

fn lower_protection_filter(
    flt: &v2::ObjectFilter,
    ctx: &LowerCtx<'_>,
) -> LowerResult<Vec<KeywordAbility>> {
    if let [v2::FilterPredicate::AnyOf(parts)] = flt.predicates.as_slice()
        && flt.all_types.is_empty()
        && flt.any_types.is_empty()
        && flt.subtypes.is_empty()
        && flt.any_colors.is_empty()
        && flt.color_arity.is_none()
    {
        let mut out = Vec::new();
        for p in parts {
            out.extend(lower_protection_filter(p, ctx)?);
        }
        return if out.is_empty() {
            Err(LoweringError::Unsupported("v2:kw:empty-protection"))
        } else {
            Ok(out)
        };
    }

    if !flt.forbidden_types.is_empty()
        || !flt.required_supertypes.is_empty()
        || !flt.forbidden_supertypes.is_empty()
        || !flt.forbidden_subtypes.is_empty()
        || !flt.forbidden_colors.is_empty()
        || flt.controller.is_some()
        || flt.owner.is_some()
        || !flt.zones.is_empty()
        || flt.combat.is_some()
        || flt.tapped.is_some()
        || flt.token.is_some()
        || !flt.required_keywords.is_empty()
        || !flt.forbidden_keywords.is_empty()
        || !flt.stat_bounds.is_empty()
        || !flt.flags.is_empty()
        || !flt.predicates.is_empty()
    {
        return Err(LoweringError::Unsupported(
            "v2:kw:complex-protection-filter",
        ));
    }

    if !flt.any_colors.is_empty()
        && flt.all_types.is_empty()
        && flt.any_types.is_empty()
        && flt.subtypes.is_empty()
        && flt.color_arity.is_none()
    {
        return lower_color_mask_to_protections(flt.any_colors);
    }

    if let Some(arity) = flt.color_arity
        && flt.all_types.is_empty()
        && flt.any_types.is_empty()
        && flt.subtypes.is_empty()
        && flt.any_colors.is_empty()
    {
        return match arity {
            ColorArity::Multicolored => Ok(vec![KeywordAbility::ProtectionFromMulticolored]),
            ColorArity::Monocolored => Ok(vec![KeywordAbility::ProtectionFrom(
                ProtectionFromQuality::Monocolored,
            )]),
            ColorArity::Colorless => Ok(vec![KeywordAbility::ProtectionFrom(
                ProtectionFromQuality::Colorless,
            )]),
            ColorArity::Colored => lower_color_mask_to_protections(
                ColorMask::WHITE
                    .union(ColorMask::BLUE)
                    .union(ColorMask::BLACK)
                    .union(ColorMask::RED)
                    .union(ColorMask::GREEN),
            ),
        };
    }

    if flt.subtypes.is_empty() && flt.any_colors.is_empty() && flt.color_arity.is_none() {
        let mask = flt.all_types.union(flt.any_types);
        if !mask.is_empty() && flt.all_types.is_empty() {
            let mut out = Vec::new();
            for (bit, ct) in card_type_mask_bits() {
                if (mask.0 & (bit).0) != 0 {
                    out.push(KeywordAbility::ProtectionFrom(
                        ProtectionFromQuality::CardType(ct),
                    ));
                }
            }
            if !out.is_empty() {
                return Ok(out);
            }
        }
        if let Some(ct) = single_card_type_from_mask(mask) {
            return Ok(vec![KeywordAbility::ProtectionFrom(
                ProtectionFromQuality::CardType(ct),
            )]);
        }
    }

    if !flt.subtypes.is_empty() && flt.any_colors.is_empty() && flt.color_arity.is_none() {
        let types = flt.all_types.union(flt.any_types);
        let creatures_only = types == AstCardTypeMask::CREATURE;
        if !types.is_empty() && !creatures_only {
            return Err(LoweringError::Unsupported("v2:kw:protection-subtype-type"));
        }
        let mut out = Vec::new();
        for raw_sub in &flt.subtypes {
            let subtype = ctx
                .lookup_creature_type(raw_sub)
                .ok_or(LoweringError::Unsupported(
                    "v2:kw:protection-unknown-subtype",
                ))?;
            out.push(KeywordAbility::ProtectionFromSubtype(SubtypeProtection {
                subtype,
                creatures_only,
                exclude_subtype: false,
            }));
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    Err(LoweringError::Unsupported("v2:kw:unsupported-protection"))
}

fn lower_hexproof_from_param(
    param: &v2::KeywordParam,
    ctx: &LowerCtx<'_>,
) -> LowerResult<Vec<HexproofQuality>> {
    match param {
        v2::KeywordParam::Color(mask) => {
            let mut out = Vec::new();
            if (mask.0 & (ColorMask::WHITE).0) != 0 {
                out.push(HexproofQuality::Color(Color::White));
            }
            if (mask.0 & (ColorMask::BLUE).0) != 0 {
                out.push(HexproofQuality::Color(Color::Blue));
            }
            if (mask.0 & (ColorMask::BLACK).0) != 0 {
                out.push(HexproofQuality::Color(Color::Black));
            }
            if (mask.0 & (ColorMask::RED).0) != 0 {
                out.push(HexproofQuality::Color(Color::Red));
            }
            if (mask.0 & (ColorMask::GREEN).0) != 0 {
                out.push(HexproofQuality::Color(Color::Green));
            }
            if out.is_empty() {
                Err(LoweringError::Unsupported("v2:kw:hexproof-empty-color"))
            } else {
                Ok(out)
            }
        }
        v2::KeywordParam::Filter(flt) => {
            if let [v2::FilterPredicate::AnyOf(parts)] = flt.predicates.as_slice()
                && flt.all_types.is_empty()
                && flt.any_types.is_empty()
                && flt.subtypes.is_empty()
            {
                let mut out = Vec::new();
                for p in parts {
                    out.extend(lower_hexproof_from_param(
                        &v2::KeywordParam::Filter(Box::new(p.clone())),
                        ctx,
                    )?);
                }
                return Ok(out);
            }
            if flt.is_ability {
                return Ok(vec![HexproofQuality::ActivatedOrTriggeredAbility]);
            }
            if let Some(arity) = flt.color_arity
                && flt.all_types.is_empty()
                && flt.any_types.is_empty()
                && flt.subtypes.is_empty()
            {
                return match arity {
                    ColorArity::Monocolored => Ok(vec![HexproofQuality::Monocolored]),
                    ColorArity::Multicolored => Ok(vec![HexproofQuality::Multicolored]),
                    _ => Err(LoweringError::Unsupported("v2:kw:hexproof-color-arity")),
                };
            }
            if let Some(ct) = single_card_type_from_filter(flt) {
                return Ok(vec![HexproofQuality::CardType(ct)]);
            }
            if flt.subtypes.len() == 1
                && flt.all_types.is_empty()
                && flt.any_types.is_empty()
                && let Some(st) = ctx.lookup_creature_type(&flt.subtypes[0])
            {
                return Ok(vec![HexproofQuality::Subtype(st)]);
            }
            Err(LoweringError::Unsupported("v2:kw:hexproof-filter"))
        }
        _ => Err(LoweringError::Unsupported("v2:kw:hexproof-param")),
    }
}

fn card_type_mask_bits() -> [(AstCardTypeMask, CardType); 9] {
    [
        (AstCardTypeMask::ARTIFACT, CardType::Artifact),
        (AstCardTypeMask::BATTLE, CardType::Battle),
        (AstCardTypeMask::CREATURE, CardType::Creature),
        (AstCardTypeMask::ENCHANTMENT, CardType::Enchantment),
        (AstCardTypeMask::INSTANT, CardType::Instant),
        (AstCardTypeMask::KINDRED, CardType::Kindred),
        (AstCardTypeMask::LAND, CardType::Land),
        (AstCardTypeMask::PLANESWALKER, CardType::Planeswalker),
        (AstCardTypeMask::SORCERY, CardType::Sorcery),
    ]
}

fn single_card_type_from_mask(mask: AstCardTypeMask) -> Option<CardType> {
    let mut found = None;
    for (bit, ct) in card_type_mask_bits() {
        if (mask.0 & (bit).0) != 0 {
            if found.is_some() {
                return None;
            }
            found = Some(ct);
        }
    }
    found
}

fn single_card_type_from_filter(flt: &v2::ObjectFilter) -> Option<CardType> {
    if !flt.subtypes.is_empty() || !flt.forbidden_types.is_empty() {
        return None;
    }
    single_card_type_from_mask(flt.all_types.union(flt.any_types))
}

/// Ensure imported helper types stay cleanly referenced.
#[allow(dead_code)]
fn _unused_kw_check() {
    let _ = (
        std::any::TypeId::of::<HandDiscardKeyword>(),
        std::any::TypeId::of::<MoveSubjectRef>(),
        std::any::TypeId::of::<SubjectDestination>(),
    );
}
