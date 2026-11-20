//! Shared bounded loopback transport for explicit trusted-upstream fault tests.
use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use std::{sync::Arc, time::Duration};

pub(super) enum ResponseDelivery {
    Forward,
    Truncate,
}
pub(super) trait Fault: Send + Sync {
    fn request(&self, _method: &reqwest::Method, _path: &str, _body: &mut Vec<u8>) -> Option<u16> {
        None
    }
    fn response(&self, method: &reqwest::Method, path: &str, status: u16, body: &mut Vec<u8>);
    fn delivery(
        &self,
        _method: &reqwest::Method,
        _path: &str,
        _status: u16,
        _body: &[u8],
    ) -> ResponseDelivery {
        ResponseDelivery::Forward
    }
}
struct ProxyState {
    client: reqwest::Client,
    base: String,
    key: Option<String>,
    fault: Arc<dyn Fault>,
}
pub(super) struct Proxy {
    runtime: tokio::runtime::Runtime,
    task: tokio::task::JoinHandle<()>,
    url: String,
}
impl Proxy {
    pub(super) fn new(base: &str, key: Option<&str>, fault: Arc<dyn Fault>) -> Self {
        let origin = reqwest::Url::parse(base).unwrap();
        assert!(origin.scheme() == "http" && origin.host_str() == Some("127.0.0.1"));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let state = Arc::new(ProxyState {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap(),
            base: base.trim_end_matches('/').to_owned(),
            key: key.map(str::to_owned),
            fault,
        });
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().fallback(forward).with_state(state);
        let task = runtime.spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { runtime, task, url }
    }
    pub(super) fn url(&self) -> &str {
        &self.url
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
        let _ = self.runtime.block_on(&mut self.task);
    }
}
async fn forward(State(state): State<Arc<ProxyState>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let table_read = matches!(
        path.as_str(),
        "/rest/v1/replay_manifest" | "/rest/v1/game_result" | "/rest/v1/game_event"
    );
    assert!(path.starts_with("/storage/v1/") || path.starts_with("/rest/v1/rpc/") || table_read);
    assert!(request.uri().query().is_none() || table_read);
    assert!(!table_read || request.method() == reqwest::Method::GET);
    let destination = request.uri().path_and_query().unwrap().as_str().to_owned();
    let method = request.method().clone();
    let mut headers = request.headers().clone();
    headers.remove("host");
    headers.remove("content-length");
    headers.remove("connection");
    if let Some(key) = &state.key {
        assert_eq!(headers["apikey"], "sb_secret_test_only");
        headers.insert("apikey", key.parse().unwrap());
    }
    let mut body = to_bytes(request.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    if let Some(status) = state.fault.request(&method, &path, &mut body) {
        return axum::http::StatusCode::from_u16(status)
            .unwrap()
            .into_response();
    }
    let mut upstream = state
        .client
        .request(method.clone(), format!("{}{}", state.base, destination))
        .headers(headers)
        .body(body)
        .send()
        .await
        .unwrap();
    let status = upstream.status().as_u16();
    let content_type = upstream.headers().get("content-type").cloned();
    let encoding = upstream.headers().get("content-encoding").cloned();
    let mut bytes = Vec::new();
    while let Some(chunk) = upstream.chunk().await.unwrap() {
        assert!(
            bytes
                .len()
                .checked_add(chunk.len())
                .is_some_and(|size| size <= 8 * 1024 * 1024)
        );
        bytes.extend_from_slice(&chunk);
    }
    state.fault.response(&method, &path, status, &mut bytes);
    let mut response = Response::builder().status(status);
    if let Some(value) = content_type {
        response = response.header("content-type", value);
    }
    if let Some(value) = encoding {
        response = response.header("content-encoding", value);
    }
    match state.fault.delivery(&method, &path, status, &bytes) {
        ResponseDelivery::Forward => response.body(Body::from(bytes)).unwrap(),
        ResponseDelivery::Truncate => {
            // The real upstream reply was completely received above. Deliver a
            // strict prefix with its original length, then fail the HTTP body.
            // This exercises the downstream client's body IO, not JSON parsing.
            assert!(bytes.len() > 1);
            let length = bytes.len();
            bytes.truncate(length / 2);
            let prefix = futures::stream::once(async move {
                Ok::<_, std::io::Error>(axum::body::Bytes::from(bytes))
            });
            let failure = futures::stream::once(async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Err::<axum::body::Bytes, _>(std::io::Error::other(
                    "owned downstream acknowledgment loss",
                ))
            });
            response
                .header("content-length", length)
                .body(Body::from_stream(prefix.chain(failure)))
                .unwrap()
        }
    }
}
