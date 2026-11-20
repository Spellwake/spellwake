//! Private possessed-controller AI ante transport. SQL136 owns economics and
//! SQL241 owns canonical authority; this adapter never supplies sponsor or assets.
use crate::{
    ApiResponse,
    ai_ante_port::{AdmittedAiAnteContext, AiAnteStake, PreparedAiAnte},
    ante::OfferAck,
    backend::{BackendClient, BackendConfig, BackendError},
    commands::CommandDisposition,
    controller_possession::{
        ControllerPossessionError, ControllerSecretStore, PossessedController,
    },
    player_actor::PlayerActor,
    provider_json::ObjectResponse,
    source_protocol::{SourceResponseError, one_failure, parse_utc_microsecond},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{error::Error, fmt, time::Duration};
use uuid::Uuid;
const LIMIT: u64 = 32_768;
const ORIGINAL_FAILURES: &[(&str, &str)] = &[
    ("actor", "unavailable"),
    ("request", "invalid"),
    ("request", "conflict"),
];
const CONTEXT_FAILURES: &[(&str, &str)] = &[("actor", "unavailable"), ("match", "unavailable")];

/// Safe private service failure. Underlying transport/codec causes remain available
/// through Error::source; they must not be copied into public messages.
pub struct ControllerAnteError(Failure);
enum Failure {
    Invalid,
    Backend(BackendError),
    Source(SourceResponseError),
    Possession(ControllerPossessionError),
}
impl fmt::Display for ControllerAnteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("controller ante source is unavailable or inconsistent")
    }
}
impl fmt::Debug for ControllerAnteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for ControllerAnteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.0 {
            Failure::Invalid => None,
            Failure::Backend(e) => Some(e),
            Failure::Source(e) => Some(e),
            Failure::Possession(e) => Some(e),
        }
    }
}
fn invalid() -> ControllerAnteError {
    ControllerAnteError(Failure::Invalid)
}
fn unavailable<T>() -> ApiResponse<T> {
    ApiResponse::Fail {
        data: [("actor".to_owned(), "unavailable".to_owned())].into(),
    }
}
fn current(
    store: &ControllerSecretStore,
    proof: &PossessedController,
) -> Result<bool, ControllerAnteError> {
    store
        .current(proof)
        .map_err(|e| ControllerAnteError(Failure::Possession(e)))
}
// Enclose IO, strict decoding and result composition, including every error path.
// The check is an observation, not a durable authorization lease.
fn guarded<T>(
    store: &ControllerSecretStore,
    proof: &PossessedController,
    operation: impl FnOnce() -> Result<ApiResponse<T>, ControllerAnteError>,
) -> Result<ApiResponse<T>, ControllerAnteError> {
    if !current(store, proof)? {
        return Ok(unavailable());
    }
    let result = operation();
    if !current(store, proof)? {
        return Ok(unavailable());
    }
    result
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Context {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    sponsor_id: Uuid,
    match_started_at: String,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    commitment_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    exposure_id: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextRecord {
    #[serde(deserialize_with = "crate::provider_json::object")]
    actor: PlayerActor,
    #[serde(deserialize_with = "crate::provider_json::object")]
    context: Context,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginalRecord {
    #[serde(deserialize_with = "crate::provider_json::object")]
    actor: PlayerActor,
    #[serde(deserialize_with = "crate::provider_json::object")]
    original: AiAnteStake,
    #[serde(deserialize_with = "crate::provider_json::object")]
    disposition: CommandDisposition<OfferAck>,
}

/// Server-only private AI ante repository. It requires the real process-owned
/// store and opaque possession proof on every call. No public HTTP route exists.
pub struct ControllerAnteRepository {
    backend: BackendClient,
}
impl ControllerAnteRepository {
    pub(crate) fn new(backend: BackendClient) -> Self {
        Self { backend }
    }
    /// Read trusted backend configuration without embedding credentials in clients.
    /// # Errors
    /// Rejects missing/invalid origin, backend key or transport configuration.
    pub fn from_env() -> Result<Self, ControllerAnteError> {
        let config =
            BackendConfig::from_env().map_err(|e| ControllerAnteError(Failure::Backend(e)))?;
        BackendClient::new(
            config,
            "spellwake-controller-ante/1",
            Duration::from_secs(60),
        )
        .map(Self::new)
        .map_err(|e| ControllerAnteError(Failure::Backend(e)))
    }
    fn payload(proof: &PossessedController) -> Value {
        json!({"p_grant":proof.grant(),"p_epoch":proof.epoch(),"p_secret_ref":proof.reference()})
    }
    /// Observe canonical admitted context before native opening, without a View
    /// or host lease. Sponsor and match-start day are selected by SQL.
    /// # Errors
    /// Rejects nil scope, malformed/foreign records, noncanonical timestamps,
    /// impossible exposure presence and undeclared replies. Local revocation
    /// overrides every backend outcome, including decode and transport failures.
    pub fn context(
        &self,
        store: &ControllerSecretStore,
        proof: &PossessedController,
        matched: Uuid,
    ) -> Result<ApiResponse<AdmittedAiAnteContext>, ControllerAnteError> {
        guarded(store, proof, || {
            if matched.is_nil() {
                return Err(invalid());
            }
            let mut payload = Self::payload(proof);
            payload["p_match"] = json!(matched);
            let response = self
                .backend
                .rpc_bounded::<ObjectResponse<ContextRecord>>(
                    "read_controller_ai_ante_context",
                    &payload,
                    LIMIT,
                )
                .map_err(|e| ControllerAnteError(Failure::Backend(e)))?
                .0;
            match response {
                ApiResponse::Success { data } => {
                    let c = data.context;
                    if !data.actor.matches_controller(proof)
                        || data.actor.seat_id != c.seat_id
                        || c.match_id != matched
                        || c.exposure_id.is_some() && c.commitment_id.is_none()
                    {
                        return Err(invalid());
                    }
                    let match_started_at =
                        parse_utc_microsecond(&c.match_started_at).ok_or_else(invalid)?;
                    Ok(ApiResponse::Success {
                        data: AdmittedAiAnteContext {
                            match_id: c.match_id,
                            seat_id: c.seat_id,
                            sponsor_id: c.sponsor_id,
                            match_started_at,
                            commitment_id: c.commitment_id,
                            exposure_id: c.exposure_id,
                        },
                    })
                }
                ApiResponse::Fail { data } if one_failure(&data, CONTEXT_FAILURES) => {
                    Ok(ApiResponse::Fail { data })
                }
                _ => Err(invalid()),
            }
        })
    }
    fn original_rpc(
        &self,
        store: &ControllerSecretStore,
        proof: &PossessedController,
        original: &AiAnteStake,
        submit: bool,
    ) -> Result<ApiResponse<CommandDisposition<OfferAck>>, ControllerAnteError> {
        guarded(store, proof, || {
            if !original.valid() {
                return Err(invalid());
            }
            let mut payload = Self::payload(proof);
            payload["p_source"] = json!(original);
            let response = self
                .backend
                .rpc_bounded::<ObjectResponse<OriginalRecord>>(
                    if submit {
                        "submit_controller_ai_ante"
                    } else {
                        "read_controller_ai_ante_original"
                    },
                    &payload,
                    LIMIT,
                )
                .map_err(|e| ControllerAnteError(Failure::Backend(e)))?
                .0;
            match response {
                ApiResponse::Success { data } => {
                    if !data.actor.matches_controller(proof)
                        || data.actor.seat_id != original.seat_id
                        || data.original != *original
                        || !original.valid_disposition(&data.disposition)
                        || submit && matches!(data.disposition, CommandDisposition::Unknown)
                    {
                        return Err(invalid());
                    }
                    Ok(ApiResponse::Success {
                        data: data.disposition,
                    })
                }
                ApiResponse::Fail { data } if one_failure(&data, ORIGINAL_FAILURES) => {
                    Ok(ApiResponse::Fail { data })
                }
                _ => Err(invalid()),
            }
        })
    }
    /// Recover an exact original before mutable context or native work. Current
    /// replacement authority may recover the same seat's old original.
    /// # Errors
    /// Rejects invalid original, over-budget/malformed/uncorrelated replies and
    /// undeclared failures. Unknown and pending remain explicit, never fabricated.
    pub fn original(
        &self,
        store: &ControllerSecretStore,
        proof: &PossessedController,
        original: &AiAnteStake,
    ) -> Result<ApiResponse<CommandDisposition<OfferAck>>, ControllerAnteError> {
        self.original_rpc(store, proof, original, false)
    }
    /// Recover first, then prepare canonical context and submit the unchanged
    /// source once. An uncertain acknowledgment triggers original lookup only.
    /// Local post-IO revocation can suppress delivery after remote commit; it
    /// does not promise remote rollback or authorize a new request ID.
    /// # Errors
    /// Preserves transport/codec causes when original recovery is still unknown
    /// or fails. Rejects identity drift and fresh Ack/context mismatch.
    pub fn execute(
        &self,
        store: &ControllerSecretStore,
        proof: &PossessedController,
        original: &AiAnteStake,
    ) -> Result<ApiResponse<CommandDisposition<OfferAck>>, ControllerAnteError> {
        guarded(store, proof, || {
            match self.original(store, proof, original)? {
                ApiResponse::Success {
                    data: CommandDisposition::Unknown,
                } => {}
                known => return Ok(known),
            }
            let context = match self.context(store, proof, original.match_id)? {
                ApiResponse::Success { data } => data,
                ApiResponse::Fail { data } => return Ok(ApiResponse::Fail { data }),
                ApiResponse::Error { .. } => return Err(invalid()),
            };
            let prepared = PreparedAiAnte::new(original.clone(), context)
                .map_err(|e| ControllerAnteError(Failure::Source(e)))?;
            let result = self
                .original_rpc(store, proof, original, true)
                .and_then(|response| {
                    if let ApiResponse::Success {
                        data: CommandDisposition::Accepted { ack },
                    } = &response
                        && !prepared.valid_ack(ack)
                    {
                        return Err(invalid());
                    }
                    Ok(response)
                });
            match result {
                Ok(response) => Ok(response),
                Err(error) => match self.original(store, proof, original) {
                    Ok(ApiResponse::Success {
                        data: CommandDisposition::Unknown,
                    })
                    | Err(_) => Err(error),
                    Ok(ApiResponse::Success {
                        data: CommandDisposition::Accepted { ack },
                    }) if !prepared.valid_ack(&ack) => Err(error),
                    Ok(recovered) => Ok(recovered),
                },
            }
        })
    }
}
#[cfg(test)]
mod connected;
#[cfg(test)]
mod tests;
