//! Whole faces with high-mana-value cost reduction and protection.
use super::*;
use spellwake_engine::characteristics::StaticAbilityId;
use std::num::NonZeroU8;

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || card.name != face.name {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let (keywords, effect) = text.rsplit_once('\n')?;
    if effect
        != "Spells you cast with mana value 5 or greater cost {1} less to cast and can't be countered."
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some(keywords)).ok()?;
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CostModifier {
            id: StaticAbilityId(1),
            modifier:
                spellwake_engine::characteristics::SpellCostModifier::HighManaValueProtected {
                    minimum: NonZeroU8::new(5)?,
                    reduction: NonZeroU8::new(1)?,
                },
        },
    );
    Some(program)
}
