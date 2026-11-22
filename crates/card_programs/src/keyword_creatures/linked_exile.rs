//! Complete faces with source-linked exile abilities.
use super::*;
use spellwake_engine::characteristics::{
    ControllerSelector, NumericExpression, NumericPair, PermanentSelector, StaticPart, StaticParts,
    StaticSubtypeOperand, StaticTypeChange, SubtypeOperation, TypeOperation,
};
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
    if let Some(characteristics) = oracle.trim().strip_prefix(
        "Imprint — When this creature enters, you may exile target nontoken creature.\n",
    ) {
        let stats = NumericPair {
            power: NumericExpression::ImprintedStat {
                stat: spellwake_engine::program::ObjectStatKind::Power,
                total: false,
            },
            toughness: NumericExpression::ImprintedStat {
                stat: spellwake_engine::program::ObjectStatKind::Toughness,
                total: false,
            },
        };
        let parts = match characteristics {
            "This creature gets +X/+Y, where X is the exiled creature card's power and Y is its toughness." =>
            {
                vec![StaticPart::ModifyPT(NumericPair {
                    power: NumericExpression::ImprintedStat {
                        stat: spellwake_engine::program::ObjectStatKind::Power,
                        total: true,
                    },
                    toughness: NumericExpression::ImprintedStat {
                        stat: spellwake_engine::program::ObjectStatKind::Toughness,
                        total: true,
                    },
                })]
            }
            "As long as a card exiled with this creature is a creature card, this creature has the power, toughness, and creature types of the last creature card exiled with it. It's still a Shapeshifter." =>
            {
                vec![
                    StaticPart::Type(StaticTypeChange {
                        types: TypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        land_subtypes: SubtypeOperation::Keep,
                        subtypes: SubtypeOperation::Set(
                            StaticSubtypeOperand::ImprintedCreatureTypes {
                                keep: BTreeSet::from([vocabulary.resolve_label("Shapeshifter")?]),
                            },
                        ),
                    }),
                    StaticPart::SetPT(stats),
                ]
            }
            _ => return None,
        };
        let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
        army_grants::add_battlefield_ability(
            &mut program,
            PermanentSelector {
                controller: ControllerSelector::SourceItself,
                source_has_imprinted_creature: true,
                ..PermanentSelector::default()
            },
            StaticParts(parts),
        )?;
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::FilteredPermanent(ObjectFilter::creature().nontoken()),
                vec![
                    Instruction::OptionalEffect {
                        chooser: spellwake_engine::program::ResolvingPlayer::Controller,
                    },
                    Instruction::ImprintTargetCreature,
                ],
            ),
        });
        return Some(program);
    }
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
                vec![Instruction::ExileOnePerPlayerUntilSourceLeaves],
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
            vec![Instruction::ExileTargetUntilSourceLeaves],
        ),
    });
    program.validate(vocabulary).ok()?;
    Some(program)
}
