//! Original bearer room discovery and single-snapshot slot pages.
use super::*;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PageQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SlotQuery {
    #[serde(
        default,
        alias = "after_slot",
        deserialize_with = "crate::identifiers::nullable_uuid"
    )]
    after_slot: Option<Uuid>,
    #[serde(alias = "slot_limit")]
    slot_limit: Option<usize>,
}
pub(super) async fn list(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<PageQuery>::try_from_uri(request.uri()) {
        Ok(Query(v)) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) || query.after.is_some_and(|id| id.is_nil()) {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            economy_routes::projection(
                config
                    .actor()
                    .and_then(|client| client.rooms(&token, query.after, limit)),
                None,
            )
        })
        .await
}
pub(super) async fn detail(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let room = match identifier(path) {
        Ok(room) => room,
        Err(reason) => return fail(StatusCode::BAD_REQUEST, "room", reason),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<SlotQuery>::try_from_uri(request.uri()) {
        Ok(Query(v)) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.slot_limit.unwrap_or(50);
    if room.is_nil()
        || !(1..=100).contains(&limit)
        || query.after_slot.is_some_and(|id| id.is_nil())
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            economy_routes::projection(
                config
                    .actor()
                    .and_then(|client| client.room(&token, room, query.after_slot, limit)),
                Some("room"),
            )
        })
        .await
}
