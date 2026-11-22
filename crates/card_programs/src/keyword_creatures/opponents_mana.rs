//! Mana whose choice is derived from opponent-controlled lands.
use super::*;
use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaOutput};

pub(super) fn compile(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.oracle_text.as_deref()
            != Some(
                "{T}: Add one mana of any color that a land an opponent controls could produce.",
            )
    {
        return None;
    }
    let ability = ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::SpecialColor(
            spellwake_engine::mana::SpecialColorYield::OpponentsLandColors,
        ),
    };
    match face.type_line.as_deref()? {
        "Land" if face.mana_cost.as_deref().is_none_or(str::is_empty) => {
            Some(Program::land(BTreeSet::new(), vec![ability]))
        }
        "Artifact" if face.mana_cost.as_deref() == Some("{2}") => {
            let mut program = Program::artifact(Some(fixed_cost("{2}")?), vec![]);
            program.mana.push(ability);
            Some(program)
        }
        _ => None,
    }
}
