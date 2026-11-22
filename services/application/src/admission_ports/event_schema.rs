//! Closed private codec claim, never evidence of native execution by itself.
use super::*;
use serde::Serialize;

/// Explicit Event codec declared by new native originals and accepted frames.
/// Historical absence is represented separately; present null/objects are invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub enum NativeEventSchema {
    /// Reconstruction-backed accepted transition, linked to its exact State.
    TransitionV1,
    /// Completed physical cast activity with exact retained recomputation.
    ActivityV1,
}
impl NativeEventSchema {
    /// Canonical private wire label. This label grants no acceptance authority.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TransitionV1 => "native-transition-v1",
            Self::ActivityV1 => "native-transition-activity-v1",
        }
    }
}
impl TryFrom<String> for NativeEventSchema {
    type Error = SourceResponseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "native-transition-v1" => Ok(Self::TransitionV1),
            "native-transition-activity-v1" => Ok(Self::ActivityV1),
            _ => Err(SourceResponseError::context()),
        }
    }
}
impl From<NativeEventSchema> for String {
    fn from(value: NativeEventSchema) -> Self {
        value.as_str().to_owned()
    }
}
/// Serde default handles missing; any present value must be the closed string.
pub(crate) fn present<'de, D: Deserializer<'de>>(
    input: D,
) -> Result<Option<NativeEventSchema>, D::Error> {
    NativeEventSchema::deserialize(input).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope {
        #[serde(default, deserialize_with = "present")]
        schema: Option<NativeEventSchema>,
    }
    #[test]
    fn absent_history_differs_from_present_null_object_and_unknown_codec() {
        assert!(
            crate::decode_wire_object::<Envelope>(b"{}")
                .unwrap()
                .schema
                .is_none()
        );
        let supplied =
            crate::decode_wire_object::<Envelope>(br#"{"schema":"native-transition-v1"}"#).unwrap();
        assert_eq!(supplied.schema, Some(NativeEventSchema::TransitionV1));
        for bytes in [
            br#"{"schema":null}"#.as_slice(),
            br#"{"schema":{"native-transition-v1":null}}"#,
            br#"{"schema":"unknown"}"#,
            br#"{"schema":["native-transition-v1"]}"#,
            br#"{"schema":"native-transition-v1","schema":"native-transition-v1"}"#,
        ] {
            assert!(crate::decode_wire_object::<Envelope>(bytes).is_err());
        }
        assert_eq!(
            serde_json::to_string(&NativeEventSchema::TransitionV1).unwrap(),
            "\"native-transition-v1\""
        );
    }
    #[test]
    fn complete_opening_and_candidate_intake_keep_legacy_absence_and_reject_null() {
        use serde_json::{Value, json};
        let mut opening = json!({"contextId":Uuid::from_u128(1),"snapshotRef":"fixture:initial","contextRevision":"fixture:context","stateRef":"fixture:state","factsRef":"fixture:facts","eventRef":"fixture:event","qualificationRef":"fixture:not-native-proof"});
        let mut candidate = json!({"contextId":Uuid::from_u128(1),"stateRef":"fixture:state","factsRef":"fixture:facts","events":["fixture:event"],"facts":[],"clocks":[{"seatId":Uuid::from_u128(2),"clockRevision":"0","remainingMs":"0","paused":true}],"participation":[]});
        assert!(
            OpeningArtifacts::from_bytes(&serde_json::to_vec(&opening).unwrap())
                .unwrap()
                .event_schema_revision()
                .is_none()
        );
        assert!(
            HostCandidate::from_bytes(&serde_json::to_vec(&candidate).unwrap())
                .unwrap()
                .event_schema_revision()
                .is_none()
        );
        for value in [
            json!("native-transition-v1"),
            Value::Null,
            json!({"native-transition-v1":null}),
            json!("unknown"),
        ] {
            opening["eventSchemaRevision"] = value.clone();
            candidate["eventSchemaRevision"] = value.clone();
            let a = OpeningArtifacts::from_bytes(&serde_json::to_vec(&opening).unwrap());
            let b = HostCandidate::from_bytes(&serde_json::to_vec(&candidate).unwrap());
            if value == json!("native-transition-v1") {
                assert_eq!(
                    a.unwrap().event_schema_revision(),
                    Some(NativeEventSchema::TransitionV1)
                );
                assert_eq!(
                    b.unwrap().event_schema_revision(),
                    Some(NativeEventSchema::TransitionV1)
                );
            } else {
                assert!(a.is_err());
                assert!(b.is_err());
            }
        }
    }
}
