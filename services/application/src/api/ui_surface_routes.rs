//! UI-surface completion routes covering rich card metadata, deck management,
//! cosmetic pricing & pets, profile badge & cross-device Patreon status, League
//! leaderboard & player summaries, searchable market & predictions, bilateral
//! live trade rooms, room game handoff & options, live limited draft, and DM cards.
use super::*;
use crate::commands::CommandAck;

fn relay_rpc(
    result: Result<ApiResponse<Value>, ActorError>,
    not_found_keys: &[&str],
    success_status: StatusCode,
) -> Response {
    match result {
        Ok(ApiResponse::Success { data }) => {
            (success_status, Json(ApiResponse::Success { data })).into_response()
        }
        Ok(ApiResponse::Fail { data }) => {
            if data.len() == 1 && data.get("session").is_some_and(|v| v == "unauthorized") {
                return fail(StatusCode::UNAUTHORIZED, "session", "unauthorized");
            }
            if data.len() == 1
                && not_found_keys
                    .iter()
                    .any(|key| data.get(*key).is_some_and(|v| v == "unavailable"))
            {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse::<Value>::Fail { data }),
                )
                    .into_response();
            }
            if data.len() == 1
                && data
                    .values()
                    .next()
                    .is_some_and(|v| matches!(v.as_str(), "invalid" | "unsupported"))
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse::<Value>::Fail { data }),
                )
                    .into_response();
            }
            (
                StatusCode::CONFLICT,
                Json(ApiResponse::<Value>::Fail { data }),
            )
                .into_response()
        }
        Ok(ApiResponse::Error { .. }) => unavailable(),
        Err(error) => actor_error(error),
    }
}

async fn parse_json_body<T: serde::de::DeserializeOwned>(
    request: Request,
    max_bytes: usize,
) -> Result<(String, T), Response> {
    if request.uri().query().is_some() {
        return Err(fail(StatusCode::BAD_REQUEST, "query", "unsupported"));
    }
    let token = bearer(request.headers()).map_err(actor_error)?;
    if !is_json_content_type(request.headers()) {
        return Err(fail(StatusCode::BAD_REQUEST, "body", "json_required"));
    }
    let bytes = to_bytes(request.into_body(), max_bytes)
        .await
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"))?;
    let parsed: T =
        parse_object(&bytes).map_err(|_| fail(StatusCode::BAD_REQUEST, "body", "invalid"))?;
    Ok((token, parsed))
}

// ---------------------------------------------------------------------------
// 1. Rich card metadata & searchable card catalogue
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CardMetadataRequest {
    #[serde(default)]
    printing_ids: Vec<Uuid>,
    #[serde(default)]
    definition_ids: Vec<Uuid>,
}

pub(super) async fn card_metadata(State(state): State<ApiState>, request: Request) -> Response {
    let (token, input) = match parse_json_body::<CardMetadataRequest>(request, 16384).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if input.printing_ids.len() > 100
        || input.definition_ids.len() > 100
        || (input.printing_ids.is_empty() && input.definition_ids.is_empty())
        || input.printing_ids.iter().any(Uuid::is_nil)
        || input.definition_ids.iter().any(Uuid::is_nil)
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "read_card_metadata",
                        &json!({
                            "p_printing_ids": input.printing_ids,
                            "p_definition_ids": input.definition_ids,
                        }),
                    )
                }),
                &[],
                StatusCode::OK,
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RichCardSearchQuery {
    #[serde(default)]
    query: String,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    format_id: Option<Uuid>,
    rarity: Option<String>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    set_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}

pub(super) async fn search_cards_rich(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<RichCardSearchQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if query.query.chars().count() > 256
        || query.query.contains('\0')
        || !(1..=100).contains(&limit)
        || [query.format_id, query.set_id, query.after]
            .into_iter()
            .flatten()
            .any(|id| id.is_nil())
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "search_cards_rich",
                        &json!({
                            "p_query": query.query,
                            "p_format": query.format_id,
                            "p_rarity": query.rarity,
                            "p_set_id": query.set_id,
                            "p_after": query.after,
                            "p_limit": limit,
                        }),
                    )
                }),
                &[],
                StatusCode::OK,
            )
        })
        .await
}

// ---------------------------------------------------------------------------
// 2. Deck delete & duplicate
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteDeckQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    request_id: Option<Uuid>,
}

pub(super) async fn delete_deck(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let deck = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "deck", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<DeleteDeckQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let request_id = query.request_id.unwrap_or_else(Uuid::new_v4);
    if deck.is_nil() || request_id.is_nil() {
        return fail(StatusCode::BAD_REQUEST, "deck", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let backend = match config.backend("spellwake-deck-delete") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<CommandAck>, _> = backend.rpc(
                "delete_deck_command",
                &json!({
                    "p_subject": actor.subject(),
                    "p_session": actor.session(),
                    "p_request": request_id,
                    "p_deck": deck,
                }),
            );
            match result {
                Ok(ApiResponse::Success { data }) => success(data),
                Ok(ApiResponse::Fail { data }) => {
                    if data.get("session").is_some_and(|v| v == "unauthorized") {
                        fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
                    } else if data.get("deck").is_some_and(|v| v == "deck unavailable") {
                        fail(StatusCode::NOT_FOUND, "deck", "unavailable")
                    } else {
                        (
                            StatusCode::CONFLICT,
                            Json(ApiResponse::<Value>::Fail { data }),
                        )
                            .into_response()
                    }
                }
                _ => unavailable(),
            }
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DuplicateDeckRequest {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    request_id: Option<Uuid>,
    name: Option<String>,
}

pub(super) async fn duplicate_deck(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let deck = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "deck", "invalid"),
    };
    let (token, input) = match parse_json_body::<DuplicateDeckRequest>(request, 4096).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let request_id = input.request_id.unwrap_or_else(Uuid::new_v4);
    if deck.is_nil()
        || request_id.is_nil()
        || input
            .name
            .as_ref()
            .is_some_and(|n| n.trim().is_empty() || n.chars().count() > 64 || n.contains('\0'))
    {
        return fail(StatusCode::BAD_REQUEST, "deck", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let backend = match config.backend("spellwake-deck-duplicate") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<Value>, _> = backend.rpc(
                "duplicate_deck_command",
                &json!({
                    "p_subject": actor.subject(),
                    "p_session": actor.session(),
                    "p_request": request_id,
                    "p_source_deck": deck,
                    "p_name": input.name,
                }),
            );
            match result {
                Ok(ApiResponse::Success { data }) => {
                    (StatusCode::CREATED, Json(ApiResponse::Success { data })).into_response()
                }
                Ok(ApiResponse::Fail { data }) => {
                    if data.get("session").is_some_and(|v| v == "unauthorized") {
                        fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
                    } else if data.get("deck").is_some_and(|v| v == "deck unavailable") {
                        fail(StatusCode::NOT_FOUND, "deck", "unavailable")
                    } else {
                        (
                            StatusCode::CONFLICT,
                            Json(ApiResponse::<Value>::Fail { data }),
                        )
                            .into_response()
                    }
                }
                _ => unavailable(),
            }
        })
        .await
}

// ---------------------------------------------------------------------------
// 3. Rich cosmetics catalogue (prices, preview art, pet slot)
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CosmeticsRichQuery {
    kind: Option<String>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}

pub(super) async fn cosmetics_rich(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<CosmeticsRichQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit)
        || query.after.is_some_and(|id| id.is_nil())
        || query
            .kind
            .as_deref()
            .is_some_and(|k| !matches!(k, "back" | "sleeve" | "dice" | "playmat" | "pet"))
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "read_cosmetic_catalogue_rich",
                        &json!({
                            "p_kind": query.kind,
                            "p_after": query.after,
                            "p_limit": limit,
                        }),
                    )
                }),
                &[],
                StatusCode::OK,
            )
        })
        .await
}

// ---------------------------------------------------------------------------
// 4. Profile badge, cross-device Patreon status & Stripe billing portal
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetBadgeRequest {
    badge: Option<String>,
}

pub(super) async fn set_profile_badge(State(state): State<ApiState>, request: Request) -> Response {
    let (token, input) = match parse_json_body::<SetBadgeRequest>(request, 1024).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if input
        .badge
        .as_ref()
        .is_some_and(|b| b.chars().count() > 64 || b.contains('\0'))
    {
        return fail(StatusCode::BAD_REQUEST, "badge", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "set_my_profile_badge",
                        &json!({ "p_badge": input.badge }),
                    )
                }),
                &[],
                StatusCode::OK,
            )
        })
        .await
}

pub(super) async fn patreon_account_status(
    State(state): State<ApiState>,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(&token, "read_my_patreon_status", &json!({}))
                }),
                &[],
                StatusCode::OK,
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BillingPortalRequest {
    return_url: String,
}

pub(super) async fn billing_portal(State(state): State<ApiState>, request: Request) -> Response {
    let (token, input) = match parse_json_body::<BillingPortalRequest>(request, 2048).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let parsed_url = match reqwest::Url::parse(&input.return_url) {
        Ok(u) if matches!(u.scheme(), "https" | "http") && u.host_str().is_some() => u,
        _ => return fail(StatusCode::BAD_REQUEST, "returnUrl", "invalid"),
    };
    let return_url = parsed_url.to_string();
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let Some(stripe_cfg) = config.stripe.clone() else {
                return fail(StatusCode::CONFLICT, "billing", "unavailable");
            };
            let customer_resp = match config
                .actor()
                .and_then(|c| c.authenticated_rpc(&token, "read_my_stripe_customer", &json!({})))
            {
                Ok(ApiResponse::Success { data }) => data,
                Ok(ApiResponse::Fail { data }) => {
                    return (
                        StatusCode::CONFLICT,
                        Json(ApiResponse::<Value>::Fail { data }),
                    )
                        .into_response();
                }
                Ok(ApiResponse::Error { .. }) => return unavailable(),
                Err(error) => return actor_error(error),
            };
            let Some(customer_id) = customer_resp
                .get("stripeCustomerId")
                .and_then(Value::as_str)
                .filter(|id| id.starts_with("cus_"))
            else {
                return fail(StatusCode::CONFLICT, "billing", "no_customer");
            };
            match crate::stripe::client::create_billing_portal_session(
                stripe_cfg,
                customer_id,
                &return_url,
            ) {
                Ok(url) => success(json!({ "url": url })),
                Err(_) => unavailable(),
            }
        })
        .await
}

// ---------------------------------------------------------------------------
// 5. League leaderboard & public player summary (profile + ranks + W-L-D)
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LeaderboardQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    format_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    season_id: Option<Uuid>,
    offset: Option<usize>,
    limit: Option<usize>,
}

pub(super) async fn league_leaderboard(
    State(state): State<ApiState>,
    request: Request,
) -> Response {
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<LeaderboardQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    if !(1..=100).contains(&limit)
        || offset > 10_000
        || [query.format_id, query.season_id]
            .into_iter()
            .flatten()
            .any(|id| id.is_nil())
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "read_league_leaderboard",
                        &json!({
                            "p_format": query.format_id,
                            "p_season": query.season_id,
                            "p_offset": offset,
                            "p_limit": limit,
                        }),
                    )
                }),
                &[],
                StatusCode::OK,
            )
        })
        .await
}

pub(super) async fn player_public_summary(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let player = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "player", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "read_player_public_summary",
                        &json!({ "p_player": player }),
                    )
                }),
                &["player"],
                StatusCode::OK,
            )
        })
        .await
}

// ---------------------------------------------------------------------------
// 6. Searchable & sortable market listings
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MarketSearchQuery {
    search: Option<String>,
    kind: Option<String>,
    rarity: Option<String>,
    min_price: Option<u64>,
    max_price: Option<u64>,
    owned: Option<bool>,
    sort: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}

pub(super) async fn market_search(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<MarketSearchQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(24);
    let offset = query.offset.unwrap_or(0);
    if !(1..=100).contains(&limit)
        || offset > 10_000
        || query
            .search
            .as_ref()
            .is_some_and(|s| s.chars().count() > 256 || s.contains('\0'))
        || query
            .kind
            .as_deref()
            .is_some_and(|k| !matches!(k, "card" | "pack" | "coin"))
        || query
            .sort
            .as_deref()
            .is_some_and(|s| !matches!(s, "newest" | "price_asc" | "price_desc"))
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "search_market_listings",
                        &json!({
                            "p_search": query.search,
                            "p_kind": query.kind,
                            "p_rarity": query.rarity,
                            "p_min_price": query.min_price,
                            "p_max_price": query.max_price,
                            "p_owned": query.owned.unwrap_or(false),
                            "p_sort": query.sort.unwrap_or_else(|| "newest".into()),
                            "p_offset": offset,
                            "p_limit": limit,
                        }),
                    )
                }),
                &[],
                StatusCode::OK,
            )
        })
        .await
}

// ---------------------------------------------------------------------------
// 7. Bilateral live editable trade rooms
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TradeSessionsListQuery {
    state: Option<String>,
    limit: Option<usize>,
}

pub(super) async fn trade_sessions_list(
    State(state): State<ApiState>,
    request: Request,
) -> Response {
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<TradeSessionsListQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(20);
    if !(1..=50).contains(&limit)
        || query
            .state
            .as_deref()
            .is_some_and(|s| !matches!(s, "active" | "completed" | "canceled"))
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "list_my_trade_sessions",
                        &json!({ "p_state": query.state, "p_limit": limit }),
                    )
                }),
                &[],
                StatusCode::OK,
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTradeSessionRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    counterparty_id: Uuid,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    request_id: Option<Uuid>,
}

pub(super) async fn trade_session_create(
    State(state): State<ApiState>,
    request: Request,
) -> Response {
    let (token, input) = match parse_json_body::<CreateTradeSessionRequest>(request, 2048).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let request_id = input.request_id.unwrap_or_else(Uuid::new_v4);
    if input.counterparty_id.is_nil() || request_id.is_nil() {
        return fail(StatusCode::BAD_REQUEST, "counterparty", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "create_trade_session",
                        &json!({
                            "p_counterparty": input.counterparty_id,
                            "p_request": request_id,
                        }),
                    )
                }),
                &["counterparty"],
                StatusCode::CREATED,
            )
        })
        .await
}

pub(super) async fn trade_session_detail(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let session = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "session", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "read_my_trade_session",
                        &json!({ "p_session": session }),
                    )
                }),
                &["session"],
                StatusCode::OK,
            )
        })
        .await
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TradeSessionItemInput {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    asset_id: Uuid,
    quantity: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateTradeOfferRequest {
    expected_revision: i64,
    coins: u64,
    #[serde(default)]
    items: Vec<TradeSessionItemInput>,
}

pub(super) async fn trade_session_offer(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let session = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "session", "invalid"),
    };
    let (token, input) = match parse_json_body::<UpdateTradeOfferRequest>(request, 32768).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if input.expected_revision < 1
        || input.items.len() > 100
        || input
            .items
            .iter()
            .any(|i| i.asset_id.is_nil() || i.quantity == 0)
    {
        return fail(StatusCode::BAD_REQUEST, "offer", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "update_trade_session_offer",
                        &json!({
                            "p_session": session,
                            "p_expected_revision": input.expected_revision,
                            "p_coins": input.coins,
                            "p_items": input.items,
                        }),
                    )
                }),
                &["session"],
                StatusCode::OK,
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TradeReadyRequest {
    expected_revision: i64,
    ready: bool,
}

pub(super) async fn trade_session_ready(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let session = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "session", "invalid"),
    };
    let (token, input) = match parse_json_body::<TradeReadyRequest>(request, 1024).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if input.expected_revision < 1 {
        return fail(StatusCode::BAD_REQUEST, "revision", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "set_trade_session_ready",
                        &json!({
                            "p_session": session,
                            "p_expected_revision": input.expected_revision,
                            "p_ready": input.ready,
                        }),
                    )
                }),
                &["session"],
                StatusCode::OK,
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TradeConfirmRequest {
    expected_revision: i64,
}

pub(super) async fn trade_session_confirm(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let session = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "session", "invalid"),
    };
    let (token, input) = match parse_json_body::<TradeConfirmRequest>(request, 1024).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if input.expected_revision < 1 {
        return fail(StatusCode::BAD_REQUEST, "revision", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "confirm_trade_session",
                        &json!({
                            "p_session": session,
                            "p_expected_revision": input.expected_revision,
                        }),
                    )
                }),
                &["session"],
                StatusCode::OK,
            )
        })
        .await
}

pub(super) async fn trade_session_cancel(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let session = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "session", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "cancel_trade_session",
                        &json!({ "p_session": session }),
                    )
                }),
                &["session"],
                StatusCode::OK,
            )
        })
        .await
}

// ---------------------------------------------------------------------------
// 8. Predictions search (with settled result price) & one-bet execution
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PredictionSearchQuery {
    search: Option<String>,
    state: Option<String>,
    sort: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}

pub(super) async fn predictions_search(
    State(state): State<ApiState>,
    request: Request,
) -> Response {
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<PredictionSearchQuery>::try_from_uri(request.uri()) {
        Ok(Query(q)) => q,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(24);
    let offset = query.offset.unwrap_or(0);
    if !(1..=100).contains(&limit)
        || offset > 10_000
        || query
            .search
            .as_ref()
            .is_some_and(|s| s.chars().count() > 256 || s.contains('\0'))
        || query
            .sort
            .as_deref()
            .is_some_and(|s| !matches!(s, "closing_soon" | "newest" | "resolved_recent"))
    {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "search_prediction_markets",
                        &json!({
                            "p_search": query.search,
                            "p_state": query.state,
                            "p_sort": query.sort.unwrap_or_else(|| "closing_soon".into()),
                            "p_offset": offset,
                            "p_limit": limit,
                        }),
                    )
                }),
                &[],
                StatusCode::OK,
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OneBetRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    request_id: Uuid,
    side: String,
    limit_cost_coins: u64,
    quantity: u64,
}

pub(super) async fn prediction_bet(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let market_id = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "market", "invalid"),
    };
    let (token, input) = match parse_json_body::<OneBetRequest>(request, 4096).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if market_id.is_nil()
        || input.request_id.is_nil()
        || !matches!(input.side.as_str(), "yes" | "no")
        || !(1..=99).contains(&input.limit_cost_coins)
        || input.quantity == 0
    {
        return fail(StatusCode::BAD_REQUEST, "bet", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|c| c.resolve(&token)) {
                Ok(a) => a,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let backend = match config.backend("spellwake-prediction-bet") {
                Ok(b) => b,
                Err(_) => return unavailable(),
            };
            let result: Result<ApiResponse<Value>, _> = backend.rpc(
                "execute_prediction_order",
                &json!({
                    "p_subject": actor.subject(),
                    "p_session": actor.session(),
                    "p_input": {
                        "kind": "place_order",
                        "command": {
                            "marketId": market_id,
                            "side": input.side,
                            "limitCost": input.limit_cost_coins.to_string(),
                            "quantity": input.quantity.to_string(),
                            "requestId": input.request_id,
                        }
                    },
                }),
            );
            match result {
                Ok(ApiResponse::Success { data }) => {
                    (StatusCode::ACCEPTED, Json(ApiResponse::Success { data })).into_response()
                }
                Ok(ApiResponse::Fail { data }) => {
                    if data.get("session").is_some_and(|v| v == "unauthorized") {
                        fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
                    } else {
                        (
                            StatusCode::CONFLICT,
                            Json(ApiResponse::<Value>::Fail { data }),
                        )
                            .into_response()
                    }
                }
                _ => unavailable(),
            }
        })
        .await
}

// ---------------------------------------------------------------------------
// 9. Lobby / Room game handoff, enriched detail, options & named invites
// ---------------------------------------------------------------------------
pub(super) async fn room_game(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let room = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "room", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(&token, "read_room_game", &json!({ "p_room": room }))
                }),
                &["room"],
                StatusCode::OK,
            )
        })
        .await
}

pub(super) async fn room_enriched(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let room = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "room", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(&token, "read_room_enriched", &json!({ "p_room": room }))
                }),
                &["room"],
                StatusCode::OK,
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateRoomOptionsRequest {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    format_id: Option<Uuid>,
    ai_difficulty: Option<String>,
    planechase_enabled: Option<bool>,
    range_of_influence: Option<i32>,
}

pub(super) async fn update_room_options(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let room = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "room", "invalid"),
    };
    let (token, input) = match parse_json_body::<UpdateRoomOptionsRequest>(request, 2048).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if input.format_id.is_some_and(|id| id.is_nil())
        || input
            .ai_difficulty
            .as_deref()
            .is_some_and(|d| !matches!(d, "casual" | "standard" | "competitive"))
        || input
            .range_of_influence
            .is_some_and(|r| !(1..=5).contains(&r))
    {
        return fail(StatusCode::BAD_REQUEST, "options", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "update_room_options",
                        &json!({
                            "p_room": room,
                            "p_format_id": input.format_id,
                            "p_ai_difficulty": input.ai_difficulty,
                            "p_planechase_enabled": input.planechase_enabled,
                            "p_range_of_influence": input.range_of_influence,
                        }),
                    )
                }),
                &["room"],
                StatusCode::OK,
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InviteByUsernameRequest {
    username: String,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    request_id: Option<Uuid>,
}

pub(super) async fn invite_room_by_username(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let room = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "room", "invalid"),
    };
    let (token, input) = match parse_json_body::<InviteByUsernameRequest>(request, 1024).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let trimmed = input.username.trim();
    if trimmed.len() < 3 || trimmed.len() > 24 || trimmed.contains('\0') {
        return fail(StatusCode::BAD_REQUEST, "username", "invalid");
    }
    let request_id = input.request_id.unwrap_or_else(Uuid::new_v4);
    let username = trimmed.to_owned();
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "invite_room_player_by_username",
                        &json!({
                            "p_room": room,
                            "p_username": username,
                            "p_request": request_id,
                        }),
                    )
                }),
                &["room", "player"],
                StatusCode::CREATED,
            )
        })
        .await
}

// ---------------------------------------------------------------------------
// 10. Live Limited Draft HTTP routes (wrapping migration 027200)
// ---------------------------------------------------------------------------
pub(super) async fn limited_draft_read(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let room = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "room", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(&token, "read_my_limited_draft", &json!({ "p_room": room }))
                }),
                &["draft"],
                StatusCode::OK,
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartDraftRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    source_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    request_id: Uuid,
}

pub(super) async fn limited_draft_start(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let room = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "room", "invalid"),
    };
    let (token, input) = match parse_json_body::<StartDraftRequest>(request, 1024).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if input.source_id.is_nil() || input.request_id.is_nil() {
        return fail(StatusCode::BAD_REQUEST, "draft", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "start_limited_draft",
                        &json!({
                            "p_room": room,
                            "p_source": input.source_id,
                            "p_request": input.request_id,
                        }),
                    )
                }),
                &["draft"],
                StatusCode::CREATED,
            )
        })
        .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftPickRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    draft_id: Uuid,
    step: i32,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    item_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    request_id: Uuid,
}

pub(super) async fn limited_draft_pick(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let room = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "room", "invalid"),
    };
    let (token, input) = match parse_json_body::<DraftPickRequest>(request, 1024).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if input.draft_id.is_nil()
        || input.item_id.is_nil()
        || input.request_id.is_nil()
        || !(0..=59).contains(&input.step)
    {
        return fail(StatusCode::BAD_REQUEST, "pick", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "pick_limited_draft_card",
                        &json!({
                            "p_room": room,
                            "p_draft": input.draft_id,
                            "p_step": input.step,
                            "p_item": input.item_id,
                            "p_request": input.request_id,
                        }),
                    )
                }),
                &["draft", "item"],
                StatusCode::OK,
            )
        })
        .await
}

pub(super) async fn limited_draft_seal(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let room = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "room", "invalid"),
    };
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "seal_my_limited_draft_pool",
                        &json!({ "p_room": room }),
                    )
                }),
                &["draft"],
                StatusCode::OK,
            )
        })
        .await
}

// ---------------------------------------------------------------------------
// 11. DM structured invite attachments (trade_invite / game_invite)
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DmAttachmentRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    request_id: Uuid,
    #[serde(default)]
    body: String,
    attachment: Value,
}

pub(super) async fn dm_send_attachment(
    State(state): State<ApiState>,
    path: IdentifierPath,
    request: Request,
) -> Response {
    let conversation = match identifier(path) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "conversation", "invalid"),
    };
    let (token, input) = match parse_json_body::<DmAttachmentRequest>(request, 8192).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let valid_kind = input
        .attachment
        .as_object()
        .and_then(|m| m.get("kind"))
        .and_then(Value::as_str)
        .is_some_and(|k| matches!(k, "trade_invite" | "game_invite"));
    if input.request_id.is_nil()
        || !valid_kind
        || input.body.chars().count() > 1000
        || input.body.contains('\0')
    {
        return fail(StatusCode::BAD_REQUEST, "message", "invalid");
    }
    state
        .blocking(move |config| {
            relay_rpc(
                config.actor().and_then(|c| {
                    c.authenticated_rpc(
                        &token,
                        "send_dm_attachment",
                        &json!({
                            "p_conversation": conversation,
                            "p_body": input.body,
                            "p_attachment": input.attachment,
                            "p_request": input.request_id,
                        }),
                    )
                }),
                &["conversation"],
                StatusCode::CREATED,
            )
        })
        .await
}
