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
    names::EnglishCardName,
};
use spellwake_engine::{
    activated::{
        ActivatedAbility, ActivatedAbilityId, ActivationRestriction, CounterCostDelta,
        DiscardCostMode, HandDiscardKeyword, LifeCostAmount, PowerAbility, SelectedSacrificeFilter,
        SourceCost, TapCostCount, TapCostFilter,
    },
    attachment::{AttachAbility, AttachmentKind, EnchantRestriction},
    characteristics::{
        AbilityChange, AbilityRemoval, BlockingRestriction, CastPermissionRule, Color, ColorFilter,
        ControllerSelector, DefiningPart, DefiningParts, KeywordAbility, NumericExpression,
        NumericPair, PermanentSelector, PowerToughness, PowerToughnessCounters,
        PrintedPowerToughness, PrintedStat, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        SpellCostModifier, SpellDiscount, StaticAbilityId, StaticContinuousAbility, StaticDrawRule,
        StaticModifierRule, StaticPart, StaticParts, StaticPlayerStat, StaticRestriction,
        StaticUntapRule, StaticZone,
    },
    damage::TurnPreventionFilter,
    effects::CounterKind,
    face_down::FaceUpAbility,
    land::{BasicLandType, EntryTap, LandType},
    life::{LifeAdjustment, LifeChangeKind},
    mana::{
        ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaOutputModifier, ManaRestriction,
        ManaSacrificeCost, ManaSideEffect, ManaType, SpecialColorYield,
    },
    program::{
        AdditionalCastCost, AffectedPlayers, ArtifactSubtype, AttackRequirement, AttackTriggerMode,
        BlockTriggerKind, BodyTarget, CardType, CardTypeMask, CastCopyKeyword, CastCostCount,
        CastReduction, ColorMask, CombatFilter, CombatTriggerKeyword, CombatUntapGroup,
        ConditionalCastCost, ControllerFilter, CounterKeyword, CounterTriggerKind, DamageGroup,
        DamageTriggerRecipient, DiesTriggerMode, EffectManaYield, EffectSubject,
        EnchantmentSubtype, EndStepCast, EntryCounterAbility, EventValueKind, ExecutableBody,
        ExileAction, Instruction, InterveningCondition, LibraryPlacement, LibrarySearch,
        ManaValueFilter, ObjectClause, ObjectColorFilter, ObjectFilter, ObjectFilterFlags,
        ObjectStatKind, PermanentDefinition, PermanentTargetAction, PermanentTypeFilter,
        PhasingSubject, PlanarTriggerEvent, PlayerRelation, PlayerStatExpr, PlayerTriggerAction,
        PowerFilter, Program, ProgramKind, PublicZoneDestination, ResolvingPlayer,
        SearchCardMatcher, SearchCount, SearchDestination, SearchExileLink, SearchManaValue,
        SearchSource, SourceRelation, SpellDefinition, SpellFilter, SpellModes, SpellSubtype,
        StatAggregate, StatusTriggerKind, SubtypeFilter, Supertype, SupertypeMask, ToughnessFilter,
        TriggerCondition, TriggerEvent, TriggerFromZone, TriggerGrouping, TriggerKeywordEvent,
        TriggerPlayerScope, TriggerSpec, TriggeredAbility, TriggeredAbilityId, UpkeepKeyword,
        ValueExpr, ValueObjectRef,
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

/// Strictly compile a single-face normal card via `spellwake-oracle-compiler` and lower it
/// to a validated native [`Program`].
pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    compile_with_reason(card, vocabulary).ok()
}

/// Diagnostic entry point returning a concise static reason when a card cannot be lowered.
pub(crate) fn compile_with_reason(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Program, &'static str> {
    let [face] = card.faces.as_slice() else {
        return Err("oracle:multi-face");
    };
    if card.layout != "normal" || face.name != card.name {
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
    let program = lower_face(card, face, face_ast, vocabulary)?;
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
    is_aura: bool,
    is_equipment: bool,
    is_fortification: bool,
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
        is_aura: false,
        is_equipment: false,
        is_fortification: false,
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
            "Planeswalker" | "Battle" => {
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
            i += 1;
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

fn parse_mana_cost_with_x(raw: &str) -> Option<(ManaCost, u8)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut cost = ManaCost::free();
    let mut x_count: u8 = 0;
    let symbols = trimmed.strip_prefix('{')?.strip_suffix('}')?;
    for sym in symbols.split("}{") {
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
    Some((cost, x_count))
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

    let (casting_cost, x_symbols) = if is_land {
        if face.mana_cost.as_deref().is_some_and(|s| !s.is_empty()) {
            return Err("oracle:land-with-mana-cost");
        }
        (None, 0)
    } else {
        let raw_cost = face.mana_cost.as_deref().unwrap_or("");
        if raw_cost.is_empty() {
            return Err("oracle:missing-mana-cost");
        }
        let (cost, x) = parse_mana_cost_with_x(raw_cost).ok_or("oracle:unsupported-mana-cost")?;
        (Some(cost), x)
    };

    let mut ids = LoweringIds::default();
    let mut program = if is_spell {
        let cost = casting_cost.ok_or("oracle:spell-without-cost")?;
        let (body, modes) = lower_spell_effects(&face_ast.spell_effects, vocabulary, &mut ids)?;
        let mut p = Program::new(
            types.card_types.clone(),
            types.creature_subtypes.clone(),
            BTreeSet::new(),
            ProgramKind::Spell(SpellDefinition { cost, body, modes }),
        );
        p.spell_subtypes = types.spell_subtypes;
        p.supertypes = types.supertypes;
        p.x_mana_symbols = x_symbols;
        p
    } else {
        if !face_ast.spell_effects.is_empty() {
            return Err("oracle:permanent-with-spell-effects");
        }
        let printed_pt = if is_creature {
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
            let restriction = find_aura_restriction(&face_ast.statics)?;
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
        p.supertypes = types.supertypes;
        p.x_mana_symbols = x_symbols;
        p
    };

    // Lower keywords
    for kw in &face_ast.keywords {
        lower_keyword(kw, &mut program, vocabulary, &mut ids, is_spell)?;
    }

    // Lower replacement effects
    for rep in &face_ast.replacements {
        lower_replacement(rep, &mut program, vocabulary)?;
    }

    // Lower static abilities
    for st in &face_ast.statics {
        lower_static(st, &mut program, vocabulary, &mut ids, is_spell)?;
    }

    // Lower abilities (activated + triggered)
    for ab in &face_ast.abilities {
        lower_ability(ab, &mut program, vocabulary, &mut ids)?;
    }

    Ok(program)
}

fn find_aura_restriction(statics: &[StaticAbilityAst]) -> Result<EnchantRestriction, &'static str> {
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
                found = Some(lower_enchant_restriction(f)?);
            } else {
                return Err("oracle:empty-enchant");
            }
        }
    }
    found.ok_or("oracle:aura-missing-enchant")
}

fn lower_enchant_restriction(f: &ObjectFilterAst) -> Result<EnchantRestriction, &'static str> {
    if !f.required_supertypes.is_empty()
        || !f.forbidden_supertypes.is_empty()
        || !f.subtypes.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.any_colors.is_empty()
        || f.color_arity.is_some()
        || f.zone.is_some()
        || f.combat.is_some()
        || f.tapped.is_some()
        || f.token.is_some()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.stat_bounds.is_empty()
        || f.counter_kind.is_some()
        || !f.flags.is_empty()
    {
        return Err("oracle:complex-enchant-restriction");
    }
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
        if !f.forbidden_colors.is_empty() {
            return Err("oracle:enchant-color-restriction");
        }
        return match f.controller {
            None => Ok(EnchantRestriction::Creature),
            Some(ControllerRelation::You) => Ok(EnchantRestriction::CreatureYouControl),
            Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                Ok(EnchantRestriction::CreatureAnOpponentControls)
            }
            _ => Err("oracle:enchant-controller-restriction"),
        };
    }
    if !f.forbidden_colors.is_empty() || !f.forbidden_types.is_empty() || f.controller.is_some() {
        return Err("oracle:complex-enchant-restriction");
    }
    if f.any_types == AstCardTypeMask::ARTIFACT {
        Ok(EnchantRestriction::Artifact)
    } else if f.any_types == AstCardTypeMask::ENCHANTMENT {
        Ok(EnchantRestriction::Enchantment)
    } else if f.any_types == AstCardTypeMask::LAND {
        Ok(EnchantRestriction::Land)
    } else {
        Err("oracle:unsupported-enchant-target")
    }
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
        _ => None,
    }
}

fn lower_keyword(
    kw: &KeywordAst,
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
        "protection" => {
            let p = param.ok_or("oracle:protection-no-param")?;
            let stripped = p.strip_prefix("from ").unwrap_or(p);
            for part in stripped.split(" and ") {
                let word = part.trim().strip_prefix("from ").unwrap_or(part.trim());
                let ka = match word.to_ascii_lowercase().as_str() {
                    "white" => KeywordAbility::ProtectionFromWhite,
                    "blue" => KeywordAbility::ProtectionFromBlue,
                    "black" => KeywordAbility::ProtectionFromBlack,
                    "red" => KeywordAbility::ProtectionFromRed,
                    "green" => KeywordAbility::ProtectionFromGreen,
                    "multicolored" => KeywordAbility::ProtectionFromMulticolored,
                    _ => return Err("oracle:unsupported-protection"),
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
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: TriggerSpec::new(TriggerEvent::Keyword(TriggerKeywordEvent::Evolve)),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::ResolveGrowth(
                        spellwake_engine::program::GrowthKeyword::Evolve,
                    )],
                ),
            });
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
        _ => Err("oracle:unsupported-keyword"),
    }
}

fn lower_replacement(
    rep: &ReplacementEffectAst,
    program: &mut Program,
    _vocabulary: &CreatureTypeVocabulary,
) -> Result<(), &'static str> {
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
            per: None,
            x_expr: None,
            condition,
        } => {
            let ck = counter_kind.to_ascii_lowercase();
            match (ck.as_str(), count.as_ref(), condition.as_deref()) {
                ("+1/+1", ValueExprAst::Const(n), None) if *n > 0 => {
                    program
                        .entry_counter_abilities
                        .push(EntryCounterAbility::FixedPlusOne(i64::from(*n).into()));
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
                _ => Err("oracle:unsupported-entry-counters"),
            }
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
                _ => Err("oracle:unsupported-entry-tapped-counters"),
            }
        }
        ReplacementEffectAst::AsEntersChoose { choice } => {
            if choice.eq_ignore_ascii_case("a creature type")
                || choice.eq_ignore_ascii_case("creature type")
            {
                if def
                    .entry_tap
                    .replace(EntryTap::ChooseCreatureType)
                    .is_some()
                {
                    return Err("oracle:duplicate-entry-tap");
                }
                Ok(())
            } else {
                Err("oracle:unsupported-as-enters-choose")
            }
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
            grant_ability_text: None,
            scale,
            condition,
            paired_rule,
        } => {
            let mut selector = lower_permanent_selector(subject, vocabulary)?;
            if let Some(cond) = condition {
                apply_selector_condition(&mut selector, cond, vocabulary)?;
            }
            let mut parts = Vec::new();
            let (p_expr, t_expr) = lower_pt_delta_pair(dx, dy, scale.as_deref(), vocabulary)?;
            parts.push(StaticPart::ModifyPT(NumericPair {
                power: p_expr,
                toughness: t_expr,
            }));
            if !grant_keywords.is_empty() {
                let kws = lower_keyword_list(grant_keywords)?;
                parts.push(StaticPart::Abilities(AbilityChange::grant_keywords(kws)));
            }
            if let Some(paired) = paired_rule {
                match paired.as_ref() {
                    StaticAbilityAst::RuleOrCombatRestriction { rule, .. } => {
                        if let Some(br) = lower_blocking_restriction(rule) {
                            // Note: BlockingRestriction shares Abilities bucket with Abilities!
                            if !grant_keywords.is_empty() {
                                push_static_ability(
                                    program,
                                    StaticContinuousAbility::Battlefield {
                                        id: ids.static_id(),
                                        selector: selector.clone(),
                                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                            br,
                                        )]),
                                    },
                                )?;
                            } else {
                                parts.push(StaticPart::BlockingRestriction(br));
                            }
                        } else {
                            return Err("oracle:unsupported-paired-static");
                        }
                    }
                    _ => return Err("oracle:unsupported-paired-static"),
                }
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
        StaticAbilityAst::GrantKeywords {
            subject,
            keywords,
            condition,
            paired_rule: None,
        } => {
            let mut selector = lower_permanent_selector(subject, vocabulary)?;
            if let Some(cond) = condition {
                apply_selector_condition(&mut selector, cond, vocabulary)?;
            }
            let kws = lower_keyword_list(keywords)?;
            if kws.is_empty() {
                return Err("oracle:empty-grant-keywords");
            }
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ids.static_id(),
                    selector,
                    parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
                        kws,
                    ))]),
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
            condition: None,
        } => {
            // Self spell cost reduction: "This spell costs {N} less to cast for each ..."
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) {
                if let Some(per_filter) = per {
                    let count = lower_cast_cost_count(per_filter)?;
                    if delta.trim() != "{1}" {
                        return Err("oracle:unsupported-self-discount-delta");
                    }
                    program
                        .cast_reductions
                        .push(CastReduction::PerObject(count));
                    return Ok(());
                }
                return Err("oracle:unsupported-self-discount");
            }
            // Battlefield permanent discounting spells
            if is_spell || per.is_some() {
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
            condition: None,
        } => {
            if rule == "CantBeCountered"
                && filter.is_none()
                && matches!(subject.as_deref(), None | Some(SubjectRefAst::SelfObject))
            {
                program.cannot_be_countered = true;
                return Ok(());
            }
            if rule == "DoesntUntap" && filter.is_none() {
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
            if rule == "UntapEachOtherUntapStep"
                && filter.is_none()
                && matches!(subject.as_deref(), None | Some(SubjectRefAst::SelfObject))
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::UntapRule {
                        id: ids.static_id(),
                        rule: StaticUntapRule::UntapDuringOtherPlayersUntap,
                    },
                );
            }
            if rule == "CantAttackYouOrBlockYourCreatures" && filter.is_none() {
                if program
                    .attack_requirement
                    .replace(AttackRequirement::EnchantedCannotAttackOrBlockController)
                    .is_some()
                {
                    return Err("oracle:duplicate-attack-req");
                }
                return Ok(());
            }
            if rule == "CanBlockOnly"
                && let Some(flt) = filter
                && flt.required_keywords == ["flying"]
                && flt.any_types == AstCardTypeMask::CREATURE
            {
                let selector = match subject.as_deref() {
                    Some(s) => lower_permanent_selector(s, vocabulary)?,
                    None => PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    },
                };
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CanBlockOnlyFlying,
                        )]),
                    },
                );
            }
            if filter.is_some() {
                return Err("oracle:unsupported-rule-restriction");
            }
            let r_low = rule.to_ascii_lowercase();
            if r_low.contains("can't be countered")
                && matches!(subject.as_deref(), None | Some(SubjectRefAst::SelfObject))
            {
                program.cannot_be_countered = true;
                return Ok(());
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
            if r_low.contains("doesn't untap during its controller's untap step") {
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
            if r_low.contains("attacks each combat if able")
                || r_low.contains("attacks each turn if able")
            {
                let req = match subject.as_deref() {
                    Some(SubjectRefAst::SelfObject) | None => AttackRequirement::SourceAttacks,
                    Some(SubjectRefAst::AttachedHost(_)) => {
                        AttackRequirement::AttachedCreatureAttacks
                    }
                    _ => return Err("oracle:unsupported-attack-req"),
                };
                if program.attack_requirement.replace(req).is_some() {
                    return Err("oracle:duplicate-attack-req");
                }
                return Ok(());
            }
            if let Some(br) = lower_blocking_restriction(rule) {
                let selector = match subject.as_deref() {
                    Some(s) => lower_permanent_selector(s, vocabulary)?,
                    None => PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    },
                };
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(br)]),
                    },
                );
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

fn lower_blocking_restriction(rule: &str) -> Option<BlockingRestriction> {
    match rule.to_ascii_lowercase().as_str() {
        "can't block" | "cant_block" | "cantblock" => Some(BlockingRestriction::CannotBlock),
        "can't attack or block" | "cant_attack_or_block" | "cantattackorblock" => {
            Some(BlockingRestriction::CannotAttackOrBlock)
        }
        "can't be blocked" | "cant_be_blocked" | "cantbeblocked" | "unblockable" => {
            Some(BlockingRestriction::CannotBeBlocked)
        }
        "can block only creatures with flying" | "only_block_flying" => {
            Some(BlockingRestriction::CanBlockOnlyFlying)
        }
        "must be blocked if able" | "must_be_blocked" | "mustbeblocked" => {
            Some(BlockingRestriction::MustBeBlocked)
        }
        "allableblock" => Some(BlockingRestriction::AllDefendingCreaturesMustBlock),
        "can block an additional creature each combat" | "additional_block" => {
            Some(BlockingRestriction::AdditionalBlock)
        }
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
        if f.controller.is_none() && f.combat.is_none() && f.flags.is_empty() {
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
        || !f.required_keywords.is_empty() && f.required_keywords != ["flying"]
        || !f.forbidden_keywords.is_empty()
        || !f.stat_bounds.is_empty()
        || f.counter_kind.is_some()
        || f.any_spell
        || f.is_ability
    {
        return Err("oracle:unsupported-selector-predicate");
    }
    let mut sel = PermanentSelector::default();
    if f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0 {
        sel.exclude_source = true;
    }
    if f.flags.0 & AstFilterFlags::WITH_ANY_COUNTER.0 != 0 {
        sel.any_counter = true;
    }
    if f.flags.0 & AstFilterFlags::EQUIPPED.0 != 0 {
        sel.equipped_by_at_least = 1;
    }
    if f.flags.0 & AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0 != 0 {
        sel.matches_source_entry_type = true;
    }
    let unsupported_flags = f.flags.0
        & !(AstFilterFlags::EXCLUDE_SELF.0
            | AstFilterFlags::WITH_ANY_COUNTER.0
            | AstFilterFlags::EQUIPPED.0
            | AstFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR.0);
    if unsupported_flags != 0 {
        return Err("oracle:unsupported-selector-flag");
    }

    if f.required_keywords == ["flying"] {
        sel.requires_flying = true;
    }
    sel.token = f.token;
    match f.tapped {
        None => {}
        Some(false) => sel.requires_untapped = true,
        Some(true) => return Err("oracle:selector-tapped-unsupported"),
    }
    match f.combat {
        None => {}
        Some(CombatState::Attacking) => {
            sel.combat_role = Some(spellwake_engine::characteristics::CombatSelector::Attacking);
        }
        _ => return Err("oracle:selector-combat-unsupported"),
    }
    // Types: PermanentSelector only supports `required_types` (conjunctive), so allow at most 1 bit in any_types
    let mut any_types_set = card_type_mask_to_set(f.any_types)?;
    if any_types_set.len() > 1 {
        return Err("oracle:selector-disjunctive-types");
    }
    if let Some(t) = any_types_set.pop_first() {
        sel.required_types.insert(t);
    }
    sel.forbidden_types = card_type_mask_to_set(f.forbidden_types)?;
    sel.required_supertypes = supertype_mask_to_set(f.required_supertypes)?;
    sel.forbidden_supertypes = supertype_mask_to_set(f.forbidden_supertypes)?;

    // Subtypes
    if f.subtypes.len() > 1 || f.forbidden_subtypes.len() > 1 {
        return Err("oracle:selector-multi-subtypes");
    }
    if let Some(st) = f.subtypes.first() {
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
            ColorArity::Colored => Err("oracle:selector-colored-unsupported"),
        };
    }
    if !any.is_empty() && !forbidden.is_empty() {
        return Err("oracle:mixed-color-filter");
    }
    if !any.is_empty() {
        let c = single_color_from_mask(any).ok_or("oracle:multi-color-selector")?;
        return Ok(ColorFilter::Has(c));
    }
    if !forbidden.is_empty() {
        let c = single_color_from_mask(forbidden).ok_or("oracle:multi-forbidden-color-selector")?;
        return Ok(ColorFilter::Lacks(c));
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
        } if filter.subtypes.len() == 1 => {
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
        } if *value > 0 && filter.subtypes.is_empty() => {
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
        _ => Err("oracle:unsupported-static-cond"),
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
            _ => Err("oracle:unsupported-player-stat"),
        },
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
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let kind = if counter_kind.eq_ignore_ascii_case("any") {
                None
            } else {
                Some(parse_counter_kind(counter_kind).ok_or("oracle:unsupported-counter-kind")?)
            };
            Ok(ValueExpr::CounterCount {
                subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
                kind,
            })
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
        ValueExprAst::PriorResult => {
            Ok(ValueExpr::EventValue(EventValueKind::CombatDamageToPlayer))
        }
        _ => Err("oracle:unsupported-value-expr"),
    }
}

fn parse_counter_kind(raw: &str) -> Option<CounterKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "+1/+1" => Some(CounterKind::PlusOne),
        "-1/-1" => Some(CounterKind::MinusOne),
        "charge" => Some(CounterKind::Charge),
        "storage" => Some(CounterKind::Storage),
        "quest" => Some(CounterKind::Quest),
        "page" => Some(CounterKind::Page),
        "verse" => Some(CounterKind::Verse),
        "suspect" => Some(CounterKind::Suspect),
        "lore" => Some(CounterKind::Lore),
        "loyalty" => Some(CounterKind::Loyalty),
        "defense" => Some(CounterKind::Defense),
        "time" => Some(CounterKind::Time),
        "fade" => Some(CounterKind::Fade),
        "age" => Some(CounterKind::Age),
        "indestructible" => Some(CounterKind::Indestructible),
        "corpse" => Some(CounterKind::Corpse),
        "vow" => Some(CounterKind::Vow),
        "blessing" => Some(CounterKind::Blessing),
        "despair" => Some(CounterKind::Despair),
        "hatchling" => Some(CounterKind::Hatchling),
        "study" => Some(CounterKind::Study),
        "eyeball" => Some(CounterKind::Eyeball),
        "slime" => Some(CounterKind::Slime),
        "poison" => Some(CounterKind::Poison),
        "experience" => Some(CounterKind::Experience),
        _ => None,
    }
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
            let (exec_body, restriction) = lower_activated_body(body, vocabulary, ids)?;
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
            let (spec, grouping, guard) = lower_trigger_spec(trigger, vocabulary)?;
            let mut exec_body = lower_effect_body(body, vocabulary, ids, true)?;
            if let Some(cond) = guard {
                exec_body
                    .instructions
                    .insert(0, Instruction::RequireCondition(cond));
            }
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: spec,
                grouping,
                body: exec_body,
            });
            Ok(())
        }
        AbilityAst::Loyalty { .. }
        | AbilityAst::SagaChapter { .. }
        | AbilityAst::ClassLevel { .. } => Err("oracle:unsupported-ability-kind"),
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
                colors.insert(parse_single_mana_type(o)?);
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
        output = output.with_modifier(ManaOutputModifier::Sacrifice(ManaSacrificeCost::Source {
            tap: has_tap,
        }));
    } else if sac_creature && !has_tap {
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
            CostAtomAst::SacrificeSelf => sac_self = true,
            CostAtomAst::ExileSelf {
                from_graveyard,
                from_hand: false,
            } => {
                exile_self = true;
                exile_from_gy = *from_graveyard;
            }
            CostAtomAst::DiscardSelf => discard_self = true,
            CostAtomAst::DiscardHand => discard_mode = Some(DiscardCostMode::WholeHand),
            CostAtomAst::Discard {
                count,
                filter,
                random: false,
            } if matches!(count.as_ref(), ValueExprAst::Const(1))
                && filter.any_types.is_empty()
                && filter.forbidden_types.is_empty() =>
            {
                discard_mode = Some(DiscardCostMode::SelectedOne);
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
        + usize::from(discard_self)
        + usize::from(discard_mode.is_some())
        + usize::from(tap_perms.is_some())
        + usize::from(counter_cost.is_some())
        + usize::from(mill_count.is_some())
        + usize::from(x_symbols > 0);
    if non_mana_count > 1 {
        return Err("oracle:compound-activation-cost");
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
    let event = if events.len() == 1 {
        events.pop().ok_or("oracle:unreachable")?
    } else {
        TriggerEvent::AnyOf(events)
    };
    let mut spec = TriggerSpec::new(event);
    let mut guard = None;
    if let Some(cond) = &trig.intervening_if {
        let tc = lower_trigger_condition(cond)?;
        guard = InterveningCondition::from_trigger_condition(tc);
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
                (Some("battlefield"), None) => Ok((
                    TriggerEvent::LeavesBattlefield { subject: filter },
                    grouping,
                )),
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
            let mut sf = match spell {
                Some(f) => lower_spell_filter(f, vocabulary)?,
                None => SpellFilter::any(),
            };
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
                    min_mana_value: None,
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
            let src = match source {
                Some(s) => lower_trigger_subject_filter(s, vocabulary)?,
                None => ObjectFilter::any(),
            };
            let rec = match recipient.as_deref() {
                Some(SubjectRefAst::Player(PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer)) => {
                    DamageTriggerRecipient::AnyPlayer
                }
                Some(SubjectRefAst::Player(
                    PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
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
            tapped: true,
            for_mana: false,
        } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::BecomesTapped,
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

fn lower_trigger_condition(cond: &ConditionAst) -> Result<TriggerCondition, &'static str> {
    match cond {
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
    if let [InstructionAst::Modal { choice, modes }] = ins {
        let (min, max) = parse_modal_choice(choice)?;
        if modes.len() < 2 {
            return Err("oracle:modal-too-few");
        }
        let mut lowered = Vec::with_capacity(modes.len());
        for m in modes {
            lowered.push(lower_effect_body(&m.body, vocabulary, ids, false)?);
        }
        let first = lowered.remove(0);
        return Ok((
            first,
            Some(SpellModes {
                minimum: min,
                maximum: max,
                allow_repeat: false,
                additional: lowered,
            }),
        ));
    }
    let body = lower_effect_body(ins, vocabulary, ids, false)?;
    Ok((body, None))
}

fn parse_modal_choice(choice: &str) -> Result<(u8, u8), &'static str> {
    match choice.to_ascii_lowercase().trim() {
        "choose one" | "one" => Ok((1, 1)),
        "choose two" | "two" => Ok((2, 2)),
        "choose one or both" | "one or both" => Ok((1, 2)),
        "choose one or two" | "one or two" => Ok((1, 2)),
        "choose one or more" | "one or more" => Ok((1, 255)),
        _ => Err("oracle:unsupported-modal-choice"),
    }
}

fn lower_activated_body(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(ExecutableBody, Option<ActivationRestriction>), &'static str> {
    let mut restriction = None;
    let mut filtered = Vec::with_capacity(ins.len());
    for item in ins {
        if let InstructionAst::AbilityRestriction {
            restriction: r,
            condition: None,
        } = item
        {
            if restriction.is_some() {
                return Err("oracle:multiple-act-restrictions");
            }
            restriction = Some(parse_activation_restriction(r)?);
        } else {
            filtered.push(item.clone());
        }
    }
    let body = lower_effect_body(&filtered, vocabulary, ids, false)?;
    Ok((body, restriction))
}

fn parse_activation_restriction(raw: &str) -> Result<ActivationRestriction, &'static str> {
    match raw {
        "Sorcery" => return Ok(ActivationRestriction::Sorcery),
        "OnceEachTurn" => return Ok(ActivationRestriction::OnceEachTurn),
        "DuringUpkeep" => return Ok(ActivationRestriction::DuringUpkeep),
        "OwnTurnBeforeAttackers" => return Ok(ActivationRestriction::OwnTurnBeforeAttackers),
        _ => {}
    }
    let low = raw.to_ascii_lowercase();
    if low.contains("sorcery") {
        Ok(ActivationRestriction::Sorcery)
    } else if low.contains("once each turn") || low.contains("once per turn") {
        Ok(ActivationRestriction::OnceEachTurn)
    } else if low.contains("during your upkeep") {
        Ok(ActivationRestriction::DuringUpkeep)
    } else if low.contains("any player may activate") {
        Ok(ActivationRestriction::AnyPlayer)
    } else if low.contains("creature died this turn") {
        Ok(ActivationRestriction::CreatureDiedThisTurn)
    } else if low.contains("no cards in hand") {
        Ok(ActivationRestriction::EmptyHand)
    } else {
        Err("oracle:unsupported-act-restriction")
    }
}

fn lower_effect_body(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
) -> Result<ExecutableBody, &'static str> {
    let mut builder = BodyBuilder::new();
    lower_instruction_seq(ins, &mut builder, vocabulary, ids, in_trigger, None)?;
    builder.finish()
}

fn lower_instruction_seq(
    ins: &[InstructionAst],
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
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
            lower_instruction(&ins[i], builder, vocabulary, ids, in_trigger, bound_x)?;
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
        lower_instruction(&ins[i], builder, vocabulary, ids, in_trigger, bound_x)?;
        i += 1;
    }
    Ok(())
}

fn resolve_x<'a>(expr: &'a ValueExprAst, bound_x: Option<&'a ValueExprAst>) -> &'a ValueExprAst {
    if matches!(expr, ValueExprAst::ChosenX)
        && let Some(bx) = bound_x
    {
        return bx;
    }
    expr
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
                        if inner.is_empty() || inner.any_card {
                            Ok(BodyTarget::SingleOwnGraveyardCard)
                        } else {
                            Ok(BodyTarget::FilteredOwnGraveyardCard(lower_object_filter(
                                &inner, vocabulary,
                            )?))
                        }
                    }
                    ZoneFilter::AnyGraveyard => {
                        if inner.is_empty() || inner.any_card {
                            Ok(BodyTarget::AnyGraveyardCard)
                        } else if inner.any_types == AstCardTypeMask::CREATURE
                            && inner.forbidden_types.is_empty()
                            && inner.subtypes.is_empty()
                        {
                            Ok(BodyTarget::AnyGraveyardCreature)
                        } else if inner.any_types
                            == AstCardTypeMask::CREATURE.union(AstCardTypeMask::PLANESWALKER)
                            && inner.forbidden_types.is_empty()
                        {
                            Ok(BodyTarget::AnyGraveyardCreatureOrPlaneswalker)
                        } else {
                            Err("oracle:unsupported-any-gy-target")
                        }
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
        PlayerRefAst::ThatPlayer => {
            if matches!(
                builder.target,
                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
            ) {
                Ok(ResolvingPlayer::TargetPlayer)
            } else {
                Ok(ResolvingPlayer::UniqueRelatedPlayer)
            }
        }
        PlayerRefAst::ItsController => match builder.target {
            BodyTarget::SingleSpell(_) => Ok(ResolvingPlayer::TargetSpellController),
            _ => Err("oracle:unsupported-its-controller"),
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
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    match ins {
        InstructionAst::BindX { x_expr, body } => {
            lower_instruction_seq(body, builder, vocabulary, ids, in_trigger, Some(x_expr))
        }
        InstructionAst::Optional(inner) => {
            let start = builder.instructions.len();
            lower_instruction_seq(inner, builder, vocabulary, ids, in_trigger, bound_x)?;
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
            if !matches!(source.as_ref(), SubjectRefAst::SelfObject) {
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
                    let ValueExpr::Literal(n) = amt_expr else {
                        return Err("oracle:dynamic-damage-each-player");
                    };
                    builder.instructions.push(Instruction::DamageGroup {
                        group: DamageGroup::Players,
                        amount: n,
                    });
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::You) => {
                    let ValueExpr::Literal(n) = amt_expr else {
                        return Err("oracle:dynamic-damage-you");
                    };
                    builder.instructions.push(Instruction::DamageGroup {
                        group: DamageGroup::Controller,
                        amount: n,
                    });
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer) => {
                    builder
                        .instructions
                        .push(Instruction::DamageRelatedPlayerAmount(amt_expr));
                    Ok(())
                }
                SubjectRefAst::Each(f) | SubjectRefAst::All(f) => {
                    let ValueExpr::Literal(n) = amt_expr else {
                        return Err("oracle:dynamic-damage-group");
                    };
                    let group = lower_damage_group(f, vocabulary)?;
                    builder
                        .instructions
                        .push(Instruction::DamageGroup { group, amount: n });
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
            actor: None,
            subject,
        } => match subject.as_ref() {
            SubjectRefAst::SelfObject => {
                builder.instructions.push(Instruction::ExileAbilitySource);
                Ok(())
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } => {
                let bt = lower_target_subject(subject, vocabulary)?;
                builder.bind_target(bt)?;
                if filter.zone.is_some() {
                    builder.instructions.push(Instruction::ExileGraveyardTarget);
                } else {
                    builder.instructions.push(Instruction::ExileTarget);
                }
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
            if whose.eq_ignore_ascii_case("all") {
                builder.instructions.push(Instruction::ExileAllGraveyards);
                Ok(())
            } else {
                Err("oracle:unsupported-exile-gy")
            }
        }
        InstructionAst::ReturnZone {
            subject,
            from_zone: _,
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
                        builder.instructions.push(Instruction::ReturnSourceToHand);
                        Ok(())
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if !enters_tapped => {
                        let bt = lower_target_subject(subject, vocabulary)?;
                        builder.bind_target(bt)?;
                        if filter.zone == Some(ZoneFilter::YourGraveyard) {
                            builder
                                .instructions
                                .push(Instruction::ReturnTargetGraveyardCreatureToHand);
                        } else if filter.zone.is_none() {
                            builder.instructions.push(Instruction::ReturnToOwnersHand);
                        } else {
                            return Err("oracle:unsupported-return-hand-zone");
                        }
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
                    _ => Err("oracle:unsupported-return-hand"),
                }
            } else if to_bf {
                match subject.as_ref() {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let bt = lower_target_subject(subject, vocabulary)?;
                        builder.bind_target(bt)?;
                        match filter.zone {
                            Some(ZoneFilter::YourGraveyard) => {
                                if !enters_tapped
                                    && filter.any_types == AstCardTypeMask::CREATURE
                                    && filter.forbidden_types.is_empty()
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
                            Some(ZoneFilter::AnyGraveyard) if !enters_tapped => {
                                builder
                                    .instructions
                                    .push(Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker);
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
            let dest = to.to_ascii_lowercase();
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
            Err("oracle:unsupported-move-zone")
        }
        InstructionAst::Sacrifice { player, subject } => {
            let players = lower_affected_players(*player, builder)?;
            match subject.as_ref() {
                SubjectRefAst::SelfObject if matches!(player, PlayerRefAst::You) => {
                    builder
                        .instructions
                        .push(Instruction::SacrificeStateTriggerSource);
                    Ok(())
                }
                SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f) => {
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
        InstructionAst::Discard {
            player,
            count: Some(c),
            filter: None,
            anaphora: None,
            random,
        } => {
            let amt = lower_value_expr(resolve_x(c, bound_x), vocabulary)?;
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
                lower_library_search(library, count, filter, *reveal, to_zone, vocabulary)?;
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
        InstructionAst::ShuffleGraveyardIntoLibrary {
            player: PlayerRefAst::You,
        } => {
            builder
                .instructions
                .push(Instruction::ShuffleOwnGraveyardIntoLibrary);
            Ok(())
        }
        InstructionAst::TapPermanent { subject } => match subject.as_ref() {
            SubjectRefAst::SelfObject => {
                builder.instructions.push(Instruction::TapAbilitySource);
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
            SubjectRefAst::SelfObject => {
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
            SubjectRefAst::SelfObject => {
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
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            builder.instructions.push(Instruction::TransformSource);
            Ok(())
        }
        InstructionAst::PhaseOut { subject } => match subject.as_ref() {
            SubjectRefAst::SelfObject => {
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
            if matches!(a.as_ref(), SubjectRefAst::SelfObject) {
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
                    lower_modify_pt_target(subject, pt, dur, builder, vocabulary, in_trigger)
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
            let kws = lower_keyword_list(keywords)?;
            if kws.is_empty() {
                return Err("oracle:empty-grant-kw");
            }
            match subject.as_ref() {
                SubjectRefAst::SelfObject if dur == ResolvedDuration::UntilEndOfTurn => {
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
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                AbilityChange::grant_keywords(kws),
                            )]),
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
                        parts
                            .0
                            .push(ResolvedPartSpec::Abilities(AbilityChange::grant_keywords(
                                kws,
                            )));
                        return Ok(());
                    }
                    if let Some(Instruction::BoostTargetPower {
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
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                AbilityChange::grant_keywords(kws),
                            )]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if dur == ResolvedDuration::UntilEndOfTurn =>
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
            let ValueExpr::Literal(n) = amt else {
                return Err("oracle:dynamic-put-counters");
            };
            let n_usize = n.to_usize().ok_or("oracle:nonpos-counters")?;
            if n_usize == 0 {
                return Err("oracle:nonpos-counters");
            }
            match subject.as_ref() {
                SubjectRefAst::SelfObject => {
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
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
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
            if let Some(br) = lower_blocking_restriction(rule) {
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
        InstructionAst::AbandonSourceScheme => {
            builder.instructions.push(Instruction::AbandonSourceScheme);
            Ok(())
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
    {
        return Ok(DamageGroup::CreaturesAndPlaneswalkers);
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
) -> Result<(), &'static str> {
    match subject {
        SubjectRefAst::SelfObject if dur == ResolvedDuration::UntilEndOfTurn => {
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
        SubjectRefAst::SelfObject => {
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
        SubjectRefAst::All(f) | SubjectRefAst::Each(f)
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
    let search_count = if matcher == SearchCardMatcher::Any {
        SearchCount::Exact(NonZeroU8::new(n_u8).ok_or("oracle:search-zero")?)
    } else {
        SearchCount::UpTo(n_u8)
    };
    let dest_low = to_zone.to_ascii_lowercase();
    let destination = if dest_low.contains("hand") {
        SearchDestination::Hand
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
                (CompareOp::Le, ValueExprAst::ChosenX) => Some(SearchManaValue::AtMostAnnouncedX),
                _ => return Err("oracle:unsupported-search-mv"),
            };
        } else {
            return Err("oracle:unsupported-search-stat");
        }
    } else if !filter.stat_bounds.is_empty() {
        return Err("oracle:multi-search-stat");
    }
    Ok(ls)
}

fn lower_search_matcher(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SearchCardMatcher, &'static str> {
    if !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || !f.forbidden_supertypes.is_empty()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
    {
        return Err("oracle:complex-search-matcher");
    }
    if f.any_card && f.any_types.is_empty() && f.forbidden_types.is_empty() && f.subtypes.is_empty()
    {
        return Ok(SearchCardMatcher::Any);
    }
    if f.forbidden_types == AstCardTypeMask::LAND && f.any_types.is_empty() && f.subtypes.is_empty()
    {
        return Ok(SearchCardMatcher::Nonland);
    }
    if f.required_supertypes == AstSupertypeMask::BASIC
        && f.any_types == AstCardTypeMask::LAND
        && f.subtypes.is_empty()
    {
        return Ok(SearchCardMatcher::BasicLand);
    }
    if !f.required_supertypes.is_empty() || !f.forbidden_types.is_empty() {
        return Err("oracle:unsupported-search-supertype");
    }
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
        for st in &f.subtypes {
            let lt = parse_land_subtype(st).ok_or("oracle:unsupported-search-subtype")?;
            lands.insert(lt);
        }
        return Ok(SearchCardMatcher::LandSubtypes {
            subtypes: lands,
            require_land_type: f.any_types.0 & AstCardTypeMask::LAND.0 != 0,
        });
    }
    let types = card_type_mask_to_set(f.any_types)?;
    if !types.is_empty() {
        return Ok(SearchCardMatcher::CardTypes(types));
    }
    Ok(SearchCardMatcher::Any)
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
            if let Ok(prog) = lower_face(card, face, &ast.faces[0], &vocabulary)
                && let Err(e) = prog.validate(&vocabulary)
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
        eprintln!(
            "Legacy table overlap: avacyn_restored {avr_covered}/{avr_total}, innistrad {isd_covered}/{isd_total}"
        );
    }
}
