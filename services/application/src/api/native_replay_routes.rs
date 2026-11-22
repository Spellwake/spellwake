//! Participant replay frames from the published portable archive.
use super::native_delivery_routes as delivery;
use super::*;
use crate::{
    native_game::{NativeReadOutcome, NativeReplayRead},
    player_wire::{MAX_COMPLETED_REPLAY_BYTES, PlayerReplayMode, PlayerReplayRequest},
};

pub(super) async fn read(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let (token, query) = match delivery::parse::<PlayerReplayRequest>(path, request, |q, game| {
        q.game_id == game
            && q.history.game_id == game
            && q.history.context_id == q.context_id
            && !q.context_id.is_nil()
            && !q.perspective_seat_id.is_nil()
            && (1..=100).contains(&q.history.limit.get())
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
            let repository = delivery::repository(config, "spellwake-native-replay")?;
            let response = match runtime.replay_frame(&repository, &actor, &query) {
                Ok(NativeReadOutcome::Ready(NativeReplayRead::Frame(frame))) => {
                    if query.mode == PlayerReplayMode::CompletedFullInformation {
                        delivery::bounded_success_with_limit(*frame, MAX_COMPLETED_REPLAY_BYTES)
                    } else {
                        delivery::bounded_success(*frame)
                    }
                }
                Ok(NativeReadOutcome::Ready(NativeReplayRead::ModeUnavailable)) => {
                    fail(StatusCode::CONFLICT, "mode", "unavailable")
                }
                Ok(NativeReadOutcome::Ready(NativeReplayRead::CoordinateUnavailable)) => {
                    fail(StatusCode::CONFLICT, "atEventSequence", "unavailable")
                }
                Ok(NativeReadOutcome::Ready(NativeReplayRead::CursorUnavailable)) => {
                    fail(StatusCode::CONFLICT, "cursor", "unavailable")
                }
                Ok(NativeReadOutcome::Refused(reason)) => delivery::refusal(reason),
                Err(_) => unavailable(),
            };
            // Renew the bearer even for native refusals/errors: a live SQL
            // session cannot extend this original JWT's verified deadline.
            delivery::guard(&actor, auth.resolve(&token), response)
        })
        .await;
    delivery::publish(result)
}
