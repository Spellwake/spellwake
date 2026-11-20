//! Private178 cleanup contracts. No resource worker or runtime SQL capability is installed.
//! Auth absence and revoked authority do not prove secret/process resource absence.
use crate::{
    amount::Amount,
    source_protocol::{SecretReference, SourceProof},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Actual resource observation; Unknown never removes the work from discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum CleanupOutcome {
    Unknown,
    Absent,
}
impl TryFrom<String> for CleanupOutcome {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "unknown" => Ok(Self::Unknown),
            "absent" => Ok(Self::Absent),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Source-bound private work. Secret locators have neither Debug nor Serialize.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceWork {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub work_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub grant_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub binding_id: Uuid,
    pub epoch: Amount,
    pub secret_ref: SecretReference,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub retirement_operation_id: Uuid,
}
/// The exact178 page envelope; owner is verified by SQL, not echoed in each item.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourcePage {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<ResourceWork>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl ResourcePage {
    /// Bind a page to the caller's canonical retirement operation and original bounds.
    /// This checks payload coherence; it does not independently authenticate the owner.
    pub fn valid_for(&self, operation: Uuid, after: Option<Uuid>, limit: u16) -> bool {
        if operation.is_nil()
            || !(1..=100).contains(&limit)
            || after.is_some_and(|id| id.is_nil())
            || self.items.len() > usize::from(limit)
        {
            return false;
        }
        let expected = if self.items.len() == usize::from(limit) {
            self.items.last().map(|v| v.work_id)
        } else {
            None
        };
        self.next_cursor == expected
            && self.items.iter().all(|v| {
                v.retirement_operation_id == operation && after.is_none_or(|id| v.work_id > id)
            })
            && self.items.windows(2).all(|v| v[0].work_id < v[1].work_id)
    }
}
/// Original trusted observation. SQL derives workHash and retirement source;
/// the narrower page projection must never be hashed as those canonical terms.
/// Recover an unknown transport result by repeating this exact observation:
/// account-resources:<account>/account_resource_ack_v1/request binds workId,
/// canonical workHash, epoch, outcome and original proof under the retained source.
pub struct CleanupObservation {
    pub account_id: Uuid,
    pub work_id: Uuid,
    pub epoch: Amount,
    pub outcome: CleanupOutcome,
    pub proof_ref: SourceProof,
    pub request_id: Uuid,
}
impl CleanupObservation {
    /// Envelope validity does not prove resource absence or authorize a caller.
    pub fn valid(&self) -> bool {
        !self.account_id.is_nil() && !self.work_id.is_nil() && !self.request_id.is_nil()
    }
}
/// Accepted source observation, not whole-account deletion completion.
#[derive(Debug)]
pub struct CleanupAck {
    pub resource_id: Uuid,
    pub disposition: CleanupOutcome,
}
impl<'de> Deserialize<'de> for CleanupAck {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            resource_id: Uuid,
            disposition: CleanupOutcome,
        }
        let wire: Wire = crate::provider_json::object(input)?;
        Ok(Self {
            resource_id: wire.resource_id,
            disposition: wire.disposition,
        })
    }
}
impl CleanupAck {
    /// Compare the accepted observation with the exact original work and outcome.
    pub fn valid_for(&self, original: &CleanupObservation) -> bool {
        original.valid()
            && self.resource_id == original.work_id
            && self.disposition == original.outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode_wire_object, test_http::positional};
    use serde_json::json;
    #[test]
    fn pages_bind_retirement_order_and_full_page_cursor_without_exposing_secret_debug() {
        let operation = Uuid::new_v4();
        let work = Uuid::from_u128(100);
        let item = json!({"workId":work,"grantId":Uuid::new_v4(),"bindingId":Uuid::new_v4(),"epoch":"0","secretRef":"controller-secret:fixture","retirementOperationId":operation});
        let value = json!({"items":[item.clone()],"nextCursor":work});
        let page: ResourcePage = decode_wire_object(value.to_string().as_bytes()).unwrap();
        assert!(page.valid_for(operation, None, 1));
        assert!(!page.valid_for(operation, None, 2));
        assert!(!page.valid_for(Uuid::new_v4(), None, 1));
        assert!(!page.valid_for(operation, Some(work), 1));
        let partial = json!({"items":[item.clone()],"nextCursor":null});
        let page: ResourcePage = decode_wire_object(partial.to_string().as_bytes()).unwrap();
        assert!(page.valid_for(operation, None, 2));
        for bad in [
            json!({"items":[positional(&item,&["workId","grantId","bindingId","epoch","secretRef","retirementOperationId"])],"nextCursor":null}),
            json!({"items":[]}),
            json!({"items":[],"nextCursor":"00000000-0000-0000-0000-000000000000"}),
        ] {
            assert!(decode_wire_object::<ResourcePage>(bad.to_string().as_bytes()).is_err());
        }
        assert!("🂡".repeat(512).parse::<SecretReference>().is_ok());
        assert!("🂡".repeat(513).parse::<SecretReference>().is_err());
    }
    #[test]
    fn observation_ack_preserves_unknown_versus_verified_absence_and_original_identity() {
        let original = CleanupObservation {
            account_id: Uuid::new_v4(),
            work_id: Uuid::new_v4(),
            epoch: Amount::from(0),
            outcome: CleanupOutcome::Unknown,
            proof_ref: "fixture:observed-unknown".parse().unwrap(),
            request_id: Uuid::new_v4(),
        };
        let ack: CleanupAck = decode_wire_object(
            json!({"resourceId":original.work_id,"disposition":"unknown"})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        assert!(ack.valid_for(&original));
        let absent = CleanupAck {
            resource_id: original.work_id,
            disposition: CleanupOutcome::Absent,
        };
        assert!(!absent.valid_for(&original));
        let foreign = CleanupAck {
            resource_id: Uuid::new_v4(),
            disposition: CleanupOutcome::Unknown,
        };
        assert!(!foreign.valid_for(&original));
        let duplicate = format!(
            "{{\"resourceId\":\"{}\",\"resourceId\":\"{}\",\"disposition\":\"unknown\"}}",
            original.work_id, original.work_id
        );
        assert!(decode_wire_object::<CleanupAck>(duplicate.as_bytes()).is_err());
        assert!(
            decode_wire_object::<crate::ApiResponse<CleanupAck>>(
                json!({"status":"success","data":[original.work_id,"unknown"]})
                    .to_string()
                    .as_bytes()
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<CleanupOutcome>(&["unknown", "absent"]);
    }
}
