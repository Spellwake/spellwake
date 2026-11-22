//! Reveal-or-tapped dual lands using the common entry decision.
use super::*;
use spellwake_engine::{
    land::{BasicLandType, EntryTap},
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
    use BasicLandType::{Forest, Island, Mountain, Plains, Swamp};
    let (first, second) = match face.oracle_text.as_deref()? {
        "As this land enters, you may reveal a Forest or Plains card from your hand. If you don't, this land enters tapped.\n{T}: Add {G} or {W}." => {
            (Forest, Plains)
        }
        "As this land enters, you may reveal an Island or Mountain card from your hand. If you don't, this land enters tapped.\n{T}: Add {U} or {R}." => {
            (Island, Mountain)
        }
        "As this land enters, you may reveal a Mountain or Plains card from your hand. If you don't, this land enters tapped.\n{T}: Add {R} or {W}." => {
            (Mountain, Plains)
        }
        "As this land enters, you may reveal a Swamp or Forest card from your hand. If you don't, this land enters tapped.\n{T}: Add {B} or {G}." => {
            (Swamp, Forest)
        }
        "As this land enters, you may reveal a Plains or Island card from your hand. If you don't, this land enters tapped.\n{T}: Add {W} or {U}." => {
            (Plains, Island)
        }
        "As this land enters, you may reveal a Plains or Swamp card from your hand. If you don't, this land enters tapped.\n{T}: Add {W} or {B}." => {
            (Plains, Swamp)
        }
        _ => return None,
    };
    let mut land = Program::land(
        BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::ChooseColor {
                amount: 1.into(),
                colors: [first.mana_type(), second.mana_type()].into(),
            },
        }],
    );
    let ProgramKind::Permanent(definition) = &mut land.kind else {
        return None;
    };
    definition.entry_tap = Some(EntryTap::UnlessRevealHandLandType(first, second));
    Some(land)
}
