//! Synthetic HTTP fixtures test the adapter contract, not JWT signature verification.
use super::*;
use crate::{
    backend::BackendConfig,
    test_http::{Mock, Step},
};
use std::{io::ErrorKind, net::TcpListener};

const ISSUER: &str = "https://auth.example.test/auth/v1";
const PUBLIC_KEY: &str = "sb_publishable_test_only";
const PATH: &str = "/rest/v1/rpc/read_current_session";

#[path = "actor_tests/object_shapes.rs"]
mod object_shapes;
fn claims() -> Value {
    json!({"sub":Uuid::new_v4(),"session_id":Uuid::new_v4(),"iss":ISSUER,"aud":"authenticated",
        "role":"authenticated","exp":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()+3600,
        "spellwake_auth":{"version":1,"method":"email_code","bindingId":Uuid::new_v4(),"sessionGrantId":Uuid::new_v4()}})
}
fn token(claims: &Value) -> String {
    format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap()),
        URL_SAFE_NO_PAD.encode(b"synthetic-not-signed")
    )
}
fn success(claims: &Value, state: &str, account: Value) -> Value {
    json!({"status":"success","data":{"subjectId":claims["sub"],"sessionId":claims["session_id"],
        "method":claims["spellwake_auth"]["method"],"state":state,"accountId":account}})
}
fn step(bearer: &str, response: Value) -> Step {
    let mut step = Step::json(PATH, response);
    step.credentials = crate::test_http::Credentials::Caller(PUBLIC_KEY.into(), bearer.into());
    step.expect = Some(b"{}".to_vec());
    step
}
fn client(base: &str) -> ActorClient {
    ActorClient::new(ActorConfig::new(base, PUBLIC_KEY, ISSUER).unwrap()).unwrap()
}

#[test]
fn public_and_worker_configuration_are_distinct_and_bounded() {
    assert!(ActorConfig::new("https://example.test/", "sb_secret_test_only", ISSUER).is_err());
    assert!(ActorConfig::new("https://example.test/", "", ISSUER).is_err());
    assert!(BackendConfig::new("https://example.test/", PUBLIC_KEY).is_err());
    for base in [
        "http://example.test/",
        "https://u:p@example.test/",
        "https://example.test/path",
        "https://example.test/?x=1",
    ] {
        assert!(ActorConfig::new(base, PUBLIC_KEY, ISSUER).is_err());
    }
    for issuer in [
        "bad",
        "http://example.test/auth/v1",
        "https://u:p@example.test/auth/v1",
        "https://example.test/auth/v1?key=x",
    ] {
        assert!(ActorConfig::new("https://example.test/", PUBLIC_KEY, issuer).is_err());
    }
    for timeout in [Duration::ZERO, Duration::from_secs(61)] {
        assert!(
            ActorConfig::with_timeout("https://example.test/", PUBLIC_KEY, ISSUER, timeout)
                .is_err()
        );
    }
}

#[test]
fn invalid_local_tokens_make_no_network_requests() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = client(&format!("http://{}/", listener.local_addr().unwrap()));
    let original = claims();
    let mut variants = vec![
        String::new(),
        "a.b.c".into(),
        "a.b.c.d".into(),
        " ".repeat(MAX_BEARER + 1),
    ];
    for (key, value) in [
        ("iss", json!("other")),
        ("role", json!("service_role")),
        ("aud", json!(["authenticated", "other"])),
        ("exp", json!(0)),
        ("exp", json!(1.5)),
        ("exp", json!("future")),
        ("exp", json!(1e30)),
        ("sub", json!("bad")),
        ("session_id", json!(Uuid::nil())),
        ("spellwake_auth", json!({})),
    ] {
        let mut invalid = original.clone();
        invalid[key] = value;
        variants.push(token(&invalid));
    }
    for (key, value) in [
        ("version", json!(2)),
        ("method", json!("password")),
        ("bindingId", json!("bad")),
        ("sessionGrantId", json!(Uuid::nil())),
    ] {
        let mut invalid = original.clone();
        invalid["spellwake_auth"][key] = value;
        variants.push(token(&invalid));
    }
    let mut metadata = original.clone();
    let root = metadata
        .as_object_mut()
        .unwrap()
        .remove("spellwake_auth")
        .unwrap();
    metadata["user_metadata"] = json!({"spellwake_auth":root});
    variants.push(token(&metadata));
    for invalid in variants {
        assert!(matches!(
            client.resolve(&invalid),
            Err(ActorError::Unauthorized)
        ));
    }
    assert!(matches!(listener.accept(),Err(e) if e.kind()==ErrorKind::WouldBlock));
}

#[test]
fn canonical_states_and_consecutive_callers_preserve_request_local_bearers() {
    let mut first = claims();
    first["aud"] = json!(["authenticated"]);
    let second = claims();
    let first_token = token(&first);
    let second_token = token(&second);
    let account = Uuid::new_v4();
    let mock = Mock::new(vec![
        step(
            &first_token,
            success(&first, "needs_registration", Value::Null),
        ),
        step(&second_token, success(&second, "active", json!(account))),
        step(
            &second_token,
            success(&second, "deletion_pending", json!(account)),
        ),
    ]);
    let client = client(&mock.base);
    let actor = client.resolve(&first_token).unwrap();
    assert_eq!(
        actor.subject(),
        Uuid::parse_str(first["sub"].as_str().unwrap()).unwrap()
    );
    assert_eq!(
        actor.session(),
        Uuid::parse_str(first["session_id"].as_str().unwrap()).unwrap()
    );
    assert_eq!(actor.method(), SignInMethod::EmailCode);
    assert_eq!(actor.state(), ActorState::NeedsRegistration);
    assert_eq!(actor.account(), None);
    let actor = client.resolve(&second_token).unwrap();
    assert_eq!(actor.account(), Some(account));
    assert_eq!(actor.state(), ActorState::Active);
    assert_eq!(
        client.resolve(&second_token).unwrap().state(),
        ActorState::DeletionPending
    );
    for (_, headers, _) in mock.finish() {
        assert_eq!(
            headers.get("accept-profile").map(String::as_str),
            Some("app")
        );
    }
}

#[test]
fn canonical_envelope_and_identity_must_match_exactly() {
    let claims = claims();
    let bearer = token(&claims);
    let valid = success(&claims, "active", json!(Uuid::new_v4()));
    let mut variants = vec![
        json!({"status":"fail","data":{}}),
        json!({"status":"fail","data":{"session":"other"}}),
        json!({"status":"fail","data":{"session":"unauthorized","extra":"reason"}}),
        json!({"status":"error","message":"vendor internal","code":"internal"}),
    ];
    for (key, value) in [
        ("subjectId", json!(Uuid::new_v4())),
        ("sessionId", json!(Uuid::new_v4())),
        ("method", json!("google")),
        ("method", json!("unknown")),
        ("state", json!("unknown")),
        ("accountId", Value::Null),
        ("accountId", json!(Uuid::nil())),
        ("extra", json!(true)),
    ] {
        let mut invalid = valid.clone();
        invalid["data"][key] = value;
        variants.push(invalid);
    }
    for key in ["subjectId", "sessionId", "method", "state", "accountId"] {
        let mut invalid = valid.clone();
        invalid["data"].as_object_mut().unwrap().remove(key);
        variants.push(invalid);
    }
    variants.push(success(
        &claims,
        "needs_registration",
        json!(Uuid::new_v4()),
    ));
    variants.push(success(&claims, "deletion_pending", Value::Null));
    let mut invalid = success(&claims, "needs_registration", Value::Null);
    invalid["data"].as_object_mut().unwrap().remove("accountId");
    variants.push(invalid);
    let mut invalid = valid;
    invalid["extra"] = json!(true);
    variants.push(invalid);
    let count = variants.len();
    let mock = Mock::new(variants.into_iter().map(|v| step(&bearer, v)).collect());
    let client = client(&mock.base);
    // Every response has the same outgoing valid caller; backend drift is Unavailable.
    for _ in 0..count {
        assert!(matches!(
            client.resolve(&bearer),
            Err(error) if error == ActorError::Unavailable
        ));
    }
    mock.finish();
}

#[test]
fn remote_denial_failure_body_bounds_and_redirects_are_distinguished() {
    let claims = claims();
    let bearer = token(&claims);
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let mut redirect = step(&bearer, json!({}));
    redirect.status = 302;
    redirect.response_headers.push((
        "Location".into(),
        format!("http://{}/capture", target.local_addr().unwrap()),
    ));
    let mut steps = vec![step(
        &bearer,
        json!({"status":"fail","data":{"session":"unauthorized"}}),
    )];
    for status in [401, 403, 500] {
        let mut value = step(&bearer, json!({}));
        value.status = status;
        steps.push(value);
    }
    steps.push(redirect);
    for body in [b"not json".to_vec(), vec![b' '; 1024 * 1024 + 1]] {
        let mut value = step(&bearer, json!({}));
        value.body = body;
        steps.push(value);
    }
    let mock = Mock::new(steps);
    let client = client(&mock.base);
    for expected in [
        ActorError::Unauthorized,
        ActorError::Unauthorized,
        ActorError::Unavailable,
        ActorError::Unavailable,
        ActorError::Unavailable,
        ActorError::Unavailable,
        ActorError::Unavailable,
    ] {
        assert!(matches!(client.resolve(&bearer),Err(e) if e==expected));
    }
    mock.finish();
    assert!(matches!(target.accept(),Err(e) if e.kind()==ErrorKind::WouldBlock));
}

#[test]
fn stalled_response_obeys_actor_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let waiting = std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_millis(200));
    });
    let client = ActorClient::new(
        ActorConfig::with_timeout(&base, PUBLIC_KEY, ISSUER, Duration::from_millis(30)).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        client.resolve(&token(&claims())),
        Err(error) if error == ActorError::Unavailable
    ));
    waiting.join().unwrap();
}

/// Opt-in only: the Python driver owns the fresh gateway and all expected facts.
#[test]
#[ignore = "requires a new owned managed-local Supabase fixture"]
fn owned_managed_gateway_resolves_canonical_actor() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("SPELLWAKE_ACTOR_FIXTURE_URL")?;
    let url = reqwest::Url::parse(&base).map_err(|_| "invalid fixture URL")?;
    if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") || url.port().is_none() {
        return Err("fixture must use an explicit owned loopback port".into());
    }
    let key = std::env::var("SPELLWAKE_ACTOR_FIXTURE_PUBLIC_KEY")?;
    let issuer = std::env::var("SPELLWAKE_ACTOR_FIXTURE_ISSUER")?;
    let bearer = std::env::var("SPELLWAKE_ACTOR_FIXTURE_BEARER")?;
    let expected = std::env::var("SPELLWAKE_ACTOR_FIXTURE_EXPECTED")?;
    let client = ActorClient::new(ActorConfig::new(&base, &key, &issuer)?)?;
    let result = client.resolve(&bearer);
    if expected == "unauthorized" {
        assert!(
            matches!(result, Err(ActorError::Unauthorized)),
            "fixture denial differs"
        );
    } else {
        let expected_state = match expected.as_str() {
            "needs_registration" => ActorState::NeedsRegistration,
            "active" => ActorState::Active,
            "deletion_pending" => ActorState::DeletionPending,
            _ => return Err("invalid expected fixture state".into()),
        };
        let subject = Uuid::parse_str(&std::env::var("SPELLWAKE_ACTOR_FIXTURE_SUBJECT")?)
            .map_err(|_| "invalid fixture subject")?;
        let session = Uuid::parse_str(&std::env::var("SPELLWAKE_ACTOR_FIXTURE_SESSION")?)
            .map_err(|_| "invalid fixture session")?;
        let account = std::env::var("SPELLWAKE_ACTOR_FIXTURE_ACCOUNT")?;
        let account = if account == "none" {
            None
        } else {
            Some(Uuid::parse_str(&account).map_err(|_| "invalid fixture account")?)
        };
        let actor = result?;
        assert!(actor.subject() == subject, "canonical subject differs");
        assert!(actor.session() == session, "canonical session differs");
        assert!(actor.account() == account, "canonical account differs");
        assert!(
            actor.method() == SignInMethod::EmailCode,
            "canonical method differs"
        );
        assert!(actor.state() == expected_state, "canonical state differs");
    }
    println!("OWNED_MANAGED_ACTOR_CASE_OK");
    Ok(())
}

#[test]
fn actor_wraps_backend_json_cause_without_exposing_response_or_bearer() {
    use std::error::Error;
    let bearer = token(&claims());
    let mock = Mock::new(vec![step(
        &bearer,
        json!("private-actor-response-sentinel"),
    )]);
    let error = client(&mock.base).resolve(&bearer).err().unwrap();
    assert_eq!(error, ActorError::Unavailable);
    assert_eq!(error.clone(), error);
    let backend = error.source().unwrap();
    assert!(
        backend
            .downcast_ref::<crate::backend::BackendError>()
            .is_some()
    );
    let json_error = backend.source().unwrap();
    assert!(json_error.downcast_ref::<serde_json::Error>().is_some());
    for formatted in [
        format!("{error}"),
        format!("{error:?}"),
        format!("{error:#?}"),
    ] {
        assert!(!formatted.contains("private-actor-response-sentinel"));
        assert!(!formatted.contains(&bearer));
        assert!(!formatted.contains(&mock.base));
    }
    mock.finish();
}

#[test]
fn retained_causes_preserve_clone_and_failure_classification() {
    use std::error::Error;
    let cause = ActorError::caused(std::io::Error::other("private-cause-sentinel"));
    let cloned = cause.clone();
    assert!(std::ptr::eq(
        cause.source().unwrap(),
        cloned.source().unwrap()
    ));
    let errors = [
        ActorError::Configuration,
        ActorError::Unauthorized,
        ActorError::Unavailable,
        cause,
    ];
    for (i, left) in errors.iter().enumerate() {
        for (j, right) in errors.iter().enumerate() {
            assert_eq!(left == right, i.min(2) == j.min(2));
        }
    }
    let dependency = crate::backend::BackendError::caused(
        "json",
        std::io::Error::other("private-cause-sentinel"),
    );
    let clone = dependency.clone();
    assert!(std::ptr::eq(
        dependency.source().unwrap(),
        clone.source().unwrap()
    ));
    assert_eq!(dependency, crate::backend::BackendError::Request);
    assert_ne!(dependency, crate::backend::BackendError::Configuration);
    assert_ne!(
        crate::backend::BackendError::Request,
        crate::backend::BackendError::Configuration
    );
}

#[test]
fn account_tuple_decoder_preserves_null_and_requires_canonical_public_identifiers() {
    let canonical = "11111111-1111-1111-1111-111111111111";
    assert!(
        matches!(serde_json::from_value::<RequiredAccount>(json!(canonical)).unwrap(),RequiredAccount::Present(id) if id.to_string()==canonical)
    );
    assert!(matches!(
        serde_json::from_value::<RequiredAccount>(Value::Null).unwrap(),
        RequiredAccount::Absent(())
    ));
    for alias in [
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
        "urn:uuid:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "00000000-0000-0000-0000-000000000000",
    ] {
        assert!(serde_json::from_value::<RequiredAccount>(json!(alias)).is_err());
    }
}

#[test]
fn actor_refuses_expiry_crossed_during_authentication_response_io() {
    // Synthetic transport timing only; the live delivery suite must separately
    // use a genuine managed bearer and real Auth before claiming authentication.
    let mut claims = claims();
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 2;
    claims["exp"] = json!(expiry);
    let bearer = token(&claims);
    let response = success(&claims, "active", json!(Uuid::new_v4()));
    let body = serde_json::to_vec(&response).unwrap();
    let mut delayed = step(&bearer, response);
    delayed.reply = Some(Box::new(move |_| {
        while SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            < expiry
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        body.clone()
    }));
    let mock = Mock::new(vec![delayed]);
    assert!(matches!(
        client(&mock.base).resolve(&bearer),
        Err(ActorError::Unauthorized)
    ));
    assert_eq!(mock.finish().len(), 1);
}
#[test]
fn actor_private_deadline_and_identity_comparison_do_not_replace_current_auth() {
    let claims = claims();
    let bearer = token(&claims);
    let account = Uuid::new_v4();
    let response = success(&claims, "active", json!(account));
    let mock = Mock::new(vec![
        step(&bearer, response.clone()),
        step(&bearer, response),
    ]);
    let client = client(&mock.base);
    let mut original = client.resolve(&bearer).unwrap();
    let current = client.resolve(&bearer).unwrap();
    assert!(original.same_identity(&current));
    original.ensure_unexpired().unwrap();
    original.verified_bearer_expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(matches!(
        original.ensure_unexpired(),
        Err(ActorError::Unauthorized)
    ));
    original.session = Uuid::new_v4();
    assert!(!original.same_identity(&current));
    assert_eq!(mock.finish().len(), 2);
}
