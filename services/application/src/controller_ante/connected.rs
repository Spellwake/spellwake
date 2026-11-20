//! Actual private PostgREST + canonical SQL + local credential possession.
//! Deck/provider/Ready facts are controlled metadata, not native qualification.
use super::*;
use crate::{
    amount::Amount,
    test_http::{Mock, Step},
};
use std::sync::Arc;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fixture {
    matched: Uuid,
    seat: Uuid,
    grant: Uuid,
    binding: Uuid,
    epoch: Amount,
    reference: String,
    revision: Uuid,
    configuration: Uuid,
    foreign_matched: Uuid,
    foreign_grant: Uuid,
    foreign_binding: Uuid,
    foreign_epoch: Amount,
    foreign_reference: String,
}
fn source(f: &Fixture, revision: u64, coins: u64) -> AiAnteStake {
    AiAnteStake {
        request_id: Uuid::new_v4(),
        match_id: f.matched,
        seat_id: f.seat,
        expected_terms_revision: revision.into(),
        coins: coins.into(),
    }
}
fn accepted(response: ApiResponse<CommandDisposition<OfferAck>>) -> OfferAck {
    let ApiResponse::Success {
        data: CommandDisposition::Accepted { ack },
    } = response
    else {
        panic!("expected canonical accepted original")
    };
    ack
}
fn unavailable<T>(response: ApiResponse<T>) {
    assert!(
        matches!(response,ApiResponse::Fail{data}if data.get("actor").is_some_and(|v|v=="unavailable"))
    );
}
fn backend() -> BackendClient {
    BackendClient::new(
        BackendConfig::from_env().unwrap(),
        "ante-connected-fixture",
        Duration::from_secs(10),
    )
    .unwrap()
}
fn rpc(name: &str, payload: Value) -> Value {
    let result: Value = backend().rpc(name, &payload).unwrap();
    assert_eq!(result["status"], "success");
    result["data"].clone()
}
fn forwarded(name: &'static str) -> Step {
    let mut step = Step::json(&format!("/rest/v1/rpc/{name}"), Value::Null);
    step.reply = Some(Box::new(move |body| {
        let payload: Value = serde_json::from_slice(body).unwrap();
        backend().rpc_bytes_bounded(name, &payload, LIMIT).unwrap()
    }));
    step
}
#[test]
#[ignore = "requires unique owned local Supabase realm and admitted Collection metadata fixture"]
fn owned_private_ai_ante_possession_originals_and_economics() {
    let fixture = std::fs::read(std::env::var("SPELLWAKE_AI_ANTE_FIXTURE").unwrap()).unwrap();
    let f: Fixture = crate::decode_wire_object(&fixture).unwrap();
    let repo = ControllerAnteRepository::from_env().unwrap();
    let store = ControllerSecretStore::default();
    let token = store
        .install(
            f.grant,
            f.binding,
            f.epoch.clone(),
            f.reference.parse().unwrap(),
        )
        .unwrap();
    let proof = store.authenticate(f.grant, &token).unwrap().unwrap();
    let ApiResponse::Success { data: context } = repo.context(&store, &proof, f.matched).unwrap()
    else {
        panic!()
    };
    assert_eq!(context.seat_id, f.seat);
    assert!(context.commitment_id.is_none());
    assert!(context.exposure_id.is_none());
    let first = source(&f, 0, 100);
    let ack = accepted(repo.execute(&store, &proof, &first).unwrap());
    assert_eq!(ack.terms_revision, 1.into());
    let ApiResponse::Success { data: context } = repo.context(&store, &proof, f.matched).unwrap()
    else {
        panic!()
    };
    assert_eq!(context.commitment_id, Some(ack.resource_id));
    assert!(context.exposure_id.is_some());
    let changed = source(&f, 1, 50);
    // Forward all four actual private RPCs to the owned gateway; drop only the
    // first submit body after SQL has already committed. No fabricated SQL reply.
    let mut steps = Vec::new();
    for name in [
        "read_controller_ai_ante_original",
        "read_controller_ai_ante_context",
        "submit_controller_ai_ante",
        "read_controller_ai_ante_original",
    ] {
        let mut step = forwarded(name);
        step.drop_ack = name == "submit_controller_ai_ante";
        steps.push(step);
    }
    let mock = Mock::new(steps);
    let proxy = ControllerAnteRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "ante-connected-fault",
            Duration::from_secs(10),
        )
        .unwrap(),
    );
    assert_eq!(
        accepted(proxy.execute(&store, &proof, &changed).unwrap()).terms_revision,
        2.into()
    );
    let captured = mock.finish();
    assert_eq!(
        captured
            .iter()
            .filter(|(p, _, _)| p.ends_with("submit_controller_ai_ante"))
            .count(),
        1
    );
    assert!(
        captured
            .iter()
            .all(|(_, _, body)| !String::from_utf8_lossy(body)
                .contains(&token.expose_for_delivery()))
    );
    assert_eq!(
        accepted(repo.execute(&store, &proof, &source(&f, 2, 0)).unwrap()).terms_revision,
        3.into()
    );
    let reoffer = source(&f, 3, 300);
    // An independent live local store models a process that loses its authority
    // observation after the actual SQL commit, before result delivery.
    let late_store = Arc::new(ControllerSecretStore::default());
    let late_token = late_store
        .install(
            f.grant,
            f.binding,
            f.epoch.clone(),
            f.reference.parse().unwrap(),
        )
        .unwrap();
    let late = late_store
        .authenticate(f.grant, &late_token)
        .unwrap()
        .unwrap();
    let revoke = late_store.clone();
    let mut last = forwarded("submit_controller_ai_ante");
    let reply = last.reply.take().unwrap();
    let old_grant = f.grant;
    last.reply = Some(Box::new(move |body| {
        let committed = reply(body);
        revoke.revoke(old_grant).unwrap();
        committed
    }));
    let mock = Mock::new(vec![
        forwarded("read_controller_ai_ante_original"),
        forwarded("read_controller_ai_ante_context"),
        last,
    ]);
    let proxy = ControllerAnteRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "ante-late-denial",
            Duration::from_secs(10),
        )
        .unwrap(),
    );
    unavailable(proxy.execute(&late_store, &late, &reoffer).unwrap());
    assert_eq!(mock.finish().len(), 3);
    assert_eq!(
        accepted(repo.original(&store, &proof, &reoffer).unwrap()).terms_revision,
        4.into()
    );
    let recovered = accepted(repo.execute(&store, &proof, &first).unwrap());
    assert_eq!(recovered.resource_id, ack.resource_id);
    assert_eq!(recovered.terms_hash, ack.terms_hash);
    assert_eq!(recovered.terms_revision, ack.terms_revision);
    let mut conflict = first.clone();
    conflict.coins = 101.into();
    assert!(
        matches!(repo.execute(&store,&proof,&conflict).unwrap(),ApiResponse::Fail{data}if data.get("request").is_some_and(|v|v=="conflict"))
    );
    let stale = source(&f, 0, 50);
    assert!(matches!(
        repo.execute(&store, &proof, &stale).unwrap(),
        ApiResponse::Success {
            data: CommandDisposition::Rejected { .. }
        }
    ));
    assert!(matches!(
        repo.execute(&store, &proof, &stale).unwrap(),
        ApiResponse::Success {
            data: CommandDisposition::Rejected { .. }
        }
    ));
    // A current, independently valid foreign-match grant still cannot fund this seat.
    let foreign_token = store
        .install(
            f.foreign_grant,
            f.foreign_binding,
            f.foreign_epoch.clone(),
            f.foreign_reference.parse().unwrap(),
        )
        .unwrap();
    let foreign = store
        .authenticate(f.foreign_grant, &foreign_token)
        .unwrap()
        .unwrap();
    assert!(matches!(
        repo.context(&store, &foreign, f.foreign_matched).unwrap(),
        ApiResponse::Success { .. }
    ));
    unavailable(repo.original(&store, &foreign, &first).unwrap());
    let unknown_grant = Uuid::new_v4();
    let bad = store
        .install(
            unknown_grant,
            f.binding,
            f.epoch.clone(),
            format!("fixture:unknown:{unknown_grant}").parse().unwrap(),
        )
        .unwrap();
    let unknown = store.authenticate(unknown_grant, &bad).unwrap().unwrap();
    unavailable(repo.original(&store, &unknown, &first).unwrap());
    let wrong_store = ControllerSecretStore::default();
    let bad = wrong_store
        .install(
            f.grant,
            f.binding,
            f.epoch.checked_add(&1.into()).unwrap(),
            f.reference.parse().unwrap(),
        )
        .unwrap();
    let wrong = wrong_store.authenticate(f.grant, &bad).unwrap().unwrap();
    unavailable(repo.original(&wrong_store, &wrong, &first).unwrap());
    // Rebind canonically while the old local proof is still current.
    let rebound = rpc(
        "rebind_ai_runner",
        json!({"p_binding":f.binding,"p_epoch":f.epoch,"p_proof":"fixture:ante-connected-rebind","p_request":Uuid::new_v4()}),
    );
    unavailable(repo.execute(&store, &proof, &first).unwrap());
    unavailable(repo.execute(&store, &proof, &source(&f, 4, 50)).unwrap());
    rpc(
        "observe_ai_stop",
        json!({"p_binding":f.binding,"p_epoch":f.epoch,"p_stop":rebound["previousStopId"],"p_proof":"fixture:ante-process-absent","p_request":Uuid::new_v4()}),
    );
    rpc(
        "observe_ai_provider",
        json!({"p_binding":f.binding,"p_epoch":rebound["epoch"],"p_start":rebound["startKey"],"p_outcome":"running","p_provider_ref":format!("fixture:ante-connected-process:{}",Uuid::new_v4()),"p_proof":"fixture:ante-provider-observed","p_request":Uuid::new_v4()}),
    );
    let reference = format!("fixture:ante-connected-replacement:{}", Uuid::new_v4());
    let issued = rpc(
        "issue_ai_grant",
        json!({"p_binding":f.binding,"p_epoch":rebound["epoch"],"p_secret_ref":reference,"p_request":Uuid::new_v4()}),
    );
    let grant: Uuid = serde_json::from_value(issued["resourceId"].clone()).unwrap();
    rpc(
        "observe_ai_grant_delivery",
        json!({"p_grant":grant,"p_expected":"0","p_outcome":"delivered","p_proof":"fixture:ante-delivery-observed","p_request":Uuid::new_v4()}),
    );
    rpc(
        "commit_ai_ready_confirmation",
        json!({"p_grant":grant,"p_epoch":rebound["epoch"],"p_revision":f.revision,"p_qualification":"fixture:ante-deck-observed","p_authentication":"fixture:ante-auth-observed","p_configuration":f.configuration,"p_request":Uuid::new_v4()}),
    );
    let epoch: Amount = serde_json::from_value(rebound["epoch"].clone()).unwrap();
    let new_token = store
        .install(grant, f.binding, epoch, reference.parse().unwrap())
        .unwrap();
    let new = store.authenticate(grant, &new_token).unwrap().unwrap();
    let recovered = accepted(repo.execute(&store, &new, &first).unwrap());
    assert_eq!(recovered.terms_hash, ack.terms_hash);
    assert_eq!(recovered.terms_revision, ack.terms_revision);
    store.revoke(grant).unwrap();
    unavailable(repo.context(&store, &new, f.matched).unwrap());
    let result = json!({"actualPossessionAndCanonicalRpc":true,"reservedCollectionContext":true,"fourOriginalEconomicChanges":true,"lostAckRecoveredWithoutResubmit":true,"historicalOriginalAfterLaterTerms":true,"changedSourceConflict":true,"validForeignGrantDenied":true,"canonicalRebindOldLocalProofDenied":true,"replacementOriginalRecovery":true,"localRevocationDenied":true,"rawCredentialExcludedFromRpc":true,"postCommitLocalDenialAndCanonicalRecovery":true,"canonicalEconomicRejectionRecovered":true,"unknownAndWrongEpochDenied":true});
    std::fs::write(
        std::env::var("SPELLWAKE_AI_ANTE_RESULT").unwrap(),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
}
