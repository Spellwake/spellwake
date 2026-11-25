//! Composable spell filter unifying spell-cast trigger filters and spell-target filters.
use super::{
    CardType, CardTypeMask, DefinitionError, ObjectClause, ObjectColorFilter, ObjectFilter,
    ObjectFilterFlags, PowerFilter, SubtypeFilter, Supertype,
};
use crate::{
    characteristics::{Color, ObjectCharacteristics},
    occurrence::SupportedAppearance,
};
use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};
use std::{collections::BTreeSet, num::NonZeroU32};

/// Origin-zone constraint on how a spell was cast.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SpellCastZoneFilter {
    #[default]
    Any,
    /// Cast from the ability controller's library.
    OwnLibrary,
    /// Cast directly from a graveyard.
    Graveyard,
    /// Cast directly from exile.
    Exile,
    /// Cast directly from a player's hand.
    Hand,
    /// Not cast from its owner's hand (including copies never cast).
    NotOwnersHand,
}

/// Target-shape constraint on a spell on the stack.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SpellTargetShape {
    #[default]
    Any,
    /// Has exactly one chosen target.
    SingleTarget,
    /// Targets at least one player.
    TargetingPlayer,
    /// Targets only the ability source permanent (and at least one target).
    TargetingOnlySource,
}

/// Specialized stack or cast predicate not expressed by static object characteristics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SpellSpecialFilter {
    #[default]
    None,
    /// Mana value strictly exceeds the controller's experience counters.
    AboveExperience,
    /// Matches the creature subtype chosen as the ability source entered.
    ChosenEntryCreatureType,
    /// Current copy-interpreter capacity (copyable instruction spell payload).
    CopyableInstruction,
    /// Aura or Equipment spell.
    AuraOrEquipment,
}

/// Composable predicate over spells on the stack and completed casts,
/// replacing both `SpellFilter` and `SpellFilter`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpellFilter {
    pub object: ObjectFilter,
    pub cast_zone: SpellCastZoneFilter,
    pub target_shape: SpellTargetShape,
    pub ordinal_this_turn: Option<NonZeroU32>,
    pub special: SpellSpecialFilter,
}

impl SpellFilter {
    /// Any spell.
    #[must_use]
    pub const fn any() -> Self {
        Self {
            object: ObjectFilter::any(),
            cast_zone: SpellCastZoneFilter::Any,
            target_shape: SpellTargetShape::Any,
            ordinal_this_turn: None,
            special: SpellSpecialFilter::None,
        }
    }

    /// Wrap an arbitrary [`ObjectFilter`] as a spell filter.
    #[must_use]
    pub const fn from_object(object: ObjectFilter) -> Self {
        Self {
            object,
            ..Self::any()
        }
    }

    /// Any creature spell.
    #[must_use]
    pub const fn creature() -> Self {
        Self::from_object(ObjectFilter::creature())
    }

    /// Any noncreature spell.
    #[must_use]
    pub const fn noncreature() -> Self {
        Self::from_object(ObjectFilter::noncreature())
    }

    /// Any colorless spell.
    #[must_use]
    pub const fn colorless() -> Self {
        Self::from_object(ObjectFilter::any().with_color(ObjectColorFilter::Colorless))
    }

    /// Any instant or sorcery spell (including kindred instants and sorceries).
    #[must_use]
    pub const fn instant_or_sorcery() -> Self {
        Self::from_object(ObjectFilter::instant_or_sorcery())
    }

    /// Any instant, sorcery, or spell with creature subtype `kind`.
    #[must_use]
    pub const fn instant_sorcery_or_subtype(kind: CreatureTypeId) -> Self {
        Self::from_object(ObjectFilter::either(
            ObjectClause::any().any_types(CardTypeMask::INSTANT.union(CardTypeMask::SORCERY)),
            ObjectClause::any().with_subtype(kind),
        ))
    }

    /// Any spell with `kind` among its card types.
    #[must_use]
    pub const fn of_type(kind: CardType) -> Self {
        Self::from_object(ObjectFilter::any_types(CardTypeMask::from_card_type(kind)))
    }

    /// Any historic spell (artifact, legendary, or Saga).
    #[must_use]
    pub const fn historic() -> Self {
        Self::from_object(ObjectFilter::any().with_flags(ObjectFilterFlags::HISTORIC))
    }

    /// Any spell of a specific color.
    #[must_use]
    pub const fn of_color(color: Color) -> Self {
        Self::from_object(ObjectFilter::any().with_color(ObjectColorFilter::has(color)))
    }

    /// Any spell with a specific creature subtype.
    #[must_use]
    pub const fn of_subtype(kind: CreatureTypeId) -> Self {
        Self::from_object(ObjectFilter::any().with_subtype(kind))
    }

    /// Any spell with either of two creature subtypes.
    #[must_use]
    pub const fn either_subtype(first: CreatureTypeId, second: CreatureTypeId) -> Self {
        Self::from_object(ObjectFilter::either(
            ObjectClause::any().with_subtype(first),
            ObjectClause::any().with_subtype(second),
        ))
    }

    /// Any Aura or Equipment spell.
    #[must_use]
    pub const fn aura_or_equipment() -> Self {
        Self {
            special: SpellSpecialFilter::AuraOrEquipment,
            ..Self::any()
        }
    }

    /// Restrict to spells with a power predicate (for creature spells).
    #[must_use]
    pub const fn with_power(mut self, power: PowerFilter) -> Self {
        self.object = self.object.with_power(power);
        self
    }

    /// Current copy-interpreter capacity: copyable instruction spell.
    #[must_use]
    pub const fn copyable_instruction() -> Self {
        Self {
            special: SpellSpecialFilter::CopyableInstruction,
            ..Self::any()
        }
    }

    /// Restrict to colorless spells.
    #[must_use]
    pub const fn with_colorless(mut self) -> Self {
        self.object = self.object.with_color(ObjectColorFilter::Colorless);
        self
    }

    /// Restrict to spells controlled by the acting/resolving player.
    #[must_use]
    pub const fn controlled(mut self) -> Self {
        self.object = self.object.controlled();
        self
    }

    /// Restrict to spells cast from the ability controller's library.
    #[must_use]
    pub const fn from_own_library(mut self) -> Self {
        self.cast_zone = SpellCastZoneFilter::OwnLibrary;
        self
    }

    /// Restrict to spells cast directly from a graveyard.
    #[must_use]
    pub const fn from_graveyard(mut self) -> Self {
        self.cast_zone = SpellCastZoneFilter::Graveyard;
        self
    }

    /// Restrict to spells cast directly from exile.
    #[must_use]
    pub const fn from_exile(mut self) -> Self {
        self.cast_zone = SpellCastZoneFilter::Exile;
        self
    }

    /// Restrict to spells cast directly from a player's hand.
    #[must_use]
    pub const fn from_hand(mut self) -> Self {
        self.cast_zone = SpellCastZoneFilter::Hand;
        self
    }

    /// Restrict to spells not cast from their owner's hand (including copies).
    #[must_use]
    pub const fn not_from_owners_hand(mut self) -> Self {
        self.cast_zone = SpellCastZoneFilter::NotOwnersHand;
        self
    }

    /// Restrict to spells with exactly one target.
    #[must_use]
    pub const fn single_target(mut self) -> Self {
        self.target_shape = SpellTargetShape::SingleTarget;
        self
    }

    /// Restrict to spells targeting at least one player.
    #[must_use]
    pub const fn targeting_player(mut self) -> Self {
        self.target_shape = SpellTargetShape::TargetingPlayer;
        self
    }

    /// Restrict to spells targeting only the ability source permanent.
    #[must_use]
    pub const fn targeting_only_source(mut self) -> Self {
        self.target_shape = SpellTargetShape::TargetingOnlySource;
        self
    }

    /// Restrict to the Nth spell cast this turn.
    #[must_use]
    pub const fn ordinal_this_turn(mut self, ordinal: NonZeroU32) -> Self {
        self.ordinal_this_turn = Some(ordinal);
        self
    }

    /// Restrict to spells with mana value strictly above controller's experience counters.
    #[must_use]
    pub const fn above_experience(mut self) -> Self {
        self.special = SpellSpecialFilter::AboveExperience;
        self
    }

    /// Restrict to creature spells sharing the source's entry-chosen creature subtype.
    #[must_use]
    pub const fn of_chosen_entry_type(mut self) -> Self {
        self.special = SpellSpecialFilter::ChosenEntryCreatureType;
        self
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type(&mut self, from: CreatureTypeId, to: CreatureTypeId) {
        self.object.rewrite_creature_type(from, to);
    }

    /// Validate that every referenced creature subtype exists in `vocabulary`.
    pub(crate) fn validate_vocabulary(
        self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.object.validate_vocabulary(vocabulary)
    }

    /// Match card types alone (used when only copiable card types are available, e.g. spell copies).
    #[must_use]
    pub(crate) fn matches_card_types(self, types: &BTreeSet<CardType>) -> bool {
        self.object.matches_card_types_only(types)
    }

    /// Match the static appearance of a cast spell (types, supertypes, subtypes, colors, historic).
    #[must_use]
    pub(crate) fn matches_spell_appearance(self, spell: &SupportedAppearance) -> bool {
        self.object.matches_spell_appearance(spell)
    }

    /// Match a spell's evaluated characteristics for static cost discounts and flash permissions.
    #[must_use]
    pub(crate) fn matches_characteristics(
        self,
        spell: &ObjectCharacteristics,
        aura_or_equipment: bool,
        source_entry_type: Option<CreatureTypeId>,
    ) -> bool {
        if self.special == SpellSpecialFilter::AuraOrEquipment {
            return aura_or_equipment;
        }
        if self.special == SpellSpecialFilter::ChosenEntryCreatureType {
            return spell.is_creature()
                && source_entry_type
                    .is_some_and(|kind| spell.current_creature_subtypes.contains(&kind));
        }
        let match_clause = |c: ObjectClause| {
            if !c.any_types.matches_any(&spell.current_types)
                || !c.required_types.matches_all(&spell.current_types)
                || !c.forbidden_types.matches_none(&spell.current_types)
            {
                return false;
            }
            match c.subtypes {
                SubtypeFilter::Any => {}
                SubtypeFilter::Has(kind) => {
                    if !spell.current_creature_subtypes.contains(&kind) {
                        return false;
                    }
                }
                SubtypeFilter::Lacks(kind) => {
                    if spell.current_creature_subtypes.contains(&kind) {
                        return false;
                    }
                }
                SubtypeFilter::Enchantment(kind) => {
                    if !spell.current_enchantment_subtypes.contains(&kind) {
                        return false;
                    }
                }
                SubtypeFilter::Spell(kind) => {
                    if !spell.current_spell_subtypes.contains(&kind) {
                        return false;
                    }
                }
                SubtypeFilter::Artifact(kind) => {
                    if !spell.current_artifact_subtypes.contains(&kind) {
                        return false;
                    }
                }
                SubtypeFilter::Planeswalker(kind) => {
                    if !spell.current_planeswalker_subtypes.contains(&kind) {
                        return false;
                    }
                }
                SubtypeFilter::AnyLandMask(mask) => {
                    if !spell
                        .current_land_subtypes
                        .iter()
                        .any(|lt| (mask & lt.bit()) != 0)
                    {
                        return false;
                    }
                }
                SubtypeFilter::NotLandMask(mask) => {
                    if spell
                        .current_land_subtypes
                        .iter()
                        .any(|lt| (mask & lt.bit()) != 0)
                    {
                        return false;
                    }
                }
                _ => return false,
            }
            if !c.color.matches(&spell.current_colors) {
                return false;
            }
            if let PowerFilter::AtLeast(min) = c.power
                && !spell
                    .power_toughness
                    .as_ref()
                    .is_some_and(|pt| pt.derived.power >= i64::from(min).into())
            {
                return false;
            }
            if c.flags.contains(ObjectFilterFlags::HISTORIC)
                && !(spell.has_type(CardType::Artifact)
                    || spell.current_supertypes.contains(&Supertype::Legendary)
                    || spell
                        .current_enchantment_subtypes
                        .contains(&super::EnchantmentSubtype::Saga))
            {
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
            if c.flags
                .contains(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_TYPE)
                && !source_entry_type
                    .is_some_and(|kind| spell.current_creature_subtypes.contains(&kind))
            {
                return false;
            }
            true
        };
        match self.object {
            ObjectFilter::Clause(c) => match_clause(c),
            ObjectFilter::Either(a, b) => match_clause(a) || match_clause(b),
        }
    }
}
