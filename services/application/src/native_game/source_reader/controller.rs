//! Existing private SQL access composed only after actual local credential proof.
use super::*;
use crate::controller_possession::{ControllerSecretStore, PossessedController};

impl NativeSourceRepository {
    /// Observe current canonical AI access using this store's actual possession proof.
    /// SQL independently rechecks issued/delivered current grant and adopted seat.
    /// Raw credential bytes never enter SQL. Local revocation is checked before
    /// and after IO; the returned observation is not authority for later delivery.
    /// # Errors
    /// Rejects nil scope, malformed/foreign actor or source, unknown failure
    /// vocabulary, storage failures and backend errors, preserving private causes.
    pub fn controller_access(
        &self,
        store: &ControllerSecretStore,
        proof: &PossessedController,
        game: Uuid,
        context: Uuid,
    ) -> Result<NativeControllerAccessOutcome, NativeSourceError> {
        if game.is_nil() || context.is_nil() {
            return Err(invalid());
        }
        let current = || {
            store
                .current(proof)
                .map_err(|e| NativeSourceError(Failure::Possession(e)))
        };
        if !current()? {
            return Ok(NativeControllerAccessOutcome::Unavailable);
        }
        let response:Result<ObjectResponse<access::Record>,_>=self.backend.rpc("read_native_controller_access",&json!({"p_grant":proof.grant(),"p_epoch":proof.epoch(),"p_secret_ref":proof.reference(),"p_game":game,"p_context":context}));
        if !current()? {
            return Ok(NativeControllerAccessOutcome::Unavailable);
        }
        match response
            .map_err(|e| NativeSourceError(Failure::Backend(e)))?
            .0
        {
            ApiResponse::Success { data } => data
                .into_controller_access(proof, game, context)
                .map(NativeControllerAccessOutcome::Granted),
            ApiResponse::Fail { data }
                if data.len() == 1
                    && (data.get("actor").is_some_and(|v| v == "unavailable")
                        || data.get("game").is_some_and(|v| v == "unavailable")) =>
            {
                Ok(NativeControllerAccessOutcome::Unavailable)
            }
            _ => Err(invalid()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        controller_possession::ControllerCredential,
        test_http::{Mock, Step},
    };
    use serde_json::Value;
    use std::sync::Arc;
    const PATH: &str = "/rest/v1/rpc/read_native_controller_access";
    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
    fn fixture() -> Value {
        let mut value = access::tests::fixture();
        value["data"]["actor"] = json!({"seatId":id(6),"kind":"ai","accountId":null,"grantId":id(10),"bindingId":id(11),"bindingEpoch":"2"});
        value
    }
    fn installed(store: &ControllerSecretStore) -> (ControllerCredential, PossessedController) {
        let credential = store
            .install(
                id(10),
                id(11),
                2.into(),
                "private:test-generation".parse().unwrap(),
            )
            .unwrap();
        let proof = store.authenticate(id(10), &credential).unwrap().unwrap();
        (credential, proof)
    }
    fn repository(mock: &Mock) -> NativeSourceRepository {
        NativeSourceRepository::new(
            BackendClient::new(
                BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
                "controller-access-test",
                Duration::from_secs(2),
            )
            .unwrap(),
        )
    }
    #[test]
    fn actual_possession_supplies_only_derived_metadata_to_existing_rpc() {
        let store = ControllerSecretStore::default();
        let (token, proof) = installed(&store);
        let mut step = Step::json(PATH, fixture());
        step.expect=Some(serde_json::to_vec(&json!({"p_grant":id(10),"p_epoch":"2","p_secret_ref":"private:test-generation","p_game":id(4),"p_context":id(5)})).unwrap());
        let mock = Mock::new(vec![step]);
        let repo = repository(&mock);
        assert!(
            repo.controller_access(&store, &proof, Uuid::nil(), id(5))
                .is_err()
        );
        let NativeControllerAccessOutcome::Granted(access) = repo
            .controller_access(&store, &proof, id(4), id(5))
            .unwrap()
        else {
            panic!("expected source-backed AI record")
        };
        assert_eq!(access.viewer_seat_id(), id(6));
        assert_eq!(access.basis().game_id, id(4));
        let captured = mock.finish();
        assert!(!String::from_utf8_lossy(&captured[0].2).contains(&token.expose_for_delivery()));
    }
    #[test]
    fn foreign_store_and_revoked_proofs_cannot_dispatch() {
        let store = ControllerSecretStore::default();
        let (_, proof) = installed(&store);
        let other = ControllerSecretStore::default();
        let mock = Mock::new(vec![]);
        let repo = repository(&mock);
        assert!(matches!(
            repo.controller_access(&other, &proof, id(4), id(5))
                .unwrap(),
            NativeControllerAccessOutcome::Unavailable
        ));
        store.revoke(id(10)).unwrap();
        assert!(matches!(
            repo.controller_access(&store, &proof, id(4), id(5))
                .unwrap(),
            NativeControllerAccessOutcome::Unavailable
        ));
        mock.finish();
    }
    #[test]
    fn revocation_during_response_composition_denies_success() {
        let store = Arc::new(ControllerSecretStore::default());
        let (_, proof) = installed(&store);
        let revoke = store.clone();
        let mut step = Step::json(PATH, fixture());
        step.reply = Some(Box::new(move |_| {
            revoke.revoke(id(10)).unwrap();
            serde_json::to_vec(&fixture()).unwrap()
        }));
        let mock = Mock::new(vec![step]);
        let repo = repository(&mock);
        assert!(matches!(
            repo.controller_access(&store, &proof, id(4), id(5))
                .unwrap(),
            NativeControllerAccessOutcome::Unavailable
        ));
        mock.finish();
    }
    #[test]
    fn revocation_during_denial_decode_error_or_backend_error_never_delivers() {
        for mode in 0..3 {
            let store = Arc::new(ControllerSecretStore::default());
            let (_, proof) = installed(&store);
            let revoke = store.clone();
            let mut step = Step::json(PATH, fixture());
            if mode == 2 {
                step.status = 503;
            }
            step.reply = Some(Box::new(move |_| {
                revoke.revoke(id(10)).unwrap();
                if mode == 0 {
                    serde_json::to_vec(&json!({"status":"fail","data":{"actor":"unavailable"}}))
                        .unwrap()
                } else {
                    b"not-json".to_vec()
                }
            }));
            let mock = Mock::new(vec![step]);
            let repo = repository(&mock);
            assert!(matches!(
                repo.controller_access(&store, &proof, id(4), id(5))
                    .unwrap(),
                NativeControllerAccessOutcome::Unavailable
            ));
            mock.finish();
        }
    }
    #[test]
    fn foreign_identity_scope_and_inconsistent_source_are_refused() {
        let store = ControllerSecretStore::default();
        let (_, proof) = installed(&store);
        let mut cases = Vec::new();
        for (key, value) in [
            ("grantId", json!(id(12))),
            ("bindingId", json!(id(12))),
            ("bindingEpoch", json!("3")),
            ("accountId", json!(id(3))),
            ("kind", json!("human")),
            ("seatId", Value::Null),
        ] {
            let mut v = fixture();
            v["data"]["actor"][key] = value;
            cases.push(v);
        }
        for (key, value) in [
            ("gameId", json!(id(12))),
            ("contextId", json!(id(12))),
            ("runtimeId", json!("a")),
            ("matchState", json!("completed")),
            ("stateRef", json!("not-an-artifact")),
        ] {
            let mut v = fixture();
            v["data"]["source"][key] = value;
            cases.push(v);
        }
        let mut bad = fixture();
        bad["data"]["disclosure"]["live"] = json!(false);
        cases.push(bad);
        let mock = Mock::new(cases.iter().map(|v| Step::json(PATH, v.clone())).collect());
        let repo = repository(&mock);
        for _ in cases {
            assert!(
                repo.controller_access(&store, &proof, id(4), id(5))
                    .is_err()
            );
        }
        mock.finish();
    }
    #[test]
    fn canonical_denials_are_distinct_from_invalid_private_envelopes() {
        let store = ControllerSecretStore::default();
        let (_, proof) = installed(&store);
        let mut positional = fixture();
        positional["data"]["actor"] = Value::Array(vec![]);
        let mut duplicate = Step::json(PATH, fixture());
        duplicate.body = String::from_utf8(duplicate.body)
            .unwrap()
            .replace(
                "\"seatId\":",
                "\"seatId\":\"00000000-0000-0000-0000-000000000006\",\"seatId\":",
            )
            .into_bytes();
        let mock = Mock::new(vec![
            Step::json(
                PATH,
                json!({"status":"fail","data":{"actor":"unavailable"}}),
            ),
            Step::json(PATH, json!({"status":"fail","data":{"game":"unavailable"}})),
            Step::json(PATH, json!({"status":"fail","data":{"actor":"unknown"}})),
            Step::json(PATH, positional),
            duplicate,
        ]);
        let repo = repository(&mock);
        for _ in 0..2 {
            assert!(matches!(
                repo.controller_access(&store, &proof, id(4), id(5))
                    .unwrap(),
                NativeControllerAccessOutcome::Unavailable
            ));
        }
        for _ in 0..3 {
            assert!(
                repo.controller_access(&store, &proof, id(4), id(5))
                    .is_err()
            );
        }
        mock.finish();
    }
}
