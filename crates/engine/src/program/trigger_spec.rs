//! Composable trigger specification unifying ordinary triggers, keyword triggers,
//! supplemental paper formats (Planechase, Archenemy), and opt-in Tabletop Honor-System triggers.
use super::{
    Comparison, CounterKeyword, DefinitionError, ObjectFilter, PlayerRelation,
    PreviousTurnCastCondition, SpellFilter, UpkeepKeyword,
};
use crate::{effects::CounterKind, life::LifeChangeKind, object_identity::ObjectZone, turn::Step};
use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};
use std::num::NonZeroU16;

/// Zone in which a triggered ability functions (CR 113.6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TriggerZone {
    #[default]
    Battlefield,
    Graveyard,
    Stack,
    CommandZone,
    /// Functions from any zone (for example, self-cycled triggers after discard).
    Any,
}

impl TriggerZone {
    /// Whether this trigger functions while its source is in `zone`.
    #[must_use]
    pub const fn matches(self, zone: ObjectZone) -> bool {
        match self {
            Self::Battlefield => matches!(zone, ObjectZone::Battlefield),
            Self::Graveyard => matches!(zone, ObjectZone::Graveyard(_)),
            Self::Stack => matches!(zone, ObjectZone::Stack),
            Self::CommandZone => matches!(zone, ObjectZone::Command),
            Self::Any => true,
        }
    }
}

/// Origin zone required for a zone-change trigger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TriggerFromZone {
    #[default]
    Any,
    Graveyard,
    ControllerGraveyard,
    Hand,
    Exile,
    Library,
    Battlefield,
}

/// Cast-origin requirement on an entering permanent trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastEntryKind {
    Cast { from_hand: bool },
    Kicked,
    Evoked,
}

/// Matching mode for a battlefield-to-graveyard trigger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiesTriggerMode {
    /// Standard creature death ("dies"); non-creature self allowed for `or_self()` nontoken-creature patterns.
    #[default]
    Creature,
    /// Any permanent put into a graveyard from the battlefield regardless of creature type.
    Permanent,
    /// Creature put into the ability controller's graveyard from the battlefield.
    ToControllerGraveyard,
    /// Creature dealt damage by the source earlier this turn dies.
    DamagedBySource,
    /// Death trigger granted by paying blitz.
    Blitz,
    /// Permanent put into a graveyard or command zone from the battlefield.
    OrCommandZone,
}

/// Player scope for step and spell-cast triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerPlayerScope {
    Relation(PlayerRelation),
    EnchantedPlayer,
}

/// Attack-declaration matching mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackTriggerMode {
    /// Trigger once per matching declared attacker.
    Individual { capture_defender: bool },
    /// First time this incarnation attacks this turn.
    FirstThisTurn { highest_life: bool },
    /// Source attacks alongside at least `u8` other attackers.
    WithAtLeastOthers(u8),
    /// Source attacks the player with the most life (or tied).
    HighestLifeDefender,
    /// Source attacks alongside a creature you control with greater power (Training).
    WithStrongerCreature,
    /// Exactly one creature declared as an attacker by controller (Exalted / attacks alone).
    Alone,
    /// One or more controlled creatures of `CreatureTypeId` attack (captures group).
    GroupSubtype(CreatureTypeId),
    /// Controller attacks with creatures of total power at least `u16`.
    GroupTotalPowerAtLeast(u16),
    /// At least `minimum` creatures controlled by an opponent attack controller.
    OpponentsAtController { minimum: u8 },
    /// A creature attacks controller or a planeswalker they control.
    AtControllerOrPlaneswalker,
    /// One or more creatures attack an opponent or their planeswalker (cohorts by attacked target).
    AtOpponentOrPlaneswalker,
    /// Controller attacks a player with one or more equipped creatures (cohorts by defending player).
    EquippedAtPlayer,
    /// A matching creature attacks the ability controller directly (not a planeswalker).
    AtController,
    /// A matching creature attacks an opponent of the ability controller directly.
    AtOpponent,
    /// A matching creature attacks any player directly.
    AtPlayer,
    /// A matching creature attacks a planeswalker.
    AtPlaneswalker,
    /// A matching creature attacks the enchanted player.
    AtEnchantedPlayer,
    /// At least `u8` matching creatures attack together in a single attack declaration.
    GroupAtLeast(u8),
}

/// Block-declaration matching mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockTriggerKind {
    BecomesBlocked {
        capture_defender: bool,
    },
    Unblocked,
    Blocks,
    BlocksOrBecomesBlocked,
    /// Once per blocker blocking `subject`.
    BlockedByEach {
        without_flanking_only: bool,
    },
    /// Once per creature blocking or blocked by `subject`.
    BlocksOrBlockedByEach {
        required_subtype: Option<CreatureTypeId>,
    },
    /// Once per blocker matching `ObjectFilter` blocking `subject`.
    BlockedByMatching(Box<ObjectFilter>),
    /// Once per attacker matching `ObjectFilter` blocked by `subject`.
    BlocksMatching(Box<ObjectFilter>),
}

/// Damage recipient predicate on a `DealsDamage` trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageTriggerRecipient {
    /// Any damage recipient (player, creature, planeswalker, or battle).
    Any,
    AnyPlayer,
    Opponent,
    Controller,
    EnchantedPlayer,
    PlayerOrPlaneswalker,
    Creature,
}

/// Counter-change trigger kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterTriggerKind {
    /// One or more +1/+1 counters placed on `subject`.
    PlusOnePlaced { by_controller_only: bool },
    /// One or more counters of `kind` (or any counter if `None`) placed or removed on `subject`.
    CounterChanged {
        kind: Option<CounterKind>,
        added: bool,
        by_controller_only: bool,
    },
    /// Last defense counter removed from source battle.
    LastDefenseRemoved,
    /// Lore counter count crossed chapter `u8` on source Saga.
    SagaChapter(u8),
}

/// State-triggered ability threshold (CR 603.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateThresholdKind {
    SourceCounters {
        kind: CounterKind,
        comparison: Comparison,
        amount: u8,
    },
    ControlledSubtype {
        subtype: CreatureTypeId,
        minimum: u16,
    },
}

/// Player-scoped action trigger.
// PlayerTriggerAction is intentionally Copy and zero-allocation; Sacrificed holds ObjectFilter inline.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerTriggerAction {
    Sacrificed(ObjectFilter),
    Discarded,
    Cycled {
        self_only: bool,
    },
    TokenCreated,
    DrewCard {
        nth_each_turn: Option<NonZeroU16>,
    },
    Scried,
    Surveilled,
    ArtifactTappedForMana,
    /// Any battlefield permanent matching `ObjectFilter` was tapped for mana.
    PermanentTappedForMana(ObjectFilter),
    SagaFinalChapterResolved,
    /// Player targeted an opponent, anything an opponent controls, or a card in an opponent's graveyard (CR 700.13).
    CommittedCrime,
}

/// Status or physical-transition trigger on a permanent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTriggerKind {
    BecomesTapped,
    BecomesUntapped,
    TurnsFaceUp,
    Transforms,
    Phased { out: bool },
    Mutated,
}

/// Keyword-owned trigger event with specialized rules semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKeywordEvent {
    Upkeep(UpkeepKeyword),
    Counter(CounterKeyword),
    Soulbond { entrant_only: bool },
    Evolve,
    Graft,
    Increment,
}

/// Planechase planar trigger event (207 real-paper Planes and Phenomena).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanarTriggerEvent {
    PlaneswalkToSelf,
    PlaneswalkAwayFromSelf,
    ChaosEnsues,
    EncounterPhenomenon,
}

/// Intervening-if or trigger-time condition attached to a [`TriggerSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerCondition {
    Morbid,
    DuringStep(Step),
    SourceHasLoyalty,
    SourceCounterAtLeast {
        kind: CounterKind,
        minimum: u8,
    },
    NoOtherControlledCreatures,
    DefendingPlayerHasMoreLands,
    UnrenownedSource,
    SourceSaddled,
    SourceDidNotAttackThisTurn,
    ControllerHasCityBlessing,
    Monarch {
        opponent: bool,
    },
    ControlsOwnCommander,
    OpponentHasMoreLands,
    ControlsGreatestArtifact,
    NoLoyaltyActivationThisTurn,
    OpponentLostLifeAtLeast(u8),
    PreviousTurnCasts(PreviousTurnCastCondition),
    ControlledCreatureCount(u16),
    ControlledCreaturePowerAtLeast(u16),
    TotalControlledPowerAtLeast(u16),
    HandSizeBelow(u8),
    ControlsMeldPair,
    ControllersTurn,
    OpponentsTurn,
    Delirium,
    Metalcraft,
    Threshold,
    HandSizeAtMost(u8),
    HandSizeAtLeast(u8),
    ControllerLifeAtLeast(u16),
    ControllerLifeAtMost(u16),
    ControlsCardType {
        kind: super::CardType,
        minimum: u8,
    },
    ControlsSubtype {
        kind: CreatureTypeId,
        minimum: u8,
    },
    GraveyardCardTypeCount {
        kind: Option<super::CardType>,
        minimum: u8,
    },
    SourceIsTapped,
    SourceIsUntapped,
    SourceIsAttacking,
    ControllerGainedLifeThisTurn,
    ControllerLostLifeThisTurn,
    ControllerAttackedThisTurn,
    ControllerCastSpellsThisTurn(u16),
    ControllerDiscardedThisTurn,
    ControllerSacrificedThisTurn,
    PermanentLeftBattlefieldThisTurn,
    CreatureEnteredUnderControlThisTurn,
    DescendedThisTurn,
    CommittedCrimeThisTurn,
    ControllerDrewCardsThisTurn(u16),
    OpponentHasMoreLife,
    OpponentControlsMoreCreatures,
    OpponentHasMoreCardsInHand,
    ControlsMatching {
        filter: ObjectFilter,
        minimum: u8,
    },
    OpponentControlsMatching {
        filter: ObjectFilter,
        minimum: u8,
    },
    /// The trigger source permanent matches `filter` on the battlefield.
    SourceMatches(Box<ObjectFilter>),
    /// Controller's graveyard contains at least `minimum` cards matching `filter`.
    GraveyardMatching {
        filter: Box<ObjectFilter>,
        minimum: u8,
    },
    Not(Box<TriggerCondition>),
    And(Box<TriggerCondition>, Box<TriggerCondition>),
    Or(Box<TriggerCondition>, Box<TriggerCondition>),
}

/// Core orthogonal trigger event replacing the flat 135-variant `OrdinaryCondition` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerEvent {
    Enters {
        subject: ObjectFilter,
        from_zone: TriggerFromZone,
        cast_origin: Option<CastEntryKind>,
        attached_to_creature: bool,
    },
    LeavesBattlefield {
        subject: ObjectFilter,
    },
    Dies {
        subject: ObjectFilter,
        mode: DiesTriggerMode,
    },
    PutIntoGraveyard {
        subject: ObjectFilter,
        player: PlayerRelation,
        from_library: bool,
        own_turn_only: bool,
    },
    /// An object matching `subject` is put into exile (optionally restricted to `from_zone`).
    Exiled {
        subject: ObjectFilter,
        from_zone: TriggerFromZone,
    },
    /// A permanent matching `subject` is returned to a player's hand from the battlefield.
    ReturnedToHand {
        subject: ObjectFilter,
    },
    /// A card matching `subject` leaves a player's graveyard.
    LeavesGraveyard {
        subject: ObjectFilter,
        player: PlayerRelation,
    },
    StepBegan {
        step: Step,
        player: TriggerPlayerScope,
    },
    SpellCast {
        caster: TriggerPlayerScope,
        filter: SpellFilter,
        self_cast: bool,
        include_copy: bool,
        min_mana_value: Option<u16>,
    },
    Attacks {
        subject: ObjectFilter,
        mode: AttackTriggerMode,
    },
    BlocksOrBlocked {
        subject: ObjectFilter,
        kind: BlockTriggerKind,
    },
    DealsDamage {
        source: ObjectFilter,
        recipient: DamageTriggerRecipient,
        combat_only: bool,
    },
    DealtDamage {
        subject: ObjectFilter,
        linked_player: bool,
    },
    Targeted {
        subject: ObjectFilter,
        by_opponent: bool,
        spell_only: bool,
        instant_or_sorcery_only: bool,
    },
    CountersChanged {
        subject: ObjectFilter,
        kind: CounterTriggerKind,
    },
    StateThreshold(StateThresholdKind),
    PlayerAction {
        player: PlayerRelation,
        action: PlayerTriggerAction,
    },
    StatusChanged {
        subject: ObjectFilter,
        kind: StatusTriggerKind,
    },
    LifeChanged {
        kind: LifeChangeKind,
        player: PlayerRelation,
    },
    PlayerLosesGame(PlayerRelation),
    Keyword(TriggerKeywordEvent),
    /// Disjunction of events ("When ~ enters or attacks", compound Planechase headers, etc.).
    AnyOf(Vec<TriggerEvent>),
    /// Planechase planar events.
    Planar(PlanarTriggerEvent),
    /// Archenemy scheme set in motion.
    SchemeSetInMotion,
    /// Opt-in Tabletop Honor-System trigger for physical/social/voice Un-cards.
    ManualTabletopClaim {
        prompt: Box<str>,
        any_player_can_trigger: bool,
    },
}

impl TriggerEvent {
    /// Whether this event uses look-back-in-time (CR 603.10) source discovery.
    #[must_use]
    pub fn is_lookback(&self) -> bool {
        match self {
            Self::LeavesBattlefield { .. }
            | Self::Dies { .. }
            | Self::ReturnedToHand { .. }
            | Self::Exiled {
                from_zone: TriggerFromZone::Battlefield,
                ..
            }
            | Self::PlayerLosesGame(_) => true,
            Self::StatusChanged {
                kind: StatusTriggerKind::Phased { out: true },
                ..
            } => true,
            Self::CountersChanged {
                kind:
                    CounterTriggerKind::PlusOnePlaced {
                        by_controller_only: true,
                    },
                ..
            } => true,
            Self::PlayerAction {
                action:
                    PlayerTriggerAction::Sacrificed(_) | PlayerTriggerAction::SagaFinalChapterResolved,
                ..
            } => true,
            Self::Keyword(TriggerKeywordEvent::Counter(k)) => k.death_return(),
            Self::AnyOf(events) => events.iter().any(Self::is_lookback),
            _ => false,
        }
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type(&mut self, from: CreatureTypeId, to: CreatureTypeId) {
        let word = |kind: &mut CreatureTypeId| {
            if *kind == from {
                *kind = to;
            }
        };
        match self {
            Self::Enters { subject, .. }
            | Self::LeavesBattlefield { subject }
            | Self::Dies { subject, .. }
            | Self::PutIntoGraveyard { subject, .. }
            | Self::Exiled { subject, .. }
            | Self::ReturnedToHand { subject }
            | Self::LeavesGraveyard { subject, .. }
            | Self::DealtDamage { subject, .. }
            | Self::Targeted { subject, .. }
            | Self::CountersChanged { subject, .. }
            | Self::StatusChanged { subject, .. } => subject.rewrite_creature_type(from, to),
            Self::DealsDamage { source, .. } => source.rewrite_creature_type(from, to),
            Self::SpellCast { filter, .. } => filter.rewrite_creature_type(from, to),
            Self::Attacks { subject, mode } => {
                subject.rewrite_creature_type(from, to);
                if let AttackTriggerMode::GroupSubtype(kind) = mode {
                    word(kind);
                }
            }
            Self::BlocksOrBlocked { subject, kind } => {
                subject.rewrite_creature_type(from, to);
                match kind {
                    BlockTriggerKind::BlocksOrBlockedByEach {
                        required_subtype: Some(subtype),
                    } => word(subtype),
                    BlockTriggerKind::BlockedByMatching(other)
                    | BlockTriggerKind::BlocksMatching(other) => {
                        other.rewrite_creature_type(from, to);
                    }
                    _ => {}
                }
            }
            Self::StateThreshold(StateThresholdKind::ControlledSubtype { subtype, .. }) => {
                word(subtype);
            }
            Self::PlayerAction {
                action:
                    PlayerTriggerAction::Sacrificed(filter)
                    | PlayerTriggerAction::PermanentTappedForMana(filter),
                ..
            } => filter.rewrite_creature_type(from, to),
            Self::AnyOf(events) => {
                for event in events {
                    event.rewrite_creature_type(from, to);
                }
            }
            Self::StepBegan { .. }
            | Self::StateThreshold(StateThresholdKind::SourceCounters { .. })
            | Self::PlayerAction { .. }
            | Self::LifeChanged { .. }
            | Self::PlayerLosesGame(_)
            | Self::Keyword(_)
            | Self::Planar(_)
            | Self::SchemeSetInMotion
            | Self::ManualTabletopClaim { .. } => {}
        }
    }

    /// Validate that every referenced creature subtype exists in `vocabulary`.
    pub(crate) fn validate_vocabulary(
        &self,
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
            Self::Enters { subject, .. }
            | Self::LeavesBattlefield { subject }
            | Self::Dies { subject, .. }
            | Self::PutIntoGraveyard { subject, .. }
            | Self::Exiled { subject, .. }
            | Self::ReturnedToHand { subject }
            | Self::LeavesGraveyard { subject, .. }
            | Self::DealtDamage { subject, .. }
            | Self::Targeted { subject, .. }
            | Self::CountersChanged { subject, .. }
            | Self::StatusChanged { subject, .. } => subject.validate_vocabulary(vocabulary),
            Self::DealsDamage { source, .. } => source.validate_vocabulary(vocabulary),
            Self::SpellCast { filter, .. } => filter.validate_vocabulary(vocabulary),
            Self::Attacks { subject, mode } => {
                subject.validate_vocabulary(vocabulary)?;
                if let AttackTriggerMode::GroupSubtype(kind) = *mode {
                    check(kind)?;
                }
                Ok(())
            }
            Self::BlocksOrBlocked { subject, kind } => {
                subject.validate_vocabulary(vocabulary)?;
                match kind {
                    BlockTriggerKind::BlocksOrBlockedByEach {
                        required_subtype: Some(subtype),
                    } => check(*subtype)?,
                    BlockTriggerKind::BlockedByMatching(other)
                    | BlockTriggerKind::BlocksMatching(other) => {
                        other.validate_vocabulary(vocabulary)?;
                    }
                    _ => {}
                }
                Ok(())
            }
            Self::StateThreshold(StateThresholdKind::ControlledSubtype { subtype, .. }) => {
                check(*subtype)
            }
            Self::PlayerAction {
                action:
                    PlayerTriggerAction::Sacrificed(filter)
                    | PlayerTriggerAction::PermanentTappedForMana(filter),
                ..
            } => filter.validate_vocabulary(vocabulary),
            Self::AnyOf(events) => {
                if events.is_empty() {
                    return Err(DefinitionError::InvalidInstruction);
                }
                for event in events {
                    event.validate_vocabulary(vocabulary)?;
                }
                Ok(())
            }
            Self::StepBegan { .. }
            | Self::StateThreshold(StateThresholdKind::SourceCounters { .. })
            | Self::PlayerAction { .. }
            | Self::LifeChanged { .. }
            | Self::PlayerLosesGame(_)
            | Self::Keyword(_)
            | Self::Planar(_)
            | Self::SchemeSetInMotion
            | Self::ManualTabletopClaim { .. } => Ok(()),
        }
    }
}

/// Complete composable trigger header: event, active zone, and optional intervening-if condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerSpec {
    pub event: TriggerEvent,
    pub zone: TriggerZone,
    pub intervening_if: Option<TriggerCondition>,
}

impl TriggerSpec {
    /// Battlefield trigger with no intervening-if condition.
    #[must_use]
    pub const fn new(event: TriggerEvent) -> Self {
        Self {
            event,
            zone: TriggerZone::Battlefield,
            intervening_if: None,
        }
    }

    /// Override the zone in which this trigger functions.
    #[must_use]
    pub const fn in_zone(mut self, zone: TriggerZone) -> Self {
        self.zone = zone;
        self
    }

    /// Attach an intervening-if or trigger-time condition.
    #[must_use]
    pub fn only_if(mut self, condition: TriggerCondition) -> Self {
        self.intervening_if = Some(condition);
        self
    }

    /// Enters-the-battlefield trigger for permanents matching `subject`.
    #[must_use]
    pub const fn enters(subject: ObjectFilter) -> Self {
        Self::new(TriggerEvent::Enters {
            subject,
            from_zone: TriggerFromZone::Any,
            cast_origin: None,
            attached_to_creature: false,
        })
    }

    /// Trigger when this permanent enters the battlefield.
    #[must_use]
    pub const fn self_enters() -> Self {
        Self::enters(ObjectFilter::self_object())
    }

    /// Leaves-the-battlefield trigger for permanents matching `subject`.
    #[must_use]
    pub const fn leaves_battlefield(subject: ObjectFilter) -> Self {
        Self::new(TriggerEvent::LeavesBattlefield { subject })
    }

    /// Creature death trigger ("dies") for objects matching `subject`.
    #[must_use]
    pub const fn dies(subject: ObjectFilter) -> Self {
        Self::new(TriggerEvent::Dies {
            subject,
            mode: DiesTriggerMode::Creature,
        })
    }

    /// Trigger when this creature dies.
    #[must_use]
    pub const fn self_dies() -> Self {
        Self::dies(ObjectFilter::self_object())
    }

    /// Trigger when a permanent matching `subject` becomes tapped.
    #[must_use]
    pub const fn becomes_tapped(subject: ObjectFilter) -> Self {
        Self::new(TriggerEvent::StatusChanged {
            subject,
            kind: StatusTriggerKind::BecomesTapped,
        })
    }

    /// Trigger at the beginning of `step` for `player`.
    #[must_use]
    pub const fn step_began(step: Step, player: PlayerRelation) -> Self {
        Self::new(TriggerEvent::StepBegan {
            step,
            player: TriggerPlayerScope::Relation(player),
        })
    }

    /// Trigger when `player` casts a spell matching `filter`.
    #[must_use]
    pub const fn spell_cast(player: PlayerRelation, filter: SpellFilter) -> Self {
        Self::new(TriggerEvent::SpellCast {
            caster: TriggerPlayerScope::Relation(player),
            filter,
            self_cast: false,
            include_copy: false,
            min_mana_value: None,
        })
    }

    /// Trigger on the stack when this spell is cast.
    #[must_use]
    pub const fn self_cast() -> Self {
        Self::new(TriggerEvent::SpellCast {
            caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
            filter: SpellFilter::any(),
            self_cast: true,
            include_copy: false,
            min_mana_value: None,
        })
        .in_zone(TriggerZone::Stack)
    }

    /// Trigger when this permanent attacks.
    #[must_use]
    pub const fn self_attacks() -> Self {
        Self::new(TriggerEvent::Attacks {
            subject: ObjectFilter::self_object(),
            mode: AttackTriggerMode::Individual {
                capture_defender: false,
            },
        })
    }

    /// Trigger when this permanent deals combat damage to a player.
    #[must_use]
    pub const fn self_deals_combat_damage_to_player() -> Self {
        Self::new(TriggerEvent::DealsDamage {
            source: ObjectFilter::self_object(),
            recipient: DamageTriggerRecipient::AnyPlayer,
            combat_only: true,
        })
    }

    /// Trigger when `player` gains or loses life.
    #[must_use]
    pub const fn life_changed(kind: LifeChangeKind, player: PlayerRelation) -> Self {
        Self::new(TriggerEvent::LifeChanged { kind, player })
    }

    /// Trigger on a player action (draw, discard, sacrifice, cycle, scry, surveil, token created).
    #[must_use]
    pub const fn player_action(player: PlayerRelation, action: PlayerTriggerAction) -> Self {
        Self::new(TriggerEvent::PlayerAction { player, action })
    }

    /// Compound disjunction trigger ("When ~ enters or attacks", etc.).
    #[must_use]
    pub fn any_of(events: Vec<TriggerEvent>) -> Self {
        Self::new(TriggerEvent::AnyOf(events))
    }

    /// Keyword-owned trigger event.
    #[must_use]
    pub const fn keyword(event: TriggerKeywordEvent) -> Self {
        Self::new(TriggerEvent::Keyword(event))
    }

    /// Counter-keyword trigger (fading, vanishing, suspend, persist, undying).
    #[must_use]
    pub const fn counter_keyword(keyword: CounterKeyword) -> Self {
        Self::keyword(TriggerKeywordEvent::Counter(keyword))
    }

    /// Upkeep-keyword trigger (echo, cumulative upkeep, other-creatures upkeep).
    #[must_use]
    pub const fn upkeep_keyword(keyword: UpkeepKeyword) -> Self {
        Self::keyword(TriggerKeywordEvent::Upkeep(keyword))
    }

    /// State-triggered ability threshold (CR 603.8).
    #[must_use]
    pub const fn state_threshold(kind: StateThresholdKind) -> Self {
        Self::new(TriggerEvent::StateThreshold(kind))
    }

    /// Status or physical-transition trigger on a permanent.
    #[must_use]
    pub const fn status_changed(subject: ObjectFilter, kind: StatusTriggerKind) -> Self {
        Self::new(TriggerEvent::StatusChanged { subject, kind })
    }

    /// Attack-declaration trigger.
    #[must_use]
    pub const fn attacks(subject: ObjectFilter, mode: AttackTriggerMode) -> Self {
        Self::new(TriggerEvent::Attacks { subject, mode })
    }

    /// Block-declaration trigger.
    #[must_use]
    pub const fn blocks_or_blocked(subject: ObjectFilter, kind: BlockTriggerKind) -> Self {
        Self::new(TriggerEvent::BlocksOrBlocked { subject, kind })
    }

    /// Damage-dealt trigger.
    #[must_use]
    pub const fn deals_damage(
        source: ObjectFilter,
        recipient: DamageTriggerRecipient,
        combat_only: bool,
    ) -> Self {
        Self::new(TriggerEvent::DealsDamage {
            source,
            recipient,
            combat_only,
        })
    }

    /// Counter-change trigger.
    #[must_use]
    pub const fn counters_changed(subject: ObjectFilter, kind: CounterTriggerKind) -> Self {
        Self::new(TriggerEvent::CountersChanged { subject, kind })
    }

    /// Whether every supported trigger event captures a related player participant.
    #[must_use]
    pub const fn captures_related_player(&self) -> bool {
        true
    }

    /// Whether this trigger uses look-back-in-time (CR 603.10) source discovery.
    #[must_use]
    pub fn is_lookback(&self) -> bool {
        self.event.is_lookback()
    }

    /// Whether this trigger functions while its source is in `zone`.
    #[must_use]
    pub const fn functions_in_zone(&self, zone: ObjectZone) -> bool {
        self.zone.matches(zone)
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type(&mut self, from: CreatureTypeId, to: CreatureTypeId) {
        self.event.rewrite_creature_type(from, to);
    }

    /// Validate that every referenced creature subtype exists in `vocabulary`.
    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.event.validate_vocabulary(vocabulary)
    }
}
