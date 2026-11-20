//! Read original native dispositions, never resubmit or execute a move.
use super::*;
use crate::player_wire::PlayerReceiptQuery;
pub(super) async fn read(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let (token, query) =
        match native_delivery_routes::parse::<PlayerReceiptQuery>(path, request, |q, game| {
            q.game_id == game && q.validate().is_ok()
        })
        .await
        {
            Ok(parsed) => parsed,
            Err(response) => return *response,
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
            let repository =
                match native_delivery_routes::repository(config, "spellwake-native-receipts") {
                    Ok(repo) => repo,
                    Err(response) => return *response,
                };
            match repository.read_human_receipts(&actor, &query) {
                Ok(ApiResponse::Success { data }) => success(json!({"receipts":data})),
                Ok(ApiResponse::Fail { data }) => failure(data),
                _ => unavailable(),
            }
        })
        .await
}

/// One closed receipt failure vocabulary shared by receipt and reconnect delivery.
pub(super) fn failure(data: BTreeMap<String, String>) -> Response {
    if data.len() != 1 {
        return unavailable();
    }
    let Some((field, reason)) = data.iter().next() else {
        return unavailable();
    };
    let status = match (field.as_str(), reason.as_str()) {
        ("session", "unauthorized") => StatusCode::UNAUTHORIZED,
        ("game", "unavailable") => StatusCode::NOT_FOUND,
        ("request", "conflict") => StatusCode::CONFLICT,
        ("request", "invalid") => StatusCode::BAD_REQUEST,
        _ => return unavailable(),
    };
    (status, Json(ApiResponse::<Value>::Fail { data })).into_response()
}
