//! Thin selected resolution-value stage policies over the shared root issuer.
use serde_json::Value;
use std::path::Path;
pub fn run_file(path: &Path, case: &str) -> Value {
    crate::root_recipes::run_file(
        path,
        case,
        crate::root_recipes::input::StagePolicy::ResolutionValues,
    )
}
pub fn run_matched_file(path: &Path, case: &str) -> Value {
    crate::root_recipes::run_file(
        path,
        case,
        crate::root_recipes::input::StagePolicy::MatchedReference,
    )
}
