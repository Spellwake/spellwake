//! Printed Scryfall frame metadata lowering (`CatalogFace` -> base `Program`).
//!
//! This is the ONLY module in `v2_lowering` permitted to inspect Scryfall printed frame fields
//! (`type_line`, `mana_cost`, `power`, `toughness`, `color_indicator`, `loyalty`, `defense`).
//! It NEVER inspects `oracle_text`.

use std::collections::BTreeSet;
use std::num::NonZeroU8;

use spellwake_content::{CatalogFace, InventoryCard};
use spellwake_contracts::{
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    activated::ExtraActivationCost,
    attachment::AttachmentKind,
    characteristics::{Color, PrintedPowerToughness, PrintedStat},
    land::{EntryTap, LandType},
    mana::{HybridAlternative, HybridSymbol, ManaCost, ManaType},
    program::{
        ArtifactSubtype, BodyTarget, CardType, EnchantmentSubtype, ExecutableBody,
        PermanentDefinition, PlaneswalkerSubtype, Program, ProgramKind, SpellDefinition,
        SpellSubtype, Supertype,
    },
};

use crate::v2_lowering::error::{LowerResult, LoweringError};

/// Source-verified frame metadata options passed into single-face base program construction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FaceMetaOptions {
    pub printed_loyalty: Option<u16>,
    pub printed_defense: Option<u16>,
    pub color_indicator: Option<Vec<String>>,
    pub allow_empty_mana_cost: bool,
}

/// Structured breakdown of a printed `type_line`.
#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedTypeLine {
    pub supertypes: BTreeSet<Supertype>,
    pub card_types: BTreeSet<CardType>,
    pub creature_subtypes: BTreeSet<CreatureTypeId>,
    pub land_subtypes: BTreeSet<LandType>,
    pub artifact_subtypes: BTreeSet<ArtifactSubtype>,
    pub enchantment_subtypes: BTreeSet<EnchantmentSubtype>,
    pub spell_subtypes: BTreeSet<SpellSubtype>,
    pub planeswalker_subtypes: BTreeSet<PlaneswalkerSubtype>,
    pub is_aura: bool,
    pub is_equipment: bool,
    pub is_fortification: bool,
    pub is_vehicle: bool,
    pub is_siege: bool,
}

/// Build the skeleton [`Program`] for one printed card face from its frame metadata.
///
/// For spells, `SpellDefinition::body` starts empty and MUST be populated by `card.rs` from
/// `CompiledFaceV2::spell_effects`. For Auras, `PermanentDefinition::attachment` starts as `None`
/// and MUST be populated by `keywords.rs` from `KeywordKind::Enchant` before validation.
pub(crate) fn build_base_program(
    card: &InventoryCard,
    face: &CatalogFace,
    opts: &FaceMetaOptions,
    vocab: &CreatureTypeVocabulary,
) -> LowerResult<Program> {
    let type_line = face
        .type_line
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(LoweringError::InvalidMetadata("v2:meta:missing-type-line"))?;

    let types = parse_type_line(type_line, vocab)?;
    let is_spell = (types.card_types.contains(&CardType::Instant)
        || types.card_types.contains(&CardType::Sorcery))
        && !types.card_types.iter().any(|t| t.is_permanent());
    let is_land = types.card_types.contains(&CardType::Land);
    let is_creature = types.card_types.contains(&CardType::Creature);
    let is_planeswalker = types.card_types.contains(&CardType::Planeswalker);
    let is_battle = types.card_types.contains(&CardType::Battle);

    if is_planeswalker && opts.printed_loyalty.is_none() {
        return Err(LoweringError::InvalidMetadata("v2:meta:missing-loyalty"));
    }
    if is_battle && (!types.is_siege || opts.printed_defense.is_none()) {
        return Err(LoweringError::InvalidMetadata("v2:meta:unsupported-battle"));
    }

    let (casting_cost, x_symbols, hybrid_symbols, snow_mana_count) = if is_land {
        if face
            .mana_cost
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
        {
            return Err(LoweringError::InvalidMetadata("v2:meta:land-with-mana-cost"));
        }
        (None, 0u8, Vec::new(), 0u8)
    } else {
        let raw_cost = face.mana_cost.as_deref().unwrap_or("").trim();
        if raw_cost.is_empty() {
            // CR 118.6: Non-land faces with no printed mana cost (e.g. Suspend spells, transform
            // back faces, flip backs) are legal with `casting_cost: None` or
            // `uncastable_mana_cost: true`.
            let _ = opts.allow_empty_mana_cost;
            (None, 0u8, Vec::new(), 0u8)
        } else {
            let (cost, x, hybrids, snow) = parse_mana_cost_with_snow_str(raw_cost)?;
            (Some(cost), x, hybrids, snow)
        }
    };

    let mut program = if is_spell {
        let (cost, uncastable_mana_cost) = match casting_cost {
            Some(c) => (c, false),
            None => (ManaCost::free(), true),
        };
        let mut p = Program::new(
            types.card_types,
            types.creature_subtypes,
            BTreeSet::new(),
            ProgramKind::Spell(SpellDefinition {
                cost,
                uncastable_mana_cost,
                body: ExecutableBody::new(BodyTarget::None, Vec::new()),
                modes: None,
            }),
        );
        p.spell_subtypes = types.spell_subtypes;
        p
    } else {
        let is_spacecraft = types
            .artifact_subtypes
            .contains(&ArtifactSubtype::Spacecraft);
        let printed_pt = if is_creature
            || types.is_vehicle
            || (is_spacecraft && face.power.is_some() && face.toughness.is_some())
        {
            let p_raw = face
                .power
                .as_deref()
                .ok_or(LoweringError::InvalidMetadata("v2:meta:missing-power"))?;
            let t_raw = face
                .toughness
                .as_deref()
                .ok_or(LoweringError::InvalidMetadata("v2:meta:missing-toughness"))?;
            Some(PrintedPowerToughness {
                power: parse_printed_stat(p_raw)?,
                toughness: parse_printed_stat(t_raw)?,
            })
        } else {
            None
        };
        let attachment = if types.is_equipment {
            Some(AttachmentKind::Equipment)
        } else if types.is_fortification {
            Some(AttachmentKind::Fortification)
        } else {
            // Aura attachment restriction is wired by `keywords.rs` from `KeywordKind::Enchant`.
            None
        };
        let mut p = Program::new(
            types.card_types,
            types.creature_subtypes,
            types.land_subtypes,
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
        p.printed_loyalty = opts.printed_loyalty.map(|n| i64::from(n).into());
        p.printed_defense = opts.printed_defense.map(|n| i64::from(n).into());
        p
    };

    let face_name = if face.name.trim().is_empty() {
        card.name.clone()
    } else {
        face.name.clone()
    };
    program.names = ObjectNames::single(
        EnglishCardName::new(face_name)
            .map_err(|_| LoweringError::InvalidMetadata("v2:meta:bad-face-name"))?,
    );
    program.supertypes = types.supertypes;
    program.x_mana_symbols = x_symbols;
    program.hybrid_symbols = hybrid_symbols;

    if let Some(snow_nz) = NonZeroU8::new(snow_mana_count) {
        program
            .extra_cast_costs
            .push(ExtraActivationCost::PaySnowMana(snow_nz));
    }

    if let Some(ind) = opts.color_indicator.as_deref() {
        let colors = parse_color_indicator(ind)?;
        if !colors.is_empty() {
            program.color_indicator = Some(colors);
        }
    }

    Ok(program)
}

/// Parse a Scryfall printed mana cost string (e.g. `"{2}{W}{U}"`, `"{X}{R/G}"`, `"{2/W}"`, `"{B/P}"`)
/// into `(ManaCost, x_mana_symbols, hybrid_symbols)`.
pub(crate) fn parse_mana_cost_str(raw: &str) -> LowerResult<(ManaCost, u8, Vec<HybridSymbol>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(LoweringError::InvalidMetadata("v2:meta:empty-mana-cost"));
    }
    let inner = trimmed
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or(LoweringError::InvalidMetadata("v2:meta:bad-mana-cost"))?;

    let mut cost = ManaCost::free();
    let mut x_count: u8 = 0;
    let mut hybrids = Vec::new();

    for sym in inner.split("}{") {
        let sym = sym.trim();
        if sym.is_empty() {
            return Err(LoweringError::InvalidMetadata("v2:meta:bad-mana-cost"));
        }
        if let Some((first_s, second_s)) = sym.split_once('/') {
            // Reject triple-slash or Phyrexian hybrid symbols like `{G/U/P}` unless handled separately.
            if second_s.contains('/') {
                return Err(LoweringError::Unsupported("v2:meta:phyrexian-hybrid-mana"));
            }
            if first_s == "2" {
                let first = parse_colored_mana_code(second_s)?;
                hybrids.push(HybridSymbol {
                    first,
                    alternative: HybridAlternative::TwoGeneric,
                });
                add_required_mana(&mut cost, first, 1);
                continue;
            }
            if first_s == "C" && second_s == "P" {
                hybrids.push(HybridSymbol {
                    first: ManaType::Colourless,
                    alternative: HybridAlternative::TwoLife,
                });
                add_required_mana(&mut cost, ManaType::Colourless, 1);
                continue;
            }
            if first_s == "C" {
                let first = parse_colored_mana_code(second_s)?;
                hybrids.push(HybridSymbol {
                    first,
                    alternative: HybridAlternative::Mana(ManaType::Colourless),
                });
                add_required_mana(&mut cost, first, 1);
                continue;
            }
            let first = parse_colored_mana_code(first_s)?;
            let alternative = if second_s == "P" {
                HybridAlternative::TwoLife
            } else if second_s == "C" {
                HybridAlternative::Mana(ManaType::Colourless)
            } else {
                HybridAlternative::Mana(parse_colored_mana_code(second_s)?)
            };
            hybrids.push(HybridSymbol { first, alternative });
            add_required_mana(&mut cost, first, 1);
            continue;
        }
        match sym {
            "X" => {
                x_count = x_count
                    .checked_add(1)
                    .ok_or(LoweringError::InvalidMetadata("v2:meta:mana-x-overflow"))?;
            }
            "W" => add_required_mana(&mut cost, ManaType::White, 1),
            "U" => add_required_mana(&mut cost, ManaType::Blue, 1),
            "B" => add_required_mana(&mut cost, ManaType::Black, 1),
            "R" => add_required_mana(&mut cost, ManaType::Red, 1),
            "G" => add_required_mana(&mut cost, ManaType::Green, 1),
            "C" => add_required_mana(&mut cost, ManaType::Colourless, 1),
            "S" => return Err(LoweringError::Unsupported("v2:meta:snow-mana-cost")),
            _ => {
                if !sym.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(LoweringError::InvalidMetadata("v2:meta:unknown-mana-symbol"));
                }
                let n: i64 = sym
                    .parse()
                    .map_err(|_| LoweringError::InvalidMetadata("v2:meta:bad-generic-mana"))?;
                cost.generic = cost.generic.plus(&n.into());
            }
        }
    }

    Ok((cost, x_count, hybrids))
}

/// Parse a printed mana cost string, extracting any `{S}` snow mana symbols separately.
pub(crate) fn parse_mana_cost_with_snow_str(
    raw: &str,
) -> LowerResult<(ManaCost, u8, Vec<HybridSymbol>, u8)> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or(LoweringError::InvalidMetadata("v2:meta:bad-mana-cost"))?;

    let mut stripped = String::with_capacity(trimmed.len());
    let mut snow_count: u8 = 0;
    for sym in inner.split("}{") {
        if sym.eq_ignore_ascii_case("S") {
            snow_count = snow_count
                .checked_add(1)
                .ok_or(LoweringError::InvalidMetadata("v2:meta:snow-overflow"))?;
        } else {
            stripped.push('{');
            stripped.push_str(sym);
            stripped.push('}');
        }
    }

    if stripped.is_empty() {
        Ok((ManaCost::free(), 0, Vec::new(), snow_count))
    } else {
        let (cost, x, hybrids) = parse_mana_cost_str(&stripped)?;
        Ok((cost, x, hybrids, snow_count))
    }
}

/// Parse a Scryfall color indicator array (`["W", "U", ...]`) into engine [`Color`] values.
pub(crate) fn parse_color_indicator(ind: &[String]) -> LowerResult<BTreeSet<Color>> {
    let mut colors = BTreeSet::new();
    for c in ind {
        let color = match c.as_str() {
            "W" => Color::White,
            "U" => Color::Blue,
            "B" => Color::Black,
            "R" => Color::Red,
            "G" => Color::Green,
            _ => {
                return Err(LoweringError::InvalidMetadata(
                    "v2:meta:bad-color-indicator",
                ));
            }
        };
        colors.insert(color);
    }
    Ok(colors)
}

/// Parse a printed `type_line` string into typed supertypes, card types, and subtypes.
pub(crate) fn parse_type_line(
    line: &str,
    vocab: &CreatureTypeVocabulary,
) -> LowerResult<ParsedTypeLine> {
    let (types_part, subtypes_part) = line
        .split_once(" \u{2014} ")
        .or_else(|| line.split_once('\u{2014}'))
        .unwrap_or((line, ""));
    let mut parsed = ParsedTypeLine::default();

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
            _ => {
                return Err(LoweringError::InvalidMetadata("v2:meta:unknown-card-type"));
            }
        }
    }

    if parsed.card_types.is_empty() {
        return Err(LoweringError::InvalidMetadata("v2:meta:empty-card-types"));
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
                vocab.resolve_label(&format!("{} {}", sub_tokens[i], sub_tokens[i + 1]))
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
        if is_creature_family && let Some(ct) = vocab.resolve_label(tok) {
            parsed.creature_subtypes.insert(ct);
            i += 1;
            continue;
        }
        return Err(LoweringError::InvalidMetadata("v2:meta:unsupported-subtype"));
    }

    Ok(parsed)
}

/// Parse a printed power or toughness string (`"2"`, `"*"`, `"*+1"`, `"1+*"`, `"7-*"`, `"-1"`).
pub(crate) fn parse_printed_stat(raw: &str) -> LowerResult<PrintedStat> {
    let t = raw.trim();
    if t == "*" {
        return Ok(PrintedStat::Star {
            multiplier: 1.into(),
            offset: 0.into(),
        });
    }
    if let Some(off) = t.strip_prefix("*+").or_else(|| t.strip_suffix("+*")) {
        let n: i64 = off
            .parse()
            .map_err(|_| LoweringError::InvalidMetadata("v2:meta:bad-pt"))?;
        return Ok(PrintedStat::Star {
            multiplier: 1.into(),
            offset: n.into(),
        });
    }
    if let Some(off) = t.strip_suffix("-*") {
        let n: i64 = off
            .parse()
            .map_err(|_| LoweringError::InvalidMetadata("v2:meta:bad-pt"))?;
        return Ok(PrintedStat::Star {
            multiplier: (-1).into(),
            offset: n.into(),
        });
    }
    let n: i64 = t
        .parse()
        .map_err(|_| LoweringError::InvalidMetadata("v2:meta:bad-pt"))?;
    Ok(PrintedStat::Number(n.into()))
}

fn parse_colored_mana_code(code: &str) -> LowerResult<ManaType> {
    match code {
        "W" => Ok(ManaType::White),
        "U" => Ok(ManaType::Blue),
        "B" => Ok(ManaType::Black),
        "R" => Ok(ManaType::Red),
        "G" => Ok(ManaType::Green),
        _ => Err(LoweringError::InvalidMetadata("v2:meta:bad-mana-color")),
    }
}

fn add_required_mana(cost: &mut ManaCost, mana: ManaType, qty: i64) {
    let entry = cost.required.entry(mana).or_insert_with(|| 0.into());
    *entry = entry.plus(&qty.into());
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
