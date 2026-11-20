//! Typed Stripe HTTPS facade. Supabase credentials never enter this client.
use super::{
    envelope::{self as field, Array, Object},
    provider_id,
};
use crate::http_transport::{HttpTransport, Origin, bounded_body, standard_client};
use chrono::{DateTime, Utc};
use reqwest::{
    Method,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(crate) const API_VERSION: &str = "2026-10-28.endive";

pub(crate) enum ClientError {
    Configuration,
    Network(reqwest::Error),
    Body(std::io::Error),
    Json(serde_json::Error),
    Status(u16),
    Contract,
    Deadline,
    Canceled,
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
impl fmt::Debug for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Status(code) = self {
            return write!(f, "stripe did not acknowledge the request (status {code})");
        }
        f.write_str(match self {
            Self::Configuration => "stripe configuration is invalid",
            Self::Network(_) => "stripe request could not be established",
            Self::Body(_) => "stripe body could not be read",
            Self::Json(_) => "stripe source shape is invalid",
            Self::Status(_) => "stripe did not acknowledge the request",
            Self::Contract => "stripe source does not match the purchase",
            Self::Deadline => "checkout creation acknowledgment remains unresolved",
            Self::Canceled => "stripe work was canceled before dispatch",
        })
    }
}
impl Error for ClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Network(e) => Some(e),
            Self::Body(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}
impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        Self::Network(e.without_url())
    }
}

#[derive(Clone)]
pub(crate) struct StripeConfig {
    pub secret: String,
    pub webhook_secret: String,
    pub merchant: String,
    pub live: bool,
    pub prices: BTreeMap<Uuid, String>,
    pub success_url: String,
    pub cancel_url: String,
    pub evidence_bucket: String,
    pub stopped: Arc<AtomicBool>,
    base: String,
}
impl StripeConfig {
    pub fn from_env() -> Result<Self, ClientError> {
        let get = |key| std::env::var(key).map_err(|_| ClientError::Configuration);
        let secret = get("SPELLWAKE_STRIPE_SECRET_KEY")?;
        let prices = serde_json::from_str(&get("SPELLWAKE_STRIPE_PRICES")?)
            .map_err(|_| ClientError::Configuration)?;
        // An explicit fixture override can only reach the constructor's
        // loopback HTTP/test-key gate. Live keys always use Stripe's origin.
        let fixture_base = std::env::var("SPELLWAKE_STRIPE_FIXTURE_BASE_URL").ok();
        if fixture_base.is_some()
            && std::env::var("SPELLWAKE_STRIPE_ALLOW_LOCAL_FIXTURE").as_deref() != Ok("1")
        {
            return Err(ClientError::Configuration);
        }
        let mut config = Self::new(
            secret,
            get("SPELLWAKE_STRIPE_WEBHOOK_SECRET")?,
            get("SPELLWAKE_STRIPE_ACCOUNT_ID")?,
            prices,
            get("SPELLWAKE_STRIPE_SUCCESS_URL")?,
            get("SPELLWAKE_STRIPE_CANCEL_URL")?,
            fixture_base.as_deref().unwrap_or("https://api.stripe.com/"),
        )?;
        config.evidence_bucket = get("SPELLWAKE_PAYMENT_EVIDENCE_BUCKET")?;
        if config.evidence_bucket.is_empty()
            || config.evidence_bucket.len() > 100
            || !config
                .evidence_bucket
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(ClientError::Configuration);
        }
        Ok(config)
    }
    pub fn new(
        secret: String,
        webhook_secret: String,
        merchant: String,
        prices: BTreeMap<Uuid, String>,
        success_url: String,
        cancel_url: String,
        base: &str,
    ) -> Result<Self, ClientError> {
        let live = secret.starts_with("sk_live_");
        let url = reqwest::Url::parse(base).map_err(|_| ClientError::Configuration)?;
        let fixture = matches!(url.host_str(), Some("127.0.0.1" | "::1" | "[::1]"))
            && url.scheme() == "http"
            && !live;
        if !(base == "https://api.stripe.com/" || fixture)
            || !(secret.starts_with("sk_test_") || live)
            || !(16..=512).contains(&secret.len())
            || !secret
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !provider_id(&merchant, "acct_")
            || prices.is_empty()
            || prices
                .iter()
                .any(|(id, price)| id.is_nil() || !provider_id(price, "price_"))
        {
            return Err(ClientError::Configuration);
        }
        Origin::new(base).map_err(|_| ClientError::Configuration)?;
        for target in [&success_url, &cancel_url] {
            let target = reqwest::Url::parse(target).map_err(|_| ClientError::Configuration)?;
            if target.scheme() != "https"
                || target.host_str().is_none()
                || !target.username().is_empty()
                || target.password().is_some()
                || target.fragment().is_some()
            {
                return Err(ClientError::Configuration);
            }
        }
        super::StripeWebhookVerifier::new(webhook_secret.clone(), API_VERSION.into(), live)
            .map_err(|_| ClientError::Configuration)?;
        Ok(Self {
            secret,
            webhook_secret,
            merchant,
            live,
            prices,
            success_url,
            cancel_url,
            evidence_bucket: "payment-evidence".into(),
            stopped: Arc::new(AtomicBool::new(false)),
            base: base.into(),
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CheckoutTerms {
    pub version: u8,
    pub price_id: String,
    pub currency: String,
    pub minor_units: String,
    pub live: bool,
    pub api_version: String,
    pub success_url: String,
    pub cancel_url: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Purchase {
    pub id: Uuid,
    pub account_id: Uuid,
    pub account_state: String,
    pub cosmetic_id: Uuid,
    pub state: String,
    pub revision: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub terms: CheckoutTerms,
    pub terms_hash: String,
    pub request_key: String,
    pub checkout_ref: Option<String>,
    pub payment_ref: Option<String>,
    pub deadline: Option<DateTime<Utc>>,
    pub reversal_event_id: Option<Uuid>,
    pub reversal_key: Option<String>,
}

#[derive(Deserialize)]
struct Merchant {
    #[serde(deserialize_with = "field::string")]
    id: String,
    #[serde(deserialize_with = "field::string")]
    object: String,
}
#[derive(Deserialize)]
struct Price {
    #[serde(deserialize_with = "field::string")]
    id: String,
    #[serde(deserialize_with = "field::string")]
    object: String,
    #[serde(deserialize_with = "field::boolean")]
    active: bool,
    #[serde(deserialize_with = "field::boolean")]
    livemode: bool,
    #[serde(deserialize_with = "field::string")]
    currency: String,
    #[serde(deserialize_with = "field::optional_integer")]
    unit_amount: Option<i64>,
    #[serde(deserialize_with = "field::string")]
    billing_scheme: String,
    #[serde(rename = "type", deserialize_with = "field::string")]
    kind: String,
    #[serde(default, deserialize_with = "field::null")]
    #[serde(rename = "recurring")]
    _recurring: (),
    #[serde(default, deserialize_with = "field::null")]
    #[serde(rename = "custom_unit_amount")]
    _custom_unit_amount: (),
    #[serde(default, deserialize_with = "field::null")]
    #[serde(rename = "transform_quantity")]
    _transform_quantity: (),
}
#[derive(Deserialize)]
pub(super) struct Metadata {
    #[serde(default, deserialize_with = "field::optional_string")]
    pub spellwake_purchase_id: Option<String>,
}
#[derive(Deserialize)]
pub(super) struct Checkout {
    #[serde(deserialize_with = "field::string")]
    pub id: String,
    #[serde(deserialize_with = "field::string")]
    object: String,
    #[serde(deserialize_with = "field::string")]
    pub status: String,
    #[serde(deserialize_with = "field::string")]
    pub payment_status: String,
    #[serde(deserialize_with = "field::string")]
    mode: String,
    #[serde(deserialize_with = "field::boolean")]
    livemode: bool,
    #[serde(deserialize_with = "field::optional_string")]
    pub payment_intent: Option<String>,
    #[serde(deserialize_with = "field::optional_string")]
    pub client_reference_id: Option<String>,
    #[serde(deserialize_with = "field::optional_string")]
    currency: Option<String>,
    #[serde(deserialize_with = "field::optional_integer")]
    amount_total: Option<i64>,
    #[serde(deserialize_with = "field::optional_string")]
    pub url: Option<String>,
    #[serde(deserialize_with = "field::integer")]
    pub expires_at: i64,
    pub metadata: Object<Metadata>,
}
#[derive(Deserialize)]
struct Line {
    #[serde(deserialize_with = "field::string")]
    id: String,
    #[serde(deserialize_with = "field::integer")]
    quantity: i64,
    price: Object<Price>,
}
#[derive(Deserialize)]
struct Collection<T> {
    #[serde(deserialize_with = "field::string")]
    object: String,
    data: Array<Object<T>>,
    #[serde(deserialize_with = "field::boolean")]
    has_more: bool,
}
#[derive(Deserialize)]
pub(super) struct Payment {
    #[serde(deserialize_with = "field::string")]
    pub id: String,
    #[serde(deserialize_with = "field::string")]
    object: String,
    #[serde(deserialize_with = "field::string")]
    pub status: String,
    #[serde(deserialize_with = "field::integer")]
    pub amount: i64,
    #[serde(deserialize_with = "field::integer")]
    pub amount_received: i64,
    #[serde(deserialize_with = "field::string")]
    pub currency: String,
    #[serde(deserialize_with = "field::boolean")]
    livemode: bool,
    #[serde(deserialize_with = "field::optional_string")]
    pub latest_charge: Option<String>,
    pub metadata: Object<Metadata>,
}
#[derive(Deserialize)]
pub(super) struct Charge {
    #[serde(deserialize_with = "field::string")]
    pub id: String,
    #[serde(deserialize_with = "field::string")]
    object: String,
    #[serde(deserialize_with = "field::optional_string")]
    pub payment_intent: Option<String>,
    #[serde(deserialize_with = "field::integer")]
    pub amount: i64,
    #[serde(deserialize_with = "field::integer")]
    pub amount_captured: i64,
    #[serde(deserialize_with = "field::integer")]
    pub amount_refunded: i64,
    #[serde(deserialize_with = "field::string")]
    pub currency: String,
    #[serde(deserialize_with = "field::boolean")]
    pub paid: bool,
    #[serde(deserialize_with = "field::boolean")]
    pub captured: bool,
    #[serde(deserialize_with = "field::boolean")]
    livemode: bool,
}
#[derive(Deserialize)]
pub(super) struct Refund {
    #[serde(deserialize_with = "field::string")]
    pub id: String,
    #[serde(deserialize_with = "field::string")]
    object: String,
    #[serde(deserialize_with = "field::optional_string")]
    pub payment_intent: Option<String>,
    #[serde(deserialize_with = "field::optional_string")]
    pub charge: Option<String>,
    #[serde(deserialize_with = "field::string")]
    pub status: String,
    #[serde(deserialize_with = "field::integer")]
    pub amount: i64,
    #[serde(deserialize_with = "field::string")]
    pub currency: String,
    #[serde(default, deserialize_with = "field::optional_string")]
    pub failure_balance_transaction: Option<String>,
}
#[derive(Deserialize)]
pub(super) struct RefundReturn {
    #[serde(deserialize_with = "field::string")]
    pub id: String,
    #[serde(deserialize_with = "field::string")]
    object: String,
    #[serde(deserialize_with = "field::optional_string")]
    pub source: Option<String>,
    #[serde(rename = "type", deserialize_with = "field::string")]
    pub kind: String,
    #[serde(deserialize_with = "field::integer")]
    pub amount: i64,
    #[serde(deserialize_with = "field::string")]
    pub currency: String,
}
#[derive(Deserialize)]
pub(super) struct Movement {
    #[serde(deserialize_with = "field::string")]
    pub id: String,
    #[serde(deserialize_with = "field::string")]
    pub currency: String,
    #[serde(deserialize_with = "field::integer")]
    pub amount: i64,
    #[serde(deserialize_with = "field::integer")]
    pub created: i64,
}
#[derive(Deserialize)]
pub(super) struct Dispute {
    #[serde(deserialize_with = "field::string")]
    pub id: String,
    #[serde(deserialize_with = "field::string")]
    object: String,
    #[serde(deserialize_with = "field::string")]
    pub charge: String,
    #[serde(deserialize_with = "field::optional_string")]
    pub payment_intent: Option<String>,
    #[serde(deserialize_with = "field::string")]
    pub status: String,
    #[serde(deserialize_with = "field::boolean")]
    livemode: bool,
    pub balance_transactions: Array<Object<Movement>>,
}

pub(super) struct StripeClient {
    http: HttpTransport,
    config: StripeConfig,
    deadline: Instant,
}
impl StripeClient {
    pub fn find_checkout(&self, purchase: &Purchase) -> Result<Option<Checkout>, ClientError> {
        let Some(deadline) = purchase.deadline else {
            return Ok(None);
        };
        let from = (deadline - chrono::Duration::hours(23)).timestamp();
        let mut cursor: Option<String> = None;
        for _ in 0..10 {
            let mut query = vec![("created[gte]", from.to_string()), ("limit", "100".into())];
            if let Some(id) = &cursor {
                query.push(("starting_after", id.clone()));
            }
            let page: Collection<Checkout> = self.get("v1/checkout/sessions", &query)?;
            if page.object != "list" || page.data.0.len() > 100 {
                return Err(ClientError::Contract);
            }
            let next = page.data.0.last().map(|x| x.0.id.clone());
            for row in page.data.0 {
                let checkout = row.0;
                if checkout.client_reference_id.as_deref() == Some(purchase.id.to_string().as_str())
                {
                    self.match_checkout(&checkout, purchase)?;
                    return Ok(Some(checkout));
                }
            }
            if !page.has_more {
                return Ok(None);
            }
            if next.is_none() || next == cursor {
                return Err(ClientError::Contract);
            }
            cursor = next;
        }
        Ok(None) // Incomplete/empty search never proves cancellation.
    }
    pub fn disputes(&self, payment: &str) -> Result<Vec<Dispute>, ClientError> {
        if !provider_id(payment, "pi_") {
            return Err(ClientError::Contract);
        }
        let mut results = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..10 {
            let mut query = vec![
                ("payment_intent", payment.to_owned()),
                ("limit", "100".into()),
            ];
            if let Some(id) = &cursor {
                query.push(("starting_after", id.clone()));
            }
            let page: Collection<Dispute> = self.get("v1/disputes", &query)?;
            if page.object != "list" || page.data.0.len() > 100 {
                return Err(ClientError::Contract);
            }
            let next = page.data.0.last().map(|x| x.0.id.clone());
            for row in page.data.0 {
                let row = row.0;
                if !provider_id(&row.id, "du_")
                    || row.object != "dispute"
                    || row.livemode != self.config.live
                    || row.payment_intent.as_deref() != Some(payment)
                    || results.iter().any(|d: &Dispute| d.id == row.id)
                {
                    return Err(ClientError::Contract);
                }
                results.push(row);
            }
            if !page.has_more {
                return Ok(results);
            }
            if next.is_none() || next == cursor {
                return Err(ClientError::Contract);
            }
            cursor = next;
        }
        Err(ClientError::Contract)
    }
    pub fn new(config: StripeConfig) -> Result<Self, ClientError> {
        let mut headers = HeaderMap::new();
        let mut auth = HeaderValue::from_str(&format!("Bearer {}", config.secret))
            .map_err(|_| ClientError::Configuration)?;
        auth.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth);
        headers.insert("stripe-version", HeaderValue::from_static(API_VERSION));
        let client = standard_client(headers, "spellwake-stripe", Duration::from_secs(30))?;
        let http = HttpTransport::from_client(
            Origin::new(&config.base).map_err(|_| ClientError::Configuration)?,
            client,
        );
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(90))
            .ok_or(ClientError::Configuration)?;
        let client = Self {
            http,
            config,
            deadline,
        };
        let merchant: Merchant = client.get("v1/account", &[])?;
        if merchant.object != "account" || merchant.id != client.config.merchant {
            return Err(ClientError::Contract);
        }
        Ok(client)
    }
    fn decode<T: DeserializeOwned>(
        &self,
        response: reqwest::blocking::Response,
    ) -> Result<T, ClientError> {
        if !response.status().is_success() {
            return Err(ClientError::Status(response.status().as_u16()));
        }
        let bytes = bounded_body(response, 1024 * 1024).map_err(ClientError::Body)?;
        serde_json::from_slice::<Object<T>>(&bytes)
            .map(|x| x.0)
            .map_err(ClientError::Json)
    }
    fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, ClientError> {
        let url = self
            .http
            .url(path)
            .map_err(|_| ClientError::Configuration)?;
        let response = self
            .http
            .request(Method::GET, url)
            .map_err(|_| ClientError::Configuration)?
            .query(query)
            .timeout(self.remaining()?)
            .send()?;
        self.decode(response)
    }
    fn remaining(&self) -> Result<Duration, ClientError> {
        if self.config.stopped.load(Ordering::Acquire) {
            return Err(ClientError::Canceled);
        }
        let left = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or(ClientError::Deadline)?;
        if left.is_zero() {
            return Err(ClientError::Deadline);
        }
        Ok(left.min(Duration::from_secs(30)))
    }
    pub fn terms(&self, cosmetic: Uuid) -> Result<CheckoutTerms, ClientError> {
        let id = self
            .config
            .prices
            .get(&cosmetic)
            .ok_or(ClientError::Configuration)?;
        let price: Price = self.get(&format!("v1/prices/{id}"), &[])?;
        self.check_price(&price, id, true)?;
        Ok(CheckoutTerms {
            version: 1,
            price_id: id.clone(),
            currency: price.currency,
            minor_units: price.unit_amount.ok_or(ClientError::Contract)?.to_string(),
            live: self.config.live,
            api_version: API_VERSION.into(),
            success_url: self.config.success_url.clone(),
            cancel_url: self.config.cancel_url.clone(),
        })
    }
    fn check_price(&self, price: &Price, id: &str, new: bool) -> Result<(), ClientError> {
        if price.object != "price"
            || price.id != id
            || price.livemode != self.config.live
            || (new && !price.active)
            || price.billing_scheme != "per_unit"
            || price.kind != "one_time"
            || !price
                .unit_amount
                .is_some_and(|n| (1..=99_999_999).contains(&n))
            || price.currency.len() != 3
            || !price.currency.bytes().all(|b| b.is_ascii_lowercase())
        {
            return Err(ClientError::Contract);
        }
        Ok(())
    }
    pub fn create(&self, purchase: &Purchase) -> Result<Checkout, ClientError> {
        if purchase.account_id.is_nil()
            || purchase.cosmetic_id.is_nil()
            || purchase.terms.version != 1
        {
            return Err(ClientError::Contract);
        }
        if purchase.deadline.is_none_or(|d| d <= Utc::now()) || purchase.state != "creating" {
            return Err(ClientError::Deadline);
        }
        if purchase.terms.api_version != API_VERSION || purchase.terms.live != self.config.live {
            return Err(ClientError::Contract);
        }
        let form = vec![
            ("mode", "payment".into()),
            ("line_items[0][price]", purchase.terms.price_id.clone()),
            ("line_items[0][quantity]", "1".into()),
            ("client_reference_id", purchase.id.to_string()),
            ("metadata[spellwake_purchase_id]", purchase.id.to_string()),
            (
                "payment_intent_data[metadata][spellwake_purchase_id]",
                purchase.id.to_string(),
            ),
            ("success_url", purchase.terms.success_url.clone()),
            ("cancel_url", purchase.terms.cancel_url.clone()),
        ];
        let response = self
            .http
            .request(
                Method::POST,
                self.http
                    .url("v1/checkout/sessions")
                    .map_err(|_| ClientError::Configuration)?,
            )
            .map_err(|_| ClientError::Configuration)?
            .header("Idempotency-Key", &purchase.request_key)
            .form(&form)
            .timeout(self.remaining()?)
            .send()?;
        let checkout: Checkout = self.decode(response)?;
        self.match_checkout(&checkout, purchase)?;
        Ok(checkout)
    }
    pub fn checkout(&self, id: &str) -> Result<Checkout, ClientError> {
        if !provider_id(id, "cs_") {
            return Err(ClientError::Contract);
        }
        let c: Checkout = self.get(&format!("v1/checkout/sessions/{id}"), &[])?;
        if c.id != id
            || c.object != "checkout.session"
            || c.livemode != self.config.live
            || c.mode != "payment"
        {
            return Err(ClientError::Contract);
        }
        Ok(c)
    }
    pub fn match_checkout(&self, c: &Checkout, p: &Purchase) -> Result<(), ClientError> {
        if c.object != "checkout.session"
            || !provider_id(&c.id, "cs_")
            || c.livemode != p.terms.live
            || c.mode != "payment"
            || c.client_reference_id.as_deref() != Some(p.id.to_string().as_str())
            || c.metadata.0.spellwake_purchase_id.as_deref() != Some(p.id.to_string().as_str())
            || p.checkout_ref.as_ref().is_some_and(|id| id != &c.id)
            || c.currency.as_deref() != Some(&p.terms.currency)
            || c.amount_total.map(|n| n.to_string()).as_deref() != Some(&p.terms.minor_units)
            || p.payment_ref
                .as_ref()
                .is_some_and(|id| c.payment_intent.as_ref() != Some(id))
        {
            return Err(ClientError::Contract);
        }
        let lines: Collection<Line> = self.get(
            &format!("v1/checkout/sessions/{}/line_items", c.id),
            &[("limit", "2".into())],
        )?;
        if lines.object != "list" || lines.has_more || lines.data.0.len() != 1 {
            return Err(ClientError::Contract);
        }
        let line = &lines.data.0[0].0;
        self.check_price(&line.price.0, &p.terms.price_id, false)?;
        if !provider_id(&line.id, "li_")
            || line.quantity != 1
            || line.price.0.unit_amount.map(|n| n.to_string()).as_deref()
                != Some(&p.terms.minor_units)
            || line.price.0.currency != p.terms.currency
        {
            return Err(ClientError::Contract);
        }
        if let Some(url) = &c.url {
            let u = reqwest::Url::parse(url).map_err(|_| ClientError::Contract)?;
            if u.scheme() != "https"
                || u.origin().ascii_serialization() != "https://checkout.stripe.com"
                || !u.username().is_empty()
                || u.password().is_some()
            {
                return Err(ClientError::Contract);
            }
        }
        Ok(())
    }
    pub fn payment(&self, id: &str) -> Result<Payment, ClientError> {
        if !provider_id(id, "pi_") {
            return Err(ClientError::Contract);
        }
        let p: Payment = self.get(&format!("v1/payment_intents/{id}"), &[])?;
        if p.id != id || p.object != "payment_intent" || p.livemode != self.config.live {
            return Err(ClientError::Contract);
        }
        Ok(p)
    }
    pub fn charge(&self, id: &str) -> Result<Charge, ClientError> {
        if !provider_id(id, "ch_") {
            return Err(ClientError::Contract);
        }
        let c: Charge = self.get(&format!("v1/charges/{id}"), &[])?;
        if c.id != id || c.object != "charge" || c.livemode != self.config.live {
            return Err(ClientError::Contract);
        }
        Ok(c)
    }
    pub fn refund(&self, id: &str) -> Result<Refund, ClientError> {
        if !provider_id(id, "re_") {
            return Err(ClientError::Contract);
        }
        let r: Refund = self.get(&format!("v1/refunds/{id}"), &[])?;
        if r.id != id || r.object != "refund" {
            return Err(ClientError::Contract);
        }
        Ok(r)
    }
    pub fn refund_return(&self, id: &str) -> Result<RefundReturn, ClientError> {
        if !provider_id(id, "txn_") {
            return Err(ClientError::Contract);
        }
        let movement: RefundReturn = self.get(&format!("v1/balance_transactions/{id}"), &[])?;
        if movement.id != id || movement.object != "balance_transaction" {
            return Err(ClientError::Contract);
        }
        Ok(movement)
    }
    pub fn dispute(&self, id: &str) -> Result<Dispute, ClientError> {
        if !provider_id(id, "du_") {
            return Err(ClientError::Contract);
        }
        let d: Dispute = self.get(&format!("v1/disputes/{id}"), &[])?;
        if d.id != id || d.object != "dispute" || d.livemode != self.config.live {
            return Err(ClientError::Contract);
        }
        Ok(d)
    }
    pub fn refunds(&self, payment: &str) -> Result<Vec<Refund>, ClientError> {
        if !provider_id(payment, "pi_") {
            return Err(ClientError::Contract);
        }
        let mut rows = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..10 {
            let mut query = vec![
                ("payment_intent", payment.to_owned()),
                ("limit", "100".into()),
            ];
            if let Some(c) = &cursor {
                query.push(("starting_after", c.clone()));
            }
            let page: Collection<Refund> = self.get("v1/refunds", &query)?;
            if page.object != "list" || page.data.0.len() > 100 {
                return Err(ClientError::Contract);
            }
            let more = page.has_more;
            let next = page.data.0.last().map(|x| x.0.id.clone());
            for r in page.data.0 {
                let r = r.0;
                if !provider_id(&r.id, "re_")
                    || r.object != "refund"
                    || r.payment_intent.as_deref() != Some(payment)
                    || rows.iter().any(|prior: &Refund| prior.id == r.id)
                {
                    return Err(ClientError::Contract);
                }
                rows.push(r);
            }
            if !more {
                return Ok(rows);
            }
            if next.is_none() || next == cursor {
                return Err(ClientError::Contract);
            }
            cursor = next;
        }
        Err(ClientError::Contract)
    }
    pub fn expire(&self, id: &str, key: &str) -> Result<Checkout, ClientError> {
        if !provider_id(id, "cs_") {
            return Err(ClientError::Contract);
        }
        let response = self
            .http
            .request(
                Method::POST,
                self.http
                    .url(&format!("v1/checkout/sessions/{id}/expire"))
                    .map_err(|_| ClientError::Configuration)?,
            )
            .map_err(|_| ClientError::Configuration)?
            .header("Idempotency-Key", key)
            .timeout(self.remaining()?)
            .send()?;
        let c: Checkout = self.decode(response)?;
        if c.id != id || c.status != "expired" || c.livemode != self.config.live {
            return Err(ClientError::Contract);
        }
        Ok(c)
    }
}
