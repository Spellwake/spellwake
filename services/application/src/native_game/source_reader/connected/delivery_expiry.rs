//! Genuine signed-bearer expiry across actual accepted upstream replies.
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
const AUTH: &str = "read_current_session";
const ACCESS: &str = "read_native_human_access";
const PRESENTATION: &str = "read_native_human_presentation";
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn wait_until(at: u64) {
    while now() < at {
        std::thread::sleep(Duration::from_millis(20));
    }
}
struct ReplyHold {
    armed: AtomicBool,
    held: AtomicBool,
    target: &'static str,
    ordinal: usize,
    reconnect: bool,
    expires: u64,
    game: Uuid,
    subject: Uuid,
    session: Uuid,
    trace: Mutex<Vec<String>>,
}
impl fault_proxy::Fault for ReplyHold {
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
        let ordinal = trace
            .iter()
            .filter(|previous| previous.as_str() == tag)
            .count()
            + 1;
        let previous = trace.last().cloned();
        trace.push(tag.to_owned());
        if tag != self.target || ordinal != self.ordinal || self.held.load(Ordering::SeqCst) {
            return;
        }
        assert_eq!(
            status, 200,
            "the controlled reply must actually be accepted upstream"
        );
        assert!(
            now() < self.expires,
            "upstream acceptance must precede the original expiry"
        );
        let actual: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(actual["status"], "success");
        if tag == AUTH {
            assert_eq!(actual["data"]["subjectId"], json!(self.subject));
            assert_eq!(actual["data"]["sessionId"], json!(self.session));
            assert_eq!(actual["data"]["state"], "active");
            assert_eq!(
                previous.as_deref(),
                Some(if self.reconnect {
                    "read_native_human_receipts"
                } else {
                    PRESENTATION
                })
            );
        } else {
            assert_eq!(actual["data"]["source"]["gameId"], json!(self.game));
            assert_eq!(previous.as_deref(), Some(AUTH));
        }
        drop(trace);
        // No invented success body or credential: hold only these exact accepted bytes.
        wait_until(self.expires + 1);
        self.held.store(true, Ordering::SeqCst);
    }
}

pub(super) fn run(
    reservation: &Reservation,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    phase: &str,
) {
    let (target, ordinal, reconnect, unpolled) = match phase {
        "expiry-auth-preflight" => (AUTH, 2, false, false),
        "expiry-source-preflight" => (ACCESS, 2, false, false),
        "expiry-auth-first" => (AUTH, 3, false, false),
        "expiry-source-first" => (ACCESS, 3, false, false),
        "expiry-auth-later" => (AUTH, 5, false, false),
        "expiry-source-later" => (ACCESS, 5, false, false),
        "expiry-unpolled" => (AUTH, usize::MAX, false, true),
        "expiry-reconnect-auth" => (AUTH, 2, true, false),
        "expiry-reconnect-source" => (ACCESS, 2, true, false),
        _ => panic!("unknown owned expiry case"),
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
    let bearer = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let expires: u64 = std::env::var("SPELLWAKE_NATIVE_EXPIRY")
        .unwrap()
        .parse()
        .unwrap();
    let hold = Arc::new(ReplyHold {
        armed: AtomicBool::new(false),
        held: AtomicBool::new(false),
        target,
        ordinal,
        reconnect,
        expires,
        game: reservation.game_id,
        subject: std::env::var("SPELLWAKE_NATIVE_SUBJECT")
            .unwrap()
            .parse()
            .unwrap(),
        session: std::env::var("SPELLWAKE_NATIVE_SESSION")
            .unwrap()
            .parse()
            .unwrap(),
        trace: Mutex::new(Vec::new()),
    });
    let proxy = fault_proxy::Proxy::new(&base, None, hold.clone());
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
            let value = record(&body.next().await.unwrap().unwrap());
            assert_eq!(value["status"], "success");
            assert_eq!(value["data"]["snapshot"]["basis"]["revision"], "0");
            drop(body);
        }
        assert_eq!(probe.available(), (16, 16));
    });
    assert!(
        expires > now() + 7,
        "fresh genuine credential must survive the positive setup control"
    );
    wait_until(expires - 6);
    hold.armed.store(true, Ordering::SeqCst);
    runtime.block_on(async {
        let response = probe
            .request(request(&bearer, reservation.game_id, operation, &query))
            .await;
        if ordinal == 2 {
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                json_response(response).await,
                json!({"status":"fail","data":{"session":"unauthorized"}})
            );
        } else {
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "preflight must succeed before the later controlled boundary"
            );
            let mut body = response.into_body().into_data_stream();
            if ordinal == 5 {
                assert_eq!(
                    record(&body.next().await.unwrap().unwrap())["status"],
                    "success"
                );
            }
            if unpolled {
                let count = hold.trace.lock().unwrap().len();
                // Test-only blocking wait runs outside runtime workers and does not
                // poll the owned body; the clock is real, never a fabricated JWT.
                tokio::task::spawn_blocking(move || wait_until(expires + 1))
                    .await
                    .unwrap();
                assert_eq!(hold.trace.lock().unwrap().len(), count);
            }
            let failure = record(&body.next().await.unwrap().unwrap());
            assert_eq!(
                failure,
                json!({"status":"fail","data":{"session":"unauthorized"}})
            );
            assert!(body.next().await.is_none());
            drop(body);
        }
        assert_eq!(probe.available(), (16, 16));
    });
    assert!(now() > expires);
    assert_eq!(hold.held.load(Ordering::SeqCst), !unpolled);
    assert_eq!(source_fingerprint(&repository, reservation.game_id), before);
    let trace = hold.trace.lock().unwrap().clone();
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({
            "phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,
            "selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,
            "positiveDelivery":true,"actualOriginalExpiryCrossed":true,"expectedUnauthorized":true,"sourceUnchanged":true,
            "actualAcceptedReplyHeld":!unpolled,"target":target,"targetOrdinal":if unpolled {0} else {ordinal},
            "responseTrace":trace,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false,
        })
    );
}
