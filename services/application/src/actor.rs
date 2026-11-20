//! Request identity established by the original bearer and canonical database authority.
//! Blocking clients must be constructed, used and dropped inside `spawn_blocking`
//! when called from an asynchronous runtime. No local JWT parse authenticates a caller.
use crate::{
    ApiResponse,
    http_transport::{HttpTransport, Origin, read_json_body, sensitive_key},
    inventory::InventoryPage,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{StatusCode, header::HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fmt,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const MAX_BEARER: usize = 16 * 1024;

/// Every caller read projects an object. Lists inside those projections have their own decoders.
fn read_object_response<T: serde::de::DeserializeOwned>(
    response: reqwest::blocking::Response,
) -> Result<ApiResponse<T>, ActorError> {
    read_json_body::<crate::provider_json::ObjectResponse<T>>(response)
        .map(|response| response.0)
        .map_err(ActorError::caused)
}

/// Root formatting is safe; retained diagnostic sources must never be logged recursively.
#[derive(Clone)]
pub enum ActorError {
    Configuration,
    Unauthorized,
    Unavailable,
    /// The source is retained for trusted debugging; never recursively log it.
    UnavailableCause {
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
}
impl ActorError {
    fn caused(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::UnavailableCause {
            source: Arc::new(source),
        }
    }
    fn kind(&self) -> u8 {
        match self {
            Self::Configuration => 0,
            Self::Unauthorized => 1,
            Self::Unavailable | Self::UnavailableCause { .. } => 2,
        }
    }
}
// Compare safe failure classifications, never the diagnostic source's identity or text.
impl PartialEq for ActorError {
    fn eq(&self, other: &Self) -> bool {
        self.kind() == other.kind()
    }
}
impl Eq for ActorError {}
impl fmt::Debug for ActorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for ActorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "request authentication configuration is invalid",
            Self::Unauthorized => "a valid sign-in session is required",
            Self::Unavailable | Self::UnavailableCause { .. } => {
                "request authentication is temporarily unavailable"
            }
        })
    }
}
impl std::error::Error for ActorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnavailableCause { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

/// The one method family bound to this Spellwake account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum SignInMethod {
    Google,
    Apple,
    Discord,
    EmailCode,
}
impl TryFrom<String> for SignInMethod {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "google" => Ok(Self::Google),
            "apple" => Ok(Self::Apple),
            "discord" => Ok(Self::Discord),
            "email_code" => Ok(Self::EmailCode),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Identity is readable while deletion waits; writers still enforce lifecycle rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum ActorState {
    NeedsRegistration,
    Active,
    DeletionPending,
}
impl TryFrom<String> for ActorState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "needs_registration" => Ok(Self::NeedsRegistration),
            "active" => Ok(Self::Active),
            "deletion_pending" => Ok(Self::DeletionPending),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Canonical identity snapshot. Not a durable permission to make a later mutation.
/// No Debug or serialization implementation exposes identity through diagnostics.
pub struct RequestActor {
    subject: Uuid,
    session: Uuid,
    method: SignInMethod,
    state: ActorState,
    account: Option<Uuid>,
    verified_bearer_expiry: u64,
}
impl RequestActor {
    /// Observe an already authenticated original deadline in owned fixture tests.
    /// This cannot construct or extend a caller proof and is absent in production.
    #[cfg(test)]
    pub(crate) fn test_verified_bearer_expiry(&self) -> u64 {
        self.verified_bearer_expiry
    }
    /// Check the original verified bearer's deadline immediately before publication.
    /// This is only an additional refusal guard, never authentication or a revocation check.
    /// # Errors
    /// Returns Unauthorized at/after expiry or Unavailable if the system clock is invalid.
    pub(crate) fn ensure_unexpired(&self) -> Result<(), ActorError> {
        ensure_bearer_deadline(self.verified_bearer_expiry)
    }
    /// Compare canonical identity after fresh original-bearer authentication.
    /// Lifecycle remains separately enforced by the actual operation's SQL access check.
    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        (self.subject, self.session, self.method, self.account)
            == (other.subject, other.session, other.method, other.account)
    }
    pub fn subject(&self) -> Uuid {
        self.subject
    }
    pub fn session(&self) -> Uuid {
        self.session
    }
    pub fn method(&self) -> SignInMethod {
        self.method
    }
    pub fn state(&self) -> ActorState {
        self.state
    }
    pub fn account(&self) -> Option<Uuid> {
        self.account
    }
}

/// Public gateway configuration cannot accept a trusted worker secret.
pub struct ActorConfig {
    origin: Origin,
    key: HeaderValue,
    issuer: String,
    timeout: Duration,
}
impl ActorConfig {
    pub fn new(base: &str, publishable_key: &str, issuer: &str) -> Result<Self, ActorError> {
        Self::with_timeout(base, publishable_key, issuer, Duration::from_secs(10))
    }
    pub fn with_timeout(
        base: &str,
        publishable_key: &str,
        issuer: &str,
        timeout: Duration,
    ) -> Result<Self, ActorError> {
        let issuer_url = reqwest::Url::parse(issuer).map_err(|_| ActorError::Configuration)?;
        let issuer_origin = issuer_url.origin().ascii_serialization();
        Origin::new(&issuer_origin).map_err(|_| ActorError::Configuration)?;
        if !issuer_url.username().is_empty()
            || issuer_url.password().is_some()
            || issuer_url.query().is_some()
            || issuer_url.fragment().is_some()
            || issuer.len() > 2048
            || timeout.is_zero()
            || timeout > Duration::from_secs(60)
        {
            return Err(ActorError::Configuration);
        }
        Ok(Self {
            origin: Origin::new(base).map_err(|_| ActorError::Configuration)?,
            key: sensitive_key(publishable_key, "sb_publishable_")
                .map_err(|_| ActorError::Configuration)?,
            issuer: issuer.into(),
            timeout,
        })
    }
}

/// Reusable HTTP connection pool; a caller bearer is attached only to one request.
pub struct ActorClient {
    http: HttpTransport,
    issuer: String,
}
impl ActorClient {
    pub fn new(config: ActorConfig) -> Result<Self, ActorError> {
        Ok(Self {
            http: HttpTransport::new(
                config.origin,
                config.key,
                "spellwake-request-actor",
                config.timeout,
            )
            .map_err(|_| ActorError::Configuration)?,
            issuer: config.issuer,
        })
    }
    pub fn resolve(&self, original_bearer: &str) -> Result<RequestActor, ActorError> {
        let local = parse_claims(original_bearer, &self.issuer)?;
        let response = self.caller_rpc(original_bearer, "read_current_session", &json!({}))?;
        let response: ApiResponse<SessionWire> = read_object_response(response)?;
        match response {
            ApiResponse::Success { data } => data.into_actor(local),
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("session").is_some_and(|v| v == "unauthorized") =>
            {
                Err(ActorError::Unauthorized)
            }
            _ => Err(ActorError::Unavailable),
        }
    }
    /// Owner reads retain the original bearer so Postgres applies its caller RLS.
    pub(crate) fn inventory(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<InventoryPage>, ActorError> {
        if !(1..=100).contains(&limit) {
            return Err(ActorError::Configuration);
        }
        parse_claims(bearer, &self.issuer)?;
        let response = self.caller_rpc(
            bearer,
            "read_my_inventory_page",
            &json!({"p_after":after,"p_limit":limit}),
        )?;
        let response: ApiResponse<InventoryPage> = read_object_response(response)?;
        match &response {
            ApiResponse::Success { data } => data
                .validate_after(limit, after)
                .map_err(|_| ActorError::Unavailable)?,
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("session").is_some_and(|v| v == "unauthorized") =>
            {
                return Err(ActorError::Unauthorized);
            }
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("account").is_some_and(|v| v == "unavailable") => {}
            _ => return Err(ActorError::Unavailable),
        }
        Ok(response)
    }
    pub(crate) fn purchase(
        &self,
        bearer: &str,
        id: Uuid,
    ) -> Result<ApiResponse<crate::stripe::PurchaseView>, ActorError> {
        parse_claims(bearer, &self.issuer)?;
        let response = self.caller_rpc(bearer, "read_my_purchase", &json!({"p_purchase":id}))?;
        let result: ApiResponse<crate::stripe::PurchaseView> = read_object_response(response)?;
        match &result {
            ApiResponse::Success { data } => {
                data.validate(id).map_err(|_| ActorError::Unavailable)?
            }
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("session").is_some_and(|v| v == "unauthorized") =>
            {
                return Err(ActorError::Unauthorized);
            }
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("purchase").is_some_and(|v| v == "unavailable") => {}
            _ => return Err(ActorError::Unavailable),
        }
        Ok(result)
    }
    pub(crate) fn account(
        &self,
        bearer: &str,
    ) -> Result<ApiResponse<crate::account::AccountSummary>, ActorError> {
        parse_claims(bearer, &self.issuer)?;
        let response = self.caller_rpc(bearer, "read_my_account", &json!({}))?;
        let result: ApiResponse<crate::account::AccountSummary> = read_object_response(response)?;
        match &result {
            ApiResponse::Success { data } => {
                data.validate().map_err(|_| ActorError::Unavailable)?
            }
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("session").is_some_and(|v| v == "unauthorized") =>
            {
                return Err(ActorError::Unauthorized);
            }
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("account").is_some_and(|v| v == "unavailable") => {}
            _ => return Err(ActorError::Unavailable),
        }
        Ok(result)
    }
    pub(crate) fn deck_library(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::account::DeckLibraryPage>, ActorError> {
        if !(1..=100).contains(&limit) {
            return Err(ActorError::Configuration);
        }
        parse_claims(bearer, &self.issuer)?;
        let response = self.caller_rpc(
            bearer,
            "read_my_deck_library",
            &json!({"p_after":after,"p_limit":limit}),
        )?;
        let result: ApiResponse<crate::account::DeckLibraryPage> = read_object_response(response)?;
        match &result {
            ApiResponse::Success { data } => data
                .validate_after(limit, after)
                .map_err(|_| ActorError::Unavailable)?,
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("session").is_some_and(|v| v == "unauthorized") =>
            {
                return Err(ActorError::Unauthorized);
            }
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("account").is_some_and(|v| v == "unavailable") => {}
            _ => return Err(ActorError::Unavailable),
        }
        Ok(result)
    }
    pub(crate) fn cosmetic_catalogue(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::store::StorePage<crate::store::CosmeticSummary>>, ActorError>
    {
        self.store_page(
            bearer,
            "read_my_cosmetic_catalogue",
            after,
            limit,
            |page: &crate::store::StorePage<crate::store::CosmeticSummary>| {
                page.validate_after(after, limit)
            },
        )
    }
    pub(crate) fn purchases(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::store::StorePage<crate::stripe::PurchaseView>>, ActorError> {
        self.store_page(
            bearer,
            "read_my_purchases",
            after,
            limit,
            |page: &crate::store::StorePage<crate::stripe::PurchaseView>| {
                page.validate_after(after, limit)
            },
        )
    }
    pub(crate) fn formats(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::decks::reads::FormatPage>, ActorError> {
        self.store_page(
            bearer,
            "read_my_formats",
            after,
            limit,
            |page: &crate::decks::reads::FormatPage| page.validate(after, limit),
        )
    }
    pub(crate) fn deck(
        &self,
        bearer: &str,
        deck: Uuid,
    ) -> Result<ApiResponse<crate::decks::reads::DeckDetail>, ActorError> {
        if deck.is_nil() {
            return Err(ActorError::Configuration);
        }
        parse_claims(bearer, &self.issuer)?;
        let response = self.caller_rpc(bearer, "read_my_deck", &json!({"p_deck":deck}))?;
        let result: ApiResponse<crate::decks::reads::DeckDetail> = read_object_response(response)?;
        match &result {
            ApiResponse::Success { data } => {
                data.validate(deck).map_err(|_| ActorError::Unavailable)?
            }
            ApiResponse::Fail { data }
                if data.len() == 1
                    && data
                        .get("session")
                        .is_some_and(|value| value == "unauthorized") =>
            {
                return Err(ActorError::Unauthorized);
            }
            ApiResponse::Fail { data }
                if data.len() == 1
                    && (data.get("deck").is_some_and(|value| value == "unavailable")
                        || data
                            .get("account")
                            .is_some_and(|value| value == "unavailable")) => {}
            _ => return Err(ActorError::Unavailable),
        }
        Ok(result)
    }
    fn store_page<T: serde::de::DeserializeOwned>(
        &self,
        bearer: &str,
        name: &str,
        after: Option<Uuid>,
        limit: usize,
        validate: impl FnOnce(&T) -> Result<(), &'static str>,
    ) -> Result<ApiResponse<T>, ActorError> {
        if !(1..=100).contains(&limit) || after.is_some_and(|id| id.is_nil()) {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            name,
            &json!({"p_after":after,"p_limit":limit}),
            validate,
        )
    }
    /// Bind one current account to an original-bearer projection and its response context.
    fn account_projection<T: serde::de::DeserializeOwned>(
        &self,
        bearer: &str,
        name: &str,
        payload: &Value,
        unavailable: &[&'static str],
        validate: impl FnOnce(&T, Uuid) -> Result<(), &'static str>,
    ) -> Result<ApiResponse<T>, ActorError> {
        let Some(owner) = self.resolve(bearer)?.account() else {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("account".into(), "unavailable".into())]),
            });
        };
        self.scoped_projection_keys(bearer, name, payload, unavailable, |data| {
            validate(data, owner)
        })
    }
    fn owner_projection<T: serde::de::DeserializeOwned>(
        &self,
        bearer: &str,
        name: &str,
        payload: &Value,
        validate: impl FnOnce(&T) -> Result<(), &'static str>,
    ) -> Result<ApiResponse<T>, ActorError> {
        self.scoped_projection(bearer, name, payload, None, validate)
    }
    fn scoped_projection<T: serde::de::DeserializeOwned>(
        &self,
        bearer: &str,
        name: &str,
        payload: &Value,
        unavailable_resource: Option<&'static str>,
        validate: impl FnOnce(&T) -> Result<(), &'static str>,
    ) -> Result<ApiResponse<T>, ActorError> {
        self.scoped_projection_keys(
            bearer,
            name,
            payload,
            unavailable_resource.as_slice(),
            validate,
        )
    }
    fn scoped_projection_keys<T: serde::de::DeserializeOwned>(
        &self,
        bearer: &str,
        name: &str,
        payload: &Value,
        unavailable_resources: &[&'static str],
        validate: impl FnOnce(&T) -> Result<(), &'static str>,
    ) -> Result<ApiResponse<T>, ActorError> {
        parse_claims(bearer, &self.issuer)?;
        let response = self.caller_rpc(bearer, name, payload)?;
        let result: ApiResponse<T> = read_object_response(response)?;
        match &result {
            ApiResponse::Success { data } => validate(data).map_err(|_| ActorError::Unavailable)?,
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("session").is_some_and(|v| v == "unauthorized") =>
            {
                return Err(ActorError::Unauthorized);
            }
            ApiResponse::Fail { data }
                if data.len() == 1
                    && (data
                        .get("account")
                        .is_some_and(|value| value == "unavailable")
                        || unavailable_resources.iter().any(|resource| {
                            data.get(*resource)
                                .is_some_and(|value| value == "unavailable")
                        })) => {}
            _ => return Err(ActorError::Unavailable),
        }
        Ok(result)
    }
    pub(crate) fn trade(
        &self,
        bearer: &str,
        trade: Uuid,
    ) -> Result<ApiResponse<crate::exchange_reads::OwnedTrade>, ActorError> {
        if trade.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.scoped_projection(
            bearer,
            "read_my_trade",
            &json!({"p_trade":trade}),
            Some("trade"),
            |value: &crate::exchange_reads::OwnedTrade| value.validate(trade),
        )
    }
    pub(crate) fn trades(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
        state: Option<crate::exchange_reads::TradeState>,
    ) -> Result<ApiResponse<crate::exchange_reads::TradePage>, ActorError> {
        if !(1..=crate::exchange_reads::EXCHANGE_PAGE_LIMIT).contains(&limit)
            || after.is_some_and(|id| id.is_nil())
        {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            "read_my_trades",
            &json!({"p_after":after,"p_limit":limit,"p_state":state}),
            |page: &crate::exchange_reads::TradePage| page.validate(after, limit, state),
        )
    }
    pub(crate) fn market_listing(
        &self,
        bearer: &str,
        listing: Uuid,
    ) -> Result<ApiResponse<crate::exchange_reads::OwnedListing>, ActorError> {
        if listing.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.scoped_projection(
            bearer,
            "read_my_market_listing",
            &json!({"p_listing":listing}),
            Some("listing"),
            |value: &crate::exchange_reads::OwnedListing| value.validate(listing),
        )
    }
    pub(crate) fn market(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
        owned: bool,
    ) -> Result<ApiResponse<crate::exchange_reads::ListingPage>, ActorError> {
        if !(1..=crate::exchange_reads::EXCHANGE_PAGE_LIMIT).contains(&limit)
            || after.is_some_and(|id| id.is_nil())
        {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            "read_my_market",
            &json!({"p_after":after,"p_limit":limit,"p_owned":owned}),
            |page: &crate::exchange_reads::ListingPage| page.validate(after, limit, owned),
        )
    }
    pub(crate) fn prediction_probe_basis(
        &self,
        bearer: &str,
        printing: Uuid,
    ) -> Result<ApiResponse<crate::prediction_publication::OwnedProbeBasis>, ActorError> {
        if printing.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.scoped_projection_keys(
            bearer,
            "read_my_prediction_probe_basis",
            &json!({"p_printing":printing}),
            &["printing", "price"],
            |value: &crate::prediction_publication::OwnedProbeBasis| value.validate(printing),
        )
    }
    pub(crate) fn prediction_publication_source(
        &self,
        bearer: &str,
        printing: Uuid,
    ) -> Result<ApiResponse<crate::prediction_publication::OwnedPublicationSource>, ActorError>
    {
        if printing.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.scoped_projection_keys(
            bearer,
            "read_my_prediction_publication_source",
            &json!({"p_printing":printing}),
            &["printing", "price"],
            |value: &crate::prediction_publication::OwnedPublicationSource| {
                value.validate(printing)
            },
        )
    }
    pub(crate) fn prediction_market(
        &self,
        bearer: &str,
        market: Uuid,
    ) -> Result<ApiResponse<crate::prediction_reads::OwnedMarket>, ActorError> {
        if market.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.scoped_projection(
            bearer,
            "read_my_prediction_market",
            &json!({"p_market":market}),
            Some("market"),
            |value: &crate::prediction_reads::OwnedMarket| value.validate(market),
        )
    }
    pub(crate) fn prediction_markets(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::prediction_reads::MarketPage>, ActorError> {
        self.store_page(
            bearer,
            "read_my_prediction_markets",
            after,
            limit,
            |value: &crate::prediction_reads::MarketPage| value.validate(after, limit),
        )
    }
    pub(crate) fn prediction_orders(
        &self,
        bearer: &str,
        market: Option<Uuid>,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::prediction_reads::OrderPage>, ActorError> {
        if market.is_some_and(|id| id.is_nil())
            || after.is_some_and(|id| id.is_nil())
            || !(1..=100).contains(&limit)
        {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            "read_my_prediction_orders",
            &json!({"p_market":market,"p_after":after,"p_limit":limit}),
            |value: &crate::prediction_reads::OrderPage| value.validate(after, limit, market),
        )
    }
    pub(crate) fn prediction_positions(
        &self,
        bearer: &str,
        market: Option<Uuid>,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::prediction_reads::PositionPage>, ActorError> {
        if market.is_some_and(|id| id.is_nil())
            || after.is_some_and(|id| id.is_nil())
            || !(1..=100).contains(&limit)
        {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            "read_my_prediction_positions",
            &json!({"p_market":market,"p_after":after,"p_limit":limit}),
            |value: &crate::prediction_reads::PositionPage| value.validate(after, limit, market),
        )
    }
    pub(crate) fn pack_products(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::pack_shop::PackProductPage>, ActorError> {
        if !(1..=crate::pack_shop::PACK_PRODUCT_PAGE_LIMIT).contains(&limit)
            || after.is_some_and(|id| id.is_nil())
        {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            "read_my_pack_products",
            &json!({"p_after":after,"p_limit":limit}),
            |page: &crate::pack_shop::PackProductPage| page.validate(after, limit),
        )
    }
    pub(crate) fn pack_quote(
        &self,
        bearer: &str,
        recipe: Uuid,
        quantity: crate::supply::PackQuantity,
    ) -> Result<ApiResponse<crate::pack_shop::OwnedPackQuote>, ActorError> {
        if recipe.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.scoped_projection(
            bearer,
            "read_my_pack_quote",
            &json!({"p_recipe":recipe,"p_quantity":quantity.get()}),
            Some("product"),
            |value: &crate::pack_shop::OwnedPackQuote| value.validate(recipe, quantity),
        )
    }
    pub(crate) fn pack_outcome(
        &self,
        bearer: &str,
        opened: Uuid,
    ) -> Result<ApiResponse<crate::packs::OwnedPackOutcome>, ActorError> {
        if opened.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.scoped_projection(
            bearer,
            "read_my_pack_open",
            &json!({"p_open":opened}),
            Some("pack"),
            |value: &crate::packs::OwnedPackOutcome| value.validate(opened),
        )
    }
    pub(crate) fn social_status(
        &self,
        bearer: &str,
        person: Option<Uuid>,
    ) -> Result<ApiResponse<crate::social::SocialStatus>, ActorError> {
        if person.is_some_and(|id| id.is_nil()) {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            "read_my_social_status",
            &json!({"p_peer": person}),
            |status: &crate::social::SocialStatus| status.validate(person),
        )
    }
    pub(crate) fn social_people(
        &self,
        bearer: &str,
        query: Option<&str>,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::social::SocialPeoplePage>, ActorError> {
        if !(1..=100).contains(&limit)
            || after.is_some_and(|id| id.is_nil())
            || query.is_some_and(|query| {
                query.contains('\0') || !(1..=64).contains(&query.trim_matches(' ').chars().count())
            })
        {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            "read_my_social_people",
            &json!({"p_kind":if query.is_some() {"directory"} else {"threads"},"p_query":query,"p_after":after,"p_limit":limit}),
            |page: &crate::social::SocialPeoplePage| page.validate(query, after, limit),
        )
    }
    pub(crate) fn profile_art(
        &self,
        bearer: &str,
    ) -> Result<ApiResponse<crate::profile_art::OwnProfileArt>, ActorError> {
        self.account_projection(
            bearer,
            "read_my_profile_art",
            &json!({}),
            &[],
            |profile: &crate::profile_art::OwnProfileArt, owner| profile.validate(owner),
        )
    }
    pub(crate) fn public_profile_art(
        &self,
        bearer: &str,
        target: Uuid,
    ) -> Result<ApiResponse<crate::profile_art::PublicProfileArt>, ActorError> {
        self.account_projection(
            bearer,
            "read_public_profile_art",
            &json!({"p_player":target}),
            &["player"],
            |profile: &crate::profile_art::PublicProfileArt, owner| profile.validate(owner, target),
        )
    }
    pub(crate) fn game_chat(
        &self,
        bearer: &str,
        match_id: Uuid,
        before: Option<&crate::social::MessageCursor>,
        limit: usize,
    ) -> Result<ApiResponse<crate::game_chat::GameChatPage>, ActorError> {
        self.account_projection(bearer,"read_my_game_chat",
            &json!({"p_match":match_id,"p_before_at":before.map(|c|c.sent_at),"p_before_id":before.map(|c|c.id),"p_limit":limit}),
            &["conversation"],|page: &crate::game_chat::GameChatPage,owner|page.validate(owner,match_id,before,limit))
    }
    pub(crate) fn notifications(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::notifications::NotificationPage>, ActorError> {
        self.account_projection(
            bearer,
            "read_my_notifications",
            &json!({"p_after":after,"p_limit":limit}),
            &[],
            |page: &crate::notifications::NotificationPage, owner| {
                page.validate(owner, after, limit)
            },
        )
    }
    pub(crate) fn direct_messages(
        &self,
        bearer: &str,
        conversation: Uuid,
        before: Option<&crate::social::MessageCursor>,
        limit: usize,
    ) -> Result<ApiResponse<crate::social::DirectMessagePage>, ActorError> {
        if conversation.is_nil()
            || !(1..=crate::social::DIRECT_MESSAGE_PAGE_LIMIT).contains(&limit)
            || before.is_some_and(|cursor| cursor.id.is_nil())
        {
            return Err(ActorError::Configuration);
        }
        self.scoped_projection(
            bearer,
            "read_my_direct_messages",
            &json!({"p_conversation":conversation,"p_before_at":before.map(|cursor|cursor.sent_at),"p_before_id":before.map(|cursor|cursor.id),"p_limit":limit}),
            Some("conversation"),
            |page: &crate::social::DirectMessagePage| page.validate(conversation, before, limit),
        )
    }
    pub(crate) fn sets(
        &self,
        bearer: &str,
        query: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::catalogue::SetPage>, ActorError> {
        if query.chars().count() > 1024
            || query.contains('\0')
            || !(1..=100).contains(&limit)
            || after.is_some_and(|id| id.is_nil())
        {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            "read_my_sets",
            &json!({"p_query":query,"p_after":after,"p_limit":limit}),
            |page: &crate::catalogue::SetPage| page.validate_after(after, limit),
        )
    }
    pub(crate) fn cards(
        &self,
        bearer: &str,
        query: &str,
        exact: bool,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::catalogue::CardPage>, ActorError> {
        if query.chars().count() > 1024
            || query.contains('\0')
            || !(1..=100).contains(&limit)
            || after.is_some_and(|id| id.is_nil())
        {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            "read_my_cards",
            &json!({"p_query":query,"p_exact":exact,"p_after":after,"p_limit":limit}),
            |page: &crate::catalogue::CardPage| page.validate_after(after, limit),
        )
    }
    pub(crate) fn printings(
        &self,
        bearer: &str,
        filter: &crate::catalogue::PrintingFilter<'_>,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::catalogue::PrintingPage>, ActorError> {
        if filter.validate().is_err()
            || !(1..=100).contains(&limit)
            || after.is_some_and(|id| id.is_nil())
        {
            return Err(ActorError::Configuration);
        }
        self.owner_projection(
            bearer,
            "read_my_printings",
            &json!({"p_definition":filter.definition,"p_name":filter.name,"p_set_code":filter.set_code,"p_collector":filter.collector,"p_after":after,"p_limit":limit}),
            |page: &crate::catalogue::PrintingPage| page.validate_after(filter.definition, after, limit),
        )
    }
    pub(crate) fn catalogue(
        &self,
        bearer: &str,
        request: &crate::catalogue::CatalogueLookup,
    ) -> Result<ApiResponse<crate::catalogue::CatalogueLabels>, ActorError> {
        request.validate().map_err(|_| ActorError::Configuration)?;
        self.owner_projection(
            bearer,
            "read_my_catalogue",
            &json!({"p_definitions":request.definition_ids,"p_printings":request.printing_ids}),
            |labels: &crate::catalogue::CatalogueLabels| labels.validate(request),
        )
    }
    /// Read only caller-owned safe queue status, filtered before UUID pagination.
    pub(crate) fn queue(
        &self,
        bearer: &str,
        intent: Option<Uuid>,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::matchmaking::QueuePage>, ActorError> {
        if [intent, after].into_iter().flatten().any(|id| id.is_nil())
            || !(1..=100).contains(&limit)
        {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_my_queue",
            &json!({"p_intent":intent,"p_after":after,"p_limit":limit}),
            &[],
            |page: &crate::matchmaking::QueuePage, owner| {
                page.validate(owner, intent, after, limit)
            },
        )
    }
    /// Read a caller-owned original intent without private normalized qualification fields.
    pub(crate) fn queue_intent(
        &self,
        bearer: &str,
        intent: Uuid,
    ) -> Result<ApiResponse<crate::matchmaking::QueueIntent>, ActorError> {
        if intent.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_my_queue_intent",
            &json!({"p_intent":intent}),
            &["intent"],
            |value: &crate::matchmaking::QueueIntent, owner| value.validate(owner, intent),
        )
    }
    /// Published profile metadata does not establish native coverage or current game availability.
    pub(crate) fn table_profiles(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::matchmaking::TableProfilePage>, ActorError> {
        if after.is_some_and(|id| id.is_nil()) || !(1..=100).contains(&limit) {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_table_profiles",
            &json!({"p_after":after,"p_limit":limit}),
            &[],
            |page: &crate::matchmaking::TableProfilePage, owner| page.validate(owner, after, limit),
        )
    }
    /// Page immutable profile configuration ordinals without private layouts/artifact references.
    pub(crate) fn profile_configurations(
        &self,
        bearer: &str,
        profile: Uuid,
        after: Option<u16>,
        limit: usize,
    ) -> Result<ApiResponse<crate::matchmaking::ProfileConfigurationPage>, ActorError> {
        if profile.is_nil() || after.is_some_and(|index| index > 511) || !(1..=100).contains(&limit)
        {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_table_profile_configurations",
            &json!({"p_profile":profile,"p_after":after,"p_limit":limit}),
            &["profile"],
            |page: &crate::matchmaking::ProfileConfigurationPage, owner| {
                page.validate(owner, profile, after, limit)
            },
        )
    }
    /// Read only the caller's issuer-owned safe invitation metadata.
    pub(crate) fn invitations(
        &self,
        bearer: &str,
        room: Option<Uuid>,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::rooms::invitations::InvitationPage>, ActorError> {
        if [room, after].into_iter().flatten().any(|id| id.is_nil()) || !(1..=100).contains(&limit)
        {
            return Err(ActorError::Configuration);
        }
        let Some(owner) = self.resolve(bearer)?.account() else {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("account".into(), "unavailable".into())]),
            });
        };
        self.owner_projection(
            bearer,
            "read_my_room_invitations",
            &json!({"p_room":room,"p_after":after,"p_limit":limit}),
            |page: &crate::rooms::invitations::InvitationPage| {
                page.validate(owner, room, after, limit)
            },
        )
    }
    /// Read current host-only configuration through its self-authorizing projection.
    pub(crate) fn room_configuration(
        &self,
        bearer: &str,
        room: Uuid,
    ) -> Result<ApiResponse<crate::rooms::configuration::HostConfiguration>, ActorError> {
        if room.is_nil() {
            return Err(ActorError::Configuration);
        }
        let Some(owner) = self.resolve(bearer)?.account() else {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("account".into(), "unavailable".into())]),
            });
        };
        self.scoped_projection_keys(
            bearer,
            "read_host_room_configuration",
            &json!({"p_room":room}),
            &["room", "configuration"],
            |value: &crate::rooms::configuration::HostConfiguration| value.validate(owner, room),
        )
    }
    /// Read visible room summaries through original-bearer RLS.
    pub(crate) fn rooms(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::rooms::reads::RoomPage>, ActorError> {
        let Some(owner) = self.resolve(bearer)?.account() else {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("account".into(), "unavailable".into())]),
            });
        };
        self.store_page(
            bearer,
            "read_visible_rooms",
            after,
            limit,
            |page: &crate::rooms::reads::RoomPage| page.validate(owner, after, limit),
        )
    }
    /// Read one authorized room and a bounded current-slot snapshot.
    pub(crate) fn room(
        &self,
        bearer: &str,
        room: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::rooms::reads::RoomDetail>, ActorError> {
        if room.is_nil() || after.is_some_and(|id| id.is_nil()) || !(1..=100).contains(&limit) {
            return Err(ActorError::Configuration);
        }
        let Some(owner) = self.resolve(bearer)?.account() else {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("account".into(), "unavailable".into())]),
            });
        };
        self.scoped_projection(
            bearer,
            "read_visible_room",
            &json!({"p_room":room,"p_after":after,"p_limit":limit}),
            Some("room"),
            |detail: &crate::rooms::reads::RoomDetail| detail.validate(owner, room, after, limit),
        )
    }
    /// Read stored owner progression through original-bearer RLS.
    pub(crate) fn quests(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::progression::QuestPage>, ActorError> {
        let Some(owner) = self.resolve(bearer)?.account() else {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("account".into(), "unavailable".into())]),
            });
        };
        self.store_page(
            bearer,
            "read_my_quests",
            after,
            limit,
            |page: &crate::progression::QuestPage| page.validate(owner, after, limit),
        )
    }
    pub(crate) fn ranks(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
        format: Option<Uuid>,
        season: Option<Uuid>,
    ) -> Result<ApiResponse<crate::progression::RankPage>, ActorError> {
        if !(1..=100).contains(&limit)
            || [after, format, season]
                .into_iter()
                .flatten()
                .any(|id| id.is_nil())
        {
            return Err(ActorError::Configuration);
        }
        let Some(owner) = self.resolve(bearer)?.account() else {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("account".into(), "unavailable".into())]),
            });
        };
        self.owner_projection(
            bearer,
            "read_my_ranks",
            &json!({"p_after":after,"p_limit":limit,"p_format":format,"p_season":season}),
            |page: &crate::progression::RankPage| {
                page.validate(owner, after, limit, format, season)
            },
        )
    }
    pub(crate) fn seasons(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::progression::SeasonPage>, ActorError> {
        let Some(owner) = self.resolve(bearer)?.account() else {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("account".into(), "unavailable".into())]),
            });
        };
        self.store_page(
            bearer,
            "read_my_seasons",
            after,
            limit,
            |page: &crate::progression::SeasonPage| page.validate(owner, after, limit),
        )
    }
    /// Original-bearer safe projection; no worker/source authority is carried by the caller.
    pub(crate) fn match_list(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::match_history::MatchList>, ActorError> {
        if !(1..=100).contains(&limit) || after.is_some_and(|id| id.is_nil()) {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_my_matches",
            &json!({"p_after":after,"p_limit":limit}),
            &[],
            |page: &crate::match_history::MatchList, owner| page.validate(owner, after, limit),
        )
    }
    /// Original-bearer safe projection; no worker/source authority is carried by the caller.
    pub(crate) fn match_games(
        &self,
        bearer: &str,
        matched: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::match_history::GamePage>, ActorError> {
        if !(1..=100).contains(&limit) || after.is_some_and(|id| id.is_nil()) || matched.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_match_games",
            &json!({"p_match":matched,"p_after":after,"p_limit":limit}),
            &["match"],
            |page: &crate::match_history::GamePage, owner| {
                page.validate(owner, matched, after, limit)
            },
        )
    }
    /// Original-bearer safe projection; no worker/source authority is carried by the caller.
    pub(crate) fn match_seats(
        &self,
        bearer: &str,
        matched: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::match_history::SeatPage>, ActorError> {
        if !(1..=100).contains(&limit) || after.is_some_and(|id| id.is_nil()) || matched.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_match_seats",
            &json!({"p_match":matched,"p_after":after,"p_limit":limit}),
            &["match"],
            |page: &crate::match_history::SeatPage, owner| {
                page.validate(owner, matched, after, limit)
            },
        )
    }
    /// Original-bearer safe projection; no worker/source authority is carried by the caller.
    pub(crate) fn match_decisions(
        &self,
        bearer: &str,
        matched: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::match_history::DecisionPage>, ActorError> {
        if !(1..=100).contains(&limit) || after.is_some_and(|id| id.is_nil()) || matched.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_my_match_decisions",
            &json!({"p_match":matched,"p_after":after,"p_limit":limit}),
            &["match"],
            |page: &crate::match_history::DecisionPage, owner| {
                page.validate(owner, matched, after, limit)
            },
        )
    }
    /// Original-bearer safe projection; no worker/source authority is carried by the caller.
    pub(crate) fn daily_wins(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::competitive_history::DailyWinPage>, ActorError> {
        if !(1..=100).contains(&limit) || after.is_some_and(|id| id.is_nil()) {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_my_daily_wins",
            &json!({"p_after":after,"p_limit":limit}),
            &[],
            |page: &crate::competitive_history::DailyWinPage, owner| {
                page.validate(owner, after, limit)
            },
        )
    }
    /// Original-bearer safe projection; no worker/source authority is carried by the caller.
    pub(crate) fn ballot(
        &self,
        bearer: &str,
        season: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::supply_ballot::BallotPage>, ActorError> {
        if !(1..=100).contains(&limit) || after.is_some_and(|id| id.is_nil()) || season.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_supply_ballot",
            &json!({"p_season":season,"p_after":after,"p_limit":limit}),
            &["ballot"],
            |page: &crate::supply_ballot::BallotPage, owner| {
                page.validate(owner, season, after, limit)
            },
        )
    }
    /// Original-bearer safe projection; no worker/source authority is carried by the caller.
    pub(crate) fn room_ai_status(
        &self,
        bearer: &str,
        room: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<ApiResponse<crate::room_controls::AiStatusPage>, ActorError> {
        if !(1..=100).contains(&limit) || after.is_some_and(|id| id.is_nil()) || room.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_room_ai_status",
            &json!({"p_room":room,"p_after":after,"p_limit":limit}),
            &["room"],
            |page: &crate::room_controls::AiStatusPage, owner| {
                page.validate(owner, room, after, limit)
            },
        )
    }
    pub(crate) fn match_detail(
        &self,
        bearer: &str,
        matched: Uuid,
    ) -> Result<ApiResponse<crate::match_history::MatchDetail>, ActorError> {
        if matched.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_match_metadata",
            &json!({"p_match":matched}),
            &["match"],
            |detail: &crate::match_history::MatchDetail, owner| detail.validate(owner, matched),
        )
    }
    pub(crate) fn match_ante(
        &self,
        bearer: &str,
        matched: Uuid,
    ) -> Result<ApiResponse<crate::ante::AnteDetail>, ActorError> {
        if matched.is_nil() {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_match_ante",
            &json!({"p_match":matched}),
            &["match"],
            |detail: &crate::ante::AnteDetail, owner| detail.validate(owner, matched),
        )
    }
    pub(crate) fn competitive_results(
        &self,
        bearer: &str,
        after: Option<Uuid>,
        limit: usize,
        format: Option<Uuid>,
        season: Option<Uuid>,
    ) -> Result<ApiResponse<crate::competitive_history::CreditPage>, ActorError> {
        if !(1..=100).contains(&limit)
            || [after, format, season]
                .into_iter()
                .flatten()
                .any(|id| id.is_nil())
        {
            return Err(ActorError::Configuration);
        }
        self.account_projection(
            bearer,
            "read_my_competitive_results",
            &json!({"p_after":after,"p_limit":limit,"p_format":format,"p_season":season}),
            &[],
            |page: &crate::competitive_history::CreditPage, owner| {
                page.validate(owner, after, limit, format, season)
            },
        )
    }
    fn caller_rpc(
        &self,
        bearer: &str,
        name: &str,
        payload: &Value,
    ) -> Result<reqwest::blocking::Response, ActorError> {
        let mut header = HeaderValue::from_str(&format!("Bearer {bearer}"))
            .map_err(|_| ActorError::Unauthorized)?;
        header.set_sensitive(true);
        let response = self
            .http
            .rpc(name)
            .map_err(ActorError::caused)?
            .header("Authorization", header)
            .json(payload)
            .send()
            .map_err(|source| ActorError::caused(source.without_url()))?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(ActorError::Unauthorized);
        }
        if !response.status().is_success() {
            return Err(ActorError::Unavailable);
        }
        Ok(response)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RootAuthority {
    version: u8,
    method: SignInMethod,
    binding_id: Uuid,
    session_grant_id: Uuid,
}
#[derive(Deserialize)]
struct Claims {
    sub: Uuid,
    session_id: Uuid,
    iss: String,
    aud: Value,
    exp: u64,
    role: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    spellwake_auth: RootAuthority,
}
#[derive(Deserialize)]
struct TokenHeader {
    alg: String,
}
fn ensure_bearer_deadline(expiry: u64) -> Result<(), ActorError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ActorError::Unavailable)?
        .as_secs();
    if expiry <= now {
        Err(ActorError::Unauthorized)
    } else {
        Ok(())
    }
}
fn parse_claims(bearer: &str, issuer: &str) -> Result<Claims, ActorError> {
    let invalid = || ActorError::Unauthorized;
    if bearer.len() > MAX_BEARER || bearer.is_empty() {
        return Err(invalid());
    }
    let mut parts = bearer.split('.');
    let mut decode = || {
        let part = parts.next().ok_or_else(invalid)?;
        if part.is_empty() {
            return Err(invalid());
        }
        URL_SAFE_NO_PAD.decode(part).map_err(|_| invalid())
    };
    let header = serde_json::from_slice::<crate::provider_json::Object<TokenHeader>>(&decode()?)
        .map_err(|_| invalid())?
        .0;
    let claims: Claims = serde_json::from_slice::<crate::provider_json::Object<Claims>>(&decode()?)
        .map_err(|_| invalid())?
        .0;
    let signature = decode()?;
    if parts.next().is_some()
        || signature.is_empty()
        || header.alg.is_empty()
        || header.alg == "none"
    {
        return Err(invalid());
    }
    ensure_bearer_deadline(claims.exp)?;
    if claims.iss != issuer
        || claims.role != "authenticated"
        || (claims.aud != json!("authenticated") && claims.aud != json!(["authenticated"]))
        || claims.spellwake_auth.version != 1
        || claims.spellwake_auth.binding_id.is_nil()
        || claims.spellwake_auth.session_grant_id.is_nil()
        || claims.sub.is_nil()
        || claims.session_id.is_nil()
    {
        return Err(invalid());
    }
    Ok(claims)
}

// A custom wrapper requires the field to be present even when its value is null.
#[derive(Deserialize)]
#[serde(untagged)]
enum RequiredAccount {
    Present(#[serde(deserialize_with = "crate::identifiers::uuid")] Uuid),
    Absent(()),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SessionWire {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    subject_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    session_id: Uuid,
    method: SignInMethod,
    state: ActorState,
    account_id: RequiredAccount,
}
impl SessionWire {
    fn into_actor(self, local: Claims) -> Result<RequestActor, ActorError> {
        let account = match self.account_id {
            RequiredAccount::Present(id) if !id.is_nil() => Some(id),
            RequiredAccount::Absent(()) => None,
            _ => return Err(ActorError::Unavailable),
        };
        if self.subject_id != local.sub
            || self.session_id != local.session_id
            || self.method != local.spellwake_auth.method
            || (self.state == ActorState::NeedsRegistration) != account.is_none()
        {
            return Err(ActorError::Unavailable);
        }
        // Authentication/body IO may have crossed the original JWT deadline.
        // Never return a newly resolved actor based only on the pre-RPC check.
        ensure_bearer_deadline(local.exp)?;
        Ok(RequestActor {
            verified_bearer_expiry: local.exp,
            subject: self.subject_id,
            session: self.session_id,
            method: self.method,
            state: self.state,
            account,
        })
    }
}

#[cfg(test)]
#[path = "actor_tests.rs"]
mod tests;

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<SignInMethod>(&[
            "google",
            "apple",
            "discord",
            "email_code",
        ]);
        crate::test_http::string_enum_roundtrip::<ActorState>(&[
            "needs_registration",
            "active",
            "deletion_pending",
        ]);
    }
}
