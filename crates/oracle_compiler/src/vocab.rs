//! Canonical Magic vocabulary loaded from Spellwake's pinned CR creature-type input.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::CompilerError;
use crate::ast::{CardTypeMask, ColorMask, SupertypeMask};

#[derive(Debug, Deserialize)]
struct CreatureVocabEntry {
    canonical_label: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CreatureVocabFile {
    Members { members: Vec<String> },
    Entries { entries: Vec<CreatureVocabEntry> },
}

/// Read-only canonical vocabulary shared across all card compilations.
#[derive(Debug, Clone)]
pub struct Vocabulary {
    subtypes: HashMap<String, String>,
    evergreen_keywords: HashSet<&'static str>,
}

impl Vocabulary {
    /// Load creature subtypes from Spellwake's pinned `creature-vocabulary.input.v1.json`
    /// or `creature-vocabulary.input.json`.
    ///
    /// # Errors
    /// Returns [`CompilerError::Io`] if the file cannot be read or [`CompilerError::Json`]
    /// if the JSON structure does not contain a `members` or `entries` array.
    pub fn load_from_file(path: &Path) -> Result<Self, CompilerError> {
        let raw = fs::read_to_string(path)?;
        let parsed: CreatureVocabFile = serde_json::from_str(&raw)?;
        let members: Vec<String> = match parsed {
            CreatureVocabFile::Members { members } => members,
            CreatureVocabFile::Entries { entries } => {
                entries.into_iter().map(|e| e.canonical_label).collect()
            }
        };
        Ok(Self::from_creature_members(&members))
    }

    /// Build a vocabulary from a slice of canonical creature subtype names.
    #[must_use]
    pub fn from_creature_members(members: &[String]) -> Self {
        let mut subtypes = HashMap::with_capacity(members.len() * 2 + 128);
        for name in members {
            let low = name.to_ascii_lowercase();
            subtypes.insert(low.clone(), name.clone());
            if low.ends_with('y')
                && !low.ends_with("ay")
                && !low.ends_with("ey")
                && !low.ends_with("oy")
                && !low.ends_with("uy")
            {
                subtypes.insert(format!("{}ies", &low[..low.len() - 1]), name.clone());
            } else if low.ends_with('s')
                || low.ends_with('x')
                || low.ends_with('z')
                || low.ends_with("ch")
                || low.ends_with("sh")
            {
                subtypes.insert(format!("{low}es"), name.clone());
            } else if low.ends_with('f') {
                subtypes.insert(format!("{}ves", &low[..low.len() - 1]), name.clone());
            } else if low.ends_with("fe") {
                subtypes.insert(format!("{}ves", &low[..low.len() - 2]), name.clone());
            } else {
                subtypes.insert(format!("{low}s"), name.clone());
            }
        }

        // Irregular creature plurals and non-creature subtypes (land, artifact, enchantment, spell)
        for (k, v) in EXTRA_SUBTYPES {
            subtypes.insert((*k).to_string(), (*v).to_string());
        }

        let evergreen_keywords: HashSet<&'static str> =
            EVERGREEN_KEYWORDS.iter().copied().collect();

        Self {
            subtypes,
            evergreen_keywords,
        }
    }

    /// Look up a canonical creature or noncreature subtype from a lowercase token.
    #[must_use]
    pub fn lookup_subtype(&self, lower_tok: &str) -> Option<&str> {
        self.subtypes.get(lower_tok).map(String::as_str)
    }

    /// True if `lower_tok` is a known standalone CR 702 keyword.
    #[must_use]
    pub fn is_evergreen_keyword(&self, lower_tok: &str) -> bool {
        self.evergreen_keywords.contains(lower_tok)
    }
}

/// Map a lowercase token to a `CardTypeMask` bit.
#[must_use]
pub fn lookup_card_type(lower_tok: &str) -> Option<CardTypeMask> {
    match lower_tok {
        "creature" | "creatures" => Some(CardTypeMask::CREATURE),
        "artifact" | "artifacts" => Some(CardTypeMask::ARTIFACT),
        "enchantment" | "enchantments" => Some(CardTypeMask::ENCHANTMENT),
        "land" | "lands" => Some(CardTypeMask::LAND),
        "planeswalker" | "planeswalkers" => Some(CardTypeMask::PLANESWALKER),
        "instant" | "instants" => Some(CardTypeMask::INSTANT),
        "sorcery" | "sorceries" => Some(CardTypeMask::SORCERY),
        "battle" | "battles" => Some(CardTypeMask::BATTLE),
        "kindred" | "tribal" => Some(CardTypeMask::KINDRED),
        _ => None,
    }
}

/// Map a lowercase token to a `ColorMask` bit.
#[must_use]
pub fn lookup_color(lower_tok: &str) -> Option<ColorMask> {
    match lower_tok {
        "white" => Some(ColorMask::WHITE),
        "blue" => Some(ColorMask::BLUE),
        "black" => Some(ColorMask::BLACK),
        "red" => Some(ColorMask::RED),
        "green" => Some(ColorMask::GREEN),
        _ => None,
    }
}

/// Map a lowercase token to a `SupertypeMask` bit.
#[must_use]
pub fn lookup_supertype(lower_tok: &str) -> Option<SupertypeMask> {
    match lower_tok {
        "legendary" => Some(SupertypeMask::LEGENDARY),
        "basic" => Some(SupertypeMask::BASIC),
        "snow" => Some(SupertypeMask::SNOW),
        "world" => Some(SupertypeMask::WORLD),
        "historic" => Some(SupertypeMask::HISTORIC),
        _ => None,
    }
}

/// Parse an English number word or ASCII integer into `i32`.
#[must_use]
pub fn parse_number_word(lower_tok: &str) -> Option<i32> {
    if let Ok(n) = lower_tok.parse::<i32>() {
        return Some(n);
    }
    if lower_tok.contains(',')
        && lower_tok.bytes().all(|b| b.is_ascii_digit() || b == b',')
        && let Ok(n) = lower_tok.replace(',', "").parse::<i32>()
    {
        return Some(n);
    }
    match lower_tok {
        "zero" => Some(0),
        "a" | "an" | "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        "eleven" => Some(11),
        "twelve" => Some(12),
        "thirteen" => Some(13),
        "fourteen" => Some(14),
        "fifteen" => Some(15),
        "sixteen" => Some(16),
        "seventeen" => Some(17),
        "eighteen" => Some(18),
        "nineteen" => Some(19),
        "twenty" => Some(20),
        "thirty" => Some(30),
        "forty" => Some(40),
        "fifty" => Some(50),
        "sixty" => Some(60),
        "seventy" => Some(70),
        "eighty" => Some(80),
        "ninety" => Some(90),
        "hundred" | "one hundred" => Some(100),
        _ => {
            if let Some((tens_s, ones_s)) = lower_tok.split_once('-') {
                let tens = match tens_s {
                    "twenty" => Some(20),
                    "thirty" => Some(30),
                    "forty" => Some(40),
                    "fifty" => Some(50),
                    "sixty" => Some(60),
                    "seventy" => Some(70),
                    "eighty" => Some(80),
                    "ninety" => Some(90),
                    _ => None,
                }?;
                let ones = match ones_s {
                    "one" => Some(1),
                    "two" => Some(2),
                    "three" => Some(3),
                    "four" => Some(4),
                    "five" => Some(5),
                    "six" => Some(6),
                    "seven" => Some(7),
                    "eight" => Some(8),
                    "nine" => Some(9),
                    _ => None,
                }?;
                Some(tens + ones)
            } else {
                None
            }
        }
    }
}

const EXTRA_SUBTYPES: &[(&str, &str)] = &[
    // Irregular creature plurals & groupings
    ("hero", "Hero"),
    ("heroes", "Hero"),
    ("timelord", "Time Lord"),
    ("timelords", "Time Lord"),
    ("lesson", "Lesson"),
    ("lessons", "Lesson"),
    ("lander", "Lander"),
    ("landers", "Lander"),
    ("mine", "Mine"),
    ("power-plant", "Power-Plant"),
    ("tower", "Tower"),
    ("walker", "Walker"),
    ("walkers", "Walker"),
    ("attraction", "Attraction"),
    ("attractions", "Attraction"),
    ("contraption", "Contraption"),
    ("contraptions", "Contraption"),
    ("spacecraft", "Spacecraft"),
    ("spacecrafts", "Spacecraft"),
    ("room", "Room"),
    ("rooms", "Room"),
    ("bobblehead", "Bobblehead"),
    ("bobbleheads", "Bobblehead"),
    ("plan", "Plan"),
    ("plans", "Plan"),
    ("rune", "Rune"),
    ("runes", "Rune"),
    ("ring-bearer", "Ring-bearer"),
    ("basri", "Basri"),
    ("urza's", "Urza's"),
    ("elves", "Elf"),
    ("dwarves", "Dwarf"),
    ("wolves", "Wolf"),
    ("werewolves", "Werewolf"),
    ("allies", "Ally"),
    ("armies", "Army"),
    ("faeries", "Faerie"),
    ("harpies", "Harpy"),
    ("merfolk", "Merfolk"),
    ("eldrazi", "Eldrazi"),
    ("treefolk", "Treefolk"),
    ("kithkin", "Kithkin"),
    ("fungi", "Fungus"),
    ("mice", "Mouse"),
    ("pegasi", "Pegasus"),
    ("myr", "Myr"),
    ("kavu", "Kavu"),
    ("viashino", "Viashino"),
    ("kor", "Kor"),
    ("vedalken", "Vedalken"),
    ("leonin", "Leonin"),
    ("loxodon", "Loxodon"),
    ("fish", "Fish"),
    ("elk", "Elk"),
    ("oxen", "Ox"),
    ("outlaw", "Outlaw"),
    ("outlaws", "Outlaw"),
    // Land subtypes
    ("plains", "Plains"),
    ("island", "Island"),
    ("islands", "Island"),
    ("swamp", "Swamp"),
    ("swamps", "Swamp"),
    ("mountain", "Mountain"),
    ("mountains", "Mountain"),
    ("forest", "Forest"),
    ("forests", "Forest"),
    ("gate", "Gate"),
    ("gates", "Gate"),
    ("desert", "Desert"),
    ("deserts", "Desert"),
    ("cave", "Cave"),
    ("caves", "Cave"),
    ("lair", "Lair"),
    ("lairs", "Lair"),
    ("locus", "Locus"),
    ("loci", "Locus"),
    ("sphere", "Sphere"),
    ("spheres", "Sphere"),
    ("town", "Town"),
    ("towns", "Town"),
    ("planet", "Planet"),
    ("planets", "Planet"),
    // Artifact / Enchantment / Spell / Battle subtypes
    ("aura", "Aura"),
    ("auras", "Aura"),
    ("cartouche", "Cartouche"),
    ("cartouches", "Cartouche"),
    ("equipment", "Equipment"),
    ("vehicle", "Vehicle"),
    ("vehicles", "Vehicle"),
    ("mount", "Mount"),
    ("mounts", "Mount"),
    ("siege", "Siege"),
    ("sieges", "Siege"),
    ("omen", "Omen"),
    ("omens", "Omen"),
    ("saga", "Saga"),
    ("sagas", "Saga"),
    ("shrine", "Shrine"),
    ("shrines", "Shrine"),
    ("curse", "Curse"),
    ("curses", "Curse"),
    ("clue", "Clue"),
    ("clues", "Clue"),
    ("treasure", "Treasure"),
    ("treasures", "Treasure"),
    ("food", "Food"),
    ("foods", "Food"),
    ("blood", "Blood"),
    ("bloods", "Blood"),
    ("map", "Map"),
    ("maps", "Map"),
    ("junk", "Junk"),
    ("junks", "Junk"),
    ("role", "Role"),
    ("roles", "Role"),
    ("shard", "Shard"),
    ("shards", "Shard"),
    ("powerstone", "Powerstone"),
    ("powerstones", "Powerstone"),
    ("incubator", "Incubator"),
    ("incubators", "Incubator"),
    ("gold", "Gold"),
    ("golds", "Gold"),
    ("fortification", "Fortification"),
    ("fortifications", "Fortification"),
    ("rune", "Rune"),
    ("runes", "Rune"),
    ("background", "Background"),
    ("backgrounds", "Background"),
    ("room", "Room"),
    ("rooms", "Room"),
    ("case", "Case"),
    ("cases", "Case"),
    ("class", "Class"),
    ("classes", "Class"),
    ("arcane", "Arcane"),
    ("lesson", "Lesson"),
    ("lessons", "Lesson"),
    ("trap", "Trap"),
    ("traps", "Trap"),
    ("contraption", "Contraption"),
    ("contraptions", "Contraption"),
    ("attraction", "Attraction"),
    ("attractions", "Attraction"),
    ("spacecraft", "Spacecraft"),
    ("spacecrafts", "Spacecraft"),
    ("bobblehead", "Bobblehead"),
    ("bobbleheads", "Bobblehead"),
    ("book", "Book"),
    ("books", "Book"),
    ("mutagen", "Mutagen"),
    ("lander", "Lander"),
    ("walker", "Walker"),
    ("brainiac", "Brainiac"),
    ("brainiacs", "Brainiac"),
    // Planeswalker subtypes
    ("ajani", "Ajani"),
    ("angrath", "Angrath"),
    ("ashiok", "Ashiok"),
    ("bolas", "Bolas"),
    ("chandra", "Chandra"),
    ("domri", "Domri"),
    ("dovin", "Dovin"),
    ("elspeth", "Elspeth"),
    ("garruk", "Garruk"),
    ("gideon", "Gideon"),
    ("huatli", "Huatli"),
    ("jace", "Jace"),
    ("jeska", "Jeska"),
    ("kaito", "Kaito"),
    ("karn", "Karn"),
    ("kaya", "Kaya"),
    ("kiora", "Kiora"),
    ("liliana", "Liliana"),
    ("lukka", "Lukka"),
    ("nahiri", "Nahiri"),
    ("narset", "Narset"),
    ("nissa", "Nissa"),
    ("oko", "Oko"),
    ("ral", "Ral"),
    ("rowan", "Rowan"),
    ("saheeli", "Saheeli"),
    ("sarkhan", "Sarkhan"),
    ("serra", "Serra"),
    ("sorin", "Sorin"),
    ("tamiyo", "Tamiyo"),
    ("teferi", "Teferi"),
    ("tezzeret", "Tezzeret"),
    ("tibalt", "Tibalt"),
    ("tyvar", "Tyvar"),
    ("ugin", "Ugin"),
    ("venser", "Venser"),
    ("vivien", "Vivien"),
    ("vraska", "Vraska"),
    ("will", "Will"),
    ("wrenn", "Wrenn"),
    ("xenagos", "Xenagos"),
    ("yanggu", "Yanggu"),
    ("yanling", "Yanling"),
];

const EVERGREEN_KEYWORDS: &[&str] = &[
    "flying",
    "first strike",
    "double strike",
    "deathtouch",
    "haste",
    "hexproof",
    "indestructible",
    "lifelink",
    "menace",
    "reach",
    "shroud",
    "trample",
    "vigilance",
    "defender",
    "flash",
    "fear",
    "intimidate",
    "skulk",
    "shadow",
    "horsemanship",
    "flanking",
    "banding",
    "phasing",
    "wither",
    "infect",
    "changeling",
    "devoid",
    "decayed",
    "split second",
    "convoke",
    "delve",
    "improvise",
    "cascade",
    "storm",
    "rebound",
    "retrace",
    "jump-start",
    "undaunted",
    "sunburst",
    "daybound",
    "nightbound",
    "ascend",
    "riot",
    "unleash",
    "soulbond",
    "prowess",
    "exalted",
    "extort",
    "evolve",
    "undying",
    "persist",
    "battle cry",
    "melee",
    "myriad",
    "dethrone",
    "training",
    "mentor",
    "exploit",
    "ingest",
    "provoke",
    "haunt",
    "living weapon",
    "for mirrodin!",
    "read ahead",
    "bargain",
    "partner",
    "umbra armor",
    "totem armor",
    "demonstrate",
    "assist",
    "conspire",
    "plainswalk",
    "islandwalk",
    "swampwalk",
    "mountainwalk",
    "forestwalk",
    "desertwalk",
    "nonbasic landwalk",
    "legendary landwalk",
    "snow landwalk",
    "snow forestwalk",
    "snow swampwalk",
    "artifact landwalk",
    "landwalk",
    "hidden agenda",
    "double agenda",
    "denimwalk",
    "art rampage",
    "triple strike",
    "last strike",
    // Expanded CR 702 standalone keywords
    "fuse",
    "aftermath",
    "spree",
    "tiered",
    "start your engines!",
    "choose a background",
    "doctor's companion",
    "friends forever",
    "station",
    "living metal",
    "compleated",
    "ravenous",
    "enlist",
    "epic",
    "cipher",
    "gravestorm",
    "job select",
    "space sculptor",
    "partner\u{2014}character select",
    "partner\u{2014}survivors",
    "partner\u{2014}father & son",
    "partner\u{2014}friends forever",
    "converge",
    "exhaust",
    "increment",
    "storied",
    "suspend",
    "flashback",
    "encore",
    "escape",
    "unearth",
    "paradigm",
];
