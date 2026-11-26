//! Whole faces whose single ability watches the controller cast an enchantment.
use super::*;
use spellwake_engine::characteristics::{
    ControllerSelector, NumericExpression, NumericPair, PermanentSelector, StaticPart, StaticParts,
};
use spellwake_engine::program::{
    BodyTarget, ExecutableBody, Instruction, PlayerRelation, ResolvingPlayer, TriggerGrouping,
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
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let types = face.type_line.as_deref()?;
    if types == "Enchantment"
        && text.trim()
            == "At the beginning of your upkeep, you may return target enchantment card from your graveyard to the battlefield.\nAs long as you control five or more enchantments, each other non-Aura enchantment you control is a creature in addition to its other types and has base power and base toughness each equal to its mana value."
    {
        use spellwake_engine::characteristics::{
            StaticTypeChange, SubtypeOperation, TypeOperation,
        };
        use spellwake_engine::program::ObjectFilter;
        use spellwake_engine::turn::Step;
        let mut program = Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::step_began(
                Step::Upkeep,
                PlayerRelation::Controller,
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::enchantment()),
                vec![
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(false),
                ],
            ),
        });
        army_grants::add_battlefield_ability(
            &mut program,
            PermanentSelector {
                required_types: [CardType::Enchantment].into(),
                controller: ControllerSelector::SourceController,
                controller_type_threshold: Some((
                    CardType::Enchantment,
                    std::num::NonZeroU16::new(5)?,
                )),
                exclude_source: true,
                exclude_auras: true,
                ..Default::default()
            },
            StaticParts(vec![
                StaticPart::Type(StaticTypeChange {
                    types: TypeOperation::Add([CardType::Creature].into()),
                    supertypes: TypeOperation::Keep,
                    subtypes: SubtypeOperation::Keep,
                    land_subtypes: SubtypeOperation::Keep,
                }),
                StaticPart::SetPTToOwnManaValue,
            ]),
        )?;
        return Some(program);
    }
    if text.trim()
        == "Flying\nWhenever a nontoken creature an opponent controls enters, create a token that's a copy of that creature except it's an artifact in addition to its other types. Then exile all other tokens created with this creature."
    {
        let mut program =
            ordinary_creature(card, vocabulary, Profile::Current, Some("Flying")).ok()?;
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::creature()
                    .opposing()
                    .nontoken(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::CopyEntrantAsArtifact,
                    Instruction::ReplaceSourceCreatedTokens,
                ],
            ),
        });
        return Some(program);
    }
    if text.trim()
        == "Whenever an enchantment you control enters, you may create a token that's a copy of it. Do this only once each turn."
    {
        let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::any_types(
                    spellwake_engine::program::CardTypeMask::from_card_type(CardType::Enchantment),
                )
                .controlled(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    Instruction::CopyEntrantOnceEachTurn,
                ],
            ),
        });
        return Some(program);
    }
    if types == "Enchantment"
        && text.trim()
            == "Constellation — Whenever this enchantment or another enchantment you control enters, put a blessing counter on this enchantment.\nCreatures you control get +1/+1 for each blessing counter on this enchantment."
    {
        let mut program = Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::any_types(
                    spellwake_engine::program::CardTypeMask::from_card_type(CardType::Enchantment),
                )
                .controlled()
                .or_self(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::PutBlessingCounterOnSource],
            ),
        });
        army_grants::add_battlefield_ability(
            &mut program,
            PermanentSelector {
                required_types: BTreeSet::from([CardType::Creature]),
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            },
            StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Counters {
                    source: None,
                    kind: spellwake_engine::effects::CounterKind::Blessing,
                },
                toughness: NumericExpression::Counters {
                    source: None,
                    kind: spellwake_engine::effects::CounterKind::Blessing,
                },
            })]),
        )?;
        return Some(program);
    }
    let (optional, gain_life) = match text.trim() {
        "Whenever you cast an enchantment spell, you may draw a card." => (true, false),
        "Whenever you cast an enchantment spell, draw a card." => (false, false),
        "Whenever you cast an enchantment spell, you gain 1 life and draw a card." => (false, true),
        _ => return None,
    };
    let mut program = if types
        .split(" — ")
        .next()?
        .split_whitespace()
        .any(|t| t == "Creature")
    {
        ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?
    } else if types == "Enchantment" {
        Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?))
    } else {
        return None;
    };
    let mut instructions = Vec::new();
    if optional {
        instructions.push(Instruction::OptionalEffect {
            chooser: ResolvingPlayer::Controller,
        });
    }
    if gain_life {
        instructions.push(Instruction::AdjustControllerLife {
            adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
        });
    }
    instructions.push(Instruction::DrawController {
        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
    });
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::spell_cast(
            PlayerRelation::Controller,
            spellwake_engine::program::SpellFilter::of_type(CardType::Enchantment),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, instructions),
    });
    Some(program)
}
