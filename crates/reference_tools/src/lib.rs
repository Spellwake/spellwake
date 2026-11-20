//! Shared observation serializers for native reference-tool executables.
#[path = "../../engine/test_support/check_trace.rs"]
pub mod check_trace;
#[path = "../../engine/test_support/damage_trace.rs"]
pub mod damage_trace;
pub mod ordinary_game;
pub mod ordinary_trigger;

#[path = "../../engine/test_support/creature_types.rs"]
pub mod creature_types;
pub mod current_profile;
#[path = "../../engine/test_support/mana_ref.rs"]
pub mod mana_ref;
#[path = "../../engine/test_support/packet.rs"]
pub mod packet;
#[path = "../../engine/test_support/program.rs"]
pub mod program;
#[path = "../../engine/test_support/projection.rs"]
pub mod projection;
#[path = "../../engine/test_support/receipt_joins.rs"]
pub mod receipt_joins;
pub mod resolution_values;
pub mod root_recipes;

#[path = "../../engine/test_support/creature_vocabulary.rs"]
pub mod creature_vocabulary;

pub mod current_capture;

pub mod capture_points;

pub mod native_calls;

#[cfg(test)]
mod contract_cases;
#[cfg(test)]
#[path = "../../engine/test_support/resolution_contract_fixture.rs"]
mod resolution_contract_fixture;

#[cfg(test)]
#[path = "../../../tests/support/resolution_contract_cases.rs"]
mod resolution_contract_cases;

/// Input-only qualification fault data; no engine mutation API is exposed.
#[path = "../../engine/test_support/related_player_fault.rs"]
pub mod related_player_fault;
