//! Layer-six grants of abilities on other creatures. Capture the donor's evaluated
//! ability and its restrictions together so activation always uses the recipient.
use super::*;
use crate::{
    activated::{ActivatedAbility, ActivatedAbilityId, ActivationRestriction},
    mana::{ManaAbilityRef, ManaOutput, ManaType},
};

/// Stable history for one ability, including intervening grants when multiple
/// permanents inherit from one another. Handles are scoped to the evaluated cut.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActivationOriginPath {
    via: Vec<(ObjectId, StaticAbilityId)>,
    origin: Option<EffectInstanceKey>,
    original_id: ActivatedAbilityId,
}

impl ActivationOriginPath {
    pub(crate) fn direct(
        original_id: ActivatedAbilityId,
        origin: Option<EffectInstanceKey>,
    ) -> Self {
        Self {
            via: Vec::new(),
            original_id,
            origin,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivationTerms {
    pub restriction: Option<ActivationRestriction>,
    pub loyalty: Option<i32>,
    pub variable_loyalty: bool,
    pub(crate) origin: ActivationOriginPath,
}

/// Independent abilities captured during a layer-six dependency probe.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct InheritedAbilities {
    pub activated: BTreeMap<EffectInstanceKey, (ActivatedAbility, ActivationTerms)>,
    pub mana: BTreeMap<EffectInstanceKey, ManaOutput>,
}

pub(super) fn countered_creature_abilities(
    source: ObjectId,
    ability: StaticAbilityId,
    objects: &BTreeMap<ObjectId, ObjectCharacteristics>,
    battlefield: &BTreeSet<ObjectId>,
) -> InheritedAbilities {
    inherit_activated_abilities(
        source,
        ability,
        battlefield
            .iter()
            .copied()
            .filter(|id| *id != source)
            .map(|id| &objects[&id])
            .filter(|donor| donor.is_creature() && donor.has_plus_one_counter),
    )
}

pub(crate) fn inherit_activated_abilities<'a>(
    source: ObjectId,
    ability: StaticAbilityId,
    donors: impl IntoIterator<Item = &'a ObjectCharacteristics>,
) -> InheritedAbilities {
    let mut result = InheritedAbilities::default();
    for other in donors {
        let donor = other.object;
        let key = |ordinal| EffectInstanceKey::Inherited {
            source,
            ability,
            donor,
            ordinal,
        };
        let printed = other
            .printed_text
            .activated
            .iter()
            .filter(|a| other.abilities.activated.contains(&a.id))
            .map(|a| (a, other.activation_terms(a.id, None)));
        let granted = other
            .granted_activated
            .iter()
            .map(|(effect, a)| (a, other.activation_terms(a.id, Some(*effect))));
        for (ordinal, (definition, mut terms)) in printed.chain(granted).enumerate() {
            terms.origin.via.push((donor, ability));
            let mut granted = definition.clone();
            granted.id = ActivatedAbilityId(0);
            result.activated.insert(key(ordinal), (granted, terms));
        }
        for (ordinal, reference) in other.mana_abilities().into_iter().enumerate() {
            if let Some(output) = other.mana_output(reference) {
                result.mana.insert(key(ordinal), output.into_owned());
            }
        }
    }
    result
}

impl ObjectCharacteristics {
    pub(crate) fn activation_terms(
        &self,
        id: ActivatedAbilityId,
        grant: Option<EffectInstanceKey>,
    ) -> ActivationTerms {
        if let Some(key) = grant {
            if let Some(terms) = self.inherited_activation_terms.get(&key) {
                return terms.clone();
            }
            let restriction = self
                .granted_activated
                .get(&key)
                .filter(|a| {
                    matches!(
                        a.body.instructions.as_slice(),
                        [crate::program::Instruction::StackOp(spec)]
                            if matches!(spec.op, crate::toolbox::StackOpKind::Encore)
                    )
                })
                .map(|_| ActivationRestriction::Sorcery);
            return ActivationTerms {
                restriction,
                loyalty: None,
                variable_loyalty: false,
                origin: ActivationOriginPath::direct(id, grant),
            };
        }
        ActivationTerms {
            restriction: self.printed_text.activation_restrictions.get(&id).cloned(),
            loyalty: self.printed_text.loyalty_costs.get(&id).copied(),
            variable_loyalty: self.printed_text.variable_loyalty_costs.contains(&id),
            origin: ActivationOriginPath::direct(id, None),
        }
    }

    /// Output associated with a currently available activated or triggered mana ability.
    pub(crate) fn mana_output(
        &self,
        reference: ManaAbilityRef,
    ) -> Option<std::borrow::Cow<'_, ManaOutput>> {
        use std::borrow::Cow;
        match reference {
            ManaAbilityRef::Printed(id) => self
                .printed_text
                .mana
                .iter()
                .find(|a| a.id == id && self.abilities.mana.contains(&id))
                .map(|a| Cow::Borrowed(&a.output)),
            ManaAbilityRef::BasicLand(kind) if self.intrinsic_mana.contains(&kind) => Some(
                Cow::Owned(ManaOutput::Fixed([(kind.mana_type(), 1.into())].into())),
            ),
            ManaAbilityRef::GrantedFixedColor(key, color)
                if self.granted_fixed_mana.get(&key) == Some(&color) =>
            {
                Some(Cow::Owned(ManaOutput::Fixed([(color, 1.into())].into())))
            }
            ManaAbilityRef::GrantedAnyColor(key) if self.granted_any_color_mana.contains(&key) => {
                Some(Cow::Owned(ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaType::ALL
                        .into_iter()
                        .filter(|c| *c != ManaType::Colourless)
                        .collect(),
                }))
            }
            ManaAbilityRef::Inherited(key) => self.inherited_mana.get(&key).map(Cow::Borrowed),
            _ => None,
        }
    }
}
