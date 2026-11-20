//! Demand-driven current snapshots. No publisher task, catch-up queue or gameplay writes.
use super::native_delivery_routes as delivery;
use super::*;
use crate::{
    cosmetics::AppearanceSelection,
    native_game::{
        IssuedNativeCursor, NativeHumanAccess, NativeHumanAccessOutcome, NativePresentationOutcome,
        NativeReadOutcome, NativeSourceRepository,
    },
    player_wire::{
        MAX_PLAYER_WIRE_BYTES, PlayerCursor, PlayerStreamFrame, PlayerStreamRequest,
        PlayerViewRequest,
    },
};
use axum::body::{Body, Bytes};
use futures::stream;
use std::{convert::Infallible, io::Write};
use tokio::sync::OwnedSemaphorePermit;

const MAX_FRAME_BYTES: usize = 2 * MAX_PLAYER_WIRE_BYTES;
type Failure = Box<Response>;
type Outcome<T> = Result<T, Failure>;

/// Constructed only from the actual scoped SQL-winning View and its source operands.
/// No borrowed native State escapes; public records cannot construct this private proof.
struct Prepared {
    frame: PlayerStreamFrame,
    access: NativeHumanAccess,
    roster: Vec<Uuid>,
    cursor: IssuedNativeCursor,
    actor: RequestActor,
}
struct Delivered {
    bytes: Bytes,
    cursor: IssuedNativeCursor,
    actor: RequestActor,
}
enum Previous {
    Requested(Option<PlayerCursor>),
    Yielded(IssuedNativeCursor),
}
struct StreamState {
    api: ApiState,
    token: String,
    scope: PlayerViewRequest,
    first: Option<Prepared>,
    previous: Option<IssuedNativeCursor>,
    // This belongs to the body state, never to an uncancellable blocking closure.
    _lifetime: OwnedSemaphorePermit,
}

pub(super) async fn read(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let (token, query) = match delivery::parse::<PlayerStreamRequest>(path, request, |q, game| {
        q.game_id == game && scope(q).validate().is_ok()
    })
    .await
    {
        Ok(parsed) => parsed,
        Err(response) => return *response,
    };
    let Ok(lifetime) = state.stream_permits.clone().try_acquire_owned() else {
        return unavailable();
    };
    let selected = scope(&query);
    let work_scope = selected.clone();
    let work_token = token.clone();
    let first = match state
        .blocking_value(move |config| {
            prepare(
                config,
                &work_token,
                &work_scope,
                Previous::Requested(query.cursor),
            )
        })
        .await
    {
        Ok(Ok(prepared)) => prepared,
        Ok(Err(response)) => return *response,
        Err(()) => return unavailable(),
    };
    // The completed worker may wait for this route to be polled. Expiry here
    // precedes headers and therefore remains an ordinary unauthorized JSON reply.
    if let Err(error) = first.actor.ensure_unexpired() {
        return actor_error(error);
    }
    let body = StreamState {
        api: state,
        token,
        scope: selected,
        first: Some(first),
        previous: None,
        _lifetime: lifetime,
    };
    (
        [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Body::from_stream(stream::unfold(Some(body), next)),
    )
        .into_response()
}
fn scope(request: &PlayerStreamRequest) -> PlayerViewRequest {
    PlayerViewRequest {
        game_id: request.game_id,
        context_id: request.context_id,
    }
}
fn unavailable_failure() -> Failure {
    Box::new(unavailable())
}
fn cursor_failure() -> Failure {
    Box::new(fail(StatusCode::CONFLICT, "cursor", "unavailable"))
}
fn stale() -> Failure {
    Box::new(fail(StatusCode::CONFLICT, "basis", "stale"))
}
fn free(config: &ApiConfig) -> AppearanceSelection {
    AppearanceSelection {
        back_id: config.defaults[0],
        sleeve_id: config.defaults[1],
        dice_id: config.defaults[2],
        playmat_id: None,
    }
}
fn fresh_actor(auth: &ActorClient, token: &str, previous: &RequestActor) -> Outcome<RequestActor> {
    delivery::verified_actor(previous, auth.resolve(token)).map_err(|e| Box::new(actor_error(e)))
}
fn presentation(
    repository: &NativeSourceRepository,
    actor: &RequestActor,
    access: &NativeHumanAccess,
    roster: &[Uuid],
    configured_free: &AppearanceSelection,
) -> Outcome<crate::player_wire::PlayerPresentation> {
    let basis = access.basis();
    match repository
        .human_presentation(actor, basis.game_id, basis.context_id, configured_free)
        .map_err(|_| unavailable_failure())?
    {
        NativePresentationOutcome::Observed(observed) => {
            if !observed.matches_access(access) {
                return Err(stale());
            }
            observed
                .validate_against(access, roster)
                .map_err(|_| unavailable_failure())?;
            Ok(observed.presentation().clone())
        }
        NativePresentationOutcome::Unauthorized => {
            Err(Box::new(actor_error(ActorError::Unauthorized)))
        }
        NativePresentationOutcome::GameUnavailable => {
            Err(Box::new(fail(StatusCode::NOT_FOUND, "game", "unavailable")))
        }
        NativePresentationOutcome::PresentationUnavailable => Err(unavailable_failure()),
    }
}
fn previous(
    repository: &NativeSourceRepository,
    access: &NativeHumanAccess,
    previous: Previous,
) -> Outcome<Option<IssuedNativeCursor>> {
    let source = match previous {
        Previous::Requested(None) => return Ok(None),
        Previous::Requested(Some(requested)) => repository
            .read_human_cursor(access, &requested)
            .map_err(|_| unavailable_failure())?
            .ok_or_else(cursor_failure)?,
        Previous::Yielded(source) => source,
    };
    source
        .validate_current(access)
        .map_err(|_| cursor_failure())?;
    Ok(Some(source))
}
fn prepare(
    config: &ApiConfig,
    token: &str,
    scope: &PlayerViewRequest,
    prior: Previous,
) -> Outcome<Prepared> {
    let auth = config.actor().map_err(|e| Box::new(actor_error(e)))?;
    let actor = auth.resolve(token).map_err(|e| Box::new(actor_error(e)))?;
    if actor.account().is_none() {
        return Err(Box::new(fail(
            StatusCode::CONFLICT,
            "account",
            "unavailable",
        )));
    }
    let runtime = config
        .native_views
        .as_ref()
        .ok_or_else(unavailable_failure)?;
    let repository = delivery::repository(config, "spellwake-native-stream")?;
    let result = runtime
        .with_human_view(&repository, &actor, scope, |access, recovered, issued| {
            let composed = (|| {
                let prior = previous(&repository, access, prior)?;
                let cursor = repository
                    .issue_human_snapshot_cursor(&issued)
                    .map_err(|_| unavailable_failure())?;
                cursor
                    .validate_current(access)
                    .map_err(|_| unavailable_failure())?;
                let public = cursor.cursor();
                if let Some(prior) = prior {
                    public
                        .validate_progress_from(&prior.cursor())
                        .map_err(|_| cursor_failure())?;
                }
                if public.basis != issued.view().snapshot().basis
                    || public.through_event_sequence
                        != issued.view().snapshot().through_event_sequence
                {
                    return Err(unavailable_failure());
                }
                let roster: Vec<_> = recovered
                    .source()
                    .controllers()
                    .map(|(seat, _, _)| seat)
                    .collect();
                // All expensive native/View/cursor IO precedes this coherent current observation.
                let appearance = presentation(&repository, &actor, access, &roster, &free(config))?;
                let frame = PlayerStreamFrame::Snapshot {
                    snapshot: Box::new(issued.into_snapshot()),
                    cursor: public,
                    presentation: appearance,
                };
                // Before headers, validate the actual complete record's serialized bound.
                encode(&frame)?;
                Ok((frame, access.clone(), roster, cursor))
            })();
            // Even a composition refusal renews the original bearer. The shared scope
            // performs complete native access renewal AFTER this real Auth observation.
            Ok((composed, fresh_actor(&auth, token, &actor)))
        })
        .map_err(|_| unavailable_failure())?;
    match result {
        NativeReadOutcome::Refused(reason) => Err(Box::new(delivery::refusal(reason))),
        NativeReadOutcome::Ready((composed, fresh)) => {
            let fresh = fresh?;
            fresh
                .ensure_unexpired()
                .map_err(|e| Box::new(actor_error(e)))?;
            let (frame, access, roster, cursor) = composed?;
            Ok(Prepared {
                frame,
                access,
                roster,
                cursor,
                actor: fresh,
            })
        }
    }
}
/// Reobserve appearance and authority for the exact prepared cut; never silently replace it.
fn renew(config: &ApiConfig, token: &str, mut prepared: Prepared) -> Outcome<Delivered> {
    let auth = config.actor().map_err(|e| Box::new(actor_error(e)))?;
    let repository = delivery::repository(config, "spellwake-native-stream-renew")?;
    let appearance = presentation(
        &repository,
        &prepared.actor,
        &prepared.access,
        &prepared.roster,
        &free(config),
    );
    let fresh = fresh_actor(&auth, token, &prepared.actor)?;
    let basis = prepared.access.basis();
    match repository
        .human_access(&fresh, basis.game_id, basis.context_id)
        .map_err(|_| unavailable_failure())?
    {
        NativeHumanAccessOutcome::Granted(current)
            if prepared.access.same_delivery_source(&current) => {}
        NativeHumanAccessOutcome::Granted(_) => return Err(stale()),
        NativeHumanAccessOutcome::Unauthorized => {
            return Err(Box::new(actor_error(ActorError::Unauthorized)));
        }
        NativeHumanAccessOutcome::Unavailable => {
            return Err(Box::new(fail(StatusCode::NOT_FOUND, "game", "unavailable")));
        }
    }
    let PlayerStreamFrame::Snapshot { presentation, .. } = &mut prepared.frame else {
        return Err(unavailable_failure());
    };
    *presentation = appearance?;
    finish(prepared.frame, prepared.cursor, fresh)
}
fn finish(
    frame: PlayerStreamFrame,
    cursor: IssuedNativeCursor,
    actor: RequestActor,
) -> Outcome<Delivered> {
    let bytes = encode(&frame)?;
    actor
        .ensure_unexpired()
        .map_err(|e| Box::new(actor_error(e)))?;
    Ok(Delivered {
        bytes,
        cursor,
        actor,
    })
}
async fn next(
    state: Option<StreamState>,
) -> Option<(Result<Bytes, Infallible>, Option<StreamState>)> {
    let mut state = state?;
    let token = state.token.clone();
    let delivery = if let Some(first) = state.first.take() {
        // Deferred until the FIRST body poll, even after a successful preflight.
        state
            .api
            .blocking_value(move |config| renew(config, &token, first))
            .await
    } else {
        // A demand-driven wait starts after the preceding yield; no work permit or
        // running producer is retained, and dropping this future schedules no work.
        tokio::time::sleep(Duration::from_secs(1)).await;
        let prior = state.previous.take()?;
        let scope = state.scope.clone();
        state
            .api
            .blocking_value(move |config| {
                let prepared = prepare(config, &token, &scope, Previous::Yielded(prior))?;
                finish(prepared.frame, prepared.cursor, prepared.actor)
            })
            .await
    };
    let delivered = match delivery {
        Ok(Ok(delivered)) => delivered,
        Ok(Err(failure)) => return Some((Ok(terminal(*failure).await), None)),
        Err(()) => return Some((Ok(terminal(unavailable()).await), None)),
    };
    // A blocking result may wait to be polled. Check again after that scheduling
    // delay, immediately before server application yield, without another await.
    if let Err(error) = delivered.actor.ensure_unexpired() {
        return Some((Ok(terminal(actor_error(error)).await), None));
    }
    state.previous = Some(delivered.cursor);
    Some((Ok(delivered.bytes), Some(state)))
}
struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_FRAME_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("stream record limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode(frame: &PlayerStreamFrame) -> Outcome<Bytes> {
    let mut bytes = BoundedBytes(Vec::new());
    serde_json::to_writer(&mut bytes, &ApiResponse::Success { data: frame })
        .map_err(|_| unavailable_failure())?;
    Ok(sse(&bytes.0))
}
fn sse(json: &[u8]) -> Bytes {
    let mut bytes = Vec::with_capacity(json.len() + 8);
    bytes.extend_from_slice(b"data: ");
    bytes.extend_from_slice(json);
    bytes.extend_from_slice(b"\n\n");
    bytes.into()
}
async fn terminal(response: Response) -> Bytes {
    // Only this module's already-safe ordinary API responses enter this adapter.
    match to_bytes(response.into_body(), MAX_FRAME_BYTES).await {
        Ok(bytes) => sse(&bytes),
        Err(_) => sse(br#"{"status":"error","message":"Application service is temporarily unavailable","code":"unavailable"}"#),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialization_limit_is_atomic_and_sse_uses_only_a_data_record() {
        let mut writer = BoundedBytes(Vec::new());
        writer.write_all(&vec![b'x'; MAX_FRAME_BYTES]).unwrap();
        assert!(writer.write_all(b"y").is_err());
        assert_eq!(writer.0.len(), MAX_FRAME_BYTES);
        let json = br#"{"status":"fail","data":{"session":"unauthorized"}}"#;
        let record = sse(json);
        assert_eq!(&record[..6], b"data: ");
        assert_eq!(&record[6..record.len() - 2], json);
        assert_eq!(&record[record.len() - 2..], b"\n\n");
    }
    #[tokio::test]
    async fn terminal_preserves_the_safe_api_envelope() {
        let record = terminal(fail(StatusCode::CONFLICT, "basis", "stale")).await;
        let value: Value = serde_json::from_slice(&record[6..record.len() - 2]).unwrap();
        assert_eq!(value, json!({"status":"fail","data":{"basis":"stale"}}));
    }
}
