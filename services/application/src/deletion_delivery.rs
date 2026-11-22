//! Bounded deletion delivery with durable retirement and Auth cleanup reconciliation.
use crate::{
    ApiResponse,
    backend::{BackendClient, BackendConfig, BackendError},
    deletion::{CleanupWork, validate_cleanup_work},
};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use uuid::Uuid;

const WORK_LIMIT: usize = 25;
/// Opaque local backend diagnostic; never serialize or print the cause tree.
#[derive(Clone)]
pub struct DeliveryBackendFailure(BackendError);
impl fmt::Debug for DeliveryBackendFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeliveryBackendFailure")
            .finish_non_exhaustive()
    }
}

/// Safe delivery classification with original backend causes retained locally.
/// Equality compares classifications, while clones share the diagnostic cause.
#[derive(Debug, Clone)]
pub enum DeliveryError {
    Configuration,
    Backend,
    BackendCause(DeliveryBackendFailure),
    Canceled,
}
impl PartialEq for DeliveryError {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Configuration, Self::Configuration) | (Self::Canceled, Self::Canceled)
        ) || (matches!(self, Self::Backend | Self::BackendCause(_))
            && matches!(other, Self::Backend | Self::BackendCause(_)))
    }
}
impl Eq for DeliveryError {}
impl fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "deletion delivery configuration is invalid",
            Self::Backend | Self::BackendCause(_) => {
                "deletion cleanup disposition could not be established"
            }
            Self::Canceled => "deletion cleanup attempt was canceled",
        })
    }
}
impl std::error::Error for DeliveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BackendCause(cause) => Some(&cause.0),
            _ => None,
        }
    }
}
impl From<BackendError> for DeliveryError {
    fn from(value: BackendError) -> Self {
        match value {
            BackendError::Configuration => Self::Configuration,
            BackendError::Request => Self::Backend,
            cause @ BackendError::RequestCause { .. } => {
                Self::BackendCause(DeliveryBackendFailure(cause))
            }
        }
    }
}

pub struct DeletionDelivery {
    backend: BackendClient,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AccountPage {
    items: Vec<Uuid>,
    next_cursor: Option<Uuid>,
}

// The transient Auth subject deliberately has neither Debug nor Serialize.
// It stays inside the blocking activity and never enters Temporal history.
#[derive(Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
/// Trusted retirement work, including the separate external-resource phase.
/// Auth subjects stay transient: this contract has neither Debug nor Serialize.
pub enum RetirementWork {
    Pending {
        account_id: Uuid,
    },
    Cleanup {
        account_id: Uuid,
        retirement_operation_id: Uuid,
        subject: Uuid,
    },
    Complete {
        account_id: Uuid,
        retirement_operation_id: Uuid,
        auth_cleanup_receipt_id: Uuid,
    },
    Resources {
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        account_id: Uuid,
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        retirement_operation_id: Uuid,
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        auth_cleanup_receipt_id: Uuid,
    },
    Unproven {
        account_id: Uuid,
        retirement_operation_id: Uuid,
    },
}
impl RetirementWork {
    fn matches(&self, actor: Uuid) -> bool {
        match *self {
            Self::Pending { account_id } => account_id == actor,
            Self::Cleanup {
                account_id,
                retirement_operation_id,
                subject,
            } => account_id == actor && !retirement_operation_id.is_nil() && !subject.is_nil(),
            Self::Complete {
                account_id,
                retirement_operation_id,
                auth_cleanup_receipt_id,
            }
            | Self::Resources {
                account_id,
                retirement_operation_id,
                auth_cleanup_receipt_id,
            } => {
                account_id == actor
                    && !retirement_operation_id.is_nil()
                    && !auth_cleanup_receipt_id.is_nil()
            }
            Self::Unproven {
                account_id,
                retirement_operation_id,
            } => account_id == actor && !retirement_operation_id.is_nil(),
        }
    }
}
#[derive(Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum RetirementOutcome {
    Waiting {
        account_id: Uuid,
    },
    Retired {
        account_id: Uuid,
        retirement_operation_id: Uuid,
    },
}

/// Attempt counts are transport acknowledgments in this call, never proof that
/// an account is ready for final deletion or that every obligation is settled.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeletionBatch {
    pub attempted: u32,
    pub acknowledged: u32,
    pub next_cursor: Option<Uuid>,
    pub more: bool,
    /// This account needs another sweep; this does not certify deletion.
    pub retry_required: bool,
}
impl DeletionDelivery {
    /// Construct the private client from the configured backend origin and key.
    /// # Errors
    /// Returns safe configuration/client failures, retaining available causes locally.
    pub fn from_env() -> Result<Self, DeliveryError> {
        Self::new(BackendConfig::from_env()?)
    }
    pub(crate) fn new(config: BackendConfig) -> Result<Self, DeliveryError> {
        Ok(Self {
            backend: BackendClient::new(
                config,
                "Spellwake-Private-Deletion-Worker/0.1",
                Duration::from_secs(60),
            )?,
        })
    }
    /// Attempt one account and preserve its durable retry/discovery semantics.
    /// # Errors
    /// Rejects a nil cursor or unavailable initial account-page transport/decoder.
    /// Per-account backend failures are recorded as `retry_required` in the batch.
    pub fn deliver(&self, after: Option<Uuid>) -> Result<DeletionBatch, DeliveryError> {
        self.deliver_cancellable(after, &AtomicBool::new(false))
    }
    pub(crate) fn deliver_cancellable(
        &self,
        after: Option<Uuid>,
        canceled: &AtomicBool,
    ) -> Result<DeletionBatch, DeliveryError> {
        let check = || {
            if canceled.load(Ordering::Acquire) {
                Err(DeliveryError::Canceled)
            } else {
                Ok(())
            }
        };
        check()?;
        if after.is_some_and(|id| id.is_nil()) {
            return Err(DeliveryError::Configuration);
        }
        let page: AccountPage = self.backend.rpc_object(
            "read_deletion_account_page",
            &json!({"p_after":after,"p_limit":1}),
        )?;
        if page.items.len() > 1
            || page
                .items
                .iter()
                .any(|id| id.is_nil() || after.is_some_and(|previous| *id <= previous))
            || page.next_cursor != page.items.last().copied()
        {
            return Err(DeliveryError::Backend);
        }
        let Some(actor) = page.items.first().copied() else {
            return Ok(DeletionBatch {
                attempted: 0,
                acknowledged: 0,
                next_cursor: None,
                more: false,
                retry_required: false,
            });
        };
        let mut batch = DeletionBatch {
            attempted: 0,
            acknowledged: 0,
            next_cursor: Some(actor),
            more: true,
            retry_required: false,
        };
        match self.deliver_account(actor, canceled, &mut batch) {
            Ok(()) => {}
            Err(DeliveryError::Backend | DeliveryError::BackendCause(_)) => {
                // The durable account remains discoverable. Advance only this
                // sweep's cursor, without claiming any cleanup completion.
                batch.retry_required = true;
                eprintln!("deletion_workflow event=account_retry_required account_id={actor}");
            }
            Err(error) => return Err(error),
        }
        Ok(batch)
    }
    fn deliver_account(
        &self,
        actor: Uuid,
        canceled: &AtomicBool,
        batch: &mut DeletionBatch,
    ) -> Result<(), DeliveryError> {
        let check = || {
            if canceled.load(Ordering::Acquire) {
                Err(DeliveryError::Canceled)
            } else {
                Ok(())
            }
        };
        check()?;
        let state = self.retirement_work(actor)?;
        if !matches!(state, RetirementWork::Pending { .. }) {
            self.cleanup_auth(actor, state, canceled)?;
            return Ok(());
        }
        check()?;
        let work: Vec<crate::provider_json::Object<CleanupWork>> = self.backend.rpc(
            "read_deletion_work",
            &json!({"p_actor":actor,"p_limit":WORK_LIMIT}),
        )?;
        let work: Vec<_> = work.into_iter().map(|object| object.0).collect();
        validate_cleanup_work(&work, WORK_LIMIT).map_err(|_| DeliveryError::Backend)?;
        for item in work {
            check()?;
            batch.attempted += 1;
            let disposition: ApiResponse<crate::deletion::CleanupAck> = self.backend.rpc(
                "cancel_deletion_work",
                &json!({"p_actor":actor,"p_kind":item.kind,"p_target":item.id}),
            )?;
            match disposition {
                ApiResponse::Success { data } if data.matches(&item) => {
                    batch.acknowledged += 1;
                }
                // A concurrent manual cancellation or an unknown response stops
                // this attempt. Retry re-reads durable work; terminal items are
                // absent then, while genuinely unresolved failures stay visible.
                _ => return Err(DeliveryError::Backend),
            }
        }
        check()?;
        let outcome: ApiResponse<RetirementOutcome> = self
            .backend
            .rpc("retire_account", &json!({"p_account":actor}))?;
        match outcome {
            ApiResponse::Success {
                data: RetirementOutcome::Waiting { account_id },
            } if account_id == actor => {}
            ApiResponse::Success {
                data:
                    RetirementOutcome::Retired {
                        account_id,
                        retirement_operation_id,
                    },
            } if account_id == actor && !retirement_operation_id.is_nil() => {
                check()?;
                let work = self.retirement_work(actor)?;
                match &work {
                    RetirementWork::Cleanup {
                        retirement_operation_id: confirmed,
                        ..
                    }
                    | RetirementWork::Complete {
                        retirement_operation_id: confirmed,
                        ..
                    }
                    | RetirementWork::Resources {
                        retirement_operation_id: confirmed,
                        ..
                    } if *confirmed == retirement_operation_id => {}
                    _ => return Err(DeliveryError::Backend),
                }
                self.cleanup_auth(actor, work, canceled)?;
            }
            _ => return Err(DeliveryError::Backend),
        }
        Ok(())
    }
    fn retirement_work(&self, actor: Uuid) -> Result<RetirementWork, DeliveryError> {
        let work: RetirementWork = self
            .backend
            .rpc("read_account_retirement_work", &json!({"p_account":actor}))?;
        if !work.matches(actor) {
            return Err(DeliveryError::Backend);
        }
        Ok(work)
    }
    fn cleanup_auth(
        &self,
        actor: Uuid,
        work: RetirementWork,
        canceled: &AtomicBool,
    ) -> Result<(), DeliveryError> {
        let RetirementWork::Cleanup {
            subject,
            retirement_operation_id,
            ..
        } = work
        else {
            // Auth absence does not establish external resource absence. A
            // resources state stays discoverable without deleting Auth again.
            return if matches!(work, RetirementWork::Complete { .. }) {
                Ok(())
            } else {
                Err(DeliveryError::Backend)
            };
        };
        if canceled.load(Ordering::Acquire) {
            return Err(DeliveryError::Canceled);
        }
        // The exact target comes only from the canonical retirement proof. The
        // Admin response (including 404 or unknown acknowledgment) is never the
        // completion proof. A committed guard receipt must be read afterward.
        let url = self
            .backend
            .url(&format!("auth/v1/admin/users/{subject}"))?;
        let _transport_outcome = self
            .backend
            .request(Method::DELETE, url)?
            .json(&json!({"should_soft_delete":false}))
            .send();
        if canceled.load(Ordering::Acquire) {
            return Err(DeliveryError::Canceled);
        }
        match self.retirement_work(actor)? {
            RetirementWork::Complete {
                retirement_operation_id: confirmed,
                ..
            } if confirmed == retirement_operation_id => Ok(()),
            _ => Err(DeliveryError::Backend),
        }
    }
}

#[cfg(test)]
#[path = "deletion_delivery/diagnostics.rs"]
mod diagnostic_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Mock, Step};
    fn client(mock: &Mock) -> DeletionDelivery {
        DeletionDelivery::new(BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap())
            .unwrap()
    }
    fn account(actor: Uuid) -> Step {
        Step::json(
            "/rest/v1/rpc/read_deletion_account_page",
            json!({"items":[actor],"nextCursor":actor}),
        )
    }
    fn pending(actor: Uuid) -> Step {
        Step::json(
            "/rest/v1/rpc/read_account_retirement_work",
            json!({"accountId":actor,"state":"pending"}),
        )
    }
    fn waiting(actor: Uuid) -> Step {
        Step::json(
            "/rest/v1/rpc/retire_account",
            json!({"status":"success","data":{"accountId":actor,"state":"waiting"}}),
        )
    }
    #[test]
    fn private_objects_reject_page_and_work_arrays_before_cleanup_actions() {
        let actor = Uuid::new_v4();
        let mock = Mock::new(vec![Step::json(
            "/rest/v1/rpc/read_deletion_account_page",
            json!([[actor], actor]),
        )]);
        let error = client(&mock).deliver(None).unwrap_err();
        assert_eq!(error, DeliveryError::Backend);
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(mock.finish().len(), 1);
        let target = Uuid::new_v4();
        let mock = Mock::new(vec![
            account(actor),
            pending(actor),
            Step::json(
                "/rest/v1/rpc/read_deletion_work",
                json!([["trade", target]]),
            ),
        ]);
        let batch = client(&mock).deliver(None).unwrap();
        assert_eq!(batch.attempted, 0);
        assert_eq!(batch.acknowledged, 0);
        assert!(batch.retry_required);
        assert_eq!(mock.finish().len(), 3);
    }
    #[test]
    fn cleanup_delivers_only_validated_work_and_preserves_exact_targets() {
        let actor = Uuid::new_v4();
        let target = Uuid::new_v4();
        let mock = Mock::new(vec![
            account(actor),
            pending(actor),
            Step::json(
                "/rest/v1/rpc/read_deletion_work",
                json!([{"kind":"trade","id":target}]),
            ),
            Step::json(
                "/rest/v1/rpc/cancel_deletion_work",
                json!({"status":"success","data":{"resourceId":target}}),
            ),
            waiting(actor),
        ]);
        let result = client(&mock).deliver(None).unwrap();
        assert_eq!(
            (
                result.attempted,
                result.acknowledged,
                result.next_cursor,
                result.more
            ),
            (1, 1, Some(actor), true)
        );
        let seen = mock.finish();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&seen[3].2).unwrap(),
            json!({"p_actor":actor,"p_kind":"trade","p_target":target})
        );
    }
    #[test]
    fn all_canonical_cleanup_kinds_keep_their_specific_acknowledgments() {
        let actor = Uuid::new_v4();
        let mut work = Vec::new();
        let mut steps = vec![account(actor), pending(actor)];
        let kinds = [
            "ai_slot",
            "ai_stop",
            "ante",
            "invitation",
            "listing",
            "prediction",
            "queue",
            "room",
            "trade",
        ];
        for kind in kinds {
            work.push(json!({"kind":kind,"id":Uuid::new_v4()}));
        }
        steps.push(Step::json("/rest/v1/rpc/read_deletion_work", json!(work)));
        for item in &work {
            let data = match item["kind"].as_str().unwrap() {
                "ante" => json!({"resourceId":item["id"],"state":"canceled"}),
                "ai_stop" => {
                    json!({"resourceId":item["id"],"stopId":Uuid::new_v4(),"disposition":"stop requested"})
                }
                _ => json!({"resourceId":item["id"]}),
            };
            steps.push(Step::json(
                "/rest/v1/rpc/cancel_deletion_work",
                json!({"status":"success","data":data}),
            ));
        }
        steps.push(waiting(actor));
        let mock = Mock::new(steps);
        let batch = client(&mock).deliver(None).unwrap();
        assert_eq!((batch.attempted, batch.acknowledged), (9, 9));
        let seen = mock.finish();
        for (item, request) in work.iter().zip(&seen[3..12]) {
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&request.2).unwrap(),
                json!({"p_actor":actor,"p_kind":item["kind"],"p_target":item["id"]})
            );
        }
        work.reverse();
        let mock = Mock::new(vec![
            account(actor),
            pending(actor),
            Step::json("/rest/v1/rpc/read_deletion_work", json!(work)),
        ]);
        let rejected = client(&mock).deliver(None).unwrap();
        assert_eq!((rejected.attempted, rejected.acknowledged), (0, 0));
        assert!(rejected.retry_required);
        assert_eq!(mock.finish().len(), 3);
    }
    fn retirement(actor: Uuid, operation: Uuid, subject: Uuid) -> Step {
        Step::json(
            "/rest/v1/rpc/read_account_retirement_work",
            json!({"state":"cleanup","accountId":actor,"retirementOperationId":operation,"subject":subject}),
        )
    }
    fn complete(actor: Uuid, operation: Uuid) -> Step {
        Step::json(
            "/rest/v1/rpc/read_account_retirement_work",
            json!({"state":"complete","accountId":actor,"retirementOperationId":operation,"authCleanupReceiptId":Uuid::new_v4()}),
        )
    }
    fn admin(subject: Uuid) -> Step {
        let mut step = Step::json(&format!("/auth/v1/admin/users/{subject}"), json!({}));
        step.method = "DELETE";
        step.expect = Some(serde_json::to_vec(&json!({"should_soft_delete":false})).unwrap());
        step
    }
    fn resources(actor: Uuid, operation: Uuid) -> Step {
        Step::json(
            "/rest/v1/rpc/read_account_retirement_work",
            json!({"state":"resources","accountId":actor,"retirementOperationId":operation,"authCleanupReceiptId":Uuid::new_v4()}),
        )
    }
    #[test]
    fn resource_cleanup_remains_retryable_without_repeating_auth_cleanup() {
        for phase in 0..3 {
            let actor = Uuid::new_v4();
            let operation = Uuid::new_v4();
            let subject = Uuid::new_v4();
            let mut steps = vec![account(actor)];
            match phase {
                0 => {}
                1 => steps.extend([
                    pending(actor),
                    Step::json("/rest/v1/rpc/read_deletion_work", json!([])),
                    Step::json(
                        "/rest/v1/rpc/retire_account",
                        json!({"status":"success","data":{"state":"retired","accountId":actor,"retirementOperationId":operation}}),
                    ),
                ]),
                _ => steps.extend([retirement(actor, operation, subject), admin(subject)]),
            }
            steps.extend([
                resources(actor, operation),
                account(actor),
                complete(actor, operation),
            ]);
            let mock = Mock::new(steps);
            let delivery = client(&mock);
            assert!(delivery.deliver(None).unwrap().retry_required);
            assert!(!delivery.deliver(None).unwrap().retry_required);
            assert_eq!(
                mock.finish()
                    .iter()
                    .filter(|request| request.0.starts_with("/auth/"))
                    .count(),
                usize::from(phase == 2),
            );
        }
    }
    #[test]
    fn restarted_worker_cleans_exact_retired_target_and_reconciles_even_unknown_ack() {
        for unknown in [false, true] {
            let actor = Uuid::new_v4();
            let operation = Uuid::new_v4();
            let subject = Uuid::new_v4();
            let mut delete = admin(subject);
            delete.drop_ack = unknown;
            let mock = Mock::new(vec![
                account(actor),
                retirement(actor, operation, subject),
                delete,
                complete(actor, operation),
            ]);
            let result = client(&mock).deliver(None).unwrap();
            assert_eq!(
                (result.attempted, result.acknowledged, result.next_cursor),
                (0, 0, Some(actor))
            );
            let history = serde_json::to_string(&result).unwrap();
            assert!(!history.contains(&subject.to_string()));
            assert_eq!(mock.finish().len(), 4);
        }
    }
    #[test]
    fn retirement_commits_before_admin_cleanup_and_completed_retry_skips_admin() {
        let actor = Uuid::new_v4();
        let operation = Uuid::new_v4();
        let subject = Uuid::new_v4();
        let mock = Mock::new(vec![
            account(actor),
            pending(actor),
            Step::json("/rest/v1/rpc/read_deletion_work", json!([])),
            Step::json(
                "/rest/v1/rpc/retire_account",
                json!({"status":"success","data":{"state":"retired","accountId":actor,"retirementOperationId":operation}}),
            ),
            retirement(actor, operation, subject),
            admin(subject),
            complete(actor, operation),
            account(actor),
            complete(actor, operation),
        ]);
        let delivery = client(&mock);
        delivery.deliver(None).unwrap();
        delivery.deliver(None).unwrap();
        assert_eq!(
            mock.finish()
                .iter()
                .filter(|r| r.0.starts_with("/auth/"))
                .count(),
            1
        );
    }
    #[test]
    fn admin_http_success_or_not_found_without_canonical_proof_never_completes() {
        for status in [200, 404, 500] {
            let actor = Uuid::new_v4();
            let operation = Uuid::new_v4();
            let subject = Uuid::new_v4();
            let mut delete = admin(subject);
            delete.status = status;
            let mock = Mock::new(vec![
                account(actor),
                retirement(actor, operation, subject),
                delete,
                retirement(actor, operation, subject),
            ]);
            assert!(client(&mock).deliver(None).unwrap().retry_required);
            mock.finish();
        }
    }
    #[test]
    fn mismatched_or_unproven_retirement_never_sends_an_admin_request() {
        let actor = Uuid::new_v4();
        for state in [
            json!({"state":"cleanup","accountId":Uuid::new_v4(),"retirementOperationId":Uuid::new_v4(),"subject":Uuid::new_v4()}),
            json!({"state":"cleanup","accountId":actor,"retirementOperationId":Uuid::new_v4(),"subject":Uuid::nil()}),
            json!({"state":"unproven","accountId":actor,"retirementOperationId":Uuid::new_v4()}),
            json!({"state":"complete","accountId":actor,"retirementOperationId":Uuid::new_v4(),"authCleanupReceiptId":Uuid::nil()}),
            json!({"state":"resources","accountId":Uuid::new_v4(),"retirementOperationId":Uuid::new_v4(),"authCleanupReceiptId":Uuid::new_v4()}),
            json!({"state":"resources","accountId":actor,"retirementOperationId":Uuid::nil(),"authCleanupReceiptId":Uuid::new_v4()}),
            json!({"state":"resources","accountId":actor,"retirementOperationId":Uuid::new_v4(),"authCleanupReceiptId":Uuid::nil()}),
        ] {
            let mock = Mock::new(vec![
                account(actor),
                Step::json("/rest/v1/rpc/read_account_retirement_work", state),
            ]);
            assert!(client(&mock).deliver(None).unwrap().retry_required);
            assert_eq!(mock.finish().len(), 2);
        }
    }
    #[test]
    fn unresolved_account_advances_only_this_sweep_and_remains_retryable_after_tail() {
        let first = Uuid::from_u128(10);
        let second = Uuid::from_u128(20);
        let operation = Uuid::new_v4();
        let mock = Mock::new(vec![
            account(first),
            Step::json(
                "/rest/v1/rpc/read_account_retirement_work",
                json!({"state":"unproven","accountId":first,"retirementOperationId":operation}),
            ),
            account(second),
            complete(second, Uuid::new_v4()),
            Step::json(
                "/rest/v1/rpc/read_deletion_account_page",
                json!({"items":[],"nextCursor":null}),
            ),
            account(first),
            Step::json(
                "/rest/v1/rpc/read_account_retirement_work",
                json!({"state":"unproven","accountId":first,"retirementOperationId":operation}),
            ),
        ]);
        let delivery = client(&mock);
        let failed = delivery.deliver(None).unwrap();
        assert!(failed.retry_required && failed.more);
        assert_eq!(
            (failed.attempted, failed.acknowledged, failed.next_cursor),
            (0, 0, Some(first))
        );
        let next = delivery.deliver(failed.next_cursor).unwrap();
        assert!(!next.retry_required);
        assert_eq!(next.next_cursor, Some(second));
        let tail = delivery.deliver(next.next_cursor).unwrap();
        assert!(!tail.more && !tail.retry_required);
        let retry = delivery.deliver(None).unwrap();
        assert!(retry.retry_required);
        assert_eq!(retry.next_cursor, Some(first));
        let seen = mock.finish();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&seen[2].2).unwrap(),
            json!({"p_after":first,"p_limit":1})
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&seen[4].2).unwrap(),
            json!({"p_after":second,"p_limit":1})
        );
    }
    #[test]
    fn malformed_pages_are_rejected_before_any_cancellation() {
        let actor = Uuid::new_v4();
        let target = Uuid::new_v4();
        for work in [
            json!([{"kind":"trade","id":target},{"kind":"trade","id":target}]),
            json!([{"kind":"filled_position","id":target}]),
            json!([{"kind":"listing","id":Uuid::nil()}]),
        ] {
            let mock = Mock::new(vec![
                account(actor),
                pending(actor),
                Step::json("/rest/v1/rpc/read_deletion_work", work),
            ]);
            assert!(client(&mock).deliver(None).unwrap().retry_required);
            assert_eq!(mock.finish().len(), 3);
        }
        let mock = Mock::new(vec![Step::json(
            "/rest/v1/rpc/read_deletion_account_page",
            json!({"items":[actor],"nextCursor":null}),
        )]);
        assert_eq!(
            client(&mock).deliver(None).unwrap_err(),
            DeliveryError::Backend
        );
        mock.finish();
    }
    #[test]
    fn lost_ack_retries_from_durable_work_without_resending_terminal_target() {
        let actor = Uuid::new_v4();
        let target = Uuid::new_v4();
        let mut dropped = Step::json("/rest/v1/rpc/cancel_deletion_work", json!({}));
        dropped.drop_ack = true;
        let mock = Mock::new(vec![
            account(actor),
            pending(actor),
            Step::json(
                "/rest/v1/rpc/read_deletion_work",
                json!([{"kind":"listing","id":target}]),
            ),
            dropped,
            account(actor),
            pending(actor),
            Step::json("/rest/v1/rpc/read_deletion_work", json!([])),
            waiting(actor),
        ]);
        let delivery = client(&mock);
        let incomplete = delivery.deliver(None).unwrap();
        assert!(incomplete.retry_required);
        assert_eq!((incomplete.attempted, incomplete.acknowledged), (1, 0));
        let resumed = delivery.deliver(None).unwrap();
        assert_eq!((resumed.attempted, resumed.acknowledged), (0, 0));
        assert_eq!(
            mock.finish()
                .iter()
                .filter(|request| request.0.ends_with("cancel_deletion_work"))
                .count(),
            1
        );
    }
    #[test]
    fn empty_tail_resets_only_via_workflow_and_business_failures_are_not_acknowledged() {
        let mock = Mock::new(vec![Step::json(
            "/rest/v1/rpc/read_deletion_account_page",
            json!({"items":[],"nextCursor":null}),
        )]);
        let tail = client(&mock).deliver(Some(Uuid::new_v4())).unwrap();
        assert!(!tail.more);
        assert!(tail.next_cursor.is_none());
        mock.finish();
        let actor = Uuid::new_v4();
        let target = Uuid::new_v4();
        let mock = Mock::new(vec![
            account(actor),
            pending(actor),
            Step::json(
                "/rest/v1/rpc/read_deletion_work",
                json!([{"kind":"prediction","id":target}]),
            ),
            Step::json(
                "/rest/v1/rpc/cancel_deletion_work",
                json!({"status":"fail","data":{"order":"already terminal"}}),
            ),
        ]);
        let incomplete = client(&mock).deliver(None).unwrap();
        assert!(incomplete.retry_required);
        assert_eq!((incomplete.attempted, incomplete.acknowledged), (1, 0));
        mock.finish();
    }
    #[test]
    fn cancellation_stops_between_rpc_calls_without_releasing_the_next_target() {
        let actor = Uuid::new_v4();
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        let canceled = std::sync::Arc::new(AtomicBool::new(false));
        let observer = canceled.clone();
        let mock = Mock::new_with_observer(
            vec![
                account(actor),
                pending(actor),
                Step::json(
                    "/rest/v1/rpc/read_deletion_work",
                    json!([{"kind":"trade","id":first},{"kind":"trade","id":second}]),
                ),
                Step::json(
                    "/rest/v1/rpc/cancel_deletion_work",
                    json!({"status":"success","data":{"resourceId":first}}),
                ),
            ],
            move |path| {
                if path.ends_with("cancel_deletion_work") {
                    observer.store(true, Ordering::Release);
                }
            },
        );
        assert_eq!(
            client(&mock)
                .deliver_cancellable(None, &canceled)
                .unwrap_err(),
            DeliveryError::Canceled
        );
        assert_eq!(mock.finish().len(), 4);
    }
}
