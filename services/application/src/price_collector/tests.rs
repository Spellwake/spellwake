use super::*;
use crate::price_source::hash_file;
use crate::test_http::{Mock, Step};
use flate2::{Compression, write::GzEncoder};
use reqwest::{blocking::Client, redirect::Policy};
use std::fs::File;
use std::io::Write;

struct Stage(PathBuf);
impl Stage {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("spellwake-collector-test-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn time(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}
fn make_candidate(stage: &Stage) -> QualifiedArtifact {
    let card =
        json!({"object":"card","id":Uuid::new_v4(),"finishes":["nonfoil"],"prices":{"usd":"1.23"}});
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(card.to_string().as_bytes()).unwrap();
    let bytes = gz.finish().unwrap();
    let artifact = stage.put("cards.gz", &bytes);
    let metadata:crate::price_source::BulkArtifact=serde_json::from_value(json!({"object":"bulk_data","id":"922288cb-4bef-45e1-bb30-0c2bd3d3534f","type":"all_cards","updated_at":"2026-11-01T11:00:00Z","jsonl_download_uri":"https://data.scryfall.io/all-cards/all-cards-20261101110000.jsonl.gz","compressed_size":bytes.len()})).unwrap();
    let index = stage.put(
        "index.json",
        &serde_json::to_vec(&json!({"object":"list","has_more":false,"data":[metadata]})).unwrap(),
    );
    let (hash, size) = hash_file(&artifact).unwrap();
    let (metadata_hash, _) = hash_file(&index).unwrap();
    QualifiedArtifact {
        metadata,
        acquired_at: time("2026-11-01T12:10:00Z"),
        artifact_path: artifact,
        metadata_path: index,
        hash,
        metadata_hash,
        qualification_path: stage.0.join("fixture.json"),
        compressed_bytes: size,
        decompressed_bytes: 0,
        record_count: 1,
    }
}
fn selected(candidate: &QualifiedArtifact) -> EstablishedSlot {
    serde_json::from_value(json!({"id":Uuid::new_v4(),"manifest":{"version":"scryfall-slot-v1","state":"valid","source":"scryfall:all_cards","slot":"2026-11-01","hash":candidate.hash,"artifactRef":candidate.artifact_ref(),"metadataHash":candidate.metadata_hash,"metadataRef":candidate.metadata_ref(),"sourceUpdatedAt":candidate.metadata.updated_at,"acquiredAt":candidate.acquired_at,"recordCount":"1","downloadUri":candidate.metadata.jsonl_download_uri}})).unwrap()
}
fn missing() -> EstablishedSlot {
    serde_json::from_value(json!({"id":Uuid::new_v4(),"manifest":{"version":"scryfall-slot-v1","state":"missing","slot":"2026-11-01"}})).unwrap()
}

#[test]
fn private_objects_reject_slot_and_work_arrays_before_storage_or_market_actions() {
    let stage = Stage::new();
    let slot = time("2026-11-01T12:00:00Z").date_naive();
    let selected = serde_json::to_value(missing()).unwrap();
    let mock = Mock::new(vec![Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        crate::test_http::positional(&selected, &["id", "manifest"]),
    )]);
    assert!(mock.client(&stage).read_slot(slot).is_err());
    assert_eq!(mock.finish().len(), 1);
    for page in [json!([[], null]), json!([[Uuid::new_v4()], null])] {
        let mock = Mock::new(vec![Step::json(
            "/rest/v1/rpc/read_prediction_maintenance_page",
            page,
        )]);
        assert!(mock.client(&stage).maintain_predictions(None).is_err());
        assert_eq!(mock.finish().len(), 1);
    }
    let mock = Mock::new(vec![
        Step::json("/rest/v1/rpc/read_established_price_slot", selected),
        Step::json(
            "/rest/v1/rpc/read_prediction_extraction_page",
            json!({"items":[[Uuid::new_v4(),Uuid::new_v4().to_string()]],"nextCursor":null}),
        ),
    ]);
    assert!(mock.client(&stage).extract_slot(slot).is_err());
    assert_eq!(mock.finish().len(), 2);
}
impl Mock {
    fn client(&self, stage: &Stage) -> PriceCollector {
        PriceCollector::new(
            CollectorConfig::new(
                &self.base,
                "sb_secret_test_only",
                "evidence",
                stage.0.clone(),
            )
            .unwrap(),
        )
        .unwrap()
    }
}
fn retention_steps(candidate: &QualifiedArtifact) -> Vec<Step> {
    let mut steps = vec![Step::get(
        "/storage/v1/bucket/evidence".into(),
        200,
        serde_json::to_vec(&json!({"id":"evidence","public":false})).unwrap(),
    )];
    for (file, key) in [
        (&candidate.metadata_path, candidate.metadata_ref()),
        (&candidate.artifact_path, candidate.artifact_ref()),
    ] {
        let get = format!("/storage/v1/object/authenticated/evidence/{key}");
        let content = fs::read(file).unwrap();
        steps.push(Step::get(get.clone(), 400, br#"{"statusCode":"404","code":"NoSuchKey","error":"not_found","message":"Object not found"}"#.to_vec()));
        steps.push(Step {
            reply: None,
            raw_response: None,
            method: "POST",
            path: format!("/storage/v1/object/evidence/{key}"),
            status: 200,
            body: b"{}".to_vec(),
            expect: Some(content.clone()),
            drop_ack: false,
            credentials: crate::test_http::Credentials::Backend,
            response_headers: Vec::new(),
        });
        steps.push(Step::get(get, 200, content));
    }
    steps
}
#[test]
fn retains_exact_index_and_gzip_then_returns_the_actual_winning_receipt() {
    let stage = Stage::new();
    let candidate = make_candidate(&stage);
    let other_stage = Stage::new();
    let winner = selected(&make_candidate(&other_stage));
    let mut steps = vec![Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        Value::Null,
    )];
    steps.extend(retention_steps(&candidate));
    steps.push(Step::json(
        "/rest/v1/rpc/commit_price_slot",
        json!(winner.id),
    ));
    steps.push(Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        serde_json::to_value(&winner).unwrap(),
    ));
    let mock = Mock::new(steps);
    let client = mock.client(&stage);
    let expected = serde_json::to_value(&winner).unwrap();
    let result = client
        .collect_at(time("2026-11-01T12:00:00Z"), || Ok(candidate))
        .unwrap();
    assert_eq!(serde_json::to_value(result).unwrap(), expected);
    let requests = mock.finish();
    let committed: Value = serde_json::from_slice(&requests[8].2).unwrap();
    assert_eq!(committed["p_artifact"]["recordCount"], "1");
    assert!(committed["p_artifact"]["metadataHash"].is_string());
    for (_, headers, _) in requests
        .iter()
        .filter(|(path, _, _)| path.starts_with("/storage/v1/object/evidence/"))
    {
        assert_eq!(headers.get("x-upsert").map(String::as_str), Some("false"));
    }
}
#[test]
fn unknown_commit_acknowledgement_is_reconciled_without_replaying_the_source() {
    let stage = Stage::new();
    let candidate = make_candidate(&stage);
    let winner = selected(&candidate);
    let mut steps = vec![Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        Value::Null,
    )];
    steps.extend(retention_steps(&candidate));
    let mut commit = Step::json("/rest/v1/rpc/commit_price_slot", Value::Null);
    commit.drop_ack = true;
    steps.push(commit);
    steps.push(Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        serde_json::to_value(&winner).unwrap(),
    ));
    let mock = Mock::new(steps);
    assert_eq!(
        mock.client(&stage)
            .collect_at(time("2026-11-01T12:00:00Z"), || Ok(candidate))
            .unwrap()
            .id,
        winner.id
    );
    mock.finish();
}
#[test]
fn existing_or_missed_slots_do_not_fetch_or_upload_a_new_artifact() {
    let stage = Stage::new();
    let winner = missing();
    let mock = Mock::new(vec![Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        serde_json::to_value(&winner).unwrap(),
    )]);
    assert_eq!(
        mock.client(&stage)
            .collect_at(time("2026-11-01T12:00:00Z"), || panic!(
                "source must not be fetched"
            ))
            .unwrap()
            .id,
        winner.id
    );
    mock.finish();
    let mock = Mock::new(vec![
        Step::json("/rest/v1/rpc/read_established_price_slot", Value::Null),
        Step::json("/rest/v1/rpc/commit_price_slot", json!(winner.id)),
        Step::json(
            "/rest/v1/rpc/read_established_price_slot",
            serde_json::to_value(&winner).unwrap(),
        ),
    ]);
    assert_eq!(
        mock.client(&stage)
            .collect_at(time("2026-11-01T13:00:00Z"), || panic!(
                "late source must not be fetched"
            ))
            .unwrap()
            .id,
        winner.id
    );
    let calls = mock.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[1].2).unwrap()["p_artifact"],
        Value::Null
    );
    let mock = Mock::new(vec![Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        Value::Null,
    )]);
    assert!(matches!(
        mock.client(&stage)
            .collect_at(time("2026-11-01T11:59:59Z"), || panic!(
                "early source must not be fetched"
            )),
        Err(CollectorError::NotDue)
    ));
    mock.finish();
}
#[test]
fn delayed_and_future_date_bound_tasks_never_acquire_a_different_days_source() {
    let stage = Stage::new();
    let winner = missing();
    let slot = time("2026-11-01T12:00:00Z").date_naive();
    let mock = Mock::new(vec![
        Step::json("/rest/v1/rpc/read_established_price_slot", Value::Null),
        Step::json("/rest/v1/rpc/commit_price_slot", json!(winner.id)),
        Step::json(
            "/rest/v1/rpc/read_established_price_slot",
            serde_json::to_value(&winner).unwrap(),
        ),
    ]);
    assert_eq!(
        mock.client(&stage)
            .collect_slot_at(slot, time("2026-11-02T12:00:00Z"), || panic!(
                "a delayed task must not fetch today's source"
            ))
            .unwrap()
            .id,
        winner.id
    );
    for (_, _, body) in mock.finish() {
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["p_slot"], "2026-11-01");
        if let Some(artifact) = body.get("p_artifact") {
            assert!(artifact.is_null());
        }
    }
    let mock = Mock::new(vec![Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        Value::Null,
    )]);
    assert!(matches!(
        mock.client(&stage).collect_slot_at(
            time("2026-11-02T12:00:00Z").date_naive(),
            time("2026-11-01T13:00:00Z"),
            || panic!("a future task must not fetch today's source"),
        ),
        Err(CollectorError::NotDue)
    ));
    let calls = mock.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[0].2).unwrap()["p_slot"],
        "2026-11-02"
    );
}

#[test]
fn corrupt_existing_storage_and_public_buckets_cannot_publish_a_slot() {
    let stage = Stage::new();
    let candidate = make_candidate(&stage);
    let get = format!(
        "/storage/v1/object/authenticated/evidence/{}",
        candidate.metadata_ref()
    );
    let mock = Mock::new(vec![
        Step::get(
            "/storage/v1/bucket/evidence".into(),
            200,
            br#"{"id":"evidence","public":false}"#.to_vec(),
        ),
        Step::get(
            get,
            200,
            vec![b'x'; fs::metadata(&candidate.metadata_path).unwrap().len() as usize],
        ),
    ]);
    assert!(matches!(
        mock.client(&stage)
            .select_candidate(time("2026-11-01T12:00:00Z").date_naive(), candidate),
        Err(CollectorError::RetentionMismatch)
    ));
    mock.finish();
    let candidate = make_candidate(&stage);
    let mock = Mock::new(vec![Step::get(
        "/storage/v1/bucket/evidence".into(),
        200,
        br#"{"id":"evidence","public":true}"#.to_vec(),
    )]);
    assert!(matches!(
        mock.client(&stage)
            .select_candidate(time("2026-11-01T12:00:00Z").date_naive(), candidate),
        Err(CollectorError::Storage(_))
    ));
    mock.finish();
}

#[test]
fn file_retention_rejects_transformed_readback_without_uploading_or_publishing() {
    let stage = Stage::new();
    let candidate = make_candidate(&stage);
    let mut response = Step::get(
        format!(
            "/storage/v1/object/authenticated/evidence/{}",
            candidate.metadata_ref()
        ),
        200,
        fs::read(&candidate.metadata_path).unwrap(),
    );
    response
        .response_headers
        .push(("Content-Encoding".into(), "gzip".into()));
    let mock = Mock::new(vec![
        Step::get(
            "/storage/v1/bucket/evidence".into(),
            200,
            br#"{"id":"evidence","public":false}"#.to_vec(),
        ),
        response,
    ]);
    assert!(matches!(
        mock.client(&stage)
            .select_candidate(time("2026-11-01T12:00:00Z").date_naive(), candidate),
        Err(CollectorError::Storage(_))
    ));
    assert_eq!(mock.finish().len(), 2);
}

#[test]
fn upload_lost_ack_and_conflict_are_verified_by_readback_without_overwrite() {
    for lost_ack in [true, false] {
        let stage = Stage::new();
        let candidate = make_candidate(&stage);
        let winner = selected(&candidate);
        let mut steps = retention_steps(&candidate);
        // Index upload acknowledgement is either lost or reports a same-key conflict.
        steps[2].drop_ack = lost_ack;
        steps[2].status = 409;
        steps.push(Step::json(
            "/rest/v1/rpc/commit_price_slot",
            json!(winner.id),
        ));
        steps.push(Step::json(
            "/rest/v1/rpc/read_established_price_slot",
            serde_json::to_value(&winner).unwrap(),
        ));
        let mock = Mock::new(steps);
        assert_eq!(
            mock.client(&stage)
                .select_candidate(time("2026-11-01T12:00:00Z").date_naive(), candidate)
                .unwrap()
                .id,
            winner.id
        );
        mock.finish();
    }
}
#[test]
fn existing_verified_objects_are_not_uploaded_again() {
    let stage = Stage::new();
    let candidate = make_candidate(&stage);
    let winner = selected(&candidate);
    let mut steps = vec![Step::get(
        "/storage/v1/bucket/evidence".into(),
        200,
        br#"{"id":"evidence","public":false}"#.to_vec(),
    )];
    for (file, key) in [
        (&candidate.metadata_path, candidate.metadata_ref()),
        (&candidate.artifact_path, candidate.artifact_ref()),
    ] {
        steps.push(Step::get(
            format!("/storage/v1/object/authenticated/evidence/{key}"),
            200,
            fs::read(file).unwrap(),
        ));
    }
    steps.push(Step::json(
        "/rest/v1/rpc/commit_price_slot",
        json!(winner.id),
    ));
    steps.push(Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        serde_json::to_value(&winner).unwrap(),
    ));
    let mock = Mock::new(steps);
    mock.client(&stage)
        .select_candidate(time("2026-11-01T12:00:00Z").date_naive(), candidate)
        .unwrap();
    assert!(
        mock.finish()
            .iter()
            .all(|(path, _, _)| !path.starts_with("/storage/v1/object/evidence/"))
    );
}
#[test]
fn unrelated_storage_error_is_not_mistaken_for_an_absent_object() {
    let stage = Stage::new();
    let candidate = make_candidate(&stage);
    let mock = Mock::new(vec![
        Step::get(
            "/storage/v1/bucket/evidence".into(),
            200,
            br#"{"id":"evidence","public":false}"#.to_vec(),
        ),
        Step::get(
            format!(
                "/storage/v1/object/authenticated/evidence/{}",
                candidate.metadata_ref()
            ),
            400,
            br#"{"statusCode":"404","code":"NoSuchBucket"}"#.to_vec(),
        ),
    ]);
    assert!(matches!(
        mock.client(&stage)
            .select_candidate(time("2026-11-01T12:00:00Z").date_naive(), candidate),
        Err(CollectorError::Storage(_))
    ));
    mock.finish();
}
#[test]
fn configuration_and_established_manifest_cannot_smuggle_credentials_or_another_source() {
    let stage = Stage::new();
    for (url, key, bucket) in [
        ("http://example.com/", "sb_secret_test_only", "evidence"),
        (
            "https://example.com/?token=bad",
            "sb_secret_test_only",
            "evidence",
        ),
        (
            "https://user:pass@example.com/",
            "sb_secret_test_only",
            "evidence",
        ),
        (
            "https://example.com/",
            "sb_publishable_test_only",
            "evidence",
        ),
        ("https://example.com/", "sb_secret_test_only", "../public"),
    ] {
        assert!(CollectorConfig::new(url, key, bucket, stage.0.clone()).is_err());
    }
    let fixture = make_candidate(&stage);
    let record = selected(&fixture);
    let value = serde_json::to_value(record).unwrap();
    for (field, bad) in [
        ("artifactRef", json!("../public/file.gz")),
        ("source", json!("another-provider")),
        ("recordCount", json!("5000001")),
        ("recordCount", json!("01")),
        ("slot", json!("2026-11-02")),
        ("downloadUri", json!("http://example.com/price.gz")),
    ] {
        let mut changed = value.clone();
        changed["manifest"][field] = bad;
        let parsed: EstablishedSlot = serde_json::from_value(changed).unwrap();
        assert!(
            parsed
                .validate(time("2026-11-01T12:00:00Z").date_naive())
                .is_err()
        );
    }
}

#[test]
#[ignore = "requires the isolated real Spellwake local Supabase stack and private local key"]
fn isolated_supabase_storage_and_source_rpc_interoperate() {
    // Exact local target guard precedes loading the private key. Never point this
    // qualification at an existing hosted project or another local workspace.
    assert!(
        std::env::var("SPELLWAKE_SUPABASE_URL").is_ok_and(|url| url == "http://127.0.0.1:55431")
    );
    assert!(
        std::env::var("SPELLWAKE_PRICE_BUCKET").is_ok_and(|bucket| bucket == "spellwake-evidence")
    );
    let stage = Stage::new();
    let client = PriceCollector::new(CollectorConfig::from_env().unwrap()).unwrap();
    client.private_bucket().unwrap();
    assert!(client.read_slot(Utc::now().date_naive()).unwrap().is_none());
    let candidate = make_candidate(&stage);
    let mut cleanup = Vec::new();
    for (path, key, hash, kind) in [
        (
            &candidate.metadata_path,
            candidate.metadata_ref(),
            &candidate.metadata_hash,
            "application/json",
        ),
        (
            &candidate.artifact_path,
            candidate.artifact_ref(),
            &candidate.hash,
            "application/gzip",
        ),
    ] {
        let size = fs::metadata(path).unwrap().len();
        client
            .retain(path, &key, hash, size, &stage.0, kind)
            .unwrap();
        // Retry follows the real existing-object readback path without upsert.
        client
            .retain(path, &key, hash, size, &stage.0, kind)
            .unwrap();
        cleanup.push((key, hash.clone(), size));
    }
    let publishable = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    assert!(publishable.starts_with("sb_publishable_"));
    let anonymous = Client::builder().redirect(Policy::none()).build().unwrap();
    let denied = anonymous
        .get(client.object_url(&candidate.artifact_ref(), true).unwrap())
        .header("apikey", &publishable)
        .send()
        .unwrap();
    assert!(
        !denied.status().is_success(),
        "private evidence must not be readable with the public key"
    );
    let denied = anonymous
        .post(
            client
                .backend
                .url("rest/v1/rpc/read_established_price_slot")
                .unwrap(),
        )
        .header("apikey", &publishable)
        .header("Content-Profile", "app")
        .json(&json!({"p_slot":Utc::now().date_naive()}))
        .send()
        .unwrap();
    assert!(
        !denied.status().is_success(),
        "source receipt RPC must remain backend-only"
    );
    if let Some(raw) = std::env::var_os("SPELLWAKE_PRICE_LARGE_ARTIFACT") {
        let artifact = fs::canonicalize(PathBuf::from(raw)).unwrap();
        let owned = fs::canonicalize(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("target/source-qualification"),
        )
        .unwrap();
        assert!(artifact.starts_with(owned));
        let folder = artifact.parent().unwrap();
        let proof: Value =
            serde_json::from_slice(&fs::read(folder.join("inspection.json")).unwrap()).unwrap();
        assert_eq!(proof["status"], "success");
        for (path, field, kind) in [
            (
                folder.join("bulk-index.json"),
                "metadataHash",
                "application/json",
            ),
            (artifact.clone(), "hash", "application/gzip"),
        ] {
            let expected: EvidenceHash =
                serde_json::from_value(proof["data"][field].clone()).unwrap();
            let (hash, size) = hash_file(&path).unwrap();
            assert_eq!(hash, expected);
            let key = if field == "hash" {
                format!("prices/scryfall/{}.jsonl.gz", hash.hex())
            } else {
                format!("prices/scryfall/metadata/{}.json", hash.hex())
            };
            client
                .retain(&path, &key, &hash, size, &stage.0, kind)
                .unwrap();
            cleanup.push((key, hash.clone(), size));
        }
    }
    // No current daily slot or observation was created. The historical fixture
    // below exercises only an explicit missed-slot disposition, never a price.
    assert!(client.read_slot(Utc::now().date_naive()).unwrap().is_none());
    let historical = NaiveDate::from_ymd_opt(1900, 1, 1).unwrap();
    let missing = client.decide(historical, None).unwrap();
    assert!(matches!(missing.manifest, SlotManifest::Missing(_)));
    assert_eq!(client.decide(historical, None).unwrap().id, missing.id);
    let response = client
        .backend
        .request(
            Method::DELETE,
            client
                .backend
                .url("storage/v1/object/spellwake-evidence")
                .unwrap(),
        )
        .unwrap()
        .json(&json!({"prefixes":cleanup.iter().map(|(key,_,_)|key).collect::<Vec<_>>()}))
        .send()
        .unwrap();
    assert!(response.status().is_success());
    for (key, hash, size) in cleanup {
        assert!(
            !crate::private_storage::verify_file(
                &client.backend,
                &client.bucket,
                &key,
                &hash,
                size,
                &stage.0
            )
            .unwrap()
        );
    }
}

fn card_in(candidate: &QualifiedArtifact) -> Value {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(File::open(&candidate.artifact_path).unwrap())
        .read_to_end(&mut bytes)
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn maintenance_delivers_only_identities_and_validates_terminal_dispositions() {
    let stage = Stage::new();
    let ids = [Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3)];
    let operation = Uuid::new_v4();
    let mock = Mock::new(vec![
        Step::json(
            "/rest/v1/rpc/read_prediction_maintenance_page",
            json!({"items":ids,"nextCursor":null}),
        ),
        Step::json(
            "/rest/v1/rpc/maintain_prediction_market",
            json!({"state":"pending"}),
        ),
        Step::json(
            "/rest/v1/rpc/maintain_prediction_market",
            json!({"state":"final","outcome":"yes","operationId":operation}),
        ),
        Step::json(
            "/rest/v1/rpc/maintain_prediction_market",
            json!({"state":"not_due"}),
        ),
    ]);
    let batch = mock.client(&stage).maintain_predictions(None).unwrap();
    assert_eq!(
        (
            batch.processed,
            batch.settled,
            batch.more,
            batch.next_cursor
        ),
        (3, 1, false, None)
    );
    let calls = mock.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[0].2).unwrap(),
        json!({"p_after":null,"p_limit":100})
    );
    for (call, id) in calls[1..].iter().zip(ids) {
        assert_eq!(
            serde_json::from_slice::<Value>(&call.2).unwrap(),
            json!({"p_market":id})
        );
    }
}

#[test]
fn maintenance_rejects_invalid_work_pages_before_mutating_any_market() {
    let stage = Stage::new();
    let a = Uuid::from_u128(1);
    let b = Uuid::from_u128(2);
    let excessive: Vec<_> = (1..=101).map(Uuid::from_u128).collect();
    for items in [vec![b, a], vec![a, a], vec![Uuid::nil()], excessive] {
        let mock = Mock::new(vec![Step::json(
            "/rest/v1/rpc/read_prediction_maintenance_page",
            json!({"items":items,"nextCursor":null}),
        )]);
        assert!(mock.client(&stage).maintain_predictions(None).is_err());
        assert_eq!(mock.finish().len(), 1);
    }
    let mock = Mock::new(vec![Step::json(
        "/rest/v1/rpc/read_prediction_maintenance_page",
        json!({"items":[a],"nextCursor":a}),
    )]);
    assert!(mock.client(&stage).maintain_predictions(None).is_err());
    mock.finish();
    let mock = Mock::new(vec![]);
    assert!(matches!(
        mock.client(&stage).maintain_predictions(Some(Uuid::nil())),
        Err(CollectorError::InvalidCursor)
    ));
    assert!(mock.finish().is_empty());
}

#[test]
fn maintenance_lost_ack_stops_and_retry_uses_remaining_work_without_replaying_a_posting() {
    let stage = Stage::new();
    let id = Uuid::from_u128(1);
    let mut lost = Step::json("/rest/v1/rpc/maintain_prediction_market", Value::Null);
    lost.drop_ack = true;
    let mock = Mock::new(vec![
        Step::json(
            "/rest/v1/rpc/read_prediction_maintenance_page",
            json!({"items":[id],"nextCursor":null}),
        ),
        lost,
        Step::json(
            "/rest/v1/rpc/read_prediction_maintenance_page",
            json!({"items":[],"nextCursor":null}),
        ),
    ]);
    let client = mock.client(&stage);
    assert!(client.maintain_predictions(None).is_err());
    assert_eq!(client.maintain_predictions(None).unwrap().processed, 0);
    assert_eq!(mock.finish().len(), 3);
    for state in [
        json!({"state":"final","outcome":"maybe","operationId":Uuid::new_v4()}),
        json!({"state":"final","outcome":"yes","operationId":Uuid::nil()}),
    ] {
        let mock = Mock::new(vec![
            Step::json(
                "/rest/v1/rpc/read_prediction_maintenance_page",
                json!({"items":[id],"nextCursor":null}),
            ),
            Step::json("/rest/v1/rpc/maintain_prediction_market", state),
        ]);
        assert!(mock.client(&stage).maintain_predictions(None).is_err());
        mock.finish();
    }
}

#[test]
fn maintenance_full_page_returns_cursor_without_starting_an_unbounded_next_page() {
    let stage = Stage::new();
    let ids: Vec<_> = (1..=100).map(Uuid::from_u128).collect();
    let last = *ids.last().unwrap();
    let mut steps = vec![Step::json(
        "/rest/v1/rpc/read_prediction_maintenance_page",
        json!({"items":ids,"nextCursor":last}),
    )];
    steps.extend((0..100).map(|_| {
        Step::json(
            "/rest/v1/rpc/maintain_prediction_market",
            json!({"state":"pending"}),
        )
    }));
    let mock = Mock::new(steps);
    let batch = mock.client(&stage).maintain_predictions(None).unwrap();
    assert_eq!(
        (
            batch.processed,
            batch.settled,
            batch.more,
            batch.next_cursor
        ),
        (100, 0, true, Some(last))
    );
    assert_eq!(mock.finish().len(), 101);
}
fn selected_downloads(candidate: &QualifiedArtifact) -> Vec<Step> {
    vec![
        Step::get(
            format!(
                "/storage/v1/object/authenticated/evidence/{}",
                candidate.metadata_ref()
            ),
            200,
            fs::read(&candidate.metadata_path).unwrap(),
        ),
        Step::get(
            format!(
                "/storage/v1/object/authenticated/evidence/{}",
                candidate.artifact_ref()
            ),
            200,
            fs::read(&candidate.artifact_path).unwrap(),
        ),
    ]
}
#[test]
fn extraction_delivers_only_cards_from_the_winning_retained_artifact() {
    let stage = Stage::new();
    let candidate = make_candidate(&stage);
    let selected = selected(&candidate);
    let card = card_in(&candidate);
    let printing = card["id"].as_str().unwrap();
    let absent = Uuid::new_v4();
    let mut markets = [Uuid::new_v4(), Uuid::new_v4()];
    markets.sort();
    let mut steps = vec![
        Step::json(
            "/rest/v1/rpc/read_established_price_slot",
            serde_json::to_value(&selected).unwrap(),
        ),
        Step::json(
            "/rest/v1/rpc/read_prediction_extraction_page",
            json!({"items":[{"marketId":markets[0],"printingRef":printing},{"marketId":markets[1],"printingRef":absent}],"nextCursor":null}),
        ),
    ];
    steps.extend(selected_downloads(&candidate));
    steps.push(Step::json(
        "/rest/v1/rpc/record_prediction_observation",
        json!(Uuid::new_v4()),
    ));
    steps.push(Step::json(
        "/rest/v1/rpc/record_prediction_observation",
        json!(Uuid::new_v4()),
    ));
    let mock = Mock::new(steps);
    let result = mock
        .client(&stage)
        .extract_slot(time("2026-11-01T13:00:00Z").date_naive())
        .unwrap();
    assert_eq!(result.processed, 2);
    assert!(!result.more);
    assert_eq!(result.manifest_id, selected.id);
    let calls = mock.finish();
    let found: Value = serde_json::from_slice(&calls[4].2).unwrap();
    let missing: Value = serde_json::from_slice(&calls[5].2).unwrap();
    assert_eq!(found["p_card"]["prices"]["usd"], "1.23");
    assert_eq!(found["p_manifest"], selected.id.to_string());
    assert_eq!(missing["p_card"], Value::Null);
    assert!(!fs::read_dir(&stage.0).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("retained-")
    }));
}
#[test]
fn missing_manifest_delivers_no_price_and_never_reads_storage() {
    let stage = Stage::new();
    let selected = missing();
    let market = Uuid::new_v4();
    let mock = Mock::new(vec![
        Step::json(
            "/rest/v1/rpc/read_established_price_slot",
            serde_json::to_value(&selected).unwrap(),
        ),
        Step::json(
            "/rest/v1/rpc/read_prediction_extraction_page",
            json!({"items":[{"marketId":market,"printingRef":Uuid::new_v4()}],"nextCursor":null}),
        ),
        Step::json(
            "/rest/v1/rpc/record_prediction_observation",
            json!(Uuid::new_v4()),
        ),
    ]);
    assert_eq!(
        mock.client(&stage)
            .extract_slot(time("2026-11-01T13:00:00Z").date_naive())
            .unwrap()
            .processed,
        1
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&mock.finish()[2].2).unwrap()["p_card"],
        Value::Null
    );
}
#[test]
fn corruption_or_unreadable_selected_storage_cannot_be_reported_as_missing_card() {
    for unavailable in [false, true] {
        let stage = Stage::new();
        let candidate = make_candidate(&stage);
        let selected = selected(&candidate);
        let mut steps = vec![
            Step::json(
                "/rest/v1/rpc/read_established_price_slot",
                serde_json::to_value(&selected).unwrap(),
            ),
            Step::json(
                "/rest/v1/rpc/read_prediction_extraction_page",
                json!({"items":[{"marketId":Uuid::new_v4(),"printingRef":Uuid::new_v4()}],"nextCursor":null}),
            ),
        ];
        let mut downloads = selected_downloads(&candidate);
        if unavailable {
            downloads.truncate(1);
            downloads[0].status = 404;
            downloads[0].body.clear();
        } else {
            downloads[1].body[10] ^= 1;
        }
        steps.extend(downloads);
        let mock = Mock::new(steps);
        assert!(
            mock.client(&stage)
                .extract_slot(time("2026-11-01T13:00:00Z").date_naive())
                .is_err()
        );
        mock.finish();
    }
}
fn fresh_probe_source(stage: &Stage) -> (QualifiedArtifact, EstablishedSlot) {
    let mut candidate = make_candidate(stage);
    let now = Utc::now();
    let slot = if now.hour() < 12 {
        now.date_naive() - Duration::days(1)
    } else {
        now.date_naive()
    };
    candidate.acquired_at = slot
        .and_hms_opt(12, if now.hour() == 12 { 0 } else { 30 }, 0)
        .unwrap()
        .and_utc();
    candidate.metadata.updated_at = candidate.acquired_at - Duration::hours(1);
    candidate.metadata.jsonl_download_uri = format!(
        "https://data.scryfall.io/all-cards/all-cards-{}.jsonl.gz",
        candidate.metadata.updated_at.format("%Y%m%d%H%M%S")
    );
    fs::write(
        &candidate.metadata_path,
        serde_json::to_vec(&json!({"object":"list","has_more":false,"data":[candidate.metadata]}))
            .unwrap(),
    )
    .unwrap();
    candidate.metadata_hash = hash_file(&candidate.metadata_path).unwrap().0;
    let mut selected = selected(&candidate);
    let SlotManifest::Valid(manifest) = &mut selected.manifest else {
        unreachable!()
    };
    manifest.slot = slot;
    (candidate, selected)
}

#[test]
fn probe_records_exact_selected_printing_without_an_alternative_source() {
    let stage = Stage::new();
    let (candidate, selected) = fresh_probe_source(&stage);
    let SlotManifest::Valid(manifest) = &selected.manifest else {
        unreachable!()
    };
    let slot = manifest.slot;
    let printing = Uuid::parse_str(card_in(&candidate)["id"].as_str().unwrap()).unwrap();
    let probe = Uuid::new_v4();
    let mut steps = vec![Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        serde_json::to_value(&selected).unwrap(),
    )];
    steps.push(Step::json(
        "/rest/v1/rpc/read_established_price_probe",
        Value::Null,
    ));
    steps.extend(selected_downloads(&candidate));
    steps.push(Step::json("/rest/v1/rpc/record_price_probe", json!(probe)));
    steps.push(Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        serde_json::to_value(&selected).unwrap(),
    ));
    steps.push(Step::json(
        "/rest/v1/rpc/read_established_price_probe",
        json!(probe),
    ));
    let mock = Mock::new(steps);
    assert_eq!(
        mock.client(&stage).probe_printing(slot, printing).unwrap(),
        probe
    );
    assert_eq!(
        mock.client(&stage).probe_printing(slot, printing).unwrap(),
        probe
    );
    let calls = mock.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[4].2).unwrap()["p_card"]["id"],
        printing.to_string()
    );
}

#[test]
fn extraction_stops_at_batch_limit_and_reports_remaining_work() {
    let stage = Stage::new();
    let selected = missing();
    let mut markets = (0..1000).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
    markets.sort();
    let printing = Uuid::new_v4();
    let mut steps = vec![Step::json(
        "/rest/v1/rpc/read_established_price_slot",
        serde_json::to_value(&selected).unwrap(),
    )];
    for chunk in markets.chunks(100) {
        let items = chunk
            .iter()
            .map(|market| json!({"marketId":market,"printingRef":printing}))
            .collect::<Vec<_>>();
        steps.push(Step::json(
            "/rest/v1/rpc/read_prediction_extraction_page",
            json!({"items":items,"nextCursor":chunk.last().unwrap()}),
        ));
    }
    for _ in &markets {
        steps.push(Step::json(
            "/rest/v1/rpc/record_prediction_observation",
            json!(Uuid::new_v4()),
        ));
    }
    let mock = Mock::new(steps);
    let report = mock
        .client(&stage)
        .extract_slot(time("2026-11-01T13:00:00Z").date_naive())
        .unwrap();
    assert_eq!(report.processed, 1000);
    assert!(report.more);
    mock.finish();
}
#[test]
fn lost_delivery_ack_resumes_from_established_work_without_replaying_an_acquisition() {
    let stage = Stage::new();
    let selected = missing();
    let mut markets = [Uuid::new_v4(), Uuid::new_v4()];
    markets.sort();
    let items = markets
        .iter()
        .map(|market| json!({"marketId":market,"printingRef":Uuid::new_v4()}))
        .collect::<Vec<_>>();
    let mut lost = Step::json("/rest/v1/rpc/record_prediction_observation", Value::Null);
    lost.drop_ack = true;
    let steps = vec![
        Step::json(
            "/rest/v1/rpc/read_established_price_slot",
            serde_json::to_value(&selected).unwrap(),
        ),
        Step::json(
            "/rest/v1/rpc/read_prediction_extraction_page",
            json!({"items":items,"nextCursor":null}),
        ),
        Step::json(
            "/rest/v1/rpc/record_prediction_observation",
            json!(Uuid::new_v4()),
        ),
        lost,
        Step::json(
            "/rest/v1/rpc/read_established_price_slot",
            serde_json::to_value(&selected).unwrap(),
        ),
        Step::json(
            "/rest/v1/rpc/read_prediction_extraction_page",
            json!({"items":[],"nextCursor":null}),
        ),
    ];
    let mock = Mock::new(steps);
    let collector = mock.client(&stage);
    let slot = time("2026-11-01T13:00:00Z").date_naive();
    assert!(collector.extract_slot(slot).is_err());
    assert_eq!(collector.extract_slot(slot).unwrap().processed, 0);
    mock.finish();
}

#[path = "wire_tests.rs"]
mod wire;

#[test]
fn extraction_stage_preserves_real_filesystem_failure_without_private_path_formatting() {
    use std::error::Error;
    let stage = Stage::new();
    let marker = "private-collector-stage-sentinel";
    let root = stage.put(marker, b"retained input");
    let expected = fs::create_dir_all(&root).unwrap_err().kind();
    let error = ExtractionStage::new(&root)
        .err()
        .expect("file is not a directory");
    assert!(matches!(error, CollectorError::Storage(_)));
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        expected
    );
    assert_eq!(error, error.clone());
    assert_eq!(
        error,
        CollectorError::storage(io::Error::other("different cause"))
    );
    assert_eq!(error.to_string(), "private price evidence retention failed");
    assert!(!format!("{error:?} {error}").contains(marker));
    assert_eq!(fs::read(root).unwrap(), b"retained input");
}

#[test]
fn retained_download_preserves_create_new_error_without_overwriting_existing_file() {
    use std::error::Error;
    let stage = Stage::new();
    let destination = stage.put("private-download-path-sentinel", b"original bytes");
    let mock = Mock::new(vec![Step::get(
        "/storage/v1/object/authenticated/evidence/retained.json".into(),
        200,
        b"replacement bytes".to_vec(),
    )]);
    let error = mock
        .client(&stage)
        .download_retained("retained.json", &destination, 1024, false)
        .unwrap_err();
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert!(!format!("{error:?} {error}").contains("private-download-path-sentinel"));
    assert_eq!(fs::read(destination).unwrap(), b"original bytes");
    assert_eq!(mock.finish().len(), 1);
}

#[test]
fn retained_download_keeps_http_status_cause_without_body_or_created_output() {
    use std::error::Error;
    let stage = Stage::new();
    let destination = stage.0.join("not-created.json");
    let marker = "private-storage-response-sentinel sb_secret_fixture_only";
    let mock = Mock::new(vec![Step::get(
        "/storage/v1/object/authenticated/evidence/retained.json".into(),
        503,
        marker.as_bytes().to_vec(),
    )]);
    let error = mock
        .client(&stage)
        .download_retained("retained.json", &destination, 1024, false)
        .unwrap_err();
    assert!(matches!(
        error
            .source()
            .unwrap()
            .downcast_ref::<crate::private_storage::StorageError>(),
        Some(crate::private_storage::StorageError::Status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        ))
    ));
    assert!(!format!("{error:?} {error}").contains(marker));
    assert!(!destination.exists());
    assert_eq!(mock.finish().len(), 1);
}

#[test]
fn retained_download_keeps_url_free_request_error_after_lost_response() {
    use std::error::Error;
    let stage = Stage::new();
    let key = "private-object-key-sentinel.json";
    let mut step = Step::get(
        format!("/storage/v1/object/authenticated/evidence/{key}"),
        200,
        b"ignored".to_vec(),
    );
    step.drop_ack = true;
    let mock = Mock::new(vec![step]);
    let destination = stage.0.join("not-created.json");
    let error = mock
        .client(&stage)
        .download_retained(key, &destination, 1024, false)
        .unwrap_err();
    let request = error
        .source()
        .unwrap()
        .downcast_ref::<reqwest::Error>()
        .unwrap();
    assert!(request.url().is_none());
    assert!(!format!("{error:?} {error} {request:?}").contains(key));
    assert!(!destination.exists());
    assert_eq!(mock.finish().len(), 1);
}
