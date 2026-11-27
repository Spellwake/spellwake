//! Complete planeswalker faces using source-authenticated printed loyalty.
use super::{Inventory, InventoryCard, fixed_cost, without_reminder};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{KeywordAbility, PowerToughness, StaticAbilityId},
    emblem::EmblemDefinition,
    mana::{ManaCost, ManaType},
    program::{
        BodyTarget, CardType, CardTypeMask, ExecutableBody, Instruction, ObjectClause,
        ObjectColorFilter, ObjectFilter, PermanentDefinition, PlaneswalkerSubtype, PowerFilter,
        Program, ProgramKind, ResolvingPlayer, Supertype,
    },
};
use std::collections::BTreeSet;

pub(super) const LOYALTY_SOURCE_SHA256: &str =
    "76c807dfd42662a5e83dea167e9dc8672502b4b6ddfc07552fd9a8290e415d4e";

/// Compile only a whole face whose every loyalty ability has an admitted body.
pub(super) fn compile(
    card: &InventoryCard,
    inventory: &Inventory,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    loyalty_source_verified: bool,
) -> Option<Program> {
    if !loyalty_source_verified {
        return None;
    }
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || card.name != face.name {
        return None;
    }
    let loyalty = inventory
        .source_loyalty(card.oracle_id.as_deref()?, 0)?
        .parse::<u16>()
        .ok()?;
    let cost = fixed_cost(face.mana_cost.as_deref()?)?;
    let mut program = planeswalker_face(face.type_line.as_deref()?, Some(cost), loyalty)?;
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(price) = line.strip_prefix("Kicker ") {
            program.additional_cast_costs.push(
                spellwake_engine::program::AdditionalCastCost::Kicker(fixed_cost(price)?),
            );
            continue;
        }
        if let Some(token) = line
            .strip_prefix(&format!(
                "When {} enters, create ",
                card.name.split(',').next()?
            ))
            .and_then(|s| s.strip_suffix('.'))
        {
            use spellwake_engine::program::{
                TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
            };
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(index.try_into().ok()?),
                condition: spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![super::ordinary_text::create_tokens(token, vocabulary)?],
                ),
            });
            continue;
        }
        if let Some(token) = line
            .strip_prefix("Whenever you cast a noncreature spell, create ")
            .and_then(|s| s.strip_suffix('.'))
        {
            use spellwake_engine::program::{
                PlayerRelation, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
            };
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(index.try_into().ok()?),
                condition: spellwake_engine::program::TriggerSpec::spell_cast(
                    PlayerRelation::Controller,
                    spellwake_engine::program::SpellFilter::noncreature(),
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![super::ordinary_text::create_tokens(token, vocabulary)?],
                ),
            });
            continue;
        }
        if let Some(kind) = line
            .strip_prefix("Whenever a creature attacks you or a planeswalker you control, each ")
            .and_then(|s| s.strip_suffix(" you control deals 1 damage to that creature."))
        {
            use spellwake_engine::program::{
                TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
            };
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(index.try_into().ok()?),
                condition: spellwake_engine::program::TriggerSpec::attacks(
                    spellwake_engine::program::ObjectFilter::any(),
                    spellwake_engine::program::AttackTriggerMode::AtControllerOrPlaneswalker,
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage_from(
                            spellwake_engine::toolbox::DamageSourceQuery::Objects(
                                spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                        Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        ),
                                    ),
                                    spellwake_engine::program::ObjectFilter::any()
                                        .with_subtype(vocabulary.resolve_label(kind)?),
                                ),
                            ),
                            spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                                spellwake_engine::program::DamageRecipientsRef::TriggeringObject,
                            )),
                            spellwake_engine::program::DamageAmountSpec::Expr(
                                spellwake_engine::program::ValueExpr::literal(1),
                            ),
                            false,
                        ),
                    ],
                ),
            });
            continue;
        }
        if line == "This spell can't be countered." {
            program.cannot_be_countered = true;
            continue;
        }
        if line
            == "At the beginning of your end step, Chandra deals X damage to each opponent, where X is the number of planeswalkers you control."
        {
            use spellwake_engine::program::{
                PlayerRelation, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
            };
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(u64::try_from(index).ok()?),
                condition: spellwake_engine::program::TriggerSpec::step_began(
                    spellwake_engine::turn::Step::End,
                    PlayerRelation::Controller,
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Players(
                                spellwake_engine::toolbox::PlayerSelector::Group(
                                    spellwake_engine::program::AffectedPlayers::EachOpponent,
                                ),
                            ),
                            spellwake_engine::program::ValueExpr::controlled_type(
                                CardType::Planeswalker,
                            ),
                            false,
                        ),
                    ],
                ),
            });
            continue;
        }
        if line
            == "At the beginning of your end step, put a loyalty counter on another target planeswalker you control."
        {
            use spellwake_engine::program::{
                PlayerRelation, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
            };
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(u64::try_from(index).ok()?),
                condition: spellwake_engine::program::TriggerSpec::step_began(
                    spellwake_engine::turn::Step::End,
                    PlayerRelation::Controller,
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::FilteredPermanent(
                        ObjectFilter::planeswalker()
                            .controlled()
                            .other_than_trigger_source(),
                    ),
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(spellwake_engine::effects::CounterKind::Loyalty, spellwake_engine::program::ValueExpr::literal(1))],
                ),
            });
            continue;
        }
        if line == format!("{} can be your commander.", card.name) {
            continue;
        }
        if line == "Colorless spells you cast cost {2} less to cast." {
            use spellwake_engine::characteristics::SpellDiscount;
            if program
                .continuous
                .replace(
                    spellwake_engine::characteristics::StaticContinuousAbility::CostModifier {
                        id: StaticAbilityId(1),
                        modifier: spellwake_engine::characteristics::SpellCostModifier::Discount {
                            any_player: false,
                            matching: spellwake_engine::program::SpellFilter::colorless(),
                            amount: SpellDiscount::Fixed(std::num::NonZeroU8::new(2)?),
                        },
                    },
                )
                .is_some()
            {
                return None;
            }
            continue;
        }
        if line == "Each opponent can't draw more than one card each turn." {
            if program
                .continuous
                .replace(
                    spellwake_engine::characteristics::StaticContinuousAbility::DrawRule {
                        id: StaticAbilityId(1),
                        rule: spellwake_engine::characteristics::StaticDrawRule::OpponentsAtMostOne,
                    },
                )
                .is_some()
            {
                return None;
            }
            continue;
        }
        if line
            == "Prevent all noncombat damage that would be dealt to you and other permanents you control."
        {
            if program
                .continuous
                .replace(
                    spellwake_engine::characteristics::StaticContinuousAbility::DamageRule {
                        id: StaticAbilityId(1),
                        rule: spellwake_engine::characteristics::StaticDamageRule::PreventNoncombat,
                    },
                )
                .is_some()
            {
                return None;
            }
            continue;
        }
        let (signed, effect) = line.split_once(": ")?;
        let signed = signed.replace('−', "-");
        let variable_loyalty = signed == "-X";
        let loyalty_cost = if variable_loyalty {
            0
        } else {
            signed.parse::<i32>().ok()?
        };
        if let Some((target, instructions)) = match effect {
            "Destroy up to three target creatures and/or other planeswalkers. Return each card put into a graveyard this way to the battlefield under your control." => {
                Some((
                    BodyTarget::FilteredPermanent(ObjectFilter::either(
                        ObjectClause::creature(),
                        ObjectClause::planeswalker().other_than_trigger_source(),
                    )),
                    vec![
                        Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: 3,
                            action: spellwake_engine::program::PermanentTargetAction::Destroy,
                        },
                        Instruction::ZoneOp(Box::new(
                            spellwake_engine::toolbox::ZoneOpSpec::return_destroyed_this_way(),
                        )),
                    ],
                ))
            }
            "Each player discards a card." => Some((
                BodyTarget::None,
                vec![Instruction::Discard {
                    players: spellwake_engine::program::AffectedPlayers::EachPlayer,
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                }],
            )),
            "Target player sacrifices a creature." => Some((
                BodyTarget::SinglePlayer,
                vec![Instruction::Sacrifice {
                    players: spellwake_engine::program::AffectedPlayers::One(
                        ResolvingPlayer::TargetPlayer,
                    ),
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                    filter: spellwake_engine::program::PermanentTypeFilter {
                        any_types: BTreeSet::new(),
                        required_types: BTreeSet::from([CardType::Creature]),
                        forbidden_types: BTreeSet::new(),
                        object_filter: None,
                    },
                }],
            )),
            "Tap target permanent. It doesn't untap during its controller's next untap step." => {
                Some((
                    BodyTarget::SinglePermanent,
                    vec![Instruction::AffectPermanentTargets {
                        minimum: 1,
                        maximum: 1,
                        action:
                            spellwake_engine::program::PermanentTargetAction::TapAndSkipNextUntap,
                    }],
                ))
            }
            "Draw a card for each tapped creature target player controls." => Some((
                BodyTarget::SinglePlayer,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::count_player_battlefield(
                            spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                            spellwake_engine::program::ObjectFilter::creature().tapped(),
                        ),
                    ),
                ],
            )),
            "You get an emblem with \"You have no maximum hand size\" and \"Whenever a card is put into your graveyard from anywhere, you may return it to your hand.\"" => {
                Some((
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::create_emblem(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            EmblemDefinition::UnlimitedHandWithGraveyardReturn,
                        ),
                    ],
                ))
            }

            "Draw a card, then discard a card at random." => Some((
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_one_at_random(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                ],
            )),
            "Tibalt deals damage equal to the number of cards in target player's hand to that player." => {
                Some((
                    BodyTarget::SinglePlayer,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Targets,
                            spellwake_engine::program::ValueExpr::count_hand(
                                spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                            ),
                            false,
                        ),
                    ],
                ))
            }
            "Gain control of all creatures until end of turn. Untap them. They gain haste until end of turn." => {
                use spellwake_engine::characteristics::{
                    AbilityChange, ControlRecipient, ResolvedDuration, ResolvedPartSpec,
                    ResolvedPartsSpec,
                };
                Some((
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::set_group_tapped(
                            ObjectFilter::creature(),
                            false,
                        ),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                ObjectFilter::creature(),
                            ),
                            ResolvedPartsSpec(vec![
                                ResolvedPartSpec::Control(ControlRecipient::ResolvingController),
                                ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                                    KeywordAbility::Haste,
                                ])),
                            ]),
                            ResolvedDuration::UntilEndOfTurn,
                        ),
                    ],
                ))
            }
            "You and target opponent each draw a card." => Some((
                BodyTarget::SingleOpponent,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_target_player(
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                ],
            )),
            "Put a +1/+1 counter on each creature you control and a loyalty counter on each other planeswalker you control." => {
                Some((
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                ObjectFilter::creature().controlled(),
                            ),
                            spellwake_engine::effects::CounterKind::PlusOne,
                            spellwake_engine::program::ValueExpr::literal(1),
                        ),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                ObjectFilter::planeswalker()
                                    .controlled()
                                    .other_than_ability_source(),
                            ),
                            spellwake_engine::effects::CounterKind::Loyalty,
                            spellwake_engine::program::ValueExpr::literal(1),
                        ),
                    ],
                ))
            }
            text if text
                == format!(
                    "You draw X cards and {} deals X damage to each opponent, where X is the number of planeswalkers you control.",
                    card.name
                ) =>
            {
                let count =
                    spellwake_engine::program::ValueExpr::controlled_type(CardType::Planeswalker);
                Some((
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                            count.clone(),
                        ),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Players(
                                spellwake_engine::toolbox::PlayerSelector::Group(
                                    spellwake_engine::program::AffectedPlayers::EachOpponent,
                                ),
                            ),
                            count,
                            false,
                        ),
                    ],
                ))
            }
            _ => None,
        } {
            let id = ActivatedAbilityId(u64::try_from(index).ok()?);
            program.loyalty_costs.insert(id, loyalty_cost);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(target, instructions),
            });
            continue;
        }
        if effect
            == "You gain 2 life. Add {U}, {R}, or {W}. Spend this mana only to cast a noncreature spell."
        {
            let id = ActivatedAbilityId(u64::try_from(index).ok()?);
            program.loyalty_costs.insert(id, loyalty_cost);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            spellwake_engine::program::ValueExpr::literal(2),
                        ),
                        Instruction::MutatePlayer(Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                                spellwake_engine::program::EffectManaYield::RestrictedChoice {
                                    colors: BTreeSet::from([
                                        ManaType::Blue,
                                        ManaType::Red,
                                        ManaType::White,
                                    ]),
                                    amount: spellwake_engine::program::ValueExpr::literal(1),
                                    restriction:
                                        spellwake_engine::mana::ManaRestriction::NoncreatureSpell,
                                },
                            ),
                        )),
                    ],
                ),
            });
            continue;
        }
        if effect == "Create a 5/5 black Demon creature token with flying. You lose 2 life." {
            let id = ActivatedAbilityId(u64::try_from(index).ok()?);
            program.loyalty_costs.insert(id, loyalty_cost);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        super::ordinary_text::create_tokens(
                            "a 5/5 black Demon creature token with flying",
                            vocabulary,
                        )?,
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            spellwake_engine::program::ValueExpr::literal(2),
                        ),
                    ],
                ),
            });
            continue;
        }
        if effect == "Discard up to two cards, then draw that many cards." {
            let id = ActivatedAbilityId(index.try_into().ok()?);
            program.loyalty_costs.insert(id, loyalty_cost);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_up_to(2),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                            spellwake_engine::program::ValueExpr::event(
                                spellwake_engine::program::EventValueKind::LargestDiscard,
                            ),
                        ),
                    ],
                ),
            });
            continue;
        }
        if effect == "You may discard a card. If you do, draw a card." {
            let id = ActivatedAbilityId(index.try_into().ok()?);
            program.loyalty_costs.insert(id, loyalty_cost);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 2,
                        },
                        Instruction::Discard {
                            players: spellwake_engine::program::AffectedPlayers::One(
                                ResolvingPlayer::Controller,
                            ),
                            amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        },
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                            spellwake_engine::program::ValueExpr::event(
                                spellwake_engine::program::EventValueKind::LargestDiscard,
                            ),
                        ),
                    ],
                ),
            });
            continue;
        }
        if effect
            == format!(
                "Until end of turn, {} becomes a 6/6 Human Soldier creature that's still a planeswalker. Prevent all damage that would be dealt to him this turn.",
                card.name
            )
        {
            use spellwake_engine::characteristics::{
                InstructionSubtypeOperand, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
                SubtypeOperation, TypeChangeSpec, TypeOperation,
            };
            let parts = ResolvedPartsSpec(vec![
                ResolvedPartSpec::Type(TypeChangeSpec {
                    types: TypeOperation::Add([CardType::Creature].into()),
                    supertypes: TypeOperation::Keep,
                    subtypes: SubtypeOperation::Add(InstructionSubtypeOperand::Literal(
                        [
                            vocabulary.resolve_label("Human")?,
                            vocabulary.resolve_label("Soldier")?,
                        ]
                        .into(),
                    )),
                    land_subtypes: SubtypeOperation::Keep,
                }),
                ResolvedPartSpec::SetPT(PowerToughness {
                    power: 6.into(),
                    toughness: 6.into(),
                }),
            ]);
            let id = ActivatedAbilityId(index.try_into().ok()?);
            program.loyalty_costs.insert(id, loyalty_cost);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            parts,
                            ResolvedDuration::UntilEndOfTurn,
                        ),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                            spellwake_engine::damage::TurnPreventionSpec {
                                combat_only: false,
                                source: spellwake_engine::damage::PreventionSourceSpec::Any,
                                recipient:
                                    spellwake_engine::damage::PreventionRecipientSpec::AbilitySource,
                                both_directions: false,
                            },
                        ),
                    ],
                ),
            });
            continue;
        }
        let one = || spellwake_engine::program::ValueExpr::Literal(1.into());
        let (target, instruction) = match effect {
            "You get an emblem with \"Whenever an artifact is put into your graveyard from the battlefield, return that card to the battlefield at the beginning of the next end step.\"" => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::create_emblem(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    EmblemDefinition::ReturnFallenArtifacts,
                ),
            ),
            text if text.starts_with("Create a colorless Equipment artifact token named ") => {
                let (name,abilities)=text.strip_prefix("Create a colorless Equipment artifact token named ")?.split_once(". It has indestructible, \"Equipped creature ")?;
                let (host,price)=abilities.split_once(",\" and equip ")?;
                let mut token=Program::equipment(None,fixed_cost(price.strip_suffix('.')?)?);
                token.artifact_subtypes.insert(spellwake_engine::program::ArtifactSubtype::Equipment);
                token.printed_keywords.insert(KeywordAbility::Indestructible);
                token.continuous=Some(super::attachments::host_effect(&format!("{host}."),vocabulary)?);
                let name=spellwake_contracts::names::EnglishCardName::new(name.to_owned()).ok()?;
                let definition=spellwake_engine::token::TokenDefinition::new(token,spellwake_engine::token::TokenName::Specified(name),vocabulary).ok()?;
                (BodyTarget::None,Instruction::CreateTokens {creations:vec![spellwake_engine::token::TokenCreation::one(definition)]})
            }
            "Target artifact you control becomes a 9/9 Construct artifact creature and gains vigilance, indestructible, and \"This creature can't be blocked.\"" => {
                use spellwake_engine::characteristics::{AbilityChange,ResolvedPartsSpec,ResolvedPartSpec,ResolvedDuration,TypeChangeSpec,TypeOperation,SubtypeOperation,InstructionSubtypeOperand};
                let parts=ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {types:TypeOperation::Add([CardType::Artifact,CardType::Creature].into()),supertypes:TypeOperation::Keep,
                    subtypes:SubtypeOperation::Add(InstructionSubtypeOperand::Literal([vocabulary.resolve_label("Construct")?].into())),land_subtypes:SubtypeOperation::Keep}),
                    ResolvedPartSpec::SetPT(PowerToughness {power:9.into(),toughness:9.into()}),
                    ResolvedPartSpec::Abilities(AbilityChange { grants_unblockable:true, ..AbilityChange::grant_keywords([KeywordAbility::Vigilance,KeywordAbility::Indestructible]) })]);
                (BodyTarget::FilteredPermanent(ObjectFilter::artifact().controlled()),crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets,parts,ResolvedDuration::NoStatedDuration))
            }
            "Scry 2." => (BodyTarget::None, Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::scry(ResolvingPlayer::Controller, spellwake_engine::program::ValueExpr::Literal(2.into()))))),
            "Exile target enchantment, tapped artifact, or tapped creature." => (BodyTarget::FilteredPermanent(ObjectFilter::either(ObjectClause::enchantment(), ObjectClause::any().any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)).tapped())),crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target()),
            "Search your library for an artifact or creature card, put it onto the battlefield, then shuffle. It gains haste. Return it to your hand at the beginning of the next end step." =>
                (BodyTarget::None, Instruction::SearchLibrary { filter: Box::new(spellwake_engine::program::LibrarySearch::to_battlefield(
                        spellwake_engine::program::SearchCardMatcher::CardTypes([CardType::Artifact, CardType::Creature].into()),
                        false,
                    )
                    .with_destination(spellwake_engine::program::SearchDestination::Battlefield {
                        tapped: false,
                        post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::HasteAndReturnAtEndStep,
                    })) }),
            "Until end of turn, each planeswalker you control becomes a 4/4 red Dragon creature and gains flying." => {
                use spellwake_engine::characteristics::{Color,ColorOperation,AbilityChange,ResolvedPartsSpec,ResolvedPartSpec,ResolvedDuration,TypeChangeSpec,TypeOperation,SubtypeOperation,InstructionSubtypeOperand};
                let parts=ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {types:TypeOperation::Set([CardType::Creature].into()),supertypes:TypeOperation::Keep,
                    subtypes:SubtypeOperation::Set(InstructionSubtypeOperand::Literal([vocabulary.resolve_label("Dragon")?].into())),land_subtypes:SubtypeOperation::Keep}),
                    ResolvedPartSpec::Color(ColorOperation::Set([Color::Red].into())),ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([KeywordAbility::Flying])),
                    ResolvedPartSpec::SetPT(PowerToughness {power:4.into(),toughness:4.into()})]);
                (
                    BodyTarget::None,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Battlefield,
                            ObjectFilter::planeswalker().controlled(),
                        ),
                        parts,
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                )
            }
            "Create a 4/4 red Dragon creature token with flying." => (BodyTarget::None,super::ordinary_text::create_tokens("a 4/4 red Dragon creature token with flying",vocabulary)?),
            "Destroy target tapped creature." => (BodyTarget::FilteredPermanent(ObjectFilter::creature().tapped()),crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } }))),
            "Each opponent loses 1 life. You gain life equal to the life lost this way." => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::drain_life(
                    spellwake_engine::toolbox::PlayerSelector::Group(
                        spellwake_engine::program::AffectedPlayers::EachOpponent,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    true,
                ),
            ),
            "Add {R} for each planeswalker you control." => (BodyTarget::None, Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::add_mana(spellwake_engine::program::ResolvingPlayer::Controller, spellwake_engine::program::EffectManaYield::Counted { kind: ManaType::Red, amount: spellwake_engine::program::ValueExpr::controlled_type(CardType::Planeswalker) })))),
            "Add {R}{R}." => (BodyTarget::None, Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::add_mana(spellwake_engine::program::ResolvingPlayer::Controller, spellwake_engine::program::EffectManaYield::Fixed([(ManaType::Red, 2.into())].into()))))),
            "Create a 1/1 red Wizard creature token with \"{T}: Add {R}. Spend this mana only to cast a planeswalker spell.\"" => {
                use spellwake_engine::{characteristics::Color, mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaRestriction}, token::{TokenCreation, TokenDefinition, TokenName, TokenSubtype}};
                let wizard = vocabulary.resolve_label("Wizard")?;
                let mut token = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([wizard]));
                token.effect_defined_colors = Some(BTreeSet::from([Color::Red]));
                token.mana.push(ManaAbility { id: ManaAbilityId(0), output: ManaOutput::Fixed([(ManaType::Red, 1.into())].into()).restricted(ManaRestriction::PlaneswalkerSpell) });
                let name = TokenName::Default { ordered_subtypes: vec![TokenSubtype::Creature(wizard)] };
                let creation = TokenCreation::one(TokenDefinition::new(token, name, vocabulary).ok()?);
                (BodyTarget::None, Instruction::CreateTokens { creations: vec![creation] })
            },
            "Untap up to four target permanents." => (BodyTarget::SinglePermanent, Instruction::AffectPermanentTargets {
                minimum: 0, maximum: 4, action: spellwake_engine::program::PermanentTargetAction::Untap,
            }),
            "Until end of turn, up to one target creature gets +1/+1 and gains first strike, vigilance, and lifelink." => {
                use spellwake_engine::characteristics::{AbilityChange, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec};
                let parts = ResolvedPartsSpec(vec![
                    ResolvedPartSpec::ModifyPT(PowerToughness { power: 1.into(), toughness: 1.into() }),
                    ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([KeywordAbility::FirstStrike, KeywordAbility::Vigilance, KeywordAbility::Lifelink])),
                ]);
                (BodyTarget::SingleCreature, Instruction::AffectPermanentTargets { minimum: 0, maximum: 1,
                    action: spellwake_engine::program::PermanentTargetAction::Modify { parts, duration: ResolvedDuration::UntilEndOfTurn } })
            },
            "You get an emblem with \"If a source would deal damage to you or a planeswalker you control, prevent all but 1 of that damage.\"" => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::create_emblem(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    EmblemDefinition::PreventAllButOne,
                ),
            ),
            "You get an emblem with \"{1}{B}, Sacrifice a creature: You gain X life and draw X cards, where X is the sacrificed creature's power.\"" => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::create_emblem(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    EmblemDefinition::SacrificeCreatureGainLifeDraw,
                ),
            ),
            "You get an emblem with \"You may activate loyalty abilities of planeswalkers you control on any player's turn any time you could cast an instant.\"" => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::create_emblem(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    EmblemDefinition::LoyaltyAnyTurn,
                ),
            ),
            "Create a 1/1 green Elf Druid creature token with \"{T}: Add {G}.\"" => {
                use spellwake_engine::{characteristics::Color, mana::{ManaAbility, ManaAbilityId, ManaOutput}, token::{TokenCreation, TokenDefinition, TokenName, TokenSubtype}};
                let elf = vocabulary.resolve_label("Elf")?;
                let druid = vocabulary.resolve_label("Druid")?;
                let mut token = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([elf, druid]));
                token.effect_defined_colors = Some(BTreeSet::from([Color::Green]));
                token.mana.push(ManaAbility { id: ManaAbilityId(0), output: ManaOutput::Fixed([(ManaType::Green, 1.into())].into()) });
                let name = TokenName::Default { ordered_subtypes: vec![TokenSubtype::Creature(elf), TokenSubtype::Creature(druid)] };
                let creation = TokenCreation::one(TokenDefinition::new(token, name, vocabulary).ok()?);
                (BodyTarget::None, Instruction::CreateTokens { creations: vec![creation] })
            },
            "Destroy target permanent that's one or more colors." => (BodyTarget::FilteredPermanent(ObjectFilter::permanent().with_color(ObjectColorFilter::Colored)), crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } }))),
            "Destroy target artifact or enchantment." => (BodyTarget::FilteredPermanent(ObjectFilter::artifact_or_enchantment()), crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } }))),
            "Draw a card for each green creature you control." => (BodyTarget::None, crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(spellwake_engine::program::ValueExpr::count_battlefield(spellwake_engine::program::ObjectFilter::creature().controlled().with_color(spellwake_engine::program::ObjectColorFilter::has(spellwake_engine::characteristics::Color::Green))))),
            text if text == format!("{} deals 3 damage to each non-Elemental creature.", card.name.split(',').next()?) => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                        spellwake_engine::program::DamageRecipientsRef::Group(
                            spellwake_engine::program::DamageGroup::CreaturesExcept(
                                vocabulary.resolve_label("Elemental")?,
                            ),
                        ),
                    )),
                    spellwake_engine::program::ValueExpr::literal(3),
                    false,
                ),
            ),
            text if variable_loyalty && text == format!("{} deals X damage to target creature or planeswalker. If a permanent dealt damage this way would die this turn, exile it instead.", card.name.split(',').next()?) => (
                BodyTarget::FilteredPermanent(ObjectFilter::creature_or_planeswalker()),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::chosen_x(),
                    true,
                ),
            ),
            "Each opponent gets an emblem with \"At the beginning of your upkeep, this emblem deals 1 damage to you.\"" => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::create_emblem(
                    spellwake_engine::toolbox::PlayerSelector::Group(
                        spellwake_engine::program::AffectedPlayers::EachOpponent,
                    ),
                    EmblemDefinition::UpkeepDamage { amount: 1.into() },
                ),
            ),
            text if text == format!("{} deals 4 damage to target creature.", card.name.split(',').next()?) || text == format!("{} deals 4 damage to target creature.", card.name) => (
                BodyTarget::SingleCreature,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(4),
                    false,
                ),
            ),
            "Create a 1/1 black Vampire creature token with lifelink." =>
                (BodyTarget::None, super::ordinary_text::create_tokens("a 1/1 black Vampire creature token with lifelink", vocabulary)?),
            "You get an emblem with \"Creatures you control get +1/+0.\"" => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::create_emblem(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    EmblemDefinition::ControlledCreatureBonus {
                        power_toughness: PowerToughness {
                            power: 1.into(),
                            toughness: 0.into(),
                        },
                        keywords: BTreeSet::new(),
                    },
                ),
            ),
            "Create three 1/1 white Soldier creature tokens." => (BodyTarget::None, super::ordinary_text::create_tokens("three 1/1 white Soldier creature tokens", vocabulary)?),
            "Destroy all creatures with power 4 or greater." => (BodyTarget::None, crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(ObjectFilter::creature().with_power(PowerFilter::AtLeast(4)), true)),
            "You get an emblem with \"Whenever you cast a spell, this emblem deals 5 damage to any target.\"" => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::create_emblem(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    EmblemDefinition::SpellcastDamage {
                        amount: 5.into(),
                        noncreature_only: false,
                    },
                ),
            ),
            "You get an emblem with \"Whenever you cast a noncreature spell, this emblem deals 2 damage to any target.\"" => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::create_emblem(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    EmblemDefinition::SpellcastDamage {
                        amount: 2.into(),
                        noncreature_only: true,
                    },
                ),
            ),
            "You get an emblem with \"Creatures you control get +2/+2 and have flying.\"" => (
                BodyTarget::None,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::create_emblem(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    EmblemDefinition::ControlledCreatureBonus {
                        power_toughness: PowerToughness {
                            power: 2.into(),
                            toughness: 2.into(),
                        },
                        keywords: BTreeSet::from([KeywordAbility::Flying]),
                    },
                ),
            ),
            "Exile target creature with power 4 or greater." => (BodyTarget::FilteredPermanent(ObjectFilter::creature().with_power(PowerFilter::AtLeast(4))), crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target()),
            "Each player draws a card." => (BodyTarget::None, crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_each_player(one())),
            "Target player draws a card." => (BodyTarget::SinglePlayer, crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_target_player(one())),
            "Target player mills twenty cards." => (
                BodyTarget::SinglePlayer,
                Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::mill(
                        ResolvingPlayer::TargetPlayer,
                        spellwake_engine::program::ValueExpr::Literal(20.into()),
                    ),
                )),
            ),
            _ => return None,
        };
        let id = ActivatedAbilityId(u64::try_from(index).ok()?);
        program.loyalty_costs.insert(id, loyalty_cost);
        if variable_loyalty {
            program.variable_loyalty_costs.insert(id);
        }
        program.activated.push(ActivatedAbility {
            id,
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(target, vec![instruction]),
        });
    }
    (!program.activated.is_empty()).then_some(program)
}

/// Compile permanents that change the rules for planeswalker entry or loyalty activations.
pub(super) fn loyalty_support(
    card: &InventoryCard,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::program::{
        ObjectFilter, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
    };
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || card.name != face.name {
        return None;
    }
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut lines = text.trim().lines();
    let first = lines.next()?;
    let second = lines.next()?;
    if lines.next().is_some() {
        return None;
    }
    let cost = Some(fixed_cost(face.mana_cost.as_deref()?)?);
    let mut program = match face.type_line.as_deref()? {
        "Legendary Artifact" => Program::artifact(cost, vec![]),
        "Legendary Enchantment" => Program::enchantment(cost),
        _ => return None,
    };
    program.supertypes.insert(Supertype::Legendary);
    let (condition, body) = if second
        == "Each planeswalker you control enters with an additional loyalty counter on it."
    {
        let template = first
            .strip_prefix(&format!("When {} enters, create ", face.name))?
            .strip_suffix('.')?;
        program.continuous = Some(
            spellwake_engine::characteristics::StaticContinuousAbility::Modifier {
                id: StaticAbilityId(1),
                rule: spellwake_engine::characteristics::StaticModifierRule::ExtraEntryLoyalty,
            },
        );
        (
            spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            ExecutableBody::new(
                BodyTarget::None,
                vec![super::ordinary_text::create_tokens(template, vocabulary)?],
            ),
        )
    } else if second
        == "You may activate the loyalty abilities of planeswalkers you control twice each turn rather than only once."
        && first
            == format!(
                "When {} enters, exile another target permanent you control. Return it to the battlefield under its owner's control at the beginning of the next end step.",
                face.name
            )
    {
        program.continuous = Some(
            spellwake_engine::characteristics::StaticContinuousAbility::Modifier {
                id: StaticAbilityId(1),
                rule: spellwake_engine::characteristics::StaticModifierRule::TwiceLoyaltyPerTurn,
            },
        );
        (
            spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            ExecutableBody::new(
                BodyTarget::FilteredPermanent(
                    ObjectFilter::permanent()
                        .controlled()
                        .other_than_trigger_source(),
                ),
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_until_next_end_step()],
            ),
        )
    } else if first
        == "At the beginning of your end step, if you didn't activate a loyalty ability of a planeswalker this turn, you lose 2 life."
    {
        let mana=second.strip_suffix(", {T}: For each planeswalker you control, you may activate one of its loyalty abilities once this turn as though none of its loyalty abilities have been activated this turn.")?;
        program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: fixed_cost(mana)?,
            source_cost: SourceCost::Tap,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::additional_loyalty_activation(),
                ))],
            ),
        });
        (
            spellwake_engine::program::TriggerSpec::step_began(
                spellwake_engine::turn::Step::End,
                spellwake_engine::program::PlayerRelation::Controller,
            )
            .only_if(spellwake_engine::program::TriggerCondition::NoLoyaltyActivationThisTurn),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::RequireCondition(
                        spellwake_engine::program::InterveningCondition::NoLoyaltyActivation,
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::literal(2),
                    ),
                ],
            ),
        )
    } else {
        return None;
    };
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body,
    });
    Some(program)
}

fn planeswalker_face(line: &str, cost: Option<ManaCost>, loyalty: u16) -> Option<Program> {
    let subtype_line = line.strip_prefix("Legendary Planeswalker")?;
    let subtypes = if subtype_line.is_empty() {
        BTreeSet::new()
    } else {
        subtype_line
            .strip_prefix(" — ")?
            .split_whitespace()
            .map(PlaneswalkerSubtype::from_label)
            .collect::<Option<BTreeSet<_>>>()?
    };
    let mut program = Program::new(
        BTreeSet::from([CardType::Planeswalker]),
        BTreeSet::new(),
        BTreeSet::new(),
        ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: cost,
        }),
    );
    program.supertypes.insert(Supertype::Legendary);
    program.planeswalker_subtypes = subtypes;
    program.printed_loyalty = Some(i64::from(loyalty).into());
    Some(program)
}

pub(super) fn compile_transforming(
    _card: &InventoryCard,
    _inventory: &Inventory,
    _vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    _indicator: Option<&[String]>,
    _loyalty_source_verified: bool,
) -> Option<Program> {
    None
}
