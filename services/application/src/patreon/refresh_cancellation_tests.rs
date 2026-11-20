//! Real SDK cancellation and heartbeat observations while production activity HTTP remains gated.
use super::runtime::{client, history};
use super::*;
use crate::patreon::member_activity_test_support::{CancellationObservation, ReleaseGate};
use crate::test_http::{Mock, Step};
use futures::TryStreamExt;
use std::{
    collections::BTreeMap,
    sync::{Mutex, mpsc},
};
use temporalio_client::{
    WorkflowCancelOptions, WorkflowFetchHistoryOptions, WorkflowGetResultOptions,
};
use temporalio_common::data_converters::ActivitySerializationContext;
use temporalio_common::protos::temporal::api::history::v1::history_event::Attributes;

// Own the Tokio runtime outside the async test body. Dropping it drains every
// spawn_blocking closure before closing HTTP guards, including a detached canceled request.
#[test]
#[ignore = "requires tools/test_local_patreon_workflow.py owned server/namespace"]
fn production_temporal_cancellation_observes_context_and_blocks_later_provider_work() {
    for cancel_active in [false, true] {
        let association = Uuid::from_u128(if cancel_active { 502 } else { 501 });
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered_tx));
        let (release_tx, release_rx) = mpsc::channel();
        let mut prepared = Step::json("/rest/v1/rpc/read_patreon_refresh", Value::Null);
        let body = if cancel_active {
            json!({"version":1,"accountId":Uuid::from_u128(602),"associationId":association,"userId":"456","campaignId":"123","memberId":"member-abc","expectedSourceId":Uuid::from_u128(702),"anchorEventId":Uuid::from_u128(802),"generation":"1"})
        } else {
            Value::Null
        };
        prepared.reply = Some(Box::new(move |_| {
            if let Some(sender) = entered.lock().unwrap().take() {
                let _ = sender.send(());
            }
            release_rx
                .recv_timeout(Duration::from_secs(45))
                .expect("bounded gated preparation release");
            serde_json::to_vec(&body).unwrap()
        }));
        let mut steps = vec![
            Step::json(
                "/rest/v1/rpc/read_patreon_refresh_work",
                json!([association]),
            ),
            Step::json("/rest/v1/rpc/read_patreon_refresh_result", Value::Null),
            prepared,
        ];
        if !cancel_active {
            steps.push(Step::json(
                "/rest/v1/rpc/read_patreon_refresh_work",
                json!([]),
            ));
        }
        let backend = Mock::new(steps);
        let provider = Mock::new(vec![]);
        let backend_base = backend.base.clone();
        let provider_base = provider.base.clone();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let receipt=runtime.block_on(async move {
            let activities=tokio::task::spawn_blocking(move||PatreonCurrentActivities::new(BackendConfig::new(&backend_base,"sb_secret_test_only").unwrap(),PatreonConfig::fixture(&provider_base)).unwrap()).await.unwrap();
            let client=client().await;let sdk=Runtime::from_current_tokio(Default::default()).unwrap();
            let queue=format!("patreon-current-test-{}",Uuid::new_v4());let id=format!("patreon-current-test-{}",Uuid::new_v4());
            let handle=client.start_workflow(PatreonCurrentWorkflow::run,RecordPayload(PatreonCurrentCursor::default()),WorkflowStartOptions::new(&queue,&id).build()).await.unwrap();
            let observed_cancel=Arc::new(AtomicBool::new(false));let notification=Arc::new(tokio::sync::Notify::new());
            let mut options=worker_options(&queue,activities).unwrap();
            options.activity_inbound_interceptor(CancellationObservation{activity_name:"spellwake-patreon-current-deliver-v1",seen:observed_cancel.clone(),notify:notification.clone()});
            let mut worker=Worker::new(&sdk,client,options).unwrap();
            let shutdown=worker.shutdown_handle();
            let exercise=async {
                let mut gate=ReleaseGate(Some(release_tx));
                let observed=tokio::time::timeout(Duration::from_secs(35),async {
                    entered_rx.await.unwrap();
                    if cancel_active {
                        handle.cancel(WorkflowCancelOptions::default()).await.unwrap();
                        while !observed_cancel.load(Ordering::Acquire){notification.notified().await;}
                        json!({"activityReturnedCancelled":true,"heartbeatSnapshot":null})
                    } else {
                        let converter=DataConverter::default();
                        let context=SerializationContextData::Activity(ActivitySerializationContext::new());
                        let canonical=converter.to_payloads(&context,&()).await.unwrap();
                        loop {
                            let description=handle.describe(WorkflowDescribeOptions::default()).await.unwrap();
                            if let Some(activity)=description.raw().pending_activities.iter().find(|a|a.activity_type.as_ref().is_some_and(|t|t.name=="spellwake-patreon-current-deliver-v1") && a.last_heartbeat_time.is_some()) {
                                let actual=activity.heartbeat_details.as_ref().map_or_else(Vec::new,|p|p.payloads.clone());
                                assert_eq!(actual,canonical);assert_eq!(activity.attempt,1);
                                break json!({"activityReturnedCancelled":false,"heartbeatSnapshot":{"lastHeartbeatRecorded":true,"canonicalUnitPayloadCount":canonical.len(),"payloads":actual.iter().map(|p|json!({"metadata":p.metadata,"data":p.data})).collect::<Vec<_>>()}});
                            }
                            tokio::time::sleep(Duration::from_millis(25)).await;
                        }
                    }
                }).await;
                // For cancellation this release follows the actual production activity's
                // Cancelled return, so StopOnDrop has already fenced subsequent source IO.
                gate.release();
                let tail=if !cancel_active && observed.is_ok() {
                    tokio::time::timeout(Duration::from_secs(10),async {
                        loop {
                            let events:Vec<_>=handle.fetch_history(WorkflowFetchHistoryOptions::default()).try_collect().await.unwrap();
                            if events.iter().any(|e|matches!(e.attributes,Some(Attributes::TimerStartedEventAttributes(_)))) {break;}
                            tokio::time::sleep(Duration::from_millis(25)).await;
                        }
                    }).await
                } else {Ok(())};
                let cancellation=handle.cancel(WorkflowCancelOptions::default()).await;
                let terminal=tokio::time::timeout(Duration::from_secs(10),handle.get_result(WorkflowGetResultOptions::default())).await;
                shutdown();(observed,tail,cancellation,terminal)
            };
            let (run,(observed,tail,cancellation,terminal))=tokio::join!(worker.run(),exercise);run.unwrap();tail.unwrap();cancellation.unwrap();assert!(terminal.unwrap().is_err());
            let observation=observed.unwrap();
            let events:Vec<_>=handle.fetch_history(WorkflowFetchHistoryOptions::default()).try_collect().await.unwrap();
            assert!(events.iter().any(|e|matches!(e.attributes,Some(Attributes::WorkflowExecutionCanceledEventAttributes(_)))));
            let mut selected=BTreeMap::new();let mut scheduled=BTreeMap::new();assert_eq!(history(&events,&mut selected,&mut scheduled),(if cancel_active{1}else{2},1));
            for event in &events {if let Some(Attributes::ActivityTaskCanceledEventAttributes(canceled))=event.attributes.as_ref(){assert!(canceled.details.as_ref().is_none_or(|p|p.payloads.is_empty()));}}
            if cancel_active {assert!(observed_cancel.load(Ordering::Acquire));assert!(!events.iter().any(|e|matches!(e.attributes,Some(Attributes::TimerStartedEventAttributes(_)))));}
            let run_id=handle.describe(WorkflowDescribeOptions::default()).await.unwrap().run_id().to_owned();
            json!({"workflowId":id,"runs":[run_id],"cancelActive":cancel_active,"observed":observation,"backendCalls":if cancel_active{3}else{4},"providerRequests":0,"workflowCanceled":true,"runtimeDrainRequiredBeforeFixtureFinish":true})
        });
        // Tokio1.54.1 runtime Drop waits for running blocking functions to return.
        // Do not use shutdown_timeout/background: either could leave late source IO unobserved.
        drop(runtime);
        assert_eq!(backend.finish().len(), if cancel_active { 3 } else { 4 });
        assert!(provider.finish().is_empty());
        let name = if cancel_active {
            "cancellation"
        } else {
            "heartbeat"
        };
        let directory =
            std::path::PathBuf::from(std::env::var("SPELLWAKE_TEST_PATREON_ARTIFACT_DIR").unwrap());
        std::fs::write(
            directory.join(format!("{name}-receipt.json")),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
    }
}
