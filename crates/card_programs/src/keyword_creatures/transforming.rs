//! Compile transforming faces with independent Oracle abilities and upkeep conditions.
use super::*;
use spellwake_engine::{
    activated::SourceCost,
    characteristics::{
        AbilityChange, Color, CombatSelector, ControllerSelector, NumericExpression, NumericPair,
        PermanentSelector, StaticPart, StaticParts,
    },
    life::LifeAdjustment,
    program::{
        AttackRequirement, BodyTarget, CardType, DamageGroup as DamageCohort, Instruction,
        PlayerRelation, PreviousTurnCastCondition, ProgramKind, ResolvingPlayer,
    },
    turn::Step,
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    indicator: Option<&[String]>,
) -> Option<Program> {
    let [front, back] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "transform"
        || card.name != format!("{} // {}", front.name, back.name)
        || !back.mana_cost.as_deref().unwrap_or("").is_empty()
    {
        return None;
    }
    let (mut front, front_transforms) = compile_face(card, front, vocabulary, false)?;
    let (mut back, back_transforms) = compile_face(card, back, vocabulary, true)?;
    if !front_transforms && !back_transforms {
        return None;
    }
    if let Some(indicator) = indicator {
        back.color_indicator = Some(color_indicator(indicator)?);
    }
    front.transform_back = Some(Box::new(back));
    Some(front)
}

fn color_indicator(indicator: &[String]) -> Option<BTreeSet<Color>> {
    indicator
        .iter()
        .map(|color| match color.as_str() {
            "W" => Some(Color::White),
            "U" => Some(Color::Blue),
            "B" => Some(Color::Black),
            "R" => Some(Color::Red),
            "G" => Some(Color::Green),
            _ => None,
        })
        .collect()
}

fn compile_face(
    card: &InventoryCard,
    face: &CatalogFace,
    vocabulary: &CreatureTypeVocabulary,
    reverse: bool,
) -> Option<(Program, bool)> {
    use super::avacyn_restored::{activation, add_trigger};
    use BodyTarget::None as NoTarget;
    use Instruction::*;
    use spellwake_engine::program::{Comparison, ResolutionCondition};
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut reduced = card.clone();
    reduced.layout = "normal".into();
    reduced.name = face.name.clone();
    reduced.faces = vec![face.clone()];
    if reverse {
        reduced.faces[0].mana_cost = Some("{0}".into());
    }
    let mut remaining = Vec::new();
    let mut custom = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if is_transform_clause(line) {
            custom.push(line);
        } else {
            remaining.push(line);
        }
    }
    reduced.faces[0].oracle_text = Some(remaining.join("\n"));
    let mut program = if remaining.is_empty() && face.type_line.as_deref() == Some("Artifact") {
        Program::artifact(
            Some(fixed_cost(reduced.faces[0].mana_cost.as_deref()?)?),
            vec![],
        )
    } else if face.type_line.as_deref() == Some("Legendary Artifact — Equipment") {
        reduced.faces[0].type_line = Some("Artifact — Equipment".to_owned());
        let mut equipment = super::attachments::compile(&reduced, vocabulary, Profile::Current)?;
        equipment
            .supertypes
            .insert(spellwake_engine::program::Supertype::Legendary);
        equipment
    } else {
        super::compile(&reduced, vocabulary, Profile::Current).ok()?
    };
    program.names = ObjectNames::single(EnglishCardName::new(face.name.clone()).ok()?);
    if reverse {
        let ProgramKind::Permanent(permanent) = &mut program.kind else {
            return None;
        };
        permanent.casting_cost = None;
    }
    let mut transforms = false;
    let upkeep = spellwake_engine::program::TriggerSpec::step_began(
        Step::Upkeep,
        PlayerRelation::Controller,
    );
    for clause in custom {
        let (condition, instructions) = match clause {
            "When equipped creature deals combat damage to a player, unattach Elbrus, then transform it." if !reverse => {
                transforms = true;
                (spellwake_engine::program::TriggerSpec::deals_damage(spellwake_engine::program::ObjectFilter::any().attached_to_self(), spellwake_engine::program::DamageTriggerRecipient::AnyPlayer, true), vec![Unattach, TransformSource])
            }
            "Whenever a player loses the game, put thirteen +1/+1 counters on Withengar Unbound." if reverse => {
                (spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::PlayerLosesGame(PlayerRelation::AnyParticipant)),
                    vec![PutCountersOnSource { amount: 13.into() }])
            }
            "At the beginning of each upkeep, if no spells were cast last turn, transform this creature."
                if !reverse =>
            {
                transforms = true;
                (
                    spellwake_engine::program::TriggerSpec::step_began(spellwake_engine::turn::Step::Upkeep, spellwake_engine::program::PlayerRelation::AnyParticipant).only_if(spellwake_engine::program::TriggerCondition::PreviousTurnCasts(PreviousTurnCastCondition::NoSpells)),
                    vec![TransformSource],
                )
            }
            "At the beginning of each upkeep, if a player cast two or more spells last turn, transform this creature."
                if reverse =>
            {
                transforms = true;
                (
                    spellwake_engine::program::TriggerSpec::step_began(spellwake_engine::turn::Step::Upkeep, spellwake_engine::program::PlayerRelation::AnyParticipant).only_if(spellwake_engine::program::TriggerCondition::PreviousTurnCasts(PreviousTurnCastCondition::OnePlayerCastAtLeastTwo,)),
                    vec![TransformSource],
                )
            }
            "At the beginning of your upkeep, look at the top card of your library. You may reveal that card. If an instant or sorcery card is revealed this way, transform this creature."
                if !reverse =>
            {
                transforms = true;
                (
                    upkeep.clone(),
                    vec![
                        MayRevealLibraryTop,
                        BranchIfFalse {
                            condition: ResolutionCondition {
                                left: spellwake_engine::program::ValueExpr::object_matches(spellwake_engine::program::MatchObjectRef::RevealedTop, spellwake_engine::program::ObjectFilter::any_types(spellwake_engine::program::CardTypeMask::from_card_type(CardType::Instant).union(spellwake_engine::program::CardTypeMask::from_card_type(CardType::Sorcery)))),
                                comparison: Comparison::Equal,
                                right: spellwake_engine::program::ValueExpr::Literal(1.into()),
                            },
                            destination: 3,
                        },
                        TransformSource,
                    ],
                )
            }
            "{T}: You gain 1 life. Then if you have at least 10 life more than your starting life total, transform this artifact."
                if !reverse =>
            {
                transforms = true;
                program.activated.push(activation(
                    spellwake_engine::mana::ManaCost::free(),
                    SourceCost::Tap,
                    NoTarget,
                    vec![
                        AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(1.into()),
                        },
                        BranchIfFalse {
                            condition: ResolutionCondition {
                                left: spellwake_engine::program::ValueExpr::PlayerStat(spellwake_engine::program::PlayerStatExpr::ControllerLifeAboveStarting),
                                comparison: Comparison::GreaterOrEqual,
                                right: spellwake_engine::program::ValueExpr::Literal(10.into()),
                            },
                            destination: 3,
                        },
                        TransformSource,
                    ],
                ));
                continue;
            }
            "{T}: Target player loses 5 life." if reverse => {
                program.activated.push(activation(
                    spellwake_engine::mana::ManaCost::free(),
                    SourceCost::Tap,
                    BodyTarget::SinglePlayer,
                    vec![AdjustTargetLife {
                        adjustment: LifeAdjustment::Lose(5.into()),
                    }],
                ));
                continue;
            }
            "Whenever this creature transforms into Werewolf Ransacker, you may destroy target artifact. If that artifact is put into a graveyard this way, this creature deals 3 damage to that artifact's controller."
                if reverse =>
            {
                add_trigger(
                    &mut program,
                    spellwake_engine::program::TriggerSpec::status_changed(spellwake_engine::program::ObjectFilter::self_object(), spellwake_engine::program::StatusTriggerKind::Transforms),
                    BodyTarget::FilteredPermanent(
                        spellwake_engine::program::ObjectFilter::artifact(),
                    ),
                    vec![
                        OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        },
                        DestroyTarget,
                        DamageGroup {
                            group: DamageCohort::DestroyedTargetController,
                            amount: 3.into(),
                        },
                    ],
                );
                continue;
            }
            "Sacrifice a Human: Transform this creature. Activate only as a sorcery."
                if !reverse =>
            {
                transforms = true;
                let ability = activation(
                    spellwake_engine::mana::ManaCost::free(),
                    SourceCost::SacrificeSelected {
                        filter: spellwake_engine::activated::SelectedSacrificeFilter::Subtype(
                            vocabulary.resolve_label("Human")?,
                        ),
                        count: 1,
                        tap_source: false,
                        from_graveyard: false,
                    },
                    NoTarget,
                    vec![TransformSource],
                );
                program.activation_restrictions.insert(
                    ability.id,
                    spellwake_engine::activated::ActivationRestriction::Sorcery,
                );
                program.activated.push(ability);
                continue;
            }
            "At the beginning of your upkeep, sacrifice a Human. If you can't, tap this creature and it deals 9 damage to you."
                if reverse =>
            {
                (
                    upkeep.clone(),
                    vec![
                        SacrificeMatching(spellwake_engine::program::SacrificeSelection::Subtype(
                            vocabulary.resolve_label("Human")?,
                        )),
                        BranchIfFalse {
                            condition: ResolutionCondition {
                                left: spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::LastSacrificeSucceeded),
                                comparison: Comparison::Equal,
                                right: spellwake_engine::program::ValueExpr::Literal(0.into()),
                            },
                            destination: 4,
                        },
                        TapAbilitySource,
                        DamageGroup {
                            group: DamageCohort::Controller,
                            amount: 9.into(),
                        },
                    ],
                )
            }
            "At the beginning of your upkeep, you may transform this creature." => {
                transforms = true;
                (
                    upkeep.clone(),
                    vec![
                        OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        },
                        TransformSource,
                    ],
                )
            }
            "At the beginning of your upkeep, you may pay {2}{B}{B}. If you do, transform this creature." =>
            {
                transforms = true;
                (
                    upkeep.clone(),
                    vec![
                        OfferManaPayment {
                            payer: ResolvingPlayer::Controller,
                            cost: fixed_cost("{2}{B}{B}")?,
                            paid_destination: 1,
                            declined_destination: 2,
                        },
                        TransformSource,
                    ],
                )
            }
            "Whenever another creature you control dies, you may transform this creature." => {
                transforms = true;
                (
                    spellwake_engine::program::TriggerSpec::dies(spellwake_engine::program::ObjectFilter::creature().controlled().other_than_ability_source()),
                    vec![
                        OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        },
                        TransformSource,
                    ],
                )
            }
            "{T}, Tap an untapped Vampire you control: Transform this creature." => {
                transforms = true;
                let mut ability = activation(
                    ManaCost::free(),
                    SourceCost::TapPermanents {
                        filter: spellwake_engine::activated::TapCostFilter::Subtype(
                            vocabulary.resolve_label("Vampire")?,
                        ),
                        count: spellwake_engine::activated::TapCostCount::Fixed(
                            std::num::NonZeroU8::MIN,
                        ),
                        tap_source: true,
                        exclude_source: true,
                    },
                    NoTarget,
                    vec![TransformSource],
                );
                ability.id = ActivatedAbilityId(program.activated.len().try_into().ok()?);
                program.activated.push(ability);
                continue;
            }
            "{T}: This creature deals 2 damage to target creature with flying." => {
                let ability = activation(
                    ManaCost::free(),
                    SourceCost::Tap,
                    BodyTarget::FilteredPermanent(
                        spellwake_engine::program::ObjectFilter::creature()
                            .with_keyword(spellwake_engine::characteristics::KeywordAbility::Flying),
                    ),
                    vec![Damage { amount: 2.into() }],
                );
                program.activated.push(ability);
                continue;
            }
            "{T}: Create a 2/2 black Vampire creature token with flying." => {
                let mut ability = activation(
                    spellwake_engine::mana::ManaCost::free(),
                    SourceCost::Tap,
                    NoTarget,
                    vec![super::ordinary_text::create_tokens(
                        "a 2/2 black Vampire creature token with flying",
                        vocabulary,
                    )?],
                );
                ability.id =
                    spellwake_engine::activated::ActivatedAbilityId(program.activated.len() as u64);
                program.activated.push(ability);
                continue;
            }
            "{B}: Transform this creature. Activate only if you control five or more Vampires."
                if !reverse =>
            {
                use spellwake_engine::activated::{ActivatedAbilityId, ActivationRestriction};
                let mut ability = activation(
                    fixed_cost("{B}")?,
                    SourceCost::None,
                    NoTarget,
                    vec![TransformSource],
                );
                ability.id = ActivatedAbilityId(program.activated.len() as u64);
                program.activation_restrictions.insert(
                    ability.id,
                    ActivationRestriction::ControlsSubtypeCount {
                        kind: vocabulary.resolve_label("Vampire")?,
                        minimum: std::num::NonZeroU8::new(5)?,
                    },
                );
                program.activated.push(ability);
                transforms = true;
                continue;
            }
            "{R}, {T}: This creature fights target creature." => {
                program.activated.push(activation(
                    fixed_cost("{R}")?,
                    SourceCost::Tap,
                    BodyTarget::SingleCreature,
                    vec![FightSourceWithTarget],
                ));
                continue;
            }
            "{1}{U}: Put a hatchling counter on this creature. Then if there are five or more hatchling counters on it, remove all of them and transform it." =>
            {
                use spellwake_engine::{
                    effects::CounterKind,
                    program::{Comparison, ResolutionCondition},
                };
                transforms = true;
                let kind = CounterKind::Hatchling;
                program.activated.push(activation(
                    fixed_cost("{1}{U}")?,
                    SourceCost::None,
                    NoTarget,
                    vec![
                        PutNamedCounterOnSource { kind, amount: 1 },
                        BranchIfFalse {
                            condition: ResolutionCondition {
                                left: spellwake_engine::program::ValueExpr::counters(spellwake_engine::program::CounterSubjectRef::AbilitySource, Some(kind)),
                                comparison: Comparison::GreaterOrEqual,
                                right: spellwake_engine::program::ValueExpr::Literal(5.into()),
                            },
                            destination: 4,
                        },
                        ClearSourceCounters(kind),
                        TransformSource,
                    ],
                ));
                continue;
            }
            "{1}{G}: This creature gets +2/+2 until end of turn. Activate only once each turn."
            | "{3}{G}: This creature gets +4/+4 until end of turn. Activate only once each turn." =>
            {
                let (cost, boost) = if clause.starts_with("{1}") {
                    ("{1}{G}", 2)
                } else {
                    ("{3}{G}", 4)
                };
                let ability = activation(
                    fixed_cost(cost)?,
                    SourceCost::None,
                    NoTarget,
                    vec![ChangeAbilitySource {
                        parts: spellwake_engine::characteristics::ResolvedPartsSpec(vec![
                            spellwake_engine::characteristics::ResolvedPartSpec::ModifyPT(
                                spellwake_engine::characteristics::PowerToughness {
                                    power: boost.into(),
                                    toughness: boost.into(),
                                },
                            ),
                        ]),
                        duration:
                            spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                    }],
                );
                program.activation_restrictions.insert(
                    ability.id,
                    spellwake_engine::activated::ActivationRestriction::OnceEachTurn,
                );
                program.activated.push(ability);
                continue;
            }
            "{T}: Draw a card, then discard a card. If a creature card is discarded this way, untap this creature, then transform it." => {
                transforms = true;
                program.activated.push(activation(
                    fixed_cost("{0}")?,
                    SourceCost::Tap,
                    NoTarget,
                    vec![
                        DrawController { amount: spellwake_engine::program::ValueExpr::Literal(1.into()) },
                        Discard {
                            players: spellwake_engine::program::AffectedPlayers::One(ResolvingPlayer::Controller),
                            amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        },
                        BranchIfFalse {
                            condition: ResolutionCondition {
                                left: spellwake_engine::program::ValueExpr::object_matches(spellwake_engine::program::MatchObjectRef::DiscardedCard, spellwake_engine::program::ObjectFilter::any_types(spellwake_engine::program::CardTypeMask::from_card_type(CardType::Creature))),
                                comparison: Comparison::Equal,
                                right: spellwake_engine::program::ValueExpr::Literal(1.into()),
                            },
                            destination: 5,
                        },
                        UntapAbilitySource,
                        TransformSource,
                    ],
                ));
                continue;
            }
            "At the beginning of your end step, if this creature didn't attack this turn, tap this creature, then transform it." => (
                spellwake_engine::program::TriggerSpec::step_began(spellwake_engine::turn::Step::End, spellwake_engine::program::PlayerRelation::Controller).only_if(spellwake_engine::program::TriggerCondition::SourceDidNotAttackThisTurn),
                vec![
                    BranchIfFalse {
                        condition: ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::SourceAttackedThisTurn),
                            comparison: Comparison::Equal,
                            right: spellwake_engine::program::ValueExpr::Literal(0.into()),
                        },
                        destination: 3,
                    },
                    TapAbilitySource,
                    TransformSource,
                ],
            ),
            "When this creature deals combat damage to a player, you may transform it. If you do, attach it to target creature that player controls." if !reverse => {
                transforms = true;
                add_trigger(
                    &mut program, spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
                    BodyTarget::FilteredPermanent(spellwake_engine::program::ObjectFilter::creature().of_related_player()),
                    vec![
                        OptionalEffect { chooser: ResolvingPlayer::Controller },
                        TransformSourceAndAttachToTarget,
                    ],
                );
                continue;
            }
            "At the beginning of your end step, you lose 1 life." => (
                spellwake_engine::program::TriggerSpec::step_began(Step::End, PlayerRelation::Controller),
                vec![AdjustControllerLife {
                    adjustment: LifeAdjustment::Lose(1.into()),
                }],
            ),
            "When this creature dies, return it to the battlefield transformed under your control at the beginning of the next end step." if !reverse => {
                transforms = true;
                (
                    spellwake_engine::program::TriggerSpec::dies(spellwake_engine::program::ObjectFilter::self_object()),
                    vec![ScheduleGraveyardReturnAtEndStep(
                        spellwake_engine::program::DelayedGraveyardReturn::DyingSourceTransformed,
                    )],
                )
            }
            "Whenever this creature deals combat damage to a player, draw a card." => (
                spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
                vec![DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                }],
            ),
            "Whenever an opponent casts a spell, this creature deals 2 damage to that player." => (
                spellwake_engine::program::TriggerSpec::spell_cast(PlayerRelation::Opponent, spellwake_engine::program::SpellFilter::any()),
                vec![DamageGroup {
                    group: DamageCohort::RelatedPlayer,
                    amount: 2.into(),
                }],
            ),
            "Whenever this creature enters or transforms into Huntmaster of the Fells, create a 2/2 green Wolf creature token and you gain 2 life." if !reverse => (
                spellwake_engine::program::TriggerSpec::any_of(vec![spellwake_engine::program::TriggerEvent::Enters { subject: spellwake_engine::program::ObjectFilter::self_object(), from_zone: spellwake_engine::program::TriggerFromZone::Any, cast_origin: None, attached_to_creature: false }, spellwake_engine::program::TriggerEvent::StatusChanged { subject: spellwake_engine::program::ObjectFilter::self_object(), kind: spellwake_engine::program::StatusTriggerKind::Transforms }]),
                vec![
                    ordinary_text::create_tokens("a 2/2 green Wolf creature token", vocabulary)?,
                    AdjustControllerLife { adjustment: LifeAdjustment::Gain(2.into()) },
                ],
            ),
            "Whenever this creature transforms into Ravager of the Fells, it deals 2 damage to target opponent or planeswalker and 2 damage to up to one target creature that player or that planeswalker's controller controls." if reverse => {
                add_trigger(
                    &mut program, spellwake_engine::program::TriggerSpec::status_changed(spellwake_engine::program::ObjectFilter::self_object(), spellwake_engine::program::StatusTriggerKind::Transforms),
                    BodyTarget::OpponentOrPlaneswalkerThenCreature,
                    vec![DamageOrderedTargets { first: 2.into(), others: 2.into() }],
                );
                continue;
            }
            "At the beginning of your end step, create a 2/2 green Wolf creature token." => (
                spellwake_engine::program::TriggerSpec::step_began(Step::End, PlayerRelation::Controller),
                vec![ordinary_text::create_tokens(
                    "a 2/2 green Wolf creature token",
                    vocabulary,
                )?],
            ),
            "This creature must be blocked if able." => {
                static_effect(
                    &mut program,
                    PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..Default::default()
                    },
                    vec![StaticPart::BlockingRestriction(
                        spellwake_engine::characteristics::BlockingRestriction::MustBeBlocked,
                    )],
                )?;
                continue;
            }
            "This creature attacks each combat if able." => {
                program.attack_requirement = Some(AttackRequirement::SourceAttacks);
                continue;
            }
            "Attacking creatures you control get +1/+0."
            | "Attacking creatures you control get +3/+0." => {
                let power = if clause.contains("+1/") { 1 } else { 3 };
                static_effect(
                    &mut program,
                    PermanentSelector {
                        combat_role: Some(CombatSelector::Attacking),
                        ..controlled_creatures()
                    },
                    vec![stat_bonus(power, 0)],
                )?;
                continue;
            }
            "Other Vampire creatures you control get +2/+2." => {
                static_effect(
                    &mut program,
                    PermanentSelector {
                        creature_subtype: Some(vocabulary.resolve_label("Vampire")?),
                        exclude_source: true,
                        ..controlled_creatures()
                    },
                    vec![stat_bonus(2, 2)],
                )?;
                continue;
            }
            "Other Human creatures you control get +1/+1." => {
                static_effect(
                    &mut program,
                    PermanentSelector {
                        creature_subtype: Some(vocabulary.resolve_label("Human")?),
                        exclude_source: true,
                        ..controlled_creatures()
                    },
                    vec![stat_bonus(1, 1)],
                )?;
                continue;
            }
            "Each other creature you control that's a Werewolf or a Wolf gets +1/+1." => {
                let werewolf = vocabulary.resolve_label("Werewolf")?;
                static_effect(
                    &mut program,
                    PermanentSelector {
                        creature_subtype: Some(werewolf),
                        exclude_source: true,
                        ..controlled_creatures()
                    },
                    vec![stat_bonus(1, 1)],
                )?;
                static_effect(
                    &mut program,
                    PermanentSelector {
                        creature_subtype: Some(vocabulary.resolve_label("Wolf")?),
                        forbidden_creature_subtype: Some(werewolf),
                        exclude_source: true,
                        ..controlled_creatures()
                    },
                    vec![stat_bonus(1, 1)],
                )?;
                continue;
            }
            "Werewolves you control have menace." => {
                static_effect(
                    &mut program,
                    PermanentSelector {
                        creature_subtype: Some(vocabulary.resolve_label("Werewolf")?),
                        controller: ControllerSelector::SourceController,
                        ..Default::default()
                    },
                    vec![StaticPart::Abilities(AbilityChange::grant_keywords([
                        KeywordAbility::Menace,
                    ]))],
                )?;
                continue;
            }
            _ => return None,
        };
        add_trigger(&mut program, condition, NoTarget, instructions);
    }
    Some((program, transforms))
}

fn is_transform_clause(line: &str) -> bool {
    matches!(
        line,
        "When equipped creature deals combat damage to a player, unattach Elbrus, then transform it."
            | "Whenever a player loses the game, put thirteen +1/+1 counters on Withengar Unbound."
            | "At the beginning of each upkeep, if no spells were cast last turn, transform this creature."
            | "At the beginning of each upkeep, if a player cast two or more spells last turn, transform this creature."
            | "At the beginning of your upkeep, you may transform this creature."
            | "Sacrifice a Human: Transform this creature. Activate only as a sorcery."
            | "At the beginning of your upkeep, sacrifice a Human. If you can't, tap this creature and it deals 9 damage to you."
            | "Whenever this creature transforms into Werewolf Ransacker, you may destroy target artifact. If that artifact is put into a graveyard this way, this creature deals 3 damage to that artifact's controller."
            | "At the beginning of your upkeep, look at the top card of your library. You may reveal that card. If an instant or sorcery card is revealed this way, transform this creature."
            | "{T}: You gain 1 life. Then if you have at least 10 life more than your starting life total, transform this artifact."
            | "{T}: Target player loses 5 life."
            | "At the beginning of your upkeep, you may pay {2}{B}{B}. If you do, transform this creature."
            | "Whenever another creature you control dies, you may transform this creature."
            | "{T}, Tap an untapped Vampire you control: Transform this creature."
            | "{T}: This creature deals 2 damage to target creature with flying."
            | "{R}, {T}: This creature fights target creature."
            | "{T}: Create a 2/2 black Vampire creature token with flying."
            | "{B}: Transform this creature. Activate only if you control five or more Vampires."
            | "Other Vampire creatures you control get +2/+2."
            | "{1}{U}: Put a hatchling counter on this creature. Then if there are five or more hatchling counters on it, remove all of them and transform it."
            | "{1}{G}: This creature gets +2/+2 until end of turn. Activate only once each turn."
            | "{3}{G}: This creature gets +4/+4 until end of turn. Activate only once each turn."
            | "{T}: Draw a card, then discard a card. If a creature card is discarded this way, untap this creature, then transform it."
            | "At the beginning of your end step, if this creature didn't attack this turn, tap this creature, then transform it."
            | "At the beginning of your end step, you lose 1 life."
            | "When this creature deals combat damage to a player, you may transform it. If you do, attach it to target creature that player controls."
            | "When this creature dies, return it to the battlefield transformed under your control at the beginning of the next end step."
            | "Whenever this creature deals combat damage to a player, draw a card."
            | "Whenever an opponent casts a spell, this creature deals 2 damage to that player."
            | "At the beginning of your end step, create a 2/2 green Wolf creature token."
            | "Whenever this creature enters or transforms into Huntmaster of the Fells, create a 2/2 green Wolf creature token and you gain 2 life."
            | "Whenever this creature transforms into Ravager of the Fells, it deals 2 damage to target opponent or planeswalker and 2 damage to up to one target creature that player or that planeswalker's controller controls."
            | "This creature must be blocked if able."
            | "This creature attacks each combat if able."
            | "Attacking creatures you control get +1/+0."
            | "Attacking creatures you control get +3/+0."
            | "Other Human creatures you control get +1/+1."
            | "Each other creature you control that's a Werewolf or a Wolf gets +1/+1."
            | "Werewolves you control have menace."
    )
}

fn controlled_creatures() -> PermanentSelector {
    PermanentSelector {
        required_types: [CardType::Creature].into(),
        controller: ControllerSelector::SourceController,
        ..Default::default()
    }
}

fn static_effect(
    program: &mut Program,
    selector: PermanentSelector,
    parts: Vec<StaticPart>,
) -> Option<()> {
    super::army_grants::add_battlefield_ability(program, selector, StaticParts(parts))
}

fn stat_bonus(power: i64, toughness: i64) -> StaticPart {
    StaticPart::ModifyPT(NumericPair {
        power: NumericExpression::Literal(power.into()),
        toughness: NumericExpression::Literal(toughness.into()),
    })
}
