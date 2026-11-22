//! Thriving land entry-color grammar for the supported entry continuations.
use super::*;
use spellwake_engine::{
    land::EntryTap,
    mana::{ManaAbility, ManaAbilityId, ManaOutput},
    program::ProgramKind,
};

pub(super) fn compile(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Land")
        || face
            .mana_cost
            .as_deref()
            .is_some_and(|cost| !cost.is_empty())
    {
        return None;
    }
    let fixed = match face.oracle_text.as_deref()? {
        "This land enters tapped. As it enters, choose a color other than white.\n{T}: Add {W} or one mana of the chosen color." => {
            ManaType::White
        }
        "This land enters tapped. As it enters, choose a color other than blue.\n{T}: Add {U} or one mana of the chosen color." => {
            ManaType::Blue
        }
        "This land enters tapped. As it enters, choose a color other than black.\n{T}: Add {B} or one mana of the chosen color." => {
            ManaType::Black
        }
        "This land enters tapped. As it enters, choose a color other than red.\n{T}: Add {R} or one mana of the chosen color." => {
            ManaType::Red
        }
        "This land enters tapped. As it enters, choose a color other than green.\n{T}: Add {G} or one mana of the chosen color." => {
            ManaType::Green
        }
        _ => return None,
    };
    let mut land = Program::land(
        BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::SpecialColor(
                spellwake_engine::mana::SpecialColorYield::EntryChosenColor { fixed },
            ),
        }],
    );
    let ProgramKind::Permanent(definition) = &mut land.kind else {
        return None;
    };
    definition.entry_tap = Some(EntryTap::AlwaysChooseColorExcept(fixed));
    Some(land)
}
