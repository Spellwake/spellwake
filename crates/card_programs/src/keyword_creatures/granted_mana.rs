//! Sliver abilities granting a distinct tap-for-any-color mana ability.
use super::*;

/// Rishkar grants each controlled creature with any counter a distinct green tap ability.
pub(super) fn rishkar(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    use spellwake_engine::characteristics::{
        ControllerSelector, PermanentSelector, StaticPart, StaticParts,
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || without_reminder(face.oracle_text.as_deref()?)?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            != [
                "When Rishkar enters, put a +1/+1 counter on each of up to two target creatures.",
                "Each creature you control with a counter on it has \"{T}: Add {G}.\"",
            ]
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    army_grants::add_battlefield_ability(
        &mut program,
        PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            controller: ControllerSelector::SourceController,
            any_counter: true,
            ..PermanentSelector::default()
        },
        StaticParts(vec![StaticPart::GrantFixedColorMana(
            spellwake_engine::mana::ManaType::Green,
        )]),
    )?;
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::SINGLE_CREATURE,
            vec![
                crate::keyword_creatures::toolbox_emit::put_counters_on_target(
                    spellwake_engine::effects::CounterKind::PlusOne,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ],
        )
        .with_permanent_target_bounds(0, 2),
    });
    Some(program)
}
use spellwake_engine::characteristics::{
    ControllerSelector, PermanentSelector, StaticAbilityId, StaticContinuousAbility, StaticPart,
    StaticParts,
};

/// The artifact's own ability and each independently granted land ability coexist.
pub(super) fn lantern(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Artifact")
        || face.mana_cost.as_deref() != Some("{3}")
        || face.oracle_text.as_deref()
            != Some(
                "Lands you control have \"{T}: Add one mana of any color.\"\n{T}: Add one mana of any color.",
            )
    {
        return None;
    }
    let mut program = Program::artifact(Some(fixed_cost("{3}")?), vec![]);
    program.mana.push(spellwake_engine::mana::ManaAbility {
        id: spellwake_engine::mana::ManaAbilityId(0),
        output: basic_lands::mana_output("one mana of any color")?,
    });
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            controller: ControllerSelector::SourceController,
            required_types: [CardType::Land].into(),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::GrantAnyColorMana]),
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
    let (controller, creature_only) = match text.trim() {
        "All Slivers have \"{T}: Add one mana of any color.\"" => (ControllerSelector::Any, false),
        "Sliver creatures you control have \"{T}: Add one mana of any color.\"" => {
            (ControllerSelector::SourceController, true)
        }
        _ => return None,
    };
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            controller,
            required_types: if creature_only {
                [CardType::Creature].into()
            } else {
                BTreeSet::new()
            },
            creature_subtype: Some(vocabulary.resolve_label("Sliver")?),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::GrantAnyColorMana]),
    });
    Some(program)
}
