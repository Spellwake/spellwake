//! Current participant delivery; never advances a move or accepts client artifacts.
use super::*;
use crate::{native_game::NativeReadOutcome, player_wire::PlayerViewRequest};

pub(super) async fn read(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let (token, query) =
        match native_delivery_routes::parse::<PlayerViewRequest>(path, request, |q, game| {
            q.game_id == game && q.validate().is_ok()
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
                Err(error) => return Err(Box::new(actor_error(error))),
            };
            let actor = match auth.resolve(&token) {
                Ok(actor) => actor,
                Err(error) => return Err(Box::new(actor_error(error))),
            };
            if actor.account().is_none() {
                return Err(Box::new(fail(
                    StatusCode::CONFLICT,
                    "account",
                    "unavailable",
                )));
            }
            let Some(runtime) = &config.native_views else {
                return Err(Box::new(unavailable()));
            };
            let repository = match native_delivery_routes::repository(
                config,
                "spellwake-native-view-delivery",
            ) {
                Ok(repo) => repo,
                Err(response) => return Err(response),
            };
            match runtime.with_human_view(&repository, &actor, &query, |_, _, issued| {
                // Renew the ORIGINAL bearer after private recovery and issuance.
                // The shared scope then performs its last complete SQL source check.
                Ok((issued.into_snapshot(), auth.resolve(&token)))
            }) {
                Ok(NativeReadOutcome::Ready((snapshot, fresh))) => {
                    #[derive(Serialize)]
                    struct View {
                        snapshot: Box<crate::player_wire::PlayerSnapshot>,
                    }
                    let response = success(View {
                        snapshot: Box::new(snapshot),
                    });
                    native_delivery_routes::guard(&actor, fresh, response)
                }
                Ok(NativeReadOutcome::Refused(reason)) => {
                    Err(Box::new(native_delivery_routes::refusal(reason)))
                }
                Err(_) => Err(Box::new(unavailable())),
            }
        })
        .await;
    native_delivery_routes::publish(result)
}
