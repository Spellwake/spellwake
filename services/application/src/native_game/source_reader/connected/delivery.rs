//! Actual router/body lifecycle against owned Auth/SQL/Storage, without fake source proofs.
use super::*;
use crate::api::NativeStreamProbe;
use crate::player_wire::{PlayerAction, PlayerChoice, PlayerCommand, PlayerDomain};
use axum::{
    body::{Body, to_bytes},
    extract::Request,
    http::{StatusCode, header},
    response::Response,
};
use futures::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Calls {
    total: AtomicUsize,
    initiated: AtomicUsize,
    auth: AtomicUsize,
    successful_auth: Mutex<Vec<serde_json::Value>>,
    after_rpc: Mutex<Option<&'static str>>,
    pending_auth: AtomicBool,
    last_rpc: Mutex<Option<String>>,
    hold_auth: AtomicBool,
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    released: Mutex<bool>,
    release: Condvar,
}
impl Calls {
    pub(super) fn successful_auth(&self) -> Vec<serde_json::Value> {
        self.successful_auth.lock().unwrap().clone()
    }
    pub(super) fn auth_count(&self) -> usize {
        self.auth.load(Ordering::SeqCst)
    }
    pub(super) fn last_rpc(&self) -> Option<String> {
        self.last_rpc.lock().unwrap().clone()
    }
    pub(super) fn initiated(&self) -> usize {
        self.initiated.load(Ordering::SeqCst)
    }
    pub(super) fn count(&self) -> usize {
        self.total.load(Ordering::SeqCst)
    }
    pub(super) fn arm(&self) -> tokio::sync::oneshot::Receiver<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        *self.entered.lock().unwrap() = Some(tx);
        *self.released.lock().unwrap() = false;
        self.hold_auth.store(true, Ordering::SeqCst);
        rx
    }
    fn arm_after(&self, path: &'static str) -> tokio::sync::oneshot::Receiver<()> {
        *self.after_rpc.lock().unwrap() = Some(path);
        self.arm()
    }
    pub(super) fn arm_after_presentation(&self) -> tokio::sync::oneshot::Receiver<()> {
        self.arm_after("/rest/v1/rpc/read_native_human_presentation")
    }
    pub(super) fn arm_after_receipts(&self) -> tokio::sync::oneshot::Receiver<()> {
        self.arm_after("/rest/v1/rpc/read_native_human_receipts")
    }
    pub(super) fn arm_pending_after_presentation(&self) -> tokio::sync::oneshot::Receiver<()> {
        self.pending_auth.store(true, Ordering::SeqCst);
        self.arm_after_presentation()
    }
    pub(super) fn resume(&self) {
        *self.released.lock().unwrap() = true;
        self.release.notify_all();
    }
}
impl fault_proxy::Fault for Calls {
    fn request(&self, _: &reqwest::Method, _: &str, _: &mut Vec<u8>) -> Option<u16> {
        self.initiated.fetch_add(1, Ordering::SeqCst);
        None
    }
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        self.total.fetch_add(1, Ordering::SeqCst);
        if path == "/rest/v1/rpc/read_current_session" {
            self.auth.fetch_add(1, Ordering::SeqCst);
            if status == 200
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(body)
                && value["status"] == "success"
            {
                self.successful_auth.lock().unwrap().push(value);
            }
        }
        let mut last = self.last_rpc.lock().unwrap();
        let follows_target = self
            .after_rpc
            .lock()
            .unwrap()
            .is_none_or(|target| last.as_deref() == Some(target));
        *last = Some(path.to_owned());
        drop(last);
        if path == "/rest/v1/rpc/read_current_session"
            && follows_target
            && self.hold_auth.swap(false, Ordering::SeqCst)
        {
            assert_eq!(status, 200);
            let actual: serde_json::Value = serde_json::from_slice(body).unwrap();
            assert_eq!(actual["status"], "success");
            assert_eq!(
                actual["data"]["state"],
                if self.pending_auth.load(Ordering::SeqCst) {
                    "deletion_pending"
                } else {
                    "active"
                }
            );
            self.entered
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(())
                .unwrap();
            let (_released, timed) = self
                .release
                .wait_timeout_while(
                    self.released.lock().unwrap(),
                    Duration::from_secs(5),
                    |released| !*released,
                )
                .unwrap();
            assert!(
                !timed.timed_out(),
                "controlled accepted Auth reply must be released"
            );
        }
    }
}
pub(super) fn request(
    token: &str,
    game: Uuid,
    operation: &str,
    payload: &impl Serialize,
) -> Request {
    Request::builder()
        .method("POST")
        .uri(format!("/v1/games/{game}/{operation}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(payload).unwrap()))
        .unwrap()
}
async fn stream(probe: &NativeStreamProbe, token: &str, reservation: &Reservation) -> Response {
    let response = probe
        .request(request(
            token,
            reservation.game_id,
            "stream",
            &json!({
                "gameId":reservation.game_id,"contextId":reservation.context_id,"cursor":null,
            }),
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    response
}
pub(super) fn record(bytes: &[u8]) -> serde_json::Value {
    let json = bytes
        .strip_prefix(b"data: ")
        .unwrap()
        .strip_suffix(b"\n\n")
        .unwrap();
    assert!(json.len() <= 2 * crate::player_wire::MAX_PLAYER_WIRE_BYTES);
    serde_json::from_slice(json).unwrap()
}
pub(super) async fn json_response(response: Response) -> serde_json::Value {
    serde_json::from_slice(
        &to_bytes(
            response.into_body(),
            2 * crate::player_wire::MAX_PLAYER_WIRE_BYTES,
        )
        .await
        .unwrap(),
    )
    .unwrap()
}
pub(super) fn source_fingerprint(repository: &NativeSourceRepository, game: Uuid) -> [u8; 32] {
    let source = repository.read(game).unwrap().unwrap();
    let observed = json!({"revision":source.accepted_revision(),"checkpoint":source.checkpoint_ids(),
        "frontier":source.last_event_sequence(),"operational":source.operational_basis(),
        "clocks":source.clocks().collect::<Vec<_>>()});
    Sha256::digest(serde_json::to_vec(&observed).unwrap()).into()
}
pub(super) async fn cut(game: Uuid) -> [u8; 32] {
    tokio::task::spawn_blocking(move || {
        source_fingerprint(&NativeSourceRepository::from_env().unwrap(), game)
    })
    .await
    .unwrap()
}
async fn choose(probe: &NativeStreamProbe, token: &str, snapshot: &serde_json::Value, game: Uuid) {
    let snapshot: crate::player_wire::PlayerSnapshot =
        serde_json::from_value(snapshot.clone()).unwrap();
    let decision = snapshot
        .decisions
        .first()
        .expect("real opening decision required");
    let PlayerDomain::Options { options, .. } = &decision.domain else {
        panic!("real finite choice required")
    };
    let original = PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: snapshot.basis,
        action: PlayerAction::Choose {
            decision: decision.decision.clone(),
            response: PlayerChoice::Option {
                option: options[0].handle.clone(),
            },
        },
    };
    let response = probe
        .request(request(token, game, "commands", &original))
        .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let acknowledgement = json_response(response).await;
    assert_eq!(acknowledgement["status"], "success");
}
async fn unchanged_calls(calls: &Calls) {
    let before = calls.count();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        calls.count(),
        before,
        "unpolled/dropped body must not schedule a sample"
    );
}

pub(super) fn run(
    reservation: &Reservation,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let recovered = repository
        .recover_with_history(
            reservation.game_id,
            store,
            programs,
            NativeRecoveryBudget::new(100, 4_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered.history_evidence(),
        NativeHistoryEvidence::TransitionV1
    );
    super::selected_origin(
        recovered.checkpoint().provenance(),
        recovered.checkpoint().state(),
        reservation,
    );
    assert_eq!(recovered.source().accepted_revision(), &Amount::from(0));
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let peer = std::env::var("SPELLWAKE_NATIVE_SOURCE_PEER_BEARER").unwrap();
    let calls = Arc::new(Calls::default());
    let proxy = fault_proxy::Proxy::new(&base, None, calls.clone());
    let config = http::configuration(
        proxy.url(),
        &key,
        &base,
        |runtime| runtime,
        Some(NativeHostOwner::new("fixture:source-reader").unwrap()),
    );
    let probe = NativeStreamProbe::new(config);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let initial = source_fingerprint(&repository, reservation.game_id);
    runtime.block_on(async {
        // Actual preflight is complete, while first body poll has not happened.
        let response = stream(&probe, &token, reservation).await;
        assert_eq!(probe.available(), (15, 16));
        assert!(calls.count() > 0);
        unchanged_calls(&calls).await;
        drop(response);
        assert_eq!(probe.available(), (16, 16));
        unchanged_calls(&calls).await;

        // All16prepared bodies hold distinct lifetime permits but no blocking work.
        let mut slow = Vec::new();
        for _ in 0..16 {
            slow.push(stream(&probe, &token, reservation).await);
        }
        assert_eq!(probe.available(), (0, 16));
        let before = calls.count();
        let overloaded = probe
            .request(request(
                &token,
                reservation.game_id,
                "stream",
                &json!({
                    "gameId":reservation.game_id,"contextId":reservation.context_id,"cursor":null,
                }),
            ))
            .await;
        assert_eq!(overloaded.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json_response(overloaded).await["status"], "error");
        assert_eq!(
            calls.count(),
            before,
            "overload must precede expensive preflight"
        );
        drop(slow);
        assert_eq!(probe.available(), (16, 16));
        unchanged_calls(&calls).await;

        assert_eq!(cut(reservation.game_id).await, initial);

        // An unpolled next record cannot run a detached producer; the cadence
        // begins after actual application yield, without a TCP prefetch race.
        let response = stream(&probe, &token, reservation).await;
        let mut body = response.into_body().into_data_stream();
        let first = record(&body.next().await.unwrap().unwrap());
        assert_eq!(first["status"], "success");
        assert_eq!(first["data"]["snapshot"]["basis"]["revision"], "0");
        unchanged_calls(&calls).await;
        let at = Instant::now();
        let equal = record(&body.next().await.unwrap().unwrap());
        assert!(at.elapsed() >= Duration::from_secs(1));
        assert_eq!(first, equal);
        choose(
            &probe,
            &token,
            &first["data"]["snapshot"],
            reservation.game_id,
        )
        .await;
        let revision_one = cut(reservation.game_id).await;
        let advanced = record(&body.next().await.unwrap().unwrap());
        assert_eq!(advanced["status"], "success");
        assert_eq!(advanced["data"]["snapshot"]["basis"]["revision"], "1");
        assert_eq!(
            advanced["data"]["cursor"]["basis"],
            advanced["data"]["snapshot"]["basis"]
        );
        let equal_after = record(&body.next().await.unwrap().unwrap());
        assert_eq!(advanced, equal_after);
        drop(body);
        assert_eq!(probe.available(), (16, 16));
        unchanged_calls(&calls).await;

        assert_eq!(cut(reservation.game_id).await, revision_one);

        // A successfully prepared old cut MUST fail at first poll after a real
        // peer command, rather than quietly substituting the newer snapshot.
        let prepared = stream(&probe, &token, reservation).await;
        let peer_view = probe
            .request(request(
                &peer,
                reservation.game_id,
                "view",
                &json!({
                    "gameId":reservation.game_id,"contextId":reservation.context_id,
                }),
            ))
            .await;
        assert_eq!(peer_view.status(), StatusCode::OK);
        let peer_view = json_response(peer_view).await;
        choose(
            &probe,
            &peer,
            &peer_view["data"]["snapshot"],
            reservation.game_id,
        )
        .await;
        let revision_two = cut(reservation.game_id).await;
        let mut stale = prepared.into_body().into_data_stream();
        let refused = record(&stale.next().await.unwrap().unwrap());
        assert_eq!(refused, json!({"status":"fail","data":{"basis":"stale"}}));
        assert!(stale.next().await.is_none());
        drop(stale);
        assert_eq!(probe.available(), (16, 16));

        // Drop while a demand-driven sleep is pending: no worker is reserved.
        let response = stream(&probe, &token, reservation).await;
        let mut body = response.into_body().into_data_stream();
        assert_eq!(
            record(&body.next().await.unwrap().unwrap())["status"],
            "success"
        );
        let mut next = Box::pin(body.next());
        assert!(futures::poll!(&mut next).is_pending());
        assert_eq!(probe.available(), (15, 16));
        drop(next);
        drop(body);
        assert_eq!(probe.available(), (16, 16));
        unchanged_calls(&calls).await;

        // Drop during an actual accepted upstream Auth response: the body releases
        // its lifetime permit immediately, the running closure retains its worker.
        let response = stream(&probe, &token, reservation).await;
        let entered = calls.arm();
        let task = tokio::spawn(async move {
            let mut body = response.into_body().into_data_stream();
            body.next().await
        });
        tokio::time::timeout(Duration::from_secs(4), entered)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(probe.available(), (15, 15));
        task.abort();
        let _ = task.await;
        assert_eq!(probe.available(), (16, 15));
        calls.resume();
        tokio::time::timeout(Duration::from_secs(5), async {
            while probe.available() != (16, 16) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        unchanged_calls(&calls).await;
        assert_eq!(cut(reservation.game_id).await, revision_two);
    });
    let after = repository.read(reservation.game_id).unwrap().unwrap();
    assert_eq!(after.accepted_revision(), &Amount::from(2));
    assert_ne!(
        source_fingerprint(&repository, reservation.game_id),
        initial
    );
    // Exactly two commands were intentionally submitted. The parent verifies the
    // SQL original counts, while this immutable fingerprint covers the final reads.
    let final_cut = source_fingerprint(&repository, reservation.game_id);
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(
        source_fingerprint(&repository, reservation.game_id),
        final_cut
    );
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({
            "phase":"delivery","actualNativeTransitionEvidence":true,"initialVersion2":true,
            "selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,
            "actualControlledBodyPolling":true,"actualFirstPollStale":true,"actualStreamCursorAdvanceSequence":true,
            "actualStreamCapacity":true,"actualStreamPrePollDrop":true,"actualStreamDelayDrop":true,
            "actualStreamInFlightDrop":true,"actualStreamWorkPermitRetained":true,"noDetachedSamples":true,
            "actualStreamExpiryQualified":false,"actualStreamCosmeticRacesQualified":false,
            "submittedPlayerCommands":2,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,
            "deckFormatAdmissionQualified":false,"displayQualified":false,
        })
    );
}
