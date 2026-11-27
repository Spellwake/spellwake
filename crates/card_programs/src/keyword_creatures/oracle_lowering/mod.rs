//! Strict AST-to-Program lowering adapter from `spellwake-oracle-compiler` to `spellwake-engine`.
//!
//! Every AST node on a strictly-compiled card face must lower faithfully to native engine IR
//! without dropping clauses, modifiers, or restrictions. Any unsupported AST construct returns
//! `Err` cleanly, and every assembled [`Program`] is verified via [`Program::validate`].

use std::collections::{BTreeMap, BTreeSet};
use std::num::{NonZeroU8, NonZeroU16, NonZeroU32};
use std::sync::{Arc, Mutex, OnceLock};

use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    activated::{
        ActivatedAbility, ActivatedAbilityId, ActivationRestriction, CostExileZone,
        CounterCostDelta, DiscardCostMode, ExtraActivationCost, GraveyardExileCost,
        HandDiscardKeyword, LifeCostAmount, PowerAbility, SelectedSacrificeFilter, SourceCost,
        TapCostCount, TapCostFilter,
    },
    attachment::{AttachAbility, AttachmentKind, EnchantRestriction},
    characteristics::{
        AbilityChange, AbilityRemoval, BlockingRestriction, CastPermissionRule, Color, ColorFilter,
        ColorOperation, ControllerSelector, DamagePreventFollowup, DamagePreventMode,
        DamageRedirectTarget, DefiningPart, DefiningParts, HexproofQuality,
        InstructionSubtypeOperand, KeywordAbility, LibraryTopPermission, NumericExpression,
        NumericPair, PermanentSelector, PowerToughness, PrintedPowerToughness, PrintedStat,
        ReplaceableEvent, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, SpellCostModifier,
        SpellDiscount, StaticAbilityId, StaticColorOperand, StaticContinuousAbility,
        StaticDamageKind, StaticDamageOp, StaticDamageRecipient, StaticDamageRule, StaticDrawRule,
        StaticLandSubtypeOperand, StaticModifierRule, StaticPart, StaticParts, StaticPlayerStat,
        StaticReplacementAction, StaticRestriction, StaticSourceCondition, StaticSubtypeOperand,
        StaticTypeChange, StaticUntapRule, StaticZone, SubtypeOperation, SubtypeProtection,
        TypeChangeSpec, TypeOperation,
    },
    effects::CounterKind,
    emblem::EmblemDefinition,
    face_down::FaceUpAbility,
    land::{BasicLandType, EntryTap, LandType},
    life::LifeChangeKind,
    mana::{
        EnchantedLandManaBonus, HybridAlternative, HybridSymbol, ManaAbility, ManaAbilityId,
        ManaCost, ManaOutput, ManaOutputModifier, ManaRestriction, ManaSacrificeCost,
        ManaSideEffect, ManaType, SpecialColorYield, TriggeredManaRule,
    },
    program::{
        AdditionalCastCost, AffectedPlayers, ArtifactSubtype, AttackRequirement, AttackTriggerMode,
        BlockTriggerKind, BodyTarget, CardType, CardTypeMask, CastCopyKeyword, CastCostCount,
        CastEntryKind, CastReduction, CastTimingRestriction, ChosenHandAction, ColorMask,
        CombatActionKind, CombatFilter, CombatTriggerKeyword, CombatUntapGroup, Comparison,
        ConditionalCastCost, ControllerFilter, CopyExceptions, CounterKeyword, CounterTriggerKind,
        DamageAmountSpec, DamageGroup, DamageRecipientsRef, DamageSourceRef,
        DamageTriggerRecipient, DiesTriggerMode, Disturb, EffectManaYield, EffectSubject,
        EnchantmentSubtype, EndStepCast, EntryCopyRule, EntryCopySpecExt, EntryCopyZone,
        EntryCounterAbility, EventValueKind, ExecutableBody, ExileAction, GrowthKeyword,
        Instruction, InterveningCondition, LibraryPlacement, LibrarySearch, ManaValueFilter,
        MatchObjectRef, MiracleCost, ModalUniqueness, MoveSubjectRef, ObjectClause,
        ObjectColorFilter, ObjectFilter, ObjectFilterFlags, ObjectStatKind, PermanentDefinition,
        PermanentTypeFilter, PlanarTriggerEvent, PlaneswalkerSubtype, PlayerRelation,
        PlayerStatExpr, PlayerTriggerAction, PowerFilter, Program, ProgramKind,
        ProtectionRecipients, PublicZoneDestination, ResolutionCondition, ResolvingPlayer,
        SearchCardMatcher, SearchCount, SearchDestination, SearchExileLink, SearchManaValue,
        SearchSource, SelfFlashPermission, SourceRelation, SourceStateExpr, SourceZoneFilter,
        SpellDefinition, SpellFilter, SpellModes, SpellSubtype, SpliceAbility, SpliceQuality,
        SplitCard, StatAggregate, StateThresholdKind, StatusTriggerKind, SubjectDestination,
        SubtypeFilter, Supertype, SupertypeMask, TemporaryRuleKind, ToughnessFilter,
        TransferKeyword, TriggerCondition, TriggerEvent, TriggerFromZone, TriggerGrouping,
        TriggerKeywordEvent, TriggerPlayerScope, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
        TriggeredModes, UpkeepKeyword, ValueExpr, ValueObjectRef, ValueZone, ZoneActionKind,
        ZoneActionPredicate, ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId,
        ZoneReplacementChoice, ZoneReplacementExtra, ZoneReplacementSubject,
    },
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSize, TokenSubtype},
    turn::Step,
};
use spellwake_oracle_compiler::{
    CardInput, FaceInput, Vocabulary,
    ast::{
        AbilityAst, AggregateOp, CardTypeMask as AstCardTypeMask, ColorArity,
        ColorMask as AstColorMask, CombatState, CompareOp, ConditionAst, ControllerRelation,
        CostAtomAst, DelayedStepScope, DurationAst, FaceAst, GameMarker, InstructionAst,
        InteractiveChoiceKind, KeywordAst, ManaSpecAst, ObjectFilterAst,
        ObjectFilterFlags as AstFilterFlags, PlayerRefAst, PlayerStatKind, ReplacementEffectAst,
        Rounding, StatKind, StaticAbilityAst, SubjectRefAst, SupertypeMask as AstSupertypeMask,
        TargetCountAst, TokenSpecAst, TriggerEventAst, TriggerSpecAst, TurnStepAst, ValueExprAst,
        ZoneFilter,
    },
    compile_card,
};

use super::InventoryCard;

mod abilities;
mod body;
mod filters;
mod helpers;
mod instructions;
mod interactive;
mod keywords;
mod replacements;
mod statics;
#[cfg(test)]
mod tests;
pub(crate) mod toolbox_emit;

use abilities::*;
use body::*;
use filters::*;
use helpers::*;
use instructions::*;
use keywords::*;
use replacements::*;
use statics::*;

type VocabCacheEntry = Option<(String, Arc<Vocabulary>)>;

static VOCAB_CACHE: OnceLock<Mutex<VocabCacheEntry>> = OnceLock::new();

fn cached_compiler_vocab(vocabulary: &CreatureTypeVocabulary) -> Arc<Vocabulary> {
    let key = vocabulary.id().to_string();
    let cache = VOCAB_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cache.lock() {
        if let Some((cached_key, cached_vocab)) = guard.as_ref()
            && cached_key == &key
        {
            return Arc::clone(cached_vocab);
        }
        let built = Arc::new(Vocabulary::from_creature_members(vocabulary.members()));
        *guard = Some((key, Arc::clone(&built)));
        built
    } else {
        Arc::new(Vocabulary::from_creature_members(vocabulary.members()))
    }
}

/// Strictly compile a single-face, saga, split, or adventure card via `spellwake-oracle-compiler`
/// and lower it to a validated native [`Program`].
pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    compile_with_reason(card, vocabulary).ok()
}

/// Strictly compile a transforming double-faced card via `spellwake-oracle-compiler`
/// with its source-authenticated back-face color indicator, loyalty, and defense.
pub(super) fn compile_transforming(
    card: &InventoryCard,
    inventory: &super::Inventory,
    vocabulary: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
    loyalty_source_verified: bool,
) -> Option<Program> {
    compile_transforming_with_reason(
        card,
        inventory,
        vocabulary,
        indicator,
        loyalty_source_verified,
    )
    .ok()
}

/// Strictly compile a modal double-faced card via `spellwake-oracle-compiler`
/// with optional source-authenticated loyalty and back-face color indicator.
pub(super) fn compile_modal_dfc(
    card: &InventoryCard,
    inventory: &super::Inventory,
    vocabulary: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
    loyalty_source_verified: bool,
) -> Option<Program> {
    compile_modal_dfc_inner(
        card,
        Some(inventory),
        vocabulary,
        indicator,
        loyalty_source_verified,
    )
    .ok()
}

fn compile_modal_dfc_inner(
    card: &InventoryCard,
    inventory: Option<&super::Inventory>,
    vocabulary: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
    loyalty_source_verified: bool,
) -> Result<Program, &'static str> {
    let [front_face, back_face] = card.faces.as_slice() else {
        return Err("oracle:multi-face");
    };
    if card.layout != "modal_dfc" {
        return Err("oracle:non-normal-layout");
    }
    let front_tl = front_face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    let back_tl = back_face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    let oracle_id = card.oracle_id.as_deref();
    let front_loyalty = if loyalty_source_verified && front_tl.contains("Planeswalker") {
        oracle_id
            .and_then(|id| inventory.and_then(|inv| inv.source_loyalty(id, 0)))
            .and_then(|s| s.parse::<u16>().ok())
    } else {
        None
    };
    let back_loyalty = if loyalty_source_verified && back_tl.contains("Planeswalker") {
        oracle_id
            .and_then(|id| inventory.and_then(|inv| inv.source_loyalty(id, 1)))
            .and_then(|s| s.parse::<u16>().ok())
    } else {
        None
    };
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let input = CardInput {
        oracle_id: card.oracle_id.as_deref().unwrap_or(""),
        name: &card.name,
        layout: &card.layout,
        faces: vec![
            FaceInput {
                name: &front_face.name,
                type_line: front_tl,
                oracle_text: front_face.oracle_text.as_deref().unwrap_or(""),
            },
            FaceInput {
                name: &back_face.name,
                type_line: back_tl,
                oracle_text: back_face.oracle_text.as_deref().unwrap_or(""),
            },
        ],
    };
    let card_ast = compile_card(&input, &compiler_vocab);
    if !card_ast.strictly_compiled {
        return Err("oracle:ast-parse-incomplete");
    }
    let [front_ast, back_ast] = card_ast.faces.as_slice() else {
        return Err("oracle:multi-face-ast");
    };
    let mut front = lower_face(
        card,
        front_face,
        front_ast,
        vocabulary,
        FaceLoweringOptions {
            allow_empty_mana_cost: false,
            allow_aftermath: false,
            allow_fuse: false,
            allow_disturb: false,
            printed_loyalty: front_loyalty,
            printed_defense: None,
        },
    )?;
    let mut back = lower_face(
        card,
        back_face,
        back_ast,
        vocabulary,
        FaceLoweringOptions {
            allow_empty_mana_cost: false,
            allow_aftermath: false,
            allow_fuse: false,
            allow_disturb: false,
            printed_loyalty: back_loyalty,
            printed_defense: None,
        },
    )?;
    if let Some(ind) = indicator {
        let mut colors = BTreeSet::new();
        for c in ind {
            colors.insert(match c.as_str() {
                "W" => Color::White,
                "U" => Color::Blue,
                "B" => Color::Black,
                "R" => Color::Red,
                "G" => Color::Green,
                _ => return Err("oracle:bad-color-indicator"),
            });
        }
        if !colors.is_empty() {
            back.color_indicator = Some(colors);
        }
    }
    front.names = ObjectNames::single(
        EnglishCardName::new(front_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
    );
    back.names = ObjectNames::single(
        EnglishCardName::new(back_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
    );
    if front.has_type(CardType::Land)
        && back.has_type(CardType::Land)
        && let ProgramKind::Permanent(def) = &mut front.kind
        && def.entry_tap.replace(EntryTap::ChooseModalFace).is_some()
    {
        return Err("oracle:duplicate-entry-tap");
    }
    front.modal_back = Some(Box::new(back));
    front
        .validate(vocabulary)
        .map_err(|_| "oracle:engine-validation-failed")?;
    Ok(front)
}

fn compile_flip_inner(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Program, &'static str> {
    let [front_face, back_face] = card.faces.as_slice() else {
        return Err("oracle:multi-face");
    };
    let front_tl = front_face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    let back_tl = back_face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    if front_tl.contains("Token") || back_tl.contains("Token") {
        return Err("oracle:non-normal-layout");
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let input = CardInput {
        oracle_id: card.oracle_id.as_deref().unwrap_or(""),
        name: &card.name,
        layout: &card.layout,
        faces: vec![
            FaceInput {
                name: &front_face.name,
                type_line: front_tl,
                oracle_text: front_face.oracle_text.as_deref().unwrap_or(""),
            },
            FaceInput {
                name: &back_face.name,
                type_line: back_tl,
                oracle_text: back_face.oracle_text.as_deref().unwrap_or(""),
            },
        ],
    };
    let card_ast = compile_card(&input, &compiler_vocab);
    if !card_ast.strictly_compiled {
        return Err("oracle:ast-parse-incomplete");
    }
    let [front_ast, back_ast] = card_ast.faces.as_slice() else {
        return Err("oracle:multi-face-ast");
    };
    let mut front = lower_face(
        card,
        front_face,
        front_ast,
        vocabulary,
        FaceLoweringOptions::default(),
    )?;
    let mut back = lower_face(
        card,
        back_face,
        back_ast,
        vocabulary,
        FaceLoweringOptions {
            allow_empty_mana_cost: true,
            ..FaceLoweringOptions::default()
        },
    )?;
    // CR 710.4: flipped permanent retains the unflipped half's mana cost and color.
    if let (ProgramKind::Permanent(fdef), ProgramKind::Permanent(bdef)) =
        (&front.kind, &mut back.kind)
    {
        bdef.casting_cost = fdef.casting_cost.clone();
    }
    back.hybrid_symbols = front.hybrid_symbols.clone();
    back.x_mana_symbols = front.x_mana_symbols;
    back.color_indicator = front.color_indicator.clone();
    front.names = ObjectNames::single(
        EnglishCardName::new(front_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
    );
    back.names = ObjectNames::single(
        EnglishCardName::new(back_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
    );
    front.flip_back = Some(Box::new(back));
    front
        .validate(vocabulary)
        .map_err(|_| "oracle:engine-validation-failed")?;
    Ok(front)
}

fn compile_prepare_inner(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Program, &'static str> {
    let [front_face, spell_face] = card.faces.as_slice() else {
        return Err("oracle:multi-face");
    };
    let front_tl = front_face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    let spell_tl = spell_face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let input = CardInput {
        oracle_id: card.oracle_id.as_deref().unwrap_or(""),
        name: &card.name,
        layout: &card.layout,
        faces: vec![
            FaceInput {
                name: &front_face.name,
                type_line: front_tl,
                oracle_text: front_face.oracle_text.as_deref().unwrap_or(""),
            },
            FaceInput {
                name: &spell_face.name,
                type_line: spell_tl,
                oracle_text: spell_face.oracle_text.as_deref().unwrap_or(""),
            },
        ],
    };
    let card_ast = compile_card(&input, &compiler_vocab);
    if !card_ast.strictly_compiled {
        return Err("oracle:ast-parse-incomplete");
    }
    let [front_ast, spell_ast] = card_ast.faces.as_slice() else {
        return Err("oracle:multi-face-ast");
    };
    let mut front = lower_face(
        card,
        front_face,
        front_ast,
        vocabulary,
        FaceLoweringOptions::default(),
    )?;
    let mut spell = lower_face(
        card,
        spell_face,
        spell_ast,
        vocabulary,
        FaceLoweringOptions::default(),
    )?;
    front.names = ObjectNames::single(
        EnglishCardName::new(front_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
    );
    spell.names = ObjectNames::single(
        EnglishCardName::new(spell_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
    );
    front.prepare_spell = Some(Box::new(spell));
    front
        .validate(vocabulary)
        .map_err(|_| "oracle:engine-validation-failed")?;
    Ok(front)
}

/// Strictly compile a single-face planeswalker via `spellwake-oracle-compiler`
/// with source-authenticated printed loyalty.
pub(super) fn compile_planeswalker(
    card: &InventoryCard,
    inventory: &super::Inventory,
    vocabulary: &CreatureTypeVocabulary,
    loyalty_source_verified: bool,
) -> Option<Program> {
    compile_planeswalker_with_reason(card, inventory, vocabulary, loyalty_source_verified).ok()
}

pub(super) fn compile_planeswalker_with_reason(
    card: &InventoryCard,
    inventory: &super::Inventory,
    vocabulary: &CreatureTypeVocabulary,
    loyalty_source_verified: bool,
) -> Result<Program, &'static str> {
    if !loyalty_source_verified {
        return Err("oracle:planeswalker-unverified-loyalty");
    }
    let [face] = card.faces.as_slice() else {
        return Err("oracle:multi-face");
    };
    if card.layout != "normal" || card.name != face.name {
        return Err("oracle:non-normal-layout");
    }
    let type_line = face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    if !type_line.contains("Planeswalker") {
        return Err("oracle:not-planeswalker");
    }
    let loyalty = inventory
        .source_loyalty(
            card.oracle_id
                .as_deref()
                .ok_or("oracle:missing-oracle-id")?,
            0,
        )
        .ok_or("oracle:missing-loyalty")?
        .parse::<u16>()
        .map_err(|_| "oracle:bad-loyalty")?;
    let oracle_text = face.oracle_text.as_deref().unwrap_or("");
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let input = CardInput {
        oracle_id: card.oracle_id.as_deref().unwrap_or(""),
        name: &card.name,
        layout: &card.layout,
        faces: vec![FaceInput {
            name: &face.name,
            type_line,
            oracle_text,
        }],
    };
    let card_ast = compile_card(&input, &compiler_vocab);
    if !card_ast.strictly_compiled {
        return Err("oracle:ast-parse-incomplete");
    }
    let [face_ast] = card_ast.faces.as_slice() else {
        return Err("oracle:multi-face-ast");
    };
    let program = lower_face(
        card,
        face,
        face_ast,
        vocabulary,
        FaceLoweringOptions {
            allow_empty_mana_cost: false,
            allow_aftermath: false,
            allow_fuse: false,
            allow_disturb: false,
            printed_loyalty: Some(loyalty),
            printed_defense: None,
        },
    )?;
    program
        .validate(vocabulary)
        .map_err(|_| "oracle:engine-validation-failed")?;
    Ok(program)
}

#[derive(Default, Clone, Copy)]
struct FaceLoweringOptions {
    allow_empty_mana_cost: bool,
    allow_aftermath: bool,
    allow_fuse: bool,
    allow_disturb: bool,
    printed_loyalty: Option<u16>,
    printed_defense: Option<u16>,
}

fn compile_transforming_with_reason(
    card: &InventoryCard,
    inventory: &super::Inventory,
    vocabulary: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
    loyalty_source_verified: bool,
) -> Result<Program, &'static str> {
    let [front_face, back_face] = card.faces.as_slice() else {
        return Err("oracle:multi-face");
    };
    if card.layout != "transform" || !back_face.mana_cost.as_deref().unwrap_or("").is_empty() {
        return Err("oracle:non-normal-layout");
    }
    let front_tl = front_face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    let back_tl = back_face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    let oracle_id = card.oracle_id.as_deref();
    let front_loyalty = if loyalty_source_verified && front_tl.contains("Planeswalker") {
        oracle_id
            .and_then(|id| inventory.source_loyalty(id, 0))
            .and_then(|s| s.parse::<u16>().ok())
    } else {
        None
    };
    let back_loyalty = if loyalty_source_verified && back_tl.contains("Planeswalker") {
        oracle_id
            .and_then(|id| inventory.source_loyalty(id, 1))
            .and_then(|s| s.parse::<u16>().ok())
    } else {
        None
    };
    let front_defense = if front_tl.contains("Battle") {
        oracle_id
            .and_then(|id| inventory.source_defense(id, 0))
            .and_then(|s| s.parse::<u16>().ok())
    } else {
        None
    };
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let input = CardInput {
        oracle_id: card.oracle_id.as_deref().unwrap_or(""),
        name: &card.name,
        layout: &card.layout,
        faces: vec![
            FaceInput {
                name: &front_face.name,
                type_line: front_tl,
                oracle_text: front_face.oracle_text.as_deref().unwrap_or(""),
            },
            FaceInput {
                name: &back_face.name,
                type_line: back_tl,
                oracle_text: back_face.oracle_text.as_deref().unwrap_or(""),
            },
        ],
    };
    let card_ast = compile_card(&input, &compiler_vocab);
    if !card_ast.strictly_compiled {
        return Err("oracle:ast-parse-incomplete");
    }
    let [front_ast, back_ast] = card_ast.faces.as_slice() else {
        return Err("oracle:multi-face-ast");
    };
    let mut disturb_cost: Option<ManaCost> = None;
    for kw in &front_ast.keywords {
        if kw.name.eq_ignore_ascii_case("disturb") {
            let param = kw
                .parameter
                .as_deref()
                .ok_or("oracle:disturb-missing-cost")?;
            let (mc, x, hyb) = parse_mana_cost_full(param).ok_or("oracle:bad-disturb-cost")?;
            if x != 0 || !hyb.is_empty() {
                return Err("oracle:bad-disturb-cost");
            }
            if disturb_cost.replace(mc).is_some() {
                return Err("oracle:duplicate-disturb");
            }
        }
    }
    let mut front = lower_face(
        card,
        front_face,
        front_ast,
        vocabulary,
        FaceLoweringOptions {
            allow_empty_mana_cost: false,
            allow_aftermath: false,
            allow_fuse: false,
            allow_disturb: disturb_cost.is_some(),
            printed_loyalty: front_loyalty,
            printed_defense: front_defense,
        },
    )?;
    let mut back = lower_face(
        card,
        back_face,
        back_ast,
        vocabulary,
        FaceLoweringOptions {
            allow_empty_mana_cost: true,
            allow_aftermath: false,
            allow_fuse: false,
            allow_disturb: false,
            printed_loyalty: back_loyalty,
            printed_defense: None,
        },
    )?;
    if let Some(ind) = indicator {
        let mut colors = BTreeSet::new();
        for c in ind {
            colors.insert(match c.as_str() {
                "W" => Color::White,
                "U" => Color::Blue,
                "B" => Color::Black,
                "R" => Color::Red,
                "G" => Color::Green,
                _ => return Err("oracle:bad-color-indicator"),
            });
        }
        if !colors.is_empty() {
            back.color_indicator = Some(colors);
        }
    }
    front.names = ObjectNames::single(
        EnglishCardName::new(front_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
    );
    back.names = ObjectNames::single(
        EnglishCardName::new(back_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
    );
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
    front
        .validate(vocabulary)
        .map_err(|_| "oracle:engine-validation-failed")?;
    Ok(front)
}

/// Diagnostic entry point routing single-face, planeswalker, transforming, and modal DFC cards
/// through `oracle_lowering` with inventory metadata (loyalty, defense, back color indicators).
#[cfg(test)]
pub(crate) fn compile_with_inventory_reason(
    card: &InventoryCard,
    inventory: &super::Inventory,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Program, &'static str> {
    if card.layout == "transform" {
        let indicator = card
            .oracle_id
            .as_deref()
            .and_then(|id| inventory.back_color_indicator(id));
        return compile_transforming_with_reason(card, inventory, vocabulary, indicator, true);
    }
    if card.layout == "modal_dfc" {
        let indicator = card
            .oracle_id
            .as_deref()
            .and_then(|id| inventory.back_color_indicator(id));
        return compile_modal_dfc_inner(card, Some(inventory), vocabulary, indicator, true);
    }
    if card.layout == "normal"
        && let [face] = card.faces.as_slice()
        && face
            .type_line
            .as_deref()
            .is_some_and(|tl| tl.contains("Planeswalker"))
    {
        return compile_planeswalker_with_reason(card, inventory, vocabulary, true);
    }
    compile_with_reason(card, vocabulary)
}

/// Diagnostic entry point returning a concise static reason when a card cannot be lowered.
pub(crate) fn compile_with_reason(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Program, &'static str> {
    if card.layout == "split" {
        let [left_face, right_face] = card.faces.as_slice() else {
            return Err("oracle:multi-face");
        };
        let left_tl = left_face
            .type_line
            .as_deref()
            .ok_or("oracle:missing-type-line")?;
        let right_tl = right_face
            .type_line
            .as_deref()
            .ok_or("oracle:missing-type-line")?;
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        let input = CardInput {
            oracle_id: card.oracle_id.as_deref().unwrap_or(""),
            name: &card.name,
            layout: &card.layout,
            faces: vec![
                FaceInput {
                    name: &left_face.name,
                    type_line: left_tl,
                    oracle_text: left_face.oracle_text.as_deref().unwrap_or(""),
                },
                FaceInput {
                    name: &right_face.name,
                    type_line: right_tl,
                    oracle_text: right_face.oracle_text.as_deref().unwrap_or(""),
                },
            ],
        };
        let card_ast = compile_card(&input, &compiler_vocab);
        if !card_ast.strictly_compiled {
            return Err("oracle:ast-parse-incomplete");
        }
        let [left_ast, right_ast] = card_ast.faces.as_slice() else {
            return Err("oracle:multi-face-ast");
        };
        let aftermath = right_ast
            .keywords
            .iter()
            .any(|k| k.name.eq_ignore_ascii_case("aftermath"));
        let has_fuse = |ast: &FaceAst| {
            ast.keywords
                .iter()
                .any(|k| k.name.eq_ignore_ascii_case("fuse") && k.parameter.is_none())
        };
        let fuse = !aftermath && (has_fuse(left_ast) || has_fuse(right_ast));
        let mut left = lower_face(
            card,
            left_face,
            left_ast,
            vocabulary,
            FaceLoweringOptions {
                allow_fuse: fuse,
                ..FaceLoweringOptions::default()
            },
        )?;
        let mut right = lower_face(
            card,
            right_face,
            right_ast,
            vocabulary,
            FaceLoweringOptions {
                allow_empty_mana_cost: false,
                allow_aftermath: true,
                allow_fuse: fuse,
                allow_disturb: false,
                printed_loyalty: None,
                printed_defense: None,
            },
        )?;
        left.names = ObjectNames::single(
            EnglishCardName::new(left_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
        );
        right.names = ObjectNames::single(
            EnglishCardName::new(right_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
        );
        let combined = SplitCard {
            left,
            right,
            aftermath,
            fuse,
        }
        .into_program()
        .ok_or("oracle:split-combine-failed")?;
        combined
            .validate(vocabulary)
            .map_err(|_| "oracle:engine-validation-failed")?;
        return Ok(combined);
    }
    if card.layout == "adventure" {
        let [front_face, adv_face] = card.faces.as_slice() else {
            return Err("oracle:multi-face");
        };
        let front_tl = front_face
            .type_line
            .as_deref()
            .ok_or("oracle:missing-type-line")?;
        let adv_tl = adv_face
            .type_line
            .as_deref()
            .ok_or("oracle:missing-type-line")?;
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        let input = CardInput {
            oracle_id: card.oracle_id.as_deref().unwrap_or(""),
            name: &card.name,
            layout: &card.layout,
            faces: vec![
                FaceInput {
                    name: &front_face.name,
                    type_line: front_tl,
                    oracle_text: front_face.oracle_text.as_deref().unwrap_or(""),
                },
                FaceInput {
                    name: &adv_face.name,
                    type_line: adv_tl,
                    oracle_text: adv_face.oracle_text.as_deref().unwrap_or(""),
                },
            ],
        };
        let card_ast = compile_card(&input, &compiler_vocab);
        if !card_ast.strictly_compiled {
            return Err("oracle:ast-parse-incomplete");
        }
        let [front_ast, adv_ast] = card_ast.faces.as_slice() else {
            return Err("oracle:multi-face-ast");
        };
        let mut front = lower_face(
            card,
            front_face,
            front_ast,
            vocabulary,
            FaceLoweringOptions::default(),
        )?;
        let mut adventure = lower_face(
            card,
            adv_face,
            adv_ast,
            vocabulary,
            FaceLoweringOptions::default(),
        )?;
        adventure.names = ObjectNames::single(
            EnglishCardName::new(adv_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
        );
        adventure.spell_subtypes.insert(SpellSubtype::Adventure);
        front.names = ObjectNames::single(
            EnglishCardName::new(front_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
        );
        front.adventure = Some(Box::new(adventure));
        front
            .validate(vocabulary)
            .map_err(|_| "oracle:engine-validation-failed")?;
        return Ok(front);
    }
    if card.layout == "modal_dfc" {
        return compile_modal_dfc_inner(card, None, vocabulary, None, false);
    }
    if card.layout == "flip" {
        return compile_flip_inner(card, vocabulary);
    }
    if card.layout == "prepare" {
        return compile_prepare_inner(card, vocabulary);
    }
    let [face] = card.faces.as_slice() else {
        return Err("oracle:multi-face");
    };
    if !matches!(
        card.layout.as_str(),
        "normal" | "saga" | "mutate" | "prototype" | "leveler" | "class" | "case" | "meld"
    ) || face.name != card.name
    {
        return Err("oracle:non-normal-layout");
    }
    let type_line = face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    let oracle_text = face.oracle_text.as_deref().unwrap_or("");
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let input = CardInput {
        oracle_id: card.oracle_id.as_deref().unwrap_or(""),
        name: &card.name,
        layout: &card.layout,
        faces: vec![FaceInput {
            name: &face.name,
            type_line,
            oracle_text,
        }],
    };
    let card_ast = compile_card(&input, &compiler_vocab);
    if !card_ast.strictly_compiled {
        return Err("oracle:ast-parse-incomplete");
    }
    let [face_ast] = card_ast.faces.as_slice() else {
        return Err("oracle:multi-face-ast");
    };
    let program = lower_face(
        card,
        face,
        face_ast,
        vocabulary,
        FaceLoweringOptions::default(),
    )?;
    program
        .validate(vocabulary)
        .map_err(|_| "oracle:engine-validation-failed")?;
    Ok(program)
}

#[derive(Default)]
struct LoweringIds {
    next_activated: u64,
    next_triggered: u64,
    next_mana: u64,
    next_static: u64,
    next_ct_slot: u16,
}

impl LoweringIds {
    fn activated(&mut self) -> ActivatedAbilityId {
        let id = self.next_activated;
        self.next_activated += 1;
        ActivatedAbilityId(id)
    }
    fn triggered(&mut self) -> TriggeredAbilityId {
        let id = self.next_triggered;
        self.next_triggered += 1;
        TriggeredAbilityId(id)
    }
    fn mana(&mut self) -> ManaAbilityId {
        let id = self.next_mana;
        self.next_mana += 1;
        ManaAbilityId(id)
    }
    fn static_id(&mut self) -> StaticAbilityId {
        self.next_static += 1;
        StaticAbilityId(self.next_static)
    }
    fn creature_type_slot(&mut self) -> spellwake_engine::program::CreatureTypeSlot {
        let id = self.next_ct_slot;
        self.next_ct_slot = self.next_ct_slot.saturating_add(1);
        spellwake_engine::program::CreatureTypeSlot::new(id)
    }
}

struct ParsedTypeLine {
    supertypes: BTreeSet<Supertype>,
    card_types: BTreeSet<CardType>,
    creature_subtypes: BTreeSet<CreatureTypeId>,
    land_subtypes: BTreeSet<LandType>,
    artifact_subtypes: BTreeSet<ArtifactSubtype>,
    enchantment_subtypes: BTreeSet<EnchantmentSubtype>,
    spell_subtypes: BTreeSet<SpellSubtype>,
    planeswalker_subtypes: BTreeSet<PlaneswalkerSubtype>,
    is_aura: bool,
    is_equipment: bool,
    is_fortification: bool,
    is_vehicle: bool,
    is_siege: bool,
}

fn parse_type_line(
    line: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ParsedTypeLine, &'static str> {
    let (types_part, subtypes_part) = line.split_once(" \u{2014} ").unwrap_or((line, ""));
    let mut parsed = ParsedTypeLine {
        supertypes: BTreeSet::new(),
        card_types: BTreeSet::new(),
        creature_subtypes: BTreeSet::new(),
        land_subtypes: BTreeSet::new(),
        artifact_subtypes: BTreeSet::new(),
        enchantment_subtypes: BTreeSet::new(),
        spell_subtypes: BTreeSet::new(),
        planeswalker_subtypes: BTreeSet::new(),
        is_aura: false,
        is_equipment: false,
        is_fortification: false,
        is_vehicle: false,
        is_siege: false,
    };

    for token in types_part.split_whitespace() {
        match token {
            "Basic" => {
                parsed.supertypes.insert(Supertype::Basic);
            }
            "Legendary" => {
                parsed.supertypes.insert(Supertype::Legendary);
            }
            "Snow" => {
                parsed.supertypes.insert(Supertype::Snow);
            }
            "World" => {
                parsed.supertypes.insert(Supertype::World);
            }
            "Ongoing" => {
                parsed.supertypes.insert(Supertype::Ongoing);
            }
            "Creature" => {
                parsed.card_types.insert(CardType::Creature);
            }
            "Artifact" => {
                parsed.card_types.insert(CardType::Artifact);
            }
            "Enchantment" => {
                parsed.card_types.insert(CardType::Enchantment);
            }
            "Land" => {
                parsed.card_types.insert(CardType::Land);
            }
            "Instant" => {
                parsed.card_types.insert(CardType::Instant);
            }
            "Sorcery" => {
                parsed.card_types.insert(CardType::Sorcery);
            }
            "Kindred" | "Tribal" => {
                parsed.card_types.insert(CardType::Kindred);
            }
            "Planeswalker" => {
                parsed.card_types.insert(CardType::Planeswalker);
            }
            "Battle" => {
                parsed.card_types.insert(CardType::Battle);
            }
            _ => return Err("oracle:unknown-card-type"),
        }
    }
    if parsed.card_types.is_empty() {
        return Err("oracle:empty-card-types");
    }

    let is_creature_family = parsed.card_types.contains(&CardType::Creature)
        || parsed.card_types.contains(&CardType::Kindred);
    let is_land = parsed.card_types.contains(&CardType::Land);
    let is_artifact = parsed.card_types.contains(&CardType::Artifact);
    let is_enchantment = parsed.card_types.contains(&CardType::Enchantment);
    let is_planeswalker = parsed.card_types.contains(&CardType::Planeswalker);
    let is_battle = parsed.card_types.contains(&CardType::Battle);
    let is_spell = parsed.card_types.contains(&CardType::Instant)
        || parsed.card_types.contains(&CardType::Sorcery);

    let sub_tokens: Vec<&str> = subtypes_part.split_whitespace().collect();
    let mut i = 0usize;
    while i < sub_tokens.len() {
        let tok = sub_tokens[i];
        if is_battle && tok == "Siege" {
            parsed.is_siege = true;
            i += 1;
            continue;
        }
        // Handle two-word Urza's Power-Plant
        if is_land && tok == "Urza's" {
            parsed.land_subtypes.insert(LandType::Urzas);
            i += 1;
            continue;
        }
        if is_land && tok == "Power-Plant" {
            parsed.land_subtypes.insert(LandType::PowerPlant);
            i += 1;
            continue;
        }
        if is_enchantment && tok == "Aura" {
            parsed.is_aura = true;
            i += 1;
            continue;
        }
        if is_artifact && tok == "Equipment" {
            parsed.is_equipment = true;
            parsed.artifact_subtypes.insert(ArtifactSubtype::Equipment);
            i += 1;
            continue;
        }
        if is_artifact && tok == "Fortification" {
            parsed.is_fortification = true;
            parsed
                .artifact_subtypes
                .insert(ArtifactSubtype::Fortification);
            i += 1;
            continue;
        }
        if is_artifact && tok == "Vehicle" {
            parsed.is_vehicle = true;
            parsed.artifact_subtypes.insert(ArtifactSubtype::Vehicle);
            i += 1;
            continue;
        }
        if is_creature_family
            && i + 1 < sub_tokens.len()
            && let Some(ct) =
                vocabulary.resolve_label(&format!("{} {}", sub_tokens[i], sub_tokens[i + 1]))
        {
            parsed.creature_subtypes.insert(ct);
            i += 2;
            continue;
        }
        if is_land && let Some(lt) = parse_land_subtype(tok) {
            parsed.land_subtypes.insert(lt);
            i += 1;
            continue;
        }
        if is_artifact && let Some(at) = ArtifactSubtype::from_wire_label(tok) {
            parsed.artifact_subtypes.insert(at);
            i += 1;
            continue;
        }
        if is_enchantment && let Some(et) = EnchantmentSubtype::from_wire_label(tok) {
            parsed.enchantment_subtypes.insert(et);
            i += 1;
            continue;
        }
        if is_spell && let Some(st) = SpellSubtype::from_wire_label(tok) {
            parsed.spell_subtypes.insert(st);
            i += 1;
            continue;
        }
        if is_planeswalker && let Some(pt) = PlaneswalkerSubtype::from_label(tok) {
            parsed.planeswalker_subtypes.insert(pt);
            i += 1;
            continue;
        }
        if is_creature_family && let Some(ct) = vocabulary.resolve_label(tok) {
            parsed.creature_subtypes.insert(ct);
            i += 1;
            continue;
        }
        return Err("oracle:unsupported-subtype");
    }

    Ok(parsed)
}

fn parse_land_subtype(label: &str) -> Option<LandType> {
    match label {
        "Plains" => Some(LandType::Plains),
        "Island" => Some(LandType::Island),
        "Swamp" => Some(LandType::Swamp),
        "Mountain" => Some(LandType::Mountain),
        "Forest" => Some(LandType::Forest),
        "Cave" => Some(LandType::Cave),
        "Desert" => Some(LandType::Desert),
        "Gate" => Some(LandType::Gate),
        "Lair" => Some(LandType::Lair),
        "Locus" => Some(LandType::Locus),
        "Mine" => Some(LandType::Mine),
        "Planet" => Some(LandType::Planet),
        "Power-Plant" => Some(LandType::PowerPlant),
        "Sphere" => Some(LandType::Sphere),
        "Tower" => Some(LandType::Tower),
        "Town" => Some(LandType::Town),
        "Urza's" => Some(LandType::Urzas),
        _ => None,
    }
}

fn parse_basic_land_subtype(label: &str) -> Option<BasicLandType> {
    match label {
        "Plains" | "plains" => Some(BasicLandType::Plains),
        "Island" | "island" => Some(BasicLandType::Island),
        "Swamp" | "swamp" => Some(BasicLandType::Swamp),
        "Mountain" | "mountain" => Some(BasicLandType::Mountain),
        "Forest" | "forest" => Some(BasicLandType::Forest),
        _ => None,
    }
}

fn parse_mana_cost_full(raw: &str) -> Option<(ManaCost, u8, Vec<HybridSymbol>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut cost = ManaCost::free();
    let mut x_count: u8 = 0;
    let mut hybrids = Vec::new();
    let color_of = |s: &str| match s {
        "W" => Some(ManaType::White),
        "U" => Some(ManaType::Blue),
        "B" => Some(ManaType::Black),
        "R" => Some(ManaType::Red),
        "G" => Some(ManaType::Green),
        _ => None,
    };
    let symbols = trimmed.strip_prefix('{')?.strip_suffix('}')?;
    for sym in symbols.split("}{") {
        if let Some((first_s, second_s)) = sym.split_once('/') {
            if first_s == "2" {
                let first = color_of(second_s)?;
                hybrids.push(HybridSymbol {
                    first,
                    alternative: HybridAlternative::TwoGeneric,
                });
                let entry = cost.required.entry(first).or_insert_with(|| 0.into());
                *entry = entry.plus(&1.into());
                continue;
            }
            if first_s == "C" && second_s == "P" {
                hybrids.push(HybridSymbol {
                    first: ManaType::Colourless,
                    alternative: HybridAlternative::TwoLife,
                });
                let entry = cost
                    .required
                    .entry(ManaType::Colourless)
                    .or_insert_with(|| 0.into());
                *entry = entry.plus(&1.into());
                continue;
            }
            if first_s == "C" {
                let first = color_of(second_s)?;
                hybrids.push(HybridSymbol {
                    first,
                    alternative: HybridAlternative::Mana(ManaType::Colourless),
                });
                let entry = cost.required.entry(first).or_insert_with(|| 0.into());
                *entry = entry.plus(&1.into());
                continue;
            }
            let first = color_of(first_s)?;
            let alternative = if second_s == "P" {
                HybridAlternative::TwoLife
            } else if second_s == "C" {
                HybridAlternative::Mana(ManaType::Colourless)
            } else {
                HybridAlternative::Mana(color_of(second_s)?)
            };
            hybrids.push(HybridSymbol { first, alternative });
            let entry = cost.required.entry(first).or_insert_with(|| 0.into());
            *entry = entry.plus(&1.into());
            continue;
        }
        match sym {
            "X" => {
                x_count = x_count.checked_add(1)?;
            }
            "W" => {
                let entry = cost
                    .required
                    .entry(ManaType::White)
                    .or_insert_with(|| 0.into());
                *entry = entry.plus(&1.into());
            }
            "U" => {
                let entry = cost
                    .required
                    .entry(ManaType::Blue)
                    .or_insert_with(|| 0.into());
                *entry = entry.plus(&1.into());
            }
            "B" => {
                let entry = cost
                    .required
                    .entry(ManaType::Black)
                    .or_insert_with(|| 0.into());
                *entry = entry.plus(&1.into());
            }
            "R" => {
                let entry = cost
                    .required
                    .entry(ManaType::Red)
                    .or_insert_with(|| 0.into());
                *entry = entry.plus(&1.into());
            }
            "G" => {
                let entry = cost
                    .required
                    .entry(ManaType::Green)
                    .or_insert_with(|| 0.into());
                *entry = entry.plus(&1.into());
            }
            "C" => {
                let entry = cost
                    .required
                    .entry(ManaType::Colourless)
                    .or_insert_with(|| 0.into());
                *entry = entry.plus(&1.into());
            }
            _ => {
                if !sym.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let n: i64 = sym.parse().ok()?;
                cost.generic = cost.generic.plus(&n.into());
            }
        }
    }
    Some((cost, x_count, hybrids))
}

fn parse_mana_cost_with_x(raw: &str) -> Option<(ManaCost, u8)> {
    let (cost, x, hybrids) = parse_mana_cost_full(raw)?;
    hybrids.is_empty().then_some((cost, x))
}

fn parse_fixed_mana_cost(raw: &str) -> Option<ManaCost> {
    let (cost, x_count) = parse_mana_cost_with_x(raw)?;
    (x_count == 0).then_some(cost)
}

fn parse_mana_bundle(raw: &str) -> Option<BTreeMap<ManaType, RulesInteger>> {
    let trimmed = raw.trim();
    let symbols = trimmed.strip_prefix('{')?.strip_suffix('}')?;
    let mut out: BTreeMap<ManaType, RulesInteger> = BTreeMap::new();
    for sym in symbols.split("}{") {
        let up = sym.trim().to_ascii_uppercase();
        if let Some((mt, qty)) = match up.as_str() {
            "W" => Some((ManaType::White, 1i64)),
            "U" => Some((ManaType::Blue, 1)),
            "B" => Some((ManaType::Black, 1)),
            "R" => Some((ManaType::Red, 1)),
            "G" => Some((ManaType::Green, 1)),
            "C" => Some((ManaType::Colourless, 1)),
            s if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
                let n: i64 = s.parse().ok()?;
                (n > 0).then_some((ManaType::Colourless, n))
            }
            _ => None,
        } {
            let entry = out.entry(mt).or_insert_with(|| 0.into());
            *entry = entry.plus(&qty.into());
        } else {
            return None;
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

fn parse_single_mana_type(raw: &str) -> Option<ManaType> {
    let t = raw.trim().to_ascii_lowercase();
    match t.as_str() {
        "{w}" | "w" | "white" => Some(ManaType::White),
        "{u}" | "u" | "blue" => Some(ManaType::Blue),
        "{b}" | "b" | "black" => Some(ManaType::Black),
        "{r}" | "r" | "red" => Some(ManaType::Red),
        "{g}" | "g" | "green" => Some(ManaType::Green),
        "{c}" | "c" | "colorless" | "{1}" => Some(ManaType::Colourless),
        _ => None,
    }
}

fn parse_printed_stat(raw: &str) -> Option<PrintedStat> {
    let t = raw.trim();
    if t == "*" {
        return Some(PrintedStat::Star {
            multiplier: 1.into(),
            offset: 0.into(),
        });
    }
    if let Some(off) = t.strip_prefix("*+").or_else(|| t.strip_suffix("+*")) {
        let n: i64 = off.parse().ok()?;
        return Some(PrintedStat::Star {
            multiplier: 1.into(),
            offset: n.into(),
        });
    }
    if let Some(off) = t.strip_suffix("-*") {
        let n: i64 = off.parse().ok()?;
        return Some(PrintedStat::Star {
            multiplier: (-1).into(),
            offset: n.into(),
        });
    }
    let n: i64 = t.parse().ok()?;
    Some(PrintedStat::Number(n.into()))
}

fn lower_face(
    _card: &InventoryCard,
    face: &spellwake_content::CatalogFace,
    face_ast: &FaceAst,
    vocabulary: &CreatureTypeVocabulary,
    options: FaceLoweringOptions,
) -> Result<Program, &'static str> {
    let type_line = face
        .type_line
        .as_deref()
        .ok_or("oracle:missing-type-line")?;
    let types = parse_type_line(type_line, vocabulary)?;
    let is_spell = (types.card_types.contains(&CardType::Instant)
        || types.card_types.contains(&CardType::Sorcery))
        && !types.card_types.iter().any(|t| t.is_permanent());
    let is_land = types.card_types.contains(&CardType::Land);
    let is_creature = types.card_types.contains(&CardType::Creature);
    let is_planeswalker = types.card_types.contains(&CardType::Planeswalker);
    let is_battle = types.card_types.contains(&CardType::Battle);

    if is_planeswalker && options.printed_loyalty.is_none() {
        return Err("oracle:unsupported-card-type");
    }
    if is_battle && (!types.is_siege || options.printed_defense.is_none()) {
        return Err("oracle:unsupported-card-type");
    }

    let (casting_cost, x_symbols, hybrid_symbols, snow_mana_count) = if is_land {
        if face
            .mana_cost
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
        {
            return Err("oracle:land-with-mana-cost");
        }
        (None, 0, Vec::new(), 0u8)
    } else {
        let _ = options.allow_empty_mana_cost;
        let raw_cost = face.mana_cost.as_deref().unwrap_or("").trim();
        if raw_cost.is_empty() {
            // CR 118.6: Non-land faces with no printed mana cost are legal; permanents use
            // `casting_cost: None` and spells set `uncastable_mana_cost: true`.
            (None, 0, Vec::new(), 0u8)
        } else {
            let (stripped_cost, snow_count) =
                strip_snow_mana_symbols(raw_cost).ok_or("oracle:unsupported-mana-cost")?;
            let (cost, x, hybrids) = if stripped_cost.is_empty() {
                (ManaCost::free(), 0, Vec::new())
            } else {
                parse_mana_cost_full(&stripped_cost).ok_or("oracle:unsupported-mana-cost")?
            };
            (Some(cost), x, hybrids, snow_count)
        }
    };

    let mut ids = LoweringIds::default();
    let mut program = if is_spell {
        let (cost, uncastable_mana_cost) = match casting_cost {
            Some(cost) => (cost, false),
            None => (ManaCost::free(), true),
        };
        let mut cast_during_own_combat = false;
        let mut cannot_be_copied = false;
        let mut conditional_flash: Option<SelfFlashPermission> = None;
        let mut cast_timings: Vec<CastTimingRestriction> = Vec::new();
        let mut conditional_free_cast: Option<ConditionalCastCost> = None;
        let mut pre_filtered_spell_effects = Vec::with_capacity(face_ast.spell_effects.len());
        for ins in &face_ast.spell_effects {
            if let InstructionAst::CombatRuleMod {
                subject,
                rule,
                duration: DurationAst::Instant,
            } = ins
                && matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && rule.eq_ignore_ascii_case("cant_be_copied")
            {
                cannot_be_copied = true;
                continue;
            }
            if let InstructionAst::TemporaryStatic { static_ability, .. } = ins
                && let StaticAbilityAst::PlayOrCastPermission {
                    cast_only: true,
                    filter,
                    permission,
                    condition: Some(cond),
                } = static_ability.as_ref()
                && permission
                    .trim()
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case("as though it had flash")
                && is_self_flash_filter(filter)
            {
                if conditional_flash.is_some() {
                    return Err("oracle:duplicate-conditional-flash");
                }
                conditional_flash = Some(lower_self_flash_condition(cond, vocabulary)?);
                continue;
            }
            if let InstructionAst::IfCondition {
                condition,
                then_body,
            } = ins
                && let [InstructionAst::TemporaryStatic { static_ability, .. }] =
                    then_body.as_slice()
                && let StaticAbilityAst::PlayOrCastPermission {
                    cast_only: true,
                    filter,
                    permission,
                    condition: inner_cond,
                } = static_ability.as_ref()
                && permission
                    .trim()
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case("as though it had flash")
                && is_self_flash_filter(filter)
            {
                if conditional_flash.is_some() {
                    return Err("oracle:duplicate-conditional-flash");
                }
                let combined_cond = match inner_cond.as_deref() {
                    Some(ic) => ConditionAst::And(condition.clone(), Box::new(ic.clone())),
                    None => condition.as_ref().clone(),
                };
                conditional_flash = Some(lower_self_flash_condition(&combined_cond, vocabulary)?);
                continue;
            }
            if let InstructionAst::TemporaryStatic { static_ability, .. } = ins
                && let StaticAbilityAst::GrantKeywords {
                    subject,
                    keywords,
                    condition: Some(cond),
                    paired_rule: None,
                } = static_ability.as_ref()
                && matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && matches!(keywords.as_slice(), [kw] if kw.trim().eq_ignore_ascii_case("flash"))
            {
                if conditional_flash.is_some() {
                    return Err("oracle:duplicate-conditional-flash");
                }
                conditional_flash = Some(lower_self_flash_condition(cond, vocabulary)?);
                continue;
            }
            if let InstructionAst::IfCondition {
                condition,
                then_body,
            } = ins
                && let [InstructionAst::TemporaryStatic { static_ability, .. }] =
                    then_body.as_slice()
                && let StaticAbilityAst::GrantKeywords {
                    subject,
                    keywords,
                    condition: inner_cond,
                    paired_rule: None,
                } = static_ability.as_ref()
                && matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && matches!(keywords.as_slice(), [kw] if kw.trim().eq_ignore_ascii_case("flash"))
            {
                if conditional_flash.is_some() {
                    return Err("oracle:duplicate-conditional-flash");
                }
                let combined_cond = match inner_cond.as_deref() {
                    Some(ic) => ConditionAst::And(condition.clone(), Box::new(ic.clone())),
                    None => condition.as_ref().clone(),
                };
                conditional_flash = Some(lower_self_flash_condition(&combined_cond, vocabulary)?);
                continue;
            }
            if let InstructionAst::TemporaryStatic { static_ability, .. } = ins
                && conditional_free_cast.is_none()
                && let StaticAbilityAst::AlternativeCastCost {
                    cost,
                    condition: Some(cond),
                    for_spells: None,
                    next_only: false,
                } = static_ability.as_ref()
            {
                let alt_mana_opt = match cost.as_slice() {
                    [] => Some(ManaCost::free()),
                    [CostAtomAst::PayMana(raw)] => parse_fixed_mana_cost(&raw.to_ascii_uppercase()),
                    _ => None,
                };
                if let Some(mana) = alt_mana_opt {
                    if matches!(cond.as_ref(), ConditionAst::ControlsCommander) {
                        conditional_free_cast = Some(ConditionalCastCost::ControlsCommander(mana));
                        continue;
                    }
                    let mut tmp = BodyBuilder::new();
                    if let Ok(res_cond) =
                        lower_resolution_condition(cond, &mut tmp, vocabulary, false, None)
                        && tmp.target == BodyTarget::None
                        && res_cond.is_world_evaluable()
                        && res_cond.validate_world(vocabulary).is_ok()
                    {
                        conditional_free_cast = Some(ConditionalCastCost::Condition {
                            condition: Box::new(res_cond),
                            mana,
                        });
                        continue;
                    }
                }
            }
            if let InstructionAst::IfCondition {
                condition,
                then_body,
            } = ins
                && conditional_free_cast.is_none()
            {
                let (alt_mana_opt, extra_cond): (Option<ManaCost>, Option<&ConditionAst>) =
                    match then_body.as_slice() {
                        [
                            InstructionAst::GrantCastPermission {
                                subject,
                                free: true,
                                ..
                            },
                        ] if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
                            (Some(ManaCost::free()), None)
                        }
                        [InstructionAst::TemporaryStatic { static_ability, .. }] => {
                            match static_ability.as_ref() {
                                StaticAbilityAst::AlternativeCastCost {
                                    cost,
                                    condition: inner_cond,
                                    for_spells: None,
                                    next_only: false,
                                } => {
                                    let mana = match cost.as_slice() {
                                        [] => Some(ManaCost::free()),
                                        [CostAtomAst::PayMana(raw)] => {
                                            parse_fixed_mana_cost(&raw.to_ascii_uppercase())
                                        }
                                        _ => None,
                                    };
                                    (mana, inner_cond.as_deref())
                                }
                                _ => (None, None),
                            }
                        }
                        _ => (None, None),
                    };
                if let Some(mana) = alt_mana_opt {
                    let combined_cond = match extra_cond {
                        Some(ic) => ConditionAst::And(condition.clone(), Box::new(ic.clone())),
                        None => condition.as_ref().clone(),
                    };
                    if matches!(combined_cond, ConditionAst::ControlsCommander) {
                        conditional_free_cast = Some(ConditionalCastCost::ControlsCommander(mana));
                        continue;
                    }
                    let mut tmp = BodyBuilder::new();
                    if let Ok(res_cond) = lower_resolution_condition(
                        &combined_cond,
                        &mut tmp,
                        vocabulary,
                        false,
                        None,
                    ) && tmp.target == BodyTarget::None
                        && res_cond.is_world_evaluable()
                        && res_cond.validate_world(vocabulary).is_ok()
                    {
                        conditional_free_cast = Some(ConditionalCastCost::Condition {
                            condition: Box::new(res_cond),
                            mana,
                        });
                        continue;
                    }
                }
            }
            pre_filtered_spell_effects.push(ins.clone());
        }
        let filtered_spell_effects = strip_spell_cast_restrictions(
            &pre_filtered_spell_effects,
            &mut cast_during_own_combat,
            &mut cast_timings,
            vocabulary,
        );
        let cast_timing = match cast_timings.len() {
            0 => None,
            1 => cast_timings.pop(),
            _ => Some(CastTimingRestriction::All(cast_timings)),
        };
        let (body, modes) = lower_spell_effects(&filtered_spell_effects, vocabulary, &mut ids)?;
        let mut p = Program::new(
            types.card_types.clone(),
            types.creature_subtypes.clone(),
            BTreeSet::new(),
            ProgramKind::Spell(SpellDefinition {
                cost,
                uncastable_mana_cost,
                body,
                modes,
            }),
        );
        p.spell_subtypes = types.spell_subtypes;
        p.supertypes = types.supertypes;
        p.x_mana_symbols = x_symbols;
        p.hybrid_symbols = hybrid_symbols;
        p.cast_during_own_combat = cast_during_own_combat;
        p.cannot_be_copied = cannot_be_copied;
        p.conditional_flash = conditional_flash;
        p.cast_timing = cast_timing;
        if let Some(cc) = conditional_free_cast {
            p.conditional_cost = Some(cc);
        }
        p
    } else {
        if !face_ast.spell_effects.is_empty() {
            return Err("oracle:permanent-with-spell-effects");
        }
        let is_spacecraft = types
            .artifact_subtypes
            .contains(&ArtifactSubtype::Spacecraft);
        let printed_pt = if is_creature
            || types.is_vehicle
            || (is_spacecraft && face.power.is_some() && face.toughness.is_some())
        {
            let p_raw = face.power.as_deref().ok_or("oracle:missing-power")?;
            let t_raw = face
                .toughness
                .as_deref()
                .ok_or("oracle:missing-toughness")?;
            Some(PrintedPowerToughness {
                power: parse_printed_stat(p_raw).ok_or("oracle:unsupported-pt")?,
                toughness: parse_printed_stat(t_raw).ok_or("oracle:unsupported-pt")?,
            })
        } else {
            None
        };
        let attachment = if types.is_aura {
            let restriction = find_aura_restriction(&face_ast.statics, vocabulary)?;
            Some(AttachmentKind::Aura(restriction))
        } else if types.is_equipment {
            Some(AttachmentKind::Equipment)
        } else if types.is_fortification {
            Some(AttachmentKind::Fortification)
        } else {
            None
        };
        let mut p = Program::new(
            types.card_types.clone(),
            types.creature_subtypes.clone(),
            types.land_subtypes.clone(),
            ProgramKind::Permanent(PermanentDefinition {
                entry_tap: types.is_siege.then_some(EntryTap::SiegeProtector),
                attachment,
                printed_pt,
                casting_cost,
            }),
        );
        p.artifact_subtypes = types.artifact_subtypes;
        p.enchantment_subtypes = types.enchantment_subtypes;
        p.planeswalker_subtypes = types.planeswalker_subtypes;
        p.printed_loyalty = options.printed_loyalty.map(|n| i64::from(n).into());
        p.printed_defense = options.printed_defense.map(|n| i64::from(n).into());
        p.supertypes = types.supertypes;
        p.x_mana_symbols = x_symbols;
        p.hybrid_symbols = hybrid_symbols;
        if types.is_siege {
            p.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                grouping: TriggerGrouping::EachOccurrence,
                condition: TriggerSpec::counters_changed(
                    ObjectFilter::self_object(),
                    CounterTriggerKind::LastDefenseRemoved,
                ),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::exile_defeated_siege(),
                        )),
                        Instruction::StackOp(Box::new(
                            spellwake_engine::toolbox::StackOpSpec::cast_defeated_siege(),
                        )),
                    ],
                ),
            });
        }
        p
    };

    if let Some(snow_nz) = NonZeroU8::new(snow_mana_count) {
        program
            .extra_cast_costs
            .push(ExtraActivationCost::PaySnowMana(snow_nz));
    }

    // Track Saga Read Ahead and Prototype keywords for post-lowering attachment
    let mut has_read_ahead = false;
    let mut prototype_spec: Option<(ManaCost, u8, Vec<HybridSymbol>, PrintedPowerToughness)> = None;

    // Lower keywords
    for (kw_idx, kw) in face_ast.keywords.iter().enumerate() {
        if options.allow_aftermath
            && kw.name.eq_ignore_ascii_case("aftermath")
            && kw.parameter.is_none()
        {
            continue;
        }
        if options.allow_fuse && kw.name.eq_ignore_ascii_case("fuse") && kw.parameter.is_none() {
            continue;
        }
        if options.allow_disturb && kw.name.eq_ignore_ascii_case("disturb") {
            continue;
        }
        if kw.name.eq_ignore_ascii_case("prototype") {
            let param = kw
                .parameter
                .as_deref()
                .ok_or("oracle:prototype-missing-param")?;
            let (cost_str, pt_str) = param
                .split_once(" \u{2014} ")
                .or_else(|| param.split_once(" - "))
                .or_else(|| param.split_once('\u{2014}'))
                .ok_or("oracle:bad-prototype-param")?;
            let (proto_cost, proto_x, proto_hybrids) =
                parse_mana_cost_full(&cost_str.trim().to_ascii_uppercase())
                    .ok_or("oracle:bad-prototype-cost")?;
            let (p_raw, t_raw) = pt_str
                .trim()
                .split_once('/')
                .ok_or("oracle:bad-prototype-pt")?;
            let proto_pt = PrintedPowerToughness {
                power: parse_printed_stat(p_raw.trim()).ok_or("oracle:bad-prototype-pt")?,
                toughness: parse_printed_stat(t_raw.trim()).ok_or("oracle:bad-prototype-pt")?,
            };
            if prototype_spec
                .replace((proto_cost, proto_x, proto_hybrids, proto_pt))
                .is_some()
            {
                return Err("oracle:duplicate-prototype");
            }
            continue;
        }
        if program
            .enchantment_subtypes
            .contains(&EnchantmentSubtype::Saga)
            && kw.name.eq_ignore_ascii_case("read ahead")
            && kw.parameter.is_none()
        {
            has_read_ahead = true;
            continue;
        }
        if is_spell && kw.name.eq_ignore_ascii_case("cleave") {
            let mana = kw
                .parameter
                .as_deref()
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:cleave-cost")?;
            let raw_oracle = face.oracle_text.as_deref().unwrap_or("");
            let cleaved_text = strip_cleave_brackets(raw_oracle)?;
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            let cleaved_card = compile_card(
                &CardInput {
                    oracle_id: "",
                    name: &face.name,
                    layout: "normal",
                    faces: vec![FaceInput {
                        name: &face.name,
                        type_line,
                        oracle_text: &cleaved_text,
                    }],
                },
                &compiler_vocab,
            );
            let [cleaved_face] = cleaved_card.faces.as_slice() else {
                return Err("oracle:cleave-compile-failed");
            };
            if !cleaved_card.strictly_compiled || cleaved_face.spell_effects.is_empty() {
                return Err("oracle:cleave-compile-failed");
            }
            let cleaved_body = lower_effect_body(
                &cleaved_face.spell_effects,
                vocabulary,
                &mut ids,
                false,
                false,
            )?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Cleave {
                    mana,
                    unbracketed: Box::new(cleaved_body),
                })
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            continue;
        }
        lower_keyword(
            kw,
            kw_idx,
            face_ast,
            &mut program,
            vocabulary,
            &mut ids,
            is_spell,
        )?;
    }

    // Lower replacement effects
    let raw_oracle_low = face
        .oracle_text
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    for rep in &face_ast.replacements {
        if replacement_has_temporary_enters_as_copy(rep) {
            return Err("oracle:unsupported-enters-as-copy");
        }
        if let ReplacementEffectAst::AsEntersAction(ins) = rep {
            if raw_oracle_low.contains("becomes attached")
                || as_enters_action_mentions_attachment(ins)
            {
                return Err("oracle:unsupported-replacement");
            }
            if let [InstructionAst::Modal { choice, modes }] = ins.as_slice()
                && modes.len() >= 2
            {
                let choice_clean = choice
                    .trim()
                    .trim_end_matches(['.', ':', '\u{2014}', '\u{2013}'])
                    .trim()
                    .to_ascii_lowercase();
                let choice_norm = choice_clean
                    .strip_prefix("choose ")
                    .unwrap_or(&choice_clean)
                    .trim();
                let entry_tap = match choice_norm {
                    "khans or dragons" => EntryTap::ChooseKhansOrDragons,
                    "dragons or khans" => EntryTap::ChooseOptions(&["Dragons", "Khans"]),
                    "odd or even" => EntryTap::ChooseOddOrEven,
                    "even or odd" => EntryTap::ChooseOptions(&["Even", "Odd"]),
                    "abzan or mardu" => EntryTap::ChooseOptions(&["Abzan", "Mardu"]),
                    "mardu or abzan" => EntryTap::ChooseOptions(&["Mardu", "Abzan"]),
                    "temur or sultai" => EntryTap::ChooseOptions(&["Temur", "Sultai"]),
                    "sultai or temur" => EntryTap::ChooseOptions(&["Sultai", "Temur"]),
                    "sultai or abzan" => EntryTap::ChooseOptions(&["Sultai", "Abzan"]),
                    "abzan or sultai" => EntryTap::ChooseOptions(&["Abzan", "Sultai"]),
                    "mardu or jeskai" => EntryTap::ChooseOptions(&["Mardu", "Jeskai"]),
                    "jeskai or mardu" => EntryTap::ChooseOptions(&["Jeskai", "Mardu"]),
                    "jeskai or temur" => EntryTap::ChooseOptions(&["Jeskai", "Temur"]),
                    "temur or jeskai" => EntryTap::ChooseOptions(&["Temur", "Jeskai"]),
                    "brotherhood or enclave" => {
                        EntryTap::ChooseOptions(&["Brotherhood", "Enclave"])
                    }
                    "enclave or brotherhood" => {
                        EntryTap::ChooseOptions(&["Enclave", "Brotherhood"])
                    }
                    "mirran or phyrexian" => EntryTap::ChooseOptions(&["Mirran", "Phyrexian"]),
                    "phyrexian or mirran" => EntryTap::ChooseOptions(&["Phyrexian", "Mirran"]),
                    "ncr or legion" => EntryTap::ChooseOptions(&["NCR", "Legion"]),
                    "legion or ncr" => EntryTap::ChooseOptions(&["Legion", "NCR"]),
                    "flavorful or bland" => EntryTap::ChooseOptions(&["Flavorful", "Bland"]),
                    "bland or flavorful" => EntryTap::ChooseOptions(&["Bland", "Flavorful"]),
                    "believe or doubt" => EntryTap::ChooseOptions(&["Believe", "Doubt"]),
                    "doubt or believe" => EntryTap::ChooseOptions(&["Doubt", "Believe"]),
                    "anchors or sails" => EntryTap::ChooseOptions(&["Anchors", "Sails"]),
                    "sails or anchors" => EntryTap::ChooseOptions(&["Sails", "Anchors"]),
                    "friends or foes" => EntryTap::ChooseOptions(&["Friends", "Foes"]),
                    "foes or friends" => EntryTap::ChooseOptions(&["Foes", "Friends"]),
                    "friend or foe" => EntryTap::ChooseOptions(&["Friend", "Foe"]),
                    "foe or friend" => EntryTap::ChooseOptions(&["Foe", "Friend"]),
                    "left or right" => EntryTap::ChooseOptions(&["Left", "Right"]),
                    "right or left" => EntryTap::ChooseOptions(&["Right", "Left"]),
                    "order or chaos" => EntryTap::ChooseOptions(&["Order", "Chaos"]),
                    "chaos or order" => EntryTap::ChooseOptions(&["Chaos", "Order"]),
                    "war or peace" => EntryTap::ChooseOptions(&["War", "Peace"]),
                    "peace or war" => EntryTap::ChooseOptions(&["Peace", "War"]),
                    "wild or tame" => EntryTap::ChooseOptions(&["Wild", "Tame"]),
                    "tame or wild" => EntryTap::ChooseOptions(&["Tame", "Wild"]),
                    "day or night" => EntryTap::ChooseOptions(&["Day", "Night"]),
                    "night or day" => EntryTap::ChooseOptions(&["Night", "Day"]),
                    "light or dark" => EntryTap::ChooseOptions(&["Light", "Dark"]),
                    "dark or light" => EntryTap::ChooseOptions(&["Dark", "Light"]),
                    "sun or moon" => EntryTap::ChooseOptions(&["Sun", "Moon"]),
                    "moon or sun" => EntryTap::ChooseOptions(&["Moon", "Sun"]),
                    "life or death" => EntryTap::ChooseOptions(&["Life", "Death"]),
                    "death or life" => EntryTap::ChooseOptions(&["Death", "Life"]),
                    "fire or ice" => EntryTap::ChooseOptions(&["Fire", "Ice"]),
                    "ice or fire" => EntryTap::ChooseOptions(&["Ice", "Fire"]),
                    "good or evil" => EntryTap::ChooseOptions(&["Good", "Evil"]),
                    "evil or good" => EntryTap::ChooseOptions(&["Evil", "Good"]),
                    "high or low" => EntryTap::ChooseOptions(&["High", "Low"]),
                    "low or high" => EntryTap::ChooseOptions(&["Low", "High"]),
                    "heads or tails" => EntryTap::ChooseOptions(&["Heads", "Tails"]),
                    "tails or heads" => EntryTap::ChooseOptions(&["Tails", "Heads"]),
                    "trick or treat" => EntryTap::ChooseOptions(&["Trick", "Treat"]),
                    "treat or trick" => EntryTap::ChooseOptions(&["Treat", "Trick"]),
                    "truth or dare" => EntryTap::ChooseOptions(&["Truth", "Dare"]),
                    "dare or truth" => EntryTap::ChooseOptions(&["Dare", "Truth"]),
                    "ally or enemy" => EntryTap::ChooseOptions(&["Ally", "Enemy"]),
                    "enemy or ally" => EntryTap::ChooseOptions(&["Enemy", "Ally"]),
                    "money or power" => EntryTap::ChooseOptions(&["Money", "Power"]),
                    "power or money" => EntryTap::ChooseOptions(&["Power", "Money"]),
                    other => {
                        if modes.len() == 2
                            && let Some((opt_a, opt_b)) = other.split_once(" or ")
                            && !opt_a.trim().is_empty()
                            && !opt_b.trim().is_empty()
                            && !opt_b.contains(" or ")
                            && !opt_a.contains(',')
                            && !opt_b.contains(',')
                            && modes[0]
                                .label
                                .as_deref()
                                .is_some_and(|l| l.trim().eq_ignore_ascii_case(opt_a.trim()))
                            && modes[1]
                                .label
                                .as_deref()
                                .is_some_and(|l| l.trim().eq_ignore_ascii_case(opt_b.trim()))
                        {
                            EntryTap::ChooseOptions(&["Option 1", "Option 2"])
                        } else {
                            return Err("oracle:unsupported-replacement");
                        }
                    }
                };
                if entry_tap
                    .option_labels()
                    .is_some_and(|labels| labels.len() != modes.len())
                {
                    return Err("oracle:unsupported-replacement");
                }
                let ProgramKind::Permanent(def) = &mut program.kind else {
                    return Err("oracle:replacement-on-spell");
                };
                if def.entry_tap.replace(entry_tap).is_some() {
                    return Err("oracle:duplicate-entry-tap");
                }
                for (idx, mode) in modes.iter().enumerate() {
                    let idx_u8 = u8::try_from(idx).map_err(|_| "oracle:unsupported-replacement")?;
                    if let Some(gf) = mode.granted.as_deref() {
                        if !gf.spell_effects.is_empty() {
                            return Err("oracle:unsupported-replacement");
                        }
                        lower_gated_section(
                            face_ast,
                            &gf.keywords,
                            &gf.statics,
                            &gf.abilities,
                            &gf.replacements,
                            GatedSectionMode::EntryOption(idx_u8),
                            &mut program,
                            vocabulary,
                            &mut ids,
                        )?;
                    } else {
                        lower_as_enters_modal_body(
                            face_ast,
                            mode,
                            idx_u8,
                            &mut program,
                            vocabulary,
                            &mut ids,
                        )?;
                    }
                }
                continue;
            }
        }
        lower_replacement(rep, &mut program, vocabulary, &mut ids)?;
    }

    // Lower static abilities (deferring PrototypeStats, StationBand, and DieRollBand for post-ability wiring)
    let mut station_bands: Vec<&StaticAbilityAst> = Vec::new();
    let mut die_roll_bands: Vec<(&str, &str, Option<&[InstructionAst]>)> = Vec::new();
    let mut prev_static_subject: Option<SubjectRefAst> = None;
    for st in &face_ast.statics {
        match st {
            StaticAbilityAst::PrototypeStats {
                cost,
                power,
                toughness,
            } => {
                let [CostAtomAst::PayMana(raw)] = cost.as_slice() else {
                    return Err("oracle:bad-prototype-cost");
                };
                let (proto_cost, proto_x, proto_hybrids) =
                    parse_mana_cost_full(&raw.trim().to_ascii_uppercase())
                        .ok_or("oracle:bad-prototype-cost")?;
                let proto_pt = PrintedPowerToughness {
                    power: parse_printed_stat(power.trim()).ok_or("oracle:bad-prototype-pt")?,
                    toughness: parse_printed_stat(toughness.trim())
                        .ok_or("oracle:bad-prototype-pt")?,
                };
                if prototype_spec
                    .replace((proto_cost, proto_x, proto_hybrids, proto_pt))
                    .is_some()
                {
                    return Err("oracle:duplicate-prototype");
                }
            }
            StaticAbilityAst::StationBand { .. } | StaticAbilityAst::CompiledStationBand { .. } => {
                station_bands.push(st);
            }
            StaticAbilityAst::DieRollBand { roll, payload } => {
                die_roll_bands.push((roll.as_str(), payload.as_str(), None));
            }
            StaticAbilityAst::CompiledDieRollBand {
                roll,
                payload,
                body,
            } => {
                die_roll_bands.push((roll.as_str(), payload.as_str(), Some(body.as_slice())));
            }
            StaticAbilityAst::PlayOrCastPermission {
                cast_only: true,
                filter,
                permission,
                condition: Some(cond),
            } if permission
                .trim()
                .trim_end_matches('.')
                .eq_ignore_ascii_case("as though it had flash")
                && is_self_flash_filter(filter) =>
            {
                if program.conditional_flash.is_some() {
                    return Err("oracle:duplicate-conditional-flash");
                }
                program.conditional_flash = Some(lower_self_flash_condition(cond, vocabulary)?);
            }
            StaticAbilityAst::RuleOrCombatRestriction {
                subject,
                rule,
                filter: None,
                condition: None,
            } if matches!(subject.as_deref(), None | Some(SubjectRefAst::SelfObject))
                && matches!(
                    rule.trim()
                        .trim_end_matches('.')
                        .to_ascii_lowercase()
                        .as_str(),
                    "cant_be_copied"
                        | "~ can't be copied"
                        | "can't be copied"
                        | "this spell can't be copied"
                ) =>
            {
                program.cannot_be_copied = true;
            }
            StaticAbilityAst::Compound(parts)
                if parts.iter().any(|part| {
                    is_hoistable_self_flash_static(part)
                        || (parts.iter().any(|other| {
                            matches!(
                                other,
                                StaticAbilityAst::AlternativeCastCost {
                                    for_spells: None,
                                    next_only: false,
                                    ..
                                }
                            )
                        }) && is_self_conditional_flash_keyword_grant(part))
                }) =>
            {
                let has_self_alt_cost = parts.iter().any(|other| {
                    matches!(
                        other,
                        StaticAbilityAst::AlternativeCastCost {
                            for_spells: None,
                            next_only: false,
                            ..
                        }
                    )
                });
                let mut remaining = Vec::with_capacity(parts.len());
                for part in parts {
                    if let StaticAbilityAst::PlayOrCastPermission {
                        condition: Some(cond),
                        ..
                    } = part
                        && is_hoistable_self_flash_static(part)
                    {
                        if program.conditional_flash.is_some() {
                            return Err("oracle:duplicate-conditional-flash");
                        }
                        program.conditional_flash =
                            Some(lower_self_flash_condition(cond, vocabulary)?);
                    } else if has_self_alt_cost
                        && let StaticAbilityAst::GrantKeywords {
                            condition: Some(cond),
                            ..
                        } = part
                        && is_self_conditional_flash_keyword_grant(part)
                    {
                        if program.conditional_flash.is_some() {
                            return Err("oracle:duplicate-conditional-flash");
                        }
                        program.conditional_flash =
                            Some(lower_self_flash_condition(cond, vocabulary)?);
                    } else {
                        remaining.push(part.clone());
                    }
                }
                for rem in &remaining {
                    lower_static(rem, &mut program, vocabulary, &mut ids, is_spell)?;
                }
            }
            _ => {
                if let Some(prev) = &prev_static_subject
                    && matches!(static_ast_subject(st), Some(SubjectRefAst::Anaphora(_)))
                {
                    let mut resolved = st.clone();
                    resolve_static_anaphora_subject_across_statics(&mut resolved, prev);
                    lower_static(&resolved, &mut program, vocabulary, &mut ids, is_spell)?;
                } else {
                    if let Some(subj) = static_ast_subject(st)
                        && !matches!(subj, SubjectRefAst::Anaphora(_))
                    {
                        prev_static_subject = Some(subj.clone());
                    }
                    lower_static(st, &mut program, vocabulary, &mut ids, is_spell)?;
                }
            }
        }
    }

    if program.sacrifice_creature_cast_cost
        && let ProgramKind::Spell(spell) = &mut program.kind
    {
        rewrite_body_anaphora_stats(&mut spell.body, ValueObjectRef::SacrificedCreature);
        if let Some(modes) = &mut spell.modes {
            for mode in &mut modes.additional {
                rewrite_body_anaphora_stats(mode, ValueObjectRef::SacrificedCreature);
            }
        }
    }

    // Lower abilities (activated + triggered + loyalty + saga chapters)
    for ab in &face_ast.abilities {
        lower_ability(ab, &mut program, vocabulary, &mut ids)?;
    }

    // Wire StationBand thresholds (`N+ | <payload>`) on Spacecraft permanents
    if !station_bands.is_empty() {
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        let max_station = station_bands
            .iter()
            .filter_map(|band| match band {
                StaticAbilityAst::StationBand { min_charge, .. }
                | StaticAbilityAst::CompiledStationBand { min_charge, .. } => Some(*min_charge),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let has_pt = matches!(
            &program.kind,
            ProgramKind::Permanent(def) if def.printed_pt.is_some()
        );
        let oracle_low = face
            .oracle_text
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        for band in station_bands {
            let (min_charge, payload) = match band {
                StaticAbilityAst::StationBand {
                    min_charge,
                    payload,
                }
                | StaticAbilityAst::CompiledStationBand {
                    min_charge,
                    payload,
                    ..
                } => (*min_charge, payload.as_str()),
                _ => return Err("oracle:unsupported-station-band"),
            };
            let cond = StaticSourceCondition::SourceCounterAtLeast {
                kind: CounterKind::Charge,
                minimum: u16::from(min_charge),
            };
            let becomes_creature = !matches!(
                band,
                StaticAbilityAst::CompiledStationBand { power: Some(_), .. }
            ) && has_pt
                && (oracle_low.contains(&format!("artifact creature at {min_charge}+"))
                    || (min_charge == max_station
                        && !oracle_low.contains("artifact creature at ")));
            if becomes_creature {
                push_static_ability(
                    &mut program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            source_condition: Some(cond.clone()),
                            ..PermanentSelector::default()
                        },
                        parts: StaticParts(vec![StaticPart::Type(
                            spellwake_engine::characteristics::StaticTypeChange {
                                supertypes: TypeOperation::Keep,
                                types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                                subtypes: SubtypeOperation::Keep,
                                land_subtypes: SubtypeOperation::Keep,
                            },
                        )]),
                    },
                )?;
            }
            if let StaticAbilityAst::CompiledStationBand {
                power,
                toughness,
                compiled,
                ..
            } = band
            {
                lower_compiled_station_band(
                    face_ast,
                    power.as_deref(),
                    toughness.as_deref(),
                    compiled.as_deref(),
                    cond,
                    &mut program,
                    vocabulary,
                    &mut ids,
                )?;
                continue;
            }
            let band_card = compile_card(
                &CardInput {
                    oracle_id: "",
                    name: &face.name,
                    layout: "normal",
                    faces: vec![FaceInput {
                        name: &face.name,
                        type_line,
                        oracle_text: payload,
                    }],
                },
                &compiler_vocab,
            );
            let [band_face] = band_card.faces.as_slice() else {
                return Err("oracle:station-band-compile-failed");
            };
            if !band_card.strictly_compiled || !band_face.spell_effects.is_empty() {
                return Err("oracle:station-band-compile-failed");
            }
            lower_gated_section(
                face_ast,
                &band_face.keywords,
                &band_face.statics,
                &band_face.abilities,
                &band_face.replacements,
                GatedSectionMode::LevelerBand(cond),
                &mut program,
                vocabulary,
                &mut ids,
            )?;
        }
    }

    // Wire DieRollBand thresholds (`1-9 | ...`, `10-19 | ...`, `20 | ...`, `15+ | ...`) onto RollDie
    if !die_roll_bands.is_empty() {
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        let (roll_in_trigger, roll_target) = find_die_roll_context(&program);
        let mut parsed_bands = Vec::with_capacity(die_roll_bands.len());
        for (roll_raw, payload_raw, compiled) in die_roll_bands {
            let (min, max) = parse_die_roll_range(roll_raw).ok_or("oracle:bad-die-roll-range")?;
            let parsed;
            let ins = if let Some(compiled) = compiled {
                if compiled.is_empty() {
                    return Err("oracle:die-roll-band-parse-failed");
                }
                compiled
            } else {
                let stripped = strip_die_band_ability_word(payload_raw);
                let normalized = stripped.replace(&face.name, "~");
                let (body, fails) = spellwake_oracle_compiler::clauses::parse_effect_body(
                    &normalized,
                    &compiler_vocab,
                );
                if !fails.is_empty() || body.is_empty() {
                    return Err("oracle:die-roll-band-parse-failed");
                }
                parsed = body;
                parsed.as_slice()
            };
            let band_body = lower_die_roll_band_body(
                ins,
                vocabulary,
                &mut ids,
                roll_in_trigger,
                !is_spell,
                roll_target,
            )?;
            parsed_bands.push(spellwake_engine::program::DieRollBand {
                min,
                max,
                body: band_body,
            });
        }
        if !attach_die_roll_bands(&mut program, &parsed_bands) {
            return Err("oracle:orphan-die-roll-band");
        }
    }

    // Lower structured Leveler bands (CR 711)
    for band in &face_ast.level_bands {
        let min_cond = StaticSourceCondition::SourceCounterAtLeast {
            kind: CounterKind::Level,
            minimum: u16::from(band.min_level),
        };
        let band_cond = match band.max_level {
            Some(max) => StaticSourceCondition::And(
                Box::new(min_cond),
                Box::new(StaticSourceCondition::Not(Box::new(
                    StaticSourceCondition::SourceCounterAtLeast {
                        kind: CounterKind::Level,
                        minimum: u16::from(max).saturating_add(1),
                    },
                ))),
            ),
            None => min_cond,
        };
        if let (Some(p_raw), Some(t_raw)) = (&band.power, &band.toughness) {
            let p_val: i64 = p_raw.trim().parse().map_err(|_| "oracle:leveler-bad-pt")?;
            let t_val: i64 = t_raw.trim().parse().map_err(|_| "oracle:leveler-bad-pt")?;
            push_static_ability(
                &mut program,
                StaticContinuousAbility::Battlefield {
                    id: ids.static_id(),
                    selector: PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        source_condition: Some(band_cond.clone()),
                        ..PermanentSelector::default()
                    },
                    parts: StaticParts(vec![StaticPart::SetPT(NumericPair {
                        power: NumericExpression::Literal(p_val.into()),
                        toughness: NumericExpression::Literal(t_val.into()),
                    })]),
                },
            )?;
        }
        lower_gated_section(
            face_ast,
            &band.keywords,
            &band.statics,
            &band.abilities,
            &band.replacements,
            GatedSectionMode::LevelerBand(band_cond),
            &mut program,
            vocabulary,
            &mut ids,
        )?;
    }

    // Lower structured Class levels (CR 716)
    for section in &face_ast.class_levels {
        if section.level < 2 {
            return Err("oracle:bad-class-level");
        }
        let (mana_cost, source_cost, hybrids, extra_costs) =
            lower_activation_cost_full(&section.cost, None, vocabulary)?;
        let id = ids.activated();
        if !hybrids.is_empty() {
            program.activation_hybrid_symbols.insert(id, hybrids);
        }
        if !extra_costs.is_empty() {
            program.extra_activation_costs.insert(id, extra_costs);
        }
        program.activation_restrictions.insert(
            id,
            ActivationRestriction::All(vec![
                ActivationRestriction::Sorcery,
                ActivationRestriction::ClassLevelExact(section.level - 1),
            ]),
        );
        program.activated.push(ActivatedAbility {
            id,
            mana_cost,
            source_cost,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::advance_class_level(section.level),
                ))],
            ),
        });
        lower_gated_section(
            face_ast,
            &section.keywords,
            &section.statics,
            &section.abilities,
            &section.replacements,
            GatedSectionMode::ClassLevel(section.level),
            &mut program,
            vocabulary,
            &mut ids,
        )?;
    }

    // Lower structured Case `Solved - ...` abilities (CR 719)
    if !face_ast.case_solved.is_empty() {
        lower_gated_section(
            face_ast,
            &face_ast.case_solved.keywords,
            &face_ast.case_solved.statics,
            &face_ast.case_solved.abilities,
            &face_ast.case_solved.replacements,
            GatedSectionMode::CaseSolved,
            &mut program,
            vocabulary,
            &mut ids,
        )?;
    }

    if has_read_ahead {
        let final_chapter = program
            .triggered
            .iter()
            .filter_map(|t| match t.condition.event {
                TriggerEvent::CountersChanged {
                    kind: CounterTriggerKind::SagaChapter(n),
                    ..
                } => Some(n),
                _ => None,
            })
            .max()
            .ok_or("oracle:read-ahead-no-chapters")?;
        if let ProgramKind::Permanent(def) = &mut program.kind
            && def
                .entry_tap
                .replace(EntryTap::ReadAhead { final_chapter })
                .is_some()
        {
            return Err("oracle:duplicate-entry-tap");
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
            return Err("oracle:prototype-non-permanent");
        }
        program.prototype = Some(Box::new(proto));
    }

    Ok(program)
}

enum GatedSectionMode {
    LevelerBand(StaticSourceCondition),
    ClassLevel(u8),
    CaseSolved,
    EntryOption(u8),
}

fn strip_cleave_brackets(raw: &str) -> Result<String, &'static str> {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.to_ascii_lowercase().starts_with("cleave ") {
            continue;
        }
        lines.push(t);
    }
    let joined = lines.join("\n");
    if !joined.contains('[') {
        return Err("oracle:cleave-no-brackets");
    }
    let mut out = String::with_capacity(joined.len());
    let mut depth = 0usize;
    for ch in joined.chars() {
        match ch {
            '[' => depth += 1,
            ']' => {
                if depth == 0 {
                    return Err("oracle:cleave-unbalanced-brackets");
                }
                depth -= 1;
            }
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    if depth != 0 {
        return Err("oracle:cleave-unbalanced-brackets");
    }
    let cleaned = out
        .replace("  ", " ")
        .replace(" .", ".")
        .replace(" ,", ",")
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return Err("oracle:cleave-empty-body");
    }
    Ok(cleaned)
}

fn parse_die_roll_range(raw: &str) -> Option<(u16, u16)> {
    let t = raw.trim();
    if let Some(min_s) = t.strip_suffix('+') {
        let min = min_s.trim().parse::<u16>().ok()?;
        return Some((min, u16::MAX));
    }
    for sep in ['\u{2014}', '\u{2013}', '-'] {
        if let Some((a, b)) = t.split_once(sep) {
            let min = a.trim().parse::<u16>().ok()?;
            let max = b.trim().parse::<u16>().ok()?;
            return (min <= max).then_some((min, max));
        }
    }
    let exact = t.parse::<u16>().ok()?;
    Some((exact, exact))
}

fn strip_die_band_ability_word(raw: &str) -> &str {
    if let Some((_, rest)) = raw.split_once(" \u{2014} ") {
        rest.trim()
    } else {
        raw.trim()
    }
}

fn body_has_empty_die_roll(body: &ExecutableBody) -> bool {
    body.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::MutatePlayer(spec)
                if matches!(
                    &spec.op,
                    spellwake_engine::toolbox::PlayerMutation::RandomEvent(spellwake_engine::toolbox::RandomEventOp::RollDie { bands, .. })
                        if bands.is_empty()
                )
        )
    })
}

fn find_die_roll_context(program: &Program) -> (bool, BodyTarget) {
    if let ProgramKind::Spell(spell) = &program.kind {
        if body_has_empty_die_roll(&spell.body) {
            return (false, spell.body.target);
        }
        if let Some(modes) = &spell.modes {
            if let Some(p) = &modes.prefix
                && body_has_empty_die_roll(p)
            {
                return (false, p.target);
            }
            if let Some(s) = &modes.suffix
                && body_has_empty_die_roll(s)
            {
                return (false, s.target);
            }
            for m in &modes.additional {
                if body_has_empty_die_roll(m) {
                    return (false, m.target);
                }
            }
        }
    }
    for tr in &program.triggered {
        if body_has_empty_die_roll(&tr.body) {
            return (true, tr.body.target);
        }
    }
    for act in &program.activated {
        if body_has_empty_die_roll(&act.body) {
            return (false, act.body.target);
        }
    }
    (false, BodyTarget::None)
}

fn attach_die_roll_bands_in_body(
    body: &mut ExecutableBody,
    bands: &[spellwake_engine::program::DieRollBand],
) -> bool {
    let mut attached = false;
    for op in &mut body.instructions {
        if let Instruction::MutatePlayer(spec) = op
            && let spellwake_engine::toolbox::PlayerMutation::RandomEvent(
                spellwake_engine::toolbox::RandomEventOp::RollDie {
                    bands: target_bands,
                    ..
                },
            ) = &mut spec.op
            && target_bands.is_empty()
        {
            *target_bands = bands.to_vec();
            attached = true;
        }
    }
    attached
}

fn attach_die_roll_bands(
    program: &mut Program,
    bands: &[spellwake_engine::program::DieRollBand],
) -> bool {
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

#[allow(clippy::too_many_arguments)]
fn lower_gated_section(
    face_ast: &FaceAst,
    keywords: &[KeywordAst],
    statics: &[StaticAbilityAst],
    abilities: &[AbilityAst],
    replacements: &[ReplacementEffectAst],
    mode: GatedSectionMode,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if keywords.is_empty() && statics.is_empty() && abilities.is_empty() && replacements.is_empty()
    {
        return Ok(());
    }
    let mut scratch = Program::new(
        program.card_types.clone(),
        program.creature_subtypes.clone(),
        program.land_subtypes.clone(),
        program.kind.clone(),
    );
    scratch.artifact_subtypes = program.artifact_subtypes.clone();
    scratch.enchantment_subtypes = program.enchantment_subtypes.clone();
    scratch.planeswalker_subtypes = program.planeswalker_subtypes.clone();
    scratch.supertypes = program.supertypes.clone();
    let baseline_kind = scratch.kind.clone();
    for (idx, kw) in keywords.iter().enumerate() {
        lower_keyword(kw, idx, face_ast, &mut scratch, vocabulary, ids, false)?;
    }
    for rep in replacements {
        lower_replacement(rep, &mut scratch, vocabulary, ids)?;
    }
    for st in statics {
        lower_static(st, &mut scratch, vocabulary, ids, false)?;
    }
    for ab in abilities {
        lower_ability(ab, &mut scratch, vocabulary, ids)?;
    }
    if scratch.kind != baseline_kind
        || scratch.entry_copy.is_some()
        || scratch.entry_tap_unless.is_some()
        || !scratch.zone_replacements.is_empty()
        || !scratch.cast_reductions.is_empty()
        || !scratch.entry_counter_abilities.is_empty()
        || scratch.cannot_be_countered
        || scratch.cannot_be_copied
        || scratch.conditional_flash.is_some()
        || scratch.attack_requirement.is_some()
        || scratch.combat_limits != Default::default()
        || scratch.conditional_cost.is_some()
        || !scratch.additional_cast_costs.is_empty()
        || !scratch.extra_cast_costs.is_empty()
    {
        return Err("oracle:unsupported-gated-section-feature");
    }
    let static_cond = match &mode {
        GatedSectionMode::LevelerBand(c) => c.clone(),
        GatedSectionMode::ClassLevel(lvl) => StaticSourceCondition::SourceClassLevelAtLeast(*lvl),
        GatedSectionMode::CaseSolved => StaticSourceCondition::SourceIsSolved,
        GatedSectionMode::EntryOption(idx) => StaticSourceCondition::EntryOptionChosen(*idx),
    };
    if !scratch.printed_keywords.is_empty() {
        push_static_ability(
            program,
            StaticContinuousAbility::Battlefield {
                id: ids.static_id(),
                selector: PermanentSelector {
                    controller: ControllerSelector::SourceItself,
                    source_condition: Some(static_cond.clone()),
                    ..PermanentSelector::default()
                },
                parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
                    granted_keywords: scratch.printed_keywords,
                    ..AbilityChange::default()
                })]),
            },
        )?;
    }
    for st in scratch
        .continuous
        .into_iter()
        .chain(scratch.additional_battlefield_abilities)
    {
        let st_id = st.id();
        push_static_ability(program, st)?;
        let merged = match scratch.static_conditions.remove(&st_id) {
            Some(existing) => {
                StaticSourceCondition::And(Box::new(static_cond.clone()), Box::new(existing))
            }
            None => static_cond.clone(),
        };
        program.static_conditions.insert(st_id, merged);
    }
    match &mode {
        GatedSectionMode::LevelerBand(band_cond) => {
            for tr in scratch.triggered {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            source_condition: Some(band_cond.clone()),
                            ..PermanentSelector::default()
                        },
                        parts: StaticParts(vec![StaticPart::GrantTrigger(Box::new(tr))]),
                    },
                )?;
            }
            for mut ab in scratch.activated {
                if scratch.activation_modes.contains_key(&ab.id)
                    || scratch.activation_hybrid_symbols.contains_key(&ab.id)
                    || scratch.extra_activation_costs.contains_key(&ab.id)
                    || scratch.activation_cost_reductions.contains_key(&ab.id)
                    || scratch
                        .activation_target_cost_reductions
                        .contains_key(&ab.id)
                    || scratch.loyalty_costs.contains_key(&ab.id)
                {
                    return Err("oracle:leveler-complex-activated");
                }
                let restriction = scratch.activation_restrictions.remove(&ab.id);
                ab.id = ActivatedAbilityId(0);
                let part = match restriction {
                    Some(restriction) => StaticPart::GrantActivatedRestricted {
                        ability: Box::new(ab),
                        restriction: Box::new(restriction),
                    },
                    None => StaticPart::GrantActivated(Box::new(ab)),
                };
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            source_condition: Some(band_cond.clone()),
                            ..PermanentSelector::default()
                        },
                        parts: StaticParts(vec![part]),
                    },
                )?;
            }
            for ma in scratch.mana {
                let part = match ma.output {
                    ManaOutput::Fixed(bundle) if bundle.len() == 1 => {
                        let (&color, qty) =
                            bundle.iter().next().ok_or("oracle:leveler-complex-mana")?;
                        if *qty != 1.into() {
                            return Err("oracle:leveler-complex-mana");
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
                    _ => return Err("oracle:leveler-complex-mana"),
                };
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            source_condition: Some(band_cond.clone()),
                            ..PermanentSelector::default()
                        },
                        parts: StaticParts(vec![part]),
                    },
                )?;
            }
        }
        GatedSectionMode::ClassLevel(_)
        | GatedSectionMode::CaseSolved
        | GatedSectionMode::EntryOption(_) => {
            if !scratch.mana.is_empty() {
                return Err("oracle:gated-section-mana");
            }
            let (trig_cond, act_res) = match mode {
                GatedSectionMode::ClassLevel(lvl) => (
                    TriggerCondition::SourceClassLevelAtLeast(lvl),
                    ActivationRestriction::MinClassLevel(lvl),
                ),
                GatedSectionMode::CaseSolved => (
                    TriggerCondition::SourceIsSolved,
                    ActivationRestriction::SourceIsSolved,
                ),
                GatedSectionMode::EntryOption(idx) => (
                    TriggerCondition::SourceEntryOption(idx),
                    ActivationRestriction::SourceEntryOption(idx),
                ),
                GatedSectionMode::LevelerBand(_) => unreachable!(),
            };
            for mut tr in scratch.triggered {
                let self_transition = matches!(
                    tr.condition.event,
                    TriggerEvent::StatusChanged {
                        kind: StatusTriggerKind::ClassLevelReached(_)
                            | StatusTriggerKind::CaseSolved,
                        ..
                    }
                );
                if !self_transition {
                    tr.condition.intervening_if = Some(match tr.condition.intervening_if.take() {
                        Some(existing) => {
                            TriggerCondition::And(Box::new(trig_cond.clone()), Box::new(existing))
                        }
                        None => trig_cond.clone(),
                    });
                }
                program.triggered.push(tr);
            }
            for ab in scratch.activated {
                let ab_id = ab.id;
                if let Some(m) = scratch.activation_modes.remove(&ab_id) {
                    program.activation_modes.insert(ab_id, m);
                }
                if let Some(h) = scratch.activation_hybrid_symbols.remove(&ab_id) {
                    program.activation_hybrid_symbols.insert(ab_id, h);
                }
                if let Some(e) = scratch.extra_activation_costs.remove(&ab_id) {
                    program.extra_activation_costs.insert(ab_id, e);
                }
                if let Some(r) = scratch.activation_cost_reductions.remove(&ab_id) {
                    program.activation_cost_reductions.insert(ab_id, r);
                }
                if let Some(tr) = scratch.activation_target_cost_reductions.remove(&ab_id) {
                    program.activation_target_cost_reductions.insert(ab_id, tr);
                }
                let merged_res = combine_activation_restrictions(
                    scratch.activation_restrictions.remove(&ab_id),
                    act_res.clone(),
                );
                program.activation_restrictions.insert(ab_id, merged_res);
                program.activated.push(ab);
            }
        }
    }
    Ok(())
}

fn find_aura_restriction(
    statics: &[StaticAbilityAst],
    vocabulary: &CreatureTypeVocabulary,
) -> Result<EnchantRestriction, &'static str> {
    let mut found = None;
    for st in statics {
        if let StaticAbilityAst::AttachmentRestriction {
            filter,
            target_player,
        } = st
        {
            if found.is_some() {
                return Err("oracle:duplicate-enchant");
            }
            if let Some(tp) = target_player {
                if tp.eq_ignore_ascii_case("player") {
                    found = Some(EnchantRestriction::Player);
                } else {
                    return Err("oracle:unsupported-enchant-player");
                }
            } else if let Some(f) = filter {
                found = Some(lower_enchant_restriction(f, vocabulary)?);
            } else {
                return Err("oracle:empty-enchant");
            }
        }
    }
    found.ok_or("oracle:aura-missing-enchant")
}

fn lower_enchant_restriction(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<EnchantRestriction, &'static str> {
    if f.zone.is_some() {
        return Err("oracle:complex-enchant-restriction");
    }
    let is_simple_shape = f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.color_arity.is_none()
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.counter_kind.is_none()
        && f.flags.is_empty();
    if is_simple_shape {
        if f.any_permanent
            && f.forbidden_types.is_empty()
            && f.forbidden_colors.is_empty()
            && f.controller.is_none()
        {
            return Ok(EnchantRestriction::Permanent);
        }
        if f.any_types == AstCardTypeMask::CREATURE && f.forbidden_types.is_empty() {
            if f.forbidden_colors == AstColorMask::BLACK && f.controller.is_none() {
                return Ok(EnchantRestriction::NonblackCreature);
            }
            if f.forbidden_colors.is_empty() {
                match f.controller {
                    None => return Ok(EnchantRestriction::Creature),
                    Some(ControllerRelation::You) => {
                        return Ok(EnchantRestriction::CreatureYouControl);
                    }
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                        return Ok(EnchantRestriction::CreatureAnOpponentControls);
                    }
                    _ => {}
                }
            }
        }
        if f.forbidden_colors.is_empty() && f.forbidden_types.is_empty() && f.controller.is_none() {
            if f.any_types == AstCardTypeMask::ARTIFACT {
                return Ok(EnchantRestriction::Artifact);
            }
            if f.any_types == AstCardTypeMask::ENCHANTMENT {
                return Ok(EnchantRestriction::Enchantment);
            }
            if f.any_types == AstCardTypeMask::LAND {
                return Ok(EnchantRestriction::Land);
            }
        }
    }
    let obj =
        lower_object_filter(f, vocabulary).map_err(|_| "oracle:complex-enchant-restriction")?;
    Ok(EnchantRestriction::Matching(obj))
}

fn push_static_ability(
    program: &mut Program,
    ability: StaticContinuousAbility,
) -> Result<(), &'static str> {
    if program.continuous.is_none() {
        program.continuous = Some(ability);
        Ok(())
    } else if ability.allowed_as_additional() {
        program.additional_battlefield_abilities.push(ability);
        Ok(())
    } else if let Some(existing) = program.continuous.take() {
        if existing.allowed_as_additional() {
            program.continuous = Some(ability);
            program.additional_battlefield_abilities.push(existing);
            Ok(())
        } else {
            program.continuous = Some(existing);
            Err("oracle:duplicate-primary-static")
        }
    } else {
        program.continuous = Some(ability);
        Ok(())
    }
}

fn push_primary_static_ability(
    program: &mut Program,
    ability: StaticContinuousAbility,
) -> Result<(), &'static str> {
    if program.continuous.is_none() {
        program.continuous = Some(ability);
        Ok(())
    } else if let Some(existing) = program.continuous.take() {
        if existing.allowed_as_additional() {
            program.continuous = Some(ability);
            program.additional_battlefield_abilities.push(existing);
            Ok(())
        } else {
            program.continuous = Some(existing);
            Err("oracle:duplicate-primary-static")
        }
    } else {
        program.continuous = Some(ability);
        Ok(())
    }
}

fn is_self_flash_filter(filter: &ObjectFilterAst) -> bool {
    !filter.any_spell && !filter.any_permanent && is_unrestricted_filter(filter)
}

fn is_hoistable_self_flash_static(st: &StaticAbilityAst) -> bool {
    matches!(
        st,
        StaticAbilityAst::PlayOrCastPermission {
            cast_only: true,
            filter,
            permission,
            condition: Some(_),
        } if permission
            .trim()
            .trim_end_matches('.')
            .eq_ignore_ascii_case("as though it had flash")
            && is_self_flash_filter(filter)
    )
}

fn is_self_conditional_flash_keyword_grant(st: &StaticAbilityAst) -> bool {
    matches!(
        st,
        StaticAbilityAst::GrantKeywords {
            subject,
            keywords,
            condition: Some(_),
            paired_rule: None,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            && keywords.len() == 1
            && keywords[0].trim().eq_ignore_ascii_case("flash")
    )
}

fn value_expr_needs_cast_source(expr: &ValueExpr) -> bool {
    if expr.references_ability_source() {
        return true;
    }
    let bad_player = |p: ResolvingPlayer| {
        matches!(
            p,
            ResolvingPlayer::EnchantedPlayer
                | ResolvingPlayer::AttachedHostController
                | ResolvingPlayer::SourceChosenPlayer
        )
    };
    match expr {
        ValueExpr::CountOtherSharingSourceSubtype
        | ValueExpr::AttachedCount(_)
        | ValueExpr::EventValue(
            EventValueKind::SourceAttackedThisTurn
            | EventValueKind::ManaSpentToCast
            | EventValueKind::ColorsSpentToCast
            | EventValueKind::ManaTypeSpentToCast(_)
            | EventValueKind::SnowManaSpentToCast
            | EventValueKind::TreasureManaSpentToCast
            | EventValueKind::CreatureManaSpentToCast
            | EventValueKind::CastFromOutsideHand
            | EventValueKind::CastFromExile
            | EventValueKind::CastFromGraveyard
            | EventValueKind::CastDuringOwnMain
            | EventValueKind::Bargained
            | EventValueKind::GiftPromised
            | EventValueKind::Cleaved
            | EventValueKind::EvidenceCollected
            | EventValueKind::PaidAlternative
            | EventValueKind::KickerCount(_)
            | EventValueKind::AdditionalCostPaid,
        ) => true,
        ValueExpr::Count {
            zone,
            controller_scope,
            ..
        } => {
            let bad_zone = match *zone {
                ValueZone::Graveyard(p)
                | ValueZone::Hand(p)
                | ValueZone::Library(p)
                | ValueZone::Exile(Some(p)) => bad_player(p),
                ValueZone::LinkedExile => true,
                _ => false,
            };
            bad_zone || controller_scope.is_some_and(bad_player)
        }
        ValueExpr::DistinctManaValuesInGraveyard(p)
        | ValueExpr::PlayerStat(
            PlayerStatExpr::PlayerIsActivePlayer(p)
            | PlayerStatExpr::PlayerLife(p)
            | PlayerStatExpr::PlayerHandSize(p)
            | PlayerStatExpr::PlayerLibrarySize(p)
            | PlayerStatExpr::PlayerGraveyardSize(p)
            | PlayerStatExpr::LandsEnteredThisTurn(p),
        ) => bad_player(*p),
        ValueExpr::ObjectStat { subject, .. } => matches!(
            subject,
            ValueObjectRef::AbilitySource
                | ValueObjectRef::TriggerSource
                | ValueObjectRef::AttachedHost
        ),
        ValueExpr::CounterCount { subject, .. } => matches!(
            subject,
            spellwake_engine::program::CounterSubjectRef::AbilitySource
                | spellwake_engine::program::CounterSubjectRef::TriggerSource
                | spellwake_engine::program::CounterSubjectRef::AttachedHost
        ),
        ValueExpr::Scale { expr, .. } | ValueExpr::DivConst { expr, .. } => {
            value_expr_needs_cast_source(expr)
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            value_expr_needs_cast_source(a) || value_expr_needs_cast_source(b)
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            value_expr_needs_cast_source(left)
                || value_expr_needs_cast_source(right)
                || value_expr_needs_cast_source(then_value)
                || value_expr_needs_cast_source(else_value)
        }
        _ => false,
    }
}

fn lower_self_flash_condition(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SelfFlashPermission, &'static str> {
    if let ConditionAst::Marker(GameMarker::PayCostCondition(cost_atom)) = cond {
        if let CostAtomAst::PayMana(raw) = cost_atom.as_ref()
            && let Some(extra_cost) = parse_fixed_mana_cost(&raw.trim().to_ascii_uppercase())
            && extra_cost.total().is_positive()
        {
            return Ok(SelfFlashPermission::PayExtraMana(extra_cost));
        }
        return Err("oracle:unsupported-conditional-flash");
    }
    let mut tmp = BodyBuilder::new();
    let rc = lower_resolution_condition(cond, &mut tmp, vocabulary, false, None)
        .map_err(|_| "oracle:unsupported-conditional-flash")?;
    if tmp.target != BodyTarget::None
        || !rc.is_world_evaluable()
        || rc.validate_world(vocabulary).is_err()
        || value_expr_needs_cast_source(&rc.left)
        || value_expr_needs_cast_source(&rc.right)
    {
        return Err("oracle:unsupported-conditional-flash");
    }
    Ok(SelfFlashPermission::Condition(Box::new(rc)))
}

fn strip_snow_mana_symbols(raw: &str) -> Option<(String, u8)> {
    let trimmed = raw.trim();
    let symbols = trimmed.strip_prefix('{')?.strip_suffix('}')?;
    let mut out = String::with_capacity(trimmed.len());
    let mut snow_count: u8 = 0;
    for sym in symbols.split("}{") {
        if sym.eq_ignore_ascii_case("S") {
            snow_count = snow_count.checked_add(1)?;
        } else {
            out.push('{');
            out.push_str(sym);
            out.push('}');
        }
    }
    Some((out, snow_count))
}

fn replacement_has_temporary_enters_as_copy(rep: &ReplacementEffectAst) -> bool {
    match rep {
        ReplacementEffectAst::EntersAsCopy { exception, .. } => exception
            .as_deref()
            .is_some_and(|e| e.to_ascii_lowercase().contains("until end of turn")),
        ReplacementEffectAst::EntersAsCopyModified(m) => {
            m.duration != DurationAst::Permanent
                || m.exception
                    .as_deref()
                    .is_some_and(|e| e.to_ascii_lowercase().contains("until end of turn"))
        }
        ReplacementEffectAst::Compound(items) => {
            items.iter().any(replacement_has_temporary_enters_as_copy)
        }
        ReplacementEffectAst::ScaledBy { inner, .. } => {
            replacement_has_temporary_enters_as_copy(inner)
        }
        _ => false,
    }
}

fn text_mentions_attachment(raw: &str) -> bool {
    let low = raw.to_ascii_lowercase();
    low.contains("attached") || low.contains("equipped") || low.contains("enchanted creature")
}

fn as_enters_action_mentions_attachment(ins: &[InstructionAst]) -> bool {
    ins.iter().any(|item| {
        if matches!(item, InstructionAst::Attach { .. }) {
            return true;
        }
        let debug_repr = format!("{item:?}");
        let low = debug_repr.to_ascii_lowercase();
        low.contains("attachedhost")
            || low.contains("attached_to_self")
            || low.contains("attachedto(")
            || low.contains("attachedby(")
            || text_mentions_attachment(&low)
    })
}

fn is_permanent_siege_duration(dur: &DurationAst) -> bool {
    matches!(
        dur,
        DurationAst::Permanent | DurationAst::UntilSourceLeaves | DurationAst::Instant
    )
}

fn lower_as_enters_modal_body(
    face_ast: &FaceAst,
    mode: &spellwake_oracle_compiler::ast::ModalModeAst,
    idx_u8: u8,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if mode.body.is_empty() {
        return Err("oracle:unsupported-replacement");
    }
    let delayed_step_trigger =
        |step: TurnStepAst, scope: DelayedStepScope, body: Vec<InstructionAst>| {
            let whose = match scope {
                DelayedStepScope::YourNext | DelayedStepScope::EachOfYour => "you",
                DelayedStepScope::EachOpponents => "each opponent",
                DelayedStepScope::EachPlayers | DelayedStepScope::Each | DelayedStepScope::Next => {
                    "each player"
                }
                _ => return Err("oracle:unsupported-replacement"),
            };
            Ok(AbilityAst::Triggered {
                ability_word: mode.label.clone(),
                trigger: TriggerSpecAst {
                    events: vec![TriggerEventAst::StepBegan {
                        step,
                        whose: whose.to_string(),
                    }],
                    intervening_if: None,
                },
                body,
            })
        };

    match mode.body.as_slice() {
        [
            InstructionAst::DelayedTrigger {
                step,
                scope,
                body: trig_body,
            },
            rest @ ..,
        ] => {
            let mut full_body = trig_body.clone();
            full_body.extend_from_slice(rest);
            let ab = delayed_step_trigger(*step, *scope, full_body)?;
            return lower_gated_section(
                face_ast,
                &[],
                &[],
                &[ab],
                &[],
                GatedSectionMode::EntryOption(idx_u8),
                program,
                vocabulary,
                ids,
            );
        }
        [
            InstructionAst::DelayedEventTrigger {
                trigger,
                body: trig_body,
            },
            rest @ ..,
        ] => {
            let mut full_body = trig_body.clone();
            full_body.extend_from_slice(rest);
            let ab = AbilityAst::Triggered {
                ability_word: mode.label.clone(),
                trigger: trigger.clone(),
                body: full_body,
            };
            return lower_gated_section(
                face_ast,
                &[],
                &[],
                &[ab],
                &[],
                GatedSectionMode::EntryOption(idx_u8),
                program,
                vocabulary,
                ids,
            );
        }
        [
            InstructionAst::TemporaryTrigger {
                trigger,
                body: trig_body,
                duration,
            },
            rest @ ..,
        ] if is_permanent_siege_duration(duration) => {
            let mut full_body = trig_body.clone();
            full_body.extend_from_slice(rest);
            let ab = AbilityAst::Triggered {
                ability_word: mode.label.clone(),
                trigger: trigger.clone(),
                body: full_body,
            };
            return lower_gated_section(
                face_ast,
                &[],
                &[],
                &[ab],
                &[],
                GatedSectionMode::EntryOption(idx_u8),
                program,
                vocabulary,
                ids,
            );
        }
        _ => {}
    }

    let mut keywords: Vec<KeywordAst> = Vec::new();
    let mut statics: Vec<StaticAbilityAst> = Vec::new();
    let mut abilities: Vec<AbilityAst> = Vec::new();
    let mut replacements: Vec<ReplacementEffectAst> = Vec::new();

    for item in &mode.body {
        match item {
            InstructionAst::TemporaryStatic {
                static_ability,
                duration,
            } if is_permanent_siege_duration(duration) => {
                statics.push(static_ability.as_ref().clone());
            }
            InstructionAst::TemporaryTrigger {
                trigger,
                body: trig_body,
                duration,
            } if is_permanent_siege_duration(duration) => {
                abilities.push(AbilityAst::Triggered {
                    ability_word: mode.label.clone(),
                    trigger: trigger.clone(),
                    body: trig_body.clone(),
                });
            }
            InstructionAst::DelayedTrigger {
                step,
                scope,
                body: trig_body,
            } => {
                abilities.push(delayed_step_trigger(*step, *scope, trig_body.clone())?);
            }
            InstructionAst::DelayedEventTrigger {
                trigger,
                body: trig_body,
            } => {
                abilities.push(AbilityAst::Triggered {
                    ability_word: mode.label.clone(),
                    trigger: trigger.clone(),
                    body: trig_body.clone(),
                });
            }
            InstructionAst::TemporaryReplacement {
                replacement,
                duration,
            } if is_permanent_siege_duration(duration) => {
                replacements.push(replacement.as_ref().clone());
            }
            InstructionAst::GrantKeywords {
                subject,
                keywords: kws,
                duration,
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && is_permanent_siege_duration(duration)
                && !kws.is_empty() =>
            {
                statics.push(StaticAbilityAst::GrantKeywords {
                    subject: subject.clone(),
                    keywords: kws.clone(),
                    condition: None,
                    paired_rule: None,
                });
            }
            InstructionAst::GrantAbilityText {
                subject,
                compiled: Some(gf),
                duration,
                ..
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && is_permanent_siege_duration(duration)
                && gf.spell_effects.is_empty() =>
            {
                keywords.extend(gf.keywords.clone());
                statics.extend(gf.statics.clone());
                abilities.extend(gf.abilities.clone());
                replacements.extend(gf.replacements.clone());
            }
            _ => return Err("oracle:unsupported-replacement"),
        }
    }

    if keywords.is_empty() && statics.is_empty() && abilities.is_empty() && replacements.is_empty()
    {
        return Err("oracle:unsupported-replacement");
    }

    lower_gated_section(
        face_ast,
        &keywords,
        &statics,
        &abilities,
        &replacements,
        GatedSectionMode::EntryOption(idx_u8),
        program,
        vocabulary,
        ids,
    )
}

fn push_cast_timing_flat(acc: &mut Vec<CastTimingRestriction>, timing: CastTimingRestriction) {
    match timing {
        CastTimingRestriction::All(items) => {
            for item in items {
                push_cast_timing_flat(acc, item);
            }
        }
        other => acc.push(other),
    }
}

fn is_x_nonzero_condition_ast(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Compare { lhs, op, rhs } => {
            matches!(
                (lhs.as_ref(), *op, rhs.as_ref()),
                (ValueExprAst::ChosenX, CompareOp::Gt, ValueExprAst::Const(0))
                    | (ValueExprAst::ChosenX, CompareOp::Ge, ValueExprAst::Const(1))
                    | (ValueExprAst::Const(0), CompareOp::Lt, ValueExprAst::ChosenX)
                    | (ValueExprAst::Const(1), CompareOp::Le, ValueExprAst::ChosenX)
            )
        }
        ConditionAst::Not(inner) => matches!(
            inner.as_ref(),
            ConditionAst::Compare {
                lhs,
                op: CompareOp::Eq,
                rhs,
            } if matches!(
                (lhs.as_ref(), rhs.as_ref()),
                (ValueExprAst::ChosenX, ValueExprAst::Const(0))
                    | (ValueExprAst::Const(0), ValueExprAst::ChosenX)
            )
        ),
        _ => false,
    }
}

fn split_spell_restriction_clauses(low: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in low.split(['+', ';']) {
        for sent in seg.split(". ") {
            let trimmed = sent.trim().trim_end_matches('.').trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut cursor = trimmed;
            loop {
                let mut found: Option<(&str, &str)> = None;
                for sep in [", and only ", " and only ", ", but only ", " but only "] {
                    if let Some((lhs, rhs)) = cursor.split_once(sep) {
                        found = Some((lhs, rhs));
                        break;
                    }
                }
                if let Some((lhs, rhs)) = found {
                    let lhs_t = lhs.trim();
                    if !lhs_t.is_empty() {
                        parts.push(lhs_t);
                    }
                    cursor = rhs.trim();
                } else {
                    if !cursor.is_empty() {
                        parts.push(cursor);
                    }
                    break;
                }
            }
        }
    }
    parts
}

fn parse_spell_cast_timing_restriction(
    restriction: &str,
    condition: Option<&ConditionAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<CastTimingRestriction> {
    use CastTimingRestriction as R;

    let low = restriction
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();

    if let Some(cond) = condition {
        let cond_r = if is_x_nonzero_condition_ast(cond) {
            R::XNonZero
        } else {
            let mut dummy = BodyBuilder::new();
            let rc = lower_resolution_condition(cond, &mut dummy, vocabulary, false, None).ok()?;
            if dummy.target != BodyTarget::None
                || !rc.is_world_evaluable()
                || rc.validate_world(vocabulary).is_err()
            {
                return None;
            }
            R::Condition(Box::new(rc))
        };
        let base_tag = low
            .strip_prefix("conditional+")
            .or_else(|| low.strip_prefix("conditional"))
            .unwrap_or(&low)
            .trim();
        if base_tag.is_empty() || matches!(base_tag, "condition" | "only_if" | "if") {
            return Some(cond_r);
        }
        let timing = parse_spell_cast_timing_restriction(base_tag, None, vocabulary)?;
        let mut flat = Vec::new();
        push_cast_timing_flat(&mut flat, timing);
        push_cast_timing_flat(&mut flat, cond_r);
        return match flat.len() {
            0 => None,
            1 => flat.pop(),
            _ => Some(R::All(flat)),
        };
    }

    if low == "during combat+during opponent's turn"
        || low == "during opponent's turn+during combat"
    {
        return Some(R::DuringCombatOnOpponentsTurn);
    }
    if (low.contains("declare attackers step") && low.contains("attacked"))
        || low == "duringdeclareattackersifattacked"
    {
        return Some(R::DuringDeclareAttackersIfAttacked);
    }

    let compound = split_spell_restriction_clauses(&low);
    if compound.len() >= 2 {
        let mut parts = Vec::with_capacity(compound.len());
        for part in compound {
            let parsed = parse_spell_cast_timing_restriction(part, None, vocabulary)?;
            push_cast_timing_flat(&mut parts, parsed);
        }
        return match parts.len() {
            0 => None,
            1 => parts.pop(),
            _ => Some(R::All(parts)),
        };
    }

    let core = low
        .strip_prefix("cast this spell ")
        .or_else(|| low.strip_prefix("cast ~ "))
        .unwrap_or(&low)
        .trim();
    let core = core.strip_prefix("only ").unwrap_or(core).trim();

    if matches!(core, "during combat" | "duringcombat") {
        return Some(R::DuringCombat);
    }

    if matches!(
        core,
        "during combat on an opponent's turn"
            | "during an opponent's combat"
            | "during an opponent's combat phase"
    ) {
        return Some(R::DuringCombatOnOpponentsTurn);
    }

    if matches!(
        core,
        "xcantbezero"
            | "xnonzero"
            | "x can't be 0"
            | "x can't be zero"
            | "x is not 0"
            | "x isn't 0"
            | "x is not zero"
            | "x isn't zero"
            | "if x is not 0"
            | "if x isn't 0"
            | "if x is not zero"
            | "if x isn't zero"
            | "if x is greater than 0"
            | "if x is 1 or more"
    ) {
        return Some(R::XNonZero);
    }

    if matches!(
        core,
        "after combat" | "aftercombat" | "after the combat phase"
    ) {
        return Some(R::AfterCombat);
    }

    if matches!(
        core,
        "after combat on your turn"
            | "during your turn after combat"
            | "during your turn, after combat"
            | "after combat during your turn"
    ) {
        return Some(R::All(vec![R::DuringYourTurn, R::AfterCombat]));
    }

    if matches!(
        core,
        "after combat on an opponent's turn"
            | "during an opponent's turn after combat"
            | "during an opponent's turn, after combat"
            | "after combat during an opponent's turn"
    ) {
        return Some(R::All(vec![R::DuringOpponentsTurn, R::AfterCombat]));
    }

    if core == "during opponent's turn" {
        return Some(R::DuringOpponentsTurn);
    }

    if let Some(cond_raw) = core.strip_prefix("if ")
        && let Ok(cond_ast) = spellwake_oracle_compiler::filter::parse_condition(
            cond_raw.trim(),
            &cached_compiler_vocab(vocabulary),
        )
    {
        if is_x_nonzero_condition_ast(&cond_ast) {
            return Some(R::XNonZero);
        }
        let mut dummy = BodyBuilder::new();
        if let Ok(rc) = lower_resolution_condition(&cond_ast, &mut dummy, vocabulary, false, None)
            && dummy.target == BodyTarget::None
            && rc.is_world_evaluable()
            && rc.validate_world(vocabulary).is_ok()
        {
            return Some(R::Condition(Box::new(rc)));
        }
    }

    parse_cast_timing_restriction(restriction, None, vocabulary)
}

pub(super) fn is_sorcery_timing_restriction(restriction: &str) -> bool {
    // Only match trailing cast-permission sorcery qualifiers, never ability/spell-level
    // "Activate only as a sorcery" (emitted as "Sorcery") or "Cast this spell only as a sorcery".
    let low = restriction
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
    matches!(
        low.as_str(),
        "as a sorcery"
            | "only as a sorcery"
            | "cast only as a sorcery"
            | "play only as a sorcery"
            | "cast it only as a sorcery"
            | "play it only as a sorcery"
            | "any time you could cast a sorcery"
            | "only any time you could cast a sorcery"
    )
}

pub(super) fn is_exile_or_cast_perm_instr(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::Exile { .. }
        | InstructionAst::ExileUntilLeaves { .. }
        | InstructionAst::ExileTopLibrary { .. }
        | InstructionAst::ExileGraveyard { .. }
        | InstructionAst::ExileWithCounters { .. }
        | InstructionAst::ExileOrRevealTopUntil { .. }
        | InstructionAst::ExileAllNamedFromZones { .. }
        | InstructionAst::GrantCastPermission { .. }
        | InstructionAst::GrantModifiedCastPermission { .. } => true,
        InstructionAst::Optional(inner)
        | InstructionAst::OptionalPlayer { body: inner, .. }
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::IfCondition {
            then_body: inner, ..
        }
        | InstructionAst::Otherwise(inner)
        | InstructionAst::ReplaceEffect(inner)
        | InstructionAst::ReflexiveTrigger(inner)
        | InstructionAst::Simultaneous(inner)
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::BindXY { body: inner, .. }
        | InstructionAst::ScopedDuration { body: inner, .. }
        | InstructionAst::ForEachPlayer { body: inner, .. }
        | InstructionAst::ForEachObject { body: inner, .. }
        | InstructionAst::ForEachCount { body: inner, .. }
        | InstructionAst::ForEachSubjectPlayer { body: inner, .. }
        | InstructionAst::ForAsLongAs { body: inner, .. }
        | InstructionAst::DelayedTrigger { body: inner, .. }
        | InstructionAst::DelayedEventTrigger { body: inner, .. }
        | InstructionAst::TemporaryTrigger { body: inner, .. }
        | InstructionAst::ReflexiveEventTrigger { body: inner, .. }
        | InstructionAst::WhenYouNext { body: inner, .. } => {
            inner.iter().any(is_exile_or_cast_perm_instr)
        }
        InstructionAst::ReplaceEffectWith(rew) => rew.body.iter().any(is_exile_or_cast_perm_instr),
        InstructionAst::OneShotReplacement {
            instead_body: Some(inner),
            ..
        } => inner.iter().any(is_exile_or_cast_perm_instr),
        InstructionAst::SearchLibrary { follow_up, .. } => {
            follow_up.iter().any(is_exile_or_cast_perm_instr)
        }
        InstructionAst::Counter {
            unless_action: Some(inner),
            ..
        } => inner.iter().any(is_exile_or_cast_perm_instr),
        InstructionAst::Choice(branches) => branches
            .iter()
            .any(|br| br.iter().any(is_exile_or_cast_perm_instr)),
        _ => false,
    }
}

fn strip_spell_cast_restrictions(
    ins: &[InstructionAst],
    cast_during_own_combat: &mut bool,
    cast_timings: &mut Vec<CastTimingRestriction>,
    vocabulary: &CreatureTypeVocabulary,
) -> Vec<InstructionAst> {
    strip_spell_cast_restrictions_inner(
        ins,
        cast_during_own_combat,
        cast_timings,
        vocabulary,
        false,
    )
}

fn strip_spell_cast_restrictions_inner(
    ins: &[InstructionAst],
    cast_during_own_combat: &mut bool,
    cast_timings: &mut Vec<CastTimingRestriction>,
    vocabulary: &CreatureTypeVocabulary,
    mut after_exile_or_cast_perm: bool,
) -> Vec<InstructionAst> {
    let mut filtered = Vec::with_capacity(ins.len());
    for item in ins {
        match item {
            InstructionAst::AbilityRestriction {
                restriction,
                condition,
            } => {
                if after_exile_or_cast_perm
                    && condition.is_none()
                    && is_sorcery_timing_restriction(restriction)
                {
                    filtered.push(item.clone());
                    continue;
                }
                let low = restriction
                    .trim()
                    .trim_end_matches('.')
                    .trim()
                    .to_ascii_lowercase();
                if condition.is_none()
                    && matches!(
                        low.as_str(),
                        "during combat+during your turn"
                            | "during your turn+during combat"
                            | "during combat on your turn"
                            | "cast this spell only during combat on your turn"
                            | "cast ~ only during combat on your turn"
                    )
                {
                    *cast_during_own_combat = true;
                    continue;
                }
                if let Some(timing) = parse_spell_cast_timing_restriction(
                    restriction,
                    condition.as_deref(),
                    vocabulary,
                ) {
                    push_cast_timing_flat(cast_timings, timing);
                    continue;
                }
                filtered.push(item.clone());
            }
            InstructionAst::BindX { x_expr, body } if !body.is_empty() => {
                let inner = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::BindX {
                    x_expr: x_expr.clone(),
                    body: inner,
                });
            }
            InstructionAst::BindXY {
                x_expr,
                y_expr,
                body,
            } if !body.is_empty() => {
                let inner = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::BindXY {
                    x_expr: x_expr.clone(),
                    y_expr: y_expr.clone(),
                    body: inner,
                });
            }
            InstructionAst::ScopedDuration { duration, body } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                if !(inner_f.is_empty() && !body.is_empty()) {
                    filtered.push(InstructionAst::ScopedDuration {
                        duration: duration.clone(),
                        body: inner_f,
                    });
                }
            }
            InstructionAst::Optional(inner) => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    inner,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                if !(inner_f.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::Optional(inner_f));
                }
            }
            InstructionAst::OptionalPlayer { player, body } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                if !(inner_f.is_empty() && !body.is_empty()) {
                    filtered.push(InstructionAst::OptionalPlayer {
                        player: *player,
                        body: inner_f,
                    });
                }
            }
            InstructionAst::IfPreviousSucceeded(inner) => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    inner,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                if !(inner_f.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::IfPreviousSucceeded(inner_f));
                }
            }
            InstructionAst::IfPreviousFailed(inner) => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    inner,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                if !(inner_f.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::IfPreviousFailed(inner_f));
                }
            }
            InstructionAst::IfCondition {
                condition,
                then_body,
            } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    then_body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::IfCondition {
                    condition: condition.clone(),
                    then_body: inner_f,
                });
            }
            InstructionAst::Otherwise(inner) => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    inner,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                if !(inner_f.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::Otherwise(inner_f));
                }
            }
            InstructionAst::ReplaceEffect(inner) => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    inner,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                if !(inner_f.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::ReplaceEffect(inner_f));
                }
            }
            InstructionAst::ReplaceEffectWith(rew) => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    &rew.body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                if !(inner_f.is_empty() && !rew.body.is_empty()) {
                    let mut next = (**rew).clone();
                    next.body = inner_f;
                    filtered.push(InstructionAst::ReplaceEffectWith(Box::new(next)));
                }
            }
            InstructionAst::OneShotReplacement {
                subject,
                event,
                replace_with,
                instead_body,
            } => {
                let new_body = instead_body.as_ref().map(|inner| {
                    strip_spell_cast_restrictions_inner(
                        inner,
                        cast_during_own_combat,
                        cast_timings,
                        vocabulary,
                        after_exile_or_cast_perm,
                    )
                });
                filtered.push(InstructionAst::OneShotReplacement {
                    subject: subject.clone(),
                    event: event.clone(),
                    replace_with: replace_with.clone(),
                    instead_body: new_body,
                });
            }
            InstructionAst::SearchLibrary { follow_up, .. } if !follow_up.is_empty() => {
                let new_follow_up = strip_spell_cast_restrictions_inner(
                    follow_up,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                let mut cloned = item.clone();
                if let InstructionAst::SearchLibrary { follow_up: dst, .. } = &mut cloned {
                    *dst = new_follow_up;
                }
                filtered.push(cloned);
            }
            InstructionAst::Counter {
                target,
                unless_pay,
                unless_action: Some(act),
            } => {
                let new_act = strip_spell_cast_restrictions_inner(
                    act,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::Counter {
                    target: target.clone(),
                    unless_pay: unless_pay.clone(),
                    unless_action: Some(new_act),
                });
            }
            InstructionAst::ReflexiveTrigger(inner) => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    inner,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                if !(inner_f.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::ReflexiveTrigger(inner_f));
                }
            }
            InstructionAst::Choice(branches) => {
                let new_branches = branches
                    .iter()
                    .map(|br| {
                        strip_spell_cast_restrictions_inner(
                            br,
                            cast_during_own_combat,
                            cast_timings,
                            vocabulary,
                            after_exile_or_cast_perm,
                        )
                    })
                    .collect();
                filtered.push(InstructionAst::Choice(new_branches));
            }
            InstructionAst::DelayedTrigger { step, scope, body } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::DelayedTrigger {
                    step: *step,
                    scope: *scope,
                    body: inner_f,
                });
            }
            InstructionAst::DelayedEventTrigger { trigger, body } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::DelayedEventTrigger {
                    trigger: trigger.clone(),
                    body: inner_f,
                });
            }
            InstructionAst::TemporaryTrigger {
                trigger,
                body,
                duration,
            } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::TemporaryTrigger {
                    trigger: trigger.clone(),
                    body: inner_f,
                    duration: duration.clone(),
                });
            }
            InstructionAst::SpreeMode { cost, body } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::SpreeMode {
                    cost: cost.clone(),
                    body: inner_f,
                });
            }
            InstructionAst::ForEachPlayer { player, body } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::ForEachPlayer {
                    player: *player,
                    body: inner_f,
                });
            }
            InstructionAst::ForEachObject { filter, body } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::ForEachObject {
                    filter: filter.clone(),
                    body: inner_f,
                });
            }
            InstructionAst::ForEachCount { count, body } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::ForEachCount {
                    count: count.clone(),
                    body: inner_f,
                });
            }
            InstructionAst::ForEachSubjectPlayer { players, body } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::ForEachSubjectPlayer {
                    players: players.clone(),
                    body: inner_f,
                });
            }
            InstructionAst::ForAsLongAs { condition, body } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::ForAsLongAs {
                    condition: condition.clone(),
                    body: inner_f,
                });
            }
            InstructionAst::WhenYouNext {
                event,
                body,
                duration,
            } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                filtered.push(InstructionAst::WhenYouNext {
                    event: event.clone(),
                    body: inner_f,
                    duration: duration.clone(),
                });
            }
            InstructionAst::Simultaneous(inner) => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    inner,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                if !(inner_f.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::Simultaneous(inner_f));
                }
            }
            InstructionAst::Unless {
                body,
                unless_player,
                unless_cost,
                unless_action,
                unless_condition,
            } => {
                let inner_f = strip_spell_cast_restrictions_inner(
                    body,
                    cast_during_own_combat,
                    cast_timings,
                    vocabulary,
                    after_exile_or_cast_perm,
                );
                let new_action = unless_action.as_ref().map(|act| {
                    strip_spell_cast_restrictions_inner(
                        act,
                        cast_during_own_combat,
                        cast_timings,
                        vocabulary,
                        after_exile_or_cast_perm,
                    )
                });
                filtered.push(InstructionAst::Unless {
                    body: inner_f,
                    unless_player: *unless_player,
                    unless_cost: unless_cost.clone(),
                    unless_action: new_action,
                    unless_condition: unless_condition.clone(),
                });
            }
            InstructionAst::Modal { choice, modes } => {
                let new_modes = modes
                    .iter()
                    .map(|m| spellwake_oracle_compiler::ast::ModalModeAst {
                        body: strip_spell_cast_restrictions_inner(
                            &m.body,
                            cast_during_own_combat,
                            cast_timings,
                            vocabulary,
                            after_exile_or_cast_perm,
                        ),
                        label: m.label.clone(),
                        granted: m.granted.clone(),
                    })
                    .collect();
                filtered.push(InstructionAst::Modal {
                    choice: choice.clone(),
                    modes: new_modes,
                });
            }
            _ => filtered.push(item.clone()),
        }
        if is_exile_or_cast_perm_instr(item) {
            after_exile_or_cast_perm = true;
        }
    }
    filtered
}

#[cfg(test)]
mod wave12_die_roll_tests;

#[cfg(test)]
mod wave12_condition_scope_tests;

#[cfg(test)]
mod mega_batch_1_primitives_tests;
#[cfg(test)]
mod wave14_selector_filter_tests;

#[cfg(test)]
use super::vocabulary as creature_vocabulary;
#[cfg(test)]
#[path = "../../../../engine/test_support/characteristic_trajectories.rs"]
mod engine_trajectories;
