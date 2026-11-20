use super::*;
use std::error::Error as _;

#[test]
fn bootstrap_causes_are_original_and_root_formatting_is_private() {
    let marker = "private-bootstrap sb_secret_fixture_only https://private.invalid/?token=fixture";
    let error = TemporalRuntimeError::caused(std::io::Error::other(marker));
    let cause = error
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .unwrap();
    assert_eq!(cause.kind(), std::io::ErrorKind::Other);
    assert_eq!(cause.to_string(), marker);
    assert_eq!(
        error.to_string(),
        "workflow connection could not be established"
    );
    assert!(!format!("{error:?} {error}").contains(marker));
    let policy = TemporalConfig::new("https://example.com", "invalid namespace", None)
        .err()
        .expect("policy must reject");
    assert!(policy.source().is_none());
    assert!(!format!("{policy:?} {policy}").contains("invalid namespace"));
}

#[test]
fn actual_url_parse_failure_retains_its_concrete_error() {
    fn same_kind<E: std::error::Error + 'static>(
        cause: &(dyn std::error::Error + 'static),
        expected: E,
    ) {
        let original = cause
            .downcast_ref::<E>()
            .expect("original parser error type");
        assert_eq!(original.to_string(), expected.to_string());
    }
    let address = "://private-bootstrap.invalid/?token=fixture";
    let error = TemporalConfig::new(address, "test", None).err().unwrap();
    same_kind(error.source().unwrap(), Url::parse(address).unwrap_err());
    assert!(!format!("{error:?} {error}").contains(address));
}

#[tokio::test]
async fn actual_local_sdk_connection_failure_retains_cause_without_root_address() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let config = TemporalConfig::new(&address, "spellwake-diagnostic-test", None).unwrap();
    // Only a closed ephemeral loopback port is contacted; no workflow is installed.
    let result = tokio::time::timeout(Duration::from_secs(20), config.connect("diagnostic-test"))
        .await
        .expect("existing connection timeout must remain bounded");
    let error = result.expect_err("closed loopback port must reject");
    assert!(error.source().is_some());
    assert!(!format!("{error:?} {error}").contains(&address));
    assert_eq!(
        error.to_string(),
        "workflow connection could not be established"
    );
}
