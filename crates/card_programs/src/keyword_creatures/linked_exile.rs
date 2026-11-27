//! Complete faces with source-linked exile abilities.
use super::*;
use spellwake_engine::program::{ObjectFilter, TriggerGrouping};

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
    let oracle = without_reminder(face.oracle_text.as_deref()?)?;
    if face.type_line.as_deref()? != "Enchantment" {
        return None;
    }
    if oracle.trim()
        == "When this enchantment enters, for each opponent, exile up to one target nonland permanent that player controls until this enchantment leaves the battlefield."
    {
        let mut program = Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::FilteredPermanent(ObjectFilter::nonland().opposing()),
                vec![Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::exile_one_per_player_until_source_leaves(),
                ))],
            ),
        });
        return Some(program);
    }
    let lines = oracle
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let [flash, trigger, cycling] = lines.as_slice() else {
        return None;
    };
    if *flash != "Flash"
        || *trigger
            != "When this enchantment enters, exile target nonland permanent an opponent controls until this enchantment leaves the battlefield."
    {
        return None;
    }
    let mut program = Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
    program.printed_keywords.insert(KeywordAbility::Flash);
    program.activated.push(super::cycling::hand_ability(
        cycling,
        ActivatedAbilityId(0),
        vocabulary,
        face.mana_cost.as_deref(),
    )?);
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::FilteredPermanent(ObjectFilter::nonland().opposing()),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target_until_source_leaves()],
        ),
    });
    program.validate(vocabulary).ok()?;
    Some(program)
}
