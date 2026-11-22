//! Complete basic-subtype and land-count restricted tap-mana lands.
use super::*;
use spellwake_engine::{
    characteristics::{ControllerSelector, PermanentSelector},
    land::LandType,
    mana::{ManaAbility, ManaAbilityId, ManaOutput},
};

/// Tomb's life gain counts currently controlled colorless creatures at resolution.
pub(super) fn tomb_of_spirit_dragon(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Land")
        || face
            .mana_cost
            .as_deref()
            .is_some_and(|cost| !cost.is_empty())
        || face.oracle_text.as_deref()
            != Some(
                "{T}: Add {C}.\n{2}, {T}: You gain 1 life for each colorless creature you control.",
            )
    {
        return None;
    }
    let mut program = Program::land(
        BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::Fixed(BTreeMap::from([(
                spellwake_engine::mana::ManaType::Colourless,
                1.into(),
            )])),
        }],
    );
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: fixed_cost("{2}")?,
        source_cost: spellwake_engine::activated::SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::GainControllerLife {
                amount: spellwake_engine::program::ValueExpr::count_battlefield(
                    spellwake_engine::program::ObjectFilter::creature()
                        .controlled()
                        .with_color(spellwake_engine::program::ObjectColorFilter::Colorless),
                ),
            }],
        ),
    });
    Some(program)
}

/// Storage counters can be accumulated or removed for a chosen amount of colorless mana.
pub(super) fn mage_ring(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Land")
        || face
            .mana_cost
            .as_deref()
            .is_some_and(|cost| !cost.is_empty())
        || face.oracle_text.as_deref()
            != Some(
                "{T}: Add {C}.\n{1}, {T}: Put a storage counter on this land.\n{T}, Remove any number of storage counters from this land: Add {C} for each storage counter removed this way.",
            )
    {
        return None;
    }
    let mut program = Program::land(
        BTreeSet::new(),
        vec![
            ManaAbility {
                id: ManaAbilityId(0),
                output: ManaOutput::Fixed(BTreeMap::from([(
                    spellwake_engine::mana::ManaType::Colourless,
                    1.into(),
                )])),
            },
            ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::CounterBased(
                    spellwake_engine::mana::CounterManaYield::RemoveStorageCounters,
                ),
            },
        ],
    );
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(2),
        mana_cost: fixed_cost("{1}")?,
        source_cost: spellwake_engine::activated::SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::PutStorageCounterOnSource],
        ),
    });
    Some(program)
}

/// Colorless mana restricted to colorless Eldrazi spells and abilities.
pub(super) fn eldrazi_temple(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Land")
        || face
            .mana_cost
            .as_deref()
            .is_some_and(|cost| !cost.is_empty())
        || face.oracle_text.as_deref()
            != Some(
                "{T}: Add {C}.\n{T}: Add {C}{C}. Spend this mana only to cast colorless Eldrazi spells or activate abilities of colorless Eldrazi.",
            )
    {
        return None;
    }
    Some(Program::land(
        BTreeSet::new(),
        vec![
            ManaAbility {
                id: ManaAbilityId(0),
                output: ManaOutput::Fixed(BTreeMap::from([(
                    spellwake_engine::mana::ManaType::Colourless,
                    1.into(),
                )])),
            },
            ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::Fixed(BTreeMap::from([(
                    spellwake_engine::mana::ManaType::Colourless,
                    2.into(),
                )]))
                .restricted(
                    spellwake_engine::mana::ManaRestriction::ColorlessEldraziSpellOrAbility,
                ),
            },
        ],
    ))
}

/// Seven-land activation restriction with colorless-spell-only production.
pub(super) fn shrine(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Land")
        || face
            .mana_cost
            .as_deref()
            .is_some_and(|cost| !cost.is_empty())
        || face.oracle_text.as_deref()
            != Some(
                "{T}: Add {C}.\n{T}: Add {C}{C}. Spend this mana only to cast colorless spells. Activate only if you control seven or more lands.",
            )
    {
        return None;
    }
    let normal = ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Fixed(BTreeMap::from([(
            spellwake_engine::mana::ManaType::Colourless,
            1.into(),
        )])),
    };
    let restricted = ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(
            spellwake_engine::mana::ManaType::Colourless,
            2.into(),
        )]))
        .restricted(spellwake_engine::mana::ManaRestriction::ColorlessSpell)
        .require_permanents(
            PermanentSelector {
                required_types: BTreeSet::from([CardType::Land]),
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            },
            7.into(),
        ),
    };
    Some(Program::land(BTreeSet::new(), vec![normal, restricted]))
}

/// The Beacon's life trigger and paid, distinct-color planeswalker mana.
pub(super) fn beacon(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Land")
        || face
            .mana_cost
            .as_deref()
            .is_some_and(|cost| !cost.is_empty())
        || face.oracle_text.as_deref()
            != Some(
                "Whenever you cast a planeswalker spell, you gain 1 life.\n{T}: Add {C}.\n{1}, {T}: Add two mana of different colors. Spend this mana only to cast planeswalker spells.",
            )
    {
        return None;
    }
    let mut program = Program::land(
        BTreeSet::new(),
        vec![
            ManaAbility {
                id: ManaAbilityId(0),
                output: ManaOutput::Fixed(BTreeMap::from([(
                    spellwake_engine::mana::ManaType::Colourless,
                    1.into(),
                )])),
            },
            ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::TwoColors { distinct: true }
                    .restricted(spellwake_engine::mana::ManaRestriction::PlaneswalkerSpell)
                    .paid(fixed_cost("{1}")?),
            },
        ],
    );
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::spell_cast(
            spellwake_engine::program::PlayerRelation::Controller,
            spellwake_engine::program::SpellFilter::of_type(CardType::Planeswalker),
        ),
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
            }],
        ),
    });
    Some(program)
}

pub(super) fn compile(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Land")
        || face
            .mana_cost
            .as_deref()
            .is_some_and(|value| !value.is_empty())
    {
        return None;
    }
    let mut abilities = Vec::new();
    for line in face.oracle_text.as_deref()?.lines() {
        let (clause, condition) = line
            .strip_prefix("{T}: Add ")?
            .split_once(". ")
            .unwrap_or((line.strip_prefix("{T}: Add ")?.strip_suffix('.')?, ""));
        let output = basic_lands::mana_output(clause)?;
        let output = match condition {
            "" => output,
            "Activate only if you control a Swamp." => output.require_permanents(
                PermanentSelector {
                    required_types: BTreeSet::from([CardType::Land]),
                    land_subtype: Some(LandType::Swamp),
                    controller: ControllerSelector::SourceController,
                    ..PermanentSelector::default()
                },
                1.into(),
            ),
            "Activate only if you control five or more lands." => output.require_permanents(
                PermanentSelector {
                    required_types: BTreeSet::from([CardType::Land]),
                    controller: ControllerSelector::SourceController,
                    ..PermanentSelector::default()
                },
                5.into(),
            ),
            _ => return None,
        };
        abilities.push(ManaAbility {
            id: ManaAbilityId(abilities.len().try_into().ok()?),
            output,
        });
    }
    if abilities.iter().all(|ability| {
        !matches!(
            ability.output,
            ManaOutput::Modified {
                modifier: spellwake_engine::mana::ManaOutputModifier::RequirePermanents { .. },
                ..
            }
        )
    }) {
        return None;
    }
    Some(Program::land(BTreeSet::new(), abilities))
}
