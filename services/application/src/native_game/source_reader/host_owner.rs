//! Explicit trusted process configuration, never derived from an observed owner.
use super::*;
/// Server owner identity used to select owned native history. Not serializable,
/// not player authority, and not a lease acquisition or fencing operation.
#[derive(Clone)]
pub struct NativeHostOwner(SourceProof);
impl NativeHostOwner {
    /// Validate the existing SQL owner grammar without rewriting its identity.
    /// # Errors
    /// Rejects blank/NUL or over-256-character owner references.
    pub fn new(value: &str) -> Result<Self, NativeSourceError> {
        let proof: SourceProof = value.parse().map_err(|_| invalid())?;
        if value.trim_matches(' ').chars().count() > 256 {
            return Err(invalid());
        }
        Ok(Self(proof))
    }
    pub(super) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_owner_preserves_identity_and_sql_bounds() {
        assert_eq!(
            NativeHostOwner::new("fixture:owner").unwrap().as_str(),
            "fixture:owner"
        );
        for value in [
            "".to_owned(),
            " ".to_owned(),
            "owner\0secret".to_owned(),
            "x".repeat(257),
        ] {
            assert!(NativeHostOwner::new(&value).is_err());
        }
    }
}
