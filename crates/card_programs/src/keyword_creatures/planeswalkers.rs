//! Complete planeswalker faces using source-authenticated printed loyalty.
use super::{Inventory, InventoryCard, fixed_cost, ordinary_text, without_reminder};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{Color, KeywordAbility, PowerToughness, StaticAbilityId},
    emblem::EmblemDefinition,
    mana::{ManaCost, ManaType},
    program::{
        BodyTarget, CardType, CardTypeMask, EffectSubject, ExecutableBody, Instruction,
        ObjectClause, ObjectColorFilter, ObjectFilter, PermanentDefinition, PlaneswalkerSubtype,
        PowerFilter, Program, ProgramKind, ResolvingPlayer, Supertype,
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
        let short = card.name.split(',').next()?;
        if line
            == format!(
                "When {short} enters, if {short} was kicked, create a token that's a copy of {short}, except it's not legendary and its starting loyalty is 1."
            )
        {
            use spellwake_engine::program::{
                TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
            };
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(index.try_into().ok()?),
                condition: spellwake_engine::program::TriggerSpec::new(
                    spellwake_engine::program::TriggerEvent::Enters {
                        subject: spellwake_engine::program::ObjectFilter::self_object(),
                        from_zone: spellwake_engine::program::TriggerFromZone::Any,
                        cast_origin: Some(spellwake_engine::program::CastEntryKind::Kicked),
                        attached_to_creature: false,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::CreateNonlegendaryLoyaltyCopy],
                ),
            });
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
                    vec![Instruction::ControlledSubtypeDamagesAttacker(
                        vocabulary.resolve_label(kind)?,
                    )],
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
                    vec![Instruction::DamageOpponentsAmount(
                        spellwake_engine::program::ValueExpr::controlled_type(
                            CardType::Planeswalker,
                        ),
                    )],
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
                    vec![Instruction::PutLoyaltyCounter],
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
                        Instruction::ReturnCardsDestroyedThisWay,
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
                    },
                }],
            )),
            "Separate all permanents target player controls into two piles. That player sacrifices all permanents in the pile of their choice." => {
                Some((
                    BodyTarget::SinglePlayer,
                    vec![Instruction::DivideTargetPermanentsAndSacrificePile],
                ))
            }
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
                vec![Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::count_player_battlefield(
                        spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                        spellwake_engine::program::ObjectFilter::creature().tapped(),
                    ),
                }],
            )),
            "You get an emblem with \"You have no maximum hand size\" and \"Whenever a card is put into your graveyard from anywhere, you may return it to your hand.\"" => {
                Some((
                    BodyTarget::None,
                    vec![Instruction::CreateEmblem(
                        EmblemDefinition::UnlimitedHandWithGraveyardReturn,
                    )],
                ))
            }

            "Draw a card, then discard a card at random." => Some((
                BodyTarget::None,
                vec![
                    Instruction::DrawController {
                        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                    },
                    Instruction::DiscardOneAtRandom(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                ],
            )),
            "Tibalt deals damage equal to the number of cards in target player's hand to that player." => {
                Some((
                    BodyTarget::SinglePlayer,
                    vec![Instruction::DamageAmount(
                        spellwake_engine::program::ValueExpr::count_hand(
                            spellwake_engine::program::ResolvingPlayer::TargetPlayer,
                        ),
                    )],
                ))
            }
            "Gain control of all creatures until end of turn. Untap them. They gain haste until end of turn." => {
                Some((
                    BodyTarget::None,
                    vec![Instruction::UntapAndControlCreatures],
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
                        Instruction::AdjustControllerLife {
                            adjustment: spellwake_engine::life::LifeAdjustment::Gain(2.into()),
                        },
                        Instruction::AddMana(
                            spellwake_engine::program::EffectManaYield::RestrictedChoice {
                                colors: BTreeSet::from([
                                    ManaType::Blue,
                                    ManaType::Red,
                                    ManaType::White,
                                ]),
                                restriction:
                                    spellwake_engine::mana::ManaRestriction::NoncreatureSpell,
                            },
                        ),
                    ],
                ),
            });
            continue;
        }
        if effect
            == "Draw a card, then you may discard a card. When you discard a nonland card this way, Narset deals damage equal to that card's mana value to target creature or planeswalker."
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
                        Instruction::DrawController {
                            amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        },
                        Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        },
                        Instruction::Discard {
                            players: spellwake_engine::program::AffectedPlayers::One(
                                ResolvingPlayer::Controller,
                            ),
                            amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        },
                        Instruction::CreateReflexiveDiscardDamage,
                    ],
                ),
            });
            continue;
        }
        if effect
            == "Exile the top card of your library face down and look at it. Create a 2/2 colorless Spirit creature token. When that token leaves the battlefield, put the exiled card into your hand."
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
                        Instruction::ExileTopFaceDown,
                        super::ordinary_text::create_tokens(
                            "a 2/2 colorless Spirit creature token",
                            vocabulary,
                        )?,
                        Instruction::RegisterTokenLinkedExile,
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
                        Instruction::AdjustControllerLife {
                            adjustment: spellwake_engine::life::LifeAdjustment::Lose(2.into()),
                        },
                    ],
                ),
            });
            continue;
        }
        if effect == "Discard up to two cards, then draw that many cards."
            || effect
                == "Sacrifice an artifact. If you do, return target artifact card from your graveyard to the battlefield."
        {
            let id = ActivatedAbilityId(index.try_into().ok()?);
            let (target, ops) = if effect.starts_with("Discard ") {
                (
                    BodyTarget::None,
                    vec![
                        Instruction::DiscardUpTo { maximum: 2 },
                        Instruction::DrawController {
                            amount: spellwake_engine::program::ValueExpr::event(
                                spellwake_engine::program::EventValueKind::LargestDiscard,
                            ),
                        },
                    ],
                )
            } else {
                (
                    BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::artifact()),
                    vec![
                        Instruction::SacrificeToContinue {
                            filter: spellwake_engine::program::PermanentTypeFilter {
                                any_types: BTreeSet::new(),
                                required_types: [CardType::Artifact].into(),
                                forbidden_types: BTreeSet::new(),
                            },
                        },
                        Instruction::ReturnOwnGraveyardPermanentToBattlefield { tapped: false },
                    ],
                )
            };
            program.loyalty_costs.insert(id, loyalty_cost);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(target, ops),
            });
            continue;
        }
        if let Some(token) = effect
            .strip_prefix("Create ")
            .and_then(|s| s.strip_suffix(". You may attach an Equipment you control to it."))
        {
            let id = ActivatedAbilityId(index.try_into().ok()?);
            program.loyalty_costs.insert(id, loyalty_cost);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        super::ordinary_text::create_tokens(token, vocabulary)?,
                        Instruction::EquipNewToken,
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
                        Instruction::DrawController {
                            amount: spellwake_engine::program::ValueExpr::event(
                                spellwake_engine::program::EventValueKind::LargestDiscard,
                            ),
                        },
                    ],
                ),
            });
            continue;
        }
        if effect
            == "Target artifact you control becomes a copy of another target artifact or creature you control until end of turn, except it's an artifact in addition to its other types."
        {
            let id = ActivatedAbilityId(index.try_into().ok()?);
            let target = |filter| {
                Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                    BodyTarget::FilteredPermanent(filter),
                    vec![Instruction::RememberPermanentTarget],
                )))
            };
            program.loyalty_costs.insert(id, loyalty_cost);
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        target(ObjectFilter::artifact().controlled()),
                        target(ObjectFilter::artifact_or_creature().controlled()),
                        Instruction::CopyOtherTargetAsArtifact,
                    ],
                ),
            });
            continue;
        }
        if effect
            == "Exile target creature or enchantment you don't control until target enchantment you control leaves the battlefield."
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
                        Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                            BodyTarget::FilteredPermanent(
                                ObjectFilter::creature_or_enchantment().opposing(),
                            ),
                            vec![Instruction::ExileTarget],
                        ))),
                        Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                            BodyTarget::FilteredPermanent(ObjectFilter::enchantment().controlled()),
                            vec![Instruction::ExileTarget],
                        ))),
                        Instruction::ExileUntilOtherTargetLeaves,
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
                        Instruction::ChangeAbilitySource {
                            parts,
                            duration: ResolvedDuration::UntilEndOfTurn,
                        },
                        Instruction::PreventDamageThisTurn(
                            spellwake_engine::damage::TurnPreventionFilter::ToAbilitySource,
                        ),
                    ],
                ),
            });
            continue;
        }
        let one = || spellwake_engine::program::ValueExpr::Literal(1.into());
        let (target, instruction) = match effect {
            "You get an emblem with \"Whenever an artifact is put into your graveyard from the battlefield, return that card to the battlefield at the beginning of the next end step.\"" =>
                (BodyTarget::None,Instruction::CreateEmblem(EmblemDefinition::ReturnFallenArtifacts)),
            "You may put an Equipment card from your hand or graveyard onto the battlefield." => (BodyTarget::None,Instruction::PutEquipmentFromHandOrGraveyard),
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
            "Reveal the top three cards of your library. An opponent separates those cards into two piles. Put one pile into your hand and the other on the bottom of your library in any order." => (BodyTarget::None,Instruction::RevealHandAndBottomPiles),
            "Until your next turn, whenever a creature an opponent controls attacks, it gets -1/-0 until end of turn." => (BodyTarget::None,Instruction::WeakenOpposingAttackersUntilNextTurn),
            "For each player, search that player's library for a nonland card and exile it, then that player shuffles. You may cast those cards without paying their mana costs." => (BodyTarget::None,Instruction::SearchEachLibraryForFreeCast),
            "Up to two other target planeswalkers you control phase out at the beginning of the next end step." => (BodyTarget::FilteredPermanent(ObjectFilter::planeswalker().controlled().other_than_trigger_source()),Instruction::PhaseOtherPlaneswalkersNextEndStep),
            "For each opponent, return up to one target nonland permanent that player controls to its owner's hand." => (BodyTarget::FilteredPermanent(ObjectFilter::nonland().opposing()),Instruction::ReturnOneNonlandPerOpponent),
            "Target artifact you control becomes a 9/9 Construct artifact creature and gains vigilance, indestructible, and \"This creature can't be blocked.\"" => {
                use spellwake_engine::characteristics::{AbilityChange,ResolvedPartsSpec,ResolvedPartSpec,ResolvedDuration,TypeChangeSpec,TypeOperation,SubtypeOperation,InstructionSubtypeOperand};
                let parts=ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {types:TypeOperation::Add([CardType::Artifact,CardType::Creature].into()),supertypes:TypeOperation::Keep,
                    subtypes:SubtypeOperation::Add(InstructionSubtypeOperand::Literal([vocabulary.resolve_label("Construct")?].into())),land_subtypes:SubtypeOperation::Keep}),
                    ResolvedPartSpec::SetPT(PowerToughness {power:9.into(),toughness:9.into()}),
                    ResolvedPartSpec::Abilities(AbilityChange { grants_unblockable:true, ..AbilityChange::grant_keywords([KeywordAbility::Vigilance,KeywordAbility::Indestructible]) })]);
                (BodyTarget::FilteredPermanent(ObjectFilter::artifact().controlled()),Instruction::CreateContinuousEffect {parts,duration:ResolvedDuration::NoStatedDuration})
            }
            "Scry 2." => (BodyTarget::None,Instruction::Scry {player:ResolvingPlayer::Controller,amount:spellwake_engine::program::ValueExpr::Literal(2.into())}),
            text if text == format!("Draw a card and reveal it. Remove a number of loyalty counters equal to that card's mana value from {short}.") => (BodyTarget::None,Instruction::DrawRevealRemoveLoyalty),
            "Exile target enchantment, tapped artifact, or tapped creature." => (BodyTarget::FilteredPermanent(ObjectFilter::either(ObjectClause::enchantment(), ObjectClause::any().any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE)).tapped())),Instruction::ExileTarget),
            "Search your library for an artifact or creature card, put it onto the battlefield, then shuffle. It gains haste. Return it to your hand at the beginning of the next end step." =>
                (BodyTarget::None, Instruction::SearchLibrary { filter: Box::new(spellwake_engine::program::LibrarySearch::to_battlefield(
                        spellwake_engine::program::SearchCardMatcher::CardTypes([CardType::Artifact, CardType::Creature].into()),
                        false,
                    )
                    .with_destination(spellwake_engine::program::SearchDestination::Battlefield {
                        tapped: false,
                        post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::HasteAndReturnAtEndStep,
                    })) }),
            text if text == format!("During target opponent's next turn, creatures that player controls attack {} if able.",card.name) =>
                (BodyTarget::SingleOpponent,Instruction::RequireAttackSourceNextTurn),
            "You and target opponent each draw a card." => (BodyTarget::SingleOpponent,Instruction::DrawControllerAndTarget {amount:one()}),
            "Choose left or right. Until your next turn, each player may attack only the nearest opponent in the last chosen direction and planeswalkers controlled by that opponent." =>
                (BodyTarget::None,Instruction::ChooseAttackDirection),
            "Until end of turn, each planeswalker you control becomes a 4/4 red Dragon creature and gains flying." => {
                use spellwake_engine::characteristics::{Color,ColorOperation,AbilityChange,ResolvedPartsSpec,ResolvedPartSpec,ResolvedDuration,TypeChangeSpec,TypeOperation,SubtypeOperation,InstructionSubtypeOperand};
                let parts=ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {types:TypeOperation::Set([CardType::Creature].into()),supertypes:TypeOperation::Keep,
                    subtypes:SubtypeOperation::Set(InstructionSubtypeOperand::Literal([vocabulary.resolve_label("Dragon")?].into())),land_subtypes:SubtypeOperation::Keep}),
                    ResolvedPartSpec::Color(ColorOperation::Set([Color::Red].into())),ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([KeywordAbility::Flying])),
                    ResolvedPartSpec::SetPT(PowerToughness {power:4.into(),toughness:4.into()})]);
                (BodyTarget::None,Instruction::ChangeControlledPlaneswalkers {parts,duration:ResolvedDuration::UntilEndOfTurn})
            }
            "Create a 4/4 red Dragon creature token with flying." => (BodyTarget::None,super::ordinary_text::create_tokens("a 4/4 red Dragon creature token with flying",vocabulary)?),
            "Destroy target tapped creature." => (BodyTarget::FilteredPermanent(ObjectFilter::creature().tapped()),Instruction::DestroyTarget),
            "Each opponent loses 1 life. You gain life equal to the life lost this way." => (BodyTarget::None, Instruction::DrainOpponents(spellwake_engine::program::ValueExpr::Literal(1.into()))),
            "Remove a loyalty counter from each of any number of permanents you control. Exile that many cards from the top of your library. You may play them this turn." => (BodyTarget::None, Instruction::RemoveLoyaltyExileTopForPlay),
            "Add {R} for each planeswalker you control." => (BodyTarget::None, Instruction::AddMana(spellwake_engine::program::EffectManaYield::Counted { kind: ManaType::Red, amount: spellwake_engine::program::ValueExpr::controlled_type(CardType::Planeswalker) })),
            "Add {R}{R}." => (BodyTarget::None, Instruction::AddMana(spellwake_engine::program::EffectManaYield::Fixed([(ManaType::Red, 2.into())].into()))),
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
            text if text == format!("You draw X cards and {} deals X damage to each opponent, where X is the number of planeswalkers you control.", card.name) =>
                (BodyTarget::None, Instruction::DrawAndDamageOpponents { amount: spellwake_engine::program::ValueExpr::controlled_type(CardType::Planeswalker) }),
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
            "Put a +1/+1 counter on each creature you control and a loyalty counter on each other planeswalker you control." =>
                (BodyTarget::None, Instruction::PutCreatureAndOtherPlaneswalkerCounters),
            "You get an emblem with \"If a source would deal damage to you or a planeswalker you control, prevent all but 1 of that damage.\"" =>
                (BodyTarget::None, Instruction::CreateEmblem(EmblemDefinition::PreventAllButOne)),
            "Look at the top two cards of your library. Put one of them into your hand and the other on the bottom of your library." =>
                (BodyTarget::None, Instruction::LookTopTakeOne { count: 2 }),
            "You get an emblem with \"{1}{B}, Sacrifice a creature: You gain X life and draw X cards, where X is the sacrificed creature's power.\"" =>
                (BodyTarget::None, Instruction::CreateEmblem(EmblemDefinition::SacrificeCreatureGainLifeDraw)),
            "You get an emblem with \"You may activate loyalty abilities of planeswalkers you control on any player's turn any time you could cast an instant.\"" =>
                (BodyTarget::None, Instruction::CreateEmblem(EmblemDefinition::LoyaltyAnyTurn)),
            text if text == format!("Exile the top card of your library. You may cast that card. If you don't, {} deals 2 damage to each opponent.", card.name.split(',').next()?) =>
                (BodyTarget::None, Instruction::ImpulseCastOrDamage(2.into())),
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
            "Return all enchantment cards from your graveyard to the battlefield." => (BodyTarget::None, Instruction::ReturnGraveyardCohort(spellwake_engine::program::GraveyardCohortReturn::own(
                spellwake_engine::program::GraveyardReturnFilter::CardType(CardType::Enchantment),
            ))),
            "Destroy target permanent that's one or more colors." => (BodyTarget::FilteredPermanent(ObjectFilter::permanent().with_color(ObjectColorFilter::Colored)), Instruction::DestroyTarget),
            "Destroy target artifact or enchantment." => (BodyTarget::FilteredPermanent(ObjectFilter::artifact_or_enchantment()), Instruction::DestroyTarget),
            "Draw a card for each green creature you control." => (BodyTarget::None, Instruction::DrawController { amount: spellwake_engine::program::ValueExpr::count_battlefield(spellwake_engine::program::ObjectFilter::creature().controlled().with_color(spellwake_engine::program::ObjectColorFilter::has(spellwake_engine::characteristics::Color::Green))) }),
            "Look at the top four cards of your library. You may reveal a noncreature, nonland card from among them and put it into your hand. Put the rest on the bottom of your library in a random order." =>
                (BodyTarget::None, Instruction::LookTopReveal { count: 4, filter: spellwake_engine::program::LookTopFilter::NoncreatureNonland }),
            "Look at the top four cards of your library. You may reveal an enchantment card from among them and put it into your hand. Put the rest on the bottom of your library in a random order."
            | "Look at the top four cards of your library. You may reveal an enchantment card from among them and put that card into your hand. Put the rest on the bottom of your library in a random order." =>
                (BodyTarget::None, Instruction::LookTopReveal { count: 4, filter: spellwake_engine::program::LookTopFilter::Enchantment }),
            text if text == format!("{} deals 3 damage to each non-Elemental creature.", card.name.split(',').next()?) =>
                (BodyTarget::None, Instruction::DamageGroup { group: spellwake_engine::program::DamageGroup::CreaturesExcept(vocabulary.resolve_label("Elemental")?), amount: 3.into() }),
            text if variable_loyalty && text == format!("{} deals X damage to target creature or planeswalker. If a permanent dealt damage this way would die this turn, exile it instead.", card.name.split(',').next()?) =>
                (BodyTarget::FilteredPermanent(ObjectFilter::creature_or_planeswalker()), Instruction::DamageAmountThenExileIfDies(spellwake_engine::program::ValueExpr::chosen_x())),
            "Each opponent gets an emblem with \"At the beginning of your upkeep, this emblem deals 1 damage to you.\"" =>
                (BodyTarget::None, Instruction::CreateOpponentEmblems(EmblemDefinition::UpkeepDamage { amount: 1.into() })),
            text if text == format!("{} deals 4 damage to target creature.", card.name.split(',').next()?) || text == format!("{} deals 4 damage to target creature.", card.name) =>
                (BodyTarget::SingleCreature, Instruction::Damage { amount: 4.into() }),
            "Create a 1/1 black Vampire creature token with lifelink." =>
                (BodyTarget::None, super::ordinary_text::create_tokens("a 1/1 black Vampire creature token with lifelink", vocabulary)?),
            "You get an emblem with \"Creatures you control get +1/+0.\"" => (BodyTarget::None, Instruction::CreateEmblem(EmblemDefinition::ControlledCreatureBonus {
                power_toughness: PowerToughness { power: 1.into(), toughness: 0.into() }, keywords: BTreeSet::new(),
            })),
            "Create three 1/1 white Soldier creature tokens." => (BodyTarget::None, super::ordinary_text::create_tokens("three 1/1 white Soldier creature tokens", vocabulary)?),
            "Destroy all creatures with power 4 or greater." => (BodyTarget::None, Instruction::DestroyGroup(Box::new(ObjectFilter::creature().with_power(PowerFilter::AtLeast(4))))),
            "You get an emblem with \"Whenever you cast a spell, this emblem deals 5 damage to any target.\"" => (BodyTarget::None, Instruction::CreateEmblem(EmblemDefinition::SpellcastDamage { amount: 5.into(), noncreature_only: false })),
            "You get an emblem with \"Whenever you cast a noncreature spell, this emblem deals 2 damage to any target.\"" => (BodyTarget::None, Instruction::CreateEmblem(EmblemDefinition::SpellcastDamage { amount: 2.into(), noncreature_only: true })),
            "You get an emblem with \"Creatures you control get +2/+2 and have flying.\"" => (BodyTarget::None, Instruction::CreateEmblem(EmblemDefinition::ControlledCreatureBonus {
                power_toughness: PowerToughness { power: 2.into(), toughness: 2.into() }, keywords: BTreeSet::from([KeywordAbility::Flying]),
            })),
            "Exile target creature with power 4 or greater." => (BodyTarget::FilteredPermanent(ObjectFilter::creature().with_power(PowerFilter::AtLeast(4))), Instruction::ExileTarget),
            "Each player draws a card." => (BodyTarget::None, Instruction::DrawEachPlayer { amount: one() }),
            "Target player draws a card." => (BodyTarget::SinglePlayer, Instruction::DrawTargetPlayer { amount: one() }),
            "Target player mills twenty cards." => (BodyTarget::SinglePlayer, Instruction::Mill {
                player: ResolvingPlayer::TargetPlayer, amount: spellwake_engine::program::ValueExpr::Literal(20.into()),
            }),
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
                vec![Instruction::ExileUntilNextEndStep],
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
                vec![Instruction::AdditionalLoyaltyActivation],
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
                    Instruction::AdjustControllerLife {
                        adjustment: spellwake_engine::life::LifeAdjustment::Lose(2.into()),
                    },
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
    card: &InventoryCard,
    inventory: &Inventory,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    indicator: Option<&[String]>,
    loyalty_source_verified: bool,
) -> Option<Program> {
    use spellwake_contracts::names::{EnglishCardName, ObjectNames};
    use spellwake_engine::{
        effects::CounterKind,
        program::{
            Comparison, LibrarySearch, PermanentTypeFilter, PowerDamageSource, TriggerGrouping,
            TriggeredAbility, TriggeredAbilityId,
        },
    };
    let [front, back] = card.faces.as_slice() else {
        return None;
    };
    if !loyalty_source_verified
        || card.layout != "transform"
        || front.name != "Garruk Relentless"
        || back.name != "Garruk, the Veil-Cursed"
        || without_reminder(front.oracle_text.as_deref()?)?
            != concat!(
                "When Garruk has two or fewer loyalty counters on him, transform him.\n",
                "0: Garruk deals 3 damage to target creature. That creature deals damage equal to its power to him.\n",
                "0: Create a 2/2 green Wolf creature token."
            )
        || without_reminder(back.oracle_text.as_deref()?)?
            != concat!(
                "+1: Create a 1/1 black Wolf creature token with deathtouch.\n",
                "−1: Sacrifice a creature. If you do, search your library for a creature card, reveal it, put it into your hand, then shuffle.\n",
                "−3: Creatures you control gain trample and get +X/+X until end of turn, where X is the number of creature cards in your graveyard."
            )
        || indicator? != ["B", "G"]
    {
        return None;
    }
    let oracle = card.oracle_id.as_deref()?;
    let loyalty = inventory.source_loyalty(oracle, 0)?.parse().ok()?;
    let mut initial = planeswalker_face(
        front.type_line.as_deref()?,
        Some(fixed_cost(front.mana_cost.as_deref()?)?),
        loyalty,
    )?;
    let mut transformed = planeswalker_face(back.type_line.as_deref()?, None, 0)?;
    initial.names = ObjectNames::single(EnglishCardName::new(front.name.clone()).ok()?);
    transformed.names = ObjectNames::single(EnglishCardName::new(back.name.clone()).ok()?);
    transformed.color_indicator = Some([Color::Black, Color::Green].into());
    initial.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: spellwake_engine::program::TriggerSpec::state_threshold(
            spellwake_engine::program::StateThresholdKind::SourceCounters {
                kind: CounterKind::Loyalty,
                comparison: Comparison::LessOrEqual,
                amount: 2,
            },
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![Instruction::TransformSource]),
    });
    let add = |program: &mut Program, id, cost, target, instructions| {
        let id = ActivatedAbilityId(id);
        program.loyalty_costs.insert(id, cost);
        program.activated.push(ActivatedAbility {
            id,
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(target, instructions),
        });
    };
    add(
        &mut initial,
        1,
        0,
        BodyTarget::SingleCreature,
        vec![
            Instruction::Damage { amount: 3.into() },
            Instruction::DamageFromPower(PowerDamageSource::TargetCreatureToAbilitySource),
        ],
    );
    add(
        &mut initial,
        2,
        0,
        BodyTarget::None,
        vec![ordinary_text::create_tokens(
            "a 2/2 green Wolf creature token",
            vocabulary,
        )?],
    );
    add(
        &mut transformed,
        0,
        1,
        BodyTarget::None,
        vec![ordinary_text::create_tokens(
            "a 1/1 black Wolf creature token with deathtouch",
            vocabulary,
        )?],
    );
    add(
        &mut transformed,
        1,
        -1,
        BodyTarget::None,
        vec![
            Instruction::SacrificeToContinue {
                filter: PermanentTypeFilter {
                    any_types: BTreeSet::new(),
                    required_types: [CardType::Creature].into(),
                    forbidden_types: BTreeSet::new(),
                },
            },
            Instruction::SearchLibrary {
                filter: Box::new(LibrarySearch::card_type_to_hand(CardType::Creature)),
            },
        ],
    );
    add(
        &mut transformed,
        2,
        -3,
        BodyTarget::None,
        vec![Instruction::ModifyGroupByAmount {
            group: Box::new(EffectSubject::Matching(
                ObjectFilter::creature().controlled(),
            )),
            scale: PowerToughness {
                power: 1.into(),
                toughness: 1.into(),
            },
            amount: spellwake_engine::program::ValueExpr::count_graveyard(
                spellwake_engine::program::ResolvingPlayer::Controller,
                spellwake_engine::program::ObjectFilter::creature(),
            ),
            keywords: [KeywordAbility::Trample].into(),
        }],
    );
    initial.transform_back = Some(Box::new(transformed));
    Some(initial)
}
