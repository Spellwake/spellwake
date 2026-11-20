//! Shared exact-byte accounting for bounded native source records.
use super::*;

impl NativeSourceRepository {
    /// Bounded original table rows. Callers fix the table/projection and exact
    /// typed scope; this internal adapter is never a public query relay.
    pub(super) fn native_rows<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        query: &[(&str, String)],
        maximum: u64,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<Vec<T>, NativeSourceError> {
        let maximum = budget.remaining_bytes().min(maximum);
        if maximum == 0 {
            return Err(NativeSourceError(Failure::Checkpoint(
                NativeCheckpointError::Budget,
            )));
        }
        budget
            .charge_work(1, 0)
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        let bytes = self
            .backend
            .select_bytes_bounded(table, query, maximum)
            .map_err(|error| NativeSourceError(Failure::Backend(error)))?;
        budget
            .charge_work(0, u64::try_from(bytes.len()).map_err(|_| invalid())?)
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        let rows: Vec<crate::provider_json::Object<T>> =
            serde_json::from_slice(&bytes).map_err(|error| {
                NativeSourceError(Failure::Backend(BackendError::caused("json", error)))
            })?;
        Ok(rows.into_iter().map(|row| row.0).collect())
    }

    pub(super) fn native_record<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
        arguments: &serde_json::Value,
        mut budget: Option<&mut NativeRecoveryBudget>,
    ) -> Result<T, NativeSourceError> {
        let maximum = budget
            .as_ref()
            .map_or(1_048_576, |budget| budget.remaining_bytes().min(1_048_576));
        if maximum == 0 {
            return Err(NativeSourceError(Failure::Checkpoint(
                NativeCheckpointError::Budget,
            )));
        }
        if let Some(budget) = budget.as_deref_mut() {
            // Reserve before IO, including failed attempts. An exhausted work
            // budget must not publish a cursor or contact a provider once more.
            budget
                .charge_work(1, 0)
                .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        }
        let bytes = self
            .backend
            .rpc_bytes_bounded(name, arguments, maximum)
            .map_err(|error| NativeSourceError(Failure::Backend(error)))?;
        if let Some(budget) = budget {
            budget
                .charge_work(0, u64::try_from(bytes.len()).map_err(|_| invalid())?)
                .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        }
        crate::decode_wire_object(&bytes).map_err(|error| {
            NativeSourceError(Failure::Backend(BackendError::caused("json", error)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Mock, Step};

    fn repository(mock: &Mock) -> NativeSourceRepository {
        NativeSourceRepository::new(
            BackendClient::new(
                BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
                "native-rpc-budget-test",
                Duration::from_secs(1),
            )
            .unwrap(),
        )
    }
    fn exhausted(result: Result<serde_json::Value, NativeSourceError>) {
        let Err(error) = result else {
            panic!("exhausted budget must fail")
        };
        assert!(matches!(
            error.0,
            Failure::Checkpoint(NativeCheckpointError::Budget)
        ));
    }
    #[test]
    fn exhausted_native_rpc_work_is_refused_before_transport() {
        let mock = Mock::new(vec![]);
        let repository = repository(&mock);
        let mut budget = NativeRecoveryBudget::new(1, 1_024).unwrap();
        budget.charge_work(1, 0).unwrap();
        exhausted(repository.native_record(
            "issue_native_cursor_source",
            &json!({}),
            Some(&mut budget),
        ));
        assert!(mock.finish().is_empty());
    }
    #[test]
    fn failed_native_rpc_attempt_also_consumes_its_work_slot() {
        let mut failed = Step::json(
            "/rest/v1/rpc/read_native_archive_source",
            json!({"status":"error"}),
        );
        failed.status = 503;
        let mock = Mock::new(vec![failed]);
        let repository = repository(&mock);
        let mut budget = NativeRecoveryBudget::new(1, 1_024).unwrap();
        assert!(
            repository
                .native_record::<serde_json::Value>(
                    "read_native_archive_source",
                    &json!({}),
                    Some(&mut budget)
                )
                .is_err()
        );
        exhausted(repository.native_record(
            "read_native_archive_source",
            &json!({}),
            Some(&mut budget),
        ));
        assert_eq!(mock.finish().len(), 1);
    }
}
