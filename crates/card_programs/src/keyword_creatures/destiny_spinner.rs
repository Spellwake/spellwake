//! Retired bespoke Destiny Spinner override; falls through to oracle_lowering.
use super::*;

pub(super) fn compile(
    _card: &InventoryCard,
    _vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    None
}
