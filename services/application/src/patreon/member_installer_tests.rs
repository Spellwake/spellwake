//! Actual server readback with controlled SDK interception after/before a real start RPC.
use super::refresh_workflow;
use serde_json::json;
use std::sync::{Arc, atomic::Ordering};
use temporalio_client::{
    Client, UntypedWorkflow, WorkflowDescribeOptions, WorkflowExecutionStatus,
};

#[derive(Clone, Copy)]
pub(super) enum Domain {
    Current,
}
impl Domain {
    fn workflow(self) -> &'static str {
        match self {
            Self::Current => refresh_workflow::PATREON_CURRENT_WORKFLOW,
        }
    }
    async fn install(self, client: &Client) -> Result<(), ()> {
        match self {
            Self::Current => refresh_workflow::ensure_current_refresh(client)
                .await
                .map_err(|_| ()),
        }
    }
}
use futures::{TryStreamExt, future::BoxFuture};
use std::sync::{Mutex, atomic::AtomicUsize};
use temporalio_client::{
    ClientInterceptor, Next, StartWorkflowInput, StartWorkflowOutput, WorkflowFetchHistoryOptions,
    WorkflowTerminateOptions, errors::WorkflowStartError,
};
use temporalio_common::protos::temporal::api::history::v1::history_event::Attributes;

#[derive(Clone, Copy)]
enum StartCase {
    HideAccepted,
    WrongType,
    TerminateAccepted,
}
struct StartProbe {
    base: Client,
    workflow: &'static str,
    mode: StartCase,
    calls: AtomicUsize,
    accepted_run: Mutex<Option<String>>,
}
impl ClientInterceptor for StartProbe {
    fn start_workflow<'a>(
        &'a self,
        mut input: StartWorkflowInput,
        next: Next<
            'a,
            StartWorkflowInput,
            BoxFuture<'a, Result<StartWorkflowOutput, WorkflowStartError>>,
        >,
    ) -> BoxFuture<'a, Result<StartWorkflowOutput, WorkflowStartError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            assert_eq!(input.options.workflow_id, self.workflow);
            if matches!(self.mode, StartCase::WrongType) {
                input.workflow_type = "spellwake-patreon-wrong-type-test-v1".into();
            }
            let output = next.run(input).await?;
            *self.accepted_run.lock().unwrap() = Some(output.run_id.clone());
            if matches!(self.mode, StartCase::TerminateAccepted) {
                self.base
                    .get_workflow_handle::<UntypedWorkflow>(self.workflow)
                    .terminate(
                        WorkflowTerminateOptions::builder()
                            .reason("owned installer race fixture")
                            .build(),
                    )
                    .await
                    .unwrap();
            }
            if matches!(
                self.mode,
                StartCase::HideAccepted | StartCase::TerminateAccepted
            ) {
                return Err(WorkflowStartError::Rpc(
                    temporalio_client::tonic::Status::unavailable(
                        "controlled accepted acknowledgment hidden",
                    ),
                ));
            }
            Ok(output)
        })
    }
}
fn intercepted(
    base: &Client,
    workflow: &'static str,
    mode: StartCase,
) -> (Client, Arc<StartProbe>) {
    let probe = Arc::new(StartProbe {
        base: base.clone(),
        workflow,
        mode,
        calls: AtomicUsize::new(0),
        accepted_run: Mutex::new(None),
    });
    let mut options = base.options().clone();
    options.client_interceptors = vec![probe.clone()];
    (
        Client::new(base.connection().clone(), options).unwrap(),
        probe,
    )
}
pub(super) async fn exercise(base: Client, domain: Domain) {
    let workflow = domain.workflow();
    let (hidden, ack) = intercepted(&base, workflow, StartCase::HideAccepted);
    domain.install(&hidden).await.unwrap();
    assert_eq!(ack.calls.load(Ordering::Acquire), 1);
    let handle = base.get_workflow_handle::<UntypedWorkflow>(workflow);
    let first = handle
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap();
    assert_eq!(first.workflow_type(), workflow);
    assert_eq!(first.status(), WorkflowExecutionStatus::Running);
    assert_eq!(
        ack.accepted_run.lock().unwrap().as_deref(),
        Some(first.run_id())
    );
    let first_run = first.run_id().to_owned();
    domain.install(&base).await.unwrap();
    assert_eq!(
        handle
            .describe(WorkflowDescribeOptions::default())
            .await
            .unwrap()
            .run_id(),
        first_run
    );
    handle
        .terminate(
            WorkflowTerminateOptions::builder()
                .reason("owned installer readback fixture")
                .build(),
        )
        .await
        .unwrap();

    let (wrong, wrong_probe) = intercepted(&base, workflow, StartCase::WrongType);
    assert!(domain.install(&wrong).await.is_err());
    assert_eq!(wrong_probe.calls.load(Ordering::Acquire), 1);
    let wrong_description = handle
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap();
    assert_eq!(
        wrong_description.workflow_type(),
        "spellwake-patreon-wrong-type-test-v1"
    );
    assert_eq!(wrong_description.status(), WorkflowExecutionStatus::Running);
    assert_eq!(
        wrong_probe.accepted_run.lock().unwrap().as_deref(),
        Some(wrong_description.run_id())
    );
    let wrong_run = wrong_description.run_id().to_owned();
    assert_ne!(wrong_run, first_run);
    handle
        .terminate(
            WorkflowTerminateOptions::builder()
                .reason("owned wrong-type fixture cleanup")
                .build(),
        )
        .await
        .unwrap();

    let (terminal, terminal_probe) = intercepted(&base, workflow, StartCase::TerminateAccepted);
    assert!(domain.install(&terminal).await.is_err());
    assert_eq!(terminal_probe.calls.load(Ordering::Acquire), 1);
    let terminal_description = handle
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap();
    assert_eq!(terminal_description.workflow_type(), workflow);
    assert_ne!(
        terminal_description.status(),
        WorkflowExecutionStatus::Running
    );
    assert_eq!(
        terminal_probe.accepted_run.lock().unwrap().as_deref(),
        Some(terminal_description.run_id())
    );
    let terminal_run = terminal_description.run_id().to_owned();
    assert_ne!(terminal_run, wrong_run);
    let terminal_events: Vec<_> = handle
        .fetch_history(WorkflowFetchHistoryOptions::default())
        .try_collect()
        .await
        .unwrap();
    assert!(terminal_events.iter().any(|e| matches!(
        e.attributes,
        Some(Attributes::WorkflowExecutionTerminatedEventAttributes(_))
    )));
    assert!(!terminal_events.iter().any(|e| matches!(
        e.attributes,
        Some(Attributes::ActivityTaskScheduledEventAttributes(_))
    )));
    let directory =
        std::path::PathBuf::from(std::env::var("SPELLWAKE_TEST_PATREON_ARTIFACT_DIR").unwrap());
    std::fs::write(directory.join("installer-receipt.json"),serde_json::to_vec_pretty(&json!({"workflowId":workflow,"runs":[first_run,wrong_run,terminal_run],"hiddenAcceptedAckRecovered":true,"repeatKeptSameRun":true,"wrongActualTypeRejected":true,"actualTerminatedStateRejected":true,"workerStarted":false,"providerRequests":0,"scope":"real start/describe/terminate RPCs; controlled high-level SDK interceptor, not wire fault injection"})).unwrap()).unwrap();
}
