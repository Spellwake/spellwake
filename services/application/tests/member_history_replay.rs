//! Opt-in replay of retained real pre-refactor current histories; no server or activity IO.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use spellwake_application::patreon::refresh_workflow::PatreonCurrentWorkflow;
use std::path::PathBuf;
use temporalio_client::WorkflowHistory;
use temporalio_common::protos::temporal::api::history::v1::{History, history_event::Attributes};
use temporalio_sdk::workflow_replayer::{
    WorkflowReplayError, WorkflowReplayFailure, WorkflowReplayer, WorkflowReplayerOptions,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    histories: Vec<Retained>,
}
#[derive(Deserialize)]
struct Retained {
    path: String,
    sha256: String,
    events: usize,
}

#[tokio::test]
#[ignore = "requires retained real-history manifest; never synthesizes a passing history"]
async fn existing_current_histories_replay_and_mutated_activity_identity_is_rejected() {
    let manifest = PathBuf::from(
        std::env::var("SPELLWAKE_TEST_MEMBER_HISTORY_MANIFEST")
            .expect("explicit retained-history manifest"),
    );
    assert!(manifest.is_absolute());
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    assert!(!manifest.histories.is_empty());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let replayer = WorkflowReplayer::new(
        WorkflowReplayerOptions::new()
            .register_workflow::<PatreonCurrentWorkflow>()
            .unwrap()
            .build(),
    )
    .unwrap();
    let mut negative_checked = false;
    for retained in manifest.histories {
        let bytes = std::fs::read(root.join(&retained.path)).unwrap();
        assert_eq!(
            Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            retained.sha256
        );
        let mut events = WorkflowHistory::from_json(&bytes)
            .unwrap()
            .into_events()
            .await
            .unwrap();
        assert_eq!(events.len(), retained.events);
        let mut original = History::default();
        original.events.extend(events.clone());
        replayer.replay_workflow(original.into()).await.unwrap();
        println!(
            "replayed {} events={} sha256={}",
            retained.path, retained.events, retained.sha256
        );
        if !negative_checked {
            for event in &mut events {
                if let Some(Attributes::ActivityTaskScheduledEventAttributes(attributes)) =
                    &mut event.attributes
                {
                    attributes.activity_type.as_mut().unwrap().name =
                        "test-only-incompatible-activity".into();
                    negative_checked = true;
                    break;
                }
            }
            if negative_checked {
                let mut changed = History::default();
                changed.events.extend(events);
                assert!(matches!(
                    replayer.replay_workflow(changed.into()).await,
                    Err(WorkflowReplayError::Replay(
                        WorkflowReplayFailure::Nondeterminism { .. }
                    ))
                ));
            }
        }
    }
    assert!(
        negative_checked,
        "must exercise a scheduled activity mismatch"
    );
    assert!(WorkflowHistory::from_json(b"not a history").is_err());
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExportJob {
    address: String,
    namespace: String,
    output_directory: PathBuf,
    histories: Vec<ExportHistory>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExportHistory {
    original: PathBuf,
    sha256: String,
    workflow_id: String,
    run_id: String,
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
/// Compare every retained event identity/type/time and each durable application payload.
fn check_prefix(
    raw: &serde_json::Value,
    events: &[temporalio_common::protos::temporal::api::history::v1::HistoryEvent],
) {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use temporalio_common::protos::temporal::api::enums::v1::EventType;
    let raw = raw["events"].as_array().unwrap();
    assert!(events.len() >= raw.len());
    for (old, event) in raw.iter().zip(events) {
        assert_eq!(
            old["eventId"].as_str().unwrap().parse::<i64>().unwrap(),
            event.event_id
        );
        assert_eq!(
            old["eventType"],
            EventType::try_from(event.event_type).unwrap().as_str_name()
        );
        let time =
            chrono::DateTime::parse_from_rfc3339(old["eventTime"].as_str().unwrap()).unwrap();
        assert_eq!(event.event_time.as_ref().unwrap().seconds, time.timestamp());
        assert_eq!(
            u32::try_from(event.event_time.as_ref().unwrap().nanos).unwrap(),
            time.timestamp_subsec_nanos()
        );
        let (wire, payloads) = match event.attributes.as_ref().unwrap() {
            Attributes::WorkflowExecutionStartedEventAttributes(e) => {
                assert_eq!(
                    old["workflowExecutionStartedEventAttributes"]["workflowType"]["name"],
                    e.workflow_type.as_ref().unwrap().name
                );
                (
                    &old["workflowExecutionStartedEventAttributes"]["input"],
                    e.input.as_ref(),
                )
            }
            Attributes::ActivityTaskScheduledEventAttributes(e) => {
                assert_eq!(
                    old["activityTaskScheduledEventAttributes"]["activityType"]["name"],
                    e.activity_type.as_ref().unwrap().name
                );
                (
                    &old["activityTaskScheduledEventAttributes"]["input"],
                    e.input.as_ref(),
                )
            }
            Attributes::ActivityTaskCompletedEventAttributes(e) => (
                &old["activityTaskCompletedEventAttributes"]["result"],
                e.result.as_ref(),
            ),
            Attributes::WorkflowExecutionContinuedAsNewEventAttributes(e) => (
                &old["workflowExecutionContinuedAsNewEventAttributes"]["input"],
                e.input.as_ref(),
            ),
            _ => continue,
        };
        if let Some(payloads) = payloads {
            let entries = wire["payloads"].as_array().unwrap();
            assert_eq!(entries.len(), payloads.payloads.len());
            for (entry, payload) in entries.iter().zip(&payloads.payloads) {
                assert_eq!(
                    STANDARD.decode(entry["data"].as_str().unwrap()).unwrap(),
                    payload.data
                );
                let metadata = entry["metadata"].as_object().unwrap();
                assert_eq!(metadata.len(), payload.metadata.len());
                for (name, value) in metadata {
                    assert_eq!(
                        STANDARD.decode(value.as_str().unwrap()).unwrap(),
                        *payload.metadata.get(name).unwrap()
                    );
                }
            }
        } else {
            assert!(wire.is_null());
        }
    }
}
#[tokio::test]
#[ignore = "requires isolated owned copies of actual retained Temporal stores, never a remote service"]
async fn export_retained_owned_histories_through_pinned_sdk() {
    use spellwake_application::temporal_runtime::TemporalConfig;
    use temporalio_client::{WorkflowExecutionInfo, WorkflowFetchHistoryOptions};
    let job = PathBuf::from(
        std::env::var("SPELLWAKE_TEST_MEMBER_HISTORY_EXPORT")
            .expect("explicit owned-store export job"),
    );
    assert!(job.is_absolute());
    let job: ExportJob = serde_json::from_slice(&std::fs::read(job).unwrap()).unwrap();
    assert!(job.address.starts_with("http://127.0.0.1:"));
    let suffix = job
        .namespace
        .strip_prefix("spellwake-patreon-test-")
        .unwrap();
    assert!(suffix.len() == 32 && suffix.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(job.output_directory.is_absolute());
    let client = TemporalConfig::new(&job.address, &job.namespace, None)
        .unwrap()
        .connect("spellwake-owned-history-export")
        .await
        .unwrap();
    let mut exported = vec![];
    for (index, old) in job.histories.into_iter().enumerate() {
        let bytes = std::fs::read(&old.original).unwrap();
        assert_eq!(hash(&bytes), old.sha256);
        assert!(
            old.workflow_id.starts_with("patreon-current-test-")
                || old.workflow_id == "spellwake-patreon-current-refresh-v1"
        );
        assert_eq!(
            uuid::Uuid::parse_str(&old.run_id).unwrap().to_string(),
            old.run_id
        );
        let handle = WorkflowExecutionInfo::builder()
            .namespace(&job.namespace)
            .workflow_id(&old.workflow_id)
            .run_id(&old.run_id)
            .build()
            .bind_untyped(client.clone());
        let mut events = handle
            .fetch_history(WorkflowFetchHistoryOptions::default())
            .into_events()
            .await
            .unwrap();
        let raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        check_prefix(&raw, &events);
        // Preserve the complete fetched run and replay only the prefix retained before this refactor.
        let mut full = History::default();
        full.events.extend(events.clone());
        let full_bytes = WorkflowHistory::from(full).to_json().await.unwrap();
        std::fs::write(
            job.output_directory
                .join(format!("sdk-full-history-{index}.json")),
            &full_bytes,
        )
        .unwrap();
        events.truncate(raw["events"].as_array().unwrap().len());
        let count = events.len();
        let mut history = History::default();
        history.events.extend(events);
        let output = WorkflowHistory::from(history).to_json().await.unwrap();
        let path = job
            .output_directory
            .join(format!("sdk-history-{index}.json"));
        std::fs::write(&path, &output).unwrap();
        exported.push(serde_json::json!({"path":path,"sha256":hash(&output),"events":count,"original":old.original,"originalSha256":old.sha256,"originalEvents":raw["events"].as_array().unwrap().len(),"fullFetchedSha256":hash(&full_bytes),"workflowId":old.workflow_id,"runId":old.run_id,"prefixIdentityTimeAndApplicationPayloadsMatch":true}));
    }
    std::fs::write(
        job.output_directory.join("sdk-manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"histories":exported})).unwrap(),
    )
    .unwrap();
}
