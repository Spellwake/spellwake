//! Exact writer-point joins for the existing shared root issuer.
use crate::{
    current_profile::{ObservationJoin, Phase, Recorder, SnapshotOwner},
    packet::AliasContext,
};
use serde_json::Value;
use spellwake_engine::game::GameState;
use std::collections::BTreeMap;

pub(super) struct Capture {
    pub recorder: Recorder,
    points: Vec<String>,
    calls: Vec<String>,
    joins: BTreeMap<String, Vec<String>>,
    pointers: Vec<String>,
}
impl Capture {
    pub fn new(recorder: Recorder) -> Self {
        Self {
            recorder,
            points: vec![],
            calls: vec![],
            joins: BTreeMap::new(),
            pointers: vec![],
        }
    }
    pub fn point(
        &mut self,
        id: &str,
        observation: &str,
        phase: Phase,
        owner: SnapshotOwner<'_>,
        aliases: &AliasContext,
    ) -> Result<(), String> {
        self.recorder.snapshot(
            id,
            &[observation.to_owned()],
            phase,
            "root_recipes::runner actual saved native call",
            owner,
            aliases,
        )?;
        self.points.push(id.into());
        self.joins
            .entry(observation.into())
            .or_default()
            .push(id.into());
        Ok(())
    }
    pub fn state(
        &mut self,
        id: &str,
        observation: &str,
        phase: Phase,
        state: &GameState,
        storage: &str,
        aliases: &AliasContext,
    ) -> Result<(), String> {
        self.point(
            id,
            observation,
            phase,
            SnapshotOwner::GameState(state),
            aliases,
        )?;
        self.recorder.reference_record_point(id, storage)?;
        self.pointers.push(storage.into());
        Ok(())
    }
    pub fn call(&mut self, id: &str) {
        self.calls.push(id.into());
    }
    pub fn finish(mut self, report: &Value) -> Result<Value, String> {
        // A complete projection can serve several distinct actual point labels.
        self.pointers.sort();
        self.pointers.dedup();
        self.recorder.finish_coverage(
            self.points,
            self.calls,
            self.joins
                .into_iter()
                .map(|(pointer, points)| ObservationJoin { pointer, points })
                .collect(),
            self.pointers,
        )?;
        self.recorder.witness(report)
    }
}
