//! Session-bound deletion intent and a narrowly scoped private status capability.
use super::*;
use crate::{
    commands::CommandAck,
    deletion::{DeletionObservation, ObserveDeletion, RequestDeletion},
};

pub(super) async fn request(State(state): State<ApiState>, input: Request) -> Response {
    no_store(request_inner(state, input).await)
}
async fn request_inner(state: ApiState, input: Request) -> Response {
    if input.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(input.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    if !single_content_type(input.headers()).is_some_and(|value| value == "application/json") {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let command: RequestDeletion = match to_bytes(input.into_body(), 1024)
        .await
        .ok()
        .and_then(|body| parse_object(&body).ok())
    {
        Some(command) => command,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if command.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    state.blocking(move |config| {
        let actor=match config.actor().and_then(|client|client.resolve(&token)) { Ok(actor)=>actor,Err(error)=>return actor_error(error) };
        let Some(account)=actor.account() else { return fail(StatusCode::CONFLICT,"account","unavailable"); };
        let backend=match backend(config) { Ok(backend)=>backend,Err(_)=>return unavailable() };
        let response:Result<ApiResponse<CommandAck>,_>=backend.rpc("request_account_deletion_command",&json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":command.request_id,"p_status_key_hash":command.status_key_hash}));
        match response {
            Ok(ApiResponse::Success{data:ack}) if ack.resource_id==account=>(StatusCode::ACCEPTED,Json(ApiResponse::Success{data:ack})).into_response(),
            Ok(ApiResponse::Fail{data}) if data==BTreeMap::from([("session".into(),"unauthorized".into())])=>fail(StatusCode::UNAUTHORIZED,"session","unauthorized"),
            Ok(ApiResponse::Fail{data}) if data.len()==1=>match data.iter().next().map(|(key,value)|(key.as_str(),value.as_str())) {
                Some(("request",reason @ ("invalid"|"conflict")))=>fail(StatusCode::CONFLICT,"request",reason),
                Some(("account","unavailable"))=>fail(StatusCode::CONFLICT,"account","unavailable"),
                _=>unavailable(),
            },
            _=>unavailable(),
        }
    }).await
}

pub(super) async fn observe(State(state): State<ApiState>, input: Request) -> Response {
    no_store(observe_inner(state, input).await)
}
async fn observe_inner(state: ApiState, input: Request) -> Response {
    if input.uri().query().is_some() {
        return observer_unavailable();
    }
    if !single_content_type(input.headers()).is_some_and(|value| value == "application/json") {
        return observer_unavailable();
    }
    let capability: ObserveDeletion = match to_bytes(input.into_body(), 1024)
        .await
        .ok()
        .and_then(|body| parse_object(&body).ok())
    {
        Some(capability) => capability,
        None => return observer_unavailable(),
    };
    let digest = match capability.key_hash() {
        Ok(digest) => digest,
        Err(_) => return observer_unavailable(),
    };
    let account = capability.account_id;
    let request = capability.request_id;
    // The raw key is not captured by the blocking work, RPC, logging or receipts.
    drop(capability);
    state
        .blocking(move |config| {
            let backend = match backend(config) {
                Ok(backend) => backend,
                Err(_) => return unavailable(),
            };
            let response: Result<ApiResponse<DeletionObservation>, _> = backend.rpc(
                "observe_account_deletion",
                &json!({"p_account":account,"p_request":request,"p_status_key_hash":digest}),
            );
            match response {
                Ok(ApiResponse::Success { data }) if data.matches(account, request) => {
                    success(data)
                }
                Ok(ApiResponse::Fail { data })
                    if data == BTreeMap::from([("observer".into(), "unavailable".into())]) =>
                {
                    observer_unavailable()
                }
                _ => unavailable(),
            }
        })
        .await
}
fn backend(config: &ApiConfig) -> Result<BackendClient, crate::backend::BackendError> {
    BackendClient::new(
        BackendConfig::new(&config.base, &config.secret_key)?,
        "spellwake-deletion-api",
        Duration::from_secs(60),
    )
}
fn observer_unavailable() -> Response {
    fail(StatusCode::NOT_FOUND, "observer", "unavailable")
}
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
