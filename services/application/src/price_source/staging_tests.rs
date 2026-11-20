//! Causal staging diagnostics and safe formatting at the shared source boundary.
use super::*;
use std::error::Error;

struct Stage(PathBuf);
impl Stage {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("spellwake-price-source-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove owned source test directory");
    }
}

#[test]
fn actual_file_open_failure_preserves_io_kind_and_error_identity_on_clone() {
    let stage = Stage::new();
    let error = hash_file(&stage.0.join("absent-artifact")).unwrap_err();
    assert!(matches!(error, SourceError::Io(_)));
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        io::ErrorKind::NotFound
    );
    let cloned = error.clone();
    assert_eq!(cloned, error);
    assert!(std::ptr::eq(
        error.source().unwrap(),
        cloned.source().unwrap()
    ));
    assert_eq!(
        error,
        SourceError::from(io::Error::other("distinct staging cause"))
    );
    assert_eq!(error.to_string(), "price source staging failed");
    fn send_sync<T: Send + Sync>() {}
    send_sync::<SourceError>();
}

#[test]
fn private_io_details_remain_local_not_display_or_debug() {
    let marker = "private-path-sentinel sb_secret_fixture_only";
    let error = SourceError::from(io::Error::new(io::ErrorKind::PermissionDenied, marker));
    let cause = error.source().unwrap().downcast_ref::<io::Error>().unwrap();
    assert_eq!(cause.kind(), io::ErrorKind::PermissionDenied);
    assert!(cause.to_string().contains(marker));
    assert!(!format!("{error:?} {error}").contains(marker));
    assert!(SourceError::InvalidArtifact.source().is_none());
}

struct BrokenSerialization;
impl Serialize for BrokenSerialization {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("private-serialization-sentinel"))
    }
}

#[test]
fn actual_json_staging_failure_preserves_codec_cause_and_cleans_owned_output() {
    let stage = Stage::new();
    let path = stage.0.join("failed.json");
    let error = persist_json(&path, &BrokenSerialization).unwrap_err();
    let cause = error
        .source()
        .unwrap()
        .downcast_ref::<serde_json::Error>()
        .unwrap();
    assert!(cause.to_string().contains("private-serialization-sentinel"));
    assert!(!format!("{error:?} {error}").contains("private-serialization-sentinel"));
    assert!(path.is_file());
}

#[test]
fn actual_staging_create_failure_preserves_filesystem_cause() {
    let stage = Stage::new();
    let error = persist_json(&stage.0.join("absent-parent/out.json"), &true).unwrap_err();
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn original_index_decoder_cause_stays_private_with_unchanged_classification() {
    let marker = "private-index-value-sentinel";
    let bytes = format!(r#"{{"object":"list","has_more":"{marker}","data":[]}}"#);
    let error = parse_index(bytes.as_bytes()).unwrap_err();
    let cause = error
        .source()
        .unwrap()
        .downcast_ref::<serde_json::Error>()
        .unwrap();
    assert!(cause.is_data());
    assert!(cause.to_string().contains(marker));
    assert_eq!(error, SourceError::InvalidIndex);
    let cloned = error.clone();
    assert_eq!(cloned, error);
    assert!(std::ptr::eq(
        error.source().unwrap(),
        cloned.source().unwrap()
    ));
    assert!(!format!("{error:?} {error}").contains(marker));
    assert!(SourceError::InvalidIndex.source().is_none());
}

#[test]
fn real_corrupt_gzip_keeps_original_reader_error_and_classification() {
    let stage = Stage::new();
    let path = stage.0.join("invalid.gz");
    fs::write(&path, b"not a gzip artifact").unwrap();
    let error = scan_artifact(&path, &HashSet::new()).unwrap_err();
    assert!(error.source().unwrap().is::<io::Error>());
    assert_eq!(error, SourceError::InvalidArtifact);
    let cloned = error.clone();
    assert_eq!(cloned, error);
    assert!(std::ptr::eq(
        error.source().unwrap(),
        cloned.source().unwrap()
    ));
    assert_eq!(fs::read(path).unwrap(), b"not a gzip artifact");
}

#[test]
fn original_uuid_and_consumed_field_decoder_types_remain_available() {
    let uuid_error =
        card_identity(&serde_json::json!({"object":"card","id":"private-invalid-id-sentinel"}))
            .unwrap_err();
    assert!(uuid_error.source().unwrap().is::<uuid::Error>());
    assert_eq!(uuid_error, SourceError::InvalidArtifact);
    assert!(!format!("{uuid_error:?} {uuid_error}").contains("private-invalid-id-sentinel"));
    for bytes in [
        br#"{"prices":{"usd":"1.00","usd":"2.00"}}"#.as_slice(),
        br#"{"card_faces":[{"name":"one","name":"two"}]}"#.as_slice(),
        br#"{"card_faces":["unterminated] }"#.as_slice(),
    ] {
        let error = consumed::validate(bytes).unwrap_err();
        assert!(error.source().unwrap().is::<serde_json::Error>());
        assert_eq!(error, SourceError::InvalidArtifact);
    }
    // A semantic predicate rejection has no invented decoder error.
    assert!(
        card_identity(&serde_json::json!({"object":"not-a-card"}))
            .unwrap_err()
            .source()
            .is_none()
    );
}

#[test]
fn loopback_lost_response_keeps_original_transport_error_without_url() {
    use crate::test_http::{Mock, Step};
    let marker = "private-source-request-key-sentinel";
    let mut step = Step::get(format!("/{marker}"), 200, b"ignored".to_vec());
    step.drop_ack = true;
    let mock = Mock::new(vec![step]);
    // This exercises the maintained transport conversion with a real loopback
    // failure. It is not a request to the fixed production vendor origin.
    let error = Client::builder()
        .retry(reqwest::retry::never())
        .build()
        .unwrap()
        .get(format!("{}/{marker}", mock.base.trim_end_matches('/')))
        .header("apikey", "sb_secret_test_only")
        .send()
        .unwrap_err();
    assert!(error.url().is_some());
    let error = SourceError::network(error);
    let cause = error
        .source()
        .unwrap()
        .downcast_ref::<reqwest::Error>()
        .unwrap();
    assert!(cause.url().is_none());
    assert_eq!(error, SourceError::Network);
    let cloned = error.clone();
    assert_eq!(cloned, error);
    assert!(std::ptr::eq(
        error.source().unwrap(),
        cloned.source().unwrap()
    ));
    assert!(!format!("{error:?} {error} {cause:?}").contains(marker));
    assert_eq!(mock.finish().len(), 1);
}

#[test]
fn response_policy_keeps_non_success_status_without_body_and_preserves_rate_limit() {
    use crate::test_http::{Mock, Step};
    for status in [302, 503, 429] {
        let marker = "private-source-response-body-sentinel";
        let mock = Mock::new(vec![Step::get(
            "/status".into(),
            status,
            marker.as_bytes().to_vec(),
        )]);
        let response = Client::builder()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .unwrap()
            .get(format!("{}/status", mock.base.trim_end_matches('/')))
            .header("apikey", "sb_secret_test_only")
            .send()
            .unwrap();
        let error = checked_response(response).unwrap_err();
        if status == 429 {
            assert_eq!(error, SourceError::RateLimited);
            assert!(error.source().is_none());
        } else {
            assert_eq!(error, SourceError::Network);
            assert_eq!(
                error
                    .source()
                    .unwrap()
                    .downcast_ref::<SourceStatusFailure>()
                    .unwrap()
                    .0
                    .as_u16(),
                status
            );
        }
        assert!(!format!("{error:?} {error}").contains(marker));
        assert_eq!(mock.finish().len(), 1);
    }
}
