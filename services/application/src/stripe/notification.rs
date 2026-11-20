//! Closed canonical Stripe notification acknowledgments. These projections
//! detect malformed database responses; they do not qualify provider truth.
use serde::Deserialize;
use uuid::Uuid;

/// Bare association acknowledgment from associate_stripe_notification.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssociatedAck {
    associated: bool,
}
impl AssociatedAck {
    /// Only the canonical positive literal acknowledges association.
    pub fn confirmed(&self) -> bool {
        self.associated
    }
}
/// Bare unsupported-notification acknowledgment from ignore_stripe_notification.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IgnoredAck {
    ignored: bool,
}
impl IgnoredAck {
    /// Ignoring an unsupported notification is distinct from observing payment.
    pub fn confirmed(&self) -> bool {
        self.ignored
    }
}
/// The exact stored native effect returned on first completion or historical replay.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompletionAck {
    /// Notification reconciliation refers to a processed private API observation.
    #[serde(rename_all = "camelCase")]
    Observed {
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        observation_id: Uuid,
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        purchase_id: Uuid,
    },
    /// Previously ignored native notification, not a new financial observation.
    Unsupported {},
}
impl CompletionAck {
    /// Bind the known purchase and, when independently retained, original accepted
    /// observation. A newly reconciled retry observation is not that original ID.
    pub fn matches_observed(&self, purchase: Uuid, original_observation: Option<Uuid>) -> bool {
        match self {
            Self::Observed {
                observation_id,
                purchase_id,
            } => {
                !purchase.is_nil()
                    && !observation_id.is_nil()
                    && *purchase_id == purchase
                    && original_observation.is_none_or(|original| original == *observation_id)
            }
            Self::Unsupported {} => false,
        }
    }
}

/// Require the immutable capture hash's exact storage locator before deletion.
/// This is source identity validation, not a storage absence proof.
pub fn evidence_locator_matches(payload_ref: &str, body_hash: &str) -> bool {
    crate::source_protocol::valid_digest(body_hash)
        && payload_ref == format!("payments/stripe/{body_hash}.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_effect_is_an_exact_empty_object_variant() {
        assert!(matches!(
            crate::decode_wire_object::<CompletionAck>(br#"{"kind":"unsupported"}"#).unwrap(),
            CompletionAck::Unsupported {}
        ));
        for bytes in [
            br#"{"kind":"unsupported","extra":true}"#.as_slice(),
            br#"{"kind":"unsupported","purchaseId":null}"#.as_slice(),
            br#"{"kind":"unsupported","kind":"unsupported"}"#.as_slice(),
            br#"{"kind":{"unsupported":null}}"#.as_slice(),
        ] {
            assert!(crate::decode_wire_object::<CompletionAck>(bytes).is_err());
        }
    }
}
