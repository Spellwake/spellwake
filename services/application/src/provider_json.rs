//! Shared object-only JSON decoding and bounded provider fields.
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor, value::MapAccessDeserializer},
};
use serde_json::Value;
use std::{fmt, marker::PhantomData};

/// Failure fields are a string map with unique names, including equal-valued duplicates.
pub(crate) fn failure_fields<'de, D: Deserializer<'de>>(
    input: D,
) -> Result<std::collections::BTreeMap<String, String>, D::Error> {
    struct Fields;
    impl<'de> Visitor<'de> for Fields {
        type Value = std::collections::BTreeMap<String, String>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("unique string failure fields")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut fields = std::collections::BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                if fields.insert(key, value).is_some() {
                    return Err(de::Error::custom("duplicate failure field"));
                }
            }
            Ok(fields)
        }
    }
    input.deserialize_map(Fields)
}

/// Object-valued success projection; failure and error envelopes retain their own contracts.
pub(crate) struct ObjectResponse<T>(pub crate::ApiResponse<T>);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for ObjectResponse<T> {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        Ok(Self(
            match Object::<crate::ApiResponse<Object<T>>>::deserialize(input)?.0 {
                crate::ApiResponse::Success { data } => {
                    crate::ApiResponse::Success { data: data.0 }
                }
                crate::ApiResponse::Fail { data } => crate::ApiResponse::Fail { data },
                crate::ApiResponse::Error { message, code } => {
                    crate::ApiResponse::Error { message, code }
                }
            },
        ))
    }
}

/// Decode an envelope whose successful data is a record, preserving failure field validation.
pub(crate) fn object_response<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    input: D,
) -> Result<crate::ApiResponse<T>, D::Error> {
    ObjectResponse::<T>::deserialize(input).map(|value| value.0)
}

/// Decode an object without first collecting keys into a `Value`, preserving duplicate rejection.
pub(crate) fn object<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    input: D,
) -> Result<T, D::Error> {
    Object::<T>::deserialize(input).map(|value| value.0)
}

/// Decode a genuine list whose elements must be objects. Domain owners retain their list bounds.
pub(crate) fn objects<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    input: D,
) -> Result<Vec<T>, D::Error> {
    Vec::<Object<T>>::deserialize(input)
        .map(|values| values.into_iter().map(|value| value.0).collect())
}

/// Decode a required nullable object field; omitting the field remains an error.
pub(crate) fn optional_object<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    Option::<Object<T>>::deserialize(input).map(|value| value.map(|object| object.0))
}

/// Decode a required nullable array while requiring every element to be an object.
pub(crate) fn optional_objects<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    input: D,
) -> Result<Option<Vec<T>>, D::Error> {
    Option::<Vec<Object<T>>>::deserialize(input)
        .map(|values| values.map(|items| items.into_iter().map(|value| value.0).collect()))
}

pub(crate) fn string<'de, D: Deserializer<'de>>(input: D) -> Result<String, D::Error> {
    match Value::deserialize(input)? {
        Value::String(value) => Ok(value),
        _ => Err(de::Error::custom("invalid string field")),
    }
}
pub(crate) fn optional_string<'de, D: Deserializer<'de>>(
    input: D,
) -> Result<Option<String>, D::Error> {
    match Value::deserialize(input)? {
        Value::String(value) => Ok(Some(value)),
        Value::Null => Ok(None),
        _ => Err(de::Error::custom("invalid optional string field")),
    }
}
pub(crate) fn boolean<'de, D: Deserializer<'de>>(input: D) -> Result<bool, D::Error> {
    match Value::deserialize(input)? {
        Value::Bool(value) => Ok(value),
        _ => Err(de::Error::custom("invalid boolean field")),
    }
}
pub(crate) fn integer<'de, D: Deserializer<'de>>(input: D) -> Result<i64, D::Error> {
    match Value::deserialize(input)? {
        Value::Number(value) => value
            .as_i64()
            .ok_or_else(|| de::Error::custom("invalid integer field")),
        _ => Err(de::Error::custom("invalid integer field")),
    }
}

pub(crate) fn optional_integer<'de, D: Deserializer<'de>>(
    input: D,
) -> Result<Option<i64>, D::Error> {
    match Value::deserialize(input)? {
        Value::Null => Ok(None),
        Value::Number(n) => n
            .as_i64()
            .map(Some)
            .ok_or_else(|| de::Error::custom("invalid optional integer")),
        _ => Err(de::Error::custom("invalid optional integer")),
    }
}
pub(crate) fn null<'de, D: Deserializer<'de>>(input: D) -> Result<(), D::Error> {
    match Value::deserialize(input)? {
        Value::Null => Ok(()),
        _ => Err(de::Error::custom("unsupported nonnull field")),
    }
}

pub(crate) struct Array<T>(pub Vec<T>);
impl<T> Default for Array<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Array<T> {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        struct OnlyArray<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for OnlyArray<T> {
            type Value = Array<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("source array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? {
                    if values.len() >= 1000 {
                        return Err(de::Error::custom("source array exceeds boundary"));
                    }
                    values.push(value);
                }
                Ok(Array(values))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Err(E::custom("invalid array shape"))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Err(E::custom("invalid array shape"))
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Err(E::custom("invalid array shape"))
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Err(E::custom("invalid array shape"))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Err(E::custom("invalid array shape"))
            }
            fn visit_map<A: MapAccess<'de>>(self, _: A) -> Result<Self::Value, A::Error> {
                Err(de::Error::custom("invalid array shape"))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Err(E::custom("invalid array shape"))
            }
        }
        input.deserialize_any(OnlyArray(PhantomData))
    }
}

pub(crate) struct Object<T>(pub T);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        struct OnlyMap<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for OnlyMap<T> {
            type Value = Object<T>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(Object)
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Err(E::custom("invalid object shape"))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Err(E::custom("invalid object shape"))
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Err(E::custom("invalid object shape"))
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Err(E::custom("invalid object shape"))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Err(E::custom("invalid object shape"))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Err(E::custom("invalid object shape"))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, _: A) -> Result<Self::Value, A::Error> {
                Err(de::Error::custom("invalid object shape"))
            }
        }
        input.deserialize_any(OnlyMap(PhantomData))
    }
}

/// Fingerprint the unchanged provider bytes, independently of signature or JSON normalization.
pub(crate) fn fingerprint(body: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Exact adjacent command carrier. Boxed original payload bytes retain duplicate
/// keys for the existing nested object decoder; the discriminator must be text.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommandObject {
    pub(crate) kind: String,
    command: Box<serde_json::value::RawValue>,
}
impl CommandObject {
    pub(crate) fn payload<T: serde::de::DeserializeOwned, E: de::Error>(&self) -> Result<T, E> {
        crate::decode_wire_object(self.command.get().as_bytes()).map_err(E::custom)
    }
}
