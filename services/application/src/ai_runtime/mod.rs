//! Native Rust AI gameplay runtime supporting Normal AI (unlimited) and
//! Advanced AI (daily-quota-metered) tiers over `compact_v1` prompts,
//! scheduled pass delegation, atomic `cast-batch` execution, and pluggable
//! LLM providers (persistent ChatGPT-login Codex app-server, direct OpenAI
//! Responses API, and one-shot Codex CLI).
pub mod affordances;
pub mod codex_app_server;
pub mod compact;
pub mod driver;
pub mod knowledge;
pub mod lifecycle;
pub mod provider;
pub mod quota;
pub mod scheduler;

pub use affordances::{
    CastableSpell, ManaSourceInfo, ParsedManaCost, PriorityAffordances, auto_tap_plan,
    diagnose_rejection, fallback_command, probe_priority_affordances,
};
pub use codex_app_server::{CodexAppServerConfig, CodexAppServerProvider, HOSTED_CODEX_EXECUTABLE};
pub use driver::{AiRuntimeConfig, AiRuntimeService};
pub use knowledge::{
    HistoricalRevealView, KnowledgeSource, KnownOpponentHandCard, RevealStatusView,
    SeatKnowledgeLedger, SeatKnowledgeView, VisibleLibraryCard, VisibleLibraryPosition,
};
pub use lifecycle::AiControllerRegistry;
pub use provider::{
    AiDecisionProvider, AiModelRequest, AiModelResponse, AiProviderError, AiTierCatalog,
    AiTierModelConfig, AiUsage, CodexCliProvider, OpenAiResponsesProvider, ReasoningEffort,
    StaticScriptedProvider, provider_from_env,
};
pub use quota::{AiQuotaStatus, AiTier, SetAiTierRequest};
pub use scheduler::{AiScheduler, PassGrant, ScheduleDecision, WakeCondition, WakeReason};

#[cfg(test)]
mod knowledge_tests;
#[cfg(test)]
mod tests;
