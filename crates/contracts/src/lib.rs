//! Authoritative native contract primitives. Full game wire contracts are not implemented.
use std::{fmt, str::FromStr};

use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// Standard response envelope shared by HTTP and command-line consumers.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
pub enum ApiResponse<T, F = std::collections::BTreeMap<String, String>> {
    Success { data: T },
    Fail { data: F },
    Error { message: String, code: String },
}

/// Exact signed rules quantity, encoded as a canonical decimal JSON string.
/// This is not a timestamp, revision or coin balance.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RulesInteger(BigInt);

/// Invalid canonical decimal representation at a contract boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidRulesInteger;

impl fmt::Display for InvalidRulesInteger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected a canonical decimal integer string")
    }
}
impl std::error::Error for InvalidRulesInteger {}

impl TryFrom<String> for RulesInteger {
    type Error = InvalidRulesInteger;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let parsed = BigInt::from_str(&value).map_err(|_| InvalidRulesInteger)?;
        if parsed.to_string() != value {
            return Err(InvalidRulesInteger);
        }
        Ok(Self(parsed))
    }
}
impl From<RulesInteger> for String {
    fn from(value: RulesInteger) -> Self {
        value.0.to_string()
    }
}
impl From<i64> for RulesInteger {
    fn from(value: i64) -> Self {
        Self(BigInt::from(value))
    }
}
impl RulesInteger {
    /// Add without machine-integer overflow or floating-point rounding.
    pub fn plus(&self, other: &Self) -> Self {
        Self(&self.0 + &other.0)
    }

    /// Subtract exact quantities without narrowing either operand.
    pub fn minus(&self, other: &Self) -> Self {
        Self(&self.0 - &other.0)
    }

    /// Multiply exact signed quantities without narrowing either operand.
    pub fn times(&self, other: &Self) -> Self {
        Self(&self.0 * &other.0)
    }

    pub fn is_positive(&self) -> bool {
        self.0 > BigInt::from(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_numbers_far_beyond_javascript_precision() {
        let wire = "\"900719925474099312345678901234567890\"";
        let number: RulesInteger = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&number).unwrap(), wire);
        let incremented = number.plus(&RulesInteger::from(1));
        assert_eq!(
            String::from(incremented),
            "900719925474099312345678901234567891"
        );
    }

    #[test]
    fn rejects_numbers_and_noncanonical_or_fractional_strings() {
        for wire in [
            "3", "null", "\"03\"", "\"+3\"", "\"-0\"", "\" 3\"", "\"3.0\"", "\"1e3\"", "\"\"",
            "\"٣\"",
        ] {
            assert!(
                serde_json::from_str::<RulesInteger>(wire).is_err(),
                "{wire}"
            );
        }
        assert_eq!(
            serde_json::from_str::<RulesInteger>("\"-123\"").unwrap(),
            RulesInteger::from(-123)
        );
    }

    #[test]
    fn multiplies_large_signed_quantities_exactly() {
        let huge: RulesInteger = "900719925474099312345678901234567890"
            .to_owned()
            .try_into()
            .unwrap();
        assert_eq!(
            String::from(huge.times(&(-3).into())),
            "-2702159776422297937037036703703703670"
        );
        assert_eq!(huge.times(&0.into()), 0.into());
        assert_eq!(
            RulesInteger::from(-3),
            RulesInteger::from(-1).times(&3.into())
        );
    }

    #[test]
    fn serializes_standard_envelope_variants() {
        let success: ApiResponse<RulesInteger> = ApiResponse::Success { data: 3.into() };
        assert_eq!(
            serde_json::to_string(&success).unwrap(),
            r#"{"status":"success","data":"3"}"#
        );
        let fail: ApiResponse<()> = ApiResponse::Fail {
            data: std::collections::BTreeMap::from([("choice".into(), "invalid-choice".into())]),
        };
        assert_eq!(serde_json::to_value(fail).unwrap()["status"], "fail");
        let error: ApiResponse<()> = ApiResponse::Error {
            message: "Unavailable".into(),
            code: "unavailable".into(),
        };
        assert_eq!(serde_json::to_value(error).unwrap()["status"], "error");
    }
}
