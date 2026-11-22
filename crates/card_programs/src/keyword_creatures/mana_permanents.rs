//! Printed tap-for-mana abilities on ordinary creatures and artifacts.
use super::*;
use spellwake_engine::mana::{ManaAbility, ManaAbilityId};
use spellwake_engine::program::ProgramKind;

/// Full Great Henge: self-discount, mana/life and nontoken creature entry.
pub(super) fn great_henge(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Legendary Artifact")
        || face.mana_cost.as_deref() != Some("{7}{G}{G}")
        || face.oracle_text.as_deref()
            != Some(
                "This spell costs {X} less to cast, where X is the greatest power among creatures you control.\n{T}: Add {G}{G}. You gain 2 life.\nWhenever a nontoken creature you control enters, put a +1/+1 counter on it and draw a card.",
            )
    {
        return None;
    }
    let mut program = Program::artifact(Some(fixed_cost("{7}{G}{G}")?), vec![]);
    program.supertypes.insert(Supertype::Legendary);
    program
        .cast_reductions
        .push(CastReduction::BattlefieldValue(
            spellwake_engine::program::CastCostValue::GreatestCreaturePower,
        ));
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: spellwake_engine::mana::ManaOutput::Fixed(BTreeMap::from([(
            spellwake_engine::mana::ManaType::Green,
            2.into(),
        )]))
        .side_effect(spellwake_engine::mana::ManaSideEffect::GainLife(2.into())),
    });
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::creature()
                .controlled()
                .nontoken(),
        ),
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::PutCounterOnRelatedPermanent,
                Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                },
            ],
        ),
    });
    Some(program)
}

/// The activating controller chooses a recipient; that player chooses the color.
pub(super) fn spectral_searchlight(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Artifact")
        || face.mana_cost.as_deref() != Some("{3}")
        || face.oracle_text.as_deref()
            != Some("{T}: Choose a player. That player adds one mana of any color they choose.")
    {
        return None;
    }
    let mut program = Program::artifact(Some(fixed_cost("{3}")?), vec![]);
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: spellwake_engine::mana::ManaOutput::SpecialColor(
            spellwake_engine::mana::SpecialColorYield::ChosenPlayerColor,
        ),
    });
    Some(program)
}

/// Monument's creature bonus, additional colorless mana and cast life trigger.
pub(super) fn forsaken_monument(card: &InventoryCard, profile: Profile) -> Option<Program> {
    use spellwake_engine::characteristics::{
        ColorFilter, ControllerSelector, NumericExpression, NumericPair, PermanentSelector,
        StaticPart, StaticParts,
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Legendary Artifact")
        || face.mana_cost.as_deref() != Some("{5}")
        || face.oracle_text.as_deref()
            != Some(
                "Colorless creatures you control get +2/+2.\nWhenever you tap a permanent for {C}, add an additional {C}.\nWhenever you cast a colorless spell, you gain 2 life.",
            )
    {
        return None;
    }
    let mut program = Program::artifact(Some(fixed_cost("{5}")?), vec![]);
    program.supertypes.insert(Supertype::Legendary);
    army_grants::add_battlefield_ability(
        &mut program,
        PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            controller: ControllerSelector::SourceController,
            color: ColorFilter::Colorless,
            ..PermanentSelector::default()
        },
        StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(2.into()),
            toughness: NumericExpression::Literal(2.into()),
        })]),
    )?;
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: spellwake_engine::mana::ManaOutput::Triggered(
            spellwake_engine::mana::TriggeredManaRule::PermanentTappedForColorless,
        ),
    });
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::spell_cast(
            spellwake_engine::program::PlayerRelation::Controller,
            spellwake_engine::program::SpellFilter::colorless(),
        ),
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Gain(2.into()),
            }],
        ),
    });
    Some(program)
}

/// Full Wake: dynamic army bonus and a separately triggered mana ability.
pub(super) fn miraris_wake(card: &InventoryCard, profile: Profile) -> Option<Program> {
    use spellwake_engine::characteristics::{
        ControllerSelector, NumericExpression, NumericPair, PermanentSelector, StaticPart,
        StaticParts,
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Enchantment")
        || face.mana_cost.as_deref() != Some("{3}{G}{W}")
        || face.oracle_text.as_deref()
            != Some(
                "Creatures you control get +1/+1.\nWhenever you tap a land for mana, add one mana of any type that land produced.",
            )
    {
        return None;
    }
    let mut program = Program::enchantment(Some(fixed_cost("{3}{G}{W}")?));
    army_grants::add_battlefield_ability(
        &mut program,
        PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            controller: ControllerSelector::SourceController,
            ..PermanentSelector::default()
        },
        StaticParts(vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(1.into()),
            toughness: NumericExpression::Literal(1.into()),
        })]),
    )?;
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: spellwake_engine::mana::ManaOutput::Triggered(
            spellwake_engine::mana::TriggeredManaRule::LandTappedSameType,
        ),
    });
    Some(program)
}

/// CR 605.1a gives an activation that mills as a cost an ordinary stack entry.
pub(super) fn deranged_assistant(
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
        || without_reminder(face.oracle_text.as_deref()?)?.trim() != "{T}, Mill a card: Add {C}."
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: spellwake_engine::activated::SourceCost::Mill {
            count: std::num::NonZeroU8::MIN,
            tap_source: true,
        },
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AddMana(
                spellwake_engine::program::EffectManaYield::Fixed(
                    [(spellwake_engine::mana::ManaType::Colourless, 1.into())].into(),
                ),
            )],
        ),
    });
    Some(program)
}

/// The Weaponsmith's restricted mana and named-card search share normal mana/search paths.
pub(super) fn renowned_weaponsmith(
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
                "{T}: Add {C}{C}. Spend this mana only to cast artifact spells or activate abilities of artifacts.\n{U}, {T}: Search your library for a card named Heart-Piercer Bow or Vial of Dragonfire, reveal it, put it into your hand, then shuffle.",
            )
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: spellwake_engine::mana::ManaOutput::Fixed(BTreeMap::from([(
            spellwake_engine::mana::ManaType::Colourless,
            2.into(),
        )]))
        .restricted(spellwake_engine::mana::ManaRestriction::ArtifactSpellOrAbility),
    });
    let names = ["Heart-Piercer Bow", "Vial of Dragonfire"]
        .map(|name| spellwake_contracts::names::EnglishCardName::new(name.to_owned()));
    let [first, second] = names;
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: fixed_cost("{U}")?,
        source_cost: spellwake_engine::activated::SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::SearchLibrary {
                filter: Box::new(spellwake_engine::program::LibrarySearch::to_hand(
                    spellwake_engine::program::SearchCardMatcher::AnyOfNames(vec![
                        first.ok()?,
                        second.ok()?,
                    ]),
                )),
            }],
        ),
    });
    Some(program)
}

/// The name comparison is a literal, so copying or renaming the source does not rewrite it.
pub(super) fn bonders_ornament(card: &InventoryCard, profile: Profile) -> Option<Program> {
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
                "{T}: Add one mana of any color.\n{4}, {T}: Each player who controls a permanent named Bonder's Ornament draws a card.",
            )
    {
        return None;
    }
    let mut program = Program::artifact(Some(fixed_cost("{3}")?), vec![]);
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: basic_lands::mana_output("one mana of any color")?,
    });
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: fixed_cost("{4}")?,
        source_cost: spellwake_engine::activated::SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawPlayersControllingName(
                spellwake_contracts::names::EnglishCardName::new("Bonder's Ornament".to_owned())
                    .ok()?,
            )],
        ),
    });
    Some(program)
}

/// Selected creature sacrifice is a mana cost and creates no stack entry.
pub(super) fn ashnods_altar(card: &InventoryCard, profile: Profile) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Artifact")
        || face.mana_cost.as_deref() != Some("{3}")
        || face.oracle_text.as_deref() != Some("Sacrifice a creature: Add {C}{C}.")
    {
        return None;
    }
    let mut program = Program::artifact(Some(fixed_cost("{3}")?), vec![]);
    program.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: spellwake_engine::mana::ManaOutput::Fixed(BTreeMap::from([(
            spellwake_engine::mana::ManaType::Colourless,
            2.into(),
        )]))
        .sacrifice(spellwake_engine::mana::ManaSacrificeCost::ControlledCreature),
    });
    Some(program)
}

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let types = face.type_line.as_deref()?.split(" — ").next()?;
    let creature = types.split_whitespace().any(|t| t == "Creature");
    if card.layout != "normal"
        || face.name != card.name
        || !(creature
            || matches!(
                face.type_line.as_deref(),
                Some("Artifact" | "Legendary Artifact" | "Snow Artifact")
            ))
    {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut mana = Vec::new();
    let mut remaining = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(output) = line
            .strip_prefix("{T}: Add ")
            .and_then(|s| s.strip_suffix('.'))
        {
            mana.push(ManaAbility {
                id: ManaAbilityId(mana.len().try_into().ok()?),
                output: basic_lands::mana_output(output)?,
            });
        } else if line == "Whenever a land with the same name as the exiled card is tapped for mana, its controller adds one mana of any type that land produced." {
            mana.push(ManaAbility {id:ManaAbilityId(mana.len().try_into().ok()?),output:spellwake_engine::mana::ManaOutput::Triggered(spellwake_engine::mana::TriggeredManaRule::ImprintedLandTappedSameType)});
        } else if let Some(output) = line.strip_prefix("Tap an untapped artifact you control: Add ").and_then(|s|s.strip_suffix('.')) {
            let spellwake_engine::mana::ManaOutput::Fixed(produced) = basic_lands::mana_output(output)? else { return None; };
            mana.push(ManaAbility { id: ManaAbilityId(mana.len().try_into().ok()?), output: spellwake_engine::mana::ManaOutput::Fixed(produced).tap_controlled_artifact() });
        } else if let Some(price) = line.strip_suffix(", {T}: Add X mana in any combination of colors, where X is the greatest power among creatures you control.") {
            mana.push(ManaAbility { id: ManaAbilityId(mana.len().try_into().ok()?), output: spellwake_engine::mana::ManaOutput::Dynamic(spellwake_engine::mana::DynamicManaYield::GreatestCreaturePower).paid(fixed_cost(price)?) });
        } else if let Some((kind, quantity)) = line.strip_prefix("Sacrifice a ").and_then(|line|line.strip_suffix('.')).and_then(|line| line.split_once(": Add "))
            && let Some(subtype) = vocabulary.resolve_label(kind) {
            let output = basic_lands::mana_output(quantity)?;
            if !matches!(output, spellwake_engine::mana::ManaOutput::Fixed(_) | spellwake_engine::mana::ManaOutput::ChooseColor {..}) {return None;}
            mana.push(ManaAbility { id: ManaAbilityId(mana.len().try_into().ok()?),
                output: output.sacrifice(spellwake_engine::mana::ManaSacrificeCost::ControlledSubtype(subtype)) });
        } else {
            remaining.push(line);
        }
    }
    if mana.is_empty() {
        return None;
    }
    let rest = remaining.join("\n");
    let draw = (!creature && remaining.len() == 1)
        .then(|| draw_activation(&rest))
        .flatten();
    let mut program = if creature {
        ordinary_creature(card, vocabulary, Profile::Current, Some(&rest))
            .ok()
            .or_else(|| source_boost::compile(card, vocabulary, Some(&rest)))
    } else if matches!(
        rest.as_str(),
        "" | "Indestructible" | "This artifact enters tapped." | "You have no maximum hand size."
    ) || draw.is_some()
    {
        let mut artifact = Program::artifact(Some(fixed_cost(face.mana_cost.as_deref()?)?), vec![]);
        if rest == "You have no maximum hand size." {
            reductions::no_maximum_hand_size(&mut artifact)?;
        }
        if rest == "Indestructible" {
            artifact
                .printed_keywords
                .insert(KeywordAbility::Indestructible);
        }
        if rest == "This artifact enters tapped."
            && let ProgramKind::Permanent(definition) = &mut artifact.kind
        {
            definition.entry_tap = Some(spellwake_engine::land::EntryTap::Always);
        }
        if let Some(ability) = draw {
            artifact.activated.push(ability);
        }
        if types.starts_with("Legendary ") {
            artifact.supertypes.insert(Supertype::Legendary);
        }
        if types.starts_with("Snow ") {
            artifact.supertypes.insert(Supertype::Snow);
        }
        Some(artifact)
    } else {
        None
    }
    .or_else(|| {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(rest);
        ordinary_triggers::compile(&reduced, vocabulary)
    })?;
    program.mana = mana;
    Some(program)
}

fn draw_activation(line: &str) -> Option<ActivatedAbility> {
    let (cost, action) = line.split_once(", {T}, Sacrifice this artifact: ")?;
    let count = match action {
        "Draw a card." => 1_i64,
        "Draw two cards." => 2,
        "Draw three cards." => 3,
        _ => return None,
    };
    Some(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: fixed_cost(cost)?,
        source_cost: spellwake_engine::activated::SourceCost::TapSacrifice,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::Literal(count.into()),
            }],
        ),
    })
}
