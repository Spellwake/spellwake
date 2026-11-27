//! Catalogue wording of cost reductions with representable permanent qualities.
use super::*;
use spellwake_engine::{
    characteristics::{ControllerSelector, PermanentSelector},
    land::LandType,
};

/// Padeem grants artifact hexproof and checks the upkeep comparison twice.
pub(super) fn padeem(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    use spellwake_engine::characteristics::{AbilityChange, StaticPart, StaticParts};
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || lines
            != [
                "Artifacts you control have hexproof.",
                "At the beginning of your upkeep, if you control the artifact with the greatest mana value or tied for the greatest mana value, draw a card.",
            ]
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    army_grants::add_battlefield_ability(
        &mut program,
        PermanentSelector {
            required_types: BTreeSet::from([CardType::Artifact]),
            controller: ControllerSelector::SourceController,
            ..PermanentSelector::default()
        },
        StaticParts(vec![StaticPart::Abilities(AbilityChange {
            granted_keywords: BTreeSet::from([KeywordAbility::Hexproof]),
            ..AbilityChange::default()
        })]),
    )?;
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::step_began(
            spellwake_engine::turn::Step::Upkeep,
            spellwake_engine::program::PlayerRelation::Controller,
        )
        .only_if(spellwake_engine::program::TriggerCondition::ControlsGreatestArtifact),
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::RequireCondition(
                    spellwake_engine::program::InterveningCondition::GreatestArtifact,
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            ],
        ),
    });
    Some(program)
}

/// Historic is checked against the current spell, including legendary and Saga.
pub(super) fn raff_capashen(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    use spellwake_engine::characteristics::StaticAbilityId;
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || without_reminder(face.oracle_text.as_deref()?)?.trim()
            != "Flash\nFlying\nYou may cast historic spells as though they had flash."
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("Flash\nFlying")).ok()?;
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(0),
            permission: spellwake_engine::characteristics::CastPermissionRule::Flash {
                any_player: false,
                matching: spellwake_engine::program::SpellFilter::historic(),
            },
        },
    );
    Some(program)
}

/// Colorless spell timing permission and cast-value source boost.
pub(super) fn skittering_cicada(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    use spellwake_engine::characteristics::StaticAbilityId;
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.oracle_text.as_deref()
            != Some(
                "Flash\nYou may cast colorless spells as though they had flash.\nWhenever you cast a colorless spell, until end of turn, this creature gains trample and gets +X/+X, where X is that spell's mana value.",
            )
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("Flash")).ok()?;
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(0),
            permission: spellwake_engine::characteristics::CastPermissionRule::Flash {
                any_player: false,
                matching: spellwake_engine::program::SpellFilter::colorless(),
            },
        },
    );
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::spell_cast(
            spellwake_engine::program::PlayerRelation::Controller,
            spellwake_engine::program::SpellFilter::colorless(),
        ),
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ApplyContinuous(Box::new(
                spellwake_engine::toolbox::ApplyContinuousSpec::dynamic_boost(
                    spellwake_engine::toolbox::ObjectQuery::Source,
                    spellwake_engine::program::ValueExpr::object_stat(
                        spellwake_engine::program::ValueObjectRef::TriggeringSpell,
                        spellwake_engine::program::ObjectStatKind::ManaValue,
                    ),
                    spellwake_engine::program::ValueExpr::object_stat(
                        spellwake_engine::program::ValueObjectRef::TriggeringSpell,
                        spellwake_engine::program::ObjectStatKind::ManaValue,
                    ),
                    BTreeSet::from([KeywordAbility::Trample]),
                    BTreeSet::new(),
                    spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                ),
            ))],
        ),
    });
    Some(program)
}

/// Dryad adds a land play and adds all basic land types without removing other types.
pub(super) fn dryad_of_elysian_grove(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    use spellwake_engine::characteristics::{
        StaticAbilityId, StaticLandSubtypeOperand, StaticPart, StaticParts, StaticTypeChange,
        SubtypeOperation, TypeOperation,
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.oracle_text.as_deref()
            != Some(
                "You may play an additional land on each of your turns.\nLands you control are every basic land type in addition to their other types.",
            )
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(0),
            permission: spellwake_engine::characteristics::CastPermissionRule::AdditionalLandPlays(
                std::num::NonZeroU8::new(1)?,
            ),
        },
    );
    army_grants::add_battlefield_ability(
        &mut program,
        PermanentSelector {
            required_types: BTreeSet::from([CardType::Land]),
            controller: ControllerSelector::SourceController,
            ..PermanentSelector::default()
        },
        StaticParts(vec![StaticPart::Type(StaticTypeChange {
            land_subtypes: SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(
                BTreeSet::from([
                    LandType::Plains,
                    LandType::Island,
                    LandType::Swamp,
                    LandType::Mountain,
                    LandType::Forest,
                ]),
            )),
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
        })]),
    )?;
    Some(program)
}

/// The Immortal Sun's global loyalty lock, draw trigger, discount and army bonus.
pub(super) fn immortal_sun(card: &InventoryCard, profile: Profile) -> Option<Program> {
    use spellwake_engine::characteristics::{
        NumericExpression, NumericPair, SpellDiscount, StaticAbilityId, StaticPart, StaticParts,
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.type_line.as_deref() != Some("Legendary Artifact")
        || face.mana_cost.as_deref() != Some("{6}")
        || face.oracle_text.as_deref()
            != Some(
                "Players can't activate planeswalkers' loyalty abilities.\nAt the beginning of your draw step, draw an additional card.\nSpells you cast cost {1} less to cast.\nCreatures you control get +1/+1.",
            )
    {
        return None;
    }
    let mut program = Program::artifact(Some(fixed_cost("{6}")?), vec![]);
    program.supertypes.insert(Supertype::Legendary);
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CostModifier {
            id: StaticAbilityId(0),
            modifier: spellwake_engine::characteristics::SpellCostModifier::Discount {
                any_player: false,
                matching: spellwake_engine::program::SpellFilter::any(),
                amount: SpellDiscount::Fixed(std::num::NonZeroU8::new(1)?),
            },
        },
    );
    program.additional_battlefield_abilities.push(
        spellwake_engine::characteristics::StaticContinuousAbility::Restriction {
            id: StaticAbilityId(1),
            rule: spellwake_engine::characteristics::StaticRestriction::PreventLoyaltyActivations,
        },
    );
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
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::step_began(
            spellwake_engine::turn::Step::Draw,
            spellwake_engine::program::PlayerRelation::Controller,
        ),
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            ],
        ),
    });
    Some(program)
}

/// Enchantment-spell discount plus a lookback trigger for controlled enchantments.
pub(super) fn starfield_mystic(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    use spellwake_engine::characteristics::{SpellDiscount, StaticAbilityId};
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current
        || card.layout != "normal"
        || face.name != card.name
        || face.oracle_text.as_deref()
            != Some(
                "Enchantment spells you cast cost {1} less to cast.\nWhenever an enchantment you control is put into a graveyard from the battlefield, put a +1/+1 counter on this creature.",
            )
    {
        return None;
    }
    let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CostModifier {
            id: StaticAbilityId(0),
            modifier: spellwake_engine::characteristics::SpellCostModifier::Discount {
                any_player: false,
                matching: spellwake_engine::program::SpellFilter::of_type(CardType::Enchantment),
                amount: SpellDiscount::Fixed(std::num::NonZeroU8::new(1)?),
            },
        },
    );
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::new(
            spellwake_engine::program::TriggerEvent::Dies {
                subject: spellwake_engine::program::ObjectFilter::any_types(
                    spellwake_engine::program::CardTypeMask::from_card_type(CardType::Enchantment),
                )
                .controlled(),
                mode: spellwake_engine::program::DiesTriggerMode::Permanent,
            },
        ),
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                    spellwake_engine::effects::CounterKind::PlusOne,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ],
        ),
    });
    Some(program)
}

pub(super) fn parse(
    label: &str,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<CastReduction> {
    if profile == Profile::Current
        && let Some(operand) =
            label.strip_prefix("this spell costs {x} less to cast, where x is the ")
    {
        use spellwake_engine::program::CastCostValue;
        let value = match operand.strip_suffix('.')? {
            "total power of creatures you control" => CastCostValue::TotalCreaturePower,
            "greatest power among creatures you control" => CastCostValue::GreatestCreaturePower,
            "total mana value of noncreature artifacts you control" => {
                CastCostValue::NoncreatureArtifactManaValue
            }
            "total mana value of noncreature enchantments you control" => {
                CastCostValue::NoncreatureEnchantmentManaValue
            }
            _ => return None,
        };
        return Some(CastReduction::BattlefieldValue(value));
    }
    if profile == Profile::Current
        && let Some(quantity) = label.strip_prefix("this spell costs {1} less to cast for each ")
    {
        use spellwake_engine::program::CastCostCount;
        let count = match quantity.strip_suffix('.')? {
            "creature on the battlefield" => CastCostCount::BattlefieldCreatures,
            "attacking creature" => CastCostCount::AttackingCreatures,
            "creature you control with a +1/+1 counter on it" => {
                CastCostCount::ControlledCreaturesWithPlusOneCounters
            }
            "creature card in your graveyard" => CastCostCount::OwnGraveyardCreatures,
            "instant and sorcery card in your graveyard" => {
                CastCostCount::OwnGraveyardInstantsOrSorceries
            }
            _ => return None,
        };
        return Some(CastReduction::PerObject(count));
    }
    if label == "undaunted" {
        return Some(CastReduction::Undaunted);
    }
    let mut quality = PermanentSelector {
        controller: ControllerSelector::SourceController,
        ..Default::default()
    };
    match label.strip_prefix("affinity for ")? {
        "artifacts" => {
            quality.required_types.insert(CardType::Artifact);
        }
        "creatures" => {
            quality.required_types.insert(CardType::Creature);
        }
        "enchantments" => {
            quality.required_types.insert(CardType::Enchantment);
        }
        "artifact creatures" => {
            quality
                .required_types
                .extend([CardType::Artifact, CardType::Creature]);
        }
        "snow lands" => {
            quality.required_types.insert(CardType::Land);
            quality.required_supertypes.insert(Supertype::Snow);
        }
        kind @ ("forests" | "swamps" | "islands" | "plains" | "mountains" | "gates" | "towns") => {
            quality.land_subtype = Some(match kind {
                "forests" => LandType::Forest,
                "swamps" => LandType::Swamp,
                "islands" => LandType::Island,
                "plains" => LandType::Plains,
                "mountains" => LandType::Mountain,
                "gates" => LandType::Gate,
                _ => LandType::Town,
            });
        }
        subtype => {
            let literal = match subtype {
                "allies" => "Ally",
                "citizens" => "Citizen",
                "birds" => "Bird",
                "elves" => "Elf",
                "cats" => "Cat",
                "spirits" => "Spirit",
                "frogs" => "Frog",
                "humans" => "Human",
                "lizards" => "Lizard",
                "knights" => "Knight",
                "daleks" => "Dalek",
                "slivers" => "Sliver",
                _ => return None,
            };
            quality.creature_subtype = Some(vocabulary.resolve_label(literal)?);
        }
    };
    Some(CastReduction::Affinity(Box::new(quality)))
}

pub(super) fn conditional(
    label: &str,
    profile: Profile,
) -> Option<spellwake_engine::program::ConditionalCastCost> {
    use spellwake_engine::program::ConditionalCastCost;
    if profile == Profile::Current
        && let Some((number, word)) = label
            .strip_prefix("you may exile ")
            .and_then(|s| {
                s.strip_suffix(" cards from your hand rather than pay this spell's mana cost.")
            })
            .and_then(|s| s.split_once(' '))
    {
        use spellwake_engine::characteristics::Color;
        let color = match word {
            "white" => Color::White,
            "blue" => Color::Blue,
            "black" => Color::Black,
            "red" => Color::Red,
            "green" => Color::Green,
            _ => return None,
        };
        let count = u8::try_from(super::ordinary_text::amount(number)?).ok()?;
        return Some(ConditionalCastCost::ExileHandCards {
            mana: ManaCost::free(),
            count,
            color,
        });
    }
    if profile == Profile::Current
        && label
            == "if you control a commander, you may cast this spell without paying its mana cost."
    {
        return Some(ConditionalCastCost::ControlsCommander(ManaCost::free()));
    }
    let (name, symbols) = label.split_once(' ')?;
    match name {
        "prowl" | "freerunning" | "mayhem" | "web-slinging" if profile >= Profile::GameplayV6 => {
            let cost = fixed_cost(&symbols.to_ascii_uppercase())?;
            Some(match name {
                "prowl" => ConditionalCastCost::Prowl(cost),
                "freerunning" => ConditionalCastCost::Freerunning(cost),
                "mayhem" => ConditionalCastCost::Mayhem(cost),
                _ => ConditionalCastCost::WebSlinging(cost),
            })
        }
        "surge" => Some(ConditionalCastCost::Surge(fixed_cost(
            &symbols.to_ascii_uppercase(),
        )?)),
        "spectacle" => Some(ConditionalCastCost::Spectacle(fixed_cost(
            &symbols.to_ascii_uppercase(),
        )?)),
        _ => None,
    }
}

/// Compile the complete face when a single battlefield discount follows only ordinary keywords.
pub(super) fn battlefield(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::characteristics::{Color, SpellDiscount, StaticAbilityId};
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || card.name != face.name {
        return None;
    }
    let oracle = face.oracle_text.as_deref()?;
    let (prefix, clause) = oracle.rsplit_once('\n').unwrap_or(("", oracle));
    let (subject, reduction) = clause.split_once(" cost ")?;
    let amount = if reduction
        == "{1} less to cast for each creature you control with a +1/+1 counter on it."
    {
        SpellDiscount::ControlledCreaturesWithPlusOneCounters
    } else if let Some(value) = reduction
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix("} less to cast for each experience counter you have."))
    {
        SpellDiscount::PerExperienceCounter(value.parse().ok()?)
    } else {
        let number = reduction
            .strip_prefix('{')?
            .strip_suffix("} less to cast.")?
            .parse::<std::num::NonZeroU8>()
            .ok()?;
        SpellDiscount::Fixed(number)
    };
    let matching = match subject {
        "Creature spells you cast with power 4 or greater" => {
            spellwake_engine::program::SpellFilter::creature()
                .with_power(spellwake_engine::program::PowerFilter::AtLeast(4))
        }
        "Spells you cast" => spellwake_engine::program::SpellFilter::any(),
        subject => match subject.strip_suffix(" spells you cast")? {
            "Colorless" => spellwake_engine::program::SpellFilter::colorless(),
            "White" => spellwake_engine::program::SpellFilter::of_color(Color::White),
            "Blue" => spellwake_engine::program::SpellFilter::of_color(Color::Blue),
            "Black" => spellwake_engine::program::SpellFilter::of_color(Color::Black),
            "Red" => spellwake_engine::program::SpellFilter::of_color(Color::Red),
            "Green" => spellwake_engine::program::SpellFilter::of_color(Color::Green),
            "Artifact" => spellwake_engine::program::SpellFilter::of_type(CardType::Artifact),
            "Enchantment" => spellwake_engine::program::SpellFilter::of_type(CardType::Enchantment),
            "Creature" => spellwake_engine::program::SpellFilter::of_type(CardType::Creature),
            "Aura and Equipment" => spellwake_engine::program::SpellFilter::aura_or_equipment(),
            "Instant and sorcery" => spellwake_engine::program::SpellFilter::instant_or_sorcery(),
            _ => return None,
        },
    };
    let mut program = match face.type_line.as_deref()? {
        "Artifact" if prefix.is_empty() => {
            Program::artifact(Some(fixed_cost(face.mana_cost.as_deref()?)?), vec![])
        }
        "Enchantment" if prefix.is_empty() => {
            Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?))
        }
        _ => ordinary_creature(card, vocabulary, Profile::Current, Some(prefix)).ok()?,
    };
    if program.continuous.is_some() {
        return None;
    }
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CostModifier {
            id: StaticAbilityId(1),
            modifier: spellwake_engine::characteristics::SpellCostModifier::Discount {
                any_player: false,
                matching,
                amount,
            },
        },
    );
    Some(program)
}

/// Compile a permanent's fixed increase to its controller's land play allowance.
pub(super) fn additional_land_plays(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::characteristics::StaticAbilityId;
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || face.name != card.name {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let (prefix, line) = text.trim().rsplit_once('\n').unwrap_or(("", text.trim()));
    let count = match line {
        "You may play an additional land on each of your turns." => 1,
        "You may play two additional lands on each of your turns." => 2,
        _ => return None,
    };
    let mut program = if face.type_line.as_deref() == Some("Enchantment") && prefix.is_empty() {
        Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?))
    } else {
        ordinary_creature(card, vocabulary, Profile::Current, Some(prefix)).ok()?
    };
    if program.continuous.is_some() {
        return None;
    }
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(1),
            permission: spellwake_engine::characteristics::CastPermissionRule::AdditionalLandPlays(
                std::num::NonZeroU8::new(count)?,
            ),
        },
    );
    Some(program)
}

/// Attach the printed hand-size exception to a permanent with an available static ability slot.
pub(super) fn no_maximum_hand_size(program: &mut Program) -> Option<()> {
    use spellwake_engine::characteristics::StaticAbilityId;
    if program.continuous.is_some() {
        return None;
    }
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::DrawRule {
            id: StaticAbilityId(1),
            rule: spellwake_engine::characteristics::StaticDrawRule::NoMaximumHandSize,
        },
    );
    Some(())
}
