//! Universal composable `AdjustLife`, `DealDamage`, and `MutatePlayer` primitives.

use crate::{
    day_night::DayNight,
    effects::CounterKind,
    program::{DamageAmountSpec, DamageRecipientsRef, DefinitionError, ValueExpr},
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
}

impl DamageTargetQuery {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Targets => true,
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
            Self::Targets => Ok(()),
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
}

impl PlayerMutation {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::AddCounter { amount, .. } | Self::PayCounter { amount, .. } => {
                amount.uses_target()
            }
            Self::WinGame
            | Self::LoseGame
            | Self::DrawGame
            | Self::TakeExtraTurn
            | Self::EndTurn
            | Self::BecomeMonarch
            | Self::Ascend
            | Self::IncreaseSpeed
            | Self::SetDayNight(_)
            | Self::DisallowDamagePreventionThisTurn => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::AddCounter { amount, .. } | Self::PayCounter { amount, .. } => amount
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
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
