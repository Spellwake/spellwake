//! Actual private Storage with explicitly armed upload/readback failures.
use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Clone, Copy)]
enum Fault {
    Upload,
    Readback,
}
struct StorageFault {
    bucket: String,
    selected: Mutex<(NativeArtifactRole, Fault)>,
    written: Mutex<Vec<String>>,
    failures: AtomicUsize,
}
impl StorageFault {
    fn targeted(&self, path: &str) -> Option<(String, Fault)> {
        let normalized = path.replace("/storage/v1/object/authenticated/", "/storage/v1/object/");
        let prefix = format!("/storage/v1/object/{}/", self.bucket);
        let key = normalized.strip_prefix(&prefix)?;
        let (role, fault) = *self.selected.lock().unwrap();
        (key.split('/').nth(3) == Some(role.as_str())).then(|| (key.to_owned(), fault))
    }
}
impl super::fault_proxy::Fault for StorageFault {
    fn request(&self, method: &reqwest::Method, path: &str, _body: &mut Vec<u8>) -> Option<u16> {
        if let Some((_, Fault::Upload)) = self.targeted(path)
            && method == reqwest::Method::POST
        {
            self.failures.fetch_add(1, Ordering::SeqCst);
            return Some(503);
        }
        None
    }
    fn response(&self, method: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>) {
        let Some((key, fault)) = self.targeted(path) else {
            return;
        };
        if (200..300).contains(&status) && method == reqwest::Method::POST {
            self.written.lock().unwrap().push(key.clone());
        }
        if (200..300).contains(&status)
            && method == reqwest::Method::GET
            && matches!(fault, Fault::Readback)
            && self.written.lock().unwrap().contains(&key)
        {
            assert!(!body.is_empty());
            body[0] ^= 1;
            self.failures.fetch_add(1, Ordering::SeqCst);
        }
    }
}
pub(super) fn run(
    repository: &NativeSourceRepository,
    base: &str,
    token: &str,
    actor: &crate::actor::RequestActor,
    fresh: &PlayerCommand,
) {
    let secret = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let before = repository.read(fresh.basis.game_id).unwrap().unwrap();
    let state = Arc::new(StorageFault {
        bucket: std::env::var("SPELLWAKE_NATIVE_ARTIFACT_BUCKET").unwrap(),
        selected: Mutex::new((NativeArtifactRole::View, Fault::Upload)),
        written: Mutex::new(Vec::new()),
        failures: AtomicUsize::new(0),
    });
    let proxy = super::fault_proxy::Proxy::new(base, Some(&secret), state.clone());
    let http =
        http::HttpProbe::configure_owned(base, &secret, "fixture:source-reader", |runtime| {
            let backend = BackendClient::new(
                BackendConfig::new(proxy.url(), "sb_secret_test_only").unwrap(),
                "owned-storage-write-fault",
                Duration::from_secs(20),
            )
            .unwrap();
            runtime
                .with_test_store(NativeArtifactStore::new(backend, state.bucket.clone()).unwrap())
        });
    for role in [
        NativeArtifactRole::View,
        NativeArtifactRole::Input,
        NativeArtifactRole::State,
        NativeArtifactRole::Event,
        NativeArtifactRole::Facts,
    ] {
        for fault in [Fault::Upload, Fault::Readback] {
            *state.selected.lock().unwrap() = (role, fault);
            state.written.lock().unwrap().clear();
            state.failures.store(0, Ordering::SeqCst);
            let original = PlayerCommand {
                request_id: Uuid::new_v4(),
                action: crate::player_wire::PlayerAction::Concede {},
                ..fresh.clone()
            };
            http.command(Some(token), &original, 503);
            assert!(
                state.failures.load(Ordering::SeqCst) > 0,
                "targeted write/readback must execute"
            );
            assert!(
                repository
                    .human_original(actor, &original)
                    .unwrap()
                    .is_none()
            );
            let written = state.written.lock().unwrap().clone();
            for key in written {
                assert!(key.starts_with(&format!(
                    "native-artifact-v1/{}/{}/",
                    fresh.basis.game_id, fresh.basis.context_id
                )));
                crate::private_storage::delete_private_object(
                    &repository.backend,
                    &state.bucket,
                    &key,
                )
                .unwrap();
            }
        }
    }
    drop(http);
    let after = repository.read(fresh.basis.game_id).unwrap().unwrap();
    assert_eq!(after.checkpoint_ids(), before.checkpoint_ids());
    assert_eq!(after.accepted_revision(), before.accepted_revision());
    assert_eq!(after.last_event_sequence(), before.last_event_sequence());
}
