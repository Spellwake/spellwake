//! Original-caller projections reject private data and requested-scope mismatches.
use super::*;
fn fixture() -> Value {
    serde_json::from_str(include_str!("../../tests/fixtures/prediction-reads.json")).unwrap()
}
#[tokio::test]
async fn prediction_reads_forward_original_bearer_and_exact_market_filter() {
    let f = fixture();
    let (token, _) = identity();
    let market = f["market"]["market"]["terms"]["id"].as_str().unwrap();
    let cases = [
        (
            format!("/v1/predictions/markets/{market}"),
            "read_my_prediction_market",
            f["market"].clone(),
            json!({"p_market":market}),
        ),
        (
            "/v1/predictions/markets".into(),
            "read_my_prediction_markets",
            f["markets"].clone(),
            json!({"p_after":null,"p_limit":50}),
        ),
        (
            format!("/v1/predictions/orders?marketId={market}"),
            "read_my_prediction_orders",
            f["orders"].clone(),
            json!({"p_market":market,"p_after":null,"p_limit":50}),
        ),
        (
            format!("/v1/predictions/positions?marketId={market}"),
            "read_my_prediction_positions",
            f["positions"].clone(),
            json!({"p_market":market,"p_after":null,"p_limit":50}),
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
    for (path, _, data, _) in &cases {
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
            json!({"status":"success","data":data})
        );
    }
    for (seen, (_, _, _, params)) in backend.finish().iter().zip(&cases) {
        assert_eq!(serde_json::from_slice::<Value>(&seen.2).unwrap(), *params);
    }
}
#[tokio::test]
async fn prediction_reads_deny_unavailable_private_fields_and_foreign_market_orders() {
    let f = fixture();
    let (token, _) = identity();
    let market = f["market"]["market"]["terms"]["id"].as_str().unwrap();
    let mut private = f["market"].clone();
    private["market"]["holdId"] = json!(Uuid::new_v4());
    let mut foreign = f["orders"].clone();
    foreign["items"][0]["marketId"] = json!(Uuid::new_v4());
    let backend = Mock::new(vec![
        caller_step(
            "/rest/v1/rpc/read_my_prediction_market",
            &token,
            json!({"status":"fail","data":{"market":"unavailable"}}),
        ),
        caller_step(
            "/rest/v1/rpc/read_my_prediction_market",
            &token,
            json!({"status":"success","data":private}),
        ),
        caller_step(
            "/rest/v1/rpc/read_my_prediction_orders",
            &token,
            json!({"status":"success","data":foreign}),
        ),
    ]);
    let app = router(config(&backend.base));
    for (path, status) in [
        (
            "/v1/predictions/orders?actor=chosen".to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "/v1/predictions/markets?limit=101".to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("/v1/predictions/markets/{market}"),
            StatusCode::NOT_FOUND,
        ),
        (
            format!("/v1/predictions/markets/{market}"),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            format!("/v1/predictions/orders?marketId={market}"),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ] {
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
        assert_eq!(response.status(), status);
    }
    backend.finish();
}
