//! Event-driven live AI turn driver and background platform supervisor.
//!
//! Co-located with `NativeGameRuntime` and `AiControllerRegistry`, this driver:
//! 1. Provisions pending AI room/match bindings to authenticated `Ready` state.
//! 2. Submits AI opening play/draw choices for reserved games.
//! 3. Subscribes directly to `NativeGameRuntime::subscribe_game(game_id)` so AI
//!    seats wake in <1 ms on human moves, skip delegated priority windows with
//!    zero LLM calls, call Normal/Advanced models via `compact_v1`, and commit
//!    single actions or atomic `cast-batch` sequences back into the shared
//!    `CheckpointCache` (waking human SSE streams immediately).
use super::{
    affordances::{PriorityAffordances, auto_tap_plan, diagnose_rejection, fallback_command},
    compact::{AiActionPlan, compact_prompt_with_knowledge, compact_reply_schema_for_snapshot},
    knowledge::SeatKnowledgeView,
    lifecycle::{ActiveAiGameRow, ActiveAiSeatRow, AiControllerRegistry, AiLifecycleClient},
    provider::{AiDecisionProvider, AiModelRequest},
    scheduler::{AiScheduler, ScheduleDecision},
};
use crate::{
    ApiResponse,
    backend::{BackendClient, BackendConfig},
    native_game::{
        NativeControllerViewOutcome, NativeGameRuntime, NativeHostOwner, NativeSourceRepository,
    },
    player_wire::{PlayerAction, PlayerCommand, PlayerSnapshot, PlayerViewRequest},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;
use uuid::Uuid;

const MAX_RETRIES_PER_BOUNDARY: usize = 3;
const MAX_OWN_RECENT: usize = 6;

/// Runtime configuration for the in-process Rust AI platform supervisor.
#[derive(Clone)]
pub struct AiRuntimeConfig {
    pub supabase_base: String,
    pub supabase_secret_key: String,
    pub host_owner: NativeHostOwner,
    pub runner_label: String,
    pub poll_interval: Duration,
}

/// Shared AI runtime service mounted inside `application_api` or `ai_runtime_worker`.
pub struct AiRuntimeService {
    config: AiRuntimeConfig,
    runtime: Arc<NativeGameRuntime>,
    registry: Arc<AiControllerRegistry>,
    provider: Arc<dyn AiDecisionProvider>,
    active_tasks: Mutex<HashSet<Uuid>>,
}

impl AiRuntimeService {
    /// Construct a new AI runtime service sharing the host's `NativeGameRuntime`.
    pub fn new(
        config: AiRuntimeConfig,
        runtime: Arc<NativeGameRuntime>,
        registry: Arc<AiControllerRegistry>,
        provider: Arc<dyn AiDecisionProvider>,
    ) -> Self {
        Self {
            config,
            runtime,
            registry,
            provider,
            active_tasks: Mutex::new(HashSet::new()),
        }
    }

    /// Construct a standalone `AiRuntimeService` from environment variables.
    /// # Errors
    /// Returns a safe message if required Supabase, native runtime, or AI provider
    /// environment variables are missing or invalid.
    pub fn from_env() -> Result<Arc<Self>, String> {
        let base = std::env::var("SPELLWAKE_SUPABASE_URL")
            .map_err(|_| "SPELLWAKE_SUPABASE_URL is required".to_owned())?;
        let secret = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY")
            .map_err(|_| "SPELLWAKE_SUPABASE_SECRET_KEY is required".to_owned())?;
        let host_owner_raw = std::env::var("SPELLWAKE_NATIVE_HOST_OWNER")
            .unwrap_or_else(|_| "spellwake-ai-worker".to_owned());
        let host_owner = NativeHostOwner::new(&host_owner_raw)
            .map_err(|_| "SPELLWAKE_NATIVE_HOST_OWNER is invalid".to_owned())?;
        let runner_label = std::env::var("SPELLWAKE_AI_RUNNER_LABEL")
            .unwrap_or_else(|_| "spellwake-rust-ai-v1".to_owned());

        let native_runtime = NativeGameRuntime::from_env(&base, &secret)
            .map_err(|_| "native game runtime configuration is invalid".to_owned())?
            .ok_or_else(|| "native game runtime environment variables are required".to_owned())?;
        let provider = super::provider::provider_from_env()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                "AI runtime provider not configured (set SPELLWAKE_AI_PROVIDER=openai|codex)"
                    .to_owned()
            })?;

        let config = AiRuntimeConfig {
            supabase_base: base,
            supabase_secret_key: secret,
            host_owner,
            runner_label,
            poll_interval: Duration::from_millis(750),
        };
        let registry = Arc::new(AiControllerRegistry::new(Arc::new(
            crate::controller_possession::ControllerSecretStore::default(),
        )));
        Ok(Arc::new(Self::new(
            config,
            Arc::new(native_runtime),
            registry,
            provider,
        )))
    }

    /// Borrow the controller secret registry.
    pub fn registry(&self) -> &Arc<AiControllerRegistry> {
        &self.registry
    }

    /// Restore an account's reserved daily Advanced AI quota when Spellwake fails
    /// to open a reserved match.
    /// # Errors
    /// Returns an error if the Supabase RPC fails.
    pub fn restore_quota_for_failed_match(&self, match_id: Uuid) -> Result<(), &'static str> {
        let backend_cfg =
            BackendConfig::new(&self.config.supabase_base, &self.config.supabase_secret_key)
                .map_err(|_| "backend config invalid")?;
        let backend = BackendClient::new(
            backend_cfg,
            "spellwake-ai-quota-restore",
            Duration::from_secs(30),
        )
        .map_err(|_| "backend client failed")?;
        let lifecycle = AiLifecycleClient::new(backend, self.config.runner_label.clone());
        lifecycle
            .restore_ai_quota_on_failure(match_id)
            .map(|_| ())
            .map_err(|_| "quota restore RPC failed")
    }

    /// Execute one synchronous provisioning and active-game discovery pass.
    /// Useful for both the background supervisor loop and integration tests.
    pub fn reconcile_once(self: &Arc<Self>, shutdown: &watch::Receiver<bool>) {
        let Ok(backend_cfg) =
            BackendConfig::new(&self.config.supabase_base, &self.config.supabase_secret_key)
        else {
            return;
        };
        let Ok(backend) = BackendClient::new(
            backend_cfg,
            "spellwake-ai-lifecycle",
            Duration::from_secs(30),
        ) else {
            return;
        };
        let lifecycle = AiLifecycleClient::new(backend, self.config.runner_label.clone());

        match lifecycle.read_binding_work(None, 50) {
            Ok(items) => {
                for item in &items {
                    if *shutdown.borrow() {
                        return;
                    }
                    if let Err(err) = lifecycle.reconcile_binding(item, &self.registry) {
                        tracing::warn!(binding = %item.id, error = %err, "AI binding reconcile failed");
                    }
                }
            }
            Err(err) => {
                tracing::debug!(error = %err, "AI binding discovery unavailable");
            }
        }

        let Ok(games) = lifecycle.read_active_ai_games(50) else {
            return;
        };
        for game in games {
            if *shutdown.borrow() {
                return;
            }
            if game.game_state == "reserved" {
                for seat in &game.ai_seats {
                    let _ = self.registry.ensure_possessed(
                        seat.grant_id,
                        seat.binding_id,
                        &seat.epoch,
                        &seat.secret_ref,
                    );
                    let _ = lifecycle.submit_opening_choice(seat, game.id);
                }
                continue;
            }
            if game.game_state == "active" && game.context_id.is_some() && !game.ai_seats.is_empty()
            {
                self.ensure_game_driver(game, shutdown.clone());
            }
        }
    }

    /// Run the background supervisor loop until `shutdown` signals `true`.
    pub async fn run_supervisor(self: Arc<Self>, mut shutdown: watch::Receiver<bool>) {
        tracing::info!(runner = %self.config.runner_label, "AI runtime supervisor started");
        let mut ticker = tokio::time::interval(self.config.poll_interval);
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    let svc = self.clone();
                    let sd = shutdown.clone();
                    let _ = tokio::task::spawn_blocking(move || svc.reconcile_once(&sd)).await;
                }
            }
        }
        tracing::info!(runner = %self.config.runner_label, "AI runtime supervisor stopped");
    }

    fn ensure_game_driver(
        self: &Arc<Self>,
        game: ActiveAiGameRow,
        shutdown: watch::Receiver<bool>,
    ) {
        let Ok(mut guard) = self.active_tasks.lock() else {
            return;
        };
        if !guard.insert(game.id) {
            return;
        }
        drop(guard);

        let svc = self.clone();
        let game_id = game.id;
        tokio::spawn(async move {
            svc.clone().drive_game(game, shutdown).await;
            if let Ok(mut guard) = svc.active_tasks.lock() {
                guard.remove(&game_id);
            }
        });
    }

    async fn drive_game(
        self: Arc<Self>,
        game: ActiveAiGameRow,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let Some(context_id) = game.context_id else {
            return;
        };
        let mut updates = self.runtime.subscribe_game(game.id);
        let schedulers: Arc<Mutex<HashMap<Uuid, SeatDriverState>>> =
            Arc::new(Mutex::new(HashMap::new()));

        tracing::info!(game = %game.id, ai_seats = game.ai_seats.len(), "AI game driver attached");

        loop {
            let svc = self.clone();
            let seats = game.ai_seats.clone();
            let state_map = schedulers.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                svc.step_game_seats(game.id, context_id, &seats, &state_map)
            })
            .await;

            match outcome {
                Ok(DriverStep::Finished) => {
                    tracing::info!(game = %game.id, "AI game driver finished");
                    break;
                }
                Ok(DriverStep::Progressed) => {
                    // Immediately check again in case the same or another AI seat now has priority.
                    continue;
                }
                Ok(DriverStep::WaitingForOther) => {}
                Err(_) => break,
            }

            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        break;
                    }
                }
                recv = updates.recv() => {
                    match recv {
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    }

    fn step_game_seats(
        &self,
        game_id: Uuid,
        context_id: Uuid,
        seats: &[ActiveAiSeatRow],
        state_map: &Mutex<HashMap<Uuid, SeatDriverState>>,
    ) -> DriverStep {
        let Ok(backend_cfg) =
            BackendConfig::new(&self.config.supabase_base, &self.config.supabase_secret_key)
        else {
            return DriverStep::Finished;
        };
        let Ok(backend) =
            BackendClient::new(backend_cfg, "spellwake-ai-driver", Duration::from_secs(60))
        else {
            return DriverStep::WaitingForOther;
        };
        let repository = NativeSourceRepository::new(backend);
        let view_req = PlayerViewRequest {
            game_id,
            context_id,
        };

        let mut any_available = false;
        for seat in seats {
            let Some(proof) = self.registry.ensure_possessed(
                seat.grant_id,
                seat.binding_id,
                &seat.epoch,
                &seat.secret_ref,
            ) else {
                continue;
            };
            let view = match self.runtime.controller_view_with_actions(
                &repository,
                self.registry.secrets(),
                &proof,
                &view_req,
            ) {
                Ok(NativeControllerViewOutcome::Snapshot(v)) => v,
                Ok(NativeControllerViewOutcome::Stale) => return DriverStep::Progressed,
                Ok(NativeControllerViewOutcome::Unavailable) => continue,
                Err(err) => {
                    tracing::warn!(game = %game_id, seat = %seat.seat_id, error = %err, "AI controller_view failed");
                    return DriverStep::WaitingForOther;
                }
            };
            any_available = true;
            let snapshot = &view.snapshot;
            if snapshot.turn.is_none() && snapshot.decisions.is_empty() {
                return DriverStep::Finished;
            }
            if !seat_must_act(snapshot) {
                continue;
            }

            // Deterministic <1 ms fast-pass: when holding priority with zero legal land plays,
            // zero castable spells, and zero activatable non-mana abilities, passing is forced.
            if snapshot.decisions.is_empty() && !view.affordances.has_non_pass_play() {
                let cmd = PlayerCommand {
                    request_id: Uuid::new_v4(),
                    basis: snapshot.basis.clone(),
                    action: PlayerAction::Pass {},
                };
                if let Ok(ApiResponse::Success { .. }) = self.runtime.controller_command(
                    &repository,
                    self.registry.secrets(),
                    &proof,
                    &self.config.host_owner,
                    &cmd,
                ) {
                    return DriverStep::Progressed;
                }
            }

            let decision = {
                let mut map = match state_map.lock() {
                    Ok(m) => m,
                    Err(_) => return DriverStep::Finished,
                };
                map.entry(seat.seat_id)
                    .or_default()
                    .scheduler
                    .evaluate(snapshot)
            };

            match decision {
                ScheduleDecision::DelegatePass => {
                    let cmd = PlayerCommand {
                        request_id: Uuid::new_v4(),
                        basis: snapshot.basis.clone(),
                        action: PlayerAction::Pass {},
                    };
                    match self.runtime.controller_command(
                        &repository,
                        self.registry.secrets(),
                        &proof,
                        &self.config.host_owner,
                        &cmd,
                    ) {
                        Ok(ApiResponse::Success { .. }) => {
                            if let Ok(mut map) = state_map.lock()
                                && let Some(st) = map.get_mut(&seat.seat_id)
                            {
                                st.scheduler.record_delegated_pass();
                            }
                            return DriverStep::Progressed;
                        }
                        _ => {
                            if let Ok(mut map) = state_map.lock()
                                && let Some(st) = map.get_mut(&seat.seat_id)
                            {
                                st.scheduler.revoke();
                            }
                            return DriverStep::Progressed;
                        }
                    }
                }
                ScheduleDecision::CallModel(_reason) => {
                    let own_recent = state_map
                        .lock()
                        .ok()
                        .and_then(|m| {
                            m.get(&seat.seat_id)
                                .map(|s| s.recent.iter().cloned().collect::<Vec<_>>())
                        })
                        .unwrap_or_default();

                    if self.execute_model_turn(
                        &repository,
                        &proof,
                        seat,
                        snapshot,
                        &view.affordances,
                        &view.knowledge,
                        &own_recent,
                        &view_req,
                        state_map,
                    ) {
                        return DriverStep::Progressed;
                    }
                    return DriverStep::WaitingForOther;
                }
            }
        }

        if any_available {
            DriverStep::WaitingForOther
        } else {
            DriverStep::Finished
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_model_turn(
        &self,
        repository: &NativeSourceRepository,
        proof: &crate::controller_possession::PossessedController,
        seat: &ActiveAiSeatRow,
        snapshot: &PlayerSnapshot,
        affordances: &PriorityAffordances,
        knowledge: &SeatKnowledgeView,
        own_recent: &[String],
        view_req: &PlayerViewRequest,
        state_map: &Mutex<HashMap<Uuid, SeatDriverState>>,
    ) -> bool {
        let schema = compact_reply_schema_for_snapshot(snapshot);
        let cache_key = format!(
            "spellwake:{}:{}:{}",
            seat.ai_tier.as_str(),
            snapshot.basis.game_id,
            seat.seat_id
        );
        let mut feedback: Option<String> = None;

        for attempt in 0..MAX_RETRIES_PER_BOUNDARY {
            let Ok((prompt, codec)) = compact_prompt_with_knowledge(
                snapshot,
                affordances,
                Some(knowledge),
                own_recent,
                feedback.as_deref(),
            ) else {
                return false;
            };
            let req = AiModelRequest {
                tier: seat.ai_tier,
                game_id: snapshot.basis.game_id,
                seat_id: seat.seat_id,
                prompt,
                schema: schema.clone(),
                cache_key: cache_key.clone(),
            };
            let response = match self.provider.decide(&req) {
                Ok(resp) => resp,
                Err(err) => {
                    tracing::warn!(
                        game = %snapshot.basis.game_id,
                        seat = %seat.seat_id,
                        tier = seat.ai_tier.as_str(),
                        attempt,
                        error = %err,
                        "AI model call failed"
                    );
                    if !err.is_retryable() {
                        break;
                    }
                    continue;
                }
            };
            tracing::info!(
                game = %snapshot.basis.game_id,
                seat = %seat.seat_id,
                tier = seat.ai_tier.as_str(),
                model = %response.model,
                latency_ms = response.latency_ms,
                input_tokens = response.usage.input_tokens,
                cached_tokens = response.usage.cached_input_tokens,
                output_tokens = response.usage.output_tokens,
                "AI decision completed"
            );

            let expanded = match codec.expand_reply(&response.raw_json, &snapshot.basis) {
                Ok(exp) => exp,
                Err(err) => {
                    tracing::warn!(
                        game = %snapshot.basis.game_id,
                        seat = %seat.seat_id,
                        attempt,
                        error = %err,
                        "AI reply codec expansion rejected"
                    );
                    feedback = Some(format!("previous reply rejected before commit: {err}"));
                    continue;
                }
            };

            let plan = auto_tap_plan(
                expanded.plan,
                snapshot,
                &affordances.mana_sources,
                &snapshot.basis,
            );

            let commit_result = match &plan {
                AiActionPlan::Single(cmd) => self.runtime.controller_command(
                    repository,
                    self.registry.secrets(),
                    proof,
                    &self.config.host_owner,
                    cmd,
                ),
                AiActionPlan::CastBatch(cmds) => self.runtime.controller_cast_batch(
                    repository,
                    self.registry.secrets(),
                    proof,
                    &self.config.host_owner,
                    cmds,
                ),
            };

            match commit_result {
                Ok(ApiResponse::Success { .. }) => {
                    let post_snap = match self.runtime.controller_view_with_actions(
                        repository,
                        self.registry.secrets(),
                        proof,
                        view_req,
                    ) {
                        Ok(NativeControllerViewOutcome::Snapshot(v)) => Some(v.snapshot),
                        _ => None,
                    };
                    if let Ok(mut map) = state_map.lock() {
                        let st = map.entry(seat.seat_id).or_default();
                        let summary = summarize_plan(&plan);
                        st.push_recent(summary);
                        if let Some(post) = post_snap {
                            st.scheduler.record_model_acceptance(
                                expanded.delegate_pass,
                                expanded.wake,
                                expanded.max_skips,
                                &post,
                            );
                        } else {
                            st.scheduler.revoke();
                        }
                    }
                    return true;
                }
                Ok(ApiResponse::Fail { data }) => {
                    let msg = data
                        .iter()
                        .map(|(k, v)| format!("{k}: {v}"))
                        .collect::<Vec<_>>()
                        .join("; ");
                    if msg.contains("stale") {
                        return true;
                    }
                    tracing::warn!(
                        game = %snapshot.basis.game_id,
                        seat = %seat.seat_id,
                        attempt,
                        rejection = %msg,
                        "AI action rejected by native engine"
                    );
                    feedback = Some(diagnose_rejection(&plan, affordances, &codec, &msg));
                }
                Ok(ApiResponse::Error { .. }) | Err(_) => {
                    return false;
                }
            }
        }

        // Safe fallback so a live human match never hangs if all retries exhaust.
        if let Some(cmd) = fallback_command(snapshot) {
            tracing::warn!(
                game = %snapshot.basis.game_id,
                seat = %seat.seat_id,
                "AI retries exhausted; committing safe fallback action"
            );
            if let Ok(ApiResponse::Success { .. }) = self.runtime.controller_command(
                repository,
                self.registry.secrets(),
                proof,
                &self.config.host_owner,
                &cmd,
            ) {
                if let Ok(mut map) = state_map.lock() {
                    let st = map.entry(seat.seat_id).or_default();
                    st.scheduler.revoke();
                }
                return true;
            }
        }
        false
    }
}

#[derive(Default)]
struct SeatDriverState {
    scheduler: AiScheduler,
    recent: VecDeque<String>,
}

impl SeatDriverState {
    fn push_recent(&mut self, summary: String) {
        if self.recent.len() >= MAX_OWN_RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(summary);
    }
}

enum DriverStep {
    Progressed,
    WaitingForOther,
    Finished,
}

/// Returns true when the seat owning `snapshot` currently has a required decision
/// or holds priority.
pub fn seat_must_act(snapshot: &PlayerSnapshot) -> bool {
    if !snapshot.decisions.is_empty() {
        return true;
    }
    snapshot
        .turn
        .as_ref()
        .is_some_and(|t| t.priority_seat_ids.contains(&snapshot.viewer_seat_id))
}

fn summarize_plan(plan: &AiActionPlan) -> String {
    match plan {
        AiActionPlan::Single(cmd) => match &cmd.action {
            PlayerAction::Pass {} => "pass".to_owned(),
            PlayerAction::PlayLand { .. } => "play-land".to_owned(),
            PlayerAction::ActivateMana { .. } => "activate-mana".to_owned(),
            PlayerAction::CastUntargeted { .. } => "cast-untargeted".to_owned(),
            PlayerAction::CastTargeted { .. } => "cast-targeted".to_owned(),
            PlayerAction::Choose { .. } => "choose".to_owned(),
            _ => "action".to_owned(),
        },
        AiActionPlan::CastBatch(cmds) => format!("cast-batch({} steps)", cmds.len()),
    }
}
