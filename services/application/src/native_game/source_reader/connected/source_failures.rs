//! Controlled trusted-response faults and actual SQL rejection, not native expiry.
use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Clone, Copy)]
enum SourceFault {
    Runtime,
    Programs,
    Policy,
    Exhausted,
    SqlClock,
}
struct FaultState {
    selected: Mutex<SourceFault>,
    triggered: AtomicUsize,
    refused: AtomicUsize,
    submissions: AtomicUsize,
}
impl super::fault_proxy::Fault for FaultState {
    fn request(&self, _method: &reqwest::Method, path: &str, body: &mut Vec<u8>) -> Option<u16> {
        if path == "/rest/v1/rpc/submit_native_human_command" {
            self.submissions.fetch_add(1, Ordering::SeqCst);
        }
        if matches!(*self.selected.lock().unwrap(), SourceFault::SqlClock)
            && path == "/rest/v1/rpc/submit_native_human_command"
        {
            let mut value: serde_json::Value = serde_json::from_slice(body).unwrap();
            let clock = &mut value["p_prepared"]["candidate"]["clocks"][0]["clockRevision"];
            let revision = clock.as_str().unwrap().parse::<u128>().unwrap();
            *clock = json!(revision.checked_add(1).unwrap().to_string());
            *body = serde_json::to_vec(&value).unwrap();
            self.triggered.fetch_add(1, Ordering::SeqCst);
        }
        None
    }
    fn response(&self, _method: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        let selected = *self.selected.lock().unwrap();
        if matches!(selected, SourceFault::SqlClock)
            && path == "/rest/v1/rpc/submit_native_human_command"
        {
            assert!(!(200..300).contains(&status));
            let value: serde_json::Value = serde_json::from_slice(body).unwrap();
            assert_eq!(value["code"], json!("40001"));
            self.refused.fetch_add(1, Ordering::SeqCst);
            return;
        }
        let field = match selected {
            SourceFault::Runtime if path == "/rest/v1/rpc/read_native_opening_source" => {
                Some("runtimeId")
            }
            SourceFault::Programs if path == "/rest/v1/rpc/read_native_opening_source" => {
                Some("programSourceId")
            }
            _ => None,
        };
        if let Some(field) = field {
            assert!((200..300).contains(&status));
            let mut value: serde_json::Value = serde_json::from_slice(body).unwrap();
            assert_eq!(value["status"], "success");
            let replacement = "0".repeat(64);
            assert_ne!(value["data"][field], json!(replacement));
            value["data"][field] = json!(replacement);
            *body = serde_json::to_vec(&value).unwrap();
            self.triggered.fetch_add(1, Ordering::SeqCst);
        } else if matches!(selected, SourceFault::Policy | SourceFault::Exhausted)
            && path == "/rest/v1/rpc/read_native_game_source"
        {
            let mut value: serde_json::Value = serde_json::from_slice(body).unwrap();
            assert_eq!(value["status"], "success");
            let clock = &mut value["data"]["matchClocks"][0];
            match selected {
                SourceFault::Policy => {
                    clock["policyRef"] = json!("fixture:unsupported-clock-policy")
                }
                SourceFault::Exhausted => {
                    assert_eq!(clock["policyRef"], "bo3-overall-bank-v1");
                    assert_ne!(clock["remainingMs"], "0");
                    clock["remainingMs"] = json!("0");
                }
                _ => unreachable!("selected projection fault"),
            }
            *body = serde_json::to_vec(&value).unwrap();
            self.triggered.fetch_add(1, Ordering::SeqCst);
        }
    }
}
fn clocks(source: &NativeGameSource) -> Vec<(Uuid, Amount, Amount, bool, String)> {
    source
        .clocks()
        .map(|(seat, remaining, revision, paused, at)| {
            (
                seat,
                remaining.clone(),
                revision.clone(),
                paused,
                at.to_owned(),
            )
        })
        .collect()
}
pub(super) fn run(
    repository: &NativeSourceRepository,
    base: &str,
    token: &str,
    actor: &crate::actor::RequestActor,
    fresh: &PlayerCommand,
    structure: crate::rooms::configuration::MatchStructure,
) {
    let state = Arc::new(FaultState {
        selected: Mutex::new(SourceFault::Runtime),
        triggered: AtomicUsize::new(0),
        refused: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
    });
    let proxy = super::fault_proxy::Proxy::new(base, None, state.clone());
    let secret = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let http = http::HttpProbe::forwarded(base, proxy.url(), &secret);
    let before = repository.read(fresh.basis.game_id).unwrap().unwrap();
    let mut cases = vec![
        SourceFault::Runtime,
        SourceFault::Programs,
        SourceFault::Policy,
        SourceFault::SqlClock,
    ];
    if matches!(structure, crate::rooms::configuration::MatchStructure::Bo3) {
        cases.push(SourceFault::Exhausted);
    }
    for fault in cases {
        *state.selected.lock().unwrap() = fault;
        state.triggered.store(0, Ordering::SeqCst);
        state.refused.store(0, Ordering::SeqCst);
        state.submissions.store(0, Ordering::SeqCst);
        let original = PlayerCommand {
            request_id: Uuid::new_v4(),
            action: crate::player_wire::PlayerAction::Concede {},
            ..fresh.clone()
        };
        http.command(Some(token), &original, 503);
        assert!(state.triggered.load(Ordering::SeqCst) > 0);
        if matches!(fault, SourceFault::SqlClock) {
            assert_eq!(state.refused.load(Ordering::SeqCst), 1);
            assert_eq!(state.submissions.load(Ordering::SeqCst), 1);
        } else {
            assert_eq!(state.submissions.load(Ordering::SeqCst), 0);
        }
        assert!(
            repository
                .human_original(actor, &original)
                .unwrap()
                .is_none()
        );
        let after = repository.read(fresh.basis.game_id).unwrap().unwrap();
        assert_eq!(after.accepted_revision(), before.accepted_revision());
        assert_eq!(after.checkpoint_ids(), before.checkpoint_ids());
        assert_eq!(after.last_event_sequence(), before.last_event_sequence());
        assert_eq!(clocks(&after), clocks(&before));
    }
}
