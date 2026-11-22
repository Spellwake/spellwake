//! Concrete RPC lifecycle adapter for AI runner bindings, grants, readiness,
//! stop acknowledgments, and opening-choice responses.
use super::quota::AiTier;
use crate::{
    ApiResponse,
    amount::Amount,
    backend::{BackendClient, BackendError},
    controller_possession::{ControllerSecretStore, PossessedController},
    source_protocol::SecretReference,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

/// Active local custody of possessed AI controller credentials paired with their
/// backing `ControllerSecretStore`.
#[derive(Default)]
pub struct AiControllerRegistry {
    secrets: Arc<ControllerSecretStore>,
    possessed: Mutex<HashMap<Uuid, Arc<PossessedController>>>,
    binding_grants: Mutex<HashMap<Uuid, Vec<Uuid>>>,
}

impl AiControllerRegistry {
    /// Create a registry backed by a shared `ControllerSecretStore`.
    pub fn new(secrets: Arc<ControllerSecretStore>) -> Self {
        Self {
            secrets,
            possessed: Mutex::new(HashMap::new()),
            binding_grants: Mutex::new(HashMap::new()),
        }
    }

    /// Borrow the underlying `ControllerSecretStore`.
    pub fn secrets(&self) -> &Arc<ControllerSecretStore> {
        &self.secrets
    }

    /// Install a freshly allocated or adopted grant into local secret custody and
    /// return its verified `PossessedController` proof.
    pub fn ensure_possessed(
        &self,
        grant_id: Uuid,
        binding_id: Uuid,
        epoch: &Amount,
        secret_ref: &str,
    ) -> Option<Arc<PossessedController>> {
        if let Ok(guard) = self.possessed.lock()
            && let Some(existing) = guard.get(&grant_id)
            && self.secrets.current(existing).unwrap_or(false)
        {
            return Some(existing.clone());
        }
        let reference: SecretReference = secret_ref.parse().ok()?;
        let credential = self
            .secrets
            .install(grant_id, binding_id, epoch.clone(), reference)
            .ok()?;
        let proof = Arc::new(self.secrets.authenticate(grant_id, &credential).ok()??);
        if let Ok(mut guard) = self.possessed.lock() {
            guard.insert(grant_id, proof.clone());
        }
        if let Ok(mut guard) = self.binding_grants.lock() {
            guard.entry(binding_id).or_default().push(grant_id);
        }
        Some(proof)
    }

    /// Look up an already possessed controller proof for `grant_id`.
    pub fn get_possessed(&self, grant_id: Uuid) -> Option<Arc<PossessedController>> {
        let guard = self.possessed.lock().ok()?;
        let proof = guard.get(&grant_id)?.clone();
        if self.secrets.current(&proof).unwrap_or(false) {
            Some(proof)
        } else {
            None
        }
    }

    /// Revoke all locally held grants associated with `binding_id`.
    pub fn revoke_binding(&self, binding_id: Uuid) {
        let grants = self
            .binding_grants
            .lock()
            .ok()
            .and_then(|mut guard| guard.remove(&binding_id))
            .unwrap_or_default();
        for grant_id in grants {
            let _ = self.secrets.revoke(grant_id);
            if let Ok(mut guard) = self.possessed.lock() {
                guard.remove(&grant_id);
            }
        }
    }
}

/// One row from `app.read_native_ai_binding_work`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiBindingWorkItem {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub epoch: Amount,
    pub desired_state: String,
    pub observed_state: String,
    #[serde(default)]
    pub ai_tier: AiTier,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub room_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub slot_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub match_seat_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub deck_revision_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub configuration_id: Option<Uuid>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub format_id: Option<Uuid>,
    #[serde(default)]
    pub mode: Option<String>,
    pub start_work: Option<AiStartWorkRow>,
    pub has_pending_stops: bool,
    pub grant: Option<AiGrantRow>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStartWorkRow {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub start_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub start_key: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiGrantRow {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub epoch: Amount,
    pub secret_ref: String,
    pub delivery_revision: Amount,
    pub delivery_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkPage<T> {
    items: Vec<T>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    next_after: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StopWorkRow {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    stop_id: Uuid,
    epoch: Amount,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StopPage {
    items: Vec<StopWorkRow>,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    next_cursor: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefaultDeckRow {
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    deck_revision_id: Option<Uuid>,
}

/// Active or reserved game returned by `app.read_native_ai_active_games`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAiGameRow {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    pub game_state: String,
    pub accepted_revision: Amount,
    #[serde(default, deserialize_with = "crate::identifiers::nullable_uuid")]
    pub context_id: Option<Uuid>,
    pub ai_seats: Vec<ActiveAiSeatRow>,
}

/// One AI seat inside an active or reserved game.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAiSeatRow {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub seat_id: Uuid,
    #[serde(default)]
    pub ai_tier: AiTier,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub binding_id: Uuid,
    pub epoch: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub grant_id: Uuid,
    pub secret_ref: String,
}

/// Service-role RPC client for AI binding provisioning, stop cleanup, active
/// game discovery, and opening choices.
pub(crate) struct AiLifecycleClient {
    backend: BackendClient,
    runner_label: String,
}

impl AiLifecycleClient {
    pub(crate) fn new(backend: BackendClient, runner_label: String) -> Self {
        Self {
            backend,
            runner_label,
        }
    }

    /// Fetch one bounded page of pending AI binding work.
    pub(crate) fn read_binding_work(
        &self,
        after: Option<Uuid>,
        limit: u16,
    ) -> Result<Vec<AiBindingWorkItem>, BackendError> {
        let mut out = Vec::new();
        let mut cursor = after;
        loop {
            let page: WorkPage<AiBindingWorkItem> = self.backend.rpc(
                "read_native_ai_binding_work",
                &json!({ "p_after": cursor, "p_limit": limit.clamp(1, 100) }),
            )?;
            out.extend(page.items);
            match page.next_after {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Ok(out)
    }

    /// Discover all reserved/active games that contain at least one AI seat.
    pub(crate) fn read_active_ai_games(
        &self,
        limit: u16,
    ) -> Result<Vec<ActiveAiGameRow>, BackendError> {
        let mut out = Vec::new();
        let mut cursor: Option<Uuid> = None;
        loop {
            let page: WorkPage<ActiveAiGameRow> = self.backend.rpc(
                "read_native_ai_active_games",
                &json!({ "p_after": cursor, "p_limit": limit.clamp(1, 100) }),
            )?;
            out.extend(page.items);
            match page.next_after {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Ok(out)
    }

    /// Advance one pending AI binding through stop cleanup, provider observation,
    /// local secret custody + grant delivery, and authenticated readiness.
    pub(crate) fn reconcile_binding(
        &self,
        item: &AiBindingWorkItem,
        registry: &AiControllerRegistry,
    ) -> Result<(), BackendError> {
        if item.has_pending_stops {
            self.drain_stops(item.id, registry)?;
        }
        if item.desired_state != "running" {
            return Ok(());
        }

        // 1. Observe provider running if still pending.
        if item.observed_state == "pending"
            && let Some(start) = &item.start_work
        {
            let provider_ref = format!("{}:{}", self.runner_label, item.id);
            let proof = format!("in-process-rust-runner:{}", start.start_key);
            let _: Value = self.backend.rpc(
                "observe_ai_provider",
                &json!({
                    "p_binding": item.id,
                    "p_epoch": item.epoch.to_string(),
                    "p_start": start.start_id,
                    "p_outcome": "running",
                    "p_provider_ref": provider_ref,
                    "p_proof": proof,
                    "p_request": Uuid::new_v4(),
                }),
            )?;
        }

        // 2. Ensure an active grant exists and is delivered into local secret custody.
        let (grant_id, grant_epoch, secret_ref, delivery_rev, needs_delivery) =
            if let Some(grant) = &item.grant {
                (
                    grant.id,
                    grant.epoch.clone(),
                    grant.secret_ref.clone(),
                    grant.delivery_revision.clone(),
                    grant.delivery_state != "delivered",
                )
            } else {
                let secret_ref = format!("controller-secret:{}:{}", item.id, Uuid::new_v4());
                let resp: Value = self.backend.rpc(
                    "issue_ai_grant",
                    &json!({
                        "p_binding": item.id,
                        "p_epoch": item.epoch.to_string(),
                        "p_secret_ref": secret_ref,
                        "p_request": Uuid::new_v4(),
                    }),
                )?;
                if resp.get("status").and_then(Value::as_str) != Some("success") {
                    return Ok(());
                }
                let Some(gid) = resp
                    .pointer("/data/resourceId")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse::<Uuid>().ok())
                else {
                    return Ok(());
                };
                (
                    gid,
                    item.epoch.clone(),
                    secret_ref,
                    Amount::from(0_u64),
                    true,
                )
            };

        let Some(_proof) = registry.ensure_possessed(grant_id, item.id, &grant_epoch, &secret_ref)
        else {
            return Ok(());
        };

        if needs_delivery {
            let _: Value = self.backend.rpc(
                "observe_ai_grant_delivery",
                &json!({
                    "p_grant": grant_id,
                    "p_expected": delivery_rev.to_string(),
                    "p_outcome": "delivered",
                    "p_proof": format!("local-secret-store:{}:{}", self.runner_label, grant_id),
                    "p_request": Uuid::new_v4(),
                }),
            )?;
        }

        // 3. Confirm authenticated readiness with either the sponsor's chosen deck or
        //    a sealed system-pool deck for "Random Deck".
        let Some(config_id) = item.configuration_id else {
            return Ok(());
        };
        let deck_revision = if let Some(rev) = item.deck_revision_id {
            Some(rev)
        } else if let (Some(format_id), Some(mode)) = (item.format_id, item.mode.as_deref()) {
            let resp: ApiResponse<DefaultDeckRow> = self.backend.rpc(
                "read_native_ai_default_deck",
                &json!({ "p_format_id": format_id, "p_mode": mode }),
            )?;
            match resp {
                ApiResponse::Success { data } => data.deck_revision_id,
                _ => None,
            }
        } else {
            None
        };

        let Some(revision_id) = deck_revision else {
            tracing::debug!(binding = %item.id, "AI binding waiting for sealed deck revision");
            return Ok(());
        };

        let _: Value = self.backend.rpc(
            "commit_ai_ready_confirmation",
            &json!({
                "p_grant": grant_id,
                "p_epoch": grant_epoch.to_string(),
                "p_revision": revision_id,
                "p_qualification": format!("native-ai-runtime:{}:{}", item.ai_tier.as_str(), self.runner_label),
                "p_authentication": format!("controller-hmac-v1:{grant_id}"),
                "p_configuration": config_id,
                "p_request": Uuid::new_v4(),
            }),
        )?;
        Ok(())
    }

    fn drain_stops(
        &self,
        binding_id: Uuid,
        registry: &AiControllerRegistry,
    ) -> Result<(), BackendError> {
        registry.revoke_binding(binding_id);
        let mut cursor: Option<Uuid> = None;
        loop {
            let page: StopPage = self.backend.rpc(
                "read_ai_stop_work_page",
                &json!({
                    "p_binding": binding_id,
                    "p_after": cursor,
                    "p_limit": 50
                }),
            )?;
            for stop in &page.items {
                let _: Value = self.backend.rpc(
                    "observe_ai_stop",
                    &json!({
                        "p_binding": binding_id,
                        "p_epoch": stop.epoch.to_string(),
                        "p_stop": stop.stop_id,
                        "p_proof": format!("local-runner-stopped:{}:{}", self.runner_label, stop.stop_id),
                        "p_request": Uuid::new_v4(),
                    }),
                )?;
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Ok(())
    }

    /// Submit an AI seat's opening play/draw decision when it wins the opening roll.
    /// Defaults to playing first (`p_play_first: true`).
    pub(crate) fn submit_opening_choice(
        &self,
        seat: &ActiveAiSeatRow,
        game_id: Uuid,
    ) -> Result<Value, BackendError> {
        self.backend.rpc(
            "native_opening_controller_choice",
            &json!({
                "p_grant": seat.grant_id,
                "p_epoch": seat.epoch.to_string(),
                "p_secret_ref": seat.secret_ref,
                "p_game": game_id,
                "p_play_first": true,
                "p_prior_chooser": Value::Null,
                "p_first_seat": Value::Null,
            }),
        )
    }

    /// Restore an account's reserved Advanced AI quota if Spellwake fails before opening.
    pub(crate) fn restore_ai_quota_on_failure(
        &self,
        match_id: Uuid,
    ) -> Result<Value, BackendError> {
        self.backend.rpc(
            "restore_ai_usage_reservation",
            &json!({ "p_match": match_id }),
        )
    }
}
