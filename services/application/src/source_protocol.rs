//! Shared private source grammar, strict response decoding and safe failure classes.
//! Valid text is not proof that an event happened.
use crate::ApiResponse;
use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Deserializer, de};
use std::{collections::BTreeMap, fmt, str::FromStr};
use uuid::Uuid;

/// Source references retain original text; SQL measures ASCII-space-trimmed characters.
/// This private evidence text intentionally has neither Debug nor Serialize.
pub struct SourceProof(String);
/// Safe envelope rejection without supplied evidence text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSourceProof;
impl fmt::Display for InvalidSourceProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid source proof envelope")
    }
}
impl std::error::Error for InvalidSourceProof {}
impl FromStr for SourceProof {
    type Err = InvalidSourceProof;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let length = value.trim_matches(' ').chars().count();
        if !(1..=2048).contains(&length) || value.contains('\0') {
            return Err(InvalidSourceProof);
        }
        Ok(Self(value.to_owned()))
    }
}
impl SourceProof {
    /// Original trusted source text, never a verified event by itself.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for SourceProof {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        String::deserialize(input)?
            .parse()
            .map_err(de::Error::custom)
    }
}
/// Bare canonical database digest; callers must not substitute another JSON canonicalizer.
pub(crate) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Decode one canonical bare SHA256 value without accepting aliases/normalization.
pub(crate) fn digest_bytes(value: &str) -> Option<[u8; 32]> {
    if !valid_digest(value) {
        return None;
    }
    let mut output = [0; 32];
    for (target, pair) in output.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        let nibble = |byte: u8| {
            if byte.is_ascii_digit() {
                byte - b'0'
            } else {
                byte - b'a' + 10
            }
        };
        *target = (nibble(pair[0]) << 4) | nibble(pair[1]);
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_grammar_matches_trimmed_postgres_characters_without_changing_text() {
        for value in [
            " proof ".to_owned(),
            "\u{00a0}".to_owned(),
            "🂡".repeat(2048),
            format!("{}proof", " ".repeat(3000)),
        ] {
            let proof: SourceProof = value.parse().unwrap();
            assert_eq!(proof.as_str(), value);
        }
        for value in [
            "".to_owned(),
            "   ".to_owned(),
            "x\0y".to_owned(),
            "🂡".repeat(2049),
        ] {
            assert!(value.parse::<SourceProof>().is_err());
        }
        assert!(valid_digest(&"a".repeat(64)));
        assert!(!valid_digest(&"A".repeat(64)));
        assert!(!valid_digest(&format!("sha256:{}", "a".repeat(64))));
    }
}

/// One private secret-store generation, with the database's UTF8 byte bound.
/// This is a locator, never the secret value or proof of its presence/absence.
pub struct SecretReference(String);
/// Safe rejection without the supplied private locator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSecretReference;
impl fmt::Display for InvalidSecretReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid secret reference envelope")
    }
}
impl std::error::Error for InvalidSecretReference {}
impl FromStr for SecretReference {
    type Err = InvalidSecretReference;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.trim_matches(' ').is_empty() || value.len() > 2048 || value.contains('\0') {
            return Err(InvalidSecretReference);
        }
        Ok(Self(value.to_owned()))
    }
}
impl SecretReference {
    /// Original locator for a trusted secret-store adapter, never public diagnostics.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for SecretReference {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        String::deserialize(input)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Internal source failure classification, never raw SQL diagnostics on a public boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFailure {
    Retry,
    MissingSource,
    InvalidEnvelope,
    InvalidSource,
    Unavailable,
}
/// Classify only known canonical SQLSTATEs. Unknown failures remain unavailable;
/// this does not turn a SQL exception into a stored business rejection.
pub fn classify_sqlstate(code: &str) -> SourceFailure {
    match code {
        "40001" => SourceFailure::Retry,
        "23503" => SourceFailure::MissingSource,
        "22023" => SourceFailure::InvalidEnvelope,
        "23514" => SourceFailure::InvalidSource,
        _ => SourceFailure::Unavailable,
    }
}

/// Safe decode/context failure with the underlying syntax cause retained privately.
pub struct SourceResponseError {
    source: Option<serde_json::Error>,
}
impl SourceResponseError {
    pub(crate) fn context() -> Self {
        Self { source: None }
    }
}
impl From<serde_json::Error> for SourceResponseError {
    fn from(source: serde_json::Error) -> Self {
        Self {
            source: Some(source),
        }
    }
}
impl std::fmt::Display for SourceResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("normalized source response is unavailable")
    }
}
impl std::fmt::Debug for SourceResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for SourceResponseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}
pub(crate) fn record_response<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<ApiResponse<T>, SourceResponseError> {
    Ok(crate::decode_wire_object::<crate::provider_json::ObjectResponse<T>>(bytes)?.0)
}
pub(crate) fn accepted_response<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, SourceResponseError> {
    match record_response(bytes)? {
        ApiResponse::Success { data } => Ok(data),
        _ => Err(SourceResponseError::context()),
    }
}

pub(crate) fn one_failure(errors: &BTreeMap<String, String>, allowed: &[(&str, &str)]) -> bool {
    errors.len() == 1
        && allowed
            .iter()
            .any(|(key, message)| errors.get(*key).is_some_and(|value| value == message))
}

// These original terms are flat objects of strings/nulls. PostgreSQL JSONB
// adds one space after each colon and comma. Key ordering cannot change length.
// This counts bytes only, never computes a replacement canonical source hash.
pub(crate) fn flat_jsonb_octets(terms: &serde_json::Value) -> Option<usize> {
    let fields = terms.as_object()?;
    if fields
        .values()
        .any(|value| !value.is_string() && !value.is_null())
    {
        return None;
    }
    source_jsonb_octets(terms)
}

/// PostgreSQL JSONB text size for closed source terms containing strings,
/// booleans, nulls and nested records/lists. Quantities are canonical strings;
/// numbers are deliberately unsupported. This never computes a source hash.
pub(crate) fn source_jsonb_octets(value: &serde_json::Value) -> Option<usize> {
    use serde_json::Value;
    match value {
        Value::String(text) => (!text.contains('\0'))
            .then(|| serde_json::to_vec(text).ok().map(|bytes| bytes.len()))
            .flatten(),
        Value::Bool(value) => Some(if *value { 4 } else { 5 }),
        Value::Null => Some(4),
        Value::Number(_) => None,
        Value::Array(values) => {
            let separators = values.len().saturating_sub(1).checked_mul(2)?;
            values
                .iter()
                .try_fold(2usize.checked_add(separators)?, |size, child| {
                    size.checked_add(source_jsonb_octets(child)?)
                })
        }
        Value::Object(fields) => {
            let separators = fields.len().saturating_sub(1).checked_mul(2)?;
            fields
                .iter()
                .try_fold(2usize.checked_add(separators)?, |size, (key, child)| {
                    if key.contains('\0') {
                        return None;
                    }
                    size.checked_add(serde_json::to_vec(key).ok()?.len())?
                        .checked_add(2)?
                        .checked_add(source_jsonb_octets(child)?)
                })
        }
    }
}

/// Validate the concrete closed wire type before accounting original JSONB
/// terms. Duplicate keys must fail in that type before Value can collect them.
/// RawValue preserves the token/inner spacing, excluding outer document whitespace.
pub(crate) fn original_source_terms<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    maximum: usize,
) -> Result<(T, Box<serde_json::value::RawValue>), SourceResponseError> {
    let typed = serde_json::from_slice(bytes)?;
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    if !source_jsonb_octets(&value).is_some_and(|size| size <= maximum) {
        return Err(SourceResponseError::context());
    }
    Ok((typed, serde_json::from_slice(bytes)?))
}
pub(crate) fn original_source_octets(raw: &serde_json::value::RawValue) -> Option<usize> {
    source_jsonb_octets(&serde_json::from_str::<serde_json::Value>(raw.get()).ok()?)
}
pub(crate) fn ids(values: &[Uuid]) -> bool {
    values.iter().all(|id| !id.is_nil())
}
pub(crate) fn checked<T>(valid: bool, value: T) -> Result<T, SourceResponseError> {
    if valid {
        Ok(value)
    } else {
        Err(SourceResponseError::context())
    }
}

// Qualified adapter subset, not the whole PostgreSQL timestamptz language.
// UTC, four-digit AD years, exact microseconds and no leap-second aliases make
// this text identical to source191's to_char format. SQL still owns its hash.
pub(crate) fn utc_microsecond_text(value: DateTime<Utc>) -> Option<String> {
    ((1..=9999).contains(&value.year())
        && value.nanosecond() < 1_000_000_000
        && value.nanosecond().is_multiple_of(1000))
    .then(|| value.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string())
}

/// Parse the exact source financial_instant UTC grammar without rewriting text.
pub(crate) fn parse_utc_microsecond(text: &str) -> Option<DateTime<Utc>> {
    let instant: DateTime<Utc> = text.parse().ok()?;
    (utc_microsecond_text(instant).as_deref() == Some(text)).then_some(instant)
}
