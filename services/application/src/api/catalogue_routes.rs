//! Caller-only catalogue reads. These labels confer no inventory or rules support.
use super::*;
use crate::catalogue::{CatalogueLookup, PrintingFilter};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CardQuery {
    #[serde(default)]
    query: String,
    #[serde(default)]
    exact: bool,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NamePrintingQuery {
    name: String,
    set_code: Option<String>,
    collector_number: Option<String>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
fn projection<T: Serialize>(result: Result<ApiResponse<T>, ActorError>) -> Response {
    match result {
        Ok(ApiResponse::Success { data }) => success(data),
        Ok(ApiResponse::Fail { .. }) => fail(StatusCode::CONFLICT, "account", "unavailable"),
        Ok(ApiResponse::Error { .. }) => unavailable(),
        Err(error) => actor_error(error),
    }
}
pub(super) async fn cards(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    let query = match Query::<CardQuery>::try_from_uri(request.uri()) {
        Ok(Query(query)) => query,
        _ => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if query.query.chars().count() > 1024
        || query.query.contains('\0')
        || !(1..=100).contains(&limit)
        || query.after.is_some_and(|id| id.is_nil())
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            projection(config.actor().and_then(|client| {
                client.cards(&token, &query.query, query.exact, query.after, limit)
            }))
        })
        .await
}
pub(super) async fn printings(
    State(state): State<ApiState>,
    definition: IdentifierPath,
    request: Request,
) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    let definition = match identifier(definition) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "definition", "invalid"),
    };
    let query = match Query::<PageQuery>::try_from_uri(request.uri()) {
        Ok(Query(query)) => query,
        _ => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) || query.after.is_some_and(|id| id.is_nil()) {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            projection(config.actor().and_then(|client| {
                client.printings(
                    &token,
                    &PrintingFilter {
                        definition: Some(definition),
                        name: None,
                        set_code: None,
                        collector: None,
                    },
                    query.after,
                    limit,
                )
            }))
        })
        .await
}
pub(super) async fn named_printings(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    let query = match Query::<NamePrintingQuery>::try_from_uri(request.uri()) {
        Ok(Query(query)) => query,
        _ => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    let filter = PrintingFilter {
        definition: None,
        name: Some(&query.name),
        set_code: query.set_code.as_deref(),
        collector: query.collector_number.as_deref(),
    };
    if filter.validate().is_err()
        || !(1..=100).contains(&limit)
        || query.after.is_some_and(|id| id.is_nil())
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            projection(config.actor().and_then(|client| {
                client.printings(
                    &token,
                    &PrintingFilter {
                        definition: None,
                        name: Some(&query.name),
                        set_code: query.set_code.as_deref(),
                        collector: query.collector_number.as_deref(),
                    },
                    query.after,
                    limit,
                )
            }))
        })
        .await
}
pub(super) async fn resolve(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let input: CatalogueLookup = match to_bytes(request.into_body(), 16384)
        .await
        .ok()
        .and_then(|bytes| parse_object(&bytes).ok())
    {
        Some(input) => input,
        None => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.validate().is_err() {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            projection(
                config
                    .actor()
                    .and_then(|client| client.catalogue(&token, &input)),
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetQuery {
    #[serde(default)]
    query: String,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
pub(super) async fn sets(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return super::command_routes::private(actor_error(error)),
    };
    let query = match Query::<SetQuery>::try_from_uri(request.uri()) {
        Ok(Query(query)) => query,
        _ => {
            return super::command_routes::private(fail(
                StatusCode::BAD_REQUEST,
                "query",
                "invalid",
            ));
        }
    };
    let limit = query.limit.unwrap_or(40);
    if query.query.chars().count() > 1024
        || query.query.contains('\0')
        || !(1..=100).contains(&limit)
        || query.after.is_some_and(|id| id.is_nil())
    {
        return super::command_routes::private(fail(StatusCode::BAD_REQUEST, "query", "invalid"));
    }
    state
        .blocking(move |config| {
            super::command_routes::private(projection(
                config
                    .actor()
                    .and_then(|client| client.sets(&token, &query.query, query.after, limit)),
            ))
        })
        .await
}
