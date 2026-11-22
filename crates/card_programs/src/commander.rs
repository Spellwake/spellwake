//! Constructed Commander deck admission from the saved catalogue (CR 702.124, 903.3–6).
use super::{Inventory, InventoryCard, OracleId};
use spellwake_contracts::names::{EnglishCardName, NameRules, ObjectNames};
use spellwake_engine::characteristics::Color;
use std::collections::{BTreeMap, BTreeSet};

/// Both commanders are retained separately, including the physical card's chosen color.
#[derive(Debug, PartialEq, Eq)]
pub struct CommanderDeck {
    source_sha256: String,
    commanders: Vec<OracleId>,
    library: Vec<OracleId>,
    color_identity: BTreeSet<Color>,
    chosen_colors: BTreeMap<OracleId, Color>,
}
impl CommanderDeck {
    pub fn commanders(&self) -> &[OracleId] {
        &self.commanders
    }
    pub fn library(&self) -> &[OracleId] {
        &self.library
    }
    pub fn color_identity(&self) -> &BTreeSet<Color> {
        &self.color_identity
    }
    pub fn chosen_colors(&self) -> &BTreeMap<OracleId, Color> {
        &self.chosen_colors
    }
}

/// A pregame designation or deck failed validation against its catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommanderError {
    UnresolvedCatalogue,
    DeckSize,
    CommanderCount,
    UnknownCard(OracleId),
    MissingSource(OracleId),
    IneligibleCard(OracleId),
    IneligibleCommander(OracleId),
    IncompatiblePartners,
    ChosenColor(OracleId),
    OffColor(OracleId),
    CopyLimit(OracleId),
}
impl std::fmt::Display for CommanderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Commander deck admission failed: {self:?}")
    }
}
impl std::error::Error for CommanderError {}

struct Face<'a> {
    name: &'a str,
    kind: &'a str,
    types: &'a str,
    text: &'a str,
    has_toughness: bool,
}
impl<'a> Face<'a> {
    fn read(card: &'a InventoryCard, id: &OracleId) -> Result<Self, CommanderError> {
        let first = card
            .faces
            .first()
            .ok_or_else(|| CommanderError::MissingSource(id.clone()))?;
        let line = first
            .type_line
            .as_deref()
            .ok_or_else(|| CommanderError::MissingSource(id.clone()))?;
        let (kind, types) = line.split_once(" — ").unwrap_or((line, ""));
        Ok(Self {
            name: &first.name,
            kind,
            types,
            text: first.oracle_text.as_deref().unwrap_or(""),
            has_toughness: first.toughness.is_some() && first.power.is_some(),
        })
    }
    fn kind(&self, word: &str) -> bool {
        self.kind.split_whitespace().any(|s| s == word)
    }
    fn subtype(&self, word: &str) -> bool {
        self.types.split_whitespace().any(|s| s == word)
    }
    fn lines(&self) -> impl Iterator<Item = &str> {
        self.text
            .lines()
            .map(|line| line.split(" (").next().unwrap_or(line).trim())
    }
    fn independently_eligible(&self) -> bool {
        let explicit = format!("{} can be your commander.", self.name);
        self.lines().any(|s| s == explicit)
            || self.kind("Legendary")
                && (self.kind("Creature")
                    || self.subtype("Vehicle")
                    || self.subtype("Spacecraft") && self.has_toughness)
    }
    fn background(&self) -> bool {
        self.kind("Legendary") && self.kind("Enchantment") && self.subtype("Background")
    }
    fn doctor(&self) -> bool {
        self.kind("Legendary")
            && self.kind("Creature")
            && self.types == "Time Lord Doctor"
            && !self.lines().any(|s| s.eq_ignore_ascii_case("changeling"))
    }
    fn partners(&self, other: &Self, names: &NameRules) -> bool {
        if !self.kind("Legendary") || !other.kind("Legendary") {
            return false;
        }
        self.lines().any(|first| {
            if first == "Choose a Background" {
                return other.background();
            }
            if first == "Doctor's companion" {
                return self.kind("Creature") && other.doctor();
            }
            if !self.independently_eligible() || !other.independently_eligible() {
                return false;
            }
            if first == "Partner" || first.starts_with("Partner—") {
                return other.lines().any(|second| second == first);
            }
            if let Some(partner) = first.strip_prefix("Partner with ") {
                return matching_name(other.name, partner, names)
                    && other
                        .lines()
                        .filter_map(|s| s.strip_prefix("Partner with "))
                        .any(|s| matching_name(self.name, s, names));
            }
            false
        })
    }
    fn copy_limit(&self) -> usize {
        if self.kind("Basic") && self.kind("Land") {
            return 100;
        }
        let any = format!("A deck can have any number of cards named {}.", self.name);
        if self.lines().any(|line| line == any) {
            return 100;
        }
        for (word, n) in [("seven", 7), ("nine", 9)] {
            let limit = format!("A deck can have up to {word} cards named {}.", self.name);
            if self.lines().any(|line| line == limit) {
                return n;
            }
        }
        1
    }
}
fn matching_name(name: &str, requested: &str, rules: &NameRules) -> bool {
    let (Ok(name), Ok(requested)) = (
        EnglishCardName::new(name.into()),
        EnglishCardName::new(requested.into()),
    ) else {
        return false;
    };
    rules.has_name(&ObjectNames::single(name), &requested)
}

/// Resolve one constructed deck and its optional per-commander pregame color choices.
/// The returned order places commanders face up in the command zone and leaves library shuffling to the native opening.
///
/// # Errors
/// Rejects unresolved or missing source data, unrecognized cards, illegal designations, the wrong deck size,
/// off-color cards, incorrect color choices and duplicate names above their printed limit.
pub fn prepare_deck(
    inventory: &Inventory,
    rules: &NameRules,
    commanders: &[OracleId],
    library: &[OracleId],
    chosen_colors: &BTreeMap<OracleId, Color>,
) -> Result<CommanderDeck, CommanderError> {
    if !inventory.base_format_inventory_resolved {
        return Err(CommanderError::UnresolvedCatalogue);
    }
    if !(1..=2).contains(&commanders.len()) {
        return Err(CommanderError::CommanderCount);
    }
    if library.len() != 100 - commanders.len() {
        return Err(CommanderError::DeckSize);
    }
    let by_id: BTreeMap<_, _> = inventory
        .cards
        .iter()
        .filter_map(|c| Some((c.oracle_id.as_deref()?, c)))
        .collect();
    let face = |id: &OracleId| {
        let card = *by_id
            .get(id.as_str())
            .ok_or_else(|| CommanderError::UnknownCard(id.clone()))?;
        if card.legalities.get("commander") != Some(&spellwake_content::Legality::Legal) {
            return Err(CommanderError::IneligibleCard(id.clone()));
        }
        Face::read(card, id)
    };
    let identity = |id: &OracleId| {
        inventory
            .source_color_identity(id.as_str())
            .ok_or_else(|| CommanderError::MissingSource(id.clone()))?
            .iter()
            .map(|c| match c.as_str() {
                "W" => Ok(Color::White),
                "U" => Ok(Color::Blue),
                "B" => Ok(Color::Black),
                "R" => Ok(Color::Red),
                "G" => Ok(Color::Green),
                _ => Err(CommanderError::MissingSource(id.clone())),
            })
            .collect::<Result<BTreeSet<_>, _>>()
    };
    let one = face(&commanders[0])?;
    if let Some(second) = commanders.get(1) {
        let two = face(second)?;
        if !one.partners(&two, rules) && !two.partners(&one, rules) {
            return Err(CommanderError::IncompatiblePartners);
        }
    } else if !one.independently_eligible() {
        return Err(CommanderError::IneligibleCommander(commanders[0].clone()));
    }
    let mut color_identity = BTreeSet::new();
    if let Some(id) = chosen_colors.keys().find(|id| !commanders.contains(*id)) {
        return Err(CommanderError::ChosenColor(id.clone()));
    }
    for id in commanders {
        color_identity.extend(identity(id)?);
        let requires_choice = face(id)?
            .text
            .contains("choose a color before the game begins.");
        if requires_choice != chosen_colors.contains_key(id) {
            return Err(CommanderError::ChosenColor(id.clone()));
        }
        color_identity.extend(chosen_colors.get(id).copied());
    }
    let mut copies = BTreeMap::<EnglishCardName, usize>::new();
    for id in commanders.iter().chain(library) {
        let card = face(id)?;
        let mut required = identity(id)?;
        if card.kind("Land") {
            for (name, color) in [
                ("Plains", Color::White),
                ("Island", Color::Blue),
                ("Swamp", Color::Black),
                ("Mountain", Color::Red),
                ("Forest", Color::Green),
            ] {
                if card.subtype(name) {
                    required.insert(color);
                }
            }
        }
        if !required.is_subset(&color_identity) {
            return Err(CommanderError::OffColor(id.clone()));
        }
        let english = EnglishCardName::new(card.name.into())
            .map_err(|_| CommanderError::MissingSource(id.clone()))?;
        let forms = ObjectNames::single(english);
        for key in rules.comparison_forms(&forms) {
            let count = copies.entry(key.clone()).or_default();
            *count += 1;
            if *count > card.copy_limit() {
                return Err(CommanderError::CopyLimit(id.clone()));
            }
        }
    }
    Ok(CommanderDeck {
        source_sha256: inventory.compressed_sha256.clone(),
        commanders: commanders.to_vec(),
        library: library.to_vec(),
        color_identity,
        chosen_colors: chosen_colors.clone(),
    })
}

/// Failure to materialize a validated deck as native physical cards.
#[derive(Debug)]
pub enum CommanderSeedError {
    SourceMismatch,
    PhysicalIdExhausted,
    UnrepresentedColorChoice,
    MissingDefinition {
        card: OracleId,
        error: super::RegistryError,
    },
}
impl std::fmt::Display for CommanderSeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cannot materialize Commander deck: {self:?}")
    }
}
impl std::error::Error for CommanderSeedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingDefinition { error, .. } => Some(error),
            _ => None,
        }
    }
}
impl CommanderDeck {
    /// Materialize every validated identity, retaining a different physical designation for each commander.
    /// Pass the resulting cards to the engine's Commander opening.
    ///
    /// # Errors
    /// Rejects a different source, an unsupported chosen-color definition, a missing native card or exhausted physical IDs.
    pub fn seed_cards(
        &self,
        registry: &super::Registry,
        owner: super::SeatId,
        first: super::CardId,
    ) -> Result<Vec<super::SeedCard>, CommanderSeedError> {
        use super::{CardId, ObjectDestination};
        if registry.coverage().source_sha256 != self.source_sha256 {
            return Err(CommanderSeedError::SourceMismatch);
        }
        self.commanders
            .iter()
            .map(|id| (id, ObjectDestination::Command))
            .chain(
                self.library
                    .iter()
                    .map(|id| (id, ObjectDestination::LibraryBottom)),
            )
            .enumerate()
            .map(|(index, (card, destination))| {
                let physical = first
                    .0
                    .checked_add(
                        u64::try_from(index)
                            .map_err(|_| CommanderSeedError::PhysicalIdExhausted)?,
                    )
                    .ok_or(CommanderSeedError::PhysicalIdExhausted)?;
                let mut seeded = registry
                    .seed(card, CardId(physical), owner, destination)
                    .map_err(|error| CommanderSeedError::MissingDefinition {
                        card: card.clone(),
                        error,
                    })?;
                if destination == ObjectDestination::Command
                    && let Some(&chosen) = self.chosen_colors.get(card)
                {
                    *seeded
                        .program
                        .commander_color_choice_mut()
                        .ok_or(CommanderSeedError::UnrepresentedColorChoice)? = Some(chosen);
                }
                Ok(seeded)
            })
            .collect()
    }
}
