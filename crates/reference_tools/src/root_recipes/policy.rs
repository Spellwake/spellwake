//! Exact finite current-schema stage selection. Unlisted cases never inherit a seed mode.
use crate::packet::SeedMode;
use serde_json::Value;
#[cfg(test)]
pub const FROZEN_INPUT_SHA256: &str =
    "16dc72842bcd3b5bd7cda1e77bd45491cf67dccac3641ca91f4ef9befd2e80a6";
#[cfg(test)]
pub const MAIN_CASES: [&str; 22] = [
    "T01",
    "T02",
    "T03a",
    "T03b",
    "T03c",
    "T03-target-player",
    "T03-target-card",
    "T03-target-stack",
    "T04",
    "T05",
    "T06-base",
    "T06-recast",
    "T07",
    "T08",
    "T09-discard",
    "T10",
    "T11",
    "T12",
    "T15-prepared-main",
    "T15-skipped-draw",
    "T15-actual-draw",
    "T15-opening",
];

#[cfg(test)]
pub const EXTRA_PUBLIC_CASES: [&str; 39] = [
    "T16-V1-zero-empty",
    "T16-V8-copy-first-any",
    "T16-V8-counter-first-copyable",
    "T16-E6-old",
    "T16-E6-draw-inserted",
    "T16-E7-old",
    "T16-E7-draw-inserted",
    "T16-E1",
    "T16-E2",
    "T16-E3",
    "T16-E4",
    "T16-E5",
    "T16-E8-copy",
    "T16-E8-counter",
    "T16-E9",
    "T16-E10-retain",
    "T16-E10-change",
    "T16-E10-invalid-changes",
    "T16-F1",
    "T16-F3",
    "T16-F5-targeted",
    "T17-M11",
    "T17-M12",
    "T17-M13-tap",
    "T17-M13-sacrifice",
    "T17-M14",
    "T17-M15",
    "T17-M16",
    "T17-M17-entry",
    "T17-M17-countered",
    "T17-M32-two",
    "T17-M32-three",
    "T17-M34-forced-empty",
    "T17-M34-reply-empty",
    "T17-M35",
    "T17-M36",
    "T17-M36-zero-power",
    "T17-M37-empty-combat",
    "T17-M40",
];
#[cfg(test)]
pub const PUBLIC_CASES: [&str; 61] = [
    "T01",
    "T02",
    "T03a",
    "T03b",
    "T03c",
    "T03-target-player",
    "T03-target-card",
    "T03-target-stack",
    "T04",
    "T05",
    "T06-base",
    "T06-recast",
    "T07",
    "T08",
    "T09-discard",
    "T10",
    "T11",
    "T12",
    "T15-prepared-main",
    "T15-skipped-draw",
    "T15-actual-draw",
    "T15-opening",
    "T16-V1-zero-empty",
    "T16-V8-copy-first-any",
    "T16-V8-counter-first-copyable",
    "T16-E6-old",
    "T16-E6-draw-inserted",
    "T16-E7-old",
    "T16-E7-draw-inserted",
    "T16-E1",
    "T16-E2",
    "T16-E3",
    "T16-E4",
    "T16-E5",
    "T16-E8-copy",
    "T16-E8-counter",
    "T16-E9",
    "T16-E10-retain",
    "T16-E10-change",
    "T16-E10-invalid-changes",
    "T16-F1",
    "T16-F3",
    "T16-F5-targeted",
    "T17-M11",
    "T17-M12",
    "T17-M13-tap",
    "T17-M13-sacrifice",
    "T17-M14",
    "T17-M15",
    "T17-M16",
    "T17-M17-entry",
    "T17-M17-countered",
    "T17-M32-two",
    "T17-M32-three",
    "T17-M34-forced-empty",
    "T17-M34-reply-empty",
    "T17-M35",
    "T17-M36",
    "T17-M36-zero-power",
    "T17-M37-empty-combat",
    "T17-M40",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagePolicy {
    OrdinaryTriggers,
    ResolutionValues,
    MatchedReference,
}
impl StagePolicy {
    pub fn profile(self) -> &'static str {
        match self {
            Self::OrdinaryTriggers => "ordinary-trigger-root",
            Self::ResolutionValues => "resolution-values-root",
            Self::MatchedReference => "resolution-values-matched-root",
        }
    }
    pub fn input_sha256(self) -> &'static str {
        match self {
            Self::OrdinaryTriggers => {
                "16dc72842bcd3b5bd7cda1e77bd45491cf67dccac3641ca91f4ef9befd2e80a6"
            }
            Self::ResolutionValues => {
                "9fd341cb164354619e599bb875d6ed6dc6400449688daf50329693c8e2bff8fd"
            }
            Self::MatchedReference => {
                "bc42e945640f7dfe9e22a0a5ecf7996748e7a1893bf47ef20d7e66ddd0a0c27a"
            }
        }
    }
    pub(super) fn select(self, case: &str, fixture: &Value) -> Result<SeedMode, String> {
        let (scope, mode, metadata) = match (self, case) {
            (Self::OrdinaryTriggers, "T01") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":5,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T02") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":2,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T03a") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":3,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T03b") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":6,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T03c") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":5,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T03-target-player") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":2,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T03-target-card") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":2,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T03-target-stack") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":3,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T04") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":12,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"CombatDamage","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T05") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":12,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"CombatDamage","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T06-base") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":10,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T06-recast") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12,13,14],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":14,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T07") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":8,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T08") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":6,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"DeclareAttackers","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T09-discard") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":9,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Cleanup","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T10") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":9,"cleanup_iteration":1,"kind":"Ready","no_extra_pass":true,"step":"Cleanup","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T11") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":3,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T12") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":5,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T15-prepared-main") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":0,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T15-skipped-draw") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":1,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T15-actual-draw") => (
                "root_scheduled",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":1,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Draw","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T15-opening") => (
                "root_opening",
                SeedMode::PublicMain,
                r#"{"initial_boundary":{"entropy_words":[7,6,5,4,3,2,1,7,6,5,4,3,2,1],"hand_size":7,"kind":"Opening"},"native_recipe":{"action_ordinals":[1,2],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":4096,"max_root_inputs":512}},"stop":{"after_action_ordinal":2,"cleanup_iteration":0,"kind":"Ready","no_extra_pass":true,"step":"Upkeep","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-V1-zero-empty") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":2,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-V8-copy-first-any") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-V8-counter-first-copyable") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":4,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E6-old") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E6-draw-inserted") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E7-old") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":4,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E7-draw-inserted") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":4,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E1") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":2,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E2") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4,5],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E3") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":2,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E4") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":4,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E5") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":6,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E8-copy") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":3,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E8-counter") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":4,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E9") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":6,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E10-retain") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":9,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E10-change") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":9,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-E10-invalid-changes") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12,13,14],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":14,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-F1") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":11,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-F3") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":2,"kind":"AfterLastRootInput","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T16-F5-targeted") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":2,"kind":"Ready","no_extra_pass":true}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M11") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":3,"kind":"Ready","no_extra_pass":true,"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M12") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":1,"kind":"Ready","no_extra_pass":true,"step":null,"turn":null}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M13-tap") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":2,"kind":"Ready","no_extra_pass":true,"step":null,"turn":null}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M13-sacrifice") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":2,"kind":"Ready","no_extra_pass":true,"step":null,"turn":null}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M14") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4,5],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true,"step":null,"turn":null}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M15") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4,5],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true,"step":null,"turn":null}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M16") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":6,"kind":"Ready","no_extra_pass":true,"step":null,"turn":null}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M17-entry") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":2,"kind":"Ready","no_extra_pass":true,"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M17-countered") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true,"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M32-two") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":0,"kind":"AfterInitialization","no_extra_pass":true,"step":null,"turn":null}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M32-three") => (
                "root_unscheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Interaction"},"native_recipe":{"action_ordinals":[],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":0,"kind":"AfterInitialization","no_extra_pass":true,"step":null,"turn":null}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M34-forced-empty") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":3,"kind":"Ready","no_extra_pass":true,"step":"DeclareAttackers","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M34-reply-empty") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":4,"kind":"Ready","no_extra_pass":true,"step":"DeclareAttackers","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M35") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":10,"kind":"Ready","no_extra_pass":true,"step":"DeclareBlockers","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M36") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12,13,14],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":14,"kind":"Ready","no_extra_pass":true,"step":"CombatDamage","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M36-zero-power") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":6,"kind":"Ready","no_extra_pass":true,"step":"CombatDamage","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M37-empty-combat") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"Untap","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true,"step":"PostcombatMain","turn":1}}"#,
            ),
            (Self::OrdinaryTriggers, "T17-M40") => (
                "root_opening",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"entropy_words":[7,6,5,4,3,2,1,7,6,5,4,3,2,1],"hand_size":7,"kind":"Opening"},"native_recipe":{"action_ordinals":[1,2,3,4],"budget":{"max_private_owner_calls":4096,"max_root_inputs":1024}},"stop":{"after_action_ordinal":4,"kind":"PendingMulligan","no_extra_pass":true,"step":null,"turn":null}}"#,
            ),
            (Self::ResolutionValues, "S2R01-DM") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":6}},"stop":{"after_action_ordinal":6,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R01-DM-owner-control") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":3,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R02-wall-negative") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":9}},"stop":{"after_action_ordinal":9,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R03-slots-distinct") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":8}},"stop":{"after_action_ordinal":9,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R03-slots-equal") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":8}},"stop":{"after_action_ordinal":9,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R04-late-bounce") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":4,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R05-all-illegal-choice") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":7}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R05-all-illegal-literal") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":7}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R06-draw-prefix") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":6}},"stop":{"after_action_ordinal":6,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R07-TimeLord-zero") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":4,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R08-empty-loss") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":3,"kind":"Outcome","no_extra_pass":true,"outcome_guard":{"priority_forbidden":true,"state_based_losses":["A"],"winner":"B"},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R09-deferred-loss") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":6}},"stop":{"after_action_ordinal":7,"kind":"Outcome","no_extra_pass":true,"outcome_guard":{"priority_forbidden":true,"state_based_losses":["A"],"winner":"B"},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R10-sacrificed-source") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":4,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R11-two-trigger-entries") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":10}},"stop":{"after_action_ordinal":10,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R12-waiting-through-choice") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":5}},"stop":{"after_action_ordinal":5,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":["leaves_entry"]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R13-parent-copy-retain") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12,13],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":12}},"stop":{"after_action_ordinal":13,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":["O_entry","created_copy"]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R13-parent-copy-change") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12,13],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":12}},"stop":{"after_action_ordinal":13,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":["O_entry","created_copy"]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R14-fresh-copy-single") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":13}},"stop":{"after_action_ordinal":12,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R14-fresh-copy-two") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":20}},"stop":{"after_action_ordinal":18,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R14-fresh-copy-all-illegal") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12,13],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":16}},"stop":{"after_action_ordinal":13,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R15-damage-multiplier-first") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":7}},"stop":{"after_action_ordinal":7,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R15-damage-prevention-first") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":7}},"stop":{"after_action_ordinal":7,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R15-damage-pending") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":4,"kind":"PendingDamage","no_extra_pass":true,"pending_guard":{"application_effect_refs":["shield","multiplier"],"chooser":"B","entry_ref":"main_entry","instruction":"3","kind":"DamageOrder","parent":{"entry_ref":"main_entry","instruction":"3","kind":"Resolution","locals":[{"slot":"0","value":{"ordinal":"85","vocabulary":"39721c01eef9fd3f869588b097651bb6115f9cdde5d61bf822d65a8e5435c792"}}]}},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::ResolutionValues, "S2R08-fully-empty-loss") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":3,"kind":"Outcome","no_extra_pass":true,"outcome_guard":{"priority_forbidden":true,"state_based_losses":["A"],"winner":"B"},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF01-DM-Elf") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":8}},"stop":{"after_action_ordinal":10,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF02-DM-Time-Lord") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":8}},"stop":{"after_action_ordinal":10,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF03-Exclude-Wall-Elf") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":6,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF04-Controlled-Artifact-Live") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":6,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF04-Controlled-Artifact-Sacrificed") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":6,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF05-Double-Then-Shield") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":8}},"stop":{"after_action_ordinal":11,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF05-Shield-Then-Double") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":8}},"stop":{"after_action_ordinal":11,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF05-Late-Missing-Target") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":4}},"stop":{"after_action_ordinal":6,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF05-All-Targets-Illegal") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":7}},"stop":{"after_action_ordinal":8,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF06-Copy-Choice-Damage-Count") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":14}},"stop":{"after_action_ordinal":19,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            (Self::MatchedReference, "S2NR-RF06-Copy-Choice-Shield-Damage-Count") => (
                "root_scheduled",
                SeedMode::PreparedRoot,
                r#"{"initial_boundary":{"kind":"Scheduled","step":"PrecombatMain","turn":1},"native_recipe":{"action_ordinals":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_native_auto_work_returns":0,"max_private_owner_calls":0,"max_root_inputs":19}},"stop":{"after_action_ordinal":26,"kind":"Ready","no_extra_pass":true,"ready_guard":{"priority":"A","stack_entry_refs_bottom_to_top":[]},"step":"PrecombatMain","turn":1}}"#,
            ),
            _ => {
                return Err(format!(
                    "unsupported_scope: unlisted case {case} for selected current stage"
                ));
            }
        };
        if fixture["scope"] != scope {
            return Err("selected scope differs from sealed stage policy".into());
        }
        let metadata: Value = serde_json::from_str(metadata).map_err(|error| error.to_string())?;
        // The file issuer owns the complete packet SHA gate. Raw codec units intentionally
        // reach native Seed/Program/context admission under this same selected stage policy.
        if fixture["seed"]["initial_boundary"] != metadata["initial_boundary"]
            || fixture["engine_recipes"]["native"] != metadata["native_recipe"]
            || fixture["stop"] != metadata["stop"]
        {
            return Err("selected initializer/budget/ordinal/binding/stop contract differs from sealed stage policy".into());
        }
        Ok(mode)
    }
}
