//! Protocol fixtures qualify byte mechanics only; owned Storage test is separate.
use super::*;
use crate::{
    backend::BackendConfig,
    test_http::{Mock, Step},
};
use serde_json::json;
fn ids() -> (Uuid, Uuid) {
    (Uuid::from_u128(0xab41), Uuid::from_u128(0xab42))
}
fn reference() -> NativeArtifactRef {
    let (g, c) = ids();
    NativeArtifactRef::for_bytes(g, c, NativeArtifactRole::Input, b"private-native-fixture")
        .unwrap()
}
fn client(mock: &Mock) -> NativeArtifactStore {
    NativeArtifactStore::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "native-artifact-test",
            Duration::from_secs(1),
        )
        .unwrap(),
        "native-evidence".into(),
    )
    .unwrap()
}
fn bucket() -> Step {
    Step::get(
        "/storage/v1/bucket/native-evidence".into(),
        200,
        br#"{"id":"native-evidence","public":false}"#.to_vec(),
    )
}
fn get(reference: &NativeArtifactRef, status: u16, bytes: &[u8]) -> Step {
    Step::get(
        format!(
            "/storage/v1/object/authenticated/native-evidence/{}",
            reference.as_str()
        ),
        status,
        bytes.to_vec(),
    )
}
#[test]
fn closed_scalar_refs_preserve_originals_and_reject_path_aliases() {
    let r = reference();
    let text = r.as_str();
    assert!(NativeArtifactRef::try_from(text.to_owned()).unwrap() == r);
    assert!(serde_json::from_value::<NativeArtifactRef>(json!(text)).unwrap() == r);
    assert!(serde_json::from_value::<NativeArtifactRef>(json!({"key":text})).is_err());
    let (g, c) = ids();
    assert_eq!(r.game_id(), g);
    assert_eq!(r.context_id(), c);
    assert_eq!(r.size(), 22);
    assert!(r.role() == NativeArtifactRole::Input);
    for changed in [
        text.replace("native-artifact-v1", "native-artifact-v2"),
        text.replace(&g.to_string(), &Uuid::nil().to_string()),
        text.replace(&g.to_string(), &g.to_string().to_uppercase()),
        text.replace("/input/", "/unknown/"),
        text.replace("/22/", "/022/"),
        text.replace("/22/", "/0/"),
        text.replace("/22/", "/33554433/"),
        text.replace("/22/", "/18446744073709551616/"),
        text.replace("/22/", "/+22/"),
        text.replace(".json", ".json?token=private"),
        text.replace(".json", ".json/.."),
        format!("https://example.invalid/{text}"),
        text.replace("/input/", "/%69nput/"),
        text.replace("/input/", "/../"),
        text.replace('/', "\\"),
        format!("{text}#secret"),
        text.replace(".json", ".JSON"),
    ] {
        // These test UUIDs contain alphabetic hex so uppercase is a real mutation.
        if changed != text {
            assert!(NativeArtifactRef::try_from(changed).is_err());
        }
    }
    assert!(NativeArtifactRef::for_bytes(g, c, NativeArtifactRole::Input, b"").is_err());
    assert!(NativeArtifactRef::for_bytes(Uuid::nil(), c, NativeArtifactRole::Input, b"x").is_err());
    let huge = vec![0; usize::try_from(MAX_NATIVE_ARTIFACT_BYTES + 1).unwrap()];
    assert!(NativeArtifactRef::for_bytes(g, c, NativeArtifactRole::Initial, &huge).is_err());
    let max = NativeArtifactRef::try_from(text.replace("/22/", "/33554432/")).unwrap();
    assert_eq!(max.size(), MAX_NATIVE_ARTIFACT_BYTES);
}
#[test]
fn scope_rejections_happen_before_any_private_io() {
    let mock = Mock::new(vec![]);
    let store = client(&mock);
    let r = reference();
    let (g, c) = ids();
    for (game, context, role) in [
        (Uuid::from_u128(99), c, NativeArtifactRole::Input),
        (g, Uuid::from_u128(99), NativeArtifactRole::Input),
        (g, c, NativeArtifactRole::View),
    ] {
        let err = store.read(&r, game, context, role).err().unwrap();
        assert_eq!(err.failure(), NativeArtifactFailure::WrongScope);
        assert!(err.source().is_none());
    }
    assert_eq!(mock.finish().len(), 0);
}
#[test]
fn exact_read_and_missing_corrupt_encoded_and_metadata_controls() {
    let r = reference();
    let (g, c) = ids();
    let original = b"private-native-fixture";
    let mock = Mock::new(vec![bucket(), get(&r, 200, original)]);
    let store = client(&mock);
    let artifact = store.read(&r, g, c, NativeArtifactRole::Input).unwrap();
    assert!(artifact.reference() == &r);
    assert_eq!(artifact.bytes(), original);
    assert_eq!(artifact.into_bytes(), original);
    assert_eq!(mock.finish().len(), 2);
    for (status, body, encoded) in [
        (404, b"not found".as_slice(), false),
        (200, b"private-native-fixturX".as_slice(), false),
        (200, b"short".as_slice(), false),
        (200, b"private-native-fixture-extra".as_slice(), false),
        (200, original.as_slice(), true),
    ] {
        let mut response = get(&r, status, body);
        if encoded {
            response
                .response_headers
                .push(("Content-Encoding".into(), "gzip".into()));
        }
        let mock = Mock::new(vec![bucket(), response]);
        let store = client(&mock);
        let err = store
            .read(&r, g, c, NativeArtifactRole::Input)
            .err()
            .unwrap();
        assert!(!format!("{err:?}").contains(r.as_str()));
        assert!(!err.to_string().contains("private-native-fixture"));
        if status == 404 {
            assert_eq!(err.failure(), NativeArtifactFailure::Missing);
        } else {
            assert_eq!(err.failure(), NativeArtifactFailure::Storage);
            assert!(err.source().is_some());
        }
        assert_eq!(mock.finish().len(), 2);
    }
    for body in [
        br#"{"id":"native-evidence","public":true}"#.as_slice(),
        br#"{"id":"native-evidence","public":false,"public":false}"#.as_slice(),
        br#"["native-evidence",false]"#.as_slice(),
    ] {
        let mock = Mock::new(vec![Step::get(
            "/storage/v1/bucket/native-evidence".into(),
            200,
            body.to_vec(),
        )]);
        let store = client(&mock);
        assert!(store.read(&r, g, c, NativeArtifactRole::Input).is_err());
        assert_eq!(mock.finish().len(), 1);
    }
}
#[test]
fn retain_requires_matching_readback_and_preserves_unresolved_upload_cause() {
    let r = reference();
    let (g, c) = ids();
    let original = b"private-native-fixture";
    for (status, drop_ack, read_status, expected) in [
        (200, false, 200, true),
        (409, false, 200, true),
        (200, true, 200, true),
        (503, false, 404, false),
        (200, true, 404, false),
        (200, false, 404, false),
    ] {
        let mut upload = Step::json(
            &format!("/storage/v1/object/native-evidence/{}", r.as_str()),
            json!({}),
        );
        upload.status = status;
        upload.drop_ack = drop_ack;
        upload.expect = Some(original.to_vec());
        let mock = Mock::new(vec![
            bucket(),
            get(&r, 404, b"missing"),
            upload,
            get(&r, read_status, original),
        ]);
        let store = client(&mock);
        let result = store.retain(g, c, NativeArtifactRole::Input, original);
        if expected {
            assert!(result.unwrap() == r);
        } else {
            let error = result.err().unwrap();
            assert_eq!(error.failure(), NativeArtifactFailure::Storage);
            assert!(error.source().is_some());
            if status == 503 || drop_ack {
                assert!(error.source().unwrap().source().is_some());
            }
        }
        let seen = mock.finish();
        assert_eq!(seen.len(), 4);
        assert_eq!(seen[2].1.get("x-upsert").map(String::as_str), Some("false"));
    }
    let mock = Mock::new(vec![bucket(), get(&r, 200, b"private-native-fixturX")]);
    let store = client(&mock);
    assert!(
        store
            .retain(g, c, NativeArtifactRole::Input, original)
            .is_err()
    );
    assert_eq!(mock.finish().len(), 2);
    let mock = Mock::new(vec![bucket(), get(&r, 200, original)]);
    let store = client(&mock);
    assert!(
        store
            .retain(g, c, NativeArtifactRole::Input, original)
            .unwrap()
            == r
    );
    assert_eq!(mock.finish().len(), 2);
}

#[test]
#[ignore = "requires owned disposable Storage gateway and genuine native vocabulary; never hosted activation"]
fn owned_native_input_retention_and_reconstruction() {
    use crate::native_game::tests::fixture::{binding, command, scope, state, viewer_a};
    use crate::{native_game::restore_input, player_wire::PlayerAction};
    let url = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let origin = reqwest::Url::parse(&url).unwrap();
    assert_eq!(origin.scheme(), "http");
    assert_eq!(origin.host_str(), Some("127.0.0.1"));
    assert_eq!(
        std::env::var("SPELLWAKE_NATIVE_STORAGE_QUALIFY").unwrap(),
        "1"
    );
    let bucket = std::env::var("SPELLWAKE_NATIVE_ARTIFACT_BUCKET").unwrap();
    assert!(bucket.starts_with("native-owned-"));
    let store = NativeArtifactStore::from_env().unwrap();
    let source = state(vec![]);
    let view = scope(&source, viewer_a());
    let proposal = view
        .propose(&command(&view, PlayerAction::Pass {}))
        .unwrap();
    let bytes = view.encode_input(&proposal).unwrap();
    let g = source.basis().game.0;
    let c = view.basis().context_id;
    let reference = store
        .retain(Uuid::from_u128(g), c, NativeArtifactRole::Input, &bytes)
        .unwrap();
    let artifact = store
        .read(&reference, Uuid::from_u128(g), c, NativeArtifactRole::Input)
        .unwrap();
    assert_eq!(artifact.bytes(), bytes);
    let restored = restore_input(&source, binding(&source, viewer_a()), artifact.bytes()).unwrap();
    assert!(restored.original() == proposal.original());
    assert_eq!(restored.native_input(), proposal.native_input());
    assert_eq!(restored.candidate(), proposal.candidate());
    let config = BackendConfig::from_env().unwrap();
    let scope_game = Uuid::from_u128(g);
    let second_bucket = bucket.clone();
    let concurrent_bytes = bytes.clone();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let other_barrier = std::sync::Arc::clone(&barrier);
    let first = std::thread::spawn(move || {
        let other = NativeArtifactStore::new(
            BackendClient::new(
                config,
                "native-artifact-concurrency",
                Duration::from_secs(30),
            )
            .unwrap(),
            second_bucket,
        )
        .unwrap();
        other_barrier.wait();
        other
            .retain(scope_game, c, NativeArtifactRole::Event, &concurrent_bytes)
            .unwrap()
    });
    barrier.wait();
    let second = store
        .retain(scope_game, c, NativeArtifactRole::Event, &bytes)
        .unwrap();
    let concurrent = first.join().unwrap();
    assert!(second == concurrent);
    let ordinary = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    assert!(public.starts_with("sb_publishable_"));
    let plain = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .unwrap();
    // Positive Storage controls prove this actual JWT is accepted and the signed
    // URL route exists. The driver grants SELECT on this one fixture-only key;
    // native artifacts remain outside that policy.
    let control = "qualification-positive.json";
    for sign in [false, true] {
        let path = if sign {
            format!("storage/v1/object/sign/{bucket}/{control}")
        } else {
            format!("storage/v1/object/authenticated/{bucket}/{control}")
        };
        let endpoint = store.backend.url(&path).unwrap();
        let request = if sign {
            plain.post(endpoint).json(&json!({"expiresIn":60}))
        } else {
            plain.get(endpoint)
        };
        let response = request
            .header("apikey", &public)
            .bearer_auth(&ordinary)
            .send()
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body = crate::http_transport::bounded_body(response, 1_048_576).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        if sign {
            assert!(
                value
                    .get("signedURL")
                    .or_else(|| value.get("signedUrl"))
                    .is_some_and(serde_json::Value::is_string)
            );
        } else {
            assert_eq!(value, json!({"qualification":"ordinary-read-control"}));
        }
    }
    for (path, bearer, sign) in [
        (
            format!(
                "storage/v1/object/authenticated/{bucket}/{}",
                reference.as_str()
            ),
            None,
            false,
        ),
        (
            format!("storage/v1/object/public/{bucket}/{}", reference.as_str()),
            None,
            false,
        ),
        (
            format!(
                "storage/v1/object/authenticated/{bucket}/{}",
                reference.as_str()
            ),
            Some(ordinary.as_str()),
            false,
        ),
        (
            format!("storage/v1/object/sign/{bucket}/{}", reference.as_str()),
            Some(ordinary.as_str()),
            true,
        ),
    ] {
        let endpoint = store.backend.url(&path).unwrap();
        let mut request = if sign {
            plain.post(endpoint).json(&json!({"expiresIn":60}))
        } else {
            plain.get(endpoint)
        };
        request = request.header("apikey", &public);
        if let Some(bearer) = bearer {
            request = request.bearer_auth(bearer);
        }
        let response = request.send().unwrap();
        assert!([400, 401, 403, 404].contains(&response.status().as_u16()));
        let body = crate::http_transport::bounded_body(response, 1_048_576).unwrap();
        assert_ne!(body, bytes);
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) {
            assert!(value.get("signedURL").is_none() && value.get("signedUrl").is_none());
        }
    }
    assert_eq!(
        store
            .read(&reference, scope_game, c, NativeArtifactRole::Input)
            .unwrap()
            .bytes(),
        bytes
    );
    // Keys are fixture-private, not public responses. The driver uses its own
    // unique-bucket metadata for exact cleanup even if this assertion path fails.
    println!(
        "OWNED_NATIVE_ARTIFACT_OK {{\"originalBytes\":{},\"nativeRoundTrip\":true,\"concurrentSameRef\":true,\"privacyDenials\":4,\"ordinaryStorageControls\":2}}",
        bytes.len()
    );
}

#[test]
fn shared_file_retainer_preserves_unresolved_transport_status_cause() {
    struct Stage(std::path::PathBuf);
    impl Drop for Stage {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let stage =
        Stage(std::env::temp_dir().join(format!("spellwake-native-file-{}", Uuid::new_v4())));
    std::fs::create_dir(&stage.0).unwrap();
    let bytes = b"private-native-fixture";
    let path = stage.0.join("fixture.json");
    std::fs::write(&path, bytes).unwrap();
    let hash = crate::price_source::hash_text(&Sha256::digest(bytes));
    let r = reference();
    for (status, read_status, success) in [(503, 404, false), (302, 404, false), (409, 200, true)] {
        let mut upload = Step::json(
            &format!("/storage/v1/object/native-evidence/{}", r.as_str()),
            json!({}),
        );
        upload.status = status;
        upload.expect = Some(bytes.to_vec());
        let mock = Mock::new(vec![
            get(&r, 404, b"missing"),
            upload,
            get(&r, read_status, bytes),
        ]);
        let store = client(&mock);
        let result = private_storage::retain_file(
            &store.backend,
            &store.bucket,
            &stage.0,
            &private_storage::RetainedFile {
                key: r.as_str(),
                path: &path,
                hash: &hash,
                size: u64::try_from(bytes.len()).unwrap(),
                content_type: "application/json",
            },
        );
        if success {
            assert!(result.is_ok());
        } else {
            let error = result.err().unwrap();
            if status == 503 {
                assert!(error.source().is_some());
            } else {
                assert!(matches!(error,StorageError::Status(value) if value.as_u16()==302));
            }
            assert!(!error.to_string().contains(r.as_str()));
        }
        assert_eq!(mock.finish().len(), 3);
    }
}

#[test]
fn observed_length_and_truncated_raw_transfers_fail_closed() {
    let r = reference();
    let (g, c) = ids();
    let original = b"private-native-fixture";
    let digest: [u8; 32] = Sha256::digest(original).into();
    assert_eq!(r.sha256(), &digest);
    let cases=[
        (format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}",String::from_utf8_lossy(original)),true),
        ("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nshort".into(),false),
        ("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nprivate-native-fixture-over-bound".into(),false),
        ("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n18\r\nprivate-native-fixture-X\r\n0\r\n\r\n".into(),false),
        ("HTTP/1.1 200 OK\r\nContent-Length: 22\r\nConnection: close\r\n\r\nshort".into(),false),
    ];
    for (raw, valid) in cases {
        let mut step = get(&r, 200, b"");
        step.raw_response = Some(raw.into_bytes());
        let mock = Mock::new(vec![bucket(), step]);
        let store = client(&mock);
        let result = store.read(&r, g, c, NativeArtifactRole::Input);
        if valid {
            assert_eq!(result.unwrap().bytes(), original);
        } else {
            let error = result.err().unwrap();
            assert_eq!(error.failure(), NativeArtifactFailure::Storage);
            assert!(error.source().is_some());
        }
        assert_eq!(mock.finish().len(), 2);
    }
}
