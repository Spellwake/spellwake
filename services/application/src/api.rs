//! Caller-scoped HTTP boundary. Blocking credentials and clients never enter logs.
use crate::{
    ApiResponse,
    actor::{ActorClient, ActorConfig, ActorError, ActorState, RequestActor, SignInMethod},
    backend::{BackendClient, BackendConfig},
    stripe::{
        MAX_WEBHOOK_BODY, StripeWebhookVerifier, client::StripeConfig, service::StripeService,
    },
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tower_http::cors::CorsLayer;
use uuid::Uuid;
mod admission_receipt_routes;
mod ante_routes;
mod ballot_routes;
mod catalogue_routes;
mod chat_notification_routes;
mod command_routes;
mod commander_admission_routes;
mod deck_routes;
mod deletion_routes;
mod economy_routes;
mod history_routes;
mod import_routes;
mod invitation_routes;
mod matchmaking_routes;
mod native_command_routes;
mod native_delivery_routes;
mod native_history_routes;
mod native_receipt_routes;
mod native_reconnect_routes;
mod native_replay_routes;
mod native_stream_routes;
mod native_view_routes;
mod opening_choice_routes;
mod patreon_routes;
mod prediction_routes;
mod profile_routes;
mod progression_routes;
mod quest_reroll_routes;
mod room_configuration_routes;
mod room_control_routes;
mod room_routes;
mod social_routes;
mod ui_surface_routes;

/// Extractor errors and identifier spellings share each route's safe JSON failure mapping.
type IdentifierPath = Result<Path<String>, axum::extract::rejection::PathRejection>;
fn identifier(path: IdentifierPath) -> Result<Uuid, &'static str> {
    let raw = path.map_err(|_| "invalid")?.0;
    crate::identifiers::parse(&raw).map_err(|_| "invalid")
}

/// Public command bodies are JSON objects, including commands with object-shaped payloads.
fn parse_object<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    crate::decode_wire_object(bytes)
}

/// Validated startup configuration. Deliberately neither Debug nor serializable.
pub struct ApiConfig {
    base: String,
    public_key: String,
    secret_key: String,
    issuer: String,
    defaults: [Uuid; 3],
    origins: Vec<HeaderValue>,
    stripe: Option<StripeConfig>,
    temporal: Option<crate::temporal_runtime::TemporalConfig>,
    patreon: Option<Arc<crate::patreon::oauth::OAuthConfig>>,
    patreon_intake: Option<Arc<crate::patreon::intake::IntakeConfig>>,
    invitation_key: Option<Arc<crate::rooms::invitations::InvitationKey>>,
    commander_admission: Option<Arc<crate::native_game::CommanderRelease>>,
    admission: Option<Arc<crate::native_game::AdmissionManifest>>,
    native_views: Option<Arc<crate::native_game::NativeGameRuntime>>,
    native_host_owner: Option<crate::native_game::NativeHostOwner>,
}
impl ApiConfig {
    pub fn new(
        base: String,
        public_key: String,
        secret_key: String,
        issuer: String,
        defaults: [Uuid; 3],
        origins: Vec<String>,
    ) -> Result<Self, ActorError> {
        ActorConfig::new(&base, &public_key, &issuer)?;
        BackendConfig::new(&base, &secret_key).map_err(|_| ActorError::Configuration)?;
        if defaults.iter().any(Uuid::is_nil) {
            return Err(ActorError::Configuration);
        }
        let origins = origins
            .into_iter()
            .map(|origin| {
                // The bundled WebKit host uses this exact non-HTTP caller origin.
                // It is never a credential-bearing backend target URL.
                if origin == "tauri://localhost" {
                    return Ok(HeaderValue::from_static("tauri://localhost"));
                }
                let url = reqwest::Url::parse(&origin).map_err(|_| ActorError::Configuration)?;
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || url.origin().ascii_serialization() != origin
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || url.path() != "/"
                {
                    return Err(ActorError::Configuration);
                }
                HeaderValue::from_str(&origin).map_err(|_| ActorError::Configuration)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            base,
            public_key,
            secret_key,
            issuer,
            defaults,
            origins,
            stripe: None,
            temporal: None,
            patreon: None,
            patreon_intake: None,
            invitation_key: None,
            commander_admission: None,
            admission: None,
            native_views: None,
            native_host_owner: None,
        })
    }

    pub fn from_env() -> Result<Self, ActorError> {
        let required = |name| std::env::var(name).map_err(|_| ActorError::Configuration);
        let id = |name| {
            required(name)?
                .parse()
                .map_err(|_| ActorError::Configuration)
        };
        let mut config = Self::new(
            required("SPELLWAKE_SUPABASE_URL")?,
            required("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY")?,
            required("SPELLWAKE_SUPABASE_SECRET_KEY")?,
            required("SPELLWAKE_AUTH_ISSUER")?,
            [
                id("SPELLWAKE_FREE_BACK_ID")?,
                id("SPELLWAKE_FREE_SLEEVE_ID")?,
                id("SPELLWAKE_FREE_DICE_ID")?,
            ],
            std::env::var("SPELLWAKE_API_ORIGINS")
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        )?;
        if std::env::var_os("SPELLWAKE_STRIPE_SECRET_KEY").is_some() {
            config.stripe = Some(StripeConfig::from_env().map_err(|_| ActorError::Configuration)?);
        }
        if [
            "SPELLWAKE_TEMPORAL_ADDRESS",
            "SPELLWAKE_TEMPORAL_NAMESPACE",
            "SPELLWAKE_TEMPORAL_API_KEY",
        ]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
        {
            config.temporal = Some(
                crate::temporal_runtime::TemporalConfig::from_env()
                    .map_err(|_| ActorError::Configuration)?,
            );
        }
        if [
            "SPELLWAKE_PATREON_CLIENT_ID",
            "SPELLWAKE_PATREON_CLIENT_SECRET",
            "SPELLWAKE_PATREON_REDIRECT_URI",
        ]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
        {
            config.patreon = Some(Arc::new(
                crate::patreon::oauth::OAuthConfig::from_env()
                    .map_err(|_| ActorError::Configuration)?,
            ));
        }
        if [
            "SPELLWAKE_PATREON_WEBHOOK_SECRET",
            "SPELLWAKE_PATREON_EVIDENCE_BUCKET",
        ]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
        {
            config.patreon_intake = Some(Arc::new(
                crate::patreon::intake::IntakeConfig::from_env()
                    .map_err(|_| ActorError::Configuration)?,
            ));
        }
        if std::env::var_os("SPELLWAKE_ROOM_INVITATION_KEY").is_some() {
            config.invitation_key = Some(Arc::new(
                crate::rooms::invitations::InvitationKey::new(&required(
                    "SPELLWAKE_ROOM_INVITATION_KEY",
                )?)
                .map_err(|_| ActorError::Configuration)?,
            ));
        }
        config.commander_admission =
            crate::native_game::CommanderRelease::from_env()?.map(Arc::new);
        config.admission = crate::native_game::AdmissionManifest::from_env()?.map(Arc::new);
        config.native_views =
            crate::native_game::NativeGameRuntime::from_env(&config.base, &config.secret_key)
                .map_err(|_| ActorError::Configuration)?
                .map(Arc::new);
        if (config.admission.is_some() || config.commander_admission.is_some())
            && config.native_views.is_none()
        {
            return Err(ActorError::Configuration);
        }
        if let Some(value) = std::env::var_os("SPELLWAKE_NATIVE_HOST_OWNER") {
            let value = value.into_string().map_err(|_| ActorError::Configuration)?;
            config.native_host_owner = Some(
                crate::native_game::NativeHostOwner::new(&value)
                    .map_err(|_| ActorError::Configuration)?,
            );
        }
        Ok(config)
    }

    /// Install a dedicated server-only invitation key; retain it across restarts and promotions.
    pub fn with_invitation_key(mut self, key: crate::rooms::invitations::InvitationKey) -> Self {
        self.invitation_key = Some(Arc::new(key));
        self
    }

    /// Install immutable server-only native view dependencies, without gameplay admission.
    pub fn with_native_views(mut self, runtime: crate::native_game::NativeGameRuntime) -> Self {
        self.native_views = Some(Arc::new(runtime));
        self
    }

    /// Enable fresh command execution only for this explicit server owner.
    /// This does not acquire a lease or admit a game; original recovery needs none.
    pub fn with_native_host_owner(mut self, owner: crate::native_game::NativeHostOwner) -> Self {
        self.native_host_owner = Some(owner);
        self
    }

    fn actor(&self) -> Result<ActorClient, ActorError> {
        ActorClient::new(ActorConfig::new(
            &self.base,
            &self.public_key,
            &self.issuer,
        )?)
    }

    pub(super) fn backend(
        &self,
        user_agent: &'static str,
    ) -> Result<BackendClient, crate::backend::BackendError> {
        BackendClient::new(
            BackendConfig::new(&self.base, &self.secret_key)?,
            user_agent,
            Duration::from_secs(60),
        )
    }
}

const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 256;
const DEFAULT_MAX_CONCURRENT_STREAMS: usize = 512;
const PERMIT_WAIT_TIMEOUT: Duration = Duration::from_millis(50);

fn configured_limit(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| (1..=4096).contains(&n))
        .unwrap_or(default)
}

type RateLimitBuckets = HashMap<(String, bool), (f64, Instant)>;

#[derive(Clone)]
struct RateLimiter {
    buckets: Arc<Mutex<RateLimitBuckets>>,
    general_per_min: f64,
    unauth_per_min: f64,
}

impl RateLimiter {
    fn from_env() -> Self {
        let general = std::env::var("SPELLWAKE_API_RATE_LIMIT_PER_MIN")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(600);
        let unauth = std::env::var("SPELLWAKE_API_UNAUTH_RATE_LIMIT_PER_MIN")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(20);
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            general_per_min: f64::from(general),
            unauth_per_min: f64::from(unauth),
        }
    }

    fn allow(&self, client_key: &str, unauthenticated_sensitive: bool) -> bool {
        let capacity = if unauthenticated_sensitive {
            self.unauth_per_min
        } else {
            self.general_per_min
        };
        if capacity <= 0.0 {
            return true;
        }
        let refill_per_sec = capacity / 60.0;
        let now = Instant::now();
        let Ok(mut guard) = self.buckets.lock() else {
            return true;
        };
        if guard.len() > 8192 {
            guard
                .retain(|_, (_, ts)| now.saturating_duration_since(*ts) < Duration::from_secs(120));
        }
        let entry = guard
            .entry((client_key.to_owned(), unauthenticated_sensitive))
            .or_insert((capacity, now));
        let elapsed = now.saturating_duration_since(entry.1).as_secs_f64();
        entry.0 = (entry.0 + elapsed * refill_per_sec).min(capacity);
        entry.1 = now;
        if entry.0 >= 1.0 {
            entry.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

fn client_ip_key(request: &Request) -> Option<String> {
    if let Some(ip) = request
        .headers()
        .get("fly-client-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| s.parse::<IpAddr>().is_ok())
    {
        return Some(ip.to_owned());
    }
    if let Some(ip) = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .filter(|s| s.parse::<IpAddr>().is_ok())
    {
        return Some(ip.to_owned());
    }
    request
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string())
}

#[derive(Clone)]
struct ApiState {
    config: Arc<ApiConfig>,
    permits: Arc<Semaphore>,
    stream_permits: Arc<Semaphore>,
}
impl ApiState {
    fn new(config: ApiConfig) -> Self {
        Self {
            config: Arc::new(config),
            permits: Arc::new(Semaphore::new(configured_limit(
                "SPELLWAKE_API_MAX_CONCURRENT_REQUESTS",
                DEFAULT_MAX_CONCURRENT_REQUESTS,
            ))),
            stream_permits: Arc::new(Semaphore::new(configured_limit(
                "SPELLWAKE_API_MAX_CONCURRENT_STREAMS",
                DEFAULT_MAX_CONCURRENT_STREAMS,
            ))),
        }
    }
    /// The permit belongs to the actual blocking closure, including after caller cancellation.
    async fn blocking_value<T, F>(&self, work: F) -> Result<T, ()>
    where
        T: Send + 'static,
        F: FnOnce(&ApiConfig) -> T + Send + 'static,
    {
        let permit =
            match tokio::time::timeout(PERMIT_WAIT_TIMEOUT, self.permits.clone().acquire_owned())
                .await
            {
                Ok(Ok(permit)) => permit,
                _ => return Err(()),
            };
        let config = self.config.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work(&config)
        })
        .await
        .map_err(|_| ())
    }
    async fn blocking<F>(&self, work: F) -> Response
    where
        F: FnOnce(&ApiConfig) -> Response + Send + 'static,
    {
        self.blocking_value(work)
            .await
            .unwrap_or_else(|()| unavailable())
    }
}

/// Build one bounded single-write-path API; no background credentials are activated.
pub fn router(config: ApiConfig) -> Router {
    router_with_state(ApiState::new(config))
}
fn router_with_state(state: ApiState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(state.config.origins.clone())
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);
    Router::new()
        .route(
            "/health",
            get(|State(state): State<ApiState>| async move {
                success(json!({
                    "service": "spellwake-application",
                    "gameplayEnabled": state.config.native_views.is_some(),
                }))
            }),
        )
        .route("/v1/matches", get(history_routes::matches))
        .route("/v1/matches/{match}", get(history_routes::detail))
        .route("/v1/matches/{match}/games", get(history_routes::games))
        .route("/v1/matches/{match}/seats", get(history_routes::seats))
        .route(
            "/v1/games/{game}/receipts",
            post(native_receipt_routes::read),
        )
        .route(
            "/v1/games/{game}/opening-choice",
            post(opening_choice_routes::handle),
        )
        .route("/v1/games/{game}/view", post(native_view_routes::read))
        .route("/v1/games/{game}/stream", post(native_stream_routes::read))
        .route(
            "/v1/games/{game}/reconnect",
            post(native_reconnect_routes::read),
        )
        .route(
            "/v1/games/{game}/history",
            post(native_history_routes::read),
        )
        .route("/v1/games/{game}/replay", post(native_replay_routes::read))
        .route(
            "/v1/games/{game}/commands",
            post(native_command_routes::execute),
        )
        .route(
            "/v1/matches/{match}/decisions",
            get(history_routes::decisions),
        )
        .route(
            "/v1/matches/{match}/ante",
            get(history_routes::ante).post(ante_routes::offer),
        )
        .route("/v1/competitive-results", get(history_routes::competitive))
        .route("/v1/daily-wins", get(history_routes::daily_wins))
        .route(
            "/v1/seasons/{season}/ballot",
            get(history_routes::ballot).post(ballot_routes::replace),
        )
        .route(
            "/v1/rooms/{room}/ai",
            get(history_routes::ai_status).post(room_control_routes::add),
        )
        .route(
            "/v1/matches/{match}/ante/disposition",
            post(ante_routes::offer_disposition),
        )
        .route("/v1/ante/{commitment}/accept", post(ante_routes::accept))
        .route(
            "/v1/ante/{commitment}/accept/disposition",
            post(ante_routes::accept_disposition),
        )
        .route("/v1/ante/{commitment}/cancel", post(ante_routes::cancel))
        .route(
            "/v1/ante/{commitment}/cancel/disposition",
            post(ante_routes::cancel_disposition),
        )
        .route(
            "/v1/seasons/{season}/ballot/disposition",
            post(ballot_routes::disposition),
        )
        .route(
            "/v1/rooms/{room}/ai/disposition",
            post(room_control_routes::add_disposition),
        )
        .route(
            "/v1/ai-slots/{slot}/bootstrap",
            post(room_control_routes::bootstrap),
        )
        .route(
            "/v1/ai-slots/{slot}/bootstrap/disposition",
            post(room_control_routes::bootstrap_disposition),
        )
        .route(
            "/v1/ai-slots/{slot}/remove",
            post(room_control_routes::remove),
        )
        .route(
            "/v1/ai-slots/{slot}/remove/disposition",
            post(room_control_routes::remove_disposition),
        )
        .route(
            "/v1/rooms/{room}/unready",
            post(room_control_routes::unready),
        )
        .route(
            "/v1/rooms/{room}/unready/disposition",
            post(room_control_routes::unready_disposition),
        )
        .route("/v1/session", get(session))
        .route("/v1/quests", get(progression_routes::quests))
        .route("/v1/quests/reroll", post(quest_reroll_routes::execute))
        .route(
            "/v1/quests/reroll/disposition",
            post(quest_reroll_routes::disposition),
        )
        .route("/v1/ranks", get(progression_routes::ranks))
        .route("/v1/seasons", get(progression_routes::seasons))
        .route("/v1/account", post(register).get(account))
        .route("/v1/account/patreon/start", post(patreon_routes::start))
        .route(
            "/v1/account/patreon/complete",
            post(patreon_routes::complete),
        )
        .route("/v1/account/patreon/status", post(patreon_routes::status))
        .route(
            "/v1/economy/commands",
            post(command_routes::economy_execute),
        )
        .route(
            "/v1/economy/commands/disposition",
            post(command_routes::economy_disposition),
        )
        .route(
            "/v1/rooms/{room}/configuration",
            get(room_configuration_routes::read).post(room_configuration_routes::execute),
        )
        .route(
            "/v1/rooms/{room}/configuration/disposition",
            post(room_configuration_routes::disposition),
        )
        .route(
            "/v1/rooms/{room}/invitations",
            post(invitation_routes::issue),
        )
        .route(
            "/v1/rooms/{room}/invitations/disposition",
            post(invitation_routes::issue_disposition),
        )
        .route("/v1/rooms/{room}/join", post(invitation_routes::join))
        .route(
            "/v1/rooms/{room}/join/disposition",
            post(invitation_routes::join_disposition),
        )
        .route("/v1/invitations", get(invitation_routes::list))
        .route(
            "/v1/invitations/{invitation}/revoke",
            post(invitation_routes::revoke),
        )
        .route(
            "/v1/invitations/{invitation}/revoke/disposition",
            post(invitation_routes::revoke_disposition),
        )
        .route(
            "/v1/queue",
            get(matchmaking_routes::queue).post(matchmaking_routes::enqueue),
        )
        .route(
            "/v1/queue/disposition",
            post(matchmaking_routes::enqueue_disposition),
        )
        .route("/v1/queue/{entry}/cancel", post(matchmaking_routes::cancel))
        .route(
            "/v1/queue/{entry}/cancel/disposition",
            post(matchmaking_routes::cancel_disposition),
        )
        .route(
            "/v1/queue/intents/{intent}",
            get(matchmaking_routes::intent),
        )
        .route(
            "/v1/matches/{match}/chat",
            get(chat_notification_routes::chat),
        )
        .route(
            "/v1/matches/{match}/chat/messages",
            post(chat_notification_routes::send),
        )
        .route(
            "/v1/matches/{match}/chat/messages/disposition",
            post(chat_notification_routes::send_disposition),
        )
        .route(
            "/v1/notifications",
            get(chat_notification_routes::notifications),
        )
        .route(
            "/v1/notifications/read",
            post(chat_notification_routes::mark_read),
        )
        .route(
            "/v1/notifications/read/disposition",
            post(chat_notification_routes::read_disposition),
        )
        .route("/v1/table-profiles", get(matchmaking_routes::profiles))
        .route(
            "/v1/table-profiles/{profile}/configurations",
            get(matchmaking_routes::configurations),
        )
        .route(
            "/v1/between-game/replies",
            post(admission_receipt_routes::execute_between),
        )
        .route(
            "/v1/between-game/replies/disposition",
            post(admission_receipt_routes::between),
        )
        .route(
            "/v1/rooms/{room}/ready",
            post(commander_admission_routes::execute_ready),
        )
        .route(
            "/v1/rooms/{room}/ready/disposition",
            post(commander_admission_routes::ready),
        )
        .route(
            "/v1/rooms/{room}/force-start",
            post(commander_admission_routes::execute_force_start),
        )
        .route(
            "/v1/rooms/{room}/force-start/disposition",
            post(commander_admission_routes::force_start),
        )
        .route("/v1/rooms", get(room_routes::list))
        .route("/v1/rooms/{room}", get(room_routes::detail))
        .route("/v1/rooms/commands", post(command_routes::room_execute))
        .route(
            "/v1/rooms/commands/disposition",
            post(command_routes::room_disposition),
        )
        .route("/v1/social/commands", post(command_routes::social_execute))
        .route("/v1/social/read", post(command_routes::mark_read))
        .route(
            "/v1/social/read/disposition",
            post(command_routes::read_disposition),
        )
        .route("/v1/social/people", get(social_routes::people))
        .route("/v1/social/status", get(social_routes::status))
        .route("/v1/social/threads", get(social_routes::threads))
        .route(
            "/v1/social/conversations/{conversation}/messages",
            get(social_routes::messages),
        )
        .route(
            "/v1/social/commands/disposition",
            post(command_routes::social_disposition),
        )
        .route("/v1/account/deletion", post(deletion_routes::request))
        .route(
            "/v1/account/deletion/observe",
            post(deletion_routes::observe),
        )
        .route("/v1/decks/import", post(import_routes::execute))
        .route(
            "/v1/decks/import/disposition",
            post(import_routes::disposition),
        )
        .route("/v1/decks", get(deck_library).post(deck_routes::create))
        .route(
            "/v1/decks/{deck}",
            get(deck_routes::detail).delete(ui_surface_routes::delete_deck),
        )
        .route("/v1/decks/{deck}/revisions", post(deck_routes::save))
        .route("/v1/decks/{deck}/settings", post(deck_routes::settings))
        .route(
            "/v1/account/profile",
            get(profile_routes::art_own).post(command_routes::profile_art_execute),
        )
        .route(
            "/v1/account/profile/disposition",
            post(command_routes::profile_art_disposition),
        )
        .route(
            "/v1/players/{player}/profile",
            get(profile_routes::art_public),
        )
        .route("/v1/account/defaults", post(profile_routes::change))
        .route(
            "/v1/account/defaults/disposition",
            post(profile_routes::disposition),
        )
        .route(
            "/v1/decks/commands/disposition",
            post(deck_routes::disposition),
        )
        .route("/v1/formats", get(deck_routes::formats))
        .route(
            "/v1/predictions/printings/{printing}/source",
            get(prediction_routes::publication_source).post(prediction_routes::prepare_source),
        )
        .route("/v1/predictions/markets", get(prediction_routes::markets))
        .route(
            "/v1/predictions/markets/{market}",
            get(prediction_routes::market),
        )
        .route("/v1/predictions/orders", get(prediction_routes::orders))
        .route(
            "/v1/predictions/positions",
            get(prediction_routes::positions),
        )
        .route("/v1/sets", get(catalogue_routes::sets))
        .route("/v1/cards", get(catalogue_routes::cards))
        .route(
            "/v1/cards/printings",
            get(catalogue_routes::named_printings),
        )
        .route(
            "/v1/cards/{definition}/printings",
            get(catalogue_routes::printings),
        )
        .route("/v1/catalogue/resolve", post(catalogue_routes::resolve))
        .route("/v1/inventory", get(inventory))
        .route("/v1/packs/products", get(economy_routes::pack_products))
        .route(
            "/v1/packs/products/{recipe}/quote",
            get(economy_routes::pack_quote),
        )
        .route(
            "/v1/predictions/commands",
            post(command_routes::prediction_execute),
        )
        .route(
            "/v1/predictions/commands/disposition",
            post(command_routes::prediction_disposition),
        )
        .route("/v1/trades", get(economy_routes::trades))
        .route("/v1/trades/{trade}", get(economy_routes::trade))
        .route("/v1/market/listings", get(economy_routes::market))
        .route(
            "/v1/market/listings/{listing}",
            get(economy_routes::listing),
        )
        .route(
            "/v1/packs/openings/{opened}",
            get(economy_routes::pack_outcome),
        )
        .route("/v1/cosmetics", get(cosmetic_catalogue))
        .route("/v1/purchases", post(checkout).get(purchases))
        .route("/v1/purchases/{purchase}", get(purchase))
        // UI-surface completion endpoints
        .route(
            "/v1/catalogue/metadata",
            post(ui_surface_routes::card_metadata),
        )
        .route(
            "/v1/catalogue/cards",
            get(ui_surface_routes::search_cards_rich),
        )
        .route(
            "/v1/decks/{deck}/duplicate",
            post(ui_surface_routes::duplicate_deck),
        )
        .route(
            "/v1/cosmetics/catalogue",
            get(ui_surface_routes::cosmetics_rich),
        )
        .route(
            "/v1/account/profile/badge",
            post(ui_surface_routes::set_profile_badge),
        )
        .route(
            "/v1/account/patreon",
            get(ui_surface_routes::patreon_account_status),
        )
        .route(
            "/v1/billing/portal",
            post(ui_surface_routes::billing_portal),
        )
        .route(
            "/v1/league/leaderboard",
            get(ui_surface_routes::league_leaderboard),
        )
        .route(
            "/v1/players/{player}/summary",
            get(ui_surface_routes::player_public_summary),
        )
        .route("/v1/market/search", get(ui_surface_routes::market_search))
        .route(
            "/v1/trades/sessions",
            get(ui_surface_routes::trade_sessions_list)
                .post(ui_surface_routes::trade_session_create),
        )
        .route(
            "/v1/trades/sessions/{session}",
            get(ui_surface_routes::trade_session_detail),
        )
        .route(
            "/v1/trades/sessions/{session}/offer",
            patch(ui_surface_routes::trade_session_offer),
        )
        .route(
            "/v1/trades/sessions/{session}/ready",
            post(ui_surface_routes::trade_session_ready),
        )
        .route(
            "/v1/trades/sessions/{session}/confirm",
            post(ui_surface_routes::trade_session_confirm),
        )
        .route(
            "/v1/trades/sessions/{session}/cancel",
            post(ui_surface_routes::trade_session_cancel),
        )
        .route(
            "/v1/predictions/markets/search",
            get(ui_surface_routes::predictions_search),
        )
        .route(
            "/v1/predictions/markets/{market}/bet",
            post(ui_surface_routes::prediction_bet),
        )
        .route("/v1/rooms/{room}/game", get(ui_surface_routes::room_game))
        .route(
            "/v1/rooms/{room}/detail",
            get(ui_surface_routes::room_enriched),
        )
        .route(
            "/v1/rooms/{room}/options",
            patch(ui_surface_routes::update_room_options),
        )
        .route(
            "/v1/rooms/{room}/invitations/by-username",
            post(ui_surface_routes::invite_room_by_username),
        )
        .route(
            "/v1/rooms/{room}/draft",
            get(ui_surface_routes::limited_draft_read),
        )
        .route(
            "/v1/rooms/{room}/draft/start",
            post(ui_surface_routes::limited_draft_start),
        )
        .route(
            "/v1/rooms/{room}/draft/pick",
            post(ui_surface_routes::limited_draft_pick),
        )
        .route(
            "/v1/rooms/{room}/draft/seal",
            post(ui_surface_routes::limited_draft_seal),
        )
        .route(
            "/v1/social/conversations/{conversation}/attachments",
            post(ui_surface_routes::dm_send_attachment),
        )
        .route("/v1/webhooks/stripe", post(stripe_webhook))
        .route("/v1/webhooks/patreon", post(patreon_routes::webhook))
        .fallback(|| async { fail(StatusCode::NOT_FOUND, "route", "not_found") })
        .method_not_allowed_fallback(|| async {
            fail(StatusCode::METHOD_NOT_ALLOWED, "method", "not_allowed")
        })
        .layer(axum::middleware::from_fn({
            let limiter = RateLimiter::from_env();
            move |request: Request, next: axum::middleware::Next| {
                let limiter = limiter.clone();
                async move {
                    let method = request.method().clone();
                    let path = request.uri().path().to_owned();
                    let is_private = path != "/health";
                    let started = Instant::now();
                    if is_private && let Some(ip) = client_ip_key(&request) {
                        let sensitive_unauth = path == "/v1/account/deletion/observe";
                        if !limiter.allow(&ip, sensitive_unauth) {
                            tracing::warn!(
                                http.method = %method,
                                http.path = %path,
                                http.status = 429_u16,
                                "request rate limited"
                            );
                            return command_routes::private(fail(
                                StatusCode::TOO_MANY_REQUESTS,
                                "rate",
                                "limited",
                            ));
                        }
                    }
                    let response = next.run(request).await;
                    let status = response.status().as_u16();
                    let latency_ms = started.elapsed().as_millis() as u64;
                    if status >= 500 {
                        tracing::warn!(
                            http.method = %method,
                            http.path = %path,
                            http.status = status,
                            latency_ms,
                            "request completed with server error"
                        );
                    } else if path != "/health" {
                        tracing::debug!(
                            http.method = %method,
                            http.path = %path,
                            http.status = status,
                            latency_ms,
                            "request completed"
                        );
                    }
                    if is_private {
                        command_routes::private(response)
                    } else {
                        response
                    }
                }
            }
        }))
        .layer(cors)
        .with_state(state)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckoutInput {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    cosmetic_id: Uuid,
}
fn stripe_service(config: &ApiConfig) -> Result<StripeService, ActorError> {
    let stripe = config.stripe.clone().ok_or(ActorError::Unavailable)?;
    StripeService::new(
        BackendConfig::new(&config.base, &config.secret_key)
            .map_err(|_| ActorError::Configuration)?,
        stripe,
    )
    .map_err(|_| ActorError::Unavailable)
}
async fn checkout(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(e) => return actor_error(e),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), 4096).await {
        Ok(bytes) => bytes,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    let input: CheckoutInput = match parse_object(&bytes) {
        Ok(input) => input,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if input.request_id.is_nil() || input.cosmetic_id.is_nil() {
        return fail(StatusCode::BAD_REQUEST, "body", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|client| client.resolve(&token)) {
                Ok(actor) => actor,
                Err(e) => return actor_error(e),
            };
            if actor.account().is_none() {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let service = match stripe_service(config) {
                Ok(service) => service,
                Err(_) => return unavailable(),
            };
            match service.reserve(&actor, input.request_id, input.cosmetic_id) {
                Ok(ApiResponse::Success { data }) if !data.resource_id.is_nil() => {
                    (StatusCode::ACCEPTED, Json(ApiResponse::Success { data })).into_response()
                }
                Ok(ApiResponse::Fail { data })
                    if data.len() == 4
                        && data.get("request").map(String::as_str)
                            == Some("terminal_rejection")
                        && data.get("requestId") == Some(&input.request_id.to_string())
                        && data.get("cosmeticId") == Some(&input.cosmetic_id.to_string())
                        && matches!(
                            data.get("reason").map(String::as_str),
                            Some("cosmetic_unavailable" | "account_unavailable")
                        ) =>
                {
                    (
                        StatusCode::CONFLICT,
                        Json(ApiResponse::<crate::commands::CommandAck>::Fail { data }),
                    )
                        .into_response()
                }
                Ok(ApiResponse::Fail { data }) if data.len() == 1 => {
                    match data.iter().next().map(|(k, v)| (k.as_str(), v.as_str())) {
                        Some(("session", "unauthorized")) => {
                            fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
                        }
                        Some((field @ ("account" | "cosmetic"), "unavailable")) => {
                            fail(StatusCode::CONFLICT, field, "unavailable")
                        }
                        Some(("request", "conflict")) => {
                            fail(StatusCode::CONFLICT, "request", "conflict")
                        }
                        _ => unavailable(),
                    }
                }
                _ => unavailable(),
            }
        })
        .await
}
async fn purchase(
    State(state): State<ApiState>,
    raw: IdentifierPath,
    request: Request,
) -> Response {
    let id = match identifier(raw) {
        Ok(id) => id,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "purchase", "invalid"),
    };
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(token) => token,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(move |config| {
            match config
                .actor()
                .and_then(|client| client.purchase(&token, id))
            {
                Ok(ApiResponse::Success { data }) => success(data),
                Ok(ApiResponse::Fail { .. }) => {
                    fail(StatusCode::NOT_FOUND, "purchase", "unavailable")
                }
                Ok(_) => unavailable(),
                Err(e) => actor_error(e),
            }
        })
        .await
}
async fn stripe_webhook(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let mut headers = request.headers().get_all("stripe-signature").iter();
    let signature = headers
        .next()
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    if signature.is_none() || headers.next().is_some() {
        return fail(StatusCode::BAD_REQUEST, "signature", "invalid");
    }
    let body = match to_bytes(request.into_body(), MAX_WEBHOOK_BODY).await {
        Ok(bytes) => bytes,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    state
        .blocking(move |config| {
            let Some(settings) = &config.stripe else {
                return unavailable();
            };
            let verifier = match StripeWebhookVerifier::new(
                settings.webhook_secret.clone(),
                crate::stripe::client::API_VERSION.into(),
                settings.live,
            ) {
                Ok(v) => v,
                Err(_) => return unavailable(),
            };
            let notice = match verifier.verify(
                &body,
                signature.as_deref().unwrap_or(""),
                chrono::Utc::now().timestamp(),
            ) {
                Ok(n) => n,
                Err(_) => return fail(StatusCode::BAD_REQUEST, "signature", "invalid"),
            };
            match stripe_service(config).and_then(|s| {
                s.ingest(&notice, &body)
                    .map_err(|_| ActorError::Unavailable)
            }) {
                Ok(()) => success(json!({"received":true})),
                Err(_) => unavailable(),
            }
        })
        .await
}
fn success(data: impl Serialize) -> Response {
    Json(ApiResponse::Success { data }).into_response()
}
fn fail(status: StatusCode, field: &str, reason: &str) -> Response {
    (
        status,
        Json(ApiResponse::<Value>::Fail {
            data: BTreeMap::from([(field.into(), reason.into())]),
        }),
    )
        .into_response()
}
fn unavailable() -> Response {
    tracing::warn!(code = "unavailable", "application_api service unavailable");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ApiResponse::<Value>::Error {
            message: "Application service is temporarily unavailable".into(),
            code: "unavailable".into(),
        }),
    )
        .into_response()
}
fn actor_error(error: ActorError) -> Response {
    match error {
        ActorError::Unauthorized => fail(StatusCode::UNAUTHORIZED, "session", "unauthorized"),
        _ => unavailable(),
    }
}
fn bearer(headers: &HeaderMap) -> Result<String, ActorError> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next().and_then(|v| v.to_str().ok());
    if values.next().is_some() {
        return Err(ActorError::Unauthorized);
    }
    value
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| {
            !v.is_empty() && v.len() <= 16 * 1024 && !v.bytes().any(|b| b.is_ascii_whitespace())
        })
        .map(str::to_owned)
        .ok_or(ActorError::Unauthorized)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionOutput {
    state: ActorState,
    method: SignInMethod,
    subject_id: Uuid,
    session_id: Uuid,
    account_id: Option<Uuid>,
}
impl From<RequestActor> for SessionOutput {
    fn from(actor: RequestActor) -> Self {
        Self {
            state: actor.state(),
            method: actor.method(),
            subject_id: actor.subject(),
            session_id: actor.session(),
            account_id: actor.account(),
        }
    }
}
async fn session(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    state
        .blocking(
            move |config| match config.actor().and_then(|client| client.resolve(&token)) {
                Ok(actor) => success(SessionOutput::from(actor)),
                Err(e) => actor_error(e),
            },
        )
        .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    username: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RegistrationOutput {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    account_id: Uuid,
}
async fn register(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let body = match to_bytes(request.into_body(), 4096).await {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    let input: Registration = match parse_object(&body) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    if !input
        .username
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || !(3..=24).contains(&input.username.chars().count())
        || input.username.trim() != input.username
    {
        return fail(StatusCode::BAD_REQUEST, "username", "invalid");
    }
    state
        .blocking(move |config| {
            let actor = match config.actor().and_then(|client| client.resolve(&token)) {
                Ok(v) => v,
                Err(e) => return actor_error(e),
            };
            if actor.state() == ActorState::DeletionPending {
                return fail(StatusCode::CONFLICT, "account", "unavailable");
            }
            let client = BackendConfig::new(&config.base, &config.secret_key).and_then(|config| {
                BackendClient::new(config, "spellwake-application-api", Duration::from_secs(60))
            });
            let result = client.and_then(|client| {
                client.rpc::<crate::provider_json::ObjectResponse<RegistrationOutput>>("register_account_command", &json!({
            "p_subject":actor.subject(),"p_session":actor.session(),"p_username":input.username,
            "p_back":config.defaults[0],"p_sleeve":config.defaults[1],"p_dice":config.defaults[2],
        })).map(|response| response.0)
            });
            match result {
                Ok(ApiResponse::Success { data })
                    if !data.account_id.is_nil()
                        && actor.account().is_none_or(|id| id == data.account_id) =>
                {
                    success(data)
                }
                Ok(ApiResponse::Fail { data }) if data.len() == 1 => {
                    match data.iter().next().map(|(k, v)| (k.as_str(), v.as_str())) {
                        Some(("session", "unauthorized")) => {
                            fail(StatusCode::UNAUTHORIZED, "session", "unauthorized")
                        }
                        Some((field @ ("username" | "account"), "unavailable")) => {
                            fail(StatusCode::CONFLICT, field, "unavailable")
                        }
                        _ => unavailable(),
                    }
                }
                _ => unavailable(),
            }
        })
        .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    after: Option<Uuid>,
    limit: Option<usize>,
}
async fn inventory(State(state): State<ApiState>, request: Request) -> Response {
    let token = match bearer(request.headers()) {
        Ok(v) => v,
        Err(e) => return actor_error(e),
    };
    let query = match Query::<PageQuery>::try_from_uri(request.uri()) {
        Ok(Query(v)) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "query", "invalid"),
    };
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) || query.after.is_some_and(|v| v.is_nil()) {
        return fail(StatusCode::BAD_REQUEST, "query", "invalid");
    }
    state
        .blocking(move |config| {
            match config
                .actor()
                .and_then(|client| client.inventory(&token, query.after, limit))
            {
                Ok(ApiResponse::Success { data }) => success(data),
                Ok(ApiResponse::Fail { .. }) => {
                    fail(StatusCode::CONFLICT, "account", "unavailable")
                }
                _result @ Ok(_) => unavailable(),
                Err(e) => actor_error(e),
            }
        })
        .await
}

async fn account(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let token = match bearer(request.headers()) {
        Ok(value) => value,
        Err(error) => return actor_error(error),
    };
    state
        .blocking(
            move |config| match config.actor().and_then(|client| client.account(&token)) {
                Ok(ApiResponse::Success { data }) => success(data),
                Ok(ApiResponse::Fail { .. }) => {
                    fail(StatusCode::CONFLICT, "account", "unavailable")
                }
                Ok(_) => unavailable(),
                Err(error) => actor_error(error),
            },
        )
        .await
}
async fn deck_library(State(state): State<ApiState>, request: Request) -> Response {
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
            match config
                .actor()
                .and_then(|client| client.deck_library(&token, query.after, limit))
            {
                Ok(ApiResponse::Success { data }) => success(data),
                Ok(ApiResponse::Fail { .. }) => {
                    fail(StatusCode::CONFLICT, "account", "unavailable")
                }
                Ok(_) => unavailable(),
                Err(error) => actor_error(error),
            }
        })
        .await
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;

async fn cosmetic_catalogue(State(state): State<ApiState>, request: Request) -> Response {
    store_page(state, request, false).await
}
async fn purchases(State(state): State<ApiState>, request: Request) -> Response {
    store_page(state, request, true).await
}
async fn store_page(state: ApiState, request: Request, is_purchase: bool) -> Response {
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
            let actor = match config.actor() {
                Ok(value) => value,
                Err(error) => return actor_error(error),
            };
            if is_purchase {
                match actor.purchases(&token, query.after, limit) {
                    Ok(ApiResponse::Success { data }) => success(data),
                    Err(error) => actor_error(error),
                    _ => unavailable(),
                }
            } else {
                match actor.cosmetic_catalogue(&token, query.after, limit) {
                    Ok(ApiResponse::Success { data }) => success(data),
                    Err(error) => actor_error(error),
                    _ => unavailable(),
                }
            }
        })
        .await
}

/// A singleton media type avoids silently selecting one of conflicting declarations.
/// The caller retains its existing media-type/parameter acceptance policy.
fn single_content_type(headers: &HeaderMap) -> Option<&HeaderValue> {
    let mut values = headers.get_all(header::CONTENT_TYPE).iter();
    let value = values.next()?;
    if values.next().is_some() {
        None
    } else {
        Some(value)
    }
}

/// Accept a singleton `application/json` media type with optional RFC 9110 parameters
/// (such as `; charset=utf-8`), while rejecting duplicate or ambiguous headers.
pub(super) fn is_json_content_type(headers: &HeaderMap) -> bool {
    let Some(raw) = single_content_type(headers).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let mut parts = raw.split(';');
    let Some(media_type) = parts.next() else {
        return false;
    };
    if !media_type.trim().eq_ignore_ascii_case("application/json") {
        return false;
    }
    // Reject comma-separated multi-type headers like `application/json, text/plain`
    // and malformed parameters lacking `key=value`.
    for param in parts {
        let trimmed = param.trim();
        if trimmed.is_empty() || trimmed.contains(',') || !trimmed.contains('=') {
            return false;
        }
    }
    true
}

/// Qualification-only observer of the real router/body resources, not a proof factory.
#[cfg(test)]
pub(crate) struct NativeStreamProbe {
    router: Router,
    state: ApiState,
}
#[cfg(test)]
impl NativeStreamProbe {
    /// Pure serializer control only: an unguarded candidate confers no caller,
    /// native-source or delivery proof and is never mounted as an endpoint.
    pub(crate) fn bounded_candidate(data: impl serde::Serialize) -> Response {
        native_delivery_routes::bounded_success(data)
    }
    pub(crate) fn new(config: ApiConfig) -> Self {
        let state = ApiState::new(config);
        Self {
            router: router_with_state(state.clone()),
            state,
        }
    }
    pub(crate) fn available(&self) -> (usize, usize) {
        (
            self.state.stream_permits.available_permits(),
            self.state.permits.available_permits(),
        )
    }
    pub(crate) async fn request(&self, request: Request) -> Response {
        use tower::ServiceExt;
        self.router.clone().oneshot(request).await.unwrap()
    }
}
