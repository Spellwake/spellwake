//! Oracle clauses composed from the current engine's existing program vocabulary.
use super::{Profile, fixed_cost, ordinary_creature, ordinary_text};
use spellwake_content::InventoryCard;
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    attachment::EnchantRestriction,
    characteristics::{
        AbilityChange, AbilityRemoval, BlockingRestriction, Color, ColorOperation,
        ControllerSelector, InstructionSubtypeOperand, KeywordAbility, NumericExpression,
        NumericPair, PermanentSelector, PowerToughness, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec, SpellDiscount, StaticAbilityId, StaticColorOperand, StaticPart,
        StaticParts, StaticSubtypeOperand, StaticTypeChange, SubtypeOperation, TypeChangeSpec,
        TypeOperation,
    },
    effects::CounterKind,
    life::LifeChangeKind,
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    program::{
        AffectedPlayers, AttackRequirement, BodyTarget, CardType, CardTypeMask, Comparison,
        EffectSubject, ExecutableBody, Instruction, LibraryPlacement, ObjectFilter,
        PermanentTypeFilter, PlayerRelation, Program, ProgramKind, ResolutionCondition,
        ResolvingPlayer, SpellModes, TriggerGrouping, TriggerSpec, TriggeredAbility,
        TriggeredAbilityId, ValueExpr,
    },
    turn::Step,
};
use std::collections::BTreeSet;

fn literal(value: i64) -> ValueExpr {
    spellwake_engine::program::ValueExpr::Literal(value.into())
}

fn skip_unless_present(value: ValueExpr, destination: usize) -> Instruction {
    Instruction::BranchCondition {
        condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
            Box::new(ResolutionCondition {
                left: value,
                comparison: Comparison::Greater,
                right: spellwake_engine::program::ValueExpr::Literal(0.into()),
            }),
        )),
        destination,
    }
}

fn controlled_creature_pair(second: BodyTarget, effect: Instruction) -> Vec<Instruction> {
    let clause = |target| {
        Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
            target,
            vec![Instruction::MutateObject(Box::new(
                spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
            ))],
        )))
    };
    vec![
        clause(BodyTarget::FilteredPermanent(
            ObjectFilter::creature().controlled(),
        )),
        clause(second),
        effect,
    ]
}

fn optional_tap_change() -> Vec<Instruction> {
    vec![
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 1,
        },
        crate::keyword_creatures::oracle_lowering::toolbox_emit::toggle_targets_tapped(),
    ]
}

fn modify_target(parts: Vec<ResolvedPartSpec>) -> Instruction {
    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
        spellwake_engine::toolbox::ObjectQuery::Targets,
        ResolvedPartsSpec(parts),
        ResolvedDuration::UntilEndOfTurn,
    )
}

fn stats(power: i64, toughness: i64) -> PowerToughness {
    PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    }
}

fn spell(name: &str, rules: &str, vocabulary: &CreatureTypeVocabulary) -> Option<ExecutableBody> {
    use BodyTarget::{None as NoTarget, SingleAnyTarget, SingleCreature, SinglePlayer};
    use Instruction::*;
    let draw =
        |n| crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(literal(n));
    let discard = |player, amount| {
        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
            AffectedPlayers::One(player),
            amount,
        )))
    };
    let (target, instructions) = match rules {
        "Until end of turn, target creature gets +6/+6 and gains trample, and all creatures able to block it this turn do so." => {
            (
                SingleCreature,
                vec![
                    modify_target(vec![
                        ResolvedPartSpec::ModifyPT(stats(6, 6)),
                        ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                            KeywordAbility::Trample,
                        ])),
                    ]),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::TargetObject, spellwake_engine::program::CombatActionKind::MustBeBlocked),
                ],
            )
        }

        "As an additional cost to cast this spell, tap any number of untapped creatures you control.\nBurn at the Stake deals damage to any target equal to three times the number of creatures tapped this way." => {
            (
                SingleAnyTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::event(
                            spellwake_engine::program::EventValueKind::PaidCreatureTaps,
                        )
                        .scaled(i64::from(3)),
                        false,
                    ),
                ],
            )
        }

        "Target opponent reveals their hand. You choose a card from it with mana value 4 or greater and exile that card." => {
            (
                BodyTarget::SingleOpponent,
                vec![Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::choose_revealed_hand_card(
                        4,
                        false,
                        None,
                        std::num::NonZeroU8::MIN,
                        spellwake_engine::program::ChosenHandAction::Exile,
                    ),
                ))],
            )
        }
        "Return target creature card from your graveyard to your hand. Target player sacrifices a creature of their choice." =>
        {
            let clause = |target, instruction| {
                FixedTargetClause(Box::new(ExecutableBody::new(target, vec![instruction])))
            };
            (
                NoTarget,
                vec![
                    clause(
                        BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature()),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_hand(),
                    ),
                    clause(
                        SinglePlayer,
                        Instruction::ZoneOp(Box::new(
                            spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                                AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                                literal(1),
                                PermanentTypeFilter {
                                    any_types: BTreeSet::new(),
                                    required_types: [CardType::Creature].into(),
                                    forbidden_types: BTreeSet::new(),
                                    object_filter: None,
                                },
                            ),
                        )),
                    ),
                ],
            )
        }

        "Destroy target creature with flying and all Equipment attached to that creature." => (
            BodyTarget::FilteredPermanent(
                ObjectFilter::creature().with_keyword(KeywordAbility::Flying),
            ),
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Union(vec![
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                ObjectFilter::artifact()
                                    .with_artifact_subtype(
                                        spellwake_engine::program::ArtifactSubtype::Equipment,
                                    )
                                    .with_flags(
                                        spellwake_engine::program::ObjectFilterFlags::ATTACHED_TO_TARGET,
                                    ),
                            ),
                        ]),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: true,
                            },
                        },
                    ),
                ),
            ],
        ),

        "Return target creature card from your graveyard to the battlefield. If it's an Angel, put two +1/+1 counters on it." => {
            let angel = vocabulary.resolve_label("Angel")?;
            (
                BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature()),
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
                                                ResolvingPlayer::Controller,
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
                    Instruction::BranchCondition {
                        condition: Box::new(spellwake_engine::toolbox::ConditionExpr::AnyMatch {
                            query: Box::new(spellwake_engine::toolbox::ObjectQuery::Bound(
                                spellwake_engine::toolbox::BindingId(0),
                            )),
                            filter: Box::new(ObjectFilter::any().with_subtype(angel)),
                        }),
                        destination: 3,
                    },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                        spellwake_engine::toolbox::ObjectQuery::Bound(
                            spellwake_engine::toolbox::BindingId(0),
                        ),
                        spellwake_engine::effects::CounterKind::PlusOne,
                        literal(2),
                    ),
                ],
            )
        }

        "Bonfire of the Damned deals X damage to target player or planeswalker and each creature that player or that planeswalker's controller controls." => {
            (
                BodyTarget::SinglePlayerOrPlaneswalker,
                vec![Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec::target_controllers_creatures(
                        spellwake_engine::program::ValueExpr::chosen_x(),
                        true,
                        false,
                    ),
                ))],
            )
        }
        "Aggravate deals 1 damage to each creature target player controls. Each creature dealt damage this way attacks this turn if able." => {
            (
                SinglePlayer,
                vec![Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec::target_controllers_creatures(
                        literal(1),
                        false,
                        true,
                    ),
                ))],
            )
        }

        "Target player loses X life and you gain X life, where X is the greatest power among creatures you control." => {
            (
                SinglePlayer,
                vec![Instruction::AdjustLife(Box::new(
                    spellwake_engine::toolbox::AdjustLifeSpec::drain(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::TargetPlayer,
                        ),
                        spellwake_engine::program::ValueExpr::aggregate(
                            spellwake_engine::program::ObjectFilter::creature().controlled(),
                            spellwake_engine::program::ObjectStatKind::Power,
                            spellwake_engine::program::StatAggregate::Max,
                        ),
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        false,
                    ),
                ))],
            )
        }

        "Draw three cards, then put a card from your hand on the bottom of your library." => {
            (NoTarget, vec![
                draw(3),
                Instruction::ZoneOp(Box::new(
                    spellwake_engine::toolbox::ZoneOpSpec::simple(
                        spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                            zone: spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                            )),
                            filter: Box::new(ObjectFilter::any()),
                            slice: spellwake_engine::toolbox::QuerySlice::All,
                        },
                        spellwake_engine::toolbox::ZoneSelection::Prompt {
                            chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            min: Box::new(ValueExpr::literal(1)),
                            max: Box::new(ValueExpr::literal(1)),
                            constraint: None,
                            category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                            hidden: true,
                        },
                        spellwake_engine::toolbox::ZoneDest::Library {
                            position: spellwake_engine::toolbox::LibraryOrderPos::Bottom,
                        },
                    ),
                )),
            ])
        }

        "Create three 1/1 red Human creature tokens with haste. Sacrifice those tokens at the beginning of the next end step." => {
            (
                NoTarget,
                vec![
                    ordinary_text::create_tokens(
                        "three 1/1 red Human creature tokens with haste",
                        vocabulary,
                    )?,
                    Instruction::ScheduleDelayedStepTrigger { step: spellwake_engine::turn::Step::End, own_step_only: false, body: Box::new(ExecutableBody::new(BodyTarget::None, vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::CreatedTokens, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice })))])) },
                ],
            )
        }

        "Put target artifact, creature, or enchantment on the bottom of its owner's library." => (
            BodyTarget::FilteredPermanent(ObjectFilter::any_types(
                CardTypeMask::ARTIFACT
                    .union(CardTypeMask::CREATURE)
                    .union(CardTypeMask::ENCHANTMENT),
            )),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_target_in_library(LibraryPlacement::Bottom)],
        ),
        "Put all creatures on the bottom of their owners' libraries." => (
            NoTarget,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_group_on_library_bottom(ObjectFilter::creature())],
        ),

        "Target creature gains flying until end of turn. Prevent all damage that would be dealt to that creature this turn." => {
            (
                SingleCreature,
                vec![
                    modify_target(vec![ResolvedPartSpec::Abilities(
                        AbilityChange::grant_keywords([KeywordAbility::Flying]),
                    )]),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                        spellwake_engine::damage::TurnPreventionSpec {
                            combat_only: false,
                            source: spellwake_engine::damage::PreventionSourceSpec::Any,
                            recipient: spellwake_engine::damage::PreventionRecipientSpec::Target,
                            both_directions: false,
                        },
                    ),
                ],
            )
        }
        "Prevent all combat damage that would be dealt by creatures other than target creature this turn." => {
            (
                SingleCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                        spellwake_engine::damage::TurnPreventionSpec {
                            combat_only: true,
                            source:
                                spellwake_engine::damage::PreventionSourceSpec::CreaturesOtherThanTarget,
                            recipient: spellwake_engine::damage::PreventionRecipientSpec::Any,
                            both_directions: false,
                        },
                    ),
                ],
            )
        }
        "Counter target spell that's the second spell cast this turn." => (
            BodyTarget::SingleSpell(
                spellwake_engine::program::SpellFilter::any()
                    .ordinal_this_turn(std::num::NonZeroU32::new(2)?),
            ),
            vec![Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::counter_targeted(spellwake_engine::toolbox::CounterDestination::Graveyard)))],
        ),
        "Target creature gets +2/+2 until end of turn. If it's paired with a creature, that creature also gets +2/+2 until end of turn." => {
            (
                SingleCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::TargetsWithPairedPartner,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(stats(2, 2))]),
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                ],
            )
        }
        "Counter target spell that targets a player." => (
            BodyTarget::SingleSpell(
                spellwake_engine::program::SpellFilter::any().targeting_player(),
            ),
            vec![Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::counter_targeted(spellwake_engine::toolbox::CounterDestination::Graveyard)))],
        ),
        "Up to two target creatures can't be blocked this turn." => {
            return Some(
                ExecutableBody::new(
                    SingleCreature,
                    vec![modify_target(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::CannotBeBlocked,
                    )])],
                )
                .with_permanent_target_bounds(0, 2),
            );
        }
        "Return all nonland permanents to their owners' hands." => (
            NoTarget,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group(ObjectFilter::nonland(), spellwake_engine::program::PublicZoneDestination::Hand)],
        ),
        "Each player discards their hand, then draws seven cards." => (
            NoTarget,
            vec![
                Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                    AffectedPlayers::EachPlayer,
                    spellwake_engine::program::ValueExpr::PlayerStat(
                        spellwake_engine::program::PlayerStatExpr::LargestHandSize,
                    ),
                ))),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_each_player(literal(7)),
            ],
        ),
        "Create X 4/4 white Angel creature tokens with flying." => {
            let CreateTokens { mut creations } = ordinary_text::create_tokens(
                "a 4/4 white Angel creature token with flying",
                vocabulary,
            )?
            else {
                return None;
            };
            let [creation] = creations.as_mut_slice() else {
                return None;
            };
            creation.amount = spellwake_engine::program::ValueExpr::chosen_x();
            (NoTarget, vec![CreateTokens { creations }])
        }
        _ if rules
            .strip_prefix("Take an extra turn after this one. Exile ")
            .and_then(|tail| tail.strip_suffix('.'))
            == Some(name) =>
        {
            (
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::take_extra_turn(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_resolving_spell(),
                ],
            )
        }
        "Discard your hand, then draw two cards." => (
            NoTarget,
            vec![
                discard(
                    ResolvingPlayer::Controller,
                    spellwake_engine::program::ValueExpr::PlayerStat(
                        spellwake_engine::program::PlayerStatExpr::LargestHandSize,
                    ),
                ),
                draw(2),
            ],
        ),
        "Return up to two target creatures to their owners' hands." => {
            return Some(
                ExecutableBody::new(
                    SingleCreature,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_hand()],
                )
                .with_permanent_target_bounds(0, 2),
            );
        }
        "Target player discards two cards and loses 2 life." => (
            SinglePlayer,
            vec![
                discard(ResolvingPlayer::TargetPlayer, literal(2)),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::TargetPlayer,
                    ),
                    literal(2),
                ),
            ],
        ),
        "Target creature gains deathtouch until end of turn. Regenerate it." => (
            SingleCreature,
            vec![
                modify_target(vec![ResolvedPartSpec::Abilities(
                    AbilityChange::grant_keywords([KeywordAbility::Deathtouch]),
                )]),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::regenerate_target(),
            ],
        ),
        _ if rules.strip_prefix(name)
            == Some(
                " deals 2 damage to any target. If a creature dealt damage this way would die this turn, exile it instead.",
            ) =>
        {
            (
                SingleAnyTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        literal(2),
                        true,
                    ),
                ],
            )
        }
        _ => return None,
    };
    Some(ExecutableBody::new(target, instructions))
}

pub(super) fn add_trigger(
    program: &mut Program,
    condition: TriggerSpec,
    target: BodyTarget,
    instructions: Vec<Instruction>,
) {
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(program.triggered.len() as u64),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(target, instructions),
    });
}

pub(super) fn activation(
    cost: ManaCost,
    source_cost: SourceCost,
    target: BodyTarget,
    instructions: Vec<Instruction>,
) -> ActivatedAbility {
    ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: cost,
        source_cost,
        body: ExecutableBody::new(target, instructions),
    }
}

fn attached(program: &mut Program, parts: Vec<StaticPart>) -> Option<()> {
    super::army_grants::add_battlefield_ability(
        program,
        PermanentSelector {
            attached_to_source: true,
            ..Default::default()
        },
        StaticParts(parts),
    )
}

fn static_stats(power: i64, toughness: i64) -> StaticPart {
    StaticPart::ModifyPT(NumericPair {
        power: NumericExpression::Literal(power.into()),
        toughness: NumericExpression::Literal(toughness.into()),
    })
}

fn add_subtype(vocabulary: &CreatureTypeVocabulary, name: &str) -> Option<StaticPart> {
    Some(StaticPart::Type(StaticTypeChange {
        types: TypeOperation::Keep,
        supertypes: TypeOperation::Keep,
        land_subtypes: SubtypeOperation::Keep,
        subtypes: SubtypeOperation::Add(StaticSubtypeOperand::Literal(
            [vocabulary.resolve_label(name)?].into(),
        )),
    }))
}

fn add_color(color: Color) -> StaticPart {
    StaticPart::Color(ColorOperation::Add(StaticColorOperand::Literal(
        [color].into(),
    )))
}

fn equipment(cost: ManaCost, rules: &str, vocabulary: &CreatureTypeVocabulary) -> Option<Program> {
    let (text, equip) = rules.rsplit_once("\nEquip ")?;
    let mut program = Program::equipment(Some(cost), fixed_cost(equip.trim())?);
    match text {
        "Equipped creature gets +3/+0 and attacks each combat if able." => {
            attached(&mut program, vec![static_stats(3, 0)])?;
            program.attack_requirement = Some(AttackRequirement::AttachedCreatureAttacks);
        }
        "Equipped creature gets +0/+3 and can block an additional creature each combat." => {
            attached(
                &mut program,
                vec![
                    static_stats(0, 3),
                    StaticPart::BlockingRestriction(BlockingRestriction::AdditionalBlock),
                ],
            )?
        }
        "Equipped creature gets +1/+1.\nAs long as equipped creature is a Human or an Angel, it has vigilance." =>
        {
            attached(&mut program, vec![static_stats(1, 1)])?;
            for label in ["Human", "Angel"] {
                super::army_grants::add_battlefield_ability(
                    &mut program,
                    PermanentSelector {
                        attached_to_source: true,
                        creature_subtype: Some(vocabulary.resolve_label(label)?),
                        ..Default::default()
                    },
                    StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
                        [KeywordAbility::Vigilance],
                    ))]),
                )?;
            }
        }

        "Equipped creature gets +2/+2, has flying, and is a white Angel in addition to its other colors and types." => {
            attached(
                &mut program,
                vec![
                    static_stats(2, 2),
                    add_subtype(vocabulary, "Angel")?,
                    add_color(Color::White),
                    StaticPart::Abilities(AbilityChange::grant_keywords([KeywordAbility::Flying])),
                ],
            )?
        }
        _ => return None,
    }
    Some(program)
}

fn aura(cost: ManaCost, rules: &str, vocabulary: &CreatureTypeVocabulary) -> Option<Program> {
    let restriction = if rules.starts_with("Enchant nonblack creature\n") {
        EnchantRestriction::NonblackCreature
    } else {
        EnchantRestriction::Creature
    };
    let mut program = Program::aura(Some(cost), restriction);
    match rules {
        "Enchant creature\nWhen enchanted creature dies, return that card to the battlefield under your control." =>
        {
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::any().attached_to_self(),
                ),
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Intersect(
                                Box::new(spellwake_engine::toolbox::ObjectQuery::TriggerSubject),
                                Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                        spellwake_engine::toolbox::PlayerSelector::OwnersOf(
                                            Box::new(
                                                spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                                            ),
                                        ),
                                    )),
                                    ObjectFilter::permanent(),
                                )),
                            ),
                            spellwake_engine::toolbox::ZoneDest::Battlefield {
                                controller: spellwake_engine::toolbox::ZoneController::Player(
                                    Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    )),
                                ),
                                tapped: false,
                                attacking: false,
                                transformed: false,
                                face_down: None,
                                entering_counters: Vec::new(),
                                attach_to: None,
                            },
                        ),
                    ),
                ],
            );
        }

        "Enchant creature\nEnchanted creature has \"Whenever this creature attacks, you may tap or untap target permanent.\"" =>
        {
            attached(
                &mut program,
                vec![StaticPart::GrantTrigger(Box::new(TriggeredAbility {
                    id: TriggeredAbilityId(0),
                    condition: spellwake_engine::program::TriggerSpec::self_attacks(),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(BodyTarget::SinglePermanent, optional_tap_change()),
                }))],
            )?;
        }

        "Enchant creature\nPrevent all damage that would be dealt by enchanted creature." => {
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::DamageRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticDamageRule::PreventEnchantedCreature { combat_only: false } });
        }
        "Enchant creature\nEnchanted creature doesn't untap during its controller's untap step.\nWhen enchanted creature becomes the target of a spell, sacrifice this Aura." =>
        {
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::UntapRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticUntapRule::EnchantedCreatureSkipsUntap });
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::new(
                    spellwake_engine::program::TriggerEvent::Targeted {
                        subject: spellwake_engine::program::ObjectFilter::any().attached_to_self(),
                        by_opponent: false,
                        spell_only: true,
                        instant_or_sorcery_only: false,
                    },
                ),
                BodyTarget::None,
                vec![Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::sacrifice_state_trigger_source(),
                ))],
            );
        }

        "Enchant creature\nEnchanted creature gets +2/+1.\nEnchanted creature has intimidate as long as its controller controls no other creatures." =>
        {
            attached(&mut program, vec![static_stats(2, 1)])?;
            super::army_grants::add_battlefield_ability(
                &mut program,
                PermanentSelector {
                    attached_to_source: true,
                    sole_controlled_creature: true,
                    ..Default::default()
                },
                StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
                    [KeywordAbility::Intimidate],
                ))]),
            )?;
        }

        "Enchant creature\nEnchanted creature loses flying." => attached(
            &mut program,
            vec![StaticPart::Abilities(AbilityChange::remove(
                AbilityRemoval::Flying,
            ))],
        )?,
        "Enchant creature\nEnchanted creature gets +1/-1 and attacks each combat if able." => {
            attached(&mut program, vec![static_stats(1, -1)])?;
            program.attack_requirement = Some(AttackRequirement::AttachedCreatureAttacks);
        }
        "Enchant nonblack creature\nEnchanted creature gets +1/+2, has flying, and is an Angel in addition to its other types." => {
            attached(
                &mut program,
                vec![
                    static_stats(1, 2),
                    add_subtype(vocabulary, "Angel")?,
                    StaticPart::Abilities(AbilityChange::grant_keywords([KeywordAbility::Flying])),
                ],
            )?
        }
        "Enchant creature\nEnchanted creature has \"{T}: Target creature can't block this turn.\"" =>
        {
            let ability = activation(
                ManaCost::free(),
                SourceCost::Tap,
                BodyTarget::SingleCreature,
                vec![modify_target(vec![ResolvedPartSpec::BlockingRestriction(
                    BlockingRestriction::CannotBlock,
                )])],
            );
            attached(
                &mut program,
                vec![StaticPart::GrantActivated(Box::new(ability))],
            )?;
        }
        "Enchant creature\nEnchanted creature gets -1/-1 and is a black Zombie in addition to its other colors and types." =>
        {
            attached(
                &mut program,
                vec![
                    static_stats(-1, -1),
                    add_subtype(vocabulary, "Zombie")?,
                    add_color(Color::Black),
                ],
            )?;
        }
        _ => return None,
    }
    Some(program)
}
fn enchantment(
    cost: ManaCost,
    rules: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let mut program = Program::enchantment(Some(cost));
    match rules {
        "Instant and sorcery spells cost {2} less to cast." => {
            program.continuous = Some(
                spellwake_engine::characteristics::StaticContinuousAbility::CostModifier {
                    id: StaticAbilityId(0),
                    modifier: spellwake_engine::characteristics::SpellCostModifier::Discount {
                        any_player: true,
                        matching: spellwake_engine::program::SpellFilter::instant_or_sorcery(),
                        amount: spellwake_engine::characteristics::SpellDiscount::Fixed(
                            std::num::NonZeroU8::new(2)?,
                        ),
                    },
                },
            );
        }

        "At the beginning of your end step, if you control exactly one creature, create a 5/5 black Demon creature token with flying." =>
        {
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::step_began(
                    Step::End,
                    spellwake_engine::program::PlayerRelation::Controller,
                )
                .only_if(spellwake_engine::program::TriggerCondition::ControlledCreatureCount(1)),
                BodyTarget::None,
                vec![
                    Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                            spellwake_engine::program::InterveningCondition::ControlledCreatureCount(1),
                        ),
                    )),
                    ordinary_text::create_tokens(
                        "a 5/5 black Demon creature token with flying",
                        vocabulary,
                    )?,
                ],
            );
        }
        "At the beginning of your upkeep, target opponent discards a card if you control the creature with the greatest power or tied for the greatest power." =>
        {
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::step_began(
                    Step::Upkeep,
                    PlayerRelation::Controller,
                ),
                BodyTarget::SingleOpponent,
                vec![
                    skip_unless_present(
                        spellwake_engine::program::ValueExpr::PlayerStat(
                            spellwake_engine::program::PlayerStatExpr::ControlsGreatestCreature,
                        ),
                        2,
                    ),
                    Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                        AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                        literal(1),
                    ))),
                ],
            );
        }
        "At the beginning of your upkeep, draw a card if you control the creature with the greatest power or tied for the greatest power." =>
        {
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::step_began(
                    Step::Upkeep,
                    PlayerRelation::Controller,
                ),
                BodyTarget::None,
                vec![
                    skip_unless_present(
                        spellwake_engine::program::ValueExpr::PlayerStat(
                            spellwake_engine::program::PlayerStatExpr::ControlsGreatestCreature,
                        ),
                        2,
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        literal(1),
                    ),
                ],
            );
        }
        "Whenever a creature you control becomes the target of an instant or sorcery spell, that creature gets +3/+3 until end of turn." => {
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::new(
                    spellwake_engine::program::TriggerEvent::Targeted {
                        subject: spellwake_engine::program::ObjectFilter::creature().controlled(),
                        by_opponent: false,
                        spell_only: true,
                        instant_or_sorcery_only: true,
                    },
                ),
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        EffectSubject::TriggeringTarget,
                        literal(3),
                        literal(3),
                        BTreeSet::new(),
                        BTreeSet::new(),
                    ),
                ],
            )
        }
        "Whenever a creature you control attacks, put a charge counter on this enchantment.\nRemove a charge counter from this enchantment: Add one mana of any color." =>
        {
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::attacks(
                    spellwake_engine::program::ObjectFilter::creature().controlled(),
                    spellwake_engine::program::AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
                ),
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                        spellwake_engine::effects::CounterKind::Charge,
                        literal(1),
                    ),
                ],
            );
            program.mana.push(ManaAbility {
                id: ManaAbilityId(0),
                output: ManaOutput::CounterBased(
                    spellwake_engine::mana::CounterManaYield::RemoveChargeForColor,
                ),
            });
        }
        "As long as you control exactly one creature, that creature gets +3/+1 and has lifelink." => {
            super::army_grants::add_battlefield_ability(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceController,
                    sole_controlled_creature: true,
                    ..Default::default()
                },
                StaticParts(vec![
                    static_stats(3, 1),
                    StaticPart::Abilities(AbilityChange::grant_keywords([
                        KeywordAbility::Lifelink,
                    ])),
                ]),
            )?
        }

        "Whenever an opponent loses life, you gain that much life." => add_trigger(
            &mut program,
            spellwake_engine::program::TriggerSpec::life_changed(
                LifeChangeKind::Loss,
                PlayerRelation::Opponent,
            ),
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::event(
                        spellwake_engine::program::EventValueKind::TriggeringLifeChange,
                    ),
                ),
            ],
        ),
        "Creatures with flying can't block creatures you control." => {
            super::army_grants::add_battlefield_ability(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceController,
                    required_types: [CardType::Creature].into(),
                    ..Default::default()
                },
                StaticParts(vec![StaticPart::BlockingRestriction(
                    BlockingRestriction::CannotBeBlockedByFlying,
                )]),
            )?;
        }
        _ => return None,
    }
    Some(program)
}

fn blink(optional: bool) -> Vec<Instruction> {
    let mut instructions = Vec::new();
    if optional {
        instructions.push(Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: 2,
        });
    }
    instructions.extend([
        crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
        Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::return_exiled_target_cohort(false, false),
        )),
    ]);
    instructions
}

fn artifact(cost: ManaCost, rules: &str, vocabulary: &CreatureTypeVocabulary) -> Option<Program> {
    let mut program = Program::artifact(Some(cost), vec![]);
    if rules
        == "When this artifact enters, put target card from a graveyard on the bottom of its owner's library.\n{T}: Add one mana of any color."
    {
        add_trigger(
            &mut program,
            spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            BodyTarget::AnyGraveyardCard,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_target_in_library(
                    LibraryPlacement::Bottom,
                ),
            ],
        );
        program.mana.push(ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::ChooseColor {
                amount: 1.into(),
                colors: [
                    ManaType::White,
                    ManaType::Blue,
                    ManaType::Black,
                    ManaType::Red,
                    ManaType::Green,
                ]
                .into(),
            },
        });
        return Some(program);
    }

    if rules
        == "{3}, {T}, Tap three untapped Humans you control: Destroy target creature. Its controller creates a 1/1 white Spirit creature token with flying."
    {
        let Instruction::CreateTokens { mut creations } = ordinary_text::create_tokens(
            "a 1/1 white Spirit creature token with flying",
            vocabulary,
        )?
        else {
            return None;
        };
        let [creation] = creations.as_mut_slice() else {
            return None;
        };
        creation.recipients = spellwake_engine::token::TokenRecipients::RememberedTargetController;
        program.activated.push(activation(
            fixed_cost("{3}")?,
            SourceCost::TapPermanents {
                filter: spellwake_engine::activated::TapCostFilter::Subtype(
                    vocabulary.resolve_label("Human")?,
                ),
                count: spellwake_engine::activated::TapCostCount::Fixed(std::num::NonZeroU8::new(
                    3,
                )?),
                tap_source: true,
                exclude_source: true,
            },
            BodyTarget::SingleCreature,
            vec![
                Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
                )),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: true,
                            },
                        },
                    ),
                ),
                Instruction::CreateTokens { creations },
            ],
        ));
        return Some(program);
    }

    let single_subtype = |kind| {
        Some(skip_unless_present(
            spellwake_engine::program::ValueExpr::controlled_subtype(
                vocabulary.resolve_label(kind)?,
            ),
            3,
        ))
    };
    let scroll = match rules {
        "{1}, Sacrifice this artifact: Draw a card. If you control an Angel, you gain 5 life." => {
            Some((
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        literal(1),
                    ),
                    single_subtype("Angel")?,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        literal(5),
                    ),
                ],
            ))
        }
        "{1}, Sacrifice this artifact: Target opponent discards a card. If you control a Demon, that player loses 3 life." => {
            Some((
                BodyTarget::SingleOpponent,
                vec![
                    Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                        AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                        literal(1),
                    ))),
                    single_subtype("Demon")?,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::TargetPlayer,
                        ),
                        literal(3),
                    ),
                ],
            ))
        }
        _ => None,
    };
    if let Some((target, instructions)) = scroll {
        program.activated.push(activation(
            fixed_cost("{1}")?,
            SourceCost::Sacrifice,
            target,
            instructions,
        ));
        return Some(program);
    }
    let (event, target, instructions) = match rules {
        "At the beginning of your end step, you may exile target creature you control, then return that card to the battlefield under your control." => {
            (
                spellwake_engine::program::TriggerSpec::step_began(
                    Step::End,
                    PlayerRelation::Controller,
                ),
                BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled()),
                blink(true),
            )
        }
        "Whenever a creature you control enters, you may have this artifact become a 3/3 white Angel artifact creature with flying until end of turn." =>
        {
            let parts = ResolvedPartsSpec(vec![
                ResolvedPartSpec::Type(TypeChangeSpec {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Add([CardType::Creature].into()),
                    subtypes: SubtypeOperation::Set(InstructionSubtypeOperand::Literal(
                        [vocabulary.resolve_label("Angel")?].into(),
                    )),
                }),
                ResolvedPartSpec::Color(ColorOperation::Set([Color::White].into())),
                ResolvedPartSpec::SetPT(stats(3, 3)),
                ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                    KeywordAbility::Flying,
                ])),
            ]);
            (
                spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::creature().controlled(),
                ),
                BodyTarget::None,
                vec![
                    Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 1,
                    },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                        parts,
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                ],
            )
        }
        _ => return None,
    };
    add_trigger(&mut program, event, target, instructions);
    Some(program)
}

fn combat_creature(
    card: &InventoryCard,
    rules: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let (keywords, restriction, controlled) = match rules {
        "This creature can't attack or block unless it's paired with a creature with soulbond." => {
            ("", BlockingRestriction::RequiresSoulbondPartner, false)
        }
        "Reach\nThis creature can block only creatures with flying." => {
            ("Reach", BlockingRestriction::CanBlockOnlyFlying, false)
        }
        "Flying\nThis creature can block only creatures with flying." => {
            ("Flying", BlockingRestriction::CanBlockOnlyFlying, false)
        }
        "This creature can't block Humans." => (
            "",
            BlockingRestriction::CannotBlockSubtype(vocabulary.resolve_label("Human")?),
            false,
        ),
        "Creatures with power less than this creature's power can't block it." => {
            ("", BlockingRestriction::BlockerPowerAtLeastSource, false)
        }
        "Creatures with power less than this creature's power can't block it.\nUndying" => (
            "Undying",
            BlockingRestriction::BlockerPowerAtLeastSource,
            false,
        ),
        "Creatures with power less than this creature's power can't block creatures you control.\nWhenever another creature you control enters, put a +1/+1 counter on this creature." => {
            ("", BlockingRestriction::BlockerPowerAtLeastSource, true)
        }
        _ => return None,
    };
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some(keywords)).ok()?;
    let controller = if controlled {
        ControllerSelector::SourceController
    } else {
        ControllerSelector::SourceItself
    };
    super::army_grants::add_battlefield_ability(
        &mut program,
        PermanentSelector {
            controller,
            required_types: [CardType::Creature].into(),
            ..Default::default()
        },
        StaticParts(vec![StaticPart::BlockingRestriction(restriction)]),
    )?;
    if controlled {
        add_trigger(
            &mut program,
            spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source(),
            ),
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                    spellwake_engine::effects::CounterKind::PlusOne,
                    literal(1),
                ),
            ],
        );
    }
    Some(program)
}

fn colorless_mana() -> ManaAbility {
    ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Fixed([(ManaType::Colourless, 1.into())].into()),
    }
}

fn land(rules: &str, vocabulary: &CreatureTypeVocabulary) -> Option<Program> {
    if rules
        == "{T}: Add {C}.\n{G}{U}, {T}: You may cast spells this turn as though they had flash."
    {
        let mut program = Program::land(BTreeSet::new(), vec![colorless_mana()]);
        program.activated.push(activation(
            fixed_cost("{G}{U}")?,
            SourceCost::Tap,
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(
                    spellwake_engine::program::TemporaryRuleKind::FilteredFlash {
                        player: spellwake_engine::program::ResolvingPlayer::Controller,
                        filter: spellwake_engine::program::ObjectFilter::any(),
                        next_only: false,
                    },
                    spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                ),
            ],
        ));
        return Some(program);
    }

    if rules
        == "When this land enters, you gain 1 life.\nWhenever an Angel you control enters, you gain 1 life.\n{T}: Add {C}."
    {
        let mut program = Program::land(BTreeSet::new(), vec![colorless_mana()]);
        for relation in [
            spellwake_engine::program::ObjectFilter::self_object(),
            spellwake_engine::program::ObjectFilter::any()
                .controlled()
                .with_subtype(vocabulary.resolve_label("Angel")?),
        ] {
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::enters(relation),
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        literal(1),
                    ),
                ],
            );
        }
        return Some(program);
    }
    None
}

fn creature(
    card: &InventoryCard,
    rules: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use BodyTarget::{None as NoTarget, SingleCreature, SinglePlayer};
    use Instruction::*;

    let base = || ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok();
    let counter = || {
        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
            spellwake_engine::effects::CounterKind::PlusOne,
            literal(1),
        )
    };
    let (keywords, clauses) = match rules {
        "If combat damage would be dealt to this creature, prevent that damage and exile that many cards from the top of your library." => {
            let mut program = base()?;
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::DamageRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticDamageRule::PreventIncomingCombatAndExile });
            return Some(program);
        }

        "Whenever a creature dealt damage by this creature this turn dies, return it to the battlefield under your control. That creature is a black Zombie in addition to its other colors and types." => {
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Dies { subject: spellwake_engine::program::ObjectFilter::creature(), mode: spellwake_engine::program::DiesTriggerMode::DamagedBySource }),
                    NoTarget,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec {
                                bind_moved: Some(spellwake_engine::toolbox::BindingId(0)),
                                ..spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                    spellwake_engine::toolbox::ObjectQuery::Intersect(
                                        Box::new(
                                            spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                                        ),
                                        Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                            spellwake_engine::toolbox::QueryZone::Graveyard(
                                                Box::new(
                                                    spellwake_engine::toolbox::PlayerSelector::OwnersOf(
                                                        Box::new(
                                                            spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                                                        ),
                                                    ),
                                                ),
                                            ),
                                            ObjectFilter::permanent(),
                                        )),
                                    ),
                                    spellwake_engine::toolbox::ZoneDest::Battlefield {
                                        controller:
                                            spellwake_engine::toolbox::ZoneController::Player(
                                                Box::new(
                                                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                        ResolvingPlayer::Controller,
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
                                                    [vocabulary.resolve_label("Zombie")?].into(),
                                                ),
                                            ),
                                            land_subtypes: spellwake_engine::characteristics::SubtypeOperation::Keep,
                                        },
                                    ),
                                    spellwake_engine::characteristics::ResolvedPartSpec::Color(
                                        spellwake_engine::characteristics::ColorOperation::Add(
                                            [Color::Black].into(),
                                        ),
                                    ),
                                ]),
                                spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration,
                            ),
                        )),
                    ],
                )],
            )
        }

        "Protection from Vampires" => {
            let mut program = base()?;
            super::army_grants::add_battlefield_ability(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceItself,
                    ..Default::default()
                },
                StaticParts(vec![StaticPart::GrantSubtypeProtection {
                    types: StaticSubtypeOperand::Literal(
                        [vocabulary.resolve_label("Vampire")?].into(),
                    ),
                    creatures_only: false,
                }]),
            )?;
            return Some(program);
        }
        "Vigilance\nAs this creature enters, choose a creature type.\nHuman creatures you control have protection from creatures of the chosen type." =>
        {
            let mut program =
                ordinary_creature(card, vocabulary, Profile::Current, Some("Vigilance")).ok()?;
            let ProgramKind::Permanent(permanent) = &mut program.kind else {
                return None;
            };
            permanent.entry_tap = Some(spellwake_engine::land::EntryTap::ChooseCreatureType);
            super::army_grants::add_battlefield_ability(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceController,
                    required_types: [CardType::Creature].into(),
                    creature_subtype: Some(vocabulary.resolve_label("Human")?),
                    ..Default::default()
                },
                StaticParts(vec![StaticPart::GrantSubtypeProtection {
                    types: StaticSubtypeOperand::SourceEntryCreatureType,
                    creatures_only: true,
                }]),
            )?;
            return Some(program);
        }

        "Damage that would be dealt by this creature can't be prevented." => {
            let mut program = base()?;
            program
                .printed_keywords
                .insert(KeywordAbility::DamageCannotBePrevented);
            return Some(program);
        }

        "Flying, hexproof\nSpells and abilities your opponents control can't cause you to sacrifice permanents." =>
        {
            let mut program =
                ordinary_creature(card, vocabulary, Profile::Current, Some("Flying, hexproof"))
                    .ok()?;
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::Restriction { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticRestriction::ProhibitOpponentsCausingSacrifice });
            return Some(program);
        }

        "Flying\nOther nonblack creatures you control get +1/+1.\nPlayers can't pay life or sacrifice creatures to cast spells or activate abilities." =>
        {
            let mut program =
                ordinary_creature(card, vocabulary, Profile::Current, Some("Flying")).ok()?;
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::Restriction { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticRestriction::ProhibitLifeAndCreatureCosts });
            super::army_grants::add_battlefield_ability(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceController,
                    required_types: [CardType::Creature].into(),
                    color: spellwake_engine::characteristics::ColorFilter::Lacks(Color::Black),
                    exclude_source: true,
                    ..Default::default()
                },
                StaticParts(vec![static_stats(1, 1)]),
            )?;
            return Some(program);
        }

        "{2}{B}, Sacrifice a creature: Target opponent reveals their hand. You choose a card from it. That player discards that card. Activate only as a sorcery." =>
        {
            let mut program = base()?;
            program.activated.push(activation(
                fixed_cost("{2}{B}")?,
                SourceCost::SacrificeSelected {
                    filter: spellwake_engine::activated::SelectedSacrificeFilter::Creature,
                    count: 1,
                    tap_source: false,
                    from_graveyard: false,
                },
                BodyTarget::SingleOpponent,
                vec![Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::choose_revealed_hand_card(
                        0,
                        false,
                        None,
                        std::num::NonZeroU8::MIN,
                        spellwake_engine::program::ChosenHandAction::Discard,
                    ),
                ))],
            ));
            program.activation_restrictions.insert(
                ActivatedAbilityId(0),
                spellwake_engine::activated::ActivationRestriction::Sorcery,
            );
            return Some(program);
        }

        "Hexproof\nWhenever this creature deals combat damage to a player, if you control no other creatures, look at the top four cards of your library. Put one of them into your hand and the rest on the bottom of your library in any order." => {
            (
                "Hexproof",
                vec![(
                    spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player().only_if(spellwake_engine::program::TriggerCondition::NoOtherControlledCreatures),
                    NoTarget,
                    {
                        let mut ops = vec![Instruction::MutatePlayer(Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                                spellwake_engine::program::InterveningCondition::NoOtherControlledCreatures,
                            ),
                        ))];
                        ops.extend(
                            spellwake_engine::toolbox::ZoneOpSpec::look_top_choose_instructions(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                                spellwake_engine::program::ResolvingPlayer::Controller,
                                spellwake_engine::program::ValueExpr::Literal(4.into()),
                                None,
                                1,
                                false,
                                spellwake_engine::program::LookTopKeepDest::Hand,
                                spellwake_engine::program::LookTopRestDest::BottomOrder,
                                false,
                                false,
                                false,
                                None,
                            ),
                        );
                        ops
                    },
                )],
            )
        }

        "Whenever this creature becomes blocked by a creature, this creature deals 1 damage to that creature." => {
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::blocks_or_blocked(spellwake_engine::program::ObjectFilter::self_object(), spellwake_engine::program::BlockTriggerKind::BlockedByEach { without_flanking_only: false }),
                    NoTarget,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Objects(
                                spellwake_engine::toolbox::ObjectQuery::Intersect(
                                    Box::new(spellwake_engine::toolbox::ObjectQuery::TriggerSubject),
                                    Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                        spellwake_engine::toolbox::QueryZone::Battlefield,
                                        ObjectFilter::creature(),
                                    )),
                                ),
                            ),
                            spellwake_engine::program::ValueExpr::literal(1),
                            false,
                        ),
                    ],
                )],
            )
        }

        "During your turn, this creature gets +2/+2." => {
            let mut program = base()?;
            super::army_grants::add_battlefield_ability(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceItself,
                    ..Default::default()
                },
                StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                    power: NumericExpression::ConditionalBonus {
                        condition: spellwake_engine::characteristics::StaticBonusCondition::DuringControllersTurn,
                        bonus: 2.into(),
                    },
                    toughness: NumericExpression::ConditionalBonus {
                        condition: spellwake_engine::characteristics::StaticBonusCondition::DuringControllersTurn,
                        bonus: 2.into(),
                    },
                })]),
            )?;
            return Some(program);
        }

        "Flying\nAt the beginning of your upkeep, sacrifice this creature unless you pay {1} for each other creature you control." =>
        {
            let mut upkeep =
                ordinary_creature(card, vocabulary, Profile::Current, Some("Flying")).ok()?;
            upkeep.triggered.push(
                spellwake_engine::program::UpkeepKeyword::OtherCreaturesUpkeep
                    .ability(TriggeredAbilityId(0), fixed_cost("{1}")?),
            );
            return Some(upkeep);
        }
        "When this creature enters, target opponent chooses a permanent they control at random and sacrifices it. If a nonland permanent is sacrificed this way, repeat this process." => {
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::enters(ObjectFilter::self_object()),
                    BodyTarget::SingleOpponent,
                    vec![Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::sacrifice_random_until_land(),
                    ))],
                )],
            )
        }
        "When this creature enters from a graveyard, target opponent gains control of it.\nUndying" => {
            (
                "Undying",
                vec![(
                    spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Enters { subject: spellwake_engine::program::ObjectFilter::self_object(), from_zone: spellwake_engine::program::TriggerFromZone::Graveyard, cast_origin: None, attached_to_creature: false }),
                    BodyTarget::SingleOpponent,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::all(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            spellwake_engine::toolbox::ObjectMutation::Control(spellwake_engine::toolbox::ControlOp::Gain {
                                controller: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::TargetPlayer,
                                ),
                                duration: spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration,
                            }),
                        ),
                    ))],
                )],
            )
        }

        "{1}{G}, {T}: Target creature you control fights another target creature." => {
            let mut program = base()?;
            program.activated.push(activation(
                fixed_cost("{1}{G}")?,
                SourceCost::Tap,
                NoTarget,
                controlled_creature_pair(SingleCreature, Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::fight(spellwake_engine::toolbox::ObjectQuery::from_move_subject(spellwake_engine::program::MoveSubjectRef::RememberedFirst), spellwake_engine::toolbox::ObjectQuery::from_move_subject(spellwake_engine::program::MoveSubjectRef::RememberedSecond))))),
            ));
            return Some(program);
        }

        "{2}{W}, {T}: Tap target creature. If that creature is a Zombie, exile it." => {
            let mut program = base()?;
            program.activated.push(activation(
                fixed_cost("{2}{W}")?,
                SourceCost::Tap,
                SingleCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(true),
                    skip_unless_present(
                        spellwake_engine::program::ValueExpr::object_matches(spellwake_engine::program::MatchObjectRef::TargetObject, spellwake_engine::program::ObjectFilter::creature().with_subtype(
                            vocabulary.resolve_label("Zombie")?,
                        )),
                        3,
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                ],
            ));
            return Some(program);
        }

        "Whenever this creature attacks, Human creatures you control get +1/+0 until end of turn." => {
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::self_attacks(),
                    NoTarget,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        EffectSubject::Matching(
                            ObjectFilter::creature()
                                .controlled()
                                .with_subtype(vocabulary.resolve_label("Human")?),
                        ),
                        literal(1),
                        literal(0),
                        BTreeSet::new(),
                        BTreeSet::new(),
                    )],
                )],
            )
        }

        "Flying\nWhenever this creature attacks, put a +1/+1 counter on it.\nAngel spells and Human spells you cast cost {1} less to cast for each +1/+1 counter on this creature." =>
        {
            let mut program =
                ordinary_creature(card, vocabulary, Profile::Current, Some("Flying")).ok()?;
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::CostModifier { id: StaticAbilityId(0), modifier: spellwake_engine::characteristics::SpellCostModifier::Discount { any_player: false, matching: spellwake_engine::program::SpellFilter::either_subtype(vocabulary.resolve_label("Angel")?, vocabulary.resolve_label("Human")?), amount: SpellDiscount::SourcePlusOneCounters } });
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::self_attacks(),
                NoTarget,
                vec![counter()],
            );
            return Some(program);
        }

        "Sacrifice this creature: Exile target card from a graveyard." => {
            let mut program = base()?;
            program.activated.push(activation(
                ManaCost::free(),
                SourceCost::Sacrifice,
                BodyTarget::AnyGraveyardCard,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target()],
            ));
            return Some(program);
        }

        "{T}, Tap two untapped Humans you control: Exile target artifact or enchantment." => {
            let mut program = base()?;
            program.activated.push(activation(
                ManaCost::free(),
                SourceCost::TapPermanents {
                    filter: spellwake_engine::activated::TapCostFilter::Subtype(vocabulary.resolve_label("Human")?),
                    count: spellwake_engine::activated::TapCostCount::Fixed(std::num::NonZeroU8::new(2)?),
                    tap_source: true,
                    exclude_source: true,
                },
                BodyTarget::FilteredPermanent(ObjectFilter::artifact_or_enchantment()),
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target()],
            ));
            return Some(program);
        }

        "Flying\nYou may cast this card from exile." => {
            let mut program =
                ordinary_creature(card, vocabulary, Profile::Current, Some("Flying")).ok()?;
            program.cast_from_own_exile = true;
            return Some(program);
        }
        "{2}{G}{G}: This creature gets +X/+X until end of turn, where X is its power." => {
            let mut program = base()?;
            program.activated.push(activation(
                fixed_cost("{2}{G}{G}")?,
                SourceCost::None,
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                    EffectSubject::AbilitySource,
                    spellwake_engine::program::ValueExpr::object_stat(spellwake_engine::program::ValueObjectRef::AbilitySource, spellwake_engine::program::ObjectStatKind::Power),
                    spellwake_engine::program::ValueExpr::object_stat(spellwake_engine::program::ValueObjectRef::AbilitySource, spellwake_engine::program::ObjectStatKind::Power),
                    BTreeSet::new(),
                    BTreeSet::new(),
                )],
            ));
            return Some(program);
        }
        "Whenever this creature deals combat damage to a player, put a +1/+1 counter on it.\n{2}{R}: This creature deals damage to target creature equal to the number of +1/+1 counters on this creature." =>
        {
            let mut program = base()?;
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
                NoTarget,
                vec![counter()],
            );
            program.activated.push(activation(
                fixed_cost("{2}{R}")?,
                SourceCost::None,
                SingleCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::counters(
                            spellwake_engine::program::CounterSubjectRef::AbilitySource,
                            Some(CounterKind::PlusOne),
                        ),
                        false,
                    ),
                ],
            ));
            return Some(program);
        }
        "Flying\n{3}{W}: Prevent all combat damage that would be dealt to and dealt by this creature this turn." =>
        {
            let mut program =
                ordinary_creature(card, vocabulary, Profile::Current, Some("Flying")).ok()?;
            program.activated.push(activation(
                fixed_cost("{3}{W}")?,
                SourceCost::None,
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                        spellwake_engine::damage::TurnPreventionSpec {
                            combat_only: true,
                            source: spellwake_engine::damage::PreventionSourceSpec::AbilitySource,
                            recipient: spellwake_engine::damage::PreventionRecipientSpec::Any,
                            both_directions: true,
                        },
                    ),
                ],
            ));
            return Some(program);
        }
        "Whenever you cast an instant spell, you may pay {1}. If you do, draw a card." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::spell_cast(PlayerRelation::Controller, spellwake_engine::program::SpellFilter::of_type(CardType::Instant)),
                NoTarget,
                vec![
                    Instruction::PromptChoice(Box::new(
                        spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                            ResolvingPlayer::Controller,
                            fixed_cost("{1}")?,
                            spellwake_engine::program::ValueExpr::Literal(0.into()),
                            1,
                            2,
                        ),
                    )),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(literal(1)),
                ],
            )],
        ),
        "{3}{U}, {T}: Exile another target creature you control, then return that card to the battlefield under your control." =>
        {
            let mut program = base()?;
            program.activated.push(activation(
                fixed_cost("{3}{U}")?,
                SourceCost::Tap,
                BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled().other_than_ability_source()),
                blink(false),
            ));
            return Some(program);
        }
        "Flash\nFlying\nWhen this creature enters, you may exile target non-Angel creature you control, then return that card to the battlefield under your control." => {
            (
                "Flash, flying",
                vec![(
                    spellwake_engine::program::TriggerSpec::enters(ObjectFilter::self_object()),
                    BodyTarget::FilteredPermanent(
                        ObjectFilter::creature()
                            .controlled()
                            .without_subtype(vocabulary.resolve_label("Angel")?),
                    ),
                    blink(true),
                )],
            )
        }
        "Whenever this creature deals combat damage to a player, put a +1/+1 counter on it.\nWhenever another creature dies, put a +1/+1 counter on this creature." => {
            (
                "",
                vec![
                    (
                        spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
                        NoTarget,
                        vec![counter()],
                    ),
                    (spellwake_engine::program::TriggerSpec::dies(ObjectFilter::creature().other_than_ability_source()), NoTarget, vec![counter()]),
                ],
            )
        }
        "Intimidate\nWhenever this creature deals combat damage to a player, put a +1/+1 counter on it." => {
            (
                "Intimidate",
                vec![(
                    spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
                    NoTarget,
                    vec![counter()],
                )],
            )
        }
        "Whenever this creature or another creature dies, target player loses 1 life and you gain 1 life." => {
            (
                "",
                [spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Dies { subject: spellwake_engine::program::ObjectFilter::self_object(), mode: spellwake_engine::program::DiesTriggerMode::Permanent }), spellwake_engine::program::TriggerSpec::dies(ObjectFilter::creature().other_than_ability_source())]
                    .into_iter()
                    .map(|condition| {
                        (
                            condition,
                            SinglePlayer,
                            vec![
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::TargetPlayer,
                                    ),
                                    literal(1),
                                ),
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                                        ResolvingPlayer::Controller,
                                    ),
                                    literal(1),
                                ),
                            ],
                        )
                    })
                    .collect(),
            )
        }
        "Flying\nWhen this creature enters, return a permanent you control to its owner's hand." => {
            (
                "Flying",
                vec![(
                    spellwake_engine::program::TriggerSpec::enters(ObjectFilter::self_object()),
                    NoTarget,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_controlled_permanents(AffectedPlayers::One(ResolvingPlayer::Controller), literal(1), PermanentTypeFilter {
                            any_types: BTreeSet::new(),
                            required_types: BTreeSet::new(),
                            forbidden_types: BTreeSet::new(), object_filter: None })],
                )],
            )
        }
        "When this creature dies, it deals 3 damage divided as you choose among one, two, or three targets." => {
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Dies { subject: spellwake_engine::program::ObjectFilter::self_object(), mode: spellwake_engine::program::DiesTriggerMode::Permanent }),
                    BodyTarget::SingleAnyTarget,
                    vec![Instruction::DealDamage(Box::new(
                        spellwake_engine::toolbox::DealDamageSpec::divide_three(),
                    ))],
                )],
            )
        }
        "When this creature enters, it deals damage to target player or planeswalker equal to the number of Humans you control." => {
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::enters(ObjectFilter::self_object()),
                    BodyTarget::SinglePlayerOrPlaneswalker,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Targets,
                            spellwake_engine::program::ValueExpr::controlled_subtype(
                                vocabulary.resolve_label("Human")?,
                            ),
                            false,
                        ),
                    ],
                )],
            )
        }
        "When this creature dies, target player mills five cards." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Dies { subject: spellwake_engine::program::ObjectFilter::self_object(), mode: spellwake_engine::program::DiesTriggerMode::Permanent }),
                SinglePlayer,
                vec![Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::mill(
                        ResolvingPlayer::TargetPlayer,
                        literal(5),
                    ),
                ))],
            )],
        ),
        "When this creature dies, each player loses 3 life." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Dies { subject: spellwake_engine::program::ObjectFilter::self_object(), mode: spellwake_engine::program::DiesTriggerMode::Permanent }),
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                        spellwake_engine::toolbox::PlayerSelector::Group(
                            spellwake_engine::program::AffectedPlayers::EachPlayer,
                        ),
                        literal(3),
                    ),
                ],
            )],
        ),
        "When this creature dies, you may have target creature get -2/-2 until end of turn." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Dies { subject: spellwake_engine::program::ObjectFilter::self_object(), mode: spellwake_engine::program::DiesTriggerMode::Permanent }),
                SingleCreature,
                vec![
                    OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 1,
                    },
                    modify_target(vec![ResolvedPartSpec::ModifyPT(stats(-2, -2))]),
                ],
            )],
        ),
        _ => return None,
    };
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some(keywords)).ok()?;
    for (condition, target, instructions) in clauses {
        add_trigger(&mut program, condition, target, instructions);
    }
    Some(program)
}

fn rain_of_thorns(cost: ManaCost) -> Program {
    let targets = [
        ObjectFilter::artifact(),
        ObjectFilter::enchantment(),
        ObjectFilter::land(),
    ];
    let [artifact, enchantment, land] = targets.map(|filter| {
        ExecutableBody::new(
            BodyTarget::FilteredPermanent(filter),
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: true,
                            },
                        },
                    ),
                ),
            ],
        )
    });
    let mut program = Program::sorcery(cost, artifact);
    if let ProgramKind::Spell(definition) = &mut program.kind {
        definition.modes = Some(SpellModes {
            minimum: 1,
            maximum: 3,
            conditional_maximum: None,
            allow_repeat: false,
            distinct_targets: false,
            uniqueness: spellwake_engine::program::ModalUniqueness::None,
            mode_costs: Vec::new(),
            escalate_mana: None,
            prefix: None,
            suffix: None,
            additional: vec![enchantment, land],
        });
    }
    program
}

pub(super) fn cost_with_x(mut text: &str) -> Option<(ManaCost, u8)> {
    let mut x = 0u8;
    while let Some(rest) = text.strip_prefix("{X}") {
        x = x.checked_add(1)?;
        text = rest;
    }
    let cost = if text.is_empty() && x > 0 {
        ManaCost::free()
    } else {
        fixed_cost(text)?
    };
    Some((cost, x))
}

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
    let rules = super::without_reminder(face.oracle_text.as_deref()?)?;
    let rules = rules.lines().map(str::trim).collect::<Vec<_>>().join("\n");
    let rules = rules.trim();
    let type_line = face.type_line.as_deref()?;
    let cost = || fixed_cost(face.mana_cost.as_deref()?);
    match type_line.split(" — ").next()? {
        "Sorcery"
            if type_line == "Sorcery"
                && rules
                    == "Choose one or more —\n• Destroy target artifact.\n• Destroy target enchantment.\n• Destroy target land." =>
        {
            Some(rain_of_thorns(cost()?))
        }
        "Instant" | "Sorcery" if matches!(type_line, "Instant" | "Sorcery") => {
            let (price, x_symbols) = cost_with_x(face.mana_cost.as_deref()?)?;
            let body = spell(&card.name, rules, vocabulary)?;
            let mut program = if type_line == "Instant" {
                Program::instant(price, body)
            } else {
                Program::sorcery(price, body)
            };
            program.x_mana_symbols = x_symbols;
            program.tap_creatures_cast_cost = rules.starts_with("As an additional cost to cast this spell, tap any number of untapped creatures you control.\n");
            Some(program)
        }
        "Artifact" if type_line == "Artifact" => artifact(cost()?, rules, vocabulary),
        "Artifact" if type_line == "Artifact — Book" => {
            let mut program = artifact(cost()?, rules, vocabulary)?;
            program
                .artifact_subtypes
                .insert(spellwake_engine::program::ArtifactSubtype::Book);
            Some(program)
        }
        "Artifact" if type_line == "Artifact — Equipment" => {
            equipment(cost()?, rules, vocabulary)
        }
        "Enchantment" if type_line == "Enchantment — Aura" => aura(cost()?, rules, vocabulary),
        "Enchantment" if type_line == "Enchantment" => enchantment(cost()?, rules, vocabulary),
        "Land" if type_line == "Land" => land(rules, vocabulary),
        "Creature" | "Legendary Creature" => {
            combat_creature(card, rules, vocabulary).or_else(|| creature(card, rules, vocabulary))
        }
        _ => None,
    }
}
