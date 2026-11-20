//! Genuine needs-registration caller intake, distinct from retirement/native absence.
use super::delivery::{Calls, json_response, request, source_fingerprint};
use super::*;
use crate::{
    actor::{ActorClient, ActorConfig, ActorState, RequestActor},
    api::NativeStreamProbe,
};
use axum::http::StatusCode;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub(super) fn unregistered(base: &str, key: &str, public: &str) -> (String, RequestActor) {
    let token =
        history_delivery::fresh_identity(base, key, public, "native-unregistered@example.invalid");
    let actor =
        ActorClient::new(ActorConfig::new(base, public, &format!("{base}/auth/v1")).unwrap())
            .unwrap()
            .resolve(&token)
            .unwrap();
    assert!(actor.account().is_none());
    assert_eq!(actor.state(), ActorState::NeedsRegistration);
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    let source = lifecycle_reads::sql(
        &database,
        "SELECT jsonb_build_object('authSubjects',(SELECT count(*) FROM auth.users WHERE id=$1),'applicationAccounts',(SELECT count(*) FROM app.account WHERE auth_subject=$1))",
        &[&actor.subject()],
    );
    assert_eq!(source, json!({"authSubjects":1,"applicationAccounts":0}));
    println!(
        "OWNED_NATIVE_UNREGISTERED_SOURCE {}",
        json!({"subject":actor.subject(),"source":source})
    );
    (token, actor)
}
pub(super) fn replies(values: &[Value], actor: &RequestActor, count: usize) {
    assert_eq!(values.len(), count);
    assert!(actor.account().is_none());
    for reply in values {
        assert_eq!(reply["status"], "success");
        assert_eq!(reply["data"]["subjectId"], json!(actor.subject()));
        assert_eq!(reply["data"]["sessionId"], json!(actor.session()));
        assert_eq!(reply["data"]["accountId"], Value::Null);
        assert_eq!(reply["data"]["method"], json!(actor.method()));
        assert_eq!(reply["data"]["state"], "needs_registration");
    }
}
pub(super) fn run(r: &Reservation, phase: &str) {
    let operation = match phase {
        "read-account-history" => "history",
        "read-account-replay" => "replay",
        _ => panic!("unknown account intake"),
    };
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let repository = NativeSourceRepository::from_env().unwrap();
    let before = source_fingerprint(&repository, r.game_id);
    let calls = Arc::new(Calls::default());
    let proxy = fault_proxy::Proxy::new(&base, None, calls.clone());
    let composed = Arc::new(AtomicUsize::new(0));
    let observed = composed.clone();
    let probe = NativeStreamProbe::new(http::configuration(
        proxy.url(),
        &key,
        &base,
        move |runtime| {
            runtime.with_read_delivery_hook(
                history_delivery::stage(operation),
                move |_, recovered, _, _| {
                    assert!(recovered.is_some());
                    observed.fetch_add(1, Ordering::SeqCst);
                },
            )
        },
        None,
    ));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let payload = history_delivery::query(r, operation);
    let owner = history_delivery::fresh_owner(&base, &key, &public);
    let positive = runtime.block_on(async {
        let response = probe
            .request(request(&owner, r.game_id, operation, &payload))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        json_response(response).await
    });
    let (token, actor) = unregistered(&base, &key, &public);
    let initial = (
        calls.initiated(),
        calls.auth_count(),
        calls.successful_auth().len(),
        composed.load(Ordering::SeqCst),
    );
    runtime.block_on(async {
        let response = probe
            .request(request(&token, r.game_id, operation, &payload))
            .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            json_response(response).await,
            json!({"status":"fail","data":{"account":"unavailable"}})
        );
    });
    replies(&calls.successful_auth()[initial.2..], &actor, 2);
    assert_eq!(calls.auth_count() - initial.1, 2);
    assert_eq!(
        calls.initiated() - initial.0,
        2,
        "only initial and final session RPCs; no native/Storage initiation"
    );
    assert_eq!(composed.load(Ordering::SeqCst), initial.3);
    runtime.block_on(async {
        let response = probe
            .request(request(&owner, r.game_id, operation, &payload))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_response(response).await, positive);
    });
    assert_eq!(source_fingerprint(&repository, r.game_id), before);
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"genuineAccountUnavailable":true,"unregisteredSubject":actor.subject(),"acceptedUnregisteredReplies":2,"noNativeIo":true,"restoredExactPositive":true,"sourceUnchanged":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
