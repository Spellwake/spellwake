//! Closed private initial decision evidence, never proof of native execution alone.
use super::*;
use serde::{Serialize, Serializer, ser::SerializeStruct};
use std::{error::Error, fmt};

#[derive(Deserialize)]
enum Reason {
    #[serde(rename = "native_decision")]
    Decision,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Raw {
    #[serde(rename = "reason")]
    _reason: Reason,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    accepted_revision: Amount,
    proof_ref: SourceProof,
}
/// Initial decision observation bound to a retained original state reference.
/// This private trusted-source object has no Debug/public player projection.
/// A parsed observation still requires comparison with actual initialized state.
#[derive(Deserialize)]
#[serde(try_from = "crate::provider_json::Object<Raw>")]
pub struct OpeningClockObservation {
    seat_id: Uuid,
    proof_ref: SourceProof,
}
/// Safe rejection without disclosing private proof text.
#[derive(Debug)]
pub struct InvalidOpeningClockObservation;
impl fmt::Display for InvalidOpeningClockObservation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid initial clock observation")
    }
}
impl Error for InvalidOpeningClockObservation {}
impl TryFrom<crate::provider_json::Object<Raw>> for OpeningClockObservation {
    type Error = InvalidOpeningClockObservation;
    fn try_from(value: crate::provider_json::Object<Raw>) -> Result<Self, Self::Error> {
        let value = value.0;
        if !value.accepted_revision.is_zero() || value.seat_id.is_nil() {
            return Err(InvalidOpeningClockObservation);
        }
        Ok(Self {
            seat_id: value.seat_id,
            proof_ref: value.proof_ref,
        })
    }
}
impl Serialize for OpeningClockObservation {
    fn serialize<S: Serializer>(&self, output: S) -> Result<S::Ok, S::Error> {
        // Only this closed commit record can encode its proof. SourceProof itself
        // intentionally remains non-serializable and has no Debug representation.
        let mut record = output.serialize_struct("OpeningClockObservation", 4)?;
        record.serialize_field("reason", "native_decision")?;
        record.serialize_field("seatId", &self.seat_id)?;
        record.serialize_field("acceptedRevision", "0")?;
        record.serialize_field("proofRef", self.proof_ref.as_str())?;
        record.end()
    }
}
impl OpeningClockObservation {
    pub(crate) fn from_native(
        seat: Uuid,
        state: &str,
    ) -> Result<Self, InvalidOpeningClockObservation> {
        if seat.is_nil() {
            return Err(InvalidOpeningClockObservation);
        }
        Ok(Self {
            seat_id: seat,
            proof_ref: state.parse().map_err(|_| InvalidOpeningClockObservation)?,
        })
    }
    pub(crate) fn valid_for(&self, state: &str) -> bool {
        self.proof_ref.as_str() == state
    }
    pub(crate) fn matches(&self, seat: Uuid, state: &str) -> bool {
        self.seat_id == seat && self.valid_for(state)
    }
}
/// Missing historical field is permitted; an explicit null is never an object.
pub(crate) fn present<'de, D: Deserializer<'de>>(
    input: D,
) -> Result<Option<OpeningClockObservation>, D::Error> {
    crate::provider_json::object(input).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn strict_initial_clock_record_retains_exact_proof_and_rejects_aliases() {
        let seat = Uuid::from_u128(12);
        let valid = json!({"reason":"native_decision","seatId":seat,"acceptedRevision":"0","proofRef":"fixture:original-state"});
        let parsed: OpeningClockObservation = serde_json::from_value(valid.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), valid);
        assert!(parsed.matches(seat, "fixture:original-state"));
        assert!(!parsed.matches(Uuid::from_u128(13), "fixture:original-state"));
        assert!(!parsed.valid_for("fixture:other-state"));
        for (key, value) in [
            ("reason", json!("native_priority")),
            ("seatId", json!(Uuid::nil())),
            ("acceptedRevision", json!(0)),
            ("acceptedRevision", json!("1")),
            ("proofRef", json!(" ")),
            ("proofRef", Value::Null),
            ("extra", json!(true)),
        ] {
            let mut wrong = valid.clone();
            wrong[key] = value;
            assert!(serde_json::from_value::<OpeningClockObservation>(wrong).is_err());
        }
        for value in [
            Value::Null,
            json!(["native_decision", seat, "0", "fixture:state"]),
            json!({}),
        ] {
            assert!(serde_json::from_value::<OpeningClockObservation>(value).is_err());
        }
        let duplicate = valid.to_string().replacen(
            "\"seatId\":",
            &format!("\"seatId\":\"{seat}\",\"seatId\":"),
            1,
        );
        assert!(serde_json::from_str::<OpeningClockObservation>(&duplicate).is_err());
    }
}
