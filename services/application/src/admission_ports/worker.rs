//! Service-role adapters for a retained matcher selection and a due room countdown.
use super::{MatchmakingClaim, RoomStartAck, decode_start};
use crate::{
    ApiResponse,
    amount::Amount,
    backend::{BackendClient, BackendConfig, BackendError},
    commands::CommandAck,
    source_protocol::SourceResponseError,
};
use serde_json::{Value, json};
use std::{error::Error, time::Duration};
use uuid::Uuid;

/// A private admission client configured for one backend. Keep the request ID
/// and original selection or room revision across retries.
pub struct AdmissionWorker(BackendClient);

impl AdmissionWorker {
    /// Construct from the service's Supabase origin and private key.
    /// # Errors
    /// Returns the transport failure if configuration or initialization fails.
    pub fn from_env() -> Result<Self, Box<dyn Error>> {
        Ok(Self(BackendClient::new(
            BackendConfig::from_env()?,
            "spellwake-admission-worker/1",
            Duration::from_secs(60),
        )?))
    }

    /// Claim the exact selection authorized by the players' stored queue intents.
    /// SQL rechecks the copied consent and owns the canonical original.
    /// # Errors
    /// Returns an error for invalid identifiers, transport failure, or an invalid receipt.
    pub fn claim(
        &self,
        original: &MatchmakingClaim,
    ) -> Result<ApiResponse<CommandAck>, Box<dyn Error>> {
        if original.request_id.is_nil() {
            return Err(SourceResponseError::context().into());
        }
        let selection: Value = serde_json::from_str(original.selection.original_json())?;
        let body = self.call(
            "claim_matchmaking",
            &json!({"p_selection":selection,"p_request":original.request_id}),
        )?;
        Ok(original.decode_response(&body)?)
    }

    /// Reserve a game from the retained Ready or force-start that advanced this countdown.
    /// SQL resolves the release and sealed deck inputs and checks the deadline.
    /// # Errors
    /// Returns an error on malformed coordinates, a missing qualification,
    /// a conflicting request, transport failure, or an invalid receipt.
    pub fn start(
        &self,
        room: Uuid,
        basis: &Amount,
        request: Uuid,
    ) -> Result<ApiResponse<RoomStartAck>, Box<dyn Error>> {
        if room.is_nil() || request.is_nil() {
            return Err(SourceResponseError::context().into());
        }
        let body = self.call(
            "start_validated_countdown",
            &json!({"p_room":room,"p_expected":basis,"p_request":request}),
        )?;
        Ok(decode_start(&body)?)
    }

    fn call(&self, rpc: &str, arguments: &Value) -> Result<Vec<u8>, BackendError> {
        self.0.rpc_bytes_bounded(rpc, arguments, 32 * 1024)
    }
}
