//! Ordered any-target attack damage with the complete authenticated face.
use super::*;
use spellwake_engine::program::{
    BodyTarget, ExecutableBody, Instruction, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout == "normal"
        && card.name == face.name
        && without_reminder(face.oracle_text.as_deref()?)?.trim()
            == "{R}: This creature gets +1/+0 until end of turn.\nWhenever this creature enters or attacks, it deals 3 damage divided as you choose among one, two, or three targets."
    {
        let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
        program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: fixed_cost("{R}")?,
            source_cost: spellwake_engine::activated::SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        spellwake_engine::program::EffectSubject::AbilitySource,
                        spellwake_engine::program::ValueExpr::literal(1),
                        spellwake_engine::program::ValueExpr::literal(0),
                        BTreeSet::new(),
                        BTreeSet::new(),
                    ),
                ],
            ),
        });
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::any_of(vec![
                spellwake_engine::program::TriggerEvent::Enters {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    from_zone: spellwake_engine::program::TriggerFromZone::Any,
                    cast_origin: None,
                    attached_to_creature: false,
                },
                spellwake_engine::program::TriggerEvent::Attacks {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    mode: spellwake_engine::program::AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
                },
            ]),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::SINGLE_ANY_TARGET,
                vec![Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec::divide_three(),
                ))],
            ),
        });
        return Some(program);
    }
    if card.layout != "normal"
        || card.name != "Drakuseth, Maw of Flames"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Legendary Creature — Dragon")
        || face.mana_cost.as_deref() != Some("{4}{R}{R}{R}")
        || without_reminder(face.oracle_text.as_deref()?)?.trim()
            != "Flying\nWhenever Drakuseth attacks, Drakuseth deals 4 damage to any target and 3 damage to each of up to two other targets."
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("Flying")).ok()?;
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::self_attacks(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::SINGLE_ANY_TARGET,
            vec![Instruction::DealDamage(Box::new(
                spellwake_engine::toolbox::DealDamageSpec::ordered_targets(4.into(), 3.into()),
            ))],
        ),
    });
    Some(program)
}
