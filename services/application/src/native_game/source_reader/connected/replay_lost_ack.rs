//! Real accepted SQL publication whose downstream HTTP body is cut short.
use super::delivery::source_fingerprint;
use super::*;
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
const PUBLICATION: &str = "/rest/v1/rpc/publish_game_replay";
struct LostAck {
    accepted: Mutex<Option<Value>>,
    lost: AtomicUsize,
    no_rebuild: AtomicBool,
    rebuild_attempts: AtomicUsize,
}
impl fault_proxy::Fault for LostAck {
    fn request(&self, method: &reqwest::Method, path: &str, _: &mut Vec<u8>) -> Option<u16> {
        if self.no_rebuild.load(Ordering::SeqCst)
            && (path == PUBLICATION
                || path == "/rest/v1/rpc/publish_native_view_source"
                || (path.starts_with("/storage/v1/object/") && *method == reqwest::Method::POST))
        {
            self.rebuild_attempts.fetch_add(1, Ordering::SeqCst);
            return Some(503);
        }
        None
    }
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        if path == PUBLICATION {
            assert_eq!(status, 200);
            let value: Value = serde_json::from_slice(body).unwrap();
            assert_eq!(value["status"], "success");
            assert!(
                value["data"]["resourceId"]
                    .as_str()
                    .unwrap()
                    .parse::<Uuid>()
                    .is_ok()
            );
            let mut ack = self.accepted.lock().unwrap();
            assert!(ack.is_none(), "one real accepted publication only");
            *ack = Some(value);
        }
    }
    fn delivery(
        &self,
        _: &reqwest::Method,
        path: &str,
        _: u16,
        _: &[u8],
    ) -> fault_proxy::ResponseDelivery {
        if path == PUBLICATION {
            assert!(
                self.accepted.lock().unwrap().is_some(),
                "complete successful upstream acknowledgment was decoded before loss"
            );
            assert_eq!(self.lost.fetch_add(1, Ordering::SeqCst), 0);
            fault_proxy::ResponseDelivery::Truncate
        } else {
            fault_proxy::ResponseDelivery::Forward
        }
    }
}
pub(super) fn run(r: &Reservation) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let commands = http::HttpProbe::owned(&base, &key, "fixture:source-reader");
    commands.concede(&token, r.game_id, r.context_id);
    let terminal = repository.read(r.game_id).unwrap().unwrap();
    assert_eq!(
        terminal.phases().0,
        crate::match_history::GamePhase::Completed
    );
    assert_eq!(terminal.accepted_revision(), &Amount::from(1));
    let terminal_view = commands.read(Some(&token), r.game_id, r.context_id, 200);
    let source = source_fingerprint(&repository, r.game_id);
    let history = json!({"gameId":r.game_id,"contextId":r.context_id,"cursor":null,"limit":"3"});
    commands.history(Some(&token), r.game_id, &history, 200);
    let manifest = || {
        let bytes = repository
            .backend
            .select_bytes_bounded(
                "replay_manifest",
                &[
                    ("select", "id,game_id,context_id".to_owned()),
                    ("game_id", format!("eq.{}", r.game_id)),
                    ("limit", "2".to_owned()),
                ],
                4096,
            )
            .unwrap();
        serde_json::from_slice::<Vec<Value>>(&bytes).unwrap()
    };
    assert!(
        manifest().is_empty(),
        "first terminal replay still has no publication"
    );
    let loss = Arc::new(LostAck {
        accepted: Mutex::new(None),
        lost: AtomicUsize::new(0),
        no_rebuild: AtomicBool::new(false),
        rebuild_attempts: AtomicUsize::new(0),
    });
    let proxy = fault_proxy::Proxy::new(&base, None, loss.clone());
    let http = http::HttpProbe::forwarded_read(proxy.url(), &base, &key, |runtime| runtime);
    let request = json!({"gameId":r.game_id,"contextId":r.context_id,"atEventSequence":terminal.last_event_sequence(),"perspectiveSeatId":r.seat_ids[0],"mode":"historical-seat","history":history});
    let failure = http.replay(Some(&token), r.game_id, &request, 503);
    assert_eq!(
        failure,
        json!({"status":"error","message":"Application service is temporarily unavailable","code":"unavailable"})
    );
    assert_eq!(loss.lost.load(Ordering::SeqCst), 1);
    let accepted = loss.accepted.lock().unwrap().clone().unwrap();
    let rows = manifest();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], accepted["data"]["resourceId"]);
    assert_eq!(rows[0]["game_id"], json!(r.game_id));
    assert_eq!(rows[0]["context_id"], json!(r.context_id));
    assert_eq!(source_fingerprint(&repository, r.game_id), source);
    loss.no_rebuild.store(true, Ordering::SeqCst);
    let retry = http::HttpProbe::forwarded_read(proxy.url(), &base, &key, |_| {
        NativeGameRuntime::with_pool(proxy.url(), &key, "resolution-selection").unwrap()
    });
    let restored = retry.replay(Some(&token), r.game_id, &request, 200);
    assert_eq!(
        restored["data"]["snapshot"],
        terminal_view["data"]["snapshot"]
    );
    assert_eq!(
        manifest(),
        rows,
        "retry uses exact original canonical manifest"
    );
    assert_eq!(loss.lost.load(Ordering::SeqCst), 1);
    assert_eq!(
        loss.rebuild_attempts.load(Ordering::SeqCst),
        0,
        "retry neither rebuilds nor republishes"
    );
    assert_eq!(source_fingerprint(&repository, r.game_id), source);
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"replay-lost-ack","actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"actualTerminalCommand":true,"acceptedSqlPublication":true,"lostAcknowledgmentBodies":1,"actualSafeInitialFailure":true,"sameCanonicalManifestRetry":true,"incompatibleCurrentProgramsRetry":true,"rebuildAttempts":0,"sourceUnchangedAcrossPublication":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
