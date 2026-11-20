//! Controlled protocol fixtures; real writer authority is exercised in database tests.
use super::*;

#[tokio::test]
async fn pack_outcome_reads_use_original_bearer_without_seed_or_foreign_receipt() {
    let (token, _) = identity();
    let owner = Uuid::new_v4();
    let opened = Uuid::new_v4();
    let valid = json!({"status":"success","data":{"accountId":owner,"outcome":{"id":opened,"inventoryAssetId":Uuid::new_v4(),"recipeId":Uuid::new_v4(),"operationId":Uuid::new_v4(),"contents":[{"printingId":Uuid::new_v4(),"quantity":"2"}]}}});
    let mut foreign = valid.clone();
    foreign["data"]["outcome"]["id"] = json!(Uuid::new_v4());
    let mut seed = valid.clone();
    seed["data"]["outcome"]["seed"] = json!("private-seed");
    let backend = Mock::new(
        [
            valid,
            foreign,
            seed,
            json!({"status":"fail","data":{"pack":"unavailable"}}),
        ]
        .into_iter()
        .map(|data| caller_step("/rest/v1/rpc/read_my_pack_open", &token, data))
        .collect(),
    );
    let app = router(config(&backend.base));
    for status in [
        StatusCode::OK,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::NOT_FOUND,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/packs/openings/{opened}"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(!body(response).await.to_string().contains("private-seed"));
    }
    for (_, _, bytes) in backend.finish() {
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({"p_open":opened})
        );
    }
}

#[tokio::test]
async fn seven_economy_intents_bind_original_session_and_exact_terms() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let target = Uuid::new_v4();
    let request = Uuid::new_v4();
    let big = "9007199254740993";
    let inputs = vec![
        json!({"kind":"trade_create","command":{"requestId":request,"recipientId":target,"give":[],"request":[{"assetId":target,"quantity":big}]}}),
        json!({"kind":"trade_response","command":{"requestId":request,"tradeId":target,"action":"accept"}}),
        json!({"kind":"listing_create","command":{"requestId":request,"lot":{"assetId":target,"quantity":big},"totalPrice":big}}),
        json!({"kind":"listing_cancel","command":{"requestId":request,"listingId":target}}),
        json!({"kind":"listing_buy","command":{"requestId":request,"listingId":target,"expectedRevision":"1","expectedTotal":big}}),
        json!({"kind":"pack_buy","command":{"requestId":request,"recipeId":target,"quantity":"1000","expectedPolicyId":target,"expectedRevision":"0","expectedTotal":big}}),
        json!({"kind":"pack_open","command":{"requestId":request,"inventoryAssetId":target}}),
    ];
    let mut steps = Vec::new();
    for _ in &inputs {
        for lookup in [false, true] {
            steps.push(caller_step(
                "/rest/v1/rpc/read_current_session",
                &token,
                session.clone(),
            ));
            steps.push(Step::json(if lookup{"/rest/v1/rpc/read_economy_command"}else{"/rest/v1/rpc/execute_economy_command"},
            if lookup{json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":target}}}})}else{json!({"status":"success","data":{"resourceId":target}})}));
        }
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for input in &inputs {
        for lookup in [false, true] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(if lookup {
                            "/v1/economy/commands/disposition"
                        } else {
                            "/v1/economy/commands"
                        })
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(serde_json::to_vec(input).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if lookup {
                    StatusCode::OK
                } else {
                    StatusCode::ACCEPTED
                }
            );
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
    }
    let seen = backend.finish();
    for (index, input) in inputs.iter().enumerate() {
        for lookup in 0..2 {
            assert_eq!(
                serde_json::from_slice::<Value>(&seen[index * 4 + lookup * 2 + 1].2).unwrap(),
                json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_input":input})
            );
        }
    }
}

#[tokio::test]
async fn economy_rejects_injected_authority_bad_ids_and_foreign_acknowledgments() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let input = json!({"kind":"listing_buy","command":{"requestId":Uuid::new_v4(),"listingId":Uuid::new_v4(),"expectedRevision":"1","expectedTotal":"10"}});
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        Step::json(
            "/rest/v1/rpc/execute_economy_command",
            json!({"status":"success","data":{"resourceId":Uuid::new_v4()}}),
        ),
    ]);
    let app = router(config(&backend.base));
    let mut injected = input.clone();
    injected["command"]["actorId"] = json!(Uuid::new_v4());
    let mut nil = input.clone();
    nil["command"]["requestId"] = json!(Uuid::nil());
    let mut numeric = input.clone();
    numeric["command"]["expectedTotal"] = json!(10);
    for (value, status) in [
        (injected, StatusCode::BAD_REQUEST),
        (nil, StatusCode::BAD_REQUEST),
        (numeric, StatusCode::BAD_REQUEST),
        (input, StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/economy/commands")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&value).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    backend.finish();
}
