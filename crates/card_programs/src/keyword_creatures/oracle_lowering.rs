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
        ActivatedAbility, ActivatedAbilityId, ActivationRestriction, CounterCostDelta,
        DiscardCostMode, GraveyardExileCost, HandDiscardKeyword, LifeCostAmount, PowerAbility,
        SelectedSacrificeFilter, SourceCost, TapCostCount, TapCostFilter,
    },
    attachment::{AttachAbility, AttachmentKind, EnchantRestriction},
    characteristics::{
        AbilityChange, AbilityRemoval, BlockingRestriction, CastPermissionRule, Color, ColorFilter,
        ColorOperation, ControllerSelector, DefiningPart, DefiningParts, InstructionSubtypeOperand,
        KeywordAbility, LibraryTopPermission, NumericExpression, NumericPair, PermanentSelector,
        PowerToughness, PowerToughnessCounters, PrintedPowerToughness, PrintedStat,
        ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, SpellCostModifier, SpellDiscount,
        StaticAbilityId, StaticColorOperand, StaticContinuousAbility, StaticDamageRule,
        StaticDrawRule, StaticLandSubtypeOperand, StaticModifierRule, StaticPart, StaticParts,
        StaticPlayerStat, StaticRestriction, StaticSubtypeOperand, StaticTypeChange,
        StaticUntapRule, StaticZone, SubtypeOperation, SubtypeProtection, TypeChangeSpec,
        TypeOperation,
    },
    damage::TurnPreventionFilter,
    effects::CounterKind,
    face_down::FaceUpAbility,
    land::{BasicLandType, EntryTap, LandType},
    life::{LifeAdjustment, LifeChangeKind},
    mana::{
        HybridAlternative, HybridSymbol, ManaAbility, ManaAbilityId, ManaCost, ManaOutput,
        ManaOutputModifier, ManaRestriction, ManaSacrificeCost, ManaSideEffect, ManaType,
        SpecialColorYield,
    },
    program::{
        AdditionalCastCost, AffectedPlayers, ArtifactSubtype, AttackRequirement, AttackTriggerMode,
        BlockTriggerKind, BodyTarget, CardType, CardTypeMask, CastCopyKeyword, CastCostCount,
        CastEntryKind, CastReduction, ChosenHandAction, ColorMask, CombatFilter,
        CombatTriggerKeyword, CombatUntapGroup, Comparison, ConditionalCastCost, ControllerFilter,
        CounterKeyword, CounterTriggerKind, DamageGroup, DamageTriggerRecipient, DiesTriggerMode,
        EffectManaYield, EffectSubject, EnchantmentSubtype, EndStepCast, EntryCopyRule,
        EntryCounterAbility, EventValueKind, ExecutableBody, ExileAction, GrowthKeyword,
        Instruction, InterveningCondition, LibraryPlacement, LibrarySearch, ManaValueFilter,
        MatchObjectRef, MiracleCost, ObjectClause, ObjectColorFilter, ObjectFilter,
        ObjectFilterFlags, ObjectStatKind, PermanentDefinition, PermanentTargetAction,
        PermanentTypeFilter, PhasingSubject, PlanarTriggerEvent, PlaneswalkerSubtype,
        PlayerRelation, PlayerStatExpr, PlayerTriggerAction, PowerDamageSource, PowerFilter,
        Program, ProgramKind, ProtectionRecipients, PublicZoneDestination, ResolutionCondition,
        ResolvingPlayer, SearchCardMatcher, SearchCount, SearchDestination, SearchExileLink,
        SearchManaValue, SearchSource, SourceRelation, SpellDefinition, SpellFilter, SpellModes,
        SpellSubtype, SpliceAbility, SpliceQuality, SplitCard, StatAggregate, StateThresholdKind,
        StatusTriggerKind, SubtypeFilter, Supertype, SupertypeMask, ToughnessFilter,
        TransferKeyword, TriggerCondition, TriggerEvent, TriggerFromZone, TriggerGrouping,
        TriggerKeywordEvent, TriggerPlayerScope, TriggerSpec, TriggeredAbility, TriggeredAbilityId,
        TriggeredModes, UpkeepKeyword, ValueExpr, ValueObjectRef, ZoneActionPredicate,
        ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementSubject,
    },
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSize, TokenSubtype},
    turn::Step,
};
use spellwake_oracle_compiler::{
    CardInput, FaceInput, Vocabulary,
    ast::{
        AbilityAst, AggregateOp, CardTypeMask as AstCardTypeMask, ColorArity,
        ColorMask as AstColorMask, CombatState, CompareOp, ConditionAst, ControllerRelation,
        CostAtomAst, DurationAst, FaceAst, GameMarker, InstructionAst, KeywordAst, ManaSpecAst,
        ObjectFilterAst, ObjectFilterFlags as AstFilterFlags, PlayerRefAst, PlayerStatKind,
        ReplacementEffectAst, Rounding, StatKind, StaticAbilityAst, SubjectRefAst,
        SupertypeMask as AstSupertypeMask, TargetCountAst, TokenSpecAst, TriggerEventAst,
        TriggerSpecAst, TurnStepAst, ValueExprAst, ZoneFilter,
    },
    compile_card,
};

use super::InventoryCard;

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
/// with its source-authenticated back-face color indicator.
pub(super) fn compile_transforming(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
) -> Option<Program> {
    compile_transforming_with_reason(card, vocabulary, indicator).ok()
}

/// Strictly compile a single-face planeswalker via `spellwake-oracle-compiler`
/// with source-authenticated printed loyalty.
pub(super) fn compile_planeswalker(
    card: &InventoryCard,
    inventory: &super::Inventory,
    vocabulary: &CreatureTypeVocabulary,
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
    let type_line = face.type_line.as_deref()?;
    if !type_line.contains("Planeswalker") {
        return None;
    }
    let loyalty = inventory
        .source_loyalty(card.oracle_id.as_deref()?, 0)?
        .parse::<u16>()
        .ok()?;
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
        return None;
    }
    let [face_ast] = card_ast.faces.as_slice() else {
        return None;
    };
    let program = lower_face(
        card,
        face,
        face_ast,
        vocabulary,
        FaceLoweringOptions {
            allow_empty_mana_cost: false,
            allow_aftermath: false,
            printed_loyalty: Some(loyalty),
        },
    )
    .ok()?;
    program.validate(vocabulary).ok()?;
    Some(program)
}

#[derive(Default, Clone, Copy)]
struct FaceLoweringOptions {
    allow_empty_mana_cost: bool,
    allow_aftermath: bool,
    printed_loyalty: Option<u16>,
}

fn compile_transforming_with_reason(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
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
            allow_aftermath: false,
            printed_loyalty: None,
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
        back.color_indicator = Some(colors);
    }
    front.names = ObjectNames::single(
        EnglishCardName::new(front_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
    );
    back.names = ObjectNames::single(
        EnglishCardName::new(back_face.name.clone()).map_err(|_| "oracle:bad-face-name")?,
    );
    if front.printed_keywords.contains(&KeywordAbility::Daybound)
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
        let mut left = lower_face(
            card,
            left_face,
            left_ast,
            vocabulary,
            FaceLoweringOptions::default(),
        )?;
        let mut right = lower_face(
            card,
            right_face,
            right_ast,
            vocabulary,
            FaceLoweringOptions {
                allow_empty_mana_cost: false,
                allow_aftermath: true,
                printed_loyalty: None,
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
    let [face] = card.faces.as_slice() else {
        return Err("oracle:multi-face");
    };
    if !matches!(card.layout.as_str(), "normal" | "saga") || face.name != card.name {
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
                return Err("oracle:unsupported-card-type");
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
    let is_spell = parsed.card_types.contains(&CardType::Instant)
        || parsed.card_types.contains(&CardType::Sorcery);

    let sub_tokens: Vec<&str> = subtypes_part.split_whitespace().collect();
    let mut i = 0usize;
    while i < sub_tokens.len() {
        let tok = sub_tokens[i];
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
            let first = color_of(first_s)?;
            let alternative = if second_s == "P" {
                HybridAlternative::TwoLife
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
        let mt = match sym {
            "W" => ManaType::White,
            "U" => ManaType::Blue,
            "B" => ManaType::Black,
            "R" => ManaType::Red,
            "G" => ManaType::Green,
            "C" => ManaType::Colourless,
            _ => return None,
        };
        let entry = out.entry(mt).or_insert_with(|| 0.into());
        *entry = entry.plus(&1.into());
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

fn parse_single_mana_type(raw: &str) -> Option<ManaType> {
    match raw.trim() {
        "{W}" | "W" | "white" => Some(ManaType::White),
        "{U}" | "U" | "blue" => Some(ManaType::Blue),
        "{B}" | "B" | "black" => Some(ManaType::Black),
        "{R}" | "R" | "red" => Some(ManaType::Red),
        "{G}" | "G" | "green" => Some(ManaType::Green),
        "{C}" | "C" | "colorless" => Some(ManaType::Colourless),
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

    if is_planeswalker && options.printed_loyalty.is_none() {
        return Err("oracle:unsupported-card-type");
    }

    let (casting_cost, x_symbols, hybrid_symbols) = if is_land {
        if face.mana_cost.as_deref().is_some_and(|s| !s.is_empty()) {
            return Err("oracle:land-with-mana-cost");
        }
        (None, 0, Vec::new())
    } else {
        let raw_cost = face.mana_cost.as_deref().unwrap_or("");
        if raw_cost.is_empty() {
            if options.allow_empty_mana_cost {
                (None, 0, Vec::new())
            } else {
                return Err("oracle:missing-mana-cost");
            }
        } else {
            let (cost, x, hybrids) =
                parse_mana_cost_full(raw_cost).ok_or("oracle:unsupported-mana-cost")?;
            (Some(cost), x, hybrids)
        }
    };

    let mut ids = LoweringIds::default();
    let mut program = if is_spell {
        let cost = casting_cost.ok_or("oracle:spell-without-cost")?;
        let mut cast_during_own_combat = false;
        let mut filtered_spell_effects = Vec::with_capacity(face_ast.spell_effects.len());
        for ins in &face_ast.spell_effects {
            if let InstructionAst::AbilityRestriction {
                restriction,
                condition: None,
            } = ins
            {
                let low = restriction.to_ascii_lowercase();
                if low == "during combat+during your turn"
                    || low == "during your turn+during combat"
                    || low == "during combat on your turn"
                    || low == "cast this spell only during combat on your turn"
                {
                    cast_during_own_combat = true;
                    continue;
                }
            }
            filtered_spell_effects.push(ins.clone());
        }
        let (body, modes) = lower_spell_effects(&filtered_spell_effects, vocabulary, &mut ids)?;
        let mut p = Program::new(
            types.card_types.clone(),
            types.creature_subtypes.clone(),
            BTreeSet::new(),
            ProgramKind::Spell(SpellDefinition { cost, body, modes }),
        );
        p.spell_subtypes = types.spell_subtypes;
        p.supertypes = types.supertypes;
        p.x_mana_symbols = x_symbols;
        p.hybrid_symbols = hybrid_symbols;
        p.cast_during_own_combat = cast_during_own_combat;
        p
    } else {
        if !face_ast.spell_effects.is_empty() {
            return Err("oracle:permanent-with-spell-effects");
        }
        let printed_pt = if is_creature || types.is_vehicle {
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
                entry_tap: None,
                attachment,
                printed_pt,
                casting_cost,
            }),
        );
        p.artifact_subtypes = types.artifact_subtypes;
        p.enchantment_subtypes = types.enchantment_subtypes;
        p.planeswalker_subtypes = types.planeswalker_subtypes;
        p.printed_loyalty = options.printed_loyalty.map(|n| i64::from(n).into());
        p.supertypes = types.supertypes;
        p.x_mana_symbols = x_symbols;
        p.hybrid_symbols = hybrid_symbols;
        p
    };

    // Track Saga Read Ahead keyword for entry_tap assignment after abilities are lowered
    let mut has_read_ahead = false;

    // Lower keywords
    for (kw_idx, kw) in face_ast.keywords.iter().enumerate() {
        if options.allow_aftermath
            && kw.name.eq_ignore_ascii_case("aftermath")
            && kw.parameter.is_none()
        {
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
    for rep in &face_ast.replacements {
        lower_replacement(rep, &mut program, vocabulary, &mut ids)?;
    }

    // Lower static abilities
    for st in &face_ast.statics {
        lower_static(st, &mut program, vocabulary, &mut ids, is_spell)?;
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

    Ok(program)
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
    } else {
        program.additional_battlefield_abilities.push(ability);
        Ok(())
    }
}

fn lower_keyword_ability_name(name: &str) -> Option<KeywordAbility> {
    match name.to_ascii_lowercase().as_str() {
        "flying" => Some(KeywordAbility::Flying),
        "reach" => Some(KeywordAbility::Reach),
        "first strike" => Some(KeywordAbility::FirstStrike),
        "double strike" => Some(KeywordAbility::DoubleStrike),
        "deathtouch" => Some(KeywordAbility::Deathtouch),
        "lifelink" => Some(KeywordAbility::Lifelink),
        "vigilance" => Some(KeywordAbility::Vigilance),
        "trample" => Some(KeywordAbility::Trample),
        "haste" => Some(KeywordAbility::Haste),
        "menace" => Some(KeywordAbility::Menace),
        "defender" => Some(KeywordAbility::Defender),
        "indestructible" => Some(KeywordAbility::Indestructible),
        "hexproof" => Some(KeywordAbility::Hexproof),
        "shroud" => Some(KeywordAbility::Shroud),
        "flash" => Some(KeywordAbility::Flash),
        "fear" => Some(KeywordAbility::Fear),
        "intimidate" => Some(KeywordAbility::Intimidate),
        "skulk" => Some(KeywordAbility::Skulk),
        "shadow" => Some(KeywordAbility::Shadow),
        "horsemanship" => Some(KeywordAbility::Horsemanship),
        "phasing" => Some(KeywordAbility::Phasing),
        "wither" => Some(KeywordAbility::Wither),
        "infect" => Some(KeywordAbility::Infect),
        "split second" => Some(KeywordAbility::SplitSecond),
        "changeling" => Some(KeywordAbility::Changeling),
        "devoid" => Some(KeywordAbility::Devoid),
        "partner" => Some(KeywordAbility::Partner),
        "choose a background" => Some(KeywordAbility::ChooseABackground),
        "doctor's companion" => Some(KeywordAbility::DoctorsCompanion),
        "convoke" => Some(KeywordAbility::Convoke),
        "improvise" => Some(KeywordAbility::Improvise),
        "delve" => Some(KeywordAbility::Delve),
        "bargain" => Some(KeywordAbility::Bargain),
        "rebound" => Some(KeywordAbility::Rebound),
        "retrace" => Some(KeywordAbility::Retrace),
        "jump-start" => Some(KeywordAbility::JumpStart),
        "banding" => Some(KeywordAbility::Banding),
        "decayed" => Some(KeywordAbility::Decayed),
        "plainswalk" => Some(KeywordAbility::Plainswalk),
        "islandwalk" => Some(KeywordAbility::Islandwalk),
        "swampwalk" => Some(KeywordAbility::Swampwalk),
        "mountainwalk" => Some(KeywordAbility::Mountainwalk),
        "forestwalk" => Some(KeywordAbility::Forestwalk),
        "desertwalk" => Some(KeywordAbility::Desertwalk),
        "nonbasic landwalk" => Some(KeywordAbility::NonbasicLandwalk),
        "legendary landwalk" => Some(KeywordAbility::LegendaryLandwalk),
        "snow landwalk" => Some(KeywordAbility::SnowLandwalk),
        "snow forestwalk" => Some(KeywordAbility::SnowForestwalk),
        "snow swampwalk" => Some(KeywordAbility::SnowSwampwalk),
        "artifact landwalk" => Some(KeywordAbility::ArtifactLandwalk),
        "start your engines!" => Some(KeywordAbility::StartYourEngines),
        "daybound" => Some(KeywordAbility::Daybound),
        "nightbound" => Some(KeywordAbility::Nightbound),
        _ => None,
    }
}

fn lower_keyword(
    kw: &KeywordAst,
    kw_idx: usize,
    face_ast: &FaceAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    is_spell: bool,
) -> Result<(), &'static str> {
    let lower = kw.name.to_ascii_lowercase();
    let param = kw.parameter.as_deref().map(str::trim);

    // Standalone static keyword abilities
    if param.is_none()
        && let Some(ka) = lower_keyword_ability_name(&lower)
    {
        if ka == KeywordAbility::Decayed {
            program.printed_keywords.insert(KeywordAbility::Decayed);
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Decayed,
            ));
            return Ok(());
        }
        program.printed_keywords.insert(ka);
        return Ok(());
    }

    match lower.as_str() {
        "landwalk" => {
            let p = param.ok_or("oracle:kw-param")?.to_ascii_lowercase();
            let ka = match p.as_str() {
                "plains" => KeywordAbility::Plainswalk,
                "island" => KeywordAbility::Islandwalk,
                "swamp" => KeywordAbility::Swampwalk,
                "mountain" => KeywordAbility::Mountainwalk,
                "forest" => KeywordAbility::Forestwalk,
                "desert" => KeywordAbility::Desertwalk,
                "nonbasic land" => KeywordAbility::NonbasicLandwalk,
                "legendary land" => KeywordAbility::LegendaryLandwalk,
                "snow land" => KeywordAbility::SnowLandwalk,
                "snow forest" => KeywordAbility::SnowForestwalk,
                "snow swamp" => KeywordAbility::SnowSwampwalk,
                "artifact land" => KeywordAbility::ArtifactLandwalk,
                _ => return Err("oracle:unsupported-keyword"),
            };
            program.printed_keywords.insert(ka);
            Ok(())
        }
        "protection" => {
            let p = param.ok_or("oracle:protection-no-param")?;
            let stripped = p.strip_prefix("from ").unwrap_or(p);
            for part in stripped.split(" and ") {
                let word = part.trim().strip_prefix("from ").unwrap_or(part.trim());
                let low_w = word.to_ascii_lowercase();
                let ka = match low_w.as_str() {
                    "white" => KeywordAbility::ProtectionFromWhite,
                    "blue" => KeywordAbility::ProtectionFromBlue,
                    "black" => KeywordAbility::ProtectionFromBlack,
                    "red" => KeywordAbility::ProtectionFromRed,
                    "green" => KeywordAbility::ProtectionFromGreen,
                    "multicolored" => KeywordAbility::ProtectionFromMulticolored,
                    _ => {
                        let (base_word, creatures_only) =
                            if let Some(prefix) = low_w.strip_suffix(" creatures") {
                                (prefix.trim(), true)
                            } else {
                                (low_w.as_str(), false)
                            };
                        let resolved = resolve_protection_subtype(base_word, vocabulary)
                            .ok_or("oracle:unsupported-protection")?;
                        KeywordAbility::ProtectionFromSubtype(SubtypeProtection {
                            subtype: resolved,
                            creatures_only,
                            exclude_subtype: false,
                        })
                    }
                };
                program.printed_keywords.insert(ka);
            }
            Ok(())
        }
        "prowess" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Prowess,
            ));
            Ok(())
        }
        "exalted" => {
            program
                .triggered
                .push(TriggeredAbility::exalted(ids.triggered()));
            Ok(())
        }
        "extort" => {
            program
                .triggered
                .push(TriggeredAbility::extort(ids.triggered()));
            Ok(())
        }
        "cascade" => {
            program
                .triggered
                .push(TriggeredAbility::cascade(ids.triggered()));
            Ok(())
        }
        "storm" if is_spell => {
            program.triggered.push(TriggeredAbility::spell_copies(
                ids.triggered(),
                CastCopyKeyword::Storm,
            ));
            Ok(())
        }
        "gravestorm" if is_spell => {
            program.triggered.push(TriggeredAbility::spell_copies(
                ids.triggered(),
                CastCopyKeyword::Gravestorm,
            ));
            Ok(())
        }
        "flanking" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Flanking,
            ));
            Ok(())
        }
        "battle cry" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::BattleCry,
            ));
            Ok(())
        }
        "melee" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Melee,
            ));
            Ok(())
        }
        "dethrone" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Dethrone,
            ));
            Ok(())
        }
        "training" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Training,
            ));
            Ok(())
        }
        "ingest" => {
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Ingest,
            ));
            Ok(())
        }
        "afflict" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Afflict(n.into()),
            ));
            Ok(())
        }
        "annihilator" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program
                .triggered
                .push(TriggeredAbility::annihilator(ids.triggered(), n.into()));
            Ok(())
        }
        "bushido" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Bushido(n.into()),
            ));
            Ok(())
        }
        "frenzy" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Frenzy(n.into()),
            ));
            Ok(())
        }
        "mobilize" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Mobilize(n.into()),
            ));
            Ok(())
        }
        "poisonous" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Poisonous(n.into()),
            ));
            Ok(())
        }
        "rampage" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Rampage(n.into()),
            ));
            Ok(())
        }
        "renown" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.triggered.push(TriggeredAbility::combat_keyword(
                ids.triggered(),
                CombatTriggerKeyword::Renown(n.into()),
            ));
            Ok(())
        }
        "evolve" => {
            program
                .triggered
                .push(GrowthKeyword::Evolve.ability(ids.triggered()));
            Ok(())
        }
        "mentor" => {
            program
                .triggered
                .push(GrowthKeyword::Mentor.ability(ids.triggered()));
            Ok(())
        }
        "modular" => {
            let n: u32 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            let tk = TransferKeyword::Modular(n);
            if let Some(entry) = tk.entry() {
                program.entry_counter_abilities.push(entry);
            }
            program.triggered.push(tk.ability(ids.triggered()));
            Ok(())
        }
        "graft" => {
            let n: u32 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            let tk = TransferKeyword::Graft(n);
            if let Some(entry) = tk.entry() {
                program.entry_counter_abilities.push(entry);
            }
            program.triggered.push(tk.ability(ids.triggered()));
            Ok(())
        }
        "soulshift" => {
            let n: u32 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program
                .triggered
                .push(TransferKeyword::Soulshift(n).ability(ids.triggered()));
            Ok(())
        }
        "soulbond" => {
            for entrant_only in [false, true] {
                program.triggered.push(TriggeredAbility {
                    id: ids.triggered(),
                    condition: TriggerSpec::keyword(TriggerKeywordEvent::Soulbond { entrant_only }),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::PairWithCreature { entrant_only }],
                    ),
                });
            }
            Ok(())
        }
        "undying" => {
            program
                .triggered
                .push(CounterKeyword::Undying.ability(ids.triggered()));
            Ok(())
        }
        "persist" => {
            program
                .triggered
                .push(CounterKeyword::Persist.ability(ids.triggered()));
            Ok(())
        }
        "vanishing" => {
            let n = match param {
                None => None,
                Some(s) => Some(s.parse::<u32>().map_err(|_| "oracle:kw-param")?),
            };
            program
                .triggered
                .push(CounterKeyword::Vanishing(n).ability(ids.triggered()));
            program
                .triggered
                .push(CounterKeyword::VanishingLastCounter.ability(ids.triggered()));
            Ok(())
        }
        "fading" => {
            let n: u32 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program
                .triggered
                .push(CounterKeyword::Fading(n).ability(ids.triggered()));
            Ok(())
        }
        "echo" => {
            let cost = match param {
                Some(s) => parse_fixed_mana_cost(s).ok_or("oracle:echo-cost")?,
                None => program
                    .printed_mana_cost()
                    .cloned()
                    .ok_or("oracle:echo-no-cost")?,
            };
            program
                .triggered
                .push(UpkeepKeyword::Echo.ability(ids.triggered(), cost));
            Ok(())
        }
        "cumulative upkeep" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:cum-upkeep-cost")?;
            program
                .triggered
                .push(UpkeepKeyword::CumulativeUpkeep.ability(ids.triggered(), cost));
            Ok(())
        }
        "afterlife" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            let ab = spellwake_engine::program::TokenKeyword::Afterlife
                .ability(ids.triggered(), n.into(), vocabulary)
                .map_err(|_| "oracle:afterlife-failed")?;
            program.triggered.push(ab);
            Ok(())
        }
        "fabricate" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            let ab = spellwake_engine::program::TokenKeyword::Fabricate
                .ability(ids.triggered(), n.into(), vocabulary)
                .map_err(|_| "oracle:fabricate-failed")?;
            program.triggered.push(ab);
            Ok(())
        }
        "ward" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:ward-non-mana")?;
            program
                .triggered
                .push(TriggeredAbility::ward(ids.triggered(), cost));
            Ok(())
        }
        "myriad" => {
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
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
                        Instruction::Myriad,
                        Instruction::ScheduleCreatedTokenExile(Step::EndCombat),
                    ],
                ),
            });
            Ok(())
        }
        "bloodthirst" => {
            let p = param.ok_or("oracle:kw-param")?;
            if p.eq_ignore_ascii_case("x") {
                program
                    .entry_counter_abilities
                    .push(EntryCounterAbility::BloodthirstX);
            } else {
                let n: i64 = p.parse().map_err(|_| "oracle:kw-param")?;
                program
                    .entry_counter_abilities
                    .push(EntryCounterAbility::Bloodthirst(n.into()));
            }
            Ok(())
        }
        "devour" => {
            let n: u8 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::Devour(n));
            Ok(())
        }
        "sunburst" => {
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::Sunburst);
            Ok(())
        }
        "toxic" => {
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.printed_toxic = Some(match &program.printed_toxic {
                Some(prev) => prev.plus(&n.into()),
                None => n.into(),
            });
            Ok(())
        }
        "equip" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:equip-non-mana")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Attach(AttachAbility::Equip)],
                ),
            });
            Ok(())
        }
        "reconfigure" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:reconfigure-cost")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost.clone(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Attach(AttachAbility::Reconfigure)],
                ),
            });
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Unattach]),
            });
            Ok(())
        }
        "fortify" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:fortify-cost")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SinglePermanent,
                    vec![Instruction::Attach(AttachAbility::Fortify)],
                ),
            });
            Ok(())
        }
        "cycling" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:cycling-non-mana")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::DrawController {
                        amount: ValueExpr::literal(1),
                    }],
                ),
            });
            Ok(())
        }
        "basic landcycling" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:cycling-non-mana")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::SearchLibrary {
                        filter: Box::new(LibrarySearch::to_hand(SearchCardMatcher::BasicLand)),
                    }],
                ),
            });
            Ok(())
        }
        "plainscycling" | "islandcycling" | "swampcycling" | "mountaincycling"
        | "forestcycling" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:cycling-non-mana")?;
            let lt = match lower.as_str() {
                "plainscycling" => LandType::Plains,
                "islandcycling" => LandType::Island,
                "swampcycling" => LandType::Swamp,
                "mountaincycling" => LandType::Mountain,
                _ => LandType::Forest,
            };
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::SearchLibrary {
                        filter: Box::new(LibrarySearch::land_subtype_to_hand(lt)),
                    }],
                ),
            });
            Ok(())
        }
        "transmute" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:transmute-cost")?;
            let id = ids.activated();
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Transmute),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::SearchLibrary {
                        filter: Box::new(
                            LibrarySearch::to_hand(SearchCardMatcher::Any)
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
        "unearth" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:unearth-cost")?;
            let id = ids.activated();
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::GraveyardSelf { unearth: true },
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Unearth]),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "crew" => {
            let n: u64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            program.activated.push(ActivatedAbility::tap_for_power(
                ids.activated(),
                PowerAbility::Crew,
                n,
            ));
            Ok(())
        }
        "saddle" => {
            let n: u64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            let id = ids.activated();
            program
                .activated
                .push(ActivatedAbility::tap_for_power(id, PowerAbility::Saddle, n));
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "station" if param.is_none() => {
            let id = ids.activated();
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
        "reinforce" => {
            let p = param.ok_or("oracle:reinforce-param")?;
            let (count_raw, cost_raw) = p
                .split_once('\u{2014}')
                .or_else(|| p.split_once(" - "))
                .or_else(|| p.split_once(' '))
                .ok_or("oracle:reinforce-param")?;
            let n: i64 = count_raw
                .trim()
                .parse()
                .map_err(|_| "oracle:reinforce-count")?;
            if n <= 0 {
                return Err("oracle:reinforce-count");
            }
            let mana_cost =
                parse_fixed_mana_cost(cost_raw.trim()).ok_or("oracle:reinforce-cost")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost,
                source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Reinforce),
                body: ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::PutPowerToughnessCounters {
                        counters: PowerToughnessCounters {
                            plus_one: n.into(),
                            minus_one: 0.into(),
                        },
                    }],
                ),
            });
            Ok(())
        }
        "ninjutsu" | "commander ninjutsu" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:ninjutsu-cost")?;
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: cost,
                source_cost: SourceCost::Ninjutsu {
                    from_command: lower == "commander ninjutsu",
                },
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Ninjutsu]),
            });
            Ok(())
        }
        "flashback" => {
            let (cost, x) = param
                .and_then(parse_mana_cost_with_x)
                .ok_or("oracle:flashback-non-mana")?;
            if program.flashback.replace(cost).is_some() {
                return Err("oracle:duplicate-flashback");
            }
            program.flashback_x_mana_symbols = x;
            Ok(())
        }
        "madness" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:madness-cost")?;
            if program.madness.replace(cost).is_some() {
                return Err("oracle:duplicate-madness");
            }
            Ok(())
        }
        "plot" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:plot-cost")?;
            if program.plot.replace(cost).is_some() {
                return Err("oracle:duplicate-plot");
            }
            Ok(())
        }
        "foretell" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:foretell-cost")?;
            if program
                .exile_action
                .replace(ExileAction::Foretell(cost))
                .is_some()
            {
                return Err("oracle:duplicate-exile-action");
            }
            Ok(())
        }
        "suspend" => {
            let p = param.ok_or("oracle:suspend-param")?;
            let (n_str, cost_str) = p
                .split_once('\u{2014}')
                .or_else(|| p.split_once('-'))
                .ok_or("oracle:suspend-param")?;
            let counters: u16 = n_str.trim().parse().map_err(|_| "oracle:suspend-count")?;
            let price = parse_fixed_mana_cost(cost_str.trim()).ok_or("oracle:suspend-cost")?;
            if program
                .exile_action
                .replace(ExileAction::Suspend { counters, price })
                .is_some()
            {
                return Err("oracle:duplicate-exile-action");
            }
            Ok(())
        }
        "kicker" => {
            let (cost, x) = param
                .and_then(parse_mana_cost_with_x)
                .ok_or("oracle:kicker-non-mana")?;
            if x == 0 {
                program
                    .additional_cast_costs
                    .push(AdditionalCastCost::Kicker(cost));
            } else if x == 1 {
                program
                    .additional_cast_costs
                    .push(AdditionalCastCost::KickerX(cost));
            } else {
                return Err("oracle:kicker-multi-x");
            }
            Ok(())
        }
        "multikicker" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:multikicker-non-mana")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Multikicker(cost));
            Ok(())
        }
        "buyback" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:buyback-non-mana")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Buyback(cost));
            Ok(())
        }
        "entwine" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:entwine-non-mana")?;
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Entwine(cost));
            Ok(())
        }
        "replicate" if is_spell => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:replicate-non-mana")?;
            let idx = program.additional_cast_costs.len();
            program
                .additional_cast_costs
                .push(AdditionalCastCost::Replicate(cost));
            program.triggered.push(TriggeredAbility::spell_copies(
                ids.triggered(),
                CastCopyKeyword::Replicate(idx),
            ));
            Ok(())
        }
        "evoke" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:evoke-cost")?;
            if program.conditional_cost.is_some() {
                return Err("oracle:duplicate-conditional-cost");
            }
            let tid = ids.triggered();
            *program = std::mem::replace(program, Program::enchantment(None)).with_evoke(cost, tid);
            Ok(())
        }
        "dash" | "blitz" | "warp" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:endstep-cast-cost")?;
            if program.conditional_cost.is_some() {
                return Err("oracle:duplicate-conditional-cost");
            }
            let kind = match lower.as_str() {
                "dash" => EndStepCast::Dash,
                "blitz" => EndStepCast::Blitz,
                _ => EndStepCast::Warp,
            };
            let tid = ids.triggered();
            *program = std::mem::replace(program, Program::enchantment(None))
                .with_end_step_cast(kind, cost, tid);
            Ok(())
        }
        "bestow" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:bestow-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Bestow(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "surge" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:surge-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Surge(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "spectacle" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:spectacle-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Spectacle(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "prowl" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:prowl-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Prowl(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "freerunning" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:freerunning-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Freerunning(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "morph" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:morph-non-mana")?;
            program.face_up_abilities.push(FaceUpAbility::Morph(cost));
            Ok(())
        }
        "megamorph" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:megamorph-non-mana")?;
            program
                .face_up_abilities
                .push(FaceUpAbility::Megamorph(cost));
            Ok(())
        }
        "disguise" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:disguise-non-mana")?;
            program
                .face_up_abilities
                .push(FaceUpAbility::Disguise(cost));
            Ok(())
        }
        "undaunted" => {
            program.cast_reductions.push(CastReduction::Undaunted);
            Ok(())
        }
        "affinity" => {
            let p = param.ok_or("oracle:affinity-param")?;
            let stripped = p
                .strip_prefix("for ")
                .unwrap_or(p)
                .trim()
                .to_ascii_lowercase();
            let mut sel = PermanentSelector {
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            };
            match stripped.as_str() {
                "artifacts" => {
                    sel.required_types.insert(CardType::Artifact);
                }
                "creatures" => {
                    sel.required_types.insert(CardType::Creature);
                }
                "enchantments" => {
                    sel.required_types.insert(CardType::Enchantment);
                }
                "lands" => {
                    sel.required_types.insert(CardType::Land);
                }
                "plains" => {
                    sel.land_subtype = Some(LandType::Plains);
                }
                "islands" => {
                    sel.land_subtype = Some(LandType::Island);
                }
                "swamps" => {
                    sel.land_subtype = Some(LandType::Swamp);
                }
                "mountains" => {
                    sel.land_subtype = Some(LandType::Mountain);
                }
                "forests" => {
                    sel.land_subtype = Some(LandType::Forest);
                }
                "equipment" => {
                    sel.artifact_subtype = Some(ArtifactSubtype::Equipment);
                }
                _ => return Err("oracle:unsupported-affinity"),
            }
            program
                .cast_reductions
                .push(CastReduction::Affinity(Box::new(sel)));
            Ok(())
        }
        "ascend" => {
            push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::Ascend,
                },
            )?;
            Ok(())
        }
        "increment" => {
            program
                .triggered
                .push(GrowthKeyword::Increment.ability(ids.triggered()));
            Ok(())
        }
        "backup" => {
            // CR 702.165: Backup grants only abilities printed below it. Admit cleanly when
            // no non-keyword abilities/statics/replacements exist and all following keywords
            // lower to supported static KeywordAbility values.
            if !face_ast.abilities.is_empty()
                || !face_ast.statics.is_empty()
                || !face_ast.replacements.is_empty()
                || !face_ast.spell_effects.is_empty()
            {
                return Err("oracle:unsupported-backup-abilities");
            }
            let n: i64 = param
                .and_then(|s| s.parse().ok())
                .ok_or("oracle:kw-param")?;
            if n <= 0 {
                return Err("oracle:kw-param");
            }
            let mut printed_below = BTreeSet::new();
            for next_kw in &face_ast.keywords[kw_idx + 1..] {
                if next_kw.name.eq_ignore_ascii_case("backup") {
                    continue;
                }
                if next_kw.parameter.is_some() {
                    return Err("oracle:unsupported-backup-abilities");
                }
                let ka = lower_keyword_ability_name(&next_kw.name)
                    .ok_or("oracle:unsupported-backup-abilities")?;
                printed_below.insert(ka);
            }
            program.triggered.push(
                GrowthKeyword::Backup {
                    amount: n.into(),
                    printed_below,
                }
                .ability(ids.triggered()),
            );
            Ok(())
        }
        "encore" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:encore-cost")?;
            let id = ids.activated();
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: true,
                },
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Encore]),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "escape" => {
            let p = param.ok_or("oracle:escape-param")?;
            let (mana_raw, rest) = p
                .split_once(", Exile ")
                .or_else(|| p.split_once(", exile "))
                .ok_or("oracle:escape-param")?;
            let mana = parse_fixed_mana_cost(mana_raw.trim()).ok_or("oracle:escape-cost")?;
            let (count_word, tail) = rest.trim().split_once(' ').ok_or("oracle:escape-param")?;
            let exile: u16 = match count_word.to_ascii_lowercase().as_str() {
                "a" | "one" => 1,
                "two" => 2,
                "three" => 3,
                "four" => 4,
                "five" => 5,
                "six" => 6,
                "seven" => 7,
                "eight" => 8,
                other => other.parse().map_err(|_| "oracle:escape-count")?,
            };
            let tail_low = tail.trim().trim_end_matches('.').to_ascii_lowercase();
            if tail_low != "other card from your graveyard"
                && tail_low != "other cards from your graveyard"
            {
                return Err("oracle:escape-param");
            }
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Escape {
                    mana,
                    exile,
                    entry_counters: 0,
                })
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "sneak" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:sneak-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Sneak(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "mayhem" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:mayhem-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Mayhem(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "mutate" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:mutate-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Mutate(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "web-slinging" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:webslinging-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::WebSlinging(cost))
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        "emerge" => {
            let p = param.ok_or("oracle:emerge-param")?;
            let (from, mana_str) = if let Some(rest) = p.strip_prefix("from ") {
                let (kind_str, cost_str) = rest.split_once(' ').ok_or("oracle:emerge-param")?;
                let ct = match kind_str.to_ascii_lowercase().as_str() {
                    "creature" => CardType::Creature,
                    "artifact" => CardType::Artifact,
                    "enchantment" => CardType::Enchantment,
                    "land" => CardType::Land,
                    _ => return Err("oracle:emerge-type"),
                };
                (ct, cost_str.trim())
            } else {
                (CardType::Creature, p)
            };
            let mana = parse_fixed_mana_cost(mana_str).ok_or("oracle:emerge-cost")?;
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Emerge { mana, from })
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        kw_name @ ("living weapon" | "for mirrodin!" | "job select") if param.is_none() => {
            let (pt, color, labels): (i64, Option<Color>, &[&str]) = match kw_name {
                "living weapon" => (0, Some(Color::Black), &["Phyrexian", "Germ"]),
                "for mirrodin!" => (2, Some(Color::Red), &["Rebel"]),
                "job select" => (1, None, &["Hero"]),
                _ => return Err("oracle:unsupported-keyword"),
            };
            let types = labels
                .iter()
                .map(|word| vocabulary.resolve_label(word))
                .collect::<Option<Vec<_>>>()
                .ok_or("oracle:token-unknown-subtype")?;
            let mut token =
                Program::creature(None, pt.into(), pt.into(), types.iter().copied().collect());
            token.color_indicator = color.map(|c| BTreeSet::from([c]));
            let definition = spellwake_engine::token::TokenDefinition::new(
                token,
                spellwake_engine::token::TokenName::Default {
                    ordered_subtypes: types
                        .into_iter()
                        .map(spellwake_engine::token::TokenSubtype::Creature)
                        .collect(),
                },
                vocabulary,
            )
            .map_err(|_| "oracle:invalid-token-def")?;
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::enters(ObjectFilter::self_object()),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::CreateTokenAndAttach {
                        definition: Box::new(definition),
                    }],
                ),
            });
            Ok(())
        }
        "outlast" => {
            let cost = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:outlast-cost")?;
            let id = ids.activated();
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: cost,
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::PutCountersOnSource { amount: 1.into() }],
                ),
            });
            program
                .activation_restrictions
                .insert(id, ActivationRestriction::Sorcery);
            Ok(())
        }
        "miracle" => {
            let (mana, x_symbols) = param
                .and_then(parse_mana_cost_with_x)
                .ok_or("oracle:miracle-cost")?;
            if program
                .miracle
                .replace(MiracleCost { mana, x_symbols })
                .is_some()
            {
                return Err("oracle:duplicate-miracle");
            }
            Ok(())
        }
        "splice onto" => {
            if !is_spell {
                return Err("oracle:splice-non-spell");
            }
            let p = param.ok_or("oracle:splice-param")?;
            let (quality, cost_raw) = if let Some(rest) = p
                .strip_prefix("Arcane ")
                .or_else(|| p.strip_prefix("arcane "))
            {
                (SpliceQuality::Arcane, rest)
            } else if let Some(rest) = p
                .strip_prefix("instant or sorcery ")
                .or_else(|| p.strip_prefix("Instant or Sorcery "))
            {
                (SpliceQuality::InstantOrSorcery, rest)
            } else {
                return Err("oracle:splice-quality");
            };
            let cost = parse_fixed_mana_cost(cost_raw.trim()).ok_or("oracle:splice-cost")?;
            if !program.supports_splice_text() {
                return Err("oracle:splice-unsupported-text");
            }
            if program
                .splice
                .replace(SpliceAbility { quality, cost })
                .is_some()
            {
                return Err("oracle:duplicate-splice");
            }
            Ok(())
        }
        "overload" => {
            if !is_spell || face_ast.spell_effects.is_empty() {
                return Err("oracle:overload-non-spell");
            }
            let mana = param
                .and_then(parse_fixed_mana_cost)
                .ok_or("oracle:overload-cost")?;
            let mut transformed = Vec::with_capacity(face_ast.spell_effects.len());
            let mut changed_any = false;
            for ins in &face_ast.spell_effects {
                let (next, changed) = replace_targets_with_each_ins(ins)?;
                changed_any |= changed;
                transformed.push(next);
            }
            if !changed_any {
                return Err("oracle:overload-no-target");
            }
            let each_body = lower_effect_body(&transformed, vocabulary, ids, false, false)?;
            if each_body.target != BodyTarget::None {
                return Err("oracle:overload-remaining-target");
            }
            if program
                .conditional_cost
                .replace(ConditionalCastCost::Overload {
                    mana,
                    each: Box::new(each_body),
                })
                .is_some()
            {
                return Err("oracle:duplicate-conditional-cost");
            }
            Ok(())
        }
        _ => Err("oracle:unsupported-keyword"),
    }
}

fn replace_targets_with_each_subject(
    subj: &SubjectRefAst,
) -> Result<(SubjectRefAst, bool), &'static str> {
    match subj {
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if filter.zone.is_none() && !filter.any_spell => {
            Ok((SubjectRefAst::Each(filter.clone()), true))
        }
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => Ok((
            SubjectRefAst::Player(if *opponent_only {
                PlayerRefAst::EachOpponent
            } else {
                PlayerRefAst::EachPlayer
            }),
            true,
        )),
        SubjectRefAst::TargetObject { .. }
        | SubjectRefAst::TargetPlayer { .. }
        | SubjectRefAst::AnyTarget
        | SubjectRefAst::TargetPlayerOrPlaneswalker { .. } => {
            Err("oracle:overload-unsupported-target")
        }
        other => Ok((other.clone(), false)),
    }
}

fn replace_targets_with_each_player(p: PlayerRefAst) -> (PlayerRefAst, bool) {
    match p {
        PlayerRefAst::TargetPlayer => (PlayerRefAst::EachPlayer, true),
        PlayerRefAst::TargetOpponent => (PlayerRefAst::EachOpponent, true),
        other => (other, false),
    }
}

fn replace_targets_with_each_ins(
    ins: &InstructionAst,
) -> Result<(InstructionAst, bool), &'static str> {
    match ins {
        InstructionAst::Destroy { subject } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::Destroy {
                    subject: Box::new(s),
                },
                c,
            ))
        }
        InstructionAst::Exile { actor, subject } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::Exile {
                    actor: *actor,
                    subject: Box::new(s),
                },
                c,
            ))
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped,
            transformed,
            with_counter,
            attached_to,
        } if attached_to.is_none() => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::ReturnZone {
                    subject: Box::new(s),
                    from_zone: from_zone.clone(),
                    to: to.clone(),
                    tapped: *tapped,
                    transformed: *transformed,
                    with_counter: with_counter.clone(),
                    attached_to: None,
                },
                c,
            ))
        }
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => {
            let (t, c) = replace_targets_with_each_subject(target)?;
            Ok((
                InstructionAst::DealDamage {
                    source: source.clone(),
                    target: Box::new(t),
                    amount: amount.clone(),
                },
                c,
            ))
        }
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale,
        } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::ModifyPT {
                    subject: Box::new(s),
                    dx: dx.clone(),
                    dy: dy.clone(),
                    duration: duration.clone(),
                    scale: scale.clone(),
                },
                c,
            ))
        }
        InstructionAst::GrantKeywords {
            subject,
            keywords,
            duration,
        } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::GrantKeywords {
                    subject: Box::new(s),
                    keywords: keywords.clone(),
                    duration: duration.clone(),
                },
                c,
            ))
        }
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora,
            random,
        } => {
            let (p, c) = replace_targets_with_each_player(*player);
            Ok((
                InstructionAst::Discard {
                    player: p,
                    count: count.clone(),
                    filter: filter.clone(),
                    anaphora: anaphora.clone(),
                    random: *random,
                },
                c,
            ))
        }
        InstructionAst::Draw { player, count } => {
            let (p, c) = replace_targets_with_each_player(*player);
            Ok((
                InstructionAst::Draw {
                    player: p,
                    count: count.clone(),
                },
                c,
            ))
        }
        InstructionAst::Mill { player, count } => {
            let (p, c) = replace_targets_with_each_player(*player);
            Ok((
                InstructionAst::Mill {
                    player: p,
                    count: count.clone(),
                },
                c,
            ))
        }
        InstructionAst::TapPermanent { subject } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::TapPermanent {
                    subject: Box::new(s),
                },
                c,
            ))
        }
        InstructionAst::UntapPermanent { subject } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::UntapPermanent {
                    subject: Box::new(s),
                },
                c,
            ))
        }
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } => {
            let (s, c) = replace_targets_with_each_subject(subject)?;
            Ok((
                InstructionAst::PutCounters {
                    subject: Box::new(s),
                    counter_kind: counter_kind.clone(),
                    count: count.clone(),
                },
                c,
            ))
        }
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } => {
            let (p, c) = replace_targets_with_each_player(*player);
            Ok((
                InstructionAst::ChangeLife {
                    player: p,
                    delta: delta.clone(),
                    gain: *gain,
                },
                c,
            ))
        }
        _ => Err("oracle:overload-unsupported-ins"),
    }
}

fn resolve_protection_subtype(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<CreatureTypeId> {
    let trimmed = raw.trim();
    let title = |s: &str| {
        let mut chars = s.chars();
        match chars.next() {
            Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
            None => String::new(),
        }
    };
    if let Some(id) = vocabulary.resolve_label(&title(trimmed)) {
        return Some(id);
    }
    if let Some(stem) = trimmed.strip_suffix("es")
        && let Some(id) = vocabulary.resolve_label(&title(stem))
    {
        return Some(id);
    }
    if let Some(stem) = trimmed.strip_suffix('s')
        && let Some(id) = vocabulary.resolve_label(&title(stem))
    {
        return Some(id);
    }
    if let Some(stem) = trimmed.strip_suffix("ves")
        && let Some(id) = vocabulary.resolve_label(&title(&format!("{stem}f")))
    {
        return Some(id);
    }
    None
}

fn lower_replacement(
    rep: &ReplacementEffectAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if let ReplacementEffectAst::EscapesWithCounters {
        counter_kind,
        count,
    } = rep
        && counter_kind == "+1/+1"
        && let ValueExprAst::Const(n) = count.as_ref()
        && let Ok(u) = u16::try_from(*n)
        && let Some(ConditionalCastCost::Escape {
            ref mut entry_counters,
            ..
        }) = program.conditional_cost
    {
        *entry_counters = entry_counters
            .checked_add(u)
            .ok_or("oracle:escape-counter-ovf")?;
        return Ok(());
    }
    if let ReplacementEffectAst::EventInstead {
        subject_or_event,
        would,
        instead,
        instead_body: None,
    } = rep
    {
        let ev_low = format!(
            "{} {} {}",
            subject_or_event.to_ascii_lowercase(),
            would.to_ascii_lowercase(),
            instead.to_ascii_lowercase()
        );
        if (subject_or_event.trim() == "~"
            || subject_or_event.to_ascii_lowercase().starts_with("this "))
            && would
                .to_ascii_lowercase()
                .contains("put into a graveyard from anywhere")
            && instead.to_ascii_lowercase().contains("exile it instead")
        {
            let next_id = u64::try_from(program.zone_replacements.len()).unwrap_or(0);
            program.zone_replacements.push(ZoneReplacementAbility {
                id: ZoneReplacementAbilityId(next_id),
                actions: ZoneActionPredicate::Any,
                objects: ZoneObjectKind::Either,
                subject: ZoneReplacementSubject::SourceSelf,
                destination: PublicZoneDestination::Graveyard,
                replace_with: PublicZoneDestination::Exile,
                choice: ZoneReplacementChoice::Mandatory,
            });
            return Ok(());
        }
        if (ev_low.contains("create one or more tokens under your control")
            || ev_low.contains("put one or more tokens onto the battlefield under your control"))
            && ev_low.contains("twice that many")
        {
            return push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::DoubleCreatedTokens,
                },
            );
        }
        if ev_low.contains("put one or more counters on a permanent you control")
            && ev_low.contains("twice that many")
        {
            return push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::DoublePlacedCounters,
                },
            );
        }
        if ev_low.contains("one or more +1/+1 counters would be put on ~")
            && ev_low.contains("that many plus one")
        {
            return push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::AdditionalSelfPlusOneCounter,
                },
            );
        }
        if ev_low.contains("source you control would deal damage to an opponent")
            && ev_low.contains("deals double that damage")
        {
            return push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::DoubleToOpponents,
                },
            );
        }
    }
    let ProgramKind::Permanent(def) = &mut program.kind else {
        return Err("oracle:replacement-on-spell");
    };
    match rep {
        ReplacementEffectAst::EntersTapped { unless: None } => {
            if def.entry_tap.replace(EntryTap::Always).is_some() {
                return Err("oracle:duplicate-entry-tap");
            }
            Ok(())
        }
        ReplacementEffectAst::EntersTapped { unless: Some(cond) } => {
            let entry_tap = lower_enters_tapped_unless(cond)?;
            if def.entry_tap.replace(entry_tap).is_some() {
                return Err("oracle:duplicate-entry-tap");
            }
            Ok(())
        }
        ReplacementEffectAst::EntersTappedUnlessPayLife { life } if *life > 0 => {
            let amount = u8::try_from(*life).map_err(|_| "oracle:life-overflow")?;
            if def
                .entry_tap
                .replace(EntryTap::UnlessPayLife(amount))
                .is_some()
            {
                return Err("oracle:duplicate-entry-tap");
            }
            Ok(())
        }
        ReplacementEffectAst::OthersEnterTapped { subject } => {
            let selector = lower_permanent_selector(subject, vocabulary)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ids.static_id(),
                    rule: StaticRestriction::OthersEnterTapped(Box::new(selector)),
                },
            )
        }
        ReplacementEffectAst::EntersTappedUnlessReveal { filter } => {
            if filter.subtypes.len() == 2
                && let (Some(a), Some(b)) = (
                    parse_basic_land_subtype(&filter.subtypes[0]),
                    parse_basic_land_subtype(&filter.subtypes[1]),
                )
            {
                if def
                    .entry_tap
                    .replace(EntryTap::UnlessRevealHandLandType(a, b))
                    .is_some()
                {
                    return Err("oracle:duplicate-entry-tap");
                }
                return Ok(());
            }
            Err("oracle:unsupported-entry-reveal")
        }
        ReplacementEffectAst::EntersWithCounters {
            counter_kind,
            count,
            per,
            x_expr,
            condition,
        } => {
            let ck = counter_kind.to_ascii_lowercase();
            // Case 1: "enters with [count] [kind] counters for each [per]"
            if let Some(per_filter) = per
                && x_expr.is_none()
                && condition.is_none()
                && let ValueExprAst::Const(mult) = count.as_ref()
                && *mult > 0
            {
                let kind = parse_counter_kind(&ck).ok_or("oracle:unsupported-entry-counters")?;
                if !CounterKind::PERMANENT.contains(&kind) {
                    return Err("oracle:unsupported-entry-counters");
                }
                let mut inner = per_filter.as_ref().clone();
                let zone = match inner.zone.take() {
                    None => spellwake_engine::program::ValueZone::Battlefield,
                    Some(ZoneFilter::YourGraveyard) => {
                        spellwake_engine::program::ValueZone::Graveyard(ResolvingPlayer::Controller)
                    }
                    Some(ZoneFilter::YourHand) => {
                        spellwake_engine::program::ValueZone::Hand(ResolvingPlayer::Controller)
                    }
                    _ => return Err("oracle:unsupported-entry-counters"),
                };
                let obj_filter = if is_unrestricted_filter(&inner) {
                    ObjectFilter::any()
                } else {
                    lower_object_filter(&inner, vocabulary)?
                };
                let base = ValueExpr::Count {
                    zone,
                    filter: Box::new(obj_filter),
                    controller_scope: None,
                };
                let amount = if *mult == 1 {
                    base
                } else {
                    ValueExpr::Scale {
                        expr: Box::new(base),
                        factor: i64::from(*mult).into(),
                    }
                };
                program
                    .entry_counter_abilities
                    .push(EntryCounterAbility::DynamicCounter { kind, amount });
                return Ok(());
            }
            // Case 2: "enters with X [kind] counters, where X is [x_expr]"
            if let Some(xe) = x_expr
                && per.is_none()
                && condition.is_none()
                && matches!(count.as_ref(), ValueExprAst::ChosenX)
            {
                let kind = parse_counter_kind(&ck).ok_or("oracle:unsupported-entry-counters")?;
                if !CounterKind::PERMANENT.contains(&kind) {
                    return Err("oracle:unsupported-entry-counters");
                }
                let amount = lower_value_expr(xe, vocabulary)?;
                program
                    .entry_counter_abilities
                    .push(EntryCounterAbility::DynamicCounter { kind, amount });
                return Ok(());
            }
            if per.is_some() || x_expr.is_some() {
                return Err("oracle:unsupported-entry-counters");
            }
            match (ck.as_str(), count.as_ref(), condition.as_deref()) {
                ("+1/+1", ValueExprAst::Const(n), None) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedPlusOne(i64::from(*n).into()));
                    Ok(())
                }
                ("-1/-1", ValueExprAst::Const(n), None) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedMinusOne(i64::from(*n).into()));
                    Ok(())
                }
                ("+1/+1", ValueExprAst::ChosenX, None) => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::ChosenXPlusOne);
                    Ok(())
                }
                (
                    "+1/+1",
                    ValueExprAst::Const(n),
                    Some(ConditionAst::Morbid {
                        your_control_only: false,
                    }),
                ) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::MorbidPlusOne(i64::from(*n).into()));
                    Ok(())
                }
                ("charge", ValueExprAst::Const(n), None) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedCharge(i64::from(*n).into()));
                    Ok(())
                }
                ("loyalty", ValueExprAst::Const(n), None) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedLoyalty(i64::from(*n).into()));
                    Ok(())
                }
                (other, ValueExprAst::Const(n), None) if *n > 0 => {
                    let kind =
                        parse_counter_kind(other).ok_or("oracle:unsupported-entry-counters")?;
                    if matches!(
                        kind,
                        CounterKind::PlusOne
                            | CounterKind::MinusOne
                            | CounterKind::Poison
                            | CounterKind::Experience
                            | CounterKind::Energy
                    ) {
                        return Err("oracle:unsupported-entry-counters");
                    }
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedNamed {
                            kind,
                            amount: i64::from(*n).into(),
                        });
                    Ok(())
                }
                (other, expr, None) => {
                    let kind =
                        parse_counter_kind(other).ok_or("oracle:unsupported-entry-counters")?;
                    if !CounterKind::PERMANENT.contains(&kind) {
                        return Err("oracle:unsupported-entry-counters");
                    }
                    let amount = lower_value_expr(expr, vocabulary)?;
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::DynamicCounter { kind, amount });
                    Ok(())
                }
                _ => Err("oracle:unsupported-entry-counters"),
            }
        }
        ReplacementEffectAst::OthersEnterWithCounters {
            subject,
            counter_kind,
            count,
            additional: _,
        } => {
            let kind = parse_counter_kind(counter_kind)
                .ok_or("oracle:unsupported-others-entry-counters")?;
            if !CounterKind::PERMANENT.contains(&kind) {
                return Err("oracle:unsupported-others-entry-counters");
            }
            let ValueExprAst::Const(n) = count.as_ref() else {
                return Err("oracle:unsupported-others-entry-counters");
            };
            let cnt = u8::try_from(*n)
                .ok()
                .filter(|&v| v > 0)
                .ok_or("oracle:unsupported-others-entry-counters")?;
            let selectors = lower_permanent_selectors(subject, vocabulary)?;
            for sel in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Modifier {
                        id: ids.static_id(),
                        rule: StaticModifierRule::OthersEnterWithCounters {
                            selector: Box::new(sel),
                            kind,
                            count: cnt,
                        },
                    },
                )?;
            }
            Ok(())
        }
        ReplacementEffectAst::EntersTappedWithCounters {
            counter_kind,
            count,
        } => {
            if def.entry_tap.replace(EntryTap::Always).is_some() {
                return Err("oracle:duplicate-entry-tap");
            }
            match (counter_kind.to_ascii_lowercase().as_str(), count.as_ref()) {
                ("+1/+1", ValueExprAst::Const(n)) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedPlusOne(i64::from(*n).into()));
                    Ok(())
                }
                ("+1/+1", ValueExprAst::ChosenX) => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::ChosenXPlusOne);
                    Ok(())
                }
                ("charge", ValueExprAst::Const(n)) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedCharge(i64::from(*n).into()));
                    Ok(())
                }
                (other, ValueExprAst::Const(n)) if *n > 0 => {
                    let kind = parse_counter_kind(other)
                        .ok_or("oracle:unsupported-entry-tapped-counters")?;
                    if matches!(
                        kind,
                        CounterKind::PlusOne
                            | CounterKind::MinusOne
                            | CounterKind::Poison
                            | CounterKind::Experience
                            | CounterKind::Energy
                    ) {
                        return Err("oracle:unsupported-entry-tapped-counters");
                    }
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedNamed {
                            kind,
                            amount: i64::from(*n).into(),
                        });
                    Ok(())
                }
                _ => Err("oracle:unsupported-entry-tapped-counters"),
            }
        }
        ReplacementEffectAst::AsEntersChoose { choice } => {
            let low = choice.trim().to_ascii_lowercase();
            let entry_tap = match low.as_str() {
                "a creature type" | "creature type" => EntryTap::ChooseCreatureType,
                "a nonland card name" | "nonland card name" => EntryTap::ChooseNonlandCardName,
                "two players" => EntryTap::ChooseTwoPlayers,
                _ => return Err("oracle:unsupported-as-enters-choose"),
            };
            if def.entry_tap.replace(entry_tap).is_some() {
                return Err("oracle:duplicate-entry-tap");
            }
            Ok(())
        }
        ReplacementEffectAst::EntersAsCopy {
            optional: true,
            filter,
            exception: None,
        } if filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty()
            && filter.forbidden_subtypes.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.required_supertypes.is_empty()
            && filter.forbidden_supertypes.is_empty()
            && filter.required_keywords.is_empty()
            && filter.forbidden_keywords.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.combat.is_none()
            && filter.tapped.is_none()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            let rule = match (filter.zone, filter.controller, filter.any_types) {
                (Some(ZoneFilter::AnyGraveyard), None, m) if m == AstCardTypeMask::CREATURE => {
                    EntryCopyRule::GraveyardCreature
                }
                (None | Some(ZoneFilter::Battlefield), None, m)
                    if m == AstCardTypeMask::CREATURE =>
                {
                    EntryCopyRule::BattlefieldCreature
                }
                (None | Some(ZoneFilter::Battlefield), Some(ControllerRelation::You), m)
                    if m == AstCardTypeMask::CREATURE =>
                {
                    EntryCopyRule::ControlledCreature
                }
                (None | Some(ZoneFilter::Battlefield), None, m)
                    if m == AstCardTypeMask::ARTIFACT =>
                {
                    EntryCopyRule::BattlefieldArtifact
                }
                (None | Some(ZoneFilter::Battlefield), None, m)
                    if m == AstCardTypeMask::ENCHANTMENT =>
                {
                    EntryCopyRule::BattlefieldEnchantment
                }
                _ => return Err("oracle:unsupported-enters-as-copy"),
            };
            if program.entry_copy.replace(rule).is_some() {
                return Err("oracle:duplicate-entry-copy");
            }
            Ok(())
        }
        _ => Err("oracle:unsupported-replacement"),
    }
}

fn lower_enters_tapped_unless(cond: &ConditionAst) -> Result<EntryTap, &'static str> {
    match cond {
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 2,
        } => {
            if filter.any_types == AstCardTypeMask::LAND
                && filter.required_supertypes == AstSupertypeMask::BASIC
            {
                return Ok(EntryTap::UnlessTwoBasicLands);
            }
            if filter.any_types == AstCardTypeMask::LAND
                && filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0
                && filter.required_supertypes.is_empty()
            {
                return Ok(EntryTap::UnlessTwoOtherLands);
            }
            Err("oracle:unsupported-entry-unless")
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Le,
            value: 2,
        } if filter.any_types == AstCardTypeMask::LAND
            && filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0 =>
        {
            Ok(EntryTap::UnlessTwoOrFewerOtherLands)
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes.len() == 2 => {
            if let (Some(a), Some(b)) = (
                parse_basic_land_subtype(&filter.subtypes[0]),
                parse_basic_land_subtype(&filter.subtypes[1]),
            ) {
                return Ok(EntryTap::UnlessLandType(a, b));
            }
            Err("oracle:unsupported-entry-unless")
        }
        ConditionAst::OpponentCountCompare {
            op: CompareOp::Ge,
            value: 2,
        } => Ok(EntryTap::UnlessTwoOpponents),
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op: CompareOp::Ge,
            value: 8,
        } if filter.any_types == AstCardTypeMask::LAND => Ok(EntryTap::UnlessOpponentLandsEight),
        ConditionAst::LifeCompare {
            player: PlayerRefAst::AnyPlayer,
            op: CompareOp::Le,
            value: 13,
        } => Ok(EntryTap::UnlessPlayerLifeThirteenOrLess),
        _ => Err("oracle:unsupported-entry-unless"),
    }
}

fn lower_static(
    st: &StaticAbilityAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    is_spell: bool,
) -> Result<(), &'static str> {
    match st {
        StaticAbilityAst::AttachmentRestriction { .. } => {
            // Already consumed when creating Aura attachment restriction
            if matches!(
                program.permanent().and_then(|p| p.attachment),
                Some(AttachmentKind::Aura(_))
            ) {
                Ok(())
            } else {
                Err("oracle:enchant-on-non-aura")
            }
        }
        StaticAbilityAst::ContinuousModifyPT {
            subject,
            dx,
            dy,
            grant_keywords,
            grant_ability_text,
            scale,
            condition,
            paired_rule,
        } => {
            let mut selectors = lower_permanent_selectors(subject, vocabulary)?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let (p_expr, t_expr) = lower_pt_delta_pair(dx, dy, scale.as_deref(), vocabulary)?;
            let ab_change = if !grant_keywords.is_empty() {
                Some(lower_ability_change(grant_keywords, vocabulary)?)
            } else {
                None
            };
            let granted_part = if let Some(raw_text) = grant_ability_text {
                let cleaned = raw_text.trim().trim_matches('"').trim();
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let synthetic = compile_card(
                    &CardInput {
                        oracle_id: "",
                        name: "Synthetic",
                        layout: "normal",
                        faces: vec![FaceInput {
                            name: "Synthetic",
                            type_line: "Creature",
                            oracle_text: cleaned,
                        }],
                    },
                    &compiler_vocab,
                );
                if !synthetic.strictly_compiled || synthetic.faces.len() != 1 {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                Some(lower_granted_ability_face(
                    &synthetic.faces[0],
                    vocabulary,
                    ids,
                )?)
            } else {
                None
            };
            let paired_br = if let Some(paired) = paired_rule {
                match paired.as_ref() {
                    StaticAbilityAst::RuleOrCombatRestriction {
                        rule,
                        filter,
                        condition: None,
                        ..
                    } => Some(
                        lower_rule_blocking_restriction(rule, filter.as_deref(), vocabulary)
                            .ok_or("oracle:unsupported-paired-static")?,
                    ),
                    _ => return Err("oracle:unsupported-paired-static"),
                }
            } else {
                None
            };
            for selector in selectors {
                let mut parts = vec![StaticPart::ModifyPT(NumericPair {
                    power: p_expr.clone(),
                    toughness: t_expr.clone(),
                })];
                if let Some(ab) = &ab_change {
                    parts.push(StaticPart::Abilities(ab.clone()));
                }
                if let Some(gp) = &granted_part {
                    if ab_change.is_some() {
                        push_static_ability(
                            program,
                            StaticContinuousAbility::Battlefield {
                                id: ids.static_id(),
                                selector: selector.clone(),
                                parts: StaticParts(vec![gp.clone()]),
                            },
                        )?;
                    } else {
                        parts.push(gp.clone());
                    }
                }
                if let Some(br) = &paired_br {
                    if ab_change.is_some() || granted_part.is_some() {
                        push_static_ability(
                            program,
                            StaticContinuousAbility::Battlefield {
                                id: ids.static_id(),
                                selector: selector.clone(),
                                parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                    br.clone(),
                                )]),
                            },
                        )?;
                    } else {
                        parts.push(StaticPart::BlockingRestriction(br.clone()));
                    }
                }
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(parts),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::GrantKeywords {
            subject,
            keywords,
            condition,
            paired_rule,
        } => {
            let mut selectors = lower_permanent_selectors(subject, vocabulary)?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let ab_change = lower_ability_change(keywords, vocabulary)?;
            let paired_br = if let Some(paired) = paired_rule {
                match paired.as_ref() {
                    StaticAbilityAst::RuleOrCombatRestriction {
                        rule,
                        filter,
                        condition: None,
                        ..
                    } => Some(
                        lower_rule_blocking_restriction(rule, filter.as_deref(), vocabulary)
                            .ok_or("oracle:unsupported-paired-static")?,
                    ),
                    _ => return Err("oracle:unsupported-paired-static"),
                }
            } else {
                None
            };
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: selector.clone(),
                        parts: StaticParts(vec![StaticPart::Abilities(ab_change.clone())]),
                    },
                )?;
                if let Some(br) = &paired_br {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![StaticPart::BlockingRestriction(br.clone())]),
                        },
                    )?;
                }
            }
            Ok(())
        }
        StaticAbilityAst::GrantAbilityText {
            subject,
            ability_text: _,
            compiled: Some(compiled_face),
            condition,
        } => {
            let mut selectors = lower_permanent_selectors(subject, vocabulary)?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let part = lower_granted_ability_face(compiled_face, vocabulary, ids)?;
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![part.clone()]),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::SetOrAddType {
            subject,
            type_or_subtype,
            additive,
            condition,
        } => {
            let mut selectors = lower_permanent_selectors(subject, vocabulary)?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let type_change = parse_static_type_change(type_or_subtype, *additive, vocabulary)?;
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::Type(type_change.clone())]),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::ContinuousLayerEffect {
            subject,
            set_types,
            additive_types,
            base_pt,
            set_colors,
            grant_keywords,
            lose_keywords,
            lose_all_abilities,
            condition,
        } => {
            let mut selector = lower_permanent_selector(subject, vocabulary)?;
            if let Some(cond) = condition {
                apply_selector_condition(&mut selector, cond, vocabulary)?;
            }
            let mut parts = Vec::new();
            if let Some(desc) = set_types {
                parts.push(StaticPart::Type(parse_static_type_change(
                    desc,
                    *additive_types,
                    vocabulary,
                )?));
            }
            if let Some(color_str) = set_colors {
                let mut colors = BTreeSet::new();
                let mut saw_colorless = false;
                for raw_tok in color_str
                    .replace(" and ", " ")
                    .replace(',', " ")
                    .split_whitespace()
                {
                    match raw_tok.to_ascii_lowercase().as_str() {
                        "white" => {
                            colors.insert(Color::White);
                        }
                        "blue" => {
                            colors.insert(Color::Blue);
                        }
                        "black" => {
                            colors.insert(Color::Black);
                        }
                        "red" => {
                            colors.insert(Color::Red);
                        }
                        "green" => {
                            colors.insert(Color::Green);
                        }
                        "colorless" => {
                            saw_colorless = true;
                        }
                        "a" | "an" => {}
                        _ => return Err("oracle:unsupported-layer-color"),
                    }
                }
                if colors.is_empty() && !saw_colorless {
                    return Err("oracle:unsupported-layer-color");
                }
                parts.push(StaticPart::Color(ColorOperation::Set(
                    StaticColorOperand::Literal(colors),
                )));
            }
            if *lose_all_abilities || !grant_keywords.is_empty() || !lose_keywords.is_empty() {
                let mut change = if !grant_keywords.is_empty() {
                    lower_ability_change(grant_keywords, vocabulary)?
                } else {
                    AbilityChange::default()
                };
                if *lose_all_abilities {
                    change.removal = Some(AbilityRemoval::All);
                }
                if !lose_keywords.is_empty() {
                    change
                        .suppressed_keywords
                        .extend(lower_keyword_list(lose_keywords)?);
                }
                parts.push(StaticPart::Abilities(change));
            }
            if let Some((p_raw, t_raw)) = base_pt {
                let p: i64 = p_raw
                    .trim()
                    .parse()
                    .map_err(|_| "oracle:unsupported-layer-pt")?;
                let t: i64 = t_raw
                    .trim()
                    .parse()
                    .map_err(|_| "oracle:unsupported-layer-pt")?;
                parts.push(StaticPart::SetPT(NumericPair {
                    power: NumericExpression::Literal(p.into()),
                    toughness: NumericExpression::Literal(t.into()),
                }));
            }
            if parts.is_empty() {
                return Err("oracle:empty-layer-effect");
            }
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ids.static_id(),
                    selector,
                    parts: StaticParts(parts),
                },
            )
        }
        StaticAbilityAst::Compound(items) => {
            for item in items {
                lower_static(item, program, vocabulary, ids, is_spell)?;
            }
            Ok(())
        }
        StaticAbilityAst::PreventAllDamageStatic {
            combat_only,
            subject,
            by: _,
        } => {
            let rule = match subject.as_ref() {
                SubjectRefAst::SelfObject if *combat_only => StaticDamageRule::PreventSourceCombat,
                SubjectRefAst::AttachedHost(_) => StaticDamageRule::PreventEnchantedCreature {
                    combat_only: *combat_only,
                },
                _ => return Err("oracle:unsupported-prevent-static"),
            };
            push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule,
                },
            )
        }
        StaticAbilityAst::ControlAttachedHost => push_static_ability(
            program,
            StaticContinuousAbility::Battlefield {
                id: ids.static_id(),
                selector: PermanentSelector {
                    attached_to_source: true,
                    ..PermanentSelector::default()
                },
                parts: StaticParts(vec![StaticPart::ControlSourceController]),
            },
        ),
        StaticAbilityAst::CdaPowerToughness { power, toughness } => {
            let p = match power {
                Some(v) => Some(Box::new(lower_numeric_expr(v, vocabulary)?)),
                None => None,
            };
            let t = match toughness {
                Some(v) => Some(Box::new(lower_numeric_expr(v, vocabulary)?)),
                None => None,
            };
            if p.is_none() && t.is_none() {
                return Err("oracle:empty-cda-pt");
            }
            push_static_ability(
                program,
                StaticContinuousAbility::CharacteristicDefining {
                    id: ids.static_id(),
                    parts: DefiningParts(vec![DefiningPart::PowerToughness {
                        power: p,
                        toughness: t,
                    }]),
                },
            )
        }
        StaticAbilityAst::CostModifier {
            subject,
            delta,
            less: true,
            per,
            scale_expr: None,
            condition,
        } => {
            // Self spell cost reduction: "This spell costs {N} less to cast [for each ...] [if ...]"
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) {
                let generic_n = parse_generic_mana_amount(delta)
                    .ok_or("oracle:unsupported-self-discount-delta")?;
                if generic_n == 0 {
                    return Err("oracle:unsupported-self-discount-delta");
                }
                if let Some(per_filter) = per
                    && condition.is_none()
                {
                    if generic_n == 1
                        && let Ok(count) = lower_cast_cost_count(per_filter)
                    {
                        program
                            .cast_reductions
                            .push(CastReduction::PerObject(count));
                        return Ok(());
                    }
                    if per_filter.zone.is_none()
                        && per_filter.controller == Some(ControllerRelation::You)
                        && let Ok(mut sel) =
                            lower_filter_to_permanent_selector(per_filter, vocabulary)
                    {
                        sel.controller = ControllerSelector::SourceController;
                        for _ in 0..generic_n {
                            program
                                .cast_reductions
                                .push(CastReduction::Affinity(Box::new(sel.clone())));
                        }
                        return Ok(());
                    }
                    let mut inner = per_filter.as_ref().clone();
                    let zone = match inner.zone.take() {
                        None => spellwake_engine::program::ValueZone::Battlefield,
                        Some(ZoneFilter::YourGraveyard) => {
                            spellwake_engine::program::ValueZone::Graveyard(
                                ResolvingPlayer::Controller,
                            )
                        }
                        Some(ZoneFilter::YourHand) => {
                            spellwake_engine::program::ValueZone::Hand(ResolvingPlayer::Controller)
                        }
                        _ => return Err("oracle:unsupported-self-discount"),
                    };
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    program.cast_reductions.push(CastReduction::CountZone {
                        zone,
                        filter: Box::new(obj),
                        per_card: generic_n,
                    });
                    return Ok(());
                }
                if per.is_none()
                    && let Some(cond) = condition
                {
                    let mut tmp_builder = BodyBuilder::new();
                    let rc = lower_resolution_condition(
                        cond,
                        &mut tmp_builder,
                        vocabulary,
                        false,
                        None,
                    )?;
                    if tmp_builder.target != BodyTarget::None {
                        return Err("oracle:unsupported-self-discount");
                    }
                    program.cast_reductions.push(CastReduction::Conditional {
                        condition: Box::new(rc),
                        discount: generic_n,
                    });
                    return Ok(());
                }
                return Err("oracle:unsupported-self-discount");
            }
            // Battlefield permanent discounting spells
            if is_spell || per.is_some() || condition.is_some() {
                return Err("oracle:unsupported-cost-modifier");
            }
            let generic_n =
                parse_generic_mana_amount(delta).ok_or("oracle:non-generic-discount")?;
            let amount = NonZeroU8::new(generic_n).ok_or("oracle:zero-discount")?;
            let (any_player, matching) = lower_spell_subject_filter(subject, vocabulary)?;
            push_static_ability(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ids.static_id(),
                    modifier: SpellCostModifier::Discount {
                        any_player,
                        matching,
                        amount: SpellDiscount::Fixed(amount),
                    },
                },
            )
        }
        StaticAbilityAst::CostModifier {
            subject,
            delta,
            less: false,
            per: None,
            scale_expr: None,
            condition: None,
        } if !is_spell => {
            let generic_n = parse_generic_mana_amount(delta).ok_or("oracle:non-generic-tax")?;
            let amount = NonZeroU8::new(generic_n).ok_or("oracle:zero-tax")?;
            let f = match subject.as_ref() {
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => f,
                _ => return Err("oracle:unsupported-cost-tax"),
            };
            if f.any_types.is_empty()
                && f.controller.is_none()
                && f.subtypes.is_empty()
                && f.any_colors.is_empty()
                && f.forbidden_colors.is_empty()
                && f.color_arity.is_none()
            {
                let mut forb = card_type_mask_to_set(f.forbidden_types)?;
                if forb.len() == 1
                    && let Some(exempt) = forb.pop_first()
                {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::CostModifier {
                            id: ids.static_id(),
                            modifier: SpellCostModifier::IncreaseExceptType { exempt, amount },
                        },
                    );
                }
            }
            let (any_player, opponents_only) = match f.controller {
                None => (true, false),
                Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => (false, true),
                Some(ControllerRelation::You) => (false, false),
                _ => return Err("oracle:unsupported-cost-modifier"),
            };
            let matching = lower_spell_filter(f, vocabulary)?;
            push_static_ability(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ids.static_id(),
                    modifier: SpellCostModifier::Increase {
                        any_player,
                        opponents_only,
                        matching,
                        amount,
                    },
                },
            )
        }
        StaticAbilityAst::PlayOrCastPermission {
            cast_only: _,
            filter,
            permission,
            condition: None,
        } => {
            let p_low = permission.to_ascii_lowercase();
            if p_low.contains("flash") {
                let any_player = filter.controller.is_none();
                let matching = lower_spell_filter(filter, vocabulary)?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::Flash {
                            any_player,
                            matching,
                        },
                    },
                );
            }
            if p_low.contains("additional land") {
                let n: u8 = if p_low.contains("two") || p_low.contains('2') {
                    2
                } else if p_low.contains("three") || p_low.contains('3') {
                    3
                } else {
                    1
                };
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::AdditionalLandPlays(
                            NonZeroU8::new(n).ok_or("oracle:unreachable")?,
                        ),
                    },
                );
            }
            if p_low.contains("top of your library") {
                if p_low.contains("look at the top") {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::CastPermission {
                            id: ids.static_id(),
                            permission: CastPermissionRule::SeeLibraryTop { public: false },
                        },
                    );
                }
                if p_low.contains("revealed") {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::CastPermission {
                            id: ids.static_id(),
                            permission: CastPermissionRule::SeeLibraryTop { public: true },
                        },
                    );
                }
                if filter.any_types == AstCardTypeMask::LAND && filter.forbidden_types.is_empty() {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::CastPermission {
                            id: ids.static_id(),
                            permission: CastPermissionRule::PlayLibraryTop(
                                LibraryTopPermission::Land,
                            ),
                        },
                    );
                }
                if filter.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY)
                    && filter.forbidden_types.is_empty()
                {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::CastPermission {
                            id: ids.static_id(),
                            permission: CastPermissionRule::PlayLibraryTop(
                                LibraryTopPermission::InstantOrSorcery,
                            ),
                        },
                    );
                }
            }
            if p_low.contains("from your graveyard")
                && filter.any_types == AstCardTypeMask::CREATURE
                && filter.forbidden_types.is_empty()
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::GraveyardCreatureCast,
                    },
                );
            }
            Err("oracle:unsupported-cast-permission")
        }
        StaticAbilityAst::AdditionalCastCost { cost } => match cost {
            CostAtomAst::Sacrifice { count, filter }
                if matches!(count.as_ref(), ValueExprAst::Const(1))
                    && filter.any_types == AstCardTypeMask::CREATURE
                    && filter.subtypes.is_empty()
                    && filter.forbidden_types.is_empty() =>
            {
                if program.sacrifice_creature_cast_cost {
                    return Err("oracle:duplicate-additional-cost");
                }
                program.sacrifice_creature_cast_cost = true;
                Ok(())
            }
            CostAtomAst::Discard {
                count,
                filter,
                random: false,
            } if matches!(count.as_ref(), ValueExprAst::Const(1))
                && filter.any_types.is_empty()
                && filter.forbidden_types.is_empty() =>
            {
                if program.discard_cast_cost {
                    return Err("oracle:duplicate-additional-cost");
                }
                program.discard_cast_cost = true;
                Ok(())
            }
            CostAtomAst::PayLife(amount) => {
                let expr = lower_value_expr(amount, vocabulary)?;
                if program.additional_life_cost.replace(expr).is_some() {
                    return Err("oracle:duplicate-additional-cost");
                }
                Ok(())
            }
            _ => Err("oracle:unsupported-additional-cast-cost"),
        },
        StaticAbilityAst::RuleOrCombatRestriction {
            subject,
            rule,
            filter,
            condition,
        } => {
            if condition.is_none()
                && (rule.eq_ignore_ascii_case("cantbecountered")
                    || rule.eq_ignore_ascii_case("can't be countered"))
                && filter.is_none()
            {
                match subject.as_deref() {
                    None | Some(SubjectRefAst::SelfObject) => {
                        program.cannot_be_countered = true;
                        return Ok(());
                    }
                    Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f),
                    ) if f.controller == Some(ControllerRelation::You)
                        && f.forbidden_types.is_empty()
                        && f.subtypes.is_empty()
                        && f.any_colors.is_empty()
                        && f.forbidden_colors.is_empty()
                        && f.color_arity.is_none() =>
                    {
                        let types = card_type_mask_to_set(f.any_types)?;
                        if !types.is_empty() {
                            return push_static_ability(
                                program,
                                StaticContinuousAbility::CastPermission {
                                    id: ids.static_id(),
                                    permission:
                                        CastPermissionRule::ControllerSpellsCannotBeCountered(types),
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }
            if condition.is_none() && rule == "DoesntUntap" && filter.is_none() {
                let untap_rule = match subject.as_deref() {
                    Some(SubjectRefAst::SelfObject) | None => StaticUntapRule::SourceSkipsUntap,
                    Some(SubjectRefAst::AttachedHost(_)) => {
                        StaticUntapRule::EnchantedCreatureSkipsUntap
                    }
                    _ => return Err("oracle:unsupported-untap-rule"),
                };
                return push_static_ability(
                    program,
                    StaticContinuousAbility::UntapRule {
                        id: ids.static_id(),
                        rule: untap_rule,
                    },
                );
            }
            if condition.is_none() && rule == "UntapEachOtherUntapStep" && filter.is_none() {
                match subject.as_deref() {
                    None | Some(SubjectRefAst::SelfObject) => {
                        return push_static_ability(
                            program,
                            StaticContinuousAbility::UntapRule {
                                id: ids.static_id(),
                                rule: StaticUntapRule::UntapDuringOtherPlayersUntap,
                            },
                        );
                    }
                    Some(subj) => {
                        let selectors = lower_permanent_selectors(subj, vocabulary)?;
                        for sel in selectors {
                            push_static_ability(
                                program,
                                StaticContinuousAbility::UntapRule {
                                    id: ids.static_id(),
                                    rule: StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(
                                        Box::new(sel),
                                    ),
                                },
                            )?;
                        }
                        return Ok(());
                    }
                }
            }
            if condition.is_none()
                && rule == "CantAttackYouOrBlockYourCreatures"
                && filter.is_none()
            {
                if program
                    .attack_requirement
                    .replace(AttackRequirement::EnchantedCannotAttackOrBlockController)
                    .is_some()
                {
                    return Err("oracle:duplicate-attack-req");
                }
                return Ok(());
            }
            if rule.eq_ignore_ascii_case("can't block and can't be blocked") && filter.is_none() {
                let mut selector = match subject.as_deref() {
                    Some(s) => lower_permanent_selector(s, vocabulary)?,
                    None => PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    },
                };
                if let Some(cond) = condition {
                    apply_selector_condition(&mut selector, cond, vocabulary)?;
                }
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: selector.clone(),
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotBlock,
                        )]),
                    },
                )?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotBeBlocked,
                        )]),
                    },
                );
            }
            if rule.eq_ignore_ascii_case("can't attack or block alone") && filter.is_none() {
                let mut selector = match subject.as_deref() {
                    Some(s) => lower_permanent_selector(s, vocabulary)?,
                    None => PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    },
                };
                if let Some(cond) = condition {
                    apply_selector_condition(&mut selector, cond, vocabulary)?;
                }
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: selector.clone(),
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotAttackAlone,
                        )]),
                    },
                )?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotBlockAlone,
                        )]),
                    },
                );
            }
            if let Some(br) = lower_rule_blocking_restriction(rule, filter.as_deref(), vocabulary) {
                let mut selector = match subject.as_deref() {
                    Some(s) => lower_permanent_selector(s, vocabulary)?,
                    None => PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    },
                };
                if let Some(cond) = condition {
                    apply_selector_condition(&mut selector, cond, vocabulary)?;
                }
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(br)]),
                    },
                );
            }
            if condition.is_none()
                && rule == "ActivatedAbilitiesCantBeActivated"
                && subject.is_none()
                && let Some(flt) = filter.as_deref()
            {
                let selector = lower_filter_to_permanent_selector(flt, vocabulary)?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PreventMatchingActivations(Box::new(selector)),
                    },
                );
            }
            if filter.is_some() {
                return Err("oracle:unsupported-rule-restriction");
            }
            if condition.is_some() {
                return Err("oracle:unsupported-static");
            }
            let r_low = rule.to_ascii_lowercase();
            if (r_low.contains("can't be countered") || r_low == "cantbecountered")
                && matches!(subject.as_deref(), None | Some(SubjectRefAst::SelfObject))
            {
                program.cannot_be_countered = true;
                return Ok(());
            }
            if r_low == "activatedabilitiescantbeactivated"
                && let Some(subj) = subject.as_deref()
            {
                let selector = lower_permanent_selector(subj, vocabulary)?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PreventMatchingActivations(Box::new(selector)),
                    },
                );
            }
            if r_low == "can't attack or block, and its activated abilities can't be activated"
                && let Some(subj) = subject.as_deref()
            {
                let selector = lower_permanent_selector(subj, vocabulary)?;
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: selector.clone(),
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotAttackOrBlock,
                        )]),
                    },
                )?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PreventMatchingActivations(Box::new(selector)),
                    },
                );
            }
            if r_low == "you may look at the top card of your library any time" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::SeeLibraryTop { public: false },
                    },
                );
            }
            if r_low == "play with the top card of your library revealed" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::SeeLibraryTop { public: true },
                    },
                );
            }
            if r_low == "you may play an additional land on each of your turns" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::AdditionalLandPlays(
                            NonZeroU8::new(1).ok_or("oracle:unreachable")?,
                        ),
                    },
                );
            }
            if r_low.contains("no maximum hand size") {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DrawRule {
                        id: ids.static_id(),
                        rule: StaticDrawRule::NoMaximumHandSize,
                    },
                );
            }
            if r_low == "each player can't cast more than one spell each turn" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OneSpellPerTurn,
                    },
                );
            }
            if r_low == "entertriggerssuppressed" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::SuppressEntersTriggers,
                    },
                );
            }
            if r_low == "legendruleexempt" {
                let rule = match subject.as_deref() {
                    None => StaticModifierRule::IgnoreLegendRule,
                    Some(SubjectRefAst::All(f) | SubjectRefAst::Group(f))
                        if f.controller == Some(ControllerRelation::You)
                            && f.subtypes.is_empty()
                            && f.forbidden_types.is_empty()
                            && f.any_types.is_empty() =>
                    {
                        StaticModifierRule::IgnoreLegendRuleForController
                    }
                    Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Group(f) | SubjectRefAst::Each(f),
                    ) if f.subtypes.len() == 1
                        && f.forbidden_types.is_empty()
                        && let Some(ct) = vocabulary.resolve_label(&f.subtypes[0]) =>
                    {
                        StaticModifierRule::IgnoreLegendForSubtype(ct)
                    }
                    _ => return Err("oracle:unsupported-rule-restriction"),
                };
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Modifier {
                        id: ids.static_id(),
                        rule,
                    },
                );
            }
            if r_low == "players can't gain life" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PlayersCannotGainLife,
                    },
                );
            }
            if r_low.contains("opponents can't gain life") {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OpponentsCannotGainLife,
                    },
                );
            }
            if r_low.contains("you have hexproof") {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerHasHexproof,
                    },
                );
            }
            if r_low == "players can't cast spells from graveyards or libraries" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::RestrictGraveyardAndLibrary,
                    },
                );
            }
            if r_low
                == "players can't pay life or sacrifice creatures to cast spells or activate abilities"
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ProhibitLifeAndCreatureCosts,
                    },
                );
            }
            if r_low.contains(
                "spells and abilities your opponents control can't cause you to sacrifice",
            ) {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ProhibitOpponentsCausingSacrifice,
                    },
                );
            }
            if r_low == "activated abilities of artifacts can't be activated" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PreventArtifactActivations,
                    },
                );
            }
            if r_low == "activated abilities of planeswalkers can't be activated" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PreventLoyaltyActivations,
                    },
                );
            }
            if r_low == "your opponents can't cast spells during your turn" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::RestrictOpponentsDuringOwnTurn,
                    },
                );
            }
            if r_low == "each opponent can't draw more than one card each turn" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DrawRule {
                        id: ids.static_id(),
                        rule: StaticDrawRule::OpponentsAtMostOne,
                    },
                );
            }
            if r_low == "skip your draw step" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DrawRule {
                        id: ids.static_id(),
                        rule: StaticDrawRule::SkipControllerDrawStep,
                    },
                );
            }
            if r_low == "damage that would be dealt by ~ can't be prevented"
                || (r_low == "damage can't be prevented"
                    && matches!(subject.as_deref(), Some(SubjectRefAst::SelfObject)))
            {
                program
                    .printed_keywords
                    .insert(KeywordAbility::DamageCannotBePrevented);
                return Ok(());
            }
            if r_low.contains("doesn't untap during its controller's untap step")
                || r_low.contains("doesn't untap during your untap step")
            {
                let untap_rule = match subject.as_deref() {
                    Some(SubjectRefAst::SelfObject) | None => StaticUntapRule::SourceSkipsUntap,
                    Some(SubjectRefAst::AttachedHost(_)) => {
                        StaticUntapRule::EnchantedCreatureSkipsUntap
                    }
                    _ => return Err("oracle:unsupported-untap-rule"),
                };
                return push_static_ability(
                    program,
                    StaticContinuousAbility::UntapRule {
                        id: ids.static_id(),
                        rule: untap_rule,
                    },
                );
            }
            if rule == "AttacksEachCombatIfAble"
                || r_low.contains("attacks each combat if able")
                || r_low.contains("attacks each turn if able")
            {
                let req = match subject.as_deref() {
                    Some(SubjectRefAst::SelfObject) | None => AttackRequirement::SourceAttacks,
                    Some(SubjectRefAst::AttachedHost(_)) => {
                        AttackRequirement::AttachedCreatureAttacks
                    }
                    Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f),
                    ) if f.any_types == AstCardTypeMask::CREATURE
                        && f.forbidden_types.is_empty()
                        && f.controller.is_none()
                        && f.subtypes.is_empty()
                        && f.any_colors.is_empty()
                        && f.forbidden_colors.is_empty()
                        && f.flags.is_empty() =>
                    {
                        AttackRequirement::AllCreaturesAttack
                    }
                    _ => return Err("oracle:unsupported-attack-req"),
                };
                if program.attack_requirement.replace(req).is_some() {
                    return Err("oracle:duplicate-attack-req");
                }
                return Ok(());
            }
            Err("oracle:unsupported-rule-restriction")
        }
        StaticAbilityAst::ConspiracyAgenda {
            hidden,
            named_slots,
        } => push_static_ability(
            program,
            StaticContinuousAbility::CommandZoneActive {
                id: ids.static_id(),
                agenda: spellwake_engine::characteristics::AgendaSpec {
                    hidden: *hidden,
                    named_slots: *named_slots,
                },
            },
        ),
        _ => Err("oracle:unsupported-static"),
    }
}

fn lower_granted_ability_face(
    compiled_face: &FaceAst,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<StaticPart, &'static str> {
    if !compiled_face.failures.is_empty()
        || !compiled_face.statics.is_empty()
        || !compiled_face.replacements.is_empty()
        || !compiled_face.keywords.is_empty()
        || !compiled_face.spell_effects.is_empty()
        || compiled_face.abilities.len() != 1
    {
        return Err("oracle:unsupported-grant-ability-text");
    }
    match &compiled_face.abilities[0] {
        AbilityAst::Activated {
            ability_word: _,
            zone: None,
            cost,
            body,
        } => {
            if let Some(mana_out) = try_lower_mana_ability(cost, body, vocabulary) {
                match mana_out {
                    ManaOutput::ChooseColor { amount, colors }
                        if amount == 1.into() && colors == ManaOutput::all_colors() =>
                    {
                        Ok(StaticPart::GrantAnyColorMana)
                    }
                    ManaOutput::Fixed(bundle)
                        if bundle.len() == 1 && bundle.values().next() == Some(&1.into()) =>
                    {
                        let mt = *bundle.keys().next().ok_or("oracle:unreachable")?;
                        Ok(StaticPart::GrantFixedColorMana(mt))
                    }
                    _ => Err("oracle:unsupported-granted-mana-ability"),
                }
            } else {
                let (mana_cost, source_cost) = lower_activation_cost(cost, None, vocabulary)?;
                let (exec_body, restriction) = lower_activated_body(body, vocabulary, ids)?;
                if restriction.is_some() {
                    return Err("oracle:unsupported-granted-act-restriction");
                }
                Ok(StaticPart::GrantActivated(Box::new(ActivatedAbility {
                    id: ActivatedAbilityId(0),
                    mana_cost,
                    source_cost,
                    body: exec_body,
                })))
            }
        }
        AbilityAst::Triggered {
            ability_word: _,
            trigger,
            body,
        } => {
            let (mut spec, mut grouping, guard) = lower_trigger_spec(trigger, vocabulary)?;
            let self_pronoun = trigger_subject_is_self(trigger);
            let stripped_body = strip_triggered_restrictions(body, &mut spec, &mut grouping);
            let mut exec_body =
                lower_effect_body(&stripped_body, vocabulary, ids, true, self_pronoun)?;
            if let Some(cond) = guard {
                exec_body
                    .instructions
                    .insert(0, Instruction::RequireCondition(cond));
            }
            apply_trigger_body_rewrites(&mut exec_body, &spec, self_pronoun);
            Ok(StaticPart::GrantTrigger(Box::new(TriggeredAbility {
                id: ids.triggered(),
                condition: spec,
                grouping,
                body: exec_body,
            })))
        }
        _ => Err("oracle:unsupported-grant-ability-text"),
    }
}

fn parse_static_type_change(
    raw: &str,
    additive: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<StaticTypeChange, &'static str> {
    let clean = raw
        .replace(" in addition to its other types", "")
        .replace(" in addition to their other types", "")
        .replace(" and ", " ")
        .replace(',', " ");
    let mut supertypes = BTreeSet::new();
    let mut card_types = BTreeSet::new();
    let mut creature_subtypes = BTreeSet::new();
    let mut land_subtypes = BTreeSet::new();
    for raw_tok in clean.split_whitespace() {
        let tok = raw_tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'');
        if tok.is_empty() {
            continue;
        }
        match tok.to_ascii_lowercase().as_str() {
            "a" | "an" | "the" | "type" | "types" | "is" | "are" | "becomes" | "become" => {}
            "legendary" => {
                supertypes.insert(Supertype::Legendary);
            }
            "snow" => {
                supertypes.insert(Supertype::Snow);
            }
            "basic" => {
                supertypes.insert(Supertype::Basic);
            }
            "creature" | "creatures" => {
                card_types.insert(CardType::Creature);
            }
            "artifact" | "artifacts" => {
                card_types.insert(CardType::Artifact);
            }
            "enchantment" | "enchantments" => {
                card_types.insert(CardType::Enchantment);
            }
            "land" | "lands" => {
                card_types.insert(CardType::Land);
            }
            "planeswalker" | "planeswalkers" => {
                card_types.insert(CardType::Planeswalker);
            }
            other => {
                let singular = other.strip_suffix('s').unwrap_or(other);
                if let Some(lt) = parse_land_subtype(tok).or_else(|| {
                    let cap = {
                        let mut ch = singular.chars();
                        ch.next()
                            .map(|c| c.to_uppercase().collect::<String>() + ch.as_str())
                            .unwrap_or_default()
                    };
                    parse_land_subtype(&cap)
                }) {
                    land_subtypes.insert(lt);
                } else if let Some(ct) = vocabulary.resolve_label(tok).or_else(|| {
                    let cap = {
                        let mut ch = singular.chars();
                        ch.next()
                            .map(|c| c.to_uppercase().collect::<String>() + ch.as_str())
                            .unwrap_or_default()
                    };
                    vocabulary.resolve_label(&cap)
                }) {
                    creature_subtypes.insert(ct);
                } else {
                    return Err("oracle:unsupported-type-change");
                }
            }
        }
    }
    if supertypes.is_empty()
        && card_types.is_empty()
        && creature_subtypes.is_empty()
        && land_subtypes.is_empty()
    {
        return Err("oracle:unsupported-type-change");
    }
    Ok(StaticTypeChange {
        supertypes: if supertypes.is_empty() {
            TypeOperation::Keep
        } else if additive {
            TypeOperation::Add(supertypes)
        } else {
            TypeOperation::Set(supertypes)
        },
        types: if card_types.is_empty() {
            TypeOperation::Keep
        } else if additive {
            TypeOperation::Add(card_types)
        } else {
            TypeOperation::Set(card_types)
        },
        subtypes: if creature_subtypes.is_empty() {
            SubtypeOperation::Keep
        } else if additive {
            SubtypeOperation::Add(StaticSubtypeOperand::Literal(creature_subtypes))
        } else {
            SubtypeOperation::Set(StaticSubtypeOperand::Literal(creature_subtypes))
        },
        land_subtypes: if land_subtypes.is_empty() {
            SubtypeOperation::Keep
        } else if additive {
            SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(land_subtypes))
        } else {
            SubtypeOperation::Set(StaticLandSubtypeOperand::Literal(land_subtypes))
        },
    })
}

fn lower_rule_blocking_restriction(
    rule: &str,
    filter: Option<&ObjectFilterAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<BlockingRestriction> {
    if let Some(flt) = filter {
        if rule == "CanBlockOnly"
            && flt.required_keywords == ["flying"]
            && flt.any_types == AstCardTypeMask::CREATURE
            && flt.forbidden_types.is_empty()
            && flt.subtypes.is_empty()
            && flt.any_colors.is_empty()
            && flt.forbidden_colors.is_empty()
            && flt.stat_bounds.is_empty()
        {
            return Some(BlockingRestriction::CanBlockOnlyFlying);
        }
        if rule == "CantBeBlockedBy"
            && flt.color_arity.is_none()
            && flt.forbidden_keywords.is_empty()
            && flt.forbidden_subtypes.is_empty()
            && flt.controller.is_none()
        {
            let plain_flags = flt.flags.is_empty();
            if plain_flags
                && flt.forbidden_types.is_empty()
                && flt.any_colors.is_empty()
                && flt.forbidden_colors.is_empty()
            {
                if flt.required_keywords == ["flying"]
                    && flt.subtypes.is_empty()
                    && flt.stat_bounds.is_empty()
                {
                    return Some(BlockingRestriction::CannotBeBlockedByFlying);
                }
                if flt.subtypes.len() == 1
                    && flt.required_keywords.is_empty()
                    && flt.stat_bounds.is_empty()
                    && let Some(ct) = vocabulary.resolve_label(&flt.subtypes[0])
                {
                    return Some(BlockingRestriction::CannotBeBlockedBySubtype(ct));
                }
                if flt.stat_bounds.len() == 1
                    && flt.subtypes.is_empty()
                    && flt.required_keywords.is_empty()
                    && let ValueExprAst::Const(v) = flt.stat_bounds[0].value.as_ref()
                    && let Ok(u) = u16::try_from(*v)
                {
                    match (flt.stat_bounds[0].stat, flt.stat_bounds[0].op) {
                        (StatKind::Toughness, CompareOp::Ge) if u > 0 => {
                            return Some(BlockingRestriction::CannotBeBlockedByToughnessAtLeast(u));
                        }
                        (StatKind::Power, CompareOp::Le) => {
                            return Some(BlockingRestriction::CannotBeBlockedByPowerAtMost(u));
                        }
                        (StatKind::Power, CompareOp::Lt) if u > 0 => {
                            return Some(BlockingRestriction::CannotBeBlockedByPowerAtMost(u - 1));
                        }
                        (StatKind::Power, CompareOp::Ge) => {
                            return Some(BlockingRestriction::CannotBeBlockedByPowerAtLeast(u));
                        }
                        (StatKind::Power, CompareOp::Gt) => {
                            return Some(BlockingRestriction::CannotBeBlockedByPowerAtLeast(
                                u.saturating_add(1),
                            ));
                        }
                        _ => {}
                    }
                }
            }
            let single_color = |mask: AstColorMask| -> Option<Color> {
                match mask {
                    m if m == AstColorMask::WHITE => Some(Color::White),
                    m if m == AstColorMask::BLUE => Some(Color::Blue),
                    m if m == AstColorMask::BLACK => Some(Color::Black),
                    m if m == AstColorMask::RED => Some(Color::Red),
                    m if m == AstColorMask::GREEN => Some(Color::Green),
                    _ => None,
                }
            };
            if plain_flags
                && flt.forbidden_types.is_empty()
                && flt.forbidden_colors.is_empty()
                && flt.subtypes.is_empty()
                && flt.required_keywords.is_empty()
                && flt.stat_bounds.is_empty()
                && let Some(c) = single_color(flt.any_colors)
            {
                return Some(BlockingRestriction::CannotBeBlockedByColor(c));
            }
            if flt.token == Some(true)
                && plain_flags
                && flt.forbidden_types.is_empty()
                && flt.any_colors.is_empty()
                && flt.forbidden_colors.is_empty()
                && flt.subtypes.is_empty()
                && flt.required_keywords.is_empty()
                && flt.stat_bounds.is_empty()
            {
                return Some(BlockingRestriction::CannotBeBlockedByTokens);
            }
            if plain_flags
                && flt.forbidden_types == AstCardTypeMask::ARTIFACT
                && flt.any_colors.is_empty()
                && flt.forbidden_colors.is_empty()
                && flt.subtypes.is_empty()
                && flt.required_keywords.is_empty()
                && flt.stat_bounds.is_empty()
            {
                return Some(BlockingRestriction::CannotBeBlockedByNonArtifact);
            }
        }
        if rule == "CantBeBlockedExceptBy"
            && flt.forbidden_types.is_empty()
            && flt.forbidden_colors.is_empty()
            && flt.color_arity.is_none()
            && flt.forbidden_keywords.is_empty()
            && flt.forbidden_subtypes.is_empty()
            && flt.controller.is_none()
            && flt.stat_bounds.is_empty()
            && flt.flags.is_empty()
        {
            let single_color = |mask: AstColorMask| -> Option<Color> {
                match mask {
                    m if m == AstColorMask::WHITE => Some(Color::White),
                    m if m == AstColorMask::BLUE => Some(Color::Blue),
                    m if m == AstColorMask::BLACK => Some(Color::Black),
                    m if m == AstColorMask::RED => Some(Color::Red),
                    m if m == AstColorMask::GREEN => Some(Color::Green),
                    _ => None,
                }
            };
            let has_artifact = (flt.any_types.0 & AstCardTypeMask::ARTIFACT.0) != 0;
            if flt.any_colors.is_empty() {
                if flt.subtypes.len() == 1
                    && flt.required_keywords.is_empty()
                    && let Some(ct) = vocabulary.resolve_label(&flt.subtypes[0])
                {
                    return Some(BlockingRestriction::OnlyBlockedBySubtype(ct));
                }
                if flt.subtypes.is_empty()
                    && ((flt.required_keywords.len() == 2
                        && flt.required_keywords.iter().any(|k| k == "flying")
                        && flt.required_keywords.iter().any(|k| k == "reach"))
                        || flt.required_keywords == ["flying or reach"])
                {
                    return Some(BlockingRestriction::OnlyBlockedByFlyingOrReach);
                }
                if flt.subtypes.is_empty() && flt.required_keywords.is_empty() && has_artifact {
                    return Some(BlockingRestriction::OnlyBlockedByArtifact);
                }
            } else if let Some(c) = single_color(flt.any_colors)
                && flt.subtypes.is_empty()
                && flt.required_keywords.is_empty()
            {
                if has_artifact {
                    return Some(BlockingRestriction::OnlyBlockedByColorOrArtifact(c));
                }
                return Some(BlockingRestriction::OnlyBlockedByColor(c));
            }
        }
        if rule == "CantBlock"
            && flt.subtypes.len() == 1
            && flt.forbidden_types.is_empty()
            && flt.any_colors.is_empty()
            && flt.forbidden_colors.is_empty()
            && flt.required_keywords.is_empty()
            && flt.forbidden_keywords.is_empty()
            && flt.stat_bounds.is_empty()
            && flt.controller.is_none()
            && flt.flags.is_empty()
            && let Some(ct) = vocabulary.resolve_label(&flt.subtypes[0])
        {
            return Some(BlockingRestriction::CannotBlockSubtype(ct));
        }
        if let Ok(obj) = lower_object_filter(flt, vocabulary) {
            match rule {
                "CantBeBlockedBy" => {
                    return Some(BlockingRestriction::CannotBeBlockedByMatching(obj));
                }
                "CantBeBlockedExceptBy" => {
                    return Some(BlockingRestriction::OnlyBlockedByMatching(obj));
                }
                "CantBlock" => {
                    return Some(BlockingRestriction::CannotBlockMatching(obj));
                }
                _ => {}
            }
        }
        return None;
    }
    lower_blocking_restriction(rule)
}

fn lower_blocking_restriction(rule: &str) -> Option<BlockingRestriction> {
    match rule.to_ascii_lowercase().as_str() {
        "can't block" | "cant_block" | "cantblock" => Some(BlockingRestriction::CannotBlock),
        "can't attack" | "cant_attack" | "cantattack" => Some(BlockingRestriction::CannotAttack),
        "can't attack or block" | "cant_attack_or_block" | "cantattackorblock" => {
            Some(BlockingRestriction::CannotAttackOrBlock)
        }
        "can't attack alone" | "cant_attack_alone" | "cantattackalone" => {
            Some(BlockingRestriction::CannotAttackAlone)
        }
        "can't block alone" | "cant_block_alone" | "cantblockalone" => {
            Some(BlockingRestriction::CannotBlockAlone)
        }
        "assigns combat damage equal to its toughness rather than its power" => {
            Some(BlockingRestriction::AssignDamageByToughness)
        }
        "can block any number of creatures" => Some(BlockingRestriction::CanBlockAnyNumber),
        "can attack as though they didn't have defender"
        | "can attack as though it didn't have defender" => {
            Some(BlockingRestriction::CanAttackAsThoughNoDefender)
        }
        "you may have ~ assign its combat damage as though it weren't blocked" => {
            Some(BlockingRestriction::AssignDamageAsThoughUnblocked)
        }
        "atmostoneblocker"
        | "at_most_one_blocker"
        | "can't be blocked by more than one creature" => {
            Some(BlockingRestriction::AtMostOneBlocker)
        }
        "can't be blocked" | "cant_be_blocked" | "cantbeblocked" | "cantbeblockedif"
        | "unblockable" => Some(BlockingRestriction::CannotBeBlocked),
        "can block only creatures with flying" | "only_block_flying" => {
            Some(BlockingRestriction::CanBlockOnlyFlying)
        }
        "must be blocked if able" | "must_be_blocked" | "mustbeblocked" => {
            Some(BlockingRestriction::MustBeBlocked)
        }
        "allableblock" => Some(BlockingRestriction::AllDefendingCreaturesMustBlock),
        "minblockers:three or more creatures" => Some(BlockingRestriction::AtLeastThreeBlockers),
        "can block an additional creature each combat"
        | "additional_block"
        | "block_additional:creature each combat" => Some(BlockingRestriction::AdditionalBlock),
        _ => None,
    }
}

fn parse_generic_mana_amount(delta: &str) -> Option<u8> {
    let inner = delta.trim().strip_prefix('{')?.strip_suffix('}')?;
    inner.parse::<u8>().ok()
}

fn lower_cast_cost_count(f: &ObjectFilterAst) -> Result<CastCostCount, &'static str> {
    if f.zone == Some(ZoneFilter::YourGraveyard) {
        if f.any_types == AstCardTypeMask::CREATURE && f.forbidden_types.is_empty() {
            return Ok(CastCostCount::OwnGraveyardCreatures);
        }
        if f.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY)
            && f.forbidden_types.is_empty()
        {
            return Ok(CastCostCount::OwnGraveyardInstantsOrSorceries);
        }
    }
    if f.zone.is_none() && f.any_types == AstCardTypeMask::CREATURE && f.forbidden_types.is_empty()
    {
        if f.combat == Some(CombatState::Attacking) && f.controller.is_none() {
            return Ok(CastCostCount::AttackingCreatures);
        }
        if f.controller == Some(ControllerRelation::You)
            && f.counter_kind
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case("+1/+1"))
        {
            return Ok(CastCostCount::ControlledCreaturesWithPlusOneCounters);
        }
        if f.controller.is_none()
            && f.combat.is_none()
            && f.flags.is_empty()
            && f.counter_kind.is_none()
        {
            return Ok(CastCostCount::BattlefieldCreatures);
        }
    }
    Err("oracle:unsupported-cast-cost-count")
}

fn lower_keyword_list(kws: &[String]) -> Result<BTreeSet<KeywordAbility>, &'static str> {
    let mut out = BTreeSet::new();
    for k in kws {
        let ka = lower_keyword_ability_name(k).ok_or("oracle:unsupported-granted-keyword")?;
        out.insert(ka);
    }
    Ok(out)
}

fn lower_ability_change(
    kws: &[String],
    vocabulary: &CreatureTypeVocabulary,
) -> Result<AbilityChange, &'static str> {
    if kws.is_empty() {
        return Err("oracle:empty-grant-keywords");
    }
    let mut change = AbilityChange::default();
    for raw in kws {
        let low = raw.trim().to_ascii_lowercase();
        if let Some(ka) = lower_keyword_ability_name(&low) {
            change.granted_keywords.insert(ka);
            continue;
        }
        if let Some(prot) = low.strip_prefix("protection from ") {
            if matches!(prot.trim(), "each color" | "all colors") {
                change
                    .granted_keywords
                    .insert(KeywordAbility::ProtectionFromWhite);
                change
                    .granted_keywords
                    .insert(KeywordAbility::ProtectionFromBlue);
                change
                    .granted_keywords
                    .insert(KeywordAbility::ProtectionFromBlack);
                change
                    .granted_keywords
                    .insert(KeywordAbility::ProtectionFromRed);
                change
                    .granted_keywords
                    .insert(KeywordAbility::ProtectionFromGreen);
                continue;
            }
            for part in prot.split(" and ") {
                let word = part.trim().strip_prefix("from ").unwrap_or(part.trim());
                let ka = match word {
                    "white" => KeywordAbility::ProtectionFromWhite,
                    "blue" => KeywordAbility::ProtectionFromBlue,
                    "black" => KeywordAbility::ProtectionFromBlack,
                    "red" => KeywordAbility::ProtectionFromRed,
                    "green" => KeywordAbility::ProtectionFromGreen,
                    "multicolored" => KeywordAbility::ProtectionFromMulticolored,
                    _ => {
                        let (base_word, creatures_only) =
                            if let Some(prefix) = word.strip_suffix(" creatures") {
                                (prefix.trim(), true)
                            } else {
                                (word, false)
                            };
                        let resolved = resolve_protection_subtype(base_word, vocabulary)
                            .ok_or("oracle:unsupported-granted-keyword")?;
                        KeywordAbility::ProtectionFromSubtype(SubtypeProtection {
                            subtype: resolved,
                            creatures_only,
                            exclude_subtype: false,
                        })
                    }
                };
                change.granted_keywords.insert(ka);
            }
            continue;
        }
        match low.as_str() {
            "can't be blocked" | "unblockable" => {
                change.grants_unblockable = true;
            }
            "exalted" => {
                change.granted_exalted = change
                    .granted_exalted
                    .checked_add(1)
                    .ok_or("oracle:exalted-ovf")?;
            }
            "prowess" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Prowess),
            "flanking" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Flanking),
            "battle cry" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::BattleCry),
            "melee" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Melee),
            "dethrone" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Dethrone),
            "training" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Training),
            "ingest" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Ingest),
            "myriad" if change.granted_trigger.is_none() => {
                change.granted_trigger = Some(Box::new(TriggeredAbility {
                    id: TriggeredAbilityId(0),
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
                            Instruction::Myriad,
                            Instruction::ScheduleCreatedTokenExile(Step::EndCombat),
                        ],
                    ),
                }));
            }
            "evolve" if change.granted_trigger.is_none() => {
                change.granted_trigger = Some(Box::new(
                    GrowthKeyword::Evolve.ability(TriggeredAbilityId(0)),
                ));
            }
            "undying" if change.granted_trigger.is_none() => {
                change.granted_trigger = Some(Box::new(
                    CounterKeyword::Undying.ability(TriggeredAbilityId(0)),
                ));
            }
            "persist" if change.granted_trigger.is_none() => {
                change.granted_trigger = Some(Box::new(
                    CounterKeyword::Persist.ability(TriggeredAbilityId(0)),
                ));
            }
            _ => {
                if let Some(n_str) = low.strip_prefix("toxic ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                    && n >= 0
                {
                    change.granted_toxic = Some(match &change.granted_toxic {
                        Some(prev) => prev.plus(&n.into()),
                        None => n.into(),
                    });
                } else if let Some(n_str) = low.strip_prefix("afflict ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Afflict(n.into()));
                } else if let Some(n_str) = low.strip_prefix("bushido ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Bushido(n.into()));
                } else if let Some(n_str) = low.strip_prefix("frenzy ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Frenzy(n.into()));
                } else if let Some(n_str) = low.strip_prefix("mobilize ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Mobilize(n.into()));
                } else if let Some(n_str) = low.strip_prefix("poisonous ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Poisonous(n.into()));
                } else if let Some(n_str) = low.strip_prefix("rampage ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Rampage(n.into()));
                } else if let Some(n_str) = low.strip_prefix("renown ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Renown(n.into()));
                } else if let Some(cost_str) = low.strip_prefix("ward ")
                    && change.granted_trigger.is_none()
                    && let Some(cost) = parse_fixed_mana_cost(cost_str.trim())
                {
                    change.granted_trigger = Some(Box::new(TriggeredAbility::ward(
                        TriggeredAbilityId(0),
                        cost,
                    )));
                } else {
                    return Err("oracle:unsupported-granted-keyword");
                }
            }
        }
    }
    Ok(change)
}

fn parse_signed_pt_delta(raw: &str) -> Option<i64> {
    let t = raw.trim().replace('\u{2212}', "-");
    let t = t.strip_prefix('+').unwrap_or(&t);
    t.parse::<i64>().ok()
}

fn lower_pt_delta_pair(
    dx: &str,
    dy: &str,
    scale: Option<&ValueExprAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(NumericExpression, NumericExpression), &'static str> {
    let p_num = parse_signed_pt_delta(dx).ok_or("oracle:non-numeric-pt-delta")?;
    let t_num = parse_signed_pt_delta(dy).ok_or("oracle:non-numeric-pt-delta")?;
    if let Some(scale_expr) = scale {
        let base = lower_numeric_expr(scale_expr, vocabulary)?;
        let scale_one = |coeff: i64| -> Result<NumericExpression, &'static str> {
            if coeff == 0 {
                return Ok(NumericExpression::Literal(0.into()));
            }
            match &base {
                NumericExpression::Count {
                    selector,
                    multiplier,
                    offset,
                } if *offset == 0.into() => Ok(NumericExpression::Count {
                    selector: selector.clone(),
                    multiplier: multiplier.times(&coeff.into()),
                    offset: 0.into(),
                }),
                NumericExpression::AttachedAurasAndEquipment { multiplier } => {
                    Ok(NumericExpression::AttachedAurasAndEquipment {
                        multiplier: multiplier.times(&coeff.into()),
                    })
                }
                _ if coeff == 1 => Ok(base.clone()),
                _ => Err("oracle:unsupported-scaled-static-pt"),
            }
        };
        Ok((scale_one(p_num)?, scale_one(t_num)?))
    } else {
        Ok((
            NumericExpression::Literal(p_num.into()),
            NumericExpression::Literal(t_num.into()),
        ))
    }
}

fn lower_numeric_expr(
    expr: &ValueExprAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<NumericExpression, &'static str> {
    match expr {
        ValueExprAst::Const(n) => Ok(NumericExpression::Literal(i64::from(*n).into())),
        ValueExprAst::Count(f) => {
            if f.zone == Some(ZoneFilter::YourGraveyard) {
                let types = lower_card_type_mask(f.any_types)?;
                if !f.forbidden_types.is_empty()
                    || !f.subtypes.is_empty()
                    || !f.any_colors.is_empty()
                    || f.color_arity.is_some()
                {
                    return Err("oracle:complex-graveyard-numeric-count");
                }
                return Ok(NumericExpression::ZoneCount {
                    zone: StaticZone::OwnGraveyard,
                    types,
                });
            }
            if f.zone == Some(ZoneFilter::YourHand) && f.any_types.is_empty() {
                return Ok(NumericExpression::ZoneCount {
                    zone: StaticZone::OwnHand,
                    types: CardTypeMask::EMPTY,
                });
            }
            let selector = lower_filter_to_permanent_selector(f, vocabulary)?;
            Ok(NumericExpression::count(selector))
        }
        ValueExprAst::PlayerStat {
            player: PlayerRefAst::You,
            stat: PlayerStatKind::CardsInHand,
        } => Ok(NumericExpression::ZoneCount {
            zone: StaticZone::OwnHand,
            types: CardTypeMask::EMPTY,
        }),
        ValueExprAst::PlayerStat {
            player: PlayerRefAst::You,
            stat: PlayerStatKind::CardsInGraveyard,
        } => Ok(NumericExpression::ZoneCount {
            zone: StaticZone::OwnGraveyard,
            types: CardTypeMask::EMPTY,
        }),
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let kind = parse_counter_kind(counter_kind).ok_or("oracle:unsupported-counter-kind")?;
            Ok(NumericExpression::Counters { source: None, kind })
        }
        ValueExprAst::Devotion { colors } => {
            let color = match colors.to_ascii_lowercase().as_str() {
                "white" => Color::White,
                "blue" => Color::Blue,
                "black" => Color::Black,
                "red" => Color::Red,
                "green" => Color::Green,
                _ => return Err("oracle:multi-color-devotion"),
            };
            Ok(NumericExpression::PlayerStat(StaticPlayerStat::Devotion(
                color,
            )))
        }
        _ => Err("oracle:unsupported-numeric-expr"),
    }
}

fn lower_permanent_selectors(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Vec<PermanentSelector>, &'static str> {
    match subject {
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.extend(lower_permanent_selectors(item, vocabulary)?);
            }
            Ok(out)
        }
        other => Ok(vec![lower_permanent_selector(other, vocabulary)?]),
    }
}

fn lower_permanent_selector(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    match subject {
        SubjectRefAst::SelfObject => Ok(PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        }),
        SubjectRefAst::AttachedHost(_) => Ok(PermanentSelector {
            attached_to_source: true,
            ..PermanentSelector::default()
        }),
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
            lower_filter_to_permanent_selector(f, vocabulary)
        }
        _ => Err("oracle:unsupported-permanent-selector"),
    }
}

fn lower_filter_to_permanent_selector(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    if f.zone.is_some()
        || f.required_keywords.len() > 1
        || f.forbidden_keywords.len() > 1
        || f.any_spell
        || f.is_ability
    {
        return Err("oracle:unsupported-selector-predicate");
    }
    let mut sel = PermanentSelector::default();
    if let Some(ck_raw) = f.counter_kind.as_deref() {
        match ck_raw.trim().to_ascii_lowercase().as_str() {
            "+1/+1" => sel.plus_one_counter = true,
            "-1/-1" => sel.minus_one_counter = true,
            _ => return Err("oracle:unsupported-selector-predicate"),
        }
    }
    for sb in &f.stat_bounds {
        let ValueExprAst::Const(v) = sb.value.as_ref() else {
            return Err("oracle:unsupported-selector-predicate");
        };
        let v_i64 = i64::from(*v);
        match sb.stat {
            StatKind::Power => match sb.op {
                CompareOp::Ge => sel.min_power = Some(v_i64),
                CompareOp::Gt => sel.min_power = Some(v_i64 + 1),
                CompareOp::Le => sel.max_power = Some(v_i64),
                CompareOp::Lt => sel.max_power = Some(v_i64 - 1),
                CompareOp::Eq => {
                    sel.min_power = Some(v_i64);
                    sel.max_power = Some(v_i64);
                }
            },
            StatKind::Toughness => match sb.op {
                CompareOp::Ge => sel.min_toughness = Some(v_i64),
                CompareOp::Gt => sel.min_toughness = Some(v_i64 + 1),
                CompareOp::Le => sel.max_toughness = Some(v_i64),
                CompareOp::Lt => sel.max_toughness = Some(v_i64 - 1),
                CompareOp::Eq => {
                    sel.min_toughness = Some(v_i64);
                    sel.max_toughness = Some(v_i64);
                }
            },
            StatKind::ManaValue => {
                let v_u32 =
                    u32::try_from(*v).map_err(|_| "oracle:unsupported-selector-predicate")?;
                match sb.op {
                    CompareOp::Ge => sel.min_mana_value = Some(v_u32),
                    CompareOp::Gt => sel.min_mana_value = Some(v_u32.saturating_add(1)),
                    CompareOp::Le => sel.max_mana_value = Some(v_u32),
                    CompareOp::Lt => {
                        sel.max_mana_value = Some(
                            v_u32
                                .checked_sub(1)
                                .ok_or("oracle:unsupported-selector-predicate")?,
                        );
                    }
                    CompareOp::Eq => {
                        sel.min_mana_value = Some(v_u32);
                        sel.max_mana_value = Some(v_u32);
                    }
                }
            }
            _ => return Err("oracle:unsupported-selector-predicate"),
        }
    }
    if f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0 {
        sel.exclude_source = true;
    }
    if f.flags.0 & AstFilterFlags::WITH_ANY_COUNTER.0 != 0 {
        sel.any_counter = true;
    }
    if f.flags.0 & AstFilterFlags::EQUIPPED.0 != 0 {
        sel.equipped_by_at_least = 1;
    }
    if f.flags.0 & AstFilterFlags::ENCHANTED.0 != 0 {
        sel.requires_enchanted = true;
    }
    if f.flags.0 & AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0 != 0 {
        sel.matches_source_entry_type = true;
    }
    let unsupported_flags = f.flags.0
        & !(AstFilterFlags::EXCLUDE_SELF.0
            | AstFilterFlags::WITH_ANY_COUNTER.0
            | AstFilterFlags::EQUIPPED.0
            | AstFilterFlags::ENCHANTED.0
            | AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0);
    if unsupported_flags != 0 {
        return Err("oracle:unsupported-selector-flag");
    }

    if let Some(kw_raw) = f.required_keywords.first() {
        let kw =
            lower_keyword_ability_name(kw_raw).ok_or("oracle:unsupported-selector-predicate")?;
        if kw == KeywordAbility::Flying {
            sel.requires_flying = true;
        } else {
            sel.required_keyword = Some(kw);
        }
    }
    if let Some(kw_raw) = f.forbidden_keywords.first() {
        let kw =
            lower_keyword_ability_name(kw_raw).ok_or("oracle:unsupported-selector-predicate")?;
        sel.forbidden_keyword = Some(kw);
    }
    sel.token = f.token;
    match f.tapped {
        None => {}
        Some(false) => sel.requires_untapped = true,
        Some(true) => sel.requires_tapped = true,
    }
    match f.combat {
        None => {}
        Some(CombatState::Attacking) => {
            sel.combat_role = Some(spellwake_engine::characteristics::CombatSelector::Attacking);
        }
        Some(CombatState::Blocking) => {
            sel.combat_role = Some(spellwake_engine::characteristics::CombatSelector::Blocking);
        }
        Some(CombatState::AttackingOrBlocking) => {
            sel.combat_role =
                Some(spellwake_engine::characteristics::CombatSelector::AttackingOrBlocking);
        }
        _ => return Err("oracle:selector-combat-unsupported"),
    }
    let mut any_types_set = card_type_mask_to_set(f.any_types)?;
    if any_types_set.len() == 1 {
        if let Some(t) = any_types_set.pop_first() {
            sel.required_types.insert(t);
        }
    } else if any_types_set.len() > 1 {
        sel.any_types = any_types_set;
    }
    sel.forbidden_types = card_type_mask_to_set(f.forbidden_types)?;
    sel.required_supertypes = supertype_mask_to_set(f.required_supertypes)?;
    sel.forbidden_supertypes = supertype_mask_to_set(f.forbidden_supertypes)?;

    // Subtypes
    if f.forbidden_subtypes.len() > 1 {
        return Err("oracle:selector-multi-subtypes");
    }
    if f.subtypes.len() > 1 {
        for st in &f.subtypes {
            let ct = vocabulary
                .resolve_label(st)
                .ok_or("oracle:selector-multi-subtypes")?;
            sel.any_creature_subtypes.insert(ct);
        }
    } else if let Some(st) = f.subtypes.first() {
        if let Some(ct) = vocabulary.resolve_label(st) {
            sel.creature_subtype = Some(ct);
        } else if let Some(lt) = parse_land_subtype(st) {
            sel.land_subtype = Some(lt);
        } else if let Some(at) = ArtifactSubtype::from_wire_label(st) {
            sel.artifact_subtype = Some(at);
        } else {
            return Err("oracle:selector-unknown-subtype");
        }
    }
    if let Some(fst) = f.forbidden_subtypes.first() {
        if fst.eq_ignore_ascii_case("aura") {
            sel.exclude_auras = true;
        } else if let Some(ct) = vocabulary.resolve_label(fst) {
            sel.forbidden_creature_subtype = Some(ct);
        } else {
            return Err("oracle:selector-unknown-forbidden-subtype");
        }
    }

    // Color
    sel.color = lower_color_filter(f.any_colors, f.forbidden_colors, f.color_arity)?;

    // Controller
    sel.controller = match f.controller {
        None => ControllerSelector::Any,
        Some(ControllerRelation::You) => ControllerSelector::SourceController,
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
            ControllerSelector::Opponents
        }
        _ => return Err("oracle:selector-unsupported-controller"),
    };
    Ok(sel)
}

fn ast_color_mask_to_bits(mask: AstColorMask) -> u8 {
    let mut bits = 0u8;
    if mask.0 & AstColorMask::WHITE.0 != 0 {
        bits |= Color::White.bit();
    }
    if mask.0 & AstColorMask::BLUE.0 != 0 {
        bits |= Color::Blue.bit();
    }
    if mask.0 & AstColorMask::BLACK.0 != 0 {
        bits |= Color::Black.bit();
    }
    if mask.0 & AstColorMask::RED.0 != 0 {
        bits |= Color::Red.bit();
    }
    if mask.0 & AstColorMask::GREEN.0 != 0 {
        bits |= Color::Green.bit();
    }
    bits
}

fn lower_color_filter(
    any: AstColorMask,
    forbidden: AstColorMask,
    arity: Option<ColorArity>,
) -> Result<ColorFilter, &'static str> {
    if let Some(a) = arity {
        if !any.is_empty() || !forbidden.is_empty() {
            return Err("oracle:mixed-color-filter");
        }
        return match a {
            ColorArity::Colorless => Ok(ColorFilter::Colorless),
            ColorArity::Monocolored => Ok(ColorFilter::Monocolored),
            ColorArity::Multicolored => Ok(ColorFilter::Multicolored),
            ColorArity::Colored => Ok(ColorFilter::Colored),
        };
    }
    if !any.is_empty() && !forbidden.is_empty() {
        return Err("oracle:mixed-color-filter");
    }
    if !any.is_empty() {
        if let Some(c) = single_color_from_mask(any) {
            return Ok(ColorFilter::Has(c));
        }
        return Ok(ColorFilter::AnyOf(ast_color_mask_to_bits(any)));
    }
    if !forbidden.is_empty() {
        if let Some(c) = single_color_from_mask(forbidden) {
            return Ok(ColorFilter::Lacks(c));
        }
        return Ok(ColorFilter::LacksAll(ast_color_mask_to_bits(forbidden)));
    }
    Ok(ColorFilter::Any)
}

fn single_color_from_mask(mask: AstColorMask) -> Option<Color> {
    match mask {
        AstColorMask::WHITE => Some(Color::White),
        AstColorMask::BLUE => Some(Color::Blue),
        AstColorMask::BLACK => Some(Color::Black),
        AstColorMask::RED => Some(Color::Red),
        AstColorMask::GREEN => Some(Color::Green),
        _ => None,
    }
}

fn apply_selector_condition(
    sel: &mut PermanentSelector,
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), &'static str> {
    use spellwake_engine::characteristics::StaticSourceCondition;
    match cond {
        ConditionAst::ControlsCommander => {
            sel.while_controlling_own_commander = true;
            Ok(())
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes.len() == 1
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.stat_bounds.is_empty()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            let ct = vocabulary
                .resolve_label(&filter.subtypes[0])
                .ok_or("oracle:cond-unknown-subtype")?;
            sel.source_condition = Some(StaticSourceCondition::ControllerControlsSubtype(ct));
            Ok(())
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.subtypes.is_empty()
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.stat_bounds.is_empty()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            let mut types = card_type_mask_to_set(filter.any_types)?;
            if types.len() == 1
                && let Some(t) = types.pop_first()
            {
                sel.controller_type_threshold =
                    Some((t, NonZeroU16::new(*value).ok_or("oracle:zero")?));
                return Ok(());
            }
            Err("oracle:unsupported-static-cond")
        }
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } if action == "two or more equipment are attached to it" => {
            sel.equipped_by_at_least = 2;
            Ok(())
        }
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Le,
            value,
        } if *value >= 0 => {
            sel.source_condition = Some(StaticSourceCondition::ControllerLifeAtMost(
                u16::try_from(*value).map_err(|_| "oracle:life-overflow")?,
            ));
            Ok(())
        }
        ConditionAst::DevotionCompare {
            colors,
            op: CompareOp::Lt,
            value,
        } => {
            let color = match colors.to_ascii_lowercase().as_str() {
                "white" => Color::White,
                "blue" => Color::Blue,
                "black" => Color::Black,
                "red" => Color::Red,
                "green" => Color::Green,
                _ => return Err("oracle:multi-color-devotion"),
            };
            sel.devotion_below = Some((
                color,
                u8::try_from(*value).map_err(|_| "oracle:dev-overflow")?,
            ));
            Ok(())
        }
        ConditionAst::HasKeyword { subject, keyword }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            let kw = lower_keyword_ability_name(keyword).ok_or("oracle:unsupported-static-cond")?;
            sel.source_condition = Some(StaticSourceCondition::HasKeyword(kw));
            Ok(())
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes.len() == 1 => {
            let ct = vocabulary
                .resolve_label(&filter.subtypes[0])
                .ok_or("oracle:cond-unknown-subtype")?;
            sel.source_condition = Some(StaticSourceCondition::OpponentControlsSubtype(ct));
            Ok(())
        }
        ConditionAst::IsYourTurn => {
            sel.source_condition = Some(StaticSourceCondition::ControllersTurn);
            Ok(())
        }
        ConditionAst::Not(inner) if matches!(inner.as_ref(), ConditionAst::IsYourTurn) => {
            sel.source_condition = Some(StaticSourceCondition::OpponentsTurn);
            Ok(())
        }
        ConditionAst::Delirium => {
            sel.source_condition = Some(StaticSourceCondition::Delirium);
            Ok(())
        }
        ConditionAst::Metalcraft => {
            sel.source_condition = Some(StaticSourceCondition::Metalcraft);
            Ok(())
        }
        ConditionAst::Threshold => {
            sel.source_condition = Some(StaticSourceCondition::Threshold);
            Ok(())
        }
        ConditionAst::Marker(GameMarker::CitysBlessing) => {
            sel.source_condition = Some(StaticSourceCondition::ControllerHasCityBlessing);
            Ok(())
        }
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } if *value >= 0 => {
            sel.source_condition = Some(StaticSourceCondition::ControllerLifeAtLeast(
                u16::try_from(*value).map_err(|_| "oracle:life-overflow")?,
            ));
            Ok(())
        }
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op,
            value,
        } => {
            let sc = match op {
                CompareOp::Le => StaticSourceCondition::HandCountAtMost(*value),
                CompareOp::Lt if *value > 0 => StaticSourceCondition::HandCountAtMost(*value - 1),
                CompareOp::Eq if *value == 0 => StaticSourceCondition::HandCountAtMost(0),
                CompareOp::Ge => StaticSourceCondition::HandCountAtLeast(*value),
                CompareOp::Gt => StaticSourceCondition::HandCountAtLeast(value.saturating_add(1)),
                _ => return Err("oracle:unsupported-static-cond"),
            };
            sel.source_condition = Some(sc);
            Ok(())
        }
        ConditionAst::GraveyardCount {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } => {
            sel.source_condition = Some(StaticSourceCondition::GraveyardCountAtLeast {
                types: CardTypeMask::EMPTY,
                minimum: *value,
            });
            Ok(())
        }
        ConditionAst::ZoneObjectCount {
            filter,
            op: CompareOp::Ge,
            value,
        } if filter.zone == Some(ZoneFilter::YourGraveyard)
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty() =>
        {
            let types = lower_card_type_mask(filter.any_types)?;
            sel.source_condition = Some(StaticSourceCondition::GraveyardCountAtLeast {
                types,
                minimum: *value,
            });
            Ok(())
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("tapped") =>
        {
            sel.source_condition = Some(StaticSourceCondition::SourceIsTapped);
            Ok(())
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("untapped") =>
        {
            sel.requires_untapped = true;
            Ok(())
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("attacking") =>
        {
            sel.combat_role = Some(spellwake_engine::characteristics::CombatSelector::Attacking);
            Ok(())
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op: CompareOp::Ge,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            && counter_kind == "+1/+1"
            && *value >= 1 =>
        {
            if *value == 1 {
                sel.plus_one_counter = true;
            } else {
                sel.minimum_plus_one_counters = *value;
            }
            Ok(())
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op: CompareOp::Ge,
            value: 1,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            && counter_kind.eq_ignore_ascii_case("any") =>
        {
            sel.any_counter = true;
            Ok(())
        }
        ConditionAst::Marker(GameMarker::MaxSpeed)
            if sel.controller == ControllerSelector::SourceItself =>
        {
            sel.controller = ControllerSelector::SelfAtMaxSpeed;
            Ok(())
        }
        ConditionAst::Marker(GameMarker::PairedWithAnotherCreature)
            if sel.controller == ControllerSelector::SourceItself =>
        {
            sel.controller = ControllerSelector::SourcePair;
            Ok(())
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Eq,
            value: 0,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0
            && sel.controller == ControllerSelector::SourceItself =>
        {
            sel.sole_controlled_creature = true;
            Ok(())
        }
        other => {
            if let Some(sc) = lower_static_source_condition(other, vocabulary) {
                sel.source_condition = Some(sc);
                Ok(())
            } else {
                Err("oracle:unsupported-static-cond")
            }
        }
    }
}

fn lower_static_source_condition(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::characteristics::StaticSourceCondition> {
    use spellwake_engine::characteristics::StaticSourceCondition;
    match cond {
        ConditionAst::Not(inner) => {
            if matches!(inner.as_ref(), ConditionAst::IsYourTurn) {
                return Some(StaticSourceCondition::OpponentsTurn);
            }
            Some(StaticSourceCondition::Not(Box::new(
                lower_static_source_condition(inner, vocabulary)?,
            )))
        }
        ConditionAst::And(a, b) => Some(StaticSourceCondition::And(
            Box::new(lower_static_source_condition(a, vocabulary)?),
            Box::new(lower_static_source_condition(b, vocabulary)?),
        )),
        ConditionAst::Or(a, b) => Some(StaticSourceCondition::Or(
            Box::new(lower_static_source_condition(a, vocabulary)?),
            Box::new(lower_static_source_condition(b, vocabulary)?),
        )),
        ConditionAst::GainedLifeThisTurn => {
            Some(StaticSourceCondition::ControllerGainedLifeThisTurn)
        }
        ConditionAst::OpponentLostLifeThisTurn => {
            Some(StaticSourceCondition::OpponentLostLifeThisTurn)
        }
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::You,
            min_amount: 1,
        } => Some(StaticSourceCondition::ControllerLostLifeThisTurn),
        ConditionAst::AttackedThisTurn => Some(StaticSourceCondition::ControllerAttackedThisTurn),
        ConditionAst::EnteredUnderYourControlThisTurn {
            filter,
            min_count: 1,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.flags.is_empty() =>
        {
            Some(StaticSourceCondition::CreatureEnteredUnderControlThisTurn)
        }
        ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter,
        } if filter.is_empty() => Some(StaticSourceCondition::ControllerCastSpellsThisTurn(1)),
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } => match action.as_str() {
            "gained_life" | "gain_life" | "you gained life this turn" => {
                Some(StaticSourceCondition::ControllerGainedLifeThisTurn)
            }
            "lost_life" | "lose_life" | "you lost life this turn" => {
                Some(StaticSourceCondition::ControllerLostLifeThisTurn)
            }
            "attacked" | "attack" | "you attacked this turn" => {
                Some(StaticSourceCondition::ControllerAttackedThisTurn)
            }
            "discard" | "discarded" | "you discarded a card this turn" => {
                Some(StaticSourceCondition::ControllerDiscardedThisTurn)
            }
            "sacrifice" | "sacrificed" | "you sacrificed a permanent this turn" => {
                Some(StaticSourceCondition::ControllerSacrificedThisTurn)
            }
            "revolt"
            | "permanent_left_battlefield"
            | "a permanent you controlled left the battlefield this turn" => {
                Some(StaticSourceCondition::PermanentLeftBattlefieldThisTurn)
            }
            "descended" | "you descended this turn" => {
                Some(StaticSourceCondition::DescendedThisTurn)
            }
            "crime" | "committed_crime" | "you committed a crime this turn" => {
                Some(StaticSourceCondition::CommittedCrimeThisTurn)
            }
            "cast_two_spells" | "you've cast two or more spells this turn" => {
                Some(StaticSourceCondition::ControllerCastSpellsThisTurn(2))
            }
            "drew_two_cards" | "you've drawn two or more cards this turn" => {
                Some(StaticSourceCondition::ControllerDrewCardsThisTurn(2))
            }
            _ => None,
        },
        ConditionAst::IsYourTurn => Some(StaticSourceCondition::ControllersTurn),
        ConditionAst::Delirium => Some(StaticSourceCondition::Delirium),
        ConditionAst::Metalcraft => Some(StaticSourceCondition::Metalcraft),
        ConditionAst::Threshold => Some(StaticSourceCondition::Threshold),
        ConditionAst::Marker(GameMarker::CitysBlessing) => {
            Some(StaticSourceCondition::ControllerHasCityBlessing)
        }
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Le,
            value,
        } if *value >= 0 => Some(StaticSourceCondition::ControllerLifeAtMost(
            u16::try_from(*value).ok()?,
        )),
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } if *value >= 0 => Some(StaticSourceCondition::ControllerLifeAtLeast(
            u16::try_from(*value).ok()?,
        )),
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op,
            value,
        } => match op {
            CompareOp::Le => Some(StaticSourceCondition::HandCountAtMost(*value)),
            CompareOp::Lt if *value > 0 => Some(StaticSourceCondition::HandCountAtMost(*value - 1)),
            CompareOp::Eq if *value == 0 => Some(StaticSourceCondition::HandCountAtMost(0)),
            CompareOp::Ge => Some(StaticSourceCondition::HandCountAtLeast(*value)),
            CompareOp::Gt => Some(StaticSourceCondition::HandCountAtLeast(
                value.saturating_add(1),
            )),
            _ => None,
        },
        ConditionAst::GraveyardCount {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } => Some(StaticSourceCondition::GraveyardCountAtLeast {
            types: CardTypeMask::EMPTY,
            minimum: *value,
        }),
        ConditionAst::ZoneObjectCount {
            filter,
            op: CompareOp::Ge,
            value,
        } if filter.zone == Some(ZoneFilter::YourGraveyard)
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty() =>
        {
            let types = lower_card_type_mask(filter.any_types).ok()?;
            Some(StaticSourceCondition::GraveyardCountAtLeast {
                types,
                minimum: *value,
            })
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes.len() == 1
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.stat_bounds.is_empty()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            let ct = vocabulary.resolve_label(&filter.subtypes[0])?;
            Some(StaticSourceCondition::ControllerControlsSubtype(ct))
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.subtypes.len() == 1
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.stat_bounds.is_empty()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            let ct = vocabulary.resolve_label(&filter.subtypes[0])?;
            Some(StaticSourceCondition::OpponentControlsSubtype(ct))
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.subtypes.is_empty()
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.stat_bounds.is_empty()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            let mut types = card_type_mask_to_set(filter.any_types).ok()?;
            if types.len() == 1
                && let Some(kind) = types.pop_first()
            {
                return Some(StaticSourceCondition::ControlsCardType {
                    kind,
                    minimum: *value,
                });
            }
            None
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0 => {
            if let Ok(selector) = lower_filter_to_permanent_selector(filter, vocabulary) {
                return Some(StaticSourceCondition::ControlsMatching {
                    selector: Box::new(selector),
                    minimum: *value,
                });
            }
            None
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0 => {
            if let Ok(selector) = lower_filter_to_permanent_selector(filter, vocabulary) {
                return Some(StaticSourceCondition::OpponentControlsMatching {
                    selector: Box::new(selector),
                    minimum: *value,
                });
            }
            None
        }
        ConditionAst::HasKeyword { subject, keyword }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            Some(StaticSourceCondition::HasKeyword(
                lower_keyword_ability_name(keyword)?,
            ))
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("tapped") =>
        {
            Some(StaticSourceCondition::SourceIsTapped)
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("untapped") =>
        {
            Some(StaticSourceCondition::SourceIsUntapped)
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op: CompareOp::Ge,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) && *value >= 1 => {
            let kind = parse_counter_kind(counter_kind)?;
            Some(StaticSourceCondition::SourceCounterAtLeast {
                kind,
                minimum: *value,
            })
        }
        _ => None,
    }
}

fn lower_card_type_mask(mask: AstCardTypeMask) -> Result<CardTypeMask, &'static str> {
    let mut out = CardTypeMask::EMPTY;
    if mask.0 & AstCardTypeMask::BATTLE.0 != 0 {
        out = out.union(CardTypeMask::BATTLE);
    }
    if mask.0 & AstCardTypeMask::PLANESWALKER.0 != 0 {
        out = out.union(CardTypeMask::PLANESWALKER);
    }
    if mask.0 & AstCardTypeMask::LAND.0 != 0 {
        out = out.union(CardTypeMask::LAND);
    }
    if mask.0 & AstCardTypeMask::ARTIFACT.0 != 0 {
        out = out.union(CardTypeMask::ARTIFACT);
    }
    if mask.0 & AstCardTypeMask::CREATURE.0 != 0 {
        out = out.union(CardTypeMask::CREATURE);
    }
    if mask.0 & AstCardTypeMask::ENCHANTMENT.0 != 0 {
        out = out.union(CardTypeMask::ENCHANTMENT);
    }
    if mask.0 & AstCardTypeMask::KINDRED.0 != 0 {
        out = out.union(CardTypeMask::KINDRED);
    }
    if mask.0 & AstCardTypeMask::INSTANT.0 != 0 {
        out = out.union(CardTypeMask::INSTANT);
    }
    if mask.0 & AstCardTypeMask::SORCERY.0 != 0 {
        out = out.union(CardTypeMask::SORCERY);
    }
    Ok(out)
}

fn card_type_mask_to_set(mask: AstCardTypeMask) -> Result<BTreeSet<CardType>, &'static str> {
    let mut set = BTreeSet::new();
    let pairs = [
        (AstCardTypeMask::BATTLE, CardType::Battle),
        (AstCardTypeMask::PLANESWALKER, CardType::Planeswalker),
        (AstCardTypeMask::LAND, CardType::Land),
        (AstCardTypeMask::ARTIFACT, CardType::Artifact),
        (AstCardTypeMask::CREATURE, CardType::Creature),
        (AstCardTypeMask::ENCHANTMENT, CardType::Enchantment),
        (AstCardTypeMask::KINDRED, CardType::Kindred),
        (AstCardTypeMask::INSTANT, CardType::Instant),
        (AstCardTypeMask::SORCERY, CardType::Sorcery),
    ];
    for (bit, kind) in pairs {
        if mask.0 & bit.0 != 0 {
            set.insert(kind);
        }
    }
    Ok(set)
}

fn supertype_mask_to_set(mask: AstSupertypeMask) -> Result<BTreeSet<Supertype>, &'static str> {
    let mut set = BTreeSet::new();
    if mask.0 & AstSupertypeMask::BASIC.0 != 0 {
        set.insert(Supertype::Basic);
    }
    if mask.0 & AstSupertypeMask::LEGENDARY.0 != 0 {
        set.insert(Supertype::Legendary);
    }
    if mask.0 & AstSupertypeMask::SNOW.0 != 0 {
        set.insert(Supertype::Snow);
    }
    if mask.0 & AstSupertypeMask::WORLD.0 != 0 {
        set.insert(Supertype::World);
    }
    if mask.0 & AstSupertypeMask::HISTORIC.0 != 0 {
        return Err("oracle:historic-in-supertype-set");
    }
    Ok(set)
}

fn lower_color_mask(mask: AstColorMask) -> ColorMask {
    let mut out = ColorMask::EMPTY;
    if mask.0 & AstColorMask::WHITE.0 != 0 {
        out = out.union(ColorMask::WHITE);
    }
    if mask.0 & AstColorMask::BLUE.0 != 0 {
        out = out.union(ColorMask::BLUE);
    }
    if mask.0 & AstColorMask::BLACK.0 != 0 {
        out = out.union(ColorMask::BLACK);
    }
    if mask.0 & AstColorMask::RED.0 != 0 {
        out = out.union(ColorMask::RED);
    }
    if mask.0 & AstColorMask::GREEN.0 != 0 {
        out = out.union(ColorMask::GREEN);
    }
    out
}

fn color_mask_to_set(mask: AstColorMask) -> BTreeSet<Color> {
    let mut out = BTreeSet::new();
    if mask.0 & AstColorMask::WHITE.0 != 0 {
        out.insert(Color::White);
    }
    if mask.0 & AstColorMask::BLUE.0 != 0 {
        out.insert(Color::Blue);
    }
    if mask.0 & AstColorMask::BLACK.0 != 0 {
        out.insert(Color::Black);
    }
    if mask.0 & AstColorMask::RED.0 != 0 {
        out.insert(Color::Red);
    }
    if mask.0 & AstColorMask::GREEN.0 != 0 {
        out.insert(Color::Green);
    }
    out
}

fn lower_object_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ObjectFilter, &'static str> {
    if f.is_ability || f.counter_kind.is_some() {
        return Err("oracle:unsupported-object-filter");
    }
    let mut clause = ObjectClause::any();
    clause.any_types = lower_card_type_mask(f.any_types)?;
    clause.forbidden_types = lower_card_type_mask(f.forbidden_types)?;

    // Supertypes (Historic maps to ObjectFilterFlags::HISTORIC)
    let mut req_sup = SupertypeMask::EMPTY;
    if f.required_supertypes.0 & AstSupertypeMask::BASIC.0 != 0 {
        req_sup = req_sup.union(SupertypeMask::BASIC);
    }
    if f.required_supertypes.0 & AstSupertypeMask::LEGENDARY.0 != 0 {
        req_sup = req_sup.union(SupertypeMask::LEGENDARY);
    }
    if f.required_supertypes.0 & AstSupertypeMask::SNOW.0 != 0 {
        req_sup = req_sup.union(SupertypeMask::SNOW);
    }
    if f.required_supertypes.0 & AstSupertypeMask::WORLD.0 != 0 {
        req_sup = req_sup.union(SupertypeMask::WORLD);
    }
    if f.required_supertypes.0 & AstSupertypeMask::HISTORIC.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::HISTORIC);
    }
    clause.required_supertypes = req_sup;

    let mut forb_sup = SupertypeMask::EMPTY;
    if f.forbidden_supertypes.0 & AstSupertypeMask::BASIC.0 != 0 {
        forb_sup = forb_sup.union(SupertypeMask::BASIC);
    }
    if f.forbidden_supertypes.0 & AstSupertypeMask::LEGENDARY.0 != 0 {
        forb_sup = forb_sup.union(SupertypeMask::LEGENDARY);
    }
    if f.forbidden_supertypes.0 & AstSupertypeMask::SNOW.0 != 0 {
        forb_sup = forb_sup.union(SupertypeMask::SNOW);
    }
    if f.forbidden_supertypes.0 & AstSupertypeMask::WORLD.0 != 0 {
        forb_sup = forb_sup.union(SupertypeMask::WORLD);
    }
    if f.forbidden_supertypes.0 & AstSupertypeMask::HISTORIC.0 != 0 {
        return Err("oracle:forbidden-historic");
    }
    clause.forbidden_supertypes = forb_sup;

    // Colors
    clause.color = if let Some(arity) = f.color_arity {
        if !f.any_colors.is_empty() || !f.forbidden_colors.is_empty() {
            return Err("oracle:mixed-color-arity");
        }
        match arity {
            ColorArity::Colorless => ObjectColorFilter::Colorless,
            ColorArity::Colored => ObjectColorFilter::Colored,
            ColorArity::Multicolored => ObjectColorFilter::Multicolored,
            ColorArity::Monocolored => ObjectColorFilter::Monocolored,
        }
    } else if !f.any_colors.is_empty() || !f.forbidden_colors.is_empty() {
        ObjectColorFilter::Mask {
            any_of: lower_color_mask(f.any_colors),
            none_of: lower_color_mask(f.forbidden_colors),
        }
    } else {
        ObjectColorFilter::Any
    };

    // Controller
    clause.controller = match f.controller {
        None => ControllerFilter::Any,
        Some(ControllerRelation::You) => ControllerFilter::You,
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
            ControllerFilter::Opponent
        }
        Some(ControllerRelation::NotYou) => ControllerFilter::NotYou,
        Some(ControllerRelation::OwnerYou) => ControllerFilter::OwnerYou,
        _ => return Err("oracle:unsupported-controller-filter"),
    };

    // Combat
    clause.combat = match f.combat {
        None => CombatFilter::Any,
        Some(CombatState::Attacking) => CombatFilter::Attacking,
        Some(CombatState::Blocking) => CombatFilter::Blocking,
        Some(CombatState::AttackingOrBlocking) => CombatFilter::AttackingOrBlocking,
        Some(CombatState::Blocked) => CombatFilter::Blocked,
        Some(CombatState::Unblocked) => CombatFilter::Unblocked,
        Some(CombatState::BlockingSelf) => CombatFilter::BlockingSource,
        Some(CombatState::BlockedBySelf) => CombatFilter::BlockedBySource,
        Some(CombatState::BlockingOrBlockedBySelf) => CombatFilter::BlockingOrBlockedBySource,
        Some(CombatState::NotAttacking) => return Err("oracle:unsupported-not-attacking"),
    };

    clause.tapped = f.tapped;
    clause.token = f.token;

    // Keywords
    if f.required_keywords.len() > 1 || f.forbidden_keywords.len() > 1 {
        return Err("oracle:multi-keyword-filter");
    }
    if let Some(k) = f.required_keywords.first() {
        clause.keyword = Some(lower_keyword_ability_name(k).ok_or("oracle:filter-unknown-kw")?);
    }
    if let Some(k) = f.forbidden_keywords.first() {
        clause.forbidden_keyword =
            Some(lower_keyword_ability_name(k).ok_or("oracle:filter-unknown-forb-kw")?);
    }

    // Stat bounds
    for sb in &f.stat_bounds {
        match (sb.stat, sb.op, sb.value.as_ref()) {
            (StatKind::Power, CompareOp::Ge, ValueExprAst::Const(n)) => {
                clause.power =
                    PowerFilter::AtLeast(i16::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Power, CompareOp::Gt, ValueExprAst::Const(n)) => {
                clause.power =
                    PowerFilter::AtLeast(i16::try_from(*n + 1).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Power, CompareOp::Le, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.power =
                    PowerFilter::AtMost(u16::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Power, CompareOp::Lt, ValueExprAst::Const(n)) if *n > 0 => {
                clause.power =
                    PowerFilter::AtMost(u16::try_from(*n - 1).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Power, CompareOp::Le, ValueExprAst::ChosenX) => {
                clause.power = PowerFilter::AtMostX;
            }
            (StatKind::Toughness, CompareOp::Ge, ValueExprAst::Const(n)) => {
                clause.toughness =
                    ToughnessFilter::AtLeast(i16::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::Toughness, CompareOp::Le, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.toughness =
                    ToughnessFilter::AtMost(u16::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::ManaValue, CompareOp::Le, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.mana_value =
                    ManaValueFilter::AtMost(u32::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::ManaValue, CompareOp::Ge, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.mana_value =
                    ManaValueFilter::AtLeast(u32::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::ManaValue, CompareOp::Eq, ValueExprAst::Const(n)) if *n >= 0 => {
                clause.mana_value =
                    ManaValueFilter::Equal(u32::try_from(*n).map_err(|_| "oracle:stat-ovf")?);
            }
            (StatKind::ManaValue, CompareOp::Le, ValueExprAst::ChosenX) => {
                clause.mana_value = ManaValueFilter::AtMostX;
            }
            _ => return Err("oracle:unsupported-stat-bound"),
        }
    }

    // Flags
    if f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0 {
        clause.source_relation = SourceRelation::OtherThanAbilitySource;
    }
    if f.flags.0 & AstFilterFlags::ENTERED_THIS_TURN.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::ENTERED_THIS_TURN);
    }
    if f.flags.0 & AstFilterFlags::DEALT_DAMAGE_THIS_TURN.0 != 0 {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
    }
    if f.flags.0 & AstFilterFlags::ATTACKED_THIS_TURN.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::ATTACKED_THIS_TURN);
    }
    if f.flags.0 & AstFilterFlags::MODIFIED.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::MODIFIED);
    }
    if f.flags.0 & AstFilterFlags::EQUIPPED.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::EQUIPPED);
    }
    if f.flags.0 & AstFilterFlags::ENCHANTED.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::ENCHANTED);
    }
    if f.flags.0 & AstFilterFlags::WITH_ANY_COUNTER.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::WITH_ANY_COUNTER);
    }
    if f.flags.0 & AstFilterFlags::WITHOUT_COUNTERS.0 != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
    }
    if f.flags.0 & AstFilterFlags::ATTACHED_TO_SELF.0 != 0 {
        clause.source_relation = SourceRelation::AttachedToSelf;
    }
    if f.flags.0 & AstFilterFlags::MATCHES_NAMED_AGENDA.0 != 0 {
        clause.source_relation = SourceRelation::MatchesNamedAgendaSlot;
    }
    let handled_flags = AstFilterFlags::EXCLUDE_SELF.0
        | AstFilterFlags::ENTERED_THIS_TURN.0
        | AstFilterFlags::DEALT_DAMAGE_THIS_TURN.0
        | AstFilterFlags::ATTACKED_THIS_TURN.0
        | AstFilterFlags::MODIFIED.0
        | AstFilterFlags::EQUIPPED.0
        | AstFilterFlags::ENCHANTED.0
        | AstFilterFlags::WITH_ANY_COUNTER.0
        | AstFilterFlags::WITHOUT_COUNTERS.0
        | AstFilterFlags::ATTACHED_TO_SELF.0
        | AstFilterFlags::MATCHES_NAMED_AGENDA.0;
    if (f.flags.0 & !handled_flags) != 0 {
        return Err("oracle:unsupported-filter-flag");
    }

    // Forbidden subtypes
    for fst in &f.forbidden_subtypes {
        if fst.eq_ignore_ascii_case("aura") {
            clause.flags = clause.flags.union(ObjectFilterFlags::NON_AURA);
        } else if fst.eq_ignore_ascii_case("saga") {
            clause.flags = clause.flags.union(ObjectFilterFlags::NON_SAGA);
        } else if let Some(ct) = vocabulary.resolve_label(fst) {
            if clause.subtypes != SubtypeFilter::Any {
                return Err("oracle:multi-subtype-filter");
            }
            clause.subtypes = SubtypeFilter::Lacks(ct);
        } else if let Some(lt) = parse_land_subtype(fst) {
            if clause.subtypes != SubtypeFilter::Any {
                return Err("oracle:multi-subtype-filter");
            }
            clause.subtypes = SubtypeFilter::NotLand(lt);
        } else {
            return Err("oracle:unsupported-forbidden-subtype");
        }
    }

    // Positive subtypes
    if f.subtypes.len() == 1 {
        if clause.subtypes != SubtypeFilter::Any {
            return Err("oracle:mixed-subtype-filter");
        }
        let st = &f.subtypes[0];
        if st.eq_ignore_ascii_case("aura") {
            clause.subtypes = SubtypeFilter::Aura;
        } else if let Some(ct) = vocabulary.resolve_label(st) {
            clause.subtypes = SubtypeFilter::Has(ct);
        } else if let Some(lt) = parse_land_subtype(st) {
            clause.subtypes = SubtypeFilter::Land(lt);
        } else if let Some(at) = ArtifactSubtype::from_wire_label(st) {
            clause.subtypes = SubtypeFilter::Artifact(at);
        } else if let Some(et) = EnchantmentSubtype::from_wire_label(st) {
            clause.subtypes = SubtypeFilter::Enchantment(et);
        } else {
            return Err("oracle:unsupported-subtype-filter");
        }
    } else if f.subtypes.len() == 2 && clause.subtypes == SubtypeFilter::Any {
        let mut c1 = clause;
        let mut c2 = clause;
        if let (Some(a), Some(b)) = (
            vocabulary.resolve_label(&f.subtypes[0]),
            vocabulary.resolve_label(&f.subtypes[1]),
        ) {
            c1.subtypes = SubtypeFilter::Has(a);
            c2.subtypes = SubtypeFilter::Has(b);
            return Ok(ObjectFilter::either(c1, c2));
        }
        if let (Some(a), Some(b)) = (
            parse_land_subtype(&f.subtypes[0]),
            parse_land_subtype(&f.subtypes[1]),
        ) {
            c1.subtypes = SubtypeFilter::Land(a);
            c2.subtypes = SubtypeFilter::Land(b);
            return Ok(ObjectFilter::either(c1, c2));
        }
        return Err("oracle:unsupported-multi-subtype");
    } else if f.subtypes.len() > 2 {
        return Err("oracle:too-many-subtypes");
    }

    Ok(ObjectFilter::Clause(clause))
}

fn lower_spell_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SpellFilter, &'static str> {
    if f.zone.is_some() {
        return Err("oracle:spell-filter-zone");
    }
    let obj = lower_object_filter(f, vocabulary)?;
    Ok(SpellFilter::from_object(obj))
}

fn lower_spell_subject_filter(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(bool, SpellFilter), &'static str> {
    let f = match subject {
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => f,
        _ => return Err("oracle:unsupported-spell-subject"),
    };
    let any_player = f.controller.is_none();
    let mut stripped = f.as_ref().clone();
    if stripped.controller == Some(ControllerRelation::You) {
        stripped.controller = None;
    } else if stripped.controller.is_some() {
        return Err("oracle:spell-discount-controller");
    }
    let sf = lower_spell_filter(&stripped, vocabulary)?;
    Ok((any_player, sf))
}

fn lower_value_expr(
    expr: &ValueExprAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ValueExpr, &'static str> {
    match expr {
        ValueExprAst::Const(n) => Ok(ValueExpr::literal(i64::from(*n))),
        ValueExprAst::ChosenX => Ok(ValueExpr::chosen_x()),
        ValueExprAst::Count(f) => {
            if let Some(zone) = f.zone {
                let mut inner = f.as_ref().clone();
                inner.zone = None;
                let obj = lower_object_filter(&inner, vocabulary)?;
                return match zone {
                    ZoneFilter::YourGraveyard => {
                        Ok(ValueExpr::count_graveyard(ResolvingPlayer::Controller, obj))
                    }
                    ZoneFilter::TargetPlayerGraveyard => Ok(ValueExpr::count_graveyard(
                        ResolvingPlayer::TargetPlayer,
                        obj,
                    )),
                    ZoneFilter::YourHand if obj == ObjectFilter::any() => {
                        Ok(ValueExpr::count_hand(ResolvingPlayer::Controller))
                    }
                    ZoneFilter::Battlefield => Ok(ValueExpr::count_battlefield(obj)),
                    _ => Err("oracle:unsupported-count-zone"),
                };
            }
            let obj = lower_object_filter(f, vocabulary)?;
            Ok(ValueExpr::count_battlefield(obj))
        }
        ValueExprAst::PlayerStat { player, stat } => match (player, stat) {
            (PlayerRefAst::You, PlayerStatKind::LifeTotal) => {
                Ok(ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife))
            }
            (PlayerRefAst::You, PlayerStatKind::CardsInHand) => {
                Ok(ValueExpr::count_hand(ResolvingPlayer::Controller))
            }
            (
                PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent,
                PlayerStatKind::CardsInHand,
            ) => Ok(ValueExpr::count_hand(ResolvingPlayer::TargetPlayer)),
            (PlayerRefAst::You, PlayerStatKind::CardsInGraveyard) => Ok(
                ValueExpr::count_graveyard(ResolvingPlayer::Controller, ObjectFilter::any()),
            ),
            (
                PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent,
                PlayerStatKind::CardsInGraveyard,
            ) => Ok(ValueExpr::count_graveyard(
                ResolvingPlayer::TargetPlayer,
                ObjectFilter::any(),
            )),
            _ => Err("oracle:unsupported-player-stat"),
        },
        ValueExprAst::CardTypesInGraveyard {
            all_graveyards: false,
        } => Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::OwnGraveyardDistinctCardTypes,
        )),
        ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::You,
            gained: true,
        } => Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::ControllerLifeGainedThisTurn,
        )),
        ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::You,
            gained: false,
        } => Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::ControllerLifeLostThisTurn,
        )),
        ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::EachOpponent,
            gained: false,
        } => Ok(ValueExpr::PlayerStat(
            PlayerStatExpr::OpponentsLifeLostThisTurn,
        )),
        ValueExprAst::TimesKicked => Ok(ValueExpr::EventValue(EventValueKind::KickerCount(0))),
        ValueExprAst::ObjectStat { subject, stat } => {
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => return Err("oracle:loyalty-stat-unsupported"),
            };
            let subj = match subject.as_ref() {
                SubjectRefAst::SelfObject => ValueObjectRef::AbilitySource,
                SubjectRefAst::TriggerSubject => ValueObjectRef::DyingCreature,
                SubjectRefAst::Anaphora(_) => ValueObjectRef::TargetObject,
                _ => return Err("oracle:unsupported-stat-subject"),
            };
            Ok(ValueExpr::ObjectStat {
                subject: subj,
                stat: st,
            })
        }
        ValueExprAst::AggregateStat { op, stat, filter } => {
            let agg = match op {
                AggregateOp::Max => StatAggregate::Max,
                AggregateOp::Sum => StatAggregate::Sum,
                AggregateOp::Min => return Err("oracle:min-aggregate-unsupported"),
            };
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => return Err("oracle:loyalty-stat-unsupported"),
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ValueExpr::aggregate(obj, st, agg))
        }
        ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::You,
            counter_kind,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:unsupported-counter-kind")?;
            let subject = match ck {
                CounterKind::Experience => {
                    spellwake_engine::program::CounterSubjectRef::ControllerExperience
                }
                CounterKind::Energy => {
                    spellwake_engine::program::CounterSubjectRef::ControllerEnergy
                }
                CounterKind::Poison => {
                    spellwake_engine::program::CounterSubjectRef::ControllerPoison
                }
                _ => return Err("oracle:unsupported-counter-kind"),
            };
            Ok(ValueExpr::CounterCount {
                subject,
                kind: None,
            })
        }
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } if matches!(
            subject.as_ref(),
            SubjectRefAst::SelfObject | SubjectRefAst::TriggerSubject
        ) =>
        {
            let kind = if counter_kind.eq_ignore_ascii_case("any") {
                None
            } else {
                Some(parse_counter_kind(counter_kind).ok_or("oracle:unsupported-counter-kind")?)
            };
            let subj = match subject.as_ref() {
                SubjectRefAst::SelfObject => {
                    spellwake_engine::program::CounterSubjectRef::AbilitySource
                }
                SubjectRefAst::TriggerSubject => {
                    spellwake_engine::program::CounterSubjectRef::TriggerSource
                }
                _ => unreachable!(),
            };
            Ok(ValueExpr::CounterCount {
                subject: subj,
                kind,
            })
        }
        ValueExprAst::Add(a, b) => Ok(ValueExpr::Add(
            Box::new(lower_value_expr(a, vocabulary)?),
            Box::new(lower_value_expr(b, vocabulary)?),
        )),
        ValueExprAst::Sub(a, b) => Ok(ValueExpr::Sub(
            Box::new(lower_value_expr(a, vocabulary)?),
            Box::new(lower_value_expr(b, vocabulary)?),
        )),
        ValueExprAst::DomainCount => Ok(ValueExpr::DomainCount),
        ValueExprAst::OpponentsCount => Ok(ValueExpr::OpponentsCount),
        ValueExprAst::Devotion { colors } => {
            let mut mask = ColorMask::EMPTY;
            let low = colors.to_ascii_lowercase();
            if low.contains("white") || colors.contains('W') {
                mask = mask.union(ColorMask::WHITE);
            }
            if low.contains("blue") || colors.contains('U') {
                mask = mask.union(ColorMask::BLUE);
            }
            if low.contains("black") || colors.contains('B') {
                mask = mask.union(ColorMask::BLACK);
            }
            if low.contains("red") || colors.contains('R') {
                mask = mask.union(ColorMask::RED);
            }
            if low.contains("green") || colors.contains('G') {
                mask = mask.union(ColorMask::GREEN);
            }
            if mask.is_empty() {
                return Err("oracle:unsupported-value-expr");
            }
            Ok(ValueExpr::Devotion(mask))
        }
        ValueExprAst::Mul(a, b) => match (a.as_ref(), b.as_ref()) {
            (ValueExprAst::Const(k), inner) | (inner, ValueExprAst::Const(k)) => {
                Ok(ValueExpr::Scale {
                    expr: Box::new(lower_value_expr(inner, vocabulary)?),
                    factor: i64::from(*k).into(),
                })
            }
            _ => Err("oracle:non-const-mul"),
        },
        ValueExprAst::DivConst {
            expr,
            denom,
            rounding,
        } if *denom > 0 => {
            let d = NonZeroU16::new(u16::try_from(*denom).map_err(|_| "oracle:div-ovf")?)
                .ok_or("oracle:div-zero")?;
            Ok(ValueExpr::DivConst {
                expr: Box::new(lower_value_expr(expr, vocabulary)?),
                divisor: d,
                round_up: *rounding == Rounding::Up,
            })
        }
        ValueExprAst::PriorResult => Ok(ValueExpr::EventValue(EventValueKind::LastActionCount)),
        _ => Err("oracle:unsupported-value-expr"),
    }
}

fn parse_look_top_pick_filter(
    pick_low: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let stripped = pick_low
        .trim()
        .trim_start_matches("up to one ")
        .trim_start_matches("a ")
        .trim_start_matches("an ")
        .trim_start_matches("one ")
        .trim_end_matches(" card from among them")
        .trim_end_matches(" from among them")
        .trim_end_matches(" card")
        .trim();
    match stripped {
        "creature" => Some(ObjectFilter::creature()),
        "land" => Some(ObjectFilter::land()),
        "artifact" => Some(ObjectFilter::artifact()),
        "enchantment" => Some(ObjectFilter::enchantment()),
        "planeswalker" => Some(ObjectFilter::planeswalker()),
        "instant" => Some(ObjectFilter::instant()),
        "sorcery" => Some(ObjectFilter::sorcery()),
        "permanent" => Some(ObjectFilter::permanent()),
        "nonland" | "nonland permanent" => Some(ObjectFilter::nonland()),
        "noncreature" => Some(ObjectFilter::from_clause(
            ObjectClause::any().forbid_types(CardTypeMask::CREATURE),
        )),
        "instant or sorcery" | "sorcery or instant" => Some(ObjectFilter::instant_or_sorcery()),
        "creature or land" | "land or creature" => Some(ObjectFilter::any_types(
            CardTypeMask::CREATURE.union(CardTypeMask::LAND),
        )),
        "creature or planeswalker" | "planeswalker or creature" => Some(ObjectFilter::any_types(
            CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER),
        )),
        "artifact or enchantment" | "enchantment or artifact" => Some(ObjectFilter::any_types(
            CardTypeMask::ARTIFACT.union(CardTypeMask::ENCHANTMENT),
        )),
        "artifact or creature" | "creature or artifact" => Some(ObjectFilter::any_types(
            CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE),
        )),
        "artifact or land" | "land or artifact" => Some(ObjectFilter::any_types(
            CardTypeMask::ARTIFACT.union(CardTypeMask::LAND),
        )),
        "creature or enchantment" | "enchantment or creature" => Some(ObjectFilter::any_types(
            CardTypeMask::CREATURE.union(CardTypeMask::ENCHANTMENT),
        )),
        "basic land" => {
            let mut clause = ObjectClause::land();
            clause.required_supertypes = spellwake_engine::program::SupertypeMask::BASIC;
            Some(ObjectFilter::from_clause(clause))
        }
        "legendary" | "legendary permanent" => Some(ObjectFilter::any().legendary()),
        "legendary creature" => Some(ObjectFilter::creature().legendary()),
        other => {
            let cap = {
                let mut chars = other.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            };
            vocabulary
                .resolve_label(&cap)
                .map(|ct| ObjectFilter::any().with_subtype(ct))
        }
    }
}

fn parse_counter_kind(raw: &str) -> Option<CounterKind> {
    let lower = raw.trim().to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "{e}" | "e" | "energy" => "energy",
        other => other,
    };
    CounterKind::from_oracle_label(normalized)
}

fn lower_ability(
    ab: &AbilityAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    match ab {
        AbilityAst::Activated {
            ability_word: _,
            zone,
            cost,
            body,
        } => {
            // Try lowering as a ManaAbility first (battlefield, untargeted AddMana)
            if zone.is_none()
                && let Some(mana_out) = try_lower_mana_ability(cost, body, vocabulary)
            {
                program.mana.push(ManaAbility {
                    id: ids.mana(),
                    output: mana_out,
                });
                return Ok(());
            }
            let (mana_cost, source_cost) =
                lower_activation_cost(cost, zone.as_deref(), vocabulary)?;
            let anaphora_ref = match source_cost {
                SourceCost::SacrificeSelected { .. }
                | SourceCost::PayLife {
                    sacrifice: Some(_), ..
                } => ValueObjectRef::SacrificedCreature,
                SourceCost::ExileGraveyard { .. } => ValueObjectRef::ExiledCostCreature,
                _ => ValueObjectRef::AbilitySource,
            };
            let (filtered_ins, restriction) = strip_activated_restrictions(body, vocabulary)?;
            let allows_modal_split = !matches!(
                restriction,
                Some(
                    ActivationRestriction::OnceEachTurn
                        | ActivationRestriction::SorceryOnceEachTurn
                        | ActivationRestriction::DuringYourTurnOnceEachTurn
                )
            );
            if allows_modal_split
                && let [InstructionAst::Modal { choice, modes }] = filtered_ins.as_slice()
                && let Ok((1, 1)) = parse_modal_choice(choice)
                && modes.len() >= 2
            {
                for m in modes {
                    let mut exec_body = lower_effect_body(&m.body, vocabulary, ids, false, true)?;
                    rewrite_body_anaphora_stats(&mut exec_body, anaphora_ref);
                    let id = ids.activated();
                    if let Some(res) = restriction {
                        program.activation_restrictions.insert(id, res);
                    }
                    program.activated.push(ActivatedAbility {
                        id,
                        mana_cost: mana_cost.clone(),
                        source_cost,
                        body: exec_body,
                    });
                }
                return Ok(());
            }
            let mut exec_body = lower_effect_body(&filtered_ins, vocabulary, ids, false, true)?;
            rewrite_body_anaphora_stats(&mut exec_body, anaphora_ref);
            let id = ids.activated();
            if let Some(res) = restriction {
                program.activation_restrictions.insert(id, res);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost,
                source_cost,
                body: exec_body,
            });
            Ok(())
        }
        AbilityAst::Triggered {
            ability_word: _,
            trigger,
            body,
        } => {
            let (mut spec, mut grouping, guard) = lower_trigger_spec(trigger, vocabulary)?;
            if matches!(program.kind, ProgramKind::Spell(_))
                && spec.zone == spellwake_engine::program::TriggerZone::Battlefield
                && body_references_self_from_graveyard(body)
            {
                spec.zone = spellwake_engine::program::TriggerZone::Graveyard;
            }
            let self_pronoun = trigger_subject_is_self(trigger);
            let stripped_body = strip_triggered_restrictions(body, &mut spec, &mut grouping);
            let body = &stripped_body;
            let mut exec_body = if guard.is_none()
                && let [InstructionAst::Modal { choice, modes }] = body.as_slice()
            {
                let (min, raw_max) = parse_modal_choice(choice)?;
                if modes.len() < 2 {
                    return Err("oracle:modal-too-few");
                }
                let max = raw_max.min(u8::try_from(modes.len()).unwrap_or(u8::MAX));
                let mut options = Vec::with_capacity(modes.len());
                for m in modes {
                    options.push(lower_effect_body(
                        &m.body,
                        vocabulary,
                        ids,
                        true,
                        self_pronoun,
                    )?);
                }
                ExecutableBody::with_triggered_modes(TriggeredModes {
                    minimum: min,
                    maximum: max,
                    distinct_targets: false,
                    options,
                })
            } else {
                let mut b = lower_effect_body(body, vocabulary, ids, true, self_pronoun)?;
                if let Some(cond) = guard {
                    b.instructions
                        .insert(0, Instruction::RequireCondition(cond));
                }
                b
            };
            apply_trigger_body_rewrites(&mut exec_body, &spec, self_pronoun);
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: spec,
                grouping,
                body: exec_body,
            });
            Ok(())
        }
        AbilityAst::Loyalty { delta, body } => {
            let trimmed = delta.trim().replace('\u{2212}', "-");
            let parsed_delta: Option<i32> = if trimmed.eq_ignore_ascii_case("-x") {
                None
            } else {
                let n: i32 = trimmed
                    .strip_prefix('+')
                    .unwrap_or(&trimmed)
                    .parse()
                    .map_err(|_| "oracle:bad-loyalty-delta")?;
                Some(n)
            };
            if let [InstructionAst::Modal { choice, modes }] = body.as_slice()
                && let Ok((1, 1)) = parse_modal_choice(choice)
                && modes.len() >= 2
            {
                for m in modes {
                    let id = ids.activated();
                    if let Some(n) = parsed_delta {
                        program.loyalty_costs.insert(id, n);
                    } else {
                        program.loyalty_costs.insert(id, 0);
                        program.variable_loyalty_costs.insert(id);
                    }
                    let exec_body = lower_effect_body(&m.body, vocabulary, ids, false, true)?;
                    program.activated.push(ActivatedAbility {
                        id,
                        mana_cost: ManaCost::free(),
                        source_cost: SourceCost::None,
                        body: exec_body,
                    });
                }
                return Ok(());
            }
            let id = ids.activated();
            if let Some(n) = parsed_delta {
                program.loyalty_costs.insert(id, n);
            } else {
                program.loyalty_costs.insert(id, 0);
                program.variable_loyalty_costs.insert(id);
            }
            let exec_body = lower_effect_body(body, vocabulary, ids, false, true)?;
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: exec_body,
            });
            Ok(())
        }
        AbilityAst::SagaChapter { chapters, body } => {
            if chapters.is_empty() {
                return Err("oracle:empty-saga-chapter");
            }
            let exec_body = lower_effect_body(body, vocabulary, ids, true, true)?;
            for ch in chapters {
                let num: u8 = match ch.trim() {
                    "I" | "1" => 1,
                    "II" | "2" => 2,
                    "III" | "3" => 3,
                    "IV" | "4" => 4,
                    "V" | "5" => 5,
                    "VI" | "6" => 6,
                    _ => return Err("oracle:unsupported-saga-chapter"),
                };
                program.triggered.push(TriggeredAbility {
                    id: ids.triggered(),
                    condition: TriggerSpec::counters_changed(
                        ObjectFilter::self_object(),
                        CounterTriggerKind::SagaChapter(num),
                    ),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: exec_body.clone(),
                });
            }
            Ok(())
        }
        AbilityAst::ClassLevel { .. } => Err("oracle:unsupported-ability-kind"),
    }
}

fn try_lower_mana_ability(
    cost: &[CostAtomAst],
    body: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ManaOutput> {
    // Extract AddMana + optional SpendRestriction + optional Life/Damage side effect
    let mut mana_spec: Option<&ManaSpecAst> = None;
    let mut restriction: Option<ManaRestriction> = None;
    let mut side_effect: Option<ManaSideEffect> = None;
    for ins in body {
        match ins {
            InstructionAst::AddMana {
                player: PlayerRefAst::You,
                mana,
            } => {
                if mana_spec.is_some() {
                    return None;
                }
                mana_spec = Some(mana);
            }
            InstructionAst::ManaSpendRestriction(r) => {
                if restriction.is_some() {
                    return None;
                }
                restriction = Some(parse_mana_restriction(r)?);
            }
            InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                delta,
                gain: true,
            } => {
                let ValueExprAst::Const(n) = delta.as_ref() else {
                    return None;
                };
                if *n <= 0 || side_effect.is_some() {
                    return None;
                }
                side_effect = Some(ManaSideEffect::GainLife(i64::from(*n).into()));
            }
            InstructionAst::DealDamage {
                source,
                target,
                amount,
            } if matches!(source.as_ref(), SubjectRefAst::SelfObject)
                && matches!(target.as_ref(), SubjectRefAst::Player(PlayerRefAst::You)) =>
            {
                let ValueExprAst::Const(n) = amount.as_ref() else {
                    return None;
                };
                if *n <= 0 || side_effect.is_some() {
                    return None;
                }
                side_effect = Some(ManaSideEffect::DamageController(i64::from(*n).into()));
            }
            _ => return None,
        }
    }
    let spec = mana_spec?;
    let mut output = match spec {
        ManaSpecAst::Fixed(raw) => ManaOutput::Fixed(parse_mana_bundle(raw)?),
        ManaSpecAst::Choice(opts) => {
            let mut colors = BTreeSet::new();
            for o in opts {
                let mt = parse_single_mana_type(o)?;
                if mt == ManaType::Colourless {
                    return None;
                }
                colors.insert(mt);
            }
            if colors.is_empty() {
                return None;
            }
            ManaOutput::ChooseColor {
                amount: 1.into(),
                colors,
            }
        }
        ManaSpecAst::AnyColor {
            amount,
            single_color: true,
        } => {
            let ValueExprAst::Const(n) = amount.as_ref() else {
                return None;
            };
            if *n <= 0 {
                return None;
            }
            ManaOutput::ChooseColor {
                amount: i64::from(*n).into(),
                colors: ManaOutput::all_colors(),
            }
        }
        ManaSpecAst::DifferentColors { amount }
            if matches!(amount.as_ref(), ValueExprAst::Const(2)) =>
        {
            ManaOutput::TwoColors { distinct: true }
        }
        ManaSpecAst::ScaledSymbol { symbol, amount } => {
            let mt = parse_single_mana_type(symbol)?;
            let ValueExprAst::Count(filter) = amount.as_ref() else {
                return None;
            };
            let permanents = lower_filter_to_permanent_selector(filter, vocabulary).ok()?;
            ManaOutput::Counted {
                mana_type: Some(mt),
                permanents,
            }
        }
        ManaSpecAst::CommanderColorIdentity => {
            ManaOutput::SpecialColor(SpecialColorYield::CommanderColorIdentity)
        }
        ManaSpecAst::AnyLandCouldProduce {
            controller: ControllerRelation::Opponent | ControllerRelation::Opponents,
        } => ManaOutput::SpecialColor(SpecialColorYield::OpponentsLandColors),
        _ => return None,
    };

    // Parse cost atoms for mana ability
    let mut has_tap = false;
    let mut pay_mana: Option<ManaCost> = None;
    let mut sac_self = false;
    let mut sac_creature = false;
    let mut pay_life: Option<i64> = None;
    for atom in cost {
        match atom {
            CostAtomAst::TapSelf => {
                if has_tap {
                    return None;
                }
                has_tap = true;
            }
            CostAtomAst::PayMana(raw) => {
                if pay_mana.is_some() {
                    return None;
                }
                pay_mana = Some(parse_fixed_mana_cost(raw)?);
            }
            CostAtomAst::SacrificeSelf => {
                if sac_self {
                    return None;
                }
                sac_self = true;
            }
            CostAtomAst::Sacrifice { count, filter }
                if matches!(count.as_ref(), ValueExprAst::Const(1))
                    && filter.any_types == AstCardTypeMask::CREATURE
                    && filter.subtypes.is_empty()
                    && filter.forbidden_types.is_empty() =>
            {
                if sac_creature {
                    return None;
                }
                sac_creature = true;
            }
            CostAtomAst::PayLife(expr) => {
                let ValueExprAst::Const(n) = expr.as_ref() else {
                    return None;
                };
                if *n <= 0 || pay_life.is_some() {
                    return None;
                }
                pay_life = Some(i64::from(*n));
            }
            _ => return None,
        }
    }

    if pay_life.is_some() {
        // Engine ManaSideEffect models damage-to-controller, not pay-life cost;
        // let paid-life mana abilities fall back cleanly unless exact.
        return None;
    }
    if (sac_self || sac_creature) && (pay_mana.is_some() || side_effect.is_some()) {
        // Engine ManaOutput::valid_for_sacrifice_inner and mana_activation reject combining
        // ManaOutputModifier::Sacrifice with PayMana or SideEffect; fall back to ActivatedAbility.
        return None;
    }
    if let Some(ref mc) = pay_mana
        && !mc.total().is_positive()
        && !matches!(
            output.base(),
            ManaOutput::Dynamic(spellwake_engine::mana::DynamicManaYield::GreatestCreaturePower)
                | ManaOutput::SpecialColor(SpecialColorYield::CommanderColorIdentity)
        )
    {
        return None;
    }

    if let Some(res) = restriction {
        output = output.restricted(res);
    }
    if let Some(se) = side_effect {
        output = output.with_modifier(ManaOutputModifier::SideEffect(se));
    }
    if let Some(mc) = pay_mana {
        output = output.paid(mc);
    }
    if sac_self {
        if !matches!(
            output,
            ManaOutput::Fixed(_)
                | ManaOutput::ChooseColor { .. }
                | ManaOutput::Modified {
                    modifier: ManaOutputModifier::Restrict(
                        spellwake_engine::mana::ManaOutputRestriction::Fixed(_)
                    ),
                    ..
                }
        ) {
            return None;
        }
        output = output.with_modifier(ManaOutputModifier::Sacrifice(ManaSacrificeCost::Source {
            tap: has_tap,
        }));
    } else if sac_creature && !has_tap {
        if !matches!(
            output,
            ManaOutput::Fixed(_)
                | ManaOutput::ChooseColor { .. }
                | ManaOutput::Modified {
                    modifier: ManaOutputModifier::Restrict(
                        spellwake_engine::mana::ManaOutputRestriction::Fixed(_)
                    ),
                    ..
                }
        ) {
            return None;
        }
        output = output.with_modifier(ManaOutputModifier::Sacrifice(
            ManaSacrificeCost::ControlledCreature,
        ));
    } else if !has_tap {
        return None;
    }
    Some(output)
}

fn parse_mana_restriction(raw: &str) -> Option<ManaRestriction> {
    let low = raw.to_ascii_lowercase();
    if low.contains("creature spell") && !low.contains("noncreature") {
        Some(ManaRestriction::CreatureSpell)
    } else if low.contains("noncreature spell") {
        Some(ManaRestriction::NoncreatureSpell)
    } else if low.contains("artifact spell") {
        Some(ManaRestriction::ArtifactSpellOrAbility)
    } else if low.contains("planeswalker spell") {
        Some(ManaRestriction::PlaneswalkerSpell)
    } else if low.contains("colorless spell") {
        Some(ManaRestriction::ColorlessSpell)
    } else if low.contains("commander spell") {
        Some(ManaRestriction::CommanderSpell)
    } else {
        None
    }
}

fn lower_activation_cost(
    cost: &[CostAtomAst],
    zone: Option<&str>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(ManaCost, SourceCost), &'static str> {
    let mut mana_cost = ManaCost::free();
    let mut x_symbols: u8 = 0;
    let mut tap_self = false;
    let mut sac_self = false;
    let mut exile_self = false;
    let mut exile_from_gy = false;
    let mut pay_life: Option<u16> = None;
    let mut discard_self = false;
    let mut discard_mode: Option<DiscardCostMode> = None;
    let mut sac_selected: Option<(SelectedSacrificeFilter, u8)> = None;
    let mut tap_perms: Option<(TapCostFilter, TapCostCount, bool)> = None;
    let mut counter_cost: Option<(CounterKind, CounterCostDelta)> = None;
    let mut mill_count: Option<NonZeroU8> = None;
    let mut exile_gy_cost: Option<GraveyardExileCost> = None;
    let mut pay_energy: Option<u16> = None;
    let mut untap_self = false;
    let mut return_self_to_hand = false;

    for atom in cost {
        match atom {
            CostAtomAst::PayMana(raw) => {
                let (mc, x) = parse_mana_cost_with_x(raw).ok_or("oracle:act-mana-cost")?;
                mana_cost.generic = mana_cost.generic.plus(&mc.generic);
                for (k, v) in mc.required {
                    let entry = mana_cost.required.entry(k).or_insert_with(|| 0.into());
                    *entry = entry.plus(&v);
                }
                x_symbols = x_symbols.checked_add(x).ok_or("oracle:x-ovf")?;
            }
            CostAtomAst::TapSelf => tap_self = true,
            CostAtomAst::UntapSelf => untap_self = true,
            CostAtomAst::PayEnergy(n) if *n > 0 => {
                pay_energy = Some(pay_energy.unwrap_or(0).saturating_add(*n));
            }
            CostAtomAst::BounceToHand(subj)
                if matches!(subj.as_ref(), SubjectRefAst::SelfObject) =>
            {
                return_self_to_hand = true;
            }
            CostAtomAst::SacrificeSelf => sac_self = true,
            CostAtomAst::ExileSelf {
                from_graveyard,
                from_hand: false,
            } => {
                exile_self = true;
                exile_from_gy = *from_graveyard;
            }
            CostAtomAst::Exile { count, filter }
                if filter.zone == Some(ZoneFilter::YourGraveyard) =>
            {
                if matches!(count.as_ref(), ValueExprAst::Const(1))
                    && filter.any_types == AstCardTypeMask::CREATURE
                    && filter.forbidden_types.is_empty()
                    && filter.required_supertypes.is_empty()
                    && filter.forbidden_supertypes.is_empty()
                    && filter.subtypes.is_empty()
                    && filter.forbidden_subtypes.is_empty()
                    && filter.any_colors.is_empty()
                    && filter.forbidden_colors.is_empty()
                    && filter.color_arity.is_none()
                    && filter.required_keywords.is_empty()
                    && filter.forbidden_keywords.is_empty()
                    && filter.stat_bounds.is_empty()
                    && filter.counter_kind.is_none()
                    && filter.flags.0 == 0
                {
                    exile_gy_cost = Some(GraveyardExileCost::Creature {
                        pay_mana_cost: false,
                    });
                } else if matches!(count.as_ref(), ValueExprAst::ChosenX)
                    && (filter.any_card || filter.any_types.is_empty())
                    && filter.forbidden_types.is_empty()
                    && filter.subtypes.is_empty()
                    && filter.any_colors.is_empty()
                    && filter.forbidden_colors.is_empty()
                    && filter.stat_bounds.is_empty()
                    && filter.flags.0 == 0
                {
                    exile_gy_cost = Some(GraveyardExileCost::AnyCountX);
                } else if let ValueExprAst::Const(n) = count.as_ref()
                    && *n > 0
                    && let Ok(u) = u8::try_from(*n)
                    && let Some(nz) = NonZeroU8::new(u)
                {
                    let mut inner = filter.as_ref().clone();
                    inner.zone = None;
                    let obj = if inner.is_empty() || inner.any_card {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    exile_gy_cost = Some(GraveyardExileCost::Filtered {
                        filter: obj,
                        count: nz,
                    });
                } else {
                    return Err("oracle:unsupported-activation-cost");
                }
            }
            CostAtomAst::DiscardSelf => discard_self = true,
            CostAtomAst::DiscardHand => discard_mode = Some(DiscardCostMode::WholeHand),
            CostAtomAst::Discard {
                count,
                filter,
                random: false,
            } if matches!(count.as_ref(), ValueExprAst::Const(n) if *n > 0) => {
                let ValueExprAst::Const(n) = count.as_ref() else {
                    unreachable!()
                };
                let n_u8 = u8::try_from(*n).map_err(|_| "oracle:discard-ovf")?;
                let nz = NonZeroU8::new(n_u8).ok_or("oracle:discard-zero")?;
                let plain_any = filter.any_types.is_empty()
                    && filter.forbidden_types.is_empty()
                    && filter.required_supertypes.is_empty()
                    && filter.forbidden_supertypes.is_empty()
                    && filter.subtypes.is_empty()
                    && filter.forbidden_subtypes.is_empty()
                    && filter.any_colors.is_empty()
                    && filter.forbidden_colors.is_empty()
                    && filter.color_arity.is_none()
                    && filter.required_keywords.is_empty()
                    && filter.forbidden_keywords.is_empty()
                    && filter.stat_bounds.is_empty()
                    && filter.counter_kind.is_none();
                if n_u8 == 1 && plain_any {
                    discard_mode = Some(DiscardCostMode::SelectedOne);
                } else {
                    let obj = if plain_any {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(filter, vocabulary)?
                    };
                    discard_mode = Some(DiscardCostMode::Filtered {
                        filter: obj,
                        count: nz,
                    });
                }
            }
            CostAtomAst::PayLife(expr) => {
                let ValueExprAst::Const(n) = expr.as_ref() else {
                    return Err("oracle:act-dynamic-life-cost");
                };
                if *n <= 0 {
                    return Err("oracle:act-nonpos-life");
                }
                pay_life = Some(u16::try_from(*n).map_err(|_| "oracle:life-ovf")?);
            }
            CostAtomAst::Sacrifice { count, filter } => {
                let ValueExprAst::Const(n) = count.as_ref() else {
                    return Err("oracle:act-dynamic-sac");
                };
                let n_u8 = u8::try_from(*n).map_err(|_| "oracle:sac-ovf")?;
                if n_u8 == 0 {
                    return Err("oracle:sac-zero");
                }
                let ssf = lower_selected_sacrifice_filter(filter, vocabulary)?;
                sac_selected = Some((ssf, n_u8));
            }
            CostAtomAst::TapPermanent { count, filter } => {
                let tc = match count.as_ref() {
                    ValueExprAst::Const(n) if *n > 0 => TapCostCount::Fixed(
                        NonZeroU8::new(u8::try_from(*n).map_err(|_| "oracle:tap-ovf")?)
                            .ok_or("oracle:tap-zero")?,
                    ),
                    ValueExprAst::ChosenX => TapCostCount::ChosenX,
                    _ => return Err("oracle:act-dynamic-tap"),
                };
                let tcf = lower_tap_cost_filter(filter, vocabulary)?;
                let exclude_self = filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0;
                tap_perms = Some((tcf, tc, exclude_self));
            }
            CostAtomAst::RemoveCounters {
                all,
                count,
                counter_kind,
                subject,
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
                let ck = parse_counter_kind(counter_kind).ok_or("oracle:act-counter-kind")?;
                let delta = if *all {
                    CounterCostDelta::RemoveAll
                } else if let Some(c) = count {
                    let ValueExprAst::Const(n) = c.as_ref() else {
                        return Err("oracle:act-dynamic-counter");
                    };
                    CounterCostDelta::Remove(
                        NonZeroU8::new(u8::try_from(*n).map_err(|_| "oracle:cnt-ovf")?)
                            .ok_or("oracle:cnt-zero")?,
                    )
                } else {
                    return Err("oracle:act-counter-missing-count");
                };
                counter_cost = Some((ck, delta));
            }
            CostAtomAst::PutCounters {
                count,
                counter_kind,
                subject,
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
                let ck = parse_counter_kind(counter_kind).ok_or("oracle:act-counter-kind")?;
                let ValueExprAst::Const(n) = count.as_ref() else {
                    return Err("oracle:act-dynamic-counter");
                };
                let delta = CounterCostDelta::Add(
                    NonZeroU8::new(u8::try_from(*n).map_err(|_| "oracle:cnt-ovf")?)
                        .ok_or("oracle:cnt-zero")?,
                );
                counter_cost = Some((ck, delta));
            }
            CostAtomAst::Mill(expr) => {
                let ValueExprAst::Const(n) = expr.as_ref() else {
                    return Err("oracle:act-dynamic-mill");
                };
                mill_count = Some(
                    NonZeroU8::new(u8::try_from(*n).map_err(|_| "oracle:mill-ovf")?)
                        .ok_or("oracle:mill-zero")?,
                );
            }
            _ => return Err("oracle:unsupported-activation-cost"),
        }
    }

    let from_gy = zone == Some("graveyard") || exile_from_gy;
    let from_hand = zone == Some("hand");

    // Count non-tap/non-mana extra cost components to make sure they combine cleanly
    let non_mana_count = usize::from(exile_self)
        + usize::from(exile_gy_cost.is_some())
        + usize::from(discard_self)
        + usize::from(discard_mode.is_some())
        + usize::from(tap_perms.is_some())
        + usize::from(counter_cost.is_some())
        + usize::from(mill_count.is_some())
        + usize::from(pay_energy.is_some())
        + usize::from(untap_self)
        + usize::from(return_self_to_hand)
        + usize::from(x_symbols > 0);
    if non_mana_count > 1 {
        return Err("oracle:compound-activation-cost");
    }

    if let Some(amount) = pay_energy {
        if sac_self || pay_life.is_some() || sac_selected.is_some() || from_gy || from_hand {
            return Err("oracle:compound-energy-cost");
        }
        return Ok((
            mana_cost,
            SourceCost::PayEnergy {
                amount,
                tap_source: tap_self,
            },
        ));
    }
    if untap_self {
        if tap_self
            || sac_self
            || pay_life.is_some()
            || sac_selected.is_some()
            || from_gy
            || from_hand
        {
            return Err("oracle:compound-untap-cost");
        }
        return Ok((mana_cost, SourceCost::Untap));
    }
    if return_self_to_hand {
        if sac_self || pay_life.is_some() || sac_selected.is_some() || from_gy || from_hand {
            return Err("oracle:compound-bounce-cost");
        }
        return Ok((
            mana_cost,
            SourceCost::ReturnSelfToHand {
                tap_source: tap_self,
            },
        ));
    }

    if let Some(filter) = exile_gy_cost {
        if sac_self || pay_life.is_some() || sac_selected.is_some() || from_gy || from_hand {
            return Err("oracle:compound-exile-gy-cost");
        }
        return Ok((
            mana_cost,
            SourceCost::ExileGraveyard {
                filter,
                tap_source: tap_self,
            },
        ));
    }

    if discard_self {
        if tap_self || sac_self || pay_life.is_some() || sac_selected.is_some() || from_gy {
            return Err("oracle:compound-discard-self");
        }
        return Ok((
            mana_cost,
            SourceCost::HandDiscardSelf(HandDiscardKeyword::Channel),
        ));
    }
    if from_hand {
        return Err("oracle:unsupported-hand-activation");
    }
    if exile_self {
        if sac_self || pay_life.is_some() || sac_selected.is_some() {
            return Err("oracle:compound-exile-self");
        }
        return Ok((
            mana_cost,
            SourceCost::Exile {
                tap_source: tap_self,
                from_graveyard: from_gy,
            },
        ));
    }
    if let Some((kind, delta)) = counter_cost {
        if pay_life.is_some() || sac_selected.is_some() || from_gy {
            return Err("oracle:compound-counter-cost");
        }
        return Ok((
            mana_cost,
            SourceCost::Counter {
                kind,
                delta,
                tap_source: tap_self,
                sacrifice_source: sac_self,
            },
        ));
    }
    if let Some(amount) = pay_life {
        if sac_self || from_gy {
            return Err("oracle:compound-life-cost");
        }
        let sac = match sac_selected {
            None => None,
            Some((f, 1)) => Some(f),
            Some(_) => return Err("oracle:life-multi-sac"),
        };
        return Ok((
            mana_cost,
            SourceCost::PayLife {
                amount: LifeCostAmount::Fixed(amount),
                tap_source: tap_self,
                sacrifice: sac,
            },
        ));
    }
    if x_symbols > 0 {
        if sac_self || sac_selected.is_some() || from_gy {
            return Err("oracle:compound-x-cost");
        }
        return Ok((
            mana_cost,
            SourceCost::GenericX {
                symbols: x_symbols,
                tap_source: tap_self,
            },
        ));
    }
    if let Some((filter, count, exclude_source)) = tap_perms {
        if sac_self || sac_selected.is_some() || from_gy {
            return Err("oracle:compound-tap-perms");
        }
        return Ok((
            mana_cost,
            SourceCost::TapPermanents {
                filter,
                count,
                tap_source: tap_self,
                exclude_source,
            },
        ));
    }
    if let Some((filter, count)) = sac_selected {
        if sac_self {
            return Err("oracle:compound-sac");
        }
        return Ok((
            mana_cost,
            SourceCost::SacrificeSelected {
                filter,
                count,
                tap_source: tap_self,
                from_graveyard: from_gy,
            },
        ));
    }
    if let Some(mode) = discard_mode {
        if sac_self || from_gy {
            return Err("oracle:compound-discard");
        }
        return Ok((
            mana_cost,
            SourceCost::Discard {
                mode,
                tap_source: tap_self,
            },
        ));
    }
    if let Some(count) = mill_count {
        if sac_self || from_gy {
            return Err("oracle:compound-mill");
        }
        return Ok((
            mana_cost,
            SourceCost::Mill {
                count,
                tap_source: tap_self,
            },
        ));
    }
    if from_gy {
        if tap_self || sac_self {
            return Err("oracle:gy-tap-or-sac");
        }
        return Ok((mana_cost, SourceCost::GraveyardSelf { unearth: false }));
    }

    let source_cost = match (tap_self, sac_self) {
        (false, false) => SourceCost::None,
        (true, false) => SourceCost::Tap,
        (false, true) => SourceCost::Sacrifice,
        (true, true) => SourceCost::TapSacrifice,
    };
    Ok((mana_cost, source_cost))
}

fn lower_selected_sacrifice_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SelectedSacrificeFilter, &'static str> {
    if !f.forbidden_types.is_empty()
        || !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || f.token.is_some()
    {
        return Err("oracle:unsupported-sac-cost-filter");
    }
    let exclude_self = f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0;
    if f.subtypes.len() == 1 {
        let st = &f.subtypes[0];
        if st.eq_ignore_ascii_case("desert") {
            return Ok(SelectedSacrificeFilter::Desert);
        }
        if let Some(ct) = vocabulary.resolve_label(st) {
            return Ok(SelectedSacrificeFilter::Subtype(ct));
        }
        return Err("oracle:unsupported-sac-subtype");
    }
    if !f.subtypes.is_empty() {
        return Err("oracle:multi-sac-subtype");
    }
    if f.any_types == AstCardTypeMask::CREATURE {
        return Ok(if exclude_self {
            SelectedSacrificeFilter::OtherCreature
        } else {
            SelectedSacrificeFilter::Creature
        });
    }
    if f.any_types == AstCardTypeMask::ARTIFACT && !exclude_self {
        return Ok(SelectedSacrificeFilter::Artifact);
    }
    if f.any_types == AstCardTypeMask::ENCHANTMENT && exclude_self {
        return Ok(SelectedSacrificeFilter::OtherEnchantment);
    }
    if f.any_types == AstCardTypeMask::ARTIFACT.union(AstCardTypeMask::CREATURE) && !exclude_self {
        return Ok(SelectedSacrificeFilter::ArtifactOrCreature);
    }
    if f.any_types == AstCardTypeMask::CREATURE.union(AstCardTypeMask::ENCHANTMENT) && !exclude_self
    {
        return Ok(SelectedSacrificeFilter::CreatureOrEnchantment);
    }
    Err("oracle:unsupported-sac-cost-filter")
}

fn lower_tap_cost_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<TapCostFilter, &'static str> {
    if f.subtypes.len() == 1 {
        let ct = vocabulary
            .resolve_label(&f.subtypes[0])
            .ok_or("oracle:unsupported-tap-subtype")?;
        return Ok(TapCostFilter::Subtype(ct));
    }
    if f.required_supertypes == AstSupertypeMask::LEGENDARY && f.any_types.is_empty() {
        return Ok(TapCostFilter::Legendary);
    }
    if f.any_types == AstCardTypeMask::CREATURE {
        return Ok(TapCostFilter::Creature);
    }
    if f.any_types == AstCardTypeMask::ARTIFACT {
        return Ok(TapCostFilter::Artifact);
    }
    Err("oracle:unsupported-tap-cost-filter")
}

fn lower_trigger_spec(
    trig: &TriggerSpecAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(TriggerSpec, TriggerGrouping, Option<InterveningCondition>), &'static str> {
    if trig.events.is_empty() {
        return Err("oracle:empty-trigger");
    }
    let mut grouping = TriggerGrouping::EachOccurrence;
    let mut events = Vec::with_capacity(trig.events.len());
    for ev in &trig.events {
        let (e, g) = lower_trigger_event(ev, vocabulary)?;
        if g != TriggerGrouping::EachOccurrence {
            grouping = g;
        }
        events.push(e);
    }
    let mut event = if events.len() == 1 {
        events.pop().ok_or("oracle:unreachable")?
    } else {
        TriggerEvent::AnyOf(events)
    };
    let mut consumed_if = false;
    if let (
        TriggerEvent::Enters {
            cast_origin: origin @ None,
            ..
        },
        Some(cond),
    ) = (&mut event, &trig.intervening_if)
    {
        match cond {
            ConditionAst::WasKicked => {
                *origin = Some(CastEntryKind::Kicked);
                consumed_if = true;
            }
            ConditionAst::WasCast { from_hand, .. } => {
                *origin = Some(CastEntryKind::Cast {
                    from_hand: *from_hand,
                });
                consumed_if = true;
            }
            _ => {}
        }
    }
    let mut spec = TriggerSpec::new(event);
    let mut guard = None;
    if !consumed_if && let Some(cond) = &trig.intervening_if {
        let tc = lower_trigger_condition(cond, vocabulary)?;
        guard = InterveningCondition::from_trigger_condition(&tc);
        spec = spec.only_if(tc);
    }
    Ok((spec, grouping, guard))
}

fn lower_trigger_event(
    ev: &TriggerEventAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(TriggerEvent, TriggerGrouping), &'static str> {
    match ev {
        TriggerEventAst::ZoneChange {
            from,
            to,
            subject,
            grouped,
            entering_controller,
        } => {
            let grouping = if *grouped {
                TriggerGrouping::OneOrMoreTogether
            } else {
                TriggerGrouping::EachOccurrence
            };
            let mut filter = lower_trigger_subject_filter(subject, vocabulary)?;
            if let Some(ctrl) = entering_controller {
                let cf = match ctrl {
                    ControllerRelation::You => ControllerFilter::You,
                    ControllerRelation::Opponent | ControllerRelation::Opponents => {
                        ControllerFilter::Opponent
                    }
                    _ => return Err("oracle:trigger-entering-controller"),
                };
                if let ObjectFilter::Clause(ref mut c) = filter {
                    c.controller = cf;
                }
            }
            match (from.as_deref(), to.as_deref()) {
                (None, Some("battlefield")) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::Any,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (Some("graveyard"), Some("battlefield")) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::Graveyard,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (Some("hand"), Some("battlefield")) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::Hand,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (Some("exile"), Some("battlefield")) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::Exile,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (Some("library"), Some("battlefield")) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::Library,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (Some("battlefield"), None) => Ok((
                    TriggerEvent::LeavesBattlefield { subject: filter },
                    grouping,
                )),
                (Some("battlefield"), Some("hand")) => {
                    Ok((TriggerEvent::ReturnedToHand { subject: filter }, grouping))
                }
                (from_z, Some("exile")) => {
                    let fz = match from_z {
                        None => TriggerFromZone::Any,
                        Some("battlefield") => TriggerFromZone::Battlefield,
                        Some("graveyard") => TriggerFromZone::Graveyard,
                        Some("hand") => TriggerFromZone::Hand,
                        Some("library") => TriggerFromZone::Library,
                        _ => return Err("oracle:unsupported-zone-trigger"),
                    };
                    Ok((
                        TriggerEvent::Exiled {
                            subject: filter,
                            from_zone: fz,
                        },
                        grouping,
                    ))
                }
                (Some("battlefield"), Some("graveyard")) => {
                    let mode = if matches!(subject.as_ref(), SubjectRefAst::SelfObject) {
                        DiesTriggerMode::Creature
                    } else if let ObjectFilter::Clause(c) = filter
                        && !c.any_types.contains(CardType::Creature)
                        && !c.any_types.is_empty()
                    {
                        DiesTriggerMode::Permanent
                    } else {
                        DiesTriggerMode::Creature
                    };
                    Ok((
                        TriggerEvent::Dies {
                            subject: filter,
                            mode,
                        },
                        grouping,
                    ))
                }
                (Some("library"), Some("graveyard")) => Ok((
                    TriggerEvent::PutIntoGraveyard {
                        subject: filter,
                        player: PlayerRelation::AnyParticipant,
                        from_library: true,
                        own_turn_only: false,
                    },
                    grouping,
                )),
                (None, Some("graveyard")) => Ok((
                    TriggerEvent::PutIntoGraveyard {
                        subject: filter,
                        player: PlayerRelation::AnyParticipant,
                        from_library: false,
                        own_turn_only: false,
                    },
                    grouping,
                )),
                _ => Err("oracle:unsupported-zone-trigger"),
            }
        }
        TriggerEventAst::StepBegan { step, whose } => {
            let st = match step {
                TurnStepAst::Upkeep => Step::Upkeep,
                TurnStepAst::DrawStep => Step::Draw,
                TurnStepAst::BeginningOfCombat => Step::BeginningCombat,
                TurnStepAst::EndOfCombat => Step::EndCombat,
                TurnStepAst::EndStep => Step::End,
                TurnStepAst::PrecombatMain => Step::PrecombatMain,
                TurnStepAst::PostcombatMain => Step::PostcombatMain,
                _ => return Err("oracle:unsupported-trigger-step"),
            };
            let scope = match whose.to_ascii_lowercase().as_str() {
                "your" | "you" => TriggerPlayerScope::Relation(PlayerRelation::Controller),
                "each" | "each_player" | "each player's" | "any" => {
                    TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant)
                }
                "each_opponent" | "each opponent's" | "an opponent's" | "opponent" => {
                    TriggerPlayerScope::Relation(PlayerRelation::Opponent)
                }
                "enchanted_player" | "enchanted player's" => TriggerPlayerScope::EnchantedPlayer,
                _ => return Err("oracle:unsupported-step-whose"),
            };
            Ok((
                TriggerEvent::StepBegan {
                    step: st,
                    player: scope,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::SpellCast {
            caster,
            spell,
            self_spell,
            targets: None,
            nth_each_turn,
            include_copy,
        } => {
            let scope = match caster {
                PlayerRefAst::You => TriggerPlayerScope::Relation(PlayerRelation::Controller),
                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                    TriggerPlayerScope::Relation(PlayerRelation::Opponent)
                }
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => {
                    TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant)
                }
                _ => return Err("oracle:unsupported-spell-caster"),
            };
            let (sf, min_mv) = match spell {
                Some(f) => {
                    let mut cloned = f.as_ref().clone();
                    let mut min_mv = None;
                    if cloned.stat_bounds.len() == 1
                        && cloned.stat_bounds[0].stat == StatKind::ManaValue
                        && cloned.stat_bounds[0].op == CompareOp::Ge
                        && let ValueExprAst::Const(v) = cloned.stat_bounds[0].value.as_ref()
                        && let Ok(u) = u16::try_from(*v)
                        && u > 0
                    {
                        min_mv = Some(u);
                        cloned.stat_bounds.clear();
                    }
                    (lower_spell_filter(&cloned, vocabulary)?, min_mv)
                }
                None => (SpellFilter::any(), None),
            };
            let mut sf = sf;
            if let Some(nth) = nth_each_turn {
                let nz = NonZeroU32::new(u32::from(*nth)).ok_or("oracle:zero-nth")?;
                sf = sf.ordinal_this_turn(nz);
            }
            Ok((
                TriggerEvent::SpellCast {
                    caster: scope,
                    filter: sf,
                    self_cast: *self_spell,
                    include_copy: *include_copy,
                    min_mana_value: min_mv,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Attacks {
            subject,
            actor: None | Some(PlayerRefAst::You),
            qualifier,
            grouped,
        } => {
            let subj = match subject {
                Some(s) => lower_trigger_subject_filter(s, vocabulary)?,
                None => ObjectFilter::creature().controlled(),
            };
            let mode = if qualifier
                .as_deref()
                .is_some_and(|q| q.eq_ignore_ascii_case("alone"))
            {
                AttackTriggerMode::Alone
            } else if qualifier.is_some() {
                return Err("oracle:unsupported-attack-qualifier");
            } else {
                AttackTriggerMode::Individual {
                    capture_defender: false,
                }
            };
            let grouping = if *grouped {
                TriggerGrouping::OneOrMoreTogether
            } else {
                TriggerGrouping::EachOccurrence
            };
            Ok((
                TriggerEvent::Attacks {
                    subject: subj,
                    mode,
                },
                grouping,
            ))
        }
        TriggerEventAst::AttacksWithOthers {
            subject,
            min_others,
            other_filter,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            && (other_filter.is_empty()
                || (other_filter.any_types == AstCardTypeMask::CREATURE
                    && other_filter.forbidden_types.is_empty()
                    && other_filter.subtypes.is_empty()
                    && other_filter.any_colors.is_empty()
                    && other_filter.forbidden_colors.is_empty())) =>
        {
            Ok((
                TriggerEvent::Attacks {
                    subject: ObjectFilter::self_object(),
                    mode: AttackTriggerMode::WithAtLeastOthers(*min_others),
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::AttacksUnblocked { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind: BlockTriggerKind::Unblocked,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Blocks {
            subject,
            attacker: None,
        } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind: BlockTriggerKind::Blocks,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::BecomesBlocked {
            subject,
            blocker: None,
        } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind: BlockTriggerKind::BecomesBlocked {
                        capture_defender: false,
                    },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::DamageDealt {
            combat_only,
            noncombat_only: false,
            source,
            recipient,
            grouped,
        } => {
            // "Whenever ~ / a creature you control is dealt damage..." -> TriggerEvent::DealtDamage
            if source.is_none()
                && !*combat_only
                && !*grouped
                && let Some(rec_subj) = recipient.as_deref()
                && !matches!(
                    rec_subj,
                    SubjectRefAst::Player(_) | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                )
                && let Ok(subj_filter) = lower_trigger_subject_filter(rec_subj, vocabulary)
            {
                return Ok((
                    TriggerEvent::DealtDamage {
                        subject: subj_filter,
                        linked_player: false,
                    },
                    TriggerGrouping::EachOccurrence,
                ));
            }
            let src = match source {
                Some(s) => lower_trigger_subject_filter(s, vocabulary)?,
                None => ObjectFilter::any(),
            };
            let rec = match recipient.as_deref() {
                None => DamageTriggerRecipient::Any,
                Some(SubjectRefAst::Player(
                    PlayerRefAst::AnyPlayer
                    | PlayerRefAst::EachPlayer
                    | PlayerRefAst::TargetPlayer
                    | PlayerRefAst::ThatPlayer,
                )) => DamageTriggerRecipient::AnyPlayer,
                Some(SubjectRefAst::Player(
                    PlayerRefAst::AnyOpponent
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::DefendingPlayer,
                )) => DamageTriggerRecipient::Opponent,
                Some(SubjectRefAst::Player(PlayerRefAst::You)) => {
                    DamageTriggerRecipient::Controller
                }
                Some(SubjectRefAst::Player(PlayerRefAst::EnchantedPlayer)) => {
                    DamageTriggerRecipient::EnchantedPlayer
                }
                Some(SubjectRefAst::TargetPlayerOrPlaneswalker { .. }) => {
                    DamageTriggerRecipient::PlayerOrPlaneswalker
                }
                Some(SubjectRefAst::Compound(items))
                    if items.len() == 2
                        && items.iter().any(|s| {
                            matches!(
                                s,
                                SubjectRefAst::Player(
                                    PlayerRefAst::AnyPlayer
                                        | PlayerRefAst::EachPlayer
                                        | PlayerRefAst::AnyOpponent
                                        | PlayerRefAst::EachOpponent
                                )
                            )
                        })
                        && items.iter().any(|s| {
                            matches!(
                                s,
                                SubjectRefAst::Group(f)
                                    | SubjectRefAst::Each(f)
                                    | SubjectRefAst::All(f)
                                    if f.any_types == AstCardTypeMask::PLANESWALKER
                            )
                        }) =>
                {
                    DamageTriggerRecipient::PlayerOrPlaneswalker
                }
                Some(SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f))
                    if f.any_types == AstCardTypeMask::CREATURE =>
                {
                    DamageTriggerRecipient::Creature
                }
                _ => return Err("oracle:unsupported-damage-recipient"),
            };
            let grouping = if *grouped {
                TriggerGrouping::OneOrMoreByPlayer
            } else {
                TriggerGrouping::EachOccurrence
            };
            Ok((
                TriggerEvent::DealsDamage {
                    source: src,
                    recipient: rec,
                    combat_only: *combat_only,
                },
                grouping,
            ))
        }
        TriggerEventAst::LifeChanged {
            player,
            gained,
            first_each_turn,
        } => {
            let rel = lower_player_relation(*player)?;
            let kind = if *gained {
                LifeChangeKind::Gain
            } else {
                LifeChangeKind::Loss
            };
            let grouping = if *first_each_turn {
                TriggerGrouping::OneOrMoreOnceEachTurn
            } else {
                TriggerGrouping::EachOccurrence
            };
            Ok((TriggerEvent::LifeChanged { kind, player: rel }, grouping))
        }
        TriggerEventAst::CardDrawn { player, qualifier } => {
            let rel = lower_player_relation(*player)?;
            let nth = match qualifier.to_ascii_lowercase().as_str() {
                "" | "any" => None,
                "second" | "2" => NonZeroU16::new(2),
                _ => return Err("oracle:unsupported-draw-qualifier"),
            };
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::DrewCard { nth_each_turn: nth },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::CardDiscarded { player, filter }
            if filter.is_empty() || filter.any_card =>
        {
            let rel = lower_player_relation(*player)?;
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::Discarded,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::PermanentSacrificed { player, subject } => {
            let rel = lower_player_relation(*player)?;
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::Sacrificed(subj),
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Cycled { subject } => {
            let self_only = matches!(subject.as_ref(), SubjectRefAst::SelfObject);
            Ok((
                TriggerEvent::PlayerAction {
                    player: PlayerRelation::Controller,
                    action: PlayerTriggerAction::Cycled { self_only },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::TapStateChanged {
            subject,
            tapped,
            for_mana: false,
        } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: if *tapped {
                        StatusTriggerKind::BecomesTapped
                    } else {
                        StatusTriggerKind::BecomesUntapped
                    },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::CrimeCommitted { player } => {
            let rel = lower_player_relation(*player)?;
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::CommittedCrime,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::TurnedFaceUp { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::TurnsFaceUp,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Transforms { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::Transforms,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::PhaseChanged {
            subject,
            phased_out,
        } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::Phased { out: *phased_out },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Mutates { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::Mutated,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::KeywordActionPerformed { player, action } => {
            let rel = lower_player_relation(*player)?;
            let act = match action.trim().to_ascii_lowercase().as_str() {
                "scry" | "scries" => PlayerTriggerAction::Scried,
                "surveil" | "surveils" => PlayerTriggerAction::Surveilled,
                "cycle" | "cycles" | "cycle a card" | "cycles a card" => {
                    PlayerTriggerAction::Cycled { self_only: false }
                }
                "discard" | "discards" | "discard a card" | "discards a card" => {
                    PlayerTriggerAction::Discarded
                }
                "commit a crime" | "commits a crime" => PlayerTriggerAction::CommittedCrime,
                "draw a card" | "draws a card" => PlayerTriggerAction::DrewCard {
                    nth_each_turn: None,
                },
                _ => return Err("oracle:unsupported-trigger-event"),
            };
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: act,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::BecomesTargeted {
            subject,
            by_opponent,
            by_text,
        } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            let (spell_only, inst_sorc) =
                match by_text.as_deref().map(str::to_ascii_lowercase).as_deref() {
                    None | Some("a spell or ability") => (false, false),
                    Some("a spell") => (true, false),
                    Some("an instant or sorcery spell") => (true, true),
                    _ => return Err("oracle:unsupported-targeted-by"),
                };
            Ok((
                TriggerEvent::Targeted {
                    subject: subj,
                    by_opponent: *by_opponent,
                    spell_only,
                    instant_or_sorcery_only: inst_sorc,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::CountersChanged {
            subject,
            counter_kind,
            added: true,
            by_you,
        } if counter_kind == "+1/+1" => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::CountersChanged {
                    subject: subj,
                    kind: CounterTriggerKind::PlusOnePlaced {
                        by_controller_only: *by_you,
                    },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::TapStateChanged {
            subject,
            tapped: true,
            for_mana: true,
        } => {
            if let SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f) =
                subject.as_ref()
                && f.any_types == AstCardTypeMask::ARTIFACT
                && f.forbidden_types.is_empty()
                && f.subtypes.is_empty()
            {
                let rel = match f.controller {
                    None => PlayerRelation::AnyParticipant,
                    Some(ControllerRelation::You) => PlayerRelation::Controller,
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                        PlayerRelation::Opponent
                    }
                    _ => return Err("oracle:unsupported-trigger-event"),
                };
                return Ok((
                    TriggerEvent::PlayerAction {
                        player: rel,
                        action: PlayerTriggerAction::ArtifactTappedForMana,
                    },
                    TriggerGrouping::EachOccurrence,
                ));
            }
            Err("oracle:unsupported-trigger-event")
        }
        TriggerEventAst::StateTrigger(cond) => match cond.as_ref() {
            ConditionAst::CounterCompare {
                subject,
                counter_kind,
                op,
                value,
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
                let kind =
                    parse_counter_kind(counter_kind).ok_or("oracle:unsupported-trigger-event")?;
                let amount =
                    u8::try_from(*value).map_err(|_| "oracle:unsupported-trigger-event")?;
                Ok((
                    TriggerEvent::StateThreshold(StateThresholdKind::SourceCounters {
                        kind,
                        comparison: lower_comparison(*op),
                        amount,
                    }),
                    TriggerGrouping::EachOccurrence,
                ))
            }
            ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter,
                op: CompareOp::Ge,
                value,
            } if *value > 0
                && filter.subtypes.len() == 1
                && filter.forbidden_types.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty() =>
            {
                let ct = vocabulary
                    .resolve_label(&filter.subtypes[0])
                    .ok_or("oracle:unsupported-trigger-event")?;
                Ok((
                    TriggerEvent::StateThreshold(StateThresholdKind::ControlledSubtype {
                        subtype: ct,
                        minimum: *value,
                    }),
                    TriggerGrouping::EachOccurrence,
                ))
            }
            _ => Err("oracle:unsupported-trigger-event"),
        },
        TriggerEventAst::PlaneswalkToSelf => Ok((
            TriggerEvent::Planar(PlanarTriggerEvent::PlaneswalkToSelf),
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::PlaneswalkAwayFromSelf => Ok((
            TriggerEvent::Planar(PlanarTriggerEvent::PlaneswalkAwayFromSelf),
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::ChaosEnsues => Ok((
            TriggerEvent::Planar(PlanarTriggerEvent::ChaosEnsues),
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::EncounterPhenomenon => Ok((
            TriggerEvent::Planar(PlanarTriggerEvent::EncounterPhenomenon),
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::SchemeSetInMotion => Ok((
            TriggerEvent::SchemeSetInMotion,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::ManualTabletopClaim {
            prompt,
            any_player_can_trigger,
        } => Ok((
            TriggerEvent::ManualTabletopClaim {
                prompt: prompt.clone().into_boxed_str(),
                any_player_can_trigger: *any_player_can_trigger,
            },
            TriggerGrouping::EachOccurrence,
        )),
        _ => Err("oracle:unsupported-trigger-event"),
    }
}

fn lower_trigger_subject_filter(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ObjectFilter, &'static str> {
    match subject {
        SubjectRefAst::SelfObject => Ok(ObjectFilter::self_object()),
        SubjectRefAst::AttachedHost(_) => {
            Ok(ObjectFilter::Clause(ObjectClause::any().attached_to_self()))
        }
        SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f) => {
            lower_object_filter(f, vocabulary)
        }
        _ => Err("oracle:unsupported-trigger-subject"),
    }
}

fn lower_player_relation(p: PlayerRefAst) -> Result<PlayerRelation, &'static str> {
    match p {
        PlayerRefAst::You => Ok(PlayerRelation::Controller),
        PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => Ok(PlayerRelation::Opponent),
        PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => Ok(PlayerRelation::AnyParticipant),
        _ => Err("oracle:unsupported-player-relation"),
    }
}

fn lower_trigger_condition(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<TriggerCondition, &'static str> {
    match cond {
        ConditionAst::IsYourTurn => Ok(TriggerCondition::ControllersTurn),
        ConditionAst::Not(inner) if matches!(inner.as_ref(), ConditionAst::IsYourTurn) => {
            Ok(TriggerCondition::OpponentsTurn)
        }
        ConditionAst::Delirium => Ok(TriggerCondition::Delirium),
        ConditionAst::Metalcraft => Ok(TriggerCondition::Metalcraft),
        ConditionAst::Threshold => Ok(TriggerCondition::Threshold),
        ConditionAst::Morbid {
            your_control_only: false,
        } => Ok(TriggerCondition::Morbid),
        ConditionAst::ControlsCommander => Ok(TriggerCondition::ControlsOwnCommander),
        ConditionAst::Marker(GameMarker::CitysBlessing) => {
            Ok(TriggerCondition::ControllerHasCityBlessing)
        }
        ConditionAst::Marker(GameMarker::Monarch(PlayerRefAst::You)) => {
            Ok(TriggerCondition::Monarch { opponent: false })
        }
        ConditionAst::Marker(GameMarker::Monarch(
            PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
        )) => Ok(TriggerCondition::Monarch { opponent: true }),
        ConditionAst::OpponentLostLifeThisTurn => Ok(TriggerCondition::OpponentLostLifeAtLeast(1)),
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Eq,
            value: 0,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0 =>
        {
            Ok(TriggerCondition::NoOtherControlledCreatures)
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty()
            && filter.stat_bounds.len() == 1
            && filter.stat_bounds[0].stat == StatKind::Power
            && filter.stat_bounds[0].op == CompareOp::Ge
            && matches!(filter.stat_bounds[0].value.as_ref(), ValueExprAst::Const(p) if *p > 0) =>
        {
            let ValueExprAst::Const(p) = filter.stat_bounds[0].value.as_ref() else {
                return Err("oracle:unsupported-trigger-condition");
            };
            Ok(TriggerCondition::ControlledCreaturePowerAtLeast(
                u16::try_from(*p).map_err(|_| "oracle:stat-ovf")?,
            ))
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.any_types == AstCardTypeMask::CREATURE
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.flags.is_empty() =>
        {
            Ok(TriggerCondition::ControlledCreatureCount(*value))
        }
        ConditionAst::TotalStatCompare {
            filter,
            stat: StatKind::Power,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.any_types == AstCardTypeMask::CREATURE
            && filter.controller == Some(ControllerRelation::You)
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty() =>
        {
            Ok(TriggerCondition::TotalControlledPowerAtLeast(
                u16::try_from(*value).map_err(|_| "oracle:stat-ovf")?,
            ))
        }
        ConditionAst::OpponentMoreThanYou { metric, .. }
            if metric.eq_ignore_ascii_case("lands") =>
        {
            Ok(TriggerCondition::OpponentHasMoreLands)
        }
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op: CompareOp::Le,
            value,
        } => Ok(TriggerCondition::HandSizeBelow(
            u8::try_from(*value + 1).map_err(|_| "oracle:hand-ovf")?,
        )),
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op: CompareOp::Lt,
            value,
        } => Ok(TriggerCondition::HandSizeBelow(
            u8::try_from(*value).map_err(|_| "oracle:hand-ovf")?,
        )),
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("saddled") =>
        {
            Ok(TriggerCondition::SourceSaddled)
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op: CompareOp::Ge,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) && *value > 0 => {
            let kind =
                parse_counter_kind(counter_kind).ok_or("oracle:unsupported-trigger-condition")?;
            let min_u8 =
                u8::try_from(*value).map_err(|_| "oracle:unsupported-trigger-condition")?;
            Ok(TriggerCondition::SourceCounterAtLeast {
                kind,
                minimum: min_u8,
            })
        }
        ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
            subject,
            or_entered_this_turn: false,
        }) if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            Ok(TriggerCondition::SourceDidNotAttackThisTurn)
        }
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op: CompareOp::Eq,
            value: 0,
        } => Ok(TriggerCondition::HandSizeAtMost(0)),
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } => Ok(TriggerCondition::HandSizeAtLeast(
            u8::try_from(*value).map_err(|_| "oracle:hand-ovf")?,
        )),
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } if *value >= 0 => Ok(TriggerCondition::ControllerLifeAtLeast(
            u16::try_from(*value).map_err(|_| "oracle:life-ovf")?,
        )),
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Le,
            value,
        } if *value >= 0 => Ok(TriggerCondition::ControllerLifeAtMost(
            u16::try_from(*value).map_err(|_| "oracle:life-ovf")?,
        )),
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.flags.is_empty() =>
        {
            let min_u8 = u8::try_from(*value).map_err(|_| "oracle:count-ovf")?;
            if filter.subtypes.len() == 1 {
                let ct = vocabulary
                    .resolve_label(&filter.subtypes[0])
                    .ok_or("oracle:unsupported-trigger-condition")?;
                return Ok(TriggerCondition::ControlsSubtype {
                    kind: ct,
                    minimum: min_u8,
                });
            }
            if filter.subtypes.is_empty() {
                let mut types = card_type_mask_to_set(filter.any_types)?;
                if types.len() == 1
                    && let Some(kind) = types.pop_first()
                {
                    return Ok(TriggerCondition::ControlsCardType {
                        kind,
                        minimum: min_u8,
                    });
                }
            }
            if let Ok(obj) = lower_object_filter(filter, vocabulary) {
                return Ok(TriggerCondition::ControlsMatching {
                    filter: obj,
                    minimum: min_u8,
                });
            }
            Err("oracle:unsupported-trigger-condition")
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0 => {
            let min_u8 = u8::try_from(*value).map_err(|_| "oracle:count-ovf")?;
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(TriggerCondition::ControlsMatching {
                filter: obj,
                minimum: min_u8,
            })
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0 => {
            let min_u8 = u8::try_from(*value).map_err(|_| "oracle:count-ovf")?;
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(TriggerCondition::OpponentControlsMatching {
                filter: obj,
                minimum: min_u8,
            })
        }
        ConditionAst::GraveyardCount {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } => Ok(TriggerCondition::GraveyardCardTypeCount {
            kind: None,
            minimum: u8::try_from(*value).map_err(|_| "oracle:count-ovf")?,
        }),
        ConditionAst::ZoneObjectCount {
            filter,
            op: CompareOp::Ge,
            value,
        } if filter.zone == Some(ZoneFilter::YourGraveyard)
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty() =>
        {
            let min_u8 = u8::try_from(*value).map_err(|_| "oracle:count-ovf")?;
            if filter.any_types.is_empty() {
                return Ok(TriggerCondition::GraveyardCardTypeCount {
                    kind: None,
                    minimum: min_u8,
                });
            }
            let mut types = card_type_mask_to_set(filter.any_types)?;
            if types.len() == 1
                && let Some(kind) = types.pop_first()
            {
                return Ok(TriggerCondition::GraveyardCardTypeCount {
                    kind: Some(kind),
                    minimum: min_u8,
                });
            }
            Err("oracle:unsupported-trigger-condition")
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("tapped") =>
        {
            Ok(TriggerCondition::SourceIsTapped)
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("untapped") =>
        {
            Ok(TriggerCondition::SourceIsUntapped)
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("attacking") =>
        {
            Ok(TriggerCondition::SourceIsAttacking)
        }
        ConditionAst::GainedLifeThisTurn => Ok(TriggerCondition::ControllerGainedLifeThisTurn),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::You,
            min_amount: 1,
        } => Ok(TriggerCondition::ControllerLostLifeThisTurn),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            min_amount,
        } if *min_amount > 0 => Ok(TriggerCondition::OpponentLostLifeAtLeast(
            u8::try_from(*min_amount).map_err(|_| "oracle:life-ovf")?,
        )),
        ConditionAst::AttackedThisTurn => Ok(TriggerCondition::ControllerAttackedThisTurn),
        ConditionAst::EnteredUnderYourControlThisTurn {
            filter,
            min_count: 1,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.flags.is_empty() =>
        {
            Ok(TriggerCondition::CreatureEnteredUnderControlThisTurn)
        }
        ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter,
        } if filter.is_empty() => Ok(TriggerCondition::ControllerCastSpellsThisTurn(1)),
        ConditionAst::OpponentMoreThanYou { metric, .. } => {
            match metric.to_ascii_lowercase().as_str() {
                "life" => Ok(TriggerCondition::OpponentHasMoreLife),
                "creatures" => Ok(TriggerCondition::OpponentControlsMoreCreatures),
                "cards in hand" | "cards" => Ok(TriggerCondition::OpponentHasMoreCardsInHand),
                _ => Err("oracle:unsupported-trigger-condition"),
            }
        }
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } => match action.as_str() {
            "gained_life" | "gain_life" | "you gained life this turn" => {
                Ok(TriggerCondition::ControllerGainedLifeThisTurn)
            }
            "lost_life" | "lose_life" | "you lost life this turn" => {
                Ok(TriggerCondition::ControllerLostLifeThisTurn)
            }
            "attacked" | "attack" | "you attacked this turn" => {
                Ok(TriggerCondition::ControllerAttackedThisTurn)
            }
            "discard" | "discarded" | "you discarded a card this turn" => {
                Ok(TriggerCondition::ControllerDiscardedThisTurn)
            }
            "sacrifice" | "sacrificed" | "you sacrificed a permanent this turn" => {
                Ok(TriggerCondition::ControllerSacrificedThisTurn)
            }
            "revolt"
            | "permanent_left_battlefield"
            | "a permanent you controlled left the battlefield this turn" => {
                Ok(TriggerCondition::PermanentLeftBattlefieldThisTurn)
            }
            "descended" | "you descended this turn" => Ok(TriggerCondition::DescendedThisTurn),
            "crime" | "committed_crime" | "you committed a crime this turn" => {
                Ok(TriggerCondition::CommittedCrimeThisTurn)
            }
            "cast_two_spells" | "you've cast two or more spells this turn" => {
                Ok(TriggerCondition::ControllerCastSpellsThisTurn(2))
            }
            "drew_two_cards" | "you've drawn two or more cards this turn" => {
                Ok(TriggerCondition::ControllerDrewCardsThisTurn(2))
            }
            _ => Err("oracle:unsupported-trigger-condition"),
        },
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op,
            value,
        } => {
            let obj = lower_object_filter(filter, vocabulary)?;
            match (op, *value) {
                (CompareOp::Eq, 0) => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::ControlsMatching {
                        filter: obj,
                        minimum: 1,
                    },
                ))),
                (CompareOp::Gt, v) => Ok(TriggerCondition::ControlsMatching {
                    filter: obj,
                    minimum: u8::try_from(v + 1).map_err(|_| "oracle:count-ovf")?,
                }),
                (CompareOp::Le, v) => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::ControlsMatching {
                        filter: obj,
                        minimum: u8::try_from(v + 1).map_err(|_| "oracle:count-ovf")?,
                    },
                ))),
                (CompareOp::Lt, v) if v > 0 => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::ControlsMatching {
                        filter: obj,
                        minimum: u8::try_from(v).map_err(|_| "oracle:count-ovf")?,
                    },
                ))),
                (CompareOp::Eq, v) if v > 0 => {
                    let min = u8::try_from(v).map_err(|_| "oracle:count-ovf")?;
                    let over = u8::try_from(v + 1).map_err(|_| "oracle:count-ovf")?;
                    Ok(TriggerCondition::And(
                        Box::new(TriggerCondition::ControlsMatching {
                            filter: obj,
                            minimum: min,
                        }),
                        Box::new(TriggerCondition::Not(Box::new(
                            TriggerCondition::ControlsMatching {
                                filter: obj,
                                minimum: over,
                            },
                        ))),
                    ))
                }
                _ => Err("oracle:unsupported-trigger-condition"),
            }
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op,
            value,
        } => {
            let obj = lower_object_filter(filter, vocabulary)?;
            match (op, *value) {
                (CompareOp::Eq, 0) => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::OpponentControlsMatching {
                        filter: obj,
                        minimum: 1,
                    },
                ))),
                (CompareOp::Le, v) => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::OpponentControlsMatching {
                        filter: obj,
                        minimum: u8::try_from(v + 1).map_err(|_| "oracle:count-ovf")?,
                    },
                ))),
                (CompareOp::Lt, v) if v > 0 => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::OpponentControlsMatching {
                        filter: obj,
                        minimum: u8::try_from(v).map_err(|_| "oracle:count-ovf")?,
                    },
                ))),
                _ => Err("oracle:unsupported-trigger-condition"),
            }
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let kind =
                parse_counter_kind(counter_kind).ok_or("oracle:unsupported-trigger-condition")?;
            match (op, *value) {
                (CompareOp::Eq, 0) => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::SourceCounterAtLeast { kind, minimum: 1 },
                ))),
                (CompareOp::Gt, v) => Ok(TriggerCondition::SourceCounterAtLeast {
                    kind,
                    minimum: v
                        .checked_add(1)
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or("oracle:count-ovf")?,
                }),
                (CompareOp::Le, v) => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::SourceCounterAtLeast {
                        kind,
                        minimum: v
                            .checked_add(1)
                            .and_then(|n| u8::try_from(n).ok())
                            .ok_or("oracle:count-ovf")?,
                    },
                ))),
                (CompareOp::Lt, v) if v > 0 => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::SourceCounterAtLeast {
                        kind,
                        minimum: u8::try_from(v).map_err(|_| "oracle:count-ovf")?,
                    },
                ))),
                _ => Err("oracle:unsupported-trigger-condition"),
            }
        }
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(TriggerCondition::SourceMatches(Box::new(obj)))
        }
        ConditionAst::HasKeyword { subject, keyword }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            let kw = lower_keyword_ability_name(keyword)
                .ok_or("oracle:unsupported-trigger-condition")?;
            Ok(TriggerCondition::SourceMatches(Box::new(
                ObjectFilter::any().with_keyword(kw),
            )))
        }
        ConditionAst::ZoneObjectCount { filter, op, value }
            if filter.zone == Some(ZoneFilter::YourGraveyard) =>
        {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)?
            };
            match (op, *value) {
                (CompareOp::Ge, v) if v > 0 => Ok(TriggerCondition::GraveyardMatching {
                    filter: Box::new(obj),
                    minimum: u8::try_from(v).map_err(|_| "oracle:count-ovf")?,
                }),
                (CompareOp::Eq, 0) => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::GraveyardMatching {
                        filter: Box::new(obj),
                        minimum: 1,
                    },
                ))),
                (CompareOp::Le, v) => Ok(TriggerCondition::Not(Box::new(
                    TriggerCondition::GraveyardMatching {
                        filter: Box::new(obj),
                        minimum: u8::try_from(v + 1).map_err(|_| "oracle:count-ovf")?,
                    },
                ))),
                _ => Err("oracle:unsupported-trigger-condition"),
            }
        }
        ConditionAst::Marker(GameMarker::DescendedThisTurn) => {
            Ok(TriggerCondition::DescendedThisTurn)
        }
        ConditionAst::Marker(GameMarker::CommittedCrimeThisTurn) => {
            Ok(TriggerCondition::CommittedCrimeThisTurn)
        }
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
            from_hand_only: false,
        }) if *value > 0 => Ok(TriggerCondition::ControllerCastSpellsThisTurn(*value)),
        ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        }) if *value > 0 => Ok(TriggerCondition::ControllerDrewCardsThisTurn(*value)),
        ConditionAst::Not(inner) => Ok(TriggerCondition::Not(Box::new(lower_trigger_condition(
            inner, vocabulary,
        )?))),
        ConditionAst::And(a, b) => Ok(TriggerCondition::And(
            Box::new(lower_trigger_condition(a, vocabulary)?),
            Box::new(lower_trigger_condition(b, vocabulary)?),
        )),
        ConditionAst::Or(a, b) => Ok(TriggerCondition::Or(
            Box::new(lower_trigger_condition(a, vocabulary)?),
            Box::new(lower_trigger_condition(b, vocabulary)?),
        )),
        _ => Err("oracle:unsupported-trigger-condition"),
    }
}

// =========================================================================
// Effect Body & Instruction Lowering
// =========================================================================

struct BodyBuilder {
    target: BodyTarget,
    instructions: Vec<Instruction>,
}

impl BodyBuilder {
    fn new() -> Self {
        Self {
            target: BodyTarget::None,
            instructions: Vec::new(),
        }
    }

    fn bind_target(&mut self, next: BodyTarget) -> Result<(), &'static str> {
        if next == BodyTarget::None {
            return Ok(());
        }
        if self.target == BodyTarget::None {
            self.target = next;
            Ok(())
        } else if self.target == next {
            // Multiple instructions referencing the same single target are only safe
            // when the second instruction uses anaphora; reject distinct target clauses.
            Err("oracle:multiple-targets")
        } else {
            Err("oracle:conflicting-targets")
        }
    }

    fn finish(self) -> Result<ExecutableBody, &'static str> {
        if self.instructions.is_empty() {
            return Err("oracle:empty-body");
        }
        Ok(ExecutableBody::new(self.target, self.instructions))
    }
}

fn lower_spell_effects(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(ExecutableBody, Option<SpellModes>), &'static str> {
    let modal_info = match ins {
        [InstructionAst::Modal { choice, modes }] => Some((choice, modes, false)),
        [
            InstructionAst::Modal { choice, modes },
            InstructionAst::AllowRepeatModes,
        ]
        | [
            InstructionAst::AllowRepeatModes,
            InstructionAst::Modal { choice, modes },
        ] => Some((choice, modes, true)),
        _ => None,
    };
    if let Some((choice, modes, allow_repeat)) = modal_info {
        let (min, raw_max) = parse_modal_choice(choice)?;
        if modes.len() < 2 {
            return Err("oracle:modal-too-few");
        }
        let max = raw_max.min(u8::try_from(modes.len()).unwrap_or(u8::MAX));
        let mut lowered = Vec::with_capacity(modes.len());
        for m in modes {
            lowered.push(lower_effect_body(&m.body, vocabulary, ids, false, false)?);
        }
        let first = lowered.remove(0);
        return Ok((
            first,
            Some(SpellModes {
                minimum: min,
                maximum: max,
                allow_repeat,
                additional: lowered,
            }),
        ));
    }
    let body = lower_effect_body(ins, vocabulary, ids, false, false)?;
    let mut body = body;
    let saw_sacrifice = body.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::Sacrifice { .. } | Instruction::SacrificeMatching(_)
        )
    });
    if saw_sacrifice {
        rewrite_body_anaphora_stats(&mut body, ValueObjectRef::LastSacrificedCreature);
    }
    Ok((body, None))
}

fn parse_modal_choice(choice: &str) -> Result<(u8, u8), &'static str> {
    match choice.to_ascii_lowercase().trim() {
        "choose one" | "one" => Ok((1, 1)),
        "choose two" | "two" => Ok((2, 2)),
        "choose three" | "three" => Ok((3, 3)),
        "choose one or both" | "one or both" => Ok((1, 2)),
        "choose one or two" | "one or two" => Ok((1, 2)),
        "choose one, two, or three" | "one, two, or three" => Ok((1, 3)),
        "choose two or more" | "two or more" => Ok((2, 255)),
        "choose one or more" | "one or more" => Ok((1, 255)),
        _ => Err("oracle:unsupported-modal-choice"),
    }
}

fn rewrite_value_expr_anaphora(expr: &mut ValueExpr, replacement: ValueObjectRef) {
    match expr {
        ValueExpr::ObjectStat { subject, .. } if *subject == ValueObjectRef::TargetObject => {
            *subject = replacement;
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            rewrite_value_expr_anaphora(inner, replacement);
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            rewrite_value_expr_anaphora(a, replacement);
            rewrite_value_expr_anaphora(b, replacement);
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            rewrite_value_expr_anaphora(left, replacement);
            rewrite_value_expr_anaphora(right, replacement);
            rewrite_value_expr_anaphora(then_value, replacement);
            rewrite_value_expr_anaphora(else_value, replacement);
        }
        _ => {}
    }
}

fn rewrite_instruction_anaphora(op: &mut Instruction, replacement: ValueObjectRef) {
    match op {
        Instruction::GainControllerLife { amount }
        | Instruction::DrainTargetAndGain(amount)
        | Instruction::OpponentsLoseAndControllerGains(amount)
        | Instruction::DrawController { amount }
        | Instruction::DrawTargetPlayer { amount }
        | Instruction::DrawControllerAndTarget { amount }
        | Instruction::DrawEachPlayer { amount }
        | Instruction::Mill { amount, .. }
        | Instruction::Scry { amount, .. }
        | Instruction::Surveil { amount, .. }
        | Instruction::DamageAmount(amount)
        | Instruction::DamageAmountThenExileIfDies(amount)
        | Instruction::DamageOpponentsAmount(amount)
        | Instruction::DamageRelatedPlayerAmount(amount)
        | Instruction::DamageGroupAmount { amount, .. }
        | Instruction::DrainOpponents(amount)
        | Instruction::Discard { amount, .. }
        | Instruction::DiscardFiltered { amount, .. }
        | Instruction::Sacrifice { amount, .. }
        | Instruction::ReturnControlledPermanents { amount, .. }
        | Instruction::AddPlayerCounter { amount, .. }
        | Instruction::PutNamedCounterOnTarget { amount, .. }
        | Instruction::ExileTopPlay { count: amount, .. }
        | Instruction::LookTopChoose { count: amount, .. }
        | Instruction::ModifyGroupByAmount { amount, .. }
        | Instruction::BoostTargetPower { amount, .. } => {
            rewrite_value_expr_anaphora(amount, replacement);
        }
        Instruction::CreateTokens { creations }
        | Instruction::CreateAttackingTokens { creations } => {
            for spec in creations {
                rewrite_value_expr_anaphora(&mut spec.amount, replacement);
            }
        }
        Instruction::BranchIfFalse { condition, .. } => {
            rewrite_value_expr_anaphora(&mut condition.left, replacement);
            rewrite_value_expr_anaphora(&mut condition.right, replacement);
        }
        Instruction::EmitReflexiveTrigger { body }
        | Instruction::ScheduleDelayedStepTrigger { body, .. } => {
            rewrite_body_anaphora_stats(body, replacement);
        }
        _ => {}
    }
}

fn rewrite_body_anaphora_stats(body: &mut ExecutableBody, replacement: ValueObjectRef) {
    let allow_rewrite = matches!(
        body.target,
        BodyTarget::None
            | BodyTarget::SinglePlayer
            | BodyTarget::SingleOpponent
            | BodyTarget::SingleSpell(_)
            | BodyTarget::SingleAnyTarget
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
    );
    if !allow_rewrite {
        return;
    }
    for op in &mut body.instructions {
        rewrite_instruction_anaphora(op, replacement);
    }
}

fn rewrite_value_expr_prior_result(expr: &mut ValueExpr, replacement: EventValueKind) {
    match expr {
        ValueExpr::EventValue(kind) if *kind == EventValueKind::LastActionCount => {
            *kind = replacement;
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            rewrite_value_expr_prior_result(inner, replacement);
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            rewrite_value_expr_prior_result(a, replacement);
            rewrite_value_expr_prior_result(b, replacement);
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            rewrite_value_expr_prior_result(left, replacement);
            rewrite_value_expr_prior_result(right, replacement);
            rewrite_value_expr_prior_result(then_value, replacement);
            rewrite_value_expr_prior_result(else_value, replacement);
        }
        _ => {}
    }
}

fn rewrite_body_prior_result(body: &mut ExecutableBody, replacement: EventValueKind) {
    let mut saw_count_action = false;
    for op in &mut body.instructions {
        if !saw_count_action {
            match op {
                Instruction::GainControllerLife { amount }
                | Instruction::DrainTargetAndGain(amount)
                | Instruction::OpponentsLoseAndControllerGains(amount)
                | Instruction::DrawController { amount }
                | Instruction::DrawTargetPlayer { amount }
                | Instruction::DrawControllerAndTarget { amount }
                | Instruction::DrawEachPlayer { amount }
                | Instruction::Mill { amount, .. }
                | Instruction::Scry { amount, .. }
                | Instruction::Surveil { amount, .. }
                | Instruction::DamageAmount(amount)
                | Instruction::DamageAmountThenExileIfDies(amount)
                | Instruction::DamageOpponentsAmount(amount)
                | Instruction::DamageRelatedPlayerAmount(amount)
                | Instruction::DamageGroupAmount { amount, .. }
                | Instruction::DrainOpponents(amount)
                | Instruction::Discard { amount, .. }
                | Instruction::DiscardFiltered { amount, .. }
                | Instruction::Sacrifice { amount, .. }
                | Instruction::ReturnControlledPermanents { amount, .. }
                | Instruction::AddPlayerCounter { amount, .. }
                | Instruction::PutNamedCounterOnTarget { amount, .. }
                | Instruction::ExileTopPlay { count: amount, .. }
                | Instruction::LookTopChoose { count: amount, .. }
                | Instruction::ModifyGroupByAmount { amount, .. }
                | Instruction::BoostTargetPower { amount, .. } => {
                    rewrite_value_expr_prior_result(amount, replacement);
                }
                Instruction::CreateTokens { creations }
                | Instruction::CreateAttackingTokens { creations } => {
                    for spec in creations {
                        rewrite_value_expr_prior_result(&mut spec.amount, replacement);
                    }
                }
                Instruction::BranchIfFalse { condition, .. } => {
                    rewrite_value_expr_prior_result(&mut condition.left, replacement);
                    rewrite_value_expr_prior_result(&mut condition.right, replacement);
                }
                Instruction::EmitReflexiveTrigger { body: inner }
                | Instruction::ScheduleDelayedStepTrigger { body: inner, .. } => {
                    rewrite_body_prior_result(inner, replacement);
                }
                _ => {}
            }
        }
        if matches!(
            op,
            Instruction::Discard { .. }
                | Instruction::DiscardFiltered { .. }
                | Instruction::DiscardUpTo { .. }
                | Instruction::Mill { .. }
                | Instruction::Sacrifice { .. }
                | Instruction::SacrificeMatching(_)
                | Instruction::DestroyGroup(_)
                | Instruction::DestroyGroupWithoutRegeneration(_)
                | Instruction::MoveGroup { .. }
        ) {
            saw_count_action = true;
        }
    }
}

fn strip_triggered_restrictions(
    body: &[InstructionAst],
    spec: &mut TriggerSpec,
    grouping: &mut TriggerGrouping,
) -> Vec<InstructionAst> {
    let mut stripped_body = Vec::with_capacity(body.len());
    for ins in body {
        if let InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } = ins
        {
            let low = restriction.to_ascii_lowercase();
            if restriction == "OnceEachTurn"
                || restriction == "TriggersOnceEachTurn"
                || low.contains("once each turn")
                || low.contains("once per turn")
            {
                *grouping = TriggerGrouping::OneOrMoreOnceEachTurn;
                continue;
            }
            if (restriction == "DuringYourTurn" || low == "during your turn")
                && spec.intervening_if.is_none()
            {
                *spec = spec.clone().only_if(TriggerCondition::ControllersTurn);
                continue;
            }
            if (restriction == "DuringOpponentsTurn" || low.contains("opponent's turn"))
                && spec.intervening_if.is_none()
            {
                *spec = spec.clone().only_if(TriggerCondition::OpponentsTurn);
                continue;
            }
        }
        stripped_body.push(ins.clone());
    }
    stripped_body
}

fn body_references_self_from_graveyard(body: &[InstructionAst]) -> bool {
    for ins in body {
        match ins {
            InstructionAst::ReturnZone {
                subject,
                from_zone: Some(from),
                ..
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && from.to_ascii_lowercase().contains("graveyard") =>
            {
                return true;
            }
            InstructionAst::MoveZone { subject, to }
                if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                    && to.to_ascii_lowercase().contains("hand") =>
            {
                return true;
            }
            InstructionAst::Optional(inner) | InstructionAst::IfPreviousSucceeded(inner)
                if body_references_self_from_graveyard(inner) =>
            {
                return true;
            }
            InstructionAst::IfCondition { then_body, .. }
                if body_references_self_from_graveyard(then_body) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn apply_trigger_body_rewrites(
    exec_body: &mut ExecutableBody,
    spec: &TriggerSpec,
    self_pronoun: bool,
) {
    if matches!(spec.event, TriggerEvent::Dies { .. }) {
        rewrite_body_anaphora_stats(exec_body, ValueObjectRef::DyingCreature);
    } else if matches!(spec.event, TriggerEvent::Enters { .. }) && !self_pronoun {
        rewrite_body_anaphora_stats(exec_body, ValueObjectRef::EnteringPermanent);
    } else if self_pronoun {
        rewrite_body_anaphora_stats(exec_body, ValueObjectRef::TriggerSource);
    }
    match &spec.event {
        TriggerEvent::DealsDamage { .. } => {
            rewrite_body_prior_result(exec_body, EventValueKind::CombatDamageToPlayer);
        }
        TriggerEvent::DealtDamage {
            linked_player: false,
            ..
        } => {
            rewrite_body_prior_result(exec_body, EventValueKind::DamageReceived);
        }
        TriggerEvent::LifeChanged { .. } => {
            rewrite_body_prior_result(exec_body, EventValueKind::TriggeringLifeChange);
        }
        _ => {}
    }
}

fn strip_activated_restrictions(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(Vec<InstructionAst>, Option<ActivationRestriction>), &'static str> {
    let mut restriction = None;
    let mut filtered = Vec::with_capacity(ins.len());
    for item in ins {
        match item {
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: None,
            } if r == "Instant" || r.eq_ignore_ascii_case("as an instant") => {
                // Normal ActivatedAbility already enforces instant-speed priority timing.
            }
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: None,
            } => {
                let next_r = parse_activation_restriction(r)?;
                restriction = match (restriction, next_r) {
                    (None, r) => Some(r),
                    (Some(ActivationRestriction::Sorcery), ActivationRestriction::OnceEachTurn)
                    | (Some(ActivationRestriction::OnceEachTurn), ActivationRestriction::Sorcery) => {
                        Some(ActivationRestriction::SorceryOnceEachTurn)
                    }
                    (
                        Some(ActivationRestriction::DuringYourTurn),
                        ActivationRestriction::OnceEachTurn,
                    )
                    | (
                        Some(ActivationRestriction::OnceEachTurn),
                        ActivationRestriction::DuringYourTurn,
                    ) => Some(ActivationRestriction::DuringYourTurnOnceEachTurn),
                    _ => return Err("oracle:multiple-act-restrictions"),
                };
            }
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: Some(cond),
            } if r == "Conditional" || r == "Condition" => {
                if restriction.is_some() {
                    return Err("oracle:multiple-act-restrictions");
                }
                restriction = Some(lower_conditional_activation_restriction(cond, vocabulary)?);
            }
            _ => {
                filtered.push(item.clone());
            }
        }
    }
    Ok((filtered, restriction))
}

fn lower_activated_body(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(ExecutableBody, Option<ActivationRestriction>), &'static str> {
    let (filtered, restriction) = strip_activated_restrictions(ins, vocabulary)?;
    let body = lower_effect_body(&filtered, vocabulary, ids, false, true)?;
    Ok((body, restriction))
}

fn lower_conditional_activation_restriction(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ActivationRestriction, &'static str> {
    match cond {
        ConditionAst::Marker(GameMarker::CreatedTokenThisTurn {
            player: PlayerRefAst::You,
        }) => Ok(ActivationRestriction::CreatedTokenThisTurn),
        ConditionAst::Marker(GameMarker::CitysBlessing) => {
            Ok(ActivationRestriction::HasCityBlessing)
        }
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op: CompareOp::Eq,
            value: 0,
        } => Ok(ActivationRestriction::EmptyHand),
        ConditionAst::Morbid {
            your_control_only: false,
        } => Ok(ActivationRestriction::CreatureDiedThisTurn),
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty()
            && filter.stat_bounds.len() == 1
            && filter.stat_bounds[0].stat == StatKind::Power
            && filter.stat_bounds[0].op == CompareOp::Ge
            && matches!(filter.stat_bounds[0].value.as_ref(), ValueExprAst::Const(4)) =>
        {
            Ok(ActivationRestriction::ControlsCreaturePowerFourOrGreater)
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.subtypes.len() == 1
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.flags.is_empty() =>
        {
            let ct = vocabulary
                .resolve_label(&filter.subtypes[0])
                .ok_or("oracle:unsupported-act-restriction")?;
            let min_u8 = u8::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?;
            let minimum = NonZeroU8::new(min_u8).ok_or("oracle:unsupported-act-restriction")?;
            Ok(ActivationRestriction::ControlsSubtypeCount { kind: ct, minimum })
        }
        ConditionAst::Threshold => Ok(ActivationRestriction::Threshold),
        ConditionAst::Delirium => Ok(ActivationRestriction::Delirium),
        ConditionAst::Metalcraft => Ok(ActivationRestriction::Metalcraft),
        ConditionAst::IsYourTurn => Ok(ActivationRestriction::DuringYourTurn),
        ConditionAst::Not(inner) if matches!(inner.as_ref(), ConditionAst::IsYourTurn) => {
            Ok(ActivationRestriction::DuringOpponentsTurn)
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("tapped") =>
        {
            Ok(ActivationRestriction::SourceIsTapped)
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("untapped") =>
        {
            Ok(ActivationRestriction::SourceIsUntapped)
        }
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } if *value >= 0 => Ok(ActivationRestriction::ControllerLifeAtLeast(
            u16::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?,
        )),
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Le,
            value,
        } if *value >= 0 => Ok(ActivationRestriction::ControllerLifeAtMost(
            u16::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?,
        )),
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.subtypes.is_empty()
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.flags.is_empty() =>
        {
            let mut types = card_type_mask_to_set(filter.any_types)?;
            if types.len() == 1
                && let Some(kind) = types.pop_first()
            {
                let min_u8 =
                    u8::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?;
                return Ok(ActivationRestriction::ControlsCardType {
                    kind,
                    minimum: min_u8,
                });
            }
            Err("oracle:unsupported-act-restriction")
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0 => {
            let min_u8 = u8::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?;
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ActivationRestriction::ControlsMatching {
                filter: obj,
                minimum: min_u8,
            })
        }
        _ => Err("oracle:unsupported-act-restriction"),
    }
}

fn parse_activation_restriction(raw: &str) -> Result<ActivationRestriction, &'static str> {
    match raw {
        "Sorcery" => return Ok(ActivationRestriction::Sorcery),
        "OnceEachTurn" => return Ok(ActivationRestriction::OnceEachTurn),
        "DuringUpkeep" => return Ok(ActivationRestriction::DuringUpkeep),
        "OwnTurnBeforeAttackers" => return Ok(ActivationRestriction::OwnTurnBeforeAttackers),
        "Exhaust" | "Once" => return Ok(ActivationRestriction::Exhaust),
        "TriggersOnceEachTurn" => return Ok(ActivationRestriction::OnceEachTurn),
        "DuringYourTurn" => return Ok(ActivationRestriction::DuringYourTurn),
        "DuringOpponentsTurn" => return Ok(ActivationRestriction::DuringOpponentsTurn),
        "DuringCombat" => return Ok(ActivationRestriction::DuringCombat),
        "SorceryOnceEachTurn" => return Ok(ActivationRestriction::SorceryOnceEachTurn),
        "DuringYourTurnOnceEachTurn" => {
            return Ok(ActivationRestriction::DuringYourTurnOnceEachTurn);
        }
        _ => {}
    }
    let low = raw.to_ascii_lowercase();
    let has_once = low.contains("once each turn") || low.contains("once per turn");
    if low == "as an instant" {
        return Err("oracle:instant-restriction-handled-above");
    }
    if low.contains("sorcery") && has_once {
        Ok(ActivationRestriction::SorceryOnceEachTurn)
    } else if low.contains("during your turn") && has_once {
        Ok(ActivationRestriction::DuringYourTurnOnceEachTurn)
    } else if low.contains("during your turn") && low.contains("before attackers") {
        Ok(ActivationRestriction::OwnTurnBeforeAttackers)
    } else if low.contains("sorcery") {
        Ok(ActivationRestriction::Sorcery)
    } else if has_once {
        Ok(ActivationRestriction::OnceEachTurn)
    } else if low == "activate only once" || low == "exhaust" || low == "once" {
        Ok(ActivationRestriction::Exhaust)
    } else if low.contains("during your upkeep") {
        Ok(ActivationRestriction::DuringUpkeep)
    } else if low.contains("during your turn") {
        Ok(ActivationRestriction::DuringYourTurn)
    } else if low.contains("opponent's turn") || low.contains("opponents' turns") {
        Ok(ActivationRestriction::DuringOpponentsTurn)
    } else if low.contains("during combat") {
        Ok(ActivationRestriction::DuringCombat)
    } else if low.contains("any player may activate") || low.starts_with("anyplayer:") {
        Ok(ActivationRestriction::AnyPlayer)
    } else if low.contains("creature died this turn") {
        Ok(ActivationRestriction::CreatureDiedThisTurn)
    } else if low.contains("no cards in hand") {
        Ok(ActivationRestriction::EmptyHand)
    } else {
        Err("oracle:unsupported-act-restriction")
    }
}

fn trigger_subject_is_self(trig: &TriggerSpecAst) -> bool {
    !trig.events.is_empty()
        && trig.events.iter().all(|ev| match ev {
            TriggerEventAst::ZoneChange { subject, .. }
            | TriggerEventAst::AttacksWithOthers { subject, .. }
            | TriggerEventAst::AttacksUnblocked { subject }
            | TriggerEventAst::Blocks { subject, .. }
            | TriggerEventAst::BecomesBlocked { subject, .. }
            | TriggerEventAst::TapStateChanged { subject, .. }
            | TriggerEventAst::TurnedFaceUp { subject }
            | TriggerEventAst::Transforms { subject }
            | TriggerEventAst::PhaseChanged { subject, .. }
            | TriggerEventAst::Mutates { subject }
            | TriggerEventAst::BecomesTargeted { subject, .. }
            | TriggerEventAst::CountersChanged { subject, .. }
            | TriggerEventAst::Cycled { subject } => {
                matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            }
            TriggerEventAst::Attacks { subject, .. } => {
                matches!(subject.as_deref(), Some(SubjectRefAst::SelfObject))
            }
            TriggerEventAst::DamageDealt { source, .. } => {
                matches!(source.as_deref(), Some(SubjectRefAst::SelfObject))
            }
            _ => false,
        })
}

fn is_self_pronoun(
    subject: &SubjectRefAst,
    builder: &BodyBuilder,
    self_pronoun_valid: bool,
) -> bool {
    match subject {
        SubjectRefAst::SelfObject => true,
        SubjectRefAst::Anaphora(p) if self_pronoun_valid && builder.target == BodyTarget::None => {
            matches!(
                p.to_ascii_lowercase().as_str(),
                "it" | "this creature"
                    | "this permanent"
                    | "this artifact"
                    | "this enchantment"
                    | "this land"
            )
        }
        _ => false,
    }
}

fn lower_comparison(op: CompareOp) -> Comparison {
    match op {
        CompareOp::Eq => Comparison::Equal,
        CompareOp::Lt => Comparison::Less,
        CompareOp::Le => Comparison::LessOrEqual,
        CompareOp::Gt => Comparison::Greater,
        CompareOp::Ge => Comparison::GreaterOrEqual,
    }
}

fn negate_comparison(c: Comparison) -> Comparison {
    match c {
        Comparison::Equal => Comparison::NotEqual,
        Comparison::NotEqual => Comparison::Equal,
        Comparison::Less => Comparison::GreaterOrEqual,
        Comparison::LessOrEqual => Comparison::Greater,
        Comparison::Greater => Comparison::LessOrEqual,
        Comparison::GreaterOrEqual => Comparison::Less,
    }
}

fn lower_resolution_condition(
    cond: &ConditionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<ResolutionCondition, &'static str> {
    match cond {
        ConditionAst::Not(inner) => {
            let mut rc =
                lower_resolution_condition(inner, builder, vocabulary, in_trigger, bound_x)?;
            rc.comparison = negate_comparison(rc.comparison);
            Ok(rc)
        }
        ConditionAst::And(a, b) => {
            let ca = lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
            let cb = lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
            let ba = ValueExpr::Conditional {
                left: Box::new(ca.left),
                comparison: ca.comparison,
                right: Box::new(ca.right),
                then_value: Box::new(ValueExpr::literal(1)),
                else_value: Box::new(ValueExpr::literal(0)),
            };
            let bb = ValueExpr::Conditional {
                left: Box::new(cb.left),
                comparison: cb.comparison,
                right: Box::new(cb.right),
                then_value: Box::new(ValueExpr::literal(1)),
                else_value: Box::new(ValueExpr::literal(0)),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::Add(Box::new(ba), Box::new(bb)),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(2),
            })
        }
        ConditionAst::Or(a, b) => {
            let ca = lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
            let cb = lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
            let ba = ValueExpr::Conditional {
                left: Box::new(ca.left),
                comparison: ca.comparison,
                right: Box::new(ca.right),
                then_value: Box::new(ValueExpr::literal(1)),
                else_value: Box::new(ValueExpr::literal(0)),
            };
            let bb = ValueExpr::Conditional {
                left: Box::new(cb.left),
                comparison: cb.comparison,
                right: Box::new(cb.right),
                then_value: Box::new(ValueExpr::literal(1)),
                else_value: Box::new(ValueExpr::literal(0)),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::Add(Box::new(ba), Box::new(bb)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Compare { lhs, op, rhs } => Ok(ResolutionCondition {
            left: lower_value_expr(resolve_x(lhs, bound_x), vocabulary)?,
            comparison: lower_comparison(*op),
            right: lower_value_expr(resolve_x(rhs, bound_x), vocabulary)?,
        }),
        ConditionAst::ControlsCount {
            player,
            filter,
            op,
            value,
        } => {
            let obj = lower_object_filter(filter, vocabulary)?;
            let left = match player {
                PlayerRefAst::You => {
                    ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj)
                }
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => {
                    ValueExpr::count_battlefield(obj)
                }
                PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent => {
                    let rp = lower_resolving_player(*player, builder)?;
                    ValueExpr::count_player_battlefield(rp, obj)
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::PlayerCounterCompare {
            player: PlayerRefAst::You,
            counter_kind,
            op,
            value,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:unsupported-if-condition")?;
            let subject = match ck {
                CounterKind::Experience => {
                    spellwake_engine::program::CounterSubjectRef::ControllerExperience
                }
                CounterKind::Energy => {
                    spellwake_engine::program::CounterSubjectRef::ControllerEnergy
                }
                CounterKind::Poison => {
                    spellwake_engine::program::CounterSubjectRef::ControllerPoison
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::counters(subject, Some(ck)),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::Metalcraft => Ok(ResolutionCondition {
            left: ValueExpr::controlled_type(CardType::Artifact),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(3),
        }),
        ConditionAst::Threshold => Ok(ResolutionCondition {
            left: ValueExpr::count_graveyard(ResolvingPlayer::Controller, ObjectFilter::any()),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(7),
        }),
        ConditionAst::HandCount { player, op, value } => {
            let rp = lower_resolving_player(*player, builder)?;
            Ok(ResolutionCondition {
                left: ValueExpr::count_hand(rp),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::GraveyardCount { player, op, value } => {
            let rp = lower_resolving_player(*player, builder)?;
            Ok(ResolutionCondition {
                left: ValueExpr::count_graveyard(rp, ObjectFilter::any()),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::ZoneObjectCount { filter, op, value } => {
            let left = lower_value_expr(&ValueExprAst::Count(filter.clone()), vocabulary)?;
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op,
            value,
        } => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let kind = if counter_kind.eq_ignore_ascii_case("any") {
                None
            } else {
                Some(parse_counter_kind(counter_kind).ok_or("oracle:unsupported-if-condition")?)
            };
            let subj = if in_trigger {
                spellwake_engine::program::CounterSubjectRef::TriggerSource
            } else {
                spellwake_engine::program::CounterSubjectRef::AbilitySource
            };
            Ok(ResolutionCondition {
                left: ValueExpr::counters(subj, kind),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::TotalStatCompare {
            filter,
            stat,
            op,
            value,
        } => {
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => return Err("oracle:unsupported-if-condition"),
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::aggregate(obj, st, StatAggregate::Sum),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::StatCompare {
            subject,
            stat,
            op,
            value,
        } => {
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => return Err("oracle:unsupported-if-condition"),
            };
            let subj = match subject.as_ref() {
                SubjectRefAst::SelfObject => {
                    if in_trigger {
                        ValueObjectRef::TriggerSource
                    } else {
                        ValueObjectRef::AbilitySource
                    }
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    ValueObjectRef::TargetObject
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_stat(subj, st),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(MatchObjectRef::AbilitySource, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                && builder.target != BodyTarget::None =>
        {
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(MatchObjectRef::TargetObject, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || (matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                    && builder.target != BodyTarget::None) =>
        {
            let mref = if matches!(subject.as_ref(), SubjectRefAst::SelfObject) {
                MatchObjectRef::AbilitySource
            } else {
                MatchObjectRef::TargetObject
            };
            let obj = match status.trim().to_ascii_lowercase().as_str() {
                "tapped" => ObjectFilter::any().tapped(),
                "untapped" => ObjectFilter::Clause(ObjectClause::any().untapped()),
                "attacking" => ObjectFilter::any().attacking(),
                "blocking" => ObjectFilter::any().combat(CombatFilter::Blocking),
                "attacking or blocking" => ObjectFilter::any().attacking_or_blocking(),
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::HasKeyword { subject, keyword }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || (matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                    && builder.target != BodyTarget::None) =>
        {
            let mref = if matches!(subject.as_ref(), SubjectRefAst::SelfObject) {
                MatchObjectRef::AbilitySource
            } else {
                MatchObjectRef::TargetObject
            };
            let kw =
                lower_keyword_ability_name(keyword).ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, ObjectFilter::any().with_keyword(kw)),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::CitysBlessing) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerHasCityBlessing),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Morbid {
            your_control_only: false,
        } => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CreaturesDiedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::WasKicked => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::KickerCount(0)),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::Bargained) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::Bargained),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::DescendedThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::DescendedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CommittedCrimeThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CommittedCrimeThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::NoCreaturesOnBattlefield) => Ok(ResolutionCondition {
            left: ValueExpr::count_battlefield(ObjectFilter::creature()),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::You,
            op,
            value,
            from_hand_only: false,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player: PlayerRefAst::You,
            op,
            value,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CardsDrawnThisTurn),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::Marker(GameMarker::CastFromGraveyard) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CastFromGraveyard),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CastDuringMainPhase) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CastDuringOwnMain),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::Monarch(PlayerRefAst::You)) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsMonarch),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::ControlsGreatestStat {
            player: PlayerRefAst::You,
            stat: StatKind::Power,
            ..
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControlsGreatestCreature),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::OpponentLostLifeThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::GainedLifeThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeGainedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::You,
            min_amount,
        } if *min_amount > 0 => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_amount)),
        }),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            min_amount,
        } if *min_amount > 0 => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_amount)),
        }),
        ConditionAst::AttackedThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerAttackedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::EnteredUnderYourControlThisTurn { filter, min_count }
            if *min_count > 0
                && filter.any_types == AstCardTypeMask::CREATURE
                && filter.forbidden_types.is_empty()
                && filter.subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.stat_bounds.is_empty()
                && filter.flags.is_empty() =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::CreatureEnteredUnderControlThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*min_count)),
            })
        }
        ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter,
        } if filter.is_empty() => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::IsYourTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Delirium => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OwnGraveyardDistinctCardTypes),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(4),
        }),
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } => {
            let (stat, min): (PlayerStatExpr, i64) = match action.as_str() {
                "lost_life" | "lose_life" | "you lost life this turn" => {
                    (PlayerStatExpr::ControllerLifeLostThisTurn, 1)
                }
                "gained_life" | "gain_life" | "you gained life this turn" => {
                    (PlayerStatExpr::ControllerLifeGainedThisTurn, 1)
                }
                "attacked" | "attack" | "you attacked this turn" => {
                    (PlayerStatExpr::ControllerAttackedThisTurn, 1)
                }
                "discard" | "discarded" | "you discarded a card this turn" => {
                    (PlayerStatExpr::ControllerDiscardedThisTurn, 1)
                }
                "sacrifice" | "sacrificed" | "you sacrificed a permanent this turn" => {
                    (PlayerStatExpr::ControllerSacrificedThisTurn, 1)
                }
                "revolt"
                | "permanent_left_battlefield"
                | "a permanent you controlled left the battlefield this turn" => {
                    (PlayerStatExpr::PermanentLeftBattlefieldThisTurn, 1)
                }
                "descended" | "you descended this turn" => (PlayerStatExpr::DescendedThisTurn, 1),
                "crime" | "committed_crime" | "you committed a crime this turn" => {
                    (PlayerStatExpr::CommittedCrimeThisTurn, 1)
                }
                "cast_two_spells" | "you've cast two or more spells this turn" => {
                    (PlayerStatExpr::ControllerSpellsCastThisTurn, 2)
                }
                "drew_two_cards" | "you've drawn two or more cards this turn" => {
                    (PlayerStatExpr::CardsDrawnThisTurn, 2)
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(stat),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(min),
            })
        }
        ConditionAst::Marker(GameMarker::RemainsOnBattlefield) if in_trigger => {
            Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::TriggerSourceOnBattlefield),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        _ => Err("oracle:unsupported-if-condition"),
    }
}

fn lower_effect_body(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<ExecutableBody, &'static str> {
    let mut builder = BodyBuilder::new();
    lower_instruction_seq(
        ins,
        &mut builder,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        None,
    )?;
    builder.finish()
}

fn lower_instruction_seq(
    ins: &[InstructionAst],
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    let mut i = 0usize;
    while i < ins.len() {
        // Fuse paired ModifyPT + GrantKeywords on the exact same subject & duration
        // (emitted by clauses.rs for "Target creature gets +1/+1 and gains flying until end of turn").
        if i + 1 < ins.len()
            && let InstructionAst::ModifyPT {
                subject: s1,
                duration: d1,
                ..
            } = &ins[i]
            && let InstructionAst::GrantKeywords {
                subject: s2,
                keywords,
                duration: d2,
            } = &ins[i + 1]
            && s1 == s2
            && d1 == d2
        {
            lower_instruction(
                &ins[i],
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let kws = lower_keyword_list(keywords)?;
            if !kws.is_empty() {
                match builder.instructions.last_mut() {
                    Some(Instruction::BoostSource {
                        keywords: prev_kws, ..
                    }) => {
                        prev_kws.extend(kws);
                        i += 2;
                        continue;
                    }
                    Some(Instruction::BoostTargetPower {
                        keywords: prev_kws, ..
                    }) => {
                        prev_kws.extend(kws);
                        i += 2;
                        continue;
                    }
                    Some(Instruction::ModifyGroupUntilEndOfTurn {
                        keywords: prev_kws, ..
                    }) => {
                        prev_kws.extend(kws);
                        i += 2;
                        continue;
                    }
                    Some(Instruction::CreateContinuousEffect { parts, .. })
                    | Some(Instruction::ChangeAbilitySource { parts, .. })
                    | Some(Instruction::AffectPermanentTargets {
                        action: PermanentTargetAction::Modify { parts, .. },
                        ..
                    }) if !parts
                        .0
                        .iter()
                        .any(|p| matches!(p, ResolvedPartSpec::Abilities(_))) =>
                    {
                        parts
                            .0
                            .push(ResolvedPartSpec::Abilities(AbilityChange::grant_keywords(
                                kws,
                            )));
                        i += 2;
                        continue;
                    }
                    _ => {}
                }
            }
            i += 1;
            continue;
        }
        // Fuse Optional([SearchLibrary(shuffle: false)]) + ShuffleLibrary(You)
        if i + 1 < ins.len()
            && let InstructionAst::Optional(inner) = &ins[i]
            && let [
                InstructionAst::SearchLibrary {
                    library,
                    count,
                    filter,
                    reveal,
                    to_zone,
                    shuffle: false,
                },
            ] = inner.as_slice()
            && matches!(
                &ins[i + 1],
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You
                }
            )
        {
            let search =
                lower_library_search(library, count, filter, *reveal, to_zone, vocabulary, true)?;
            builder.instructions.push(Instruction::OptionalEffect {
                chooser: ResolvingPlayer::Controller,
            });
            builder.instructions.push(Instruction::SearchLibrary {
                filter: Box::new(search),
            });
            i += 2;
            continue;
        }
        // Fuse SearchLibrary(shuffle: false) + ShuffleLibrary(You) into native SearchLibrary
        if i + 1 < ins.len()
            && let InstructionAst::SearchLibrary {
                library,
                count,
                filter,
                reveal,
                to_zone,
                shuffle: false,
            } = &ins[i]
            && matches!(
                &ins[i + 1],
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You
                }
            )
        {
            let search =
                lower_library_search(library, count, filter, *reveal, to_zone, vocabulary, false)?;
            builder.instructions.push(Instruction::SearchLibrary {
                filter: Box::new(search),
            });
            i += 2;
            continue;
        }
        // Fuse Exile(target) + DelayedTrigger(EndStep, ReturnZone(Anaphora, battlefield)) -> ExileUntilNextEndStep
        if i + 1 < ins.len()
            && let InstructionAst::Exile {
                actor: None | Some(PlayerRefAst::You),
                subject,
            } = &ins[i]
            && let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
            && filter.zone.is_none()
            && let InstructionAst::DelayedTrigger {
                step: TurnStepAst::EndStep,
                body: delayed_body,
            } = &ins[i + 1]
            && let [
                InstructionAst::ReturnZone {
                    subject: ret_subj,
                    to,
                    tapped: false,
                    transformed: false,
                    with_counter: None,
                    attached_to: None,
                    ..
                },
            ] = delayed_body.as_slice()
            && matches!(ret_subj.as_ref(), SubjectRefAst::Anaphora(_))
            && matches!(
                to.trim().to_ascii_lowercase().as_str(),
                "battlefield" | "the battlefield" | "the battlefield under its owner's control"
            )
        {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::ExileUntilNextEndStep);
            i += 2;
            continue;
        }
        // Fuse FlipCoin + IfCondition(WonContest) (+ optional Otherwise) -> FlipCoin { lose_destination }
        if i + 1 < ins.len()
            && matches!(&ins[i], InstructionAst::FlipCoin)
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[i + 1]
            && matches!(condition.as_ref(), ConditionAst::WonContest)
        {
            let flip_idx = builder.instructions.len();
            builder.instructions.push(Instruction::FlipCoin {
                lose_destination: 0,
            });
            lower_instruction_seq(
                then_body,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if i + 2 < ins.len()
                && let InstructionAst::Otherwise(else_body) = &ins[i + 2]
            {
                let jump_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(Instruction::Jump { destination: 0 });
                let lose_dest = builder.instructions.len();
                if let Some(Instruction::FlipCoin { lose_destination }) =
                    builder.instructions.get_mut(flip_idx)
                {
                    *lose_destination = lose_dest;
                }
                lower_instruction_seq(
                    else_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                if let Some(Instruction::Jump { destination }) =
                    builder.instructions.get_mut(jump_idx)
                {
                    *destination = end_dest;
                }
                i += 3;
            } else {
                let lose_dest = builder.instructions.len();
                if let Some(Instruction::FlipCoin { lose_destination }) =
                    builder.instructions.get_mut(flip_idx)
                {
                    *lose_destination = lose_dest;
                }
                i += 2;
            }
            continue;
        }
        // Lower IfCondition (+ optional Otherwise) via BranchIfFalse + Jump (including And/Or)
        if let InstructionAst::IfCondition {
            condition,
            then_body,
        } = &ins[i]
        {
            let mut false_branches = Vec::new();
            match condition.as_ref() {
                ConditionAst::And(a, b) => {
                    let rc_a =
                        lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
                    let rc_b =
                        lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
                    false_branches.push(builder.instructions.len());
                    builder.instructions.push(Instruction::BranchIfFalse {
                        condition: rc_a,
                        destination: 0,
                    });
                    false_branches.push(builder.instructions.len());
                    builder.instructions.push(Instruction::BranchIfFalse {
                        condition: rc_b,
                        destination: 0,
                    });
                }
                ConditionAst::Or(a, b) => {
                    let mut rc_a =
                        lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
                    let rc_b =
                        lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
                    rc_a.comparison = negate_comparison(rc_a.comparison);
                    let true_jump_idx = builder.instructions.len();
                    builder.instructions.push(Instruction::BranchIfFalse {
                        condition: rc_a,
                        destination: 0,
                    });
                    false_branches.push(builder.instructions.len());
                    builder.instructions.push(Instruction::BranchIfFalse {
                        condition: rc_b,
                        destination: 0,
                    });
                    let then_start = builder.instructions.len();
                    if let Some(Instruction::BranchIfFalse { destination, .. }) =
                        builder.instructions.get_mut(true_jump_idx)
                    {
                        *destination = then_start;
                    }
                }
                other => {
                    let res_cond = lower_resolution_condition(
                        other, builder, vocabulary, in_trigger, bound_x,
                    )?;
                    false_branches.push(builder.instructions.len());
                    builder.instructions.push(Instruction::BranchIfFalse {
                        condition: res_cond,
                        destination: 0,
                    });
                }
            }
            lower_instruction_seq(
                then_body,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if i + 1 < ins.len()
                && let InstructionAst::Otherwise(else_body) = &ins[i + 1]
            {
                let jump_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(Instruction::Jump { destination: 0 });
                let else_dest = builder.instructions.len();
                for b_idx in false_branches {
                    if let Some(Instruction::BranchIfFalse { destination, .. }) =
                        builder.instructions.get_mut(b_idx)
                    {
                        *destination = else_dest;
                    }
                }
                lower_instruction_seq(
                    else_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                if let Some(Instruction::Jump { destination }) =
                    builder.instructions.get_mut(jump_idx)
                {
                    *destination = end_dest;
                }
                i += 2;
            } else {
                let end_dest = builder.instructions.len();
                for b_idx in false_branches {
                    if let Some(Instruction::BranchIfFalse { destination, .. }) =
                        builder.instructions.get_mut(b_idx)
                    {
                        *destination = end_dest;
                    }
                }
                i += 1;
            }
            continue;
        }
        // Fuse Optional([PayCost(PayMana(raw))]) + IfPreviousSucceeded(then_body) / ReflexiveTrigger(ref_body) -> OfferManaPayment
        if let InstructionAst::Optional(opt_inner) = &ins[i]
            && let [InstructionAst::PayCost(CostAtomAst::PayMana(raw))] = opt_inner.as_slice()
            && i + 1 < ins.len()
            && let Some(cost) = parse_fixed_mana_cost(raw)
        {
            match &ins[i + 1] {
                InstructionAst::IfPreviousSucceeded(then_body) => {
                    let offer_idx = builder.instructions.len();
                    builder.instructions.push(Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::Controller,
                        cost,
                        paid_destination: offer_idx + 1,
                        declined_destination: 0,
                    });
                    lower_instruction_seq(
                        then_body,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                    let end_dest = builder.instructions.len();
                    if let Some(Instruction::OfferManaPayment {
                        declined_destination,
                        ..
                    }) = builder.instructions.get_mut(offer_idx)
                    {
                        *declined_destination = end_dest;
                    }
                    i += 2;
                    continue;
                }
                InstructionAst::ReflexiveTrigger(ref_body) => {
                    let reflexive_body =
                        lower_effect_body(ref_body, vocabulary, ids, true, self_pronoun_valid)?;
                    let offer_idx = builder.instructions.len();
                    builder.instructions.push(Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::Controller,
                        cost,
                        paid_destination: offer_idx + 1,
                        declined_destination: offer_idx + 2,
                    });
                    builder
                        .instructions
                        .push(Instruction::EmitReflexiveTrigger {
                            body: Box::new(reflexive_body),
                        });
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        // Fuse OptionalPlayer { player, body: [PayCost(PayMana(raw))] } + IfPreviousFailed / IfPreviousSucceeded / ReflexiveTrigger -> OfferManaPayment
        if let InstructionAst::OptionalPlayer {
            player: opt_player,
            body: opt_inner,
        } = &ins[i]
            && let [InstructionAst::PayCost(CostAtomAst::PayMana(raw))] = opt_inner.as_slice()
            && i + 1 < ins.len()
            && let Some(cost) = parse_fixed_mana_cost(raw)
        {
            match &ins[i + 1] {
                InstructionAst::IfPreviousFailed(else_body) => {
                    let payer = lower_resolving_player(*opt_player, builder)?;
                    let offer_idx = builder.instructions.len();
                    builder.instructions.push(Instruction::OfferManaPayment {
                        payer,
                        cost,
                        paid_destination: 0,
                        declined_destination: offer_idx + 1,
                    });
                    lower_instruction_seq(
                        else_body,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                    let end_dest = builder.instructions.len();
                    if let Some(Instruction::OfferManaPayment {
                        paid_destination, ..
                    }) = builder.instructions.get_mut(offer_idx)
                    {
                        *paid_destination = end_dest;
                    }
                    i += 2;
                    continue;
                }
                InstructionAst::IfPreviousSucceeded(then_body) => {
                    let payer = lower_resolving_player(*opt_player, builder)?;
                    let offer_idx = builder.instructions.len();
                    builder.instructions.push(Instruction::OfferManaPayment {
                        payer,
                        cost,
                        paid_destination: offer_idx + 1,
                        declined_destination: 0,
                    });
                    lower_instruction_seq(
                        then_body,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                    let end_dest = builder.instructions.len();
                    if let Some(Instruction::OfferManaPayment {
                        declined_destination,
                        ..
                    }) = builder.instructions.get_mut(offer_idx)
                    {
                        *declined_destination = end_dest;
                    }
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        // Lower Optional([PayCost(PayMana(raw)), rest..]) -> OfferManaPayment skipping rest if declined
        if let InstructionAst::Optional(opt_inner) = &ins[i]
            && opt_inner.len() >= 2
            && let InstructionAst::PayCost(CostAtomAst::PayMana(raw)) = &opt_inner[0]
            && let Some(cost) = parse_fixed_mana_cost(raw)
        {
            let offer_idx = builder.instructions.len();
            builder.instructions.push(Instruction::OfferManaPayment {
                payer: ResolvingPlayer::Controller,
                cost,
                paid_destination: offer_idx + 1,
                declined_destination: 0,
            });
            lower_instruction_seq(
                &opt_inner[1..],
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let end_dest = builder.instructions.len();
            if let Some(Instruction::OfferManaPayment {
                declined_destination,
                ..
            }) = builder.instructions.get_mut(offer_idx)
            {
                *declined_destination = end_dest;
            }
            i += 1;
            continue;
        }
        // Lower ReflexiveTrigger ("When you do, ...") after an action/optional cost with CR 603.12 conditional guard
        if let InstructionAst::ReflexiveTrigger(ref_body) = &ins[i] {
            let prev_tracks_action = i > 0
                && matches!(
                    &ins[i - 1],
                    InstructionAst::Optional(_)
                        | InstructionAst::OptionalPlayer { .. }
                        | InstructionAst::Sacrifice { .. }
                        | InstructionAst::Discard { .. }
                        | InstructionAst::DiscardHand { .. }
                        | InstructionAst::GainPlayerCounters { .. }
                        | InstructionAst::PayCost(_)
                        | InstructionAst::RemoveCounters { .. }
                );
            let reflexive_body =
                lower_effect_body(ref_body, vocabulary, ids, true, self_pronoun_valid)?;
            if prev_tracks_action {
                let branch_idx = builder.instructions.len();
                builder.instructions.push(Instruction::BranchIfFalse {
                    condition: ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                    destination: branch_idx + 2,
                });
            }
            builder
                .instructions
                .push(Instruction::EmitReflexiveTrigger {
                    body: Box::new(reflexive_body),
                });
            i += 1;
            continue;
        }
        // Lower IfPreviousSucceeded / IfPreviousFailed right after an action that tracks last_action_succeeded
        if let InstructionAst::IfPreviousSucceeded(then_body)
        | InstructionAst::IfPreviousFailed(then_body) = &ins[i]
        {
            let succeeded = matches!(&ins[i], InstructionAst::IfPreviousSucceeded(_));
            let prev_tracks_action = i > 0
                && matches!(
                    &ins[i - 1],
                    InstructionAst::Optional(_)
                        | InstructionAst::OptionalPlayer { .. }
                        | InstructionAst::Sacrifice { .. }
                        | InstructionAst::Discard { .. }
                        | InstructionAst::DiscardHand { .. }
                        | InstructionAst::GainPlayerCounters { .. }
                        | InstructionAst::PayCost(_)
                        | InstructionAst::RemoveCounters { .. }
                );
            if prev_tracks_action {
                let is_single_sacrifice = matches!(
                    builder.instructions.last(),
                    Some(Instruction::Sacrifice {
                        players: AffectedPlayers::One(_),
                        amount: ValueExpr::Literal(v),
                        ..
                    }) if *v == 1.into()
                );
                let kind = if succeeded && is_single_sacrifice {
                    EventValueKind::LastSacrificeSucceeded
                } else {
                    EventValueKind::LastActionSucceeded
                };
                let branch_idx = builder.instructions.len();
                builder.instructions.push(Instruction::BranchIfFalse {
                    condition: ResolutionCondition {
                        left: ValueExpr::event(kind),
                        comparison: if succeeded {
                            Comparison::GreaterOrEqual
                        } else {
                            Comparison::Equal
                        },
                        right: ValueExpr::literal(if succeeded { 1 } else { 0 }),
                    },
                    destination: 0,
                });
                lower_instruction_seq(
                    then_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                if let Some(Instruction::BranchIfFalse { destination, .. }) =
                    builder.instructions.get_mut(branch_idx)
                {
                    *destination = end_dest;
                }
                i += 1;
                continue;
            }
        }
        // Lower DelayedTrigger right after CreateTokens / CreateAttackingTokens for sacrifice/exile tokens
        if let InstructionAst::DelayedTrigger { step, body } = &ins[i]
            && matches!(
                builder.instructions.last(),
                Some(Instruction::CreateTokens { .. } | Instruction::CreateAttackingTokens { .. })
            )
        {
            match (step, body.as_slice()) {
                (
                    TurnStepAst::EndStep,
                    [
                        InstructionAst::Sacrifice {
                            player: PlayerRefAst::You,
                            subject,
                        },
                    ],
                ) if matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(_) | SubjectRefAst::Group(_)
                ) =>
                {
                    builder
                        .instructions
                        .push(Instruction::ScheduleCreatedTokenSacrifice);
                    i += 1;
                    continue;
                }
                (
                    TurnStepAst::EndStep | TurnStepAst::EndOfCombat,
                    [
                        InstructionAst::Exile {
                            actor: None | Some(PlayerRefAst::You),
                            subject,
                        },
                    ],
                ) if matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(_) | SubjectRefAst::Group(_)
                ) =>
                {
                    let eng_step = if *step == TurnStepAst::EndOfCombat {
                        Step::EndCombat
                    } else {
                        Step::End
                    };
                    builder
                        .instructions
                        .push(Instruction::ScheduleCreatedTokenExile(eng_step));
                    i += 1;
                    continue;
                }
                _ => {}
            }
        }
        // Fuse paired impulse draw: ExileTopLibrary + GrantCastPermission -> ExileTopForPlay / ExileTopPlay
        if i + 1 < ins.len()
            && let InstructionAst::ExileTopLibrary {
                actor: None | Some(PlayerRefAst::You),
                whose,
                count,
                face_down: false,
            } = &ins[i]
            && matches!(whose.to_ascii_lowercase().as_str(), "your" | "own" | "")
            && let InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only,
                free,
                duration,
                ..
            } = &ins[i + 1]
        {
            let dur_low = duration.to_ascii_lowercase();
            let parsed_dur = if dur_low.contains("end of your next turn")
                || dur_low.contains("next end step")
                || dur_low.contains("until your next")
                || dur_low.contains("untilendofnextturn")
                || dur_low.contains("untilyournextturn")
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::UntilEndOfNextTurn)
            } else if dur_low.contains("this turn")
                || dur_low.contains("end of turn")
                || dur_low.contains("untilendofturn")
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::ThisTurn)
            } else if dur_low.contains("as long as")
                || dur_low.contains("foraslongas")
                || dur_low.contains("remains exiled")
                || dur_low == "permanent"
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::WhileExiled)
            } else {
                None
            };
            if let Some(impulse_dur) = parsed_dur {
                let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                if cnt == ValueExpr::literal(1)
                    && !*cast_only
                    && !*free
                    && impulse_dur == spellwake_engine::program::ImpulsePlayDuration::ThisTurn
                {
                    builder.instructions.push(Instruction::ExileTopForPlay {
                        until_next_from_source: false,
                    });
                } else {
                    builder.instructions.push(Instruction::ExileTopPlay {
                        count: cnt,
                        duration: impulse_dur,
                        cast_only: *cast_only,
                        free: *free,
                    });
                }
                i += 2;
                continue;
            }
        }
        // Fuse paired drain life:
        // "Each opponent loses N life and you gain N life" / "Target player loses N life and you gain N life"
        if i + 1 < ins.len()
            && let InstructionAst::ChangeLife {
                player: loser,
                delta: lose_amt,
                gain: false,
            } = &ins[i]
            && let InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                delta: gain_amt,
                gain: true,
            } = &ins[i + 1]
            && (lose_amt == gain_amt || matches!(gain_amt.as_ref(), ValueExprAst::PriorResult))
        {
            let amt = lower_value_expr(resolve_x(lose_amt, bound_x), vocabulary)?;
            match loser {
                PlayerRefAst::EachOpponent => {
                    builder
                        .instructions
                        .push(Instruction::OpponentsLoseAndControllerGains(amt));
                    i += 2;
                    continue;
                }
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(Instruction::DrainTargetAndGain(amt));
                    i += 2;
                    continue;
                }
                PlayerRefAst::TargetOpponent => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(Instruction::DrainTargetAndGain(amt));
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        // Fuse RevealUntil(You, filter, 1) + ChooseAndKeep / MoveZone (+ rest MoveZone or Shuffle) -> RevealUntilMatch
        if let InstructionAst::RevealUntil {
            player: PlayerRefAst::You,
            filter: until_filter,
            count: until_count,
        } = &ins[i]
            && matches!(until_count.as_ref(), ValueExprAst::Const(1))
            && let Ok(obj_filter) = lower_object_filter(until_filter, vocabulary)
        {
            // Pattern A: 2-step RevealUntil + ChooseAndKeep (e.g. Hermit Druid)
            if i + 1 < ins.len()
                && let InstructionAst::ChooseAndKeep {
                    pick,
                    keep_to,
                    rest_to,
                } = &ins[i + 1]
            {
                let pick_low = pick.trim().to_ascii_lowercase();
                let keep_low = keep_to.to_ascii_lowercase();
                let rest_low = rest_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && keep_low.contains("tapped");
                let keep_all = pick_low.contains("all cards revealed")
                    || pick_low == "all of them"
                    || pick_low == "them";
                let rest_bottom = rest_low.contains("bottom");
                let rest_gy = rest_low.contains("graveyard");
                let rest_exile = rest_low.contains("exile");
                if (to_hand || to_battlefield) && (rest_bottom || rest_gy || rest_exile || keep_all)
                {
                    builder.instructions.push(Instruction::RevealUntilMatch {
                        filter: Box::new(obj_filter),
                        to_battlefield,
                        enters_tapped,
                        keep_all_revealed: keep_all,
                        rest_to_graveyard: rest_gy,
                        rest_to_exile: rest_exile,
                        rest_to_bottom: rest_bottom,
                    });
                    i += 2;
                    continue;
                }
            }
            // Pattern B: 2- or 3-step RevealUntil + [Optional?]MoveZone(that card / all revealed -> hand/bf) + [MoveZone(rest -> bottom/gy/exile) | ShuffleLibrary]
            if i + 1 < ins.len() {
                let (move_subj, move_to, optional_put) = match &ins[i + 1] {
                    InstructionAst::MoveZone { subject, to } => {
                        (Some(subject.as_ref()), Some(to.as_str()), false)
                    }
                    InstructionAst::Optional(inner) => match inner.as_slice() {
                        [InstructionAst::MoveZone { subject, to }] => {
                            (Some(subject.as_ref()), Some(to.as_str()), true)
                        }
                        _ => (None, None, false),
                    },
                    _ => (None, None, false),
                };
                if !optional_put
                    && let (Some(SubjectRefAst::Anaphora(a)), Some(keep_to)) = (move_subj, move_to)
                {
                    let a_low = a.trim().to_ascii_lowercase();
                    let keep_all = a_low.contains("all cards revealed")
                        || a_low.contains("all revealed")
                        || a_low == "them"
                        || a_low == "those cards";
                    let single_card = a_low.contains("that card")
                        || a_low == "it"
                        || a_low.contains("revealed card");
                    let keep_low = keep_to.to_ascii_lowercase();
                    let to_hand = keep_low.contains("hand");
                    let to_battlefield = keep_low.contains("battlefield");
                    let enters_tapped = to_battlefield && keep_low.contains("tapped");
                    if (to_hand || to_battlefield) && keep_all {
                        builder.instructions.push(Instruction::RevealUntilMatch {
                            filter: Box::new(obj_filter),
                            to_battlefield,
                            enters_tapped,
                            keep_all_revealed: true,
                            rest_to_graveyard: false,
                            rest_to_exile: false,
                            rest_to_bottom: false,
                        });
                        i += 2;
                        continue;
                    }
                    if (to_hand || to_battlefield) && single_card && i + 2 < ins.len() {
                        match &ins[i + 2] {
                            InstructionAst::MoveZone {
                                subject: rest_subj,
                                to: rest_to,
                            } if matches!(
                                rest_subj.as_ref(),
                                SubjectRefAst::Anaphora(r) if {
                                    let rl = r.to_ascii_lowercase();
                                    rl.contains("rest") || rl.contains("other") || rl == "them"
                                }
                            ) =>
                            {
                                let rest_low = rest_to.to_ascii_lowercase();
                                let rest_bottom = rest_low.contains("bottom");
                                let rest_gy = rest_low.contains("graveyard");
                                let rest_exile = rest_low.contains("exile");
                                if rest_bottom || rest_gy || rest_exile {
                                    builder.instructions.push(Instruction::RevealUntilMatch {
                                        filter: Box::new(obj_filter),
                                        to_battlefield,
                                        enters_tapped,
                                        keep_all_revealed: false,
                                        rest_to_graveyard: rest_gy,
                                        rest_to_exile: rest_exile,
                                        rest_to_bottom: rest_bottom,
                                    });
                                    i += 3;
                                    continue;
                                }
                            }
                            InstructionAst::ShuffleLibrary {
                                player: PlayerRefAst::You,
                            } => {
                                builder.instructions.push(Instruction::RevealUntilMatch {
                                    filter: Box::new(obj_filter),
                                    to_battlefield,
                                    enters_tapped,
                                    keep_all_revealed: false,
                                    rest_to_graveyard: false,
                                    rest_to_exile: false,
                                    rest_to_bottom: false,
                                });
                                builder.instructions.push(Instruction::ShuffleOwnLibrary);
                                i += 3;
                                continue;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        // Fuse 3-step LookTopLibrary + [Optional(RevealFromLooked) | RevealFromLooked] + MoveZone(rest -> bottom/graveyard) -> LookTopChoose
        if i + 2 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(library.to_ascii_lowercase().as_str(), "your" | "own" | "")
            && let InstructionAst::MoveZone {
                subject: rest_subj,
                to: rest_to,
            } = &ins[i + 2]
            && matches!(
                rest_subj.as_ref(),
                SubjectRefAst::Anaphora(a) if matches!(
                    a.to_ascii_lowercase().as_str(),
                    "the rest" | "rest" | "the other cards" | "them" | "the rest of them"
                )
            )
        {
            let rev_opt = match &ins[i + 1] {
                InstructionAst::RevealFromLooked { filter, to } => Some((filter, to)),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [InstructionAst::RevealFromLooked { filter, to }] => Some((filter, to)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((filter_ast, keep_to)) = rev_opt {
                let keep_low = keep_to.to_ascii_lowercase();
                let rest_low = rest_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && keep_low.contains("tapped");
                let rest_bottom = rest_low.contains("bottom");
                let rest_gy = rest_low.contains("graveyard");
                let rest_exile = rest_low.contains("exile");
                if (to_hand || to_battlefield)
                    && (rest_bottom || rest_gy || rest_exile)
                    && let Ok(obj_filter) = lower_object_filter(filter_ast, vocabulary)
                {
                    let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                    builder.instructions.push(Instruction::LookTopChoose {
                        count: cnt,
                        filter: Some(Box::new(obj_filter)),
                        reveal: *reveal,
                        to_battlefield,
                        enters_tapped,
                        take_all_matching: false,
                        max_picks: 1,
                        rest_to_graveyard: rest_gy,
                        rest_to_exile: rest_exile,
                    });
                    i += 3;
                    continue;
                }
            }
        }
        // Fuse LookTopLibrary + ChooseAndKeep (e.g. Anticipate, Impulse, Sleight of Hand, Forbidden Alchemy, Commune, Mulch, Collected Company, Dig Through Time)
        if i + 1 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(library.to_ascii_lowercase().as_str(), "your" | "own" | "")
            && let InstructionAst::ChooseAndKeep {
                pick,
                keep_to,
                rest_to,
            } = &ins[i + 1]
        {
            let keep_low = keep_to.to_ascii_lowercase();
            let rest_low = rest_to.to_ascii_lowercase();
            let to_hand = keep_low.contains("hand");
            let to_battlefield = keep_low.contains("battlefield");
            let enters_tapped = to_battlefield && keep_low.contains("tapped");
            let rest_bottom = rest_low.contains("bottom");
            let rest_gy = rest_low.contains("graveyard");
            let rest_exile = rest_low.contains("exile");
            let pick_low = pick.trim().to_ascii_lowercase();
            if (to_hand || to_battlefield) && (rest_bottom || rest_gy || rest_exile) {
                let any_one = matches!(
                    pick_low.as_str(),
                    "one"
                        | "1"
                        | "one of them"
                        | "one of those cards"
                        | "a card from among them"
                        | "that card"
                        | "a card"
                        | "any one"
                );
                let any_multi: Option<u8> = match pick_low.as_str() {
                    "two"
                    | "2"
                    | "two of them"
                    | "up to two"
                    | "up to two of them"
                    | "two of those cards"
                    | "up to two cards from among them" => Some(2),
                    "three" | "3" | "three of them" | "up to three" | "up to three of them" => {
                        Some(3)
                    }
                    "four" | "4" | "four of them" | "up to four" | "up to four of them" => Some(4),
                    _ => None,
                };
                let (take_all_matching, max_picks, filter_opt): (
                    bool,
                    u8,
                    Option<Option<ObjectFilter>>,
                ) = if any_one {
                    (false, 1, Some(None))
                } else if let Some(k) = any_multi {
                    (false, k, Some(None))
                } else if let Some(rest) = pick_low
                    .strip_prefix("all ")
                    .or_else(|| pick_low.strip_prefix("each "))
                {
                    let cleaned = rest
                        .trim_end_matches(" revealed this way")
                        .trim_end_matches(" cards")
                        .trim_end_matches(" card")
                        .trim();
                    (
                        true,
                        1,
                        parse_look_top_pick_filter(cleaned, vocabulary).map(Some),
                    )
                } else if let Some((k, rest)) = pick_low
                    .strip_prefix("up to two ")
                    .or_else(|| pick_low.strip_prefix("two "))
                    .map(|r| (2u8, r))
                    .or_else(|| {
                        pick_low
                            .strip_prefix("up to three ")
                            .or_else(|| pick_low.strip_prefix("three "))
                            .map(|r| (3u8, r))
                    })
                    .or_else(|| {
                        pick_low
                            .strip_prefix("up to four ")
                            .or_else(|| pick_low.strip_prefix("four "))
                            .map(|r| (4u8, r))
                    })
                    .or_else(|| {
                        pick_low
                            .strip_prefix("any number of ")
                            .map(|r| (u8::MAX, r))
                    })
                {
                    let cleaned = rest
                        .trim_end_matches(" revealed this way")
                        .trim_end_matches(" from among them")
                        .trim_end_matches(" cards")
                        .trim_end_matches(" card")
                        .trim();
                    (
                        false,
                        k,
                        parse_look_top_pick_filter(cleaned, vocabulary).map(Some),
                    )
                } else {
                    (
                        false,
                        1,
                        parse_look_top_pick_filter(&pick_low, vocabulary).map(Some),
                    )
                };
                if let Some(maybe_filter) = filter_opt {
                    let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                    if !*reveal
                        && !take_all_matching
                        && max_picks == 1
                        && maybe_filter.is_none()
                        && to_hand
                        && rest_bottom
                    {
                        builder
                            .instructions
                            .push(Instruction::LookTopOneRestBottom { count: cnt });
                        i += 2;
                        continue;
                    }
                    if !*reveal
                        && !take_all_matching
                        && max_picks == 1
                        && maybe_filter.is_none()
                        && to_hand
                        && rest_gy
                        && let ValueExpr::Literal(n) = &cnt
                        && let Some(u) = n.to_usize().and_then(|v| u8::try_from(v).ok())
                        && u > 0
                    {
                        builder
                            .instructions
                            .push(Instruction::LookTopOneRestGraveyard { count: u });
                        i += 2;
                        continue;
                    }
                    builder.instructions.push(Instruction::LookTopChoose {
                        count: cnt,
                        filter: maybe_filter.map(Box::new),
                        reveal: *reveal,
                        to_battlefield,
                        enters_tapped,
                        take_all_matching,
                        max_picks,
                        rest_to_graveyard: rest_gy,
                        rest_to_exile: rest_exile,
                    });
                    i += 2;
                    continue;
                }
            }
        }
        // Fuse RevealHand(target opponent) + Discard(chosen card) -> ChooseRevealedHandCard
        if i + 1 < ins.len()
            && let InstructionAst::RevealHand {
                player: PlayerRefAst::TargetOpponent,
            } = &ins[i]
            && let InstructionAst::Discard {
                player: PlayerRefAst::ThatPlayer | PlayerRefAst::TargetOpponent,
                count,
                filter,
                anaphora: _,
                random: false,
            } = &ins[i + 1]
        {
            let cnt_u8: Option<u8> = match count.as_deref() {
                None => Some(1),
                Some(ValueExprAst::Const(n)) if *n > 0 => u8::try_from(*n).ok(),
                _ => None,
            };
            let filter_spec: Option<(bool, u16)> = match filter.as_deref() {
                None => Some((false, 0)),
                Some(f)
                    if f.any_types.is_empty()
                        && f.subtypes.is_empty()
                        && f.any_colors.is_empty()
                        && f.forbidden_colors.is_empty()
                        && f.color_arity.is_none()
                        && f.required_keywords.is_empty()
                        && f.forbidden_keywords.is_empty()
                        && f.flags.is_empty() =>
                {
                    let nonland = f.forbidden_types == AstCardTypeMask::LAND;
                    let ok_forb = nonland || f.forbidden_types.is_empty();
                    let mv = if f.stat_bounds.is_empty() {
                        Some(0u16)
                    } else if f.stat_bounds.len() == 1
                        && f.stat_bounds[0].stat == StatKind::ManaValue
                        && f.stat_bounds[0].op == CompareOp::Ge
                        && let ValueExprAst::Const(v) = f.stat_bounds[0].value.as_ref()
                    {
                        u16::try_from(*v).ok()
                    } else {
                        None
                    };
                    if ok_forb {
                        mv.map(|m| (nonland, m))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let (Some(n), Some((nonland_only, minimum_mana_value))) = (cnt_u8, filter_spec)
                && let Some(nz) = NonZeroU8::new(n)
            {
                builder.bind_target(BodyTarget::SingleOpponent)?;
                builder
                    .instructions
                    .push(Instruction::ChooseRevealedHandCard {
                        minimum_mana_value,
                        nonland_only,
                        count: nz,
                        action: ChosenHandAction::Discard,
                    });
                i += 2;
                continue;
            }
        }
        // Fuse CreateToken (1 untapped non-attacking controlled token) + Attach(self -> it) -> CreateTokenAndAttach
        if i + 1 < ins.len()
            && let InstructionAst::CreateToken {
                controller: PlayerRefAst::You,
                token,
            } = &ins[i]
            && let InstructionAst::Attach { subject, target } = &ins[i + 1]
            && is_self_pronoun(subject, builder, self_pronoun_valid)
            && matches!(target.as_ref(), SubjectRefAst::Anaphora(_))
            && let Ok((creation, false)) =
                lower_token_spec(token, TokenRecipients::Controller, vocabulary, ids, bound_x)
            && !creation.enters_tapped
            && creation.amount == ValueExpr::literal(1)
            && creation.base_size.is_none()
        {
            builder
                .instructions
                .push(Instruction::CreateTokenAndAttach {
                    definition: Box::new(creation.definition),
                });
            i += 2;
            continue;
        }
        // Fuse bite spells: "Target creature you control deals damage equal to its power to target creature you don't control."
        // (+ optional "If the creature you control has trample, excess damage is dealt to that creature's controller instead.")
        if let InstructionAst::DealDamage {
            source,
            target,
            amount,
        } = &ins[i]
            && let SubjectRefAst::TargetObject {
                filter: src_f,
                count: TargetCountAst::Exact(1),
            } = source.as_ref()
            && let SubjectRefAst::TargetObject {
                filter: dst_f,
                count: TargetCountAst::Exact(1),
            } = target.as_ref()
            && src_f.zone.is_none()
            && dst_f.zone.is_none()
            && matches!(
                amount.as_ref(),
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Power,
                } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
            )
            && let Ok(src_bt) = lower_battlefield_body_target(src_f, vocabulary)
            && let Ok(dst_bt) = lower_battlefield_body_target(dst_f, vocabulary)
            && src_bt == BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled())
            && dst_bt == BodyTarget::FilteredPermanent(ObjectFilter::creature().opposing())
            && builder.target == BodyTarget::None
            && builder.instructions.is_empty()
        {
            let trample_overflow = i + 1 < ins.len()
                && matches!(
                    &ins[i + 1],
                    InstructionAst::AbilityRestriction {
                        restriction,
                        condition: None,
                    } if restriction == "ExcessDamageToController"
                );
            let fixed = |bt: BodyTarget| {
                Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                    bt,
                    vec![Instruction::RememberPermanentTarget],
                )))
            };
            builder.instructions.push(fixed(src_bt));
            builder.instructions.push(fixed(dst_bt));
            builder
                .instructions
                .push(Instruction::CreatureDealsPowerDamageToOther { trample_overflow });
            i += if trample_overflow { 2 } else { 1 };
            continue;
        }
        // Fuse AnimateOrSetBasePT (+ optional KeepExistingTypes) -> ResolvedPartSpec::SetPT + Type + Color + Abilities
        if let InstructionAst::AnimateOrSetBasePT {
            subject,
            power,
            toughness,
            description,
            keywords,
            duration,
        } = &ins[i]
        {
            let keep_types =
                i + 1 < ins.len() && matches!(&ins[i + 1], InstructionAst::KeepExistingTypes(_));
            lower_animate_or_set_pt(
                subject,
                power,
                toughness,
                description.as_deref(),
                keywords,
                duration,
                keep_types,
                builder,
                vocabulary,
                self_pronoun_valid,
            )?;
            i += if keep_types { 2 } else { 1 };
            continue;
        }
        lower_instruction(
            &ins[i],
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )?;
        i += 1;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_animate_or_set_pt(
    subject: &SubjectRefAst,
    power: &str,
    toughness: &str,
    description: Option<&str>,
    keywords: &[String],
    duration: &DurationAst,
    keep_types: bool,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    let p: i64 = power
        .trim()
        .parse()
        .map_err(|_| "oracle:animate-non-const-pt")?;
    let t: i64 = toughness
        .trim()
        .parse()
        .map_err(|_| "oracle:animate-non-const-pt")?;
    let dur = lower_duration(duration)?;
    let mut parts = Vec::new();
    if let Some(desc) = description {
        let mut colors = BTreeSet::new();
        let mut explicit_colorless = false;
        let mut card_types = BTreeSet::new();
        let mut subtypes = BTreeSet::new();
        let mut extra_kws = Vec::new();
        let clean = desc
            .replace(" in addition to its other types", "")
            .replace(" and ", " ");
        let add_types = keep_types || desc.to_ascii_lowercase().contains("in addition to");
        for raw_tok in clean.split_whitespace() {
            let tok = raw_tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'');
            if tok.is_empty() {
                continue;
            }
            match tok.to_ascii_lowercase().as_str() {
                "a" | "an" | "with" | "still" | "that's" => {}
                "white" => {
                    colors.insert(Color::White);
                }
                "blue" => {
                    colors.insert(Color::Blue);
                }
                "black" => {
                    colors.insert(Color::Black);
                }
                "red" => {
                    colors.insert(Color::Red);
                }
                "green" => {
                    colors.insert(Color::Green);
                }
                "colorless" => {
                    explicit_colorless = true;
                }
                "creature" => {
                    card_types.insert(CardType::Creature);
                }
                "artifact" => {
                    card_types.insert(CardType::Artifact);
                }
                "enchantment" => {
                    card_types.insert(CardType::Enchantment);
                }
                "land" => {
                    card_types.insert(CardType::Land);
                }
                kw_cand if lower_keyword_ability_name(kw_cand).is_some() => {
                    extra_kws.push(kw_cand.to_string());
                }
                _ => {
                    if let Some(ct) = vocabulary.resolve_label(tok) {
                        subtypes.insert(ct);
                    } else {
                        return Err("oracle:unsupported-animate-desc");
                    }
                }
            }
        }
        if !card_types.is_empty() || !subtypes.is_empty() {
            let types_op = if card_types.is_empty() {
                TypeOperation::Keep
            } else if add_types {
                TypeOperation::Add(card_types)
            } else {
                TypeOperation::Set(card_types)
            };
            let sub_op = if subtypes.is_empty() {
                SubtypeOperation::Keep
            } else if add_types {
                SubtypeOperation::Add(InstructionSubtypeOperand::Literal(subtypes))
            } else {
                SubtypeOperation::Set(InstructionSubtypeOperand::Literal(subtypes))
            };
            parts.push(ResolvedPartSpec::Type(TypeChangeSpec {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: types_op,
                subtypes: sub_op,
            }));
        }
        if !colors.is_empty() {
            parts.push(ResolvedPartSpec::Color(ColorOperation::Set(colors)));
        } else if explicit_colorless {
            parts.push(ResolvedPartSpec::Color(
                ColorOperation::Set(BTreeSet::new()),
            ));
        }
        if !extra_kws.is_empty() || !keywords.is_empty() {
            let mut merged: Vec<String> = keywords.to_vec();
            merged.extend(extra_kws);
            parts.push(ResolvedPartSpec::Abilities(lower_ability_change(
                &merged, vocabulary,
            )?));
        }
    } else if !keywords.is_empty() {
        parts.push(ResolvedPartSpec::Abilities(lower_ability_change(
            keywords, vocabulary,
        )?));
    }
    parts.push(ResolvedPartSpec::SetPT(PowerToughness {
        power: p.into(),
        toughness: t.into(),
    }));
    let spec = ResolvedPartsSpec(parts);
    match subject {
        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
            builder.instructions.push(Instruction::ChangeAbilitySource {
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if filter.zone.is_none() => {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: spec,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: spec,
                    duration: dur,
                });
            Ok(())
        }
        _ => Err("oracle:unsupported-animate-subject"),
    }
}

fn resolve_x<'a>(expr: &'a ValueExprAst, bound_x: Option<&'a ValueExprAst>) -> &'a ValueExprAst {
    if matches!(expr, ValueExprAst::ChosenX)
        && let Some(bx) = bound_x
    {
        return bx;
    }
    expr
}

fn is_unrestricted_filter(f: &ObjectFilterAst) -> bool {
    f.any_types.is_empty()
        && f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.controller.is_none()
        && f.zone.is_none()
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.counter_kind.is_none()
        && f.flags.is_empty()
        && !f.any_permanent
}

fn lower_target_subject(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    match subject {
        SubjectRefAst::AnyTarget => Ok(BodyTarget::SingleAnyTarget),
        SubjectRefAst::TargetPlayerOrPlaneswalker {
            opponent_only: false,
        } => Ok(BodyTarget::SinglePlayerOrPlaneswalker),
        SubjectRefAst::TargetPlayerOrPlaneswalker {
            opponent_only: true,
        } => Ok(BodyTarget::SingleOpponentOrPlaneswalker),
        SubjectRefAst::TargetPlayer {
            opponent_only,
            count: TargetCountAst::Exact(1),
        } => Ok(if *opponent_only {
            BodyTarget::SingleOpponent
        } else {
            BodyTarget::SinglePlayer
        }),
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } => {
            if let Some(zone) = filter.zone {
                let mut inner = filter.as_ref().clone();
                inner.zone = None;
                return match zone {
                    ZoneFilter::YourGraveyard => {
                        if is_unrestricted_filter(&inner) {
                            Ok(BodyTarget::SingleOwnGraveyardCard)
                        } else {
                            Ok(BodyTarget::FilteredOwnGraveyardCard(lower_object_filter(
                                &inner, vocabulary,
                            )?))
                        }
                    }
                    ZoneFilter::AnyGraveyard => {
                        if is_unrestricted_filter(&inner) {
                            Ok(BodyTarget::AnyGraveyardCard)
                        } else if inner.any_types == AstCardTypeMask::CREATURE
                            && inner.forbidden_types.is_empty()
                            && inner.subtypes.is_empty()
                            && inner.any_colors.is_empty()
                            && inner.forbidden_colors.is_empty()
                            && inner.stat_bounds.is_empty()
                            && inner.flags.is_empty()
                        {
                            Ok(BodyTarget::AnyGraveyardCreature)
                        } else if inner.any_types
                            == AstCardTypeMask::CREATURE.union(AstCardTypeMask::PLANESWALKER)
                            && inner.forbidden_types.is_empty()
                            && inner.subtypes.is_empty()
                            && inner.flags.is_empty()
                        {
                            Ok(BodyTarget::AnyGraveyardCreatureOrPlaneswalker)
                        } else {
                            Ok(BodyTarget::FilteredAnyGraveyardCard {
                                filter: lower_object_filter(&inner, vocabulary)?,
                                opponents_only: false,
                            })
                        }
                    }
                    ZoneFilter::OpponentGraveyard => {
                        let filter = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        Ok(BodyTarget::FilteredAnyGraveyardCard {
                            filter,
                            opponents_only: true,
                        })
                    }
                    ZoneFilter::Stack => Ok(BodyTarget::SingleSpell(lower_spell_filter(
                        &inner, vocabulary,
                    )?)),
                    ZoneFilter::Battlefield => lower_battlefield_body_target(&inner, vocabulary),
                    _ => Err("oracle:unsupported-target-zone"),
                };
            }
            if filter.any_spell {
                let mut inner = filter.as_ref().clone();
                inner.any_spell = false;
                return Ok(BodyTarget::SingleSpell(lower_spell_filter(
                    &inner, vocabulary,
                )?));
            }
            lower_battlefield_body_target(filter, vocabulary)
        }
        _ => Err("oracle:unsupported-target-subject"),
    }
}

fn lower_battlefield_body_target(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BodyTarget, &'static str> {
    if f.any_permanent
        && f.any_types.is_empty()
        && f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.controller.is_none()
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.flags.is_empty()
    {
        return Ok(BodyTarget::SinglePermanent);
    }
    let obj = lower_object_filter(f, vocabulary)?;
    if obj == ObjectFilter::creature() {
        Ok(BodyTarget::SingleCreature)
    } else if obj == ObjectFilter::planeswalker() {
        Ok(BodyTarget::SinglePlaneswalker)
    } else {
        Ok(BodyTarget::FilteredPermanent(obj))
    }
}

fn lower_resolving_player(
    p: PlayerRefAst,
    builder: &mut BodyBuilder,
) -> Result<ResolvingPlayer, &'static str> {
    match p {
        PlayerRefAst::You => Ok(ResolvingPlayer::Controller),
        PlayerRefAst::TargetPlayer => {
            builder.bind_target(BodyTarget::SinglePlayer)?;
            Ok(ResolvingPlayer::TargetPlayer)
        }
        PlayerRefAst::TargetOpponent => {
            builder.bind_target(BodyTarget::SingleOpponent)?;
            Ok(ResolvingPlayer::TargetPlayer)
        }
        PlayerRefAst::ThatPlayer => match builder.target {
            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => {
                Ok(ResolvingPlayer::TargetPlayer)
            }
            BodyTarget::SingleSpell(_) => Ok(ResolvingPlayer::TargetSpellController),
            BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SingleAnyTarget => Ok(ResolvingPlayer::TargetController),
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_) => Ok(ResolvingPlayer::TargetOwner),
            _ => Ok(ResolvingPlayer::UniqueRelatedPlayer),
        },
        PlayerRefAst::ItsController => match builder.target {
            BodyTarget::SingleSpell(_) => Ok(ResolvingPlayer::TargetSpellController),
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
            | BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SingleAnyTarget => Ok(ResolvingPlayer::TargetController),
            _ => Err("oracle:unsupported-its-controller"),
        },
        PlayerRefAst::ItsOwner => match builder.target {
            BodyTarget::SinglePermanent
            | BodyTarget::SingleCreature
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
            | BodyTarget::SingleOwnGraveyardCard
            | BodyTarget::FilteredOwnGraveyardCard(_)
            | BodyTarget::FilteredAnyGraveyardCard { .. }
            | BodyTarget::AnyGraveyardCard
            | BodyTarget::AnyGraveyardCreature
            | BodyTarget::SingleSpell(_) => Ok(ResolvingPlayer::TargetOwner),
            _ => Err("oracle:unsupported-its-owner"),
        },
        _ => Err("oracle:unsupported-resolving-player"),
    }
}

fn lower_affected_players(
    p: PlayerRefAst,
    builder: &mut BodyBuilder,
) -> Result<AffectedPlayers, &'static str> {
    match p {
        PlayerRefAst::EachPlayer => Ok(AffectedPlayers::EachPlayer),
        PlayerRefAst::EachOpponent => Ok(AffectedPlayers::EachOpponent),
        PlayerRefAst::EachOtherPlayer => Ok(AffectedPlayers::EachOtherPlayer),
        other => Ok(AffectedPlayers::One(lower_resolving_player(
            other, builder,
        )?)),
    }
}

fn lower_duration(d: &DurationAst) -> Result<ResolvedDuration, &'static str> {
    match d {
        DurationAst::UntilEndOfTurn => Ok(ResolvedDuration::UntilEndOfTurn),
        DurationAst::Permanent | DurationAst::Instant => Ok(ResolvedDuration::NoStatedDuration),
        DurationAst::UntilSourceLeaves => Ok(ResolvedDuration::WhileControllingAbilitySource),
        _ => Err("oracle:unsupported-duration"),
    }
}

fn lower_instruction(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    match ins {
        InstructionAst::BindX { x_expr, body } => lower_instruction_seq(
            body,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            Some(x_expr),
        ),
        InstructionAst::Optional(inner) => {
            if let [
                InstructionAst::SearchLibrary {
                    library,
                    count,
                    filter,
                    reveal,
                    to_zone,
                    shuffle: true,
                },
            ] = inner.as_slice()
            {
                let search = lower_library_search(
                    library, count, filter, *reveal, to_zone, vocabulary, true,
                )?;
                builder.instructions.push(Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::Controller,
                });
                builder.instructions.push(Instruction::SearchLibrary {
                    filter: Box::new(search),
                });
                return Ok(());
            }
            if let [
                InstructionAst::SearchLibrary {
                    library,
                    count,
                    filter,
                    reveal,
                    to_zone,
                    shuffle: false,
                },
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You,
                },
            ] = inner.as_slice()
            {
                let search = lower_library_search(
                    library, count, filter, *reveal, to_zone, vocabulary, true,
                )?;
                builder.instructions.push(Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::Controller,
                });
                builder.instructions.push(Instruction::SearchLibrary {
                    filter: Box::new(search),
                });
                return Ok(());
            }
            let start = builder.instructions.len();
            lower_instruction_seq(
                inner,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let count = builder.instructions.len() - start;
            if count == 0 {
                return Err("oracle:empty-optional");
            }
            if count == 1 {
                builder.instructions.insert(
                    start,
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                );
            } else {
                builder.instructions.insert(
                    start,
                    Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: count,
                    },
                );
            }
            Ok(())
        }
        InstructionAst::OptionalPlayer { player, body } => {
            let chooser = lower_resolving_player(*player, builder)?;
            let start = builder.instructions.len();
            lower_instruction_seq(
                body,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let count = builder.instructions.len() - start;
            if count == 0 {
                return Err("oracle:empty-optional");
            }
            if count == 1 {
                builder
                    .instructions
                    .insert(start, Instruction::OptionalEffect { chooser });
            } else {
                builder.instructions.insert(
                    start,
                    Instruction::OptionalSequence {
                        chooser,
                        instructions: count,
                    },
                );
            }
            Ok(())
        }
        InstructionAst::Draw { player, count } => {
            let amount = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            match player {
                PlayerRefAst::You => {
                    builder
                        .instructions
                        .push(Instruction::DrawController { amount });
                    Ok(())
                }
                PlayerRefAst::EachPlayer => {
                    builder
                        .instructions
                        .push(Instruction::DrawEachPlayer { amount });
                    Ok(())
                }
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(Instruction::DrawTargetPlayer { amount });
                    Ok(())
                }
                PlayerRefAst::TargetOpponent => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(Instruction::DrawTargetPlayer { amount });
                    Ok(())
                }
                PlayerRefAst::ThatPlayer => {
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) {
                        builder
                            .instructions
                            .push(Instruction::DrawTargetPlayer { amount });
                        Ok(())
                    } else if amount == ValueExpr::literal(1) {
                        builder.instructions.push(Instruction::DrawRelatedPlayer);
                        Ok(())
                    } else {
                        Err("oracle:draw-related-multi")
                    }
                }
                _ => Err("oracle:unsupported-draw-player"),
            }
        }
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => {
            if !is_self_pronoun(source, builder, self_pronoun_valid) {
                // Non-self creature deals damage equal to its power:
                // 1) "Target creature deals damage equal to its power to ~" -> TargetCreatureToAbilitySource
                // 2) "Target creature deals damage equal to its power to itself" -> TargetCreature
                let is_its_power = matches!(
                    amount.as_ref(),
                    ValueExprAst::ObjectStat {
                        subject,
                        stat: StatKind::Power,
                    } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                );
                let bt_is_creature = |bt: BodyTarget| {
                    matches!(bt, BodyTarget::SingleCreature)
                        || matches!(
                            bt,
                            BodyTarget::FilteredPermanent(ObjectFilter::Clause(c))
                                if c.any_types == CardTypeMask::CREATURE
                        )
                };
                if is_its_power {
                    match source.as_ref() {
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::Exact(1),
                        } if filter.zone.is_none() => {
                            let bt = lower_battlefield_body_target(filter, vocabulary)?;
                            if bt_is_creature(bt) {
                                if is_self_pronoun(target, builder, self_pronoun_valid) {
                                    builder.bind_target(bt)?;
                                    builder.instructions.push(Instruction::DamageFromPower(
                                        PowerDamageSource::TargetCreatureToAbilitySource,
                                    ));
                                    return Ok(());
                                }
                                if matches!(target.as_ref(), SubjectRefAst::Anaphora(a) if a.eq_ignore_ascii_case("itself"))
                                {
                                    builder.bind_target(bt)?;
                                    builder.instructions.push(Instruction::DamageFromPower(
                                        PowerDamageSource::TargetCreature,
                                    ));
                                    return Ok(());
                                }
                            }
                        }
                        SubjectRefAst::Anaphora(_) if bt_is_creature(builder.target) => {
                            if is_self_pronoun(target, builder, self_pronoun_valid) {
                                builder.instructions.push(Instruction::DamageFromPower(
                                    PowerDamageSource::TargetCreatureToAbilitySource,
                                ));
                                return Ok(());
                            }
                            if matches!(target.as_ref(), SubjectRefAst::Anaphora(a) if a.eq_ignore_ascii_case("itself"))
                            {
                                builder.instructions.push(Instruction::DamageFromPower(
                                    PowerDamageSource::TargetCreature,
                                ));
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                }
                return Err("oracle:non-self-damage-source");
            }
            let amt_expr = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
            match target.as_ref() {
                SubjectRefAst::AnyTarget
                | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                | SubjectRefAst::TargetPlayer { .. }
                | SubjectRefAst::TargetObject { .. } => {
                    let bt = lower_target_subject(target, vocabulary)?;
                    builder.bind_target(bt)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    push_damage_target(&mut builder.instructions, amt_expr);
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::EachOpponent) => {
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder.instructions.push(Instruction::DamageGroup {
                            group: DamageGroup::Opponents,
                            amount: n,
                        });
                    } else {
                        builder
                            .instructions
                            .push(Instruction::DamageOpponentsAmount(amt_expr));
                    }
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::EachPlayer) => {
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder.instructions.push(Instruction::DamageGroup {
                            group: DamageGroup::Players,
                            amount: n,
                        });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group: DamageGroup::Players,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::You) => {
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder.instructions.push(Instruction::DamageGroup {
                            group: DamageGroup::Controller,
                            amount: n,
                        });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group: DamageGroup::Controller,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder.instructions.push(Instruction::DamageGroup {
                            group: DamageGroup::AbilitySource,
                            amount: n,
                        });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group: DamageGroup::AbilitySource,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer) => {
                    builder
                        .instructions
                        .push(Instruction::DamageRelatedPlayerAmount(amt_expr));
                    Ok(())
                }
                SubjectRefAst::Each(f) | SubjectRefAst::All(f) => {
                    let group = lower_damage_group(f, vocabulary)?;
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder
                            .instructions
                            .push(Instruction::DamageGroup { group, amount: n });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Compound(items) if items.len() == 2 => {
                    let is_all_creatures = |s: &SubjectRefAst| {
                        matches!(
                            s,
                            SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                if f.any_types == AstCardTypeMask::CREATURE
                                    && f.forbidden_types.is_empty()
                                    && f.controller.is_none()
                                    && f.subtypes.is_empty()
                                    && f.forbidden_subtypes.is_empty()
                                    && f.any_colors.is_empty()
                                    && f.forbidden_colors.is_empty()
                                    && f.required_keywords.is_empty()
                                    && f.forbidden_keywords.is_empty()
                                    && f.flags.is_empty()
                        )
                    };
                    let is_all_pws = |s: &SubjectRefAst| {
                        matches!(
                            s,
                            SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                                if f.any_types == AstCardTypeMask::PLANESWALKER
                                    && f.forbidden_types.is_empty()
                                    && f.controller.is_none()
                                    && f.flags.is_empty()
                        )
                    };
                    let is_each_player = |s: &SubjectRefAst| {
                        matches!(s, SubjectRefAst::Player(PlayerRefAst::EachPlayer))
                    };
                    let group = if (is_all_creatures(&items[0]) && is_each_player(&items[1]))
                        || (is_each_player(&items[0]) && is_all_creatures(&items[1]))
                    {
                        DamageGroup::CreaturesAndPlayers
                    } else if (is_all_creatures(&items[0]) && is_all_pws(&items[1]))
                        || (is_all_pws(&items[0]) && is_all_creatures(&items[1]))
                    {
                        DamageGroup::CreaturesAndPlaneswalkers
                    } else {
                        return Err("oracle:unsupported-damage-target");
                    };
                    if let ValueExpr::Literal(n) = amt_expr {
                        builder
                            .instructions
                            .push(Instruction::DamageGroup { group, amount: n });
                    } else {
                        builder.instructions.push(Instruction::DamageGroupAmount {
                            group,
                            amount: amt_expr,
                        });
                    }
                    Ok(())
                }
                _ => Err("oracle:unsupported-damage-target"),
            }
        }
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } => {
            let expr = lower_value_expr(resolve_x(delta, bound_x), vocabulary)?;
            match (player, *gain, expr) {
                (PlayerRefAst::You, true, ValueExpr::Literal(n)) => {
                    builder
                        .instructions
                        .push(Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(n),
                        });
                    Ok(())
                }
                (PlayerRefAst::You, true, expr) => {
                    builder
                        .instructions
                        .push(Instruction::GainControllerLife { amount: expr });
                    Ok(())
                }
                (PlayerRefAst::You, false, ValueExpr::Literal(n)) => {
                    builder
                        .instructions
                        .push(Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Lose(n),
                        });
                    Ok(())
                }
                (PlayerRefAst::TargetPlayer, g, ValueExpr::Literal(n)) => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder.instructions.push(Instruction::AdjustTargetLife {
                        adjustment: if g {
                            LifeAdjustment::Gain(n)
                        } else {
                            LifeAdjustment::Lose(n)
                        },
                    });
                    Ok(())
                }
                (PlayerRefAst::TargetOpponent, g, ValueExpr::Literal(n)) => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder.instructions.push(Instruction::AdjustTargetLife {
                        adjustment: if g {
                            LifeAdjustment::Gain(n)
                        } else {
                            LifeAdjustment::Lose(n)
                        },
                    });
                    Ok(())
                }
                (PlayerRefAst::ThatPlayer, g, ValueExpr::Literal(n))
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
                {
                    builder.instructions.push(Instruction::AdjustTargetLife {
                        adjustment: if g {
                            LifeAdjustment::Gain(n)
                        } else {
                            LifeAdjustment::Lose(n)
                        },
                    });
                    Ok(())
                }
                (PlayerRefAst::EachOpponent, g, ValueExpr::Literal(n)) => {
                    builder
                        .instructions
                        .push(Instruction::AdjustEachOpponentLife {
                            adjustment: if g {
                                LifeAdjustment::Gain(n)
                            } else {
                                LifeAdjustment::Lose(n)
                            },
                            controller_adjustment: None,
                        });
                    Ok(())
                }
                (PlayerRefAst::EachPlayer, g, ValueExpr::Literal(n)) => {
                    let adj = if g {
                        LifeAdjustment::Gain(n)
                    } else {
                        LifeAdjustment::Lose(n)
                    };
                    builder
                        .instructions
                        .push(Instruction::AdjustEachOpponentLife {
                            adjustment: adj.clone(),
                            controller_adjustment: Some(adj),
                        });
                    Ok(())
                }
                _ => Err("oracle:unsupported-life-change"),
            }
        }
        InstructionAst::SetLifeTotal { player, value } => {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(value, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-set-life");
            };
            match player {
                PlayerRefAst::You => {
                    builder
                        .instructions
                        .push(Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Set(n),
                        });
                    Ok(())
                }
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder.instructions.push(Instruction::AdjustTargetLife {
                        adjustment: LifeAdjustment::Set(n),
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-set-life"),
            }
        }
        InstructionAst::Destroy { subject } => match subject.as_ref() {
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::DestroyTarget);
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(n),
            } if *n >= 2 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: *n,
                        maximum: *n,
                        action: PermanentTargetAction::Destroy,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(max),
            } if *max > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: *max,
                        action: PermanentTargetAction::Destroy,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                builder.instructions.push(Instruction::DestroyTarget);
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                let obj = lower_object_filter(f, vocabulary)?;
                builder
                    .instructions
                    .push(Instruction::DestroyGroup(Box::new(obj)));
                Ok(())
            }
            _ => Err("oracle:unsupported-destroy"),
        },
        InstructionAst::ModifyPreviousDestroyNoRegen => {
            if let Some(Instruction::DestroyGroup(filter)) = builder.instructions.last() {
                let cloned = filter.clone();
                if let Some(last) = builder.instructions.last_mut() {
                    *last = Instruction::DestroyGroupWithoutRegeneration(cloned);
                    return Ok(());
                }
            }
            Err("oracle:unsupported-no-regen-modifier")
        }
        InstructionAst::Exile {
            actor: None | Some(PlayerRefAst::You),
            subject,
        } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                if !in_trigger && !self_pronoun_valid {
                    builder.instructions.push(Instruction::ExileResolvingSpell);
                } else {
                    builder.instructions.push(Instruction::ExileAbilitySource);
                }
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } => {
                let bt = lower_target_subject(subject, vocabulary)?;
                if filter.zone.is_some() {
                    if !matches!(
                        bt,
                        BodyTarget::AnyGraveyardCard
                            | BodyTarget::AnyGraveyardCreature
                            | BodyTarget::FilteredOwnGraveyardCard(_)
                            | BodyTarget::FilteredAnyGraveyardCard { .. }
                    ) {
                        return Err("oracle:unsupported-exile-gy-target");
                    }
                    builder.bind_target(bt)?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                } else {
                    builder.bind_target(bt)?;
                    builder.instructions.push(Instruction::ExileTarget);
                }
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(n),
            } if filter.zone.is_none() && *n >= 2 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: *n,
                        maximum: *n,
                        action: PermanentTargetAction::Exile,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(max),
            } if filter.zone.is_none() && *max > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: *max,
                        action: PermanentTargetAction::Exile,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                builder.instructions.push(Instruction::ExileTarget);
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                let obj = lower_object_filter(f, vocabulary)?;
                builder.instructions.push(Instruction::MoveGroup {
                    group: Box::new(obj),
                    destination: PublicZoneDestination::Exile,
                });
                Ok(())
            }
            _ => Err("oracle:unsupported-exile"),
        },
        InstructionAst::ExileUntilLeaves { subject } => {
            let bt = lower_target_subject(subject, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::ExileTargetUntilSourceLeaves);
            Ok(())
        }
        InstructionAst::ExileGraveyard { whose } => {
            let w = whose.trim().to_ascii_lowercase();
            match w.as_str() {
                "all" => {
                    builder.instructions.push(Instruction::ExileAllGraveyards);
                    Ok(())
                }
                "your" => {
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                            ResolvingPlayer::Controller,
                        )));
                    Ok(())
                }
                "each opponent" | "each opponent's" => {
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(
                            AffectedPlayers::EachOpponent,
                        ));
                    Ok(())
                }
                "target player" | "target player's" => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "target opponent" | "target opponent's" => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(Instruction::ExilePlayerGraveyards(AffectedPlayers::One(
                            ResolvingPlayer::TargetPlayer,
                        )));
                    Ok(())
                }
                "target card from a" => {
                    builder.bind_target(BodyTarget::AnyGraveyardCard)?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target creature card from a" => {
                    builder.bind_target(BodyTarget::AnyGraveyardCreature)?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target creature card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::creature(),
                    ))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target card from your" => {
                    builder
                        .bind_target(BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::any()))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target noncreature, nonland card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::from_clause(
                            ObjectClause::any()
                                .forbid_types(CardTypeMask::CREATURE.union(CardTypeMask::LAND)),
                        ),
                    ))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target instant or sorcery card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::instant_or_sorcery(),
                    ))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target artifact card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::artifact(),
                    ))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "target enchantment card from your" => {
                    builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(
                        ObjectFilter::enchantment(),
                    ))?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
                "~ from your" => {
                    builder.instructions.push(Instruction::ExileAbilitySource);
                    Ok(())
                }
                "that card from your" => {
                    builder
                        .instructions
                        .push(Instruction::ExileCapturedGraveyardCard);
                    Ok(())
                }
                other => {
                    let (prefix, zone) = if let Some(p) = other.strip_suffix(" from your") {
                        (p.trim(), ZoneFilter::YourGraveyard)
                    } else if let Some(p) = other
                        .strip_suffix(" from an opponent's")
                        .or_else(|| other.strip_suffix(" from an opponent"))
                    {
                        (p.trim(), ZoneFilter::OpponentGraveyard)
                    } else if let Some(p) = other
                        .strip_suffix(" from a")
                        .or_else(|| other.strip_suffix(" from any"))
                    {
                        (p.trim(), ZoneFilter::AnyGraveyard)
                    } else {
                        return Err("oracle:unsupported-exile-gy");
                    };
                    let Some(rest) = prefix.strip_prefix("target ") else {
                        return Err("oracle:unsupported-exile-gy");
                    };
                    let type_phrase = rest.trim_end_matches(" card").trim();
                    let obj = if type_phrase.is_empty() || type_phrase == "card" {
                        ObjectFilter::any()
                    } else if let Some(f) = parse_look_top_pick_filter(type_phrase, vocabulary) {
                        f
                    } else {
                        return Err("oracle:unsupported-exile-gy");
                    };
                    let bt = match zone {
                        ZoneFilter::YourGraveyard => BodyTarget::FilteredOwnGraveyardCard(obj),
                        ZoneFilter::OpponentGraveyard => BodyTarget::FilteredAnyGraveyardCard {
                            filter: obj,
                            opponents_only: true,
                        },
                        ZoneFilter::AnyGraveyard => BodyTarget::FilteredAnyGraveyardCard {
                            filter: obj,
                            opponents_only: false,
                        },
                        _ => return Err("oracle:unsupported-exile-gy"),
                    };
                    builder.bind_target(bt)?;
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                    Ok(())
                }
            }
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone,
            to,
            tapped,
            transformed: false,
            with_counter: None,
            attached_to: None,
        } => {
            let dest = to.trim().to_ascii_lowercase();
            let to_hand = matches!(
                dest.as_str(),
                "hand"
                    | "owner's hand"
                    | "its owner's hand"
                    | "their owners' hands"
                    | "your hand"
                    | "their hand"
            );
            let to_bf = matches!(
                dest.as_str(),
                "battlefield"
                    | "the battlefield"
                    | "the battlefield tapped"
                    | "the battlefield under your control"
                    | "the battlefield tapped under your control"
            );
            let enters_tapped = *tapped || dest.contains("tapped");
            if to_hand {
                match subject.as_ref() {
                    SubjectRefAst::SelfObject if !enters_tapped => {
                        if from_zone.as_deref() == Some("graveyard") {
                            builder
                                .instructions
                                .push(Instruction::ReturnSourceGraveyardToHand);
                        } else {
                            builder.instructions.push(Instruction::ReturnSourceToHand);
                        }
                        Ok(())
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if !enters_tapped => {
                        let effective_zone = filter.zone.or_else(|| {
                            from_zone.as_deref().and_then(|fz| {
                                let fl = fz.to_ascii_lowercase();
                                if fl.contains("your graveyard") {
                                    Some(ZoneFilter::YourGraveyard)
                                } else if fl.contains("opponent's graveyard") {
                                    Some(ZoneFilter::OpponentGraveyard)
                                } else if fl.contains("graveyard") {
                                    Some(ZoneFilter::AnyGraveyard)
                                } else {
                                    None
                                }
                            })
                        });
                        match effective_zone {
                            Some(ZoneFilter::YourGraveyard) => {
                                let mut inner = filter.as_ref().clone();
                                inner.zone = None;
                                let obj = if is_unrestricted_filter(&inner) {
                                    ObjectFilter::any()
                                } else {
                                    lower_object_filter(&inner, vocabulary)?
                                };
                                builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(obj))?;
                                builder
                                    .instructions
                                    .push(Instruction::ReturnTargetGraveyardCreatureToHand);
                            }
                            Some(ZoneFilter::AnyGraveyard | ZoneFilter::OpponentGraveyard) => {
                                let mut inner = filter.as_ref().clone();
                                inner.zone = None;
                                let obj = if is_unrestricted_filter(&inner) {
                                    ObjectFilter::any()
                                } else {
                                    lower_object_filter(&inner, vocabulary)?
                                };
                                let opponents_only =
                                    effective_zone == Some(ZoneFilter::OpponentGraveyard);
                                builder.bind_target(BodyTarget::FilteredAnyGraveyardCard {
                                    filter: obj,
                                    opponents_only,
                                })?;
                                builder.instructions.push(Instruction::ReturnToOwnersHand);
                            }
                            None => {
                                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                                builder.bind_target(bt)?;
                                builder.instructions.push(Instruction::ReturnToOwnersHand);
                            }
                            _ => return Err("oracle:unsupported-return-hand-zone"),
                        }
                        Ok(())
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(n),
                    } if filter.zone.is_none() && !enters_tapped && *n >= 2 => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: *n,
                                maximum: *n,
                                action: PermanentTargetAction::ReturnToHand,
                            });
                        Ok(())
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::UpTo(max),
                    } if filter.zone.is_none() && !enters_tapped && *max > 0 => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::AffectPermanentTargets {
                                minimum: 0,
                                maximum: *max,
                                action: PermanentTargetAction::ReturnToHand,
                            });
                        Ok(())
                    }
                    SubjectRefAst::Anaphora(_)
                        if !enters_tapped && builder.target != BodyTarget::None =>
                    {
                        builder.instructions.push(Instruction::ReturnToOwnersHand);
                        Ok(())
                    }
                    SubjectRefAst::Group(f)
                        if f.zone.is_none()
                            && !enters_tapped
                            && f.controller == Some(ControllerRelation::You)
                            && f.flags.0 == 0 =>
                    {
                        let ptf = lower_permanent_type_filter(f)?;
                        builder
                            .instructions
                            .push(Instruction::ReturnControlledPermanents {
                                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                                amount: ValueExpr::literal(1),
                                filter: ptf,
                            });
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if f.zone.is_none() && !enters_tapped =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        builder.instructions.push(Instruction::MoveGroup {
                            group: Box::new(obj),
                            destination: PublicZoneDestination::Hand,
                        });
                        Ok(())
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if f.zone == Some(ZoneFilter::YourGraveyard)
                            && !enters_tapped
                            && f.forbidden_types.is_empty()
                            && f.required_supertypes.is_empty()
                            && f.forbidden_supertypes.is_empty()
                            && f.subtypes.is_empty()
                            && f.forbidden_subtypes.is_empty()
                            && f.any_colors.is_empty()
                            && f.forbidden_colors.is_empty()
                            && f.color_arity.is_none()
                            && f.required_keywords.is_empty()
                            && f.forbidden_keywords.is_empty()
                            && f.stat_bounds.is_empty()
                            && f.counter_kind.is_none()
                            && f.flags.0 == 0 =>
                    {
                        let ct = if f.any_types == AstCardTypeMask::CREATURE {
                            CardType::Creature
                        } else if f.any_types == AstCardTypeMask::ARTIFACT {
                            CardType::Artifact
                        } else if f.any_types == AstCardTypeMask::ENCHANTMENT {
                            CardType::Enchantment
                        } else if f.any_types == AstCardTypeMask::INSTANT {
                            CardType::Instant
                        } else if f.any_types == AstCardTypeMask::SORCERY {
                            CardType::Sorcery
                        } else if f.any_types == AstCardTypeMask::LAND {
                            CardType::Land
                        } else if f.any_types == AstCardTypeMask::PLANESWALKER {
                            CardType::Planeswalker
                        } else {
                            return Err("oracle:unsupported-return-hand");
                        };
                        builder
                            .instructions
                            .push(Instruction::ReturnOwnGraveyardCardsOfTypeToHand(ct));
                        Ok(())
                    }
                    _ => Err("oracle:unsupported-return-hand"),
                }
            } else if to_bf {
                match subject.as_ref() {
                    SubjectRefAst::SelfObject if from_zone.as_deref() == Some("graveyard") => {
                        builder.instructions.push(
                            Instruction::ReturnSourceGraveyardToBattlefield {
                                tapped: enters_tapped,
                            },
                        );
                        Ok(())
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let effective_zone = filter.zone.or_else(|| {
                            from_zone.as_deref().and_then(|fz| {
                                let fl = fz.to_ascii_lowercase();
                                if fl.contains("your graveyard") {
                                    Some(ZoneFilter::YourGraveyard)
                                } else if fl.contains("opponent's graveyard") {
                                    Some(ZoneFilter::OpponentGraveyard)
                                } else if fl.contains("graveyard") {
                                    Some(ZoneFilter::AnyGraveyard)
                                } else {
                                    None
                                }
                            })
                        });
                        match effective_zone {
                            Some(ZoneFilter::YourGraveyard) => {
                                let mut patched = filter.as_ref().clone();
                                patched.zone = Some(ZoneFilter::YourGraveyard);
                                let bt = lower_target_subject(
                                    &SubjectRefAst::TargetObject {
                                        filter: Box::new(patched),
                                        count: TargetCountAst::Exact(1),
                                    },
                                    vocabulary,
                                )?;
                                builder.bind_target(bt)?;
                                if !enters_tapped
                                    && filter.any_types == AstCardTypeMask::CREATURE
                                    && filter.forbidden_types.is_empty()
                                    && filter.subtypes.is_empty()
                                    && filter.stat_bounds.is_empty()
                                    && filter.any_colors.is_empty()
                                    && filter.forbidden_colors.is_empty()
                                {
                                    builder
                                        .instructions
                                        .push(Instruction::ReturnOwnGraveyardCreatureToBattlefield);
                                } else {
                                    builder.instructions.push(
                                        Instruction::ReturnOwnGraveyardPermanentToBattlefield {
                                            tapped: enters_tapped,
                                        },
                                    );
                                }
                                Ok(())
                            }
                            Some(ZoneFilter::AnyGraveyard | ZoneFilter::OpponentGraveyard) => {
                                let mut patched = filter.as_ref().clone();
                                patched.zone = effective_zone;
                                let bt = lower_target_subject(
                                    &SubjectRefAst::TargetObject {
                                        filter: Box::new(patched),
                                        count: TargetCountAst::Exact(1),
                                    },
                                    vocabulary,
                                )?;
                                builder.bind_target(bt)?;
                                if !enters_tapped
                                    && bt == BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                                {
                                    builder.instructions.push(
                                        Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker,
                                    );
                                } else {
                                    builder.instructions.push(
                                        Instruction::ReturnOwnGraveyardPermanentToBattlefield {
                                            tapped: enters_tapped,
                                        },
                                    );
                                }
                                Ok(())
                            }
                            _ => Err("oracle:unsupported-reanimate-zone"),
                        }
                    }
                    _ => Err("oracle:unsupported-return-bf"),
                }
            } else {
                Err("oracle:unsupported-return-dest")
            }
        }
        InstructionAst::MoveZone { subject, to } => {
            let dest = to.trim().to_ascii_lowercase();
            let placement = if dest.contains("top of") && dest.contains("library") {
                Some(LibraryPlacement::Top)
            } else if dest.contains("bottom of") && dest.contains("library") {
                Some(LibraryPlacement::Bottom)
            } else {
                None
            };
            if let Some(pl) = placement {
                match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1),
                        ..
                    } => {
                        let bt = lower_target_subject(subject, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::PutTargetInLibrary(pl));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if pl == LibraryPlacement::Bottom && f.zone.is_none() =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        builder
                            .instructions
                            .push(Instruction::PutGroupOnLibraryBottom(Box::new(obj)));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let pub_dest = if matches!(
                dest.as_str(),
                "hand" | "owner's hand" | "its owner's hand" | "their owners' hands" | "your hand"
            ) {
                Some(PublicZoneDestination::Hand)
            } else if matches!(dest.as_str(), "exile" | "into exile") {
                Some(PublicZoneDestination::Exile)
            } else if matches!(
                dest.as_str(),
                "graveyard"
                    | "owner's graveyard"
                    | "its owner's graveyard"
                    | "their owners' graveyards"
            ) {
                Some(PublicZoneDestination::Graveyard)
            } else {
                None
            };
            let lib_placement = if dest.contains("top of") && dest.contains("library") {
                Some(LibraryPlacement::Top)
            } else if dest.contains("bottom of") && dest.contains("library") {
                Some(LibraryPlacement::Bottom)
            } else {
                None
            };
            if let Some(placement) = lib_placement {
                match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::PutTargetInLibrary(placement));
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                        builder
                            .instructions
                            .push(Instruction::PutTargetInLibrary(placement));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if f.zone.is_none() && placement == LibraryPlacement::Bottom =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        builder
                            .instructions
                            .push(Instruction::PutGroupOnLibraryBottom(Box::new(obj)));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if let Some(pd) = pub_dest {
                match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid)
                        && pd == PublicZoneDestination::Hand =>
                    {
                        builder.instructions.push(Instruction::ReturnSourceToHand);
                        return Ok(());
                    }
                    s if is_self_pronoun(s, builder, self_pronoun_valid)
                        && pd == PublicZoneDestination::Exile =>
                    {
                        if !in_trigger && !self_pronoun_valid {
                            builder.instructions.push(Instruction::ExileResolvingSpell);
                        } else {
                            builder.instructions.push(Instruction::ExileAbilitySource);
                        }
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() && pd == PublicZoneDestination::Hand => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::ReturnToOwnersHand);
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() && pd == PublicZoneDestination::Exile => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::ExileTarget);
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) if f.zone.is_none() => {
                        let obj = lower_object_filter(f, vocabulary)?;
                        builder.instructions.push(Instruction::MoveGroup {
                            group: Box::new(obj),
                            destination: pd,
                        });
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if dest.contains("battlefield")
                && let SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } = subject.as_ref()
                && matches!(
                    filter.zone,
                    Some(
                        ZoneFilter::YourGraveyard
                            | ZoneFilter::AnyGraveyard
                            | ZoneFilter::OpponentGraveyard
                    )
                )
            {
                let enters_tapped = dest.contains("tapped");
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                if !enters_tapped && bt == BodyTarget::AnyGraveyardCreatureOrPlaneswalker {
                    builder
                        .instructions
                        .push(Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker);
                } else {
                    builder.instructions.push(
                        Instruction::ReturnOwnGraveyardPermanentToBattlefield {
                            tapped: enters_tapped,
                        },
                    );
                }
                return Ok(());
            }
            Err("oracle:unsupported-move-zone")
        }
        InstructionAst::Sacrifice { player, subject } => {
            let players = lower_affected_players(*player, builder)?;
            match subject.as_ref() {
                s if matches!(player, PlayerRefAst::You)
                    && is_self_pronoun(s, builder, self_pronoun_valid) =>
                {
                    builder
                        .instructions
                        .push(Instruction::SacrificeStateTriggerSource);
                    Ok(())
                }
                SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f) => {
                    if matches!(player, PlayerRefAst::You)
                        && let Some(sel) = lower_sacrifice_selection(f, vocabulary)
                    {
                        builder
                            .instructions
                            .push(Instruction::SacrificeMatching(sel));
                        return Ok(());
                    }
                    let ptf = lower_permanent_type_filter(f)?;
                    builder.instructions.push(Instruction::Sacrifice {
                        players,
                        amount: ValueExpr::literal(1),
                        filter: ptf,
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-sacrifice"),
            }
        }
        InstructionAst::SacrificeUnlessPay {
            subject,
            cost: CostAtomAst::PayMana(raw),
        } if is_self_pronoun(subject, builder, self_pronoun_valid) => {
            let cost = parse_fixed_mana_cost(raw).ok_or("oracle:sac-unless-x")?;
            let idx = builder.instructions.len();
            builder.instructions.push(Instruction::OfferManaPayment {
                payer: ResolvingPlayer::Controller,
                cost,
                paid_destination: idx + 2,
                declined_destination: idx + 1,
            });
            builder
                .instructions
                .push(Instruction::SacrificeStateTriggerSource);
            Ok(())
        }
        InstructionAst::Discard {
            player,
            count,
            filter: Some(f),
            anaphora: None,
            random: false,
        } => {
            let amt = match count {
                Some(c) => lower_value_expr(resolve_x(c, bound_x), vocabulary)?,
                None => ValueExpr::literal(1),
            };
            let players = lower_affected_players(*player, builder)?;
            let obj_filter = lower_object_filter(f, vocabulary)?;
            builder.instructions.push(Instruction::DiscardFiltered {
                players,
                amount: amt,
                filter: Box::new(obj_filter),
            });
            Ok(())
        }
        InstructionAst::Discard {
            player,
            count,
            filter: None,
            anaphora: None,
            random,
        } => {
            let amt = match count {
                Some(c) => lower_value_expr(resolve_x(c, bound_x), vocabulary)?,
                None => ValueExpr::literal(1),
            };
            if *random {
                if amt == ValueExpr::literal(1) {
                    let rp = lower_resolving_player(*player, builder)?;
                    builder
                        .instructions
                        .push(Instruction::DiscardOneAtRandom(rp));
                    return Ok(());
                }
                return Err("oracle:unsupported-random-discard");
            }
            let players = lower_affected_players(*player, builder)?;
            builder.instructions.push(Instruction::Discard {
                players,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::DiscardHand { player } => {
            let rp = lower_resolving_player(*player, builder)?;
            builder.instructions.push(Instruction::Discard {
                players: AffectedPlayers::One(rp),
                amount: ValueExpr::count_hand(rp),
            });
            Ok(())
        }
        InstructionAst::Mill { player, count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            if matches!(player, PlayerRefAst::EachPlayer) {
                let ValueExpr::Literal(n) = amt else {
                    return Err("oracle:dynamic-mill-each");
                };
                let n_u16 = u16::try_from(n.to_usize().ok_or("oracle:mill-ovf")?)
                    .map_err(|_| "oracle:mill-ovf")?;
                builder
                    .instructions
                    .push(Instruction::MillEachPlayer(n_u16));
                return Ok(());
            }
            let rp = lower_resolving_player(*player, builder)?;
            builder.instructions.push(Instruction::Mill {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Scry { player, count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let rp = lower_resolving_player(*player, builder)?;
            builder.instructions.push(Instruction::Scry {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::Surveil { player, count } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let rp = lower_resolving_player(*player, builder)?;
            builder.instructions.push(Instruction::Surveil {
                player: rp,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::SearchLibrary {
            library,
            count,
            filter,
            reveal,
            to_zone,
            shuffle: true,
        } => {
            let search =
                lower_library_search(library, count, filter, *reveal, to_zone, vocabulary, false)?;
            builder.instructions.push(Instruction::SearchLibrary {
                filter: Box::new(search),
            });
            Ok(())
        }
        InstructionAst::ShuffleLibrary {
            player: PlayerRefAst::You,
        } => {
            builder.instructions.push(Instruction::ShuffleOwnLibrary);
            Ok(())
        }
        InstructionAst::ShuffleLibrary { player } => {
            let rp = lower_resolving_player(*player, builder)?;
            builder
                .instructions
                .push(Instruction::ShufflePlayerLibrary(rp));
            Ok(())
        }
        InstructionAst::RevealHand { player } => {
            let rp = lower_resolving_player(*player, builder)?;
            builder.instructions.push(Instruction::RevealPlayerHand(rp));
            Ok(())
        }
        InstructionAst::ShuffleGraveyardIntoLibrary {
            player: PlayerRefAst::You,
        } => {
            builder
                .instructions
                .push(Instruction::ShuffleOwnGraveyardIntoLibrary);
            Ok(())
        }
        InstructionAst::TapPermanent { subject } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder.instructions.push(Instruction::TapAbilitySource);
                Ok(())
            }
            SubjectRefAst::AttachedHost(_) => {
                builder.instructions.push(Instruction::TapEnchantedCreature);
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(n),
            } if filter.zone.is_none() && *n > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: *n,
                        maximum: *n,
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(n),
            } if filter.zone.is_none() && *n > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: *n,
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 1,
                        maximum: 1,
                        action: PermanentTargetAction::Tap,
                    });
                Ok(())
            }
            _ => Err("oracle:unsupported-tap-perm"),
        },
        InstructionAst::UntapPermanent { subject } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder.instructions.push(Instruction::UntapAbilitySource);
                Ok(())
            }
            SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 1,
                        maximum: 1,
                        action: PermanentTargetAction::Untap,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(n),
            } if filter.zone.is_none() && *n > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: *n,
                        maximum: *n,
                        action: PermanentTargetAction::Untap,
                    });
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::UpTo(n),
            } if filter.zone.is_none() && *n > 0 => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: *n,
                        action: PermanentTargetAction::Untap,
                    });
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                if f.controller == Some(ControllerRelation::You)
                    && f.any_types == AstCardTypeMask::LAND
                    && f.subtypes.is_empty() =>
            {
                builder.instructions.push(Instruction::UntapControlledLands);
                Ok(())
            }
            SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                if f.controller == Some(ControllerRelation::You)
                    && f.any_types == AstCardTypeMask::CREATURE
                    && f.subtypes.is_empty() =>
            {
                builder
                    .instructions
                    .push(Instruction::UntapControlledCreatures);
                Ok(())
            }
            _ => Err("oracle:unsupported-untap-perm"),
        },
        InstructionAst::TapOrUntapPermanent { subject } => match subject.as_ref() {
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 1,
                        maximum: 1,
                        action: PermanentTargetAction::ToggleTap,
                    });
                Ok(())
            }
            _ => Err("oracle:unsupported-toggle-tap"),
        },
        InstructionAst::SkipNextUntap { subject } => {
            if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                && let Some(Instruction::AffectPermanentTargets {
                    action: action @ PermanentTargetAction::Tap,
                    ..
                }) = builder.instructions.last_mut()
            {
                *action = PermanentTargetAction::TapAndSkipNextUntap;
                return Ok(());
            }
            Err("oracle:unsupported-skip-untap")
        }
        InstructionAst::Regenerate { subject } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder.instructions.push(Instruction::RegenerateSource);
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::RegenerateTarget);
                Ok(())
            }
            _ => Err("oracle:unsupported-regenerate"),
        },
        InstructionAst::Transform { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            builder.instructions.push(Instruction::TransformSource);
            Ok(())
        }
        InstructionAst::PhaseOut { subject } => match subject.as_ref() {
            s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                builder
                    .instructions
                    .push(Instruction::PhaseOut(PhasingSubject::Source));
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::PhaseOut(PhasingSubject::Target));
                Ok(())
            }
            _ => Err("oracle:unsupported-phase-out"),
        },
        InstructionAst::Fight { a, b } => {
            if is_self_pronoun(a, builder, self_pronoun_valid) {
                let bt = lower_target_subject(b, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::FightSourceWithTarget);
                return Ok(());
            }
            Err("oracle:unsupported-fight")
        }
        InstructionAst::Counter {
            target,
            unless_pay,
            unless_action: None,
        } => {
            let bt = lower_target_subject(target, vocabulary)?;
            builder.bind_target(bt)?;
            if let Some(cost_atom) = unless_pay {
                let CostAtomAst::PayMana(raw) = cost_atom else {
                    return Err("oracle:counter-unless-non-mana");
                };
                let cost = parse_fixed_mana_cost(raw).ok_or("oracle:counter-unless-x")?;
                let idx = builder.instructions.len();
                builder.instructions.push(Instruction::OfferManaPayment {
                    payer: ResolvingPlayer::TargetSpellController,
                    cost,
                    paid_destination: idx + 2,
                    declined_destination: idx + 1,
                });
                builder.instructions.push(Instruction::CounterSpell);
                Ok(())
            } else {
                builder.instructions.push(Instruction::CounterSpell);
                Ok(())
            }
        }
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale: Some(scale_expr),
        } => {
            let dur = lower_duration(duration)?;
            if dur != ResolvedDuration::UntilEndOfTurn {
                return Err("oracle:unsupported-scaled-pt-duration");
            }
            let dp = parse_signed_pt_delta(dx).ok_or("oracle:non-numeric-pt-delta")?;
            let dt = parse_signed_pt_delta(dy).ok_or("oracle:non-numeric-pt-delta")?;
            let amt = lower_value_expr(resolve_x(scale_expr, bound_x), vocabulary)?;
            let group = match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    EffectSubject::AbilitySource
                }
                SubjectRefAst::AttachedHost(_) => EffectSubject::Matching(ObjectFilter::Clause(
                    ObjectClause::any().attached_to_self(),
                )),
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
                    EffectSubject::Matching(lower_object_filter(f, vocabulary)?)
                }
                _ => return Err("oracle:unsupported-scaled-pt-subject"),
            };
            builder.instructions.push(Instruction::ModifyGroupByAmount {
                group: Box::new(group),
                scale: PowerToughness {
                    power: dp.into(),
                    toughness: dt.into(),
                },
                amount: amt,
                keywords: BTreeSet::new(),
            });
            Ok(())
        }
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale: None,
        } => {
            let dur = lower_duration(duration)?;
            let dp = parse_signed_pt_delta(dx);
            let dt = parse_signed_pt_delta(dy);
            match (dp, dt) {
                (Some(p), Some(t)) => {
                    let pt = PowerToughness {
                        power: p.into(),
                        toughness: t.into(),
                    };
                    lower_modify_pt_target(
                        subject,
                        pt,
                        dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    )
                }
                _ if dx.eq_ignore_ascii_case("-x")
                    && dy.eq_ignore_ascii_case("-x")
                    && dur == ResolvedDuration::UntilEndOfTurn =>
                {
                    if let SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } = subject.as_ref()
                        && filter.zone.is_none()
                    {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::WeakenTargetByChosenX);
                        return Ok(());
                    }
                    Err("oracle:unsupported-dynamic-modify-pt")
                }
                _ if (dx.eq_ignore_ascii_case("+x") || dx.eq_ignore_ascii_case("x"))
                    && (dy.eq_ignore_ascii_case("+x")
                        || dy.eq_ignore_ascii_case("x")
                        || dy == "+0"
                        || dy == "0")
                    && dur == ResolvedDuration::UntilEndOfTurn =>
                {
                    let also_toughness = dy != "+0" && dy != "0";
                    let amount = if let Some(bx) = bound_x {
                        lower_value_expr(bx, vocabulary)?
                    } else {
                        ValueExpr::chosen_x()
                    };
                    if let SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } = subject.as_ref()
                        && filter.zone.is_none()
                    {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder.instructions.push(Instruction::BoostTargetPower {
                            amount,
                            also_toughness,
                            keywords: BTreeSet::new(),
                        });
                        return Ok(());
                    }
                    Err("oracle:unsupported-dynamic-modify-pt")
                }
                _ => Err("oracle:unsupported-dynamic-modify-pt"),
            }
        }
        InstructionAst::GrantKeywords {
            subject,
            keywords,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            if dur == ResolvedDuration::UntilEndOfTurn
                && keywords.len() == 1
                && keywords[0]
                    .trim()
                    .eq_ignore_ascii_case("protection from the color of your choice")
            {
                match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::GrantProtectionChosenColor(
                                ProtectionRecipients::TargetPermanent,
                            ));
                        return Ok(());
                    }
                    SubjectRefAst::Compound(parts)
                        if parts.len() == 2
                            && matches!(&parts[0], SubjectRefAst::Player(PlayerRefAst::You))
                            && matches!(
                                &parts[1],
                                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                                    if f.zone.is_none()
                                        && f.controller == Some(ControllerRelation::You)
                                        && (f.any_permanent || f.any_types.is_empty())
                                        && f.forbidden_types.is_empty()
                                        && f.subtypes.is_empty()
                                        && f.any_colors.is_empty()
                                        && f.forbidden_colors.is_empty()
                                        && f.stat_bounds.is_empty()
                            ) =>
                    {
                        builder
                            .instructions
                            .push(Instruction::GrantProtectionChosenColor(
                                ProtectionRecipients::ControllerAndPermanents,
                            ));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let ab_change = lower_ability_change(keywords, vocabulary)?;
            let pure_kws = (ab_change.removal.is_none()
                && ab_change.suppressed_keywords.is_empty()
                && ab_change.granted_exalted == 0
                && !ab_change.grants_unblockable
                && ab_change.granted_trigger.is_none()
                && ab_change.granted_toxic.is_none()
                && ab_change.granted_combat_keywords.is_empty()
                && !ab_change.granted_keywords.is_empty())
            .then(|| ab_change.granted_keywords.clone());
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid)
                    && dur == ResolvedDuration::UntilEndOfTurn =>
                {
                    if let Some(kws) = pure_kws {
                        if let Some(Instruction::BoostSource {
                            keywords: prev_kws, ..
                        }) = builder.instructions.last_mut()
                        {
                            prev_kws.extend(kws);
                            return Ok(());
                        }
                        if let Some(Instruction::ModifyGroupUntilEndOfTurn {
                            group,
                            keywords: prev_kws,
                            ..
                        }) = builder.instructions.last_mut()
                            && matches!(group.as_ref(), EffectSubject::AbilitySource)
                        {
                            prev_kws.extend(kws);
                            return Ok(());
                        }
                        if in_trigger {
                            builder
                                .instructions
                                .push(Instruction::ModifyGroupUntilEndOfTurn {
                                    group: Box::new(EffectSubject::AbilitySource),
                                    power_toughness: None,
                                    keywords: kws,
                                });
                        } else {
                            builder.instructions.push(Instruction::BoostSource {
                                power_toughness: None,
                                keywords: kws,
                            });
                        }
                        return Ok(());
                    }
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                        duration: dur,
                    });
                    Ok(())
                }
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                        duration: dur,
                    });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    if let Some(Instruction::CreateContinuousEffect { parts, duration: d }) =
                        builder.instructions.last_mut()
                        && *d == dur
                        && !parts
                            .0
                            .iter()
                            .any(|p| matches!(p, ResolvedPartSpec::Abilities(_)))
                    {
                        parts.0.push(ResolvedPartSpec::Abilities(ab_change));
                        return Ok(());
                    }
                    if let Some(kws) = pure_kws
                        && let Some(Instruction::BoostTargetPower {
                            keywords: prev_kws, ..
                        }) = builder.instructions.last_mut()
                        && dur == ResolvedDuration::UntilEndOfTurn
                    {
                        prev_kws.extend(kws);
                        return Ok(());
                    }
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(n),
                } if filter.zone.is_none() && *n >= 2 => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: *n,
                            maximum: *n,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::UpTo(max),
                } if filter.zone.is_none() && *max > 0 => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: *max,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
                        });
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_)
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws =>
                {
                    builder
                        .instructions
                        .push(Instruction::ModifyGroupUntilEndOfTurn {
                            group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            power_toughness: None,
                            keywords: kws,
                        });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    builder
                        .instructions
                        .push(Instruction::ModifyGroupUntilEndOfTurn {
                            group: Box::new(EffectSubject::Matching(obj)),
                            power_toughness: None,
                            keywords: kws,
                        });
                    Ok(())
                }
                _ => Err("oracle:unsupported-grant-kw"),
            }
        }
        InstructionAst::SwitchPowerToughness { subject, duration } => {
            let dur = lower_duration(duration)?;
            match subject.as_ref() {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::SelfObject => {
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                        duration: dur,
                    });
                    Ok(())
                }
                _ => Err("oracle:unsupported-switch-pt"),
            }
        }
        InstructionAst::RemoveAllAbilities { subject, duration } => {
            let dur = lower_duration(duration)?;
            if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
                && filter.zone.is_none()
            {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                            AbilityChange::remove(AbilityRemoval::All),
                        )]),
                        duration: dur,
                    });
                return Ok(());
            }
            Err("oracle:unsupported-remove-abilities")
        }
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:put-counter-kind")?;
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
                && filter.zone.is_none()
                && !matches!(
                    ck,
                    CounterKind::PlusOne
                        | CounterKind::MinusOne
                        | CounterKind::Poison
                        | CounterKind::Experience
                        | CounterKind::Energy
                )
            {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                if ck == CounterKind::Indestructible && amt == ValueExpr::literal(1) {
                    builder
                        .instructions
                        .push(Instruction::PutIndestructibleCounter);
                } else if ck == CounterKind::Loyalty && amt == ValueExpr::literal(1) {
                    builder.instructions.push(Instruction::PutLoyaltyCounter);
                } else {
                    builder
                        .instructions
                        .push(Instruction::PutNamedCounterOnTarget {
                            kind: ck,
                            amount: amt,
                        });
                }
                return Ok(());
            }
            if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                && builder.target != BodyTarget::None
                && !matches!(
                    ck,
                    CounterKind::PlusOne
                        | CounterKind::MinusOne
                        | CounterKind::Poison
                        | CounterKind::Experience
                        | CounterKind::Energy
                )
            {
                if ck == CounterKind::Indestructible && amt == ValueExpr::literal(1) {
                    builder
                        .instructions
                        .push(Instruction::PutIndestructibleCounter);
                } else if ck == CounterKind::Loyalty && amt == ValueExpr::literal(1) {
                    builder.instructions.push(Instruction::PutLoyaltyCounter);
                } else {
                    builder
                        .instructions
                        .push(Instruction::PutNamedCounterOnTarget {
                            kind: ck,
                            amount: amt,
                        });
                }
                return Ok(());
            }
            let ValueExpr::Literal(n) = amt else {
                return Err("oracle:dynamic-put-counters");
            };
            let n_usize = n.to_usize().ok_or("oracle:nonpos-counters")?;
            if n_usize == 0 {
                return Err("oracle:nonpos-counters");
            }
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    if ck == CounterKind::PlusOne {
                        builder
                            .instructions
                            .push(Instruction::PutCountersOnSource { amount: n });
                    } else if ck == CounterKind::Charge && n_usize == 1 {
                        builder
                            .instructions
                            .push(Instruction::PutChargeCounterOnSource);
                    } else {
                        let u = u16::try_from(n_usize).map_err(|_| "oracle:cnt-ovf")?;
                        builder
                            .instructions
                            .push(Instruction::PutNamedCounterOnSource {
                                kind: ck,
                                amount: u,
                            });
                    }
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    match ck {
                        CounterKind::PlusOne => {
                            builder
                                .instructions
                                .push(Instruction::PutPowerToughnessCounters {
                                    counters: PowerToughnessCounters {
                                        plus_one: n,
                                        minus_one: 0.into(),
                                    },
                                });
                            Ok(())
                        }
                        CounterKind::MinusOne => {
                            builder
                                .instructions
                                .push(Instruction::PutPowerToughnessCounters {
                                    counters: PowerToughnessCounters {
                                        plus_one: 0.into(),
                                        minus_one: n,
                                    },
                                });
                            Ok(())
                        }
                        CounterKind::Indestructible if n_usize == 1 => {
                            builder
                                .instructions
                                .push(Instruction::PutIndestructibleCounter);
                            Ok(())
                        }
                        CounterKind::Loyalty if n_usize == 1 => {
                            builder.instructions.push(Instruction::PutLoyaltyCounter);
                            Ok(())
                        }
                        _ => Err("oracle:unsupported-target-counter"),
                    }
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(n),
                } if filter.zone.is_none()
                    && ck == CounterKind::PlusOne
                    && n_usize == 1
                    && *n >= 2 =>
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: *n,
                            maximum: *n,
                            action: PermanentTargetAction::PutPlusOneCounter,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::UpTo(max),
                } if filter.zone.is_none()
                    && ck == CounterKind::PlusOne
                    && n_usize == 1
                    && *max > 0 =>
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: *max,
                            action: PermanentTargetAction::PutPlusOneCounter,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => match ck {
                    CounterKind::PlusOne => {
                        builder
                            .instructions
                            .push(Instruction::PutPowerToughnessCounters {
                                counters: PowerToughnessCounters {
                                    plus_one: n,
                                    minus_one: 0.into(),
                                },
                            });
                        Ok(())
                    }
                    CounterKind::MinusOne => {
                        builder
                            .instructions
                            .push(Instruction::PutPowerToughnessCounters {
                                counters: PowerToughnessCounters {
                                    plus_one: 0.into(),
                                    minus_one: n,
                                },
                            });
                        Ok(())
                    }
                    CounterKind::Indestructible if n_usize == 1 => {
                        builder
                            .instructions
                            .push(Instruction::PutIndestructibleCounter);
                        Ok(())
                    }
                    _ => Err("oracle:unsupported-target-counter"),
                },
                SubjectRefAst::TriggerSubject if ck == CounterKind::PlusOne && n_usize == 1 => {
                    builder
                        .instructions
                        .push(Instruction::PutCounterOnRelatedPermanent);
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if ck == CounterKind::PlusOne
                        && n_usize == 1
                        && f.controller == Some(ControllerRelation::You)
                        && f.forbidden_types.is_empty()
                        && f.subtypes.len() == 1 =>
                {
                    let ct = vocabulary
                        .resolve_label(&f.subtypes[0])
                        .ok_or("oracle:unsupported-put-counters")?;
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnControlledSubtype(ct));
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if ck == CounterKind::PlusOne
                        && n_usize == 1
                        && f.controller == Some(ControllerRelation::You)
                        && f.any_types == AstCardTypeMask::CREATURE
                        && f.forbidden_types.is_empty()
                        && f.subtypes.is_empty() =>
                {
                    if f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0 {
                        builder
                            .instructions
                            .push(Instruction::PutCountersOnOtherCreatures);
                    } else {
                        builder
                            .instructions
                            .push(Instruction::PutCountersOnControlledCreatures);
                    }
                    Ok(())
                }
                _ => Err("oracle:unsupported-put-counters"),
            }
        }
        InstructionAst::Proliferate => {
            builder.instructions.push(Instruction::Proliferate);
            Ok(())
        }
        InstructionAst::CreateToken { controller, token } => {
            let recipients = match controller {
                PlayerRefAst::You => TokenRecipients::Controller,
                PlayerRefAst::EachPlayer => TokenRecipients::EachPlayer,
                PlayerRefAst::EachOpponent => TokenRecipients::EachOpponent,
                _ => return Err("oracle:unsupported-token-controller"),
            };
            let (creation, attacking) =
                lower_token_spec(token, recipients, vocabulary, ids, bound_x)?;
            if attacking {
                if !creation.enters_tapped
                    || creation.recipients != TokenRecipients::Controller
                    || builder.target != BodyTarget::None
                {
                    return Err("oracle:unsupported-attacking-token");
                }
                builder
                    .instructions
                    .push(Instruction::CreateAttackingTokens {
                        creations: vec![creation],
                    });
            } else {
                builder.instructions.push(Instruction::CreateTokens {
                    creations: vec![creation],
                });
            }
            Ok(())
        }
        InstructionAst::AddMana {
            player: PlayerRefAst::You,
            mana,
        } => {
            let yield_val = match mana {
                ManaSpecAst::Fixed(raw) => {
                    EffectManaYield::Fixed(parse_mana_bundle(raw).ok_or("oracle:bad-mana-bundle")?)
                }
                ManaSpecAst::ScaledSymbol { symbol, amount } => {
                    let kind = parse_single_mana_type(symbol).ok_or("oracle:bad-mana-sym")?;
                    let amt = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
                    EffectManaYield::Counted { kind, amount: amt }
                }
                ManaSpecAst::CombinationOf { amount, symbols } if symbols.len() == 2 => {
                    let first = parse_single_mana_type(&symbols[0]).ok_or("oracle:bad-mana-sym")?;
                    let second =
                        parse_single_mana_type(&symbols[1]).ok_or("oracle:bad-mana-sym")?;
                    if first == second {
                        return Err("oracle:unsupported-effect-mana");
                    }
                    let amt = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
                    EffectManaYield::Split {
                        first,
                        second,
                        amount: amt,
                    }
                }
                ManaSpecAst::Choice(opts) if opts.len() >= 2 => {
                    let mut colors = BTreeSet::new();
                    for opt in opts {
                        let mt = parse_single_mana_type(opt).ok_or("oracle:bad-mana-sym")?;
                        colors.insert(mt);
                    }
                    if colors.is_empty() {
                        return Err("oracle:unsupported-effect-mana");
                    }
                    EffectManaYield::Choice {
                        colors,
                        amount: ValueExpr::literal(1),
                    }
                }
                ManaSpecAst::AnyColor {
                    amount,
                    single_color: true,
                } => {
                    let amt = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
                    let colors = [
                        ManaType::White,
                        ManaType::Blue,
                        ManaType::Black,
                        ManaType::Red,
                        ManaType::Green,
                    ]
                    .into_iter()
                    .collect();
                    EffectManaYield::Choice {
                        colors,
                        amount: amt,
                    }
                }
                _ => return Err("oracle:unsupported-effect-mana"),
            };
            builder.instructions.push(Instruction::AddMana(yield_val));
            Ok(())
        }
        InstructionAst::GainControl {
            controller: PlayerRefAst::You,
            subject,
            duration,
        } => {
            if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
                && filter.any_spell
            {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::GainControlOfSpell);
                return Ok(());
            }
            let dur = lower_duration(duration)?;
            let bt = lower_target_subject(subject, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                        spellwake_engine::characteristics::ControlRecipient::ResolvingController,
                    )]),
                    duration: dur,
                });
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: true,
            target: None,
            duration: DurationAst::UntilEndOfTurn,
        } => {
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::Combat,
                ));
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: false,
            target: None,
            duration: DurationAst::UntilEndOfTurn,
        } => {
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::All,
                ));
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: true,
            target: Some(subj),
            duration: DurationAst::UntilEndOfTurn,
        } if is_self_pronoun(subj, builder, self_pronoun_valid) => {
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::CombatToOrFromAbilitySource,
                ));
            Ok(())
        }
        InstructionAst::RemoveCounters {
            subject,
            counter_kind,
            all,
            count,
        } => {
            let ck_low = counter_kind.trim().to_ascii_lowercase();
            let kind = if ck_low.is_empty() || ck_low == "any" {
                None
            } else {
                Some(parse_counter_kind(&ck_low).ok_or("oracle:unsupported-counter-kind")?)
            };
            let target_self = is_self_pronoun(subject, builder, self_pronoun_valid);
            let amt = if *all {
                if !target_self {
                    return Err("oracle:unsupported-remove-counters");
                }
                ValueExpr::CounterCount {
                    subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
                    kind,
                }
            } else if let Some(c) = count {
                lower_value_expr(resolve_x(c, bound_x), vocabulary)?
            } else {
                ValueExpr::literal(1)
            };
            if target_self {
                builder
                    .instructions
                    .push(Instruction::RemoveCountersFromPermanent {
                        target_self: true,
                        kind,
                        amount: amt,
                    });
                Ok(())
            } else if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
                && filter.zone.is_none()
            {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::RemoveCountersFromPermanent {
                        target_self: false,
                        kind,
                        amount: amt,
                    });
                Ok(())
            } else {
                Err("oracle:unsupported-remove-counters")
            }
        }
        InstructionAst::PreventAllDamage {
            combat_only: false,
            target: Some(subj),
            duration: DurationAst::UntilEndOfTurn,
        } if is_self_pronoun(subj, builder, self_pronoun_valid) => {
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::ToAbilitySource,
                ));
            Ok(())
        }
        InstructionAst::PreventAllDamage {
            combat_only: false,
            target: Some(subj),
            duration: DurationAst::UntilEndOfTurn,
        } => {
            let bt = lower_target_subject(subj, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::PreventDamageThisTurn(
                    TurnPreventionFilter::ToTarget,
                ));
            Ok(())
        }
        InstructionAst::PreventDamage {
            amount,
            target,
            duration: DurationAst::UntilEndOfTurn,
        } => {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-prevent-damage");
            };
            let bt = lower_target_subject(target, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::CreatePreventionShield { amount: n });
            Ok(())
        }
        InstructionAst::BecomeMonarch {
            player: PlayerRefAst::You,
        } => {
            builder.instructions.push(Instruction::BecomeMonarch(None));
            Ok(())
        }
        InstructionAst::Bolster { count } => {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(count, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-bolster");
            };
            let u = u16::try_from(n.to_usize().ok_or("oracle:bolster-ovf")?)
                .map_err(|_| "oracle:bolster-ovf")?;
            builder.instructions.push(Instruction::Bolster(u));
            Ok(())
        }
        InstructionAst::Adapt { count } => {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(count, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-adapt");
            };
            let u = u16::try_from(n.to_usize().ok_or("oracle:adapt-ovf")?)
                .map_err(|_| "oracle:adapt-ovf")?;
            builder.instructions.push(Instruction::Adapt(u));
            Ok(())
        }
        InstructionAst::Amass { subtype, count } => {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(count, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-amass");
            };
            let u = u16::try_from(n.to_usize().ok_or("oracle:amass-ovf")?)
                .map_err(|_| "oracle:amass-ovf")?;
            let army = vocabulary
                .resolve_label("Army")
                .ok_or("oracle:missing-army")?;
            let st_label = if subtype.is_empty() {
                "Zombie"
            } else {
                subtype.as_str()
            };
            let sub = vocabulary
                .resolve_label(st_label)
                .ok_or("oracle:amass-subtype")?;
            builder
                .instructions
                .push(Instruction::CreateArmyIfAbsent { army, subtype: sub });
            builder.instructions.push(Instruction::Amass {
                army,
                subtype: sub,
                amount: u,
            });
            Ok(())
        }
        InstructionAst::Explore { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            builder.instructions.push(Instruction::ExploreSource);
            Ok(())
        }
        InstructionAst::Manifest {
            count,
            dread: false,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            builder.instructions.push(Instruction::ManifestTop {
                player: ResolvingPlayer::Controller,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::ExtraTurn {
            player: PlayerRefAst::You,
        } => {
            builder.instructions.push(Instruction::TakeExtraTurn);
            Ok(())
        }
        InstructionAst::AdditionalCombat(_) => {
            builder.instructions.push(Instruction::AdditionalCombat {
                untap: CombatUntapGroup::ControlledCreatures,
            });
            Ok(())
        }
        InstructionAst::CombatRuleMod {
            subject,
            rule,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let low = rule.to_ascii_lowercase();
            if low.contains("goad") {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::GoadTarget);
                return Ok(());
            }
            if low.contains("attacks this turn if able") || low == "must_attack" {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::RequireTargetAttack);
                return Ok(());
            }
            if (low.contains("must block this turn if able")
                || low.contains("blocks this turn if able"))
                && matches!(
                    subject.as_ref(),
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(
                            f.controller,
                            Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
                        ) && f.any_types == AstCardTypeMask::CREATURE
                )
            {
                builder
                    .instructions
                    .push(Instruction::RequireOpponentsCreaturesBlockThisTurn);
                return Ok(());
            }
            if low.contains("all creatures able to block")
                && low.contains("do so")
                && let Ok(bt) = lower_target_subject(subject, vocabulary)
            {
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::RequireBlocksAgainstTargetThisTurn);
                return Ok(());
            }
            if let Some(br) = lower_blocking_restriction(rule) {
                match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(Instruction::ChangeAbilitySource {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: dur,
                            });
                        return Ok(());
                    }
                    SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: dur,
                            });
                        return Ok(());
                    }
                    _ => {}
                }
            }
            Err("oracle:unsupported-combat-mod")
        }
        InstructionAst::Investigate {
            player: PlayerRefAst::You,
            count,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let def = predefined_token_def("clue", vocabulary, ids)?;
            builder.instructions.push(Instruction::CreateTokens {
                creations: vec![TokenCreation {
                    enters_tapped: false,
                    definition: def,
                    amount: amt,
                    base_size: None,
                    recipients: TokenRecipients::Controller,
                }],
            });
            Ok(())
        }
        InstructionAst::ChangeTargets { target } => {
            let bt = lower_target_subject(target, vocabulary)?;
            builder.bind_target(bt)?;
            builder.instructions.push(Instruction::ChooseNewTargets);
            Ok(())
        }
        InstructionAst::Copy {
            subject,
            exception: None,
            count: None,
        } => {
            if let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
                && filter.any_spell
            {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder.instructions.push(Instruction::CopySpell);
                return Ok(());
            }
            Err("oracle:unsupported-copy")
        }
        InstructionAst::RemoveFromCombat { subject } => {
            let bt = lower_target_subject(subject, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::RemoveTargetFromCombat);
            Ok(())
        }
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:player-counter-kind")?;
            if !ck.is_player_counter() {
                return Err("oracle:player-counter-kind");
            }
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            if matches!(player, PlayerRefAst::You)
                && ck == CounterKind::Experience
                && amt == ValueExpr::literal(1)
            {
                builder.instructions.push(Instruction::AddExperienceCounter);
                return Ok(());
            }
            let players = lower_affected_players(*player, builder)?;
            builder.instructions.push(Instruction::AddPlayerCounter {
                players,
                kind: ck,
                amount: amt,
            });
            Ok(())
        }
        InstructionAst::EndTurn => {
            builder.instructions.push(Instruction::EndControllerTurn);
            Ok(())
        }
        InstructionAst::PayCost(CostAtomAst::PayLife(expr)) => {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(expr, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-pay-life");
            };
            let u = u16::try_from(n.to_usize().ok_or("oracle:pay-life-ovf")?)
                .map_err(|_| "oracle:pay-life-ovf")?;
            if u == 0 {
                return Err("oracle:pay-life-zero");
            }
            builder.instructions.push(Instruction::PayControllerLife(u));
            Ok(())
        }
        InstructionAst::PayCost(CostAtomAst::PayEnergy(n)) if *n > 0 => {
            builder
                .instructions
                .push(Instruction::PayControllerEnergy(*n));
            Ok(())
        }
        InstructionAst::PayCost(CostAtomAst::SacrificeSelf) => {
            builder
                .instructions
                .push(Instruction::SacrificeStateTriggerSource);
            Ok(())
        }
        InstructionAst::PayCost(CostAtomAst::DiscardHand) => {
            builder.instructions.push(Instruction::Discard {
                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                amount: ValueExpr::count_hand(ResolvingPlayer::Controller),
            });
            Ok(())
        }
        InstructionAst::PayCost(CostAtomAst::Discard {
            count,
            filter,
            random,
        }) => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let plain_any = filter.is_empty() || filter.any_card;
            if *random {
                if plain_any && amt == ValueExpr::literal(1) {
                    builder
                        .instructions
                        .push(Instruction::DiscardOneAtRandom(ResolvingPlayer::Controller));
                    return Ok(());
                }
                return Err("oracle:unsupported-pay-cost");
            }
            if plain_any {
                builder.instructions.push(Instruction::Discard {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    amount: amt,
                });
            } else {
                let obj = lower_object_filter(filter, vocabulary)?;
                builder.instructions.push(Instruction::DiscardFiltered {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    amount: amt,
                    filter: Box::new(obj),
                });
            }
            Ok(())
        }
        InstructionAst::PayCost(CostAtomAst::Sacrifice { count, filter }) => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let ptf = lower_permanent_type_filter(filter)?;
            builder.instructions.push(Instruction::Sacrifice {
                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                amount: amt,
                filter: ptf,
            });
            Ok(())
        }
        InstructionAst::PayCost(CostAtomAst::Exile { count, filter })
            if filter.zone == Some(ZoneFilter::YourGraveyard)
                && (filter.any_card || filter.any_types.is_empty())
                && filter.forbidden_types.is_empty()
                && filter.subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.stat_bounds.is_empty() =>
        {
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(count, bound_x), vocabulary)?
            else {
                return Err("oracle:unsupported-pay-cost");
            };
            let u = u8::try_from(n.to_usize().ok_or("oracle:exile-gy-ovf")?)
                .map_err(|_| "oracle:exile-gy-ovf")?;
            let nz = NonZeroU8::new(u).ok_or("oracle:exile-gy-zero")?;
            builder
                .instructions
                .push(Instruction::ExileCardsFromGraveyard {
                    player: ResolvingPlayer::Controller,
                    count: nz,
                });
            Ok(())
        }
        InstructionAst::ExileTopLibrary {
            actor: None | Some(PlayerRefAst::You),
            whose,
            count,
            face_down: false,
        } => {
            let w = whose.trim().to_ascii_lowercase();
            let rp = match w.as_str() {
                "your" | "own" | "" => ResolvingPlayer::Controller,
                "target player" | "target player's" => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    ResolvingPlayer::TargetPlayer
                }
                "target opponent" | "target opponent's" => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    ResolvingPlayer::TargetPlayer
                }
                _ => return Err("oracle:unsupported-exile-top-whose"),
            };
            let ValueExpr::Literal(n) = lower_value_expr(resolve_x(count, bound_x), vocabulary)?
            else {
                return Err("oracle:dynamic-exile-top");
            };
            let u = u16::try_from(n.to_usize().ok_or("oracle:exile-top-ovf")?)
                .map_err(|_| "oracle:exile-top-ovf")?;
            builder.instructions.push(Instruction::ExileLibraryTop {
                player: rp,
                amount: u,
            });
            Ok(())
        }
        InstructionAst::WheelShuffleHandAndGraveyard {
            player: PlayerRefAst::EachPlayer,
            draw,
        } => {
            builder
                .instructions
                .push(Instruction::ShuffleEachHandAndGraveyard);
            let amt = lower_value_expr(resolve_x(draw, bound_x), vocabulary)?;
            if amt != ValueExpr::literal(0) {
                builder
                    .instructions
                    .push(Instruction::DrawEachPlayer { amount: amt });
            }
            Ok(())
        }
        InstructionAst::ShuffleIntoLibrary { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            builder
                .instructions
                .push(Instruction::ShuffleAbilitySourceIntoLibrary);
            Ok(())
        }
        InstructionAst::ShuffleIntoLibrary { subject } => match subject.as_ref() {
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if filter.zone.is_none() => {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::ShuffleTargetIntoLibrary);
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } if matches!(
                filter.zone,
                Some(ZoneFilter::YourGraveyard | ZoneFilter::AnyGraveyard)
            ) =>
            {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                builder
                    .instructions
                    .push(Instruction::ShuffleTargetIntoLibrary);
                Ok(())
            }
            _ => Err("oracle:unsupported-shuffle-into-library"),
        },
        InstructionAst::AbandonSourceScheme => {
            builder.instructions.push(Instruction::AbandonSourceScheme);
            Ok(())
        }
        InstructionAst::DoubleStatOrCounters {
            subject,
            kind,
            duration,
        } => {
            let low = kind.trim().to_ascii_lowercase();
            if low.contains("counter")
                && matches!(
                    subject.as_ref(),
                    SubjectRefAst::TargetObject {
                        count: TargetCountAst::Exact(1) | TargetCountAst::UpTo(_),
                        ..
                    }
                )
            {
                builder.bind_target(BodyTarget::SinglePermanent)?;
                builder
                    .instructions
                    .push(Instruction::DoubleCountersOnTargets);
                return Ok(());
            }
            let (power, toughness) = match low.as_str() {
                "power" => (true, false),
                "toughness" => (false, true),
                "power and toughness" | "power_and_toughness" => (true, true),
                _ => return Err("oracle:unsupported-double-stat"),
            };
            let dur = lower_duration(duration)?;
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughness {
                            target_self: true,
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughness {
                            target_self: false,
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughness {
                            target_self: false,
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                _ => Err("oracle:unsupported-double-stat"),
            }
        }
        InstructionAst::Attach { subject, target } => {
            if is_self_pronoun(subject, builder, self_pronoun_valid) {
                if let SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } = target.as_ref()
                    && filter.zone.is_none()
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    if in_trigger
                        && bt
                            == BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled())
                    {
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::Attach(AttachAbility::Equip));
                        return Ok(());
                    }
                    if !in_trigger && bt == BodyTarget::SingleCreature {
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::Attach(AttachAbility::Equip));
                        return Ok(());
                    }
                }
            } else if matches!(subject.as_ref(), SubjectRefAst::TriggerSubject)
                && in_trigger
                && let SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } = target.as_ref()
                && filter.zone.is_none()
            {
                let bt = lower_battlefield_body_target(filter, vocabulary)?;
                if bt == BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled()) {
                    builder.bind_target(bt)?;
                    builder.instructions.push(Instruction::AttachOnEntry {
                        entering_equipment: true,
                    });
                    return Ok(());
                }
            }
            Err("oracle:unsupported-attach")
        }
        InstructionAst::DealDamageDivided {
            source,
            amount,
            targets_desc,
        } => {
            let amt = lower_value_expr(resolve_x(amount, bound_x), vocabulary)?;
            let desc_low = targets_desc.trim().to_ascii_lowercase();
            if is_self_pronoun(source, builder, self_pronoun_valid)
                && in_trigger
                && amt == ValueExpr::literal(3)
                && matches!(
                    desc_low.as_str(),
                    "one, two, or three targets"
                        | "any number of targets"
                        | "up to three targets"
                        | "one, two, or three target creatures and/or players"
                )
            {
                builder.bind_target(BodyTarget::SingleAnyTarget)?;
                builder
                    .instructions
                    .push(Instruction::DivideThreeDamageAmongTargets);
                return Ok(());
            }
            Err("oracle:unsupported-divided-damage")
        }
        InstructionAst::DelayedTrigger { step, body } => {
            let (eng_step, own_step_only) = match step {
                TurnStepAst::Upkeep => (Step::Upkeep, true),
                TurnStepAst::DrawStep => (Step::Draw, true),
                TurnStepAst::PrecombatMain | TurnStepAst::MainPhase => (Step::PrecombatMain, true),
                TurnStepAst::PostcombatMain => (Step::PostcombatMain, true),
                TurnStepAst::BeginningOfCombat => (Step::BeginningCombat, false),
                TurnStepAst::EndOfCombat => (Step::EndCombat, false),
                TurnStepAst::EndStep => (Step::End, false),
                TurnStepAst::UntapStep | TurnStepAst::CleanupStep => {
                    return Err("oracle:unsupported-delayed-step");
                }
            };
            let delayed_body = lower_effect_body(body, vocabulary, ids, true, self_pronoun_valid)?;
            builder
                .instructions
                .push(Instruction::ScheduleDelayedStepTrigger {
                    step: eng_step,
                    own_step_only,
                    body: Box::new(delayed_body),
                });
            Ok(())
        }
        InstructionAst::ReflexiveTrigger(body) => {
            let reflexive_body =
                lower_effect_body(body, vocabulary, ids, true, self_pronoun_valid)?;
            builder
                .instructions
                .push(Instruction::EmitReflexiveTrigger {
                    body: Box::new(reflexive_body),
                });
            Ok(())
        }
        InstructionAst::RemoveKeywords {
            subject,
            keywords,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let suppressed = lower_keyword_list(keywords)?;
            if suppressed.is_empty() {
                return Err("oracle:empty-remove-keywords");
            }
            let ab_change = AbilityChange {
                suppressed_keywords: suppressed,
                ..AbilityChange::default()
            };
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                        duration: dur,
                    });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                _ => Err("oracle:unsupported-remove-keywords"),
            }
        }
        InstructionAst::Unless {
            body,
            unless_player,
            unless_cost: Some(CostAtomAst::PayMana(raw)),
            unless_action: None,
            unless_condition: None,
        } => {
            let cost = parse_fixed_mana_cost(raw).ok_or("oracle:unless-pay-x")?;
            let payer =
                lower_resolving_player(unless_player.unwrap_or(PlayerRefAst::You), builder)?;
            let offer_idx = builder.instructions.len();
            builder.instructions.push(Instruction::OfferManaPayment {
                payer,
                cost,
                paid_destination: 0,
                declined_destination: offer_idx + 1,
            });
            lower_instruction_seq(
                body,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let end_dest = builder.instructions.len();
            if let Some(Instruction::OfferManaPayment {
                paid_destination, ..
            }) = builder.instructions.get_mut(offer_idx)
            {
                *paid_destination = end_dest;
            }
            Ok(())
        }
        InstructionAst::Unless {
            body,
            unless_player: _,
            unless_cost: None,
            unless_action: None,
            unless_condition: Some(cond),
        } => {
            let mut rc =
                lower_resolution_condition(cond, builder, vocabulary, in_trigger, bound_x)?;
            rc.comparison = negate_comparison(rc.comparison);
            let branch_idx = builder.instructions.len();
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: rc,
                destination: 0,
            });
            lower_instruction_seq(
                body,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let end_dest = builder.instructions.len();
            if let Some(Instruction::BranchIfFalse { destination, .. }) =
                builder.instructions.get_mut(branch_idx)
            {
                *destination = end_dest;
            }
            Ok(())
        }
        InstructionAst::TemporaryStatic {
            static_ability,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            if let StaticAbilityAst::RuleOrCombatRestriction {
                subject,
                rule,
                filter,
                condition: None,
            } = static_ability.as_ref()
                && let Some(br) =
                    lower_rule_blocking_restriction(rule, filter.as_deref(), vocabulary)
            {
                match subject.as_deref() {
                    None => {
                        if self_pronoun_valid {
                            builder.instructions.push(Instruction::ChangeAbilitySource {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: dur,
                            });
                            return Ok(());
                        }
                    }
                    Some(s) if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        builder.instructions.push(Instruction::ChangeAbilitySource {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                br,
                            )]),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    Some(SubjectRefAst::TargetObject {
                        filter: target_f,
                        count: TargetCountAst::Exact(1),
                    }) if target_f.zone.is_none() => {
                        let bt = lower_battlefield_body_target(target_f, vocabulary)?;
                        builder.bind_target(bt)?;
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: dur,
                            });
                        return Ok(());
                    }
                    Some(SubjectRefAst::Anaphora(_)) if builder.target != BodyTarget::None => {
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(br),
                                ]),
                                duration: dur,
                            });
                        return Ok(());
                    }
                    _ => {}
                }
            }
            Err("oracle:unsupported-temp-static")
        }
        InstructionAst::ForEachPlayer { player, body } => {
            if let [single] = body.as_slice() {
                if *player == PlayerRefAst::EachOpponent
                    && let Some(scaled) =
                        try_scale_controller_instruction(single, ValueExprAst::OpponentsCount)
                {
                    return lower_instruction(
                        &scaled,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
                if let Some(rewritten) = try_rewrite_for_each_player(single, *player) {
                    return lower_instruction(
                        &rewritten,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::ForEachObject { filter, body } => {
            if let [single] = body.as_slice()
                && let Some(scaled) =
                    try_scale_instruction(single, ValueExprAst::Count(filter.clone()))
            {
                return lower_instruction(
                    &scaled,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::ForEachCount { count, body } => {
            if let [single] = body.as_slice()
                && let Some(scaled) =
                    try_scale_instruction(single, resolve_x(count, bound_x).clone())
            {
                return lower_instruction(
                    &scaled,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::ManualTabletopAction { prompt } => {
            builder.instructions.push(Instruction::TabletopHonorAction {
                prompt: prompt.clone(),
            });
            Ok(())
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

fn scale_ast_amount(base: &ValueExprAst, mult: ValueExprAst) -> ValueExprAst {
    if matches!(base, ValueExprAst::Const(1)) {
        mult
    } else {
        ValueExprAst::Mul(Box::new(mult), Box::new(base.clone()))
    }
}

fn try_scale_token_spec(spec: &TokenSpecAst, mult: ValueExprAst) -> Option<TokenSpecAst> {
    match spec {
        TokenSpecAst::Predefined {
            name,
            count,
            tapped,
        } => Some(TokenSpecAst::Predefined {
            name: name.clone(),
            count: Box::new(scale_ast_amount(count, mult)),
            tapped: *tapped,
        }),
        TokenSpecAst::Creature {
            count,
            power,
            toughness,
            colors,
            supertypes,
            card_types,
            subtypes,
            keywords,
            tapped,
            attacking,
            named,
        } => Some(TokenSpecAst::Creature {
            count: Box::new(scale_ast_amount(count, mult)),
            power: power.clone(),
            toughness: toughness.clone(),
            colors: *colors,
            supertypes: *supertypes,
            card_types: *card_types,
            subtypes: subtypes.clone(),
            keywords: keywords.clone(),
            tapped: *tapped,
            attacking: *attacking,
            named: named.clone(),
        }),
        _ => None,
    }
}

fn try_scale_instruction(instr: &InstructionAst, mult: ValueExprAst) -> Option<InstructionAst> {
    match instr {
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } => Some(InstructionAst::ChangeLife {
            player: *player,
            delta: Box::new(scale_ast_amount(delta, mult)),
            gain: *gain,
        }),
        InstructionAst::Draw { player, count } => Some(InstructionAst::Draw {
            player: *player,
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::Mill { player, count } => Some(InstructionAst::Mill {
            player: *player,
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora: None,
            random: false,
        } => Some(InstructionAst::Discard {
            player: *player,
            count: Some(Box::new(scale_ast_amount(
                count.as_deref().unwrap_or(&ValueExprAst::Const(1)),
                mult,
            ))),
            filter: filter.clone(),
            anaphora: None,
            random: false,
        }),
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => Some(InstructionAst::DealDamage {
            source: source.clone(),
            target: target.clone(),
            amount: Box::new(scale_ast_amount(amount, mult)),
        }),
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } => Some(InstructionAst::PutCounters {
            subject: subject.clone(),
            counter_kind: counter_kind.clone(),
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } => Some(InstructionAst::GainPlayerCounters {
            player: *player,
            counter_kind: counter_kind.clone(),
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::CreateToken { controller, token } => Some(InstructionAst::CreateToken {
            controller: *controller,
            token: try_scale_token_spec(token, mult)?,
        }),
        InstructionAst::Investigate { player, count } => Some(InstructionAst::Investigate {
            player: *player,
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        _ => None,
    }
}

fn try_scale_controller_instruction(
    instr: &InstructionAst,
    mult: ValueExprAst,
) -> Option<InstructionAst> {
    let acts_on_you = matches!(
        instr,
        InstructionAst::ChangeLife {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::Draw {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::Mill {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::GainPlayerCounters {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::CreateToken {
            controller: PlayerRefAst::You,
            ..
        } | InstructionAst::Investigate {
            player: PlayerRefAst::You,
            ..
        }
    );
    if acts_on_you {
        try_scale_instruction(instr, mult)
    } else {
        None
    }
}

fn try_rewrite_for_each_player(
    instr: &InstructionAst,
    target_player: PlayerRefAst,
) -> Option<InstructionAst> {
    let matches_loop_player = |p: PlayerRefAst| p == PlayerRefAst::ThatPlayer || p == target_player;
    match instr {
        InstructionAst::Draw { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Draw {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::Mill { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Mill {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora,
            random,
        } if matches_loop_player(*player) => Some(InstructionAst::Discard {
            player: target_player,
            count: count.clone(),
            filter: filter.clone(),
            anaphora: anaphora.clone(),
            random: *random,
        }),
        InstructionAst::Sacrifice { player, subject } if matches_loop_player(*player) => {
            Some(InstructionAst::Sacrifice {
                player: target_player,
                subject: subject.clone(),
            })
        }
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } if matches_loop_player(*player) => Some(InstructionAst::ChangeLife {
            player: target_player,
            delta: delta.clone(),
            gain: *gain,
        }),
        InstructionAst::CreateToken { controller, token } if matches_loop_player(*controller) => {
            Some(InstructionAst::CreateToken {
                controller: target_player,
                token: token.clone(),
            })
        }
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } if matches!(target.as_ref(), SubjectRefAst::Player(p) if matches_loop_player(*p)) => {
            Some(InstructionAst::DealDamage {
                source: source.clone(),
                target: Box::new(SubjectRefAst::Player(target_player)),
                amount: amount.clone(),
            })
        }
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } if matches_loop_player(*player) => Some(InstructionAst::GainPlayerCounters {
            player: target_player,
            counter_kind: counter_kind.clone(),
            count: count.clone(),
        }),
        InstructionAst::ExileGraveyard { whose }
            if matches!(
                whose.trim().to_ascii_lowercase().as_str(),
                "their" | "that player's" | "his or her"
            ) =>
        {
            let mapped = match target_player {
                PlayerRefAst::EachPlayer => "each player's",
                PlayerRefAst::EachOpponent => "each opponent's",
                PlayerRefAst::You => "your",
                _ => return None,
            };
            Some(InstructionAst::ExileGraveyard {
                whose: mapped.to_owned(),
            })
        }
        _ => None,
    }
}

fn push_damage_target(instructions: &mut Vec<Instruction>, amt: ValueExpr) {
    if let ValueExpr::Literal(n) = amt {
        instructions.push(Instruction::Damage { amount: n });
    } else {
        instructions.push(Instruction::DamageAmount(amt));
    }
}

fn lower_damage_group(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<DamageGroup, &'static str> {
    if f.any_types == AstCardTypeMask::CREATURE
        && f.forbidden_types.is_empty()
        && f.controller.is_none()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
    {
        if f.forbidden_keywords == ["flying"] {
            return Ok(DamageGroup::CreaturesWithoutFlying);
        }
        if f.subtypes.len() == 1
            && let Some(ct) = vocabulary.resolve_label(&f.subtypes[0])
        {
            return Ok(DamageGroup::CreaturesOfSubtype(ct));
        }
        if f.forbidden_subtypes.len() == 1
            && let Some(ct) = vocabulary.resolve_label(&f.forbidden_subtypes[0])
        {
            return Ok(DamageGroup::CreaturesExcept(ct));
        }
        if f.subtypes.is_empty()
            && f.forbidden_subtypes.is_empty()
            && f.required_keywords.is_empty()
            && f.forbidden_keywords.is_empty()
            && f.flags.is_empty()
        {
            return Ok(DamageGroup::Creatures);
        }
    }
    if f.any_types == AstCardTypeMask::CREATURE.union(AstCardTypeMask::PLANESWALKER)
        && f.forbidden_types.is_empty()
        && f.controller.is_none()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.flags.is_empty()
    {
        return Ok(DamageGroup::CreaturesAndPlaneswalkers);
    }
    if f.zone.is_none() {
        let obj = lower_object_filter(f, vocabulary)?;
        return Ok(DamageGroup::Filtered(obj));
    }
    Err("oracle:unsupported-damage-group")
}

fn lower_modify_pt_target(
    subject: &SubjectRefAst,
    pt: PowerToughness,
    dur: ResolvedDuration,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    match subject {
        s if is_self_pronoun(s, builder, self_pronoun_valid)
            && dur == ResolvedDuration::UntilEndOfTurn =>
        {
            if in_trigger {
                builder
                    .instructions
                    .push(Instruction::ModifyGroupUntilEndOfTurn {
                        group: Box::new(EffectSubject::AbilitySource),
                        power_toughness: Some(pt),
                        keywords: BTreeSet::new(),
                    });
            } else {
                builder.instructions.push(Instruction::BoostSource {
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            }
            Ok(())
        }
        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
            builder.instructions.push(Instruction::ChangeAbilitySource {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if filter.zone.is_none() => {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(n),
        } if filter.zone.is_none() && *n >= 2 => {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: *n,
                    maximum: *n,
                    action: PermanentTargetAction::Modify {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::UpTo(max),
        } if filter.zone.is_none() && *max > 0 => {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: *max,
                    action: PermanentTargetAction::Modify {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::AttachedHost(_) if dur == ResolvedDuration::UntilEndOfTurn => {
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                        ObjectClause::any().attached_to_self(),
                    ))),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if dur == ResolvedDuration::UntilEndOfTurn =>
        {
            let obj = lower_object_filter(f, vocabulary)?;
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::Matching(obj)),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        _ => Err("oracle:unsupported-modify-pt-subject"),
    }
}

fn lower_sacrifice_selection(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::program::SacrificeSelection> {
    use spellwake_engine::program::SacrificeSelection;
    if !f.forbidden_types.is_empty()
        || !f.required_supertypes.is_empty()
        || !f.forbidden_supertypes.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || !matches!(f.controller, None | Some(ControllerRelation::You))
        || f.zone.is_some()
        || f.combat.is_some()
        || f.tapped.is_some()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.stat_bounds.is_empty()
        || f.counter_kind.is_some()
        || f.any_card
        || f.any_spell
        || f.is_ability
    {
        return None;
    }
    let exclude_self = f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0;
    if f.flags.0 & !AstFilterFlags::EXCLUDE_SELF.0 != 0 {
        return None;
    }
    if f.subtypes.len() == 1 {
        if exclude_self
            || f.token.is_some()
            || (!f.any_types.is_empty() && f.any_types != AstCardTypeMask::CREATURE)
        {
            return None;
        }
        let ct = vocabulary.resolve_label(&f.subtypes[0])?;
        return Some(SacrificeSelection::Subtype(ct));
    }
    if !f.subtypes.is_empty() {
        return None;
    }
    let nontoken = match f.token {
        None => false,
        Some(false) => true,
        Some(true) => return None,
    };
    let art_or_cre = AstCardTypeMask::ARTIFACT.union(AstCardTypeMask::CREATURE);
    if nontoken {
        if !exclude_self && f.any_types == AstCardTypeMask::CREATURE {
            return Some(SacrificeSelection::NontokenCreature);
        }
        return None;
    }
    if f.any_types == AstCardTypeMask::CREATURE {
        Some(if exclude_self {
            SacrificeSelection::OtherCreature
        } else {
            SacrificeSelection::Creature
        })
    } else if f.any_types == AstCardTypeMask::ARTIFACT {
        Some(if exclude_self {
            SacrificeSelection::OtherArtifact
        } else {
            SacrificeSelection::Artifact
        })
    } else if f.any_types == AstCardTypeMask::ENCHANTMENT && !exclude_self {
        Some(SacrificeSelection::Enchantment)
    } else if f.any_types == AstCardTypeMask::LAND && !exclude_self {
        Some(SacrificeSelection::Land)
    } else if f.any_types == art_or_cre {
        Some(if exclude_self {
            SacrificeSelection::OtherArtifactOrCreature
        } else {
            SacrificeSelection::ArtifactOrCreature
        })
    } else if f.any_types.is_empty() && f.any_permanent {
        Some(if exclude_self {
            SacrificeSelection::OtherPermanent
        } else {
            SacrificeSelection::Permanent
        })
    } else {
        None
    }
}

fn lower_permanent_type_filter(f: &ObjectFilterAst) -> Result<PermanentTypeFilter, &'static str> {
    if !f.subtypes.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || f.tapped.is_some()
        || f.token.is_some()
        || !f.stat_bounds.is_empty()
    {
        return Err("oracle:complex-sac-filter");
    }
    Ok(PermanentTypeFilter {
        any_types: card_type_mask_to_set(f.any_types)?,
        required_types: BTreeSet::new(),
        forbidden_types: card_type_mask_to_set(f.forbidden_types)?,
    })
}

fn lower_library_search(
    library: &str,
    count: &ValueExprAst,
    filter: &ObjectFilterAst,
    reveal: bool,
    to_zone: &str,
    vocabulary: &CreatureTypeVocabulary,
    optional_search: bool,
) -> Result<LibrarySearch, &'static str> {
    let source = match library.to_ascii_lowercase().as_str() {
        "your" | "own" | "" => SearchSource::OwnLibrary,
        _ => return Err("oracle:unsupported-search-source"),
    };
    let ValueExprAst::Const(n) = count else {
        return Err("oracle:dynamic-search-count");
    };
    let n_u8 = u8::try_from(*n).map_err(|_| "oracle:search-count-ovf")?;
    if n_u8 == 0 {
        return Err("oracle:search-zero");
    }
    let matcher = lower_search_matcher(filter, vocabulary)?;
    let search_count = if matcher == SearchCardMatcher::Any && !optional_search {
        SearchCount::Exact(NonZeroU8::new(n_u8).ok_or("oracle:search-zero")?)
    } else {
        SearchCount::UpTo(n_u8)
    };
    let dest_low = to_zone.to_ascii_lowercase();
    let destination = if dest_low.contains("hand") {
        SearchDestination::Hand
    } else if dest_low.contains("graveyard") {
        SearchDestination::Graveyard
    } else if dest_low.contains("battlefield") {
        SearchDestination::Battlefield {
            tapped: dest_low.contains("tapped"),
            post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::None,
        }
    } else if dest_low.contains("top") {
        SearchDestination::LibraryTop
    } else if dest_low.contains("exile") {
        SearchDestination::Exile {
            link: SearchExileLink::None,
        }
    } else {
        return Err("oracle:unsupported-search-dest");
    };
    let mut ls = LibrarySearch {
        searcher: ResolvingPlayer::Controller,
        source,
        matcher,
        mana_value: None,
        count: search_count,
        pair_constraint: None,
        destination,
        reveal,
    };
    if !matches!(ls.matcher, SearchCardMatcher::Filtered(_)) {
        if filter.stat_bounds.len() == 1 {
            let sb = &filter.stat_bounds[0];
            if sb.stat == StatKind::ManaValue {
                ls.mana_value = match (sb.op, sb.value.as_ref()) {
                    (CompareOp::Le, ValueExprAst::Const(v)) if *v >= 0 => Some(
                        SearchManaValue::AtMost(u32::try_from(*v).map_err(|_| "oracle:mv-ovf")?),
                    ),
                    (CompareOp::Ge, ValueExprAst::Const(v)) if *v >= 0 => Some(
                        SearchManaValue::AtLeast(u32::try_from(*v).map_err(|_| "oracle:mv-ovf")?),
                    ),
                    (CompareOp::Eq, ValueExprAst::Const(v)) if *v >= 0 => {
                        Some(SearchManaValue::Exact(i64::from(*v).into()))
                    }
                    (CompareOp::Le, ValueExprAst::ChosenX) => {
                        Some(SearchManaValue::AtMostAnnouncedX)
                    }
                    _ => return Err("oracle:unsupported-search-mv"),
                };
            } else {
                let obj = lower_object_filter(filter, vocabulary)?;
                ls.matcher = SearchCardMatcher::Filtered(Box::new(obj));
            }
        } else if !filter.stat_bounds.is_empty() {
            let obj = lower_object_filter(filter, vocabulary)?;
            ls.matcher = SearchCardMatcher::Filtered(Box::new(obj));
        }
    }
    Ok(ls)
}

fn lower_search_matcher(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SearchCardMatcher, &'static str> {
    let has_complex = !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || !f.forbidden_supertypes.is_empty()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.flags.is_empty();
    if !has_complex {
        if f.any_card
            && f.any_types.is_empty()
            && f.forbidden_types.is_empty()
            && f.subtypes.is_empty()
            && f.required_supertypes.is_empty()
        {
            return Ok(SearchCardMatcher::Any);
        }
        if f.forbidden_types == AstCardTypeMask::LAND
            && f.any_types.is_empty()
            && f.subtypes.is_empty()
            && f.required_supertypes.is_empty()
        {
            return Ok(SearchCardMatcher::Nonland);
        }
        if f.required_supertypes == AstSupertypeMask::BASIC
            && f.any_types == AstCardTypeMask::LAND
            && f.subtypes.is_empty()
            && f.forbidden_types.is_empty()
        {
            return Ok(SearchCardMatcher::BasicLand);
        }
        if f.required_supertypes.is_empty() && f.forbidden_types.is_empty() {
            if !f.subtypes.is_empty() {
                if f.subtypes.len() == 1 {
                    let st = &f.subtypes[0];
                    if st.eq_ignore_ascii_case("equipment") {
                        return Ok(SearchCardMatcher::Equipment);
                    }
                    if let Some(ct) = vocabulary.resolve_label(st) {
                        return Ok(SearchCardMatcher::CreatureSubtype(ct));
                    }
                }
                let mut lands = BTreeSet::new();
                let mut all_lands = true;
                for st in &f.subtypes {
                    if let Some(lt) = parse_land_subtype(st) {
                        lands.insert(lt);
                    } else {
                        all_lands = false;
                        break;
                    }
                }
                if all_lands && !lands.is_empty() {
                    return Ok(SearchCardMatcher::LandSubtypes {
                        subtypes: lands,
                        require_land_type: f.any_types.0 & AstCardTypeMask::LAND.0 != 0,
                    });
                }
            } else {
                let types = card_type_mask_to_set(f.any_types)?;
                if !types.is_empty() {
                    return Ok(SearchCardMatcher::CardTypes(types));
                }
                return Ok(SearchCardMatcher::Any);
            }
        }
    }
    let obj = lower_object_filter(f, vocabulary)?;
    Ok(SearchCardMatcher::Filtered(Box::new(obj)))
}

fn lower_token_spec(
    spec: &TokenSpecAst,
    recipients: TokenRecipients,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    bound_x: Option<&ValueExprAst>,
) -> Result<(TokenCreation, bool), &'static str> {
    match spec {
        TokenSpecAst::Predefined {
            name,
            count,
            tapped,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let def = predefined_token_def(name, vocabulary, ids)?;
            Ok((
                TokenCreation {
                    enters_tapped: *tapped,
                    definition: def,
                    amount: amt,
                    base_size: None,
                    recipients,
                },
                false,
            ))
        }
        TokenSpecAst::Creature {
            count,
            power,
            toughness,
            colors,
            supertypes,
            card_types,
            subtypes,
            keywords,
            tapped,
            attacking,
            named,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let (p_val, t_val, base_size) =
                if power.eq_ignore_ascii_case("x") && toughness.eq_ignore_ascii_case("x") {
                    let x_expr = if let Some(bx) = bound_x {
                        lower_value_expr(bx, vocabulary)?
                    } else {
                        ValueExpr::chosen_x()
                    };
                    (0i64, 0i64, Some(TokenSize::AtCreation(x_expr)))
                } else {
                    let p: i64 = power.parse().map_err(|_| "oracle:token-pt")?;
                    let t: i64 = toughness.parse().map_err(|_| "oracle:token-pt")?;
                    (p, t, None)
                };

            let mut creature_subtypes = BTreeSet::new();
            let mut artifact_subtypes = BTreeSet::new();
            let mut ordered_subtypes = Vec::new();
            for st in subtypes {
                if let Some(ct) = vocabulary.resolve_label(st) {
                    if creature_subtypes.insert(ct) {
                        ordered_subtypes.push(TokenSubtype::Creature(ct));
                    }
                } else if let Some(at) = ArtifactSubtype::from_wire_label(st) {
                    if artifact_subtypes.insert(at) {
                        ordered_subtypes.push(TokenSubtype::Artifact(at));
                    }
                } else {
                    return Err("oracle:token-unknown-subtype");
                }
            }

            let kws = lower_keyword_list(keywords)?;
            let mut p = Program::creature(None, p_val.into(), t_val.into(), creature_subtypes)
                .with_printed_keywords(kws);
            let extra_types = card_type_mask_to_set(*card_types)?;
            for t in extra_types {
                if t.is_permanent() {
                    p.card_types.insert(t);
                }
            }
            p.supertypes = supertype_mask_to_set(*supertypes)?;
            p.artifact_subtypes = artifact_subtypes;
            p.effect_defined_colors = Some(color_mask_to_set(*colors));

            let token_name = if let Some(n) = named {
                TokenName::Specified(
                    EnglishCardName::new(n.clone()).map_err(|_| "oracle:token-name")?,
                )
            } else {
                if ordered_subtypes.is_empty() {
                    return Err("oracle:nameless-token");
                }
                TokenName::Default { ordered_subtypes }
            };

            let def = TokenDefinition::new(p, token_name, vocabulary)
                .map_err(|_| "oracle:token-def-invalid")?;
            Ok((
                TokenCreation {
                    enters_tapped: *tapped,
                    definition: def,
                    amount: amt,
                    base_size,
                    recipients,
                },
                *attacking,
            ))
        }
        _ => Err("oracle:unsupported-token-spec"),
    }
}

fn predefined_token_def(
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<TokenDefinition, &'static str> {
    match name.to_ascii_lowercase().as_str() {
        "treasure" => {
            let mut p = Program::artifact(None, vec![]);
            p.artifact_subtypes.insert(ArtifactSubtype::Treasure);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaOutput::all_colors(),
                }
                .with_modifier(ManaOutputModifier::Sacrifice(
                    ManaSacrificeCost::Source { tap: true },
                )),
            });
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Treasure)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "clue" => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::Sacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::DrawController {
                            amount: ValueExpr::literal(1),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Clue);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Clue)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "food" => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(3.into()),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Food);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Food)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "gold" => {
            let mut p = Program::artifact(None, vec![]);
            p.artifact_subtypes.insert(ArtifactSubtype::Gold);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaOutput::all_colors(),
                }
                .with_modifier(ManaOutputModifier::Sacrifice(
                    ManaSacrificeCost::Source { tap: false },
                )),
            });
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Gold)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        _ => Err("oracle:unsupported-predef-token"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyword_creatures::tests::current_catalogue;
    use flate2::read::MultiGzDecoder;
    use spellwake_content::CatalogFace;
    use std::{
        collections::HashMap,
        fs::File,
        io::{BufRead, BufReader},
    };

    fn synthetic_vocab() -> CreatureTypeVocabulary {
        let mut members = vec![
            "Army".into(),
            "Beast".into(),
            "Construct".into(),
            "Druid".into(),
            "Elemental".into(),
            "Elf".into(),
            "Goblin".into(),
            "Human".into(),
            "Soldier".into(),
            "Spirit".into(),
            "Warrior".into(),
            "Zombie".into(),
        ];
        members.sort();
        CreatureTypeVocabulary::new("test-vocab".into(), [0u8; 32], members).expect("vocab")
    }

    fn synthetic_card(
        name: &str,
        type_line: &str,
        mana_cost: Option<&str>,
        pt: Option<(&str, &str)>,
        oracle_text: &str,
    ) -> InventoryCard {
        InventoryCard {
            scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
            oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
            name: name.into(),
            layout: "normal".into(),
            faces: vec![CatalogFace {
                name: name.into(),
                type_line: Some(type_line.into()),
                oracle_text: Some(oracle_text.into()),
                mana_cost: mana_cost.map(str::to_string),
                power: pt.map(|(p, _)| p.to_string()),
                toughness: pt.map(|(_, t)| t.to_string()),
            }],
            legalities: BTreeMap::new(),
            eligible_formats: Vec::new(),
            required_by_base_formats: true,
            program_evidence: "test",
        }
    }

    #[test]
    fn lowers_core_magic_archetypes_and_rejects_unsupported_clauses() {
        let vocab = synthetic_vocab();

        // 1. Creature with ETB draw
        let elvish = synthetic_card(
            "Elvish Visionary",
            "Creature \u{2014} Elf",
            Some("{1}{G}"),
            Some(("1", "1")),
            "When Elvish Visionary enters, draw a card.",
        );
        let p = compile_with_reason(&elvish, &vocab).expect("elvish visionary");
        assert_eq!(p.triggered.len(), 1);

        // 2. Burn spell with damage to any target
        let bolt = synthetic_card(
            "Lightning Bolt",
            "Instant",
            Some("{R}"),
            None,
            "Lightning Bolt deals 3 damage to any target.",
        );
        let p = compile_with_reason(&bolt, &vocab).expect("bolt");
        assert!(matches!(p.kind, ProgramKind::Spell(_)));

        // 3. Pump + keyword fusion (Aerial Maneuver)
        let trick = synthetic_card(
            "Aerial Maneuver",
            "Instant",
            Some("{1}{W}"),
            None,
            "Target creature gets +1/+1 and gains flying and first strike until end of turn.",
        );
        let p = compile_with_reason(&trick, &vocab).expect("aerial maneuver");
        let ProgramKind::Spell(spell) = p.kind else {
            panic!("expected spell");
        };
        assert_eq!(spell.body.instructions.len(), 1);

        // 4. Gatecrash Battalion trigger
        let battalion = synthetic_card(
            "Ordruun Veteran",
            "Creature \u{2014} Human Soldier",
            Some("{2}{R}{W}"),
            Some(("3", "1")),
            "Battalion \u{2014} Whenever Ordruun Veteran and at least two other creatures attack, Ordruun Veteran gains double strike until end of turn.",
        );
        let p = compile_with_reason(&battalion, &vocab).expect("ordruun veteran");
        assert_eq!(p.triggered.len(), 1);

        // 5. Gatecrash Bloodrush (targeted Channel from hand)
        let bloodrush = synthetic_card(
            "Viashino Shanktail",
            "Creature \u{2014} Warrior",
            Some("{3}{R}"),
            Some(("3", "1")),
            "First strike\nBloodrush \u{2014} {2}{R}, Discard Viashino Shanktail: Target attacking creature gets +3/+1 and gains first strike until end of turn.",
        );
        let p = compile_with_reason(&bloodrush, &vocab).expect("viashino shanktail");
        assert_eq!(p.activated.len(), 1);
        assert_eq!(
            p.activated[0].source_cost,
            SourceCost::HandDiscardSelf(HandDiscardKeyword::Channel)
        );

        // 6. Spark Trooper end-step self-sacrifice
        let spark = synthetic_card(
            "Spark Trooper",
            "Creature \u{2014} Elemental Soldier",
            Some("{1}{R}{R}{W}"),
            Some(("6", "1")),
            "Trample, lifelink, haste\nAt the beginning of the end step, sacrifice Spark Trooper.",
        );
        let p = compile_with_reason(&spark, &vocab).expect("spark trooper");
        assert_eq!(p.triggered.len(), 1);

        // 7. Aura with continuous +2/+2 and flying
        let aura = synthetic_card(
            "Angelic Blessing Aura",
            "Enchantment \u{2014} Aura",
            Some("{1}{W}"),
            None,
            "Enchant creature\nEnchanted creature gets +2/+2 and has flying.",
        );
        let p = compile_with_reason(&aura, &vocab).expect("aura");
        assert!(p.continuous.is_some());

        // 8. Mana artifact
        let rock = synthetic_card(
            "Emerald Relic",
            "Artifact",
            Some("{2}"),
            None,
            "{T}: Add {G}.",
        );
        let p = compile_with_reason(&rock, &vocab).expect("mana rock");
        assert_eq!(p.mana.len(), 1);

        // 9. Strict rejection: never silently drop an unrecognized clause
        let partial = synthetic_card(
            "Unparseable Card",
            "Creature \u{2014} Human",
            Some("{1}{U}"),
            Some(("2", "2")),
            "Flying\nFlibbertigibbet the multiverse twice.",
        );
        assert!(compile_with_reason(&partial, &vocab).is_err());
    }

    #[test]
    #[ignore = "uses saved dated Scryfall bulk corpus and creature vocabulary"]
    fn oracle_lowering_corpus_coverage_audit() {
        let (inventory, vocabulary, registry) = current_catalogue();

        let total_required = inventory
            .cards
            .iter()
            .filter(|c| c.required_by_base_formats)
            .count();
        let total_bound = registry
            .coverage()
            .cards
            .iter()
            .filter(|entry| matches!(entry.binding, crate::BindingStatus::Partial { .. }))
            .count();

        // Count how many required cards compile strictly via oracle_lowering alone,
        // and tally drop reasons.
        let mut oracle_ok = 0usize;
        let mut reasons: HashMap<&'static str, usize> = HashMap::new();
        let mut oracle_ok_ids: BTreeSet<String> = BTreeSet::new();

        for card in inventory
            .cards
            .iter()
            .filter(|c| c.required_by_base_formats)
        {
            let hybrid_face = (card.faces.len() == 1)
                .then(|| super::super::split_hybrid_cost(card.faces[0].mana_cost.as_deref()?))
                .flatten();
            let normalized = hybrid_face.as_ref().map(|(cost, _)| {
                let mut normal = card.clone();
                normal.faces[0].mana_cost = Some(cost.clone());
                normal
            });
            let input = normalized.as_ref().unwrap_or(card);
            match compile_with_reason(input, &vocabulary) {
                Ok(_) => {
                    oracle_ok += 1;
                    if let Some(oid) = &card.oracle_id {
                        oracle_ok_ids.insert(oid.clone());
                    }
                }
                Err(r) => {
                    *reasons.entry(r).or_insert(0) += 1;
                }
            }
        }

        // Load set codes from the gzipped oracle-cards input for benchmark sets
        let raw_path =
            std::env::var_os("SPELLWAKE_CATALOGUE_INPUT").expect("SPELLWAKE_CATALOGUE_INPUT");
        let file = File::open(raw_path).expect("open catalogue");
        let reader = BufReader::new(MultiGzDecoder::new(file));
        let mut set_oracles: HashMap<String, BTreeSet<String>> = HashMap::new();
        let target_sets = ["gtc", "isd", "dka", "avr", "cmm"];
        for line in reader.lines() {
            let line = line.expect("read line");
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(&line).expect("json");
            let Some(set_code) = v.get("set").and_then(|s| s.as_str()) else {
                continue;
            };
            if !target_sets.contains(&set_code) {
                continue;
            }
            if let Some(oid) = v.get("oracle_id").and_then(|s| s.as_str()) {
                set_oracles
                    .entry(set_code.to_string())
                    .or_default()
                    .insert(oid.to_string());
            }
        }

        let bound_ids: BTreeSet<String> = registry
            .coverage()
            .cards
            .iter()
            .filter_map(|e| match &e.binding {
                crate::BindingStatus::Partial { .. } => e.oracle_id.clone(),
                crate::BindingStatus::Blocked { .. } | crate::BindingStatus::Missing => None,
            })
            .collect();

        eprintln!("=== ORACLE LOWERING CORPUS AUDIT ===");
        eprintln!(
            "Profile::Current total bound: {total_bound} / {total_required} ({:.2}%)",
            (total_bound as f64 / total_required as f64) * 100.0
        );
        eprintln!(
            "oracle_lowering standalone valid: {oracle_ok} / {total_required} ({:.2}%)",
            (oracle_ok as f64 / total_required as f64) * 100.0
        );

        let mut card_by_oid: HashMap<&str, &InventoryCard> = HashMap::new();
        for card in inventory
            .cards
            .iter()
            .filter(|c| c.required_by_base_formats)
        {
            if let Some(oid) = card.oracle_id.as_deref() {
                card_by_oid.insert(oid, card);
            }
        }

        for set_code in target_sets {
            let oids = set_oracles.get(set_code).cloned().unwrap_or_default();
            let req_in_set: Vec<&str> = oids
                .iter()
                .map(String::as_str)
                .filter(|id| card_by_oid.contains_key(id))
                .collect();
            let bound_in_set = req_in_set
                .iter()
                .filter(|id| bound_ids.contains(**id))
                .count();
            let oracle_in_set = req_in_set
                .iter()
                .filter(|id| oracle_ok_ids.contains(**id))
                .count();
            eprintln!(
                "Set {set_code}: Current bound = {bound_in_set}/{} ({:.2}%), oracle_lowering = {oracle_in_set}/{} ({:.2}%)",
                req_in_set.len(),
                if req_in_set.is_empty() {
                    0.0
                } else {
                    (bound_in_set as f64 / req_in_set.len() as f64) * 100.0
                },
                req_in_set.len(),
                if req_in_set.is_empty() {
                    0.0
                } else {
                    (oracle_in_set as f64 / req_in_set.len() as f64) * 100.0
                },
            );
        }

        let mut sorted_reasons: Vec<(&'static str, usize)> = reasons.into_iter().collect();
        sorted_reasons.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        eprintln!("Top 20 oracle_lowering drop reasons across 32,155 cards:");
        for (reason, count) in sorted_reasons.iter().take(20) {
            eprintln!("  {count:6}  {reason}");
        }

        // Sub-breakdown of engine-validation-failed errors
        let mut val_errors: HashMap<String, Vec<&str>> = HashMap::new();
        for card in inventory
            .cards
            .iter()
            .filter(|c| c.required_by_base_formats)
        {
            let [face] = card.faces.as_slice() else {
                continue;
            };
            if card.layout != "normal" || face.name != card.name {
                continue;
            }
            let Some(tl) = face.type_line.as_deref() else {
                continue;
            };
            let compiler_vocab = cached_compiler_vocab(&vocabulary);
            let ast = compile_card(
                &CardInput {
                    oracle_id: card.oracle_id.as_deref().unwrap_or(""),
                    name: &card.name,
                    layout: &card.layout,
                    faces: vec![FaceInput {
                        name: &face.name,
                        type_line: tl,
                        oracle_text: face.oracle_text.as_deref().unwrap_or(""),
                    }],
                },
                &compiler_vocab,
            );
            if !ast.strictly_compiled {
                continue;
            }
            if let Ok(prog) = lower_face(
                card,
                face,
                &ast.faces[0],
                &vocabulary,
                FaceLoweringOptions::default(),
            ) && let Err(e) = prog.validate(&vocabulary)
            {
                val_errors
                    .entry(format!("{e:?}"))
                    .or_default()
                    .push(card.name.as_str());
            }
        }
        let mut val_sorted: Vec<(String, Vec<&str>)> = val_errors.into_iter().collect();
        val_sorted.sort_by_key(|entry| std::cmp::Reverse(entry.1.len()));
        eprintln!("Engine validation failure breakdown:");
        for (err, names) in val_sorted.iter().take(12) {
            let sample: Vec<&str> = names.iter().copied().take(4).collect();
            eprintln!("  {:5}  {err}  (e.g. {:?})", names.len(), sample);
        }

        // Gatecrash-specific drop reasons on cards not yet bound
        let mut gtc_reasons: HashMap<&'static str, Vec<&str>> = HashMap::new();
        if let Some(gtc_oids) = set_oracles.get("gtc") {
            for oid in gtc_oids {
                if bound_ids.contains(oid) {
                    continue;
                }
                if let Some(card) = card_by_oid.get(oid.as_str()) {
                    let r = compile_with_reason(card, &vocabulary).err().unwrap_or("ok");
                    gtc_reasons.entry(r).or_default().push(card.name.as_str());
                }
            }
        }
        let mut gtc_sorted: Vec<(&'static str, Vec<&str>)> = gtc_reasons.into_iter().collect();
        gtc_sorted.sort_by_key(|entry| std::cmp::Reverse(entry.1.len()));
        eprintln!("Gatecrash (gtc) unbound card drop reasons:");
        for (reason, names) in gtc_sorted.iter().take(20) {
            let sample: Vec<&str> = names.iter().copied().take(5).collect();
            eprintln!("  {:4}  {reason}  (e.g. {:?})", names.len(), sample);
        }

        // Check how many avacyn_restored and innistrad cards oracle_lowering already covers
        let mut avr_total = 0usize;
        let mut avr_covered = 0usize;
        let mut isd_total = 0usize;
        let mut isd_covered = 0usize;
        for card in inventory
            .cards
            .iter()
            .filter(|c| c.required_by_base_formats)
        {
            if super::super::avacyn_restored::compile(card, &vocabulary).is_some() {
                avr_total += 1;
                if compile_with_reason(card, &vocabulary).is_ok() {
                    avr_covered += 1;
                }
            }
            if super::super::innistrad::compile(card, &vocabulary).is_some() {
                isd_total += 1;
                if compile_with_reason(card, &vocabulary).is_ok() {
                    isd_covered += 1;
                }
            }
        }
        // Sub-bucket telemetry for the biggest drop reasons so every Lowering Wave targets exact top gaps
        fn variant_name<T: std::fmt::Debug>(v: &T) -> String {
            let s = format!("{v:?}");
            s.split(['{', '(', ' ']).next().unwrap_or("?").to_string()
        }
        let mut unsup_ins: HashMap<String, usize> = HashMap::new();
        let mut unsup_kw: HashMap<String, usize> = HashMap::new();
        let mut unsup_static: HashMap<String, usize> = HashMap::new();
        let mut unsup_rule: HashMap<String, usize> = HashMap::new();
        let mut unsup_rep: HashMap<String, usize> = HashMap::new();
        let mut unsup_trig_ev: HashMap<String, usize> = HashMap::new();
        let mut unsup_trig_cond: HashMap<String, usize> = HashMap::new();
        let mut unsup_static_cond: HashMap<String, usize> = HashMap::new();
        let mut unsup_sub: HashMap<String, usize> = HashMap::new();
        let mut unsup_dmg_src: HashMap<String, usize> = HashMap::new();
        let mut unsup_pt_subj: HashMap<String, usize> = HashMap::new();
        let mut unsup_pat_action: HashMap<String, usize> = HashMap::new();
        let mut val_fail_errs: HashMap<String, Vec<String>> = HashMap::new();
        let mut unsup_ab_res: HashMap<String, usize> = HashMap::new();
        let mut unsup_enchant: HashMap<String, usize> = HashMap::new();
        let mut unsup_act_cost: HashMap<String, usize> = HashMap::new();
        let mut unsup_exile_gy: HashMap<String, usize> = HashMap::new();
        let mut unsup_ret_hand: HashMap<String, usize> = HashMap::new();
        let mut unsup_move_zone: HashMap<String, usize> = HashMap::new();
        let mut unsup_eff_mana: HashMap<String, usize> = HashMap::new();
        let mut unsup_val_expr: HashMap<String, usize> = HashMap::new();
        let mut unsup_if_cond: HashMap<String, usize> = HashMap::new();
        let mut unsup_grant_kw: HashMap<String, usize> = HashMap::new();
        let mut multi_face_layout: HashMap<String, usize> = HashMap::new();
        let mut unsup_leaf_ins: HashMap<String, usize> = HashMap::new();
        let mut unsup_kw_action: HashMap<String, usize> = HashMap::new();
        let mut unsup_pay_cost: HashMap<String, usize> = HashMap::new();
        let mut unsup_marker: HashMap<String, usize> = HashMap::new();

        fn collect_leaf_ins<'a>(ins: &'a [InstructionAst], out: &mut Vec<&'a InstructionAst>) {
            for item in ins {
                match item {
                    InstructionAst::Optional(inner)
                    | InstructionAst::OptionalPlayer { body: inner, .. }
                    | InstructionAst::IfCondition {
                        then_body: inner, ..
                    }
                    | InstructionAst::Otherwise(inner)
                    | InstructionAst::IfPreviousSucceeded(inner)
                    | InstructionAst::IfPreviousFailed(inner)
                    | InstructionAst::BindX { body: inner, .. }
                    | InstructionAst::ReflexiveTrigger(inner)
                    | InstructionAst::DelayedTrigger { body: inner, .. }
                    | InstructionAst::ForEachPlayer { body: inner, .. }
                    | InstructionAst::ForEachObject { body: inner, .. }
                    | InstructionAst::ForEachCount { body: inner, .. } => {
                        collect_leaf_ins(inner, out);
                    }
                    _ => out.push(item),
                }
            }
        }

        let compiler_vocab = cached_compiler_vocab(&vocabulary);
        for card in inventory
            .cards
            .iter()
            .filter(|c| c.required_by_base_formats)
        {
            let reason = match compile_with_reason(card, &vocabulary) {
                Ok(_) => continue,
                Err(r) => r,
            };
            if reason == "oracle:multi-face" || reason == "oracle:non-normal-layout" {
                *multi_face_layout.entry(card.layout.clone()).or_insert(0) += 1;
                continue;
            }
            let [face] = card.faces.as_slice() else {
                continue;
            };
            if card.layout != "normal" || face.name != card.name {
                continue;
            }
            let Some(tl) = face.type_line.as_deref() else {
                continue;
            };
            if reason == "oracle:unsupported-subtype" {
                if let Some((_, subs)) = tl.split_once(" \u{2014} ") {
                    for tok in subs.split_whitespace() {
                        *unsup_sub.entry(tok.to_string()).or_insert(0) += 1;
                    }
                }
                continue;
            }
            let ast = compile_card(
                &CardInput {
                    oracle_id: card.oracle_id.as_deref().unwrap_or(""),
                    name: &card.name,
                    layout: &card.layout,
                    faces: vec![FaceInput {
                        name: &face.name,
                        type_line: tl,
                        oracle_text: face.oracle_text.as_deref().unwrap_or(""),
                    }],
                },
                &compiler_vocab,
            );
            if !ast.strictly_compiled {
                continue;
            }
            let f = &ast.faces[0];
            let mut all_ins: Vec<&InstructionAst> = f.spell_effects.iter().collect();
            let mut leaf_ins: Vec<&InstructionAst> = Vec::new();
            collect_leaf_ins(&f.spell_effects, &mut leaf_ins);
            for ab in &f.abilities {
                match ab {
                    AbilityAst::Activated { body, .. }
                    | AbilityAst::Triggered { body, .. }
                    | AbilityAst::SagaChapter { body, .. }
                    | AbilityAst::Loyalty { body, .. } => {
                        all_ins.extend(body.iter());
                        collect_leaf_ins(body, &mut leaf_ins);
                    }
                    _ => {}
                }
            }
            match reason {
                "oracle:engine-validation-failed" => {
                    if let Ok(program) =
                        lower_face(card, face, f, &vocabulary, FaceLoweringOptions::default())
                        && let Err(e) = program.validate(&vocabulary)
                    {
                        val_fail_errs
                            .entry(format!("{e:?}"))
                            .or_default()
                            .push(card.name.clone());
                    }
                }
                "oracle:complex-enchant-restriction" | "oracle:unsupported-enchant-target" => {
                    for st in &f.statics {
                        if let StaticAbilityAst::AttachmentRestriction {
                            filter: Some(ef), ..
                        } = st
                        {
                            *unsup_enchant.entry(format!("{ef:?}")).or_insert(0) += 1;
                        }
                    }
                }
                "oracle:unsupported-instruction" => {
                    let mut b = BodyBuilder::new();
                    let mut tmp_ids = LoweringIds::default();
                    for ins in &all_ins {
                        if let InstructionAst::AbilityRestriction { restriction, .. } = ins {
                            *unsup_ab_res.entry(restriction.clone()).or_insert(0) += 1;
                        }
                        if lower_instruction(
                            ins,
                            &mut b,
                            &vocabulary,
                            &mut tmp_ids,
                            false,
                            false,
                            None,
                        ) == Err("oracle:unsupported-instruction")
                        {
                            *unsup_ins.entry(variant_name(ins)).or_insert(0) += 1;
                        }
                    }
                    for ins in &leaf_ins {
                        let mut b2 = BodyBuilder::new();
                        let mut tmp_ids2 = LoweringIds::default();
                        if lower_instruction(
                            ins,
                            &mut b2,
                            &vocabulary,
                            &mut tmp_ids2,
                            false,
                            false,
                            None,
                        ) == Err("oracle:unsupported-instruction")
                        {
                            *unsup_leaf_ins.entry(variant_name(ins)).or_insert(0) += 1;
                            if let InstructionAst::PayCost(atom) = ins {
                                *unsup_pay_cost.entry(variant_name(atom)).or_insert(0) += 1;
                            }
                        }
                    }
                }
                "oracle:unsupported-activation-cost" => {
                    for ab in &f.abilities {
                        if let AbilityAst::Activated { cost, .. } = ab {
                            for atom in cost {
                                *unsup_act_cost.entry(variant_name(atom)).or_insert(0) += 1;
                            }
                        }
                    }
                }
                "oracle:unsupported-exile-gy" => {
                    for ins in &leaf_ins {
                        if let InstructionAst::ExileGraveyard { whose } = ins {
                            *unsup_exile_gy.entry(whose.clone()).or_insert(0) += 1;
                        }
                    }
                }
                "oracle:unsupported-return-hand" => {
                    for ins in &leaf_ins {
                        if let InstructionAst::ReturnZone {
                            subject,
                            from_zone,
                            to,
                            ..
                        } = ins
                        {
                            let key = format!(
                                "{} from={:?} to={}",
                                variant_name(subject.as_ref()),
                                from_zone,
                                to
                            );
                            *unsup_ret_hand.entry(key).or_insert(0) += 1;
                        }
                    }
                }
                "oracle:unsupported-move-zone" => {
                    for ins in &leaf_ins {
                        if let InstructionAst::MoveZone { subject, to } = ins {
                            let key = format!("{} -> {}", variant_name(subject.as_ref()), to);
                            *unsup_move_zone.entry(key).or_insert(0) += 1;
                        }
                    }
                }
                "oracle:unsupported-effect-mana" => {
                    for ins in &leaf_ins {
                        if let InstructionAst::AddMana { mana, .. } = ins {
                            *unsup_eff_mana.entry(variant_name(mana)).or_insert(0) += 1;
                        }
                    }
                }
                "oracle:unsupported-value-expr" => {
                    for ins in &leaf_ins {
                        let dbg = format!("{ins:?}");
                        for tok in [
                            "Add(",
                            "Sub(",
                            "Mul(",
                            "Devotion",
                            "DomainCount",
                            "OpponentsCount",
                            "ColorsOfManaSpent",
                            "ColorsAmong",
                            "CardTypesAmong",
                            "SubtypesAmong",
                            "RollDie",
                            "PartySize",
                            "TotalCardsInAllHands",
                            "ManaSpentToCast",
                            "AggregateOverPlayers",
                            "StartingLifeTotal",
                            "SelfColorsCount",
                            "CounterCount",
                            "ObjectStat",
                            "PlayerStat",
                        ] {
                            if dbg.contains(tok) {
                                *unsup_val_expr.entry(tok.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
                "oracle:unsupported-if-condition" => {
                    for ins in &all_ins {
                        if let InstructionAst::IfCondition { condition, .. } = ins {
                            *unsup_if_cond
                                .entry(variant_name(condition.as_ref()))
                                .or_insert(0) += 1;
                            if let ConditionAst::Marker(m) = condition.as_ref() {
                                *unsup_marker.entry(variant_name(m)).or_insert(0) += 1;
                            }
                        }
                    }
                }
                "oracle:unsupported-granted-keyword" => {
                    for st in &f.statics {
                        if let StaticAbilityAst::GrantKeywords { keywords, .. } = st {
                            for k in keywords {
                                *unsup_grant_kw.entry(k.to_ascii_lowercase()).or_insert(0) += 1;
                            }
                        }
                    }
                    for ins in &leaf_ins {
                        if let InstructionAst::GrantKeywords { keywords, .. } = ins {
                            for k in keywords {
                                *unsup_grant_kw.entry(k.to_ascii_lowercase()).or_insert(0) += 1;
                            }
                        }
                    }
                }
                "oracle:unsupported-keyword" => {
                    for (kw_idx, kw) in f.keywords.iter().enumerate() {
                        let mut p = Program::enchantment(None);
                        let mut tmp_ids = LoweringIds::default();
                        if lower_keyword(kw, kw_idx, f, &mut p, &vocabulary, &mut tmp_ids, false)
                            == Err("oracle:unsupported-keyword")
                        {
                            *unsup_kw.entry(kw.name.to_ascii_lowercase()).or_insert(0) += 1;
                        }
                    }
                }
                "oracle:unsupported-static" => {
                    for st in &f.statics {
                        *unsup_static.entry(variant_name(st)).or_insert(0) += 1;
                    }
                }
                "oracle:unsupported-rule-restriction" => {
                    for st in &f.statics {
                        if let StaticAbilityAst::RuleOrCombatRestriction { rule, .. } = st {
                            *unsup_rule.entry(rule.clone()).or_insert(0) += 1;
                        }
                    }
                }
                "oracle:unsupported-static-cond" => {
                    for st in &f.statics {
                        match st {
                            StaticAbilityAst::ContinuousModifyPT {
                                condition: Some(c), ..
                            }
                            | StaticAbilityAst::GrantKeywords {
                                condition: Some(c), ..
                            }
                            | StaticAbilityAst::GrantAbilityText {
                                condition: Some(c), ..
                            } => {
                                *unsup_static_cond
                                    .entry(variant_name(c.as_ref()))
                                    .or_insert(0) += 1;
                                if let ConditionAst::PlayerActionThisTurn { action, .. } =
                                    c.as_ref()
                                {
                                    *unsup_pat_action.entry(action.clone()).or_insert(0) += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "oracle:unsupported-replacement" => {
                    for rep in &f.replacements {
                        *unsup_rep.entry(variant_name(rep)).or_insert(0) += 1;
                    }
                }
                "oracle:unsupported-trigger-event" => {
                    for ab in &f.abilities {
                        if let AbilityAst::Triggered { trigger, .. } = ab {
                            for ev in &trigger.events {
                                *unsup_trig_ev.entry(variant_name(ev)).or_insert(0) += 1;
                                if let TriggerEventAst::KeywordActionPerformed { action, .. } = ev {
                                    *unsup_kw_action
                                        .entry(action.to_ascii_lowercase())
                                        .or_insert(0) += 1;
                                }
                            }
                        }
                    }
                }
                "oracle:unsupported-trigger-condition" => {
                    for ab in &f.abilities {
                        if let AbilityAst::Triggered { trigger, .. } = ab
                            && let Some(c) = &trigger.intervening_if
                        {
                            *unsup_trig_cond.entry(variant_name(c)).or_insert(0) += 1;
                            if let ConditionAst::Marker(m) = c {
                                *unsup_marker.entry(variant_name(m)).or_insert(0) += 1;
                            }
                            if let ConditionAst::PlayerActionThisTurn { action, .. } = c {
                                *unsup_pat_action.entry(action.clone()).or_insert(0) += 1;
                            }
                        }
                    }
                }
                "oracle:non-self-damage-source" => {
                    for ins in all_ins {
                        if let InstructionAst::DealDamage { source, .. } = ins {
                            *unsup_dmg_src
                                .entry(variant_name(source.as_ref()))
                                .or_insert(0) += 1;
                        }
                    }
                }
                "oracle:unsupported-modify-pt-subject" => {
                    for ins in all_ins {
                        if let InstructionAst::ModifyPT { subject, .. } = ins {
                            *unsup_pt_subj
                                .entry(variant_name(subject.as_ref()))
                                .or_insert(0) += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        fn print_top<K: std::fmt::Display>(title: &str, map: HashMap<K, usize>, n: usize) {
            let mut v: Vec<(K, usize)> = map.into_iter().collect();
            v.sort_by_key(|e| std::cmp::Reverse(e.1));
            eprintln!("--- {title} ---");
            for (k, c) in v.into_iter().take(n) {
                eprintln!("  {c:5}  {k}");
            }
        }
        print_top("unsupported-instruction by AST variant", unsup_ins, 30);
        print_top(
            "unsupported-instruction LEAF by AST variant",
            unsup_leaf_ins,
            30,
        );
        print_top("unsupported PayCost atoms", unsup_pay_cost, 15);
        print_top(
            "unsupported KeywordActionPerformed actions",
            unsup_kw_action,
            20,
        );
        print_top("unsupported GameMarker conditions", unsup_marker, 20);
        print_top("multi-face / non-normal by layout", multi_face_layout, 15);
        print_top(
            "unsupported-activation-cost by CostAtomAst",
            unsup_act_cost,
            15,
        );
        print_top("unsupported-exile-gy by whose", unsup_exile_gy, 15);
        print_top("unsupported-return-hand by subject/to", unsup_ret_hand, 15);
        print_top("unsupported-move-zone by subject/to", unsup_move_zone, 15);
        print_top("unsupported-effect-mana by ManaSpecAst", unsup_eff_mana, 12);
        print_top("unsupported-value-expr by token", unsup_val_expr, 15);
        print_top(
            "unsupported-if-condition by ConditionAst",
            unsup_if_cond,
            15,
        );
        print_top("unsupported-granted-keyword by keyword", unsup_grant_kw, 20);
        print_top("unsupported-keyword by name", unsup_kw, 25);
        print_top("unsupported-static by AST variant", unsup_static, 15);
        print_top("unsupported-rule-restriction by rule", unsup_rule, 15);
        print_top(
            "unsupported-static-cond by AST variant",
            unsup_static_cond,
            15,
        );
        print_top("unsupported-replacement by AST variant", unsup_rep, 15);
        print_top(
            "unsupported-trigger-event by AST variant",
            unsup_trig_ev,
            15,
        );
        print_top(
            "unsupported-trigger-condition by AST variant",
            unsup_trig_cond,
            15,
        );
        print_top("unsupported-subtype by token", unsup_sub, 15);
        print_top("non-self-damage-source by SubjectRefAst", unsup_dmg_src, 10);
        print_top(
            "unsupported-modify-pt-subject by SubjectRefAst",
            unsup_pt_subj,
            10,
        );
        print_top(
            "PlayerActionThisTurn top unsupported actions",
            unsup_pat_action,
            25,
        );
        print_top("AbilityRestriction top labels", unsup_ab_res, 20);
        print_top("complex-enchant top filters", unsup_enchant, 15);
        let mut val_sorted: Vec<(String, Vec<String>)> = val_fail_errs.into_iter().collect();
        val_sorted.sort_by_key(|e| std::cmp::Reverse(e.1.len()));
        eprintln!("--- engine-validation-failed by DefinitionError ---");
        for (err, names) in val_sorted.into_iter().take(15) {
            let sample: Vec<&str> = names.iter().take(6).map(String::as_str).collect();
            eprintln!("  {:5}  {err}  (e.g. {:?})", names.len(), sample);
        }

        eprintln!(
            "Legacy table overlap: avacyn_restored {avr_covered}/{avr_total}, innistrad {isd_covered}/{isd_total}"
        );
    }
}
