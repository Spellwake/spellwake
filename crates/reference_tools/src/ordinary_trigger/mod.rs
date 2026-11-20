//! Thin selected ordinary-trigger stage policy over the shared current-schema runner.
use serde_json::Value;
use std::path::Path;
pub fn run_file(path: &Path, case: &str) -> Value {
    crate::root_recipes::run_file(
        path,
        case,
        crate::root_recipes::input::StagePolicy::OrdinaryTriggers,
    )
}
