//! Original-bearer public terms and caller-owned orders/positions.
use super::{command_routes::private, economy_routes::projection, *};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CommitmentQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    market_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
pub(super) async fn market(
    State(state): State<ApiState>,
    market: IdentifierPath,
    request: Request,
) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let market = match identifier(market) {
        Ok(id) => id,
        Err(_) => return private(fail(StatusCode::BAD_REQUEST, "market", "invalid")),
    };
    if request.uri().query().is_some() {
        return private(fail(StatusCode::BAD_REQUEST, "query", "unsupported"));
    }
    state
        .blocking(move |config| {
            projection(
                config
                    .actor()
                    .and_then(|client| client.prediction_market(&token, market)),
                Some("market"),
            )
        })
        .await
}
pub(super) async fn markets(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let query = match Query::<PageQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        _ => return private(fail(StatusCode::BAD_REQUEST, "query", "invalid")),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) || query.after.is_some_and(|id| id.is_nil()) {
        return private(fail(StatusCode::BAD_REQUEST, "query", "invalid"));
    }
    state
        .blocking(move |config| {
            projection(
                config
                    .actor()
                    .and_then(|client| client.prediction_markets(&token, query.after, limit)),
                None,
            )
        })
        .await
}
pub(super) async fn orders(State(state): State<ApiState>, request: Request) -> Response {
    commitments(state, request, false).await
}
pub(super) async fn positions(State(state): State<ApiState>, request: Request) -> Response {
    commitments(state, request, true).await
}
async fn commitments(state: ApiState, request: Request, positions: bool) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let query = match Query::<CommitmentQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        _ => return private(fail(StatusCode::BAD_REQUEST, "query", "invalid")),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit)
        || query.after.is_some_and(|id| id.is_nil())
        || query.market_id.is_some_and(|id| id.is_nil())
    {
        return private(fail(StatusCode::BAD_REQUEST, "query", "invalid"));
    }
    state
        .blocking(move |config| {
            if positions {
                projection(
                    config.actor().and_then(|client| {
                        client.prediction_positions(&token, query.market_id, query.after, limit)
                    }),
                    None,
                )
            } else {
                projection(
                    config.actor().and_then(|client| {
                        client.prediction_orders(&token, query.market_id, query.after, limit)
                    }),
                    None,
                )
            }
        })
        .await
}

pub(super) async fn publication_source(
    State(state): State<ApiState>,
    printing: IdentifierPath,
    request: Request,
) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let printing = match identifier(printing) {
        Ok(id) => id,
        Err(_) => return private(fail(StatusCode::BAD_REQUEST, "printing", "invalid")),
    };
    if request.uri().query().is_some() {
        return private(fail(StatusCode::BAD_REQUEST, "query", "unsupported"));
    }
    state
        .blocking(move |config| {
            source_projection(
                config
                    .actor()
                    .and_then(|client| client.prediction_publication_source(&token, printing)),
            )
        })
        .await
}

/// Preparation only extracts from an already accepted retained source; it never acquires a feed.
pub(super) async fn prepare_source(
    State(state): State<ApiState>,
    printing: IdentifierPath,
    request: Request,
) -> Response {
    private(prepare_source_inner(state, printing, request).await)
}
async fn prepare_source_inner(
    state: ApiState,
    printing: IdentifierPath,
    request: Request,
) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return actor_error(error),
    };
    let printing = match identifier(printing) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "printing", "invalid"),
    };
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    if !single_content_type(request.headers()).is_some_and(|v| v == "application/json") {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), 64).await {
        Ok(bytes) => bytes,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if !matches!(serde_json::from_slice::<Value>(&bytes), Ok(Value::Object(ref map)) if map.is_empty())
    {
        return fail(StatusCode::BAD_REQUEST, "body", "invalid");
    }
    let Ok(permit) = state.permits.clone().try_acquire_owned() else {
        return unavailable();
    };
    let config = state.config.clone();
    // Keep admission owned by each blocking operation even if its HTTP future is dropped.
    let resolved = tokio::task::spawn_blocking(move || {
        let basis = config
            .actor()
            .and_then(|client| client.prediction_probe_basis(&token, printing));
        (permit, config, token, basis)
    })
    .await;
    let (permit, config, token, basis) = match resolved {
        Ok((permit, config, token, Ok(ApiResponse::Success { data }))) => {
            (permit, config, token, data)
        }
        Ok((_, _, _, other)) => return source_projection(other),
        Err(_) => return unavailable(),
    };
    let runtime = match config.temporal.clone() {
        Some(runtime) => runtime,
        None => return unavailable(),
    };
    let prepared = match runtime.connect("spellwake-api-price-probe").await {
        Ok(client) => crate::price_workflow::probe::ensure_probe(&client, basis.input()).await,
        Err(_) => return unavailable(),
    };
    let prepared = match prepared {
        Ok(status) => status,
        Err(_) => return unavailable(),
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        // Shared public source work can finish after logout; revalidate delivery authority.
        match config.actor().and_then(|client| client.resolve(&token)) {
            Ok(actor)
                if actor.account() == Some(basis.account_id)
                    && actor.state() == crate::actor::ActorState::Active =>
            {
                let code = if prepared == crate::price_workflow::probe::ProbeStatus::Requested {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::OK
                };
                (
                    code,
                    Json(ApiResponse::Success {
                        data: crate::prediction_publication::SourcePreparation {
                            account_id: basis.account_id,
                            printing_id: printing,
                            slot: basis.basis.slot,
                            state: prepared,
                        },
                    }),
                )
                    .into_response()
            }
            Ok(_) => fail(StatusCode::CONFLICT, "account", "unavailable"),
            Err(error) => actor_error(error),
        }
    })
    .await
    .unwrap_or_else(|_| unavailable())
}
fn source_projection<T: Serialize>(result: Result<ApiResponse<T>, ActorError>) -> Response {
    match result {
        Ok(ApiResponse::Fail { data }) if data.get("price").is_some_and(|v| v == "unavailable") => {
            private(fail(StatusCode::CONFLICT, "price", "unavailable"))
        }
        other => projection(other, Some("printing")),
    }
}
