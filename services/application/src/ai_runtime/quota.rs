//! Per-seat AI tier vocabulary and daily Advanced AI quota contracts.
use crate::{amount::Amount, identifiers, source_protocol::parse_utc_microsecond};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Per-seat AI quality tier.
/// Normal AI is unlimited; Advanced AI consumes the host account's daily quota.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", try_from = "String")]
pub enum AiTier {
    #[default]
    Normal,
    Advanced,
}

impl AiTier {
    /// Canonical lowercase wire label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Advanced => "advanced",
        }
    }
}

impl TryFrom<String> for AiTier {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "normal" => Ok(Self::Normal),
            "advanced" => Ok(Self::Advanced),
            _ => Err("invalid AI tier"),
        }
    }
}

/// Request body for `POST /v1/ai-slots/{slot}/tier`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetAiTierRequest {
    #[serde(deserialize_with = "identifiers::uuid")]
    pub request_id: Uuid,
    pub expected_revision: Amount,
    pub tier: AiTier,
}

/// Authenticated daily Advanced AI allowance snapshot returned by `GET /v1/account/ai-quota`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiQuotaStatus {
    #[serde(deserialize_with = "identifiers::uuid")]
    pub account_id: Uuid,
    pub tier_name: String,
    pub display_unlimited: bool,
    pub daily_limit: Option<u32>,
    pub used_today: u32,
    pub remaining_today: Option<u32>,
    pub normal_unlimited: bool,
    pub resets_at: String,
}

impl AiQuotaStatus {
    /// Validate quota invariants for the authenticated caller.
    /// # Errors
    /// Rejects foreign account records, invalid reset timestamps, or a Benefactor
    /// record that accidentally discloses a numeric public `dailyLimit`.
    pub fn validate(&self, owner: Uuid) -> Result<(), &'static str> {
        if owner.is_nil() || self.account_id != owner {
            return Err("invalid AI quota account");
        }
        if self.tier_name.trim().is_empty() || self.tier_name.len() > 64 {
            return Err("invalid AI quota tier name");
        }
        if !self.normal_unlimited {
            return Err("normal AI must remain unlimited");
        }
        if self.display_unlimited {
            if self.daily_limit.is_some() {
                return Err("unlimited tier must not expose internal hard cap");
            }
            if let Some(rem) = self.remaining_today
                && rem != 0
            {
                return Err("unlimited tier remaining must be null or 0 when exhausted");
            }
        } else {
            let Some(limit) = self.daily_limit else {
                return Err("finite tier requires daily limit");
            };
            let Some(rem) = self.remaining_today else {
                return Err("finite tier requires remaining count");
            };
            if rem != limit.saturating_sub(self.used_today) {
                return Err("remaining quota disagrees with limit and usage");
            }
        }
        if parse_utc_microsecond(&self.resets_at).is_none() {
            return Err("invalid AI quota reset timestamp");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_tier_roundtrip() {
        crate::test_http::string_enum_roundtrip::<AiTier>(&["normal", "advanced"]);
    }

    #[test]
    fn quota_validation_enforces_unlimited_cap_privacy() {
        let owner = Uuid::from_u128(42);
        let finite = AiQuotaStatus {
            account_id: owner,
            tier_name: "Free".into(),
            display_unlimited: false,
            daily_limit: Some(3),
            used_today: 1,
            remaining_today: Some(2),
            normal_unlimited: true,
            resets_at: "2026-11-23T00:00:00.000000Z".into(),
        };
        assert!(finite.validate(owner).is_ok());

        let benefactor = AiQuotaStatus {
            account_id: owner,
            tier_name: "Benefactor".into(),
            display_unlimited: true,
            daily_limit: None,
            used_today: 12,
            remaining_today: None,
            normal_unlimited: true,
            resets_at: "2026-11-23T00:00:00.000000Z".into(),
        };
        assert!(benefactor.validate(owner).is_ok());

        let leaked_cap = AiQuotaStatus {
            daily_limit: Some(250),
            ..benefactor
        };
        assert!(leaked_cap.validate(owner).is_err());
    }
}
