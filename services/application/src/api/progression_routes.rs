//! Caller-bound stored progression reads; no generation or scoring side effects.
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
struct RankQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    format_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    season_id: Option<Uuid>,
}
pub(super) async fn quests(State(state): State<ApiState>, request: Request) -> Response {
    read(state, request, false).await
}
pub(super) async fn seasons(State(state): State<ApiState>, request: Request) -> Response {
    read(state, request, true).await
}
async fn read(state: ApiState, request: Request, seasons: bool) -> Response {
    let token = match bearer(request.headers()) {
        Ok(value) => value,
        Err(error) => return actor_error(error),
    };
    let query = match Query::<PageQuery>::try_from_uri(request.uri()) {
        Ok(Query(value)) => value,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) || query.after.is_some_and(|id| id.is_nil()) {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            let client = match config.actor() {
                Ok(client) => client,
                Err(error) => return actor_error(error),
            };
            if seasons {
                economy_routes::projection(client.seasons(&token, query.after, limit), None)
            } else {
                economy_routes::projection(client.quests(&token, query.after, limit), None)
            }
        })
        .await
}
pub(super) async fn ranks(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(value) => value,
        Err(error) => return actor_error(error),
    };
    let query = match Query::<RankQuery>::try_from_uri(request.uri()) {
        Ok(Query(value)) => value,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit)
        || [query.after, query.format_id, query.season_id]
            .into_iter()
            .flatten()
            .any(|id| id.is_nil())
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            economy_routes::projection(
                config.actor().and_then(|client| {
                    client.ranks(&token, query.after, limit, query.format_id, query.season_id)
                }),
                None,
            )
        })
        .await
}
