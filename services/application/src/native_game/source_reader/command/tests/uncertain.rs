//! Fault-injected transport replies qualify fallback ordering, not SQL or Auth.
use super::*;
use crate::{
    actor::{ActorClient, ActorConfig},
    test_http::{Credentials, Mock, Step},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn actor() -> RequestActor {
    let issuer = "https://native-auth.example.test/auth/v1";
    let claims = json!({"sub":id(200),"session_id":id(201),"iss":issuer,
        "aud":"authenticated","role":"authenticated",
        "exp":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()+3600,
        "spellwake_auth":{"version":1,"method":"email_code","bindingId":id(202),"sessionGrantId":id(203)}});
    let token = format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap()),
        URL_SAFE_NO_PAD.encode(b"synthetic-not-auth-proof")
    );
    let mut step = Step::json(
        "/rest/v1/rpc/read_current_session",
        json!({"status":"success","data":{
        "subjectId":id(200),"sessionId":id(201),"method":"email_code","state":"active","accountId":id(204)}}),
    );
    step.credentials = Credentials::Caller("sb_publishable_test_only".to_owned(), token.clone());
    let mock = Mock::new(vec![step]);
    let actor =
        ActorClient::new(ActorConfig::new(&mock.base, "sb_publishable_test_only", issuer).unwrap())
            .unwrap()
            .resolve(&token)
            .unwrap();
    assert_eq!(mock.finish().len(), 1);
    actor
}

#[test]
fn lost_submission_ack_reads_one_original_and_never_resubmits() {
    let actor = actor();
    let original = original(10);
    let accepted = json!({"status":"success","data":ack(10)});
    let rejected = json!({"status":"fail","data":{"basis":"stale"}});
    for (state, saved, recoverable) in [
        ("accepted", accepted.clone(), true),
        ("rejected", rejected.clone(), true),
        ("unknown", Value::Null, false),
        (
            "accepted",
            json!({"status":"success","data":{"private":"invalid"}}),
            false,
        ),
    ] {
        let mut dropped = Step::json("/rest/v1/rpc/submit_native_human_command", accepted.clone());
        dropped.drop_ack = true;
        dropped.expect = Some(
            serde_json::to_vec(
                &json!({"p_subject":actor.subject(),"p_session":actor.session(),
            "p_original":original,"p_prepared":null}),
            )
            .unwrap(),
        );
        let read = Step::json(
            "/rest/v1/rpc/read_native_human_receipts",
            json!({"status":"success","data":{"receipts":[{
            "actorSeatId":id(100),"original":original,"state":state,"response":saved}]}}),
        );
        let mock = Mock::new(vec![dropped, read]);
        let backend = BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "uncertain-native-command-test",
            Duration::from_secs(1),
        )
        .unwrap();
        let result = NativeSourceRepository::new(backend).submit_human(
            &actor,
            id(100),
            &original,
            &Value::Null,
        );
        if recoverable {
            match (state, result.unwrap()) {
                ("accepted", ApiResponse::Success { data }) => {
                    data.validate(&original, id(100)).unwrap()
                }
                ("rejected", ApiResponse::Fail { data }) => {
                    assert_eq!(data.get("basis").map(String::as_str), Some("stale"))
                }
                _ => panic!("saved original must survive the missing acknowledgment"),
            }
        } else {
            assert!(result.is_err());
        }
        let captured = mock.finish();
        assert_eq!(captured.len(), 2);
        assert_eq!(
            captured
                .iter()
                .filter(|(path, _, _)| path.ends_with("submit_native_human_command"))
                .count(),
            1
        );
    }
}
