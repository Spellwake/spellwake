//! Original-bearer economy projections; no actor-selected reads or private seed.
use super::command_routes::private;
use super::*;
use crate::exchange_reads::{EXCHANGE_PAGE_LIMIT, TradeState};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TradeQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
    state: Option<TradeState>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarketQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
    owned: Option<bool>,
}
pub(super) fn projection<T: Serialize>(
    result: Result<ApiResponse<T>, ActorError>,
    missing: Option<&'static str>,
) -> Response {
    private(match result {
        Ok(ApiResponse::Success { data }) => success(data),
        Ok(ApiResponse::Fail { data }) => match missing.filter(|key| data.contains_key(*key)) {
            Some(key) => fail(StatusCode::NOT_FOUND, key, "unavailable"),
            None => fail(StatusCode::CONFLICT, "account", "unavailable"),
        },
        Ok(ApiResponse::Error { .. }) => unavailable(),
        Err(error) => actor_error(error),
    })
}
pub(super) async fn trade(
    State(state): State<ApiState>,
    id: IdentifierPath,
    request: Request,
) -> Response {
    detail(state, id, request, false).await
}
pub(super) async fn listing(
    State(state): State<ApiState>,
    id: IdentifierPath,
    request: Request,
) -> Response {
    detail(state, id, request, true).await
}
async fn detail(state: ApiState, id: IdentifierPath, request: Request, market: bool) -> Response {
    let field = if market { "listing" } else { "trade" };
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let id = match identifier(id) {
        Ok(id) => id,
        Err(_) => return private(fail(StatusCode::BAD_REQUEST, field, "invalid")),
    };
    if request.uri().query().is_some() {
        return private(fail(StatusCode::BAD_REQUEST, "query", "unsupported"));
    }
    state
        .blocking(move |config| {
            if market {
                projection(
                    config
                        .actor()
                        .and_then(|client| client.market_listing(&token, id)),
                    Some("listing"),
                )
            } else {
                projection(
                    config.actor().and_then(|client| client.trade(&token, id)),
                    Some("trade"),
                )
            }
        })
        .await
}
pub(super) async fn trades(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let query = match Query::<TradeQuery>::try_from_uri(request.uri()) {
        Ok(Query(query)) => query,
        _ => return private(fail(StatusCode::BAD_REQUEST, "query", "invalid")),
    };
    let limit = query.limit.unwrap_or(EXCHANGE_PAGE_LIMIT);
    if !(1..=EXCHANGE_PAGE_LIMIT).contains(&limit) || query.after.is_some_and(|id| id.is_nil()) {
        return private(fail(StatusCode::BAD_REQUEST, "query", "invalid"));
    }
    state
        .blocking(move |config| {
            projection(
                config
                    .actor()
                    .and_then(|client| client.trades(&token, query.after, limit, query.state)),
                None,
            )
        })
        .await
}
pub(super) async fn market(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let query = match Query::<MarketQuery>::try_from_uri(request.uri()) {
        Ok(Query(query)) => query,
        _ => return private(fail(StatusCode::BAD_REQUEST, "query", "invalid")),
    };
    let limit = query.limit.unwrap_or(EXCHANGE_PAGE_LIMIT);
    if !(1..=EXCHANGE_PAGE_LIMIT).contains(&limit) || query.after.is_some_and(|id| id.is_nil()) {
        return private(fail(StatusCode::BAD_REQUEST, "query", "invalid"));
    }
    state
        .blocking(move |config| {
            projection(
                config.actor().and_then(|client| {
                    client.market(&token, query.after, limit, query.owned.unwrap_or(false))
                }),
                None,
            )
        })
        .await
}

pub(super) async fn pack_outcome(
    State(state): State<ApiState>,
    opened: IdentifierPath,
    request: Request,
) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let opened = match identifier(opened) {
        Ok(id) => id,
        Err(_) => return private(fail(StatusCode::BAD_REQUEST, "pack", "invalid")),
    };
    if request.uri().query().is_some() {
        return private(fail(StatusCode::BAD_REQUEST, "query", "unsupported"));
    }
    state
        .blocking(move |config| {
            projection(
                config
                    .actor()
                    .and_then(|client| client.pack_outcome(&token, opened)),
                Some("pack"),
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackProductsQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackQuoteQuery {
    quantity: Option<u16>,
}
pub(super) async fn pack_products(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let query = match Query::<PackProductsQuery>::try_from_uri(request.uri()) {
        Ok(Query(query)) => query,
        _ => return private(fail(StatusCode::BAD_REQUEST, "query", "invalid")),
    };
    let limit = query
        .limit
        .unwrap_or(crate::pack_shop::PACK_PRODUCT_PAGE_LIMIT);
    if !(1..=crate::pack_shop::PACK_PRODUCT_PAGE_LIMIT).contains(&limit)
        || query.after.is_some_and(|id| id.is_nil())
    {
        return private(fail(StatusCode::BAD_REQUEST, "query", "invalid"));
    }
    state
        .blocking(move |config| {
            projection(
                config
                    .actor()
                    .and_then(|client| client.pack_products(&token, query.after, limit)),
                None,
            )
        })
        .await
}
pub(super) async fn pack_quote(
    State(state): State<ApiState>,
    recipe: IdentifierPath,
    request: Request,
) -> Response {
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(error) => return private(actor_error(error)),
    };
    let recipe = match identifier(recipe) {
        Ok(id) => id,
        Err(_) => return private(fail(StatusCode::BAD_REQUEST, "product", "invalid")),
    };
    let query = match Query::<PackQuoteQuery>::try_from_uri(request.uri()) {
        Ok(Query(query)) => query,
        _ => return private(fail(StatusCode::BAD_REQUEST, "query", "invalid")),
    };
    let quantity = match crate::supply::PackQuantity::try_from(crate::amount::Amount::from(
        u64::from(query.quantity.unwrap_or(1)),
    )) {
        Ok(value) => value,
        _ => return private(fail(StatusCode::BAD_REQUEST, "quantity", "invalid")),
    };
    state
        .blocking(move |config| {
            projection(
                config
                    .actor()
                    .and_then(|client| client.pack_quote(&token, recipe, quantity)),
                Some("product"),
            )
        })
        .await
}
