//! Artifact mana abilities with a generic payment and one chosen output color.
use super::*;
use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Artifact")
    {
        return None;
    }
    let mut paid = false;
    let mut fixed = false;
    let mut rest = Vec::new();
    for line in face.oracle_text.as_deref()?.lines() {
        match line {
            "{1}, {T}: Add one mana of any color." if !paid => paid = true,
            "{T}: Add {C}." if !fixed => fixed = true,
            other => rest.push(other),
        }
    }
    if !paid {
        return None;
    }
    let mut program = if rest.is_empty() {
        Program::artifact(Some(fixed_cost(face.mana_cost.as_deref()?)?), vec![])
    } else {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(rest.join("\n"));
        ordinary_triggers::compile(&reduced, vocabulary)?
    };
    if fixed {
        program.mana.push(ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())])),
        });
    }
    program.mana.push(ManaAbility {
        id: ManaAbilityId(program.mana.len().try_into().ok()?),
        output: ManaOutput::ChooseColor {
            amount: 1.into(),
            colors: ManaOutput::all_colors(),
        }
        .paid(ManaCost::generic(1.into())),
    });
    Some(program)
}
