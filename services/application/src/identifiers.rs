//! Canonical public application identifiers; provider-owned text uses its own protocol.
use serde::{Deserialize, Deserializer, de};
use std::{error::Error, fmt};
use uuid::Uuid;

pub(crate) enum InvalidIdentifier {
    Parse(uuid::Error),
    NonCanonical,
    Nil,
}
impl fmt::Display for InvalidIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid canonical identifier")
    }
}
impl fmt::Debug for InvalidIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for InvalidIdentifier {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(source) => Some(source),
            _ => None,
        }
    }
}
/// Preserve every Postgres UUID value, without adding RFC version/variant restrictions.
/// # Errors
/// Rejects nil and every spelling other than lowercase dashed hexadecimal.
pub(crate) fn parse(text: &str) -> Result<Uuid, InvalidIdentifier> {
    let id = Uuid::parse_str(text).map_err(InvalidIdentifier::Parse)?;
    if id.is_nil() {
        return Err(InvalidIdentifier::Nil);
    }
    if id.to_string() != text {
        return Err(InvalidIdentifier::NonCanonical);
    }
    Ok(id)
}
pub(crate) fn uuid<'de, D: Deserializer<'de>>(input: D) -> Result<Uuid, D::Error> {
    parse(&String::deserialize(input)?).map_err(de::Error::custom)
}
/// Required legacy SQL192 string sentinel: empty means no reserved successor.
/// This does not accept JSON null or change the canonical stored protocol.
pub(crate) fn empty_uuid<'de, D: Deserializer<'de>>(input: D) -> Result<Option<Uuid>, D::Error> {
    let value = String::deserialize(input)?;
    if value.is_empty() {
        Ok(None)
    } else {
        parse(&value).map(Some).map_err(de::Error::custom)
    }
}
/// An optional field that, when present, must hold a UUID: omission is the only
/// way to say "none", so an explicit JSON null is rejected.
pub(crate) fn present_uuid<'de, D: Deserializer<'de>>(input: D) -> Result<Option<Uuid>, D::Error> {
    uuid(input).map(Some)
}
/// The field attribute, not this adapter, controls whether omission is permitted.
pub(crate) fn nullable_uuid<'de, D: Deserializer<'de>>(input: D) -> Result<Option<Uuid>, D::Error> {
    Option::<String>::deserialize(input)?
        .as_deref()
        .map(parse)
        .transpose()
        .map_err(de::Error::custom)
}
pub(crate) fn uuid_list<'de, D: Deserializer<'de>>(input: D) -> Result<Vec<Uuid>, D::Error> {
    Vec::<String>::deserialize(input)?
        .iter()
        .map(|text| parse(text).map_err(de::Error::custom))
        .collect()
}
/// Required-nullable UUID vectors preserve historical unqualified result absence.
pub(crate) fn nullable_uuid_list<'de, D: Deserializer<'de>>(
    input: D,
) -> Result<Option<Vec<Uuid>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(transparent)]
    struct List(#[serde(deserialize_with = "uuid_list")] Vec<Uuid>);
    Option::<List>::deserialize(input).map(|value| value.map(|list| list.0))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn canonical_values_retain_all_uuid_bits_and_aliases_never_normalize() {
        for text in [
            "11111111-1111-1111-1111-111111111111",
            "aaaaaaaa-aaaa-faaa-aaaa-aaaaaaaaaaaa",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
        ] {
            assert_eq!(parse(text).unwrap().to_string(), text);
        }
        for text in [
            "00000000-0000-0000-0000-000000000000",
            "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
            "11111111111141118111111111111111",
            "{11111111-1111-4111-8111-111111111111}",
            "urn:uuid:11111111-1111-4111-8111-111111111111",
            " 11111111-1111-4111-8111-111111111111",
            "private-sentinel",
        ] {
            let error = parse(text).unwrap_err();
            assert!(!format!("{error:?} {error}").contains(text));
        }
        assert!(parse("private-sentinel").unwrap_err().source().is_some());
    }
    #[derive(Deserialize)]
    struct Required {
        #[serde(deserialize_with = "super::nullable_uuid")]
        id: Option<Uuid>,
    }
    #[derive(Deserialize)]
    struct Optional {
        #[serde(default, deserialize_with = "super::nullable_uuid")]
        id: Option<Uuid>,
    }
    #[derive(Deserialize)]
    struct List {
        #[serde(deserialize_with = "super::uuid_list")]
        ids: Vec<Uuid>,
    }
    #[test]
    fn required_null_optional_omission_and_array_elements_are_distinct() {
        assert!(serde_json::from_value::<Required>(json!({})).is_err());
        assert!(
            serde_json::from_value::<Required>(json!({"id":null}))
                .unwrap()
                .id
                .is_none()
        );
        assert!(
            serde_json::from_value::<Optional>(json!({}))
                .unwrap()
                .id
                .is_none()
        );
        let id = "11111111-1111-1111-1111-111111111111";
        assert_eq!(
            serde_json::from_value::<List>(json!({"ids":[id]}))
                .unwrap()
                .ids[0]
                .to_string(),
            id
        );
        assert!(
            serde_json::from_value::<List>(
                json!({"ids":[id,"{11111111-1111-4111-8111-111111111111}"]})
            )
            .is_err()
        );
    }
}
