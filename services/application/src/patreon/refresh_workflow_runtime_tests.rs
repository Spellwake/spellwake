//! Actual local Temporal with production workflow/activities and controlled backend responses.
use super::*;
use crate::test_http::{Mock, Step};
use futures::TryStreamExt;
use std::collections::BTreeMap;
use temporalio_client::{
    WorkflowCancelOptions, WorkflowFetchHistoryOptions, WorkflowGetResultOptions,
};
use temporalio_common::protos::temporal::api::{
    common::v1::Payloads,
    history::v1::{HistoryEvent, history_event::Attributes},
};

pub(super) async fn client() -> Client {
    let namespace = std::env::var("SPELLWAKE_TEST_PATREON_TEMPORAL_NAMESPACE").unwrap();
    let suffix = namespace.strip_prefix("spellwake-patreon-test-").unwrap();
    assert!(suffix.len() == 32 && suffix.bytes().all(|b| b.is_ascii_hexdigit()));
    let address = std::env::var("SPELLWAKE_TEST_PATREON_TEMPORAL_ADDRESS").unwrap();
    TemporalConfig::new(&address, &namespace, None)
        .unwrap()
        .connect("spellwake-owned-patreon-test")
        .await
        .unwrap()
}
fn one(payloads: &Option<Payloads>) -> Value {
    let payloads = &payloads.as_ref().unwrap().payloads;
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0].metadata.get("encoding").unwrap(), b"json/plain");
    serde_json::from_slice(&payloads[0].data).unwrap()
}
fn task_value(value: Value) -> PatreonRefreshTask {
    serde_json::from_value(value).unwrap()
}
// Validate every application input/result payload in a fetched run, not a substring-only secret check.
pub(super) fn history(
    events: &[HistoryEvent],
    selected: &mut BTreeMap<Uuid, Uuid>,
    scheduled: &mut BTreeMap<i64, String>,
) -> (usize, usize) {
    let mut discovers = 0;
    let mut deliveries = 0;
    for event in events {
        match event.attributes.as_ref().unwrap() {
            Attributes::WorkflowExecutionStartedEventAttributes(e) => {
                let _: PatreonCurrentCursor = serde_json::from_value(one(&e.input)).unwrap();
                assert!(e.memo.as_ref().is_none_or(|m| m.fields.is_empty()));
            }
            Attributes::ActivityTaskScheduledEventAttributes(e) => {
                assert!(e.header.as_ref().is_none_or(|h| h.fields.is_empty()));
                let name = &e.activity_type.as_ref().unwrap().name;
                scheduled.insert(event.event_id, name.clone());
                match name.as_str() {
                    "spellwake-patreon-current-discover-v1" => {
                        let _: PatreonCurrentCursor =
                            serde_json::from_value(one(&e.input)).unwrap();
                        discovers += 1;
                    }
                    "spellwake-patreon-current-deliver-v1" => {
                        let task = task_value(one(&e.input));
                        assert_eq!(
                            selected.get(&task.association_id()),
                            Some(&task.request_id())
                        );
                        deliveries += 1;
                    }
                    _ => panic!("unexpected activity"),
                }
            }
            Attributes::ActivityTaskCompletedEventAttributes(e) => {
                match scheduled.get(&e.scheduled_event_id).unwrap().as_str() {
                    "spellwake-patreon-current-discover-v1" => {
                        let batch: PatreonCurrentBatch =
                            serde_json::from_value(one(&e.result)).unwrap();
                        assert!(batch.tasks.len() <= 1);
                        for task in batch.tasks {
                            if let Some(previous) =
                                selected.insert(task.association_id(), task.request_id())
                            {
                                assert_ne!(
                                    previous,
                                    task.request_id(),
                                    "a new sweep needs a new logical task"
                                );
                            }
                        }
                    }
                    "spellwake-patreon-current-deliver-v1" => {
                        let _: PatreonRefreshOutcome =
                            serde_json::from_value(one(&e.result)).unwrap();
                    }
                    _ => panic!("unexpected activity result"),
                }
            }
            Attributes::WorkflowExecutionContinuedAsNewEventAttributes(e) => {
                let _: PatreonCurrentCursor = serde_json::from_value(one(&e.input)).unwrap();
            }
            _ => {}
        }
    }
    (discovers, deliveries)
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires tools/test_local_patreon_workflow.py owned server/namespace"]
async fn production_temporal_continues_recorded_tasks_and_cancels_idle_timer_without_provider_io() {
    let mut steps = vec![];
    for i in 1..=21u128 {
        let association = Uuid::from_u128(i);
        let mut discover = Step::json(
            "/rest/v1/rpc/read_patreon_refresh_work",
            json!([association]),
        );
        discover.expect=Some(serde_json::to_vec(&json!({"p_campaign":"123","p_after":if i==1{None}else{Some(Uuid::from_u128(i-1))},"p_limit":1})).unwrap());
        steps.push(discover);
        steps.push(Step::json("/rest/v1/rpc/read_patreon_refresh_result",json!({"associationId":association,"sourceEventId":Uuid::from_u128(100+i),"outcome":"applied"})));
    }
    let mut tail = Step::json("/rest/v1/rpc/read_patreon_refresh_work", json!([]));
    tail.expect = Some(
        serde_json::to_vec(&json!({"p_campaign":"123","p_after":Uuid::from_u128(21),"p_limit":1}))
            .unwrap(),
    );
    steps.push(tail);
    let backend = Mock::new(steps);
    let provider = Mock::new(vec![]);
    let b = backend.base.clone();
    let p = provider.base.clone();
    let activities = tokio::task::spawn_blocking(move || {
        PatreonCurrentActivities::new(
            BackendConfig::new(&b, "sb_secret_test_only").unwrap(),
            PatreonConfig::fixture(&p),
        )
        .unwrap()
    })
    .await
    .unwrap();
    let client = client().await;
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let queue = format!("patreon-current-test-{}", Uuid::new_v4());
    let id = format!("patreon-current-test-{}", Uuid::new_v4());
    let options = worker_options(&queue, activities).unwrap();
    let mut worker = Worker::new(&runtime, client.clone(), options).unwrap();
    let shutdown = worker.shutdown_handle();
    let original = client
        .start_workflow(
            PatreonCurrentWorkflow::run,
            RecordPayload(PatreonCurrentCursor::default()),
            WorkflowStartOptions::new(queue, &id).build(),
        )
        .await
        .unwrap();
    let first_run = original
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap()
        .run_id()
        .to_owned();
    let latest = client.get_workflow_handle::<PatreonCurrentWorkflow>(&id);
    let exercise = async {
        let observed = tokio::time::timeout(Duration::from_secs(45), async {
            loop {
                let events: Vec<_> = latest
                    .fetch_history(WorkflowFetchHistoryOptions::default())
                    .try_collect()
                    .await
                    .unwrap();
                if events.iter().any(|e| {
                    matches!(
                        e.attributes,
                        Some(Attributes::TimerStartedEventAttributes(_))
                    )
                }) {
                    break events;
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        })
        .await;
        let cancellation = latest.cancel(WorkflowCancelOptions::default()).await;
        let terminal = tokio::time::timeout(
            Duration::from_secs(10),
            latest.get_result(WorkflowGetResultOptions::default()),
        )
        .await;
        shutdown();
        (observed, cancellation, terminal)
    };
    let (run, (observed, cancellation, terminal)) = tokio::join!(worker.run(), exercise);
    run.unwrap();
    cancellation.unwrap();
    assert!(terminal.unwrap().is_err());
    let latest_events = observed.unwrap();
    let first_events: Vec<_> = original
        .fetch_history(WorkflowFetchHistoryOptions::default())
        .try_collect()
        .await
        .unwrap();
    let latest_description = latest
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap();
    assert_ne!(first_run, latest_description.run_id());
    let canceled: Vec<_> = latest
        .fetch_history(WorkflowFetchHistoryOptions::default())
        .try_collect()
        .await
        .unwrap();
    assert!(canceled.iter().any(|e| matches!(
        e.attributes,
        Some(Attributes::WorkflowExecutionCanceledEventAttributes(_))
    )));
    let mut selected = BTreeMap::new();
    let mut scheduled = BTreeMap::new();
    assert_eq!(
        history(&first_events, &mut selected, &mut scheduled),
        (20, 20)
    );
    scheduled.clear();
    assert_eq!(
        history(&latest_events, &mut selected, &mut scheduled),
        (2, 1)
    );
    assert_eq!(selected.len(), 21);
    let continued = first_events
        .iter()
        .find_map(|e| match e.attributes.as_ref() {
            Some(Attributes::WorkflowExecutionContinuedAsNewEventAttributes(c)) => {
                Some(one(&c.input))
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(continued, json!({"after":Uuid::from_u128(20)}));
    let started = latest_events
        .iter()
        .find_map(|e| match e.attributes.as_ref() {
            Some(Attributes::WorkflowExecutionStartedEventAttributes(c)) => Some(one(&c.input)),
            _ => None,
        })
        .unwrap();
    assert_eq!(started, continued);
    assert!(latest_events.iter().any(|e| {
        match e.attributes.as_ref() {
            Some(Attributes::TimerStartedEventAttributes(t)) => t
                .start_to_fire_timeout
                .as_ref()
                .is_some_and(|d| d.seconds == 60 && d.nanos == 0),
            _ => false,
        }
    }));
    let requests = backend.finish();
    assert_eq!(requests.len(), 43);
    for (path, _, body) in requests {
        if path.ends_with("read_patreon_refresh_result") {
            let task: Value = serde_json::from_slice(&body).unwrap();
            let association: Uuid = serde_json::from_value(task["p_association"].clone()).unwrap();
            let request: Uuid = serde_json::from_value(task["p_request"].clone()).unwrap();
            assert_eq!(selected.get(&association), Some(&request));
        }
    }
    assert!(provider.finish().is_empty());
    let directory =
        std::path::PathBuf::from(std::env::var("SPELLWAKE_TEST_PATREON_ARTIFACT_DIR").unwrap());
    std::fs::write(directory.join("continuation-receipt.json"),serde_json::to_vec_pretty(&json!({"workflowId":id,"runs":[first_run,latest_description.run_id()],"tasks":21,"discoveries":22,"providerRequests":0,"timerSeconds":60,"cancelled":true,"payloadSchemasValidated":true})).unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires tools/test_local_patreon_workflow.py owned server restart protocol"]
async fn production_temporal_recovers_recorded_task_after_worker_and_server_restart() {
    use crate::{patreon::test_fixture as fixture, test_http::Credentials};
    use std::sync::Mutex;
    let association = Uuid::from_u128(42);
    let source = Uuid::from_u128(142);
    let ticket = json!({"version":1,"accountId":Uuid::from_u128(242),"associationId":association,"userId":"456","campaignId":"123","memberId":"member-abc","expectedSourceId":Uuid::from_u128(342),"anchorEventId":Uuid::from_u128(442),"generation":"1"});
    let prepared = ticket.clone();
    let accepted = Arc::new(Mutex::new(None::<Value>));
    let committed = accepted.clone();
    let mut lost_commit = Step::json("/rest/v1/rpc/commit_patreon_refresh", Value::Null);
    lost_commit.drop_ack = true;
    lost_commit.reply = Some(Box::new(move |body| {
        let input: Value = serde_json::from_slice(body).unwrap();
        assert_eq!(input["p_ticket"], ticket);
        assert_eq!(input["p_proof"]["memberId"], "member-abc");
        let request: Uuid = serde_json::from_value(input["p_request"].clone()).unwrap();
        assert!(!request.is_nil());
        *committed.lock().unwrap() = Some(input);
        b"null".to_vec()
    }));
    let mut lost_recovery = Step::json("/rest/v1/rpc/read_patreon_refresh_result", Value::Null);
    lost_recovery.drop_ack = true;
    let confirmed = accepted.clone();
    let mut final_recovery = Step::json("/rest/v1/rpc/read_patreon_refresh_result", Value::Null);
    final_recovery.reply = Some(Box::new(move |body| {
        let input: Value = serde_json::from_slice(body).unwrap();
        let saved = confirmed.lock().unwrap();
        let saved = saved.as_ref().unwrap();
        assert_eq!(
            input,
            json!({"p_association":association,"p_request":saved["p_request"]})
        );
        serde_json::to_vec(
            &json!({"associationId":association,"sourceEventId":source,"outcome":"applied"}),
        )
        .unwrap()
    }));
    let backend = Mock::with_request_wait(
        vec![
            Step::json(
                "/rest/v1/rpc/read_patreon_refresh_work",
                json!([association]),
            ),
            Step::json("/rest/v1/rpc/read_patreon_refresh_result", Value::Null),
            Step::json("/rest/v1/rpc/read_patreon_refresh", prepared),
            lost_commit,
            lost_recovery,
            final_recovery,
            Step::json("/rest/v1/rpc/read_patreon_refresh_work", json!([])),
        ],
        Duration::from_secs(240),
    );
    let mut member = Step::get(
        fixture::member_path(),
        200,
        serde_json::to_vec(&fixture::source()["member"]).unwrap(),
    );
    member.credentials = Credentials::Bearer("patreon_test_creator_synthetic".into());
    let provider = Mock::new(vec![member]);
    let configuration = || {
        let b = backend.base.clone();
        let p = provider.base.clone();
        tokio::task::spawn_blocking(move || {
            PatreonCurrentActivities::new(
                BackendConfig::new(&b, "sb_secret_test_only").unwrap(),
                PatreonConfig::fixture(&p),
            )
            .unwrap()
        })
    };
    let first_client = client().await;
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let queue = format!("patreon-current-test-{}", Uuid::new_v4());
    let id = format!("patreon-current-test-{}", Uuid::new_v4());
    let original = first_client
        .start_workflow(
            PatreonCurrentWorkflow::run,
            RecordPayload(PatreonCurrentCursor::default()),
            WorkflowStartOptions::new(&queue, &id).build(),
        )
        .await
        .unwrap();
    let run_id = original
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap()
        .run_id()
        .to_owned();
    let mut first_worker = Worker::new(
        &runtime,
        first_client.clone(),
        worker_options(&queue, configuration().await.unwrap()).unwrap(),
    )
    .unwrap();
    let shutdown = first_worker.shutdown_handle();
    let first_phase = async {
        let observed = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let description = original
                    .describe(WorkflowDescribeOptions::default())
                    .await
                    .unwrap();
                if let Some(pending) = description.raw().pending_activities.iter().find(|a| {
                    a.activity_type
                        .as_ref()
                        .is_some_and(|t| t.name == "spellwake-patreon-current-deliver-v1")
                        && a.last_failure.is_some()
                        && a.last_attempt_complete_time.is_some()
                        && a.next_attempt_schedule_time.is_some()
                }) {
                    assert_eq!(pending.maximum_attempts, 3);
                    assert_eq!(pending.current_retry_interval.as_ref().unwrap().seconds, 30);
                    assert_eq!(
                        pending.last_failure.as_ref().unwrap().message,
                        "patreon current refresh remains unresolved"
                    );
                    break pending.attempt;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
        shutdown();
        observed
    };
    let (worker_result, attempt) = tokio::join!(first_worker.run(), first_phase);
    worker_result.unwrap();
    let before_attempt = attempt.unwrap();
    drop(first_worker);
    let before: Vec<_> = original
        .fetch_history(WorkflowFetchHistoryOptions::default())
        .try_collect()
        .await
        .unwrap();
    let mut selected = BTreeMap::new();
    let mut scheduled = BTreeMap::new();
    assert_eq!(history(&before, &mut selected, &mut scheduled), (1, 1));
    let saved_request: Uuid =
        serde_json::from_value(accepted.lock().unwrap().as_ref().unwrap()["p_request"].clone())
            .unwrap();
    assert_eq!(selected.get(&association), Some(&saved_request));
    assert!(!before.iter().any(|e| {
        match e.attributes.as_ref() {
            Some(Attributes::ActivityTaskCompletedEventAttributes(a)) => scheduled
                .get(&a.scheduled_event_id)
                .is_some_and(|n| n == "spellwake-patreon-current-deliver-v1"),
            _ => false,
        }
    }));
    let directory =
        std::path::PathBuf::from(std::env::var("SPELLWAKE_TEST_PATREON_ARTIFACT_DIR").unwrap());
    let nonce = Uuid::new_v4();
    let marker = json!({"case":"recorded-task","nonce":nonce,"workflowId":id,"runId":run_id});
    let temporary = directory.join("restart-request.tmp");
    std::fs::write(&temporary, serde_json::to_vec(&marker).unwrap()).unwrap();
    std::fs::rename(temporary, directory.join("restart-request.json")).unwrap();
    let restart = tokio::time::timeout(Duration::from_secs(150), async {
        loop {
            match std::fs::read(directory.join("restart-done.json")) {
                Ok(bytes) => break serde_json::from_slice::<Value>(&bytes).unwrap(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    tokio::time::sleep(Duration::from_millis(25)).await
                }
                Err(e) => panic!("restart acknowledgment read failed: {e}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(restart["nonce"], json!(nonce));
    assert_eq!(restart["healthReadbackSucceeded"], true);
    assert_ne!(restart["oldPid"], restart["newPid"]);
    let second_client = client().await;
    let handle = second_client.get_workflow_handle::<PatreonCurrentWorkflow>(&id);
    assert_eq!(
        handle
            .describe(WorkflowDescribeOptions::default())
            .await
            .unwrap()
            .run_id(),
        run_id
    );
    let mut second_worker = Worker::new(
        &runtime,
        second_client.clone(),
        worker_options(&queue, configuration().await.unwrap()).unwrap(),
    )
    .unwrap();
    let shutdown = second_worker.shutdown_handle();
    let second_phase = async {
        let observed = tokio::time::timeout(Duration::from_secs(65), async {
            loop {
                let events: Vec<_> = handle
                    .fetch_history(WorkflowFetchHistoryOptions::default())
                    .try_collect()
                    .await
                    .unwrap();
                if events.iter().any(|e| {
                    matches!(
                        e.attributes,
                        Some(Attributes::TimerStartedEventAttributes(_))
                    )
                }) {
                    break events;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
        let canceled = handle.cancel(WorkflowCancelOptions::default()).await;
        let terminal = tokio::time::timeout(
            Duration::from_secs(10),
            handle.get_result(WorkflowGetResultOptions::default()),
        )
        .await;
        shutdown();
        (observed, canceled, terminal)
    };
    let (run, (observed, canceled, terminal)) = tokio::join!(second_worker.run(), second_phase);
    run.unwrap();
    canceled.unwrap();
    assert!(terminal.unwrap().is_err());
    let events = observed.unwrap();
    let mut selected = BTreeMap::new();
    let mut scheduled = BTreeMap::new();
    assert_eq!(history(&events, &mut selected, &mut scheduled), (2, 1));
    assert_eq!(selected.get(&association), Some(&saved_request));
    let delivery_schedule = events
        .iter()
        .find_map(|e| match e.attributes.as_ref() {
            Some(Attributes::ActivityTaskScheduledEventAttributes(a))
                if a.activity_type.as_ref().unwrap().name
                    == "spellwake-patreon-current-deliver-v1" =>
            {
                Some(e.event_id)
            }
            _ => None,
        })
        .unwrap();
    let delivered = events
        .iter()
        .find_map(|e| match e.attributes.as_ref() {
            Some(Attributes::ActivityTaskCompletedEventAttributes(a))
                if a.scheduled_event_id == delivery_schedule =>
            {
                Some(one(&a.result))
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        delivered,
        json!({"associationId":association,"sourceEventId":source,"outcome":"applied"})
    );
    let attempt = events
        .iter()
        .find_map(|e| match e.attributes.as_ref() {
            Some(Attributes::ActivityTaskStartedEventAttributes(a))
                if a.scheduled_event_id == delivery_schedule =>
            {
                Some(a.attempt)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(attempt, 2);
    let calls = backend.finish();
    assert_eq!(calls.len(), 7);
    let first_read: Value = serde_json::from_slice(&calls[1].2).unwrap();
    for i in [4, 5] {
        assert_eq!(
            serde_json::from_slice::<Value>(&calls[i].2).unwrap(),
            first_read
        );
    }
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[3].2).unwrap()["p_request"],
        json!(saved_request)
    );
    assert_eq!(provider.finish().len(), 1);
    std::fs::write(directory.join("restart-receipt.json"),serde_json::to_vec_pretty(&json!({"workflowId":id,"runs":[run_id],"associationId":association,"requestId":saved_request,"beforeAttempt":before_attempt,"completedAttempt":attempt,"workerInstances":2,"serverRestart":restart,"backendCalls":7,"providerRequests":1,"deliverySchedules":1,"sameRecordedRequest":true,"source":"controlled backend acceptance, not managed SQL"})).unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires tools/test_local_patreon_workflow.py owned uninterrupted server"]
async fn production_temporal_exhaustion_preserves_discovery_cursor_and_revisits_failed_delivery() {
    use std::sync::Mutex;
    let initial = Uuid::from_u128(500);
    let a = Uuid::from_u128(1000);
    let b = Uuid::from_u128(2000);
    let discovery_times = Arc::new(Mutex::new(Vec::<std::time::Instant>::new()));
    let delivery_times = Arc::new(Mutex::new(Vec::<std::time::Instant>::new()));
    let discover = |after: Option<Uuid>, rows: Value| {
        let mut step = Step::json("/rest/v1/rpc/read_patreon_refresh_work", rows);
        step.expect = Some(
            serde_json::to_vec(&json!({"p_campaign":"123","p_after":after,"p_limit":1})).unwrap(),
        );
        step
    };
    let mut steps = vec![];
    for _ in 0..3 {
        let observed = discovery_times.clone();
        let mut step = discover(Some(initial), Value::Null);
        step.status = 503;
        step.reply = Some(Box::new(move |_| {
            observed.lock().unwrap().push(std::time::Instant::now());
            eprintln!("patreon_acceptance phase=discovery_http_failure");
            b"null".to_vec()
        }));
        steps.push(step);
    }
    steps.push(discover(Some(initial), json!([a])));
    for _ in 0..3 {
        let observed = delivery_times.clone();
        let mut step = Step::json("/rest/v1/rpc/read_patreon_refresh_result", Value::Null);
        step.status = 503;
        step.reply = Some(Box::new(move |_| {
            observed.lock().unwrap().push(std::time::Instant::now());
            eprintln!("patreon_acceptance phase=delivery_http_failure");
            b"null".to_vec()
        }));
        steps.push(step);
    }
    steps.push(discover(Some(a), json!([b])));
    steps.push(Step::json(
        "/rest/v1/rpc/read_patreon_refresh_result",
        json!({"associationId":b,"sourceEventId":Uuid::from_u128(2100),"outcome":"applied"}),
    ));
    steps.push(discover(Some(b), json!([])));
    steps.push(discover(None, json!([a])));
    steps.push(Step::json(
        "/rest/v1/rpc/read_patreon_refresh_result",
        json!({"associationId":a,"sourceEventId":Uuid::from_u128(1100),"outcome":"applied"}),
    ));
    steps.push(discover(Some(a), json!([])));
    let backend = Mock::with_request_wait(steps, Duration::from_secs(240));
    let provider = Mock::new(vec![]);
    let backend_base = backend.base.clone();
    let provider_base = provider.base.clone();
    let activities = tokio::task::spawn_blocking(move || {
        PatreonCurrentActivities::new(
            BackendConfig::new(&backend_base, "sb_secret_test_only").unwrap(),
            PatreonConfig::fixture(&provider_base),
        )
        .unwrap()
    })
    .await
    .unwrap();
    let client = client().await;
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let queue = format!("patreon-current-test-{}", Uuid::new_v4());
    let id = format!("patreon-current-test-{}", Uuid::new_v4());
    let handle = client
        .start_workflow(
            PatreonCurrentWorkflow::run,
            RecordPayload(PatreonCurrentCursor {
                after: Some(initial),
            }),
            WorkflowStartOptions::new(&queue, &id).build(),
        )
        .await
        .unwrap();
    let mut worker = Worker::new(
        &runtime,
        client,
        worker_options(&queue, activities).unwrap(),
    )
    .unwrap();
    let shutdown = worker.shutdown_handle();
    let exercise = async {
        let observed = tokio::time::timeout(Duration::from_secs(335), async {
            loop {
                let events: Vec<_> = handle
                    .fetch_history(WorkflowFetchHistoryOptions::default())
                    .try_collect()
                    .await
                    .unwrap();
                let fired = events
                    .iter()
                    .filter(|e| {
                        matches!(e.attributes, Some(Attributes::TimerFiredEventAttributes(_)))
                    })
                    .count();
                let started = events
                    .iter()
                    .filter(|e| {
                        matches!(
                            e.attributes,
                            Some(Attributes::TimerStartedEventAttributes(_))
                        )
                    })
                    .count();
                if fired == 2 && started == 3 {
                    break events;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
        let canceled = handle.cancel(WorkflowCancelOptions::default()).await;
        let terminal = tokio::time::timeout(
            Duration::from_secs(10),
            handle.get_result(WorkflowGetResultOptions::default()),
        )
        .await;
        shutdown();
        (observed, canceled, terminal)
    };
    let (run, (observed, canceled, terminal)) = tokio::join!(worker.run(), exercise);
    run.unwrap();
    canceled.unwrap();
    assert!(terminal.unwrap().is_err());
    let events = observed.unwrap();
    let terminal_events: Vec<_> = handle
        .fetch_history(WorkflowFetchHistoryOptions::default())
        .try_collect()
        .await
        .unwrap();
    let final_timer = events
        .iter()
        .rfind(|e| {
            matches!(
                e.attributes,
                Some(Attributes::TimerStartedEventAttributes(_))
            )
        })
        .unwrap()
        .event_id;
    assert!(terminal_events.iter().any(|e| matches!(
        e.attributes,
        Some(Attributes::WorkflowExecutionCanceledEventAttributes(_))
    )));
    assert!(terminal_events.iter().any(|e| match e.attributes.as_ref() {
        Some(Attributes::TimerCanceledEventAttributes(t)) => t.started_event_id == final_timer,
        _ => false,
    }));
    let mut selected = BTreeMap::new();
    let mut scheduled = BTreeMap::new();
    assert_eq!(history(&events, &mut selected, &mut scheduled), (6, 3));
    let failed: Vec<_> = events
        .iter()
        .filter_map(|e| match e.attributes.as_ref() {
            Some(Attributes::ActivityTaskFailedEventAttributes(a)) => Some(a),
            _ => None,
        })
        .collect();
    assert_eq!(failed.len(), 2);
    for failed in &failed {
        use temporalio_common::protos::temporal::api::{
            enums::v1::RetryState, failure::v1::failure::FailureInfo,
        };
        assert_eq!(
            failed.retry_state,
            RetryState::MaximumAttemptsReached as i32
        );
        let failure = failed.failure.as_ref().unwrap();
        assert_eq!(
            failure.message,
            "patreon current refresh remains unresolved"
        );
        assert!(failure.cause.is_none());
        assert!(failure.encoded_attributes.is_none());
        assert!(failure.stack_trace.is_empty());
        let Some(FailureInfo::ApplicationFailureInfo(application)) = failure.failure_info.as_ref()
        else {
            panic!("unexpected failure type");
        };
        assert!(!application.non_retryable);
        assert!(application.details.is_none());
        let started = events
            .iter()
            .find_map(|e| match e.attributes.as_ref() {
                Some(Attributes::ActivityTaskStartedEventAttributes(s))
                    if s.scheduled_event_id == failed.scheduled_event_id =>
                {
                    Some(s)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(started.attempt, 3);
    }
    let mut timers = BTreeMap::new();
    let mut timer_seconds = vec![];
    for event in &events {
        match event.attributes.as_ref() {
            Some(Attributes::TimerStartedEventAttributes(t)) => {
                assert_eq!(t.start_to_fire_timeout.as_ref().unwrap().seconds, 60);
                timers.insert(event.event_id, event.event_time.as_ref().unwrap());
            }
            Some(Attributes::TimerFiredEventAttributes(t)) => {
                let begin = timers.get(&t.started_event_id).unwrap();
                let end = event.event_time.as_ref().unwrap();
                let elapsed = (end.seconds - begin.seconds) as f64
                    + (f64::from(end.nanos) - f64::from(begin.nanos)) / 1e9;
                assert!(elapsed >= 59.0, "timer fired early: {elapsed}");
                timer_seconds.push(elapsed);
            }
            _ => {}
        }
    }
    assert_eq!(timer_seconds.len(), 2);
    let intervals = |times: &Arc<Mutex<Vec<std::time::Instant>>>| {
        let times = times.lock().unwrap();
        assert_eq!(times.len(), 3);
        let intervals: Vec<_> = times
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]).as_secs_f64())
            .collect();
        assert!(
            intervals[0] >= 29.0,
            "initial retry too early: {intervals:?}"
        );
        assert!(
            intervals[1] >= 59.0,
            "second retry too early: {intervals:?}"
        );
        intervals
    };
    let discovery_intervals = intervals(&discovery_times);
    let delivery_intervals = intervals(&delivery_times);
    let calls = backend.finish();
    assert_eq!(calls.len(), 13);
    let first: Value = serde_json::from_slice(&calls[4].2).unwrap();
    for i in [5, 6] {
        assert_eq!(serde_json::from_slice::<Value>(&calls[i].2).unwrap(), first);
    }
    assert_eq!(first["p_association"], json!(a));
    let revisited: Value = serde_json::from_slice(&calls[11].2).unwrap();
    assert_eq!(revisited["p_association"], json!(a));
    assert_ne!(first["p_request"], revisited["p_request"]);
    assert_eq!(provider.finish().len(), 0);
    let directory =
        std::path::PathBuf::from(std::env::var("SPELLWAKE_TEST_PATREON_ARTIFACT_DIR").unwrap());
    let run_id = handle
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap()
        .run_id()
        .to_owned();
    std::fs::write(directory.join("fairness-receipt.json"),serde_json::to_vec_pretty(&json!({"workflowId":id,"runs":[run_id],"backendCalls":13,"providerRequests":0,"discoverySchedules":6,"deliverySchedules":3,"exhaustedActivities":2,"attemptsPerExhaustedActivity":3,"discoveryCursorRetained":true,"failedDeliveryAdvanced":true,"fullSweepResetAndRevisit":true,"revisitUsesNewLogicalRequest":true,"discoveryRetryIntervalsSeconds":discovery_intervals,"deliveryRetryIntervalsSeconds":delivery_intervals,"firedTimerSeconds":timer_seconds,"cancelled":true,"finalTimerCanceled":true,"scope":"uninterrupted local server; restart delay persistence is separate"})).unwrap()).unwrap();
}
