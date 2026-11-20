//! Protocol tests use synthetic identities; managed Auth/SQL qualification is separate.
use super::*;
use crate::rooms::invitations::{InvitationKey, InvitationToken};
const DERIVATION_KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
fn keyed(base: &str) -> ApiConfig {
    config(base).with_invitation_key(InvitationKey::new(DERIVATION_KEY).unwrap())
}
#[tokio::test]
async fn issued_capability_is_delivered_only_after_acceptance_and_recovers_identically() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let room = Uuid::new_v4();
    let request = Uuid::new_v4();
    let invitation = Uuid::new_v4();
    let input = json!({"requestId":request,"expectedRevision":"9007199254740993"});
    let raw = InvitationKey::new(DERIVATION_KEY)
        .unwrap()
        .derive(owner, room, request, &"9007199254740993".parse().unwrap())
        .unwrap();
    let ack = json!({"status":"success","data":{"resourceId":invitation}});
    let mut steps = vec![];
    for response in [
        ack.clone(),
        json!({"status":"success","data":{"state":"accepted","response":ack}}),
        json!({"status":"success","data":{"state":"unknown","response":null}}),
    ] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            if steps.len() == 1 {
                "/rest/v1/rpc/issue_room_invitation_command"
            } else {
                "/rest/v1/rpc/read_room_invitation_command"
            },
            response,
        ));
    }
    let mock = Mock::new(steps);
    let app = router(keyed(&mock.base));
    for (i, path) in [
        format!("/v1/rooms/{room}/invitations"),
        format!("/v1/rooms/{room}/invitations/disposition"),
        format!("/v1/rooms/{room}/invitations/disposition"),
    ]
    .into_iter()
    .enumerate()
    {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if i == 0 {
                StatusCode::ACCEPTED
            } else {
                StatusCode::OK
            }
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let value = body(response).await;
        if i == 0 {
            assert_eq!(value["data"]["token"], serde_json::to_value(&raw).unwrap());
        } else if i == 1 {
            assert_eq!(
                value["data"]["response"]["data"]["token"],
                serde_json::to_value(&raw).unwrap()
            );
        } else {
            assert_eq!(value["data"], json!({"state":"unknown","response":null}));
        }
    }
    let seen = mock.finish();
    for request in seen.iter().skip(1).step_by(2) {
        let payload: Value = serde_json::from_slice(&request.2).unwrap();
        assert_eq!(payload["p_token_hash"], raw.hash().unwrap());
        assert!(
            !String::from_utf8_lossy(&request.2)
                .contains(serde_json::to_value(&raw).unwrap().as_str().unwrap())
        );
    }
}
#[tokio::test]
async fn missing_key_blocks_only_issuance_without_attempting_mutation() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let mock = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_current_session",
        &token,
        session,
    )]);
    let response = router(config(&mock.base))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/rooms/{}/invitations", Uuid::new_v4()))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"requestId":Uuid::new_v4(),"expectedRevision":"0"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    mock.finish();
}
#[tokio::test]
async fn join_hashes_canonical_capability_and_binds_room_and_slot_ack() {
    let (bearer_token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let room = Uuid::new_v4();
    let request = Uuid::new_v4();
    let raw = "A".repeat(43);
    let token: InvitationToken = serde_json::from_value(json!(raw)).unwrap();
    let mock = Mock::new(vec![
        caller_step(
            "/rest/v1/rpc/read_current_session",
            &bearer_token,
            session.clone(),
        ),
        Step::json(
            "/rest/v1/rpc/join_room_command",
            json!({"status":"success","data":{"resourceId":room,"slotId":Uuid::new_v4()}}),
        ),
    ]);
    let response = router(config(&mock.base))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/rooms/{room}/join"))
                .header(header::AUTHORIZATION, format!("Bearer {bearer_token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"requestId":request,"token":raw}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let seen = mock.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[1].2).unwrap(),
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_request":request,"p_room":room,"p_token_hash":token.hash().unwrap()})
    );
}
#[tokio::test]
async fn invitation_intents_reject_capability_aliases_missing_nulls_authority_and_malformed_paths()
{
    let (token, _) = identity();
    let mock = Mock::new(vec![]);
    let app = router(keyed(&mock.base));
    let room = Uuid::new_v4();
    let request = Uuid::new_v4();
    for (path, input) in [
        (
            format!("/v1/rooms/{room}/join"),
            json!({"requestId":request}),
        ),
        (
            format!("/v1/rooms/{room}/join"),
            json!({"requestId":request,"token":format!("{}B","A".repeat(42))}),
        ),
        (
            format!("/v1/rooms/{room}/join"),
            json!({"requestId":request,"token":null,"tokenHash":"f".repeat(64)}),
        ),
        (
            "/v1/invitations/%FF/revoke".into(),
            json!({"requestId":request}),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body(response).await["status"], "fail");
    }
    mock.finish();
}
