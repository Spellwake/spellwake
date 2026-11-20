//! Strict current mana selection codec shared by qualification inputs and views.
//! Raw JSON callers must first reject duplicate keys through the packet parser.
use serde::Deserialize;
use serde_json::{Value, json};
use spellwake_engine::{
    land::LandType,
    mana::{ManaAbilityId, ManaAbilityRef},
};

#[derive(Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum Wire {
    Printed {
        id: u64,
    },
    BasicLand {
        #[serde(rename = "type")]
        land_type: String,
    },
}
/// Decode one current, origin-tagged reference; numeric declaration IDs alone
/// are not valid activation references.
///
/// # Errors
/// Rejects nonobjects, missing/extra fields, invalid numeric IDs, unknown tags,
/// unknown land labels and land types outside the five basic types.
pub fn decode(value: Value) -> Result<ManaAbilityRef, String> {
    if !value.is_object() {
        return Err("expected a tagged mana reference object".into());
    }
    let raw: Wire = serde_json::from_value(value).map_err(|error| error.to_string())?;
    match raw {
        Wire::Printed { id } => Ok(ManaAbilityRef::Printed(ManaAbilityId(id))),
        Wire::BasicLand { land_type } => LandType::from_wire_label(&land_type)
            .and_then(LandType::basic)
            .map(ManaAbilityRef::BasicLand)
            .ok_or_else(|| "unknown basic-land mana reference".into()),
    }
}
/// Serde boundary adapter sharing the exact decoder with untyped command paths.
///
/// # Errors
/// Returns a deserialization error for any shape rejected by `decode`.
pub fn deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<ManaAbilityRef, D::Error> {
    decode(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}
/// Exact current shape. Authored ability definitions retain numeric `id` fields.
pub fn view(value: ManaAbilityRef) -> Value {
    match value {
        ManaAbilityRef::Printed(id) => json!({"tag":"Printed","id":id.0}),
        ManaAbilityRef::BasicLand(kind) => {
            json!({"tag":"BasicLand","type":kind.land_type().wire_label()})
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use spellwake_engine::land::BasicLandType;
    #[test]
    fn current_refs_roundtrip_all_origins_without_reserved_numeric_ids() {
        let values = [0, 1, u64::MAX]
            .into_iter()
            .map(|id| ManaAbilityRef::Printed(ManaAbilityId(id)))
            .chain(BasicLandType::ALL.map(ManaAbilityRef::BasicLand));
        for value in values {
            assert_eq!(decode(view(value)), Ok(value));
        }
        assert_ne!(
            view(ManaAbilityRef::Printed(ManaAbilityId(0))),
            view(ManaAbilityRef::BasicLand(BasicLandType::Plains))
        );
    }
    #[test]
    fn obsolete_scalars_wrong_origins_and_unrecognized_fields_reject() {
        for value in [
            json!(0),
            json!("1"),
            json!(null),
            json!([]),
            json!({}),
            json!({"tag":"Printed","id":"1"}),
            json!({"tag":"Printed","id":-1}),
            json!({"tag":"Printed","id":1.0}),
            json!({"tag":"Printed","type":"Plains"}),
            json!({"tag":"BasicLand","id":1}),
            json!({"tag":"BasicLand","type":"Gate"}),
            json!({"tag":"BasicLand","type":"forest"}),
            json!({"tag":"BasicLand","type":"Urzas"}),
            json!({"tag":"BasicLand","type":"Forest","id":1}),
            json!({"tag":"Other","id":1}),
            json!({"tag":"Printed","id":1,"extra":true}),
        ] {
            assert!(decode(value.clone()).is_err(), "{value}");
        }
    }
}
