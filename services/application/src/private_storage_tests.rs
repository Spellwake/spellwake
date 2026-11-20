//! Controlled Storage responses, not managed Storage qualification.
use super::*;
use crate::{
    backend::BackendConfig,
    test_http::{Mock, Step},
};
use serde_json::json;
use std::time::Duration;

fn backend(mock: &Mock) -> BackendClient {
    BackendClient::new(
        BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
        "storage-shape-test",
        Duration::from_secs(1),
    )
    .unwrap()
}

#[test]
fn source_objects_bucket_authority_rejects_arrays_and_duplicate_consumed_keys() {
    for (raw, valid) in [
        (
            r#"{"id":"evidence","public":false,"extension":{"x":1}}"#,
            true,
        ),
        (r#"["evidence",false]"#, false),
        (r#"{"id":"evidence","public":true}"#, false),
        (r#"{"id":"other","id":"evidence","public":false}"#, false),
        (r#"{"id":"evidence","public":true,"public":false}"#, false),
        (r#"{"id":"evidence","public":false,"public":false}"#, false),
        (r#"{"id":"evidence","public":null}"#, false),
        (r#"{"id":"evidence"}"#, false),
    ] {
        let mock = Mock::new(vec![Step::get(
            "/storage/v1/bucket/evidence".into(),
            200,
            raw.as_bytes().to_vec(),
        )]);
        let client = backend(&mock);
        if valid {
            assert!(private_bucket(&client, "evidence").is_ok());
        } else {
            // A malformed privacy assertion must stop before DELETE/object reads.
            assert!(delete_private_object(&client, "evidence", "payments/test.json").is_err());
        }
        assert_eq!(mock.finish().len(), 1);
    }
}

#[test]
fn source_objects_absence_proof_rejects_ambiguous_codes_and_keeps_http_status_rules() {
    let key = format!("payments/stripe/{}.json", "a".repeat(64));
    for (status, raw, absent) in [
        (404, r#"unparsed HTTP 404 body"#, true),
        (
            400,
            r#"{"code":"NoSuchKey","statusCode":"404","message":"extra"}"#,
            true,
        ),
        (400, r#"["NoSuchKey","404"]"#, false),
        (
            400,
            r#"{"code":"AccessDenied","code":"NoSuchKey","statusCode":"404"}"#,
            false,
        ),
        (
            400,
            r#"{"code":"NoSuchKey","statusCode":"403","statusCode":"404"}"#,
            false,
        ),
        (400, r#"{"code":"NoSuchKey","statusCode":404}"#, false),
        (400, r#"{"code":"NoSuchKey"}"#, false),
        (400, r#"{"code":"AccessDenied","statusCode":"404"}"#, false),
        (200, r#"still present"#, false),
    ] {
        let mut deleted = Step::json("/storage/v1/object/evidence", json!([]));
        deleted.method = "DELETE";
        deleted.expect = Some(serde_json::to_vec(&json!({"prefixes":[key]})).unwrap());
        let mock = Mock::new(vec![
            Step::get(
                "/storage/v1/bucket/evidence".into(),
                200,
                br#"{"id":"evidence","public":false}"#.to_vec(),
            ),
            deleted,
            Step::get(
                format!("/storage/v1/object/authenticated/evidence/{key}"),
                status,
                raw.as_bytes().to_vec(),
            ),
        ]);
        assert_eq!(
            delete_private_object(&backend(&mock), "evidence", &key).is_ok(),
            absent,
            "{status}: {raw}"
        );
        assert_eq!(mock.finish().len(), 3);
    }
}
