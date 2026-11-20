//! Strict selection-only preparation. Shared catalogue lowering owns all partitions.
use crate::ordinary_game::{
    catalogue::{
        self, DefinitionInput, PhysicalRow, PreparedBattlefield, PreparedMaterialized,
        PreparedSetup, PreparedZone, Shape,
    },
    mandatory::data::{
        InitializationScheduledBattlefieldOldestFirstItem, InitializationScheduledLife,
        InitializationScheduledZonesItem, Role, StartStep,
    },
};
use serde::Deserialize;
use spellwake_card_programs::Registry;
use spellwake_content::Inventory;
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary};
use spellwake_engine::{
    mana::{FloatingMana, ManaType},
    setup::StartingLife,
    turn::{Step, TurnStart},
};

/// New strict wire shape; old Opening/Scheduled/Mana grammars remain unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum SelectionInitialization {
    SelectionPrepared {
        turn: u64,
        step: StartStep,
        active: Role,
        life: InitializationScheduledLife,
        zones: Vec<InitializationScheduledZonesItem>,
        battlefield_oldest_first: Vec<InitializationScheduledBattlefieldOldestFirstItem>,
        floating_mana: Vec<PreparedManaInput>,
    },
}
/// All six native types, including colourless which is not a color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(in crate::ordinary_game) enum PreparedManaType {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colourless,
}
impl PreparedManaType {
    pub(in crate::ordinary_game) fn native(self) -> ManaType {
        match self {
            Self::White => ManaType::White,
            Self::Blue => ManaType::Blue,
            Self::Black => ManaType::Black,
            Self::Red => ManaType::Red,
            Self::Green => ManaType::Green,
            Self::Colourless => ManaType::Colourless,
        }
    }
}
/// Explicit seeded pool, not a claim of natural mana production.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ordinary_game) struct PreparedManaInput {
    pub(in crate::ordinary_game) player: Role,
    pub(in crate::ordinary_game) mana_type: PreparedManaType,
    pub(in crate::ordinary_game) amount: RulesInteger,
}
/// Materialize the complete guarded source9 before lowering any physical subset.
/// The actual vocabulary and decoded Inventory must belong to this Registry9.
/// Raw-byte/line receipts and printing annotations remain authenticated upstream;
/// the shared materializer's hexadecimal shape checks do not prove raw sources.
///
/// # Errors
/// Rejects wrong registry/source/printing/signature/physical membership through
/// the shared materializer, and invalid prepared resources/partitions through
/// its single prepared lowering owner. Does not initialize a game.
pub(in crate::ordinary_game) fn materialize_prepared(
    inventory: &Inventory,
    registry: &Registry,
    vocabulary: &CreatureTypeVocabulary,
    sources: &[DefinitionInput],
    rows: &[PhysicalRow],
    initialization: &SelectionInitialization,
) -> Result<PreparedMaterialized, String> {
    registry
        .validate_resolution_selection_source(inventory, vocabulary)
        .map_err(|error| format!("selection preparation source validation: {error}"))?;
    let materialized = catalogue::materialize_rows(
        inventory,
        registry,
        sources,
        rows,
        Shape::ResolutionSelectionPrepared,
    )?;
    let SelectionInitialization::SelectionPrepared {
        turn,
        step,
        active,
        life,
        zones,
        battlefield_oldest_first,
        floating_mana,
    } = initialization;
    let zones = zones
        .iter()
        .map(|row| PreparedZone {
            owner: row.role.seat(),
            hand: &row.hand,
            library_top_first: &row.library_top_first,
            graveyard_top_first: &row.graveyard_top_first,
            exile: &row.exile,
        })
        .collect::<Vec<_>>();
    let battlefield = battlefield_oldest_first
        .iter()
        .map(|row| PreparedBattlefield {
            alias: &row.alias,
            controller: row.controller.seat(),
            tapped: row.tapped,
        })
        .collect::<Vec<_>>();
    let parse_life = |value: &str| {
        RulesInteger::try_from(value.to_owned())
            .map_err(|error| format!("invalid prepared life: {error}"))
    };
    catalogue::prepare_materialized(
        materialized,
        PreparedSetup {
            start: TurnStart {
                number: *turn,
                step: match step {
                    StartStep::Untap => Step::Untap,
                    StartStep::PrecombatMain => Step::PrecombatMain,
                },
            },
            active: active.seat(),
            life: vec![
                StartingLife {
                    player: Role::P0.seat(),
                    total: parse_life(&life.p0)?,
                },
                StartingLife {
                    player: Role::P1.seat(),
                    total: parse_life(&life.p1)?,
                },
            ],
            zones: &zones,
            battlefield_oldest_first: &battlefield,
            mana: floating_mana
                .iter()
                .map(|row| FloatingMana {
                    player: row.player.seat(),
                    mana_type: row.mana_type.native(),
                    amount: row.amount.clone(),
                })
                .collect(),
        },
    )
}

#[cfg(test)]
mod tests;
