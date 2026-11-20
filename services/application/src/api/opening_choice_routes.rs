//! Authenticated pre-opening observation and immutable play/draw selection.
use super::*;

use crate::opening_choice::{OpeningChoice, Selection};

pub(super) async fn handle(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let (token, intent) =
        match native_delivery_routes::parse::<OpeningChoice>(path, request, |body, game| {
            body.game_id == game && body.valid()
        })
        .await
        {
            Ok(input) => input,
            Err(response) => return *response,
        };
    state.blocking(move |config| {
        let actor = match config.actor().and_then(|client| client.resolve(&token)) {
            Ok(actor) => actor,
            Err(error) => return actor_error(error),
        };
        let backend = match BackendConfig::new(&config.base,&config.secret_key)
            .and_then(|config| BackendClient::new(config,"spellwake-opening-choice",Duration::from_secs(60))) {
            Ok(backend) => backend, Err(_) => return unavailable(),
        };
        match backend.rpc_object::<ApiResponse<Selection>>("native_opening_human_choice",&json!({
            "p_subject":actor.subject(),"p_session":actor.session(),"p_game":intent.game_id,"p_play_first":intent.play_first,"p_prior_chooser":intent.prior_chooser_seat_id,"p_first_seat":intent.first_seat_id
        })) {
            Ok(ApiResponse::Success { data }) if data.matches(intent.game_id) => native_delivery_routes::bounded_success(data),
            Ok(ApiResponse::Fail { data }) if data.len()==1 && data.get("choice").is_some_and(|s|s=="already selected") =>
                fail(StatusCode::CONFLICT,"choice","already selected"),
            Ok(ApiResponse::Fail { data }) if data.len()==1 && data.get("game").is_some_and(|s|s=="unavailable") =>
                fail(StatusCode::NOT_FOUND,"game","unavailable"),
            Ok(ApiResponse::Fail {data}) if data.len()==1 && data.get("session").is_some_and(|s|s=="unauthorized") => fail(StatusCode::UNAUTHORIZED,"session","unauthorized"),
            Ok(ApiResponse::Fail {data}) if data.len()==1 && data.get("choice").is_some_and(|s|s=="confirm previous chooser") => fail(StatusCode::CONFLICT,"choice","confirm previous chooser"),
            Ok(ApiResponse::Fail {data}) if data.len()==1 && data.get("choice").is_some_and(|s|s=="invalid selection") => fail(StatusCode::BAD_REQUEST,"choice","invalid selection"),
            _ => unavailable(),
        }
    }).await
}
