//! Strictly-typed `v2::ir` token creation and object-copy lowering.
//!
//! Lowers `v2::TokenSpec`, `v2::PredefinedToken`, `v2::CopyException`, and
//! `v2::Effect::CopyObject` into native `spellwake_engine` token definitions,
//! `Instruction::CreateTokens`, `MutateObjectSpec::create_token_copy`, and
//! `Instruction::StackOp` without raw text inspection.

use std::collections::{BTreeMap, BTreeSet};

use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
    names::EnglishCardName,
};
use spellwake_engine::{
    activated::{
        ActivatedAbility, ActivatedAbilityId, ActivationRestriction, DiscardCostMode, SourceCost,
    },
    attachment::{AttachmentKind, EnchantRestriction},
    characteristics::{
        AbilityChange, Color, ControllerSelector, KeywordAbility, NumericExpression, NumericPair,
        PermanentSelector, PowerToughness, ResolvedDuration, StaticAbilityId,
        StaticContinuousAbility, StaticPart, StaticParts,
    },
    effects::CounterKind as EngineCounterKind,
    land::LandType,
    mana::{
        ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaOutputModifier, ManaRestriction,
        ManaSacrificeCost, ManaType,
    },
    program::{
        AffectedPlayers, ArtifactSubtype, BodyTarget, CardType, CombatTriggerKeyword, Comparison,
        CopyExceptions, DiesTriggerMode, EnchantmentSubtype, EntryCounterAbility, ExecutableBody,
        Instruction, LibrarySearch, MoveSubjectRef, ObjectFilter as EngineObjectFilter,
        ObjectStatKind, PermanentDefinition, Program, ProgramKind, ResolutionCondition,
        ResolvingPlayer, SearchBattlefieldPostEntry, SearchCardMatcher, SearchCount,
        SearchDestination, SearchSource, StackCopyTarget, TriggerCondition, TriggerEvent,
        TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
        ValueExpr as EngineValueExpr, ValueObjectRef,
    },
    token::{
        AttackingDefenderScope, TokenCreation, TokenDefinition, TokenName, TokenRecipients,
        TokenSize, TokenSubtype,
    },
    toolbox::{
        AdjustLifeSpec, ApplyContinuousSpec, BindingId, CounterOp, DrawCardsSpec, MutateObjectSpec,
        MutatePlayerSpec, ObjectMutation as EngineObjectMutation, ObjectQuery, OrientationOp,
        PlayerSelector, QuerySlice, QueryZone, StackOpSpec, TokenCopySpec,
        ZoneDest as EngineZoneDest, ZoneOpSpec,
    },
};
use spellwake_oracle_compiler::{ast, v2::ir as v2};

use crate::v2_lowering::{
    atoms, card,
    ctx::{BodyAcc, LowerCtx},
    effect,
    error::{LowerResult, LoweringError},
    keywords as kw_lowering, value,
};

/// Lower `Effect::CreateToken` into `Instruction::CreateTokens` or an attached Aura token op.
pub(crate) fn lower_token_creation(
    controller: v2::PlayerRef,
    count: &v2::ValueExpr,
    spec: &v2::TokenSpec,
    modifiers: &v2::EntryModifiers,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match spec {
        v2::TokenSpec::CopyOf { source, exceptions } => {
            lower_copy_object(controller, source, count, modifiers, exceptions, acc, ctx)
        }
        v2::TokenSpec::Predefined(kind) => {
            lower_predefined_token_creation(controller, count, *kind, modifiers, acc, ctx)
        }
        v2::TokenSpec::Creature {
            power,
            toughness,
            colors,
            card_types,
            subtypes,
            keywords,
            abilities,
        } => lower_custom_token_creation(
            controller,
            count,
            power.as_ref(),
            toughness.as_ref(),
            *colors,
            *card_types,
            subtypes,
            keywords,
            abilities,
            modifiers,
            acc,
            ctx,
        ),
    }
}

/// Lower `Effect::CopyObject` into either stack copy (`Instruction::StackOp`) or token copy
/// (`MutateObjectSpec::create_token_copy`).
pub(crate) fn lower_copy_object(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    count: &v2::ValueExpr,
    modifiers: &v2::EntryModifiers,
    exceptions: &[v2::CopyException],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if let Some(stack_op) =
        try_lower_stack_copy(actor, subject, count, modifiers, exceptions, acc, ctx)?
    {
        acc.push(Instruction::StackOp(Box::new(stack_op)));
        return Ok(());
    }

    if modifiers.face_down {
        return Err(LoweringError::Unsupported("v2:token-copy-face-down"));
    }
    if modifiers.transformed {
        return Err(LoweringError::Unsupported("v2:token-copy-transformed"));
    }
    if modifiers.attached_to.is_some() {
        return Err(LoweringError::Unsupported("v2:token-copy-attached"));
    }

    let source = atoms::lower_subject_to_query(subject, acc, ctx)?;
    effect::register_value_expr_player_targets(count, acc, ctx)?;
    let amount = value::lower_value_expr(count, ctx)?;

    let mut copy_spec = TokenCopySpec {
        count: amount,
        recipients: TokenRecipients::Controller,
        enters_tapped: modifiers.tapped,
        enters_attacking: modifiers.attacking,
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
    };

    if let Some((counter_kind, counter_count)) = &modifiers.with_counter {
        let engine_kind = atoms::lower_counter_kind(counter_kind)?;
        let n = u8::try_from(*counter_count)
            .map_err(|_| LoweringError::Unsupported("v2:token-copy-counter-count-overflow"))?;
        if n == 0 {
            return Err(LoweringError::Unsupported("v2:token-copy-zero-counters"));
        }
        copy_spec.extra_counters.push((engine_kind, n));
    }

    apply_copy_exceptions_to_token_copy(exceptions, &mut copy_spec, ctx)?;

    emit_with_token_recipients(actor, acc, ctx, |recipients| {
        let mut spec = copy_spec.clone();
        spec.recipients = recipients;
        Instruction::MutateObject(Box::new(MutateObjectSpec::create_token_copy(
            source.clone(),
            Box::new(spec),
        )))
    })
}

/// Lower a well-known MTG predefined token (`v2::PredefinedToken`) into a validated
/// [`TokenDefinition`].
pub(crate) fn lower_predefined_token_def(
    kind: v2::PredefinedToken,
    vocab: &CreatureTypeVocabulary,
) -> LowerResult<TokenDefinition> {
    let (program, name) = build_predefined_token_program(kind, vocab)?;
    TokenDefinition::new(program, name, vocab)
        .map_err(|_| LoweringError::Unsupported("v2:predefined-token-validation-failed"))
}

/// Lower `v2::CopyException` clauses into [`CopyExceptions`] for Layer 1 permanent copy effects.
pub(crate) fn lower_copy_exceptions(
    exceptions: &[v2::CopyException],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<CopyExceptions> {
    let mut out = CopyExceptions::default();
    for exc in exceptions {
        match exc {
            v2::CopyException::AddKeywords(kws) => {
                for kw in kws {
                    if kw.kind == v2::KeywordKind::Prowess && kw.param.is_none() {
                        out.extra_triggered.push(TriggeredAbility::combat_keyword(
                            ctx.alloc_triggered_id(),
                            CombatTriggerKeyword::Prowess,
                        ));
                    } else if kw.kind == v2::KeywordKind::Decayed && kw.param.is_none() {
                        out.extra_keywords.insert(KeywordAbility::Decayed);
                        out.extra_triggered.push(TriggeredAbility::combat_keyword(
                            ctx.alloc_triggered_id(),
                            CombatTriggerKeyword::Decayed,
                        ));
                    } else {
                        out.extra_keywords
                            .extend(atoms::lower_keyword_abilities(kw, ctx)?);
                    }
                }
            }
            v2::CopyException::NotLegendary => {
                out.nonlegendary = true;
            }
            v2::CopyException::AddTypes(mask) => {
                out.extra_types.extend(atoms::lower_card_type_set(*mask));
            }
            v2::CopyException::AddAbility(face) => {
                let granted = card::lower_granted_face(face, ctx)?;
                if !granted.statics.is_empty() || !granted.mana.is_empty() {
                    return Err(LoweringError::Unsupported(
                        "v2:copy-exception-static-or-mana-ability",
                    ));
                }
                out.extra_keywords.extend(granted.keywords);
                out.extra_activated.extend(granted.activated);
                out.extra_triggered.extend(granted.triggered);
            }
            v2::CopyException::KeepOwnName => {
                out.keep_name = true;
            }
            v2::CopyException::KeepOwnOtherAbilities => {
                out.keep_abilities = true;
            }
            v2::CopyException::SetBasePTAndTypes {
                power,
                toughness,
                subtypes,
                types,
                keep_existing_types,
            } => {
                match (power, toughness) {
                    (Some(p), Some(t)) => {
                        out.override_pt = Some(PowerToughness {
                            power: RulesInteger::from(i64::from(*p)),
                            toughness: RulesInteger::from(i64::from(*t)),
                        });
                    }
                    (None, None) => {}
                    _ => {
                        return Err(LoweringError::Unsupported("v2:copy-exception-partial-pt"));
                    }
                }
                if *keep_existing_types {
                    out.keep_types = true;
                }
                if !types.is_empty() {
                    if !*keep_existing_types {
                        return Err(LoweringError::Unsupported(
                            "v2:copy-exception-replace-types",
                        ));
                    }
                    out.extra_types.extend(atoms::lower_card_type_set(*types));
                }
                for raw in subtypes {
                    if !*keep_existing_types {
                        return Err(LoweringError::Unsupported(
                            "v2:copy-exception-replace-subtypes",
                        ));
                    }
                    if let Some(ct) = ctx.lookup_creature_type(raw) {
                        out.extra_creature_subtypes.insert(ct);
                    } else if let Some(art) = LowerCtx::lookup_artifact_subtype(raw) {
                        out.extra_artifact_subtypes.insert(art);
                    } else {
                        return Err(LoweringError::Unsupported(
                            "v2:copy-exception-unknown-subtype",
                        ));
                    }
                }
            }
            v2::CopyException::HasThisAbility => {
                if ctx.face_ability_count == 1 {
                    out.keep_abilities = true;
                } else {
                    return Err(LoweringError::Unsupported(
                        "v2:copy-exception-has-this-ability",
                    ));
                }
            }
            v2::CopyException::EntersWithCounters { .. } => {
                return Err(LoweringError::Unsupported(
                    "v2:copy-exception-enters-with-counters",
                ));
            }
            v2::CopyException::AddKeywordsConditional { .. } => {
                return Err(LoweringError::Unsupported(
                    "v2:copy-exception-conditional-keywords",
                ));
            }
            v2::CopyException::AddSupertypes { .. } => {
                return Err(LoweringError::Unsupported(
                    "v2:copy-exception-add-supertypes",
                ));
            }
            v2::CopyException::SetColor(_) => {
                return Err(LoweringError::Unsupported("v2:copy-exception-set-color"));
            }
        }
    }
    Ok(out)
}

fn lower_predefined_token_creation(
    controller: v2::PlayerRef,
    count: &v2::ValueExpr,
    kind: v2::PredefinedToken,
    modifiers: &v2::EntryModifiers,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if modifiers.face_down {
        return Err(LoweringError::Unsupported("v2:token-face-down"));
    }
    if modifiers.transformed {
        return Err(LoweringError::Unsupported("v2:token-transformed"));
    }

    let (mut program, name) = build_predefined_token_program(kind, ctx.vocab)?;
    if let Some((counter_kind, counter_count)) = &modifiers.with_counter {
        attach_entry_counter(&mut program, counter_kind, *counter_count)?;
    }
    let definition = TokenDefinition::new(program, name, ctx.vocab)
        .map_err(|_| LoweringError::Unsupported("v2:predefined-token-validation-failed"))?;

    if let Some(host_subj) = &modifiers.attached_to {
        return lower_attached_aura_token(
            controller, count, definition, modifiers, host_subj, acc, ctx,
        );
    }

    effect::register_value_expr_player_targets(count, acc, ctx)?;
    let amount = value::lower_value_expr(count, ctx)?;
    let attacking_defender = modifiers
        .attacking
        .then_some(AttackingDefenderScope::AnyLegal);

    emit_with_token_recipients(controller, acc, ctx, |recipients| {
        Instruction::CreateTokens {
            creations: vec![TokenCreation {
                enters_tapped: modifiers.tapped,
                definition: definition.clone(),
                amount: amount.clone(),
                base_size: None,
                recipients,
                attacking_defender,
            }],
        }
    })
}

// Parameters mirror the orthogonal fields of `v2::TokenSpec::Creature`.
#[allow(clippy::too_many_arguments)]
fn lower_custom_token_creation(
    controller: v2::PlayerRef,
    count: &v2::ValueExpr,
    power: Option<&v2::ValueExpr>,
    toughness: Option<&v2::ValueExpr>,
    colors: ast::ColorMask,
    card_types_mask: ast::CardTypeMask,
    subtypes: &[String],
    keywords: &[v2::KeywordSpec],
    abilities: &[v2::CompiledFaceV2],
    modifiers: &v2::EntryModifiers,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if modifiers.face_down {
        return Err(LoweringError::Unsupported("v2:token-face-down"));
    }
    if modifiers.transformed {
        return Err(LoweringError::Unsupported("v2:token-transformed"));
    }

    let mut card_types = atoms::lower_card_type_set(card_types_mask);
    if card_types.is_empty() && (power.is_some() || toughness.is_some()) {
        card_types.insert(CardType::Creature);
    }
    if card_types.is_empty() || !card_types.iter().all(|ct| ct.is_permanent()) {
        return Err(LoweringError::Unsupported("v2:token-invalid-card-types"));
    }

    let is_creature = card_types.contains(&CardType::Creature);
    let is_kindred = card_types.contains(&CardType::Kindred);
    let is_artifact = card_types.contains(&CardType::Artifact);
    let is_enchantment = card_types.contains(&CardType::Enchantment);
    let is_land = card_types.contains(&CardType::Land);

    let (printed_pt, base_size) = if is_creature {
        match (power, toughness) {
            (Some(p_ir), Some(t_ir)) => {
                if let (Some(p_c), Some(t_c)) = (
                    value::lower_value_const(p_ir),
                    value::lower_value_const(t_ir),
                ) {
                    (
                        Some(
                            PowerToughness {
                                power: RulesInteger::from(i64::from(p_c)),
                                toughness: RulesInteger::from(i64::from(t_c)),
                            }
                            .into(),
                        ),
                        None,
                    )
                } else {
                    effect::register_value_expr_player_targets(p_ir, acc, ctx)?;
                    effect::register_value_expr_player_targets(t_ir, acc, ctx)?;
                    let p_expr = value::lower_value_expr(p_ir, ctx)?;
                    let t_expr = value::lower_value_expr(t_ir, ctx)?;
                    let size = if p_expr == t_expr {
                        TokenSize::AtCreation(p_expr)
                    } else {
                        TokenSize::AtCreationPair {
                            power: Box::new(p_expr),
                            toughness: Box::new(t_expr),
                        }
                    };
                    (Some(PowerToughness::zero().into()), Some(size))
                }
            }
            _ => {
                return Err(LoweringError::Unsupported("v2:token-creature-missing-pt"));
            }
        }
    } else if power.is_some() || toughness.is_some() {
        return Err(LoweringError::Unsupported("v2:token-noncreature-has-pt"));
    } else {
        (None, None)
    };

    let mut creature_subtypes: BTreeSet<CreatureTypeId> = BTreeSet::new();
    let mut land_subtypes: BTreeSet<LandType> = BTreeSet::new();
    let mut artifact_subtypes: BTreeSet<ArtifactSubtype> = BTreeSet::new();
    let mut enchantment_subtypes: BTreeSet<EnchantmentSubtype> = BTreeSet::new();
    let mut ordered_subtypes: Vec<TokenSubtype> = Vec::new();
    let mut seen_ordered: BTreeSet<TokenSubtype> = BTreeSet::new();
    let mut is_aura = false;

    for raw in subtypes {
        if raw.eq_ignore_ascii_case("aura") && is_enchantment {
            is_aura = true;
            continue;
        }
        let token_sub = if (is_creature || is_kindred)
            && let Some(ct) = ctx.lookup_creature_type(raw)
        {
            creature_subtypes.insert(ct);
            TokenSubtype::Creature(ct)
        } else if is_artifact && let Some(art) = LowerCtx::lookup_artifact_subtype(raw) {
            artifact_subtypes.insert(art);
            TokenSubtype::Artifact(art)
        } else if is_enchantment && let Some(ench) = LowerCtx::lookup_enchantment_subtype(raw) {
            enchantment_subtypes.insert(ench);
            TokenSubtype::Enchantment(ench)
        } else if is_land && let Some(land) = LowerCtx::lookup_land_type(raw) {
            land_subtypes.insert(land);
            TokenSubtype::Land(land)
        } else {
            return Err(LoweringError::Unsupported("v2:token-unknown-subtype"));
        };
        if seen_ordered.insert(token_sub) {
            ordered_subtypes.push(token_sub);
        }
    }

    let attachment = if is_aura {
        Some(AttachmentKind::Aura(EnchantRestriction::Permanent))
    } else if artifact_subtypes.contains(&ArtifactSubtype::Equipment) {
        Some(AttachmentKind::Equipment)
    } else if artifact_subtypes.contains(&ArtifactSubtype::Fortification) {
        Some(AttachmentKind::Fortification)
    } else {
        None
    };

    let mut program = Program::new(
        card_types,
        creature_subtypes,
        land_subtypes,
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment,
            printed_pt,
            casting_cost: None,
        }),
    );
    program.artifact_subtypes = artifact_subtypes;
    program.enchantment_subtypes = enchantment_subtypes;
    program.effect_defined_colors = Some(atoms::lower_color_set(colors));

    for kw in keywords {
        if let Err(err) = kw_lowering::apply_face_keyword(kw, &mut program, ctx) {
            if let Ok(kas) = atoms::lower_keyword_abilities(kw, ctx) {
                program.printed_keywords.extend(kas);
            } else {
                return Err(err);
            }
        }
    }

    for face in abilities {
        let (granted, restricted) = card::lower_granted_face_with_restrictions(face, ctx)?;
        program.printed_keywords.extend(granted.keywords);
        program.activated.extend(granted.activated);
        for (ab, restriction) in restricted {
            let id = ab.id;
            program.activated.push(ab);
            program.activation_restrictions.insert(id, restriction);
        }
        program.triggered.extend(granted.triggered);
        program.mana.extend(granted.mana);
        for stat in granted.statics {
            if program.continuous.is_none() {
                program.continuous = Some(stat);
            } else {
                program.additional_battlefield_abilities.push(stat);
            }
        }
    }

    if let Some((counter_kind, counter_count)) = &modifiers.with_counter {
        attach_entry_counter(&mut program, counter_kind, *counter_count)?;
    }

    let definition =
        TokenDefinition::new(program, TokenName::Default { ordered_subtypes }, ctx.vocab)
            .map_err(|_| LoweringError::Unsupported("v2:custom-token-validation-failed"))?;

    if let Some(host_subj) = &modifiers.attached_to {
        return lower_attached_aura_token(
            controller, count, definition, modifiers, host_subj, acc, ctx,
        );
    }

    effect::register_value_expr_player_targets(count, acc, ctx)?;
    let amount = value::lower_value_expr(count, ctx)?;
    let attacking_defender = modifiers
        .attacking
        .then_some(AttackingDefenderScope::AnyLegal);

    emit_with_token_recipients(controller, acc, ctx, |recipients| {
        Instruction::CreateTokens {
            creations: vec![TokenCreation {
                enters_tapped: modifiers.tapped,
                definition: definition.clone(),
                amount: amount.clone(),
                base_size: base_size.clone(),
                recipients,
                attacking_defender,
            }],
        }
    })
}

fn lower_attached_aura_token(
    controller: v2::PlayerRef,
    count: &v2::ValueExpr,
    definition: TokenDefinition,
    modifiers: &v2::EntryModifiers,
    host_subj: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let is_aura = definition
        .program()
        .permanent()
        .and_then(|perm| perm.attachment)
        .is_some_and(|att| matches!(att, AttachmentKind::Aura(_)));
    if !is_aura {
        return Err(LoweringError::Unsupported("v2:token-non-aura-attached"));
    }
    if modifiers.tapped || modifiers.attacking || modifiers.with_counter.is_some() {
        return Err(LoweringError::Unsupported(
            "v2:token-attached-unsupported-modifiers",
        ));
    }
    if controller != v2::PlayerRef::You {
        return Err(LoweringError::Unsupported(
            "v2:token-attached-non-controller",
        ));
    }
    if value::lower_value_const(count) != Some(1) {
        return Err(LoweringError::Unsupported(
            "v2:token-attached-count-not-one",
        ));
    }
    let host = atoms::lower_subject_to_query(host_subj, acc, ctx)?;
    acc.push(Instruction::MutateObject(Box::new(
        MutateObjectSpec::create_aura_token_attached(host, Box::new(definition), false),
    )));
    Ok(())
}

fn attach_entry_counter(
    program: &mut Program,
    counter_kind: &v2::CounterKind,
    count: u16,
) -> LowerResult<()> {
    if count == 0 {
        return Err(LoweringError::Unsupported("v2:token-zero-entry-counters"));
    }
    let engine_kind = atoms::lower_counter_kind(counter_kind)?;
    let amount: RulesInteger = i64::from(count).into();
    let ability = match engine_kind {
        EngineCounterKind::PlusOne => EntryCounterAbility::FixedPlusOne(amount),
        EngineCounterKind::MinusOne => EntryCounterAbility::FixedMinusOne(amount),
        EngineCounterKind::Charge => EntryCounterAbility::FixedCharge(amount),
        EngineCounterKind::Loyalty => EntryCounterAbility::FixedLoyalty(amount),
        other => EntryCounterAbility::FixedNamed {
            kind: other,
            amount,
        },
    };
    program.entry_counter_abilities.push(ability);
    Ok(())
}

fn emit_with_token_recipients(
    controller: v2::PlayerRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
    build_instruction: impl FnOnce(TokenRecipients) -> Instruction,
) -> LowerResult<()> {
    match controller {
        v2::PlayerRef::You => {
            acc.push(build_instruction(TokenRecipients::Controller));
            Ok(())
        }
        v2::PlayerRef::EachPlayer => {
            acc.push(build_instruction(TokenRecipients::EachPlayer));
            Ok(())
        }
        v2::PlayerRef::EachOpponent => {
            acc.push(build_instruction(TokenRecipients::EachOpponent));
            Ok(())
        }
        v2::PlayerRef::EachOtherPlayer => {
            let inner = ExecutableBody::new(
                BodyTarget::None,
                vec![build_instruction(TokenRecipients::Player(
                    ResolvingPlayer::LoopPlayer,
                ))],
            );
            acc.push(Instruction::ForEachPlayerQuery {
                selector: Box::new(PlayerSelector::Group(AffectedPlayers::EachOtherPlayer)),
                bind_player: BindingId::LOOP_PLAYER,
                body: Box::new(inner),
            });
            Ok(())
        }
        other => {
            let rp = atoms::lower_resolving_player(other, acc, ctx)?;
            acc.push(build_instruction(TokenRecipients::Player(rp)));
            Ok(())
        }
    }
}

fn try_lower_stack_copy(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    count: &v2::ValueExpr,
    modifiers: &v2::EntryModifiers,
    exceptions: &[v2::CopyException],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<StackOpSpec>> {
    let is_stack_subject = match subject {
        v2::SubjectRef::TargetObject { filter, .. } => is_stack_only_filter(filter),
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSpell) => true,
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::It) | v2::SubjectRef::SelfObject
            if ctx.in_trigger && ctx.trigger_is_spell_cast =>
        {
            true
        }
        _ => false,
    };
    if !is_stack_subject {
        return Ok(None);
    }

    if actor != v2::PlayerRef::You {
        return Err(LoweringError::Unsupported("v2:copy-stack-non-controller"));
    }
    if modifiers.tapped
        || modifiers.attacking
        || modifiers.face_down
        || modifiers.transformed
        || modifiers.with_counter.is_some()
        || modifiers.attached_to.is_some()
    {
        return Err(LoweringError::Unsupported("v2:copy-stack-with-modifiers"));
    }
    if !exceptions.is_empty() {
        return Err(LoweringError::Unsupported("v2:copy-stack-with-exceptions"));
    }

    effect::register_value_expr_player_targets(count, acc, ctx)?;
    let amount = value::lower_value_expr(count, ctx)?;
    match subject {
        v2::SubjectRef::TargetObject { filter, .. } => {
            let (target, t_count) = atoms::lower_subject_to_target(subject, ctx)?;
            acc.register_target(target, t_count)?;
            let is_one = value::lower_value_const(count) == Some(1);
            if filter.is_ability && !filter.any_spell {
                if is_one {
                    Ok(Some(StackOpSpec::copy_targeted_ability()))
                } else {
                    Ok(Some(StackOpSpec::copy_stack(
                        StackCopyTarget::TargetAbility,
                        amount,
                        true,
                    )))
                }
            } else if filter.is_ability && filter.any_spell {
                Ok(Some(StackOpSpec::copy_stack(
                    StackCopyTarget::TargetSpellOrAbility,
                    amount,
                    true,
                )))
            } else if is_one {
                Ok(Some(StackOpSpec::copy_targeted_spell()))
            } else {
                Ok(Some(StackOpSpec::copy_stack(
                    StackCopyTarget::TargetSpell,
                    amount,
                    true,
                )))
            }
        }
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSpell) => {
            if !ctx.in_trigger {
                return Err(LoweringError::Unsupported(
                    "v2:copy-that-spell-outside-trigger",
                ));
            }
            Ok(Some(StackOpSpec::copy_stack(
                StackCopyTarget::TriggeringSpell,
                amount,
                true,
            )))
        }
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::It) | v2::SubjectRef::SelfObject
            if ctx.in_trigger && ctx.trigger_is_spell_cast =>
        {
            Ok(Some(StackOpSpec::copy_stack(
                StackCopyTarget::TriggeringSpell,
                amount,
                true,
            )))
        }
        v2::SubjectRef::SelfObject if ctx.is_spell_body => Ok(Some(StackOpSpec::copy_stack(
            StackCopyTarget::ResolvingSpell,
            amount,
            true,
        ))),
        _ => Ok(None),
    }
}

fn is_stack_only_filter(filter: &v2::ObjectFilter) -> bool {
    if filter.zones.iter().any(|z| z.zone != v2::ZoneKind::Stack) {
        return false;
    }
    if filter.any_spell || filter.is_ability {
        return true;
    }
    let spell_bits = ast::CardTypeMask::INSTANT.union(ast::CardTypeMask::SORCERY);
    let has_spell_types =
        (filter.all_types.0 & spell_bits.0) != 0 || (filter.any_types.0 & spell_bits.0) != 0;
    let has_perm_types = (filter.all_types.0 & ast::CardTypeMask::PERMANENT.0) != 0
        || (filter.any_types.0 & ast::CardTypeMask::PERMANENT.0) != 0;
    has_spell_types && !has_perm_types && !filter.any_card && !filter.any_permanent
}

fn apply_copy_exceptions_to_token_copy(
    exceptions: &[v2::CopyException],
    spec: &mut TokenCopySpec,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    for exc in exceptions {
        match exc {
            v2::CopyException::AddKeywords(kws) => {
                for kw in kws {
                    if kw.kind == v2::KeywordKind::Prowess && kw.param.is_none() {
                        spec.extra_triggered.push(TriggeredAbility::combat_keyword(
                            ctx.alloc_triggered_id(),
                            CombatTriggerKeyword::Prowess,
                        ));
                    } else if kw.kind == v2::KeywordKind::Decayed && kw.param.is_none() {
                        spec.extra_keywords.insert(KeywordAbility::Decayed);
                        spec.extra_triggered.push(TriggeredAbility::combat_keyword(
                            ctx.alloc_triggered_id(),
                            CombatTriggerKeyword::Decayed,
                        ));
                    } else {
                        for engine_kw in atoms::lower_keyword_abilities(kw, ctx)? {
                            if engine_kw == KeywordAbility::Haste {
                                spec.haste = true;
                            }
                            spec.extra_keywords.insert(engine_kw);
                        }
                    }
                }
            }
            v2::CopyException::NotLegendary => {
                spec.nonlegendary = true;
            }
            v2::CopyException::AddTypes(mask) => {
                spec.extra_types.extend(atoms::lower_card_type_set(*mask));
            }
            v2::CopyException::AddAbility(face) => {
                let granted = card::lower_granted_face(face, ctx)?;
                spec.extra_keywords.extend(granted.keywords);
                spec.extra_activated.extend(granted.activated);
                spec.extra_triggered.extend(granted.triggered);
                spec.extra_static.extend(granted.statics);
                spec.extra_mana.extend(granted.mana);
            }
            v2::CopyException::KeepOwnName => {
                spec.keep_name = true;
            }
            v2::CopyException::EntersWithCounters {
                kind,
                count,
                condition,
            } => {
                if condition.is_some() {
                    return Err(LoweringError::Unsupported(
                        "v2:token-copy-conditional-counters",
                    ));
                }
                let engine_kind = atoms::lower_counter_kind(kind)?;
                let Some(val) = value::lower_value_const(count) else {
                    return Err(LoweringError::Unsupported("v2:token-copy-dynamic-counters"));
                };
                let n = u8::try_from(val)
                    .map_err(|_| LoweringError::Unsupported("v2:token-copy-counter-count-range"))?;
                if n == 0 {
                    return Err(LoweringError::Unsupported("v2:token-copy-zero-counters"));
                }
                spec.extra_counters.push((engine_kind, n));
            }
            v2::CopyException::SetColor(mask) => {
                spec.override_colors = Some(atoms::lower_color_set(*mask));
            }
            v2::CopyException::SetBasePTAndTypes {
                power,
                toughness,
                subtypes,
                types,
                keep_existing_types,
            } => {
                match (power, toughness) {
                    (Some(p), Some(t)) => {
                        spec.override_pt = Some((
                            RulesInteger::from(i64::from(*p)),
                            RulesInteger::from(i64::from(*t)),
                        ));
                    }
                    (None, None) => {}
                    _ => {
                        return Err(LoweringError::Unsupported("v2:token-copy-partial-pt"));
                    }
                }
                if !types.is_empty() {
                    if !*keep_existing_types {
                        return Err(LoweringError::Unsupported("v2:token-copy-replace-types"));
                    }
                    spec.extra_types.extend(atoms::lower_card_type_set(*types));
                }
                for raw in subtypes {
                    if !*keep_existing_types {
                        return Err(LoweringError::Unsupported("v2:token-copy-replace-subtypes"));
                    }
                    let Some(ct) = ctx.lookup_creature_type(raw) else {
                        return Err(LoweringError::Unsupported(
                            "v2:token-copy-non-creature-subtype",
                        ));
                    };
                    spec.extra_subtypes.insert(ct);
                }
            }
            v2::CopyException::HasThisAbility => {
                return Err(LoweringError::Unsupported("v2:token-copy-has-this-ability"));
            }
            v2::CopyException::AddKeywordsConditional { .. } => {
                return Err(LoweringError::Unsupported(
                    "v2:token-copy-conditional-keywords",
                ));
            }
            v2::CopyException::AddSupertypes { .. } => {
                return Err(LoweringError::Unsupported("v2:token-copy-add-supertypes"));
            }
            v2::CopyException::KeepOwnOtherAbilities => {
                return Err(LoweringError::Unsupported(
                    "v2:token-copy-keep-own-abilities",
                ));
            }
        }
    }
    Ok(())
}

fn build_predefined_token_program(
    kind: v2::PredefinedToken,
    vocab: &CreatureTypeVocabulary,
) -> LowerResult<(Program, TokenName)> {
    match kind {
        v2::PredefinedToken::Treasure => {
            let mut p = Program::artifact(None, Vec::new());
            p.artifact_subtypes.insert(ArtifactSubtype::Treasure);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaOutput::all_colors(),
                }
                .with_modifier(ManaOutputModifier::Sacrifice(
                    ManaSacrificeCost::Source { tap: true },
                )),
            });
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Treasure)],
                },
            ))
        }
        v2::PredefinedToken::Clue => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ActivatedAbilityId(1),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::Sacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::DrawCards(Box::new(DrawCardsSpec::simple(
                            PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            EngineValueExpr::literal(1),
                        )))],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Clue);
            p.effect_defined_colors = Some(BTreeSet::new());
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Clue)],
                },
            ))
        }
        v2::PredefinedToken::Food => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ActivatedAbilityId(1),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::AdjustLife(Box::new(AdjustLifeSpec::gain(
                            PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            EngineValueExpr::literal(3),
                        )))],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Food);
            p.effect_defined_colors = Some(BTreeSet::new());
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Food)],
                },
            ))
        }
        v2::PredefinedToken::Blood => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ActivatedAbilityId(1),
                    mana_cost: ManaCost::generic(1.into()),
                    source_cost: SourceCost::Discard {
                        mode: DiscardCostMode::SelectedOne,
                        tap_source: true,
                        sacrifice_source: true,
                    },
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::DrawCards(Box::new(DrawCardsSpec::simple(
                            PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            EngineValueExpr::literal(1),
                        )))],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Blood);
            p.effect_defined_colors = Some(BTreeSet::new());
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Blood)],
                },
            ))
        }
        v2::PredefinedToken::Map => {
            let act_id = ActivatedAbilityId(1);
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: act_id,
                    mana_cost: ManaCost::generic(1.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::filtered_permanent(EngineObjectFilter::creature().controlled()),
                        vec![Instruction::MutateObject(Box::new(
                            MutateObjectSpec::explore(ObjectQuery::from_move_subject(
                                MoveSubjectRef::TargetObject,
                            )),
                        ))],
                    ),
                }],
            );
            p.activation_restrictions
                .insert(act_id, ActivationRestriction::Sorcery);
            p.artifact_subtypes.insert(ArtifactSubtype::Map);
            p.effect_defined_colors = Some(BTreeSet::new());
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Map)],
                },
            ))
        }
        v2::PredefinedToken::Powerstone => {
            let mut p = Program::artifact(None, Vec::new());
            p.artifact_subtypes.insert(ArtifactSubtype::Powerstone);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())]))
                    .restricted(ManaRestriction::NotNonartifactSpell),
            });
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Powerstone)],
                },
            ))
        }
        v2::PredefinedToken::Shard => {
            let mut p = Program::enchantment(None);
            p.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::generic(2.into()),
                source_cost: SourceCost::Sacrifice,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::MutatePlayer(Box::new(MutatePlayerSpec::scry(
                            ResolvingPlayer::Controller,
                            EngineValueExpr::literal(1),
                        ))),
                        Instruction::DrawCards(Box::new(DrawCardsSpec::simple(
                            PlayerSelector::Resolving(ResolvingPlayer::Controller),
                            EngineValueExpr::literal(1),
                        ))),
                    ],
                ),
            });
            p.enchantment_subtypes.insert(EnchantmentSubtype::Shard);
            p.effect_defined_colors = Some(BTreeSet::new());
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Enchantment(EnchantmentSubtype::Shard)],
                },
            ))
        }
        v2::PredefinedToken::Lander => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ActivatedAbilityId(1),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::SearchLibrary {
                            filter: Box::new(LibrarySearch {
                                searcher: ResolvingPlayer::Controller,
                                source: SearchSource::OwnLibrary,
                                matcher: SearchCardMatcher::BasicLand,
                                mana_value: None,
                                count: SearchCount::UpTo(1),
                                pair_constraint: None,
                                destination: SearchDestination::Battlefield {
                                    tapped: true,
                                    post_entry: SearchBattlefieldPostEntry::None,
                                },
                                reveal: false,
                            }),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Lander);
            p.effect_defined_colors = Some(BTreeSet::new());
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Lander)],
                },
            ))
        }
        v2::PredefinedToken::Junk => {
            let act_id = ActivatedAbilityId(1);
            let bind_id = BindingId(1);
            let mut exile_spec = ZoneOpSpec::move_all(
                ObjectQuery::ZoneScan {
                    zone: QueryZone::Library(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ))),
                    filter: Box::new(EngineObjectFilter::any()),
                    slice: QuerySlice::Top(Box::new(EngineValueExpr::literal(1))),
                },
                EngineZoneDest::exile(),
            );
            exile_spec.bind_moved = Some(bind_id);
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: act_id,
                    mana_cost: ManaCost::free(),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![
                            Instruction::ZoneOp(Box::new(exile_spec)),
                            Instruction::ApplyContinuous(Box::new(
                                ApplyContinuousSpec::play_permission(
                                    ObjectQuery::Bound(bind_id),
                                    PlayerSelector::Resolving(ResolvingPlayer::Controller),
                                    ResolvedDuration::UntilEndOfTurn,
                                    false,
                                    false,
                                    false,
                                ),
                            )),
                        ],
                    ),
                }],
            );
            p.activation_restrictions
                .insert(act_id, ActivationRestriction::Sorcery);
            p.artifact_subtypes.insert(ArtifactSubtype::Junk);
            p.effect_defined_colors = Some(BTreeSet::new());
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Junk)],
                },
            ))
        }
        v2::PredefinedToken::Gold => {
            let mut p = Program::artifact(None, Vec::new());
            p.artifact_subtypes.insert(ArtifactSubtype::Gold);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaOutput::all_colors(),
                }
                .with_modifier(ManaOutputModifier::Sacrifice(
                    ManaSacrificeCost::Source { tap: false },
                )),
            });
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Gold)],
                },
            ))
        }
        v2::PredefinedToken::Incubator => {
            let phyrexian = vocab
                .resolve_label("Phyrexian")
                .ok_or(LoweringError::Unsupported(
                    "v2:predefined-token-missing-phyrexian",
                ))?;
            let mut back = Program::creature(None, 0.into(), 0.into(), BTreeSet::from([phyrexian]));
            back.card_types.insert(CardType::Artifact);
            back.effect_defined_colors = Some(BTreeSet::new());
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ActivatedAbilityId(1),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::None,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                            ObjectQuery::Source,
                            EngineObjectMutation::Orientation(OrientationOp::Transform),
                        )))],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Incubator);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.transform_back = Some(Box::new(back));
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Incubator)],
                },
            ))
        }
        v2::PredefinedToken::Walker => {
            let zombie = vocab
                .resolve_label("Zombie")
                .ok_or(LoweringError::Unsupported(
                    "v2:predefined-token-missing-zombie",
                ))?;
            let mut p = Program::creature(None, 2.into(), 2.into(), BTreeSet::from([zombie]));
            p.effect_defined_colors = Some(BTreeSet::from([Color::Black]));
            let name = EnglishCardName::new("Walker".to_string())
                .map_err(|_| LoweringError::Unsupported("v2:predefined-token-name-invalid"))?;
            Ok((p, TokenName::Specified(name)))
        }
        v2::PredefinedToken::Mutagen => {
            let act_id = ActivatedAbilityId(1);
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: act_id,
                    mana_cost: ManaCost::generic(1.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::SINGLE_CREATURE,
                        vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                            ObjectQuery::Targets,
                            EngineObjectMutation::Counters(CounterOp::Add {
                                kind: EngineCounterKind::PlusOne,
                                amount: EngineValueExpr::literal(1),
                            }),
                        )))],
                    ),
                }],
            );
            p.activation_restrictions
                .insert(act_id, ActivationRestriction::Sorcery);
            p.artifact_subtypes.insert(ArtifactSubtype::Mutagen);
            p.effect_defined_colors = Some(BTreeSet::new());
            Ok((
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Mutagen)],
                },
            ))
        }
        v2::PredefinedToken::WickedRole
        | v2::PredefinedToken::MonsterRole
        | v2::PredefinedToken::RoyalRole
        | v2::PredefinedToken::SorcererRole
        | v2::PredefinedToken::CursedRole
        | v2::PredefinedToken::YoungHeroRole => build_role_token_program(kind),
        v2::PredefinedToken::Role => Err(LoweringError::Unsupported(
            "v2:predefined-token-generic-role",
        )),
        v2::PredefinedToken::Tarmogoyf => {
            Err(LoweringError::Unsupported("v2:predefined-token-tarmogoyf"))
        }
        v2::PredefinedToken::Heartwood => {
            Err(LoweringError::Unsupported("v2:predefined-token-heartwood"))
        }
        v2::PredefinedToken::Vibranium => {
            Err(LoweringError::Unsupported("v2:predefined-token-vibranium"))
        }
        v2::PredefinedToken::Mutavault => {
            Err(LoweringError::Unsupported("v2:predefined-token-mutavault"))
        }
    }
}

fn build_role_token_program(kind: v2::PredefinedToken) -> LowerResult<(Program, TokenName)> {
    let mut p = Program::aura(None, EnchantRestriction::Creature);
    p.enchantment_subtypes.insert(EnchantmentSubtype::Role);
    p.effect_defined_colors = Some(BTreeSet::new());

    let attached_sel = PermanentSelector {
        attached_to_source: true,
        controller: ControllerSelector::Any,
        ..PermanentSelector::default()
    };
    let plus_one_pair = NumericPair {
        power: NumericExpression::Literal(1.into()),
        toughness: NumericExpression::Literal(1.into()),
    };

    let role_name = match kind {
        v2::PredefinedToken::CursedRole => {
            p.continuous = Some(StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(1),
                selector: attached_sel,
                parts: StaticParts(vec![StaticPart::SetPT(plus_one_pair)]),
            });
            "Cursed"
        }
        v2::PredefinedToken::MonsterRole => {
            p.continuous = Some(StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(1),
                selector: attached_sel,
                parts: StaticParts(vec![
                    StaticPart::ModifyPT(plus_one_pair),
                    StaticPart::Abilities(AbilityChange::grant_keywords([KeywordAbility::Trample])),
                ]),
            });
            "Monster"
        }
        v2::PredefinedToken::RoyalRole => {
            let ward = TriggeredAbility::ward(TriggeredAbilityId(1), ManaCost::generic(1.into()));
            p.continuous = Some(StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(1),
                selector: attached_sel,
                parts: StaticParts(vec![
                    StaticPart::ModifyPT(plus_one_pair),
                    StaticPart::GrantTrigger(Box::new(ward)),
                ]),
            });
            "Royal"
        }
        v2::PredefinedToken::SorcererRole => {
            let scry_trig = TriggeredAbility {
                id: TriggeredAbilityId(1),
                condition: TriggerSpec::self_attacks(),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutatePlayer(Box::new(MutatePlayerSpec::scry(
                        ResolvingPlayer::Controller,
                        EngineValueExpr::literal(1),
                    )))],
                ),
            };
            p.continuous = Some(StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(1),
                selector: attached_sel,
                parts: StaticParts(vec![
                    StaticPart::ModifyPT(plus_one_pair),
                    StaticPart::GrantTrigger(Box::new(scry_trig)),
                ]),
            });
            "Sorcerer"
        }
        v2::PredefinedToken::WickedRole => {
            p.continuous = Some(StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(1),
                selector: attached_sel,
                parts: StaticParts(vec![StaticPart::ModifyPT(plus_one_pair)]),
            });
            p.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(1),
                condition: TriggerSpec::new(TriggerEvent::Dies {
                    subject: EngineObjectFilter::self_object(),
                    mode: DiesTriggerMode::Permanent,
                }),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustLife(Box::new(AdjustLifeSpec::lose(
                        PlayerSelector::Group(AffectedPlayers::EachOpponent),
                        EngineValueExpr::literal(1),
                    )))],
                ),
            });
            "Wicked"
        }
        v2::PredefinedToken::YoungHeroRole => {
            let attack_trig = TriggeredAbility {
                id: TriggeredAbilityId(1),
                condition: TriggerSpec::self_attacks().only_if(TriggerCondition::Eval(Box::new(
                    ResolutionCondition {
                        left: EngineValueExpr::ObjectStat {
                            subject: ValueObjectRef::AbilitySource,
                            stat: ObjectStatKind::Toughness,
                        },
                        comparison: Comparison::LessOrEqual,
                        right: EngineValueExpr::literal(3),
                    },
                ))),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                        ObjectQuery::Source,
                        EngineObjectMutation::Counters(CounterOp::Add {
                            kind: EngineCounterKind::PlusOne,
                            amount: EngineValueExpr::literal(1),
                        }),
                    )))],
                ),
            };
            p.continuous = Some(StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(1),
                selector: attached_sel,
                parts: StaticParts(vec![StaticPart::GrantTrigger(Box::new(attack_trig))]),
            });
            "Young Hero"
        }
        _ => {
            return Err(LoweringError::Unsupported(
                "v2:predefined-token-unknown-role",
            ));
        }
    };

    let name = EnglishCardName::new(role_name.to_string())
        .map_err(|_| LoweringError::Unsupported("v2:predefined-token-name-invalid"))?;
    Ok((p, TokenName::Specified(name)))
}
