//! Appearance choices refer to catalogue IDs, never caller-provided asset URLs or paid status.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Re-resolve accepted appearance IDs against current entitlements on each host view.
/// This value is independent of immutable gameplay and deck-content snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppearanceSelection {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub back_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub sleeve_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub dice_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub playmat_id: Option<Uuid>,
}

impl AppearanceSelection {
    /// Validate catalogue identities before invoking a canonical writer.
    ///
    /// # Errors
    /// Returns an error for a nil cosmetic identity, including an explicit playmat.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.back_id.is_nil()
            || self.sleeve_id.is_nil()
            || self.dice_id.is_nil()
            || self.playmat_id.is_some_and(|id| id.is_nil())
        {
            return Err("invalid cosmetic selection");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeProfileDefaults {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub expected: AppearanceSelection,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub appearance: AppearanceSelection,
}

impl ChangeProfileDefaults {
    /// Validate both snapshots; current grants are checked by the canonical writer.
    ///
    /// # Errors
    /// Rejects a nil request or cosmetic identity in either snapshot.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil() {
            return Err("invalid defaults request");
        }
        self.expected.validate()?;
        self.appearance.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeDeckAppearance {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub deck_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub appearance: AppearanceSelection,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn appearance_inputs_cannot_claim_entitlement_or_inject_an_owner() {
        let appearance = json!({"backId":Uuid::new_v4(),"sleeveId":Uuid::new_v4(),"diceId":Uuid::new_v4(),"playmatId":null});
        let mut defaults =
            json!({"requestId":Uuid::new_v4(),"expected":appearance,"appearance":appearance});
        assert!(serde_json::from_value::<ChangeProfileDefaults>(defaults.clone()).is_ok());
        assert!(
            serde_json::from_value::<ChangeProfileDefaults>(defaults.clone())
                .unwrap()
                .validate()
                .is_ok()
        );
        let mut missing = defaults.clone();
        missing.as_object_mut().unwrap().remove("expected");
        assert!(serde_json::from_value::<ChangeProfileDefaults>(missing).is_err());
        let mut nil = defaults.clone();
        nil["expected"]["diceId"] = json!(Uuid::nil());
        assert!(serde_json::from_value::<ChangeProfileDefaults>(nil).is_err());
        defaults["accountId"] = json!(Uuid::new_v4());
        assert!(serde_json::from_value::<ChangeProfileDefaults>(defaults.clone()).is_err());
        defaults.as_object_mut().unwrap().remove("accountId");
        defaults["appearance"]["paid"] = json!(true);
        assert!(serde_json::from_value::<ChangeProfileDefaults>(defaults).is_err());
    }
}
