//! Retired bespoke meld overrides; falls through to v2_lowering.
use super::*;

pub(super) fn compile(
    _card: &InventoryCard,
    _inventory: &Inventory,
    _vocabulary: &CreatureTypeVocabulary,
    _loyalty_source_verified: bool,
) -> Option<Program> {
    None
}
