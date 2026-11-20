//! Causal dependency refusals before JSON headers and after real SSE handshake.
use super::delivery::{json_response, request, source_fingerprint};
use super::delivery_presentation::{frame, open};
use super::*;
use crate::api::NativeStreamProbe;
use axum::http::StatusCode;
use futures::StreamExt;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Clone, Copy, Debug)]
enum Mode {
    Missing,
    Corrupt,
    Clock,
    Cursor,
    Presentation,
}
#[derive(Default)]
struct Fault {
    mode: Mutex<Option<Mode>>,
    hits: AtomicUsize,
}
impl fault_proxy::Fault for Fault {
    fn request(&self, method: &reqwest::Method, path: &str, _: &mut Vec<u8>) -> Option<u16> {
        if matches!(*self.mode.lock().unwrap(), Some(Mode::Missing))
            && *method == reqwest::Method::GET
            && path.starts_with("/storage/v1/object/")
        {
            self.hits.fetch_add(1, Ordering::SeqCst);
            return Some(404);
        }
        None
    }
    fn response(&self, method: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        let Some(mode) = *self.mode.lock().unwrap() else {
            return;
        };
        if matches!(mode, Mode::Corrupt)
            && *method == reqwest::Method::GET
            && path.starts_with("/storage/v1/object/")
        {
            assert_eq!(status, 200);
            assert!(!body.is_empty());
            body[0] ^= 1;
            self.hits.fetch_add(1, Ordering::SeqCst);
            return;
        }
        let target = match mode {
            Mode::Clock => "read_native_game_source",
            Mode::Cursor => "issue_native_cursor_source",
            Mode::Presentation => "read_native_human_presentation",
            _ => return,
        };
        if path.rsplit('/').next() != Some(target) {
            return;
        }
        assert_eq!(status, 200);
        let mut value: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(value["status"], "success");
        match mode {
            Mode::Clock => {
                assert!(
                    value["data"]
                        .as_object_mut()
                        .unwrap()
                        .remove("matchClocks")
                        .is_some()
                );
            }
            Mode::Cursor => {
                assert_eq!(value["data"]["version"], "native-cursor-v1");
                value["data"]["unexpectedAuthority"] = json!(true);
            }
            Mode::Presentation => {
                assert_eq!(value["data"]["version"], "native-presentation-v1");
                value["data"]["unexpectedAuthority"] = json!(true);
            }
            _ => unreachable!(),
        }
        *body = serde_json::to_vec(&value).unwrap();
        self.hits.fetch_add(1, Ordering::SeqCst);
    }
}
fn unavailable() -> serde_json::Value {
    json!({"status":"error","message":"Application service is temporarily unavailable","code":"unavailable"})
}
pub(super) fn run(r: &Reservation, store: &NativeArtifactStore, programs: &NativeProgramSource) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let recovered = repository
        .recover_with_history(
            r.game_id,
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
        r,
    );
    let before = source_fingerprint(&repository, r.game_id);
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let fault = Arc::new(Fault::default());
    let proxy = fault_proxy::Proxy::new(&base, None, fault.clone());
    let probe = NativeStreamProbe::new(http::configuration(
        proxy.url(),
        &key,
        &base,
        |runtime| runtime,
        None,
    ));
    let limited = NativeStreamProbe::new(http::configuration(
        &base,
        &key,
        &base,
        |runtime| runtime.with_test_recovery(NativeRecoveryBudget::new(1, 1).unwrap()),
        None,
    ));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut cases = 0;
    runtime.block_on(async {
        for mode in [Mode::Missing,Mode::Corrupt,Mode::Clock,Mode::Cursor,Mode::Presentation] {
            for stage in ["reconnect","stream-preflight","stream-first","stream-later"] {
                if matches!(mode,Mode::Presentation) && stage=="reconnect" {continue;}
                // First-body renewal reuses the sealed snapshot/cursor and does
                // not reread artifacts or reissue cursors. Only presentation
                // faults belong at that boundary. Later samples recover anew.
                if !matches!(mode,Mode::Presentation) && stage=="stream-first" {continue;}
                eprintln!("controlled dependency case {mode:?}/{stage}");
                let mut positive=open(&probe,&token,r).await;assert_eq!(frame(&mut positive).await["status"],"success");drop(positive);
                fault.hits.store(0,Ordering::SeqCst);
                if matches!(stage,"reconnect"|"stream-preflight") {
                    *fault.mode.lock().unwrap()=Some(mode);
                    let (operation,payload)=if stage=="reconnect" {("reconnect",json!({"receipts":{"gameId":r.game_id,"contextId":r.context_id,"outstanding":[]},"cursor":null}))} else {("stream",json!({"gameId":r.game_id,"contextId":r.context_id,"cursor":null}))};
                    let response=probe.request(request(&token,r.game_id,operation,&payload)).await;
                    assert_eq!(response.status(),StatusCode::SERVICE_UNAVAILABLE);assert_eq!(json_response(response).await,unavailable());
                } else {
                    let mut body=open(&probe,&token,r).await;
                    if stage=="stream-later" {assert_eq!(frame(&mut body).await["status"],"success");}
                    *fault.mode.lock().unwrap()=Some(mode);
                    assert_eq!(frame(&mut body).await,unavailable());assert!(body.next().await.is_none());drop(body);
                }
                assert!(fault.hits.load(Ordering::SeqCst)>0,"selected dependency fault must actually execute");
                *fault.mode.lock().unwrap()=None;
                assert_eq!(probe.available(),(16,16));
                let mut restored=open(&probe,&token,r).await;assert_eq!(frame(&mut restored).await["status"],"success");drop(restored);
                cases+=1;
            }
        }
        for operation in ["reconnect","stream"] {
            let payload=if operation=="reconnect" {json!({"receipts":{"gameId":r.game_id,"contextId":r.context_id,"outstanding":[]},"cursor":null})} else {json!({"gameId":r.game_id,"contextId":r.context_id,"cursor":null})};
            let positive=probe.request(request(&token,r.game_id,operation,&payload)).await;assert_eq!(positive.status(),StatusCode::OK);drop(positive);
            let response=limited.request(request(&token,r.game_id,operation,&payload)).await;assert_eq!(response.status(),StatusCode::SERVICE_UNAVAILABLE);assert_eq!(json_response(response).await,unavailable());assert_eq!(limited.available(),(16,16));cases+=1;
        }
    });
    assert_eq!(cases, 17);
    assert_eq!(source_fingerprint(&repository, r.game_id), before);
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"delivery-failures","actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"actualFailureCases":cases,"actualInitialJson503":true,"actualTerminalServiceClosure":true,"actualBoundedRecovery":true,"sourceUnchanged":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
