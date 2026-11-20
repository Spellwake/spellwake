//! Actual read unavailability and router cancellation, over unchanged owned source.
use super::delivery::{Calls, json_response, request, source_fingerprint};
use super::*;
use crate::api::NativeStreamProbe;
use axum::http::StatusCode;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

struct Unavailable {
    composed: AtomicBool,
    armed: AtomicBool,
    hits: AtomicUsize,
}
impl fault_proxy::Fault for Unavailable {
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        if path == "/rest/v1/rpc/read_native_human_access"
            && self.composed.load(Ordering::SeqCst)
            && self.armed.swap(false, Ordering::SeqCst)
        {
            assert_eq!(status, 200);
            let actual: serde_json::Value = serde_json::from_slice(body).unwrap();
            assert_eq!(actual["status"], "success");
            *body = serde_json::to_vec(&json!({"status":"fail","data":{"game":"unavailable"}}))
                .unwrap();
            self.hits.fetch_add(1, Ordering::SeqCst);
        }
    }
}
pub(super) fn unavailable(r: &Reservation, phase: &str) {
    let operation = if phase.starts_with("archive-unavailable-") {
        history_delivery::archive_operation(phase)
    } else {
        match phase.strip_prefix("read-unavailable-").unwrap() {
            "history" => "history",
            "replay" => "replay",
            _ => panic!("unknown read operation"),
        }
    };
    let repository = NativeSourceRepository::from_env().unwrap();
    let before = source_fingerprint(&repository, r.game_id);
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let mut token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let state = Arc::new(Unavailable {
        composed: AtomicBool::new(false),
        armed: AtomicBool::new(false),
        hits: AtomicUsize::new(0),
    });
    let proxy = fault_proxy::Proxy::new(&base, None, state.clone());
    let gate = state.clone();
    let (origin_url, secret) = (proxy.url().to_owned(), key.clone());
    let http = http::HttpProbe::forwarded_read(proxy.url(), &base, &key, move |runtime| {
        history_delivery::runtime(runtime, operation, &origin_url, &secret).with_read_delivery_hook(
            history_delivery::stage(operation),
            move |_, recovered, _, _| {
                assert_eq!(recovered.is_none(), operation.starts_with("archive-"));
                gate.composed.store(true, Ordering::SeqCst);
            },
        )
    });
    if operation.starts_with("archive-") {
        let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
        token = history_delivery::fresh_owner(&base, &key, &public);
    }
    let positive = history_delivery::read(&http, &token, r, operation, 200);
    state.composed.store(false, Ordering::SeqCst);
    state.armed.store(true, Ordering::SeqCst);
    assert_eq!(
        history_delivery::read(&http, &token, r, operation, 404),
        json!({"status":"fail","data":{"game":"unavailable"}})
    );
    assert!(state.composed.load(Ordering::SeqCst));
    assert_eq!(state.hits.load(Ordering::SeqCst), 1);
    assert_eq!(
        history_delivery::read(&http, &token, r, operation, 200),
        positive
    );
    assert_eq!(state.hits.load(Ordering::SeqCst), 1);
    assert_eq!(source_fingerprint(&repository, r.game_id), before);
    metrics(
        phase,
        json!({"controlledFinalAccessUnavailable":true,"targetedFinalAccessHits":1,"restoredExactPositive":true,"archiveReaderNoRecovery":operation.starts_with("archive-")}),
    );
}

struct Workers {
    armed: AtomicBool,
    entered: AtomicUsize,
    released: Mutex<usize>,
    wake: Condvar,
}
impl Workers {
    fn park(&self) {
        if !self.armed.load(Ordering::SeqCst) {
            return;
        }
        let ticket = self.entered.fetch_add(1, Ordering::SeqCst);
        let (guard, timeout) = self
            .wake
            .wait_timeout_while(
                self.released
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                Duration::from_secs(15),
                |released| *released <= ticket,
            )
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(guard);
        assert!(!timeout.timed_out(), "controlled workers must be released");
    }
    fn release_one(&self) {
        *self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = 1;
        self.wake.notify_all();
    }
    fn release(&self) {
        *self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = usize::MAX;
        self.wake.notify_all();
    }
}
struct Release(Arc<Workers>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

pub(super) fn cancellation(r: &Reservation, phase: &str) {
    let operation = match phase.strip_prefix("read-cancellation-").unwrap() {
        "history" => "history",
        "replay" => "replay",
        _ => panic!("unknown read operation"),
    };
    let repository = NativeSourceRepository::from_env().unwrap();
    let before = source_fingerprint(&repository, r.game_id);
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    // A distinct genuine session attributes the detached worker's final renewal.
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let canceled_token = history_delivery::fresh_owner(&base, &key, &public);
    let auth = crate::actor::ActorClient::new(
        crate::actor::ActorConfig::new(&base, &public, &format!("{base}/auth/v1")).unwrap(),
    )
    .unwrap();
    let original_actor = auth.resolve(&token).unwrap();
    let canceled_actor = auth.resolve(&canceled_token).unwrap();
    assert_eq!(original_actor.subject(), canceled_actor.subject());
    assert_eq!(original_actor.account(), canceled_actor.account());
    assert_ne!(original_actor.session(), canceled_actor.session());
    let canceled_session = canceled_actor.session();
    let calls = Arc::new(Calls::default());
    let proxy = fault_proxy::Proxy::new(&base, None, calls.clone());
    let workers = Arc::new(Workers {
        armed: AtomicBool::new(false),
        entered: AtomicUsize::new(0),
        released: Mutex::new(0),
        wake: Condvar::new(),
    });
    let gate = workers.clone();
    let config = http::configuration(
        proxy.url(),
        &key,
        &base,
        move |runtime| {
            runtime.with_read_delivery_hook(
                history_delivery::stage(operation),
                move |_, recovered, _, _| {
                    assert!(recovered.is_some());
                    gate.park();
                },
            )
        },
        None,
    );
    let probe = Arc::new(NativeStreamProbe::new(config));
    let payload = if operation == "history" {
        json!({"gameId":r.game_id,"contextId":r.context_id,"cursor":null,"limit":"3"})
    } else {
        super::replay_frames::request(
            r,
            &json!("0"),
            r.seat_ids[0],
            "historical-seat",
            &serde_json::Value::Null,
            3,
        )
    };
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        // Release paused blocking workers even when an assertion unwinds this future.
        let _release = Release(workers.clone());
        let positive = probe.request(request(&token, r.game_id, operation, &payload)).await;
        assert_eq!(positive.status(), StatusCode::OK);
        let positive = json_response(positive).await;
        assert_eq!(probe.available(), (16,16));
        let auth_before = calls.successful_auth().len();
        workers.armed.store(true, Ordering::SeqCst);
        let mut requests = Vec::new();
        for index in 0..16 {
            let worker_token = if index == 0 { &canceled_token } else { &token };
            let (probe, token, payload) = (probe.clone(), worker_token.clone(), payload.clone());
            let game = r.game_id;
            requests.push(tokio::spawn(async move {
                probe.request(request(&token, game, operation, &payload)).await
            }));
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            while workers.entered.load(Ordering::SeqCst) != 16 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        assert_eq!(probe.available(), (16,0));
        assert_eq!(calls.successful_auth().len(), auth_before+16);
        let canceled = requests.remove(0);
        canceled.abort();
        assert!(canceled.await.unwrap_err().is_cancelled());
        assert_eq!(probe.available(), (16,0), "caller cancellation cannot release running worker permit");
        let before_overflow = calls.initiated();
        let overflow = probe.request(request(&token, r.game_id, operation, &payload)).await;
        assert_eq!(overflow.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json_response(overflow).await,
            json!({"status":"error","message":"Application service is temporarily unavailable","code":"unavailable"}));
        assert_eq!(calls.initiated(), before_overflow, "overflow must perform no upstream work");
        workers.armed.store(false, Ordering::SeqCst);
        workers.release_one();
        tokio::time::timeout(Duration::from_secs(10), async {
            while probe.available() != (16,1) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        // Exactly one of the original16 workers completed;15 remain parked.
        let one_slot = probe.request(request(&token, r.game_id, operation, &payload)).await;
        assert_eq!(one_slot.status(), StatusCode::OK);
        assert_eq!(json_response(one_slot).await, positive);
        assert_eq!(probe.available(), (16,1));
        assert_eq!(*workers.released.lock().unwrap(), 1);
        workers.release();
        for task in requests {
            let response = task.await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(json_response(response).await, positive);
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            while probe.available() != (16,16) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        assert_eq!(calls.successful_auth().len(), auth_before+34,
            "all16 actual workers, including detached caller, finish final bearer renewal");
        let replies = calls.successful_auth();
        let detached: Vec<_> = replies[auth_before..].iter()
            .filter(|reply| reply["data"]["sessionId"] == json!(canceled_session)).collect();
        assert_eq!(detached.len(), 2, "canceled worker has its own initial and final successful session replies");
        for reply in detached {
            assert_eq!(reply["data"]["subjectId"], json!(canceled_actor.subject()));
            assert_eq!(reply["data"]["accountId"], json!(canceled_actor.account().unwrap()));
            assert_eq!(reply["data"]["method"], json!(canceled_actor.method()));
            assert_eq!(reply["data"]["state"], "active");
        }
        let restored = probe.request(request(&token, r.game_id, operation, &payload)).await;
        assert_eq!(restored.status(), StatusCode::OK);
        assert_eq!(json_response(restored).await, positive);
        assert_eq!(probe.available(), (16,16));
        let idle = calls.initiated();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert_eq!(calls.initiated(), idle, "completed and canceled JSON requests schedule no later sampling");
    });
    assert_eq!(source_fingerprint(&repository, r.game_id), before);
    metrics(
        phase,
        json!({"composedWorkers":16,"canceledCallers":1,"runningPermitRetained":true,
        "overflowWithoutIo":true,"completedWorkers":16,"singleWorkerRestoredCapacity":true,"canceledWorkerBearerReplies":2,"actualSuccessfulBearerReplies":32,
        "restoredExactPositive":true,"noLaterSamplingObservedMillis":1100}),
    );
}
fn metrics(phase: &str, extra: serde_json::Value) {
    let mut value = json!({"phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,
        "selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,
        "sourceUnchanged":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false});
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    println!("OWNED_NATIVE_SOURCE_READER_OK {value}");
}
