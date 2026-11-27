//! Oracle clauses that can be composed from existing rules in the original Innistrad block.
use super::{Profile, fixed_cost, ordinary_creature, ordinary_text};
use spellwake_content::InventoryCard;
use spellwake_contracts::{creature_types::CreatureTypeVocabulary, names::EnglishCardName};
use spellwake_engine::{
    activated::{ActivationRestriction, SourceCost},
    attachment::EnchantRestriction,
    characteristics::{
        AbilityChange, BlockingRestriction, ControllerSelector, KeywordAbility, PermanentSelector,
        PowerToughness, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, StaticAbilityId,
        StaticPart, StaticParts,
    },
    land::EntryTap,
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    program::{
        BodyTarget, Comparison, EffectSubject, ExecutableBody, Instruction, LibraryPlacement,
        ObjectClause, ObjectFilter, PlayerRelation, PowerFilter, Program, ProgramKind,
        ResolutionCondition, ResolvingPlayer, ValueExpr,
    },
    turn::Step,
};

fn amount(value: i64) -> ValueExpr {
    spellwake_engine::program::ValueExpr::Literal(value.into())
}

fn stats(power: i64, toughness: i64) -> PowerToughness {
    PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    }
}

fn modify(parts: Vec<ResolvedPartSpec>) -> Instruction {
    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
        spellwake_engine::toolbox::ObjectQuery::Targets,
        ResolvedPartsSpec(parts),
        ResolvedDuration::UntilEndOfTurn,
    )
}

fn keywords(values: impl IntoIterator<Item = KeywordAbility>) -> ResolvedPartSpec {
    ResolvedPartSpec::Abilities(AbilityChange::grant_keywords(values))
}

fn cannot_block() -> Instruction {
    modify(vec![ResolvedPartSpec::BlockingRestriction(
        BlockingRestriction::CannotBlock,
    )])
}

fn choose_effect(
    left: ValueExpr,
    comparison: Comparison,
    right: i64,
    when_true: Instruction,
    when_false: Instruction,
) -> Vec<Instruction> {
    vec![
        Instruction::BranchCondition {
            condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                Box::new(ResolutionCondition {
                    left,
                    comparison,
                    right: spellwake_engine::program::ValueExpr::Literal(right.into()),
                }),
            )),
            destination: 3,
        },
        when_true,
        Instruction::Jump { destination: 4 },
        when_false,
    ]
}

fn tokens(
    description: &str,
    count: ValueExpr,
    tapped: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Instruction> {
    let Instruction::CreateTokens { mut creations } =
        ordinary_text::create_tokens(description, vocabulary)?
    else {
        return None;
    };
    let [creation] = creations.as_mut_slice() else {
        return None;
    };
    creation.amount = count;
    creation.enters_tapped = tapped;
    Some(Instruction::CreateTokens { creations })
}

pub(super) fn spell(
    rules: &str,
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ExecutableBody> {
    use BodyTarget::{
        FilteredPermanent, None as NoTarget, SingleAnyTarget, SingleCreature, SingleOpponent,
        SinglePlayer,
    };
    use Instruction::*;
    let draw =
        |n| crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(amount(n));
    let lose = |n: i64| {
        crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
            spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer),
            amount(n),
        )
    };
    let damage = |n: i64| {
        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
            spellwake_engine::toolbox::DamageTargetQuery::Targets,
            amount(n),
            false,
        )
    };
    let counters = |n: i64| {
        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
            spellwake_engine::effects::CounterKind::PlusOne,
            amount(n),
        )
    };
    let subtype = |name| vocabulary.resolve_label(name);
    let vampires_werewolves_zombies = || {
        Some([
            subtype("Vampire")?,
            subtype("Werewolf")?,
            subtype("Zombie")?,
        ])
    };
    let clause = |target, op| FixedTargetClause(Box::new(ExecutableBody::new(target, vec![op])));
    let own_graveyard = BodyTarget::FilteredOwnGraveyardCard;
    let graveyard_return = match rules
        .strip_prefix("Return ")
        .and_then(|text| text.strip_suffix(" from your graveyard to your hand."))
    {
        Some("target creature card") => Some((ObjectFilter::creature(), 1)),
        Some("two target Zombie cards") => {
            Some((ObjectFilter::any().with_subtype(subtype("Zombie")?), 2))
        }
        _ => None,
    };
    if let Some((filter, count)) = graveyard_return {
        let body = ExecutableBody::new(
            own_graveyard(filter),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_hand()],
        );
        return Some(if count == 1 {
            body
        } else {
            body.with_graveyard_target_bounds(count, count)
        });
    }
    if rules
        == "Put any number of target creature cards from your graveyard on top of your library. Draw a card."
    {
        return Some(
            ExecutableBody::new(
                own_graveyard(ObjectFilter::creature()),
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::put_target_in_library(
                        LibraryPlacement::Top,
                    ),
                    draw(1),
                ],
            )
            .with_graveyard_target_bounds(0, usize::MAX),
        );
    }
    let (target, instructions) = match rules {
        "Search your library for a basic land card, reveal it, put it into your hand, then shuffle. Morbid — You may put that card onto the battlefield instead of putting it into your hand if a creature died this turn." => (
            NoTarget,
            vec![SearchLibrary { filter: Box::new(spellwake_engine::program::LibrarySearch::to_hand(
                    spellwake_engine::program::SearchCardMatcher::BasicLand,
                )
                .with_destination(
                    spellwake_engine::program::SearchDestination::HandOrBattlefieldIfCreatureDied,
                )) }],
        ),
        "Return target exiled card with flashback you own to your hand." => (
            BodyTarget::OwnedExiledFlashbackCard,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand()],
        ),
        "Target creature an opponent controls deals damage equal to its power to each other creature that player controls, then each of those creatures deals damage equal to its power to that creature." => (
            FilteredPermanent(ObjectFilter::creature().opposing()),
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec {
                        bind_moved: Some(spellwake_engine::toolbox::BindingId(0)),
                        ..spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Difference(
                                Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                        Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::TargetController,
                                            ),
                                        ),
                                    ),
                                    ObjectFilter::creature(),
                                )),
                                Box::new(spellwake_engine::toolbox::ObjectQuery::Targets),
                            ),
                            spellwake_engine::toolbox::ZoneDest::None,
                        )
                    },
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage_from(
                    spellwake_engine::toolbox::DamageSourceQuery::Objects(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                    ),
                    spellwake_engine::toolbox::DamageTargetQuery::Objects(
                        spellwake_engine::toolbox::ObjectQuery::Bound(
                            spellwake_engine::toolbox::BindingId(0),
                        ),
                    ),
                    spellwake_engine::program::DamageAmountSpec::SourcePower,
                    false,
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage_from(
                    spellwake_engine::toolbox::DamageSourceQuery::Objects(
                        spellwake_engine::toolbox::ObjectQuery::Bound(
                            spellwake_engine::toolbox::BindingId(0),
                        ),
                    ),
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::DamageAmountSpec::SourcePower,
                    false,
                ),
            ],
        ),
        "Creatures you control gain protection from non-Human creatures until end of turn." => (
            NoTarget,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                EffectSubject::Matching(ObjectFilter::creature().controlled()),
                amount(0),
                amount(0),
                [KeywordAbility::ProtectionFromSubtype(spellwake_engine::characteristics::SubtypeProtection {
                    subtype: subtype("Human")?,
                    creatures_only: true,
                    exclude_subtype: true,
                })].into(),
                Default::default(),
            )],
        ),
        "Corpse Lunge deals damage equal to the exiled card's power to target creature." if name == "Corpse Lunge" => (
            SingleCreature,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::object_stat(
                        spellwake_engine::program::ValueObjectRef::ExiledCostCreature,
                        spellwake_engine::program::ObjectStatKind::Power,
                    ),
                    false,
                ),
            ],
        ),
        "Harvest Pyre deals X damage to target creature." if name == "Harvest Pyre" => (
            SingleCreature,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::chosen_x(),
                    false,
                ),
            ],
        ),
        "Each instant and sorcery card in your graveyard gains flashback until end of turn. The flashback cost is equal to its mana cost." => (
            NoTarget,
            vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller))), ObjectFilter::instant_or_sorcery()), spellwake_engine::toolbox::ObjectMutation::GrantFlashback)))],
        ),
        "Destroy target creature. If that creature was a Human, you gain life equal to its toughness." => (
            SingleCreature,
            vec![
                Instruction::BranchCondition {
                    condition: Box::new(spellwake_engine::toolbox::ConditionExpr::AnyMatch {
                        query: Box::new(spellwake_engine::toolbox::ObjectQuery::Targets),
                        filter: Box::new(
                            spellwake_engine::program::ObjectFilter::any()
                                .with_subtype(subtype("Human")?),
                        ),
                    }),
                    destination: 4,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec {
                        bind_snapshots: Some(spellwake_engine::toolbox::BindingId(0)),
                        ..spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics:
                                    spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                        allow_regenerate: true,
                                    },
                            },
                        )
                    },
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::BoundStat {
                        binding: spellwake_engine::toolbox::BindingId(0),
                        stat: spellwake_engine::program::ObjectStatKind::Toughness,
                        op: spellwake_engine::program::StatAggregate::Sum,
                    },
                ),
                Instruction::Jump { destination: 5 },
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
        ),
        "Target player discards two cards. Morbid — If a creature died this turn, instead that player reveals their hand, you choose two cards from it, then that player discards those cards." => (
            SinglePlayer,
            choose_effect(
                spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::CreaturesDiedThisTurn),
                Comparison::Greater,
                0,
                Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::choose_revealed_hand_card(
                        0,
                        false,
                        None,
                        std::num::NonZeroU8::new(2)?,
                        spellwake_engine::program::ChosenHandAction::Discard,
                    ),
                )),
                Discard {
                    players: spellwake_engine::program::AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    amount: amount(2),
                },
            ),
        ),
        "Transform all Humans. Prevent all combat damage that would be dealt this turn by creatures other than Werewolves and Wolves." => (
            NoTarget,
            vec![
                Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::all(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Battlefield,
                            ObjectFilter::creature().with_subtype(subtype("Human")?),
                        ),
                        spellwake_engine::toolbox::ObjectMutation::Transform,
                    ),
                )),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                    spellwake_engine::damage::TurnPreventionSpec {
                        combat_only: true,
                        source: spellwake_engine::damage::PreventionSourceSpec::Filter(
                            spellwake_engine::damage::PreventionSourceFilter {
                                creatures_only: true,
                                forbidden_subtypes: [
                                    Some(subtype("Werewolf")?),
                                    Some(subtype("Wolf")?),
                                ],
                                ..Default::default()
                            },
                        ),
                        recipient: spellwake_engine::damage::PreventionRecipientSpec::Any,
                        both_directions: false,
                    },
                ),
            ],
        ),
        "Exile target creature and all other creatures with the same name as that creature." => (
            SingleCreature,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::TargetAndSameNameMatching(ObjectFilter::creature()),
                        )),
                        spellwake_engine::toolbox::ZoneDest::exile(),
                    ),
                ),
            ],
        ),
        "Exile all nonland permanents target player controls. Return the exiled cards to the battlefield under their owner's control at the beginning of the next end step." => (
            SinglePlayer,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::TargetPlayer,
                                )),
                            ),
                            ObjectFilter::nonland(),
                        ),
                        spellwake_engine::toolbox::ZoneDest::Exile {
                            face_down: false,
                            viewer: None,
                            link_to_source: false,
                            delayed_return: Some(
                                spellwake_engine::program::DelayedReturnTiming::NextEndStep,
                            ),
                            link_group: None,
                        },
                    ),
                ),
            ],
        ),
        "Counter target spell. If that spell is countered this way, exile it instead of putting it into its owner's graveyard." => {
            (
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::counter_targeted(spellwake_engine::toolbox::CounterDestination::Exile)))],
            )
        }
        "Target creature gains undying until end of turn." => {
            let trigger = spellwake_engine::program::CounterKeyword::Undying
                .ability(spellwake_engine::program::TriggeredAbilityId(0));
            let change = AbilityChange {
                granted_triggers: vec![trigger],
                ..Default::default()
            };
            (
                SingleCreature,
                vec![modify(vec![ResolvedPartSpec::Abilities(change)])],
            )
        }
        "Target permanent you control gains protection from the color of your choice until end of turn. Fateful hour — If you have 5 or less life, instead you and each permanent you control gain protection from the color of your choice until end of turn." => {
            use spellwake_engine::program::ProtectionRecipients;
            (
                FilteredPermanent(ObjectFilter::permanent().controlled()),
                vec![
                    BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::PlayerStat(spellwake_engine::program::PlayerStatExpr::ControllerLife),
                            comparison: Comparison::LessOrEqual,
                            right: spellwake_engine::program::ValueExpr::Literal(5.into()),
                        }))), destination: 3 },
                    Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::chosen_color_protection(ProtectionRecipients::ControllerAndPermanents, ResolvedDuration::UntilEndOfTurn))),
                    Jump { destination: 4 },
                    Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec::chosen_color_protection(ProtectionRecipients::TargetPermanent, ResolvedDuration::UntilEndOfTurn))),
                ],
            )
        }
        "Prevent all combat damage that would be dealt this turn. Fateful hour — If you have 5 or less life, tap all attacking creatures. Those creatures don't untap during their controller's next untap step." => {
            (
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                        spellwake_engine::damage::TurnPreventionSpec {
                            combat_only: true,
                            source: spellwake_engine::damage::PreventionSourceSpec::Any,
                            recipient: spellwake_engine::damage::PreventionRecipientSpec::Any,
                            both_directions: false,
                        },
                    ),
                    BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::PlayerStat(spellwake_engine::program::PlayerStatExpr::ControllerLife),
                            comparison: Comparison::LessOrEqual,
                            right: spellwake_engine::program::ValueExpr::Literal(5.into()),
                        }))), destination: 4 },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::set_group_tapped(
                        ObjectFilter::creature().attacking(),
                        true,
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::skip_next_untap(
                        EffectSubject::Matching(ObjectFilter::creature().attacking()),
                    ),
                ],
            )
        }
        "You gain 2 life for each creature card in your graveyard." => (
            NoTarget,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::count_graveyard(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                        spellwake_engine::program::ObjectFilter::creature(),
                    )
                    .scaled(i64::from(2)),
                ),
            ],
        ),
        "You gain 2 life for each card in your graveyard, then shuffle your graveyard into your library." => {
            (
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::count_graveyard(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                            spellwake_engine::program::ObjectFilter::any(),
                        )
                        .scaled(i64::from(2)),
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(spellwake_engine::program::ResolvingPlayer::Controller))), spellwake_engine::program::ObjectFilter::any()), spellwake_engine::toolbox::ZoneDest::Library { position: spellwake_engine::toolbox::LibraryOrderPos::ShuffleInto })),
                ],
            )
        }
        "Rolling Temblor deals 2 damage to each creature without flying."
            if name == "Rolling Temblor" =>
        {
            (
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                            spellwake_engine::program::DamageRecipientsRef::Group(
                                spellwake_engine::program::DamageGroup::CreaturesWithoutFlying,
                            ),
                        )),
                        amount(2),
                        false,
                    ),
                ],
            )
        }
        "Destroy target land. Scorch the Fields deals 1 damage to each Human creature."
            if name == "Scorch the Fields" =>
        {
            (
                FilteredPermanent(ObjectFilter::land()),
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } })),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                            spellwake_engine::program::DamageRecipientsRef::Group(
                                spellwake_engine::program::DamageGroup::CreaturesOfSubtype(
                                    subtype("Human")?,
                                ),
                            ),
                        )),
                        amount(1),
                        false,
                    ),
                ],
            )
        }
        "Destroy all enchantments. You gain 1 life for each enchantment destroyed this way." => (
            NoTarget,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(ObjectFilter::enchantment(), true),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::event(
                        spellwake_engine::program::EventValueKind::DestroyedPermanents,
                    ),
                ),
            ],
        ),
        "Discard all the cards in your hand, then draw that many cards." => (
            NoTarget,
            vec![
                Discard {
                    players: spellwake_engine::program::AffectedPlayers::One(
                        ResolvingPlayer::Controller,
                    ),
                    amount: spellwake_engine::program::ValueExpr::PlayerStat(spellwake_engine::program::PlayerStatExpr::LargestHandSize),
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::LargestDiscard),
                ),
            ],
        ),
        "Fling deals damage equal to the sacrificed creature's power to any target."
            if name == "Fling" =>
        {
            (
                SingleAnyTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::object_stat(
                            spellwake_engine::program::ValueObjectRef::SacrificedCreature,
                            spellwake_engine::program::ObjectStatKind::Power,
                        ),
                        false,
                    ),
                ],
            )
        }
        "Create five 1/1 white Human creature tokens. If this spell was cast from a graveyard, create ten of those tokens instead." => {
            (
                NoTarget,
                choose_effect(
                    spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::CastFromGraveyard),
                    Comparison::Equal,
                    1,
                    tokens(
                        "a 1/1 white Human creature token",
                        amount(10),
                        false,
                        vocabulary,
                    )?,
                    tokens(
                        "a 1/1 white Human creature token",
                        amount(5),
                        false,
                        vocabulary,
                    )?,
                ),
            )
        }
        "Put five +1/+1 counters on target creature. If this spell was cast from a graveyard, put ten +1/+1 counters on that creature instead." => {
            (
                SingleCreature,
                choose_effect(
                    spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::CastFromGraveyard),
                    Comparison::Equal,
                    1,
                    counters(10),
                    counters(5),
                ),
            )
        }
        "Target player reveals their hand. You choose a nonland card from it. Exile that card." => (
            SinglePlayer,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::choose_revealed_hand_card(
                    0,
                    true,
                    None,
                    std::num::NonZeroU8::MIN,
                    spellwake_engine::program::ChosenHandAction::Exile,
                ),
            ))],
        ),
        "Search your library for a card and put that card into your hand. If this spell was cast from a graveyard, instead search your library for two cards and put those cards into your hand. Then shuffle." => {
            use spellwake_engine::program::LibrarySearch;
            (
                NoTarget,
                choose_effect(
                    spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::CastFromGraveyard),
                    Comparison::Equal,
                    1,
                    SearchLibrary { filter: Box::new(LibrarySearch::any_to_hand(std::num::NonZeroU8::new(2)?)) },
                    SearchLibrary { filter: Box::new(LibrarySearch::any_to_hand(std::num::NonZeroU8::new(1)?)) },
                ),
            )
        }
        "Target opponent sacrifices a creature of their choice. You gain life equal to that creature's toughness." => (
            SingleOpponent,
            vec![
                Sacrifice {
                    players: spellwake_engine::program::AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    amount: amount(1),
                    filter: spellwake_engine::program::PermanentTypeFilter {
                        any_types: Default::default(),
                        required_types: [spellwake_engine::program::CardType::Creature].into(),
                        forbidden_types: Default::default(), object_filter: None },
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::object_stat(
                        spellwake_engine::program::ValueObjectRef::LastSacrificedCreature,
                        spellwake_engine::program::ObjectStatKind::Toughness,
                    ),
                ),
            ],
        ),
        "Return two cards at random from your graveyard to your hand." => (
            NoTarget,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_random_graveyard_cards(None, std::num::NonZeroU8::new(2)?, false)],
        ),
        "Counter target spell unless its controller pays {1}. That player discards a card." => (
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
            vec![
                OfferDynamicManaPayment {
                    payer: ResolvingPlayer::TargetSpellController,
                    fixed: fixed_cost("{1}")?,
                    extra_generic: spellwake_engine::program::ValueExpr::Literal(0.into()),
                    paid_destination: 2,
                    declined_destination: 1,
                },
                Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::counter_targeted(spellwake_engine::toolbox::CounterDestination::Graveyard))),
                Discard {
                    players: spellwake_engine::program::AffectedPlayers::One(ResolvingPlayer::TargetSpellController),
                    amount: amount(1),
                },
            ],
        ),
        "Copy target instant or sorcery spell you control. If this spell was cast from a graveyard, copy that spell twice instead. You may choose new targets for the copies." => (
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::instant_or_sorcery().controlled()),
            vec![
                Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell())),
                BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(ResolutionCondition {
                        left: spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::CastFromGraveyard),
                        comparison: Comparison::Equal,
                        right: spellwake_engine::program::ValueExpr::Literal(1.into()),
                    }))), destination: 3 },
                Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell())),
            ],
        ),
        "Target player mills X cards. If this spell was cast from a graveyard, that player mills twice that many cards instead." => (
            SinglePlayer,
            choose_effect(
                spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::CastFromGraveyard),
                Comparison::Equal,
                1,
                MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::mill(
                        ResolvingPlayer::TargetPlayer,
                        spellwake_engine::program::ValueExpr::chosen_x()
                            .scaled(i64::from((std::num::NonZeroU16::new(2)?).get())),
                    ),
                )),
                MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::mill(
                        ResolvingPlayer::TargetPlayer,
                        spellwake_engine::program::ValueExpr::chosen_x(),
                    ),
                )),
            ),
        ),
        "Devil's Play deals X damage to any target." if name == "Devil's Play" => (
            SingleAnyTarget,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::chosen_x(),
                    false,
                ),
            ],
        ),
        "Target creature deals damage to itself equal to its power." => (
            SingleCreature,
            vec![DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::from_power(spellwake_engine::program::PowerDamageSource::TargetCreature)))],
        ),
        "Destroy target noncreature permanent." => (
            FilteredPermanent(ObjectFilter::noncreature()),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } }))],
        ),
        "Destroy target Spirit or enchantment." => (
            FilteredPermanent(ObjectFilter::either(
                ObjectClause::any().with_subtype(subtype("Spirit")?),
                ObjectClause::enchantment(),
            )),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } }))],
        ),
        "Destroy target non-Vampire, non-Werewolf, non-Zombie creature." => (
            FilteredPermanent(ObjectFilter::creature().outside_subtypes(
                vampires_werewolves_zombies()?,
            )),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } }))],
        ),
        "Return target creature you own to your hand." => (
            FilteredPermanent(ObjectFilter::creature().owned()),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand()],
        ),
        "Target creature you control fights target creature you don't control." => (
            NoTarget,
            vec![
                clause(
                    FilteredPermanent(ObjectFilter::creature().controlled()),
                    Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
                    )),
                ),
                clause(
                    FilteredPermanent(ObjectFilter::creature().not_controlled()),
                    Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
                    )),
                ),
                Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::fight(spellwake_engine::toolbox::ObjectQuery::from_move_subject(spellwake_engine::program::MoveSubjectRef::RememberedFirst), spellwake_engine::toolbox::ObjectQuery::from_move_subject(spellwake_engine::program::MoveSubjectRef::RememberedSecond)))),
            ],
        ),
        "Target creature fights another target creature." => (
            NoTarget,
            vec![
                clause(
                    SingleCreature,
                    Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
                    )),
                ),
                clause(
                    SingleCreature,
                    Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
                    )),
                ),
                Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::fight(spellwake_engine::toolbox::ObjectQuery::from_move_subject(spellwake_engine::program::MoveSubjectRef::RememberedFirst), spellwake_engine::toolbox::ObjectQuery::from_move_subject(spellwake_engine::program::MoveSubjectRef::RememberedSecond)))),
            ],
        ),
        "Counter target spell. Return target permanent to its owner's hand." => (
            NoTarget,
            vec![
                clause(
                    BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                    Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::counter_targeted(spellwake_engine::toolbox::CounterDestination::Graveyard))),
                ),
                clause(BodyTarget::SinglePermanent, crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand()),
            ],
        ),
        "Destroy target land. Into the Maw of Hell deals 13 damage to target creature."
            if name == "Into the Maw of Hell" =>
        {
            (
                NoTarget,
                vec![
                    clause(
                        FilteredPermanent(ObjectFilter::land()),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } })),
                    ),
                    clause(SingleCreature, damage(13)),
                ],
            )
        }
        "Untap target creature. It gets +2/+4 and gains reach until end of turn." => (
            SingleCreature,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(false),
                modify(vec![
                    ResolvedPartSpec::ModifyPT(stats(2, 4)),
                    keywords([KeywordAbility::Reach]),
                ]),
            ],
        ),
        "Put target creature on top of its owner's library." => (
            SingleCreature,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_target_in_library(LibraryPlacement::Top)],
        ),
        "Target opponent loses 3 life." => (SingleOpponent, vec![lose(3)]),
        "Draw two cards, then discard a card at random." => {
            (NoTarget, vec![draw(2), crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_one_at_random(ResolvingPlayer::Controller)])
        }
        "Tap up to two target creatures." => {
            return Some(
                ExecutableBody::new(
                    SingleCreature,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(true)],
                )
                .with_permanent_target_bounds(0, 2),
            );
        }
        "Look at the top four cards of your library. Put one of them into your hand and the rest into your graveyard." => {
            (
                NoTarget,
                vec![LookTopChooseExt {
                    library_owner: ResolvingPlayer::Controller,
                    chooser: ResolvingPlayer::Controller,
                    count: amount(4),
                    filter: None,
                    max_picks: 1,
                    take_all_matching: false,
                    keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
                    rest_dest: spellwake_engine::program::LookTopRestDest::Graveyard,
                    reveal_kept: false,
                    reveal_all: false,
                    reveal_only: false,
                    secondary: None,
                }],
            )
        }
        "Exile target card from a graveyard." => {
            (BodyTarget::AnyGraveyardCard, vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target()])
        }
        "Up to two target creatures can't block this turn." => {
            return Some(
                ExecutableBody::new(SingleCreature, vec![cannot_block()])
                    .with_permanent_target_bounds(0, 2),
            );
        }
        "Target creature can't be blocked this turn." => (
            SingleCreature,
            vec![modify(vec![ResolvedPartSpec::BlockingRestriction(
                BlockingRestriction::CannotBeBlocked,
            )])],
        ),
        "Target creature gains deathtouch until end of turn and must be blocked this turn if able." => {
            (
                SingleCreature,
                vec![
                    modify(vec![keywords([KeywordAbility::Deathtouch])]),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::TargetObject, spellwake_engine::program::CombatActionKind::MustBeBlocked),
                ],
            )
        }
        "Target creature you control gets +1/+1 and gains hexproof until end of turn." => (
            FilteredPermanent(ObjectFilter::creature().controlled()),
            vec![modify(vec![
                ResolvedPartSpec::ModifyPT(stats(1, 1)),
                keywords([KeywordAbility::Hexproof]),
            ])],
        ),
        "Gain control of target creature until end of turn. Untap it. It gains trample and haste until end of turn." => {
            (
                SingleCreature,
                vec![
                    modify(vec![ResolvedPartSpec::Control(
                        spellwake_engine::characteristics::ControlRecipient::ResolvingController,
                    )]),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(false),
                    modify(vec![keywords([
                        KeywordAbility::Trample,
                        KeywordAbility::Haste,
                    ])]),
                ],
            )
        }
        "Target player draws three cards and loses 3 life." => (
            SinglePlayer,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_target_player(amount(3)), lose(3)],
        ),
        "Target player mills two cards. Draw a card." => (
            SinglePlayer,
            vec![
                MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::mill(
                        ResolvingPlayer::TargetPlayer,
                        amount(2),
                    ),
                )),
                draw(1),
            ],
        ),
        "Draw a card for each creature card in your graveyard." => (
            NoTarget,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                spellwake_engine::program::ValueExpr::count_graveyard(spellwake_engine::program::ResolvingPlayer::Controller, spellwake_engine::program::ObjectFilter::creature()),
            )],
        ),
        "Create thirteen tapped 2/2 black Zombie creature tokens." => (
            NoTarget,
            vec![tokens(
                "a 2/2 black Zombie creature token",
                amount(13),
                true,
                vocabulary,
            )?],
        ),
        "Create a 1/2 green Spider creature token with reach for each creature card in your graveyard." => {
            (
                NoTarget,
                vec![tokens(
                    "a 1/2 green Spider creature token with reach",
                    spellwake_engine::program::ValueExpr::count_graveyard(spellwake_engine::program::ResolvingPlayer::Controller, spellwake_engine::program::ObjectFilter::creature()),
                    false,
                    vocabulary,
                )?],
            )
        }
        "Create two 1/1 white Human creature tokens. Fateful hour — If you have 5 or less life, create five of those tokens instead." => {
            (
                NoTarget,
                choose_effect(
                    spellwake_engine::program::ValueExpr::PlayerStat(spellwake_engine::program::PlayerStatExpr::ControllerLife),
                    Comparison::LessOrEqual,
                    5,
                    tokens(
                        "a 1/1 white Human creature token",
                        amount(5),
                        false,
                        vocabulary,
                    )?,
                    tokens(
                        "a 1/1 white Human creature token",
                        amount(2),
                        false,
                        vocabulary,
                    )?,
                ),
            )
        }
        "Creatures you control get +1/+1 until end of turn. Fateful hour — If you have 5 or less life, those creatures gain indestructible until end of turn." =>
        {
            let boost = |indestructible: bool| crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                EffectSubject::Matching(ObjectFilter::creature().controlled()),
                amount(1),
                amount(1),
                indestructible
                    .then_some(KeywordAbility::Indestructible)
                    .into_iter()
                    .collect(),
                Default::default(),
            );
            (
                NoTarget,
                choose_effect(
                    spellwake_engine::program::ValueExpr::PlayerStat(spellwake_engine::program::PlayerStatExpr::ControllerLife),
                    Comparison::LessOrEqual,
                    5,
                    boost(true),
                    boost(false),
                ),
            )
        }
        "Put a +1/+1 counter on target creature. Morbid — Put three +1/+1 counters on that creature instead if a creature died this turn." => {
            (
                SingleCreature,
                choose_effect(
                    spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::CreaturesDiedThisTurn),
                    Comparison::Greater,
                    0,
                    counters(3),
                    counters(1),
                ),
            )
        }
        "Burning Oil deals 3 damage to target attacking or blocking creature."
            if name == "Burning Oil" =>
        {
            (
                FilteredPermanent(ObjectFilter::creature().attacking_or_blocking()),
                vec![damage(3)],
            )
        }
        "Brimstone Volley deals 3 damage to any target. Morbid — Brimstone Volley deals 5 damage instead if a creature died this turn."
            if name == "Brimstone Volley" =>
        {
            (
                SingleAnyTarget,
                choose_effect(
                    spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::CreaturesDiedThisTurn),
                    Comparison::Greater,
                    0,
                    damage(5),
                    damage(3),
                ),
            )
        }
        "Destroy target creature with flying." => {
            (FilteredPermanent(ObjectFilter::creature().with_keyword(KeywordAbility::Flying)), vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } }))])
        }
        "Vampire creatures you control get +2/+0 and gain first strike until end of turn." => (
            NoTarget,
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                EffectSubject::Matching(
                    ObjectFilter::creature()
                        .controlled()
                        .with_subtype(vocabulary.resolve_label("Vampire")?),
                ),
                amount(2),
                amount(0),
                [KeywordAbility::FirstStrike].into(),
                Default::default(),
            )],
        ),
        _ => return None,
    };
    Some(ExecutableBody::new(target, instructions))
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
    let rules = rules
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    use super::avacyn_restored::{activation, add_trigger};
    let (first, rest) = rules.split_once('\n').unwrap_or((&rules, ""));
    if let Some(exile) = first.strip_prefix("As an additional cost to cast this spell, exile ") {
        if exile == "X cards from your graveyard." {
            let mut reduced = card.clone();
            reduced.faces[0].oracle_text = Some(rest.to_owned());
            let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
            program.graveyard_exile_x_cost = true;
            return Some(program);
        }
        let count = match exile {
            "a creature card from your graveyard." => 1,
            "two creature cards from your graveyard." => 2,
            "three creature cards from your graveyard." => 3,
            _ => return None,
        };
        let mut reduced = card.clone();
        let (ordinary, graveyard_cast) = rest
            .strip_suffix("\nYou may cast this card from your graveyard.")
            .map_or((rest, false), |text| (text, true));
        reduced.faces[0].oracle_text = Some(ordinary.to_owned());
        let mut program = super::compile(&reduced, vocabulary, Profile::Current).ok()?;
        program.graveyard_creature_exile_cost = count;
        if graveyard_cast {
            program.graveyard_self_cast =
                Some(spellwake_engine::program::GraveyardSelfCast::Unconditional);
        }
        return Some(program);
    }
    use BodyTarget::{
        None as NoTarget, SingleAnyTarget, SingleCreature, SingleOpponent, SinglePlayer,
    };
    use Instruction::*;
    use spellwake_engine::characteristics::{
        NumericExpression, NumericPair, StaticSourceCondition, StaticSubtypeOperand,
    };
    use spellwake_engine::program::AttackRequirement;
    let price = || fixed_cost(face.mana_cost.as_deref()?);
    let literal_stats = |p: i64, t: i64| {
        StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(p.into()),
            toughness: NumericExpression::Literal(t.into()),
        })
    };
    let human = || vocabulary.resolve_label("Human");
    let aura = || {
        (face.type_line.as_deref() == Some("Enchantment — Aura"))
            .then(|| Some(Program::aura(Some(price()?), EnchantRestriction::Creature)))?
    };
    let attached = |subtype, excluded| PermanentSelector {
        attached_to_source: true,
        creature_subtype: subtype,
        forbidden_creature_subtype: excluded,
        ..Default::default()
    };
    let add_static = super::army_grants::add_battlefield_ability;
    let add_colorless = |program: &mut Program| {
        program.mana.push(ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::Fixed([(ManaType::Colourless, 1.into())].into()),
        });
    };
    let base =
        |keywords| ordinary_creature(card, vocabulary, Profile::Current, Some(keywords)).ok();
    let counter = || {
        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
            spellwake_engine::effects::CounterKind::PlusOne,
            amount(1),
        )
    };
    let damage_two = || {
        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
            spellwake_engine::toolbox::DamageTargetQuery::Targets,
            amount(2),
            false,
        )
    };
    let mill = |player, n| {
        MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::mill(
            player,
            amount(n),
        )))
    };
    let etb = spellwake_engine::program::TriggerSpec::enters(ObjectFilter::self_object());
    let morbid = |instruction| {
        vec![
            BranchCondition {
                condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                    Box::new(ResolutionCondition {
                        left: spellwake_engine::program::ValueExpr::event(
                            spellwake_engine::program::EventValueKind::CreaturesDiedThisTurn,
                        ),
                        comparison: Comparison::Greater,
                        right: spellwake_engine::program::ValueExpr::Literal(0.into()),
                    }),
                )),
                destination: 2,
            },
            instruction,
        ]
    };

    let (printed, triggers) = match rules.as_str() {
        "Equipped creature can't be blocked by Vampires or Zombies.\nEquipped creature has \"{T}, Sacrifice Blazing Torch: Blazing Torch deals 2 damage to any target.\"\nEquip {1}"
            if face.type_line.as_deref() == Some("Artifact — Equipment") =>
        {
            let mut program = Program::equipment(Some(price()?), fixed_cost("{1}")?);
            for name in ["Vampire", "Zombie"] {
                add_static(&mut program, attached(None, None), StaticParts(vec![StaticPart::BlockingRestriction(
                    BlockingRestriction::CannotBeBlockedBySubtype(vocabulary.resolve_label(name)?),
                )]))?;
            }
            add_static(&mut program, attached(None, None), StaticParts(vec![StaticPart::GrantActivated(Box::new(activation(
                ManaCost::free(),
                SourceCost::SacrificeSelected {
                    filter: spellwake_engine::activated::SelectedSacrificeFilter::GrantingPermanent,
                    count: 1,
                    tap_source: true,
                    from_graveyard: false,
                },
                SingleAnyTarget,
                vec![Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec::from_granting_permanent(2.into()),
                ))],
            )))]))?;
            return Some(program);
        }
        "Whenever a nontoken creature you control dies, put a slime counter on this enchantment, then create a green Ooze creature token with \"This token's power and toughness are each equal to the number of slime counters on Gutter Grime.\""
            if face.type_line.as_deref() == Some("Enchantment") =>
        {
            use spellwake_engine::effects::CounterKind;
            let CreateTokens { mut creations } = tokens("a 0/0 green Ooze creature token", amount(1), false, vocabulary)? else {
                return None;
            };
            creations.first_mut()?.base_size = Some(spellwake_engine::token::TokenSize::AbilitySourceCounters(CounterKind::Slime));
            let mut program = Program::enchantment(Some(price()?));
            add_trigger(&mut program, spellwake_engine::program::TriggerSpec::dies(spellwake_engine::program::ObjectFilter::creature().controlled().nontoken()), NoTarget, vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(CounterKind::Slime, amount(1)),
                CreateTokens { creations },
            ]);
            return Some(program);
        }
        "Whenever a creature you control dies, put two eyeball counters on this artifact.\n{3}, {T}, Remove all eyeball counters from this artifact: Look at the top X cards of your library, where X is the number of eyeball counters removed this way. Put one of them into your hand and the rest on the bottom of your library in any order."
            if face.type_line.as_deref() == Some("Artifact") =>
        {
            use spellwake_engine::{activated::CounterCostDelta, effects::CounterKind};
            let look = activation(fixed_cost("{3}")?, SourceCost::Counter {
                kind: CounterKind::Eyeball,
                delta: CounterCostDelta::RemoveAll,
                tap_source: true,
                sacrifice_source: false,
            }, NoTarget, vec![Instruction::LookTopChooseExt {
                library_owner: spellwake_engine::program::ResolvingPlayer::Controller,
                chooser: spellwake_engine::program::ResolvingPlayer::Controller,
                count: spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::PaidNamedCounters),
                filter: None,
                max_picks: 1,
                take_all_matching: false,
                keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
                rest_dest: spellwake_engine::program::LookTopRestDest::BottomOrder,
                reveal_kept: false,
                reveal_all: false,
                reveal_only: false,
                secondary: None,
            }]);
            let mut program = Program::artifact(Some(price()?), vec![look]);
            add_trigger(&mut program, spellwake_engine::program::TriggerSpec::dies(spellwake_engine::program::ObjectFilter::creature().controlled()), NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(CounterKind::Eyeball, amount(2))]);
            return Some(program);
        }
        "Creature cards in graveyards and libraries can't enter the battlefield.\nPlayers can't cast spells from graveyards or libraries."
            if face.type_line.as_deref() == Some("Artifact") =>
        {
            let mut program = Program::artifact(Some(price()?), vec![]);
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::Restriction { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticRestriction::RestrictGraveyardAndLibrary });
            return Some(program);
        }
        "Deathtouch\nWhen this creature dies, you may search your library for a Curse card, put it onto the battlefield attached to target player, then shuffle." => (
            "Deathtouch",
            vec![(spellwake_engine::program::TriggerSpec::dies(ObjectFilter::self_object()), SinglePlayer, vec![
                OptionalSequence { chooser: ResolvingPlayer::Controller, instructions: 1 },
                SearchLibrary { filter: Box::new(spellwake_engine::program::LibrarySearch::to_battlefield(
                        spellwake_engine::program::SearchCardMatcher::CurseForHost(
                            spellwake_engine::program::AuraSearchHost::TargetPlayer,
                        ),
                        false,
                    )
                    .with_destination(spellwake_engine::program::SearchDestination::Battlefield {
                        tapped: false,
                        post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::AttachToCurseHost(
                            spellwake_engine::program::AuraSearchHost::TargetPlayer,
                        ),
                    })
                    .with_reveal(true)) },
            ])],
        ),
        "Enchant player\nAt the beginning of your upkeep, you may search your library for a Curse card that doesn't have the same name as a Curse attached to enchanted player, put it onto the battlefield attached to that player, then shuffle."
            if face.type_line.as_deref() == Some("Enchantment — Aura Curse") =>
        {
            let mut program = Program::aura(Some(price()?), EnchantRestriction::Player);
            program.enchantment_subtypes.insert(spellwake_engine::program::EnchantmentSubtype::Curse);
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller),
                NoTarget,
                vec![
                    OptionalSequence { chooser: ResolvingPlayer::Controller, instructions: 1 },
                    SearchLibrary { filter: Box::new(spellwake_engine::program::LibrarySearch::to_battlefield(
                            spellwake_engine::program::SearchCardMatcher::CurseForHost(
                                spellwake_engine::program::AuraSearchHost::EnchantedPlayerWithDifferentCurseNames,
                            ),
                            false,
                        )
                        .with_destination(spellwake_engine::program::SearchDestination::Battlefield {
                            tapped: false,
                            post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::AttachToCurseHost(
                                spellwake_engine::program::AuraSearchHost::EnchantedPlayerWithDifferentCurseNames,
                            ),
                        })
                        .with_reveal(true)) },
                ],
            );
            return Some(program);
        }
        "Hexproof\nWhenever Geist of Saint Traft attacks, create a 4/4 white Angel creature token with flying that's tapped and attacking. Exile that token at end of combat." => {
            let CreateTokens { mut creations } = tokens(
                "a 4/4 white Angel creature token with flying", amount(1), true, vocabulary,
            )? else { return None };
            for creation in &mut creations {
                creation.attacking_defender = Some(spellwake_engine::token::AttackingDefenderScope::AnyLegal);
            }
            (
                "Hexproof",
                vec![(spellwake_engine::program::TriggerSpec::self_attacks(), NoTarget, vec![
                    CreateTokens { creations },
                    Instruction::ScheduleDelayedStepTrigger { step: Step::EndCombat, own_step_only: false, body: Box::new(ExecutableBody::new(BodyTarget::None, vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::CreatedTokens, spellwake_engine::toolbox::ZoneDest::exile())))])) },
                ])],
            )
        }
        "Whenever this creature attacks, create a 2/2 green Wolf creature token that's tapped and attacking for each creature card in your graveyard." => {
            let CreateTokens { mut creations } = tokens(
                "a 2/2 green Wolf creature token", spellwake_engine::program::ValueExpr::count_graveyard(spellwake_engine::program::ResolvingPlayer::Controller, spellwake_engine::program::ObjectFilter::creature()), true, vocabulary,
            )? else { return None };
            for creation in &mut creations {
                creation.attacking_defender = Some(spellwake_engine::token::AttackingDefenderScope::AnyLegal);
            }
            ("", vec![(spellwake_engine::program::TriggerSpec::self_attacks(), NoTarget, vec![CreateTokens { creations }])])
        }
        "Creatures you control enter as a copy of this creature." => {
            let mut program = base("")?;
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::Modifier { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticModifierRule::EnterControlledCreaturesAsSourceCopy });
            return Some(program);
        }
        "You may have this creature enter as a copy of any creature on the battlefield, except it has \"{U}{B}, {T}: Destroy target creature with the same name as this creature.\"" => {
            let mut program = base("")?;
            program.entry_copy = Some(spellwake_engine::program::EntryCopyRule::BattlefieldCreatureWithAbility(
                Box::new(activation(
                    fixed_cost("{U}{B}")?, SourceCost::Tap,
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().sharing_name_with_source()),
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } }))],
                )),
            ));
            return Some(program);
        }
        "This artifact enters tapped.\n{T}: Add two mana in any combination of colors. Spend this mana only to cast spells with flashback from a graveyard."
            if face.type_line.as_deref() == Some("Artifact") =>
        {
            let mut program = Program::artifact(Some(price()?), vec![]);
            program.mana.push(ManaAbility {
                id: ManaAbilityId(0),
                output: ManaOutput::TwoColors { distinct: false }
                    .restricted(spellwake_engine::mana::ManaRestriction::SpellWithFlashbackFromGraveyard),
            });
            let ProgramKind::Permanent(definition) = &mut program.kind else { return None };
            definition.entry_tap = Some(spellwake_engine::land::EntryTap::Always);
            return Some(program);
        }
        "Flying\nAs long as this creature is in your graveyard, each Human creature you control enters with an additional +1/+1 counter on it." => {
            let mut program = base("Flying")?;
            program.entry_counter_abilities.push(
                spellwake_engine::program::EntryCounterAbility::GraveyardControlledSubtypePlusOne(human()?),
            );
            return Some(program);
        }
        "This creature enters with a +1/+1 counter on it for each other Zombie you control and each Zombie card in your graveyard.\nIf this creature would be dealt damage, prevent that damage and remove a +1/+1 counter from it." => {
            let mut program = base("")?;
            program.entry_counter_abilities.push(
                spellwake_engine::program::EntryCounterAbility::ControlledAndGraveyardSubtypePlusOne(vocabulary.resolve_label("Zombie")?),
            );
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::DamageRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticDamageRule::PreventIncomingRemovePlusOne });
            return Some(program);
        }
        "When this creature enters, you may exile another target creature.\nWhen this creature leaves the battlefield, return the exiled card to the battlefield under its owner's control." => {
            let link = spellwake_engine::program::LinkedExileGroup::FiendHunter;
            (
                "",
                vec![
                    (etb, BodyTarget::FilteredPermanent(ObjectFilter::creature().other_than_trigger_source()), vec![
                        OptionalSequence { chooser: ResolvingPlayer::Controller, instructions: 1 },
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Exile { face_down: false, viewer: None, link_to_source: false, delayed_return: None, link_group: Some(link) })),
                    ]),
                    (spellwake_engine::program::TriggerSpec::leaves_battlefield(ObjectFilter::self_object()), NoTarget, vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_cards_exiled_with_link(link, false, false)]),
                ],
            )
        }
        "{1}, {T}: Exile target creature you control.\n{7}, {T}: Exile target creature you don't control.\nWhen Helvault is put into a graveyard from the battlefield, return all cards exiled with it to the battlefield under their owners' control."
            if card.name == "Helvault" =>
        {
            let link = spellwake_engine::program::LinkedExileGroup::Helvault;
            let mut program = Program::artifact(Some(price()?), Vec::new());
            program.supertypes.insert(spellwake_engine::program::Supertype::Legendary);
            for (cost, filter) in [
                ("{1}", ObjectFilter::creature().controlled()),
                ("{7}", ObjectFilter::creature().not_controlled()),
            ] {
                let mut ability = activation(
                    fixed_cost(cost)?, SourceCost::Tap,
                    BodyTarget::FilteredPermanent(filter),
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Exile { face_down: false, viewer: None, link_to_source: false, delayed_return: None, link_group: Some(link) }))],
                );
                ability.id = spellwake_engine::activated::ActivatedAbilityId(program.activated.len() as u64);
                program.activated.push(ability);
            }
            add_trigger(&mut program, spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Dies { subject: spellwake_engine::program::ObjectFilter::self_object(), mode: spellwake_engine::program::DiesTriggerMode::Permanent }), NoTarget, vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_cards_exiled_with_link(link, false, false)]);
            return Some(program);
        }
        "Indestructible\nWhenever this creature deals combat damage to a creature, flip a coin. If you win the flip, destroy that creature." => (
            "Indestructible",
            vec![(
                spellwake_engine::program::TriggerSpec::deals_damage(spellwake_engine::program::ObjectFilter::self_object(), spellwake_engine::program::DamageTriggerRecipient::Creature, true),
                NoTarget,
                vec![
                    Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::flip_coins(
                            ResolvingPlayer::Controller,
                            spellwake_engine::program::CoinFlipMode::Single,
                            None,
                            None,
                            Some(2),
                        ),
                    )),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_subject(
                        spellwake_engine::program::EffectSubject::TriggerSubject,
                        false,
                    ),
                ],
            )],
        ),
        "Equipped creature gets +1/+0.\nWhenever equipped creature blocks or becomes blocked by a Vampire, destroy that creature. It can't be regenerated.\nEquip {1}"
            if face.type_line.as_deref() == Some("Artifact — Equipment") =>
        {
            let mut program = Program::equipment(Some(price()?), fixed_cost("{1}")?);
            add_static(&mut program, attached(None, None), StaticParts(vec![literal_stats(1, 0)]))?;
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::blocks_or_blocked(spellwake_engine::program::ObjectFilter::any().attached_to_self(), spellwake_engine::program::BlockTriggerKind::BlocksOrBlockedByEach { required_subtype: Some(vocabulary.resolve_label("Vampire")?) }),
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_subject(
                    spellwake_engine::program::EffectSubject::TriggerSubject,
                    true,
                )],
            );
            return Some(program);
        }
        "Undying\nWhenever this creature or another creature enters from your graveyard, that creature deals damage equal to its power to any target." => (
            "Undying",
            vec![(
                spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Enters { subject: spellwake_engine::program::ObjectFilter::creature(), from_zone: spellwake_engine::program::TriggerFromZone::ControllerGraveyard, cast_origin: None, attached_to_creature: false }),
                SingleAnyTarget,
                vec![DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::from_power(spellwake_engine::program::PowerDamageSource::GraveyardEntrant)))],
            )],
        ),
        "If equipped creature would deal combat damage, it deals double that damage instead.\nIf another creature would deal combat damage to equipped creature, it deals double that damage to equipped creature instead.\nEquip {2}"
            if face.type_line.as_deref() == Some("Artifact — Equipment") =>
        {
            let mut program = Program::equipment(Some(price()?), fixed_cost("{2}")?);
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::DamageRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticDamageRule::DoubleEquippedCombat });
            return Some(program);
        }
        "You may pay {0} rather than pay the mana cost for Zombie creature spells you cast."
            if face.type_line.as_deref() == Some("Enchantment") =>
        {
            let mut program = Program::enchantment(Some(price()?));
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::CostModifier { id: StaticAbilityId(0), modifier: spellwake_engine::characteristics::SpellCostModifier::FreeSubtypeCreatureSpells(vocabulary.resolve_label("Zombie")?) });
            return Some(program);
        }
        "{T}: Gain control of target creature with power less than or equal to the number of creatures you control." => {
            let mut program = base("")?;
            program.activated.push(activation(
                fixed_cost("{0}")?,
                SourceCost::Tap,
                BodyTarget::FilteredPermanent(ObjectFilter::creature().with_power(PowerFilter::AtMostControlledCreatureCount)),
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                        spellwake_engine::characteristics::ControlRecipient::ResolvingController,
                    )]),
                    ResolvedDuration::NoStatedDuration,
                )],
            ));
            return Some(program);
        }
        "{T}: Add {C}.\n{T}, Sacrifice this land: Destroy target land. Its controller may search their library for a basic land card, put it onto the battlefield, then shuffle."
            if face.type_line.as_deref() == Some("Land") =>
        {
            let mut program = Program::land(Default::default(), Vec::new());
            add_colorless(&mut program);
            program.activated.push(activation(
                fixed_cost("{0}")?,
                SourceCost::TapSacrifice,
                BodyTarget::FilteredPermanent(ObjectFilter::land()),
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics:
                                    spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                        allow_regenerate: true,
                                    },
                            },
                        ),
                    ),
                    OptionalSequence { chooser: ResolvingPlayer::TargetController, instructions: 1 },
                    SearchLibrary { filter: Box::new(spellwake_engine::program::LibrarySearch::to_battlefield(
                            spellwake_engine::program::SearchCardMatcher::BasicLand,
                            false,
                        )
                        .with_searcher(ResolvingPlayer::TargetController)) },
                ],
            ));
            return Some(program);
        }
        "Morbid — {T}, Tap two untapped creatures you control: Create a 5/5 black Demon creature token with flying. Activate only if a creature died this turn." => {
            let mut program = base("")?;
            let ability = activation(
                fixed_cost("{0}")?,
                SourceCost::TapPermanents {
                    filter: spellwake_engine::activated::TapCostFilter::Creature,
                    count: spellwake_engine::activated::TapCostCount::Fixed(std::num::NonZeroU8::new(2)?),
                    tap_source: true,
                    exclude_source: true,
                },
                NoTarget,
                vec![tokens("a 5/5 black Demon creature token with flying", amount(1), false, vocabulary)?],
            );
            program.activation_restrictions.insert(ability.id, ActivationRestriction::CreatureDiedThisTurn);
            program.activated.push(ability);
            return Some(program);
        }
        "Flash\nWhen this creature enters, target instant or sorcery card in your graveyard gains flashback until end of turn. The flashback cost is equal to its mana cost." => (
            "Flash",
            vec![(
                etb,
                BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::instant_or_sorcery()),
                vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ObjectMutation::GrantFlashback)))],
            )],
        ),
        "Flash\nFlying\nWhen this creature enters, remove target attacking or blocking creature from combat." => (
            "Flash\nFlying",
            vec![(
                etb,
                BodyTarget::FilteredPermanent(ObjectFilter::creature().attacking_or_blocking()),
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_from_combat(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                    ),
                ],
            )],
        ),
        "Werewolf creatures you control get +1/+0 and have trample.\nSacrifice this enchantment: Regenerate all Werewolf creatures you control."
            if face.type_line.as_deref() == Some("Enchantment") =>
        {
            let mut program = Program::enchantment(Some(price()?));
            let werewolf = vocabulary.resolve_label("Werewolf")?;
            add_static(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceController,
                    creature_subtype: Some(werewolf),
                    required_types: [spellwake_engine::program::CardType::Creature].into(),
                    ..Default::default()
                },
                StaticParts(vec![
                    literal_stats(1, 0),
                    StaticPart::Abilities(AbilityChange::grant_keywords([KeywordAbility::Trample])),
                ]),
            )?;
            program.activated.push(activation(
                fixed_cost("{0}")?,
                SourceCost::Sacrifice,
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_all(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Battlefield, ObjectFilter::creature().controlled().with_subtype(werewolf)), spellwake_engine::toolbox::ObjectMutation::Regenerate)],
            ));
            return Some(program);
        }
        "Grimgrin enters tapped and doesn't untap during your untap step.\nSacrifice another creature: Untap Grimgrin and put a +1/+1 counter on it.\nWhenever Grimgrin attacks, destroy target creature defending player controls, then put a +1/+1 counter on Grimgrin."
            if card.name == "Grimgrin, Corpse-Born" =>
        {
            let mut program = base("")?;
            let ProgramKind::Permanent(permanent) = &mut program.kind else {
                return None;
            };
            permanent.entry_tap = Some(EntryTap::Always);
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::UntapRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticUntapRule::SourceSkipsUntap });
            program.activated.push(activation(
                fixed_cost("{0}")?,
                SourceCost::SacrificeSelected {
                    filter: spellwake_engine::activated::SelectedSacrificeFilter::OtherCreature,
                    count: 1,
                    tap_source: false,
                    from_graveyard: false,
                },
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::untap_ability_source(), counter()],
            ));
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::self_object(), spellwake_engine::program::AttackTriggerMode::Individual { capture_defender: true }),
                BodyTarget::FilteredPermanent(ObjectFilter::creature().of_related_player()),
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } })), counter()],
            );
            return Some(program);
        }
        "{2}{R}: Target creature attacks this turn if able." => {
            let mut program = base("")?;
            program.activated.push(activation(
                fixed_cost("{2}{R}")?,
                SourceCost::None,
                SingleCreature,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(EffectSubject::TargetObject, spellwake_engine::program::CombatActionKind::MustAttack)],
            ));
            return Some(program);
        }
        "Whenever another creature you control with power 2 or less enters, you may pay {1}. If you do, draw a card." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::enters(spellwake_engine::program::ObjectFilter::creature().controlled().other_than_ability_source().with_power(spellwake_engine::program::PowerFilter::AtMost(2))),
                NoTarget,
                vec![
                    OfferDynamicManaPayment {
                        payer: ResolvingPlayer::Controller,
                        fixed: fixed_cost("{1}")?,
                        extra_generic: spellwake_engine::program::ValueExpr::Literal(0.into()),
                        paid_destination: 1,
                        declined_destination: 2,
                    },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(amount(1)),
                ],
            )],
        ),
        "Flash\nWhen this creature enters, untap all creatures you control." => (
            "Flash",
            vec![(etb, NoTarget, vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::set_group_tapped(ObjectFilter::creature().controlled(), false)])],
        ),
        "Whenever a Vampire you control deals combat damage to a player, put a +1/+1 counter on it." => {
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::deals_damage(spellwake_engine::program::ObjectFilter::any().controlled().with_subtype(vocabulary.resolve_label("Vampire")?,), spellwake_engine::program::DamageTriggerRecipient::AnyPlayer, true),
                    NoTarget,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                            spellwake_engine::toolbox::ObjectQuery::Intersect(
                                Box::new(spellwake_engine::toolbox::ObjectQuery::TriggerSubject),
                                Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Battlefield,
                                    ObjectFilter::any(),
                                )),
                            ),
                            spellwake_engine::effects::CounterKind::PlusOne,
                            spellwake_engine::program::ValueExpr::literal(1),
                        ),
                    ],
                )],
            )
        }
        "Enchant creature\nWhenever enchanted creature blocks, you gain 3 life." => {
            let mut program = aura()?;
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::blocks_or_blocked(spellwake_engine::program::ObjectFilter::any().attached_to_self(), spellwake_engine::program::BlockTriggerKind::Blocks),
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        amount(3),
                    ),
                ],
            );
            return Some(program);
        }
        "Enchant creature\n{1}: Tap enchanted creature." => {
            let mut program = aura()?;
            program.activated.push(activation(
                fixed_cost("{1}")?,
                SourceCost::None,
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_all(
                        spellwake_engine::toolbox::ObjectQuery::AttachedHost,
                        spellwake_engine::toolbox::ObjectMutation::SetTapped(true),
                    ),
                ],
            ));
            return Some(program);
        }
        "Enchant creature\nEnchanted creature has flying.\nPrevent all combat damage that would be dealt to and dealt by enchanted creature." =>
        {
            use spellwake_engine::characteristics::{StaticAbilityId};
            let mut program = aura()?;
            program.continuous = Some(
                spellwake_engine::characteristics::StaticContinuousAbility::DamageRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticDamageRule::PreventEnchantedCreature { combat_only: true } },
            );
            add_static(
                &mut program,
                attached(None, None),
                StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
                    [KeywordAbility::Flying],
                ))]),
            )?;
            return Some(program);
        }
        "Equipped creature gets +1/+0 and has vigilance.\nWhenever equipped creature dies, if it was a Human, create a 1/1 white Spirit creature token with flying.\nEquip {2}"
            if face.type_line.as_deref() == Some("Artifact — Equipment") =>
        {
            let mut program = Program::equipment(Some(price()?), fixed_cost("{2}")?);
            program.continuous = Some(super::attachments::host_effect(
                "gets +1/+0 and has vigilance.",
                vocabulary,
            )?);
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::dies(spellwake_engine::program::ObjectFilter::creature().attached_to_self().with_subtype(human()?)),
                NoTarget,
                vec![tokens(
                    "a 1/1 white Spirit creature token with flying",
                    amount(1),
                    false,
                    vocabulary,
                )?],
            );
            return Some(program);
        }
        "Enchant player\nWhenever a creature deals combat damage to enchanted player, put a +1/+1 counter on that creature."
            if face.type_line.as_deref() == Some("Enchantment — Aura Curse") =>
        {
            let mut program = Program::aura(Some(price()?), EnchantRestriction::Player);
            program
                .enchantment_subtypes
                .insert(spellwake_engine::program::EnchantmentSubtype::Curse);
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::deals_damage(spellwake_engine::program::ObjectFilter::any(), spellwake_engine::program::DamageTriggerRecipient::EnchantedPlayer, true),
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                        spellwake_engine::toolbox::ObjectQuery::Intersect(
                            Box::new(spellwake_engine::toolbox::ObjectQuery::TriggerSubject),
                            Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                ObjectFilter::any(),
                            )),
                        ),
                        spellwake_engine::effects::CounterKind::PlusOne,
                        spellwake_engine::program::ValueExpr::literal(1),
                    ),
                ],
            );
            return Some(program);
        }
        "Enchant player\nAt the beginning of enchanted player's upkeep, this Aura deals 1 damage to that player or a planeswalker that player controls."
            if face.type_line.as_deref() == Some("Enchantment — Aura Curse") =>
        {
            let mut program = Program::aura(Some(price()?), EnchantRestriction::Player);
            program.enchantment_subtypes.insert(spellwake_engine::program::EnchantmentSubtype::Curse);
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::new(
                    spellwake_engine::program::TriggerEvent::StepBegan {
                        step: spellwake_engine::turn::Step::Upkeep,
                        player: spellwake_engine::program::TriggerPlayerScope::EnchantedPlayer,
                    },
                ),
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                            spellwake_engine::program::DamageRecipientsRef::Group(
                                spellwake_engine::program::DamageGroup::RelatedPlayerOrTheirPlaneswalker,
                            ),
                        )),
                        amount(1),
                        false,
                    ),
                ],
            );
            return Some(program);
        }
        "Enchant player\nAt the beginning of enchanted player's upkeep, that player exiles two cards from their graveyard."
            if face.type_line.as_deref() == Some("Enchantment — Aura Curse") =>
        {
            let mut program = Program::aura(Some(price()?), EnchantRestriction::Player);
            program.enchantment_subtypes.insert(spellwake_engine::program::EnchantmentSubtype::Curse);
            add_trigger(&mut program, spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::StepBegan { step: spellwake_engine::turn::Step::Upkeep, player: spellwake_engine::program::TriggerPlayerScope::EnchantedPlayer }), NoTarget, vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_cards_from_graveyard(ResolvingPlayer::UniqueRelatedPlayer, std::num::NonZeroU8::new(2)?),
            ]);
            return Some(program);
        }
        "Enchant player\nAt the beginning of enchanted player's upkeep, this Aura deals damage to that player equal to the number of Curses attached to them."
            if face.type_line.as_deref() == Some("Enchantment — Aura Curse") =>
        {
            let mut program = Program::aura(Some(price()?), EnchantRestriction::Player);
            program.enchantment_subtypes.insert(spellwake_engine::program::EnchantmentSubtype::Curse);
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::StepBegan { step: spellwake_engine::turn::Step::Upkeep, player: spellwake_engine::program::TriggerPlayerScope::EnchantedPlayer }),
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                            spellwake_engine::program::DamageRecipientsRef::Group(
                                spellwake_engine::program::DamageGroup::RelatedPlayer,
                            ),
                        )),
                        spellwake_engine::program::ValueExpr::AttachedCount(
                            spellwake_engine::program::AttachedCountKind::CursesOnRelatedPlayer,
                        ),
                        false,
                    ),
                ],
            );
            return Some(program);
        }
        "Enchant player\nCreatures enchanted player controls get -1/-1."
        | "Enchant player\nCreatures enchanted player controls attack each combat if able."
        | "Enchant player\nIf a source would deal damage to enchanted player, it deals double that damage to that player instead."
        | "Enchant player\nEnchanted player can't cast more than one spell each turn."
            if face.type_line.as_deref() == Some("Enchantment — Aura Curse") =>
        {
            let mut program = Program::aura(Some(price()?), EnchantRestriction::Player);
            program
                .enchantment_subtypes
                .insert(spellwake_engine::program::EnchantmentSubtype::Curse);
            match rules.as_str() {
                "Enchant player\nCreatures enchanted player controls get -1/-1." => {
                    add_static(
                        &mut program,
                        PermanentSelector {
                            controller: ControllerSelector::EnchantedPlayer,
                            required_types: [spellwake_engine::program::CardType::Creature].into(),
                            ..Default::default()
                        },
                        StaticParts(vec![literal_stats(-1, -1)]),
                    )?;
                }
                "Enchant player\nCreatures enchanted player controls attack each combat if able." => {
                    program.attack_requirement =
                        Some(AttackRequirement::EnchantedPlayerCreaturesAttack);
                }
                "Enchant player\nEnchanted player can't cast more than one spell each turn." => {
                    program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::Restriction { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticRestriction::EnchantedPlayerCastLimit });
                }
                _ => {
                    program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::DamageRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticDamageRule::DoubleToEnchantedPlayer });
                }
            }
            return Some(program);
        }
        "This creature attacks each combat if able.\nThis creature doesn't untap during your untap step.\nWhenever another creature dies, untap this creature." => {
            let mut program = base("")?;
            program.attack_requirement = Some(AttackRequirement::SourceAttacks);
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::UntapRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticUntapRule::SourceSkipsUntap });
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::dies(ObjectFilter::creature().other_than_ability_source()),
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::untap_ability_source()],
            );
            return Some(program);
        }
        "{T}: Add {C}.\n{W}{U}, {T}, Exile a creature card from your graveyard: Create a 1/1 white Spirit creature token with flying."
            if face.type_line.as_deref() == Some("Land") =>
        {
            let mut program = Program::land(Default::default(), Vec::new());
            add_colorless(&mut program);
            program.activated.push(activation(
                fixed_cost("{W}{U}")?,
                SourceCost::ExileGraveyard {
                    filter: spellwake_engine::activated::GraveyardExileCost::Creature { pay_mana_cost: false },
                    tap_source: true,
                },
                NoTarget,
                vec![ordinary_text::create_tokens(
                    "a 1/1 white Spirit creature token with flying",
                    vocabulary,
                )?],
            ));
            return Some(program);
        }
        "This artifact enters tapped.\n{T}: Add {C}.\nDuring turns other than yours, this artifact is a 2/3 Gargoyle artifact creature with flying."
            if face.type_line.as_deref() == Some("Artifact") =>
        {
            use spellwake_engine::characteristics::{StaticTypeChange, SubtypeOperation, TypeOperation};
            use spellwake_engine::program::CardType;
            let mut program = Program::artifact(Some(price()?), Vec::new());
            let ProgramKind::Permanent(permanent) = &mut program.kind else { return None; };
            permanent.entry_tap = Some(EntryTap::Always);
            add_colorless(&mut program);
            add_static(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceItself,
                    source_condition: Some(StaticSourceCondition::OpponentsTurn),
                    ..Default::default()
                },
                StaticParts(vec![
                    StaticPart::Type(StaticTypeChange {
                        types: TypeOperation::Set([CardType::Artifact, CardType::Creature].into()),
                        supertypes: TypeOperation::Keep,
                        land_subtypes: SubtypeOperation::Keep,
                        subtypes: SubtypeOperation::Set(StaticSubtypeOperand::Literal(
                            [vocabulary.resolve_label("Gargoyle")?].into()
                        )),
                    }),
                    StaticPart::SetPT(NumericPair {
                        power: NumericExpression::Literal(2.into()),
                        toughness: NumericExpression::Literal(3.into()),
                    }),
                    StaticPart::Abilities(AbilityChange::grant_keywords([KeywordAbility::Flying])),
                ]),
            )?;
            return Some(program);
        }
        "{2}{U}, {T}, Exile a creature card from your graveyard: Create a 2/2 black Zombie creature token, then put a +1/+1 counter on each Zombie creature you control." => {
            let mut program = base("")?;
            program.activated.push(activation(
                fixed_cost("{2}{U}")?,
                SourceCost::ExileGraveyard {
                    filter: spellwake_engine::activated::GraveyardExileCost::Creature { pay_mana_cost: false },
                    tap_source: true,
                },
                NoTarget,
                vec![
                    ordinary_text::create_tokens(
                        "a 2/2 black Zombie creature token",
                        vocabulary,
                    )?,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Battlefield, ObjectFilter::creature().controlled().with_subtype(vocabulary.resolve_label("Zombie")?)), spellwake_engine::effects::CounterKind::PlusOne, amount(1)),
                ],
            ));
            return Some(program);
        }
        "This creature can't be blocked except by creatures with flying or reach."
        | "This creature can't be blocked by Humans.\nWhenever this creature deals combat damage to a player, put a +1/+1 counter on it." => {
            let mut program = base("")?;
            let restriction = if rules.contains("Humans") {
                add_trigger(
                    &mut program,
                    spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
                    NoTarget,
                    vec![counter()],
                );
                BlockingRestriction::CannotBeBlockedBySubtype(human()?)
            } else {
                BlockingRestriction::OnlyBlockedByFlyingOrReach
            };
            add_static(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceItself,
                    ..Default::default()
                },
                StaticParts(vec![StaticPart::BlockingRestriction(restriction)]),
            )?;
            return Some(program);
        }
        "Whenever this creature blocks or becomes blocked by a creature, this creature deals 1 damage to that creature." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::blocks_or_blocked(spellwake_engine::program::ObjectFilter::self_object(), spellwake_engine::program::BlockTriggerKind::BlocksOrBlockedByEach { required_subtype: None }),
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
        ),
        "Haste\nWhenever a creature you control attacks, this creature deals 1 damage to the player or planeswalker it's attacking." => (
            "Haste",
            vec![(
                spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::creature().controlled(), spellwake_engine::program::AttackTriggerMode::Individual { capture_defender: true }),
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                            spellwake_engine::program::DamageRecipientsRef::Group(
                                spellwake_engine::program::DamageGroup::CapturedAttackedPlayerOrPlaneswalker,
                            ),
                        )),
                        amount(1),
                        false,
                    ),
                ],
            )],
        ),
        "When this creature enters, it deals 1 damage to target creature and 1 damage to you." => (
            "",
            vec![(
                etb,
                SingleCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                            spellwake_engine::program::DamageRecipientsRef::Group(
                                spellwake_engine::program::DamageGroup::ControllerAndTargetCreature,
                            ),
                        )),
                        amount(1),
                        false,
                    ),
                ],
            )],
        ),
        "Whenever this creature attacks or blocks, other Humans you control get +1/+1 until end of turn." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::any_of(vec![spellwake_engine::program::TriggerEvent::Attacks { subject: spellwake_engine::program::ObjectFilter::self_object(), mode: spellwake_engine::program::AttackTriggerMode::Individual { capture_defender: false } }, spellwake_engine::program::TriggerEvent::BlocksOrBlocked { subject: spellwake_engine::program::ObjectFilter::self_object(), kind: spellwake_engine::program::BlockTriggerKind::Blocks }]),
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                    EffectSubject::Matching(
                        ObjectFilter::creature()
                            .controlled()
                            .other_than_ability_source()
                            .with_subtype(human()?),
                    ),
                    amount(1),
                    amount(1),
                    Default::default(),
                    Default::default(),
                )],
            )],
        ),
        "First strike\nWhenever a creature dealt damage by this creature this turn dies, you gain life equal to that creature's toughness." => (
            "First strike",
            vec![(
                spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Dies { subject: spellwake_engine::program::ObjectFilter::creature(), mode: spellwake_engine::program::DiesTriggerMode::DamagedBySource }),
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::object_stat(
                            spellwake_engine::program::ValueObjectRef::DyingCreature,
                            spellwake_engine::program::ObjectStatKind::Toughness,
                        ),
                    ),
                ],
            )],
        ),
        "Flying\nAt the beginning of your upkeep, return target Spirit card from your graveyard to your hand." => (
            "Flying",
            vec![(
                spellwake_engine::program::TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller),
                BodyTarget::FilteredOwnGraveyardCard(
                    ObjectFilter::any().with_subtype(vocabulary.resolve_label("Spirit")?),
                ),
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_hand()],
            )],
        ),
        "{1}, Sacrifice a creature: You gain life equal to the sacrificed creature's toughness." => {
            let mut program = base("")?;
            program.activated.push(activation(
                fixed_cost("{1}")?,
                SourceCost::SacrificeSelected {
                    tap_source: false,
                    from_graveyard: false,
                    filter: spellwake_engine::activated::SelectedSacrificeFilter::Creature,
                    count: 1,
                },
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::object_stat(
                            spellwake_engine::program::ValueObjectRef::SacrificedCreature,
                            spellwake_engine::program::ObjectStatKind::Toughness,
                        ),
                    ),
                ],
            ));
            return Some(program);
        }
        "Flying, haste\nSacrifice a creature: This creature gains indestructible until end of turn. If the sacrificed creature was a Human, put a +1/+1 counter on this creature."
        | "Sacrifice a creature: This creature gains flying until end of turn. If the sacrificed creature was a Human, put a +1/+1 counter on this creature." => {
            let protected = rules.starts_with("Flying, haste");
            let mut program = base(if protected { "Flying, haste" } else { "" })?;
            let granted = if protected { KeywordAbility::Indestructible } else { KeywordAbility::Flying };
            program.activated.push(activation(
                ManaCost::free(),
                SourceCost::SacrificeSelected {
                    tap_source: false,
                    from_graveyard: false,
                    filter: spellwake_engine::activated::SelectedSacrificeFilter::Creature,
                    count: 1,
                },
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        EffectSubject::AbilitySource,
                        amount(0),
                        amount(0),
                        [granted].into(),
                        Default::default(),
                    ),
                    BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::object_matches(spellwake_engine::program::MatchObjectRef::SacrificedCreature, spellwake_engine::program::ObjectFilter::creature().with_subtype(human()?)),
                            comparison: Comparison::Equal,
                            right: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        }))), destination: 3 },
                    counter(),
                ],
            ));
            return Some(program);
        }
        "Flying\n{1}{R}: Olivia Voldaren deals 1 damage to another target creature. That creature becomes a Vampire in addition to its other types. Put a +1/+1 counter on Olivia Voldaren.\n{3}{B}{B}: Gain control of target Vampire for as long as you control Olivia Voldaren." => {
            use spellwake_engine::characteristics::{
                ControlRecipient, InstructionSubtypeOperand, ResolvedDuration, ResolvedPartSpec,
                ResolvedPartsSpec, SubtypeOperation, TypeChangeSpec, TypeOperation,
            };
            let vampire = vocabulary.resolve_label("Vampire")?;
            let mut program = base("Flying")?;
            program.activated.push(activation(
                fixed_cost("{1}{R}")?, SourceCost::None,
                BodyTarget::FilteredPermanent(ObjectFilter::creature().other_than_ability_source()),
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        amount(1),
                        false,
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                            types: TypeOperation::Keep, supertypes: TypeOperation::Keep,
                            land_subtypes: SubtypeOperation::Keep,
                            subtypes: SubtypeOperation::Add(InstructionSubtypeOperand::Literal([vampire].into())),
                        })]),
                        ResolvedDuration::NoStatedDuration,
                    ),
                    counter(),
                ],
            ));
            let mut gain = activation(
                fixed_cost("{3}{B}{B}")?, SourceCost::None,
                BodyTarget::FilteredPermanent(ObjectFilter::any().with_subtype(vampire)),
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Control(ControlRecipient::ResolvingController)]),
                    ResolvedDuration::WhileControllingAbilitySource,
                )],
            );
            gain.id = spellwake_engine::activated::ActivatedAbilityId(1);
            program.activated.push(gain);
            return Some(program);
        }
        "If a Zombie you control would deal combat damage to a player, instead that player mills that many cards.\nWhenever a creature card is put into an opponent's graveyard from their library, exile that card and create a 2/2 black Zombie creature token." => {
            let mut program = base("")?;
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::DamageRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticDamageRule::ReplaceSubtypeCombatWithMill(vocabulary.resolve_label("Zombie")?) });
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::PutIntoGraveyard { subject: spellwake_engine::program::ObjectFilter::creature(), player: PlayerRelation::Opponent, from_library: true, own_turn_only: false }),
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Intersect(
                                Box::new(spellwake_engine::toolbox::ObjectQuery::TriggerSubject),
                                Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                        spellwake_engine::toolbox::PlayerSelector::Group(
                                            spellwake_engine::program::AffectedPlayers::EachPlayer,
                                        ),
                                    )),
                                    ObjectFilter::any().nontoken(),
                                )),
                            ),
                            spellwake_engine::toolbox::ZoneDest::exile(),
                        ),
                    ),
                    ordinary_text::create_tokens("a 2/2 black Zombie creature token", vocabulary)?,
                ],
            );
            return Some(program);
        }
        "If you would draw a card while your library has no cards in it, you win the game instead." => {
            let mut program = base("")?;
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::DrawRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticDrawRule::EmptyDrawWins });
            return Some(program);
        }
        "When this creature dies, target opponent discards a card at random." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::dies(ObjectFilter::self_object()),
                SingleOpponent,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::discard_one_at_random(ResolvingPlayer::TargetPlayer)],
            )],
        ),
        "Trample\nWhen this creature dies, exile it, then return two creature cards at random from your graveyard to the battlefield." => (
            "Trample",
            vec![(
                spellwake_engine::program::TriggerSpec::dies(ObjectFilter::self_object()),
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Intersect(
                                Box::new(spellwake_engine::toolbox::ObjectQuery::TriggerSubject),
                                Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                        spellwake_engine::toolbox::PlayerSelector::Group(
                                            spellwake_engine::program::AffectedPlayers::EachPlayer,
                                        ),
                                    )),
                                    ObjectFilter::any().nontoken(),
                                )),
                            ),
                            spellwake_engine::toolbox::ZoneDest::exile(),
                        ),
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::return_random_graveyard_cards(Some(ObjectFilter::creature()), std::num::NonZeroU8::new(2)?, true),
                ],
            )],
        ),
        "When this creature enters, return a Zombie card at random from your graveyard to your hand." => (
            "",
            vec![(
                etb,
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_random_graveyard_cards(Some(ObjectFilter::any().with_subtype(vocabulary.resolve_label("Zombie")?)), std::num::NonZeroU8::new(1)?, false)],
            )],
        ),
        "Morbid — When this creature enters, if a creature died this turn, return a creature card at random from your graveyard to your hand." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::self_enters().only_if(spellwake_engine::program::TriggerCondition::Morbid),
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_random_graveyard_cards(Some(ObjectFilter::creature()), std::num::NonZeroU8::new(1)?, false)],
            )],
        ),
        "At the beginning of your upkeep, return an instant or sorcery card at random from your graveyard to your hand.\nWhenever you cast an instant or sorcery spell, this creature gets +4/+0 until end of turn." => (
            "",
            vec![
                (
                    spellwake_engine::program::TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller),
                    NoTarget,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_random_graveyard_cards(Some(ObjectFilter::instant_or_sorcery()), std::num::NonZeroU8::new(1)?, false)],
                ),
                (
                    spellwake_engine::program::TriggerSpec::spell_cast(PlayerRelation::Controller, spellwake_engine::program::SpellFilter::instant_or_sorcery()),
                    NoTarget,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        EffectSubject::AbilitySource,
                        amount(4),
                        amount(0),
                        Default::default(),
                        Default::default(),
                    )],
                ),
            ],
        ),
        "{T}: Add {C}.\n{3}, {T}, Sacrifice this land: Return a creature card at random from your graveyard to your hand."
            if face.type_line.as_deref() == Some("Land") =>
        {
            let mut program = Program::land(Default::default(), Vec::new());
            add_colorless(&mut program);
            program.activated.push(activation(
                fixed_cost("{3}")?,
                SourceCost::TapSacrifice,
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_random_graveyard_cards(Some(ObjectFilter::creature()), std::num::NonZeroU8::new(1)?, false)],
            ));
            return Some(program);
        }
        "Intimidate\nEach other creature you control that's a Wolf or a Werewolf gets +1/+1.\nNon-Human Werewolves you control can't transform." => {
            let mut program = base("Intimidate")?;
            let werewolf = vocabulary.resolve_label("Werewolf")?;
            let wolf = vocabulary.resolve_label("Wolf")?;
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::Restriction { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticRestriction::PreventControlledTransform { subtype: werewolf, except: human()? } });
            for (kind, excluded) in [(werewolf, None), (wolf, Some(werewolf))] {
                add_static(
                    &mut program,
                    PermanentSelector {
                        required_types: [spellwake_engine::program::CardType::Creature].into(),
                        creature_subtype: Some(kind),
                        forbidden_creature_subtype: excluded,
                        controller: ControllerSelector::SourceController,
                        exclude_source: true,
                        ..Default::default()
                    },
                    StaticParts(vec![literal_stats(1, 1)]),
                )?;
            }
            return Some(program);
        }
        "{T}: Put a +1/+1 counter on Mikaeus.\n{T}, Remove a +1/+1 counter from Mikaeus: Put a +1/+1 counter on each other creature you control."
            if face.name == "Mikaeus, the Lunarch" =>
        {
            // The variable-cost compiler binds X and the entry counters before matching these abilities.
            let mut program = base("")?;
            program.activated.push(activation(
                ManaCost::free(),
                SourceCost::Tap,
                NoTarget,
                vec![counter()],
            ));
            let mut other_creatures = activation(
                ManaCost::free(),
                SourceCost::Counter {
                    kind: spellwake_engine::effects::CounterKind::PlusOne,
                    delta: spellwake_engine::activated::CounterCostDelta::Remove(std::num::NonZeroU8::MIN),
                    tap_source: true,
                    sacrifice_source: false,
                },
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(spellwake_engine::toolbox::ObjectQuery::zone(spellwake_engine::toolbox::QueryZone::Battlefield, ObjectFilter::creature().controlled().other_than_ability_source()), spellwake_engine::effects::CounterKind::PlusOne, amount(1))],
            );
            other_creatures.id = spellwake_engine::activated::ActivatedAbilityId(1);
            program.activated.push(other_creatures);
            return Some(program);
        }
        "At the beginning of your end step, you may sacrifice a nontoken creature. If you do, create X 2/2 green Wolf creature tokens, where X is the sacrificed creature's toughness."
            if face.type_line.as_deref() == Some("Enchantment") =>
        {
            let mut program = Program::enchantment(Some(price()?));
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::step_began(Step::End, PlayerRelation::Controller),
                NoTarget,
                vec![
                    OptionalSequence { chooser: ResolvingPlayer::Controller, instructions: 2 },
                    Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::simple(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Battlefield,
                                ObjectFilter::creature().nontoken().controlled(),
                            ),
                            spellwake_engine::toolbox::ZoneSelection::Prompt {
                                chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::Controller,
                                ),
                                min: Box::new(spellwake_engine::program::ValueExpr::literal(1)),
                                max: Box::new(spellwake_engine::program::ValueExpr::literal(1)),
                                constraint: None,
                                category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                                hidden: false,
                            },
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice,
                            },
                        ),
                    )),
                    tokens("a 2/2 green Wolf creature token", spellwake_engine::program::ValueExpr::object_stat(spellwake_engine::program::ValueObjectRef::LastSacrificedCreature, spellwake_engine::program::ObjectStatKind::Toughness), false, vocabulary)?,
                ],
            );
            return Some(program);
        }
        "{2}, {T}: Target player exiles a card from their graveyard. If it's a creature card, you gain 2 life."
            if face.type_line.as_deref() == Some("Artifact") =>
        {
            return Some(Program::artifact(Some(price()?), vec![activation(
                fixed_cost("{2}")?,
                SourceCost::Tap,
                SinglePlayer,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_cards_from_graveyard(ResolvingPlayer::TargetPlayer, std::num::NonZeroU8::new(1)?),
                    BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::SelectedGraveyardExileCreatures),
                            comparison: Comparison::Equal,
                            right: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        }))), destination: 3 },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        amount(2),
                    ),
                ],
            )]));
        }
        "Equipped creature gets +4/+2.\nEquip—Sacrifice a creature."
            if face.type_line.as_deref() == Some("Artifact — Equipment") =>
        {
            let mut program = Program::equipment(Some(price()?), ManaCost::free());
            program.activated.first_mut()?.source_cost = SourceCost::SacrificeSelected {
                filter: spellwake_engine::activated::SelectedSacrificeFilter::Creature,
                tap_source: false,
                from_graveyard: false,
                count: 1,
            };
            add_static(
                &mut program,
                attached(None, None),
                StaticParts(vec![literal_stats(4, 2)]),
            )?;
            return Some(program);
        }
        "Equipped creature has \"{T}: This creature deals 1 damage to any target\" and \"{T}: This creature deals 3 damage to target Werewolf creature.\"\nEquip {5}"
            if face.type_line.as_deref() == Some("Artifact — Equipment") =>
        {
            let mut program = Program::equipment(Some(price()?), fixed_cost("{5}")?);
            let werewolf = vocabulary.resolve_label("Werewolf")?;
            for (target, damage) in [
                (SingleAnyTarget, 1),
                (BodyTarget::FilteredPermanent(ObjectFilter::creature().with_subtype(werewolf)), 3),
            ] {
                add_static(
                    &mut program,
                    attached(None, None),
                    StaticParts(vec![StaticPart::GrantActivated(Box::new(activation(
                        ManaCost::free(),
                        SourceCost::Tap,
                        target,
                        vec![
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                                spellwake_engine::toolbox::DamageTargetQuery::Targets,
                                spellwake_engine::program::ValueExpr::Literal(damage.into()),
                                false,
                            ),
                        ],
                    )))]),
                )?;
            }
            return Some(program);
        }
        "Flying\n{2}: Target player mills a card. This creature gets +X/+X until end of turn, where X is the milled card's mana value." => {
            let mut program = base("Flying")?;
            program.activated.push(activation(
                fixed_cost("{2}")?,
                SourceCost::None,
                SinglePlayer,
                vec![
                    mill(ResolvingPlayer::TargetPlayer, 1),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        EffectSubject::AbilitySource,
                        spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::MilledCardsManaValueSum),
                        spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::MilledCardsManaValueSum),
                        Default::default(),
                        Default::default(),
                    ),
                ],
            ));
            return Some(program);
        }
        "{3}{R}: Choose any target, then mill three cards. This enchantment deals damage to that permanent or player equal to the greatest mana value among the milled cards."
            if face.type_line.as_deref() == Some("Enchantment") =>
        {
            let mut program = Program::enchantment(Some(price()?));
            program.activated.push(activation(
                fixed_cost("{3}{R}")?,
                SourceCost::None,
                SingleAnyTarget,
                vec![
                    mill(ResolvingPlayer::Controller, 3),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::event(
                            spellwake_engine::program::EventValueKind::MilledCardsManaValueMax,
                        ),
                        false,
                    ),
                ],
            ));
            return Some(program);
        }
        "Activated abilities of artifacts can't be activated."
            if face.type_line.as_deref() == Some("Enchantment") =>
        {
            let mut program = Program::enchantment(Some(price()?));
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::Restriction { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticRestriction::PreventArtifactActivations });
            return Some(program);
        }
        "First strike\nNoncreature spells cost {1} more to cast." => {
            let mut program = base("First strike")?;
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::CostModifier { id: StaticAbilityId(0), modifier: spellwake_engine::characteristics::SpellCostModifier::IncreaseExceptType { exempt: spellwake_engine::program::CardType::Creature, amount: std::num::NonZeroU8::new(1)? } });
            return Some(program);
        }
        "Trample\nAt the beginning of your upkeep, mill two cards." => (
            "Trample",
            vec![(
                spellwake_engine::program::TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller),
                NoTarget,
                vec![mill(ResolvingPlayer::Controller, 2)],
            )],
        ),
        "Flying\nWhenever this creature deals combat damage to a player, draw a card." => (
            "Flying",
            vec![(
                spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
                NoTarget,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(amount(1))],
            )],
        ),
        "Flying\nAs long as you control a Human, this creature has hexproof and indestructible."
        | "This creature has haste as long as an opponent controls a Human." => {
            let angel = rules.starts_with("Flying");
            let mut program = base(if angel { "Flying" } else { "" })?;
            let (source_condition, grant) = if angel {
                (
                    StaticSourceCondition::ControllerControlsSubtype(human()?),
                    AbilityChange::grant_keywords([
                        KeywordAbility::Hexproof,
                        KeywordAbility::Indestructible,
                    ]),
                )
            } else {
                (
                    StaticSourceCondition::OpponentControlsSubtype(human()?),
                    AbilityChange::grant_keywords([KeywordAbility::Haste]),
                )
            };
            add_static(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceItself,
                    source_condition: Some(source_condition),
                    ..Default::default()
                },
                StaticParts(vec![StaticPart::Abilities(grant)]),
            )?;
            return Some(program);
        }
        "Enchant creature\nEnchanted creature gets +X/+X, where X is the number of creature cards in your graveyard." =>
        {
            let mut program = aura()?;
            add_static(
                &mut program,
                attached(None, None),
                StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                    power: NumericExpression::ZoneCount {
                        zone: spellwake_engine::characteristics::StaticZone::OwnGraveyard,
                        types: spellwake_engine::program::CardTypeMask::CREATURE,
                    },
                    toughness: NumericExpression::ZoneCount {
                        zone: spellwake_engine::characteristics::StaticZone::OwnGraveyard,
                        types: spellwake_engine::program::CardTypeMask::CREATURE,
                    },
                })]),
            )?;
            return Some(program);
        }
        "Equipped creature has first strike and gets +X/+0, where X is the number of instant and sorcery cards in your graveyard.\nEquip {2}"
            if face.type_line.as_deref() == Some("Artifact — Equipment") =>
        {
            let mut program = Program::equipment(Some(price()?), fixed_cost("{2}")?);
            add_static(
                &mut program,
                attached(None, None),
                StaticParts(vec![
                    StaticPart::Abilities(AbilityChange::grant_keywords([
                        KeywordAbility::FirstStrike,
                    ])),
                    StaticPart::ModifyPT(NumericPair {
                        power: NumericExpression::ZoneCount {
                            zone: spellwake_engine::characteristics::StaticZone::OwnGraveyard,
                            types: spellwake_engine::program::CardTypeMask::INSTANT
                                .union(spellwake_engine::program::CardTypeMask::SORCERY),
                        },
                        toughness: NumericExpression::Literal(0.into()),
                    }),
                ]),
            )?;
            return Some(program);
        }
        "Defender\nThis creature has indestructible as long as it has defender.\n{1}: Until end of turn, this creature loses defender and gains flying." =>
        {
            let mut program = base("Defender")?;
            add_static(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceItself,
                    source_condition: Some(StaticSourceCondition::HasKeyword(
                        KeywordAbility::Defender,
                    )),
                    ..Default::default()
                },
                StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
                    [KeywordAbility::Indestructible],
                ))]),
            )?;
            let mut change = AbilityChange::grant_keywords([KeywordAbility::Flying]);
            change.suppressed_keywords.insert(KeywordAbility::Defender);
            program.activated.push(activation(
                fixed_cost("{1}")?,
                SourceCost::None,
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(change)]),
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                ],
            ));
            return Some(program);
        }
        "Whenever you cast a spell from your graveyard, draw a card."
        | "Whenever you cast a spell from your graveyard, this enchantment deals 2 damage to any target."
            if face.type_line.as_deref() == Some("Enchantment") =>
        {
            let mut program = Program::enchantment(Some(price()?));
            let (target, effect) = if rules.ends_with("draw a card.") {
                (NoTarget, crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(amount(1)))
            } else {
                (SingleAnyTarget, damage_two())
            };
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::spell_cast(PlayerRelation::Controller, spellwake_engine::program::SpellFilter::any().from_graveyard()),
                target,
                vec![effect],
            );
            return Some(program);
        }
        "Morbid — When this creature enters, if a creature died this turn, you gain 5 life." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::self_enters().only_if(spellwake_engine::program::TriggerCondition::Morbid),
                NoTarget,
                morbid(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        amount(5),
                    ),
                ),
            )],
        ),
        "Morbid — When this creature enters, if a creature died this turn, target creature gets -4/-4 until end of turn." => {
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::self_enters().only_if(spellwake_engine::program::TriggerCondition::Morbid),
                    SingleCreature,
                    morbid(modify(vec![ResolvedPartSpec::ModifyPT(stats(-4, -4))])),
                )],
            )
        }
        "Morbid — When this creature enters, if a creature died this turn, create a 2/2 black Zombie creature token." => {
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::self_enters().only_if(spellwake_engine::program::TriggerCondition::Morbid),
                    NoTarget,
                    morbid(tokens(
                        "a 2/2 black Zombie creature token",
                        amount(1),
                        false,
                        vocabulary,
                    )?),
                )],
            )
        }
        "Morbid — When this creature enters, if a creature died this turn, put two +1/+1 counters on target creature." => {
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::self_enters().only_if(spellwake_engine::program::TriggerCondition::Morbid),
                    SingleCreature,
                    morbid(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                        spellwake_engine::effects::CounterKind::PlusOne,
                        amount(2),
                    )),
                )],
            )
        }
        "Flying\nMorbid — At the beginning of each end step, if a creature died this turn, destroy target non-Demon creature." => {
            (
                "Flying",
                vec![(
                    spellwake_engine::program::TriggerSpec::step_began(Step::End, spellwake_engine::program::PlayerRelation::AnyParticipant).only_if(spellwake_engine::program::TriggerCondition::Morbid),
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().without_subtype(
                        vocabulary.resolve_label("Demon")?,
                    )),
                    morbid(crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } }))),
                )],
            )
        }
        "When this creature enters, you may destroy target Vampire, Werewolf, or Zombie." => (
            "",
            vec![(
                etb,
                BodyTarget::FilteredPermanent(ObjectFilter::creature().among_subtypes([
                    vocabulary.resolve_label("Vampire")?,
                    vocabulary.resolve_label("Werewolf")?,
                    vocabulary.resolve_label("Zombie")?,
                ])),
                vec![
                    OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 1,
                    },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::Targets, spellwake_engine::toolbox::ZoneDest::Graveyard { semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy { allow_regenerate: true } })),
                ],
            )],
        ),
        "{T}: Exile target creature card from a graveyard." => {
            let mut program = base("")?;
            program.activated.push(activation(
                ManaCost::free(),
                SourceCost::Tap,
                BodyTarget::AnyGraveyardCreature,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target()],
            ));
            return Some(program);
        }
        "Indestructible\nWhenever this creature attacks, put a +1/+1 counter on it.\nWhenever a creature dealt damage by this creature this turn dies, put a +1/+1 counter on this creature." => {
            (
                "Indestructible",
                vec![
                    (spellwake_engine::program::TriggerSpec::self_attacks(), NoTarget, vec![counter()]),
                    (
                        spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Dies { subject: spellwake_engine::program::ObjectFilter::creature(), mode: spellwake_engine::program::DiesTriggerMode::DamagedBySource }),
                        NoTarget,
                        vec![counter()],
                    ),
                ],
            )
        }
        "Enchant creature\nWhen this Aura enters, tap enchanted creature.\nEnchanted creature doesn't untap during its controller's untap step." =>
        {
            use spellwake_engine::characteristics::{StaticAbilityId};
            let mut program = aura()?;
            program.continuous = Some(spellwake_engine::characteristics::StaticContinuousAbility::UntapRule { id: StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticUntapRule::EnchantedCreatureSkipsUntap });
            add_trigger(
                &mut program,
                etb,
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_all(
                        spellwake_engine::toolbox::ObjectQuery::AttachedHost,
                        spellwake_engine::toolbox::ObjectMutation::SetTapped(true),
                    ),
                ],
            );
            return Some(program);
        }
        "When this creature enters, you may have it deal 4 damage to target creature with flying." => {
            (
                "",
                vec![(
                    etb,
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().with_keyword(KeywordAbility::Flying)),
                    vec![
                        OptionalSequence {
                            chooser: ResolvingPlayer::Controller,
                            instructions: 1,
                        },
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Targets,
                            amount(4),
                            false,
                        ),
                    ],
                )],
            )
        }
        "Enchant creature\nEnchanted creature gets +2/+2 and attacks each combat if able." => {
            let mut program = aura()?;
            add_static(
                &mut program,
                attached(None, None),
                StaticParts(vec![literal_stats(2, 2)]),
            )?;
            program.attack_requirement = Some(AttackRequirement::AttachedCreatureAttacks);
            return Some(program);
        }
        "Enchant creature\nEnchanted creature gets +2/+2 as long as it's a Human. Otherwise, it can't attack or block." =>
        {
            let mut program = aura()?;
            let human = human()?;
            add_static(
                &mut program,
                attached(Some(human), None),
                StaticParts(vec![literal_stats(2, 2)]),
            )?;
            add_static(
                &mut program,
                attached(None, Some(human)),
                StaticParts(vec![StaticPart::BlockingRestriction(
                    BlockingRestriction::CannotAttackOrBlock,
                )]),
            )?;
            return Some(program);
        }
        "Enchant creature\nEnchanted creature gets +1/+1 and has \"{B}: Regenerate this creature.\"" =>
        {
            let mut program = aura()?;
            let grant = super::army_grants::granted_creature_ability(
                "{B}: Regenerate this creature.",
                vocabulary,
            )?;
            add_static(
                &mut program,
                attached(None, None),
                StaticParts(vec![literal_stats(1, 1)]),
            )?;
            add_static(&mut program, attached(None, None), StaticParts(vec![grant]))?;
            return Some(program);
        }
        "Flash\nEnchant creature\nEnchanted creature has \"{1}{R}: This creature gets +2/+0 until end of turn.\"" =>
        {
            let mut program = aura()?;
            program.printed_keywords.insert(KeywordAbility::Flash);
            let grant = super::army_grants::granted_creature_ability(
                "{1}{R}: This creature gets +2/+0 until end of turn.",
                vocabulary,
            )?;
            add_static(&mut program, attached(None, None), StaticParts(vec![grant]))?;
            return Some(program);
        }
        "Defender, protection from Zombies"
        | "First strike, vigilance\nProtection from Vampires, from Werewolves, and from Zombies" => {
            let (printed, labels) = if rules.starts_with("Defender") {
                ("Defender", &["Zombie"][..])
            } else {
                (
                    "First strike, vigilance",
                    &["Vampire", "Werewolf", "Zombie"][..],
                )
            };
            let mut program = base(printed)?;
            let subtypes = labels
                .iter()
                .map(|label| vocabulary.resolve_label(label))
                .collect::<Option<_>>()?;
            add_static(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceItself,
                    ..Default::default()
                },
                StaticParts(vec![StaticPart::GrantSubtypeProtection {
                    types: StaticSubtypeOperand::Literal(subtypes),
                    creatures_only: false,
                }]),
            )?;
            return Some(program);
        }
        "{2}{G}: This creature gets +2/+2 until end of turn. Activate only once each turn." => {
            let mut program = base("")?;
            let ability = activation(
                fixed_cost("{2}{G}")?,
                SourceCost::None,
                NoTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(stats(2, 2))]),
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                ],
            );
            program
                .activation_restrictions
                .insert(ability.id, ActivationRestriction::OnceEachTurn);
            program.activated.push(ability);
            return Some(program);
        }
        "This creature attacks each combat if able.\nWhenever this creature deals combat damage to a player, put a +1/+1 counter on it." =>
        {
            let mut program = base("")?;
            program.attack_requirement = Some(AttackRequirement::SourceAttacks);
            add_trigger(
                &mut program,
                spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
                NoTarget,
                vec![counter()],
            );
            return Some(program);
        }
        "Flying\nHaste\nWhenever this creature deals combat damage to a player, put two +1/+1 counters on it." => {
            (
                "Flying, haste",
                vec![(
                    spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
                    NoTarget,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(spellwake_engine::effects::CounterKind::PlusOne, amount(2))],
                )],
            )
        }
        "Flying\nWhenever this creature or another creature dies, target player loses 1 life and you gain 1 life." => {
            (
                "Flying",
                vec![(
                    spellwake_engine::program::TriggerSpec::dies(ObjectFilter::creature()),
                    SinglePlayer,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetPlayer,
                            ),
                            amount(1),
                        ),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            amount(1),
                        ),
                    ],
                )],
            )
        }
        "Flying\nWhen this creature enters, look at the top two cards of your library. Put one of them into your hand and the other into your graveyard." => {
            (
                "Flying",
                vec![(
                    etb,
                    NoTarget,
                    vec![LookTopChooseExt {
                        library_owner: ResolvingPlayer::Controller,
                        chooser: ResolvingPlayer::Controller,
                        count: amount(2),
                        filter: None,
                        max_picks: 1,
                        take_all_matching: false,
                        keep_dest: spellwake_engine::program::LookTopKeepDest::Hand,
                        rest_dest: spellwake_engine::program::LookTopRestDest::Graveyard,
                        reveal_kept: false,
                        reveal_all: false,
                        reveal_only: false,
                        secondary: None,
                    }],
                )],
            )
        }
        "When this creature dies, put a +1/+1 counter on target creature you control. If that creature is a Human, put two +1/+1 counters on it instead." =>
        {
            let counters = |n: i64| crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                spellwake_engine::effects::CounterKind::PlusOne,
                amount(n),
            );
            (
                "",
                vec![(
                    spellwake_engine::program::TriggerSpec::dies(ObjectFilter::self_object()),
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled()),
                    choose_effect(
                        spellwake_engine::program::ValueExpr::object_matches(spellwake_engine::program::MatchObjectRef::TargetObject, spellwake_engine::program::ObjectFilter::creature().with_subtype(human()?)),
                        Comparison::Equal,
                        1,
                        counters(2),
                        counters(1),
                    ),
                )],
            )
        }
        "Whenever this creature deals combat damage to a player, put a +1/+1 counter on it." => (
            "",
            vec![(spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(), NoTarget, vec![counter()])],
        ),
        "Haste\nWhenever this creature deals combat damage to a player, put a +1/+1 counter on it." => {
            (
                "Haste",
                vec![(spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(), NoTarget, vec![counter()])],
            )
        }
        "Double strike\nWhenever this creature deals combat damage to a player, put a +1/+1 counter on it." => {
            (
                "Double strike",
                vec![(spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(), NoTarget, vec![counter()])],
            )
        }
        "When this creature dies, it deals 3 damage to any target." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::dies(ObjectFilter::self_object()),
                SingleAnyTarget,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        amount(3),
                        false,
                    ),
                ],
            )],
        ),
        "Flying\nAt the beginning of your upkeep, target player draws a card and loses 1 life." => {
            (
                "Flying",
                vec![(
                    spellwake_engine::program::TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller),
                    SinglePlayer,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_target_player(amount(1)),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetPlayer,
                            ),
                            amount(1),
                        ),
                    ],
                )],
            )
        }
        "Whenever this creature or another creature dies, target player mills a card." => (
            "",
            vec![(
                spellwake_engine::program::TriggerSpec::dies(ObjectFilter::creature()),
                SinglePlayer,
                vec![mill(ResolvingPlayer::TargetPlayer, 1)],
            )],
        ),
        "Haste\nWhen this creature enters, up to two target creatures can't block this turn." => {
            let mut program = base("Haste")?;
            program.triggered.push(spellwake_engine::program::TriggeredAbility {
                id: spellwake_engine::program::TriggeredAbilityId(0),
                condition: etb,
                grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(SingleCreature, vec![cannot_block()])
                    .with_permanent_target_bounds(0, 2),
            });
            return Some(program);
        }
        "This creature can't block.\nYou may cast this card from your graveyard as long as you control a Zombie." =>
        {
            let mut program = base("")?;
            program.graveyard_self_cast = Some(
                spellwake_engine::program::GraveyardSelfCast::WhileControllingSubtype(
                    vocabulary.resolve_label("Zombie")?,
                ),
            );
            add_static(
                &mut program,
                PermanentSelector {
                    controller: ControllerSelector::SourceItself,
                    ..Default::default()
                },
                StaticParts(vec![StaticPart::BlockingRestriction(
                    BlockingRestriction::CannotBlock,
                )]),
            )?;
            return Some(program);
        }
        _ => return None,
    };
    let mut program = base(printed)?;
    for (condition, target, instructions) in triggers {
        super::avacyn_restored::add_trigger(&mut program, condition, target, instructions);
    }
    Some(program)
}

pub(super) fn compile_name_restriction(
    card: &InventoryCard,
    inventory: &spellwake_content::Inventory,
    names: &mut Option<std::sync::Arc<spellwake_engine::land::NonlandCardNames>>,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal"
        || face.type_line.as_deref()? != "Enchantment"
        || face.oracle_text.as_deref()?
            != "As this enchantment enters, choose a nonland card name.\nSpells with the chosen name can't be cast."
    {
        return None;
    }
    let names = names.get_or_insert_with(|| {
        // CR 201.4 includes the selected face of split, adventurer, transform and other multiface cards.
        let options = inventory
            .cards
            .iter()
            .filter(|item| {
                item.oracle_id.is_some()
                    && !matches!(
                        item.layout.as_str(),
                        "token" | "double_faced_token" | "emblem" | "art_series" | "minigame"
                    )
            })
            .flat_map(|item| &item.faces)
            .filter(|face| {
                face.type_line.as_deref().is_some_and(|line| {
                    let card_types = line.split('—').next().unwrap_or(line);
                    !card_types.split_whitespace().any(|kind| kind == "Land")
                })
            })
            .filter_map(|face| EnglishCardName::new(face.name.clone()).ok())
            .collect();
        std::sync::Arc::new(spellwake_engine::land::NonlandCardNames::new(options))
    });
    let mut program = Program::enchantment(Some(fixed_cost(face.mana_cost.as_deref()?)?));
    program.continuous = Some(
        spellwake_engine::characteristics::StaticContinuousAbility::Restriction {
            id: StaticAbilityId(0),
            rule: spellwake_engine::characteristics::StaticRestriction::ProhibitCastingChosenName(
                names.clone(),
            ),
        },
    );
    let ProgramKind::Permanent(permanent) = &mut program.kind else {
        return None;
    };
    permanent.entry_tap = Some(EntryTap::ChooseNonlandCardName);
    Some(program)
}
