//! Self-cast triggers that may return a targeted creature from the controller's graveyard.
use super::*;
use spellwake_engine::program::{ObjectFilter, ResolvingPlayer, TriggerGrouping};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if profile != Profile::Current || card.layout != "normal" || card.name != face.name {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    if text.trim()
        == "When this creature dies, you draw X cards and you lose X life, where X is the number of creature cards in target player's graveyard."
    {
        let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec {
                            bind_moved: Some(spellwake_engine::toolbox::BindingId(0)),
                            ..spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                                            spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                                        ),
                                    )),
                                    spellwake_engine::program::ObjectFilter::creature(),
                                ),
                                spellwake_engine::toolbox::ZoneDest::None,
                            )
                        },
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::BoundCount {
                            binding: spellwake_engine::toolbox::BindingId(0),
                            filter: None,
                        },
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::BoundCount {
                            binding: spellwake_engine::toolbox::BindingId(0),
                            filter: None,
                        },
                    ),
                ],
            ),
        });
        program.validate(vocabulary).ok()?;
        return Some(program);
    }
    if text.trim()
        == "All Nightmares get +1/+1.\n{B}{B}{B}, Pay 3 life: Put target creature card from a graveyard onto the battlefield under your control. That creature is black and is a Nightmare in addition to its other creature types.\nWhen Chainer leaves the battlefield, exile all Nightmares."
    {
        use spellwake_engine::characteristics::{
            ControllerSelector, NumericExpression, NumericPair, PermanentSelector, StaticAbilityId,
            StaticContinuousAbility, StaticPart, StaticParts,
        };
        use spellwake_engine::program::{ObjectFilter, PublicZoneDestination};
        let nightmare = vocabulary.resolve_label("Nightmare")?;
        let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
        program.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(1),
            selector: PermanentSelector {
                controller: ControllerSelector::Any,
                creature_subtype: Some(nightmare),
                ..PermanentSelector::default()
            },
            parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(1.into()),
                toughness: NumericExpression::Literal(1.into()),
            })]),
        });
        program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: fixed_cost("{B}{B}{B}")?,
            source_cost: spellwake_engine::activated::SourceCost::PayLife {
                amount: spellwake_engine::activated::LifeCostAmount::Fixed(3),
                tap_source: false,
                sacrifice: None,
            },
            body: ExecutableBody::new(
                BodyTarget::AnyGraveyardCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec {
                            bind_moved: Some(spellwake_engine::toolbox::BindingId(0)),
                            ..spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ZoneDest::Battlefield {
                                    controller: spellwake_engine::toolbox::ZoneController::Player(
                                        Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                spellwake_engine::program::ResolvingPlayer::Controller,
                                            ),
                                        ),
                                    ),
                                    tapped: false,
                                    attacking: false,
                                    transformed: false,
                                    face_down: None,
                                    entering_counters: Vec::new(),
                                    attach_to: None,
                                },
                            )
                        },
                    ),
                    Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::resolved_parts(
                            spellwake_engine::toolbox::ObjectQuery::Bound(
                                spellwake_engine::toolbox::BindingId(0),
                            ),
                            spellwake_engine::characteristics::ResolvedPartsSpec(vec![
                                spellwake_engine::characteristics::ResolvedPartSpec::Type(
                                    spellwake_engine::characteristics::TypeChangeSpec {
                                        types: spellwake_engine::characteristics::TypeOperation::Keep,
                                        supertypes: spellwake_engine::characteristics::TypeOperation::Keep,
                                        subtypes: spellwake_engine::characteristics::SubtypeOperation::Add(
                                            spellwake_engine::characteristics::InstructionSubtypeOperand::Literal(
                                                [nightmare].into(),
                                            ),
                                        ),
                                        land_subtypes: spellwake_engine::characteristics::SubtypeOperation::Keep,
                                    },
                                ),
                                spellwake_engine::characteristics::ResolvedPartSpec::Color(
                                    spellwake_engine::characteristics::ColorOperation::Set(
                                        [spellwake_engine::characteristics::Color::Black].into(),
                                    ),
                                ),
                            ]),
                            spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration,
                        ),
                    )),
                ],
            ),
        });
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::leaves_battlefield(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group(
                        ObjectFilter::any().with_subtype(nightmare),
                        PublicZoneDestination::Exile,
                    ),
                ],
            ),
        });
        program.validate(vocabulary).ok()?;
        return Some(program);
    }
    if text.trim()
        == "This spell costs {X} less to cast, where X is the total mana value of noncreature artifacts you control.\nSacrifice two artifacts: Return this card from your graveyard to your hand."
    {
        let reduction = "This spell costs {X} less to cast, where X is the total mana value of noncreature artifacts you control.";
        let mut program = ordinary_creature(card, vocabulary, profile, Some(reduction)).ok()?;
        program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: ManaCost::free(),
            source_cost: spellwake_engine::activated::SourceCost::SacrificeSelected {
                filter: spellwake_engine::activated::SelectedSacrificeFilter::Artifact,
                count: 2,
                tap_source: false,
                from_graveyard: true,
            },
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_source_graveyard_to_hand()],
            ),
        });
        program.validate(vocabulary).ok()?;
        return Some(program);
    }
    if text.trim() == "{1}{B}: Return this card from your graveyard to the battlefield tapped." {
        let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
        program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: fixed_cost("{1}{B}")?,
            source_cost: spellwake_engine::activated::SourceCost::GraveyardSelf { unearth: false },
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_source_graveyard_to_battlefield(true)],
            ),
        });
        program.validate(vocabulary).ok()?;
        return Some(program);
    }
    if text.trim()
        == "At the beginning of your upkeep, you may return this card from your graveyard to your hand."
    {
        let mut program = ordinary_creature(card, vocabulary, profile, Some("")).ok()?;
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::step_began(
                spellwake_engine::turn::Step::Upkeep,
                spellwake_engine::program::PlayerRelation::Controller,
            )
            .in_zone(spellwake_engine::program::TriggerZone::Graveyard),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 1,
                    },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::return_source_graveyard_to_hand(),
                ],
            ),
        });
        program.validate(vocabulary).ok()?;
        return Some(program);
    }
    let mut found = false;
    let mut remaining = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line
            == "When you cast this spell, you may return target creature card from your graveyard to the battlefield."
            && !found
        {
            found = true;
        } else {
            remaining.push(line);
        }
    }
    if !found {
        return None;
    }
    let mut program =
        ordinary_creature(card, vocabulary, profile, Some(&remaining.join("\n"))).ok()?;
    let id = TriggeredAbilityId(
        program
            .triggered
            .iter()
            .map(|ability| ability.id.0)
            .max()
            .map_or(Some(0), |id| id.checked_add(1))?,
    );
    program.triggered.push(TriggeredAbility {
        id,
        condition: spellwake_engine::program::TriggerSpec::self_cast(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature()),
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(false),
            ],
        ),
    });
    program.validate(vocabulary).ok()?;
    Some(program)
}

/// Exact target and effect of The Eldest Reborn's third chapter.
pub(super) fn eldest_reborn_third_chapter() -> ExecutableBody {
    ExecutableBody::new(
        BodyTarget::AnyGraveyardCreatureOrPlaneswalker,
        vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(
                false,
            ),
        ],
    )
}
