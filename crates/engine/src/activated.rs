//! Authored non-mana abilities on battlefield and hand sources.
//! The executor shares existing costs, targets, stack payloads and resolution.
use crate::{
    effects::EffectWindow,
    mana::{ManaCost, ManaPaymentRecord},
    object_identity::{GameObject, ObjectId},
    priority::SeatId,
    program::ExecutableBody,
    stack_window::StackEntryId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActivatedAbilityId(pub u64);

/// Printed restriction on activating this particular ability (CR 602.5).
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationRestriction {
    DuringUpkeep,
    /// Each player can activate this battlefield ability and pays its mana cost.
    AnyPlayer,
    /// Only opponents of this permanent's controller may activate this battlefield ability.
    OnlyOpponents,
    CreatedTokenThisTurn,
    HasCityBlessing,
    EmptyHand,
    /// A creature died earlier during the current turn.
    CreatureDiedThisTurn,
    ControlsCreaturePowerFourOrGreater,
    /// Require a minimum number of controlled permanents with this subtype.
    ControlsSubtypeCount {
        kind: spellwake_contracts::creature_types::CreatureTypeId,
        minimum: std::num::NonZeroU8,
    },
    ControlsThreeLandsWithSameName,
    Sorcery,
    /// Activate on your turn before the declare attackers step begins.
    OwnTurnBeforeAttackers,
    /// CR 702.193. Activate once; reduce the mana cost if this permanent entered this turn.
    PowerUp,
    /// Reduce the generic payment once per controlled legendary creature or planeswalker.
    DiscountLegendaryCreaturesAndPlaneswalkers,
    Exhaust,
    /// The same ability of this object may be activated once each turn.
    OnceEachTurn,
    /// Activate only during your turn.
    DuringYourTurn,
    /// Activate only during an opponent's turn.
    DuringOpponentsTurn,
    /// Activate only during combat.
    DuringCombat,
    /// Activate only during a specific turn step.
    DuringStep(crate::turn::Step),
    /// Activate only during a specific step of your turn.
    DuringOwnStep(crate::turn::Step),
    /// Activate only during a specific step of an opponent's turn.
    DuringOpponentsStep(crate::turn::Step),
    /// Activate at most `n` times each turn (CR 602.5b).
    MaxPerTurn(u8),
    /// Activate only as a sorcery and only once each turn.
    SorceryOnceEachTurn,
    /// Activate only during your turn and only once each turn.
    DuringYourTurnOnceEachTurn,
    /// Activate only if seven or more cards are in your graveyard (Threshold).
    Threshold,
    /// Activate only if four or more card types are among cards in your graveyard (Delirium).
    Delirium,
    /// Activate only if you control three or more artifacts (Metalcraft).
    Metalcraft,
    /// Activate only if this permanent is tapped.
    SourceIsTapped,
    /// Activate only if this permanent is untapped.
    SourceIsUntapped,
    /// Activate only while this Class permanent is at exactly this level (CR 716.2a).
    ClassLevelExact(u8),
    /// Activate only while this Class permanent is at least this level (CR 716.2b).
    MinClassLevel(u8),
    /// Activate only while this Case permanent is solved (CR 719.3b).
    SourceIsSolved,
    /// Activate only if your life total is at least this amount.
    ControllerLifeAtLeast(u16),
    /// Activate only if your life total is at most this amount.
    ControllerLifeAtMost(u16),
    /// Activate only if you control at least `minimum` permanents of `kind`.
    ControlsCardType {
        kind: crate::program::CardType,
        minimum: u8,
    },
    /// Activate only if you control at least `minimum` permanents matching `filter`.
    ControlsMatching {
        filter: crate::program::ObjectFilter,
        minimum: u8,
    },
    /// Activate only if the ability source itself matches `filter` (e.g., Vehicle is a creature).
    SourceMatches(crate::program::ObjectFilter),
    /// Activate only if an arbitrary resolution condition evaluates true at activation time.
    Condition(Box<crate::program::ResolutionCondition>),
    /// Activate only if every sub-restriction is satisfied.
    All(Vec<ActivationRestriction>),
}

impl ActivationRestriction {
    /// Tightest per-turn activation cap imposed by this restriction, if any.
    #[must_use]
    pub fn max_per_turn(&self) -> Option<u8> {
        match self {
            Self::OnceEachTurn | Self::SorceryOnceEachTurn | Self::DuringYourTurnOnceEachTurn => {
                Some(1)
            }
            Self::MaxPerTurn(limit) => Some(*limit),
            Self::All(items) => items.iter().filter_map(Self::max_per_turn).min(),
            _ => None,
        }
    }

    /// Whether this restriction imposes any per-turn activation cap.
    #[must_use]
    pub fn has_turn_activation_cap(&self) -> bool {
        self.max_per_turn().is_some()
    }

    /// Whether this restriction imposes a once-per-turn activation cap.
    #[must_use]
    pub fn has_once_each_turn(&self) -> bool {
        match self {
            Self::OnceEachTurn | Self::SorceryOnceEachTurn | Self::DuringYourTurnOnceEachTurn => {
                true
            }
            Self::All(items) => items.iter().any(Self::has_once_each_turn),
            _ => false,
        }
    }

    /// Whether this restriction imposes a once-per-game / exhaust activation cap.
    #[must_use]
    pub fn has_exhaust(&self) -> bool {
        match self {
            Self::Exhaust | Self::PowerUp => true,
            Self::All(items) => items.iter().any(Self::has_exhaust),
            _ => false,
        }
    }

    /// Whether any player may activate this ability.
    #[must_use]
    pub fn is_any_player(&self) -> bool {
        match self {
            Self::AnyPlayer => true,
            Self::All(items) => items.iter().any(Self::is_any_player),
            _ => false,
        }
    }

    /// Whether only opponents of the permanent's controller may activate this ability.
    #[must_use]
    pub fn is_only_opponents(&self) -> bool {
        match self {
            Self::OnlyOpponents => true,
            Self::All(items) => items.iter().any(Self::is_only_opponents),
            _ => false,
        }
    }

    /// Whether this restriction includes the Power-Up activation discount/cap.
    #[must_use]
    pub fn has_power_up(&self) -> bool {
        match self {
            Self::PowerUp => true,
            Self::All(items) => items.iter().any(Self::has_power_up),
            _ => false,
        }
    }

    /// Whether this restriction includes the legendary creatures/planeswalkers activation discount.
    #[must_use]
    pub fn has_legendary_discount(&self) -> bool {
        match self {
            Self::DiscountLegendaryCreaturesAndPlaneswalkers => true,
            Self::All(items) => items.iter().any(Self::has_legendary_discount),
            _ => false,
        }
    }

    /// Validate vocabulary references and unbound condition operands.
    pub(crate) fn validate(
        &self,
        vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    ) -> Result<(), crate::program::DefinitionError> {
        match self {
            Self::MaxPerTurn(0) => Err(crate::program::DefinitionError::InvalidInstruction),
            Self::ControlsSubtypeCount { kind, .. } => {
                if vocabulary.label(*kind).is_none() {
                    return Err(crate::program::DefinitionError::ForeignCreatureType);
                }
                Ok(())
            }
            Self::ControlsMatching { filter, .. } | Self::SourceMatches(filter) => {
                filter.validate_vocabulary(vocabulary)
            }
            Self::Condition(cond) => cond.validate(
                crate::program::BodyContext::Activated {
                    named_counter_cost: false,
                },
                vocabulary,
            ),
            Self::All(items) => {
                if items.is_empty() {
                    return Err(crate::program::DefinitionError::InvalidInstruction);
                }
                for item in items {
                    item.validate(vocabulary)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Current-characteristic filter for selected permanent sacrifice costs.
// Preserves `Copy` semantics on `SourceCost` by storing `ObjectFilter` inline.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedSacrificeFilter {
    Artifact,
    OtherCreature,
    /// An enchantment other than the ability source.
    OtherEnchantment,
    Creature,
    ArtifactOrCreature,
    CreatureOrEnchantment,
    Desert,
    /// The attached permanent that granted this activated ability.
    GrantingPermanent,
    /// A permanent with the selected creature subtype, including kindred permanents.
    Subtype(spellwake_contracts::creature_types::CreatureTypeId),
    /// Any controlled battlefield permanent matching `ObjectFilter`.
    Matching(crate::program::ObjectFilter),
}

/// Life amount paid as part of an activation cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifeCostAmount {
    /// Pay a fixed amount of life.
    Fixed(u16),
    /// Pay one life per distinct color in your commanders' combined color identity.
    CommanderColors,
}

/// Counter modification paid on the ability source as an activation cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterCostDelta {
    /// Put `n` counters of this kind on the source.
    Add(std::num::NonZeroU8),
    /// Remove `n` counters of this kind from the source.
    Remove(std::num::NonZeroU8),
    /// Remove all counters of this kind from the source (recording how many were removed).
    RemoveAll,
}

/// Permanent filter for tap-permanent activation costs.
// Preserves `Copy` semantics on `SourceCost` by storing `ObjectFilter` inline.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapCostFilter {
    /// A controlled untapped creature.
    Creature,
    /// A controlled untapped artifact.
    Artifact,
    /// A controlled untapped legendary permanent.
    Legendary,
    /// A controlled untapped permanent with the specified creature subtype.
    Subtype(spellwake_contracts::creature_types::CreatureTypeId),
    /// Any controlled untapped battlefield permanent matching `ObjectFilter`.
    Matching(crate::program::ObjectFilter),
}

/// Number of permanents tapped by a tap-permanent activation cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapCostCount {
    /// Tap an exact fixed number of matching permanents.
    Fixed(std::num::NonZeroU8),
    /// Tap X matching permanents; X is bound to the announced count.
    ChosenX,
}

/// Discard mode for an activated ability's discard cost.
// Preserves `Copy` semantics on `SourceCost` by storing `ObjectFilter` inline.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardCostMode {
    /// Discard one selected card from the activator's hand.
    SelectedOne,
    /// Discard every card in the activator's hand simultaneously.
    WholeHand,
    /// Discard one card matching the announced target spell's mana value.
    MatchingTargetSpellManaValue,
    /// Discard `count` cards matching `filter` from the activator's hand.
    Filtered {
        filter: crate::program::ObjectFilter,
        count: std::num::NonZeroU8,
    },
}

/// Graveyard-card exile policy for an activated ability cost.
// Preserves `Copy` semantics on `SourceCost` by storing `ObjectFilter` inline.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraveyardExileCost {
    /// Exile one creature card from the activator's graveyard, optionally paying its mana cost.
    Creature { pay_mana_cost: bool },
    /// Exile any number (X) of cards from the activator's graveyard.
    AnyCountX,
    /// Exile `count` cards matching `filter` from the activator's graveyard.
    Filtered {
        filter: crate::program::ObjectFilter,
        count: std::num::NonZeroU8,
    },
}

/// Hand discard-self keyword action (CR 702.29 / 702.53 / 702.77 / Channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandDiscardKeyword {
    Cycling,
    Transmute,
    Reinforce,
    Channel,
}

/// Orthogonal physical/non-mana cost paid when activating an ability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceCost {
    /// Only the separately represented mana cost is paid; no physical source change.
    None,
    /// Tap the ability source.
    Tap,
    /// Sacrifice the ability source.
    Sacrifice,
    /// Tap and sacrifice the ability source as one atomic activation cost.
    TapSacrifice,
    /// Exile the ability source from the battlefield (optionally tapping first) or graveyard.
    Exile {
        tap_source: bool,
        from_graveyard: bool,
    },
    /// Pay life, optionally tapping the source and/or sacrificing a selected permanent.
    PayLife {
        amount: LifeCostAmount,
        tap_source: bool,
        sacrifice: Option<SelectedSacrificeFilter>,
    },
    /// Pay the announced X generic mana once per printed X symbol, optionally tapping the source.
    GenericX { symbols: u8, tap_source: bool },
    /// Add or remove counters on the ability source, optionally tapping and/or sacrificing it.
    Counter {
        kind: crate::effects::CounterKind,
        delta: CounterCostDelta,
        tap_source: bool,
        sacrifice_source: bool,
    },
    /// Tap controlled untapped permanents matching `filter`, optionally also tapping the source.
    TapPermanents {
        filter: TapCostFilter,
        count: TapCostCount,
        tap_source: bool,
        exclude_source: bool,
    },
    /// Sacrifice selected controlled permanents, optionally tapping the source or activating from graveyard.
    SacrificeSelected {
        filter: SelectedSacrificeFilter,
        count: u8,
        tap_source: bool,
        from_graveyard: bool,
    },
    /// Discard cards from the activator's hand, optionally tapping and/or sacrificing the source.
    Discard {
        mode: DiscardCostMode,
        tap_source: bool,
        sacrifice_source: bool,
    },
    /// Exile cards from the activator's graveyard, optionally tapping the source.
    ExileGraveyard {
        filter: GraveyardExileCost,
        tap_source: bool,
    },
    /// Mill cards from the activator's library as an activation cost, optionally tapping the source.
    Mill {
        count: std::num::NonZeroU8,
        tap_source: bool,
    },
    /// Discard this card from its owner's hand to pay a keyword ability cost.
    HandDiscardSelf(HandDiscardKeyword),
    /// Activate this ability while its source is in its owner's graveyard (optionally Unearth).
    GraveyardSelf { unearth: bool },
    /// Activate from graveyard by discarding cards from hand (e.g., Haunted Dead).
    GraveyardDiscard { mode: DiscardCostMode },
    /// Activate from graveyard by exiling other cards from your graveyard (e.g., Postmortem Professor).
    GraveyardExileOther { filter: GraveyardExileCost },
    /// Activate from graveyard by paying life.
    GraveyardPayLife { amount: LifeCostAmount },
    /// Reveal the source in hand (or command zone for commander ninjutsu) and return an unblocked attacker.
    Ninjutsu { from_command: bool },
    /// Pay energy counters from the activator's energy pool, optionally tapping the source.
    PayEnergy { amount: u16, tap_source: bool },
    /// Untap the ability source as a cost (`{Q}`, CR 107.1q).
    Untap,
    /// Return the ability source from the battlefield to its owner's hand, optionally tapping first.
    ReturnSelfToHand { tap_source: bool },
    /// Crew, Saddle, or Station power-contributor cost.
    Power { kind: PowerAbility, minimum: u64 },
}

impl SourceCost {
    /// Whether paying this cost requires tapping the ability source (and therefore checks summoning sickness).
    #[must_use]
    pub const fn taps_source(self) -> bool {
        matches!(
            self,
            Self::Tap
                | Self::TapSacrifice
                | Self::Exile {
                    tap_source: true,
                    ..
                }
                | Self::PayLife {
                    tap_source: true,
                    ..
                }
                | Self::GenericX {
                    tap_source: true,
                    ..
                }
                | Self::Counter {
                    tap_source: true,
                    ..
                }
                | Self::TapPermanents {
                    tap_source: true,
                    ..
                }
                | Self::SacrificeSelected {
                    tap_source: true,
                    ..
                }
                | Self::Discard {
                    tap_source: true,
                    ..
                }
                | Self::ExileGraveyard {
                    tap_source: true,
                    ..
                }
                | Self::Mill {
                    tap_source: true,
                    ..
                }
                | Self::PayEnergy {
                    tap_source: true,
                    ..
                }
                | Self::ReturnSelfToHand { tap_source: true }
        )
    }

    /// Whether the source must be tapped as a separate status step before moving objects or sacrificing.
    #[must_use]
    pub const fn taps_source_before_move(self) -> bool {
        matches!(
            self,
            Self::TapSacrifice
                | Self::Exile {
                    tap_source: true,
                    ..
                }
                | Self::Counter {
                    tap_source: true,
                    sacrifice_source: true,
                    ..
                }
                | Self::SacrificeSelected {
                    tap_source: true,
                    ..
                }
                | Self::Discard {
                    tap_source: true,
                    ..
                }
                | Self::ExileGraveyard {
                    tap_source: true,
                    ..
                }
                | Self::Mill {
                    tap_source: true,
                    ..
                }
                | Self::ReturnSelfToHand { tap_source: true }
        )
    }

    /// Whether this ability is activated from its owner's graveyard.
    #[must_use]
    pub const fn is_from_graveyard(self) -> bool {
        matches!(
            self,
            Self::GraveyardSelf { .. }
                | Self::GraveyardDiscard { .. }
                | Self::GraveyardExileOther { .. }
                | Self::GraveyardPayLife { .. }
                | Self::Exile {
                    from_graveyard: true,
                    ..
                }
                | Self::SacrificeSelected {
                    from_graveyard: true,
                    ..
                }
        )
    }

    /// Whether this cost sacrifices the ability source itself.
    #[must_use]
    pub const fn sacrifices_source(self) -> bool {
        matches!(
            self,
            Self::Sacrifice
                | Self::TapSacrifice
                | Self::Counter {
                    sacrifice_source: true,
                    ..
                }
                | Self::Discard {
                    sacrifice_source: true,
                    ..
                }
        )
    }

    /// Whether this cost exiles the ability source itself.
    #[must_use]
    pub const fn exiles_source(self) -> bool {
        matches!(self, Self::Exile { .. })
    }

    /// Whether this cost returns the ability source to its owner's hand.
    #[must_use]
    pub const fn returns_source_to_hand(self) -> bool {
        matches!(self, Self::ReturnSelfToHand { .. })
    }

    /// Whether this cost untaps the ability source (`{Q}`).
    #[must_use]
    pub const fn untaps_source(self) -> bool {
        matches!(self, Self::Untap)
    }

    /// Energy counters paid as part of this cost, if any.
    #[must_use]
    pub const fn energy_cost(self) -> Option<u16> {
        match self {
            Self::PayEnergy { amount, .. } => Some(amount),
            _ => None,
        }
    }

    /// Whether this cost expects `ActivationOrigin::Contributors`.
    #[must_use]
    pub const fn uses_contributors_origin(self) -> bool {
        matches!(
            self,
            Self::Power { .. }
                | Self::TapPermanents {
                    tap_source: false,
                    exclude_source: false,
                    filter: TapCostFilter::Subtype(_) | TapCostFilter::Legendary,
                    ..
                }
        )
    }

    /// Whether this cost expects `ActivationOrigin::SelectedCost`.
    #[must_use]
    pub const fn uses_selected_cost_origin(self) -> bool {
        matches!(
            self,
            Self::SacrificeSelected { .. }
                | Self::ExileGraveyard { .. }
                | Self::GraveyardExileOther { .. }
                | Self::PayLife {
                    sacrifice: Some(_),
                    ..
                }
                | Self::Discard {
                    mode: DiscardCostMode::SelectedOne
                        | DiscardCostMode::MatchingTargetSpellManaValue
                        | DiscardCostMode::Filtered { .. },
                    ..
                }
                | Self::GraveyardDiscard {
                    mode: DiscardCostMode::SelectedOne
                        | DiscardCostMode::MatchingTargetSpellManaValue
                        | DiscardCostMode::Filtered { .. },
                }
                | Self::TapPermanents {
                    tap_source: true,
                    ..
                }
                | Self::TapPermanents {
                    filter: TapCostFilter::Artifact | TapCostFilter::Creature,
                    ..
                }
        )
    }

    /// Whether this cost pays generic mana for printed X symbols.
    #[must_use]
    pub const fn generic_x_symbols(self) -> Option<u8> {
        match self {
            Self::GenericX { symbols, .. } => Some(symbols),
            _ => None,
        }
    }

    /// Counter modification paid on the ability source, if any.
    #[must_use]
    pub const fn counter_cost(self) -> Option<(crate::effects::CounterKind, CounterCostDelta)> {
        match self {
            Self::Counter { kind, delta, .. } => Some((kind, delta)),
            _ => None,
        }
    }

    /// Power-contributor requirement, if any.
    #[must_use]
    pub const fn power_cost(self) -> Option<(PowerAbility, u64)> {
        match self {
            Self::Power { kind, minimum } => Some((kind, minimum)),
            _ => None,
        }
    }

    /// Whether this cost is ninjutsu or commander ninjutsu.
    #[must_use]
    pub const fn is_ninjutsu(self) -> bool {
        matches!(self, Self::Ninjutsu { .. })
    }

    /// Whether the given source object is in a legal activation zone for ninjutsu.
    #[must_use]
    pub fn can_activate_ninjutsu(self, source: &GameObject, actor: SeatId) -> bool {
        match self {
            Self::Ninjutsu { from_command } => {
                source.zone() == crate::object_identity::ObjectZone::Hand(actor)
                    || (from_command
                        && source.zone() == crate::object_identity::ObjectZone::Command
                        && source.owner() == actor)
            }
            _ => false,
        }
    }

    /// Whether this cost discards the source card from hand.
    #[must_use]
    pub const fn discards_source(self) -> bool {
        matches!(self, Self::HandDiscardSelf(_))
    }

    /// Whether this cost belongs to the historical core activated-ability trace scope.
    #[must_use]
    pub const fn is_core_battlefield_cost(self) -> bool {
        matches!(
            self,
            Self::None | Self::Tap | Self::Sacrifice | Self::TapSacrifice
        )
    }

    /// Canonical kebab-case trace label for diagnostics and reference views.
    #[must_use]
    pub const fn trace_label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Tap => "tap",
            Self::Sacrifice => "sacrifice",
            Self::TapSacrifice => "tap-sacrifice",
            Self::Exile {
                from_graveyard: true,
                ..
            } => "graveyard-exile",
            Self::Exile {
                tap_source: true, ..
            } => "tap-exile",
            Self::Exile { .. } => "exile",
            Self::PayLife {
                sacrifice: Some(_), ..
            } => "pay-life-sacrifice-other-creature",
            Self::PayLife {
                amount: LifeCostAmount::CommanderColors,
                ..
            } => "tap-pay-commander-colors",
            Self::PayLife {
                tap_source: true, ..
            } => "tap-pay-life",
            Self::PayLife { .. } => "pay-life",
            Self::GenericX {
                tap_source: true, ..
            } => "tap-with-generic-x",
            Self::GenericX { .. } => "pay-generic-x",
            Self::Counter {
                delta: CounterCostDelta::Add(_),
                ..
            } => "tap-put-counter",
            Self::Counter {
                sacrifice_source: true,
                ..
            } => "remove-counters-sacrifice",
            Self::Counter { .. } => "remove-counters",
            Self::TapPermanents {
                count: TapCostCount::ChosenX,
                ..
            } => "tap-source-and-selected-subtype",
            Self::TapPermanents {
                tap_source: true,
                filter: TapCostFilter::Creature,
                ..
            } => "tap-source-and-creatures",
            Self::TapPermanents {
                tap_source: true, ..
            } => "tap-source-and-fixed-subtype",
            Self::TapPermanents {
                filter: TapCostFilter::Artifact,
                ..
            } => "tap-selected-artifacts",
            Self::TapPermanents {
                filter: TapCostFilter::Legendary,
                ..
            } => "tap-legendary-permanent",
            Self::TapPermanents { .. } => "tap-permanent-of-subtype",
            Self::SacrificeSelected {
                from_graveyard: true,
                ..
            } => "graveyard-sacrifice-artifacts",
            Self::SacrificeSelected {
                filter: SelectedSacrificeFilter::GrantingPermanent,
                ..
            } => "tap-sacrifice-grantor",
            Self::SacrificeSelected { .. } => "sacrifice-selected",
            Self::Discard {
                mode: DiscardCostMode::WholeHand,
                ..
            } => "tap-discard-hand",
            Self::Discard {
                mode: DiscardCostMode::MatchingTargetSpellManaValue,
                ..
            } => "discard-matching-spell-mana-value",
            Self::Discard { .. } => "discard-selected",
            Self::ExileGraveyard {
                filter: GraveyardExileCost::AnyCountX,
                ..
            } => "tap-and-exile-x-graveyard-cards",
            Self::ExileGraveyard {
                tap_source: true, ..
            } => "tap-exile-graveyard-creature",
            Self::ExileGraveyard { .. } => "exile-graveyard-creature",
            Self::Mill { .. } => "tap-mill-one",
            Self::HandDiscardSelf(HandDiscardKeyword::Cycling) => "cycle-discard",
            Self::HandDiscardSelf(HandDiscardKeyword::Transmute) => "transmute-discard",
            Self::HandDiscardSelf(HandDiscardKeyword::Reinforce) => "reinforce-discard",
            Self::HandDiscardSelf(HandDiscardKeyword::Channel) => "channel-discard",
            Self::GraveyardSelf { unearth: true } => "unearth",
            Self::GraveyardSelf { unearth: false } => "graveyard-self",
            Self::GraveyardDiscard { .. } => "graveyard-discard",
            Self::GraveyardExileOther { .. } => "graveyard-exile-other",
            Self::GraveyardPayLife { .. } => "graveyard-pay-life",
            Self::Ninjutsu {
                from_command: false,
            } => "ninjutsu",
            Self::Ninjutsu { from_command: true } => "commander_ninjutsu",
            Self::Power {
                kind: PowerAbility::Crew,
                ..
            } => "crew",
            Self::Power {
                kind: PowerAbility::Saddle,
                ..
            } => "saddle",
            Self::Power {
                kind: PowerAbility::Station,
                ..
            } => "station",
            Self::PayEnergy {
                tap_source: true, ..
            } => "tap-pay-energy",
            Self::PayEnergy { .. } => "pay-energy",
            Self::Untap => "untap",
            Self::ReturnSelfToHand { tap_source: true } => "tap-return-self",
            Self::ReturnSelfToHand { tap_source: false } => "return-self",
        }
    }
}
/// Activated keyword with a chosen group of creatures as its entire cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAbility {
    Crew,
    Saddle,
    Station,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatedAbility {
    pub id: ActivatedAbilityId,
    pub mana_cost: ManaCost,
    pub source_cost: SourceCost,
    pub body: ExecutableBody,
}
impl ActivatedAbility {
    /// Construct the native crew or saddle ability (CR 702.122 and CR 702.171).
    pub fn tap_for_power(id: ActivatedAbilityId, kind: PowerAbility, minimum: u64) -> Self {
        Self {
            id,
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::Power {
                kind,
                minimum: if kind == PowerAbility::Station {
                    0
                } else {
                    minimum
                },
            },
            body: ExecutableBody::new(
                crate::program::BodyTarget::None,
                vec![crate::program::Instruction::ResolvePowerAbility(kind)],
            ),
        }
    }

    /// Create an instance of ordinary cycling with a fixed mana cost.
    pub fn cycling(id: ActivatedAbilityId, mana_cost: ManaCost) -> Self {
        Self {
            id,
            mana_cost,
            source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
            body: ExecutableBody::new(
                crate::program::BodyTarget::None,
                vec![crate::program::Instruction::DrawController {
                    amount: crate::program::ValueExpr::Literal(1.into()),
                }],
            ),
        }
    }
    /// Typecycling with the same activation cost and a qualified library search.
    pub fn typecycling(
        id: ActivatedAbilityId,
        mana_cost: ManaCost,
        filter: crate::program::LibrarySearch,
    ) -> Self {
        Self {
            id,
            mana_cost,
            source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling),
            body: ExecutableBody::new(
                crate::program::BodyTarget::None,
                vec![crate::program::Instruction::SearchLibrary {
                    filter: Box::new(filter),
                }],
            ),
        }
    }
    /// Reveal this card in hand and bring it into combat when the ability resolves.
    pub fn ninjutsu(id: ActivatedAbilityId, mana_cost: ManaCost) -> Self {
        Self {
            id,
            mana_cost,
            source_cost: SourceCost::Ninjutsu {
                from_command: false,
            },
            body: ExecutableBody::new(
                crate::program::BodyTarget::None,
                vec![crate::program::Instruction::Ninjutsu],
            ),
        }
    }
    /// Return this card from its owner's graveyard with the effects of unearth.
    pub fn unearth(id: ActivatedAbilityId, mana_cost: ManaCost) -> Self {
        Self {
            id,
            mana_cost,
            source_cost: SourceCost::GraveyardSelf { unearth: true },
            body: ExecutableBody::new(
                crate::program::BodyTarget::None,
                vec![crate::program::Instruction::Unearth],
            ),
        }
    }
    /// Put the printed number of +1/+1 counters on a target creature.
    pub fn reinforce(
        id: ActivatedAbilityId,
        mana_cost: ManaCost,
        amount: spellwake_contracts::RulesInteger,
    ) -> Self {
        Self {
            id,
            mana_cost,
            source_cost: SourceCost::HandDiscardSelf(HandDiscardKeyword::Reinforce),
            body: ExecutableBody::new(
                crate::program::BodyTarget::SingleCreature,
                vec![crate::program::Instruction::PutPowerToughnessCounters {
                    counters: crate::characteristics::PowerToughnessCounters {
                        plus_one: amount,
                        minus_one: 0.into(),
                    },
                }],
            ),
        }
    }
    /// Transmute searches for a card with the same mana value as its source.
    pub fn transmute(
        id: ActivatedAbilityId,
        mana_cost: ManaCost,
        source_mana_value: spellwake_contracts::RulesInteger,
    ) -> Self {
        let mut ability = Self::typecycling(
            id,
            mana_cost,
            crate::program::LibrarySearch::to_hand(crate::program::SearchCardMatcher::Any)
                .with_mana_value(crate::program::SearchManaValue::Exact(source_mana_value)),
        );
        ability.source_cost = SourceCost::HandDiscardSelf(HandDiscardKeyword::Transmute);
        ability
    }
    pub(crate) fn valid_cycling_body(&self) -> bool {
        use crate::program::{BodyTarget, Instruction};
        if self.body.target != BodyTarget::None {
            return false;
        }
        match self.body.instructions.as_slice() {
            [Instruction::SearchLibrary { filter }] => filter.is_typecycling_compatible(),
            [
                Instruction::DrawController {
                    amount: crate::program::ValueExpr::Literal(amount),
                },
            ] => amount == &1.into(),
            _ => false,
        }
    }
}
/// Source zone for an exile cost atom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostExileZone {
    Battlefield,
    Graveyard,
    Hand,
}

/// Additional non-mana cost paid atomically alongside an activated ability's `SourceCost`
/// or as a mandatory additional cost to cast a spell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtraActivationCost {
    TapSource,
    UntapSource,
    SacrificeSource,
    ExileSource {
        from_graveyard: bool,
        from_hand: bool,
    },
    ReturnSourceToHand,
    DiscardSource,
    ExertSource,
    PayLife(LifeCostAmount),
    PayHalfLife,
    PayEnergy(u16),
    PayEnergyExpr(crate::program::ValueExpr),
    Mill(std::num::NonZeroU8),
    SourceCounter {
        kind: crate::effects::CounterKind,
        delta: CounterCostDelta,
    },
    RemovePermanentCounters {
        kind: Option<crate::effects::CounterKind>,
        count: Option<std::num::NonZeroU8>,
        all: bool,
        filter: crate::program::ObjectFilter,
    },
    PutPermanentCounters {
        kind: crate::effects::CounterKind,
        count: std::num::NonZeroU8,
        filter: crate::program::ObjectFilter,
    },
    SacrificePermanent {
        filter: crate::program::ObjectFilter,
        count: Option<std::num::NonZeroU8>,
    },
    TapPermanent {
        filter: crate::program::ObjectFilter,
        count: Option<std::num::NonZeroU8>,
        exclude_source: bool,
    },
    UntapPermanent {
        filter: crate::program::ObjectFilter,
        count: std::num::NonZeroU8,
        exclude_source: bool,
    },
    BouncePermanent {
        filter: crate::program::ObjectFilter,
        count: std::num::NonZeroU8,
        exclude_source: bool,
    },
    DiscardCards {
        filter: crate::program::ObjectFilter,
        count: Option<std::num::NonZeroU8>,
        whole_hand: bool,
        random: bool,
    },
    RevealHand {
        filter: crate::program::ObjectFilter,
        count: std::num::NonZeroU8,
        allow_controlled: bool,
    },
    ExileObjects {
        zone: CostExileZone,
        filter: crate::program::ObjectFilter,
        count: Option<std::num::NonZeroU8>,
        exclude_source: bool,
    },
    CollectEvidence(u16),
    Forage,
    Waterbend(ManaCost),
    Blight(std::num::NonZeroU8),
    ProcessExiledCard,
    Unattach {
        self_source: bool,
    },
    PayMana(ManaCost),
    Either(Box<ExtraActivationCost>, Box<ExtraActivationCost>),
}

impl ExtraActivationCost {
    /// Validate vocabulary references and structural invariants for this cost atom.
    pub(crate) fn validate(
        &self,
        vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    ) -> Result<(), crate::program::DefinitionError> {
        match self {
            Self::SourceCounter { kind, .. } | Self::PutPermanentCounters { kind, .. } => {
                if !kind.is_permanent() {
                    return Err(crate::program::DefinitionError::InvalidInstruction);
                }
                if let Self::PutPermanentCounters { filter, .. } = self {
                    filter.validate_vocabulary(vocabulary)?;
                }
                Ok(())
            }
            Self::RemovePermanentCounters { kind, filter, .. } => {
                if let Some(k) = kind
                    && !k.is_permanent()
                {
                    return Err(crate::program::DefinitionError::InvalidInstruction);
                }
                filter.validate_vocabulary(vocabulary)
            }
            Self::DiscardCards { filter, .. } => filter.validate_vocabulary(vocabulary),
            Self::SacrificePermanent { filter, .. }
            | Self::TapPermanent { filter, .. }
            | Self::UntapPermanent { filter, .. }
            | Self::BouncePermanent { filter, .. }
            | Self::RevealHand { filter, .. }
            | Self::ExileObjects { filter, .. } => filter.validate_vocabulary(vocabulary),
            Self::PayEnergyExpr(expr) => expr.validate_world_condition(vocabulary),
            Self::Waterbend(cost) | Self::PayMana(cost) => {
                if cost.valid() {
                    Ok(())
                } else {
                    Err(crate::program::DefinitionError::CastingCost)
                }
            }
            Self::Either(a, b) => {
                a.validate(vocabulary)?;
                b.validate(vocabulary)
            }
            Self::TapSource
            | Self::UntapSource
            | Self::SacrificeSource
            | Self::ExileSource { .. }
            | Self::ReturnSourceToHand
            | Self::DiscardSource
            | Self::ExertSource
            | Self::PayLife(_)
            | Self::PayHalfLife
            | Self::PayEnergy(_)
            | Self::Mill(_)
            | Self::CollectEvidence(_)
            | Self::Forage
            | Self::Blight(_)
            | Self::ProcessExiledCard
            | Self::Unattach { .. } => Ok(()),
        }
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type(
        &mut self,
        from: spellwake_contracts::creature_types::CreatureTypeId,
        to: spellwake_contracts::creature_types::CreatureTypeId,
    ) {
        match self {
            Self::RemovePermanentCounters { filter, .. }
            | Self::PutPermanentCounters { filter, .. }
            | Self::SacrificePermanent { filter, .. }
            | Self::TapPermanent { filter, .. }
            | Self::UntapPermanent { filter, .. }
            | Self::BouncePermanent { filter, .. }
            | Self::DiscardCards { filter, .. }
            | Self::RevealHand { filter, .. }
            | Self::ExileObjects { filter, .. } => filter.rewrite_creature_type(from, to),
            Self::PayEnergyExpr(expr) => expr.rewrite_creature_type(from, to),
            Self::Either(a, b) => {
                a.rewrite_creature_type(from, to);
                b.rewrite_creature_type(from, to);
            }
            _ => {}
        }
    }

    /// Whether this cost atom taps the ability source itself.
    #[must_use]
    pub fn taps_source(&self) -> bool {
        matches!(self, Self::TapSource)
    }

    /// Whether this cost atom untaps the ability source itself.
    #[must_use]
    pub fn untaps_source(&self) -> bool {
        matches!(self, Self::UntapSource)
    }

    /// Whether this cost atom requires activating from the graveyard.
    #[must_use]
    pub fn is_from_graveyard(&self) -> bool {
        matches!(
            self,
            Self::ExileSource {
                from_graveyard: true,
                ..
            }
        )
    }

    /// Whether this cost atom requires activating from hand.
    #[must_use]
    pub fn is_from_hand(&self) -> bool {
        matches!(
            self,
            Self::DiscardSource
                | Self::ExileSource {
                    from_hand: true,
                    ..
                }
        )
    }

    /// Whether this cost atom requires caller-supplied selected objects.
    #[must_use]
    pub fn uses_selected_objects(&self) -> bool {
        match self {
            Self::RemovePermanentCounters { .. }
            | Self::PutPermanentCounters { .. }
            | Self::SacrificePermanent { .. }
            | Self::TapPermanent { .. }
            | Self::UntapPermanent { .. }
            | Self::BouncePermanent { .. }
            | Self::DiscardCards {
                whole_hand: false,
                random: false,
                ..
            }
            | Self::RevealHand { .. }
            | Self::ExileObjects { .. }
            | Self::CollectEvidence(_)
            | Self::Forage
            | Self::Waterbend(_)
            | Self::Blight(_)
            | Self::ProcessExiledCard
            | Self::Unattach { self_source: false } => true,
            Self::Either(a, b) => a.uses_selected_objects() || b.uses_selected_objects(),
            Self::TapSource
            | Self::UntapSource
            | Self::SacrificeSource
            | Self::ExileSource { .. }
            | Self::ReturnSourceToHand
            | Self::DiscardSource
            | Self::ExertSource
            | Self::PayLife(_)
            | Self::PayHalfLife
            | Self::PayEnergy(_)
            | Self::PayEnergyExpr(_)
            | Self::PayMana(_)
            | Self::Mill(_)
            | Self::SourceCounter { .. }
            | Self::DiscardCards {
                whole_hand: true, ..
            }
            | Self::DiscardCards { random: true, .. }
            | Self::Unattach { self_source: true } => false,
        }
    }

    /// Whether this cost atom introduces a variable X selection.
    #[must_use]
    pub fn uses_chosen_x(&self) -> bool {
        match self {
            Self::RemovePermanentCounters {
                count: None,
                all: false,
                ..
            }
            | Self::SacrificePermanent { count: None, .. }
            | Self::TapPermanent { count: None, .. }
            | Self::ExileObjects { count: None, .. }
            | Self::DiscardCards {
                count: None,
                whole_hand: false,
                ..
            } => true,
            Self::PayEnergyExpr(expr) => expr.is_chosen_x(),
            Self::Either(a, b) => a.uses_chosen_x() || b.uses_chosen_x(),
            _ => false,
        }
    }
}

/// Arguments shared by targeted battlefield abilities and cycling in hand.
pub(crate) struct ActivationRequest {
    pub granting_effect: Option<crate::characteristics::EffectInstanceKey>,
    pub chosen_x: Option<spellwake_contracts::RulesInteger>,
    pub source: ObjectId,
    pub ability: ActivatedAbilityId,
    pub origin: ActivationOrigin,
}
/// Typed choices used when announcing a represented activated ability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationOrigin {
    PermanentTargets(Vec<crate::targets::TargetRef>),
    SelectedCost {
        target: Option<crate::targets::TargetRef>,
        objects: Vec<ObjectId>,
    },
    Contributors(Vec<ObjectId>),
    Ninjutsu(ObjectId),
    Untargeted,
    Targeted(crate::targets::TargetRef),
    Modal(Vec<crate::mana::CastMode>),
    Cycling,
    Transmute,
}
/// Current privileged activation evidence. Source is the actual pre-cost snapshot,
/// not complete LKI or a physical card placed on the stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbilityActivationRecord {
    pub granting_effect: Option<crate::characteristics::EffectInstanceKey>,
    pub entry: StackEntryId,
    pub ability: ActivatedAbilityId,
    pub controller: SeatId,
    pub source: GameObject,
    pub source_cost: SourceCost,
    pub resulting_source: ObjectId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbilityActivationResult {
    /// Whether a cost or linked return still owns continuation; the other fields are provisional while true.
    pub pending: bool,
    pub activation: AbilityActivationRecord,
    pub payment: ManaPaymentRecord,
}
pub type AbilityActivationTransition =
    crate::occurrence::RulesTransition<EffectWindow, AbilityActivationResult>;
