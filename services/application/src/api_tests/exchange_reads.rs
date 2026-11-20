//! Caller transport and strict projection failures; actual roles are tested in Postgres.
use super::*;
fn fixture() -> Value {
    serde_json::from_str(include_str!("../../tests/fixtures/exchange-reads.json")).unwrap()
}

#[tokio::test]
async fn exchange_read_routes_keep_original_bearer_filters_and_resource_identity() {
    let f = fixture();
    let (token, _) = identity();
    let account = &f["offeredTrade"]["accountId"];
    let trade = f["offeredTrade"]["trade"]["id"].as_str().unwrap();
    let listing = f["listedListing"]["listing"]["id"].as_str().unwrap();
    let cases = vec![
        (
            format!("/v1/trades/{trade}"),
            "read_my_trade",
            f["offeredTrade"].clone(),
            json!({"p_trade":trade}),
        ),
        (
            format!("/v1/trades/{trade}"),
            "read_my_trade",
            f["acceptedTrade"].clone(),
            json!({"p_trade":trade}),
        ),
        (
            "/v1/trades?limit=1&state=offered".into(),
            "read_my_trades",
            json!({"accountId":account,"items":[f["offeredTrade"]["trade"]],"nextCursor":trade}),
            json!({"p_after":null,"p_limit":1,"p_state":"offered"}),
        ),
        (
            format!("/v1/market/listings/{listing}"),
            "read_my_market_listing",
            f["listedListing"].clone(),
            json!({"p_listing":listing}),
        ),
        (
            format!("/v1/market/listings/{listing}"),
            "read_my_market_listing",
            f["soldListing"].clone(),
            json!({"p_listing":listing}),
        ),
        (
            "/v1/market/listings?owned=true".into(),
            "read_my_market",
            json!({"accountId":account,"items":[f["soldListing"]["listing"]],"nextCursor":null}),
            json!({"p_after":null,"p_limit":8,"p_owned":true}),
        ),
        (
            "/v1/market/listings".into(),
            "read_my_market",
            json!({"accountId":account,"items":[f["listedListing"]["listing"]],"nextCursor":null}),
            json!({"p_after":null,"p_limit":8,"p_owned":false}),
        ),
    ];
    let backend = Mock::new(
        cases
            .iter()
            .map(|(_, rpc, data, _)| {
                caller_step(
                    &format!("/rest/v1/rpc/{rpc}"),
                    &token,
                    json!({"status":"success","data":data}),
                )
            })
            .collect(),
    );
    let app = router(config(&backend.base));
    for (path, _, expected, _) in &cases {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            body(response).await,
            json!({"status":"success","data":expected})
        );
    }
    for (seen, (_, _, _, parameters)) in backend.finish().iter().zip(&cases) {
        assert_eq!(
            serde_json::from_slice::<Value>(&seen.2).unwrap(),
            *parameters
        );
    }
}

#[tokio::test]
async fn exchange_reads_deny_foreign_results_bad_cursors_private_fields_and_unknown_queries() {
    let f = fixture();
    let (token, _) = identity();
    let trade = f["offeredTrade"]["trade"]["id"].as_str().unwrap();
    let listing = f["listedListing"]["listing"]["id"].as_str().unwrap();
    let mut foreign = f["offeredTrade"].clone();
    foreign["accountId"] = json!(Uuid::new_v4());
    let mut private = f["listedListing"].clone();
    private["listing"]["holdId"] = json!(Uuid::new_v4());
    let mut incomplete = f["soldListing"].clone();
    incomplete["listing"]["participant"]
        .as_object_mut()
        .unwrap()
        .remove("paidCoins");
    let cases = vec![
        (
            format!("/v1/trades/{trade}"),
            "read_my_trade",
            json!({"status":"success","data":foreign}),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            format!("/v1/market/listings/{listing}"),
            "read_my_market_listing",
            json!({"status":"success","data":private}),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            format!("/v1/market/listings/{listing}"),
            "read_my_market_listing",
            json!({"status":"success","data":incomplete}),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            format!("/v1/trades?after={trade}"),
            "read_my_trades",
            json!({"status":"success","data":{"accountId":f["offeredTrade"]["accountId"],"items":[f["offeredTrade"]["trade"]],"nextCursor":null}}),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            format!("/v1/trades/{trade}"),
            "read_my_trade",
            json!({"status":"fail","data":{"trade":"unavailable"}}),
            StatusCode::NOT_FOUND,
        ),
    ];
    let backend = Mock::new(
        cases
            .iter()
            .map(|(_, rpc, body, _)| {
                caller_step(&format!("/rest/v1/rpc/{rpc}"), &token, body.clone())
            })
            .collect(),
    );
    let app = router(config(&backend.base));
    for (path, _, _, status) in &cases {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), *status, "{path}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    for path in [
        "/v1/trades?actorId=private",
        "/v1/trades?limit=9",
        "/v1/trades?state=unknown",
        "/v1/market/listings?owned=unknown",
        "/v1/market/listings?limit=9",
    ] {
        assert_eq!(
            app.clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    backend.finish();
}
