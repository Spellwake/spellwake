//! Exact owned artifacts are restored after real Storage missing/hash failures.
use super::*;

pub(super) fn artifacts(
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    http: &http::HttpProbe,
    actor: &crate::actor::RequestActor,
    token: &str,
    (fresh, accepted): (&PlayerCommand, &PlayerCommand),
    references: &[NativeArtifactRef],
) {
    let bucket = std::env::var("SPELLWAKE_NATIVE_ARTIFACT_BUCKET").unwrap();
    let before = repository.read(fresh.basis.game_id).unwrap().unwrap();
    for reference in references {
        assert_eq!(reference.game_id(), fresh.basis.game_id);
        assert_eq!(reference.context_id(), fresh.basis.context_id);
        let original = store
            .read(
                reference,
                reference.game_id(),
                reference.context_id(),
                reference.role(),
            )
            .unwrap()
            .into_bytes();
        for corrupt in [false, true] {
            crate::private_storage::delete_private_object(
                &repository.backend,
                &bucket,
                reference.as_str(),
            )
            .unwrap();
            if corrupt {
                let mut bytes = original.clone();
                bytes[0] ^= 1;
                let url = crate::private_storage::object_url(
                    &repository.backend,
                    &bucket,
                    reference.as_str(),
                    false,
                )
                .unwrap();
                let uploaded = repository
                    .backend
                    .request(reqwest::Method::POST, url)
                    .unwrap()
                    .header("Content-Type", "application/json")
                    .header("x-upsert", "false")
                    .body(bytes)
                    .send()
                    .unwrap();
                assert!(uploaded.status().is_success());
            }
            // One still-unknown intent must never become a permanent rule error.
            http.command(Some(token), fresh, 503);
            assert!(repository.human_original(actor, fresh).unwrap().is_none());
            // A committed original does not need even this missing/corrupt artifact.
            http.command(Some(token), accepted, 202);
            if corrupt {
                crate::private_storage::delete_private_object(
                    &repository.backend,
                    &bucket,
                    reference.as_str(),
                )
                .unwrap();
            }
            let restored = store
                .retain(
                    reference.game_id(),
                    reference.context_id(),
                    reference.role(),
                    &original,
                )
                .unwrap();
            assert!(restored == *reference);
            assert_eq!(
                store
                    .read(
                        reference,
                        reference.game_id(),
                        reference.context_id(),
                        reference.role()
                    )
                    .unwrap()
                    .bytes(),
                original
            );
        }
    }
    let after = repository.read(fresh.basis.game_id).unwrap().unwrap();
    assert_eq!(before.checkpoint_ids(), after.checkpoint_ids());
    assert_eq!(before.last_event_sequence(), after.last_event_sequence());
    assert_eq!(before.accepted_revision(), after.accepted_revision());
}
