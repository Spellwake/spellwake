//! Nonbasic lands with a colorless ability and a two-color hybrid filter.
use super::*;
use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaOutput};

fn color(symbol: &str) -> Option<ManaType> {
    Some(match symbol {
        "W" => ManaType::White,
        "U" => ManaType::Blue,
        "B" => ManaType::Black,
        "R" => ManaType::Red,
        "G" => ManaType::Green,
        _ => return None,
    })
}
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
            .is_some_and(|value| !value.is_empty())
    {
        return None;
    }
    let mut lines = face.oracle_text.as_deref()?.lines();
    if lines.next()? != "{T}: Add {C}." {
        return None;
    }
    let line = lines.next()?;
    if lines.next().is_some() {
        return None;
    }
    let hybrid = line.strip_prefix('{')?.split_once('}')?.0;
    let (a, b) = hybrid.split_once('/')?;
    let colors = [color(a)?, color(b)?];
    if colors[0] == colors[1]
        || line
            != format!("{{{a}/{b}}}, {{T}}: Add {{{a}}}{{{a}}}, {{{a}}}{{{b}}}, or {{{b}}}{{{b}}}.")
    {
        return None;
    }
    Some(Program::land(
        BTreeSet::new(),
        vec![
            ManaAbility {
                id: ManaAbilityId(0),
                output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())])),
            },
            ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::SpecialColor(
                    spellwake_engine::mana::SpecialColorYield::HybridFilter { colors },
                ),
            },
        ],
    ))
}
