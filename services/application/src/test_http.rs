//! Bounded loopback HTTP fixtures and raw wire helpers for boundary tests.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration as Timeout,
};
/// A fixture has one credential mode; vendor requests cannot accidentally inherit a backend key.
pub(crate) enum Credentials {
    Backend,
    Form,
    Caller(String, String),
    Stripe(String),
    Bearer(String),
}
type Reply = Box<dyn Fn(&[u8]) -> Vec<u8> + Send>;
pub(crate) struct Step {
    pub(crate) method: &'static str,
    pub(crate) path: String,
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
    pub(crate) expect: Option<Vec<u8>>,
    pub(crate) drop_ack: bool,
    pub(crate) credentials: Credentials,
    pub(crate) response_headers: Vec<(String, String)>,
    pub(crate) reply: Option<Reply>,
    /// Explicit original response framing for transfer-boundary tests only.
    pub(crate) raw_response: Option<Vec<u8>>,
}
impl Step {
    pub(crate) fn no_content(path: &str) -> Self {
        let mut step = Self::json(path, Value::Null);
        step.status = 204;
        step.body.clear();
        step
    }
    pub(crate) fn json(path: &str, value: Value) -> Self {
        Self {
            method: "POST",
            path: path.into(),
            status: 200,
            body: serde_json::to_vec(&value).unwrap(),
            expect: None,
            drop_ack: false,
            credentials: Credentials::Backend,
            response_headers: Vec::new(),
            reply: None,
            raw_response: None,
        }
    }
    pub(crate) fn get(path: String, status: u16, body: Vec<u8>) -> Self {
        Self {
            method: "GET",
            path,
            status,
            body,
            expect: None,
            drop_ack: false,
            credentials: Credentials::Backend,
            response_headers: Vec::new(),
            reply: None,
            raw_response: None,
        }
    }
}
pub(crate) type CapturedRequest = (String, BTreeMap<String, String>, Vec<u8>);

/// Construct a complete positional witness in explicitly supplied declaration order.
pub(crate) fn positional(value: &Value, fields: &[&str]) -> Value {
    assert_eq!(value.as_object().unwrap().len(), fields.len());
    Value::Array(
        fields
            .iter()
            .map(|field| value.get(field).unwrap().clone())
            .collect(),
    )
}

pub(crate) struct Mock {
    pub(crate) base: String,
    stop: mpsc::Sender<()>,
    expected: usize,
    finished: thread::JoinHandle<()>,
    seen: Arc<Mutex<Vec<CapturedRequest>>>,
}
fn read_request(stream: &mut TcpStream) -> (String, String, BTreeMap<String, String>, Vec<u8>) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Timeout::from_secs(10)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let words = line.split_whitespace().collect::<Vec<_>>();
    let method = words[0].to_owned();
    let path = words[1].to_owned();
    let mut headers = BTreeMap::new();
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        let (key, value) = line.split_once(':').unwrap();
        headers.insert(key.to_ascii_lowercase(), value.trim().to_owned());
    }
    let length = headers
        .get("content-length")
        .map(|v| v.parse::<usize>().unwrap())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    (method, path, headers, body)
}
impl Mock {
    pub(crate) fn new(steps: Vec<Step>) -> Self {
        Self::new_with_observer(steps, |_| {})
    }
    pub(crate) fn new_with_observer(
        steps: Vec<Step>,
        observer: impl Fn(&str) + Send + 'static,
    ) -> Self {
        Self::configured(steps, observer, Timeout::from_secs(15))
    }
    /// Override only the bounded wait between expected requests, for actual durable backoff tests.
    /// Request IO timeouts and the unexpected-extra-request guard remain unchanged.
    pub(crate) fn with_request_wait(steps: Vec<Step>, wait: Timeout) -> Self {
        Self::configured(steps, |_| {}, wait)
    }
    fn configured(
        steps: Vec<Step>,
        observer: impl Fn(&str) + Send + 'static,
        wait: Timeout,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = seen.clone();
        let expected = steps.len();
        let (stop, stopped) = mpsc::channel();
        let finished = thread::spawn(move || {
            for step in steps {
                let deadline = std::time::Instant::now() + wait;
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            if !matches!(stopped.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                                return;
                            }
                            assert!(
                                std::time::Instant::now() < deadline,
                                "expected request {}",
                                step.path
                            );
                            thread::sleep(Timeout::from_millis(2));
                        }
                        Err(e) => panic!("mock accept failed: {e}"),
                    }
                };
                let (method, path, headers, body) = read_request(&mut stream);
                observer(&path);
                assert_eq!(method, step.method);
                assert_eq!(path, step.path);
                let (api_key, bearer, stripe_version) = match &step.credentials {
                    Credentials::Backend => (Some("sb_secret_test_only"), None, None),
                    Credentials::Form => (None, None, None),
                    Credentials::Caller(key, bearer) => {
                        (Some(key.as_str()), Some(format!("Bearer {bearer}")), None)
                    }
                    Credentials::Stripe(key) => (
                        None,
                        Some(format!("Bearer {key}")),
                        Some("2026-10-28.endive"),
                    ),
                    Credentials::Bearer(key) => (None, Some(format!("Bearer {key}")), None),
                };
                if matches!(step.credentials, Credentials::Form) {
                    assert_eq!(
                        headers.get("content-type").map(String::as_str),
                        Some("application/x-www-form-urlencoded")
                    );
                }
                assert_eq!(headers.get("apikey").map(String::as_str), api_key);
                assert_eq!(headers.get("authorization"), bearer.as_ref());
                assert_eq!(
                    headers.get("stripe-version").map(String::as_str),
                    stripe_version
                );
                if path.starts_with("/rest/") {
                    assert_eq!(
                        headers
                            .get(if method == "GET" {
                                "accept-profile"
                            } else {
                                "content-profile"
                            })
                            .map(String::as_str),
                        Some("app")
                    );
                }
                if let Some(expected) = step.expect {
                    assert_eq!(body, expected);
                }
                let response_body = step.reply.map_or(step.body, |reply| reply(&body));
                captured.lock().unwrap().push((path, headers, body));
                if step.drop_ack {
                    continue;
                }
                if let Some(raw) = step.raw_response {
                    stream.write_all(&raw).unwrap();
                    continue;
                }
                write!(stream,"HTTP/1.1 {} Test\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n",step.status,response_body.len()).unwrap();
                for (key, value) in step.response_headers {
                    write!(stream, "{key}: {value}\r\n").unwrap();
                }
                write!(stream, "\r\n").unwrap();
                stream.write_all(&response_body).unwrap();
            }
            // Keep the socket live until finish: an unexpected retry must fail the test,
            // rather than looking like the deliberately unavailable provider.
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let (_, path, _, _) = read_request(&mut stream);
                        panic!("unexpected mock request {path}")
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        if !matches!(stopped.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                            break;
                        }
                        thread::sleep(Timeout::from_millis(2));
                    }
                    Err(e) => panic!("mock accept failed: {e}"),
                }
            }
        });
        Self {
            base,
            stop,
            expected,
            finished,
            seen,
        }
    }
    pub(crate) fn finish(self) -> Vec<CapturedRequest> {
        // A failed worker is still joined below, which preserves its original assertion.
        let _ = self.stop.send(());
        self.finished.join().unwrap();
        let seen = Arc::try_unwrap(self.seen).unwrap().into_inner().unwrap();
        assert_eq!(
            seen.len(),
            self.expected,
            "fixture did not receive every declared request"
        );
        seen
    }
}

/// Shared raw scalar corpus. Each concrete enum owns its tokens and invokes this
/// helper from its private module; this does not widen production visibility.
fn string_enum_rejections<T: serde::de::DeserializeOwned>(tokens: &[&str]) {
    for token in tokens {
        for value in [
            serde_json::json!({*token:null}),
            serde_json::json!({*token:{}}),
            serde_json::json!([token]),
            serde_json::json!({"type":token}),
            serde_json::json!(format!(" {token}")),
            serde_json::json!(format!("{token} ")),
            serde_json::json!(token.to_uppercase()),
        ] {
            assert!(serde_json::from_slice::<T>(&serde_json::to_vec(&value).unwrap()).is_err());
        }
    }
    for value in [
        serde_json::json!(null),
        serde_json::json!(false),
        serde_json::json!(1),
        serde_json::json!(""),
        serde_json::json!("unrecognized_wire_enum"),
    ] {
        assert!(serde_json::from_slice::<T>(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
/// Test all canonical variants against exact existing serialized string values.
pub(crate) fn string_enum_roundtrip<T: serde::de::DeserializeOwned + serde::Serialize>(
    tokens: &[&str],
) {
    for token in tokens {
        let value: T = serde_json::from_slice(&serde_json::to_vec(token).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::json!(token)
        );
    }
    string_enum_rejections::<T>(tokens);
}
/// Deserialize-only enums retain their actual variant identity without adding a
/// Serialize implementation solely for a test helper.
pub(crate) fn string_enum_variants<T: serde::de::DeserializeOwned + std::fmt::Debug>(
    tokens: &[(&str, &str)],
) {
    for (token, variant) in tokens {
        let value: T = serde_json::from_slice(&serde_json::to_vec(token).unwrap()).unwrap();
        assert_eq!(&format!("{value:?}"), variant);
    }
    string_enum_rejections::<T>(&tokens.iter().map(|(token, _)| *token).collect::<Vec<_>>());
}
