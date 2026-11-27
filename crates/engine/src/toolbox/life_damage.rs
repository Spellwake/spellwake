//! Universal composable `AdjustLife`, `DealDamage`, and `MutatePlayer` primitives.

use crate::{
    day_night::DayNight,
    effects::CounterKind,
    program::{
        CombatUntapGroup, DamageAmountSpec, DamageRecipientsRef, DefinitionError, EffectManaYield,
        ExtremeLifeTieMode, ObjectFilter, PowerDamageSource, ValueExpr,
    },
    toolbox::{
        env::BindingId,
        query::{ObjectQuery, PlayerSelector},
    },
};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;

/// Life total mutation executed by [`AdjustLifeSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifeOp {
    Gain(ValueExpr),
    Lose(ValueExpr),
    Set(ValueExpr),
    Pay(ValueExpr),
    Drain {
        amount: ValueExpr,
        gain_to: PlayerSelector,
        per_loss: bool,
    },
    LoseHalf {
        round_up: bool,
    },
}

impl LifeOp {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Gain(v) | Self::Lose(v) | Self::Set(v) | Self::Pay(v) => v.uses_target(),
            Self::Drain {
                amount, gain_to, ..
            } => amount.uses_target() || gain_to.uses_target(),
            Self::LoseHalf { .. } => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Gain(v) | Self::Lose(v) | Self::Set(v) | Self::Pay(v) => v
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::Drain {
                amount, gain_to, ..
            } => {
                amount
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                gain_to.validate_vocabulary(vocabulary)
            }
            Self::LoseHalf { .. } => Ok(()),
        }
    }
}

/// Specification for `Instruction::AdjustLife`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjustLifeSpec {
    pub players: PlayerSelector,
    pub op: LifeOp,
    pub bind_amount: Option<BindingId>,
}

impl AdjustLifeSpec {
    #[must_use]
    pub const fn new(players: PlayerSelector, op: LifeOp) -> Self {
        Self {
            players,
            op,
            bind_amount: None,
        }
    }

    #[must_use]
    pub const fn gain(players: PlayerSelector, amount: ValueExpr) -> Self {
        Self::new(players, LifeOp::Gain(amount))
    }

    #[must_use]
    pub const fn lose(players: PlayerSelector, amount: ValueExpr) -> Self {
        Self::new(players, LifeOp::Lose(amount))
    }

    #[must_use]
    pub const fn set(players: PlayerSelector, amount: ValueExpr) -> Self {
        Self::new(players, LifeOp::Set(amount))
    }

    #[must_use]
    pub const fn pay(players: PlayerSelector, amount: ValueExpr) -> Self {
        Self::new(players, LifeOp::Pay(amount))
    }

    #[must_use]
    pub const fn drain(
        players: PlayerSelector,
        amount: ValueExpr,
        gain_to: PlayerSelector,
        per_loss: bool,
    ) -> Self {
        Self::new(
            players,
            LifeOp::Drain {
                amount,
                gain_to,
                per_loss,
            },
        )
    }

    #[must_use]
    pub const fn lose_half(players: PlayerSelector, round_up: bool) -> Self {
        Self::new(players, LifeOp::LoseHalf { round_up })
    }

    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.players.uses_target() || self.op.uses_target()
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.players.validate_vocabulary(vocabulary)?;
        self.op.validate_vocabulary(vocabulary)
    }
}

/// Source selector for [`DealDamageSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageSourceQuery {
    EntrySource,
    Objects(ObjectQuery),
}

impl DamageSourceQuery {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::EntrySource => false,
            Self::Objects(q) => q.uses_target(),
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::EntrySource => Ok(()),
            Self::Objects(q) => q.validate_vocabulary(vocabulary),
        }
    }
}

/// Recipient selector for [`DealDamageSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageTargetQuery {
    Targets,
    Objects(ObjectQuery),
    Players(PlayerSelector),
    Both {
        objects: ObjectQuery,
        players: PlayerSelector,
    },
    Relative(Box<DamageRecipientsRef>),
    /// Announced multi-targets (`min_targets..=max_targets`) dealt `spec.amount`.
    AnnouncedTargets {
        min_targets: u16,
        max_targets: u16,
    },
    /// Power-based damage source/recipient pair (`DamageFromPower`).
    FromPower(PowerDamageSource),
    /// First remembered creature deals its power as damage to second (with optional trample overflow).
    CreaturePowerToOther {
        trample_overflow: bool,
    },
    /// Damage target permanent's controller's creatures (optionally including target & exiling if dies).
    TargetControllersCreatures {
        include_target: bool,
        require_damaged_creatures_to_attack: bool,
    },
    /// Ordered targets: `first` damage to target 0, `others` damage to remaining targets.
    OrderedTargets {
        first: spellwake_contracts::RulesInteger,
        others: spellwake_contracts::RulesInteger,
    },
    /// Divide 3 damage automatically across 1..=3 ordered targets (3 / 2+1 / 1+1+1).
    DivideThree,
    /// Divide `total` damage among announced targets (`max_targets` optional cap).
    Divided {
        total: Box<ValueExpr>,
        max_targets: Option<u16>,
    },
}

impl DamageTargetQuery {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Targets
            | Self::AnnouncedTargets { .. }
            | Self::CreaturePowerToOther { .. }
            | Self::TargetControllersCreatures { .. }
            | Self::OrderedTargets { .. }
            | Self::DivideThree
            | Self::Divided { .. } => true,
            Self::FromPower(_) => true,
            Self::Objects(q) => q.uses_target(),
            Self::Players(p) => p.uses_target(),
            Self::Both { objects, players } => objects.uses_target() || players.uses_target(),
            Self::Relative(r) => match r.as_ref() {
                DamageRecipientsRef::Target | DamageRecipientsRef::TargetController => true,
                DamageRecipientsRef::Group(g) => match g {
                    crate::program::DamageGroup::ControllerAndTargetCreature => true,
                    crate::program::DamageGroup::Filtered(f) => {
                        crate::toolbox::query::filter_uses_target(f)
                    }
                    crate::program::DamageGroup::FilteredAndPlayers { filter, players } => {
                        crate::toolbox::query::filter_uses_target(filter)
                            || PlayerSelector::Group(*players).uses_target()
                    }
                    crate::program::DamageGroup::AffectedPlayers(players) => {
                        PlayerSelector::Group(*players).uses_target()
                    }
                    _ => false,
                },
                _ => false,
            },
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Targets
            | Self::AnnouncedTargets { .. }
            | Self::FromPower(_)
            | Self::CreaturePowerToOther { .. }
            | Self::TargetControllersCreatures { .. }
            | Self::OrderedTargets { .. }
            | Self::DivideThree => Ok(()),
            Self::Divided { total, .. } => total
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::Objects(q) => q.validate_vocabulary(vocabulary),
            Self::Players(p) => p.validate_vocabulary(vocabulary),
            Self::Both { objects, players } => {
                objects.validate_vocabulary(vocabulary)?;
                players.validate_vocabulary(vocabulary)
            }
            Self::Relative(r) => match r.as_ref() {
                DamageRecipientsRef::Group(g) => {
                    if matches!(
                        g,
                        crate::program::DamageGroup::CreaturesExcept(kind)
                            | crate::program::DamageGroup::CreaturesOfSubtype(kind)
                            if vocabulary.label(*kind).is_none()
                    ) {
                        return Err(DefinitionError::ForeignCreatureType);
                    }
                    if let crate::program::DamageGroup::Filtered(filter)
                    | crate::program::DamageGroup::FilteredAndPlayers { filter, .. } = g
                    {
                        filter.validate_vocabulary(vocabulary)?;
                    }
                    Ok(())
                }
                _ => Ok(()),
            },
        }
    }
}

/// Specification for `Instruction::DealDamage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DealDamageSpec {
    pub source: DamageSourceQuery,
    pub recipients: DamageTargetQuery,
    pub amount: DamageAmountSpec,
    pub exile_if_dies: bool,
}

impl DealDamageSpec {
    #[must_use]
    pub const fn new(
        source: DamageSourceQuery,
        recipients: DamageTargetQuery,
        amount: DamageAmountSpec,
    ) -> Self {
        Self {
            source,
            recipients,
            amount,
            exile_if_dies: false,
        }
    }

    #[must_use]
    pub fn announced_targets(amount: ValueExpr, min_targets: u16, max_targets: u16) -> Self {
        Self::new(
            DamageSourceQuery::EntrySource,
            DamageTargetQuery::AnnouncedTargets {
                min_targets,
                max_targets,
            },
            DamageAmountSpec::Expr(amount),
        )
    }

    #[must_use]
    pub fn from_power(source: PowerDamageSource) -> Self {
        Self::new(
            DamageSourceQuery::EntrySource,
            DamageTargetQuery::FromPower(source),
            DamageAmountSpec::Expr(ValueExpr::Literal(0.into())),
        )
    }

    #[must_use]
    pub fn creature_power_to_other(trample_overflow: bool) -> Self {
        Self::new(
            DamageSourceQuery::EntrySource,
            DamageTargetQuery::CreaturePowerToOther { trample_overflow },
            DamageAmountSpec::Expr(ValueExpr::Literal(0.into())),
        )
    }

    #[must_use]
    pub fn target_controllers_creatures(
        amount: ValueExpr,
        include_target: bool,
        require_damaged_creatures_to_attack: bool,
    ) -> Self {
        Self::new(
            DamageSourceQuery::EntrySource,
            DamageTargetQuery::TargetControllersCreatures {
                include_target,
                require_damaged_creatures_to_attack,
            },
            DamageAmountSpec::Expr(amount),
        )
    }

    #[must_use]
    pub fn ordered_targets(
        first: spellwake_contracts::RulesInteger,
        others: spellwake_contracts::RulesInteger,
    ) -> Self {
        Self::new(
            DamageSourceQuery::EntrySource,
            DamageTargetQuery::OrderedTargets { first, others },
            DamageAmountSpec::Expr(ValueExpr::Literal(0.into())),
        )
    }

    #[must_use]
    pub fn divide_three() -> Self {
        Self::new(
            DamageSourceQuery::EntrySource,
            DamageTargetQuery::DivideThree,
            DamageAmountSpec::Expr(ValueExpr::Literal(3.into())),
        )
    }

    #[must_use]
    pub fn divided(total: ValueExpr, max_targets: Option<u16>) -> Self {
        Self::new(
            DamageSourceQuery::EntrySource,
            DamageTargetQuery::Divided {
                total: Box::new(total),
                max_targets,
            },
            DamageAmountSpec::Expr(ValueExpr::Literal(0.into())),
        )
    }

    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.source.uses_target()
            || self.recipients.uses_target()
            || matches!(&self.amount, DamageAmountSpec::Expr(e) if e.uses_target())
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.source.validate_vocabulary(vocabulary)?;
        self.recipients.validate_vocabulary(vocabulary)?;
        if let DamageAmountSpec::Expr(e) = &self.amount {
            e.validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction)?;
        }
        Ok(())
    }
}

/// Player-level state transition executed by [`MutatePlayerSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerMutation {
    AddCounter {
        kind: CounterKind,
        amount: ValueExpr,
    },
    PayCounter {
        kind: CounterKind,
        amount: ValueExpr,
    },
    WinGame,
    LoseGame,
    DrawGame,
    TakeExtraTurn,
    EndTurn,
    BecomeMonarch,
    Ascend,
    IncreaseSpeed,
    SetDayNight(DayNight),
    DisallowDamagePreventionThisTurn,
    /// Grant an additional loyalty ability activation this turn.
    AdditionalLoyaltyActivation,
    /// Schedule an additional combat phase after this phase (`untap` specifies which creatures untap).
    AdditionalCombat {
        untap: CombatUntapGroup,
    },
    /// Watch the next spell or loyalty ability (`loyalty_only`: next loyalty only vs spell or loyalty twice).
    WatchNextSpellOrLoyalty {
        loyalty_only: bool,
    },
    /// Take the initiative and venture into Undercity (CR 725).
    TakeInitiative,
    /// Venture into a dungeon (CR 701.46; `undercity` forces Undercity).
    VentureIntoDungeon {
        undercity: bool,
    },
    /// The Ring tempts you (CR 701.52).
    RingTemptsYou,
    /// Select the player with highest/lowest life total into `chosen_player`.
    SelectExtremeLife {
        highest: bool,
        tie_mode: ExtremeLifeTieMode,
    },
    /// Choose one of `last_moved_objects` into `last_moved_objects`.
    ChooseOneLastMoved,
    /// Behold a permanent/hand card matching `filter` (`optional` allows declining).
    Behold {
        filter: Box<ObjectFilter>,
        optional: bool,
    },
    /// Manifest the top `amount` cards of library (CR 701.34).
    ManifestTop {
        amount: ValueExpr,
    },
    /// Manifest dread (CR 701.62).
    ManifestDread,
    /// Incubate `count` (CR 701.51).
    Incubate {
        count: ValueExpr,
    },
    /// Demonstrate (CR 702.144).
    Demonstrate,
    /// Ripple N (CR 702.60).
    Ripple(u8),
    /// Vote on `options` (`secret` hides votes until reveal).
    Vote {
        options: Vec<String>,
        secret: bool,
    },
    /// Proliferate (CR 701.27).
    Proliferate,
    /// Clash with an opponent (CR 701.23).
    Clash,
    /// Bolster N (CR 701.32).
    Bolster(u16),
    /// Populate (CR 701.30).
    Populate,
    /// Amass subtype N (CR 701.44).
    Amass {
        army: spellwake_contracts::creature_types::CreatureTypeId,
        subtype: spellwake_contracts::creature_types::CreatureTypeId,
        amount: u16,
    },
    /// Blight N (choose a controlled creature to get N -1/-1 counters).
    Blight {
        amount: ValueExpr,
    },
    /// Collect evidence N (CR 701.59).
    CollectEvidence {
        minimum: ValueExpr,
    },
    /// Add mana to the selected player's mana pool.
    AddMana(EffectManaYield),
    /// Scry N (CR 701.18).
    Scry(ValueExpr),
    /// Surveil N (CR 701.42).
    Surveil(ValueExpr),
    /// Mill N (CR 701.13).
    Mill(ValueExpr),
}

impl PlayerMutation {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::AddCounter { amount, .. }
            | Self::PayCounter { amount, .. }
            | Self::ManifestTop { amount }
            | Self::Incubate { count: amount }
            | Self::Blight { amount }
            | Self::CollectEvidence { minimum: amount }
            | Self::Scry(amount)
            | Self::Surveil(amount)
            | Self::Mill(amount) => amount.uses_target(),
            Self::Behold { filter, .. } => crate::toolbox::query::filter_uses_target(filter),
            Self::AddMana(yield_kind) => yield_kind.uses_target(),
            Self::WinGame
            | Self::LoseGame
            | Self::DrawGame
            | Self::TakeExtraTurn
            | Self::EndTurn
            | Self::BecomeMonarch
            | Self::Ascend
            | Self::IncreaseSpeed
            | Self::SetDayNight(_)
            | Self::DisallowDamagePreventionThisTurn
            | Self::AdditionalLoyaltyActivation
            | Self::AdditionalCombat { .. }
            | Self::WatchNextSpellOrLoyalty { .. }
            | Self::TakeInitiative
            | Self::VentureIntoDungeon { .. }
            | Self::RingTemptsYou
            | Self::SelectExtremeLife { .. }
            | Self::ChooseOneLastMoved
            | Self::ManifestDread
            | Self::Demonstrate
            | Self::Ripple(_)
            | Self::Vote { .. }
            | Self::Proliferate
            | Self::Clash
            | Self::Bolster(_)
            | Self::Populate
            | Self::Amass { .. } => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::AddCounter { amount, .. }
            | Self::PayCounter { amount, .. }
            | Self::ManifestTop { amount }
            | Self::Incubate { count: amount }
            | Self::Blight { amount }
            | Self::CollectEvidence { minimum: amount }
            | Self::Scry(amount)
            | Self::Surveil(amount)
            | Self::Mill(amount) => amount
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::Behold { filter, .. } => filter.validate_vocabulary(vocabulary),
            Self::Amass { army, subtype, .. } => {
                if vocabulary.label(*army).is_none() || vocabulary.label(*subtype).is_none() {
                    return Err(DefinitionError::ForeignCreatureType);
                }
                Ok(())
            }
            Self::AddMana(yield_kind) => yield_kind.validate_vocabulary(vocabulary),
            _ => Ok(()),
        }
    }
}

/// Specification for `Instruction::MutatePlayer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutatePlayerSpec {
    pub players: PlayerSelector,
    pub op: PlayerMutation,
}

impl MutatePlayerSpec {
    #[must_use]
    pub const fn new(players: PlayerSelector, op: PlayerMutation) -> Self {
        Self { players, op }
    }

    #[must_use]
    pub const fn add_counter(
        players: PlayerSelector,
        kind: CounterKind,
        amount: ValueExpr,
    ) -> Self {
        Self::new(players, PlayerMutation::AddCounter { kind, amount })
    }

    #[must_use]
    pub const fn pay_counter(
        players: PlayerSelector,
        kind: CounterKind,
        amount: ValueExpr,
    ) -> Self {
        Self::new(players, PlayerMutation::PayCounter { kind, amount })
    }

    #[must_use]
    pub const fn win_game(players: PlayerSelector) -> Self {
        Self::new(players, PlayerMutation::WinGame)
    }

    #[must_use]
    pub const fn lose_game(players: PlayerSelector) -> Self {
        Self::new(players, PlayerMutation::LoseGame)
    }

    #[must_use]
    pub const fn draw_game() -> Self {
        Self::new(
            PlayerSelector::Group(crate::program::AffectedPlayers::EachPlayer),
            PlayerMutation::DrawGame,
        )
    }

    #[must_use]
    pub const fn take_extra_turn(players: PlayerSelector) -> Self {
        Self::new(players, PlayerMutation::TakeExtraTurn)
    }

    #[must_use]
    pub const fn end_turn() -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::EndTurn,
        )
    }

    #[must_use]
    pub const fn become_monarch(players: PlayerSelector) -> Self {
        Self::new(players, PlayerMutation::BecomeMonarch)
    }

    #[must_use]
    pub const fn ascend() -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::Ascend,
        )
    }

    #[must_use]
    pub const fn increase_speed() -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::IncreaseSpeed,
        )
    }

    #[must_use]
    pub const fn set_day_night(clock: DayNight) -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::SetDayNight(clock),
        )
    }

    #[must_use]
    pub const fn disallow_damage_prevention() -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::DisallowDamagePreventionThisTurn,
        )
    }

    #[must_use]
    pub fn resolving_player(&self) -> Option<crate::program::ResolvingPlayer> {
        match &self.players {
            PlayerSelector::Resolving(p)
            | PlayerSelector::Group(crate::program::AffectedPlayers::One(p)) => Some(*p),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_pass_c_or_selection_op(&self) -> bool {
        matches!(
            self.op,
            PlayerMutation::TakeInitiative
                | PlayerMutation::VentureIntoDungeon { .. }
                | PlayerMutation::RingTemptsYou
                | PlayerMutation::ChooseOneLastMoved
                | PlayerMutation::Behold { .. }
                | PlayerMutation::ManifestDread
                | PlayerMutation::Incubate { .. }
                | PlayerMutation::Demonstrate
                | PlayerMutation::Ripple(_)
                | PlayerMutation::Vote { .. }
                | PlayerMutation::Clash
                | PlayerMutation::Bolster(_)
                | PlayerMutation::Populate
                | PlayerMutation::Amass { .. }
                | PlayerMutation::Blight { .. }
                | PlayerMutation::CollectEvidence { .. }
        )
    }

    #[must_use]
    pub const fn additional_loyalty_activation() -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::AdditionalLoyaltyActivation,
        )
    }

    #[must_use]
    pub const fn additional_combat(untap: CombatUntapGroup) -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::AdditionalCombat { untap },
        )
    }

    #[must_use]
    pub const fn watch_next_spell_or_loyalty(loyalty_only: bool) -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::WatchNextSpellOrLoyalty { loyalty_only },
        )
    }

    #[must_use]
    pub const fn take_initiative(player: crate::program::ResolvingPlayer) -> Self {
        Self::new(
            PlayerSelector::Resolving(player),
            PlayerMutation::TakeInitiative,
        )
    }

    #[must_use]
    pub const fn venture_into_dungeon(
        player: crate::program::ResolvingPlayer,
        undercity: bool,
    ) -> Self {
        Self::new(
            PlayerSelector::Resolving(player),
            PlayerMutation::VentureIntoDungeon { undercity },
        )
    }

    #[must_use]
    pub const fn ring_tempts_you() -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::RingTemptsYou,
        )
    }

    #[must_use]
    pub const fn select_extreme_life(highest: bool, tie_mode: ExtremeLifeTieMode) -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::SelectExtremeLife { highest, tie_mode },
        )
    }

    #[must_use]
    pub const fn choose_one_last_moved(chooser: crate::program::ResolvingPlayer) -> Self {
        Self::new(
            PlayerSelector::Resolving(chooser),
            PlayerMutation::ChooseOneLastMoved,
        )
    }

    #[must_use]
    pub fn behold(filter: ObjectFilter, optional: bool) -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::Behold {
                filter: Box::new(filter),
                optional,
            },
        )
    }

    #[must_use]
    pub fn manifest_top(player: crate::program::ResolvingPlayer, amount: ValueExpr) -> Self {
        Self::new(
            PlayerSelector::Resolving(player),
            PlayerMutation::ManifestTop { amount },
        )
    }

    #[must_use]
    pub const fn manifest_dread() -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::ManifestDread,
        )
    }

    #[must_use]
    pub fn incubate(count: ValueExpr) -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::Incubate { count },
        )
    }

    #[must_use]
    pub const fn demonstrate() -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::Demonstrate,
        )
    }

    #[must_use]
    pub const fn ripple(n: u8) -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::Ripple(n),
        )
    }

    #[must_use]
    pub fn vote(options: Vec<String>, secret: bool) -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::Vote { options, secret },
        )
    }

    #[must_use]
    pub const fn proliferate() -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::Proliferate,
        )
    }

    #[must_use]
    pub const fn clash(chooser: crate::program::ResolvingPlayer) -> Self {
        Self::new(PlayerSelector::Resolving(chooser), PlayerMutation::Clash)
    }

    #[must_use]
    pub const fn bolster(amount: u16) -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::Bolster(amount),
        )
    }

    #[must_use]
    pub const fn populate() -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::Populate,
        )
    }

    #[must_use]
    pub const fn amass(
        army: spellwake_contracts::creature_types::CreatureTypeId,
        subtype: spellwake_contracts::creature_types::CreatureTypeId,
        amount: u16,
    ) -> Self {
        Self::new(
            PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            PlayerMutation::Amass {
                army,
                subtype,
                amount,
            },
        )
    }

    #[must_use]
    pub fn blight(player: crate::program::ResolvingPlayer, amount: ValueExpr) -> Self {
        Self::new(
            PlayerSelector::Resolving(player),
            PlayerMutation::Blight { amount },
        )
    }

    #[must_use]
    pub fn collect_evidence(player: crate::program::ResolvingPlayer, minimum: ValueExpr) -> Self {
        Self::new(
            PlayerSelector::Resolving(player),
            PlayerMutation::CollectEvidence { minimum },
        )
    }

    #[must_use]
    pub fn add_mana(player: crate::program::ResolvingPlayer, yield_kind: EffectManaYield) -> Self {
        Self::new(
            PlayerSelector::Resolving(player),
            PlayerMutation::AddMana(yield_kind),
        )
    }

    #[must_use]
    pub fn scry(player: crate::program::ResolvingPlayer, amount: ValueExpr) -> Self {
        Self::new(
            PlayerSelector::Resolving(player),
            PlayerMutation::Scry(amount),
        )
    }

    #[must_use]
    pub fn surveil(player: crate::program::ResolvingPlayer, amount: ValueExpr) -> Self {
        Self::new(
            PlayerSelector::Resolving(player),
            PlayerMutation::Surveil(amount),
        )
    }

    #[must_use]
    pub fn mill(player: crate::program::ResolvingPlayer, amount: ValueExpr) -> Self {
        Self::new(
            PlayerSelector::Resolving(player),
            PlayerMutation::Mill(amount),
        )
    }

    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.players.uses_target() || self.op.uses_target()
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.players.validate_vocabulary(vocabulary)?;
        self.op.validate_vocabulary(vocabulary)
    }
}
