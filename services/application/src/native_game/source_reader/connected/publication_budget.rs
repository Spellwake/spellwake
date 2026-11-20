//! Actual publisher accounts and retained-payload inventory over a genuine finite game.
use super::super::replay_archive::{ReplayTarget, take_test_publication_observation};
use super::delivery::source_fingerprint;
use super::*;
use crate::actor::{ActorClient, ActorConfig};
use serde_json::Value;
use std::collections::BTreeSet;

fn count(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap()
}
fn validate(metrics: &Value, inventory: &Value) {
    assert_eq!(metrics["completed"], true);
    assert_eq!(count(metrics, "humanSeats"), 2);
    assert_eq!(count(metrics, "rosterSeats"), 2);
    assert_eq!(count(metrics, "plannedWalks"), 4);
    assert_eq!(
        metrics["firstTemplate"],
        json!({"records":10000,"bytes":10000000})
    );
    assert_eq!(
        metrics["sharedCapacity"],
        json!({"records":30000,"bytes":30000000})
    );
    assert_eq!(
        metrics["retainedPayloadOperations"],
        inventory["retainedPayloadOperations"]
    );
    assert_eq!(
        metrics["retainedPayloadBytes"],
        inventory["retainedPayloadBytes"]
    );
    assert_eq!(metrics["pageCounts"], inventory["pageCounts"]);
    assert_eq!(
        metrics["cutsPerPerspective"],
        inventory["cutsPerPerspective"]
    );
    let walks = metrics["walks"].as_array().unwrap();
    assert_eq!(walks.len(), 4);
    let mut combinations = BTreeSet::new();
    let mut shared_records = count(metrics, "retainedPayloadOperations");
    let mut shared_bytes = count(metrics, "retainedPayloadBytes");
    let mut first = 0;
    for walk in walks {
        let seat = count(walk, "seatIndex");
        assert!(seat < 2);
        let full = walk["fullInformation"].as_bool().unwrap();
        assert!(combinations.insert((seat, full)));
        if walk["first"] == true {
            first += 1;
            assert!(full);
            assert_eq!(walk["spend"], metrics["firstSpend"]);
        } else {
            shared_records = shared_records
                .checked_add(count(&walk["spend"], "records"))
                .unwrap();
            shared_bytes = shared_bytes
                .checked_add(count(&walk["spend"], "bytes"))
                .unwrap();
        }
    }
    assert_eq!(first, 1);
    assert_eq!(
        metrics["sharedSpend"],
        json!({"records":shared_records,"bytes":shared_bytes})
    );
    for axis in ["records", "bytes"] {
        assert!(count(&metrics["firstSpend"], axis) <= count(&metrics["firstTemplate"], axis));
        assert_eq!(
            count(&metrics["firstSpend"], axis)
                .checked_add(count(&metrics["firstRemaining"], axis))
                .unwrap(),
            count(&metrics["firstTemplate"], axis)
        );
        assert_eq!(
            count(&metrics["sharedSpend"], axis)
                .checked_add(count(&metrics["sharedRemaining"], axis))
                .unwrap(),
            count(&metrics["sharedCapacity"], axis)
        );
    }
    assert_eq!(count(metrics, "publicationAckResponseCap"), 65536);
}
fn inventory(
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    r: &Reservation,
    manifest: Uuid,
) -> Value {
    let rows: Value = serde_json::from_slice(
        &repository
            .backend
            .select_bytes_bounded(
                "replay_manifest",
                &[
                    (
                        "select",
                        "id,game_id,context_id,artifact_ref,visibility_index_ref".into(),
                    ),
                    ("id", format!("eq.{manifest}")),
                    ("limit", "2".into()),
                ],
                16384,
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    let row = &rows[0];
    assert_eq!(row["game_id"], json!(r.game_id));
    assert_eq!(row["context_id"], json!(r.context_id));
    let header = archive_integrity::read(store, r, &row["artifact_ref"]);
    assert_eq!(header["indexRef"], row["visibility_index_ref"]);
    let index = archive_integrity::read(store, r, &header["indexRef"]);
    assert_eq!(index["conclusion"], "native-conclusion");
    let perspectives = index["perspectives"].as_array().unwrap();
    assert_eq!(perspectives.len(), 4);
    let mut refs = vec![
        archive_integrity::reference(&row["artifact_ref"]),
        archive_integrity::reference(&header["indexRef"]),
        archive_integrity::reference(&index["programIdentityRef"]),
        archive_integrity::reference(&index["vocabularyRef"]),
    ];
    let mut page_counts = Vec::new();
    let mut cuts = None;
    let mut combinations = BTreeSet::new();
    for perspective in perspectives {
        let seat: Uuid = perspective["seatId"].as_str().unwrap().parse().unwrap();
        assert!(r.seat_ids.contains(&seat));
        let mode = perspective["disclosure"].as_str().unwrap();
        assert!(matches!(mode, "live" | "completed"));
        assert!(combinations.insert((seat, mode)));
        let pages = perspective["pages"].as_array().unwrap();
        page_counts.push(pages.len());
        let mut records = 0;
        for page in pages {
            let reference = archive_integrity::reference(&page["pageRef"]);
            let data = archive_integrity::read(store, r, &page["pageRef"]);
            assert_eq!(data["perspectiveSeatId"], json!(seat));
            assert_eq!(data["disclosure"], mode);
            let length = data["cuts"].as_array().unwrap().len();
            assert!((1..=64).contains(&length));
            records += length;
            refs.push(reference);
        }
        assert_eq!(pages.len(), records.div_ceil(64));
        if let Some(expected) = cuts {
            assert_eq!(records, expected);
        } else {
            cuts = Some(records);
        }
    }
    let bytes = refs
        .iter()
        .try_fold(0_u64, |sum, reference| sum.checked_add(reference.size()))
        .unwrap();
    json!({"retainedPayloadOperations":refs.len(),"retainedPayloadBytes":bytes,"pageCounts":page_counts,"cutsPerPerspective":cuts.unwrap()})
}
pub(super) fn run(r: &Reservation, store: &NativeArtifactStore, programs: &NativeProgramSource) {
    lifecycle_reads::prepare_archive(r, false);
    let repository = NativeSourceRepository::from_env().unwrap();
    let before = source_fingerprint(&repository, r.game_id);
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let bearer = history_delivery::fresh_owner(&base, &key, &public);
    let actor =
        ActorClient::new(ActorConfig::new(&base, &public, &format!("{base}/auth/v1")).unwrap())
            .unwrap()
            .resolve(&bearer)
            .unwrap();
    let NativeHumanAccessOutcome::Granted(access) = repository
        .human_access(&actor, r.game_id, r.context_id)
        .unwrap()
    else {
        panic!("actual current human access required")
    };
    let manifest = access.replay().0.unwrap();
    let template = NativeRecoveryBudget::new(10000, 10000000).unwrap();
    repository
        .read_portable_replay(
            store,
            ReplayTarget {
                game: r.game_id,
                context: r.context_id,
                manifest,
            },
            r.seat_ids[0],
            NativeDisclosure::CompletedParticipant,
            None,
            &mut template.clone(),
        )
        .unwrap()
        .unwrap();
    let inventory = inventory(&repository, store, r, manifest);
    take_test_publication_observation(false);
    take_test_publication_observation(true);
    assert_eq!(
        repository
            .publish_portable_replay(store, programs, &access, &template)
            .unwrap(),
        Some(manifest)
    );
    let measured = take_test_publication_observation(true).unwrap();
    validate(&measured, &inventory);
    take_test_publication_observation(false);
    let first_records = count(&measured["firstSpend"], "records");
    assert!(first_records > 0);
    assert!(
        count(&measured["sharedSpend"], "records") > first_records.checked_mul(3).unwrap(),
        "selected real fixture must isolate shared starvation after first walk"
    );
    let low = NativeRecoveryBudget::new(usize::try_from(first_records).unwrap(), 10000000).unwrap();
    let error = repository
        .publish_portable_replay(store, programs, &access, &low)
        .err()
        .unwrap();
    assert!(matches!(
        error.0,
        Failure::Checkpoint(NativeCheckpointError::Budget)
    ));
    let entered = take_test_publication_observation(false)
        .expect("actual first walk must have completed before shared refusal");
    assert_eq!(entered["firstSpend"]["records"], json!(first_records));
    assert_eq!(entered["firstTemplate"]["records"], json!(first_records));
    assert_eq!(
        entered["sharedCapacity"]["records"],
        json!(first_records * 3)
    );
    assert!(take_test_publication_observation(true).is_none());
    assert_eq!(source_fingerprint(&repository, r.game_id), before);
    assert_eq!(
        repository
            .publish_portable_replay(store, programs, &access, &template)
            .unwrap(),
        Some(manifest)
    );
    let restored = take_test_publication_observation(true).unwrap();
    validate(&restored, &inventory);
    assert_eq!(source_fingerprint(&repository, r.game_id), before);
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"publication-budget","actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"actualAggregateAccounts":true,"actualIndependentInventory":inventory,"measured":measured,"restored":restored,"sharedStarvationIsolated":true,"sourceUnchanged":true,"sameCanonicalManifest":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false,"limits":"Finite native Concede fixture; measured recovery work/retained payload only, not total network IO or demonstrated long-game headroom"})
    );
}
