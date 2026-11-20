//! Exact bounded decimal wire values. No JavaScript-number or floating-point path.

use num_bigint::BigUint;
use num_traits::Zero;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, str::FromStr};

/// Application quantities match the database's checked 78-digit whole values, independently of rules integers.
pub const MAX_AMOUNT_DIGITS: usize = 78;

/// A canonical, nonnegative whole quantity serialized only as a decimal string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount(BigUint);

/// Boundary rejection contains no untrusted input or internal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmountError {
    InvalidDecimal,
    TooLarge,
    NegativeResult,
}

impl fmt::Display for AmountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDecimal => "expected a canonical nonnegative integer decimal string",
            Self::TooLarge => "application quantity exceeds its supported precision",
            Self::NegativeResult => "application quantity would become negative",
        })
    }
}

impl std::error::Error for AmountError {}

impl Amount {
    /// Construct a bounded quantity after exact policy arithmetic.
    pub fn from_biguint(value: BigUint) -> Result<Self, AmountError> {
        if value.to_str_radix(10).len() > MAX_AMOUNT_DIGITS {
            return Err(AmountError::TooLarge);
        }
        Ok(Self(value))
    }

    pub(crate) fn integer(&self) -> &BigUint {
        &self.0
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn checked_add(&self, other: &Self) -> Result<Self, AmountError> {
        Self::from_biguint(&self.0 + &other.0)
    }

    pub fn checked_sub(&self, other: &Self) -> Result<Self, AmountError> {
        if self < other {
            return Err(AmountError::NegativeResult);
        }
        Ok(Self(&self.0 - &other.0))
    }
}

impl From<u64> for Amount {
    fn from(value: u64) -> Self {
        Self(value.into())
    }
}

impl FromStr for Amount {
    type Err = AmountError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() > MAX_AMOUNT_DIGITS {
            return Err(AmountError::TooLarge);
        }
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(AmountError::InvalidDecimal);
        }
        BigUint::parse_bytes(value.as_bytes(), 10)
            .map(Self)
            .ok_or(AmountError::InvalidDecimal)
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for Amount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}
