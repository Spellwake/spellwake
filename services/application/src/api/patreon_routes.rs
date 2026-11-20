//! Original-bearer OAuth flow endpoints; no public callback accepts an account/provider assertion.
use super::{command_routes::private, *};
use crate::patreon::{
    association::{CompleteLinkRequest, LinkRequest},
    intake::{IntakeError, IntakeService},
    service::{AssociationService, ServiceError},
};

/// Provider authentication is the exact body signature, independently of player sessions.
pub(super) async fn webhook(State(state): State<ApiState>, request: Request) -> Response {
    private(webhook_inner(state, request).await)
}
async fn webhook_inner(state: ApiState, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let mut headers = request.headers().get_all("x-patreon-signature").iter();
    let signature = headers
        .next()
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let Some(signature) = signature.filter(|_| headers.next().is_none()) else {
        return fail(StatusCode::BAD_REQUEST, "signature", "invalid");
    };
    let body = match to_bytes(request.into_body(), crate::patreon::MAX_WEBHOOK_BODY).await {
        Ok(body) => body,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    // An application occurrence identity is generated before capture; no provider ID is invented.
    let delivery = Uuid::new_v4();
    state
        .blocking(move |config| {
            let Some(settings) = &config.patreon_intake else {
                return unavailable();
            };
            let result = BackendConfig::new(&config.base, &config.secret_key)
                .map_err(IntakeError::Backend)
                .and_then(|backend| IntakeService::new(backend, settings.clone()))
                .and_then(|service| service.ingest(&body, &signature, delivery));
            match result {
                Ok(()) => success(json!({"received":true})),
                Err(IntakeError::Authentication(_)) => {
                    fail(StatusCode::BAD_REQUEST, "signature", "invalid")
                }
                Err(_) => {
                    eprintln!("patreon_notification event=intake_unresolved");
                    unavailable()
                }
            }
        })
        .await
}
#[derive(Clone, Copy)]
enum Mode {
    Start,
    Complete,
    Status,
}
enum Input {
    Flow(LinkRequest),
    Complete(CompleteLinkRequest),
}
pub(super) async fn start(State(state): State<ApiState>, request: Request) -> Response {
    handle(state, request, Mode::Start).await
}
pub(super) async fn complete(State(state): State<ApiState>, request: Request) -> Response {
    handle(state, request, Mode::Complete).await
}
pub(super) async fn status(State(state): State<ApiState>, request: Request) -> Response {
    handle(state, request, Mode::Status).await
}
async fn handle(state: ApiState, request: Request, mode: Mode) -> Response {
    private(inner(state, request, mode).await)
}
async fn inner(state: ApiState, request: Request, mode: Mode) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    if !single_content_type(request.headers()).is_some_and(|v| v == "application/json") {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), 16384).await {
        Ok(bytes) => bytes,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    let input = match mode {
        Mode::Complete => parse_object::<CompleteLinkRequest>(&bytes)
            .ok()
            .filter(|input| input.validate().is_ok())
            .map(Input::Complete),
        _ => parse_object::<LinkRequest>(&bytes)
            .ok()
            .filter(|input| input.state_hash().is_ok())
            .map(Input::Flow),
    };
    let Some(input) = input else {
        return fail(StatusCode::BAD_REQUEST, "body", "invalid");
    };
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|client| client.resolve(&token)) {
                Ok(actor) => actor,
                Err(error) => return actor_error(error),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let service = BackendConfig::new(&config.base, &config.secret_key)
                .map_err(ServiceError::Backend)
                .and_then(|backend| AssociationService::new(backend, config.patreon.clone()));
            let service = match service {
                Ok(service) => service,
                Err(error) => return outcome::<Value>(Err(error), StatusCode::OK),
            };
            match (mode, input) {
                (Mode::Start, Input::Flow(request)) => {
                    outcome(service.start(&actor, &request), StatusCode::ACCEPTED)
                }
                (Mode::Status, Input::Flow(request)) => {
                    outcome(service.status(&actor, &request), StatusCode::OK)
                }
                (Mode::Complete, Input::Complete(request)) => {
                    outcome(service.complete(&actor, &request), StatusCode::OK)
                }
                _ => unavailable(),
            }
        })
        .await
}
fn outcome<T: Serialize>(
    result: Result<ApiResponse<T>, ServiceError>,
    code: StatusCode,
) -> Response {
    match result {
        Ok(ApiResponse::Success { data }) => {
            (code, Json(ApiResponse::Success { data })).into_response()
        }
        Ok(ApiResponse::Fail { data }) => {
            let status = if data.get("session").is_some_and(|v| v == "unauthorized") {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::CONFLICT
            };
            (status, Json(ApiResponse::<Value>::Fail { data })).into_response()
        }
        _ => {
            eprintln!("patreon_association event=unresolved");
            unavailable()
        }
    }
}
