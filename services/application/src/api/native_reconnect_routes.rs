//! Independently observed issued snapshot/cursor and original receipts, never a resubmission.
use super::native_delivery_routes as delivery;
use super::*;
use crate::{
    native_game::NativeReadOutcome,
    player_wire::{PlayerReconnect, PlayerReconnectRequest, PlayerViewRequest},
};

pub(super) async fn read(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let (token, query) =
        match delivery::parse::<PlayerReconnectRequest>(path, request, |q, game| {
            q.receipts.game_id == game && q.receipts.validate().is_ok()
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
                return Err(Box::new(fail(
                    StatusCode::CONFLICT,
                    "account",
                    "unavailable",
                )));
            }
            let Some(runtime) = &config.native_views else {
                return Err(Box::new(unavailable()));
            };
            let repository = match delivery::repository(config, "spellwake-native-reconnect") {
                Ok(repo) => repo,
                Err(response) => return Err(response),
            };
            let scope = PlayerViewRequest {
                game_id: query.receipts.game_id,
                context_id: query.receipts.context_id,
            };
            match runtime.with_human_view(&repository, &actor, &scope, |access, _, issued| {
                let previous = match &query.cursor {
                    None => None,
                    Some(requested) => match repository.read_human_cursor(access, requested)? {
                        Some(previous) => Some(previous),
                        None => {
                            return Ok((
                                fail(StatusCode::CONFLICT, "cursor", "unavailable"),
                                auth.resolve(&token),
                            ));
                        }
                    },
                };
                let previous_public = previous.as_ref().map(|source| source.cursor());
                if query
                    .validate(access.viewer_seat_id(), previous_public.as_ref())
                    .is_err()
                {
                    return Ok((unavailable(), auth.resolve(&token)));
                }
                let cursor = repository.issue_human_snapshot_cursor(&issued)?;
                cursor.validate_current(access)?;
                let public = cursor.cursor();
                if previous_public
                    .as_ref()
                    .is_some_and(|previous| public.validate_progress_from(previous).is_err())
                {
                    return Ok((
                        fail(StatusCode::CONFLICT, "cursor", "unavailable"),
                        auth.resolve(&token),
                    ));
                }
                let response = match repository.read_human_receipts(&actor, &query.receipts)? {
                    ApiResponse::Success { data } => {
                        if query
                            .receipts
                            .validate_receipts(&data, access.viewer_seat_id())
                            .is_err()
                            || public.basis != issued.view().snapshot().basis
                            || public.through_event_sequence
                                != issued.view().snapshot().through_event_sequence
                        {
                            unavailable()
                        } else {
                            // The owned snapshot is moved ONLY from the sealed actual SQL-winning
                            // View. Its loader already checked native projection/registry facts;
                            // never manufacture PlayerProjectionFacts from response/request copies.
                            success(PlayerReconnect {
                                snapshot: issued.into_snapshot(),
                                cursor: public,
                                receipts: data,
                            })
                        }
                    }
                    ApiResponse::Fail { data } => native_receipt_routes::failure(data),
                    ApiResponse::Error { .. } => unavailable(),
                };
                // The common View scope renews the complete native source AFTER this
                // Auth call, including error branches. No cursor becomes authority.
                Ok((response, auth.resolve(&token)))
            }) {
                Ok(NativeReadOutcome::Ready((response, fresh))) => {
                    delivery::guard(&actor, fresh, response)
                }
                Ok(NativeReadOutcome::Refused(reason)) => Err(Box::new(delivery::refusal(reason))),
                Err(_) => Err(Box::new(unavailable())),
            }
        })
        .await;
    delivery::publish(result)
}
