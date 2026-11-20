use super::*;
use crate::test_http::{Mock, Step};
use std::{fs, io::Write};
#[path = "../../tests/support/catalogue.rs"]
mod source_fixture;
use source_fixture::{card, fixture};

#[test]
fn source_objects_qualification_and_nested_metadata_reject_arrays_before_staging() {
    for nested in [false, true] {
        let input = fixture(&[card(Uuid::new_v4(), Uuid::new_v4())]);
        let mut value: Value =
            serde_json::from_slice(&fs::read(&input.qualification).unwrap()).unwrap();
        if nested {
            value["metadata"] = crate::test_http::positional(
                &value["metadata"],
                &[
                    "object",
                    "id",
                    "type",
                    "updated_at",
                    "jsonl_download_uri",
                    "compressed_size",
                ],
            );
        } else {
            value = crate::test_http::positional(
                &value,
                &[
                    "metadata",
                    "acquiredAt",
                    "artifactPath",
                    "metadataPath",
                    "hash",
                    "metadataHash",
                    "qualificationPath",
                    "compressedBytes",
                    "decompressedBytes",
                    "recordCount",
                ],
            );
        }
        fs::write(&input.qualification, value.to_string()).unwrap();
        let before = fs::read_dir(&input.root).unwrap().count();
        assert!(matches!(
            stage_catalogue(&input.qualification, &input.root),
            Err(error) if error == crate::price_source::SourceError::InvalidIndex
        ));
        assert_eq!(fs::read_dir(&input.root).unwrap().count(), before);
    }
    let input = fixture(&[card(Uuid::new_v4(), Uuid::new_v4())]);
    let (_, stage) = stage_catalogue(&input.qualification, &input.root).unwrap();
    let mut value = serde_json::to_value(stage).unwrap();
    assert!(serde_json::from_value::<CatalogueStage>(value.clone()).is_ok());
    value["source"] = crate::test_http::positional(
        &value["source"],
        &[
            "metadata",
            "acquiredAt",
            "artifactPath",
            "metadataPath",
            "hash",
            "metadataHash",
            "qualificationPath",
            "compressedBytes",
            "decompressedBytes",
            "recordCount",
        ],
    );
    assert!(serde_json::from_value::<CatalogueStage>(value).is_err());
}

#[test]
fn source_objects_stage_records_reject_nested_arrays_and_duplicates_before_emission() {
    for kind in ["metadata_array", "set_array", "duplicate_name"] {
        let input = fixture(&[card(Uuid::new_v4(), Uuid::new_v4())]);
        let mut prepared = prepare(&input.qualification, &input.root).unwrap();
        let mut value: Value =
            serde_json::from_slice(&fs::read(&prepared.stage.records_path).unwrap()).unwrap();
        match kind {
            "metadata_array" => {
                value["metadata"] = crate::test_http::positional(
                    &value["metadata"],
                    &[
                        "externalPrintingRef",
                        "oracleRef",
                        "name",
                        "set",
                        "collectorNumber",
                        "language",
                        "rarity",
                        "finishes",
                        "layout",
                        "faceNames",
                    ],
                )
            }
            "set_array" => {
                value["metadata"]["set"] = crate::test_http::positional(
                    &value["metadata"]["set"],
                    &["externalRef", "name", "code"],
                )
            }
            _ => {}
        }
        let mut raw = value.to_string();
        if kind == "duplicate_name" {
            raw = raw.replace(
                "\"name\":",
                &format!("\"name\":{},\"name\":", value["metadata"]["name"]),
            );
        }
        raw.push('\n');
        let bytes = raw.as_bytes();
        fs::write(&prepared.stage.records_path, bytes).unwrap();
        // Keep the byte/hash envelope consistent so a later checksum failure cannot
        // conceal positional parsing. The record decoder itself must reject it.
        prepared.stage.records_hash = hash_text(&Sha256::digest(bytes));
        prepared.stage.records_bytes = bytes.len() as u64;
        let mut emitted = 0;
        let result = visit_batches(&prepared.stage, |_, _| {
            emitted += 1;
            Ok(())
        });
        assert_eq!(
            result.unwrap_err().code(),
            CatalogueFailure::Stage,
            "{kind}"
        );
        assert_eq!(emitted, 0, "{kind}");
    }
}

fn retained(prepared: &PreparedCatalogue) -> Vec<Step> {
    let mut steps = vec![Step::get(
        "/storage/v1/bucket/evidence".into(),
        200,
        br#"{"id":"evidence","public":false}"#.to_vec(),
    )];
    for (key, path) in [
        (
            prepared.terms["metadataRef"].as_str().unwrap(),
            prepared.stage.source.metadata_path.as_path(),
        ),
        (
            prepared.terms["artifactRef"].as_str().unwrap(),
            prepared.stage.source.artifact_path.as_path(),
        ),
        (
            prepared.terms["stageRef"].as_str().unwrap(),
            prepared.stage.records_path.as_path(),
        ),
    ] {
        steps.push(Step::get(
            format!("/storage/v1/object/authenticated/evidence/{key}"),
            200,
            fs::read(path).unwrap(),
        ));
    }
    steps
}
fn begin_step(prepared: &PreparedCatalogue, id: Uuid) -> Step {
    let mut step = Step::json("/rest/v1/rpc/begin_catalogue_publication", json!(id));
    step.expect = Some(serde_json::to_vec(&json!({"p_terms":prepared.terms})).unwrap());
    step
}

#[test]
fn batch_plan_bounds_and_deduplicated_identity_counts_cover_the_whole_stage() {
    let oracle = Uuid::new_v4();
    let set = Uuid::new_v4();
    let fixture = fixture(&(0..501).map(|_| card(oracle, set)).collect::<Vec<_>>());
    assert!(fixture.artifact.is_file());
    let prepared = prepare(&fixture.qualification, &fixture.root).unwrap();
    let mut counts = Vec::new();
    let hashes = visit_batches(&prepared.stage, |sequence, body| {
        assert_eq!(sequence as usize, counts.len() + 1);
        assert!(body.len() <= MAX_BODY);
        counts.push(serde_json::from_str::<Vec<Value>>(body).unwrap().len());
        Ok(())
    })
    .unwrap();
    assert_eq!(counts, vec![500, 1]);
    assert_eq!(hashes, prepared.hashes);
    assert_eq!(prepared.terms["batchHashes"], json!(hashes));
    assert_eq!(prepared.stage.printing_count, 1002);
    let mut bytes = fs::read(&prepared.stage.records_path).unwrap();
    bytes.extend_from_slice(b"\n");
    fs::write(&prepared.stage.records_path, bytes).unwrap();
    assert_eq!(
        visit_batches(&prepared.stage, |_, _| Ok(()))
            .unwrap_err()
            .code(),
        CatalogueFailure::Stage
    );
}

#[test]
fn lost_batch_acknowledgment_replays_the_same_source_plan_and_body_without_reupload() {
    let fixture = fixture(&[card(Uuid::new_v4(), Uuid::new_v4())]);
    assert!(fixture.artifact.is_file());
    let prepared = prepare(&fixture.qualification, &fixture.root).unwrap();
    let publication = Uuid::new_v4();
    let batch = Uuid::new_v4();
    let mut bodies = Vec::new();
    visit_batches(&prepared.stage, |_, body| {
        bodies.push(body.to_owned());
        Ok(())
    })
    .unwrap();
    let payload = json!({"p_publication":publication,"p_sequence":1,"p_body":bodies[0]});
    let mut steps = retained(&prepared);
    steps.push(begin_step(&prepared, publication));
    let mut lost = Step::json("/rest/v1/rpc/append_catalogue_batch", json!(batch));
    lost.expect = Some(serde_json::to_vec(&payload).unwrap());
    lost.drop_ack = true;
    steps.push(lost);
    steps.extend(retained(&prepared));
    steps.push(begin_step(&prepared, publication));
    let mut replay = Step::json("/rest/v1/rpc/append_catalogue_batch", json!(batch));
    replay.expect = Some(serde_json::to_vec(&payload).unwrap());
    steps.push(replay);
    steps.push(Step::json(
        "/rest/v1/rpc/finish_catalogue_publication",
        json!(publication),
    ));
    let mock = Mock::new(steps);
    let publisher = CataloguePublisher::new(&mock.base, "sb_secret_test_only", "evidence").unwrap();
    assert_eq!(
        publisher.publish_prepared(&prepared).unwrap_err().code(),
        CatalogueFailure::Backend
    );
    let report = publisher.publish_prepared(&prepared).unwrap();
    assert_eq!(report.publication_id, publication);
    assert_eq!(report.batch_count, 1);
    let seen = mock.finish();
    assert_eq!(
        seen.iter()
            .filter(|(path, _, _)| path == "/rest/v1/rpc/append_catalogue_batch")
            .count(),
        2
    );
    assert!(
        !seen
            .iter()
            .any(|(path, _, _)| path.starts_with("/storage/v1/object/evidence"))
    );
}

#[test]
fn mutation_after_retention_cannot_dispatch_a_changed_batch_or_reach_release() {
    let fixture = fixture(&[card(Uuid::new_v4(), Uuid::new_v4())]);
    assert!(fixture.artifact.is_file());
    let prepared = prepare(&fixture.qualification, &fixture.root).unwrap();
    let publication = Uuid::new_v4();
    let mut steps = retained(&prepared);
    steps.push(begin_step(&prepared, publication));
    let changed = prepared.stage.records_path.clone();
    let mock = Mock::new_with_observer(steps, move |path| {
        if path == "/rest/v1/rpc/begin_catalogue_publication" {
            fs::OpenOptions::new()
                .append(true)
                .open(&changed)
                .unwrap()
                .write_all(b"invalid trailing line\n")
                .unwrap();
        }
    });
    let publisher = CataloguePublisher::new(&mock.base, "sb_secret_test_only", "evidence").unwrap();
    assert_eq!(
        publisher.publish_prepared(&prepared).unwrap_err().code(),
        CatalogueFailure::Stage
    );
    let seen = mock.finish();
    assert_eq!(
        seen.iter()
            .filter(|(path, _, _)| path.starts_with("/rest/"))
            .count(),
        1
    );
}

#[test]
fn malformed_source_configuration_never_creates_a_publisher() {
    for (base, key, bucket) in [
        ("http://remote.example", "sb_secret_test_only", "evidence"),
        ("https://api.example", "publishable", "evidence"),
        ("https://api.example", "sb_secret_test_only", "../other"),
    ] {
        assert!(
            matches!(CataloguePublisher::new(base,key,bucket),Err(error) if error.code()==CatalogueFailure::Configuration)
        );
    }
}

#[test]
fn large_metadata_uses_byte_bounded_batches_before_the_record_limit() {
    let oracle = Uuid::new_v4();
    let set = Uuid::new_v4();
    let rows = (0..20)
        .map(|_| {
            let mut value = card(oracle, set);
            value["card_faces"] = json!(
                (0..16)
                    .map(|index| json!({"name":format!("Face {index} {}","★".repeat(512))}))
                    .collect::<Vec<_>>()
            );
            value
        })
        .collect::<Vec<_>>();
    let fixture = fixture(&rows);
    assert!(fixture.artifact.is_file());
    let prepared = prepare(&fixture.qualification, &fixture.root).unwrap();
    let mut total = 0;
    let mut batches = 0;
    visit_batches(&prepared.stage, |_, body| {
        assert!(body.len() <= MAX_BODY);
        let records = serde_json::from_str::<Vec<Value>>(body).unwrap();
        assert!(records.len() < 500);
        total += records.len();
        batches += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(total, 20);
    assert!(batches > 1);
}

#[test]
fn verified_uploads_and_unknown_final_ack_recover_without_changing_the_publication() {
    let fixture = fixture(&[card(Uuid::new_v4(), Uuid::new_v4())]);
    assert!(fixture.artifact.is_file());
    let prepared = prepare(&fixture.qualification, &fixture.root).unwrap();
    let publication = Uuid::new_v4();
    let batch = Uuid::new_v4();
    let mut steps = vec![Step::get(
        "/storage/v1/bucket/evidence".into(),
        200,
        br#"{"id":"evidence","public":false}"#.to_vec(),
    )];
    for (key, path) in [
        (
            prepared.terms["metadataRef"].as_str().unwrap(),
            prepared.stage.source.metadata_path.as_path(),
        ),
        (
            prepared.terms["artifactRef"].as_str().unwrap(),
            fixture.artifact.as_path(),
        ),
        (
            prepared.terms["stageRef"].as_str().unwrap(),
            prepared.stage.records_path.as_path(),
        ),
    ] {
        let bytes = fs::read(path).unwrap();
        steps.push(Step::get(
            format!("/storage/v1/object/authenticated/evidence/{key}"),
            404,
            vec![],
        ));
        let mut upload = Step::json(&format!("/storage/v1/object/evidence/{key}"), json!({}));
        upload.expect = Some(bytes.clone());
        steps.push(upload);
        steps.push(Step::get(
            format!("/storage/v1/object/authenticated/evidence/{key}"),
            200,
            bytes,
        ));
    }
    steps.push(begin_step(&prepared, publication));
    steps.push(Step::json(
        "/rest/v1/rpc/append_catalogue_batch",
        json!(batch),
    ));
    let mut lost = Step::json(
        "/rest/v1/rpc/finish_catalogue_publication",
        json!(publication),
    );
    lost.drop_ack = true;
    steps.push(lost);
    steps.extend(retained(&prepared));
    steps.push(begin_step(&prepared, publication));
    steps.push(Step::json(
        "/rest/v1/rpc/append_catalogue_batch",
        json!(batch),
    ));
    steps.push(Step::json(
        "/rest/v1/rpc/finish_catalogue_publication",
        json!(publication),
    ));
    let mock = Mock::new(steps);
    let publisher = CataloguePublisher::new(&mock.base, "sb_secret_test_only", "evidence").unwrap();
    assert_eq!(
        publisher.publish_prepared(&prepared).unwrap_err().code(),
        CatalogueFailure::Backend
    );
    assert_eq!(
        publisher
            .publish_prepared(&prepared)
            .unwrap()
            .publication_id,
        publication
    );
    let seen = mock.finish();
    let uploads = seen
        .iter()
        .filter(|(path, _, _)| path.starts_with("/storage/v1/object/evidence/"))
        .collect::<Vec<_>>();
    assert_eq!(uploads.len(), 3);
    assert!(
        uploads
            .iter()
            .all(|(_, headers, _)| headers.get("x-upsert").map(String::as_str) == Some("false"))
    );
}

#[test]
#[ignore = "requires explicitly selected retained source; performs no network or publication"]
fn retained_full_source_produces_a_repeatable_complete_batch_plan() {
    let qualification = std::env::var_os("SPELLWAKE_CATALOGUE_QUALIFICATION_INPUT")
        .expect("retained source qualification required");
    let output = std::path::PathBuf::from(
        std::env::var_os("SPELLWAKE_CATALOGUE_PLAN_OUTPUT").expect("owned plan output required"),
    );
    let destination = output.parent().expect("output parent");
    let prepared = prepare(Path::new(&qualification), destination).unwrap();
    assert_eq!(
        visit_batches(&prepared.stage, |_, _| Ok(())).unwrap(),
        prepared.hashes
    );
    let report = json!({"scope":"retained-source-and-deterministic-batch-plan-no-publication","stage":prepared.stage,"terms":prepared.terms});
    fs::write(output, serde_json::to_vec(&report).unwrap()).unwrap();
}
