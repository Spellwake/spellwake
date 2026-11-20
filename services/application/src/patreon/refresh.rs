//! Trusted current-entitlement coordinator. No financial grant or notification-completion authority.
use super::{
    PatreonError,
    client::{PatreonClient, PatreonConfig},
    member_context::{Failure, MemberTask, canonical_id, optional_id, valid_context},
};
use crate::backend::{BackendClient, BackendConfig, BackendError};
use crate::provider_json::Object;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

/// Safe failure keeps the underlying source without displaying provider data or configuration.
pub struct PatreonRefreshError(Failure);
impl fmt::Display for PatreonRefreshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("patreon membership refresh is unresolved")
    }
}
impl fmt::Debug for PatreonRefreshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for PatreonRefreshError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.0.source()
    }
}
impl From<BackendError> for PatreonRefreshError {
    fn from(e: BackendError) -> Self {
        Self(Failure::Backend(e))
    }
}
impl From<PatreonError> for PatreonRefreshError {
    fn from(e: PatreonError) -> Self {
        Self(Failure::Provider(e))
    }
}
fn invalid() -> PatreonRefreshError {
    PatreonRefreshError(Failure::Contract)
}
/// Durable logical refresh identity. Only application UUIDs may cross the workflow boundary.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(transparent)]
pub struct PatreonRefreshTask(MemberTask);
impl PatreonRefreshTask {
    /// Construct a task before dispatch. Reuse these same IDs for every delivery retry.
    /// # Errors
    /// Rejects nil association or request identifiers without external IO.
    pub fn new(association_id: Uuid, request_id: Uuid) -> Result<Self, PatreonRefreshError> {
        MemberTask::new(association_id, request_id)
            .map(Self)
            .ok_or_else(invalid)
    }
    /// Application association selected by discovery.
    pub fn association_id(&self) -> Uuid {
        self.0.association_id()
    }
    /// Stable application request recorded before delivery.
    pub fn request_id(&self) -> Uuid {
        self.0.request_id()
    }
}
impl<'de> Deserialize<'de> for PatreonRefreshTask {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        MemberTask::deserialize(input).map(Self)
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireTicket {
    version: u8,
    #[serde(deserialize_with = "canonical_id")]
    account_id: Uuid,
    #[serde(deserialize_with = "canonical_id")]
    association_id: Uuid,
    user_id: String,
    campaign_id: String,
    member_id: String,
    #[serde(deserialize_with = "canonical_id")]
    expected_source_id: Uuid,
    #[serde(deserialize_with = "canonical_id")]
    anchor_event_id: Uuid,
    generation: String,
}
#[derive(Serialize)]
#[serde(transparent)]
struct Ticket(WireTicket);
impl<'de> Deserialize<'de> for Ticket {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let value = Object::<WireTicket>::deserialize(input)?.0;
        if !valid_context(
            value.version,
            &value.user_id,
            &value.campaign_id,
            &value.member_id,
            &value.generation,
        ) {
            return Err(de::Error::custom("invalid current refresh context"));
        }
        Ok(Self(value))
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Proof {
    version: u8,
    source: &'static str,
    campaign_id: String,
    user_id: String,
    member_id: String,
    member_hash: String,
    entitled_tier_ids: Vec<String>,
}
/// Opaque in-memory attempt. Retain this value for exact commit replay after an unknown acknowledgment.
/// It is intentionally neither Debug nor serializable; it contains private selected provider identity.
pub struct PatreonRefreshAttempt {
    request: Uuid,
    ticket: Ticket,
    proof: Proof,
    backend_origin: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
enum Phase {
    Applied,
    Superseded,
    Unavailable,
}
impl TryFrom<String> for Phase {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "applied" => Ok(Self::Applied),
            "superseded" => Ok(Self::Superseded),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err("invalid string enum value"),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireResult {
    #[serde(deserialize_with = "canonical_id")]
    association_id: Uuid,
    #[serde(deserialize_with = "optional_id")]
    source_event_id: Option<Uuid>,
    outcome: Phase,
}
/// Historical source acknowledgment only. Superseded requires a fresh ticket and provider read.
pub enum PatreonRefreshOutcome {
    Applied {
        association_id: Uuid,
        source_event_id: Uuid,
    },
    Superseded {
        association_id: Uuid,
    },
    Unavailable {
        association_id: Uuid,
    },
}
impl<'de> Deserialize<'de> for PatreonRefreshOutcome {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let result = Object::<WireResult>::deserialize(input)?.0;
        let association = result.association_id;
        checked_result(result, association).map_err(de::Error::custom)
    }
}
fn checked_result(
    result: WireResult,
    association: Uuid,
) -> Result<PatreonRefreshOutcome, PatreonRefreshError> {
    if result.association_id != association {
        return Err(invalid());
    }
    match (result.outcome, result.source_event_id) {
        (Phase::Applied, Some(source_event_id)) => Ok(PatreonRefreshOutcome::Applied {
            association_id: association,
            source_event_id,
        }),
        (Phase::Superseded, None) => Ok(PatreonRefreshOutcome::Superseded {
            association_id: association,
        }),
        (Phase::Unavailable, None) => Ok(PatreonRefreshOutcome::Unavailable {
            association_id: association,
        }),
        _ => Err(invalid()),
    }
}
impl Serialize for PatreonRefreshOutcome {
    fn serialize<S: Serializer>(&self, output: S) -> Result<S::Ok, S::Error> {
        let wire = match *self {
            Self::Applied {
                association_id,
                source_event_id,
            } => WireResult {
                association_id,
                source_event_id: Some(source_event_id),
                outcome: Phase::Applied,
            },
            Self::Superseded { association_id } => WireResult {
                association_id,
                source_event_id: None,
                outcome: Phase::Superseded,
            },
            Self::Unavailable { association_id } => WireResult {
                association_id,
                source_event_id: None,
                outcome: Phase::Unavailable,
            },
        };
        wire.serialize(output)
    }
}
/// Blocking server-side source facade; never embed its elevated credentials in distributed clients.
pub struct PatreonRefreshService {
    backend: BackendClient,
    config: PatreonConfig,
    origin: String,
    stopped: Arc<AtomicBool>,
}
impl PatreonRefreshService {
    /// Attach the activity owner's cooperative stop signal before dispatch.
    /// An in-flight bounded HTTP request may finish; later network groups are fenced.
    pub fn with_stop(mut self, stopped: Arc<AtomicBool>) -> Self {
        self.stopped = stopped;
        self
    }
    /// Read explicit backend/campaign/creator environment configuration without dispatching a request.
    /// # Errors
    /// Rejects missing or malformed configuration. Construct/call from a blocking service context.
    pub fn from_env() -> Result<Self, PatreonRefreshError> {
        Self::new(BackendConfig::from_env()?, PatreonConfig::from_env()?)
    }
    pub(crate) fn new(
        backend: BackendConfig,
        config: PatreonConfig,
    ) -> Result<Self, PatreonRefreshError> {
        let backend = BackendClient::new(
            backend,
            "spellwake-patreon-current-refresh",
            Duration::from_secs(60),
        )?;
        let origin = backend.url("")?.to_string();
        Ok(Self {
            backend,
            config,
            origin,
            stopped: Arc::new(AtomicBool::new(false)),
        })
    }
    fn ensure_running(&self) -> Result<(), PatreonRefreshError> {
        if self.stopped.load(Ordering::Acquire) {
            Err(invalid())
        } else {
            Ok(())
        }
    }
    /// Discover at most one candidate and allocate its logical request outside workflow code.
    /// This method never performs provider work; record its result before delivery.
    /// # Errors
    /// Rejects malformed, oversized or nonadvancing pages and nil cursors. No empty-page fallback.
    pub fn discover(
        &self,
        after: Option<Uuid>,
    ) -> Result<Option<PatreonRefreshTask>, PatreonRefreshError> {
        self.ensure_running()?;
        let Some(association) =
            super::member_context::read_candidate(&self.backend, &self.config.campaign_id, after)
                .map_err(PatreonRefreshError)?
        else {
            return Ok(None);
        };
        self.ensure_running()?;
        Ok(Some(PatreonRefreshTask::new(association, Uuid::new_v4())?))
    }
    /// Read historical acceptance for this exact logical task without provider IO.
    /// # Errors
    /// Malformed/non-applied or foreign acknowledgments remain unresolved, never admission.
    pub fn recover(
        &self,
        task: &PatreonRefreshTask,
    ) -> Result<Option<PatreonRefreshOutcome>, PatreonRefreshError> {
        self.ensure_running()?;
        let result: Option<Object<WireResult>> = self.backend.rpc(
            "read_patreon_refresh_result",
            &serde_json::json!({"p_association":task.association_id(),"p_request":task.request_id()}),
        )?;
        let Some(result) = result else {
            return Ok(None);
        };
        match checked_result(result.0, task.association_id())? {
            applied @ PatreonRefreshOutcome::Applied { .. } => Ok(Some(applied)),
            _ => Err(invalid()),
        }
    }
    /// Deliver a recorded logical task, recovering an earlier acceptance before any new GET.
    /// A recovery after failed commit confirms the logical result, not the failed attempt's proof.
    /// # Errors
    /// Source/acknowledgment failures remain unresolved. A failed post-commit recovery preserves
    /// the original safe commit error and never fabricates an applied result.
    pub fn deliver(
        &self,
        task: &PatreonRefreshTask,
    ) -> Result<PatreonRefreshOutcome, PatreonRefreshError> {
        if let Some(result) = self.recover(task)? {
            return Ok(result);
        }
        let Some(attempt) = self.observe_request(task.association_id(), task.request_id())? else {
            return Ok(PatreonRefreshOutcome::Unavailable {
                association_id: task.association_id(),
            });
        };
        match self.commit(&attempt) {
            Ok(result) => Ok(result),
            Err(original) => {
                self.ensure_running()?;
                match self.recover(task) {
                    Ok(Some(result)) => Ok(result),
                    Ok(None) => Err(original),
                    Err(_) => {
                        eprintln!("patreon_refresh event=commit_recovery_unresolved");
                        Err(original)
                    }
                }
            }
        }
    }
    /// Prepare under canonical authority, then make one exact creator-token read after the SQL call ends.
    /// # Errors
    /// Invalid/unavailable source or malformed private projection remains unresolved, never empty entitlement.
    /// A null canonical ticket returns None without contacting Patreon or submitting a commit.
    pub fn observe(
        &self,
        association: Uuid,
    ) -> Result<Option<PatreonRefreshAttempt>, PatreonRefreshError> {
        self.observe_request(association, Uuid::new_v4())
    }
    fn observe_request(
        &self,
        association: Uuid,
        request: Uuid,
    ) -> Result<Option<PatreonRefreshAttempt>, PatreonRefreshError> {
        self.ensure_running()?;
        if association.is_nil() || request.is_nil() {
            return Err(invalid());
        }
        let Some(ticket) = self.backend.rpc::<Option<Ticket>>(
            "read_patreon_refresh",
            &serde_json::json!({"p_association":association}),
        )?
        else {
            return Ok(None);
        };
        if ticket.0.association_id != association || ticket.0.campaign_id != self.config.campaign_id
        {
            return Err(invalid());
        }
        self.ensure_running()?;
        let member = PatreonClient::new(self.config.clone())?
            .member(&ticket.0.member_id, &ticket.0.user_id)?;
        if member.campaign_id != ticket.0.campaign_id
            || member.member_id != ticket.0.member_id
            || member.user_id != ticket.0.user_id
        {
            return Err(invalid());
        }
        let proof = Proof {
            version: 1,
            source: "patreon-v2-current-member-v1",
            campaign_id: member.campaign_id,
            user_id: member.user_id,
            member_id: member.member_id,
            member_hash: member.body_sha256,
            entitled_tier_ids: member.entitled_tier_ids,
        };
        Ok(Some(PatreonRefreshAttempt {
            request,
            ticket,
            proof,
            backend_origin: self.origin.clone(),
        }))
    }
    /// Commit or explicitly recover this same exact observation; this method performs no provider read.
    /// # Errors
    /// Lost/malformed acknowledgment stays unresolved. Scope/correlation failures cannot confirm success.
    pub fn commit(
        &self,
        attempt: &PatreonRefreshAttempt,
    ) -> Result<PatreonRefreshOutcome, PatreonRefreshError> {
        self.ensure_running()?;
        if attempt.backend_origin != self.origin
            || attempt.ticket.0.campaign_id != self.config.campaign_id
        {
            return Err(invalid());
        }
        let result:Object<WireResult>=self.backend.rpc("commit_patreon_refresh",&serde_json::json!({"p_request":attempt.request,"p_ticket":attempt.ticket,"p_proof":attempt.proof}))?;
        checked_result(result.0, attempt.ticket.0.association_id)
    }
}
#[cfg(test)]
#[path = "refresh_tests.rs"]
mod tests;

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<Phase>(&["applied", "superseded", "unavailable"]);
    }
}
