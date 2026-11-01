//! Exact unrestricted payment and authored fixed noncreature tap-mana definitions.
//! EffectWindow owns production and pools. No restrictions, hybrid/snow/Phyrexian/X,
//! payment-time activation or turn cleanup.
use crate::{card_identity::CardObjectId, priority::SeatId, stack_window::StackEntryId};
use spellwake_contracts::RulesInteger;
use std::collections::BTreeMap;

/// Colorless is a sixth mana type, not a sixth color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ManaType {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colourless,
}
impl ManaType {
    pub const ALL: [Self; 6] = [
        Self::White,
        Self::Blue,
        Self::Black,
        Self::Red,
        Self::Green,
        Self::Colourless,
    ];
}

/// An explicit cost; a zero cost is distinct from an absent, unpayable cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaCost {
    pub generic: RulesInteger,
    pub required: BTreeMap<ManaType, RulesInteger>,
}
impl ManaCost {
    pub fn one(mana_type: ManaType) -> Self {
        Self::specific(mana_type, 1.into())
    }
    pub fn specific(mana_type: ManaType, amount: RulesInteger) -> Self {
        Self {
            generic: 0.into(),
            required: BTreeMap::from([(mana_type, amount)]),
        }
    }
    pub fn generic(amount: RulesInteger) -> Self {
        Self {
            generic: amount,
            required: BTreeMap::new(),
        }
    }
    pub fn free() -> Self {
        Self::generic(0.into())
    }
    pub fn total(&self) -> RulesInteger {
        self.required
            .values()
            .fold(self.generic.clone(), |sum, amount| sum.plus(amount))
    }
    pub(crate) fn valid(&self) -> bool {
        self.generic >= 0.into() && self.required.values().all(|amount| *amount >= 0.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatingMana {
    pub player: SeatId,
    pub mana_type: ManaType,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaSpend {
    pub mana_type: ManaType,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManaPayment {
    pub spend: Vec<ManaSpend>,
}
/// Current accepted cast evidence, not a pool store or durable receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaPaymentRecord {
    pub source: StackEntryId,
    pub payer: SeatId,
    pub cost: ManaCost,
    pub spend: Vec<ManaSpend>,
}
/// Identifies an authored ability on its source; not a stack entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ManaAbilityId(pub u64);

/// Closed noncreature-land ability: tap this source, add these fixed unrestricted
/// amounts to its controller's pool. No targets, choices, other costs or triggers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedManaAbility {
    pub id: ManaAbilityId,
    pub produced: BTreeMap<ManaType, RulesInteger>,
}
impl FixedManaAbility {
    pub(crate) fn valid(&self) -> bool {
        !self.produced.is_empty() && self.produced.values().all(RulesInteger::is_positive)
    }
}
/// Current native activation evidence, not an archive or durable acceptance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaProductionRecord {
    pub source: CardObjectId,
    pub ability: ManaAbilityId,
    pub controller: SeatId,
    pub produced: BTreeMap<ManaType, RulesInteger>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManaPaymentError {
    InvalidCost,
    NonPositiveSpend,
    DuplicateType,
    MissingRequired,
    WrongTotal,
    InsufficientMana,
    PaymentRequired,
}

/// Pure validation. The caller alone owns and debits the actual pool after all
/// cast checks succeed. Normalization orders types, never chooses generic spend.
pub(crate) fn prepare_payment(
    cost: &ManaCost,
    selected: Option<&ManaPayment>,
    available: impl Fn(ManaType) -> RulesInteger,
) -> Result<Vec<ManaSpend>, ManaPaymentError> {
    if !cost.valid() {
        return Err(ManaPaymentError::InvalidCost);
    }
    let Some(selected) = selected else {
        let mut residual: RulesInteger = 0.into();
        for mana_type in ManaType::ALL {
            let pool = available(mana_type);
            let required = cost
                .required
                .get(&mana_type)
                .cloned()
                .unwrap_or_else(|| 0.into());
            if pool < required {
                return Err(ManaPaymentError::InsufficientMana);
            }
            residual = residual.plus(&pool.minus(&required));
        }
        if residual < cost.generic {
            return Err(ManaPaymentError::InsufficientMana);
        }
        if cost.generic.is_positive() {
            return Err(ManaPaymentError::PaymentRequired);
        }
        return Ok(cost
            .required
            .iter()
            .filter(|(_, amount)| amount.is_positive())
            .map(|(&mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.clone(),
            })
            .collect());
    };
    let mut spend = BTreeMap::new();
    for entry in &selected.spend {
        if !entry.amount.is_positive() {
            return Err(ManaPaymentError::NonPositiveSpend);
        }
        if spend
            .insert(entry.mana_type, entry.amount.clone())
            .is_some()
        {
            return Err(ManaPaymentError::DuplicateType);
        }
    }
    for (&mana_type, required) in &cost.required {
        if spend.get(&mana_type).cloned().unwrap_or_else(|| 0.into()) < *required {
            return Err(ManaPaymentError::MissingRequired);
        }
    }
    let total = spend
        .values()
        .fold(RulesInteger::from(0), |sum, amount| sum.plus(amount));
    if total != cost.total() {
        return Err(ManaPaymentError::WrongTotal);
    }
    for (&mana_type, amount) in &spend {
        if *amount > available(mana_type) {
            return Err(ManaPaymentError::InsufficientMana);
        }
    }
    Ok(spend
        .into_iter()
        .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
        .collect())
}
