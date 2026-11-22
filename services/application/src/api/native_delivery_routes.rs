//! Shared bounded human read boundary; no native projector, writer or public framework.
use super::*;
use crate::{
    native_game::{NativeReadRefusal, NativeSourceRepository},
    player_wire::MAX_PLAYER_WIRE_BYTES,
};
/// Parse the common native read envelope; operation-specific scope stays at its caller.
pub(super) async fn parse<T: serde::de::DeserializeOwned>(
    path: IdentifierPath,
    request: Request,
    validate: impl FnOnce(&T, Uuid) -> bool,
) -> Result<(String, T), Box<Response>> {
    if request.uri().query().is_some() {
        return Err(Box::new(fail(
            StatusCode::BAD_REQUEST,
            "query",
            "unsupported",
        )));
    }
    let token = bearer(request.headers()).map_err(|error| Box::new(actor_error(error)))?;
    let game = identifier(path)
        .map_err(|reason| Box::new(fail(StatusCode::BAD_REQUEST, "game", reason)))?;
    if !is_json_content_type(request.headers()) {
        return Err(Box::new(fail(
            StatusCode::BAD_REQUEST,
            "body",
            "json_required",
        )));
    }
    let query: T = to_bytes(request.into_body(), MAX_PLAYER_WIRE_BYTES)
        .await
        .ok()
        .and_then(|bytes| parse_object(&bytes).ok())
        .ok_or_else(|| Box::new(fail(StatusCode::BAD_REQUEST, "body", "invalid")))?;
    if !validate(&query, game) {
        return Err(Box::new(fail(
            StatusCode::BAD_REQUEST,
            "request",
            "invalid",
        )));
    }
    Ok((token, query))
}
pub(super) fn repository(
    config: &ApiConfig,
    agent: &'static str,
) -> Result<NativeSourceRepository, Box<Response>> {
    BackendConfig::new(&config.base, &config.secret_key)
        .and_then(|config| BackendClient::new(config, agent, Duration::from_secs(60)))
        .map(NativeSourceRepository::new)
        .map_err(|_| Box::new(unavailable()))
}
/// Success whose entire encoded API value stays within the player wire bound.
/// A larger value is the safe unavailable response, never an oversized body.
pub(super) fn bounded_success(data: impl Serialize) -> Response {
    bounded_success_with_limit(data, MAX_PLAYER_WIRE_BYTES)
}

pub(super) fn bounded_success_with_limit(data: impl Serialize, max_bytes: usize) -> Response {
    match serde_json::to_vec(&ApiResponse::Success { data }) {
        Ok(body) if body.len() <= max_bytes => (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response(),
        _ => unavailable(),
    }
}
pub(super) fn refusal(reason: NativeReadRefusal) -> Response {
    match reason {
        NativeReadRefusal::Unauthorized => {
            fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
        }
        NativeReadRefusal::Unavailable => fail(StatusCode::NOT_FOUND, "game", "unavailable"),
        NativeReadRefusal::Stale => fail(StatusCode::CONFLICT, "basis", "stale"),
    }
}
/// Sealed JSON response plus the actually renewed caller, carried across worker handoff.
/// Fields stay private: no response/request copies can manufacture renewed authority.
pub(super) struct GuardedResponse {
    response: Response,
    actor: RequestActor,
}
/// Validate actual renewal after composition IO and retain its deadline with the response.
pub(super) fn guard(
    actor: &RequestActor,
    fresh: Result<RequestActor, ActorError>,
    response: Response,
) -> Result<GuardedResponse, Box<Response>> {
    let actor = verified_actor(actor, fresh).map_err(|error| Box::new(actor_error(error)))?;
    Ok(GuardedResponse { response, actor })
}
/// Recheck after the blocking result's scheduling delay, immediately before route return.
pub(super) fn publish(result: Result<Result<GuardedResponse, Box<Response>>, ()>) -> Response {
    match result {
        Ok(Ok(guarded)) => match guarded.actor.ensure_unexpired() {
            Ok(()) => guarded.response,
            Err(error) => actor_error(error),
        },
        Ok(Err(response)) => *response,
        Err(()) => unavailable(),
    }
}
/// Reuse the exact original identity and deadline guard without publishing a response.
/// The caller must still check the deadline after any subsequent IO or scheduling delay.
pub(super) fn verified_actor(
    actor: &RequestActor,
    fresh: Result<RequestActor, ActorError>,
) -> Result<RequestActor, ActorError> {
    let fresh = fresh?;
    if !actor.same_identity(&fresh) {
        return Err(ActorError::Unauthorized);
    }
    fresh.ensure_unexpired()?;
    Ok(fresh)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_success_refuses_an_oversized_entire_value() {
        let envelope = serde_json::to_vec(&ApiResponse::Success { data: "" })
            .unwrap()
            .len();
        let fits = "x".repeat(MAX_PLAYER_WIRE_BYTES - envelope);
        assert_eq!(bounded_success(&fits).status(), StatusCode::OK);
        let over = "x".repeat(MAX_PLAYER_WIRE_BYTES - envelope + 1);
        assert_eq!(
            bounded_success(&over).status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
