//! Entry-type choice and mana usable for matching creature spells.
use super::*;
use spellwake_engine::{
    land::EntryTap,
    mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaType},
    program::ProgramKind,
};

fn chosen_subtype_grant(
    program: &mut Program,
    selector: spellwake_engine::characteristics::PermanentSelector,
) -> Option<()> {
    use spellwake_engine::characteristics::{
        StaticPart, StaticParts, StaticSubtypeOperand, StaticTypeChange, SubtypeOperation,
        TypeOperation,
    };
    let ProgramKind::Permanent(permanent) = &mut program.kind else {
        return None;
    };
    permanent.entry_tap = Some(EntryTap::ChooseCreatureType);
    army_grants::add_battlefield_ability(
        program,
        selector,
        StaticParts(vec![StaticPart::Type(StaticTypeChange {
            types: TypeOperation::Keep,
            supertypes: TypeOperation::Keep,
            land_subtypes: SubtypeOperation::Keep,
            subtypes: SubtypeOperation::Add(StaticSubtypeOperand::SourceEntryCreatureType),
        })]),
    )
}

pub(super) fn choose_and_gain_type(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::characteristics::{ControllerSelector, PermanentSelector};
    use spellwake_engine::program::{AffectedPlayers, ResolvingPlayer, TriggerGrouping};
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    if text.trim()
        != "As this creature enters, choose a creature type.\nThis creature is the chosen type in addition to its other types.\nWhenever this creature enters or attacks, you may draw a card for each other creature you control that shares a creature type with it. If you do, discard a card."
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
    chosen_subtype_grant(
        &mut program,
        PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        },
    )?;
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
            BodyTarget::None,
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 2,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::CountOtherSharingSourceSubtype,
                ),
                Instruction::Discard {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                },
            ],
        ),
    });
    Some(program)
}

pub(super) fn chosen_type_across_zones(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::characteristics::PermanentSelector;
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut lines = text.lines().map(str::trim);
    let name = lines
        .next()?
        .strip_prefix("As ")?
        .strip_suffix(" enters, choose a creature type.")?;
    if name != face.name && Some(name) != face.name.split(',').next() {
        return None;
    }
    if lines.next()?
        != "Slivers you control and nontoken creatures you control are the chosen type in addition to their other creature types. The same is true for creature spells you control and creature cards you own that aren't on the battlefield."
    {
        return None;
    }
    let (cost, effect) = lines.next()?.split_once(", {T}: Create ")?;
    if effect != "a 1/1 colorless Sliver creature token." || lines.next().is_some() {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
    chosen_subtype_grant(
        &mut program,
        PermanentSelector {
            controlled_creatures_and_typed_tokens: Some(vocabulary.resolve_label("Sliver")?),
            ..PermanentSelector::default()
        },
    )?;
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: fixed_cost(cost)?,
        source_cost: spellwake_engine::activated::SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![ordinary_text::create_tokens(
                effect.strip_suffix('.')?,
                vocabulary,
            )?],
        ),
    });
    Some(program)
}
pub(super) fn colored_reduction(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::characteristics::{
        ControllerSelector, NumericExpression, NumericPair, PermanentSelector, StaticAbilityId,
        StaticPart, StaticParts,
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut other = Vec::new();
    let mut entry = false;
    let mut reduction = false;
    let mut bonus = false;
    for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(name) = line
            .strip_prefix("As ")
            .and_then(|s| s.strip_suffix(" enters, choose a creature type."))
            && (name == face.name
                || Some(name) == face.name.split(',').next()
                || name == "this creature")
        {
            entry = true;
        } else if line
            == "Spells of the chosen type you cast cost {W}{U}{B}{R}{G} less to cast. This effect reduces only the amount of colored mana you pay."
        {
            reduction = true;
        } else if line == "Other creatures you control of the chosen type get +1/+1." {
            bonus = true;
        } else {
            other.push(line);
        }
    }
    if !(entry && reduction && bonus) {
        return None;
    }
    let mut base =
        ordinary_creature(card, vocabulary, Profile::Current, Some(&other.join("\n"))).ok()?;
    let ProgramKind::Permanent(permanent) = &mut base.kind else {
        return None;
    };
    if permanent.entry_tap.is_some() || base.continuous.is_some() {
        return None;
    }
    permanent.entry_tap = Some(EntryTap::ChooseCreatureType);
    base.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CostModifier {
            id: StaticAbilityId(0),
            modifier:
                spellwake_engine::characteristics::SpellCostModifier::ChosenTypeOneLessOfEachColor,
        },
    );
    super::army_grants::add_battlefield_ability(
        &mut base,
        PermanentSelector {
            controller: ControllerSelector::SourceController,
            exclude_source: true,
            matches_source_entry_type: true,
            required_types: [CardType::Creature].into(),
            ..Default::default()
        },
        StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(1.into()),
            toughness: NumericExpression::Literal(1.into()),
        })]),
    )?;
    Some(base)
}

pub(super) fn compile(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current || card.layout != "normal" || face.name != card.name {
        return None;
    }
    if let Some(program) = chosen_type_artifact(card) {
        return Some(program);
    }
    let choice = ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::choose_color_for_creature_type(
            spellwake_engine::mana::ChosenTypeManaScope::CreatureSpell,
        ),
    };
    let mut program = match (face.type_line.as_deref()?, face.oracle_text.as_deref()?) {
        (
            "Land",
            "As this land enters, choose a creature type.\n{T}: Add {C}.\n{T}: Add one mana of any color. Spend this mana only to cast a creature spell of the chosen type, and that spell can't be countered.",
        ) if face.mana_cost.as_deref().is_none_or(str::is_empty) => Program::land(
            BTreeSet::new(),
            vec![
                ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())])),
                },
                ManaAbility {
                    id: ManaAbilityId(1),
                    output: ManaOutput::choose_color_for_creature_type(
                        spellwake_engine::mana::ChosenTypeManaScope::CreatureSpellUncounterable,
                    ),
                },
            ],
        ),
        (
            "Land",
            "As this land enters, choose a creature type.\n{T}: Add {C}.\n{T}: Add one mana of any color. Spend this mana only to cast a creature spell of the chosen type.",
        ) if face.mana_cost.as_deref().is_none_or(str::is_empty) => Program::land(
            BTreeSet::new(),
            vec![
                ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())])),
                },
                choice,
            ],
        ),
        (
            "Land",
            "As this land enters, choose a creature type.\n{T}: Add {C}.\n{T}: Add one mana of any color. Spend this mana only to cast a creature spell of the chosen type or activate an ability of a creature source of the chosen type.",
        ) if face.mana_cost.as_deref().is_none_or(str::is_empty) => Program::land(
            BTreeSet::new(),
            vec![
                ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())])),
                },
                ManaAbility {
                    id: ManaAbilityId(1),
                    output: ManaOutput::choose_color_for_creature_type(
                        spellwake_engine::mana::ChosenTypeManaScope::CreatureSpellOrAbility,
                    ),
                },
            ],
        ),
        (
            "Artifact",
            "As this artifact enters, choose a creature type.\n{T}: Add one mana of any color. Spend this mana only to cast a creature spell of the chosen type.",
        ) if face.mana_cost.as_deref() == Some("{2}") => {
            let mut artifact = Program::artifact(Some(fixed_cost("{2}")?), vec![]);
            artifact.mana.push(choice);
            artifact
        }
        _ => return None,
    };
    let ProgramKind::Permanent(permanent) = &mut program.kind else {
        return None;
    };
    permanent.entry_tap = Some(EntryTap::ChooseCreatureType);
    Some(program)
}

/// Commander-only mana paid by tapping and sacrificing the artifact.
pub(super) fn jeweled_lotus(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Artifact")
        || face.mana_cost.as_deref() != Some("{0}")
        || face.oracle_text.as_deref()
            != Some(
                "{T}, Sacrifice this artifact: Add three mana of any one color. Spend this mana only to cast your commander.",
            )
    {
        return None;
    }
    let mut program = Program::artifact(Some(fixed_cost("{0}")?), vec![]);
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::ChooseColor {
            amount: 3.into(),
            colors: ManaOutput::all_colors(),
        }
        .restricted(spellwake_engine::mana::ManaRestriction::CommanderSpell)
        .sacrifice(spellwake_engine::mana::ManaSacrificeCost::Source { tap: true }),
    });
    Some(program)
}

/// Lower rules that refer to the creature type selected on entry.
fn chosen_type_artifact(card: &InventoryCard) -> Option<Program> {
    use spellwake_engine::{
        activated::SourceCost,
        characteristics::{
            ControllerSelector, NumericExpression, NumericPair, PermanentSelector, SpellDiscount,
            StaticAbilityId, StaticPart, StaticParts,
        },
        program::{
            BodyTarget, ExecutableBody, Instruction, LookTopFilter, PlayerRelation, TriggerGrouping,
        },
        turn::Step,
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if face.type_line.as_deref() != Some("Artifact") {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let lines = text.trim().lines().map(str::trim).collect::<Vec<_>>();
    let [entry, static_rule, other] = lines.as_slice() else {
        return None;
    };
    if entry.strip_prefix("As this artifact enters, ") != Some("choose a creature type.") {
        return None;
    }
    let mut result = Program::artifact(Some(fixed_cost(face.mana_cost.as_deref()?)?), vec![]);
    let ProgramKind::Permanent(permanent) = &mut result.kind else {
        return None;
    };
    permanent.entry_tap = Some(EntryTap::ChooseCreatureType);
    let (condition, instruction) = if static_rule
        .strip_prefix("Creatures you control of the chosen type get ")
        == Some("+1/+1.")
    {
        super::army_grants::add_battlefield_ability(
            &mut result,
            PermanentSelector {
                controller: ControllerSelector::SourceController,
                required_types: [CardType::Creature].into(),
                matches_source_entry_type: true,
                ..Default::default()
            },
            StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(1.into()),
                toughness: NumericExpression::Literal(1.into()),
            })]),
        )?;
        if other.strip_prefix("Whenever you cast a creature spell of the chosen type, ")
            == Some("draw a card.")
        {
            (
                spellwake_engine::program::TriggerSpec::spell_cast(
                    PlayerRelation::Controller,
                    spellwake_engine::program::SpellFilter::creature().of_chosen_entry_type(),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            )
        } else {
            let (cost, effect) = other.split_once(", {T}: ")?;
            let (look, result_text) = effect
                .strip_prefix("Look at the top ")?
                .split_once(" cards of your library. ")?;
            let (reveal, bottom) = result_text.split_once(". ")?;
            if reveal.strip_prefix("You may reveal a creature card of the chosen type ")
                != Some("from among them and put it into your hand")
                || bottom.strip_prefix("Put the rest on the bottom of your library ")
                    != Some("in a random order.")
            {
                return None;
            }
            result.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(0),
                mana_cost: fixed_cost(cost)?,
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::LookTopReveal {
                        count: super::ordinary_text::amount(look)?.try_into().ok()?,
                        filter: LookTopFilter::SourceChosenCreatureType,
                    }],
                ),
            });
            return Some(result);
        }
    } else {
        let cost = static_rule
            .strip_prefix("Creature spells you cast of the chosen type cost ")?
            .strip_suffix(" less to cast.")?
            .strip_prefix('{')?
            .strip_suffix('}')?
            .parse::<std::num::NonZeroU8>()
            .ok()?;
        let remainder = other
            .strip_prefix("At the beginning of your upkeep, ")?
            .strip_prefix("look at the top card of your library. ")?;
        if remainder.strip_prefix("If it's a creature card of the chosen type, ")
            != Some("you may reveal it and put it into your hand.")
        {
            return None;
        }
        result.continuous = Some(
            spellwake_engine::characteristics::StaticContinuousAbility::CostModifier {
                id: StaticAbilityId(0),
                modifier: spellwake_engine::characteristics::SpellCostModifier::Discount {
                    any_player: false,
                    matching: spellwake_engine::program::SpellFilter::creature()
                        .of_chosen_entry_type(),
                    amount: SpellDiscount::Fixed(cost),
                },
            },
        );
        (
            spellwake_engine::program::TriggerSpec::step_began(
                Step::Upkeep,
                PlayerRelation::Controller,
            ),
            Instruction::LookTopReveal {
                count: 1,
                filter: LookTopFilter::SourceChosenCreatureTypeKeepRest,
            },
        )
    };
    result.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![instruction]),
    });
    Some(result)
}
