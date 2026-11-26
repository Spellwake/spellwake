//! Composable, zero-allocation object filters shared by battlefield targeting,
//! graveyard targeting, whole-group zone changes, and resolution-time group modifiers.
use super::{
    AffectedPlayers, ArtifactSubtype, CardType, CreatureTypeSlot, DefinitionError,
    EnchantmentSubtype, Supertype,
};
use crate::{
    characteristics::{Color, KeywordAbility},
    land::LandType,
    object_identity::ObjectId,
    priority::SeatId,
};
use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};
use std::collections::BTreeSet;

/// Bitmask over [`CardType`] supporting any subset or union without heap allocation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CardTypeMask(u16);

impl CardTypeMask {
    /// Empty mask (matches no types when used as a forbidden set, or any type when used as an `any_of` set).
    pub const EMPTY: Self = Self(0);
    pub const BATTLE: Self = Self(1 << 0);
    pub const PLANESWALKER: Self = Self(1 << 1);
    pub const LAND: Self = Self(1 << 2);
    pub const ARTIFACT: Self = Self(1 << 3);
    pub const CREATURE: Self = Self(1 << 4);
    pub const ENCHANTMENT: Self = Self(1 << 5);
    pub const KINDRED: Self = Self(1 << 6);
    pub const INSTANT: Self = Self(1 << 7);
    pub const SORCERY: Self = Self(1 << 8);
    /// Every permanent card type (artifact, battle, creature, enchantment, land, planeswalker).
    pub const PERMANENT: Self = Self(
        Self::BATTLE.0
            | Self::PLANESWALKER.0
            | Self::LAND.0
            | Self::ARTIFACT.0
            | Self::CREATURE.0
            | Self::ENCHANTMENT.0,
    );

    /// Convert a single [`CardType`] to its mask bit.
    #[must_use]
    pub const fn from_card_type(kind: CardType) -> Self {
        match kind {
            CardType::Battle => Self::BATTLE,
            CardType::Planeswalker => Self::PLANESWALKER,
            CardType::Land => Self::LAND,
            CardType::Artifact => Self::ARTIFACT,
            CardType::Creature => Self::CREATURE,
            CardType::Enchantment => Self::ENCHANTMENT,
            CardType::Kindred => Self::KINDRED,
            CardType::Instant => Self::INSTANT,
            CardType::Sorcery => Self::SORCERY,
        }
    }

    /// Combine two type masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Add one [`CardType`] to this mask.
    #[must_use]
    pub const fn with(self, kind: CardType) -> Self {
        self.union(Self::from_card_type(kind))
    }

    /// Raw bit representation.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Whether no types are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether this mask includes `kind`.
    #[must_use]
    pub const fn contains(self, kind: CardType) -> bool {
        (self.0 & Self::from_card_type(kind).0) != 0
    }

    /// True if empty (unconstrained) or if `types` contains at least one bit in this mask.
    #[must_use]
    pub fn matches_any(self, types: &BTreeSet<CardType>) -> bool {
        self.is_empty() || types.iter().any(|kind| self.contains(*kind))
    }

    /// True if `types` contains every bit set in this mask.
    #[must_use]
    pub fn matches_all(self, types: &BTreeSet<CardType>) -> bool {
        if self.is_empty() {
            return true;
        }
        let mut present = 0u16;
        for kind in types {
            present |= Self::from_card_type(*kind).0;
        }
        (present & self.0) == self.0
    }

    /// True if `types` contains none of the bits in this mask.
    #[must_use]
    pub fn matches_none(self, types: &BTreeSet<CardType>) -> bool {
        self.is_empty() || types.iter().all(|kind| !self.contains(*kind))
    }
}

/// Bitmask over [`Supertype`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SupertypeMask(u8);

impl SupertypeMask {
    pub const EMPTY: Self = Self(0);
    pub const BASIC: Self = Self(1 << 0);
    pub const LEGENDARY: Self = Self(1 << 1);
    pub const ONGOING: Self = Self(1 << 2);
    pub const SNOW: Self = Self(1 << 3);
    pub const WORLD: Self = Self(1 << 4);

    /// Convert a single [`Supertype`] to its mask bit.
    #[must_use]
    pub const fn from_supertype(kind: Supertype) -> Self {
        match kind {
            Supertype::Basic => Self::BASIC,
            Supertype::Legendary => Self::LEGENDARY,
            Supertype::Ongoing => Self::ONGOING,
            Supertype::Snow => Self::SNOW,
            Supertype::World => Self::WORLD,
        }
    }

    /// Combine two supertype masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether no supertypes are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether `supertypes` contains every bit in this mask.
    #[must_use]
    pub fn matches_all(self, supertypes: &BTreeSet<Supertype>) -> bool {
        if self.is_empty() {
            return true;
        }
        let mut present = 0u8;
        for kind in supertypes {
            present |= Self::from_supertype(*kind).0;
        }
        (present & self.0) == self.0
    }

    /// Whether `supertypes` contains none of the bits in this mask.
    #[must_use]
    pub fn matches_none(self, supertypes: &BTreeSet<Supertype>) -> bool {
        self.is_empty()
            || supertypes
                .iter()
                .all(|kind| (self.0 & Self::from_supertype(*kind).0) == 0)
    }
}

/// 5-bit mask over [`Color`] supporting arbitrary unions and exclusions without allocation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ColorMask(u8);

impl ColorMask {
    pub const EMPTY: Self = Self(0);
    pub const WHITE: Self = Self(1 << 0);
    pub const BLUE: Self = Self(1 << 1);
    pub const BLACK: Self = Self(1 << 2);
    pub const RED: Self = Self(1 << 3);
    pub const GREEN: Self = Self(1 << 4);
    pub const ALL: Self =
        Self(Self::WHITE.0 | Self::BLUE.0 | Self::BLACK.0 | Self::RED.0 | Self::GREEN.0);

    /// Convert a single [`Color`] to its mask bit.
    #[must_use]
    pub const fn from_color(color: Color) -> Self {
        match color {
            Color::White => Self::WHITE,
            Color::Blue => Self::BLUE,
            Color::Black => Self::BLACK,
            Color::Red => Self::RED,
            Color::Green => Self::GREEN,
        }
    }

    /// Combine two color masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Add one [`Color`] to this mask.
    #[must_use]
    pub const fn with(self, color: Color) -> Self {
        self.union(Self::from_color(color))
    }

    /// Whether no colors are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether this mask includes `color`.
    #[must_use]
    pub const fn contains(self, color: Color) -> bool {
        (self.0 & Self::from_color(color).0) != 0
    }

    /// True if empty (unconstrained) or if `colors` contains at least one bit in this mask.
    #[must_use]
    pub fn matches_any(self, colors: &BTreeSet<Color>) -> bool {
        self.is_empty() || colors.iter().any(|color| self.contains(*color))
    }

    /// True if `colors` contains none of the bits in this mask.
    #[must_use]
    pub fn matches_none(self, colors: &BTreeSet<Color>) -> bool {
        self.is_empty() || colors.iter().all(|color| !self.contains(*color))
    }
}

/// Color predicate on an object's current evaluated colors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ObjectColorFilter {
    #[default]
    Any,
    Colorless,
    Colored,
    Multicolored,
    Monocolored,
    /// Object must have at least one color in `any_of` (when non-empty) and none in `none_of`.
    Mask {
        any_of: ColorMask,
        none_of: ColorMask,
    },
}

impl ObjectColorFilter {
    /// Require at least this color.
    #[must_use]
    pub const fn has(color: Color) -> Self {
        Self::Mask {
            any_of: ColorMask::from_color(color),
            none_of: ColorMask::EMPTY,
        }
    }

    /// Forbid this color.
    #[must_use]
    pub const fn lacks(color: Color) -> Self {
        Self::Mask {
            any_of: ColorMask::EMPTY,
            none_of: ColorMask::from_color(color),
        }
    }

    /// Require at least one color in `mask`.
    #[must_use]
    pub const fn any_of(mask: ColorMask) -> Self {
        Self::Mask {
            any_of: mask,
            none_of: ColorMask::EMPTY,
        }
    }

    /// Forbid every color in `mask`.
    #[must_use]
    pub const fn none_of(mask: ColorMask) -> Self {
        Self::Mask {
            any_of: ColorMask::EMPTY,
            none_of: mask,
        }
    }

    /// Evaluate this color predicate against an object's current colors.
    #[must_use]
    pub fn matches(self, colors: &BTreeSet<Color>) -> bool {
        match self {
            Self::Any => true,
            Self::Colorless => colors.is_empty(),
            Self::Colored => !colors.is_empty(),
            Self::Multicolored => colors.len() >= 2,
            Self::Monocolored => colors.len() == 1,
            Self::Mask { any_of, none_of } => {
                any_of.matches_any(colors) && none_of.matches_none(colors)
            }
        }
    }
}

/// Maximum number of subtypes stored inline in a [`SubtypeList`].
pub const MAX_INLINE_SUBTYPES: usize = 16;

/// Single typed subtype atom across creature, land, artifact, enchantment, planeswalker, spell, and Aura subtypes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SingleSubtype {
    Creature(CreatureTypeId),
    Land(LandType),
    Artifact(ArtifactSubtype),
    Enchantment(EnchantmentSubtype),
    Planeswalker(super::PlaneswalkerSubtype),
    Spell(super::SpellSubtype),
    Aura,
}

impl SingleSubtype {
    /// Evaluate this single subtype atom against an object's subtype sets.
    #[must_use]
    // Borrows concrete subtype sets directly to stay allocation-free on hot filter matching.
    #[allow(clippy::too_many_arguments)]
    pub fn matches_sets(
        self,
        creatures: &BTreeSet<CreatureTypeId>,
        lands: &BTreeSet<LandType>,
        artifacts: &BTreeSet<ArtifactSubtype>,
        enchantments: &BTreeSet<EnchantmentSubtype>,
        planeswalkers: &BTreeSet<super::PlaneswalkerSubtype>,
        spells: &BTreeSet<super::SpellSubtype>,
        is_aura: bool,
    ) -> bool {
        match self {
            Self::Creature(kind) => creatures.contains(&kind),
            Self::Land(kind) => lands.contains(&kind),
            Self::Artifact(kind) => artifacts.contains(&kind),
            Self::Enchantment(kind) => enchantments.contains(&kind),
            Self::Planeswalker(kind) => planeswalkers.contains(&kind),
            Self::Spell(kind) => spells.contains(&kind),
            Self::Aura => is_aura,
        }
    }
}

/// Fixed-capacity `Copy` list of up to [`MAX_INLINE_SUBTYPES`] typed subtypes.
/// Unused trailing slots are canonically filled with [`SingleSubtype::Aura`] so
/// structural equality is exact and deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtypeList {
    len: u8,
    items: [SingleSubtype; MAX_INLINE_SUBTYPES],
}

impl SubtypeList {
    /// Construct a canonical list from `slice`, returning `None` if empty or longer than [`MAX_INLINE_SUBTYPES`].
    #[must_use]
    pub fn from_slice(slice: &[SingleSubtype]) -> Option<Self> {
        if slice.is_empty() || slice.len() > MAX_INLINE_SUBTYPES {
            return None;
        }
        let len = u8::try_from(slice.len()).ok()?;
        let mut items = [SingleSubtype::Aura; MAX_INLINE_SUBTYPES];
        items[..slice.len()].copy_from_slice(slice);
        Some(Self { len, items })
    }

    /// Borrow the active prefix of subtypes.
    #[must_use]
    pub fn as_slice(&self) -> &[SingleSubtype] {
        &self.items[..usize::from(self.len)]
    }

    /// Mutably borrow the active prefix of subtypes.
    pub fn as_mut_slice(&mut self) -> &mut [SingleSubtype] {
        &mut self.items[..usize::from(self.len)]
    }

    /// Iterate over the stored subtypes.
    pub fn iter(&self) -> std::slice::Iter<'_, SingleSubtype> {
        self.as_slice().iter()
    }
}

/// Maximum byte length for an inline exact English card name on [`ObjectClause`].
pub const MAX_EXACT_NAME_BYTES: usize = 64;

/// Fixed-capacity `Copy` exact English card name key for [`ObjectClause`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ExactNameKey {
    len: u8,
    bytes: [u8; MAX_EXACT_NAME_BYTES],
}

impl ExactNameKey {
    /// Validate and store an exact English card name label (1..=64 UTF-8 bytes).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let raw = name.as_bytes();
        if raw.is_empty() || raw.len() > MAX_EXACT_NAME_BYTES {
            return None;
        }
        if spellwake_contracts::names::EnglishCardName::new(name.to_owned()).is_err() {
            return None;
        }
        let len = u8::try_from(raw.len()).ok()?;
        let mut bytes = [0u8; MAX_EXACT_NAME_BYTES];
        bytes[..raw.len()].copy_from_slice(raw);
        Some(Self { len, bytes })
    }

    /// Borrow the stored exact card name string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }

    /// Match against `names` using `name_rules` (honoring interchangeable names, ASCII case-insensitively).
    #[must_use]
    pub fn matches_with_rules(
        self,
        names: &spellwake_contracts::names::ObjectNames,
        name_rules: &spellwake_contracts::names::NameRules,
    ) -> bool {
        let target = self.as_str();
        if names
            .iter()
            .any(|held| held.as_str().eq_ignore_ascii_case(target))
        {
            return true;
        }
        name_rules.groups().iter().any(|group| {
            group
                .iter()
                .any(|n| n.as_str().eq_ignore_ascii_case(target))
                && group
                    .iter()
                    .any(|n| names.iter().any(|held| held.as_str() == n.as_str()))
        })
    }

    /// Match directly against `names` when `NameRules` is unavailable (ASCII case-insensitively).
    #[must_use]
    pub fn matches_exact_names(self, names: &spellwake_contracts::names::ObjectNames) -> bool {
        let target = self.as_str();
        names.iter().any(|n| n.as_str().eq_ignore_ascii_case(target))
    }
}

impl std::fmt::Debug for ExactNameKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ExactNameKey({:?})", self.as_str())
    }
}

/// Subtype predicate covering creature, land, artifact, and enchantment subtypes without allocation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SubtypeFilter {
    #[default]
    Any,
    /// Object has this creature subtype (including kindred noncreatures when card type is unconstrained).
    Has(CreatureTypeId),
    /// Object lacks this creature subtype.
    Lacks(CreatureTypeId),
    /// Object has at least one of these three creature subtypes.
    AnyOf([CreatureTypeId; 3]),
    /// Object has none of these three creature subtypes.
    NoneOf([CreatureTypeId; 3]),
    /// Object has at least one subtype in this cross-category list (up to 16).
    AnyOfList(SubtypeList),
    /// Object has every subtype in this conjunctive list (for example, Eldrazi Scion).
    AllOfList(SubtypeList),
    /// Object lacks every subtype in this cross-category list (up to 16).
    NoneOfList(SubtypeList),
    /// Object lacks the creature subtype chosen into this resolution slot.
    ExceptChosenSlot(CreatureTypeSlot),
    /// Object has the creature subtype chosen into this resolution slot.
    MatchesChosenSlot(CreatureTypeSlot),
    /// Object lacks every creature subtype named during this resolution.
    OutsideNamedSlots,
    /// Object has this land subtype (for example Gate, Forest, Desert, Cave).
    Land(LandType),
    /// Object lacks this land subtype.
    NotLand(LandType),
    /// Object has at least one land subtype in this `LandType::bit()` bitmask.
    AnyLandMask(u32),
    /// Object lacks every land subtype in this `LandType::bit()` bitmask.
    NotLandMask(u32),
    /// Object has this artifact subtype (for example Equipment, Treasure, Food, Clue).
    Artifact(ArtifactSubtype),
    /// Object has this enchantment subtype (for example Saga, Curse).
    Enchantment(EnchantmentSubtype),
    /// Object has this planeswalker subtype (for example Jace, Chandra, Vraska).
    Planeswalker(super::PlaneswalkerSubtype),
    /// Object has this spell subtype (for example Arcane, Lesson, Adventure, Trap, Omen).
    Spell(super::SpellSubtype),
    /// Object is an Aura enchantment.
    Aura,
}

impl SubtypeFilter {
    /// Evaluate every non-slot subtype predicate against concrete subtype sets.
    /// Returns `None` only for resolution-slot variants (`ExceptChosenSlot`, `MatchesChosenSlot`, `OutsideNamedSlots`).
    #[must_use]
    // Borrows concrete subtype sets directly to stay allocation-free on hot filter matching.
    #[allow(clippy::too_many_arguments)]
    pub fn matches_sets(
        self,
        creatures: &BTreeSet<CreatureTypeId>,
        lands: &BTreeSet<LandType>,
        artifacts: &BTreeSet<ArtifactSubtype>,
        enchantments: &BTreeSet<EnchantmentSubtype>,
        planeswalkers: &BTreeSet<super::PlaneswalkerSubtype>,
        spells: &BTreeSet<super::SpellSubtype>,
        is_aura: bool,
    ) -> Option<bool> {
        Some(match self {
            Self::Any => true,
            Self::Has(kind) => creatures.contains(&kind),
            Self::Lacks(kind) => !creatures.contains(&kind),
            Self::AnyOf(kinds) => kinds.iter().any(|kind| creatures.contains(kind)),
            Self::NoneOf(kinds) => kinds.iter().all(|kind| !creatures.contains(kind)),
            Self::AnyOfList(list) => list.iter().any(|item| {
                item.matches_sets(
                    creatures,
                    lands,
                    artifacts,
                    enchantments,
                    planeswalkers,
                    spells,
                    is_aura,
                )
            }),
            Self::AllOfList(list) => list.iter().all(|item| {
                item.matches_sets(
                    creatures,
                    lands,
                    artifacts,
                    enchantments,
                    planeswalkers,
                    spells,
                    is_aura,
                )
            }),
            Self::NoneOfList(list) => list.iter().all(|item| {
                !item.matches_sets(
                    creatures,
                    lands,
                    artifacts,
                    enchantments,
                    planeswalkers,
                    spells,
                    is_aura,
                )
            }),
            Self::Land(kind) => lands.contains(&kind),
            Self::NotLand(kind) => !lands.contains(&kind),
            Self::AnyLandMask(mask) => lands.iter().any(|lt| (mask & lt.bit()) != 0),
            Self::NotLandMask(mask) => lands.iter().all(|lt| (mask & lt.bit()) == 0),
            Self::Artifact(kind) => artifacts.contains(&kind),
            Self::Enchantment(kind) => enchantments.contains(&kind),
            Self::Planeswalker(kind) => planeswalkers.contains(&kind),
            Self::Spell(kind) => spells.contains(&kind),
            Self::Aura => is_aura,
            Self::ExceptChosenSlot(_) | Self::MatchesChosenSlot(_) | Self::OutsideNamedSlots => {
                return None;
            }
        })
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite(&mut self, from: CreatureTypeId, to: CreatureTypeId) {
        let replace = |kind: &mut CreatureTypeId| {
            if *kind == from {
                *kind = to;
            }
        };
        match self {
            Self::Has(kind) | Self::Lacks(kind) => replace(kind),
            Self::AnyOf(kinds) | Self::NoneOf(kinds) => {
                for kind in kinds {
                    replace(kind);
                }
            }
            Self::AnyOfList(list) | Self::AllOfList(list) | Self::NoneOfList(list) => {
                for item in list.as_mut_slice() {
                    if let SingleSubtype::Creature(kind) = item {
                        replace(kind);
                    }
                }
            }
            Self::Any
            | Self::ExceptChosenSlot(_)
            | Self::MatchesChosenSlot(_)
            | Self::OutsideNamedSlots
            | Self::Land(_)
            | Self::NotLand(_)
            | Self::AnyLandMask(_)
            | Self::NotLandMask(_)
            | Self::Artifact(_)
            | Self::Enchantment(_)
            | Self::Planeswalker(_)
            | Self::Spell(_)
            | Self::Aura => {}
        }
    }

    /// Validate that every referenced creature subtype exists in the active vocabulary.
    pub(crate) fn validate_vocabulary(
        self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        let check = |kind: CreatureTypeId| {
            if vocabulary.label(kind).is_none() {
                Err(DefinitionError::ForeignCreatureType)
            } else {
                Ok(())
            }
        };
        match self {
            Self::Has(kind) | Self::Lacks(kind) => check(kind),
            Self::AnyOf(kinds) | Self::NoneOf(kinds) => {
                for kind in kinds {
                    check(kind)?;
                }
                Ok(())
            }
            Self::AnyOfList(list) | Self::AllOfList(list) | Self::NoneOfList(list) => {
                for item in list.iter() {
                    if let SingleSubtype::Creature(kind) = item {
                        check(*kind)?;
                    }
                }
                Ok(())
            }
            Self::Any
            | Self::ExceptChosenSlot(_)
            | Self::MatchesChosenSlot(_)
            | Self::OutsideNamedSlots
            | Self::Land(_)
            | Self::NotLand(_)
            | Self::AnyLandMask(_)
            | Self::NotLandMask(_)
            | Self::Artifact(_)
            | Self::Enchantment(_)
            | Self::Planeswalker(_)
            | Self::Aura
            | Self::Spell(_) => Ok(()),
        }
    }
}

/// Controller or owner restriction relative to the resolving/acting player.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ControllerFilter {
    #[default]
    Any,
    /// Controlled by the acting/resolving player.
    You,
    /// Controlled by an opponent of the acting/resolving player.
    Opponent,
    /// Controlled by any player other than the acting/resolving player.
    NotYou,
    /// Owned by the acting/resolving player.
    OwnerYou,
    /// Owned by an opponent of the acting/resolving player.
    OwnerOpponent,
    /// Controlled by the related player captured by a trigger (bound at trigger placement).
    RelatedPlayer,
    /// Controlled by the targeted player of the resolving spell or ability.
    TargetPlayer,
    /// Controlled by the targeted opponent of the resolving spell or ability.
    TargetOpponent,
    /// Controlled by the current active player.
    ActivePlayer,
    /// Controlled by the defending player in combat (or opponent of active player if none).
    DefendingPlayer,
    /// Controlled by the controller of the first target permanent in a fixed-target sequence.
    FirstTargetController,
    /// Controlled by this exact player.
    Player(SeatId),
}

/// Dynamic anchor for Copy-preserving stat comparisons on [`PowerFilter`], [`ToughnessFilter`],
/// and [`ManaValueFilter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicStatAnchor {
    ChosenX,
    SourcePower,
    SourceToughness,
    SourceManaValue,
    SourceCounters(crate::effects::CounterKind),
    ControllerLife,
    ControllerHandSize,
    ControllerGraveyardCount,
    ControllerControlledCount(CardType),
    ControllerSubtypeCount(CreatureTypeId),
    TriggerSubjectPower,
    TriggerSubjectManaValue,
    /// Power of the resolving spell or ability's targeted object (CR 608.2h).
    TargetPower,
}

impl DynamicStatAnchor {
    pub(crate) fn rewrite(&mut self, from: CreatureTypeId, to: CreatureTypeId) {
        if let Self::ControllerSubtypeCount(kind) = self
            && *kind == from
        {
            *kind = to;
        }
    }

    pub(crate) fn validate_vocabulary(
        self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        if let Self::ControllerSubtypeCount(kind) = self
            && vocabulary.label(kind).is_none()
        {
            return Err(DefinitionError::ForeignCreatureType);
        }
        Ok(())
    }
}

/// Relation to the source of an activated or triggered ability.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SourceRelation {
    #[default]
    Any,
    /// Matches only the ability's own source permanent.
    SelfObject,
    /// Matches only the permanent currently enchanted, equipped, or fortified by the source.
    AttachedToSelf,
    /// Excludes the ability's own source permanent (checked against the ability source).
    OtherThanAbilitySource,
    /// Unbound trigger/ability source exclusion, materialized via [`ObjectFilter::with_ability_source`].
    OtherThanTriggerSource,
    /// Excludes this exact permanent incarnation.
    OtherThanBound(ObjectId),
    /// Must share a current name with the ability's source permanent.
    SharingNameWithAbilitySource,
    /// Excludes the ability's own source permanent and must share a current name with it.
    OtherThanAbilitySourceSharingName,
    /// Must share a name with a secret/face-up Conspiracy agenda slot on the source.
    MatchesNamedAgendaSlot,
}

/// Power or toughness bound on a creature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PowerFilter {
    #[default]
    Any,
    /// Power is at least `minimum`.
    AtLeast(i16),
    /// Power is at most `maximum`.
    AtMost(u16),
    /// Power equals `exact`.
    Equal(i16),
    /// Power is at most the announced X (bound at activation time).
    AtMostX,
    /// Power equals the announced X (bound at cast/activation time).
    EqualX,
    /// Power is at least the announced X (bound at cast/activation time).
    AtLeastX,
    /// Power compared against a Copy-preserving dynamic world anchor.
    CompareAnchor {
        op: super::Comparison,
        anchor: DynamicStatAnchor,
    },
    /// Power is at most the number of creatures controlled by the acting player.
    AtMostControlledCreatureCount,
    /// Power OR toughness is at most `maximum`.
    PowerOrToughnessAtMost(u16),
}

/// Toughness bound on a creature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToughnessFilter {
    #[default]
    Any,
    /// Toughness is at least `minimum`.
    AtLeast(i16),
    /// Toughness is at most `maximum`.
    AtMost(u16),
    /// Toughness equals `exact`.
    Equal(i16),
    /// Toughness is at most the announced X.
    AtMostX,
    /// Toughness equals the announced X.
    EqualX,
    /// Toughness is at least the announced X.
    AtLeastX,
    /// Toughness compared against a Copy-preserving dynamic world anchor.
    CompareAnchor {
        op: super::Comparison,
        anchor: DynamicStatAnchor,
    },
}

/// Mana-value predicate on a permanent or card.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ManaValueFilter {
    #[default]
    Any,
    AtMost(u32),
    AtLeast(u32),
    Equal(u32),
    /// Mana value is at most the announced X (bound at cast/activation time).
    AtMostX,
    /// Mana value equals the announced X (bound at cast/activation time).
    EqualX,
    /// Mana value is at least the announced X (bound at cast/activation time).
    AtLeastX,
    /// Mana value compared against a Copy-preserving dynamic world anchor.
    CompareAnchor {
        op: super::Comparison,
        anchor: DynamicStatAnchor,
    },
    Even,
    Odd,
}

/// Combat-state predicate on a battlefield creature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CombatFilter {
    #[default]
    Any,
    Attacking,
    Blocking,
    AttackingOrBlocking,
    Blocked,
    Unblocked,
    /// Currently blocking the ability's source creature.
    BlockingSource,
    /// Currently blocked by the ability's source creature.
    BlockedBySource,
    /// Blocking or blocked by the ability's source creature.
    BlockingOrBlockedBySource,
    /// Currently not attacking.
    NotAttacking,
}

/// Compact bitflags for less-frequent object predicates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObjectFilterFlags(u64);

impl ObjectFilterFlags {
    pub const EMPTY: Self = Self(0);
    pub const NON_AURA: Self = Self(1 << 0);
    pub const NON_SAGA: Self = Self(1 << 1);
    pub const WITHOUT_COUNTERS: Self = Self(1 << 2);
    pub const ENTERED_THIS_TURN: Self = Self(1 << 3);
    pub const COMMANDER_ONLY: Self = Self(1 << 4);
    pub const WITH_ANY_COUNTER: Self = Self(1 << 5);
    pub const WITH_PLUS_ONE_COUNTER: Self = Self(1 << 6);
    pub const MODIFIED: Self = Self(1 << 7);
    pub const EQUIPPED: Self = Self(1 << 8);
    pub const ENCHANTED: Self = Self(1 << 9);
    pub const DEALT_DAMAGE_THIS_TURN: Self = Self(1 << 10);
    pub const ATTACKED_THIS_TURN: Self = Self(1 << 11);
    /// Artifact, legendary, or Saga (CR 700.6).
    pub const HISTORIC: Self = Self(1 << 12);
    /// Face-down permanent (morph, manifest, cloak, disguise, etc.).
    pub const FACE_DOWN: Self = Self(1 << 13);
    /// Exiled with the ability source (imprint / linked exile / ability donor).
    pub const EXILED_WITH_SELF: Self = Self(1 << 14);
    /// Dealt damage by the ability source this turn.
    pub const DEALT_DAMAGE_BY_SELF_THIS_TURN: Self = Self(1 << 15);
    /// Put into a graveyard from the battlefield this turn.
    pub const DIED_THIS_TURN: Self = Self(1 << 16);
    /// Matches the color chosen as the ability source entered the battlefield.
    pub const MATCHES_SOURCE_ENTRY_COLOR: Self = Self(1 << 17);
    /// Non-commander object (excludes commanders).
    pub const NOT_COMMANDER: Self = Self(1 << 18);
    /// Matches the creature subtype chosen as the ability source entered the battlefield.
    pub const MATCHES_SOURCE_ENTRY_TYPE: Self = Self(1 << 19);
    /// Matches the card type chosen during resolution or on source entry.
    pub const MATCHES_CHOSEN_CARD_TYPE: Self = Self(1 << 20);
    /// Matches the card name chosen during resolution or on source entry.
    pub const MATCHES_CHOSEN_NAME: Self = Self(1 << 21);
    /// Creature that did not attack this turn.
    pub const DID_NOT_ATTACK_THIS_TURN: Self = Self(1 << 22);
    /// Put into a graveyard from anywhere this turn.
    pub const PUT_INTO_GY_THIS_TURN: Self = Self(1 << 23);
    /// Put into a graveyard from a library this turn.
    pub const PUT_INTO_GY_FROM_LIBRARY_THIS_TURN: Self = Self(1 << 24);
    /// Spell or permanent that was kicked (CR 702.33).
    pub const KICKED: Self = Self(1 << 25);
    /// Creature that is renowned (CR 702.112).
    pub const RENOWNED: Self = Self(1 << 26);
    /// Mount that is saddled this turn (CR 702.171).
    pub const SADDLED: Self = Self(1 << 27);
    /// Creature that is monstrous (CR 701.31).
    pub const MONSTROUS: Self = Self(1 << 28);
    /// Creature that is suspected (CR 701.58).
    pub const SUSPECTED: Self = Self(1 << 29);
    /// Creature that is goaded (CR 701.38).
    pub const GOADED: Self = Self(1 << 30);
    /// Case enchantment that is solved (CR 719.3).
    pub const SOLVED: Self = Self(1 << 31);
    /// Permanent that is prepared.
    pub const PREPARED: Self = Self(1 << 32);
    /// Permanent that was exerted this turn (CR 701.39).
    pub const EXERTED: Self = Self(1 << 33);
    /// Non-historic object (neither artifact, legendary, nor Saga; CR 700.6).
    pub const NON_HISTORIC: Self = Self(1 << 34);
    /// Permanent with at least one -1/-1 counter on it.
    pub const WITH_MINUS_ONE_COUNTER: Self = Self(1 << 35);
    /// Object that has an Adventure alternative face (CR 715).
    pub const HAS_ADVENTURE: Self = Self(1 << 36);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }
}

/// One conjunctive clause of orthogonal object predicates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObjectClause {
    pub any_types: CardTypeMask,
    pub required_types: CardTypeMask,
    pub forbidden_types: CardTypeMask,
    pub required_supertypes: SupertypeMask,
    pub forbidden_supertypes: SupertypeMask,
    pub subtypes: SubtypeFilter,
    pub controller: ControllerFilter,
    pub owner: ControllerFilter,
    pub source_relation: SourceRelation,
    pub color: ObjectColorFilter,
    pub combat: CombatFilter,
    pub power: PowerFilter,
    pub toughness: ToughnessFilter,
    pub mana_value: ManaValueFilter,
    pub keyword: Option<KeywordAbility>,
    pub forbidden_keyword: Option<KeywordAbility>,
    pub tapped: Option<bool>,
    pub token: Option<bool>,
    pub flags: ObjectFilterFlags,
    /// Require at least one counter of this specific permanent counter kind on the object.
    pub required_counter_kind: Option<crate::effects::CounterKind>,
    /// Forbid any counter of this specific permanent counter kind on the object.
    pub forbidden_counter_kind: Option<crate::effects::CounterKind>,
    /// Require the object to have this exact English card name.
    pub exact_name: Option<ExactNameKey>,
}

impl ObjectClause {
    #[must_use]
    pub const fn any() -> Self {
        Self {
            any_types: CardTypeMask::EMPTY,
            required_types: CardTypeMask::EMPTY,
            forbidden_types: CardTypeMask::EMPTY,
            required_supertypes: SupertypeMask::EMPTY,
            forbidden_supertypes: SupertypeMask::EMPTY,
            subtypes: SubtypeFilter::Any,
            controller: ControllerFilter::Any,
            owner: ControllerFilter::Any,
            source_relation: SourceRelation::Any,
            color: ObjectColorFilter::Any,
            combat: CombatFilter::Any,
            power: PowerFilter::Any,
            toughness: ToughnessFilter::Any,
            mana_value: ManaValueFilter::Any,
            keyword: None,
            forbidden_keyword: None,
            tapped: None,
            token: None,
            flags: ObjectFilterFlags::EMPTY,
            required_counter_kind: None,
            forbidden_counter_kind: None,
            exact_name: None,
        }
    }

    #[must_use]
    pub const fn with_counter_kind(mut self, kind: crate::effects::CounterKind) -> Self {
        self.required_counter_kind = Some(kind);
        self
    }

    #[must_use]
    pub const fn without_counter_kind(mut self, kind: crate::effects::CounterKind) -> Self {
        self.forbidden_counter_kind = Some(kind);
        self
    }

    #[must_use]
    pub const fn named(mut self, name: ExactNameKey) -> Self {
        self.exact_name = Some(name);
        self
    }

    #[must_use]
    pub const fn any_types(mut self, mask: CardTypeMask) -> Self {
        self.any_types = self.any_types.union(mask);
        self
    }

    #[must_use]
    pub const fn required_types(mut self, mask: CardTypeMask) -> Self {
        self.required_types = self.required_types.union(mask);
        self
    }

    #[must_use]
    pub const fn required_type(self, kind: CardType) -> Self {
        self.required_types(CardTypeMask::from_card_type(kind))
    }

    #[must_use]
    pub const fn self_object(mut self) -> Self {
        self.source_relation = SourceRelation::SelfObject;
        self
    }

    #[must_use]
    pub const fn attached_to_self(mut self) -> Self {
        self.source_relation = SourceRelation::AttachedToSelf;
        self
    }

    #[must_use]
    pub const fn matches_named_agenda_slot(mut self) -> Self {
        self.source_relation = SourceRelation::MatchesNamedAgendaSlot;
        self
    }

    #[must_use]
    pub const fn forbid_types(mut self, mask: CardTypeMask) -> Self {
        self.forbidden_types = self.forbidden_types.union(mask);
        self
    }

    #[must_use]
    pub const fn creature() -> Self {
        Self::any().any_types(CardTypeMask::CREATURE)
    }

    #[must_use]
    pub const fn artifact() -> Self {
        Self::any().any_types(CardTypeMask::ARTIFACT)
    }

    #[must_use]
    pub const fn enchantment() -> Self {
        Self::any().any_types(CardTypeMask::ENCHANTMENT)
    }

    #[must_use]
    pub const fn land() -> Self {
        Self::any().any_types(CardTypeMask::LAND)
    }

    #[must_use]
    pub const fn planeswalker() -> Self {
        Self::any().any_types(CardTypeMask::PLANESWALKER)
    }

    #[must_use]
    pub const fn instant() -> Self {
        Self::any().any_types(CardTypeMask::INSTANT)
    }

    #[must_use]
    pub const fn sorcery() -> Self {
        Self::any().any_types(CardTypeMask::SORCERY)
    }

    #[must_use]
    pub const fn permanent() -> Self {
        Self::any().any_types(CardTypeMask::PERMANENT)
    }

    #[must_use]
    pub const fn nonland(self) -> Self {
        self.forbid_types(CardTypeMask::LAND)
    }

    #[must_use]
    pub const fn noncreature(self) -> Self {
        self.forbid_types(CardTypeMask::CREATURE)
    }

    #[must_use]
    pub const fn nonenchantment(self) -> Self {
        self.forbid_types(CardTypeMask::ENCHANTMENT)
    }

    #[must_use]
    pub const fn legendary(mut self) -> Self {
        self.required_supertypes = self.required_supertypes.union(SupertypeMask::LEGENDARY);
        self
    }

    #[must_use]
    pub const fn nonlegendary(mut self) -> Self {
        self.forbidden_supertypes = self.forbidden_supertypes.union(SupertypeMask::LEGENDARY);
        self
    }

    #[must_use]
    pub const fn controlled(mut self) -> Self {
        self.controller = ControllerFilter::You;
        self
    }

    #[must_use]
    pub const fn opposing(mut self) -> Self {
        self.controller = ControllerFilter::Opponent;
        self
    }

    #[must_use]
    pub const fn not_controlled(mut self) -> Self {
        self.controller = ControllerFilter::NotYou;
        self
    }

    #[must_use]
    pub const fn owned(mut self) -> Self {
        self.controller = ControllerFilter::OwnerYou;
        self
    }

    #[must_use]
    pub const fn of_related_player(mut self) -> Self {
        self.controller = ControllerFilter::RelatedPlayer;
        self
    }

    #[must_use]
    pub const fn of_target_player(mut self) -> Self {
        self.controller = ControllerFilter::TargetPlayer;
        self
    }

    #[must_use]
    pub const fn of_target_opponent(mut self) -> Self {
        self.controller = ControllerFilter::TargetOpponent;
        self
    }

    #[must_use]
    pub const fn of_player(mut self, seat: SeatId) -> Self {
        self.controller = ControllerFilter::Player(seat);
        self
    }

    #[must_use]
    pub const fn with_owner(mut self, owner: ControllerFilter) -> Self {
        self.owner = owner;
        self
    }

    #[must_use]
    pub const fn with_subtype(mut self, kind: CreatureTypeId) -> Self {
        self.subtypes = SubtypeFilter::Has(kind);
        self
    }

    #[must_use]
    pub const fn without_subtype(mut self, kind: CreatureTypeId) -> Self {
        self.subtypes = SubtypeFilter::Lacks(kind);
        self
    }

    #[must_use]
    pub const fn among_subtypes(mut self, kinds: [CreatureTypeId; 3]) -> Self {
        self.subtypes = SubtypeFilter::AnyOf(kinds);
        self
    }

    #[must_use]
    pub const fn outside_subtypes(mut self, kinds: [CreatureTypeId; 3]) -> Self {
        self.subtypes = SubtypeFilter::NoneOf(kinds);
        self
    }

    #[must_use]
    pub const fn except_chosen_slot(mut self, slot: CreatureTypeSlot) -> Self {
        self.subtypes = SubtypeFilter::ExceptChosenSlot(slot);
        self
    }

    #[must_use]
    pub const fn of_chosen_slot(mut self, slot: CreatureTypeSlot) -> Self {
        self.subtypes = SubtypeFilter::MatchesChosenSlot(slot);
        self
    }

    #[must_use]
    pub const fn outside_named_slots(mut self) -> Self {
        self.subtypes = SubtypeFilter::OutsideNamedSlots;
        self
    }

    #[must_use]
    pub const fn with_color(mut self, color: ObjectColorFilter) -> Self {
        self.color = color;
        self
    }

    #[must_use]
    pub const fn combat(mut self, combat: CombatFilter) -> Self {
        self.combat = combat;
        self
    }

    #[must_use]
    pub const fn with_power(mut self, power: PowerFilter) -> Self {
        self.power = power;
        self
    }

    #[must_use]
    pub const fn with_toughness(mut self, toughness: ToughnessFilter) -> Self {
        self.toughness = toughness;
        self
    }

    #[must_use]
    pub const fn with_mana_value(mut self, mana_value: ManaValueFilter) -> Self {
        self.mana_value = mana_value;
        self
    }

    #[must_use]
    pub const fn with_keyword(mut self, keyword: KeywordAbility) -> Self {
        self.keyword = Some(keyword);
        self
    }

    #[must_use]
    pub const fn without_keyword(mut self, keyword: KeywordAbility) -> Self {
        self.forbidden_keyword = Some(keyword);
        self
    }

    #[must_use]
    pub const fn with_land_subtype(mut self, kind: LandType) -> Self {
        self.subtypes = SubtypeFilter::Land(kind);
        self
    }

    #[must_use]
    pub const fn without_land_subtype(mut self, kind: LandType) -> Self {
        self.subtypes = SubtypeFilter::NotLand(kind);
        self
    }

    #[must_use]
    pub const fn with_artifact_subtype(mut self, kind: ArtifactSubtype) -> Self {
        self.subtypes = SubtypeFilter::Artifact(kind);
        self
    }

    #[must_use]
    pub const fn with_enchantment_subtype(mut self, kind: EnchantmentSubtype) -> Self {
        self.subtypes = SubtypeFilter::Enchantment(kind);
        self
    }

    #[must_use]
    pub const fn with_planeswalker_subtype(mut self, kind: super::PlaneswalkerSubtype) -> Self {
        self.subtypes = SubtypeFilter::Planeswalker(kind);
        self
    }

    #[must_use]
    pub const fn aura(mut self) -> Self {
        self.subtypes = SubtypeFilter::Aura;
        self
    }

    #[must_use]
    pub const fn with_flags(mut self, flags: ObjectFilterFlags) -> Self {
        self.flags = self.flags.union(flags);
        self
    }

    #[must_use]
    pub const fn tapped(mut self) -> Self {
        self.tapped = Some(true);
        self
    }

    #[must_use]
    pub const fn untapped(mut self) -> Self {
        self.tapped = Some(false);
        self
    }

    #[must_use]
    pub const fn nontoken(mut self) -> Self {
        self.token = Some(false);
        self
    }

    #[must_use]
    pub const fn token_only(mut self) -> Self {
        self.token = Some(true);
        self
    }

    #[must_use]
    pub const fn other_than_ability_source(mut self) -> Self {
        self.source_relation = SourceRelation::OtherThanAbilitySource;
        self
    }

    #[must_use]
    pub const fn other_than_trigger_source(mut self) -> Self {
        self.source_relation = SourceRelation::OtherThanTriggerSource;
        self
    }

    #[must_use]
    pub const fn other_than(mut self, id: ObjectId) -> Self {
        self.source_relation = SourceRelation::OtherThanBound(id);
        self
    }

    #[must_use]
    pub const fn sharing_name_with_source(mut self) -> Self {
        self.source_relation = SourceRelation::SharingNameWithAbilitySource;
        self
    }

    #[must_use]
    pub const fn non_aura(mut self) -> Self {
        self.flags = self.flags.union(ObjectFilterFlags::NON_AURA);
        self
    }

    #[must_use]
    pub const fn non_saga(mut self) -> Self {
        self.flags = self.flags.union(ObjectFilterFlags::NON_SAGA);
        self
    }

    #[must_use]
    pub const fn without_counters(mut self) -> Self {
        self.flags = self.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
        self
    }

    #[must_use]
    pub const fn entered_this_turn(mut self) -> Self {
        self.flags = self.flags.union(ObjectFilterFlags::ENTERED_THIS_TURN);
        self
    }

    #[must_use]
    pub const fn commander(mut self) -> Self {
        self.flags = self.flags.union(ObjectFilterFlags::COMMANDER_ONLY);
        self
    }

    /// True if this clause guarantees a creature object.
    #[must_use]
    pub const fn guarantees_creature(self) -> bool {
        self.any_types.0 == CardTypeMask::CREATURE.0
            || matches!(
                self.combat,
                CombatFilter::Attacking
                    | CombatFilter::Blocking
                    | CombatFilter::AttackingOrBlocking
                    | CombatFilter::Blocked
                    | CombatFilter::Unblocked
                    | CombatFilter::BlockingSource
                    | CombatFilter::BlockedBySource
                    | CombatFilter::BlockingOrBlockedBySource
            )
    }

    /// Bind unbound trigger/ability source exclusions to `source`.
    #[must_use]
    pub const fn with_ability_source(mut self, source: ObjectId) -> Self {
        if matches!(self.source_relation, SourceRelation::OtherThanTriggerSource) {
            self.source_relation = SourceRelation::OtherThanBound(source);
        }
        self
    }

    /// Bind `ControllerFilter::RelatedPlayer` to a concrete player seat.
    #[must_use]
    pub const fn bind_related_player(mut self, seat: SeatId) -> Self {
        if matches!(self.controller, ControllerFilter::RelatedPlayer) {
            self.controller = ControllerFilter::Player(seat);
        }
        if matches!(self.owner, ControllerFilter::RelatedPlayer) {
            self.owner = ControllerFilter::Player(seat);
        }
        self
    }

    /// Whether this clause matches enchantment subtype exclusions.
    #[must_use]
    pub fn matches_enchantment_exclusions(
        self,
        attachment_kind: Option<crate::attachment::AttachmentKind>,
        enchantment_subtypes: &BTreeSet<EnchantmentSubtype>,
    ) -> bool {
        if self.flags.contains(ObjectFilterFlags::NON_AURA)
            && matches!(
                attachment_kind,
                Some(crate::attachment::AttachmentKind::Aura(_))
            )
        {
            return false;
        }
        if self.flags.contains(ObjectFilterFlags::NON_SAGA)
            && enchantment_subtypes.contains(&EnchantmentSubtype::Saga)
        {
            return false;
        }
        true
    }

    /// Evaluate the non-relational object predicates (types, supertypes, subtypes, color, keyword,
    /// enchantment subtype exclusions, fixed power bounds, and mana value) against evaluated characteristics.
    #[must_use]
    pub fn matches_static_characteristics(
        self,
        p: &crate::characteristics::ObjectCharacteristics,
        excluded_subtypes: &BTreeSet<CreatureTypeId>,
        mana_value: impl FnOnce() -> Option<spellwake_contracts::RulesInteger>,
    ) -> bool {
        if !self.any_types.matches_any(&p.current_types)
            || !self.required_types.matches_all(&p.current_types)
            || !self.forbidden_types.matches_none(&p.current_types)
        {
            return false;
        }
        if !self.required_supertypes.matches_all(&p.current_supertypes)
            || !self
                .forbidden_supertypes
                .matches_none(&p.current_supertypes)
        {
            return false;
        }
        let is_aura = matches!(
            p.attachment_kind,
            Some(crate::attachment::AttachmentKind::Aura(_))
        );
        let subtype_ok = self
            .subtypes
            .matches_sets(
                &p.current_creature_subtypes,
                &p.current_land_subtypes,
                &p.current_artifact_subtypes,
                &p.current_enchantment_subtypes,
                &p.current_planeswalker_subtypes,
                &p.current_spell_subtypes,
                is_aura,
            )
            .unwrap_or_else(|| match self.subtypes {
                SubtypeFilter::ExceptChosenSlot(_) | SubtypeFilter::OutsideNamedSlots => {
                    p.current_creature_subtypes.is_disjoint(excluded_subtypes)
                }
                SubtypeFilter::MatchesChosenSlot(_) => false,
                _ => false,
            });
        if !subtype_ok {
            return false;
        }
        if !self.color.matches(&p.current_colors) {
            return false;
        }
        if let Some(keyword) = self.keyword
            && !p.has_keyword(keyword)
        {
            return false;
        }
        if let Some(keyword) = self.forbidden_keyword
            && p.has_keyword(keyword)
        {
            return false;
        }
        if !self.matches_enchantment_exclusions(p.attachment_kind, &p.current_enchantment_subtypes)
        {
            return false;
        }
        let is_historic = p.current_types.contains(&CardType::Artifact)
            || p.current_supertypes.contains(&Supertype::Legendary)
            || p.current_enchantment_subtypes
                .contains(&EnchantmentSubtype::Saga);
        if self.flags.contains(ObjectFilterFlags::HISTORIC) && !is_historic {
            return false;
        }
        if self.flags.contains(ObjectFilterFlags::NON_HISTORIC) && is_historic {
            return false;
        }
        if let Some(exact) = self.exact_name
            && !exact.matches_exact_names(&p.current_names)
        {
            return false;
        }
        if let Some(kind) = self.required_counter_kind
            && !p.present_counter_kinds.contains(&kind)
        {
            return false;
        }
        if let Some(kind) = self.forbidden_counter_kind
            && p.present_counter_kinds.contains(&kind)
        {
            return false;
        }
        if self.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS) && p.has_any_counter {
            return false;
        }
        if self.flags.contains(ObjectFilterFlags::WITH_ANY_COUNTER) && !p.has_any_counter {
            return false;
        }
        if self
            .flags
            .contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
            && !p.has_plus_one_counter
        {
            return false;
        }
        if self
            .flags
            .contains(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
            && !p.has_minus_one_counter
        {
            return false;
        }
        if self.flags.contains(ObjectFilterFlags::SADDLED) && !p.saddled {
            return false;
        }
        if self.flags.contains(ObjectFilterFlags::HAS_ADVENTURE) && !p.defined.has_adventure {
            return false;
        }
        if self.flags.contains(ObjectFilterFlags::COMMANDER_ONLY) && p.commander_owner.is_none() {
            return false;
        }
        if self.flags.contains(ObjectFilterFlags::NOT_COMMANDER) && p.commander_owner.is_some() {
            return false;
        }
        let power_ok = match self.power {
            PowerFilter::Any => true,
            PowerFilter::AtLeast(min) => p
                .power_toughness
                .as_ref()
                .is_some_and(|pt| pt.derived.power >= i64::from(min).into()),
            PowerFilter::AtMost(max) => p
                .power_toughness
                .as_ref()
                .is_some_and(|pt| pt.derived.power <= i64::from(max).into()),
            PowerFilter::Equal(exact) => p
                .power_toughness
                .as_ref()
                .is_some_and(|pt| pt.derived.power == i64::from(exact).into()),
            PowerFilter::PowerOrToughnessAtMost(max) => {
                p.power_toughness.as_ref().is_some_and(|pt| {
                    pt.derived.power <= i64::from(max).into()
                        || pt.derived.toughness <= i64::from(max).into()
                })
            }
            PowerFilter::AtMostX | PowerFilter::EqualX | PowerFilter::AtLeastX => false,
            PowerFilter::AtMostControlledCreatureCount | PowerFilter::CompareAnchor { .. } => true,
        };
        if !power_ok {
            return false;
        }
        let toughness_ok = match self.toughness {
            ToughnessFilter::Any => true,
            ToughnessFilter::AtLeast(min) => p
                .power_toughness
                .as_ref()
                .is_some_and(|pt| pt.derived.toughness >= i64::from(min).into()),
            ToughnessFilter::AtMost(max) => p
                .power_toughness
                .as_ref()
                .is_some_and(|pt| pt.derived.toughness <= i64::from(max).into()),
            ToughnessFilter::Equal(exact) => p
                .power_toughness
                .as_ref()
                .is_some_and(|pt| pt.derived.toughness == i64::from(exact).into()),
            ToughnessFilter::AtMostX | ToughnessFilter::EqualX | ToughnessFilter::AtLeastX => false,
            ToughnessFilter::CompareAnchor { .. } => true,
        };
        if !toughness_ok {
            return false;
        }
        match self.mana_value {
            ManaValueFilter::Any => true,
            ManaValueFilter::AtMost(max) => {
                mana_value().is_some_and(|value| value <= i64::from(max).into())
            }
            ManaValueFilter::AtLeast(min) => {
                mana_value().is_some_and(|value| value >= i64::from(min).into())
            }
            ManaValueFilter::Equal(exact) => {
                mana_value().is_some_and(|value| value == i64::from(exact).into())
            }
            ManaValueFilter::AtMostX | ManaValueFilter::EqualX | ManaValueFilter::AtLeastX => false,
            ManaValueFilter::CompareAnchor { .. } => true,
            ManaValueFilter::Even => mana_value().is_some_and(|value| {
                value.nonnegative_half_rounded_up() == value.nonnegative_half_rounded_down()
            }),
            ManaValueFilter::Odd => mana_value().is_some_and(|value| {
                value.nonnegative_half_rounded_up() != value.nonnegative_half_rounded_down()
            }),
        }
    }
}

/// Composable object filter supporting one conjunctive clause or a disjunction of two clauses.
// ObjectFilter is intentionally Copy and zero-allocation; Either holds two inline ObjectClause values.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectFilter {
    /// Match objects satisfying every predicate in this clause.
    Clause(ObjectClause),
    /// Match objects satisfying either of two clauses.
    Either(ObjectClause, ObjectClause),
}

impl Default for ObjectFilter {
    fn default() -> Self {
        Self::Clause(ObjectClause::any())
    }
}

impl ObjectFilter {
    #[must_use]
    pub const fn any() -> Self {
        Self::Clause(ObjectClause::any())
    }

    /// Match only the ability's own source permanent.
    #[must_use]
    pub const fn self_object() -> Self {
        Self::Clause(ObjectClause::any().self_object())
    }

    /// Match only the permanent currently enchanted, equipped, or fortified by the source.
    #[must_use]
    pub const fn attached_to_self(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.attached_to_self()),
            Self::Either(a, b) => Self::Either(a.attached_to_self(), b.attached_to_self()),
        }
    }

    /// Require `kind` among the object's card types (conjunctive with `any_types`).
    #[must_use]
    pub const fn with_required_type(self, kind: CardType) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.required_type(kind)),
            Self::Either(a, b) => Self::Either(a.required_type(kind), b.required_type(kind)),
        }
    }

    /// Combine a single-clause filter with `SelfObject` (`self or <filter>`).
    #[must_use]
    pub const fn or_self(self) -> Self {
        match self {
            Self::Clause(a) => Self::Either(ObjectClause::any().self_object(), a),
            Self::Either(a, b) => Self::Either(a, b),
        }
    }

    /// Restrict to the ability's own source permanent while keeping existing predicates (e.g. creature).
    #[must_use]
    pub const fn of_self(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.self_object()),
            Self::Either(a, b) => Self::Either(a.self_object(), b.self_object()),
        }
    }

    /// True when any clause of this filter requires `flag`.
    #[must_use]
    pub const fn has_flag(self, flag: ObjectFilterFlags) -> bool {
        match self {
            Self::Clause(a) => a.flags.contains(flag),
            Self::Either(a, b) => a.flags.contains(flag) || b.flags.contains(flag),
        }
    }

    #[must_use]
    pub const fn from_clause(clause: ObjectClause) -> Self {
        Self::Clause(clause)
    }

    #[must_use]
    pub const fn either(first: ObjectClause, second: ObjectClause) -> Self {
        Self::Either(first, second)
    }

    #[must_use]
    pub const fn creature() -> Self {
        Self::Clause(ObjectClause::creature())
    }

    #[must_use]
    pub const fn artifact() -> Self {
        Self::Clause(ObjectClause::artifact())
    }

    #[must_use]
    pub const fn enchantment() -> Self {
        Self::Clause(ObjectClause::enchantment())
    }

    #[must_use]
    pub const fn land() -> Self {
        Self::Clause(ObjectClause::land())
    }

    #[must_use]
    pub const fn planeswalker() -> Self {
        Self::Clause(ObjectClause::planeswalker())
    }

    #[must_use]
    pub const fn instant() -> Self {
        Self::Clause(ObjectClause::instant())
    }

    #[must_use]
    pub const fn sorcery() -> Self {
        Self::Clause(ObjectClause::sorcery())
    }

    #[must_use]
    pub const fn permanent() -> Self {
        Self::Clause(ObjectClause::permanent())
    }

    #[must_use]
    pub const fn any_types(mask: CardTypeMask) -> Self {
        Self::Clause(ObjectClause::any().any_types(mask))
    }

    #[must_use]
    pub const fn artifact_or_enchantment() -> Self {
        Self::any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::ENCHANTMENT))
    }

    #[must_use]
    pub const fn artifact_or_creature() -> Self {
        Self::any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE))
    }

    #[must_use]
    pub const fn artifact_or_land() -> Self {
        Self::any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::LAND))
    }

    #[must_use]
    pub const fn creature_or_planeswalker() -> Self {
        Self::any_types(CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER))
    }

    #[must_use]
    pub const fn creature_or_enchantment() -> Self {
        Self::any_types(CardTypeMask::CREATURE.union(CardTypeMask::ENCHANTMENT))
    }

    #[must_use]
    pub const fn creature_or_land() -> Self {
        Self::any_types(CardTypeMask::CREATURE.union(CardTypeMask::LAND))
    }

    #[must_use]
    pub const fn instant_or_sorcery() -> Self {
        Self::any_types(CardTypeMask::INSTANT.union(CardTypeMask::SORCERY))
    }

    #[must_use]
    pub const fn nonland() -> Self {
        Self::Clause(ObjectClause::any().nonland())
    }

    #[must_use]
    pub const fn noncreature() -> Self {
        Self::Clause(ObjectClause::any().noncreature())
    }

    #[must_use]
    pub const fn controlled(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.controlled()),
            Self::Either(a, b) => Self::Either(a.controlled(), b.controlled()),
        }
    }

    #[must_use]
    pub const fn opposing(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.opposing()),
            Self::Either(a, b) => Self::Either(a.opposing(), b.opposing()),
        }
    }

    #[must_use]
    pub const fn not_controlled(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.not_controlled()),
            Self::Either(a, b) => Self::Either(a.not_controlled(), b.not_controlled()),
        }
    }

    #[must_use]
    pub const fn owned(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.owned()),
            Self::Either(a, b) => Self::Either(a.owned(), b.owned()),
        }
    }

    #[must_use]
    pub const fn of_related_player(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.of_related_player()),
            Self::Either(a, b) => Self::Either(a.of_related_player(), b.of_related_player()),
        }
    }

    #[must_use]
    pub const fn of_target_player(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.of_target_player()),
            Self::Either(a, b) => Self::Either(a.of_target_player(), b.of_target_player()),
        }
    }

    #[must_use]
    pub const fn of_target_opponent(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.of_target_opponent()),
            Self::Either(a, b) => Self::Either(a.of_target_opponent(), b.of_target_opponent()),
        }
    }

    #[must_use]
    pub const fn of_player(self, seat: SeatId) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.of_player(seat)),
            Self::Either(a, b) => Self::Either(a.of_player(seat), b.of_player(seat)),
        }
    }

    #[must_use]
    pub const fn with_owner(self, owner: ControllerFilter) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_owner(owner)),
            Self::Either(a, b) => Self::Either(a.with_owner(owner), b.with_owner(owner)),
        }
    }

    #[must_use]
    pub const fn legendary(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.legendary()),
            Self::Either(a, b) => Self::Either(a.legendary(), b.legendary()),
        }
    }

    #[must_use]
    pub const fn nonlegendary(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.nonlegendary()),
            Self::Either(a, b) => Self::Either(a.nonlegendary(), b.nonlegendary()),
        }
    }

    #[must_use]
    pub const fn nonenchantment(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.nonenchantment()),
            Self::Either(a, b) => Self::Either(a.nonenchantment(), b.nonenchantment()),
        }
    }

    #[must_use]
    pub const fn nontoken(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.nontoken()),
            Self::Either(a, b) => Self::Either(a.nontoken(), b.nontoken()),
        }
    }

    /// Match only token permanents.
    #[must_use]
    pub const fn token(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.token_only()),
            Self::Either(a, b) => Self::Either(a.token_only(), b.token_only()),
        }
    }

    #[must_use]
    pub const fn tapped(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.tapped()),
            Self::Either(a, b) => Self::Either(a.tapped(), b.tapped()),
        }
    }

    #[must_use]
    pub const fn attacking(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.combat(CombatFilter::Attacking)),
            Self::Either(a, b) => Self::Either(
                a.combat(CombatFilter::Attacking),
                b.combat(CombatFilter::Attacking),
            ),
        }
    }

    #[must_use]
    pub const fn attacking_or_blocking(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.combat(CombatFilter::AttackingOrBlocking)),
            Self::Either(a, b) => Self::Either(
                a.combat(CombatFilter::AttackingOrBlocking),
                b.combat(CombatFilter::AttackingOrBlocking),
            ),
        }
    }

    #[must_use]
    pub const fn with_keyword(self, keyword: KeywordAbility) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_keyword(keyword)),
            Self::Either(a, b) => Self::Either(a.with_keyword(keyword), b.with_keyword(keyword)),
        }
    }

    #[must_use]
    pub const fn without_keyword(self, keyword: KeywordAbility) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.without_keyword(keyword)),
            Self::Either(a, b) => {
                Self::Either(a.without_keyword(keyword), b.without_keyword(keyword))
            }
        }
    }

    #[must_use]
    pub const fn with_toughness(self, toughness: ToughnessFilter) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_toughness(toughness)),
            Self::Either(a, b) => {
                Self::Either(a.with_toughness(toughness), b.with_toughness(toughness))
            }
        }
    }

    #[must_use]
    pub const fn with_land_subtype(self, kind: LandType) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_land_subtype(kind)),
            Self::Either(a, b) => {
                Self::Either(a.with_land_subtype(kind), b.with_land_subtype(kind))
            }
        }
    }

    #[must_use]
    pub const fn with_artifact_subtype(self, kind: ArtifactSubtype) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_artifact_subtype(kind)),
            Self::Either(a, b) => {
                Self::Either(a.with_artifact_subtype(kind), b.with_artifact_subtype(kind))
            }
        }
    }

    #[must_use]
    pub const fn with_enchantment_subtype(self, kind: EnchantmentSubtype) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_enchantment_subtype(kind)),
            Self::Either(a, b) => Self::Either(
                a.with_enchantment_subtype(kind),
                b.with_enchantment_subtype(kind),
            ),
        }
    }

    #[must_use]
    pub const fn with_planeswalker_subtype(self, kind: super::PlaneswalkerSubtype) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_planeswalker_subtype(kind)),
            Self::Either(a, b) => Self::Either(
                a.with_planeswalker_subtype(kind),
                b.with_planeswalker_subtype(kind),
            ),
        }
    }

    #[must_use]
    pub const fn aura(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.aura()),
            Self::Either(a, b) => Self::Either(a.aura(), b.aura()),
        }
    }

    #[must_use]
    pub const fn combat(self, combat: CombatFilter) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.combat(combat)),
            Self::Either(a, b) => Self::Either(a.combat(combat), b.combat(combat)),
        }
    }

    #[must_use]
    pub const fn with_flags(self, flags: ObjectFilterFlags) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_flags(flags)),
            Self::Either(a, b) => Self::Either(a.with_flags(flags), b.with_flags(flags)),
        }
    }

    #[must_use]
    pub const fn with_subtype(self, kind: CreatureTypeId) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_subtype(kind)),
            Self::Either(a, b) => Self::Either(a.with_subtype(kind), b.with_subtype(kind)),
        }
    }

    #[must_use]
    pub const fn without_subtype(self, kind: CreatureTypeId) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.without_subtype(kind)),
            Self::Either(a, b) => Self::Either(a.without_subtype(kind), b.without_subtype(kind)),
        }
    }

    #[must_use]
    pub const fn among_subtypes(self, kinds: [CreatureTypeId; 3]) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.among_subtypes(kinds)),
            Self::Either(a, b) => Self::Either(a.among_subtypes(kinds), b.among_subtypes(kinds)),
        }
    }

    #[must_use]
    pub const fn outside_subtypes(self, kinds: [CreatureTypeId; 3]) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.outside_subtypes(kinds)),
            Self::Either(a, b) => {
                Self::Either(a.outside_subtypes(kinds), b.outside_subtypes(kinds))
            }
        }
    }

    #[must_use]
    pub const fn except_chosen_slot(self, slot: CreatureTypeSlot) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.except_chosen_slot(slot)),
            Self::Either(a, b) => {
                Self::Either(a.except_chosen_slot(slot), b.except_chosen_slot(slot))
            }
        }
    }

    #[must_use]
    pub const fn of_chosen_slot(self, slot: CreatureTypeSlot) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.of_chosen_slot(slot)),
            Self::Either(a, b) => Self::Either(a.of_chosen_slot(slot), b.of_chosen_slot(slot)),
        }
    }

    #[must_use]
    pub const fn outside_named_slots(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.outside_named_slots()),
            Self::Either(a, b) => Self::Either(a.outside_named_slots(), b.outside_named_slots()),
        }
    }

    #[must_use]
    pub const fn with_color(self, color: ObjectColorFilter) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_color(color)),
            Self::Either(a, b) => Self::Either(a.with_color(color), b.with_color(color)),
        }
    }

    #[must_use]
    pub const fn with_power(self, power: PowerFilter) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_power(power)),
            Self::Either(a, b) => Self::Either(a.with_power(power), b.with_power(power)),
        }
    }

    #[must_use]
    pub const fn with_mana_value(self, mv: ManaValueFilter) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_mana_value(mv)),
            Self::Either(a, b) => Self::Either(a.with_mana_value(mv), b.with_mana_value(mv)),
        }
    }

    #[must_use]
    pub const fn other_than_ability_source(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.other_than_ability_source()),
            Self::Either(a, b) => {
                Self::Either(a.other_than_ability_source(), b.other_than_ability_source())
            }
        }
    }

    #[must_use]
    pub const fn other_than_trigger_source(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.other_than_trigger_source()),
            Self::Either(a, b) => {
                Self::Either(a.other_than_trigger_source(), b.other_than_trigger_source())
            }
        }
    }

    #[must_use]
    pub const fn other_than(self, id: ObjectId) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.other_than(id)),
            Self::Either(a, b) => Self::Either(a.other_than(id), b.other_than(id)),
        }
    }

    #[must_use]
    pub const fn sharing_name_with_source(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.sharing_name_with_source()),
            Self::Either(a, b) => {
                Self::Either(a.sharing_name_with_source(), b.sharing_name_with_source())
            }
        }
    }

    #[must_use]
    pub const fn non_aura(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.non_aura()),
            Self::Either(a, b) => Self::Either(a.non_aura(), b.non_aura()),
        }
    }

    #[must_use]
    pub const fn non_saga(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.non_saga()),
            Self::Either(a, b) => Self::Either(a.non_saga(), b.non_saga()),
        }
    }

    #[must_use]
    pub const fn without_counters(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.without_counters()),
            Self::Either(a, b) => Self::Either(a.without_counters(), b.without_counters()),
        }
    }

    #[must_use]
    pub const fn entered_this_turn(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.entered_this_turn()),
            Self::Either(a, b) => Self::Either(a.entered_this_turn(), b.entered_this_turn()),
        }
    }

    #[must_use]
    pub const fn commander(self) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.commander()),
            Self::Either(a, b) => Self::Either(a.commander(), b.commander()),
        }
    }

    /// Inspect the clauses contained in this filter (one or two).
    #[must_use]
    pub const fn clauses(&self) -> (ObjectClause, Option<ObjectClause>) {
        match *self {
            Self::Clause(a) => (a, None),
            Self::Either(a, b) => (a, Some(b)),
        }
    }

    /// True if every clause guarantees a creature object.
    #[must_use]
    pub const fn guarantees_creature(self) -> bool {
        match self {
            Self::Clause(a) => a.guarantees_creature(),
            Self::Either(a, b) => a.guarantees_creature() && b.guarantees_creature(),
        }
    }

    /// True if every clause only matches damageable permanents (creatures and/or planeswalkers).
    #[must_use]
    pub const fn guarantees_damageable(self) -> bool {
        const DAMAGEABLE: u16 =
            CardTypeMask::CREATURE.0 | CardTypeMask::PLANESWALKER.0 | CardTypeMask::BATTLE.0;
        const fn ok(c: ObjectClause) -> bool {
            !c.any_types.is_empty() && (c.any_types.0 & !DAMAGEABLE) == 0
        }
        match self {
            Self::Clause(a) => ok(a),
            Self::Either(a, b) => ok(a) && ok(b),
        }
    }

    /// True if any clause can match an instant or sorcery card.
    #[must_use]
    pub const fn allows_instant_or_sorcery(self) -> bool {
        const fn allows(c: ObjectClause) -> bool {
            c.any_types.contains(CardType::Instant) || c.any_types.contains(CardType::Sorcery)
        }
        match self {
            Self::Clause(a) => allows(a),
            Self::Either(a, b) => allows(a) || allows(b),
        }
    }

    /// True if this filter specifically targets opposing permanents with even mana value.
    #[must_use]
    pub const fn is_opposing_even_mana_value(self) -> bool {
        matches!(
            self,
            Self::Clause(ObjectClause {
                controller: ControllerFilter::Opponent,
                mana_value: ManaValueFilter::Even,
                ..
            })
        )
    }

    /// True if this filter is a single clause matching any creature/planeswalker/nonland/etc.
    /// with an unbound ability-source exclusion that is allowed only in activated abilities
    /// (for example "target creature or other planeswalker").
    #[must_use]
    pub const fn is_creature_or_other_planeswalker(self) -> bool {
        matches!(
            self,
            Self::Either(
                ObjectClause { any_types: a, .. },
                ObjectClause {
                    any_types: b,
                    source_relation: SourceRelation::OtherThanTriggerSource | SourceRelation::OtherThanBound(_),
                    ..
                },
            ) if a.0 == CardTypeMask::CREATURE.0 && b.0 == CardTypeMask::PLANESWALKER.0
        )
    }

    /// True if any clause excludes the ability source directly.
    #[must_use]
    pub const fn excludes_ability_source(self) -> bool {
        const fn excludes(r: SourceRelation) -> bool {
            matches!(
                r,
                SourceRelation::OtherThanAbilitySource
                    | SourceRelation::OtherThanAbilitySourceSharingName
            )
        }
        match self {
            Self::Clause(a) => excludes(a.source_relation),
            Self::Either(a, b) => excludes(a.source_relation) || excludes(b.source_relation),
        }
    }

    /// True if any clause carries an unbound trigger-source exclusion.
    #[must_use]
    pub const fn has_unbound_trigger_exclusion(self) -> bool {
        match self {
            Self::Clause(a) => matches!(a.source_relation, SourceRelation::OtherThanTriggerSource),
            Self::Either(a, b) => {
                matches!(a.source_relation, SourceRelation::OtherThanTriggerSource)
                    || matches!(b.source_relation, SourceRelation::OtherThanTriggerSource)
            }
        }
    }

    /// True if any clause carries an already-bound permanent exclusion.
    #[must_use]
    pub const fn has_bound_exclusion(self) -> bool {
        match self {
            Self::Clause(a) => matches!(a.source_relation, SourceRelation::OtherThanBound(_)),
            Self::Either(a, b) => {
                matches!(a.source_relation, SourceRelation::OtherThanBound(_))
                    || matches!(b.source_relation, SourceRelation::OtherThanBound(_))
            }
        }
    }

    /// True if any clause requires sharing a name with the ability source.
    #[must_use]
    pub const fn requires_same_name_as_source(self) -> bool {
        const fn shares(r: SourceRelation) -> bool {
            matches!(
                r,
                SourceRelation::SharingNameWithAbilitySource
                    | SourceRelation::OtherThanAbilitySourceSharingName
            )
        }
        match self {
            Self::Clause(a) => shares(a.source_relation),
            Self::Either(a, b) => shares(a.source_relation) || shares(b.source_relation),
        }
    }

    /// True if any clause binds power to the announced X.
    #[must_use]
    pub const fn has_power_at_most_x(self) -> bool {
        const fn has_x(c: ObjectClause) -> bool {
            matches!(
                c.power,
                PowerFilter::AtMostX
                    | PowerFilter::EqualX
                    | PowerFilter::AtLeastX
                    | PowerFilter::CompareAnchor {
                        anchor: DynamicStatAnchor::ChosenX,
                        ..
                    }
            ) || matches!(
                c.toughness,
                ToughnessFilter::AtMostX
                    | ToughnessFilter::EqualX
                    | ToughnessFilter::AtLeastX
                    | ToughnessFilter::CompareAnchor {
                        anchor: DynamicStatAnchor::ChosenX,
                        ..
                    }
            )
        }
        match self {
            Self::Clause(a) => has_x(a),
            Self::Either(a, b) => has_x(a) || has_x(b),
        }
    }

    /// Replace X-bound power/toughness predicates with a concrete bound.
    #[must_use]
    pub const fn bind_power_at_most_x(self, value: u16) -> Self {
        const fn bind(mut clause: ObjectClause, value: u16) -> ObjectClause {
            let signed = if value > i16::MAX as u16 {
                i16::MAX
            } else {
                value as i16
            };
            clause.power = match clause.power {
                PowerFilter::AtMostX => PowerFilter::AtMost(value),
                PowerFilter::EqualX => PowerFilter::Equal(signed),
                PowerFilter::AtLeastX => PowerFilter::AtLeast(signed),
                PowerFilter::CompareAnchor {
                    op,
                    anchor: DynamicStatAnchor::ChosenX,
                } => match op {
                    super::Comparison::LessOrEqual => PowerFilter::AtMost(value),
                    super::Comparison::Equal => PowerFilter::Equal(signed),
                    super::Comparison::GreaterOrEqual => PowerFilter::AtLeast(signed),
                    super::Comparison::Less => PowerFilter::AtMost(value.saturating_sub(1)),
                    super::Comparison::Greater => PowerFilter::AtLeast(signed.saturating_add(1)),
                    super::Comparison::NotEqual => clause.power,
                },
                other => other,
            };
            clause.toughness = match clause.toughness {
                ToughnessFilter::AtMostX => ToughnessFilter::AtMost(value),
                ToughnessFilter::EqualX => ToughnessFilter::Equal(signed),
                ToughnessFilter::AtLeastX => ToughnessFilter::AtLeast(signed),
                ToughnessFilter::CompareAnchor {
                    op,
                    anchor: DynamicStatAnchor::ChosenX,
                } => match op {
                    super::Comparison::LessOrEqual => ToughnessFilter::AtMost(value),
                    super::Comparison::Equal => ToughnessFilter::Equal(signed),
                    super::Comparison::GreaterOrEqual => ToughnessFilter::AtLeast(signed),
                    super::Comparison::Less => ToughnessFilter::AtMost(value.saturating_sub(1)),
                    super::Comparison::Greater => {
                        ToughnessFilter::AtLeast(signed.saturating_add(1))
                    }
                    super::Comparison::NotEqual => clause.toughness,
                },
                other => other,
            };
            clause
        }
        match self {
            Self::Clause(a) => Self::Clause(bind(a, value)),
            Self::Either(a, b) => Self::Either(bind(a, value), bind(b, value)),
        }
    }

    /// True if any clause binds mana value to the announced X.
    #[must_use]
    pub const fn has_mana_value_at_most_x(self) -> bool {
        const fn has_x(c: ObjectClause) -> bool {
            matches!(
                c.mana_value,
                ManaValueFilter::AtMostX
                    | ManaValueFilter::EqualX
                    | ManaValueFilter::AtLeastX
                    | ManaValueFilter::CompareAnchor {
                        anchor: DynamicStatAnchor::ChosenX,
                        ..
                    }
            )
        }
        match self {
            Self::Clause(a) => has_x(a),
            Self::Either(a, b) => has_x(a) || has_x(b),
        }
    }

    /// Replace X-bound mana value predicates with a concrete bound.
    #[must_use]
    pub const fn bind_mana_value_at_most_x(self, value: u32) -> Self {
        const fn bind(mut clause: ObjectClause, value: u32) -> ObjectClause {
            clause.mana_value = match clause.mana_value {
                ManaValueFilter::AtMostX => ManaValueFilter::AtMost(value),
                ManaValueFilter::EqualX => ManaValueFilter::Equal(value),
                ManaValueFilter::AtLeastX => ManaValueFilter::AtLeast(value),
                ManaValueFilter::CompareAnchor {
                    op,
                    anchor: DynamicStatAnchor::ChosenX,
                } => match op {
                    super::Comparison::LessOrEqual => ManaValueFilter::AtMost(value),
                    super::Comparison::Equal => ManaValueFilter::Equal(value),
                    super::Comparison::GreaterOrEqual => ManaValueFilter::AtLeast(value),
                    super::Comparison::Less => ManaValueFilter::AtMost(value.saturating_sub(1)),
                    super::Comparison::Greater => ManaValueFilter::AtLeast(value.saturating_add(1)),
                    super::Comparison::NotEqual => clause.mana_value,
                },
                other => other,
            };
            clause
        }
        match self {
            Self::Clause(a) => Self::Clause(bind(a, value)),
            Self::Either(a, b) => Self::Either(bind(a, value), bind(b, value)),
        }
    }

    /// True if any clause references an unbound related player.
    #[must_use]
    pub const fn has_related_player(self) -> bool {
        const fn has_rp(c: ObjectClause) -> bool {
            matches!(c.controller, ControllerFilter::RelatedPlayer)
                || matches!(c.owner, ControllerFilter::RelatedPlayer)
        }
        match self {
            Self::Clause(a) => has_rp(a),
            Self::Either(a, b) => has_rp(a) || has_rp(b),
        }
    }

    /// True if any clause filters by the resolving spell or ability's targeted player/opponent.
    #[must_use]
    pub const fn has_target_player(self) -> bool {
        const fn is_tp(c: ObjectClause) -> bool {
            matches!(
                c.controller,
                ControllerFilter::TargetPlayer | ControllerFilter::TargetOpponent
            ) || matches!(
                c.owner,
                ControllerFilter::TargetPlayer | ControllerFilter::TargetOpponent
            )
        }
        match self {
            Self::Clause(a) => is_tp(a),
            Self::Either(a, b) => is_tp(a) || is_tp(b),
        }
    }

    /// True if any clause references `ControllerFilter::FirstTargetController`.
    #[must_use]
    pub const fn uses_first_target_controller(self) -> bool {
        const fn uses_ftc(c: ObjectClause) -> bool {
            matches!(c.controller, ControllerFilter::FirstTargetController)
                || matches!(c.owner, ControllerFilter::FirstTargetController)
        }
        match self {
            Self::Clause(a) => uses_ftc(a),
            Self::Either(a, b) => uses_ftc(a) || uses_ftc(b),
        }
    }

    /// Bind `ControllerFilter::FirstTargetController` to a concrete player seat.
    #[must_use]
    pub const fn bind_first_target_controller(self, seat: SeatId) -> Self {
        const fn bind(mut c: ObjectClause, seat: SeatId) -> ObjectClause {
            if matches!(c.controller, ControllerFilter::FirstTargetController) {
                c.controller = ControllerFilter::Player(seat);
            }
            if matches!(c.owner, ControllerFilter::FirstTargetController) {
                c.owner = ControllerFilter::Player(seat);
            }
            c
        }
        match self {
            Self::Clause(a) => Self::Clause(bind(a, seat)),
            Self::Either(a, b) => Self::Either(bind(a, seat), bind(b, seat)),
        }
    }

    /// True if any clause dynamically compares power, toughness, or mana value against the resolving target permanent,
    /// or references the first target permanent's controller.
    #[must_use]
    pub const fn uses_target_permanent(self) -> bool {
        const fn clause_uses(c: ObjectClause) -> bool {
            matches!(c.controller, ControllerFilter::FirstTargetController)
                || matches!(c.owner, ControllerFilter::FirstTargetController)
                || matches!(
                    c.power,
                    PowerFilter::CompareAnchor {
                        anchor: DynamicStatAnchor::TargetPower,
                        ..
                    }
                )
                || matches!(
                    c.toughness,
                    ToughnessFilter::CompareAnchor {
                        anchor: DynamicStatAnchor::TargetPower,
                        ..
                    }
                )
                || matches!(
                    c.mana_value,
                    ManaValueFilter::CompareAnchor {
                        anchor: DynamicStatAnchor::TargetPower,
                        ..
                    }
                )
        }
        match self {
            Self::Clause(a) => clause_uses(a),
            Self::Either(a, b) => clause_uses(a) || clause_uses(b),
        }
    }

    /// Resolve `DynamicStatAnchor::TargetPower` clauses into concrete static bounds for group selection.
    #[must_use]
    pub fn bind_target_power(self, target_power: i64) -> Self {
        let bind_power = |pf: PowerFilter| -> PowerFilter {
            let PowerFilter::CompareAnchor {
                op,
                anchor: DynamicStatAnchor::TargetPower,
            } = pf
            else {
                return pf;
            };
            match op {
                super::Comparison::Equal => {
                    PowerFilter::Equal(i16::try_from(target_power).unwrap_or(i16::MAX))
                }
                super::Comparison::Less => {
                    let max = target_power.saturating_sub(1);
                    if max < 0 {
                        PowerFilter::Equal(i16::MIN)
                    } else {
                        PowerFilter::AtMost(u16::try_from(max).unwrap_or(u16::MAX))
                    }
                }
                super::Comparison::LessOrEqual => {
                    if target_power < 0 {
                        PowerFilter::Equal(i16::MIN)
                    } else {
                        PowerFilter::AtMost(u16::try_from(target_power).unwrap_or(u16::MAX))
                    }
                }
                super::Comparison::Greater => PowerFilter::AtLeast(
                    i16::try_from(target_power.saturating_add(1)).unwrap_or(i16::MAX),
                ),
                super::Comparison::GreaterOrEqual => {
                    PowerFilter::AtLeast(i16::try_from(target_power).unwrap_or(i16::MAX))
                }
                super::Comparison::NotEqual => pf,
            }
        };
        let bind_toughness = |tf: ToughnessFilter| -> ToughnessFilter {
            let ToughnessFilter::CompareAnchor {
                op,
                anchor: DynamicStatAnchor::TargetPower,
            } = tf
            else {
                return tf;
            };
            match op {
                super::Comparison::Equal => {
                    ToughnessFilter::Equal(i16::try_from(target_power).unwrap_or(i16::MAX))
                }
                super::Comparison::Less => {
                    let max = target_power.saturating_sub(1);
                    if max < 0 {
                        ToughnessFilter::Equal(i16::MIN)
                    } else {
                        ToughnessFilter::AtMost(u16::try_from(max).unwrap_or(u16::MAX))
                    }
                }
                super::Comparison::LessOrEqual => {
                    if target_power < 0 {
                        ToughnessFilter::Equal(i16::MIN)
                    } else {
                        ToughnessFilter::AtMost(u16::try_from(target_power).unwrap_or(u16::MAX))
                    }
                }
                super::Comparison::Greater => ToughnessFilter::AtLeast(
                    i16::try_from(target_power.saturating_add(1)).unwrap_or(i16::MAX),
                ),
                super::Comparison::GreaterOrEqual => {
                    ToughnessFilter::AtLeast(i16::try_from(target_power).unwrap_or(i16::MAX))
                }
                super::Comparison::NotEqual => tf,
            }
        };
        let bind_mv = |mf: ManaValueFilter| -> ManaValueFilter {
            let ManaValueFilter::CompareAnchor {
                op,
                anchor: DynamicStatAnchor::TargetPower,
            } = mf
            else {
                return mf;
            };
            match op {
                super::Comparison::Equal => {
                    ManaValueFilter::Equal(u32::try_from(target_power.max(0)).unwrap_or(u32::MAX))
                }
                super::Comparison::Less => {
                    let max = target_power.saturating_sub(1);
                    if max < 0 {
                        ManaValueFilter::AtLeast(u32::MAX)
                    } else {
                        ManaValueFilter::AtMost(u32::try_from(max).unwrap_or(u32::MAX))
                    }
                }
                super::Comparison::LessOrEqual => {
                    if target_power < 0 {
                        ManaValueFilter::AtLeast(u32::MAX)
                    } else {
                        ManaValueFilter::AtMost(u32::try_from(target_power).unwrap_or(u32::MAX))
                    }
                }
                super::Comparison::Greater => ManaValueFilter::AtLeast(
                    u32::try_from(target_power.saturating_add(1).max(0)).unwrap_or(u32::MAX),
                ),
                super::Comparison::GreaterOrEqual => {
                    ManaValueFilter::AtLeast(u32::try_from(target_power.max(0)).unwrap_or(u32::MAX))
                }
                super::Comparison::NotEqual => mf,
            }
        };
        let bind_clause = |mut c: ObjectClause| -> ObjectClause {
            c.power = bind_power(c.power);
            c.toughness = bind_toughness(c.toughness);
            c.mana_value = bind_mv(c.mana_value);
            c
        };
        match self {
            Self::Clause(a) => Self::Clause(bind_clause(a)),
            Self::Either(a, b) => Self::Either(bind_clause(a), bind_clause(b)),
        }
    }

    /// Bind `ControllerFilter::TargetPlayer` / `TargetOpponent` to a concrete player seat.
    #[must_use]
    pub const fn bind_target_player(self, seat: SeatId) -> Self {
        const fn bind(mut c: ObjectClause, seat: SeatId) -> ObjectClause {
            if matches!(
                c.controller,
                ControllerFilter::TargetPlayer | ControllerFilter::TargetOpponent
            ) {
                c.controller = ControllerFilter::Player(seat);
            }
            if matches!(
                c.owner,
                ControllerFilter::TargetPlayer | ControllerFilter::TargetOpponent
            ) {
                c.owner = ControllerFilter::Player(seat);
            }
            c
        }
        match self {
            Self::Clause(a) => Self::Clause(bind(a, seat)),
            Self::Either(a, b) => Self::Either(bind(a, seat), bind(b, seat)),
        }
    }

    /// True if any clause references a concrete player seat.
    #[must_use]
    pub const fn has_concrete_player(self) -> bool {
        const fn has_cp(c: ObjectClause) -> bool {
            matches!(c.controller, ControllerFilter::Player(_))
                || matches!(c.owner, ControllerFilter::Player(_))
        }
        match self {
            Self::Clause(a) => has_cp(a),
            Self::Either(a, b) => has_cp(a) || has_cp(b),
        }
    }

    /// Return the chosen creature-type slot referenced by this filter, if any.
    #[must_use]
    pub const fn chosen_type_slot(self) -> Option<CreatureTypeSlot> {
        match self {
            Self::Clause(a) => match a.subtypes {
                SubtypeFilter::ExceptChosenSlot(slot) | SubtypeFilter::MatchesChosenSlot(slot) => {
                    Some(slot)
                }
                _ => None,
            },
            Self::Either(a, b) => match (a.subtypes, b.subtypes) {
                (
                    SubtypeFilter::ExceptChosenSlot(slot) | SubtypeFilter::MatchesChosenSlot(slot),
                    _,
                )
                | (
                    _,
                    SubtypeFilter::ExceptChosenSlot(slot) | SubtypeFilter::MatchesChosenSlot(slot),
                ) => Some(slot),
                _ => None,
            },
        }
    }

    /// Replace `SubtypeFilter::MatchesChosenSlot` with a concrete creature subtype.
    #[must_use]
    pub fn bind_matching_chosen_slot(self, slot: CreatureTypeSlot, kind: CreatureTypeId) -> Self {
        fn bind(
            mut clause: ObjectClause,
            slot: CreatureTypeSlot,
            kind: CreatureTypeId,
        ) -> ObjectClause {
            if let SubtypeFilter::MatchesChosenSlot(current) = clause.subtypes
                && current == slot
            {
                clause.subtypes = SubtypeFilter::Has(kind);
            }
            clause
        }
        match self {
            Self::Clause(a) => Self::Clause(bind(a, slot, kind)),
            Self::Either(a, b) => Self::Either(bind(a, slot, kind), bind(b, slot, kind)),
        }
    }

    /// True if this filter excludes all creature types named during resolution.
    #[must_use]
    pub const fn uses_outside_named_slots(self) -> bool {
        match self {
            Self::Clause(a) => matches!(a.subtypes, SubtypeFilter::OutsideNamedSlots),
            Self::Either(a, b) => {
                matches!(a.subtypes, SubtypeFilter::OutsideNamedSlots)
                    || matches!(b.subtypes, SubtypeFilter::OutsideNamedSlots)
            }
        }
    }

    /// Bind unbound trigger/ability source exclusions to `source`.
    #[must_use]
    pub const fn with_ability_source(self, source: ObjectId) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.with_ability_source(source)),
            Self::Either(a, b) => {
                Self::Either(a.with_ability_source(source), b.with_ability_source(source))
            }
        }
    }

    /// Bind `ControllerFilter::RelatedPlayer` to a concrete player seat.
    #[must_use]
    pub const fn bind_related_player(self, seat: SeatId) -> Self {
        match self {
            Self::Clause(a) => Self::Clause(a.bind_related_player(seat)),
            Self::Either(a, b) => {
                Self::Either(a.bind_related_player(seat), b.bind_related_player(seat))
            }
        }
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type(&mut self, from: CreatureTypeId, to: CreatureTypeId) {
        fn rewrite_clause(c: &mut ObjectClause, from: CreatureTypeId, to: CreatureTypeId) {
            c.subtypes.rewrite(from, to);
            if let PowerFilter::CompareAnchor { anchor, .. } = &mut c.power {
                anchor.rewrite(from, to);
            }
            if let ToughnessFilter::CompareAnchor { anchor, .. } = &mut c.toughness {
                anchor.rewrite(from, to);
            }
            if let ManaValueFilter::CompareAnchor { anchor, .. } = &mut c.mana_value {
                anchor.rewrite(from, to);
            }
        }
        match self {
            Self::Clause(a) => rewrite_clause(a, from, to),
            Self::Either(a, b) => {
                rewrite_clause(a, from, to);
                rewrite_clause(b, from, to);
            }
        }
    }

    /// Validate that every referenced creature subtype belongs to `vocabulary`.
    pub(crate) fn validate_vocabulary(
        self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        fn check_clause(
            c: ObjectClause,
            vocabulary: &CreatureTypeVocabulary,
        ) -> Result<(), DefinitionError> {
            c.subtypes.validate_vocabulary(vocabulary)?;
            if let PowerFilter::CompareAnchor { anchor, .. } = c.power {
                anchor.validate_vocabulary(vocabulary)?;
            }
            if let ToughnessFilter::CompareAnchor { anchor, .. } = c.toughness {
                anchor.validate_vocabulary(vocabulary)?;
            }
            if let ManaValueFilter::CompareAnchor { anchor, .. } = c.mana_value {
                anchor.validate_vocabulary(vocabulary)?;
            }
            Ok(())
        }
        match self {
            Self::Clause(a) => check_clause(a, vocabulary),
            Self::Either(a, b) => {
                check_clause(a, vocabulary)?;
                check_clause(b, vocabulary)
            }
        }
    }

    /// Players whose permanents are scanned by a whole-group zone action.
    #[must_use]
    pub const fn affected_players(self) -> AffectedPlayers {
        let controller = match self {
            Self::Clause(a) => a.controller,
            Self::Either(a, _) => a.controller,
        };
        match controller {
            ControllerFilter::NotYou => AffectedPlayers::EachOtherPlayer,
            ControllerFilter::Opponent => AffectedPlayers::EachOpponent,
            ControllerFilter::You => AffectedPlayers::One(super::ResolvingPlayer::Controller),
            ControllerFilter::TargetPlayer | ControllerFilter::TargetOpponent => {
                AffectedPlayers::One(super::ResolvingPlayer::TargetPlayer)
            }
            _ => AffectedPlayers::EachPlayer,
        }
    }

    /// Evaluate the non-relational characteristics of `self` against `p`.
    #[must_use]
    pub fn matches_static_characteristics(
        self,
        p: &crate::characteristics::ObjectCharacteristics,
        excluded_subtypes: &BTreeSet<CreatureTypeId>,
        mana_value: impl FnOnce() -> Option<spellwake_contracts::RulesInteger>,
    ) -> bool {
        let mut provider = Some(mana_value);
        let mut cached: Option<Option<spellwake_contracts::RulesInteger>> = None;
        let mut get_mv = || {
            cached
                .get_or_insert_with(|| provider.take().and_then(|f| f()))
                .clone()
        };
        match self {
            Self::Clause(a) => a.matches_static_characteristics(p, excluded_subtypes, &mut get_mv),
            Self::Either(a, b) => {
                a.matches_static_characteristics(p, excluded_subtypes, &mut get_mv)
                    || b.matches_static_characteristics(p, excluded_subtypes, &mut get_mv)
            }
        }
    }

    /// Evaluate card-type predicates alone (used for spell copies with only copiable card types).
    #[must_use]
    pub fn matches_card_types_only(self, types: &BTreeSet<CardType>) -> bool {
        fn clause_ok(c: ObjectClause, types: &BTreeSet<CardType>) -> bool {
            if c.subtypes != SubtypeFilter::Any
                || c.color != ObjectColorFilter::Any
                || !c.required_supertypes.is_empty()
                || !c.forbidden_supertypes.is_empty()
                || c.flags.contains(ObjectFilterFlags::HISTORIC)
                || c.flags.contains(ObjectFilterFlags::NON_HISTORIC)
                || c.flags.contains(ObjectFilterFlags::HAS_ADVENTURE)
                || c.required_counter_kind.is_some()
                || c.forbidden_counter_kind.is_some()
                || c.exact_name.is_some()
            {
                return false;
            }
            c.any_types.matches_any(types)
                && c.required_types.matches_all(types)
                && c.forbidden_types.matches_none(types)
        }
        match self {
            Self::Clause(a) => clause_ok(a, types),
            Self::Either(a, b) => clause_ok(a, types) || clause_ok(b, types),
        }
    }

    /// Evaluate static spell predicates (types, supertypes, subtypes, color, historic) against a cast appearance.
    #[must_use]
    pub fn matches_spell_appearance(self, spell: &crate::occurrence::SupportedAppearance) -> bool {
        fn clause_ok(c: ObjectClause, spell: &crate::occurrence::SupportedAppearance) -> bool {
            if !c.any_types.matches_any(&spell.types)
                || !c.required_types.matches_all(&spell.types)
                || !c.forbidden_types.matches_none(&spell.types)
            {
                return false;
            }
            if !c.required_supertypes.matches_all(&spell.current_supertypes)
                || !c
                    .forbidden_supertypes
                    .matches_none(&spell.current_supertypes)
            {
                return false;
            }
            let is_historic = spell.types.contains(&CardType::Artifact)
                || spell.current_supertypes.contains(&Supertype::Legendary)
                || spell
                    .enchantment_subtypes
                    .contains(&EnchantmentSubtype::Saga);
            if c.flags.contains(ObjectFilterFlags::HISTORIC) && !is_historic {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::NON_HISTORIC) && is_historic {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
                && !spell
                    .battlefield_counters
                    .as_ref()
                    .is_some_and(|counters| counters.minus_one.is_positive())
            {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::KICKED) && spell.kicker_choices.is_empty() {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::RENOWNED) && !spell.renowned {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::SADDLED) && !spell.saddled {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::MONSTROUS) && !spell.monstrous {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::SUSPECTED) && !spell.suspected {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::GOADED) && !spell.goaded {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::SOLVED) && !spell.solved {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::PREPARED) && !spell.prepared {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::EXERTED) && !spell.exerted {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::HAS_ADVENTURE) && !spell.has_adventure {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::COMMANDER_ONLY)
                && spell.commander_owner.is_none()
            {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::NOT_COMMANDER) && spell.commander_owner.is_some()
            {
                return false;
            }
            if let Some(exact) = c.exact_name
                && !exact.matches_exact_names(&spell.current_names)
            {
                return false;
            }
            if c.required_counter_kind.is_some() || c.forbidden_counter_kind.is_some() {
                return false;
            }
            if !c.color.matches(&spell.colors) {
                return false;
            }
            let is_aura = spell.types.contains(&CardType::Enchantment)
                && spell.enchantment_subtypes.is_empty();
            c.subtypes
                .matches_sets(
                    &spell.creature_subtypes,
                    &spell.land_subtypes,
                    &spell.artifact_subtypes,
                    &spell.enchantment_subtypes,
                    &spell.planeswalker_subtypes,
                    &spell.spell_subtypes,
                    is_aura,
                )
                .unwrap_or(false)
        }
        match self {
            Self::Clause(a) => clause_ok(a, spell),
            Self::Either(a, b) => clause_ok(a, spell) || clause_ok(b, spell),
        }
    }

    /// Evaluate `self` against a trigger-time [`crate::occurrence::SupportedAppearance`].
    #[must_use]
    pub fn matches_appearance(
        self,
        source: &crate::occurrence::SupportedAppearance,
        controller: SeatId,
        object: &crate::occurrence::SupportedAppearance,
    ) -> bool {
        fn clause_ok(
            c: ObjectClause,
            source: &crate::occurrence::SupportedAppearance,
            controller: SeatId,
            object: &crate::occurrence::SupportedAppearance,
        ) -> bool {
            let same = source.object.id() == object.object.id();
            let source_ok = match c.source_relation {
                SourceRelation::Any => true,
                SourceRelation::SelfObject => same,
                SourceRelation::AttachedToSelf => {
                    source.attached_to
                        == Some(crate::targets::TargetRef::Object(object.object.id()))
                }
                SourceRelation::OtherThanAbilitySource | SourceRelation::OtherThanTriggerSource => {
                    !same
                }
                SourceRelation::OtherThanBound(id) => object.object.id() != id,
                SourceRelation::SharingNameWithAbilitySource => {
                    !source.current_names.is_empty()
                        && source
                            .current_names
                            .iter()
                            .any(|name| object.current_names.iter().any(|other| other == name))
                }
                SourceRelation::OtherThanAbilitySourceSharingName => {
                    !same
                        && !source.current_names.is_empty()
                        && source
                            .current_names
                            .iter()
                            .any(|name| object.current_names.iter().any(|other| other == name))
                }
                SourceRelation::MatchesNamedAgendaSlot => false,
            };
            if !source_ok {
                return false;
            }
            let controller_ok = match c.controller {
                ControllerFilter::Any => true,
                ControllerFilter::You => object.object.controller() == Some(controller),
                ControllerFilter::Opponent | ControllerFilter::NotYou => object
                    .object
                    .controller()
                    .is_some_and(|seat| seat != controller),
                ControllerFilter::OwnerYou => object.object.owner() == controller,
                ControllerFilter::OwnerOpponent => object.object.owner() != controller,
                ControllerFilter::RelatedPlayer
                | ControllerFilter::TargetPlayer
                | ControllerFilter::TargetOpponent
                | ControllerFilter::ActivePlayer
                | ControllerFilter::DefendingPlayer
                | ControllerFilter::FirstTargetController => false,
                ControllerFilter::Player(seat) => object.object.controller() == Some(seat),
            };
            if !controller_ok {
                return false;
            }
            let owner_ok = match c.owner {
                ControllerFilter::Any => true,
                ControllerFilter::You | ControllerFilter::OwnerYou => {
                    object.object.owner() == controller
                }
                ControllerFilter::Opponent
                | ControllerFilter::NotYou
                | ControllerFilter::OwnerOpponent => object.object.owner() != controller,
                ControllerFilter::RelatedPlayer
                | ControllerFilter::TargetPlayer
                | ControllerFilter::TargetOpponent
                | ControllerFilter::ActivePlayer
                | ControllerFilter::DefendingPlayer
                | ControllerFilter::FirstTargetController => false,
                ControllerFilter::Player(seat) => object.object.owner() == seat,
            };
            if !owner_ok {
                return false;
            }
            if let Some(expected_token) = c.token
                && object.object.is_token() != expected_token
            {
                return false;
            }
            if let Some(expected_tapped) = c.tapped
                && object.object.tapped() != Some(expected_tapped)
            {
                return false;
            }
            if !c.any_types.matches_any(&object.types)
                || !c.required_types.matches_all(&object.types)
                || !c.forbidden_types.matches_none(&object.types)
            {
                return false;
            }
            if !c
                .required_supertypes
                .matches_all(&object.current_supertypes)
                || !c
                    .forbidden_supertypes
                    .matches_none(&object.current_supertypes)
            {
                return false;
            }
            let is_historic = object.types.contains(&CardType::Artifact)
                || object.current_supertypes.contains(&Supertype::Legendary)
                || object
                    .enchantment_subtypes
                    .contains(&EnchantmentSubtype::Saga);
            if c.flags.contains(ObjectFilterFlags::HISTORIC) && !is_historic {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::NON_HISTORIC) && is_historic {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::COMMANDER_ONLY)
                && object.commander_owner.is_none()
            {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::NOT_COMMANDER)
                && object.commander_owner.is_some()
            {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
                && !object
                    .battlefield_counters
                    .as_ref()
                    .is_some_and(|counters| counters.plus_one.is_positive())
            {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
                && !object
                    .battlefield_counters
                    .as_ref()
                    .is_some_and(|counters| counters.minus_one.is_positive())
            {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::WITH_ANY_COUNTER)
                && !object
                    .battlefield_total_counters
                    .as_ref()
                    .is_some_and(spellwake_contracts::RulesInteger::is_positive)
            {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS)
                && object
                    .battlefield_total_counters
                    .as_ref()
                    .is_some_and(spellwake_contracts::RulesInteger::is_positive)
            {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::KICKED) && object.kicker_choices.is_empty() {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::RENOWNED) && !object.renowned {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::SADDLED) && !object.saddled {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::MONSTROUS) && !object.monstrous {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::SUSPECTED) && !object.suspected {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::GOADED) && !object.goaded {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::SOLVED) && !object.solved {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::PREPARED) && !object.prepared {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::EXERTED) && !object.exerted {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::HAS_ADVENTURE) && !object.has_adventure {
                return false;
            }
            if let Some(exact) = c.exact_name
                && !exact.matches_exact_names(&object.current_names)
            {
                return false;
            }
            if let Some(kind) = c.required_counter_kind {
                let present = match kind {
                    crate::effects::CounterKind::PlusOne => object
                        .battlefield_counters
                        .as_ref()
                        .is_some_and(|counters| counters.plus_one.is_positive()),
                    crate::effects::CounterKind::MinusOne => object
                        .battlefield_counters
                        .as_ref()
                        .is_some_and(|counters| counters.minus_one.is_positive()),
                    _ => false,
                };
                if !present {
                    return false;
                }
            }
            if let Some(kind) = c.forbidden_counter_kind {
                let absent = match kind {
                    crate::effects::CounterKind::PlusOne => !object
                        .battlefield_counters
                        .as_ref()
                        .is_some_and(|counters| counters.plus_one.is_positive()),
                    crate::effects::CounterKind::MinusOne => !object
                        .battlefield_counters
                        .as_ref()
                        .is_some_and(|counters| counters.minus_one.is_positive()),
                    _ => false,
                };
                if !absent {
                    return false;
                }
            }
            if !c.color.matches(&object.colors) {
                return false;
            }
            let subtype_ok = c
                .subtypes
                .matches_sets(
                    &object.creature_subtypes,
                    &object.land_subtypes,
                    &object.artifact_subtypes,
                    &object.enchantment_subtypes,
                    &object.planeswalker_subtypes,
                    &object.spell_subtypes,
                    false,
                )
                .unwrap_or(false);
            if !subtype_ok {
                return false;
            }
            if let Some(keyword) = c.keyword
                && !object.keywords.contains(&keyword)
            {
                return false;
            }
            if let Some(keyword) = c.forbidden_keyword
                && object.keywords.contains(&keyword)
            {
                return false;
            }
            let power_ok = match c.power {
                PowerFilter::Any => true,
                PowerFilter::AtLeast(min) => object
                    .current_power_toughness
                    .as_ref()
                    .is_some_and(|pt| pt.power >= i64::from(min).into()),
                PowerFilter::AtMost(max) => object
                    .current_power_toughness
                    .as_ref()
                    .is_some_and(|pt| pt.power <= i64::from(max).into()),
                PowerFilter::Equal(exact) => object
                    .current_power_toughness
                    .as_ref()
                    .is_some_and(|pt| pt.power == i64::from(exact).into()),
                PowerFilter::PowerOrToughnessAtMost(max) => {
                    object.current_power_toughness.as_ref().is_some_and(|pt| {
                        pt.power <= i64::from(max).into() || pt.toughness <= i64::from(max).into()
                    })
                }
                PowerFilter::AtMostX
                | PowerFilter::EqualX
                | PowerFilter::AtLeastX
                | PowerFilter::CompareAnchor { .. }
                | PowerFilter::AtMostControlledCreatureCount => false,
            };
            if !power_ok {
                return false;
            }
            match c.toughness {
                ToughnessFilter::Any => true,
                ToughnessFilter::AtLeast(min) => object
                    .current_power_toughness
                    .as_ref()
                    .is_some_and(|pt| pt.toughness >= i64::from(min).into()),
                ToughnessFilter::AtMost(max) => object
                    .current_power_toughness
                    .as_ref()
                    .is_some_and(|pt| pt.toughness <= i64::from(max).into()),
                ToughnessFilter::Equal(exact) => object
                    .current_power_toughness
                    .as_ref()
                    .is_some_and(|pt| pt.toughness == i64::from(exact).into()),
                ToughnessFilter::AtMostX
                | ToughnessFilter::EqualX
                | ToughnessFilter::AtLeastX
                | ToughnessFilter::CompareAnchor { .. } => false,
            }
        }
        match self {
            Self::Clause(a) => clause_ok(a, source, controller, object),
            Self::Either(a, b) => {
                clause_ok(a, source, controller, object) || clause_ok(b, source, controller, object)
            }
        }
    }
}

/// Which permanents a resolution-time modification or suppression instruction affects.
// EffectSubject is intentionally Copy and zero-allocation; Matching holds ObjectFilter inline.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectSubject {
    /// The exact creature whose attack triggered this ability.
    TriggeringAttacker,
    /// All creatures declared together in the triggering attack event.
    TriggeringAttackGroup,
    /// The exact permanent targeted in the triggering occurrence.
    TriggeringTarget,
    /// Any object subject captured by the triggering occurrence (enters/dies/attacks/blocks/etc.).
    TriggerSubject,
    /// The permanent that generated the resolving activated or triggered ability.
    AbilitySource,
    /// The currently resolving permanent target(s) of this spell or ability.
    TargetObject,
    /// Tokens created earlier during this same resolution.
    CreatedTokens,
    /// Objects moved earlier during this same resolution.
    LastMoved,
    /// Every battlefield permanent matching this composable filter at resolution.
    Matching(ObjectFilter),
    /// The targeted permanent plus every other battlefield permanent matching this filter with the same name as the target.
    TargetAndSameNameMatching(ObjectFilter),
    /// Every battlefield permanent matching this filter other than the targeted permanent.
    MatchingOtherThanTarget(ObjectFilter),
    /// The targeted permanent plus every other battlefield permanent matching this filter sharing a color with the target.
    TargetAndSharingColorMatching(ObjectFilter),
}

impl EffectSubject {
    /// Return the chosen creature-type slot referenced by this subject, if any.
    #[must_use]
    pub const fn chosen_type_slot(self) -> Option<CreatureTypeSlot> {
        match self {
            Self::Matching(filter)
            | Self::TargetAndSameNameMatching(filter)
            | Self::MatchingOtherThanTarget(filter)
            | Self::TargetAndSharingColorMatching(filter) => filter.chosen_type_slot(),
            _ => None,
        }
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type(&mut self, from: CreatureTypeId, to: CreatureTypeId) {
        if let Self::Matching(filter)
        | Self::TargetAndSameNameMatching(filter)
        | Self::MatchingOtherThanTarget(filter)
        | Self::TargetAndSharingColorMatching(filter) = self
        {
            filter.rewrite_creature_type(from, to);
        }
    }

    /// Validate that every referenced creature subtype belongs to `vocabulary`.
    pub(crate) fn validate_vocabulary(
        self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Matching(filter)
            | Self::TargetAndSameNameMatching(filter)
            | Self::MatchingOtherThanTarget(filter)
            | Self::TargetAndSharingColorMatching(filter) => filter.validate_vocabulary(vocabulary),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod wave14_object_filter_tests {
    use super::*;
    use spellwake_contracts::{
        creature_types::CreatureTypeVocabulary,
        names::{EnglishCardName, NameRules, ObjectNames},
    };
    use std::collections::BTreeSet;

    fn sample_vocab() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "wave14-test".into(),
            [0u8; 32],
            vec![
                "Eldrazi".into(),
                "Elf".into(),
                "Frog".into(),
                "Goblin".into(),
                "Human".into(),
                "Mount".into(),
                "Rabbit".into(),
                "Raccoon".into(),
                "Scion".into(),
                "Squirrel".into(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn subtype_list_all_any_none_and_capacity_bounds() {
        let vocab = sample_vocab();
        let eldrazi = vocab.resolve_label("Eldrazi").unwrap();
        let scion = vocab.resolve_label("Scion").unwrap();
        let frog = vocab.resolve_label("Frog").unwrap();
        let rabbit = vocab.resolve_label("Rabbit").unwrap();
        let raccoon = vocab.resolve_label("Raccoon").unwrap();
        let squirrel = vocab.resolve_label("Squirrel").unwrap();
        let mount = vocab.resolve_label("Mount").unwrap();
        let elf = vocab.resolve_label("Elf").unwrap();

        assert!(SubtypeList::from_slice(&[]).is_none());
        let too_many = [SingleSubtype::Creature(elf); MAX_INLINE_SUBTYPES + 1];
        assert!(SubtypeList::from_slice(&too_many).is_none());

        let empty_l = BTreeSet::new();
        let empty_a = BTreeSet::new();
        let empty_e = BTreeSet::new();
        let empty_p = BTreeSet::new();
        let empty_s = BTreeSet::new();

        // Conjunctive Eldrazi Scion
        let all_filter = SubtypeFilter::AllOfList(
            SubtypeList::from_slice(&[
                SingleSubtype::Creature(eldrazi),
                SingleSubtype::Creature(scion),
            ])
            .unwrap(),
        );
        assert_eq!(
            all_filter.matches_sets(
                &BTreeSet::from([eldrazi, scion]),
                &empty_l,
                &empty_a,
                &empty_e,
                &empty_p,
                &empty_s,
                false
            ),
            Some(true)
        );
        assert_eq!(
            all_filter.matches_sets(
                &BTreeSet::from([eldrazi]),
                &empty_l,
                &empty_a,
                &empty_e,
                &empty_p,
                &empty_s,
                false
            ),
            Some(false)
        );

        // 4-subtype Bloomburrow disjunction + cross-category Mount/Vehicle
        let any_four = SubtypeFilter::AnyOfList(
            SubtypeList::from_slice(&[
                SingleSubtype::Creature(frog),
                SingleSubtype::Creature(rabbit),
                SingleSubtype::Creature(raccoon),
                SingleSubtype::Creature(squirrel),
            ])
            .unwrap(),
        );
        assert_eq!(
            any_four.matches_sets(
                &BTreeSet::from([squirrel]),
                &empty_l,
                &empty_a,
                &empty_e,
                &empty_p,
                &empty_s,
                false
            ),
            Some(true)
        );
        assert_eq!(
            any_four.matches_sets(
                &BTreeSet::from([elf]),
                &empty_l,
                &empty_a,
                &empty_e,
                &empty_p,
                &empty_s,
                false
            ),
            Some(false)
        );

        let mixed = SubtypeFilter::AnyOfList(
            SubtypeList::from_slice(&[
                SingleSubtype::Creature(mount),
                SingleSubtype::Artifact(ArtifactSubtype::Vehicle),
            ])
            .unwrap(),
        );
        assert_eq!(
            mixed.matches_sets(
                &BTreeSet::new(),
                &empty_l,
                &BTreeSet::from([ArtifactSubtype::Vehicle]),
                &empty_e,
                &empty_p,
                &empty_s,
                false
            ),
            Some(true)
        );

        // Negated list
        let none_filter = SubtypeFilter::NoneOfList(
            SubtypeList::from_slice(&[
                SingleSubtype::Creature(elf),
                SingleSubtype::Artifact(ArtifactSubtype::Equipment),
            ])
            .unwrap(),
        );
        assert_eq!(
            none_filter.matches_sets(
                &BTreeSet::from([elf]),
                &empty_l,
                &empty_a,
                &empty_e,
                &empty_p,
                &empty_s,
                false
            ),
            Some(false)
        );
        assert_eq!(
            none_filter.matches_sets(
                &BTreeSet::new(),
                &empty_l,
                &BTreeSet::from([ArtifactSubtype::Equipment]),
                &empty_e,
                &empty_p,
                &empty_s,
                false
            ),
            Some(false)
        );
        assert_eq!(
            none_filter.matches_sets(
                &BTreeSet::from([frog]),
                &empty_l,
                &empty_a,
                &empty_e,
                &empty_p,
                &empty_s,
                false
            ),
            Some(true)
        );

        // Rewrite creature subtype inside list
        let mut rewritten = any_four;
        rewritten.rewrite(squirrel, elf);
        assert_eq!(
            rewritten.matches_sets(
                &BTreeSet::from([elf]),
                &empty_l,
                &empty_a,
                &empty_e,
                &empty_p,
                &empty_s,
                false
            ),
            Some(true)
        );
        assert!(rewritten.validate_vocabulary(&vocab).is_ok());
    }

    #[test]
    fn exact_name_key_validates_bounds_and_matches_names() {
        assert!(ExactNameKey::from_name("").is_none());
        assert!(ExactNameKey::from_name(" leading").is_none());
        let too_long = "A".repeat(MAX_EXACT_NAME_BYTES + 1);
        assert!(ExactNameKey::from_name(&too_long).is_none());

        let key = ExactNameKey::from_name("Hare Apparent").unwrap();
        assert_eq!(key.as_str(), "Hare Apparent");
        let matching = ObjectNames::single(EnglishCardName::new("Hare Apparent".into()).unwrap());
        let other = ObjectNames::single(EnglishCardName::new("Grizzly Bears".into()).unwrap());
        let rules = NameRules::literal_names();
        assert!(key.matches_with_rules(&matching, &rules));
        assert!(!key.matches_with_rules(&other, &rules));
        assert!(key.matches_exact_names(&matching));
        assert!(!key.matches_exact_names(&other));
        let lower_key = ExactNameKey::from_name("hare apparent").unwrap();
        assert!(lower_key.matches_with_rules(&matching, &rules));
        assert!(lower_key.matches_exact_names(&matching));
        assert!(!lower_key.matches_with_rules(&other, &rules));
    }
}
