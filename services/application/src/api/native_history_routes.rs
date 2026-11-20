//! Own-seat participant history pages from a fresh original native walk.
use super::native_delivery_routes as delivery;
use super::*;
use crate::{
    native_game::{NativeHistoryRead, NativeReadOutcome},
    player_wire::PlayerHistoryQuery,
};

pub(super) async fn read(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let (token, query) = match delivery::parse::<PlayerHistoryQuery>(path, request, |q, game| {
        q.game_id == game && !q.context_id.is_nil() && (1..=100).contains(&q.limit.get())
    })
    .await
    {
        Ok(parsed) => parsed,
        Err(response) => return *response,
    };
    let result = state
        .blocking_value(move |config| {
            let auth = match config.actor() {
                Ok(auth) => auth,
                Err(e) => return Err(Box::new(actor_error(e))),
            };
            let actor = match auth.resolve(&token) {
                Ok(actor) => actor,
                Err(e) => return Err(Box::new(actor_error(e))),
            };
            if actor.account().is_none() {
                // The authenticated denial also crosses the worker handoff.
                return delivery::guard(
                    &actor,
                    auth.resolve(&token),
                    fail(StatusCode::CONFLICT, "account", "unavailable"),
                );
            }
            let Some(runtime) = &config.native_views else {
                return Err(Box::new(unavailable()));
            };
            let repository = delivery::repository(config, "spellwake-native-history")?;
            let response = match runtime.history_page(&repository, &actor, &query) {
                Ok(NativeReadOutcome::Ready(NativeHistoryRead::Page(page))) => {
                    delivery::bounded_success(*page)
                }
                Ok(NativeReadOutcome::Ready(NativeHistoryRead::CursorUnavailable)) => {
                    fail(StatusCode::CONFLICT, "cursor", "unavailable")
                }
                Ok(NativeReadOutcome::Refused(reason)) => delivery::refusal(reason),
                Err(_) => unavailable(),
            };
            // Renew the original bearer for every native outcome, including
            // failures. SQL session liveness does not prove this JWT's deadline.
            // Keep the actual renewed actor across the worker/await handoff.
            delivery::guard(&actor, auth.resolve(&token), response)
        })
        .await;
    delivery::publish(result)
}
