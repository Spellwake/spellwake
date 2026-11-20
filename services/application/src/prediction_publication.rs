//! Safe publication source projection; raw artifact/probe evidence remains private.
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
/// Exact printing/finish price observed in one currently qualified accepted source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicationSource {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub printing_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub probe_id: Uuid,
    pub price_metric: String,
    pub current_usd: String,
    pub acquired_at: DateTime<Utc>,
    pub usable_until: DateTime<Utc>,
}
/// Caller-bound preflight; publication revalidates source age and future terms independently.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedPublicationSource {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub source: PublicationSource,
}
impl OwnedPublicationSource {
    /// Validate requested printing, exact USD grammar and the published24-hour evidence window.
    ///
    /// # Errors
    /// Rejects nil/mismatched identity, unsupported metric, malformed USD or inconsistent expiry.
    pub fn validate(&self, printing: Uuid) -> Result<(), &'static str> {
        if self.account_id.is_nil()
            || self.source.printing_id != printing
            || printing.is_nil()
            || self.source.probe_id.is_nil()
            || !matches!(
                self.source.price_metric.as_str(),
                "scryfall:usd" | "scryfall:usd_foil" | "scryfall:usd_etched"
            )
            || !crate::predictions::price_decimal(&self.source.current_usd, false)
            || self
                .source
                .acquired_at
                .checked_add_signed(Duration::hours(24))
                != Some(self.source.usable_until)
        {
            return Err("invalid publication source");
        }
        Ok(())
    }
}

/// Server-selected retained source identity for an authenticated preparation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeBasis {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub printing_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub external_printing_id: Uuid,
    pub slot: chrono::NaiveDate,
}
/// This identity is read with the original caller's bearer, never supplied by the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedProbeBasis {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub basis: ProbeBasis,
}
impl OwnedProbeBasis {
    /// Validate the requested printing and fixed source identity.
    /// # Errors
    /// Rejects nil, mismatched or noncanonical identities.
    pub fn validate(&self, printing: Uuid) -> Result<(), &'static str> {
        if self.account_id.is_nil() || printing.is_nil() || self.basis.printing_id != printing {
            return Err("invalid probe basis");
        }
        self.input().validate().map_err(|_| "invalid probe basis")
    }
    pub fn input(&self) -> crate::price_workflow::probe::ProbeInput {
        crate::price_workflow::probe::ProbeInput {
            slot: self.basis.slot,
            external_printing_id: self.basis.external_printing_id,
        }
    }
}
/// Shared extraction status. Completed still requires a successful finish-specific GET preflight.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourcePreparation {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub printing_id: Uuid,
    pub slot: chrono::NaiveDate,
    pub state: crate::price_workflow::probe::ProbeStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::predictions::PredictionCommand;
    use serde_json::{Value, json};
    #[test]
    fn probe_basis_rejects_wrong_printing_nil_source_and_private_fields() {
        let printing = Uuid::new_v4();
        let basis = OwnedProbeBasis {
            account_id: Uuid::new_v4(),
            basis: ProbeBasis {
                printing_id: printing,
                external_printing_id: Uuid::new_v4(),
                slot: chrono::NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            },
        };
        basis.validate(printing).unwrap();
        assert!(basis.validate(Uuid::new_v4()).is_err());
        let mut malformed = basis.clone();
        malformed.basis.external_printing_id = Uuid::nil();
        assert!(malformed.validate(printing).is_err());
        let mut private = serde_json::to_value(basis).unwrap();
        private["basis"]["artifactRef"] = json!("private");
        assert!(serde_json::from_value::<OwnedProbeBasis>(private).is_err());
    }
    #[test]
    fn publication_fixture_preserves_exact_source_expiry_and_rejects_unsupported_instants() {
        let f: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/prediction-publication.json"
        ))
        .unwrap();
        let command: PredictionCommand = serde_json::from_value(f["command"].clone()).unwrap();
        command.validate().unwrap();
        let source: OwnedPublicationSource = serde_json::from_value(f["source"].clone()).unwrap();
        source.validate(source.source.printing_id).unwrap();
        let mut precision = f["command"].clone();
        precision["command"]["tradingClosesAt"] = json!("2030-01-01T00:00:00.000000001Z");
        assert!(
            serde_json::from_value::<PredictionCommand>(precision)
                .unwrap()
                .validate()
                .is_err()
        );
        let mut zero = f["command"].clone();
        zero["command"]["targetUsd"] = json!("0.00");
        assert!(
            serde_json::from_value::<PredictionCommand>(zero)
                .unwrap()
                .validate()
                .is_err()
        );
        let mut zero = f["source"].clone();
        zero["source"]["currentUsd"] = json!("0.00");
        serde_json::from_value::<OwnedPublicationSource>(zero)
            .unwrap()
            .validate(source.source.printing_id)
            .unwrap();
        let mut expiry = f["source"].clone();
        expiry["source"]["usableUntil"] = json!("2026-11-04T12:00:00.123456Z");
        assert!(
            serde_json::from_value::<OwnedPublicationSource>(expiry)
                .unwrap()
                .validate(source.source.printing_id)
                .is_err()
        );
        let mut private = f["source"].clone();
        private["source"]["artifactRef"] = json!("private:source");
        assert!(serde_json::from_value::<OwnedPublicationSource>(private).is_err());
    }
}
