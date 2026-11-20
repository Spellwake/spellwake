//! Rehashed corruptions over a genuine published archive. Only the test proxy
//! substitutes manifest references; accepted SQL originals remain unchanged.
use super::*;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct ManifestReplacement {
    header: NativeArtifactRef,
    index: NativeArtifactRef,
    hits: AtomicUsize,
    missing_opening: bool,
    missing_view: bool,
    source_hits: AtomicUsize,
}
impl super::fault_proxy::Fault for ManifestReplacement {
    fn response(&self, _: &reqwest::Method, path: &str, status: u16, bytes: &mut Vec<u8>) {
        if (self.missing_opening && path == "/rest/v1/rpc/read_native_opening_source")
            || (self.missing_view && path == "/rest/v1/rpc/read_native_view_source")
        {
            assert_eq!(status, 200);
            let value: Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(value["status"], "success");
            *bytes = serde_json::to_vec(&json!({"status":"fail","data":if self.missing_opening { json!({"game":"unavailable"}) } else { json!({"view":"unavailable"}) }}))
                .unwrap();
            self.source_hits.fetch_add(1, Ordering::SeqCst);
            return;
        }
        if path != "/rest/v1/replay_manifest" {
            return;
        }
        assert_eq!(status, 200);
        let mut rows: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 1);
        rows[0]["artifact_ref"] = json!(self.header.as_str());
        rows[0]["visibility_index_ref"] = json!(self.index.as_str());
        rows[0]["qualification_ref"] =
            json!(format!("spellwake-replay-v2:{}", self.header.as_str()));
        *bytes = serde_json::to_vec(&rows).unwrap();
        self.hits.fetch_add(1, Ordering::SeqCst);
    }
}
pub(super) fn reference(value: &Value) -> NativeArtifactRef {
    serde_json::from_value(value.clone()).unwrap()
}
pub(super) fn read(store: &NativeArtifactStore, reservation: &Reservation, value: &Value) -> Value {
    let artifact = store
        .read(
            &reference(value),
            reservation.game_id,
            reservation.context_id,
            NativeArtifactRole::Replay,
        )
        .unwrap();
    serde_json::from_slice(artifact.bytes()).unwrap()
}
fn retain(
    store: &NativeArtifactStore,
    reservation: &Reservation,
    value: &Value,
) -> NativeArtifactRef {
    store
        .retain(
            reservation.game_id,
            reservation.context_id,
            NativeArtifactRole::Replay,
            &serde_json::to_vec(value).unwrap(),
        )
        .unwrap()
}

pub(super) fn run(
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    reservation: &Reservation,
    manifest: Uuid,
) -> Value {
    let rows: Value = serde_json::from_slice(
        &repository
            .backend
            .select_bytes_bounded(
                "replay_manifest",
                &[
                    ("select", "artifact_ref".into()),
                    ("id", format!("eq.{manifest}")),
                    ("limit", "2".into()),
                ],
                16384,
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    let original_header = read(store, reservation, &rows[0]["artifact_ref"]);
    let original_index = read(store, reservation, &original_header["indexRef"]);
    let seat = reservation.seat_ids[0];
    let perspective = original_index["perspectives"]
        .as_array()
        .unwrap()
        .iter()
        .position(|p| p["seatId"] == json!(seat) && p["disclosure"] == "live")
        .unwrap();
    let pages = original_index["perspectives"][perspective]["pages"]
        .as_array()
        .unwrap();
    let first = read(store, reservation, &pages[0]["pageRef"]);
    let last = read(store, reservation, &pages.last().unwrap()["pageRef"]);
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let target = || super::super::replay_archive::ReplayTarget {
        game: reservation.game_id,
        context: reservation.context_id,
        manifest,
    };
    let budget = || NativeRecoveryBudget::new(10000, 100000000).unwrap();
    let original = repository
        .read_portable_replay(
            store,
            target(),
            seat,
            NativeDisclosure::LiveParticipant,
            Some(&0.into()),
            &mut budget(),
        )
        .unwrap()
        .unwrap();
    let source = original.cut_source();
    use super::super::cuts::HistoryCut;
    let silent = source
        .cuts
        .windows(2)
        .position(|pair| {
            crate::player_wire::PlayerVisibleEvent::from_projections(
                &crate::player_wire::PlayerEventFacts {
                    before: pair[0].snapshot(),
                    before_projection: pair[0].cut_facts(source.vocabulary).unwrap(),
                    after: pair[1].snapshot(),
                    after_projection: pair[1].cut_facts(source.vocabulary).unwrap(),
                    terminal: pair[1].cut_terminal(),
                },
            )
            .unwrap()
            .is_none()
        })
        .map(|index| index + 1);
    // Exact unchanged SQL/Storage source is the independent positive control.
    assert!(
        repository
            .read_portable_replay(
                store,
                target(),
                seat,
                NativeDisclosure::LiveParticipant,
                Some(&0.into()),
                &mut budget()
            )
            .unwrap()
            .is_some()
    );
    let mut controls = 0;
    for name in [
        "unchanged-rehashed",
        "page-game",
        "page-context",
        "page-perspective",
        "page-disclosure",
        "page-first",
        "page-last",
        "interior-gap",
        "interior-duplicate",
        "interior-reorder",
        "empty-page",
        "baseline-event",
        "baseline-state",
        "baseline-checkpoint",
        "baseline-creator",
        "silent-creator",
        "winning-view",
        "losing-view",
        "foreign-view",
        "foreign-event",
        "foreign-state",
        "vocabulary-bytes",
        "coherent-vocabulary",
        "program-descriptor",
        "missing-opening",
        "missing-view",
        "view-receipt",
        "snapshot",
        "loaded-page-tail",
        "terminal-outcome",
        "terminal-winner",
    ] {
        // This genuine game may make a public change at every cut. Never invent
        // a silent native event to satisfy fixture shape; direct closure tests
        // cover silent mechanics separately from genuine source qualification.
        if name == "silent-creator" && silent.is_none() {
            eprintln!("REPLAY_SILENT_CUT_CONTROL unavailable_in_selected_real_game");
            continue;
        }
        controls += 1;
        let terminal = name.starts_with("terminal-");
        let page_index = if terminal { pages.len() - 1 } else { 0 };
        let mut page = if terminal {
            last.clone()
        } else {
            first.clone()
        };
        let unchanged_page = page.clone();
        let other_cut = page["cuts"].as_array().unwrap().len() - 1;
        assert!(other_cut > 0, "requires a real nontrivial source page");
        match name {
            "unchanged-rehashed" => {}
            "page-game" => page["gameId"] = json!(Uuid::new_v4()),
            "page-context" => page["contextId"] = json!(Uuid::new_v4()),
            "page-perspective" => page["perspectiveSeatId"] = json!(reservation.seat_ids[1]),
            "page-disclosure" => page["disclosure"] = json!("completed"),
            "page-first" => page["cuts"][0]["sequence"] = json!("1"),
            "page-last" => page["cuts"][other_cut]["sequence"] = json!("999999"),
            "interior-gap" => {
                assert!(other_cut > 2, "requires actual interior cuts");
                page["cuts"][1]["sequence"] = json!("2");
            }
            "interior-duplicate" => {
                assert!(other_cut > 2, "requires actual interior cuts");
                page["cuts"][1]["sequence"] = json!("0");
            }
            "interior-reorder" => {
                assert!(other_cut > 2, "requires actual interior cuts");
                page["cuts"].as_array_mut().unwrap().swap(1, 2);
            }
            "empty-page" => page["cuts"] = json!([]),
            "baseline-event" => page["cuts"][0]["eventRef"] = page["cuts"][1]["eventRef"].clone(),
            "baseline-state" => page["cuts"][0]["stateRef"] = page["cuts"][1]["stateRef"].clone(),
            "baseline-checkpoint" => {
                page["cuts"][0]["checkpointId"] = page["cuts"][1]["checkpointId"].clone()
            }
            "baseline-creator" => {
                page["cuts"][0]["creatorReceiptId"] = page["cuts"][1]["creatorReceiptId"].clone()
            }
            "silent-creator" => {
                let silent = silent.unwrap();
                assert_ne!(
                    page["cuts"][silent]["creatorReceiptId"],
                    page["cuts"][0]["creatorReceiptId"]
                );
                page["cuts"][silent]["creatorReceiptId"] =
                    page["cuts"][0]["creatorReceiptId"].clone();
            }
            "winning-view" => page["cuts"][0]["viewRef"] = page["cuts"][1]["viewRef"].clone(),
            "losing-view" => {
                let original = store
                    .read(
                        &reference(&page["cuts"][0]["viewRef"]),
                        reservation.game_id,
                        reservation.context_id,
                        NativeArtifactRole::View,
                    )
                    .unwrap();
                let mut value: Value = serde_json::from_slice(original.bytes()).unwrap();
                value["snapshot"]["players"][0]["life"] = json!("999999");
                let replacement = store
                    .retain(
                        reservation.game_id,
                        reservation.context_id,
                        NativeArtifactRole::View,
                        &serde_json::to_vec(&value).unwrap(),
                    )
                    .unwrap();
                page["cuts"][0]["viewRef"] = json!(replacement.as_str());
            }
            "foreign-view" | "foreign-event" | "foreign-state" => {
                let (field, role) = match name {
                    "foreign-view" => ("viewRef", NativeArtifactRole::View),
                    "foreign-event" => ("eventRef", NativeArtifactRole::Event),
                    _ => ("stateRef", NativeArtifactRole::State),
                };
                let replacement = NativeArtifactRef::for_bytes(
                    Uuid::new_v4(),
                    reservation.context_id,
                    role,
                    b"foreign namespace control",
                )
                .unwrap();
                page["cuts"][0][field] = json!(replacement.as_str());
            }
            "vocabulary-bytes"
            | "coherent-vocabulary"
            | "program-descriptor"
            | "missing-opening"
            | "missing-view" => {}
            "view-receipt" => {
                page["cuts"][0]["viewReceiptId"] = page["cuts"][1]["viewReceiptId"].clone()
            }
            "snapshot" => {
                page["cuts"][0]["projection"]["snapshot"]["players"][0]["life"] = json!("999999")
            }
            // This cut is later than until=0, but the whole loaded page must be authenticated.
            "loaded-page-tail" => {
                page["cuts"][other_cut]["creatorReceiptId"] =
                    page["cuts"][0]["creatorReceiptId"].clone()
            }
            "terminal-outcome" => {
                page["cuts"][other_cut]["terminal"] = json!({"outcome":"draw","winnerSeatIds":[]})
            }
            "terminal-winner" => {
                let old = page["cuts"][other_cut]["terminal"]["winnerSeatIds"].clone();
                let replacement = if old == json!([reservation.seat_ids[0]]) {
                    reservation.seat_ids[1]
                } else {
                    reservation.seat_ids[0]
                };
                page["cuts"][other_cut]["terminal"]["winnerSeatIds"] = json!([replacement]);
            }
            _ => unreachable!(),
        }
        // Retain exact new bytes and cascade genuine content-addressed references.
        if !matches!(
            name,
            "unchanged-rehashed"
                | "vocabulary-bytes"
                | "coherent-vocabulary"
                | "program-descriptor"
                | "missing-opening"
                | "missing-view"
        ) {
            assert_ne!(
                page, unchanged_page,
                "{name} must change the intended source value"
            );
        }
        let page_ref = retain(store, reservation, &page);
        let mut index = original_index.clone();
        if name == "coherent-vocabulary" {
            let mut value = read(store, reservation, &index["vocabularyRef"]);
            let changed_revision = format!(
                "{}-fault-control",
                value["rules_revision"].as_str().unwrap()
            );
            let hash =
                crate::source_protocol::digest_bytes(value["rules_sha256"].as_str().unwrap())
                    .unwrap();
            let members: Vec<String> = serde_json::from_value(value["members"].clone()).unwrap();
            let vocabulary = spellwake_contracts::creature_types::CreatureTypeVocabulary::new(
                changed_revision.clone(),
                hash,
                members,
            )
            .unwrap();
            value["rules_revision"] = json!(changed_revision);
            value["id"] = json!(vocabulary.id().to_string());
            let bytes = serde_json::to_vec(&value).unwrap();
            assert_eq!(
                crate::native_game::initial_source::decode_vocabulary_input(&bytes)
                    .unwrap()
                    .id(),
                vocabulary.id()
            );
            let replacement = store
                .retain(
                    reservation.game_id,
                    reservation.context_id,
                    NativeArtifactRole::Replay,
                    &bytes,
                )
                .unwrap();
            index["vocabularyRef"] = json!(replacement.as_str());
            index["vocabularyId"] = json!(vocabulary.id().to_string());
        }
        if name == "vocabulary-bytes" {
            let original = store
                .read(
                    &reference(&index["vocabularyRef"]),
                    reservation.game_id,
                    reservation.context_id,
                    NativeArtifactRole::Replay,
                )
                .unwrap();
            let mut bytes = original.bytes().to_vec();
            bytes.push(b'\n');
            assert!(serde_json::from_slice::<Value>(&bytes).is_ok());
            let replacement = store
                .retain(
                    reservation.game_id,
                    reservation.context_id,
                    NativeArtifactRole::Replay,
                    &bytes,
                )
                .unwrap();
            index["vocabularyRef"] = json!(replacement.as_str());
        }
        if name == "program-descriptor" {
            let mut descriptor = read(store, reservation, &index["programIdentityRef"]);
            descriptor["runtime"] = json!("0".repeat(64));
            let replacement = retain(store, reservation, &descriptor);
            index["programIdentityRef"] = json!(replacement.as_str());
        }
        index["perspectives"][perspective]["pages"][page_index]["pageRef"] =
            json!(page_ref.as_str());
        let index_ref = retain(store, reservation, &index);
        let mut header = original_header.clone();
        header["indexRef"] = json!(index_ref.as_str());
        let header_ref = retain(store, reservation, &header);
        let replacement = Arc::new(ManifestReplacement {
            header: header_ref,
            index: index_ref,
            hits: AtomicUsize::new(0),
            missing_opening: name == "missing-opening",
            missing_view: name == "missing-view",
            source_hits: AtomicUsize::new(0),
        });
        let proxy = super::fault_proxy::Proxy::new(&base, Some(&key), replacement.clone());
        let faulty = NativeSourceRepository::new(
            BackendClient::new(
                BackendConfig::new(proxy.url(), "sb_secret_test_only").unwrap(),
                "owned-rehashed-archive-control",
                Duration::from_secs(30),
            )
            .unwrap(),
        );
        let zero = Amount::from(0);
        super::super::replay_archive::take_test_fault();
        let result = faulty.read_portable_replay(
            store,
            target(),
            seat,
            NativeDisclosure::LiveParticipant,
            if terminal { None } else { Some(&zero) },
            &mut budget(),
        );
        assert_eq!(result.is_ok(), name == "unchanged-rehashed", "{name}");
        let expected_stage = match name {
            "unchanged-rehashed" => None,
            "page-game" | "page-context" | "page-perspective" | "page-disclosure"
            | "page-first" | "page-last" | "empty-page" => Some("page"),
            "foreign-view" | "foreign-event" | "foreign-state" | "interior-gap"
            | "interior-duplicate" | "interior-reorder" => Some("cut lineage"),
            "winning-view" | "losing-view" | "view-receipt" | "baseline-checkpoint" => {
                Some("sql-winning View")
            }
            "snapshot" => Some("retained View"),
            "missing-opening" => Some("original opening unavailable"),
            "missing-view" => Some("sql-winning View"),
            "vocabulary-bytes" | "coherent-vocabulary" | "program-descriptor" => {
                Some("original program identity")
            }
            "terminal-outcome" | "terminal-winner" => Some("terminal value"),
            _ => Some("accepted cut source"),
        };
        assert_eq!(
            super::super::replay_archive::take_test_fault(),
            expected_stage,
            "{name} refused at its intended invariant"
        );
        assert_eq!(replacement.hits.load(Ordering::SeqCst), 1, "{name}");
        assert_eq!(
            replacement.source_hits.load(Ordering::SeqCst),
            usize::from(matches!(name, "missing-opening" | "missing-view")),
            "{name}"
        );
        assert!(
            repository
                .read_portable_replay(
                    store,
                    target(),
                    seat,
                    NativeDisclosure::LiveParticipant,
                    Some(&zero),
                    &mut budget()
                )
                .unwrap()
                .is_some(),
            "restored {name}"
        );
        eprintln!("REPLAY_REHASHED_CONTROL {name}");
    }
    json!({"executed":controls,"genuineSilentCutInLoadedPrefix":silent.is_some(),"denseInteriorControls":3})
}
