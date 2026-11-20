//! URI query rejection across all POST routes; no upstream business work.
use super::*;

#[tokio::test]
async fn unsupported_queries_reject_every_registered_post_before_upstream_work() {
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let (token, _) = identity();
    let id = "11111111-1111-1111-1111-111111111111";
    for route in content_type::JSON_POST_ROUTES
        .iter()
        .chain(["/v1/webhooks/stripe", "/v1/webhooks/patreon"].iter())
    {
        let mut path = (*route).to_owned();
        while let Some(start) = path.find('{') {
            let end = path[start..].find('}').unwrap() + start;
            path.replace_range(start..=end, id);
        }
        let observe = route.ends_with("/deletion/observe");
        let input = if observe {
            json!({"accountId":id,"requestId":id,"statusKey":URL_SAFE_NO_PAD.encode([0_u8;32])})
                .to_string()
        } else {
            "{}".to_owned()
        };
        for query in ["?unused=1", "?", "?requestId=first&requestId=second"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("{path}{query}"))
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(input.clone()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if observe {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::BAD_REQUEST
                },
                "{route}{query}"
            );
            assert_eq!(
                body(response).await,
                if observe {
                    json!({"status":"fail","data":{"observer":"unavailable"}})
                } else {
                    json!({"status":"fail","data":{"query":"unsupported"}})
                },
                "{route}{query}"
            );
        }
    }
    assert!(backend.finish().is_empty());
}

#[tokio::test]
async fn catalogue_without_uri_query_preserves_body_selection_and_caller_success() {
    let (token, _) = identity();
    let card = Uuid::new_v4();
    let labels = json!({"cards":[{"id":card,"name":"Literal%_★","oracleRef":card}],"printings":[]});
    let backend = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_my_catalogue",
        &token,
        json!({"status":"success","data":labels}),
    )]);
    let app = router(config(&backend.base));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/catalogue/resolve")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"definitionIds":[card],"printingIds":[]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body(response).await,
        json!({"status":"success","data":labels})
    );
    let seen = backend.finish();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[0].2).unwrap(),
        json!({"p_definitions":[card],"p_printings":[]})
    );
}
