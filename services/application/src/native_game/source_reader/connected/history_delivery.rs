//! Actual late history/replay source and session boundaries on owned originals.
use super::delivery::source_fingerprint;
use super::*;
use crate::actor::{ActorClient, ActorConfig};
use crate::provider_json::ObjectResponse;
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub(super) fn query(r: &Reservation, operation: &str) -> Value {
    let history = json!({"gameId":r.game_id,"contextId":r.context_id,"cursor":null,"limit":"3"});
    if operation == "history" {
        history
    } else {
        json!({"gameId":r.game_id,"contextId":r.context_id,"atEventSequence":"0",
            "perspectiveSeatId":r.seat_ids[usize::from(operation == "archive-full")],
            "mode":if operation == "archive-full" {"completed-full-information"} else {"historical-seat"},"history":history})
    }
}
pub(super) fn read(
    http: &http::HttpProbe,
    token: &str,
    r: &Reservation,
    operation: &str,
    status: u16,
) -> Value {
    let body = query(r, operation);
    if operation == "history" {
        http.history(Some(token), r.game_id, &body, status)
    } else {
        http.replay(Some(token), r.game_id, &body, status)
    }
}
pub(super) fn stage(operation: &str) -> TestDeliveryStage {
    if operation == "history" {
        TestDeliveryStage::HistoryCompose
    } else if operation.starts_with("archive-") {
        TestDeliveryStage::ArchiveReplayCompose
    } else {
        TestDeliveryStage::LiveReplayCompose
    }
}
/// Existing archive reader under deliberately incompatible current programs.
pub(super) fn runtime(
    runtime: NativeGameRuntime,
    operation: &str,
    base: &str,
    key: &str,
) -> NativeGameRuntime {
    if operation.starts_with("archive-") {
        NativeGameRuntime::with_pool(base, key, "resolution-selection").unwrap()
    } else {
        runtime
    }
}
pub(super) fn archive_operation(phase: &str) -> &'static str {
    if phase.ends_with("completed-full") {
        "archive-full"
    } else if phase.ends_with("completed-own") || phase.ends_with("incident-own") {
        "archive-own"
    } else {
        panic!("unknown archive perspective")
    }
}
fn pause(repository: &NativeSourceRepository, game: Uuid) {
    let source = repository.read(game).unwrap().unwrap();
    let (owner, epoch, revision, _, _) = source.operational_basis();
    let at = source.clocks().map(|row| row.4).max().unwrap();
    let ObjectResponse(ApiResponse::Success { .. }): ObjectResponse<Value> = repository
        .backend
        .rpc(
            "pause_game_incident",
            &json!({"p_game":game,"p_owner":owner,"p_epoch":epoch,
            "p_operational_revision":revision,"p_incident_at":at,"p_kind":"storage",
            "p_proof":"fixture:history-delivery-pause","p_request":Uuid::new_v4()}),
        )
        .unwrap()
    else {
        panic!("actual incident pause required")
    };
}
fn restore(repository: &NativeSourceRepository, game: Uuid) {
    let source = repository.read(game).unwrap().unwrap();
    let (owner, epoch, revision, _, _) = source.operational_basis();
    let ObjectResponse(ApiResponse::Success { .. }): ObjectResponse<Value> = repository
        .backend
        .rpc(
            "restore_game_checkpoint",
            &json!({"p_game":game,"p_epoch":epoch,"p_operational_revision":revision,
            "p_checkpoint":source.checkpoint_ids().0,"p_new_owner":owner,
            "p_restoration_ref":"fixture:history-delivery-restored","p_request":Uuid::new_v4()}),
        )
        .unwrap()
    else {
        panic!("actual incident restoration required")
    };
}
pub(super) fn logout(base: &str, public: &str, bearer: &str) {
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
}
pub(super) fn fresh_owner(base: &str, key: &str, public: &str) -> String {
    fresh_identity(base, key, public, "native-artifact-owner@example.invalid")
}
pub(super) fn fresh_identity(base: &str, key: &str, public: &str, email: &str) -> String {
    assert_eq!(
        reqwest::Url::parse(base).unwrap().host_str(),
        Some("127.0.0.1")
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let generated = client
        .post(format!("{base}/auth/v1/admin/generate_link"))
        .header("apikey", key)
        .json(&json!({"type":"magiclink","email":email}))
        .send()
        .unwrap();
    assert_eq!(generated.status(), reqwest::StatusCode::OK);
    let generated: Value = generated.json().unwrap();
    let properties = generated.get("properties").unwrap_or(&generated);
    let signed=client.post(format!("{base}/auth/v1/verify")).header("apikey",public)
        .json(&json!({"email":email,"token":properties["email_otp"],"type":properties["verification_type"]}))
        .send().unwrap();
    assert_eq!(signed.status(), reqwest::StatusCode::OK);
    let signed: Value = signed.json().unwrap();
    signed["access_token"].as_str().unwrap().to_owned()
}
#[derive(Clone, Copy)]
enum SessionChange {
    Logout,
    Expire(u64),
}
fn change_session(change: SessionChange, base: &str, public: &str, token: &str) {
    match change {
        SessionChange::Logout => logout(base, public, token),
        SessionChange::Expire(at) => {
            assert!(
                now() < at,
                "the original credential must still be valid at the controlled boundary"
            );
            wait_until(at + 1);
        }
    }
}
pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
pub(super) fn wait_until(at: u64) {
    while now() < at {
        std::thread::sleep(Duration::from_millis(20));
    }
}
/// Observe the real signed deadline only after canonical caller authentication.
pub(super) fn fresh_owner_deadline(base: &str, key: &str, public: &str) -> (String, u64) {
    let token = fresh_owner(base, key, public);
    let auth =
        ActorClient::new(ActorConfig::new(base, public, &format!("{base}/auth/v1")).unwrap())
            .unwrap();
    let actor = auth.resolve(&token).unwrap();
    let at = actor.test_verified_bearer_expiry();
    assert!(
        at <= now() + 31,
        "actual credential retains the configured short lifetime"
    );
    assert!(at > now() + 7, "fresh credential survives controlled setup");
    (token, at)
}
#[derive(Clone)]
struct SessionTarget {
    token: String,
    change: SessionChange,
}
struct FinalBearerChange {
    armed: AtomicBool,
    composed: AtomicBool,
    accepted_access: AtomicBool,
    hits: AtomicUsize,
    base: String,
    public: String,
    target: Mutex<Option<SessionTarget>>,
}
impl fault_proxy::Fault for FinalBearerChange {
    fn request(&self, _: &reqwest::Method, path: &str, _: &mut Vec<u8>) -> Option<u16> {
        if path == "/rest/v1/rpc/read_current_session"
            && self.composed.load(Ordering::SeqCst)
            && self.accepted_access.load(Ordering::SeqCst)
            && self.armed.swap(false, Ordering::SeqCst)
        {
            let (base, public) = (self.base.clone(), self.public.clone());
            let target = self
                .target
                .lock()
                .unwrap()
                .clone()
                .expect("credential installed before arming");
            std::thread::spawn(move || {
                change_session(target.change, &base, &public, &target.token)
            })
            .join()
            .unwrap();
            self.hits.fetch_add(1, Ordering::SeqCst);
        }
        None
    }
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        if path == "/rest/v1/rpc/read_native_human_access"
            && self.composed.load(Ordering::SeqCst)
            && self.armed.load(Ordering::SeqCst)
        {
            assert_eq!(status, 200);
            let value: Value = serde_json::from_slice(body).unwrap();
            assert_eq!(value["status"], "success");
            self.accepted_access.store(true, Ordering::SeqCst);
        }
    }
}
pub(super) fn run(r: &Reservation, phase: &str) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let mut token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let mut source_cases = 0;
    let mut logout_cases = 0;
    let mut final_bearer_cases = 0;
    let mut expiry_cases = 0;
    let mut expiry_bearer_cases = 0;
    if phase == "history-source" {
        for operation in ["history", "replay"] {
            let armed = Arc::new(AtomicBool::new(false));
            let hits = Arc::new(AtomicUsize::new(0));
            let (enabled, count) = (armed.clone(), hits.clone());
            let game = r.game_id;
            let http = http::HttpProbe::configure(&base, &key, move |runtime| {
                runtime.with_read_delivery_hook(stage(operation), move |repo, recovered, _, _| {
                    assert!(recovered.is_some(), "active original walk was composed");
                    if enabled.swap(false, Ordering::SeqCst) {
                        pause(repo, game);
                        count.fetch_add(1, Ordering::SeqCst);
                    }
                })
            });
            read(&http, &token, r, operation, 200);
            let before = source_fingerprint(&repository, r.game_id);
            armed.store(true, Ordering::SeqCst);
            assert_eq!(
                read(&http, &token, r, operation, 409),
                json!({"status":"fail","data":{"basis":"stale"}})
            );
            assert_eq!(hits.load(Ordering::SeqCst), 1);
            assert_ne!(source_fingerprint(&repository, r.game_id), before);
            restore(&repository, r.game_id);
            let restored = source_fingerprint(&repository, r.game_id);
            read(&http, &token, r, operation, 200);
            assert_eq!(source_fingerprint(&repository, r.game_id), restored);
            source_cases += 1;
        }
    } else {
        let archive = phase.starts_with("archive-session-");
        let operation = if archive {
            archive_operation(phase)
        } else if phase.ends_with("-history") {
            "history"
        } else if phase.ends_with("-replay") {
            "replay"
        } else {
            panic!("unknown history delivery phase")
        };
        let expiry =
            phase.starts_with("history-expiry-") || phase.starts_with("archive-session-expiry-");
        let final_bearer = phase.starts_with("history-bearer-")
            || phase.starts_with("history-expiry-bearer-")
            || phase.starts_with("archive-session-logout-bearer-")
            || phase.starts_with("archive-session-expiry-bearer-");
        assert!(
            final_bearer
                || phase.starts_with("history-logout-")
                || phase.starts_with("archive-session-logout-compose-")
                || expiry
        );
        let state = Arc::new(FinalBearerChange {
            armed: AtomicBool::new(false),
            composed: AtomicBool::new(false),
            accepted_access: AtomicBool::new(false),
            hits: AtomicUsize::new(0),
            base: base.clone(),
            public: public.clone(),
            target: Mutex::new(if expiry {
                None
            } else {
                Some(SessionTarget {
                    token: token.clone(),
                    change: SessionChange::Logout,
                })
            }),
        });
        let proxy = fault_proxy::Proxy::new(&base, None, state.clone());
        let late = state.clone();
        let (origin_url, secret) = (proxy.url().to_owned(), key.clone());
        let http = http::HttpProbe::forwarded_read(proxy.url(), &base, &key, move |runtime| {
            self::runtime(runtime, operation, &origin_url, &secret).with_read_delivery_hook(
                stage(operation),
                move |_, recovered, _, _| {
                    assert_eq!(recovered.is_none(), archive);
                    if !late.armed.load(Ordering::SeqCst) {
                        return;
                    }
                    late.composed.store(true, Ordering::SeqCst);
                    if !final_bearer && late.armed.swap(false, Ordering::SeqCst) {
                        let target = late
                            .target
                            .lock()
                            .unwrap()
                            .clone()
                            .expect("credential installed before arming");
                        change_session(target.change, &late.base, &late.public, &target.token);
                        late.hits.fetch_add(1, Ordering::SeqCst);
                    }
                },
            )
        });
        if expiry || archive {
            // Configuration parses the genuine retained source before a fresh
            // short-lived credential is requested for the warm positive.
            token = fresh_owner(&base, &key, &public);
        }
        let positive = read(&http, &token, r, operation, 200);
        let before = source_fingerprint(&repository, r.game_id);
        if expiry {
            // Start the observed lifetime after all expensive setup and warming.
            let (fresh, at) = fresh_owner_deadline(&base, &key, &public);
            token = fresh;
            *state.target.lock().unwrap() = Some(SessionTarget {
                token: token.clone(),
                change: SessionChange::Expire(at),
            });
            assert!(
                at > now() + 7,
                "genuine credential must survive the positive setup control"
            );
            // Match the existing expiry controls: cross the genuine deadline
            // inside ordinary request timeouts, rather than testing a timeout.
            wait_until(at - 6);
        }
        if archive && !expiry {
            token = fresh_owner(&base, &key, &public);
            *state.target.lock().unwrap() = Some(SessionTarget {
                token: token.clone(),
                change: SessionChange::Logout,
            });
        }
        state.armed.store(true, Ordering::SeqCst);
        assert_eq!(
            read(&http, &token, r, operation, 401),
            json!({"status":"fail","data":{"session":"unauthorized"}})
        );
        assert_eq!(state.hits.load(Ordering::SeqCst), 1);
        assert!(state.composed.load(Ordering::SeqCst));
        if final_bearer {
            assert!(state.accepted_access.load(Ordering::SeqCst));
            if expiry {
                expiry_bearer_cases += 1;
            } else {
                final_bearer_cases += 1;
            }
        } else if expiry {
            expiry_cases += 1;
        } else {
            logout_cases += 1;
        }
        assert_eq!(source_fingerprint(&repository, r.game_id), before);
        // A real new owner session, rather than resurrection of the revoked one.
        let fresh = fresh_owner(&base, &key, &public);
        assert_ne!(fresh, token);
        let restored = read(&http, &fresh, r, operation, 200);
        assert_eq!(restored, positive);
        assert_eq!(source_fingerprint(&repository, r.game_id), before);
    }
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"lateSourceCases":source_cases,"lateLogoutCases":logout_cases,"requestSideFinalBearerCases":final_bearer_cases,"lateExpiryCases":expiry_cases,"requestSideExpiryCases":expiry_bearer_cases,"expiryCredentialAfterPreparation":phase.starts_with("history-expiry-") || phase.starts_with("archive-session-expiry-"),"archiveReaderNoRecovery":phase.starts_with("archive-session-"),"sourceUnchanged":phase.starts_with("archive-session-"),"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
