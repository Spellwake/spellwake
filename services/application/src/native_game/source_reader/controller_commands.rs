//! Possessed-controller originals and mutations use existing seat-owned SQL receipts.
use super::command::{
    Ack, Batch, PlayerAuthorityKind, decode_original_receipt, decode_player_batch,
    receipt_query_raw,
};
use super::*;
use crate::{
    controller_possession::{ControllerSecretStore, PossessedController},
    player_wire::{PlayerCommand, PlayerCommandAck, PlayerReceipt, PlayerReceiptQuery},
};
use std::collections::BTreeMap;
fn unavailable<T>() -> ApiResponse<T> {
    ApiResponse::Fail {
        data: BTreeMap::from([("actor".to_owned(), "unavailable".to_owned())]),
    }
}
fn current(
    store: &ControllerSecretStore,
    proof: &PossessedController,
) -> Result<bool, NativeSourceError> {
    store
        .current(proof)
        .map_err(|e| NativeSourceError(Failure::Possession(e)))
}
impl NativeSourceRepository {
    /// Recover seat-owned originals using current credential authority, without
    /// current native recovery, View generation or a host lease. Replacement
    /// controllers can recover the same seat's originals; grants are not receipt IDs.
    /// # Errors
    /// Rejects invalid/over-budget queries and malformed, uncorrelated or unsafe
    /// responses. Local revocation is rechecked after every backend outcome.
    pub fn read_controller_receipts(
        &self,
        store: &ControllerSecretStore,
        proof: &PossessedController,
        query: &PlayerReceiptQuery,
    ) -> Result<ApiResponse<Vec<PlayerReceipt>>, NativeSourceError> {
        let raw = receipt_query_raw(query)?;
        if !current(store, proof)? {
            return Ok(unavailable());
        }
        let response=self.backend.rpc_bounded::<ObjectResponse<Batch>>("read_native_controller_receipts",&json!({"p_grant":proof.grant(),"p_epoch":proof.epoch(),"p_secret_ref":proof.reference(),"p_query":raw}),2*1024*1024);
        if !current(store, proof)? {
            return Ok(unavailable());
        }
        decode_player_batch(
            response
                .map_err(|e| NativeSourceError(Failure::Backend(e)))?
                .0,
            query,
            PlayerAuthorityKind::Controller,
        )
    }
    /// Look up one exact original before current native/artifact/host work.
    /// Unknown is None only; current actor/game/request refusals remain structured.
    /// # Errors
    /// Rejects malformed/corrupted disposition or original/seat/ack correlation.
    pub fn controller_original(
        &self,
        store: &ControllerSecretStore,
        proof: &PossessedController,
        original: &PlayerCommand,
    ) -> Result<Option<ApiResponse<PlayerCommandAck>>, NativeSourceError> {
        original.validate().map_err(|_| invalid())?;
        decode_original_receipt(self.read_controller_receipts(
            store,
            proof,
            &PlayerReceiptQuery {
                game_id: original.basis.game_id,
                context_id: original.basis.context_id,
                outstanding: vec![original.clone()],
            },
        )?)
    }
}

impl NativeSourceRepository {
    pub(super) fn submit_controller(
        &self,
        store: &ControllerSecretStore,
        proof: &PossessedController,
        viewer: Uuid,
        original: &PlayerCommand,
        prepared: &serde_json::Value,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        if !current(store, proof)? {
            return Ok(unavailable());
        }
        let response=self.backend.rpc::<ObjectResponse<Ack>>("submit_native_controller_command",&json!({"p_grant":proof.grant(),"p_epoch":proof.epoch(),"p_secret_ref":proof.reference(),"p_original":original,"p_prepared":prepared}));
        if !current(store, proof)? {
            return Ok(unavailable());
        }
        let result = response
            .map_err(|e| NativeSourceError(Failure::Backend(e)))
            .and_then(|r| {
                super::command::decode_player_submission(
                    r.0,
                    original,
                    viewer,
                    PlayerAuthorityKind::Controller,
                )
            });
        match result {
            Ok(response) => Ok(response),
            Err(error) => match self.controller_original(store, proof, original) {
                Ok(Some(response)) => Ok(response),
                _ => Err(error),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        controller_possession::ControllerCredential,
        player_wire::PlayerAction,
        test_http::{Mock, Step},
    };
    use std::sync::Arc;
    const READ: &str = "/rest/v1/rpc/read_native_controller_receipts";
    const SUBMIT: &str = "/rest/v1/rpc/submit_native_controller_command";
    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
    fn original(n: u128) -> PlayerCommand {
        PlayerCommand {
            request_id: id(n),
            basis: PlayerBasis {
                game_id: id(1),
                context_id: id(2),
                epoch: 0.into(),
                revision: 3.into(),
            },
            action: PlayerAction::Pass {},
        }
    }
    fn install(
        store: &ControllerSecretStore,
        grant: u128,
    ) -> (ControllerCredential, PossessedController) {
        let token = store
            .install(
                id(grant),
                id(201),
                2.into(),
                format!("private:controller:{grant}").parse().unwrap(),
            )
            .unwrap();
        let proof = store.authenticate(id(grant), &token).unwrap().unwrap();
        (token, proof)
    }
    fn repo(mock: &Mock) -> NativeSourceRepository {
        NativeSourceRepository::new(
            BackendClient::new(
                BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
                "controller-receipt-test",
                Duration::from_secs(2),
            )
            .unwrap(),
        )
    }
    fn ack(n: u128) -> serde_json::Value {
        json!({"requestId":id(n),"actorSeatId":id(100),"gameId":id(1),"contextId":id(2),"epoch":"0","revision":"4"})
    }
    fn receipt(n: u128, state: &str, response: serde_json::Value) -> serde_json::Value {
        json!({"status":"success","data":{"receipts":[{"actorSeatId":id(100),"original":original(n),"state":state,"response":response}]}})
    }
    #[test]
    fn controller_originals_use_exact_query_without_native_reads_and_survive_replacement() {
        let store = ControllerSecretStore::default();
        let (token, proof) = install(&store, 200);
        let mut first = Step::json(
            READ,
            receipt(10, "accepted", json!({"status":"success","data":ack(10)})),
        );
        first.expect=Some(serde_json::to_vec(&json!({"p_grant":id(200),"p_epoch":"2","p_secret_ref":"private:controller:200","p_query":{"gameId":id(1),"contextId":id(2),"outstanding":[original(10)]}})).unwrap());
        let mock = Mock::new(vec![
            first,
            Step::json(
                READ,
                receipt(10, "accepted", json!({"status":"success","data":ack(10)})),
            ),
        ]);
        let repository = repo(&mock);
        assert!(matches!(
            repository
                .controller_original(&store, &proof, &original(10))
                .unwrap(),
            Some(ApiResponse::Success { .. })
        ));
        store.revoke(id(200)).unwrap();
        let (_, replacement) = install(&store, 202);
        assert!(
            matches!(repository.controller_original(&store,&proof,&original(10)).unwrap(),Some(ApiResponse::Fail{data})if data.get("actor").is_some_and(|v|v=="unavailable"))
        );
        assert!(matches!(
            repository
                .controller_original(&store, &replacement, &original(10))
                .unwrap(),
            Some(ApiResponse::Success { .. })
        ));
        let captured = mock.finish();
        assert_eq!(captured.len(), 2);
        assert!(captured.iter().all(|(path, _, _)| path == READ));
        assert!(!String::from_utf8_lossy(&captured[0].2).contains(&token.expose_for_delivery()));
    }
    #[test]
    fn receipts_reject_actor_order_original_response_and_controller_session_drift() {
        let store = ControllerSecretStore::default();
        let (_, proof) = install(&store, 200);
        let good = receipt(10, "accepted", json!({"status":"success","data":ack(10)}));
        let mut cases = vec![
            json!({"status":"fail","data":{"session":"unauthorized"}}),
            json!({"status":"fail","data":{"actor":"unknown"}}),
        ];
        for (pointer, value) in [
            ("/data/receipts/0/actorSeatId", json!(Uuid::nil())),
            ("/data/receipts/0/original/requestId", json!(id(11))),
            ("/data/receipts/0/response/data/revision", json!("3")),
            ("/data/receipts/0/response/data/actorSeatId", json!(id(101))),
            ("/data/receipts/0/state", json!("pending")),
            ("/data/receipts/0/response", serde_json::Value::Null),
        ] {
            let mut v = good.clone();
            *v.pointer_mut(pointer).unwrap() = value;
            cases.push(v);
        }
        let mock = Mock::new(cases.iter().map(|v| Step::json(READ, v.clone())).collect());
        let repository = repo(&mock);
        for _ in cases {
            assert!(
                repository
                    .controller_original(&store, &proof, &original(10))
                    .is_err()
            );
        }
        mock.finish();
    }
    #[test]
    fn late_revocation_suppresses_receipt_success_and_error_without_any_native_fallback() {
        for mode in 0..4 {
            let store = Arc::new(ControllerSecretStore::default());
            let (_, proof) = install(&store, 200);
            let revoke = store.clone();
            let mut step = Step::json(
                READ,
                receipt(10, "accepted", json!({"status":"success","data":ack(10)})),
            );
            if mode == 3 {
                step.status = 503;
            }
            step.reply = Some(Box::new(move |_| {
                revoke.revoke(id(200)).unwrap();
                if mode == 0 {
                    serde_json::to_vec(&receipt(
                        10,
                        "accepted",
                        json!({"status":"success","data":ack(10)}),
                    ))
                    .unwrap()
                } else if mode == 1 {
                    serde_json::to_vec(&json!({"status":"fail","data":{"actor":"unavailable"}}))
                        .unwrap()
                } else {
                    b"not-json".to_vec()
                }
            }));
            let mock = Mock::new(vec![step]);
            let repository = repo(&mock);
            assert!(
                matches!(repository.controller_original(&store,&proof,&original(10)).unwrap(),Some(ApiResponse::Fail{data})if data.get("actor").is_some_and(|v|v=="unavailable"))
            );
            mock.finish();
        }
    }
    #[test]
    fn local_revocation_after_submit_suppresses_success_denial_decode_and_transport_results() {
        for mode in 0..4 {
            let store = Arc::new(ControllerSecretStore::default());
            let (_, proof) = install(&store, 200);
            let revoke = store.clone();
            let mut step = Step::json(SUBMIT, json!({"status":"success","data":ack(10)}));
            if mode == 3 {
                step.status = 503;
            }
            step.reply = Some(Box::new(move |_| {
                revoke.revoke(id(200)).unwrap();
                match mode {
                    0 => serde_json::to_vec(&json!({"status":"success","data":ack(10)})).unwrap(),
                    1 => serde_json::to_vec(&json!({"status":"fail","data":{"basis":"stale"}}))
                        .unwrap(),
                    _ => b"not-json".to_vec(),
                }
            }));
            let mock = Mock::new(vec![step]);
            let repository = repo(&mock);
            assert!(
                matches!(repository.submit_controller(&store,&proof,id(100),&original(10),&serde_json::Value::Null).unwrap(),ApiResponse::Fail{data}if data.get("actor").is_some_and(|v|v=="unavailable"))
            );
            assert!(
                matches!(repository.controller_original(&store,&proof,&original(10)).unwrap(),Some(ApiResponse::Fail{data})if data.get("actor").is_some_and(|v|v=="unavailable"))
            );
            assert_eq!(mock.finish().len(), 1);
        }
    }
    #[test]
    fn uncertain_controller_submit_recovers_original_once_and_never_resubmits() {
        let store = ControllerSecretStore::default();
        let (_, proof) = install(&store, 200);
        let mut submit = Step::json(SUBMIT, json!({"status":"success","data":ack(10)}));
        submit.drop_ack = true;
        let mock = Mock::new(vec![
            submit,
            Step::json(
                READ,
                receipt(10, "accepted", json!({"status":"success","data":ack(10)})),
            ),
        ]);
        let repository = repo(&mock);
        assert!(matches!(
            repository
                .submit_controller(
                    &store,
                    &proof,
                    id(100),
                    &original(10),
                    &serde_json::Value::Null
                )
                .unwrap(),
            ApiResponse::Success { .. }
        ));
        let captured = mock.finish();
        assert_eq!(captured.iter().filter(|(p, _, _)| p == SUBMIT).count(), 1);
        assert_eq!(captured.iter().filter(|(p, _, _)| p == READ).count(), 1);
    }
}
