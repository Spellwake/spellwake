//! Orthogonal static ability rules unifying cost modifiers, casting permissions,
//! damage/draw/untap rules, restrictions, battlefield modifiers, and Conspiracy command-zone agendas.
use super::{
    BlockingRestriction, CharacteristicDefinitionError, CreatureTypeTextChange,
    DefiningCreatureTypes, DefiningPart, DefiningParts, PermanentSelector, StaticAbilityId,
    StaticPart, StaticParts, StaticSubtypeOperand, StaticTypeChange, SubtypeOperation,
    TypeOperation, validate_battlefield_types, validate_buckets, validate_subtypes,
};
use crate::{
    land::NonlandCardNames,
    mana::ManaType,
    program::{CardType, SpellFilter},
};
use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};
use std::{collections::BTreeSet, num::NonZeroU8, sync::Arc};

/// Magnitude of a generic spell cost reduction granted by a permanent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellDiscount {
    SourcePlusOneCounters,
    PerExperienceCounter(NonZeroU8),
    Fixed(NonZeroU8),
    ControlledCreaturesWithPlusOneCounters,
}

/// Which kinds of cards a permission lets its controller play from their library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryTopPermission {
    Land,
    InstantOrSorcery,
    ArtifactOrColorlessSpell,
    CreatureOfSourceEntryType,
}

/// Conspiracy command-zone agenda metadata (14 face-up, 14 Hidden Agenda, 1 Double Agenda).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgendaSpec {
    pub hidden: bool,
    pub named_slots: u8,
}

/// Static spell cost modifier or alternative-cost rule.
// Keep SpellFilter inline for value semantics on static rule definitions without extra boxing churn.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellCostModifier {
    /// Reduce generic cost of qualifying spells (either controller's spells or all players' spells).
    Discount {
        any_player: bool,
        matching: SpellFilter,
        amount: SpellDiscount,
    },
    /// Increase spell costs by `amount` except for spells with card type `exempt`.
    IncreaseExceptType { exempt: CardType, amount: NonZeroU8 },
    /// Increase generic cost of qualifying spells (for all players, only opponents, or only controller).
    Increase {
        any_player: bool,
        opponents_only: bool,
        matching: SpellFilter,
        amount: NonZeroU8,
    },
    /// Reduce cost of other spells of `subtype` from battlefield or command zone (eminence).
    SubtypeDiscountFromCommandOrBattlefield {
        subtype: CreatureTypeId,
        amount: NonZeroU8,
    },
    /// Spells of the source's chosen creature type cost one less of each color.
    ChosenTypeOneLessOfEachColor,
    /// Controller may pay zero instead of mana cost for creature spells of `subtype`.
    FreeSubtypeCreatureSpells(CreatureTypeId),
    /// Once per turn, controller may cast a colorless spell from hand without paying its mana cost.
    FreeColorlessHandSpellOnce,
    /// Once on controller's turn, controller may pay life instead of mana for an enchantment spell.
    PayLifeForEnchantmentOwnTurnOnce,
    /// Nonartifact spells controller casts have improvise.
    NonartifactSpellsHaveImprovise,
    /// Spells with mana value at least `minimum` cost `reduction` less and cannot be countered.
    HighManaValueProtected {
        minimum: NonZeroU8,
        reduction: NonZeroU8,
    },
}

/// Static casting, library-top, graveyard-cast, and land-play permission rule.
// Keep SpellFilter inline for value semantics on static permission rules.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastPermissionRule {
    /// Cast qualifying spells as though they had flash.
    Flash {
        any_player: bool,
        matching: SpellFilter,
    },
    /// Continuously reveal top library card publicly or privately to controller.
    SeeLibraryTop { public: bool },
    /// Play qualifying cards from the top of controller's library.
    PlayLibraryTop(LibraryTopPermission),
    /// Once per turn, cast a creature spell from controller's graveyard.
    GraveyardCreatureCast,
    /// Creature cards of `subtype` in controller's graveyard have encore.
    GraveyardSubtypeEncore(CreatureTypeId),
    /// Spells of `subtype` cast by controller have replicate.
    SubtypeSpellsHaveReplicate(CreatureTypeId),
    /// Colorless spells cast from hand with mana value >= `minimum_mana_value` have `count` cascade instances.
    ColorlessHandSpellsHaveCascade {
        minimum_mana_value: u16,
        count: NonZeroU8,
    },
    /// Additional land plays available to controller each turn.
    AdditionalLandPlays(NonZeroU8),
    /// Controlled spells with any listed card type cannot be countered.
    ControllerSpellsCannotBeCountered(BTreeSet<CardType>),
}

/// Static damage prevention, doubling, replacement, or lethality rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticDamageRule {
    /// Prevent noncombat damage to controller and other controlled permanents.
    PreventNoncombat,
    /// Prevent all combat damage dealt by or to source.
    PreventSourceCombat,
    /// Prevent combat damage to attacking artifact creatures controlled by source's controller.
    PreventCombatToAttackingArtifacts,
    /// Prevent damage dealt by (and if `combat_only`, dealt to) enchanted creature.
    PreventEnchantedCreature { combat_only: bool },
    /// Prevent combat damage to source and exile that many cards from controller's library.
    PreventIncomingCombatAndExile,
    /// Prevent incoming damage to source by removing +1/+1 counters.
    PreventIncomingRemovePlusOne,
    /// Prevent half of incoming damage rounded up.
    PreventHalfIncoming,
    /// Double damage dealt to opponents.
    DoubleToOpponents,
    /// Double combat damage dealt by equipped creature.
    DoubleEquippedCombat,
    /// Double damage dealt to enchanted player.
    DoubleToEnchantedPlayer,
    /// Replace combat damage dealt by controlled `CreatureTypeId` to a player with mill.
    ReplaceSubtypeCombatWithMill(CreatureTypeId),
    /// Positive marked damage is lethal against power instead of toughness for controlled creatures.
    LethalUsesPower,
}

/// Static draw replacement, draw limit, or hand-size rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticDrawRule {
    OpponentsAtMostOne,
    EmptyDrawWins,
    SplitOpponentsMultipleDraw,
    ReplaceWithLandKind,
    SkipControllerDrawStep,
    NoMaximumHandSize,
}

/// Static untap-step modification rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticUntapRule {
    UntapDuringOtherPlayersUntap,
    /// Untap all permanents matching `selector` during each other player's untap step (Seedborn Muse).
    UntapMatchingDuringOtherPlayersUntap(Box<PermanentSelector>),
    EnchantedCreatureUntapsOnlyForMonarch,
    EnchantedCreatureSkipsUntap,
    SourceSkipsUntap,
}

/// Static prohibition, cast limit, targeting restriction, or transform restriction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticRestriction {
    PreventLoyaltyActivations,
    PreventArtifactActivations,
    PreventControlledTransform {
        subtype: CreatureTypeId,
        except: CreatureTypeId,
    },
    ProhibitLifeAndCreatureCosts,
    ProhibitOpponentsCausingSacrifice,
    OpponentsCannotGainLife,
    PlayersCannotGainLife,
    OthersEnterTapped(Box<PermanentSelector>),
    PreventMatchingActivations(Box<PermanentSelector>),
    RestrictOpponentsDuringOwnTurn,
    EnchantedPlayerCastLimit,
    ProhibitCastingChosenName(Arc<NonlandCardNames>),
    RestrictGraveyardAndLibrary,
    CombatRequiresCityBlessing,
    ControllerHasHexproof,
    /// Each player can't cast more than one spell each turn (CR 101.2 / Rule of Law).
    OneSpellPerTurn,
    /// Creatures entering the battlefield don't cause abilities to trigger (Torpor Orb).
    SuppressEntersTriggers,
}

/// Static battlefield replacement, doubling, mana retention, or structural modifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticModifierRule {
    DoubleCreatedTokens,
    DoublePlacedCounters,
    AdditionalSelfPlusOneCounter,
    OtherCreaturesEnterWithSourceCounters,
    /// Matching permanents enter with `count` additional `kind` counters (Good-Fortune Unicorn, Grumgully).
    OthersEnterWithCounters {
        selector: Box<PermanentSelector>,
        kind: crate::effects::CounterKind,
        count: u8,
    },
    ExtraEntryLoyalty,
    TwiceLoyaltyPerTurn,
    AdditionalCreatureDeathTrigger,
    EnterControlledCreaturesAsEnchantedCopy,
    EnterControlledCreaturesAsSourceCopy,
    IgnoreLegendForSubtype(CreatureTypeId),
    /// The "legend rule" doesn't apply to permanents any player controls (Mirror Gallery).
    IgnoreLegendRule,
    /// The "legend rule" doesn't apply to permanents this permanent's controller controls.
    IgnoreLegendRuleForController,
    RetainMana(ManaType),
    Ascend,
    AdditionalBlockWhileMonarch,
}

/// Orthogonal static continuous ability replacing the flat 69-variant enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticContinuousAbility {
    /// Ordinary continuous effects generated by a battlefield source.
    Battlefield {
        id: StaticAbilityId,
        selector: PermanentSelector,
        parts: StaticParts,
    },
    /// Printed self-only definitions in every represented physical zone (CR 604.3a, 613.6).
    CharacteristicDefining {
        id: StaticAbilityId,
        parts: DefiningParts,
    },
    /// Spell cost discounts, taxes, and alternative-cost permissions.
    CostModifier {
        id: StaticAbilityId,
        modifier: SpellCostModifier,
    },
    /// Flash timing, library-top access, graveyard casting, extra land plays, and uncounterability.
    CastPermission {
        id: StaticAbilityId,
        permission: CastPermissionRule,
    },
    /// Static damage prevention, doubling, replacement, and lethality rules.
    DamageRule {
        id: StaticAbilityId,
        rule: StaticDamageRule,
    },
    /// Static draw replacements, limits, skips, and hand-size overrides.
    DrawRule {
        id: StaticAbilityId,
        rule: StaticDrawRule,
    },
    /// Static untap-step overrides.
    UntapRule {
        id: StaticAbilityId,
        rule: StaticUntapRule,
    },
    /// Static activation, casting, targeting, life-gain, and combat prohibitions.
    Restriction {
        id: StaticAbilityId,
        rule: StaticRestriction,
    },
    /// Static token/counter/loyalty/death-trigger/legend/mana/copy/ascend modifiers.
    Modifier {
        id: StaticAbilityId,
        rule: StaticModifierRule,
    },
    /// Conspiracy command-zone active ability with optional hidden named card slots.
    CommandZoneActive {
        id: StaticAbilityId,
        agenda: AgendaSpec,
    },
}

impl StaticContinuousAbility {
    /// The printed identity is independent of the effect's operating zone.
    #[must_use]
    pub fn id(&self) -> StaticAbilityId {
        match self {
            Self::Battlefield { id, .. }
            | Self::CharacteristicDefining { id, .. }
            | Self::CostModifier { id, .. }
            | Self::CastPermission { id, .. }
            | Self::DamageRule { id, .. }
            | Self::DrawRule { id, .. }
            | Self::UntapRule { id, .. }
            | Self::Restriction { id, .. }
            | Self::Modifier { id, .. }
            | Self::CommandZoneActive { id, .. } => *id,
        }
    }

    /// Assign an identity within the combined text of a merged permanent.
    pub(crate) fn with_component_id(mut self, new_id: StaticAbilityId) -> Self {
        match &mut self {
            Self::Battlefield { id, .. }
            | Self::CharacteristicDefining { id, .. }
            | Self::CostModifier { id, .. }
            | Self::CastPermission { id, .. }
            | Self::DamageRule { id, .. }
            | Self::DrawRule { id, .. }
            | Self::UntapRule { id, .. }
            | Self::Restriction { id, .. }
            | Self::Modifier { id, .. }
            | Self::CommandZoneActive { id, .. } => *id = new_id,
        }
        self
    }

    /// Whether this static ability requires a permanent program definition (rejected on instant/sorcery spells).
    #[must_use]
    pub(crate) fn requires_permanent(&self) -> bool {
        match self {
            Self::CharacteristicDefining { .. } | Self::CommandZoneActive { .. } => false,
            Self::Battlefield { .. } | Self::UntapRule { .. } | Self::DamageRule { .. } => true,
            Self::CostModifier { modifier, .. } => !matches!(
                modifier,
                SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { .. }
                    | SpellCostModifier::HighManaValueProtected { .. }
            ),
            Self::CastPermission { permission, .. } => !matches!(
                permission,
                CastPermissionRule::GraveyardCreatureCast
                    | CastPermissionRule::ColorlessHandSpellsHaveCascade { .. }
                    | CastPermissionRule::ControllerSpellsCannotBeCountered(_)
            ),
            Self::DrawRule { rule, .. } => matches!(rule, StaticDrawRule::NoMaximumHandSize),
            Self::Restriction { rule, .. } => !matches!(
                rule,
                StaticRestriction::ProhibitLifeAndCreatureCosts
                    | StaticRestriction::ProhibitOpponentsCausingSacrifice
            ),
            Self::Modifier { rule, .. } => {
                !matches!(rule, StaticModifierRule::AdditionalBlockWhileMonarch)
            }
        }
    }

    /// Whether this static ability is admitted in `Program::additional_battlefield_abilities`.
    #[must_use]
    pub(crate) fn allowed_as_additional(&self) -> bool {
        matches!(
            self,
            Self::Battlefield { .. }
                | Self::CostModifier { .. }
                | Self::CastPermission {
                    permission: CastPermissionRule::Flash { .. }
                        | CastPermissionRule::GraveyardCreatureCast
                        | CastPermissionRule::SubtypeSpellsHaveReplicate(_)
                        | CastPermissionRule::GraveyardSubtypeEncore(_)
                        | CastPermissionRule::SeeLibraryTop { .. }
                        | CastPermissionRule::PlayLibraryTop(_)
                        | CastPermissionRule::AdditionalLandPlays(_)
                        | CastPermissionRule::ControllerSpellsCannotBeCountered(_),
                    ..
                }
                | Self::DamageRule {
                    rule: StaticDamageRule::PreventHalfIncoming,
                    ..
                }
                | Self::DrawRule {
                    rule: StaticDrawRule::NoMaximumHandSize
                        | StaticDrawRule::OpponentsAtMostOne
                        | StaticDrawRule::SkipControllerDrawStep,
                    ..
                }
                | Self::UntapRule { .. }
                | Self::Restriction {
                    rule: StaticRestriction::CombatRequiresCityBlessing
                        | StaticRestriction::PreventLoyaltyActivations
                        | StaticRestriction::PreventArtifactActivations
                        | StaticRestriction::PreventControlledTransform { .. }
                        | StaticRestriction::ProhibitLifeAndCreatureCosts
                        | StaticRestriction::ProhibitOpponentsCausingSacrifice
                        | StaticRestriction::OpponentsCannotGainLife
                        | StaticRestriction::PlayersCannotGainLife
                        | StaticRestriction::OthersEnterTapped(_)
                        | StaticRestriction::PreventMatchingActivations(_)
                        | StaticRestriction::RestrictOpponentsDuringOwnTurn
                        | StaticRestriction::RestrictGraveyardAndLibrary
                        | StaticRestriction::ControllerHasHexproof
                        | StaticRestriction::OneSpellPerTurn
                        | StaticRestriction::SuppressEntersTriggers,
                    ..
                }
                | Self::Modifier {
                    rule: StaticModifierRule::IgnoreLegendForSubtype(_)
                        | StaticModifierRule::IgnoreLegendRule
                        | StaticModifierRule::IgnoreLegendRuleForController
                        | StaticModifierRule::Ascend
                        | StaticModifierRule::DoublePlacedCounters
                        | StaticModifierRule::DoubleCreatedTokens
                        | StaticModifierRule::AdditionalSelfPlusOneCounter
                        | StaticModifierRule::OtherCreaturesEnterWithSourceCounters
                        | StaticModifierRule::OthersEnterWithCounters { .. }
                        | StaticModifierRule::ExtraEntryLoyalty,
                    ..
                }
        )
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type_in_rule(&mut self, change: CreatureTypeTextChange) {
        let word = |kind: &mut CreatureTypeId| {
            if *kind == change.from {
                *kind = change.to;
            }
        };
        match self {
            Self::CostModifier { modifier, .. } => match modifier {
                SpellCostModifier::Discount { matching, .. } => {
                    matching.rewrite_creature_type(change.from, change.to);
                }
                SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { subtype, .. }
                | SpellCostModifier::FreeSubtypeCreatureSpells(subtype) => word(subtype),
                _ => {}
            },
            Self::CastPermission { permission, .. } => match permission {
                CastPermissionRule::Flash { matching, .. } => {
                    matching.rewrite_creature_type(change.from, change.to);
                }
                CastPermissionRule::SubtypeSpellsHaveReplicate(subtype)
                | CastPermissionRule::GraveyardSubtypeEncore(subtype) => word(subtype),
                _ => {}
            },
            Self::DamageRule {
                rule: StaticDamageRule::ReplaceSubtypeCombatWithMill(subtype),
                ..
            } => word(subtype),
            Self::Restriction {
                rule: StaticRestriction::PreventControlledTransform { subtype, except },
                ..
            } => {
                word(subtype);
                word(except);
            }
            Self::Modifier {
                rule: StaticModifierRule::IgnoreLegendForSubtype(subtype),
                ..
            } => word(subtype),
            _ => {}
        }
    }

    /// Validate ordinary selectors/parts or finite self-only defining parts.
    ///
    /// # Errors
    /// Returns a typed selector, part, vocabulary or empty-definition failure.
    pub fn validate(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), CharacteristicDefinitionError> {
        let (selector, parts) = match self {
            Self::CostModifier { modifier, .. } => match modifier {
                SpellCostModifier::Discount { matching, .. }
                | SpellCostModifier::Increase { matching, .. } => {
                    return matching
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType);
                }
                SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { subtype, .. }
                | SpellCostModifier::FreeSubtypeCreatureSpells(subtype) => {
                    return validate_subtypes(&BTreeSet::from([*subtype]), vocabulary);
                }
                SpellCostModifier::IncreaseExceptType { .. }
                | SpellCostModifier::ChosenTypeOneLessOfEachColor
                | SpellCostModifier::FreeColorlessHandSpellOnce
                | SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce
                | SpellCostModifier::NonartifactSpellsHaveImprovise
                | SpellCostModifier::HighManaValueProtected { .. } => return Ok(()),
            },
            Self::CastPermission { permission, .. } => match permission {
                CastPermissionRule::Flash { matching, .. } => {
                    return matching
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType);
                }
                CastPermissionRule::SubtypeSpellsHaveReplicate(subtype)
                | CastPermissionRule::GraveyardSubtypeEncore(subtype) => {
                    return validate_subtypes(&BTreeSet::from([*subtype]), vocabulary);
                }
                CastPermissionRule::ControllerSpellsCannotBeCountered(types) => {
                    if types.is_empty() {
                        return Err(CharacteristicDefinitionError::EmptyParts);
                    }
                    return Ok(());
                }
                CastPermissionRule::SeeLibraryTop { .. }
                | CastPermissionRule::PlayLibraryTop(_)
                | CastPermissionRule::GraveyardCreatureCast
                | CastPermissionRule::ColorlessHandSpellsHaveCascade { .. }
                | CastPermissionRule::AdditionalLandPlays(_) => return Ok(()),
            },
            Self::DamageRule { rule, .. } => {
                if let StaticDamageRule::ReplaceSubtypeCombatWithMill(subtype) = rule {
                    validate_subtypes(&BTreeSet::from([*subtype]), vocabulary)?;
                }
                return Ok(());
            }
            Self::DrawRule { .. } | Self::CommandZoneActive { .. } => {
                return Ok(());
            }
            Self::UntapRule { rule, .. } => {
                if let StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(selector) = rule {
                    return selector.validate(vocabulary);
                }
                return Ok(());
            }
            Self::Restriction { rule, .. } => match rule {
                StaticRestriction::ProhibitCastingChosenName(names) => {
                    if names.options().is_empty() {
                        return Err(CharacteristicDefinitionError::EmptyParts);
                    }
                    return Ok(());
                }
                StaticRestriction::PreventControlledTransform { subtype, except } => {
                    if vocabulary.label(*subtype).is_none() || vocabulary.label(*except).is_none() {
                        return Err(CharacteristicDefinitionError::ForeignCreatureType);
                    }
                    return Ok(());
                }
                StaticRestriction::OthersEnterTapped(selector)
                | StaticRestriction::PreventMatchingActivations(selector) => {
                    return selector.validate(vocabulary);
                }
                _ => return Ok(()),
            },
            Self::Modifier { rule, .. } => {
                match rule {
                    StaticModifierRule::IgnoreLegendForSubtype(subtype) => {
                        validate_subtypes(&BTreeSet::from([*subtype]), vocabulary)?;
                    }
                    StaticModifierRule::OthersEnterWithCounters {
                        selector,
                        kind,
                        count,
                    } => {
                        if *count == 0 || !crate::effects::CounterKind::PERMANENT.contains(kind) {
                            return Err(CharacteristicDefinitionError::EmptyParts);
                        }
                        selector.validate(vocabulary)?;
                    }
                    _ => {}
                }
                return Ok(());
            }
            Self::Battlefield {
                selector, parts, ..
            } => (selector, parts),
            Self::CharacteristicDefining { parts, .. } => {
                validate_buckets(parts.0.iter().map(DefiningPart::bucket))?;
                for part in &parts.0 {
                    match part {
                        DefiningPart::CreatureTypes(DefiningCreatureTypes::Literal(values)) => {
                            validate_subtypes(values, vocabulary)?
                        }
                        DefiningPart::PowerToughness { power, toughness } => {
                            if power.is_none() && toughness.is_none() {
                                return Err(
                                    CharacteristicDefinitionError::EmptyPowerToughnessDefinition,
                                );
                            }
                            for expression in power.iter().chain(toughness) {
                                expression.validate(vocabulary)?;
                            }
                        }
                        DefiningPart::CreatureTypes(DefiningCreatureTypes::All)
                        | DefiningPart::CommanderChosenColor(_)
                        | DefiningPart::Colors(_) => {}
                    }
                }
                return Ok(());
            }
        };
        selector.validate(vocabulary)?;
        if selector.controlled_creatures_and_typed_tokens.is_some()
            && !matches!(
                parts.0.as_slice(),
                [StaticPart::Type(StaticTypeChange {
                    types: TypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    land_subtypes: SubtypeOperation::Keep,
                    subtypes: SubtypeOperation::Add(StaticSubtypeOperand::SourceEntryCreatureType),
                })]
            )
        {
            return Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
        }
        validate_buckets(parts.0.iter().map(StaticPart::bucket))?;
        for part in &parts.0 {
            if let StaticPart::GrantSubtypeProtection {
                types: StaticSubtypeOperand::Literal(types),
                ..
            } = part
                && (types.is_empty() || types.iter().any(|kind| vocabulary.label(*kind).is_none()))
            {
                return Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
            }
            if let StaticPart::Abilities(change) = part {
                if let Some(trigger) = &change.granted_trigger {
                    trigger
                        .validate_body(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
                }
                change.validate_grants()?;
            }
            match part {
                StaticPart::BlockingRestriction(
                    BlockingRestriction::BlockerPowerAtLeastPowerOf(_),
                ) => {
                    return Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
                }
                StaticPart::BlockingRestriction(
                    BlockingRestriction::OnlyBlockedBySubtype(kind)
                    | BlockingRestriction::CannotBlockSubtype(kind)
                    | BlockingRestriction::CannotBeBlockedBySubtype(kind),
                ) => {
                    validate_subtypes(&BTreeSet::from([*kind]), vocabulary)?;
                }
                StaticPart::BlockingRestriction(
                    BlockingRestriction::CannotBlockMatching(filter)
                    | BlockingRestriction::CannotBeBlockedByMatching(filter)
                    | BlockingRestriction::OnlyBlockedByMatching(filter),
                ) => {
                    filter
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::ForeignCreatureType)?;
                }
                StaticPart::GrantActivated(ability) => {
                    let is_grantor_sac = matches!(
                        ability.source_cost,
                        crate::activated::SourceCost::SacrificeSelected {
                            filter: crate::activated::SelectedSacrificeFilter::GrantingPermanent,
                            count: 1,
                            tap_source: true,
                            from_graveyard: false,
                        }
                    );
                    if ability.id.0 != 0
                        || !ability.mana_cost.valid()
                        || !matches!(
                            ability.source_cost,
                            crate::activated::SourceCost::PayLife { .. }
                                | crate::activated::SourceCost::None
                                | crate::activated::SourceCost::Tap
                                | crate::activated::SourceCost::Sacrifice
                                | crate::activated::SourceCost::TapSacrifice
                                | crate::activated::SourceCost::Untap
                                | crate::activated::SourceCost::PayEnergy { .. }
                                | crate::activated::SourceCost::ReturnSelfToHand { .. }
                                | crate::activated::SourceCost::Exile {
                                    from_graveyard: false,
                                    ..
                                }
                                | crate::activated::SourceCost::GenericX { .. }
                                | crate::activated::SourceCost::TapPermanents { .. }
                                | crate::activated::SourceCost::Counter { .. }
                                | crate::activated::SourceCost::SacrificeSelected {
                                    count: 1..,
                                    from_graveyard: false,
                                    ..
                                }
                                | crate::activated::SourceCost::Discard {
                                    mode: crate::activated::DiscardCostMode::SelectedOne
                                        | crate::activated::DiscardCostMode::Filtered { .. },
                                    ..
                                }
                                | crate::activated::SourceCost::ExileGraveyard { .. }
                        )
                    {
                        return Err(CharacteristicDefinitionError::InvalidKeywordGrant);
                    }
                    if is_grantor_sac
                        != matches!(
                            ability.body.instructions.as_slice(),
                            [crate::program::Instruction::DamageFromGrantingPermanent { .. }]
                        )
                    {
                        return Err(CharacteristicDefinitionError::InvalidKeywordGrant);
                    }
                    let body_context = match ability.source_cost {
                        crate::activated::SourceCost::Counter { kind, .. }
                            if !crate::effects::CounterKind::PERMANENT.contains(&kind) =>
                        {
                            return Err(CharacteristicDefinitionError::InvalidKeywordGrant);
                        }
                        crate::activated::SourceCost::Counter {
                            delta:
                                crate::activated::CounterCostDelta::Remove(_)
                                | crate::activated::CounterCostDelta::RemoveAll,
                            ..
                        } => crate::program::BodyContext::Activated {
                            named_counter_cost: true,
                        },
                        crate::activated::SourceCost::SacrificeSelected {
                            filter:
                                crate::activated::SelectedSacrificeFilter::Creature
                                | crate::activated::SelectedSacrificeFilter::OtherCreature,
                            count: 1,
                            from_graveyard: false,
                            ..
                        }
                        | crate::activated::SourceCost::PayLife {
                            sacrifice:
                                Some(
                                    crate::activated::SelectedSacrificeFilter::Creature
                                    | crate::activated::SelectedSacrificeFilter::OtherCreature,
                                ),
                            ..
                        } => crate::program::BodyContext::SacrificedCreature,
                        crate::activated::SourceCost::SacrificeSelected {
                            count: 1,
                            from_graveyard: false,
                            ..
                        } => crate::program::BodyContext::SacrificedPermanent,
                        _ => crate::program::BodyContext::ACTIVATED,
                    };
                    ability
                        .body
                        .validate_in(vocabulary, body_context)
                        .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
                }
                StaticPart::GrantTrigger(ability) => {
                    ability
                        .validate_body(vocabulary)
                        .map_err(|_| CharacteristicDefinitionError::InvalidKeywordGrant)?;
                }
                StaticPart::Text(change) => {
                    validate_subtypes(&BTreeSet::from([change.from, change.to]), vocabulary)?;
                }
                StaticPart::Type(change) => {
                    validate_battlefield_types(&change.types)?;
                    match &change.subtypes {
                        SubtypeOperation::Keep
                        | SubtypeOperation::Add(StaticSubtypeOperand::SourceCurrentSubtypes)
                        | SubtypeOperation::Set(StaticSubtypeOperand::SourceCurrentSubtypes)
                        | SubtypeOperation::Add(StaticSubtypeOperand::SourceEntryCreatureType)
                        | SubtypeOperation::Set(StaticSubtypeOperand::SourceEntryCreatureType) => {}
                        SubtypeOperation::Remove(values)
                        | SubtypeOperation::Add(StaticSubtypeOperand::Literal(values))
                        | SubtypeOperation::Add(StaticSubtypeOperand::ImprintedCreatureTypes {
                            keep: values,
                        })
                        | SubtypeOperation::Set(StaticSubtypeOperand::ImprintedCreatureTypes {
                            keep: values,
                        })
                        | SubtypeOperation::Set(StaticSubtypeOperand::Literal(values)) => {
                            validate_subtypes(values, vocabulary)?
                        }
                    }
                }
                StaticPart::SetPT(pair) | StaticPart::ModifyPT(pair) => {
                    for expression in [&pair.power, &pair.toughness] {
                        expression.validate(vocabulary)?;
                    }
                }
                StaticPart::Names(_)
                | StaticPart::InheritCounteredCreatureAbilities
                | StaticPart::InheritExiledCreatureAbilities
                | StaticPart::ControlSourceController
                | StaticPart::Color(_)
                | StaticPart::Abilities(_)
                | StaticPart::GrantFixedColorMana(_)
                | StaticPart::GrantAnyColorMana
                | StaticPart::GrantSubtypeProtection { .. }
                | StaticPart::BlockingRestriction(_)
                | StaticPart::SetPTToOwnManaValue
                | StaticPart::SwitchPT => {}
            }
        }
        Ok(())
    }
}
