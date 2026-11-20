//! Shared input-only qualification codecs and read-only native serializers.
//! Compiled only by engine tests; reference tools include these same source files.
// Engine-private tests use a subset of the helpers used by the reference-tools crate.
#![allow(dead_code)]
pub(crate) mod characteristic_trajectories;
pub(crate) mod check_trace;
pub(crate) mod creature_types;
pub(crate) mod creature_vocabulary;
pub(crate) mod damage_trace;
pub(crate) mod mana_ref;
pub(crate) mod packet;
pub(crate) mod program;
pub(crate) mod projection;
pub(crate) mod receipt_joins;

#[path = "../../reference_tools/src/current_profile.rs"]
pub(crate) mod current_profile;

#[path = "../../reference_tools/src/capture_points.rs"]
pub(crate) mod capture_points;
#[path = "../../reference_tools/src/current_capture.rs"]
pub(crate) mod current_capture;
#[path = "../../reference_tools/src/native_calls.rs"]
pub(crate) mod native_calls;

pub(crate) mod resolution_contract_fixture;

#[path = "../../../tests/support/resolution_contract_cases.rs"]
pub(crate) mod resolution_contract_cases;

pub(crate) mod related_player_fault;
