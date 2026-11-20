//! Actual unexpired-session revocation after accepted private cursor/presentation IO.
use super::delivery::{json_response, record, request, source_fingerprint};
use super::*;
use crate::api::NativeStreamProbe;
use axum::http::StatusCode;
use futures::StreamExt;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Logout {
    armed: AtomicBool,
    fired: AtomicBool,
    target: &'static str,
    ordinal: usize,
    game: Uuid,
    seat: Uuid,
    base: String,
    public: String,
    bearer: String,
    trace: Mutex<Vec<String>>,
}
impl fault_proxy::Fault for Logout {
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        if !self.armed.load(Ordering::SeqCst) {
            return;
        }
        let tag = if path.starts_with("/rest/v1/rpc/") {
            path.rsplit('/').next().unwrap()
        } else {
            "storage"
        };
        let mut trace = self.trace.lock().unwrap();
        let ordinal = trace.iter().filter(|old| old.as_str() == tag).count() + 1;
        trace.push(tag.to_owned());
        if tag != self.target || ordinal != self.ordinal || self.fired.load(Ordering::SeqCst) {
            return;
        }
        assert_eq!(status, 200);
        let actual: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(actual["status"], "success");
        if tag == "issue_native_cursor_source" {
            assert_eq!(actual["data"]["version"], "native-cursor-v1");
            assert_eq!(actual["data"]["basis"]["gameId"], json!(self.game));
            assert_eq!(actual["data"]["viewerSeatId"], json!(self.seat));
        } else {
            assert_eq!(actual["data"]["version"], "native-presentation-v1");
            assert_eq!(
                actual["data"]["access"]["source"]["gameId"],
                json!(self.game)
            );
            assert_eq!(
                actual["data"]["access"]["actor"]["seatId"],
                json!(self.seat)
            );
        }
        drop(trace);
        let base = self.base.clone();
        let public = self.public.clone();
        let bearer = self.bearer.clone();
        // Join this bounded fixture action. A reqwest blocking client must not be
        // created/dropped on the proxy's async runtime worker itself.
        std::thread::spawn(move || {
            let response = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap()
                .post(format!(
                    "{}/auth/v1/logout?scope=local",
                    base.trim_end_matches('/')
                ))
                .header("apikey", public)
                .bearer_auth(bearer)
                .send()
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
        })
        .join()
        .unwrap();
        self.fired.store(true, Ordering::SeqCst);
        // Return the exact previously accepted bytes; final caller/source checks
        // must discover revocation rather than treating these bytes as a lease.
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub(super) fn run(
    reservation: &Reservation,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    phase: &str,
) {
    let (target, ordinal, reconnect) = match phase {
        "logout-reconnect-cursor" => ("issue_native_cursor_source", 1, true),
        "logout-stream-cursor" => ("issue_native_cursor_source", 1, false),
        "logout-first-presentation" => ("read_native_human_presentation", 2, false),
        "logout-later-presentation" => ("read_native_human_presentation", 3, false),
        _ => panic!("unknown owned logout case"),
    };
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
    let before = source_fingerprint(&repository, reservation.game_id);
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let bearer = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let expires: u64 = std::env::var("SPELLWAKE_NATIVE_EXPIRY")
        .unwrap()
        .parse()
        .unwrap();
    let logout = Arc::new(Logout {
        armed: AtomicBool::new(false),
        fired: AtomicBool::new(false),
        target,
        ordinal,
        game: reservation.game_id,
        seat: reservation.seat_ids[0],
        base: base.clone(),
        public,
        bearer: bearer.clone(),
        trace: Mutex::new(Vec::new()),
    });
    let proxy = fault_proxy::Proxy::new(&base, None, logout.clone());
    let probe = NativeStreamProbe::new(http::configuration(
        proxy.url(),
        &key,
        &base,
        |runtime| runtime,
        None,
    ));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let operation = if reconnect { "reconnect" } else { "stream" };
    let query = if reconnect {
        json!({"receipts":{"gameId":reservation.game_id,"contextId":reservation.context_id,"outstanding":[]},"cursor":null})
    } else {
        json!({"gameId":reservation.game_id,"contextId":reservation.context_id,"cursor":null})
    };
    runtime.block_on(async {
        let positive = probe
            .request(request(&bearer, reservation.game_id, operation, &query))
            .await;
        assert_eq!(positive.status(), StatusCode::OK);
        if reconnect {
            assert_eq!(json_response(positive).await["status"], "success");
        } else {
            let mut body = positive.into_body().into_data_stream();
            assert_eq!(
                record(&body.next().await.unwrap().unwrap())["status"],
                "success"
            );
            drop(body);
        }
        assert_eq!(probe.available(), (16, 16));
        assert!(now() < expires, "positive bearer must still be unexpired");
        logout.armed.store(true, Ordering::SeqCst);
        let response = probe
            .request(request(&bearer, reservation.game_id, operation, &query))
            .await;
        if ordinal == 1 {
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                json_response(response).await,
                json!({"status":"fail","data":{"session":"unauthorized"}})
            );
        } else {
            assert_eq!(response.status(), StatusCode::OK);
            let mut body = response.into_body().into_data_stream();
            if ordinal == 3 {
                assert_eq!(
                    record(&body.next().await.unwrap().unwrap())["status"],
                    "success"
                );
            }
            assert_eq!(
                record(&body.next().await.unwrap().unwrap()),
                json!({"status":"fail","data":{"session":"unauthorized"}})
            );
            assert!(body.next().await.is_none());
            drop(body);
        }
        assert_eq!(probe.available(), (16, 16));
    });
    assert!(logout.fired.load(Ordering::SeqCst));
    assert!(
        now() < expires,
        "expiry must not explain this unauthorized result"
    );
    assert_eq!(source_fingerprint(&repository, reservation.game_id), before);
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({
            "phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,
            "selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,
            "positiveDelivery":true,"acceptedIoBeforeLogout":true,"actualLocalLogout":true,"bearerUnexpired":true,
            "expectedUnauthorized":true,"sourceUnchanged":true,"target":target,"ordinal":ordinal,
            "responseTrace":logout.trace.lock().unwrap().clone(),"fullyQualifiedCards":0,
            "semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false,
        })
    );
}
