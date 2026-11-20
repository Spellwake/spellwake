//! Original human intent only; trusted native resources and SQL own preparation.
use super::*;
use crate::{
    native_game::{NativeHumanAccessOutcome, NativeSourceRepository},
    player_wire::{MAX_PLAYER_WIRE_BYTES, PlayerCommand, PlayerCommandAck},
};

pub(super) async fn execute(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    let game = match identifier(path) {
        Ok(game) => game,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "game", reason),
    };
    if !single_content_type(request.headers()).is_some_and(|kind| kind == "application/json") {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let original: PlayerCommand = match to_bytes(request.into_body(), MAX_PLAYER_WIRE_BYTES)
        .await
        .ok()
        .and_then(|bytes| parse_object(&bytes).ok())
    {
        Some(command) => command,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if original.basis.game_id != game || original.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "request", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|client| client.resolve(&token)) {
                Ok(actor) => actor,
                Err(error) => return actor_error(error),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let backend = match BackendConfig::new(&config.base, &config.secret_key).and_then(|c| {
                BackendClient::new(c, "spellwake-native-command", Duration::from_secs(60))
            }) {
                Ok(backend) => backend,
                Err(_) => return unavailable(),
            };
            let repository = NativeSourceRepository::new(backend);
            match repository.human_original(&actor, &original) {
                Ok(Some(response)) => return reply(response),
                Ok(None) => {}
                Err(_) => return unavailable(),
            }
            let access = match repository.human_access(
                &actor,
                original.basis.game_id,
                original.basis.context_id,
            ) {
                Ok(NativeHumanAccessOutcome::Granted(access)) => access,
                Ok(NativeHumanAccessOutcome::Unauthorized) => {
                    return fail(StatusCode::UNAUTHORIZED, "session", "unauthorized");
                }
                Ok(NativeHumanAccessOutcome::Unavailable) => {
                    return fail(StatusCode::NOT_FOUND, "game", "unavailable");
                }
                Err(_) => return unavailable(),
            };
            let current = access.basis();
            if original.basis.epoch != current.epoch || original.basis.revision > current.revision {
                // Invalid source is an admission refusal, not a fake stored rejection.
                return fail(StatusCode::CONFLICT, "basis", "invalid");
            }
            if original.basis != current || !access.active() {
                return match repository.submit_human_stale(&actor, &access, &original) {
                    Ok(response) => reply(response),
                    Err(_) => unavailable(),
                };
            }
            let (Some(runtime), Some(owner)) = (&config.native_views, &config.native_host_owner)
            else {
                return unavailable();
            };
            match runtime.command_current(&repository, &actor, owner, &access, &original) {
                Ok(response) => reply(response),
                Err(_) => unavailable(),
            }
        })
        .await
}
fn reply(response: ApiResponse<PlayerCommandAck>) -> Response {
    match response {
        ApiResponse::Success { data } => {
            (StatusCode::ACCEPTED, Json(ApiResponse::Success { data })).into_response()
        }
        ApiResponse::Fail { data } => {
            let code =
                if data.len() == 1 && data.get("session").is_some_and(|v| v == "unauthorized") {
                    StatusCode::UNAUTHORIZED
                } else if data.len() == 1
                    && (data.get("game").is_some_and(|v| v == "unavailable")
                        || data.get("actor").is_some_and(|v| v == "unavailable"))
                {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::CONFLICT
                };
            (code, Json(ApiResponse::<PlayerCommandAck>::Fail { data })).into_response()
        }
        ApiResponse::Error { .. } => unavailable(),
    }
}
