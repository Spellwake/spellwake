//! Creatures whose complete non-keyword text creates the monarch designation.
use super::*;
use spellwake_engine::program::{BodyTarget, ExecutableBody, TriggerGrouping};

/// Full Aura text, including its independent untap restriction and entry trigger.
pub(super) fn fall_from_favor(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Enchantment — Aura")
        || face.mana_cost.as_deref() != Some("{2}{U}")
        || face.oracle_text.as_deref()
            != Some(
                "Enchant creature\nWhen this Aura enters, tap enchanted creature and you become the monarch.\nEnchanted creature doesn't untap during its controller's untap step unless that player is the monarch.",
            )
    {
        return None;
    }
    let mut program = Program::aura(
        Some(fixed_cost("{2}{U}")?),
        spellwake_engine::attachment::EnchantRestriction::Creature,
    );
    program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::UntapRule { id: spellwake_engine::characteristics::StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticUntapRule::EnchantedCreatureUntapsOnlyForMonarch });
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::TapEnchantedCreature,
                Instruction::BecomeMonarch(None),
            ],
        ),
    });
    Some(program)
}

/// The extra blocking capacity applies only while this creature's controller is monarch.
pub(super) fn entourage(
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
        || face.oracle_text.as_deref()
            != Some(
                "When this creature enters, you become the monarch.\nThis creature can block an additional creature each combat as long as you're the monarch.",
            )
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::Modifier {
            id: spellwake_engine::characteristics::StaticAbilityId(0),
            rule:
                spellwake_engine::characteristics::StaticModifierRule::AdditionalBlockWhileMonarch,
        },
    );
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![Instruction::BecomeMonarch(None)]),
    });
    Some(program)
}

/// A separate triggered mana ability resolves without using the stack.
pub(super) fn behemoth(
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
        || face.oracle_text.as_deref()
            != Some(
                "Trample\nWhen this creature enters, you become the monarch.\nWhenever you tap a land for mana while you're the monarch, add an additional one mana of any color.",
            )
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("Trample")).ok()?;
    program.mana.push(spellwake_engine::mana::ManaAbility {
        id: spellwake_engine::mana::ManaAbilityId(0),
        output: spellwake_engine::mana::ManaOutput::Triggered(
            spellwake_engine::mana::TriggeredManaRule::LandTappedWhileMonarch,
        ),
    });
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![Instruction::BecomeMonarch(None)]),
    });
    Some(program)
}

/// Full two-sentence sorcery: apply the temporary modifier before the designation.
pub(super) fn feast(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Sorcery")
        || face.mana_cost.as_deref() != Some("{4}{B}{B}")
        || face.oracle_text.as_deref()
            != Some("All creatures get -4/-4 until end of turn. You become the monarch.")
    {
        return None;
    }
    let mut body = super::spells::ordinary_effects::body(
        "All creatures get -4/-4 until end of turn.",
        &card.name,
    )?;
    body.instructions.push(Instruction::BecomeMonarch(None));
    Some(Program::sorcery(fixed_cost("{4}{B}{B}")?, body))
}

/// The two distinct Palace Jailer entry abilities retain their separate ordering.
pub(super) fn jailer(
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
        || face.oracle_text.as_deref()
            != Some(
                "When this creature enters, you become the monarch.\nWhen this creature enters, exile target creature an opponent controls until an opponent becomes the monarch.",
            )
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    let condition = spellwake_engine::program::TriggerSpec::enters(
        spellwake_engine::program::ObjectFilter::self_object(),
    );
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: condition.clone(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![Instruction::BecomeMonarch(None)]),
    });
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::FilteredPermanent(
                spellwake_engine::program::ObjectFilter::creature().opposing(),
            ),
            vec![Instruction::ExileTargetUntilOpponentMonarch],
        ),
    });
    Some(program)
}

/// Monarch-conditioned upkeep token triggers with an intervening-if recheck.
pub(super) fn upkeep(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    use spellwake_engine::{
        characteristics::Color,
        token::{TokenCreation, TokenDefinition, TokenName, TokenSubtype},
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current || card.layout != "normal" || face.name != card.name {
        return None;
    }
    let (base, opponent, subtype, color, size, keywords) = match face.oracle_text.as_deref()? {
        "Deathtouch, haste\nWhen Queen Marchesa enters, you become the monarch.\nAt the beginning of your upkeep, if an opponent is the monarch, create a 1/1 black Assassin creature token with deathtouch and haste." => {
            (
                "Deathtouch, haste",
                true,
                "Assassin",
                Color::Black,
                1,
                vec![KeywordAbility::Deathtouch, KeywordAbility::Haste],
            )
        }
        "Flying\nWhen this creature enters, you become the monarch.\nAt the beginning of your upkeep, if you're the monarch, create a 5/5 red Dragon creature token with flying." => {
            (
                "Flying",
                false,
                "Dragon",
                Color::Red,
                5,
                vec![KeywordAbility::Flying],
            )
        }
        _ => return None,
    };
    let mut program = ordinary_creature(card, vocabulary, profile, Some(base)).ok()?;
    let subtype = vocabulary.resolve_label(subtype)?;
    let mut token = Program::creature(None, size.into(), size.into(), [subtype].into())
        .with_printed_keywords(keywords);
    token.effect_defined_colors = Some([color].into());
    let definition = TokenDefinition::new(
        token,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(subtype)],
        },
        vocabulary,
    )
    .ok()?;
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![Instruction::BecomeMonarch(None)]),
    });
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::step_began(
            spellwake_engine::turn::Step::Upkeep,
            spellwake_engine::program::PlayerRelation::Controller,
        )
        .only_if(spellwake_engine::program::TriggerCondition::Monarch { opponent }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::RequireCondition(
                    spellwake_engine::program::InterveningCondition::Monarch { opponent },
                ),
                Instruction::CreateTokens {
                    creations: vec![TokenCreation::one(definition)],
                },
            ],
        ),
    });
    Some(program)
}

/// Regal Sliver grants a separate entry trigger to each controlled Sliver creature.
pub(super) fn regal_sliver(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    use spellwake_engine::characteristics::{
        ControllerSelector, PermanentSelector, StaticAbilityId, StaticContinuousAbility,
        StaticPart, StaticParts,
    };
    use spellwake_engine::program::{Comparison, EffectSubject, ObjectFilter, ResolutionCondition};
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.oracle_text.as_deref()
            != Some(
                "Sliver creatures you control have \"When this creature enters, Slivers you control get +1/+1 until end of turn if you're the monarch. Otherwise, you become the monarch.\"",
            )
    {
        return None;
    }
    let subtype = vocabulary.resolve_label("Sliver")?;
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    let body = ExecutableBody::new(
        BodyTarget::None,
        vec![
            Instruction::BranchIfFalse {
                condition: ResolutionCondition {
                    left: spellwake_engine::program::ValueExpr::PlayerStat(
                        spellwake_engine::program::PlayerStatExpr::ControllerIsMonarch,
                    ),
                    comparison: Comparison::Equal,
                    right: spellwake_engine::program::ValueExpr::Literal(1.into()),
                },
                destination: 3,
            },
            crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                EffectSubject::Matching(ObjectFilter::any().controlled().with_subtype(subtype)),
                spellwake_engine::program::ValueExpr::literal(1),
                spellwake_engine::program::ValueExpr::literal(1),
                BTreeSet::new(),
                BTreeSet::new(),
            ),
            Instruction::Jump { destination: 4 },
            Instruction::BecomeMonarch(None),
        ],
    );
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceController,
            required_types: [CardType::Creature].into(),
            creature_subtype: Some(subtype),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::GrantTrigger(Box::new(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body,
        }))]),
    });
    Some(program)
}

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current || card.layout != "normal" || face.name != card.name {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let base = match text.trim() {
        "When this creature enters, you become the monarch." => "",
        "Trample\nWhen this creature enters, you become the monarch." => "Trample",
        "Vigilance\nWhen this creature enters, you become the monarch." => "Vigilance",
        "Deathtouch\nWhen this creature enters, you become the monarch." => "Deathtouch",
        _ => return None,
    };
    let mut program = ordinary_creature(card, vocabulary, profile, Some(base)).ok()?;
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![Instruction::BecomeMonarch(None)]),
    });
    Some(program)
}
