//! Source-independent, persistent objects created in the command zone.
use crate::{
    characteristics::{CharacteristicTimestamp, KeywordAbility, PowerToughness},
    priority::SeatId,
};
use std::collections::BTreeSet;

/// One game-local emblem identity. It never aliases a card or permanent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EmblemId(pub(crate) u64);
impl EmblemId {
    /// Stable game-local sequence number.
    pub fn sequence(self) -> u64 {
        self.0
    }
}
/// Authored static emblem semantics, independent of the creating permanent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmblemDefinition {
    /// Prevent all but one damage from each source to the controller or their planeswalkers.
    PreventAllButOne,
    /// Its controller may activate loyalty abilities whenever they could cast an instant.
    LoyaltyAnyTurn,
    /// At the beginning of its controller's upkeep, this emblem damages that player.
    UpkeepDamage {
        amount: spellwake_contracts::RulesInteger,
    },
    /// Whenever its controller casts a qualifying spell, this emblem deals damage to any target.
    SpellcastDamage {
        amount: spellwake_contracts::RulesInteger,
        noncreature_only: bool,
    },
    /// {1}{B}, sacrifice a creature: gain life and draw cards equal to its last-known power.
    SacrificeCreatureGainLifeDraw,
    /// Artifacts put into this player’s graveyard from the battlefield return at the next end step.
    ReturnFallenArtifacts,
    /// No maximum hand size, plus an optional return for each card entering your graveyard.
    UnlimitedHandWithGraveyardReturn,
    /// All current and future creatures controlled by the emblem owner receive this effect.
    ControlledCreatureBonus {
        power_toughness: PowerToughness,
        keywords: BTreeSet<KeywordAbility>,
    },
}
/// Retained emblem with an independent timestamp for continuous-effect ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emblem {
    pub id: EmblemId,
    pub controller: SeatId,
    pub definition: EmblemDefinition,
    pub(crate) timestamp: CharacteristicTimestamp,
}
