//! Complete permanent Ascend definitions.
use super::*;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, ActivationRestriction, SourceCost},
    characteristics::StaticAbilityId,
    mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaType},
    program::{BodyTarget, ExecutableBody, Instruction, TriggerGrouping},
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || face.name != card.name {
        return None;
    }
    let oracle = without_reminder(face.oracle_text.as_deref()?)?;
    let lines = oracle
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let ascend = spellwake_engine::characteristics::StaticContinuousAbility::Modifier {
        id: StaticAbilityId(0),
        rule: spellwake_engine::characteristics::StaticModifierRule::Ascend,
    };
    let program = match lines.as_slice() {
        [
            "Ascend",
            "{T}: Add {C}.",
            "{5}, {T}: Draw a card. Activate only if you have the city's blessing.",
        ] if face.type_line.as_deref() == Some("Land")
            && face.mana_cost.as_deref().is_none_or(str::is_empty) =>
        {
            let mut land = Program::land(
                BTreeSet::new(),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed([(ManaType::Colourless, 1.into())].into()),
                }],
            );
            let id = ActivatedAbilityId(0);
            land.continuous = Some(ascend);
            land.activated.push(ActivatedAbility {
                id,
                mana_cost: fixed_cost("{5}")?,
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                            spellwake_engine::program::ValueExpr::Literal(1.into()),
                        ),
                    ],
                ),
            });
            land.activation_restrictions
                .insert(id, ActivationRestriction::HasCityBlessing);
            land
        }
        [
            "Ascend",
            "You may play an additional land on each of your turns.",
            restriction,
        ] if restriction
            .strip_suffix(" can't attack or block unless you have the city's blessing.")
            .is_some_and(|subject| subject == "This creature" || subject == face.name) =>
        {
            let mut creature =
                ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
            creature.continuous = Some(
                spellwake_engine::characteristics::StaticContinuousAbility::CastPermission {
                    id: StaticAbilityId(1),
                    permission:
                        spellwake_engine::characteristics::CastPermissionRule::AdditionalLandPlays(
                            std::num::NonZeroU8::new(1)?,
                        ),
                },
            );
            creature.additional_battlefield_abilities.extend([
                ascend,
                spellwake_engine::characteristics::StaticContinuousAbility::Restriction { id: StaticAbilityId(2), rule: spellwake_engine::characteristics::StaticRestriction::CombatRequiresCityBlessing },
            ]);
            creature
        }
        ["Flying", "Ascend", upkeep]
            if upkeep.strip_prefix(
                "At the beginning of your upkeep, if you have the city's blessing, ",
            ) == Some(concat!(
                "reveal the top card of your library and put it into your hand. ",
                "Each opponent loses X life and you gain X life, where X is that card's mana value."
            )) =>
        {
            let mut creature =
                ordinary_creature(card, vocabulary, Profile::Current, Some("Flying")).ok()?;
            creature.continuous = Some(ascend);
            creature.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(0),
                condition: spellwake_engine::program::TriggerSpec::step_began(
                    spellwake_engine::turn::Step::Upkeep,
                    spellwake_engine::program::PlayerRelation::Controller,
                )
                .only_if(spellwake_engine::program::TriggerCondition::ControllerHasCityBlessing),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::RevealTopHandLoseManaValue { gain_life: true }],
                ),
            });
            creature
        }
        _ => return None,
    };
    program.validate(vocabulary).ok()?;
    Some(program)
}
