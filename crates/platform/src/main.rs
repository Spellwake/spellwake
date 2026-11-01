//! Local service skeleton. Game routes, authentication and storage are not enabled.
use axum::{Json, Router, http::StatusCode, routing::get};
use serde::Serialize;
use spellwake_contracts::ApiResponse;
use std::{error::Error, net::SocketAddr};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    service: &'static str,
    version: &'static str,
    gameplay_enabled: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let address: SocketAddr = match std::env::var("SPELLWAKE_BIND") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "127.0.0.1:8080".into(),
        Err(error) => return Err(error.into()),
    }
    .parse()?;
    let app = router();
    let listener = tokio::net::TcpListener::bind(address).await?;
    eprintln!(
        "spellwake listening on {} (gameplay disabled)",
        listener.local_addr()?
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("shutdown signal error: {error}");
            }
        })
        .await?;
    Ok(())
}

fn router() -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async {
                Json(ApiResponse::<Health>::Success {
                    data: Health {
                        service: "spellwake",
                        version: env!("CARGO_PKG_VERSION"),
                        gameplay_enabled: false,
                    },
                })
            }),
        )
        .fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::<()>::Fail {
                    data: std::collections::BTreeMap::from([(
                        "route".into(),
                        "route-not-found".into(),
                    )]),
                }),
            )
        })
        .method_not_allowed_fallback(|| async {
            (
                StatusCode::METHOD_NOT_ALLOWED,
                Json(ApiResponse::<()>::Fail {
                    data: std::collections::BTreeMap::from([(
                        "method".into(),
                        "method-not-allowed".into(),
                    )]),
                }),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_and_routing_failures_use_json_envelopes() {
        for (method, path, expected_code, expected_status) in [
            ("GET", "/health", 200, "success"),
            ("GET", "/missing", 404, "fail"),
            ("POST", "/health", 405, "fail"),
        ] {
            let response = router()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), expected_code);
            assert_eq!(response.headers()["content-type"], "application/json");
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["status"], expected_status);
            assert!(value["data"].is_object());
            if expected_status == "success" {
                assert_eq!(value["data"]["gameplayEnabled"], false);
            }
        }
    }
}
