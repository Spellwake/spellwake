//! Maintained native-facing public wire primitives. HTTP delivery lives in api;
//! syntax alone does not qualify native source, visibility or participant authority.
use serde::{Deserialize, Serialize};
pub use spellwake_contracts::RulesInteger;
use std::fmt;
use uuid::Uuid;
mod actions;
mod decisions;
mod history;
mod history_page;
mod receipts;
mod reconnect;
mod replay;
mod stream;
mod views;
mod vocabulary;
pub use actions::*;
pub use decisions::*;
pub use history::*;
pub use history_page::*;
pub use receipts::*;
pub use reconnect::*;
pub use replay::*;
pub use stream::*;
pub use views::*;
pub use vocabulary::*;
/// Safe syntax/context rejection, without private handles or native diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPlayerWire;
impl fmt::Display for InvalidPlayerWire {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid player wire contract")
    }
}
impl std::error::Error for InvalidPlayerWire {}
/// Native basis counter, distinct from application Amount and arbitrary rules values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NativeRevision(u64);
impl TryFrom<String> for NativeRevision {
    type Error = InvalidPlayerWire;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        let value = text.parse::<u64>().map_err(|_| InvalidPlayerWire)?;
        if value.to_string() != text {
            return Err(InvalidPlayerWire);
        }
        Ok(Self(value))
    }
}
impl From<NativeRevision> for String {
    fn from(value: NativeRevision) -> String {
        value.0.to_string()
    }
}
impl From<u64> for NativeRevision {
    fn from(value: u64) -> Self {
        Self(value)
    }
}
impl NativeRevision {
    /// Checked native scalar, never a database host-ownership fence by itself.
    pub fn get(self) -> u64 {
        self.0
    }
}
/// Full public expected basis; trusted adapter resolves the complete native context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerBasis {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub context_id: Uuid,
    pub epoch: NativeRevision,
    pub revision: NativeRevision,
}
impl PlayerBasis {
    /// Compare revisions only within the same game, context and native epoch.
    /// Different epochs are incomparable here; their lineage requires the source adapter.
    pub fn compare_revision_in_epoch(&self, other: &Self) -> Option<std::cmp::Ordering> {
        (self.game_id == other.game_id
            && self.context_id == other.context_id
            && self.epoch == other.epoch)
            .then(|| self.revision.cmp(&other.revision))
    }
}
/// Opaque issued reference. Syntax does not establish a viewer, choice or authority.
/// Deliberately no Debug: diagnostics must not collect basis-scoped capability text.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlayerHandle(String);
impl TryFrom<String> for PlayerHandle {
    type Error = InvalidPlayerWire;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.len() > 256 || !value.bytes().all(|b| (33..=126).contains(&b))
        {
            return Err(InvalidPlayerWire);
        }
        Ok(Self(value))
    }
}
impl From<PlayerHandle> for String {
    fn from(value: PlayerHandle) -> String {
        value.0
    }
}
impl PlayerHandle {
    /// Original issued token for the future trusted resolver, never a native ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// Arbitrary precision nonnegative rules quantity. No economy78-digit restriction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RulesCount(RulesInteger);
impl TryFrom<String> for RulesCount {
    type Error = InvalidPlayerWire;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        if text.starts_with('-') {
            return Err(InvalidPlayerWire);
        }
        RulesInteger::try_from(text)
            .map(Self)
            .map_err(|_| InvalidPlayerWire)
    }
}
impl From<RulesCount> for String {
    fn from(value: RulesCount) -> String {
        value.0.into()
    }
}
impl RulesCount {
    /// Native arbitrary precision value, without narrowing through Amount or f64.
    pub fn as_rules_integer(&self) -> &RulesInteger {
        &self.0
    }
}
/// Established public command-envelope byte bound. It is not a rules-integer cap.
pub const MAX_PLAYER_WIRE_BYTES: usize = 262_144;
/// Safe original-byte rejection, retaining a private syntax cause where present.
pub struct PlayerDecodeError {
    cause: Option<serde_json::Error>,
}
impl fmt::Display for PlayerDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid original player wire")
    }
}
impl fmt::Debug for PlayerDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for PlayerDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause
            .as_ref()
            .map(|x| x as &(dyn std::error::Error + 'static))
    }
}
/// Decode an original bounded record before typed/contextual semantic validation.
/// Does not authenticate a player or resolve a handle, and never normalizes JSON.
/// # Errors
/// Rejects over-budget bodies and malformed/duplicate/array-shaped record input.
pub fn decode_player_record<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, PlayerDecodeError> {
    if bytes.len() > MAX_PLAYER_WIRE_BYTES {
        return Err(PlayerDecodeError { cause: None });
    }
    crate::decode_wire_object(bytes).map_err(|e| PlayerDecodeError { cause: Some(e) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_and_rules_numeric_domains_stay_distinct() {
        for value in ["-1".to_owned(), "9".repeat(150)] {
            assert!(
                serde_json::from_str::<RulesInteger>(&serde_json::to_string(&value).unwrap())
                    .is_ok()
            );
            assert!(
                serde_json::from_str::<NativeRevision>(&serde_json::to_string(&value).unwrap())
                    .is_err()
            );
        }
        assert!(RulesCount::try_from("9".repeat(150)).is_ok());
        assert!(RulesCount::try_from("-1".to_owned()).is_err());
        for value in ["-0", "+1", "01", "1.0", "1e2", " 1", "1\n"] {
            assert!(RulesInteger::try_from(value.to_owned()).is_err());
            assert!(NativeRevision::try_from(value.to_owned()).is_err());
        }
        assert_eq!(
            NativeRevision::try_from(u64::MAX.to_string())
                .unwrap()
                .get(),
            u64::MAX
        );
        assert!(NativeRevision::try_from("18446744073709551616".to_owned()).is_err());
        assert!(serde_json::from_str::<NativeRevision>("0").is_err());
    }
    #[test]
    fn handle_syntax_does_not_normalize_or_establish_native_authority() {
        for value in ["opaque-object_1", "not-a-uuid:option"] {
            let handle = PlayerHandle::try_from(value.to_owned()).unwrap();
            assert_eq!(handle.as_str(), value);
        }
        for value in ["", " token", "token\n", "🂡"] {
            assert!(PlayerHandle::try_from(value.to_owned()).is_err());
        }
        assert!(serde_json::from_str::<PlayerHandle>("{\"opaque\":null}").is_err());
    }
}
