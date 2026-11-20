//! Explicitly synthetic validated projections for closure/composer mechanics only.
//! No native execution, accepted SQL provenance or participant authority is qualified.
use super::*;
use serde_json::Value;

pub(in crate::native_game::source_reader) fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
pub(in crate::native_game::source_reader) fn inputs(
    count: u64,
    life_digits: Option<usize>,
) -> (Vec<ArchivedCut>, CreatureTypeVocabulary) {
    let corpus: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packages/application-contracts/fixtures/player-wire.json"
    )))
    .unwrap();
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "view-turn-before-first-step")
        .unwrap();
    assert_eq!(case["valid"], true);
    let snapshot: Value = serde_json::from_str(case["wire"].as_str().unwrap()).unwrap();
    let context = &case["context"];
    let vocabulary = CreatureTypeVocabulary::new(
        "synthetic-loaded-cut-controls".into(),
        [0; 32],
        vec!["Human".into()],
    )
    .unwrap();
    let cuts = (0_u64..count).map(|sequence| {
        let mut snapshot = snapshot.clone();
        if let Some(digits) = life_digits {
            assert!(digits >= 2);
            snapshot["players"][0]["life"] = json!(format!("1{}{}", "0".repeat(digits - 2), sequence % 10));
        }
        // Raw sequence and accepted revision remain separate wire domains.
        snapshot["throughEventSequence"] = json!(sequence.to_string());
        snapshot["basis"]["revision"] = json!((sequence * 3).to_string());
        let encoded: ArchivedProjection = crate::decode_wire_object(&serde_json::to_vec(&json!({
            "snapshot":snapshot,"roster":context["roster"],"visibleObjects":context["visibleObjects"],
            "knownIdentities":context["knownIdentities"],"orderedZones":context["orderedZones"],
            "abilityBindings":context["abilityBindings"],"fullDisclosure":false,"vocabularyId":vocabulary.id()
        })).unwrap()).unwrap();
        let projection = encoded.restore();
        let snapshot = projection.snapshot();
        snapshot.validate(&projection.facts(&vocabulary).unwrap()).unwrap();
        let artifact = |role, label: &str| NativeArtifactRef::for_bytes(snapshot.basis.game_id, snapshot.basis.context_id, role, format!("{label}-{sequence}").as_bytes()).unwrap();
        ArchivedCut {
            view_receipt_id:id(1000 + u128::from(sequence)), event_id:id(2000 + u128::from(sequence)),
            event_ref:artifact(NativeArtifactRole::Event,"event"), state_ref:artifact(NativeArtifactRole::State,"state"),
            checkpoint_id:id(3000 + u128::from(sequence)),creator_receipt_id:id(4000 + u128::from(sequence)),terminal:None,projection,
        }
    }).collect();
    (cuts, vocabulary)
}
