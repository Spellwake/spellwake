//! The land subtype domain (CR 205.3i) and intrinsic mana mapping (CR 305.6).
//! Land subtypes and the Basic supertype are independent characteristics.
use crate::mana::ManaType;
use crate::object_identity::ObjectId;
use spellwake_contracts::names::EnglishCardName;

/// One canonical Oracle name in the source-qualified entry choice domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardNameIndex(pub usize);

/// Sorted names of nonland card faces from the same Oracle inventory as the program.
#[derive(Clone, PartialEq, Eq)]
pub struct NonlandCardNames(Vec<EnglishCardName>);

impl NonlandCardNames {
    pub fn new(names: std::collections::BTreeSet<EnglishCardName>) -> Self {
        Self(names.into_iter().collect())
    }

    pub fn options(&self) -> &[EnglishCardName] {
        &self.0
    }

    pub fn get(&self, index: CardNameIndex) -> Option<&EnglishCardName> {
        self.0.get(index.0)
    }
}

impl std::fmt::Debug for NonlandCardNames {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NonlandCardNames")
            .field("count", &self.0.len())
            .finish()
    }
}

/// A decision made by the controller before a permanent enters the battlefield.
/// `Reveal(None)` is an explicit choice to decline the optional reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryChoice {
    /// Starting chapter chosen for read ahead.
    Chapter(u8),
    /// Opponent who will control the permanent as it enters.
    Opponent(crate::priority::SeatId),
    /// Two distinct participants linked by the permanent’s ability.
    Players([crate::priority::SeatId; 2]),
    Color(ManaType),
    Reveal(Option<ObjectId>),
    CreatureType(spellwake_contracts::creature_types::CreatureTypeId),
    CardName(CardNameIndex),
}

/// Printed ability replacing the initial untapped status of the entering permanent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryTap {
    /// Choose how many lore counters this Saga enters with.
    ReadAhead {
        final_chapter: u8,
    },
    Always,
    /// Enter tapped after choosing a color other than the excluded color.
    AlwaysChooseColorExcept(ManaType),
    /// Choose a creature type as this permanent enters, without changing its tapped status.
    ChooseCreatureType,
    /// Choose a name using the source-qualified nonland card catalogue.
    ChooseNonlandCardName,
    /// Choose two distinct players before this permanent enters.
    ChooseTwoPlayers,
    /// Choose an opponent to protect an entering Siege (CR 310.12a).
    SiegeProtector,
    /// This permanent enters under a chosen opponent’s control.
    ChosenOpponentControls,
    UnlessTwoOtherLands,
    UnlessTwoOrFewerOtherLands,
    UnlessTwoBasicLands,
    UnlessLandType(BasicLandType, BasicLandType),
    /// The entering controller may reveal a matching land card from their hand.
    UnlessRevealHandLandType(BasicLandType, BasicLandType),
    UnlessTwoOpponents,
    UnlessOpponentLandsEight,
    UnlessPlayerLifeThirteenOrLess,
}

/// All land subtypes in the retained September 25, 2026 Comprehensive Rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LandType {
    Cave,
    Desert,
    Forest,
    Gate,
    Island,
    Lair,
    Locus,
    Mine,
    Mountain,
    Plains,
    Planet,
    PowerPlant,
    Sphere,
    Swamp,
    Tower,
    Town,
    Urzas,
}
impl LandType {
    /// Exhaustive deterministic domain, independent of any card catalogue.
    pub const ALL: [Self; 17] = [
        Self::Cave,
        Self::Desert,
        Self::Forest,
        Self::Gate,
        Self::Island,
        Self::Lair,
        Self::Locus,
        Self::Mine,
        Self::Mountain,
        Self::Plains,
        Self::Planet,
        Self::PowerPlant,
        Self::Sphere,
        Self::Swamp,
        Self::Tower,
        Self::Town,
        Self::Urzas,
    ];

    /// Stable strict contract label. `Urzas` explicitly denotes CR's `Urza’s`;
    /// source catalogue spellings are not implicitly normalized by this API.
    pub const fn wire_label(self) -> &'static str {
        match self {
            Self::Cave => "Cave",
            Self::Desert => "Desert",
            Self::Forest => "Forest",
            Self::Gate => "Gate",
            Self::Island => "Island",
            Self::Lair => "Lair",
            Self::Locus => "Locus",
            Self::Mine => "Mine",
            Self::Mountain => "Mountain",
            Self::Plains => "Plains",
            Self::Planet => "Planet",
            Self::PowerPlant => "PowerPlant",
            Self::Sphere => "Sphere",
            Self::Swamp => "Swamp",
            Self::Tower => "Tower",
            Self::Town => "Town",
            Self::Urzas => "Urzas",
        }
    }

    /// Exact source label for provenance/display; not a contract parser alias.
    pub const fn rules_label(self) -> &'static str {
        match self {
            Self::PowerPlant => "Power-Plant",
            Self::Urzas => "Urza’s",
            other => other.wire_label(),
        }
    }

    /// Accept only the current contract spelling. Unknown labels return `None`.
    pub fn from_wire_label(label: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|value| value.wire_label() == label)
    }

    /// The five basic land types are a subset of land types (CR 305.6).
    pub const fn basic(self) -> Option<BasicLandType> {
        match self {
            Self::Plains => Some(BasicLandType::Plains),
            Self::Island => Some(BasicLandType::Island),
            Self::Swamp => Some(BasicLandType::Swamp),
            Self::Mountain => Some(BasicLandType::Mountain),
            Self::Forest => Some(BasicLandType::Forest),
            Self::Cave
            | Self::Desert
            | Self::Gate
            | Self::Lair
            | Self::Locus
            | Self::Mine
            | Self::Planet
            | Self::PowerPlant
            | Self::Sphere
            | Self::Tower
            | Self::Town
            | Self::Urzas => None,
        }
    }
}

/// Basic land types, each providing its own intrinsic tap ability (CR 305.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BasicLandType {
    Plains,
    Island,
    Swamp,
    Mountain,
    Forest,
}
impl BasicLandType {
    pub const ALL: [Self; 5] = [
        Self::Plains,
        Self::Island,
        Self::Swamp,
        Self::Mountain,
        Self::Forest,
    ];

    pub const fn land_type(self) -> LandType {
        match self {
            Self::Plains => LandType::Plains,
            Self::Island => LandType::Island,
            Self::Swamp => LandType::Swamp,
            Self::Mountain => LandType::Mountain,
            Self::Forest => LandType::Forest,
        }
    }

    /// One mana from this ability; never colorless or all the land's colors.
    pub const fn mana_type(self) -> ManaType {
        match self {
            Self::Plains => ManaType::White,
            Self::Island => ManaType::Blue,
            Self::Swamp => ManaType::Black,
            Self::Mountain => ManaType::Red,
            Self::Forest => ManaType::Green,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn complete_land_domain_has_strict_distinct_contract_and_source_spellings() {
        assert_eq!(LandType::ALL.into_iter().collect::<BTreeSet<_>>().len(), 17);
        for value in LandType::ALL {
            assert_eq!(LandType::from_wire_label(value.wire_label()), Some(value));
        }
        assert_eq!(LandType::Urzas.rules_label(), "Urza’s");
        assert_eq!(LandType::PowerPlant.rules_label(), "Power-Plant");
        for unknown in [
            "Urza’s",
            "Urza's",
            "Power-Plant",
            "plains",
            "Clue",
            "Dryad",
            "Saga",
            "",
        ] {
            assert_eq!(LandType::from_wire_label(unknown), None);
        }
    }

    #[test]
    fn exactly_five_land_types_map_to_the_five_mana_colors() {
        assert_eq!(
            LandType::ALL
                .into_iter()
                .filter_map(LandType::basic)
                .collect::<BTreeSet<_>>(),
            BasicLandType::ALL.into_iter().collect()
        );
        assert_eq!(
            BasicLandType::ALL.map(BasicLandType::mana_type),
            [
                ManaType::White,
                ManaType::Blue,
                ManaType::Black,
                ManaType::Red,
                ManaType::Green
            ]
        );
        for value in BasicLandType::ALL {
            assert_eq!(value.land_type().basic(), Some(value));
        }
    }
}
