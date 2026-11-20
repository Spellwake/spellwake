//! Shared dynamic pack pricing and exact rough-value restrictions.

use crate::amount::{Amount, AmountError};
use num_bigint::BigUint;
use num_traits::{One, Zero};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{cmp::Ordering, fmt};

/// Work bound on sequential pack-price evaluation, not a currency precision bound.
pub const MAX_PACKS_PER_PURCHASE: u32 = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    ZeroDenominator,
    InvalidWindow,
    InvalidModifier,
    InvalidRecipe,
    UnavailableQuantity,
    Amount(AmountError),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroDenominator => "rational denominator must be positive",
            Self::InvalidWindow => "invalid product pricing window",
            Self::InvalidModifier => "product modifier must be positive",
            Self::InvalidRecipe => "recipe candidate count and expected copies must be positive",
            Self::UnavailableQuantity => {
                "requested pack quantity is unavailable or exceeds request bounds"
            }
            Self::Amount(_) => "policy result exceeds supported application quantity precision",
        })
    }
}

impl std::error::Error for PolicyError {}

impl From<AmountError> for PolicyError {
    fn from(error: AmountError) -> Self {
        Self::Amount(error)
    }
}

/// Normalized nonnegative rational; intermediate arithmetic does not round card values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rational {
    numerator: BigUint,
    denominator: BigUint,
}

/// Exact valuation fractions have a larger intermediate bound than asset amounts.
/// Both components use canonical reduced decimal strings, matching Postgres.
pub const MAX_RATIONAL_DIGITS: usize = 8192;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RationalWire {
    numerator: String,
    denominator: String,
}

impl Serialize for Rational {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = RationalWire {
            numerator: self.numerator.to_str_radix(10),
            denominator: self.denominator.to_str_radix(10),
        };
        if wire.numerator.len() > MAX_RATIONAL_DIGITS
            || wire.denominator.len() > MAX_RATIONAL_DIGITS
        {
            return Err(serde::ser::Error::custom(
                "exact valuation exceeds wire precision",
            ));
        }
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Rational {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire: RationalWire = crate::provider_json::object(deserializer)?;
        let parse = |text: &str| -> Result<BigUint, D::Error> {
            if text.is_empty()
                || text.len() > MAX_RATIONAL_DIGITS
                || (text.len() > 1 && text.starts_with('0'))
                || !text.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(de::Error::custom(
                    "expected exact canonical valuation component",
                ));
            }
            BigUint::parse_bytes(text.as_bytes(), 10)
                .ok_or_else(|| de::Error::custom("invalid valuation component"))
        };
        let numerator = parse(&wire.numerator)?;
        let denominator = parse(&wire.denominator)?;
        let value =
            Self::normalized(numerator.clone(), denominator.clone()).map_err(de::Error::custom)?;
        if value.numerator != numerator || value.denominator != denominator {
            return Err(de::Error::custom("valuation fraction must be reduced"));
        }
        Ok(value)
    }
}

impl Rational {
    pub fn new(numerator: Amount, denominator: Amount) -> Result<Self, PolicyError> {
        Self::normalized(numerator.integer().clone(), denominator.integer().clone())
    }

    fn normalized(numerator: BigUint, denominator: BigUint) -> Result<Self, PolicyError> {
        if denominator.is_zero() {
            return Err(PolicyError::ZeroDenominator);
        }
        let (mut left, mut right) = (numerator.clone(), denominator.clone());
        while !right.is_zero() {
            (left, right) = (right.clone(), left % right);
        }
        Ok(Self {
            numerator: numerator / &left,
            denominator: denominator / left,
        })
    }

    pub fn whole(value: Amount) -> Self {
        Self {
            numerator: value.integer().clone(),
            denominator: BigUint::one(),
        }
    }

    pub fn add(&self, other: &Self) -> Self {
        // Both denominators are positive by construction.
        let numerator = &self.numerator * &other.denominator + &other.numerator * &self.denominator;
        let denominator = &self.denominator * &other.denominator;
        Self::normalize_positive(numerator, denominator)
    }

    pub fn multiply(&self, other: &Self) -> Self {
        Self::normalize_positive(
            &self.numerator * &other.numerator,
            &self.denominator * &other.denominator,
        )
    }

    fn normalize_positive(numerator: BigUint, denominator: BigUint) -> Self {
        let (mut left, mut right) = (numerator.clone(), denominator.clone());
        while !right.is_zero() {
            (left, right) = (right.clone(), left % right);
        }
        Self {
            numerator: numerator / &left,
            denominator: denominator / left,
        }
    }

    pub fn divide(&self, other: &Self) -> Result<Self, PolicyError> {
        Self::normalized(
            &self.numerator * &other.denominator,
            &self.denominator * &other.numerator,
        )
    }

    pub fn ceil(&self) -> Result<Amount, PolicyError> {
        Ok(Amount::from_biguint(
            (&self.numerator + &self.denominator - BigUint::one()) / &self.denominator,
        )?)
    }

    pub fn compare(&self, other: &Self) -> Ordering {
        (&self.numerator * &other.denominator).cmp(&(&other.numerator * &self.denominator))
    }
}

/// Inputs are pinned to an immutable policy and current aggregate stock revision by persistence.
#[derive(Debug, Clone)]
pub struct PricingWindow {
    starting: Amount,
    remaining: Amount,
    normal: Amount,
    modifier: Rational,
}

impl PricingWindow {
    pub fn new(
        starting: Amount,
        remaining: Amount,
        normal: Amount,
        modifier: Rational,
    ) -> Result<Self, PolicyError> {
        if starting.is_zero() || normal.is_zero() || remaining > starting {
            return Err(PolicyError::InvalidWindow);
        }
        if modifier.numerator.is_zero() {
            return Err(PolicyError::InvalidModifier);
        }
        Ok(Self {
            starting,
            remaining,
            normal,
            modifier,
        })
    }

    /// At sold-out stock this is an explicit valuation-only price, never a purchase permission.
    pub fn valuation_quote(&self) -> Result<Amount, PolicyError> {
        self.quote_at(&self.remaining)
    }

    fn quote_at(&self, remaining: &Amount) -> Result<Amount, PolicyError> {
        let scarcity = Rational::new(self.normal.clone(), self.starting.clone())?;
        let scarcity = if scarcity.compare(&Rational::whole(1.into())) == Ordering::Less {
            Rational::whole(1.into())
        } else {
            scarcity
        };
        let depletion =
            Rational::new(self.starting.checked_sub(remaining)?, self.starting.clone())?;
        Rational::whole(1000.into())
            .multiply(&self.modifier)
            .multiply(&scarcity)
            .multiply(&Rational::whole(1.into()).add(&depletion))
            .ceil()
    }

    /// Each unit uses the next stock level; bulk purchase cannot bypass depletion pricing.
    pub fn purchase_total(&self, quantity: u32) -> Result<Amount, PolicyError> {
        if quantity == 0
            || quantity > MAX_PACKS_PER_PURCHASE
            || Amount::from(u64::from(quantity)) > self.remaining
        {
            return Err(PolicyError::UnavailableQuantity);
        }
        let mut total = Amount::from(0);
        for index in 0..quantity {
            total = total.checked_add(
                &self.quote_at(&self.remaining.checked_sub(&u64::from(index).into())?)?,
            )?;
        }
        Ok(total)
    }
}

/// Allocate P/(N*expected copies), keeping exact fractions for whole-bundle restrictions.
pub fn card_value(
    pack_price: Amount,
    candidates: Amount,
    expected_copies: Rational,
) -> Result<Rational, PolicyError> {
    if pack_price.is_zero() || candidates.is_zero() || expected_copies.numerator.is_zero() {
        return Err(PolicyError::InvalidRecipe);
    }
    Rational::whole(pack_price).divide(&Rational::whole(candidates).multiply(&expected_copies))
}

/// Restricted parties may receive gifts but cannot give more than 130% of incoming value.
pub fn outgoing_allowed(outgoing: &Rational, incoming: &Rational) -> bool {
    outgoing
        .multiply(&Rational::whole(10.into()))
        .compare(&incoming.multiply(&Rational::whole(13.into())))
        != Ordering::Greater
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valuation_wire_never_rounds_or_accepts_noncanonical_fractions() {
        let value: Rational =
            serde_json::from_value(json!({"numerator":"2000","denominator":"3"})).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            json!({"numerator":"2000","denominator":"3"})
        );
        for invalid in [
            json!({"numerator":1,"denominator":"3"}),
            json!({"numerator":"1","denominator":"0"}),
            json!({"numerator":"01","denominator":"3"}),
            json!({"numerator":"2","denominator":"6"}),
            json!({"numerator":"0","denominator":"2"}),
            json!({"numerator":"1.5","denominator":"1"}),
        ] {
            assert!(serde_json::from_value::<Rational>(invalid).is_err());
        }
        let huge = "9".repeat(100);
        assert!(
            serde_json::from_value::<Rational>(json!({"numerator":huge,"denominator":"1"})).is_ok()
        );
        assert!(
            serde_json::from_value::<Rational>(
                json!({"numerator":"9".repeat(MAX_RATIONAL_DIGITS+1),"denominator":"1"})
            )
            .is_err()
        );
    }
}
