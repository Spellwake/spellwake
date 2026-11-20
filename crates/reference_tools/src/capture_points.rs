//! Temporary qualification labels attached at actual scoped writers.
//! Binding walks the emitted structure's exact keys and indices, then removes labels.
use super::{current_capture, current_profile::SnapshotOwner, packet::AliasContext};
use serde_json::Value;
const MARKER: &str = "__current_capture_point";
pub fn token(
    owner_source: &str,
    owner: SnapshotOwner<'_>,
    aliases: &AliasContext,
) -> Result<Option<String>, String> {
    if current_capture::is_active() {
        current_capture::observation(owner_source, owner, aliases).map(Some)
    } else {
        Ok(None)
    }
}
pub fn mark_value(
    owner_source: &str,
    owner: SnapshotOwner<'_>,
    aliases: &AliasContext,
    mut value: Value,
) -> Result<Value, String> {
    if let Some(point) = token(owner_source, owner, aliases)? {
        let object = value
            .as_object_mut()
            .ok_or("scoped qualification writer requires an object point")?;
        if object.insert(MARKER.into(), Value::String(point)).is_some() {
            return Err("duplicate scoped qualification marker".into());
        }
    }
    Ok(value)
}
pub fn bind_value_points(value: &mut Value, prefix: &str) -> Result<(), String> {
    match value {
        Value::Object(object) => {
            if let Some(point) = object.remove(MARKER) {
                match point {
                    Value::String(point) => current_capture::bind_observation(&point, prefix)?,
                    Value::Array(points) => {
                        if points.is_empty() {
                            return Err("empty scoped qualification point vector".into());
                        }
                        let mut seen = std::collections::BTreeSet::new();
                        for point in points {
                            let point = point
                                .as_str()
                                .ok_or("invalid scoped qualification marker member")?;
                            if !seen.insert(point.to_owned()) {
                                return Err("duplicate scoped qualification point token".into());
                            }
                            current_capture::bind_observation(point, prefix)?;
                        }
                    }
                    _ => return Err("invalid scoped qualification marker".into()),
                }
            }
            for (key, value) in object {
                let key = key.replace('~', "~0").replace('/', "~1");
                bind_value_points(value, &format!("{prefix}/{key}"))?;
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                bind_value_points(value, &format!("{prefix}/{index}"))?;
            }
        }
        _ => {}
    }
    Ok(())
}
