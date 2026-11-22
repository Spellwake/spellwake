//! An any-color mana ability whose quantity depends on controlled creature power.
use super::*;
use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaOutput};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || face.oracle_text.as_deref()?
            != "{T}: Add one mana of any color. If you control a creature with power 4 or greater, add two mana of any one color instead."
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Dynamic(
            spellwake_engine::mana::DynamicManaYield::ControlledCreaturePowerColor {
                minimum_power: 4.into(),
                otherwise: 1.into(),
                matching: 2.into(),
            },
        ),
    });
    Some(program)
}
