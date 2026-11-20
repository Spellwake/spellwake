//! Execute an explicitly selected assignment or a bounded batch of committed events.
use super::{DailyQuestSelection, QuestActivityAck, QuestActivityBatch, SourceResponseError};
use crate::{
    ApiResponse,
    backend::{BackendClient, BackendConfig},
    commands::CommandAck,
};
use serde_json::json;
use std::{error::Error, time::Duration};

/// Private service client for daily quest delivery and settled activity.
pub struct QuestWorker(BackendClient);

impl QuestWorker {
    /// Load the configured private Supabase origin and credential.
    /// # Errors
    /// Returns configuration and transport initialization failures.
    pub fn from_env() -> Result<Self, Box<dyn Error>> {
        Ok(Self(BackendClient::new(
            BackendConfig::from_env()?,
            "spellwake-quest-worker/1",
            Duration::from_secs(60),
        )?))
    }

    /// Deliver the trusted selector's original assignment for the stated UTC day.
    /// # Errors
    /// Returns errors for invalid input, transport failure or an invalid acknowledgment.
    pub fn assign(
        &self,
        selection: &DailyQuestSelection,
    ) -> Result<ApiResponse<CommandAck>, Box<dyn Error>> {
        if !selection.valid() {
            return Err(SourceResponseError::context().into());
        }
        let bytes = self.0.rpc_bytes_bounded(
            "generate_daily_quest",
            &json!({
                "p_actor": selection.actor_id, "p_definition": selection.definition_id,
                "p_day": selection.day, "p_request": selection.request_id
            }),
            32 * 1024,
        )?;
        Ok(selection.decode_generate_response(&bytes)?)
    }

    /// Advance the game cursor over at most the specified number of accepted events.
    /// Concurrent calls share the same database cursor and reward records.
    /// # Errors
    /// Returns errors for invalid input, transport failure or an invalid cursor response.
    pub fn consume(&self, batch: &QuestActivityBatch) -> Result<QuestActivityAck, Box<dyn Error>> {
        if !batch.valid() {
            return Err(SourceResponseError::context().into());
        }
        let bytes = self.0.rpc_bytes_bounded(
            "consume_quest_game_activity",
            &json!({"p_game":batch.game_id,"p_limit":batch.limit}),
            32 * 1024,
        )?;
        Ok(batch.decode_response(&bytes, None)?)
    }
}
