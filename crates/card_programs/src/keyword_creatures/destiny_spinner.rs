//! Complete source face with a controlled-spell shield and a counted land animation.
use super::*;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{KeywordAbility, ResolvedDuration, StaticAbilityId},
    program::{BodyTarget, CardType, ExecutableBody, Instruction, ObjectFilter},
};
use std::collections::BTreeSet;

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
    if text.trim()
        != "Creature and enchantment spells you control can't be countered.\n{3}{G}: Target land you control becomes an X/X Elemental creature with trample and haste until end of turn, where X is the number of enchantments you control. It's still a land."
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
    program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::CastPermission { id: StaticAbilityId(1), permission: spellwake_engine::characteristics::CastPermissionRule::ControllerSpellsCannotBeCountered(BTreeSet::from([CardType::Creature, CardType::Enchantment])) });
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: fixed_cost("{3}{G}")?,
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::FilteredPermanent(ObjectFilter::land().controlled()),
            vec![Instruction::AnimateTargetAsCountedCreature {
                amount: spellwake_engine::program::ValueExpr::controlled_type(
                    CardType::Enchantment,
                ),
                subtype: vocabulary.resolve_label("Elemental")?,
                keywords: BTreeSet::from([KeywordAbility::Trample, KeywordAbility::Haste]),
                duration: ResolvedDuration::UntilEndOfTurn,
            }],
        ),
    });
    Some(program)
}
