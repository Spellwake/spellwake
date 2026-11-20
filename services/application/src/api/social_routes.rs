//! Session-bound facades over the canonical social writers and exact receipts.
use super::command_routes::private;
use super::*;
use crate::social::{DIRECT_MESSAGE_PAGE_LIMIT, MessageCursor};
use chrono::{DateTime, Utc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PeopleQuery {
    query: Option<String>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    person_id: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageQuery {
    before_at: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    before_id: Option<Uuid>,
    limit: Option<usize>,
}

fn projection<T: Serialize>(result: Result<ApiResponse<T>, ActorError>) -> Response {
    private(match result {
        Ok(ApiResponse::Success { data }) => success(data),
        Ok(ApiResponse::Fail { data }) if data.contains_key("conversation") => {
            fail(StatusCode::NOT_FOUND, "conversation", "unavailable")
        }
        Ok(ApiResponse::Fail { .. }) => fail(StatusCode::CONFLICT, "account", "unavailable"),
        Ok(ApiResponse::Error { .. }) => unavailable(),
        Err(error) => actor_error(error),
    })
}

pub(super) async fn people(State(state): State<ApiState>, input: Request) -> Response {
    read_people(state, input, false).await
}
pub(super) async fn status(State(state): State<ApiState>, input: Request) -> Response {
    let token = match bearer(input.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let query = match Query::<StatusQuery>::try_from_uri(input.uri()) {
        Ok(Query(query)) if !query.person_id.is_some_and(|id| id.is_nil()) => query,
        _ => return private(fail(StatusCode::BAD_REQUEST, "query", "invalid")),
    };
    state
        .blocking(move |config| {
            projection(
                config
                    .actor()
                    .and_then(|client| client.social_status(&token, query.person_id)),
            )
        })
        .await
}
pub(super) async fn threads(State(state): State<ApiState>, input: Request) -> Response {
    read_people(state, input, true).await
}
async fn read_people(state: ApiState, input: Request, threads: bool) -> Response {
    let token = match bearer(input.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let query = match Query::<PeopleQuery>::try_from_uri(input.uri()) {
        Ok(Query(query)) => query,
        _ => return private(fail(StatusCode::BAD_REQUEST, "query", "invalid")),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit)
        || query.after.is_some_and(|id| id.is_nil())
        || threads == query.query.is_some()
        || query.query.as_ref().is_some_and(|query| {
            query.contains('\0') || !(1..=64).contains(&query.trim_matches(' ').chars().count())
        })
    {
        return private(fail(StatusCode::BAD_REQUEST, "query", "invalid"));
    }
    state
        .blocking(move |config| {
            projection(config.actor().and_then(|client| {
                client.social_people(&token, query.query.as_deref(), query.after, limit)
            }))
        })
        .await
}

pub(super) async fn messages(
    State(state): State<ApiState>,
    conversation: IdentifierPath,
    input: Request,
) -> Response {
    let token = match bearer(input.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let conversation = match identifier(conversation) {
        Ok(id) => id,
        Err(_) => return private(fail(StatusCode::BAD_REQUEST, "conversation", "invalid")),
    };
    let query = match Query::<MessageQuery>::try_from_uri(input.uri()) {
        Ok(Query(query)) => query,
        _ => return private(fail(StatusCode::BAD_REQUEST, "query", "invalid")),
    };
    let limit = query.limit.unwrap_or(DIRECT_MESSAGE_PAGE_LIMIT);
    if !(1..=DIRECT_MESSAGE_PAGE_LIMIT).contains(&limit)
        || query.before_at.is_some() != query.before_id.is_some()
        || query.before_id.is_some_and(|id| id.is_nil())
    {
        return private(fail(StatusCode::BAD_REQUEST, "query", "invalid"));
    }
    let before = query
        .before_at
        .zip(query.before_id)
        .map(|(sent_at, id)| MessageCursor { sent_at, id });
    state
        .blocking(move |config| {
            projection(config.actor().and_then(|client| {
                client.direct_messages(&token, conversation, before.as_ref(), limit)
            }))
        })
        .await
}
