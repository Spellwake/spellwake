//! Top-level card and face lowering from `spellwake_oracle_compiler::v2` into validated
//! `spellwake_engine::program::Program` records.
//!
//! Zero string parsing, zero regex, zero raw `oracle_text` inspection. Printed frame metadata is
//! delegated exclusively to `card_meta.rs`; every unhandled `v2::ir` construct fails closed with
//! `LoweringError::Unsupported`.

use std::collections::BTreeSet;
use std::num::NonZeroU16;

use spellwake_content::{CatalogFace, Inventory, InventoryCard};
use spellwake_contracts::{
    creature_types::CreatureTypeVocabulary,
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, ActivationRestriction, SourceCost},
    characteristics::{
        AbilityChange, KeywordAbility, NumericExpression, NumericPair, PermanentSelector,
        PrintedPowerToughness, PrintedStat, StaticContinuousAbility, StaticPart, StaticParts,
        StaticSourceCondition, StaticTypeChange, SubtypeOperation, TypeOperation,
    },
    effects::CounterKind as EngineCounterKind,
    land::EntryTap,
    mana::{HybridSymbol, ManaAbility, ManaCost, ManaOutput, ManaType},
    program::{
        BodyTarget, CardType, CastTimingRestriction, CounterTriggerKind, DieRollBand, Disturb,
        EnchantmentSubtype, ExecutableBody, GrowthKeyword, Instruction, ModalUniqueness,
        ObjectFilter, Program, ProgramKind, SpellModes, SpellSubtype, SplitCard, StatusTriggerKind,
        TriggerCondition, TriggerEvent, TriggerGrouping, TriggerSpec, TriggeredAbility,
    },
    toolbox::{MutateObjectSpec, PlayerMutation, RandomEventOp, StackOpSpec},
    turn::Step,
};
use spellwake_oracle_compiler::{
    CardInput, FaceInput,
    v2::{
        compile_card_v2,
        ir::{self as v2, CompiledFaceV2},
    },
};

use crate::v2_lowering::{
    atoms::lower_step,
    card_meta::{FaceMetaOptions, build_base_program, parse_color_indicator, parse_type_line},
    condition::lower_resolution_condition,
    cost::{
        combine_restrictions, extract_modal_choice, lower_activated_ability,
        lower_activation_restrictions, lower_choice_count, lower_v2_fixed_mana_cost,
        lower_v2_mana_cost,
    },
    ctx::{LowerCtx, cached_oracle_vocab},
    effect::lower_effect_body,
    error::{LowerResult, LoweringError},
    keywords::{apply_face_keyword, simple_keyword_ability},
    replacement::lower_replacement_into_program,
    static_ability::{lower_static_into_program, push_static_ability},
    trigger::{lower_triggered_ability, lower_triggered_body},
};

/// Lowered ability bundle extracted from a granted sub-face (e.g. token abilities, copy
/// exceptions, emblems, or temporary granted abilities).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GrantedAbilities {
    pub keywords: BTreeSet<KeywordAbility>,
    pub activated: Vec<ActivatedAbility>,
    pub triggered: Vec<TriggeredAbility>,
    pub statics: Vec<StaticContinuousAbility>,
    pub mana: Vec<ManaAbility>,
}

/// Lower a nested `CompiledFaceV2` (such as abilities granted to a creature, token, copy, or
/// emblem) into [`GrantedAbilities`], failing closed on any ability feature that cannot be
/// carried in a granted bundle.
pub(crate) fn lower_granted_face(
    face: &CompiledFaceV2,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<GrantedAbilities> {
    let (granted, _) = lower_granted_face_inner(face, ctx, false)?;
    Ok(granted)
}

/// Lower a nested `CompiledFaceV2` into [`GrantedAbilities`] plus any restricted activated
/// abilities (`StaticPart::GrantActivatedRestricted` / `ResolvedPartSpec::GrantActivatedRestricted`),
/// failing closed on any other auxiliary `Program` state.
pub(crate) fn lower_granted_face_with_restrictions(
    face: &CompiledFaceV2,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(
    GrantedAbilities,
    Vec<(ActivatedAbility, ActivationRestriction)>,
)> {
    lower_granted_face_inner(face, ctx, true)
}

fn lower_granted_face_inner(
    face: &CompiledFaceV2,
    ctx: &mut LowerCtx<'_>,
    allow_activation_restrictions: bool,
) -> LowerResult<(
    GrantedAbilities,
    Vec<(ActivatedAbility, ActivationRestriction)>,
)> {
    if !face.is_ok() {
        return Err(LoweringError::ParseIncomplete);
    }
    if !face.spell_effects.is_empty() {
        return Err(LoweringError::Unsupported("v2:granted:spell-effects"));
    }
    if !face.replacements.is_empty() {
        return Err(LoweringError::Unsupported("v2:granted:replacements"));
    }

    let mut scratch = Program::artifact(None, Vec::new());

    for kw in &face.keywords {
        apply_face_keyword(kw, &mut scratch, ctx)?;
    }
    for st in &face.statics {
        lower_static_into_program(st, &mut scratch, ctx)?;
    }
    for ab in &face.abilities {
        match ab {
            v2::Ability::Activated {
                cost,
                body,
                restrictions,
            } => lower_activated_ability(cost, body, restrictions, &mut scratch, ctx)?,
            v2::Ability::Triggered { trigger, body } => {
                let lowered = lower_triggered_ability(trigger, body, ctx)?;
                scratch.triggered.extend(lowered);
            }
            v2::Ability::Loyalty { .. }
            | v2::Ability::SagaChapter { .. }
            | v2::Ability::TableRow { .. }
            | v2::Ability::StickerRow { .. }
            | v2::Ability::CaseSolveCondition(_)
            | v2::Ability::CaseSolved(_) => {
                return Err(LoweringError::Unsupported("v2:granted:unsupported-ability"));
            }
        }
    }

    // Fail closed if any lowered keyword/ability/static relied on auxiliary Program maps that
    // the caller's granted-ability carrier does not support.
    if (!allow_activation_restrictions && !scratch.activation_restrictions.is_empty())
        || !scratch.activation_modes.is_empty()
        || !scratch.activation_hybrid_symbols.is_empty()
        || !scratch.extra_activation_costs.is_empty()
        || !scratch.activation_cost_reductions.is_empty()
        || !scratch.activation_target_cost_reductions.is_empty()
        || !scratch.static_conditions.is_empty()
        || !scratch.zone_replacements.is_empty()
        || !scratch.entry_counter_abilities.is_empty()
        || !scratch.cast_reductions.is_empty()
        || !scratch.additional_cast_costs.is_empty()
        || !scratch.face_up_abilities.is_empty()
        || scratch.conditional_cost.is_some()
        || scratch.exile_action.is_some()
        || scratch.flashback.is_some()
        || scratch.madness.is_some()
        || scratch.miracle.is_some()
        || scratch.plot.is_some()
        || scratch.gift.is_some()
        || scratch.casualty.is_some()
        || scratch.dredge.is_some()
        || scratch.conspire
        || scratch.assist
        || scratch.cipher
        || scratch.printed_toxic.is_some()
    {
        return Err(LoweringError::Unsupported(
            "v2:granted:auxiliary-program-state",
        ));
    }

    let mut activated = Vec::with_capacity(scratch.activated.len());
    let mut activated_restricted = Vec::new();
    for ab in scratch.activated {
        if let Some(res) = scratch.activation_restrictions.remove(&ab.id) {
            activated_restricted.push((ab, res));
        } else {
            activated.push(ab);
        }
    }
    if !scratch.activation_restrictions.is_empty() {
        return Err(LoweringError::Unsupported(
            "v2:granted:auxiliary-program-state",
        ));
    }

    let mut statics = Vec::new();
    if let Some(primary) = scratch.continuous {
        statics.push(primary);
    }
    statics.extend(scratch.additional_battlefield_abilities);

    Ok((
        GrantedAbilities {
            keywords: scratch.printed_keywords,
            activated,
            triggered: scratch.triggered,
            statics,
            mana: scratch.mana,
        },
        activated_restricted,
    ))
}

/// Lower one strictly-compiled `CompiledFaceV2` together with its printed `CatalogFace` metadata
/// into an engine [`Program`]. Caller is responsible for calling `Program::validate`.
pub(crate) fn lower_face_v2(
    card: &InventoryCard,
    face: &CatalogFace,
    compiled: &CompiledFaceV2,
    opts: &FaceMetaOptions,
    vocab: &CreatureTypeVocabulary,
) -> LowerResult<Program> {
    lower_face_v2_inner(
        card,
        face,
        compiled,
        opts,
        vocab,
        FaceLayoutFlags::default(),
    )
}

#[derive(Debug, Clone, Copy, Default)]
struct FaceLayoutFlags {
    allow_aftermath: bool,
    allow_fuse: bool,
    allow_disturb: bool,
}

fn lower_face_v2_inner(
    card: &InventoryCard,
    face: &CatalogFace,
    compiled: &CompiledFaceV2,
    opts: &FaceMetaOptions,
    vocab: &CreatureTypeVocabulary,
    layout_flags: FaceLayoutFlags,
) -> LowerResult<Program> {
    if !compiled.is_ok() {
        return Err(LoweringError::ParseIncomplete);
    }

    let mut program = build_base_program(card, face, opts, vocab)?;
    let mut ctx = LowerCtx::new(vocab, &card.name, &face.name);
    ctx.has_only_single_ability = compiled.abilities.len() == 1
        && compiled.keywords.is_empty()
        && compiled.statics.is_empty()
        && compiled.replacements.is_empty();
    let is_spell = matches!(program.kind, ProgramKind::Spell(_));

    // Wire Siege defeated trigger (exile defeated Siege, then cast transformed).
    if program.has_type(CardType::Battle)
        && program
            .permanent()
            .is_some_and(|p| p.entry_tap == Some(EntryTap::SiegeProtector))
    {
        program.triggered.push(TriggeredAbility {
            id: ctx.alloc_triggered_id(),
            grouping: TriggerGrouping::EachOccurrence,
            condition: TriggerSpec::counters_changed(
                ObjectFilter::self_object(),
                CounterTriggerKind::LastDefenseRemoved,
            ),
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::MutateObject(Box::new(MutateObjectSpec::exile_defeated_siege())),
                    Instruction::StackOp(Box::new(StackOpSpec::cast_defeated_siege())),
                ],
            ),
        });
    }

    // 1. Lower spell effects for Instant/Sorcery faces (or reject spell effects on permanents).
    let mut spell_exec_effects: Vec<v2::Effect> = Vec::new();
    if is_spell {
        let mut spell_restrictions: Vec<v2::ActivationRestriction> = Vec::new();
        for eff in &compiled.spell_effects {
            if let v2::Effect::Restriction(rs) = eff {
                spell_restrictions.extend(rs.iter().cloned());
            } else if let v2::Effect::BranchCondition {
                condition,
                then_body,
                else_body,
            } = eff
                && else_body.is_empty()
                && let [
                    v2::Effect::ApplyContinuous {
                        effect,
                        duration: v2::Duration::Instant | v2::Duration::Permanent,
                    },
                ] = then_body.as_slice()
                && let v2::StaticAbility::AlternativeCost(costs) = effect.as_ref()
            {
                crate::v2_lowering::cost::lower_conditional_alternative_cast_costs(
                    condition,
                    costs,
                    &mut program,
                    &mut ctx,
                )?;
            } else {
                spell_exec_effects.push(eff.clone());
            }
        }
        if spell_exec_effects.is_empty() {
            return Err(LoweringError::Unsupported("v2:spell:empty-effects"));
        }
        if !spell_restrictions.is_empty() {
            program.cast_timing = lower_cast_timing_restrictions(&spell_restrictions, &mut ctx)?;
        }
        let prev_is_spell = ctx.is_spell_body;
        ctx.is_spell_body = true;
        let spell_res = lower_spell_effects_v2(&spell_exec_effects, &mut ctx);
        ctx.is_spell_body = prev_is_spell;
        let (body, modes) = spell_res?;
        if let ProgramKind::Spell(spell) = &mut program.kind {
            spell.body = body;
            spell.modes = modes;
        }
    } else if !compiled.spell_effects.is_empty() {
        return Err(LoweringError::Unsupported("v2:permanent:has-spell-effects"));
    }

    // 2. Lower face keywords, preserving context-dependent hooks (`ReadAhead`, `Prototype`, `Backup`, `Overload`).
    let mut has_read_ahead = false;
    let mut prototype_spec: Option<(ManaCost, u8, Vec<HybridSymbol>, PrintedPowerToughness)> = None;

    for (kw_idx, kw) in compiled.keywords.iter().enumerate() {
        if kw.kind == v2::KeywordKind::Overload && is_spell {
            let Some(v2::KeywordParam::Cost(costs)) = &kw.param else {
                return Err(LoweringError::Unsupported("v2:kw:overload"));
            };
            let [v2::Cost::Mana(mc)] = costs.as_slice() else {
                return Err(LoweringError::Unsupported("v2:kw:overload"));
            };
            let (overload_mana, overload_x, overload_hybrids, overload_snow) =
                lower_v2_mana_cost(mc)?;
            if overload_x != 0 || !overload_hybrids.is_empty() || overload_snow != 0 {
                return Err(LoweringError::Unsupported("v2:kw:overload"));
            }
            if program.conditional_cost.is_some() {
                return Err(LoweringError::Unsupported("v2:kw:overload"));
            }
            let (rewritten, changed) = rewrite_effects_for_overload(&spell_exec_effects)?;
            if !changed {
                return Err(LoweringError::Unsupported("v2:kw:overload"));
            }
            let prev_is_spell = ctx.is_spell_body;
            ctx.is_spell_body = true;
            let each_res = crate::v2_lowering::effect::lower_effect_body(&rewritten, &mut ctx);
            ctx.is_spell_body = prev_is_spell;
            let each_body = each_res?;
            if each_body.target != BodyTarget::None
                || each_body
                    .instructions
                    .iter()
                    .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
            {
                return Err(LoweringError::Unsupported("v2:kw:overload"));
            }
            program.conditional_cost =
                Some(spellwake_engine::program::ConditionalCastCost::Overload {
                    mana: overload_mana,
                    each: Box::new(each_body),
                });
            continue;
        }
        if layout_flags.allow_aftermath
            && kw.kind == v2::KeywordKind::Aftermath
            && kw.param.is_none()
        {
            continue;
        }
        if layout_flags.allow_fuse && kw.kind == v2::KeywordKind::Fuse && kw.param.is_none() {
            continue;
        }
        if layout_flags.allow_disturb && kw.kind == v2::KeywordKind::Disturb {
            continue;
        }
        if kw.kind == v2::KeywordKind::ReadAhead
            && kw.param.is_none()
            && program
                .enchantment_subtypes
                .contains(&EnchantmentSubtype::Saga)
        {
            has_read_ahead = true;
            continue;
        }
        if kw.kind == v2::KeywordKind::Prototype {
            let Some(v2::KeywordParam::CostAndBasePt(costs, p, t)) = &kw.param else {
                return Err(LoweringError::Unsupported("v2:kw:prototype-param"));
            };
            let [v2::Cost::Mana(mc)] = costs.as_slice() else {
                return Err(LoweringError::Unsupported("v2:kw:prototype-cost"));
            };
            let (proto_cost, proto_x, proto_hybrids, proto_snow) = lower_v2_mana_cost(mc)?;
            if proto_snow != 0 {
                return Err(LoweringError::Unsupported("v2:kw:prototype-snow"));
            }
            let proto_pt = PrintedPowerToughness {
                power: PrintedStat::Number(i64::from(*p).into()),
                toughness: PrintedStat::Number(i64::from(*t).into()),
            };
            if prototype_spec
                .replace((proto_cost, proto_x, proto_hybrids, proto_pt))
                .is_some()
            {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-prototype"));
            }
            continue;
        }
        if kw.kind == v2::KeywordKind::Backup {
            if !compiled.abilities.is_empty()
                || !compiled.statics.is_empty()
                || !compiled.replacements.is_empty()
                || !compiled.spell_effects.is_empty()
            {
                return Err(LoweringError::Unsupported("v2:kw:backup-complex-face"));
            }
            let Some(v2::KeywordParam::Number(n)) = kw.param else {
                return Err(LoweringError::Unsupported("v2:kw:backup-param"));
            };
            if n == 0 {
                return Err(LoweringError::Unsupported("v2:kw:backup-zero"));
            }
            let mut printed_below = BTreeSet::new();
            for next_kw in &compiled.keywords[kw_idx + 1..] {
                if next_kw.kind == v2::KeywordKind::Backup {
                    continue;
                }
                if next_kw.param.is_some() {
                    return Err(LoweringError::Unsupported("v2:kw:backup-param-below"));
                }
                let ka = simple_keyword_ability(next_kw.kind)
                    .ok_or(LoweringError::Unsupported("v2:kw:backup-unsupported-below"))?;
                printed_below.insert(ka);
            }
            program.triggered.push(
                GrowthKeyword::Backup {
                    amount: i64::from(n).into(),
                    printed_below,
                }
                .ability(ctx.alloc_triggered_id()),
            );
            continue;
        }
        apply_face_keyword(kw, &mut program, &mut ctx)?;
    }

    // Verify Aura faces received an `Enchant` attachment restriction.
    if let Some(tl) = face.type_line.as_deref()
        && let Ok(parsed_tl) = parse_type_line(tl, vocab)
        && parsed_tl.is_aura
        && program.permanent().is_some_and(|p| p.attachment.is_none())
    {
        return Err(LoweringError::InvalidMetadata(
            "v2:meta:aura-missing-enchant",
        ));
    }

    // 3. Lower replacement effects.
    for rep in &compiled.replacements {
        lower_replacement_into_program(rep, &mut program, &mut ctx)?;
    }

    // 4. Lower static abilities.
    for st in &compiled.statics {
        lower_static_into_program(st, &mut program, &mut ctx)?;
    }

    // 5. Lower abilities (activated, triggered, loyalty, saga chapters, class levels, table rows).
    let is_class = program
        .enchantment_subtypes
        .contains(&EnchantmentSubtype::Class);
    let mut current_class_level: Option<u8> = None;
    let mut table_rows: Vec<(u16, Option<u16>, bool, &v2::TableRowPayload)> = Vec::new();

    for ab in &compiled.abilities {
        match ab {
            v2::Ability::Activated {
                cost,
                body,
                restrictions,
            } => {
                if let Some(lvl) = extract_class_level_up_level(body) {
                    lower_activated_ability(cost, body, restrictions, &mut program, &mut ctx)?;
                    if is_class {
                        current_class_level = Some(lvl);
                    }
                    continue;
                }
                let prev_act_len = program.activated.len();
                lower_activated_ability(cost, body, restrictions, &mut program, &mut ctx)?;
                if is_class && let Some(lvl) = current_class_level {
                    for added in &program.activated[prev_act_len..] {
                        let existing = program.activation_restrictions.remove(&added.id);
                        program.activation_restrictions.insert(
                            added.id,
                            combine_restrictions(
                                existing,
                                ActivationRestriction::MinClassLevel(lvl),
                            ),
                        );
                    }
                }
            }
            v2::Ability::Triggered { trigger, body } => {
                let mut lowered = lower_triggered_ability(trigger, body, &mut ctx)?;
                if is_class && let Some(lvl) = current_class_level {
                    let gate = TriggerCondition::SourceClassLevelAtLeast(lvl);
                    for tr in &mut lowered {
                        let is_level_reached = matches!(
                            tr.condition.event,
                            TriggerEvent::StatusChanged {
                                kind: StatusTriggerKind::ClassLevelReached(_),
                                ..
                            }
                        );
                        if !is_level_reached {
                            tr.condition.intervening_if = match tr.condition.intervening_if.take() {
                                Some(existing) => Some(TriggerCondition::And(
                                    Box::new(gate.clone()),
                                    Box::new(existing),
                                )),
                                None => Some(gate.clone()),
                            };
                        }
                    }
                }
                program.triggered.extend(lowered);
            }
            v2::Ability::Loyalty { delta, body } => {
                lower_loyalty_ability(*delta, body, &mut program, &mut ctx)?;
            }
            v2::Ability::SagaChapter { chapters, body } => {
                lower_saga_chapter_ability(chapters, body, &mut program, &mut ctx)?;
            }
            v2::Ability::TableRow {
                min,
                max,
                open_ended,
                payload,
            } => {
                table_rows.push((*min, *max, *open_ended, payload));
            }
            v2::Ability::StickerRow { .. } => {
                return Err(LoweringError::Unsupported("v2:ability:sticker-row"));
            }
            v2::Ability::CaseSolveCondition(_) | v2::Ability::CaseSolved(_) => {
                return Err(LoweringError::Unsupported("v2:ability:case-solve"));
            }
        }
    }

    if !table_rows.is_empty() {
        let has_station = compiled
            .keywords
            .iter()
            .any(|k| k.kind == v2::KeywordKind::Station);
        if has_station
            && table_rows.iter().all(|(min, max, open_ended, payload)| {
                *min > 0
                    && max.is_none()
                    && *open_ended
                    && matches!(payload, v2::TableRowPayload::Face(_))
            })
        {
            lower_station_table_rows(&table_rows, &mut program, &mut ctx)?;
        } else if table_rows
            .iter()
            .all(|(_, _, _, payload)| matches!(payload, v2::TableRowPayload::Effects(_)))
        {
            lower_die_roll_table_rows(&table_rows, &mut program, &mut ctx)?;
        } else {
            return Err(LoweringError::Unsupported("v2:ability:table-row"));
        }
    }

    // 6. Wire post-lowering Saga Read Ahead and Prototype hooks.
    if has_read_ahead {
        let final_chapter = program
            .triggered
            .iter()
            .filter_map(|t| match t.condition.event {
                spellwake_engine::program::TriggerEvent::CountersChanged {
                    kind: CounterTriggerKind::SagaChapter(n),
                    ..
                } => Some(n),
                _ => None,
            })
            .max()
            .ok_or(LoweringError::Unsupported("v2:saga:read-ahead-no-chapters"))?;
        if let ProgramKind::Permanent(def) = &mut program.kind
            && def
                .entry_tap
                .replace(EntryTap::ReadAhead { final_chapter })
                .is_some()
        {
            return Err(LoweringError::Unsupported("v2:saga:duplicate-entry-tap"));
        }
    }

    if let Some((proto_cost, proto_x, proto_hybrids, proto_pt)) = prototype_spec {
        let mut proto = program.clone();
        proto.prototype = None;
        proto.x_mana_symbols = proto_x;
        proto.hybrid_symbols = proto_hybrids;
        if let ProgramKind::Permanent(def) = &mut proto.kind {
            def.casting_cost = Some(proto_cost);
            def.printed_pt = Some(proto_pt);
        } else {
            return Err(LoweringError::Unsupported("v2:kw:prototype-non-permanent"));
        }
        program.prototype = Some(Box::new(proto));
    }

    Ok(program)
}

/// Strictly compile and lower a single-face or multi-face card via `v2` into a validated
/// [`Program`], returning `None` on any error.
#[must_use]
pub fn compile_v2(card: &InventoryCard, vocab: &CreatureTypeVocabulary) -> Option<Program> {
    compile_v2_with_reason(card, vocab).ok()
}

/// Strictly compile and lower a card via `v2` without external inventory metadata (no
/// source-verified planeswalker loyalty, battle defense, or back-face color indicators).
pub fn compile_v2_with_reason(
    card: &InventoryCard,
    vocab: &CreatureTypeVocabulary,
) -> LowerResult<Program> {
    compile_v2_with_inventory(card, None, vocab, false)
}

/// Strictly compile and lower a card via `v2`, optionally consulting `inventory` for
/// source-authenticated planeswalker loyalty, battle defense, and back-face color indicators.
pub fn compile_v2_with_inventory(
    card: &InventoryCard,
    inventory: Option<&Inventory>,
    vocab: &CreatureTypeVocabulary,
    loyalty_source_verified: bool,
) -> LowerResult<Program> {
    match card.layout.as_str() {
        "split" => compile_v2_split(card, vocab),
        "adventure" => compile_v2_adventure(card, vocab),
        "transform" => {
            let indicator = card
                .oracle_id
                .as_deref()
                .and_then(|id| inventory.and_then(|inv| inv.back_color_indicator(id)));
            compile_v2_transforming_inner(
                card,
                inventory,
                vocab,
                indicator,
                loyalty_source_verified,
            )
        }
        "modal_dfc" => {
            let indicator = card
                .oracle_id
                .as_deref()
                .and_then(|id| inventory.and_then(|inv| inv.back_color_indicator(id)));
            compile_v2_modal_dfc_inner(card, inventory, vocab, indicator, loyalty_source_verified)
        }
        "flip" => compile_v2_flip(card, vocab),
        "prepare" => compile_v2_prepare(card, vocab),
        "normal" | "saga" | "mutate" | "prototype" | "leveler" | "class" | "case" | "meld" => {
            compile_v2_single_face(card, inventory, vocab, loyalty_source_verified)
        }
        _ => Err(LoweringError::InvalidMetadata("v2:meta:unsupported-layout")),
    }
}

/// Strictly compile a transforming double-faced card via `v2` with source-authenticated back-face
/// color indicator, loyalty, and defense.
#[must_use]
pub fn compile_v2_transforming(
    card: &InventoryCard,
    inventory: &Inventory,
    vocab: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
    loyalty_source_verified: bool,
) -> Option<Program> {
    compile_v2_transforming_inner(
        card,
        Some(inventory),
        vocab,
        indicator,
        loyalty_source_verified,
    )
    .ok()
}

/// Strictly compile a modal double-faced card via `v2` with optional source-authenticated loyalty
/// and back-face color indicator.
#[must_use]
pub fn compile_v2_modal_dfc(
    card: &InventoryCard,
    inventory: &Inventory,
    vocab: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
    loyalty_source_verified: bool,
) -> Option<Program> {
    compile_v2_modal_dfc_inner(
        card,
        Some(inventory),
        vocab,
        indicator,
        loyalty_source_verified,
    )
    .ok()
}

/// Strictly compile a single-face planeswalker card via `v2` with source-authenticated loyalty.
#[must_use]
pub fn compile_v2_planeswalker(
    card: &InventoryCard,
    inventory: &Inventory,
    vocab: &CreatureTypeVocabulary,
    loyalty_source_verified: bool,
) -> Option<Program> {
    if !loyalty_source_verified {
        return None;
    }
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || card.name != face.name {
        return None;
    }
    let tl = face.type_line.as_deref()?;
    let parsed_tl = parse_type_line(tl, vocab).ok()?;
    if !parsed_tl.card_types.contains(&CardType::Planeswalker) {
        return None;
    }
    compile_v2_single_face(card, Some(inventory), vocab, loyalty_source_verified).ok()
}

fn compile_v2_single_face(
    card: &InventoryCard,
    inventory: Option<&Inventory>,
    vocab: &CreatureTypeVocabulary,
    loyalty_source_verified: bool,
) -> LowerResult<Program> {
    let [face] = card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata(
            "v2:meta:multi-face-single-layout",
        ));
    };
    if face.name != card.name {
        return Err(LoweringError::InvalidMetadata("v2:meta:face-name-mismatch"));
    }
    let type_line = face
        .type_line
        .as_deref()
        .ok_or(LoweringError::InvalidMetadata("v2:meta:missing-type-line"))?;
    let parsed_tl = parse_type_line(type_line, vocab)?;
    let printed_loyalty =
        if loyalty_source_verified && parsed_tl.card_types.contains(&CardType::Planeswalker) {
            lookup_loyalty(inventory, card.oracle_id.as_deref(), 0)
        } else {
            None
        };

    let compiled_card = run_v2_compiler(card, &[face], vocab)?;
    let [compiled_face] = compiled_card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata(
            "v2:meta:face-count-mismatch",
        ));
    };

    let program = lower_face_v2(
        card,
        face,
        compiled_face,
        &FaceMetaOptions {
            printed_loyalty,
            printed_defense: None,
            color_indicator: None,
            allow_empty_mana_cost: false,
        },
        vocab,
    )?;
    program.validate(vocab)?;
    Ok(program)
}

fn compile_v2_split(card: &InventoryCard, vocab: &CreatureTypeVocabulary) -> LowerResult<Program> {
    let [left_face, right_face] = card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata("v2:meta:split-face-count"));
    };
    let compiled_card = run_v2_compiler(card, &[left_face, right_face], vocab)?;
    let [left_ir, right_ir] = compiled_card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata("v2:meta:split-ir-count"));
    };

    let aftermath = right_ir
        .keywords
        .iter()
        .any(|k| k.kind == v2::KeywordKind::Aftermath && k.param.is_none());
    let has_fuse = |f: &CompiledFaceV2| {
        f.keywords
            .iter()
            .any(|k| k.kind == v2::KeywordKind::Fuse && k.param.is_none())
    };
    let fuse = !aftermath && (has_fuse(left_ir) || has_fuse(right_ir));

    let mut left = lower_face_v2_inner(
        card,
        left_face,
        left_ir,
        &FaceMetaOptions::default(),
        vocab,
        FaceLayoutFlags {
            allow_aftermath: false,
            allow_fuse: fuse,
            allow_disturb: false,
        },
    )?;
    let mut right = lower_face_v2_inner(
        card,
        right_face,
        right_ir,
        &FaceMetaOptions::default(),
        vocab,
        FaceLayoutFlags {
            allow_aftermath: aftermath,
            allow_fuse: fuse,
            allow_disturb: false,
        },
    )?;

    left.names = single_face_name(&left_face.name)?;
    right.names = single_face_name(&right_face.name)?;

    let combined = SplitCard {
        left,
        right,
        aftermath,
        fuse,
    }
    .into_program()
    .ok_or(LoweringError::InvalidMetadata(
        "v2:meta:split-combine-failed",
    ))?;
    combined.validate(vocab)?;
    Ok(combined)
}

fn compile_v2_adventure(
    card: &InventoryCard,
    vocab: &CreatureTypeVocabulary,
) -> LowerResult<Program> {
    let [front_face, adv_face] = card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata(
            "v2:meta:adventure-face-count",
        ));
    };
    let compiled_card = run_v2_compiler(card, &[front_face, adv_face], vocab)?;
    let [front_ir, adv_ir] = compiled_card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata("v2:meta:adventure-ir-count"));
    };

    let mut front = lower_face_v2(
        card,
        front_face,
        front_ir,
        &FaceMetaOptions::default(),
        vocab,
    )?;
    let mut adventure = lower_face_v2(card, adv_face, adv_ir, &FaceMetaOptions::default(), vocab)?;
    front.names = single_face_name(&front_face.name)?;
    adventure.names = single_face_name(&adv_face.name)?;
    adventure.spell_subtypes.insert(SpellSubtype::Adventure);
    front.adventure = Some(Box::new(adventure));
    front.validate(vocab)?;
    Ok(front)
}

fn compile_v2_transforming_inner(
    card: &InventoryCard,
    inventory: Option<&Inventory>,
    vocab: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
    loyalty_source_verified: bool,
) -> LowerResult<Program> {
    let [front_face, back_face] = card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata(
            "v2:meta:transform-face-count",
        ));
    };
    if card.layout != "transform"
        || !back_face
            .mana_cost
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        return Err(LoweringError::InvalidMetadata(
            "v2:meta:transform-invalid-back",
        ));
    }

    let front_tl = front_face
        .type_line
        .as_deref()
        .ok_or(LoweringError::InvalidMetadata("v2:meta:missing-type-line"))?;
    let back_tl = back_face
        .type_line
        .as_deref()
        .ok_or(LoweringError::InvalidMetadata("v2:meta:missing-type-line"))?;
    let front_parsed = parse_type_line(front_tl, vocab)?;
    let back_parsed = parse_type_line(back_tl, vocab)?;

    let front_loyalty =
        if loyalty_source_verified && front_parsed.card_types.contains(&CardType::Planeswalker) {
            lookup_loyalty(inventory, card.oracle_id.as_deref(), 0)
        } else {
            None
        };
    let back_loyalty =
        if loyalty_source_verified && back_parsed.card_types.contains(&CardType::Planeswalker) {
            lookup_loyalty(inventory, card.oracle_id.as_deref(), 1)
        } else {
            None
        };
    let front_defense = if front_parsed.card_types.contains(&CardType::Battle) {
        lookup_defense(inventory, card.oracle_id.as_deref(), 0)
    } else {
        None
    };

    let compiled_card = run_v2_compiler(card, &[front_face, back_face], vocab)?;
    let [front_ir, back_ir] = compiled_card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata("v2:meta:transform-ir-count"));
    };

    let mut disturb_cost: Option<ManaCost> = None;
    for kw in &front_ir.keywords {
        if kw.kind == v2::KeywordKind::Disturb {
            let Some(v2::KeywordParam::Cost(costs)) = &kw.param else {
                return Err(LoweringError::Unsupported("v2:kw:disturb-missing-cost"));
            };
            let [v2::Cost::Mana(mc)] = costs.as_slice() else {
                return Err(LoweringError::Unsupported("v2:kw:disturb-non-mana-cost"));
            };
            let cost = lower_v2_fixed_mana_cost(mc)?;
            if disturb_cost.replace(cost).is_some() {
                return Err(LoweringError::Unsupported("v2:kw:duplicate-disturb"));
            }
        }
    }

    let mut front = lower_face_v2_inner(
        card,
        front_face,
        front_ir,
        &FaceMetaOptions {
            printed_loyalty: front_loyalty,
            printed_defense: front_defense,
            color_indicator: None,
            allow_empty_mana_cost: false,
        },
        vocab,
        FaceLayoutFlags {
            allow_aftermath: false,
            allow_fuse: false,
            allow_disturb: disturb_cost.is_some(),
        },
    )?;
    let mut back = lower_face_v2(
        card,
        back_face,
        back_ir,
        &FaceMetaOptions {
            printed_loyalty: back_loyalty,
            printed_defense: None,
            color_indicator: indicator.map(<[String]>::to_vec),
            allow_empty_mana_cost: true,
        },
        vocab,
    )?;

    if let Some(ind) = indicator {
        let colors = parse_color_indicator(ind)?;
        if !colors.is_empty() {
            back.color_indicator = Some(colors);
        }
    }

    front.names = single_face_name(&front_face.name)?;
    back.names = single_face_name(&back_face.name)?;

    if let Some(cost) = disturb_cost {
        front.disturb = Some(Box::new(Disturb { cost, back }));
    } else if front.has_type(CardType::Battle) {
        front.siege_back = Some(Box::new(back));
    } else if front.printed_keywords.contains(&KeywordAbility::Daybound)
        && back.printed_keywords.contains(&KeywordAbility::Nightbound)
    {
        front.nightbound_back = Some(Box::new(back));
    } else {
        front.transform_back = Some(Box::new(back));
    }

    front.validate(vocab)?;
    Ok(front)
}

fn compile_v2_modal_dfc_inner(
    card: &InventoryCard,
    inventory: Option<&Inventory>,
    vocab: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
    loyalty_source_verified: bool,
) -> LowerResult<Program> {
    let [front_face, back_face] = card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata("v2:meta:mdfc-face-count"));
    };
    if card.layout != "modal_dfc" {
        return Err(LoweringError::InvalidMetadata("v2:meta:not-modal-dfc"));
    }

    let front_tl = front_face
        .type_line
        .as_deref()
        .ok_or(LoweringError::InvalidMetadata("v2:meta:missing-type-line"))?;
    let back_tl = back_face
        .type_line
        .as_deref()
        .ok_or(LoweringError::InvalidMetadata("v2:meta:missing-type-line"))?;
    let front_parsed = parse_type_line(front_tl, vocab)?;
    let back_parsed = parse_type_line(back_tl, vocab)?;

    let front_loyalty =
        if loyalty_source_verified && front_parsed.card_types.contains(&CardType::Planeswalker) {
            lookup_loyalty(inventory, card.oracle_id.as_deref(), 0)
        } else {
            None
        };
    let back_loyalty =
        if loyalty_source_verified && back_parsed.card_types.contains(&CardType::Planeswalker) {
            lookup_loyalty(inventory, card.oracle_id.as_deref(), 1)
        } else {
            None
        };

    let compiled_card = run_v2_compiler(card, &[front_face, back_face], vocab)?;
    let [front_ir, back_ir] = compiled_card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata("v2:meta:mdfc-ir-count"));
    };

    let mut front = lower_face_v2(
        card,
        front_face,
        front_ir,
        &FaceMetaOptions {
            printed_loyalty: front_loyalty,
            printed_defense: None,
            color_indicator: None,
            allow_empty_mana_cost: false,
        },
        vocab,
    )?;
    let mut back = lower_face_v2(
        card,
        back_face,
        back_ir,
        &FaceMetaOptions {
            printed_loyalty: back_loyalty,
            printed_defense: None,
            color_indicator: indicator.map(<[String]>::to_vec),
            allow_empty_mana_cost: false,
        },
        vocab,
    )?;

    front.names = single_face_name(&front_face.name)?;
    back.names = single_face_name(&back_face.name)?;

    if front.has_type(CardType::Land)
        && back.has_type(CardType::Land)
        && let ProgramKind::Permanent(def) = &mut front.kind
        && def.entry_tap.replace(EntryTap::ChooseModalFace).is_some()
    {
        return Err(LoweringError::Unsupported("v2:mdfc:duplicate-entry-tap"));
    }

    front.modal_back = Some(Box::new(back));
    front.validate(vocab)?;
    Ok(front)
}

fn compile_v2_flip(card: &InventoryCard, vocab: &CreatureTypeVocabulary) -> LowerResult<Program> {
    let [front_face, back_face] = card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata("v2:meta:flip-face-count"));
    };
    let compiled_card = run_v2_compiler(card, &[front_face, back_face], vocab)?;
    let [front_ir, back_ir] = compiled_card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata("v2:meta:flip-ir-count"));
    };

    let mut front = lower_face_v2(
        card,
        front_face,
        front_ir,
        &FaceMetaOptions::default(),
        vocab,
    )?;
    let mut back = lower_face_v2(
        card,
        back_face,
        back_ir,
        &FaceMetaOptions {
            allow_empty_mana_cost: true,
            ..FaceMetaOptions::default()
        },
        vocab,
    )?;

    // CR 710.4: Flipped permanent retains the unflipped half's mana cost and color.
    if let (ProgramKind::Permanent(fdef), ProgramKind::Permanent(bdef)) =
        (&front.kind, &mut back.kind)
    {
        bdef.casting_cost = fdef.casting_cost.clone();
    }
    back.hybrid_symbols = front.hybrid_symbols.clone();
    back.x_mana_symbols = front.x_mana_symbols;
    back.color_indicator = front.color_indicator.clone();
    front.names = single_face_name(&front_face.name)?;
    back.names = single_face_name(&back_face.name)?;
    front.flip_back = Some(Box::new(back));
    front.validate(vocab)?;
    Ok(front)
}

fn compile_v2_prepare(
    card: &InventoryCard,
    vocab: &CreatureTypeVocabulary,
) -> LowerResult<Program> {
    let [front_face, spell_face] = card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata("v2:meta:prepare-face-count"));
    };
    let compiled_card = run_v2_compiler(card, &[front_face, spell_face], vocab)?;
    let [front_ir, spell_ir] = compiled_card.faces.as_slice() else {
        return Err(LoweringError::InvalidMetadata("v2:meta:prepare-ir-count"));
    };

    let mut front = lower_face_v2(
        card,
        front_face,
        front_ir,
        &FaceMetaOptions::default(),
        vocab,
    )?;
    let mut spell = lower_face_v2(
        card,
        spell_face,
        spell_ir,
        &FaceMetaOptions::default(),
        vocab,
    )?;
    front.names = single_face_name(&front_face.name)?;
    spell.names = single_face_name(&spell_face.name)?;
    front.prepare_spell = Some(Box::new(spell));
    front.validate(vocab)?;
    Ok(front)
}

fn run_v2_compiler(
    card: &InventoryCard,
    faces: &[&CatalogFace],
    vocab: &CreatureTypeVocabulary,
) -> LowerResult<v2::CompiledCardV2> {
    let oracle_vocab = cached_oracle_vocab(vocab);
    let mut face_inputs = Vec::with_capacity(faces.len());
    for face in faces {
        let type_line = face
            .type_line
            .as_deref()
            .ok_or(LoweringError::InvalidMetadata("v2:meta:missing-type-line"))?;
        face_inputs.push(FaceInput {
            name: &face.name,
            type_line,
            oracle_text: face.oracle_text.as_deref().unwrap_or(""),
        });
    }
    let input = CardInput {
        oracle_id: card.oracle_id.as_deref().unwrap_or(""),
        name: &card.name,
        layout: &card.layout,
        faces: face_inputs,
    };
    let compiled = compile_card_v2(&input, &oracle_vocab);
    if !compiled.strictly_compiled {
        return Err(LoweringError::ParseIncomplete);
    }
    Ok(compiled)
}

fn lower_spell_effects_v2(
    effects: &[v2::Effect],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(ExecutableBody, Option<SpellModes>)> {
    // 1. Spree / Tiered modal spell (`Effect::PromptChoice(ChoiceKind::Spree)`).
    if let [
        v2::Effect::PromptChoice {
            chooser: v2::PlayerRef::You,
            kind: v2::ChoiceKind::Spree { modes },
        },
    ] = effects
    {
        if modes.len() < 2 {
            return Err(LoweringError::Unsupported("v2:spell:spree-too-few"));
        }
        let mut mode_costs = Vec::with_capacity(modes.len());
        let mut bodies = Vec::with_capacity(modes.len());
        for mode in modes {
            let [v2::Cost::Mana(mc)] = mode.cost.as_slice() else {
                return Err(LoweringError::Unsupported("v2:spell:spree-non-mana-cost"));
            };
            mode_costs.push(lower_v2_fixed_mana_cost(mc)?);
            bodies.push(lower_effect_body(&mode.body, ctx)?);
        }
        let max = u8::try_from(bodies.len())
            .map_err(|_| LoweringError::Unsupported("v2:spell:spree-too-many"))?;
        let first = bodies.remove(0);
        return Ok((
            first,
            Some(SpellModes {
                minimum: 1,
                maximum: max,
                conditional_maximum: None,
                allow_repeat: false,
                distinct_targets: false,
                uniqueness: ModalUniqueness::None,
                mode_costs,
                escalate_mana: None,
                prefix: None,
                suffix: None,
                additional: bodies,
            }),
        ));
    }

    // 2. Standard modal spell (`Choose one - ...`, with optional prefix/suffix).
    if let Some((prefix_effs, choice_count, modes, suffix_effs)) = extract_modal_choice(effects) {
        if modes.len() < 2 {
            return Err(LoweringError::Unsupported("v2:spell:modal-too-few"));
        }
        let (min_modes, max_modes) = lower_choice_count(choice_count, modes.len())?;
        let prefix = if prefix_effs.is_empty() {
            None
        } else {
            let pb = lower_effect_body(prefix_effs, ctx)?;
            if pb.target != BodyTarget::None
                || pb
                    .instructions
                    .iter()
                    .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
            {
                return Err(LoweringError::Unsupported("v2:spell:modal-prefix-target"));
            }
            Some(Box::new(pb))
        };
        let suffix = if suffix_effs.is_empty() {
            None
        } else {
            let sb = lower_effect_body(suffix_effs, ctx)?;
            if sb.target != BodyTarget::None
                || sb
                    .instructions
                    .iter()
                    .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
            {
                return Err(LoweringError::Unsupported("v2:spell:modal-suffix-target"));
            }
            Some(Box::new(sb))
        };
        let mut lowered_modes = Vec::with_capacity(modes.len());
        for m in modes {
            let mb = lower_effect_body(&m.body, ctx)?;
            if mb.instructions.is_empty() {
                return Err(LoweringError::Unsupported("v2:spell:modal-empty-mode"));
            }
            lowered_modes.push(mb);
        }
        let first = lowered_modes.remove(0);
        return Ok((
            first,
            Some(SpellModes {
                minimum: min_modes,
                maximum: max_modes,
                conditional_maximum: None,
                allow_repeat: false,
                distinct_targets: false,
                uniqueness: ModalUniqueness::None,
                mode_costs: Vec::new(),
                escalate_mana: None,
                prefix,
                suffix,
                additional: lowered_modes,
            }),
        ));
    }

    // 3. Straight-line spell body.
    let body = lower_effect_body(effects, ctx)?;
    Ok((body, None))
}

fn lower_loyalty_ability(
    delta: v2::LoyaltyDelta,
    body: &[v2::Effect],
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let mut inline_restrictions = Vec::new();
    let mut exec_effects = Vec::with_capacity(body.len());
    for eff in body {
        if let v2::Effect::Restriction(rs) = eff {
            inline_restrictions.extend(rs.iter().cloned());
        } else {
            exec_effects.push(eff.clone());
        }
    }
    let restriction = lower_activation_restrictions(&inline_restrictions, ctx)?;
    let (exec_body, modes) = if let Some((prefix_effs, choice_count, m_list, suffix_effs)) =
        extract_modal_choice(&exec_effects)
    {
        if m_list.len() < 2 {
            return Err(LoweringError::Unsupported("v2:loyalty:modal-too-few"));
        }
        let (min_m, max_m) = lower_choice_count(choice_count, m_list.len())?;
        let prefix = if prefix_effs.is_empty() {
            None
        } else {
            let pb = lower_effect_body(prefix_effs, ctx)?;
            if pb.target != BodyTarget::None {
                return Err(LoweringError::Unsupported("v2:loyalty:modal-prefix-target"));
            }
            Some(Box::new(pb))
        };
        let suffix = if suffix_effs.is_empty() {
            None
        } else {
            let sb = lower_effect_body(suffix_effs, ctx)?;
            if sb.target != BodyTarget::None {
                return Err(LoweringError::Unsupported("v2:loyalty:modal-suffix-target"));
            }
            Some(Box::new(sb))
        };
        let mut lowered = Vec::with_capacity(m_list.len());
        for m in m_list {
            lowered.push(lower_effect_body(&m.body, ctx)?);
        }
        let first = lowered.remove(0);
        (
            first,
            Some(SpellModes {
                minimum: min_m,
                maximum: max_m,
                conditional_maximum: None,
                allow_repeat: false,
                distinct_targets: false,
                uniqueness: ModalUniqueness::None,
                mode_costs: Vec::new(),
                escalate_mana: None,
                prefix,
                suffix,
                additional: lowered,
            }),
        )
    } else {
        (lower_effect_body(&exec_effects, ctx)?, None)
    };

    let id = ctx.alloc_activated_id();
    match delta {
        v2::LoyaltyDelta::Fixed(n) => {
            program.loyalty_costs.insert(id, i32::from(n));
        }
        v2::LoyaltyDelta::MinusX => {
            program.loyalty_costs.insert(id, 0);
            program.variable_loyalty_costs.insert(id);
        }
    }
    if let Some(res) = restriction {
        program.activation_restrictions.insert(id, res);
    }
    if let Some(m) = modes {
        program.activation_modes.insert(id, m);
    }
    program.activated.push(ActivatedAbility {
        id,
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: exec_body,
    });
    Ok(())
}

fn lower_cast_timing_restrictions(
    restrictions: &[v2::ActivationRestriction],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<CastTimingRestriction>> {
    if restrictions.is_empty() {
        return Ok(None);
    }
    let mut items = Vec::with_capacity(restrictions.len());
    for r in restrictions {
        let lowered = match r {
            v2::ActivationRestriction::AsSorcery => CastTimingRestriction::Sorcery,
            v2::ActivationRestriction::DuringYourTurn => CastTimingRestriction::DuringYourTurn,
            v2::ActivationRestriction::DuringYourUpkeep => {
                CastTimingRestriction::DuringOwnStep(Step::Upkeep)
            }
            v2::ActivationRestriction::DuringCombat => CastTimingRestriction::DuringCombat,
            v2::ActivationRestriction::DuringEndOfCombat => {
                CastTimingRestriction::DuringStep(Step::EndCombat)
            }
            v2::ActivationRestriction::BeforeAttackers => {
                CastTimingRestriction::DuringCombatBeforeBlockers
            }
            v2::ActivationRestriction::DuringCombatStep(step) => match step {
                v2::CombatStepKind::DeclareAttackers => {
                    CastTimingRestriction::DuringDeclareAttackers
                }
                v2::CombatStepKind::DeclareBlockers => CastTimingRestriction::DuringDeclareBlockers,
                v2::CombatStepKind::CombatDamage => {
                    CastTimingRestriction::DuringStep(Step::CombatDamage)
                }
            },
            v2::ActivationRestriction::OnlyIf(cond) => match cond.as_ref() {
                v2::ConditionExpr::Flag(v2::ConditionFlag::IsYourTurn) => {
                    CastTimingRestriction::DuringYourTurn
                }
                v2::ConditionExpr::Not(inner)
                    if matches!(
                        inner.as_ref(),
                        v2::ConditionExpr::Flag(v2::ConditionFlag::IsYourTurn)
                    ) =>
                {
                    CastTimingRestriction::DuringOpponentsTurn
                }
                v2::ConditionExpr::EventCheck(v2::EventCheckKind::DuringStep(step)) => {
                    CastTimingRestriction::DuringStep(lower_step(*step)?)
                }
                other => {
                    let rc = lower_resolution_condition(other, ctx)?;
                    if !rc.is_world_evaluable() {
                        return Err(LoweringError::Unsupported("v2:spell:cast-timing-nonworld"));
                    }
                    CastTimingRestriction::Condition(Box::new(rc))
                }
            },
            v2::ActivationRestriction::OnceEachTurn
            | v2::ActivationRestriction::Once
            | v2::ActivationRestriction::AllowedPlayer(_)
            | v2::ActivationRestriction::AsInstant
            | v2::ActivationRestriction::ControlledContinuously => {
                return Err(LoweringError::Unsupported(
                    "v2:spell:cast-timing-restriction",
                ));
            }
        };
        items.push(lowered);
    }
    if items.len() == 1 {
        Ok(items.pop())
    } else {
        Ok(Some(CastTimingRestriction::All(items)))
    }
}

fn lower_saga_chapter_ability(
    chapters: &[u8],
    body: &[v2::Effect],
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if chapters.is_empty() {
        return Err(LoweringError::Unsupported("v2:saga:empty-chapters"));
    }
    for &ch in chapters {
        if !(1..=6).contains(&ch) {
            return Err(LoweringError::Unsupported("v2:saga:chapter-out-of-range"));
        }
    }
    let exec_body = lower_triggered_body(body, None, ctx)?;

    for &ch in chapters {
        program.triggered.push(TriggeredAbility {
            id: ctx.alloc_triggered_id(),
            condition: TriggerSpec::counters_changed(
                ObjectFilter::self_object(),
                CounterTriggerKind::SagaChapter(ch),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: exec_body.clone(),
        });
    }
    Ok(())
}

fn extract_class_level_up_level(body: &[v2::Effect]) -> Option<u8> {
    let mut found: Option<u8> = None;
    for eff in body {
        match eff {
            v2::Effect::Restriction(_) => {}
            v2::Effect::ApplyContinuous {
                effect,
                duration: v2::Duration::Permanent | v2::Duration::Instant,
            } => match effect.as_ref() {
                v2::StaticAbility::LevelStructure(v2::LevelMarker::ClassLevel(lvl))
                    if *lvl >= 2 =>
                {
                    if found.replace(*lvl).is_some() {
                        return None;
                    }
                }
                v2::StaticAbility::ActivationCostModifier {
                    less: true,
                    condition: None,
                    ..
                } => {}
                _ => return None,
            },
            _ => return None,
        }
    }
    found
}

fn body_has_empty_die_roll(body: &ExecutableBody) -> bool {
    body.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::MutatePlayer(spec)
                if matches!(
                    &spec.op,
                    PlayerMutation::RandomEvent(RandomEventOp::RollDie { bands, .. })
                        if bands.is_empty()
                )
        )
    })
}

fn find_die_roll_in_trigger(program: &Program) -> Option<bool> {
    if let ProgramKind::Spell(spell) = &program.kind {
        if body_has_empty_die_roll(&spell.body) {
            return Some(false);
        }
        if let Some(modes) = &spell.modes
            && (modes.prefix.as_deref().is_some_and(body_has_empty_die_roll)
                || modes.suffix.as_deref().is_some_and(body_has_empty_die_roll)
                || modes.additional.iter().any(body_has_empty_die_roll))
        {
            return Some(false);
        }
    }
    if program
        .triggered
        .iter()
        .any(|tr| body_has_empty_die_roll(&tr.body))
    {
        return Some(true);
    }
    if program
        .activated
        .iter()
        .any(|act| body_has_empty_die_roll(&act.body))
    {
        return Some(false);
    }
    None
}

fn attach_die_roll_bands_in_body(body: &mut ExecutableBody, bands: &[DieRollBand]) -> bool {
    let mut attached = false;
    for op in &mut body.instructions {
        if let Instruction::MutatePlayer(spec) = op
            && let PlayerMutation::RandomEvent(RandomEventOp::RollDie {
                bands: target_bands,
                ..
            }) = &mut spec.op
            && target_bands.is_empty()
        {
            *target_bands = bands.to_vec();
            attached = true;
        }
    }
    attached
}

fn attach_die_roll_bands(program: &mut Program, bands: &[DieRollBand]) -> bool {
    let mut attached = false;
    if let ProgramKind::Spell(spell) = &mut program.kind {
        attached |= attach_die_roll_bands_in_body(&mut spell.body, bands);
        if let Some(modes) = &mut spell.modes {
            if let Some(p) = &mut modes.prefix {
                attached |= attach_die_roll_bands_in_body(p, bands);
            }
            if let Some(s) = &mut modes.suffix {
                attached |= attach_die_roll_bands_in_body(s, bands);
            }
            for m in &mut modes.additional {
                attached |= attach_die_roll_bands_in_body(m, bands);
            }
        }
    }
    for tr in &mut program.triggered {
        attached |= attach_die_roll_bands_in_body(&mut tr.body, bands);
    }
    for act in &mut program.activated {
        attached |= attach_die_roll_bands_in_body(&mut act.body, bands);
    }
    attached
}

fn lower_die_roll_table_rows(
    table_rows: &[(u16, Option<u16>, bool, &v2::TableRowPayload)],
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let Some(in_trigger) = find_die_roll_in_trigger(program) else {
        return Err(LoweringError::Unsupported("v2:ability:table-row"));
    };
    let mut bands = Vec::with_capacity(table_rows.len());
    let prev_in_trigger = ctx.in_trigger;
    ctx.in_trigger = in_trigger;
    for &(min, max_opt, open_ended, payload) in table_rows {
        let max = match max_opt {
            Some(hi) => hi,
            None if open_ended => u16::MAX,
            None => min,
        };
        if min > max {
            ctx.in_trigger = prev_in_trigger;
            return Err(LoweringError::Unsupported("v2:ability:table-row-range"));
        }
        let v2::TableRowPayload::Effects(effs) = payload else {
            ctx.in_trigger = prev_in_trigger;
            return Err(LoweringError::Unsupported("v2:ability:table-row"));
        };
        let body_res = lower_effect_body(effs, ctx);
        let body = match body_res {
            Ok(b) => b,
            Err(err) => {
                ctx.in_trigger = prev_in_trigger;
                return Err(err);
            }
        };
        bands.push(DieRollBand { min, max, body });
    }
    ctx.in_trigger = prev_in_trigger;

    if !attach_die_roll_bands(program, &bands) {
        return Err(LoweringError::Unsupported("v2:ability:table-row"));
    }
    Ok(())
}

fn lower_station_table_rows(
    table_rows: &[(u16, Option<u16>, bool, &v2::TableRowPayload)],
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let printed_base_pt = if !program.has_type(CardType::Creature)
        && let ProgramKind::Permanent(perm) = &mut program.kind
        && let Some(PrintedPowerToughness {
            power: PrintedStat::Number(p),
            toughness: PrintedStat::Number(t),
        }) = perm.printed_pt.clone()
    {
        perm.printed_pt = None;
        Some((p, t))
    } else {
        None
    };

    let creature_threshold = printed_base_pt
        .as_ref()
        .and_then(|_| table_rows.iter().map(|(min, _, _, _)| *min).max());

    for &(min, _, _, payload) in table_rows {
        let Some(nz_min) = NonZeroU16::new(min) else {
            return Err(LoweringError::Unsupported("v2:station:zero-threshold"));
        };
        let v2::TableRowPayload::Face(subface) = payload else {
            return Err(LoweringError::Unsupported("v2:ability:table-row"));
        };
        let (granted, activated_restricted) = lower_granted_face_with_restrictions(subface, ctx)?;
        let mut parts: Vec<StaticPart> = Vec::new();

        if creature_threshold == Some(min)
            && let Some((ref p, ref t)) = printed_base_pt
        {
            parts.push(StaticPart::Type(StaticTypeChange {
                types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                supertypes: TypeOperation::Keep,
                subtypes: SubtypeOperation::Keep,
                land_subtypes: SubtypeOperation::Keep,
            }));
            if !granted.keywords.is_empty() {
                parts.push(StaticPart::Abilities(AbilityChange::grant_keywords(
                    granted.keywords,
                )));
            }
            parts.push(StaticPart::SetPT(NumericPair {
                power: NumericExpression::Literal(p.clone()),
                toughness: NumericExpression::Literal(t.clone()),
            }));
        } else if !granted.keywords.is_empty() {
            parts.push(StaticPart::Abilities(AbilityChange::grant_keywords(
                granted.keywords,
            )));
        }

        for tr in granted.triggered {
            parts.push(StaticPart::GrantTrigger(Box::new(tr)));
        }
        for mut ab in granted.activated {
            ab.id = ActivatedAbilityId(0);
            parts.push(StaticPart::GrantActivated(Box::new(ab)));
        }
        for (mut ab, res) in activated_restricted {
            ab.id = ActivatedAbilityId(0);
            parts.push(StaticPart::GrantActivatedRestricted {
                ability: Box::new(ab),
                restriction: Box::new(res),
            });
        }
        for ma in granted.mana {
            let part = match ma.output {
                ManaOutput::Fixed(bundle) if bundle.len() == 1 => {
                    let Some((&color, qty)) = bundle.iter().next() else {
                        return Err(LoweringError::Unsupported("v2:station:complex-mana"));
                    };
                    if *qty != 1.into() {
                        return Err(LoweringError::Unsupported("v2:station:complex-mana"));
                    }
                    StaticPart::GrantFixedColorMana(color)
                }
                ManaOutput::ChooseColor { amount, colors }
                    if amount == 1.into()
                        && colors.len() == 5
                        && !colors.contains(&ManaType::Colourless) =>
                {
                    StaticPart::GrantAnyColorMana
                }
                _ => return Err(LoweringError::Unsupported("v2:station:complex-mana")),
            };
            parts.push(part);
        }

        if !parts.is_empty() {
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: PermanentSelector {
                        station_at: Some(nz_min),
                        ..PermanentSelector::default()
                    },
                    parts: StaticParts(parts),
                },
                None,
            )?;
        }

        if !granted.statics.is_empty() {
            let min_u8 = u8::try_from(min)
                .map_err(|_| LoweringError::Unsupported("v2:station:threshold-u8"))?;
            let cond = StaticSourceCondition::SourceCounterAtLeast {
                kind: EngineCounterKind::Charge,
                minimum: u16::from(min_u8),
            };
            for st in granted.statics {
                push_static_ability(program, st, Some(cond.clone()))?;
            }
        }
    }

    Ok(())
}

fn single_face_name(name: &str) -> LowerResult<ObjectNames> {
    let english = EnglishCardName::new(name.to_string())
        .map_err(|_| LoweringError::InvalidMetadata("v2:meta:bad-face-name"))?;
    Ok(ObjectNames::single(english))
}

fn lookup_loyalty(
    inventory: Option<&Inventory>,
    oracle_id: Option<&str>,
    face_idx: usize,
) -> Option<u16> {
    inventory?
        .source_loyalty(oracle_id?, face_idx)?
        .parse::<u16>()
        .ok()
}

fn lookup_defense(
    inventory: Option<&Inventory>,
    oracle_id: Option<&str>,
    face_idx: usize,
) -> Option<u16> {
    inventory?
        .source_defense(oracle_id?, face_idx)?
        .parse::<u16>()
        .ok()
}

fn rewrite_player_for_overload(player: v2::PlayerRef, changed: &mut bool) -> v2::PlayerRef {
    match player {
        v2::PlayerRef::TargetPlayer => {
            *changed = true;
            v2::PlayerRef::EachPlayer
        }
        v2::PlayerRef::TargetOpponent => {
            *changed = true;
            v2::PlayerRef::EachOpponent
        }
        other => other,
    }
}

fn rewrite_subject_for_overload(
    subj: &v2::SubjectRef,
    changed: &mut bool,
) -> LowerResult<v2::SubjectRef> {
    match subj {
        v2::SubjectRef::TargetObject {
            filter,
            count,
            another,
        } => {
            if *another || *count != v2::TargetCount::Exact(1) {
                return Err(LoweringError::Unsupported("v2:kw:overload"));
            }
            *changed = true;
            Ok(v2::SubjectRef::All(filter.clone()))
        }
        v2::SubjectRef::TargetPlayer {
            opponent_only,
            count,
        } => {
            if *count != v2::TargetCount::Exact(1) {
                return Err(LoweringError::Unsupported("v2:kw:overload"));
            }
            *changed = true;
            Ok(v2::SubjectRef::Player(if *opponent_only {
                v2::PlayerRef::EachOpponent
            } else {
                v2::PlayerRef::EachPlayer
            }))
        }
        v2::SubjectRef::Player(p) => Ok(v2::SubjectRef::Player(rewrite_player_for_overload(
            *p, changed,
        ))),
        v2::SubjectRef::AnyTarget => Err(LoweringError::Unsupported("v2:kw:overload")),
        v2::SubjectRef::Compound(parts) => {
            let rewritten = parts
                .iter()
                .map(|p| rewrite_subject_for_overload(p, changed))
                .collect::<LowerResult<Vec<_>>>()?;
            Ok(v2::SubjectRef::Compound(rewritten))
        }
        other => Ok(other.clone()),
    }
}

fn rewrite_effects_for_overload(effects: &[v2::Effect]) -> LowerResult<(Vec<v2::Effect>, bool)> {
    let mut changed = false;
    let mut out = Vec::with_capacity(effects.len());
    for eff in effects {
        out.push(rewrite_effect_for_overload(eff, &mut changed)?);
    }
    Ok((out, changed))
}

fn rewrite_effect_for_overload(eff: &v2::Effect, changed: &mut bool) -> LowerResult<v2::Effect> {
    let mut cloned = eff.clone();
    match &mut cloned {
        v2::Effect::ZoneOp { actor, subject, .. } => {
            *actor = rewrite_player_for_overload(*actor, changed);
            *subject = rewrite_subject_for_overload(subject, changed)?;
        }
        v2::Effect::MutateObject { subject, .. } => {
            *subject = rewrite_subject_for_overload(subject, changed)?;
        }
        v2::Effect::DealDamage { source, target, .. } => {
            *source = rewrite_subject_for_overload(source, changed)?;
            *target = rewrite_subject_for_overload(target, changed)?;
        }
        v2::Effect::AdjustLife { player, .. } | v2::Effect::MutatePlayer { player, .. } => {
            *player = rewrite_player_for_overload(*player, changed);
        }
        v2::Effect::PreventDamage { source, target, .. } => {
            if let Some(s) = source {
                *s = rewrite_subject_for_overload(s, changed)?;
            }
            if let Some(t) = target {
                *t = rewrite_subject_for_overload(t, changed)?;
            }
        }
        v2::Effect::CreateToken { controller, .. } => {
            *controller = rewrite_player_for_overload(*controller, changed);
        }
        v2::Effect::Optional { actor, body } => {
            *actor = rewrite_player_for_overload(*actor, changed);
            for inner in body.iter_mut() {
                *inner = rewrite_effect_for_overload(inner, changed)?;
            }
        }
        v2::Effect::BranchCondition {
            then_body,
            else_body,
            ..
        } => {
            for inner in then_body.iter_mut() {
                *inner = rewrite_effect_for_overload(inner, changed)?;
            }
            for inner in else_body.iter_mut() {
                *inner = rewrite_effect_for_overload(inner, changed)?;
            }
        }
        v2::Effect::ScopedDuration { body, .. }
        | v2::Effect::IfPrevious { body, .. }
        | v2::Effect::BindVariable { body, .. } => {
            for inner in body.iter_mut() {
                *inner = rewrite_effect_for_overload(inner, changed)?;
            }
        }
        _ => {}
    }
    Ok(cloned)
}
