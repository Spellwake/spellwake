//! Exact unrestricted payment and authored tap-mana definitions.
//! EffectWindow owns production, pools and resolution-time mana permission.
//! Restricted mana, hybrid/snow/Phyrexian/X and broader costs remain separate work.
use crate::{
    characteristics::PermanentSelector, object_identity::ObjectId, priority::SeatId,
    stack_window::StackEntryId,
};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary};
use std::collections::{BTreeMap, BTreeSet};

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
/// One actual normalized payment for a cast, activation or resolving instruction.
/// Resolution outcomes retain their instruction separately; this is not a pool store.
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

/// Current mana activation selection. Authored IDs and rule-derived basic
/// abilities occupy disjoint domains, including authored IDs zero and u64::MAX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ManaAbilityRef {
    Printed(ManaAbilityId),
    BasicLand(crate::land::BasicLandType),
}

/// Authored tap mana ability; ordinary creature tap history is checked by the
/// enclosing executor. Output can be fixed, counted or chosen on resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaAbility {
    pub id: ManaAbilityId,
    pub output: ManaOutput,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManaOutput {
    /// Add one mana of this type per matching current battlefield permanent.
    /// The selector's source/controller are bound by the actual ability activation.
    Counted {
        mana_type: ManaType,
        permanents: PermanentSelector,
    },
    Fixed(BTreeMap<ManaType, RulesInteger>),
    ChooseColor {
        amount: RulesInteger,
        colors: BTreeSet<ManaType>,
    },
}
impl ManaAbility {
    pub(crate) fn valid(&self, vocabulary: &CreatureTypeVocabulary) -> bool {
        match &self.output {
            ManaOutput::Counted { permanents, .. } => permanents.validate(vocabulary).is_ok(),
            ManaOutput::Fixed(produced) => {
                !produced.is_empty() && produced.values().all(RulesInteger::is_positive)
            }
            ManaOutput::ChooseColor { amount, colors } => {
                amount.is_positive()
                    && !colors.is_empty()
                    && !colors.contains(&ManaType::Colourless)
            }
        }
    }
}
/// Current native activation evidence, not an archive or durable acceptance.
/// `produced` is empty when a counted ability resolves with a zero count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaProductionRecord {
    pub source: ObjectId,
    pub ability: ManaAbilityRef,
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
