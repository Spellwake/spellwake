//! Current-permanent count with a same-color mana choice.
use super::*;
use spellwake_engine::{
    characteristics::{ControllerSelector, PermanentSelector},
    mana::{ManaAbility, ManaAbilityId, ManaOutput},
};

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
            != "{T}: Add X mana of any one color, where X is the number of enchantments you control."
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Counted {
            mana_type: None,
            permanents: PermanentSelector {
                required_types: BTreeSet::from([CardType::Enchantment]),
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            },
        },
    });
    Some(program)
}
