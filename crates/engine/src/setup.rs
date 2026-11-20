//! Explicit trusted native setup, not a deck validator or saved-game format.
use crate::{
    combat::CombatRules, effects::SeedCard, mana::FloatingMana, object_identity::CardId,
    priority::SeatId,
};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartingLife {
    pub player: SeatId,
    pub total: RulesInteger,
}

/// Consumed once into the existing participant, card and resource owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSeed {
    pub seats: Vec<SeatId>,
    pub active: SeatId,
    pub combat_rules: CombatRules,
    pub cards: Vec<SeedCard>,
    /// Trusted relative timestamp order of every seeded battlefield card, exactly once.
    pub initial_battlefield_order: Vec<CardId>,
    /// Explicit uninterrupted World cohorts, oldest first; equal within a cohort.
    /// Must cover every initially evaluated battlefield World exactly once.
    pub initial_world_cohorts: Vec<Vec<CardId>>,
    pub mana: Vec<FloatingMana>,
    pub life: Vec<StartingLife>,
    pub creature_vocabulary: Arc<CreatureTypeVocabulary>,
    /// Explicit immutable interchangeable-name relation for this game.
    pub name_rules: Arc<spellwake_contracts::names::NameRules>,
}
impl GameSeed {
    /// A supplied fixture/profile policy, never an inferred format or default.
    /// Constructor validation still rejects invalid seats or resource inputs.
    #[allow(clippy::too_many_arguments)] // This explicit fixture policy also supplies setup order.
    pub fn with_uniform_life(
        seats: Vec<SeatId>,
        active: SeatId,
        cards: Vec<SeedCard>,
        initial_battlefield_order: Vec<CardId>,
        mana: Vec<FloatingMana>,
        total: RulesInteger,
        combat_rules: CombatRules,
        creature_vocabulary: Arc<CreatureTypeVocabulary>,
        name_rules: Arc<spellwake_contracts::names::NameRules>,
    ) -> Self {
        let life = seats
            .iter()
            .map(|&player| StartingLife {
                player,
                total: total.clone(),
            })
            .collect();
        Self {
            seats,
            active,
            combat_rules,
            cards,
            initial_battlefield_order,
            // This native fixture policy explicitly supplies no initial Worlds.
            // Callers seeding Worlds must supply their complete tenure premise.
            initial_world_cohorts: vec![],
            mana,
            life,
            creature_vocabulary,
            name_rules,
        }
    }
}
