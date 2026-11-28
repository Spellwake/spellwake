//! Shared primitive bitmasks and scalar enums used by `v2::ir` and downstream lowering.

use serde::{Deserialize, Serialize};

/// Bitmask over Magic card types (mirrors `spellwake_engine::program::CardTypeMask`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardTypeMask(pub u16);

impl CardTypeMask {
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
    pub const PERMANENT: Self = Self(
        Self::BATTLE.0
            | Self::PLANESWALKER.0
            | Self::LAND.0
            | Self::ARTIFACT.0
            | Self::CREATURE.0
            | Self::ENCHANTMENT.0,
    );

    /// Combine two type masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// True when no card type bits are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Five-bit color mask supporting arbitrary unions and exclusions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorMask(pub u8);

impl ColorMask {
    pub const EMPTY: Self = Self(0);
    pub const WHITE: Self = Self(1 << 0);
    pub const BLUE: Self = Self(1 << 1);
    pub const BLACK: Self = Self(1 << 2);
    pub const RED: Self = Self(1 << 3);
    pub const GREEN: Self = Self(1 << 4);

    /// Combine two color masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// True when no color bits are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Bitmask over Magic supertypes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupertypeMask(pub u8);

impl SupertypeMask {
    pub const EMPTY: Self = Self(0);
    pub const BASIC: Self = Self(1 << 0);
    pub const LEGENDARY: Self = Self(1 << 1);
    pub const SNOW: Self = Self(1 << 2);
    pub const WORLD: Self = Self(1 << 3);
    pub const HISTORIC: Self = Self(1 << 4);

    /// Combine two supertype masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// True when no supertype bits are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Color-arity predicate (`colorless`, `multicolored`, `monocolored`, `colored`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorArity {
    Colorless,
    Colored,
    Multicolored,
    Monocolored,
}

/// Controller or owner relation relative to the ability/spell controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControllerRelation {
    You,
    YourTeam,
    NotYou,
    Opponent,
    Opponents,
    TargetOpponent,
    TargetPlayer,
    ThatPlayer,
    DefendingPlayer,
    ItsController,
    OwnerYou,
    OwnerOpponent,
    OwnerThatPlayer,
    ActivePlayer,
    ChosenPlayer,
    ThatPlayerOrPlaneswalkerController,
    NextPlayerInDirection,
    PlayerToLeft,
    PlayerToRight,
}

/// Combat-state predicate on a battlefield creature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CombatState {
    Attacking,
    Blocking,
    AttackingOrBlocking,
    Blocked,
    Unblocked,
    BlockingSelf,
    BlockedBySelf,
    BlockingOrBlockedBySelf,
    NotAttacking,
}

/// Numeric comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
}

/// Object stat kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatKind {
    Power,
    Toughness,
    ManaValue,
    Loyalty,
}

/// Player numeric statistic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerStatKind {
    LifeTotal,
    CardsInHand,
    CardsInLibrary,
    CardsInGraveyard,
}

/// Rounding direction for integer division.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rounding {
    Down,
    Up,
}

/// Turn step for `StepBegins`, `StepEnds`, and delayed step triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnStepAst {
    UntapStep,
    Upkeep,
    DrawStep,
    PrecombatMain,
    PostcombatMain,
    MainPhase,
    BeginningOfCombat,
    EndOfCombat,
    EndStep,
    CleanupStep,
}
