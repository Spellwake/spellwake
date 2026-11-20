//! Source-backed complete ordinary fixtures over the existing native rules owner.
//! This is privileged local qualification, not a host, player view or full card.
mod authenticated_source;
mod catalogue;
pub mod fixture;
pub mod interaction;
pub mod library_top;
pub mod mandatory;
pub mod optional;
pub mod policy;
pub mod probes;
mod record;
pub mod selection;
mod selectors;
pub mod teaching;
mod view;

use fixture::CaseId;
pub use record::{RunReport, run_case};
use serde::Serialize;

/// The source/build reference is resolved by the separately captured execution
/// packet. An ordinary local CLI invocation alone supplies no provenance claim.
#[derive(Debug, Serialize)]
pub struct SuiteReport {
    schema_version: u32,
    profile: &'static str,
    source_build_ref: &'static str,
    runs: Vec<RunReport>,
}
impl SuiteReport {
    pub fn completed(&self) -> bool {
        !self.runs.is_empty() && self.runs.iter().all(RunReport::completed)
    }
}
pub fn run_cases(
    cases: &[CaseId],
    vocabulary: std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> SuiteReport {
    SuiteReport {
        schema_version: 1,
        profile: "ordinary-game-trajectories",
        source_build_ref: "native-execution.json",
        runs: cases
            .iter()
            .enumerate()
            .map(|(index, case)| record::run_case_at(*case, index, vocabulary.clone()))
            .collect(),
    }
}
