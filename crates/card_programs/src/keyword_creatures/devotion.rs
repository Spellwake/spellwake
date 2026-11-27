//! The Gods retain their abilities when devotion removes their creature type.
use super::*;
use spellwake_engine::{
    activated::SourceCost,
    characteristics::{
        AbilityChange, Color, ControllerSelector, PermanentSelector, StaticPart, StaticParts,
        StaticTypeChange, SubtypeOperation, TypeOperation,
    },
    program::{BodyTarget, DamageGroup, ExecutableBody, Instruction, TriggerGrouping},
};
use spellwake_engine::{
    characteristics::{PowerToughness, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec},
    program::{ObjectFilter, PlayerRelation, ResolvingPlayer},
};
pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let name = face.name.split(',').next()?;
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut lines = text.trim().lines().map(str::trim);
    if lines.next()? != "Indestructible" {
        return None;
    }
    let printed = lines.next()?.strip_prefix("As long as your devotion to ")?;
    let (color, subject) = printed.split_once(" is less than five, ")?;
    if subject.strip_suffix(" isn't a creature.")? != name {
        return None;
    }
    let color = match color {
        "white" => Color::White,
        "blue" => Color::Blue,
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        _ => return None,
    };
    let mut program =
        ordinary_creature(card, vocabulary, Profile::Current, Some("Indestructible")).ok()?;
    let selector = PermanentSelector {
        controller: ControllerSelector::SourceItself,
        devotion_below: Some((color, 5)),
        ..Default::default()
    };
    army_grants::add_battlefield_ability(
        &mut program,
        selector,
        StaticParts(vec![StaticPart::Type(StaticTypeChange {
            types: TypeOperation::Remove([CardType::Creature].into()),
            supertypes: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
            land_subtypes: SubtypeOperation::Keep,
        })]),
    )?;
    for line in lines {
        if line == "Other creatures you control have vigilance." {
            let selector = PermanentSelector {
                controller: ControllerSelector::SourceController,
                required_types: [CardType::Creature].into(),
                exclude_source: true,
                ..Default::default()
            };
            army_grants::add_battlefield_ability(
                &mut program,
                selector,
                StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
                    [KeywordAbility::Vigilance],
                ))]),
            )?;
        } else if line
            .strip_prefix("Whenever another creature you control enters, ")
            .and_then(|s| s.strip_prefix(name))
            == Some(" deals 2 damage to each opponent.")
        {
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(program.triggered.len().try_into().ok()?),
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::creature()
                        .controlled()
                        .other_than_ability_source(),
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                                spellwake_engine::program::DamageRecipientsRef::Group(
                                    DamageGroup::Opponents,
                                ),
                            )),
                            spellwake_engine::program::ValueExpr::literal(2),
                            false,
                        ),
                    ],
                ),
            });
        } else if line
            == "Whenever you gain life, put a +1/+1 counter on target creature or enchantment you control."
        {
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(program.triggered.len().try_into().ok()?),
                condition: spellwake_engine::program::TriggerSpec::life_changed(
                    spellwake_engine::life::LifeChangeKind::Gain,
                    PlayerRelation::Controller,
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::filtered_permanent(
                        ObjectFilter::creature_or_enchantment().controlled(),
                    ),
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                        spellwake_engine::effects::CounterKind::PlusOne,
                        spellwake_engine::program::ValueExpr::literal(1),
                    )],
                ),
            });
        } else if line
            == "Whenever another creature you control dies, you may pay 2 life. If you do, draw a card."
        {
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(program.triggered.len().try_into().ok()?),
                condition: spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::creature()
                        .controlled()
                        .other_than_ability_source(),
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 2,
                        },
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::pay_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            spellwake_engine::program::ValueExpr::literal(2),
                        ),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                            spellwake_engine::program::ValueExpr::Literal(1.into()),
                        ),
                    ],
                ),
            });
        } else {
            let (cost, effect) = line.split_once(": ")?;
            let (mana, source_cost) = if let Some(price) =
                cost.strip_suffix(", Sacrifice another creature")
            {
                (
                    price,
                    SourceCost::SacrificeSelected {
                        filter: spellwake_engine::activated::SelectedSacrificeFilter::OtherCreature,
                        count: 1,
                        tap_source: false,
                        from_graveyard: false,
                    },
                )
            } else {
                (cost, SourceCost::None)
            };
            if let Some((target, part)) = match effect {
                "Another target creature gains lifelink until end of turn." => Some((
                    BodyTarget::filtered_permanent(
                        ObjectFilter::creature().other_than_ability_source(),
                    ),
                    ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                        KeywordAbility::Lifelink,
                    ])),
                )),
                "Target creature gets -2/-1 until end of turn." => Some((
                    BodyTarget::SINGLE_CREATURE,
                    ResolvedPartSpec::ModifyPT(PowerToughness {
                        power: (-2).into(),
                        toughness: (-1).into(),
                    }),
                )),
                _ => None,
            } {
                program.activated.push(ActivatedAbility {
                    id: ActivatedAbilityId(program.activated.len().try_into().ok()?),
                    mana_cost: fixed_cost(mana)?,
                    source_cost,
                    body: ExecutableBody::new(
                        target,
                        vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            ResolvedPartsSpec(vec![part]),
                            ResolvedDuration::UntilEndOfTurn,
                        )],
                    ),
                });
                continue;
            }
            let effect = effect.strip_suffix('.')?;
            let instruction = if let Some(template) = effect.strip_prefix("Create ") {
                ordinary_text::create_tokens(template, vocabulary)?
            } else {
                spells::ordinary_effects::group_modifier(effect)?
            };
            program.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(program.activated.len().try_into().ok()?),
                mana_cost: fixed_cost(mana)?,
                source_cost,
                body: ExecutableBody::new(BodyTarget::None, vec![instruction]),
            });
        }
    }
    Some(program)
}
