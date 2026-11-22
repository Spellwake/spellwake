//! The three original Urza lands, whose outputs depend on current land subtypes.
use super::*;
use spellwake_engine::{
    land::LandType,
    mana::{ManaAbility, ManaAbilityId, ManaOutput},
};

pub(super) fn compile(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face
            .mana_cost
            .as_deref()
            .is_some_and(|cost| !cost.is_empty())
    {
        return None;
    }
    use LandType::{Mine, PowerPlant, Tower, Urzas};
    let (own, first, second, bonus) = match (
        face.type_line.as_deref()?,
        face.oracle_text.as_deref()?,
    ) {
        (
            "Land — Urza's Mine",
            "{T}: Add {C}. If you control an Urza's Power-Plant and an Urza's Tower, add {C}{C} instead.",
        ) => (Mine, PowerPlant, Tower, 2),
        (
            "Land — Urza's Power-Plant",
            "{T}: Add {C}. If you control an Urza's Mine and an Urza's Tower, add {C}{C} instead.",
        ) => (PowerPlant, Mine, Tower, 2),
        (
            "Land — Urza's Tower",
            "{T}: Add {C}. If you control an Urza's Mine and an Urza's Power-Plant, add {C}{C}{C} instead.",
        ) => (Tower, Mine, PowerPlant, 3),
        _ => return None,
    };
    Some(Program::land(
        BTreeSet::from([Urzas, own]),
        vec![ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::Dynamic(
                spellwake_engine::mana::DynamicManaYield::ConditionalControlledLandTypes {
                    requirements: vec![
                        BTreeSet::from([Urzas, first]),
                        BTreeSet::from([Urzas, second]),
                    ],
                    normal: BTreeMap::from([(ManaType::Colourless, 1.into())]),
                    fulfilled: BTreeMap::from([(ManaType::Colourless, bonus.into())]),
                },
            ),
        }],
    ))
}
