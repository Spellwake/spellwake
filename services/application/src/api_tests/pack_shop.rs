//! Original-bearer pack catalogue and exact quote request/response transport.
use super::*;
#[tokio::test]
async fn pack_shop_preserves_original_bearer_and_quote_quantity() {
    let f: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/pack-shop.json")).unwrap();
    let (token, _) = identity();
    let recipe = f["products"]["items"][0]["id"].as_str().unwrap();
    let cases = [
        (
            "/v1/packs/products".to_owned(),
            "read_my_pack_products",
            f["products"].clone(),
            json!({"p_after":null,"p_limit":32}),
        ),
        (
            format!("/v1/packs/products/{recipe}/quote?quantity=2"),
            "read_my_pack_quote",
            f["purchase"].clone(),
            json!({"p_recipe":recipe,"p_quantity":2}),
        ),
        (
            format!("/v1/packs/products/{recipe}/quote?quantity=11"),
            "read_my_pack_quote",
            f["valuation"].clone(),
            json!({"p_recipe":recipe,"p_quantity":11}),
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
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            body(response).await,
            json!({"status":"success","data":data})
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
async fn pack_shop_denies_bad_queries_unavailable_products_and_mismatched_quotes() {
    let f: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/pack-shop.json")).unwrap();
    let (token, _) = identity();
    let recipe = f["products"]["items"][0]["id"].as_str().unwrap();
    let backend = Mock::new(vec![
        caller_step(
            "/rest/v1/rpc/read_my_pack_quote",
            &token,
            json!({"status":"fail","data":{"product":"unavailable"}}),
        ),
        caller_step(
            "/rest/v1/rpc/read_my_pack_quote",
            &token,
            json!({"status":"success","data":f["purchase"]}),
        ),
    ]);
    let app = router(config(&backend.base));
    for (path, status) in [
        (
            "/v1/packs/products?limit=33".to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("/v1/packs/products/{recipe}/quote?quantity=0"),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("/v1/packs/products/{recipe}/quote?actor=x"),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("/v1/packs/products/{recipe}/quote"),
            StatusCode::NOT_FOUND,
        ),
        (
            format!("/v1/packs/products/{recipe}/quote?quantity=1"),
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
