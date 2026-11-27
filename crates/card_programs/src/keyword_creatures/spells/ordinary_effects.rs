//! Literal Oracle effects sharing one target and resolving in printed order.
use super::*;
use spellwake_engine::program::{
    CardTypeMask, EffectSubject, ManaValueFilter, ObjectClause, ObjectFilter, PowerFilter,
    ResolvingPlayer,
};

/// These forms describe resolving spells. In particular, a permanent's fixed
/// mana activation must remain on the mana-ability path (CR 605).
pub(super) fn spell_body(text: &str, name: &str) -> Option<ExecutableBody> {
    let rules = text.trim();
    if let Some(symbols) = rules
        .strip_prefix("Add ")
        .and_then(|line| line.strip_suffix('.'))
        && let Some(mana) = fixed_cost(symbols)
        && mana.generic == 0.into()
        && !mana.required.is_empty()
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                    ResolvingPlayer::Controller,
                    spellwake_engine::program::EffectManaYield::Fixed(mana.required),
                ),
            ))],
        ));
    }
    if let Some(amount) = rules
        .strip_prefix("Target player mills ")
        .and_then(|line| line.strip_suffix(" cards."))
        .and_then(number)
    {
        return Some(ExecutableBody::new(
            BodyTarget::SINGLE_PLAYER,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::mill(
                    ResolvingPlayer::TargetPlayer,
                    spellwake_engine::program::ValueExpr::Literal(amount.into()),
                ),
            ))],
        ));
    }
    if rules == "Switch target creature's power and toughness until end of turn." {
        use spellwake_engine::characteristics::{
            ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        };
        return Some(ExecutableBody::new(
            BodyTarget::SINGLE_CREATURE,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                    ResolvedDuration::UntilEndOfTurn,
                ),
            ],
        ));
    }
    body(text, name)
}

pub(in crate::keyword_creatures) fn body(text: &str, name: &str) -> Option<ExecutableBody> {
    if let Some(limit) = text
        .trim()
        .strip_prefix("Return target creature card with mana value ")
        .and_then(|rest| rest.strip_suffix(" or less from your graveyard to the battlefield."))
        .and_then(|amount| amount.parse::<u32>().ok())
    {
        return Some(ExecutableBody::new(
            BodyTarget::filtered_own_graveyard(
                ObjectFilter::creature().with_mana_value(ManaValueFilter::AtMost(limit)),
            ),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(false)],
        ));
    }
    if text.trim()
        == "Each player shuffles their hand and graveyard into their library, then draws seven cards. If it's your turn, end the turn."
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::shuffle_each_hand_and_graveyard(),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_each_player(
                    spellwake_engine::program::ValueExpr::Literal(7.into()),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::end_turn(),
            ],
        ));
    }
    if let Some(effect) = text
        .trim()
        .strip_prefix("Exile target creature you control, then return ")
    {
        let (return_text, draw) = effect
            .strip_suffix(" Draw a card.")
            .map_or((effect, false), |line| (line, true));
        let destination = return_text
            .strip_prefix("it ")
            .or_else(|| return_text.strip_prefix("that card "))?;
        let owner_controls = match destination {
            "to the battlefield under your control." => false,
            "to the battlefield under its owner's control." => true,
            _ => return None,
        };
        let mut instructions = vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
            Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::return_exiled_target_cohort(
                    false,
                    owner_controls,
                ),
            )),
        ];
        if draw {
            instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            );
        }
        return Some(ExecutableBody::new(
            BodyTarget::filtered_permanent(ObjectFilter::creature().controlled()),
            instructions,
        ));
    }
    let exile_return = match text.trim() {
        "Exile two target artifacts, creatures, and/or lands you control, then return those cards to the battlefield under your control." => {
            Some((
                ObjectFilter::any_types(
                    CardTypeMask::ARTIFACT
                        .union(CardTypeMask::CREATURE)
                        .union(CardTypeMask::LAND),
                )
                .controlled(),
                2,
                2,
                false,
            ))
        }
        "Exile any number of target creatures and/or planeswalkers you control. At the beginning of the next end step, return each of them to the battlefield under its owner's control. Each of them enters with an additional +1/+1 counter on it if it's a creature and an additional loyalty counter on it if it's a planeswalker." => {
            Some((
                ObjectFilter::creature_or_planeswalker().controlled(),
                0,
                u16::MAX,
                true,
            ))
        }
        _ => None,
    };
    if let Some((filter, minimum, maximum, next_end_step)) = exile_return {
        return Some(
            ExecutableBody::new(
                BodyTarget::filtered_permanent(filter),
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
                    Instruction::ZoneOp(Box::new(
                        spellwake_engine::toolbox::ZoneOpSpec::return_exiled_target_cohort(
                            next_end_step,
                            false,
                        ),
                    )),
                ],
            )
            .with_permanent_target_bounds(minimum, maximum),
        );
    }
    if text.trim() == "Gain control of target noncreature spell. You may choose new targets for it."
    {
        return Some(ExecutableBody::new(
            BodyTarget::single_spell(spellwake_engine::program::SpellFilter::noncreature()),
            vec![
                Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::gain_control(),
                )),
                Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::retarget(
                    false,
                ))),
            ],
        ));
    }
    if let Some(kind) = text
        .trim()
        .strip_prefix("Put up to two ")
        .and_then(|line| line.strip_suffix(" cards from your hand onto the battlefield."))
        && let Some(kind) = match kind {
            "creature" => Some(CardType::Creature),
            "artifact" => Some(CardType::Artifact),
            _ => None,
        }
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::put_hand_permanents_onto_battlefield(
                    ResolvingPlayer::Controller,
                    [kind].into(),
                    2,
                    false,
                ),
            ))],
        ));
    }
    let lower = text.to_ascii_lowercase();
    if lower.trim()
        == "creatures you control get +3/+3 until end of turn. each creature your opponents control blocks this turn if able."
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                    EffectSubject::Matching(ObjectFilter::creature().controlled()),
                    spellwake_engine::program::ValueExpr::literal(3),
                    spellwake_engine::program::ValueExpr::literal(3),
                    BTreeSet::new(),
                    BTreeSet::new(),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::require_combat_action(
                    EffectSubject::Matching(ObjectFilter::creature().opposing()),
                    spellwake_engine::program::CombatActionKind::MustBlock,
                ),
            ],
        ));
    }
    if let Some(remainder) = lower.trim().strip_prefix("target creature you control deals damage equal to its power to target creature you don't control.") {
        let trample_overflow = match remainder.trim() {
            "" => false,
            "if the creature you control has trample, excess damage is dealt to that creature's controller instead." => true,
            _ => return None,
        };
        let fixed = |filter| Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
            BodyTarget::filtered_permanent(filter),
            vec![Instruction::MutateObject(Box::new(
                spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
            ))],
        )));
        return Some(ExecutableBody::new(BodyTarget::None, vec![
            fixed(ObjectFilter::creature().controlled()),
            fixed(ObjectFilter::creature().opposing()),
            Instruction::DealDamage(Box::new(
                spellwake_engine::toolbox::DealDamageSpec::creature_power_to_other(
                    trample_overflow,
                ),
            )),
        ]));
    }
    if let Some(stats) = lower.trim().strip_prefix("until end of turn, target creature gets ")
        .and_then(|clause| clause.strip_suffix(" and gains \"when this creature dies, return it to the battlefield tapped under its owner's control.\""))
    {
        use spellwake_engine::characteristics::{AbilityChange, PowerToughness, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec};
        use spellwake_engine::program::{TriggerGrouping};
        let (power, toughness) = stats.split_once('/')?;
        let grant = TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::dies(spellwake_engine::program::ObjectFilter::self_object()),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
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
                                controller: spellwake_engine::toolbox::ZoneController::Owner,
                                tapped: true,
                                attacking: false,
                                transformed: false,
                                face_down: None,
                                entering_counters: Vec::new(),
                                attach_to: None,
                            },
                        ),
                    ),
                ],
            ),
        };
        let parts = ResolvedPartsSpec(vec![
            ResolvedPartSpec::Abilities(AbilityChange { granted_triggers: vec![grant], ..Default::default() }),
            ResolvedPartSpec::ModifyPT(PowerToughness {
                power: signed_stat(power)?.into(), toughness: signed_stat(toughness)?.into(),
            }),
        ]);
        return Some(ExecutableBody::new(BodyTarget::SINGLE_CREATURE, vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, parts, ResolvedDuration::UntilEndOfTurn),
        ]));
    }
    if lower.trim()
        == "destroy all creatures. they can't be regenerated. draw a card for each creature destroyed this way."
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                    ObjectFilter::creature(),
                    false,
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::event(
                        spellwake_engine::program::EventValueKind::DestroyedPermanents,
                    ),
                ),
            ],
        ));
    }
    if let Some((limit, production)) = lower
        .trim()
        .strip_prefix("destroy each nonland permanent with mana value ")
        .and_then(|clause| clause.split_once(" or less. add "))
        && let Some((first, second)) = production
            .strip_suffix(" for each permanent destroyed this way.")
            .and_then(|clause| clause.split_once(" or "))
    {
        use spellwake_engine::mana::ManaType;
        let color = |symbol| match symbol {
            "{w}" => Some(ManaType::White),
            "{u}" => Some(ManaType::Blue),
            "{b}" => Some(ManaType::Black),
            "{r}" => Some(ManaType::Red),
            "{g}" => Some(ManaType::Green),
            _ => None,
        };
        let first = color(first)?;
        let second = color(second)?;
        if first == second {
            return None;
        }
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                    ObjectFilter::nonland()
                        .with_mana_value(ManaValueFilter::AtMost(limit.parse().ok()?)),
                    true,
                ),
                Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                        ResolvingPlayer::Controller,
                        spellwake_engine::program::EffectManaYield::Split {
                            first,
                            second,
                            amount: spellwake_engine::program::ValueExpr::event(
                                spellwake_engine::program::EventValueKind::DestroyedPermanents,
                            ),
                        },
                    ),
                )),
            ],
        ));
    }
    if let Some(words) = lower
        .strip_prefix("you and target opponent each draw ")
        .and_then(|s| {
            s.strip_suffix(" cards.")
                .or_else(|| s.strip_suffix(" card."))
        })
    {
        let draw_amt = spellwake_engine::program::ValueExpr::Literal(
            super::super::ordinary_text::amount(words)?.into(),
        );
        return Some(ExecutableBody::new(
            BodyTarget::SINGLE_OPPONENT,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    draw_amt.clone(),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_target_player(
                    draw_amt,
                ),
            ],
        ));
    }
    if let Some(count) = lower.strip_prefix("each player mills ").and_then(|words| {
        words
            .strip_suffix(" cards.")
            .or_else(|| words.strip_suffix(" card."))
    }) {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::mill_players(
                    spellwake_engine::program::AffectedPlayers::EachPlayer,
                    spellwake_engine::program::ValueExpr::Literal(
                        super::super::ordinary_text::amount(count)?.into(),
                    ),
                ),
            ],
        ));
    }
    if lower
        == "untap all creatures and gain control of them until end of turn. they gain haste until end of turn."
    {
        use spellwake_engine::characteristics::{
            AbilityChange, ControlRecipient, KeywordAbility, ResolvedDuration, ResolvedPartSpec,
            ResolvedPartsSpec,
        };
        return Some(ExecutableBody::new(
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
        ));
    }
    if let Some(gain) = lower
        .strip_prefix("put a +1/+1 counter on target creature you control. it gains ")
        .and_then(|s| s.strip_suffix(" until end of turn."))
    {
        use spellwake_engine::characteristics::{
            AbilityChange, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        };
        let grant = super::super::static_keyword(gain)?;
        return Some(ExecutableBody::new(
            BodyTarget::filtered_permanent(ObjectFilter::creature().controlled()),
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                    spellwake_engine::effects::CounterKind::PlusOne,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                        AbilityChange::grant_keywords([grant]),
                    )]),
                    ResolvedDuration::UntilEndOfTurn,
                ),
            ],
        ));
    }
    if text.trim()
        == "Target creature gets -1/-1 until end of turn.
Morbid — That creature gets -13/-13 until end of turn instead if a creature died this turn."
        || text.trim()
            == "Target creature gets -1/-1 until end of turn. Morbid — That creature gets -13/-13 until end of turn instead if a creature died this turn."
    {
        return Some(ExecutableBody::new(
            BodyTarget::SINGLE_CREATURE,
            vec![
                Instruction::BranchCondition {
                    condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                        Box::new(ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::event(
                                spellwake_engine::program::EventValueKind::CreaturesDiedThisTurn,
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        }),
                    )),
                    destination: 3,
                },
                pump(-13, -13, None),
                Instruction::Jump { destination: 4 },
                pump(-1, -1, None),
            ],
        ));
    }
    if lower.split_whitespace().collect::<Vec<_>>().join(" ")
        == "target creature gains double strike until end of turn. ferocious — that creature also gains trample until end of turn if you control a creature with power 4 or greater."
    {
        let grant = |key| {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                spellwake_engine::characteristics::ResolvedPartsSpec(vec![
                    spellwake_engine::characteristics::ResolvedPartSpec::Abilities(
                        spellwake_engine::characteristics::AbilityChange::grant_keywords([key]),
                    ),
                ]),
                spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
            )
        };
        return Some(ExecutableBody::new(
            BodyTarget::SINGLE_CREATURE,
            vec![
                grant(KeywordAbility::DoubleStrike),
                Instruction::BranchCondition {
                    condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                        Box::new(ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::aggregate(
                                spellwake_engine::program::ObjectFilter::creature().controlled(),
                                spellwake_engine::program::ObjectStatKind::Power,
                                spellwake_engine::program::StatAggregate::Max,
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: spellwake_engine::program::ValueExpr::Literal(4.into()),
                        }),
                    )),
                    destination: 3,
                },
                grant(KeywordAbility::Trample),
            ],
        ));
    }
    if let Some(rest) = text.strip_prefix(name).and_then(|s|s.strip_prefix(" deals ")) &&
        let Some((normal,enhanced)) = rest.split_once(" damage to each creature and each player. If you cast this spell during your main phase, ") &&
        let Some(enhanced) = enhanced.strip_prefix(name).and_then(|s|s.strip_prefix(" deals ")).and_then(|s|s.strip_suffix(" damage to each creature and each player instead.")) {
        let damage = |value: &str| {
            Some(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                        spellwake_engine::program::DamageRecipientsRef::Group(
                            spellwake_engine::program::DamageGroup::CreaturesAndPlayers,
                        ),
                    )),
                    spellwake_engine::program::ValueExpr::literal(number(value)?),
                    false,
                ),
            )
        };
        return Some(ExecutableBody::new(BodyTarget::None,vec![Instruction::BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(ResolutionCondition {left:spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::CastDuringOwnMain),
            comparison:Comparison::Equal,right:spellwake_engine::program::ValueExpr::Literal(1.into())}))), destination:3 },damage(enhanced)?,Instruction::Jump {destination:4},damage(normal)?]));
    }
    if lower
        == "draw a card. until end of turn, target creature gains trample and gets +1/+0 for each card you've drawn this turn."
    {
        return Some(ExecutableBody::new(
            BodyTarget::SINGLE_CREATURE,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                    EffectSubject::TargetObject,
                    spellwake_engine::program::ValueExpr::PlayerStat(
                        spellwake_engine::program::PlayerStatExpr::CardsDrawnThisTurn,
                    ),
                    spellwake_engine::program::ValueExpr::literal(0),
                    [KeywordAbility::Trample].into(),
                    BTreeSet::new(),
                ),
            ],
        ));
    }
    if lower == "target creature can't attack or block this turn." {
        use spellwake_engine::characteristics::{
            BlockingRestriction, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        };
        return Some(ExecutableBody::new(
            BodyTarget::SINGLE_CREATURE,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::CannotAttackOrBlock,
                    )]),
                    ResolvedDuration::UntilEndOfTurn,
                ),
            ],
        ));
    }
    let multi = match lower.as_str() {
        "exile two target permanents." => Some((
            BodyTarget::SINGLE_PERMANENT,
            2,
            2,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
        )),
        "destroy two target creatures." => Some((
            BodyTarget::SINGLE_CREATURE,
            2,
            2,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_targets(),
        )),
        "return six target nonland permanents to their owners' hands." => Some((
            BodyTarget::filtered_permanent(ObjectFilter::nonland()),
            6,
            6,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_hand(),
        )),
        "put a +1/+1 counter on each of up to two other target creatures." => Some((
            BodyTarget::filtered_permanent(ObjectFilter::creature().other_than_ability_source()),
            0,
            2,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                spellwake_engine::effects::CounterKind::PlusOne,
                spellwake_engine::program::ValueExpr::literal(1),
            ),
        )),
        "put a +1/+1 counter on each of up to two target creatures." => Some((
            BodyTarget::SINGLE_CREATURE,
            0,
            2,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                spellwake_engine::effects::CounterKind::PlusOne,
                spellwake_engine::program::ValueExpr::literal(1),
            ),
        )),
        "untap target permanent." => Some((
            BodyTarget::SINGLE_PERMANENT,
            1,
            1,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(false),
        )),
        "untap another target permanent." => Some((
            BodyTarget::filtered_permanent(ObjectFilter::permanent().other_than_ability_source()),
            1,
            1,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(false),
        )),
        "untap up to four target permanents." => Some((
            BodyTarget::SINGLE_PERMANENT,
            0,
            4,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(false),
        )),
        _ => None,
    };
    if let Some((target, minimum, maximum, op)) = multi {
        return Some(
            ExecutableBody::new(target, vec![op]).with_permanent_target_bounds(minimum, maximum),
        );
    }
    if lower == "each player sacrifices all permanents they control that are one or more colors." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Battlefield,
                            ObjectFilter::any()
                                .with_color(spellwake_engine::program::ObjectColorFilter::Colored),
                        ),
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice,
                        },
                    ),
                ),
            ],
        ));
    }
    if lower == "destroy all creatures. they can't be regenerated." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                    ObjectFilter::creature(),
                    false,
                ),
            ],
        ));
    }
    if let Some(result) = lower
        .strip_prefix("each player discards their hand, then draws ")
        .and_then(|s| s.strip_suffix('.'))
    {
        let amount = if result
            == "cards equal to the greatest number of cards a player discarded this way"
        {
            spellwake_engine::program::ValueExpr::event(
                spellwake_engine::program::EventValueKind::LargestDiscard,
            )
        } else {
            spellwake_engine::program::ValueExpr::Literal(
                super::super::ordinary_text::amount(
                    result
                        .strip_suffix(" cards")
                        .or_else(|| result.strip_suffix(" card"))?,
                )?
                .into(),
            )
        };
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                    spellwake_engine::program::AffectedPlayers::EachPlayer,
                    spellwake_engine::program::ValueExpr::PlayerStat(
                        spellwake_engine::program::PlayerStatExpr::LargestHandSize,
                    ),
                ))),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_each_player(amount),
            ],
        ));
    }
    if lower == "each player draws a card, then discards a card." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_each_player(
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
                Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                    spellwake_engine::program::AffectedPlayers::EachPlayer,
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ))),
            ],
        ));
    }
    if let Some(result) = lower.strip_prefix("choose a creature type. ") {
        use spellwake_engine::program::CreatureTypeSlot;
        let slot = CreatureTypeSlot::new(0);
        let operation = match result {
            "destroy all creatures that aren't of the chosen type." => Some(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                    ObjectFilter::creature().except_chosen_slot(slot),
                    true,
                ),
            ),
            effect if effect.starts_with("creatures that aren't of the chosen type get ") => {
                let stats = effect
                    .strip_prefix("creatures that aren't of the chosen type get ")?
                    .strip_suffix(" until end of turn.")?;
                let (p, t) = stats.split_once('/')?;
                Some(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        EffectSubject::Matching(ObjectFilter::creature().except_chosen_slot(slot)),
                        spellwake_engine::program::ValueExpr::Literal(
                            p.parse::<i64>().ok()?.into(),
                        ),
                        spellwake_engine::program::ValueExpr::Literal(
                            t.parse::<i64>().ok()?.into(),
                        ),
                        BTreeSet::new(),
                        BTreeSet::new(),
                    ),
                )
            }
            _ => None,
        };
        if let Some(operation) = operation {
            return Some(ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::choose_creature_type(
                        slot,
                        BTreeSet::new(),
                    ),
                    operation,
                ],
            ));
        }
    }
    if text.eq_ignore_ascii_case(
        "Choose a creature type. Draw a card for each permanent you control of that type.",
    ) {
        let slot = spellwake_engine::program::CreatureTypeSlot::new(0);
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::choose_creature_type(
                    slot,
                    BTreeSet::new(),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::count_battlefield(
                        spellwake_engine::program::ObjectFilter::any()
                            .controlled()
                            .of_chosen_slot(slot),
                    ),
                ),
            ],
        ));
    }
    let mut instructions = Vec::new();
    let mut target = BodyTarget::None;
    // Removing the self name first preserves periods in actual card names.
    let text = text.replace(name, "CARDNAME");
    for clause in text
        .strip_suffix('.')?
        .split(". ")
        .flat_map(|sentence| sentence.split(", then "))
    {
        let compound = clause.to_ascii_lowercase();
        if let Some((first, next)) = compound
            .split_once(" and you lose ")
            .filter(|(first, _)| first.starts_with("you draw ") || first.starts_with("draw "))
        {
            let (BodyTarget::None, draw) = instruction(first)? else {
                return None;
            };
            instructions.extend([draw, controller_life(next, false)?.1]);
            continue;
        }
        if let Some((first, next)) = compound.split_once(" and draw ").filter(|(first, _)| {
            first.starts_with("you gain ")
                || first.starts_with("put a +1/+1 counter on this creature")
        }) {
            let (BodyTarget::None, first) = instruction(first)? else {
                return None;
            };
            let (BodyTarget::None, draw) = instruction(&format!("draw {next}"))? else {
                return None;
            };
            instructions.extend([first, draw]);
            continue;
        }
        let (effect_text, gain) = clause
            .split_once(" and you gain ")
            .map_or((clause, None), |(effect, life)| (effect, Some(life)));
        let (operand, effect) = instruction(effect_text)?;
        if operand != BodyTarget::None {
            // Two printed targets may differ, even if both say "target creature".
            if target != BodyTarget::None {
                return None;
            }
            target = operand;
        }
        if gain.is_some()
            && matches!(
                &effect,
                Instruction::DealDamage(spec)
                    if matches!(
                        &spec.recipients,
                        spellwake_engine::toolbox::DamageTargetQuery::Relative(rel)
                            if matches!(
                                rel.as_ref(),
                                spellwake_engine::program::DamageRecipientsRef::Group(_)
                            )
                    )
            )
        {
            return None;
        }
        instructions.push(effect);
        if let Some(life) = gain {
            instructions.push(controller_life(life, true)?.1);
        }
    }
    Some(ExecutableBody::new(target, instructions))
}

/// Preserve the former controller when the second sentence refers to the exiled permanent.
pub(in crate::keyword_creatures) fn exile_followup(
    text: &str,
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ExecutableBody> {
    if let Some(subject) = text
        .strip_suffix(". You lose life equal to that permanent’s mana value.")
        .or_else(|| text.strip_suffix(". You lose life equal to that permanent's mana value."))
    {
        let mut destruction = body(&format!("{subject}."), name)?;
        if destruction.instructions.as_slice()
            != [
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
            ]
        {
            return None;
        }
        destruction.instructions = vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                spellwake_engine::toolbox::ZoneOpSpec {
                    bind_snapshots: Some(spellwake_engine::toolbox::BindingId(0)),
                    ..spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: true,
                            },
                        },
                    )
                },
            ),
            crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                spellwake_engine::program::ValueExpr::BoundStat {
                    binding: spellwake_engine::toolbox::BindingId(0),
                    stat: spellwake_engine::program::ObjectStatKind::ManaValue,
                    op: spellwake_engine::program::StatAggregate::Sum,
                },
            ),
        ];
        return Some(destruction);
    }
    let (exile, next) = text.split_once(". Its controller ")?;
    let mut body = body(&format!("{exile}."), name)?;
    let destroyed = body.instructions.as_slice()
        == [
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
        ];
    if !destroyed
        && body.instructions.as_slice()
            != [crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target()]
    {
        return None;
    }
    if destroyed {
        body.instructions.insert(
            0,
            Instruction::MutateObject(Box::new(
                spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(),
            )),
        );
    }

    if next
        == "may search their library for a basic land card, put that card onto the battlefield tapped, then shuffle."
    {
        if destroyed {
            return None;
        }
        body.instructions
            .extend(super::super::basic_land_search::exiled_target_controller_search());
        return Some(body);
    }
    if !destroyed
        && next == "manifests the top card of their library."
        && body.target == BodyTarget::SINGLE_CREATURE
    {
        body.instructions.push(Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::manifest_top(
                ResolvingPlayer::ExiledTargetController,
                spellwake_engine::program::ValueExpr::Literal(1.into()),
            ),
        )));
        return Some(body);
    }
    let followup = if !destroyed
        && next == "gains life equal to its power."
        && body.target == BodyTarget::SINGLE_CREATURE
    {
        if let Some(Instruction::ZoneOp(spec)) = body.instructions.first_mut() {
            spec.bind_snapshots = Some(spellwake_engine::toolbox::BindingId(1));
        }
        crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
            spellwake_engine::toolbox::PlayerSelector::Bound(spellwake_engine::toolbox::BindingId(
                1,
            )),
            spellwake_engine::program::ValueExpr::BoundStat {
                binding: spellwake_engine::toolbox::BindingId(1),
                stat: spellwake_engine::program::ObjectStatKind::Power,
                op: spellwake_engine::program::StatAggregate::Sum,
            },
        )
    } else {
        let literal = next.strip_prefix("creates ")?.strip_suffix('.')?;
        let Instruction::CreateTokens { mut creations } =
            super::super::ordinary_text::create_tokens(literal, vocabulary)?
        else {
            return None;
        };
        for creation in &mut creations {
            creation.recipients = if destroyed {
                spellwake_engine::token::TokenRecipients::RememberedTargetController
            } else {
                spellwake_engine::token::TokenRecipients::ExiledTargetController
            };
        }
        Instruction::CreateTokens { creations }
    };
    body.instructions.push(followup);
    Some(body)
}

fn number(text: &str) -> Option<i64> {
    match text {
        "a" | "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        _ if text.bytes().all(|c| c.is_ascii_digit()) => text.parse().ok(),
        _ => None,
    }
}

fn controller_life(text: &str, gain: bool) -> Option<(BodyTarget, Instruction)> {
    let amount =
        spellwake_engine::program::ValueExpr::literal(number(text.strip_suffix(" life")?)?);
    let controller =
        spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller);
    Some((
        BodyTarget::None,
        if gain {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(controller, amount)
        } else {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(controller, amount)
        },
    ))
}

fn signed_stat(text: &str) -> Option<i64> {
    text.strip_prefix(['+', '-'])
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))?;
    text.parse().ok()
}

/// A single continuous effect snapshots every qualifying permanent on resolution.
pub(in crate::keyword_creatures) fn group_modifier(clause: &str) -> Option<Instruction> {
    use spellwake_engine::characteristics::PowerToughness;
    let lower = clause.to_ascii_lowercase();
    if lower
        == "it gets +x/+x until end of turn, where x is the greatest power among other creatures you control"
    {
        let amt = spellwake_engine::program::ValueExpr::aggregate(
            spellwake_engine::program::ObjectFilter::creature()
                .controlled()
                .other_than_ability_source(),
            spellwake_engine::program::ObjectStatKind::Power,
            spellwake_engine::program::StatAggregate::Max,
        );
        return Some(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                EffectSubject::AbilitySource,
                amt.clone(),
                amt,
                BTreeSet::new(),
                BTreeSet::new(),
            ),
        );
    }
    if lower == "it gets +1/+1 until end of turn for each land you control" {
        let amt = spellwake_engine::program::ValueExpr::controlled_type(CardType::Land);
        return Some(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                EffectSubject::AbilitySource,
                amt.clone(),
                amt,
                BTreeSet::new(),
                BTreeSet::new(),
            ),
        );
    }
    if let Some((effect, count)) = lower.split_once(" until end of turn, where x is the number of ")
    {
        let amount = match count {
            "creatures you control" => {
                spellwake_engine::program::ValueExpr::controlled_type(CardType::Creature)
            }
            "attacking creatures" => spellwake_engine::program::ValueExpr::count_battlefield(
                spellwake_engine::program::ObjectFilter::creature().attacking(),
            ),
            _ => return None,
        };
        let (group, line) = if let Some(line) = effect.strip_prefix("creatures you control ") {
            (
                EffectSubject::Matching(ObjectFilter::creature().controlled()),
                line,
            )
        } else {
            (
                EffectSubject::Matching(ObjectFilter::creature().controlled().attacking()),
                effect.strip_prefix("attacking creatures you control ")?,
            )
        };
        let mut keywords = BTreeSet::new();
        if line != "get +x/+x" {
            for word in line
                .strip_prefix("gain ")?
                .strip_suffix(" and get +x/+x")?
                .split(" and ")
            {
                keywords.insert(super::super::static_keyword(word)?);
            }
        }
        return Some(
            crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                group,
                amount.clone(),
                amount,
                keywords,
                BTreeSet::new(),
            ),
        );
    }
    let printed = lower.strip_suffix(" until end of turn")?;
    let (group, effect) = [
        (
            "each creature you control with power 4 or greater ",
            EffectSubject::Matching(
                ObjectFilter::creature()
                    .controlled()
                    .with_power(PowerFilter::AtLeast(4)),
            ),
        ),
        (
            "all creatures ",
            EffectSubject::Matching(ObjectFilter::creature()),
        ),
        (
            "creatures you control ",
            EffectSubject::Matching(ObjectFilter::creature().controlled()),
        ),
        (
            "creatures your opponents control ",
            EffectSubject::Matching(ObjectFilter::creature().opposing()),
        ),
        (
            "attacking creatures ",
            EffectSubject::Matching(ObjectFilter::creature().attacking()),
        ),
        (
            "permanents you control ",
            EffectSubject::Matching(ObjectFilter::permanent().controlled()),
        ),
    ]
    .into_iter()
    .find_map(|(subject, group)| printed.strip_prefix(subject).map(|effect| (group, effect)))?;
    let (stats, grants) = if let Some(value) = effect
        .strip_prefix("get ")
        .or_else(|| effect.strip_prefix("gets "))
    {
        let (stats, grants) = value
            .split_once(" and gain ")
            .or_else(|| value.split_once(" and gains "))
            .map_or((value, None), |(s, k)| (s, Some(k)));
        let (power, toughness) = stats.split_once('/')?;
        (
            Some(PowerToughness {
                power: signed_stat(power)?.into(),
                toughness: signed_stat(toughness)?.into(),
            }),
            grants,
        )
    } else {
        (
            None,
            Some(
                effect
                    .strip_prefix("gain ")
                    .or_else(|| effect.strip_prefix("gains "))?,
            ),
        )
    };
    let mut keywords = BTreeSet::new();
    if let Some(grants) = grants {
        for word in grants.split(" and ") {
            keywords.insert(super::super::static_keyword(word)?);
        }
    }
    let (power, toughness) = match stats {
        Some(pt) => (
            spellwake_engine::program::ValueExpr::Literal(pt.power),
            spellwake_engine::program::ValueExpr::Literal(pt.toughness),
        ),
        None => (
            spellwake_engine::program::ValueExpr::literal(0),
            spellwake_engine::program::ValueExpr::literal(0),
        ),
    };
    Some(
        crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
            group,
            power,
            toughness,
            keywords,
            BTreeSet::new(),
        ),
    )
}

/// Bind the player role once for the existing simultaneous sacrifice and discard operations.
fn player_selection(clause: &str) -> Option<(BodyTarget, Instruction)> {
    use spellwake_engine::program::{AffectedPlayers, PermanentTypeFilter};
    let (target, players, action) = [
        (
            "each other player ",
            BodyTarget::None,
            AffectedPlayers::EachOtherPlayer,
        ),
        (
            "each opponent ",
            BodyTarget::None,
            AffectedPlayers::EachOpponent,
        ),
        (
            "each player ",
            BodyTarget::None,
            AffectedPlayers::EachPlayer,
        ),
        (
            "target player ",
            BodyTarget::SINGLE_PLAYER,
            AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
        ),
    ]
    .into_iter()
    .find_map(|(prefix, target, players)| {
        clause
            .strip_prefix(prefix)
            .map(|action| (target, players, action))
    })?;
    if let Some(cards) = action.strip_prefix("discards ") {
        let count = cards
            .strip_suffix(" cards")
            .or_else(|| cards.strip_suffix(" card"))?;
        return Some((
            target,
            Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                players,
                spellwake_engine::program::ValueExpr::Literal(number(count)?.into()),
            ))),
        ));
    }
    let chosen = action
        .strip_prefix("sacrifices ")?
        .strip_suffix(" of their choice")?;
    let (count, kind) = chosen.split_once(' ')?;
    let amount = if matches!(count, "a" | "an") {
        1
    } else {
        number(count)?
    };
    let any_types = match kind {
        "creature or planeswalker" => [CardType::Creature, CardType::Planeswalker].into(),
        "artifact or creature" => [CardType::Artifact, CardType::Creature].into(),
        _ => BTreeSet::new(),
    };
    let required_types = match kind {
        _ if !any_types.is_empty() => BTreeSet::new(),
        "permanent" | "permanents" => BTreeSet::new(),
        "creature" | "creatures" => BTreeSet::from([CardType::Creature]),
        "enchantment" | "enchantments" => BTreeSet::from([CardType::Enchantment]),
        "artifact" | "artifacts" => BTreeSet::from([CardType::Artifact]),
        "land" | "lands" => BTreeSet::from([CardType::Land]),
        _ => return None,
    };
    Some((
        target,
        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
            players,
            spellwake_engine::program::ValueExpr::Literal(amount.into()),
            PermanentTypeFilter {
                any_types,
                required_types,
                forbidden_types: BTreeSet::new(),
                object_filter: None,
            },
        ))),
    ))
}

fn filtered_target(words: &str) -> Option<BodyTarget> {
    if words == "creature" {
        return Some(BodyTarget::SINGLE_CREATURE);
    }
    if words == "permanent" {
        return Some(BodyTarget::SINGLE_PERMANENT);
    }
    Some(BodyTarget::filtered_permanent(match words {
        "artifact you don't control" => ObjectFilter::artifact().not_controlled(),
        "nonland permanent you don't control" => ObjectFilter::nonland().not_controlled(),
        "artifact" => ObjectFilter::artifact(),
        "enchantment" => ObjectFilter::enchantment(),
        "land" => ObjectFilter::land(),
        "land you control" => ObjectFilter::land().controlled(),
        "creature an opponent controls" => ObjectFilter::creature().opposing(),
        "nontoken creature" => ObjectFilter::creature().nontoken(),
        "attacking creature" => ObjectFilter::creature().attacking(),
        "attacking or blocking creature" => ObjectFilter::creature().attacking_or_blocking(),
        "creature with power 4 or greater" => {
            ObjectFilter::creature().with_power(PowerFilter::AtLeast(4))
        }
        "creature with power or toughness 1 or less" => {
            ObjectFilter::creature().with_power(PowerFilter::PowerOrToughnessAtMost(1))
        }
        "creature with no counters on it" => ObjectFilter::creature().without_counters(),
        "artifact, enchantment, or land" => ObjectFilter::any_types(
            CardTypeMask::ARTIFACT
                .union(CardTypeMask::ENCHANTMENT)
                .union(CardTypeMask::LAND),
        ),
        "creature or enchantment an opponent controls" => {
            ObjectFilter::creature_or_enchantment().opposing()
        }
        "enchantment, tapped artifact, or tapped creature" => ObjectFilter::either(
            ObjectClause::enchantment(),
            ObjectClause::any()
                .any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::CREATURE))
                .tapped(),
        ),
        "tapped creature" => ObjectFilter::creature().tapped(),
        "nonland permanent" => ObjectFilter::nonland(),
        "nonland permanent an opponent controls" => ObjectFilter::nonland().opposing(),
        "artifact or enchantment" => ObjectFilter::artifact_or_enchantment(),
        "artifact or creature" => ObjectFilter::artifact_or_creature(),
        "artifact, enchantment, or creature with flying" => ObjectFilter::either(
            ObjectClause::any().any_types(CardTypeMask::ARTIFACT.union(CardTypeMask::ENCHANTMENT)),
            ObjectClause::creature().with_keyword(KeywordAbility::Flying),
        ),
        _ => return None,
    }))
}

fn instruction(text: &str) -> Option<(BodyTarget, Instruction)> {
    use BodyTarget::None as NoTarget;
    let line = text.to_ascii_lowercase();
    let line = line.as_str();
    if line
        == "target creature you control gains protection from the color of your choice until end of turn"
    {
        return Some((
            BodyTarget::filtered_permanent(ObjectFilter::creature().controlled()),
            Instruction::ApplyContinuous(Box::new(
                spellwake_engine::toolbox::ApplyContinuousSpec::chosen_color_protection(
                    spellwake_engine::program::ProtectionRecipients::TargetPermanent,
                    spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                ),
            )),
        ));
    }
    if line == "put a +1/+1 counter on target creature" {
        return Some((
            BodyTarget::SINGLE_CREATURE,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                spellwake_engine::effects::CounterKind::PlusOne,
                spellwake_engine::program::ValueExpr::literal(1),
            ),
        ));
    }
    if line == "double the number of each kind of counter on any number of target permanents" {
        return Some((
            BodyTarget::SINGLE_PERMANENT,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::double_counters_on_query(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                None,
            ),
        ));
    }
    if line == "proliferate" {
        return Some((
            NoTarget,
            Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::proliferate(),
            )),
        ));
    }
    if line == "goad all creatures you don't control" {
        return Some((
            NoTarget,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::goad_other_players_creatures(),
        ));
    }
    if let Some(subject) = line.strip_prefix("goad target ") {
        let target = match subject {
            "creature" => BodyTarget::SINGLE_CREATURE,
            "creature an opponent controls" => {
                BodyTarget::filtered_permanent(ObjectFilter::creature().opposing())
            }
            "creature defending player controls" | "creature that player controls" => {
                BodyTarget::filtered_permanent(ObjectFilter::creature().of_related_player())
            }
            _ => return None,
        };
        return Some((
            target,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::goad_target(),
        ));
    }
    if line == "take an extra turn after this one" {
        return Some((
            NoTarget,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::take_extra_turn(
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
            ),
        ));
    }
    if let Some(damage) = line.strip_prefix("prevent all ") {
        use spellwake_engine::damage::{
            PreventionRecipientSpec, PreventionSourceFilter, PreventionSourceSpec,
            TurnPreventionSpec,
        };
        let spec = match damage {
            "combat damage that would be dealt this turn" => TurnPreventionSpec {
                combat_only: true,
                source: PreventionSourceSpec::Any,
                recipient: PreventionRecipientSpec::Any,
                both_directions: false,
            },
            "damage that would be dealt this turn by creatures your opponents control" => {
                TurnPreventionSpec {
                    combat_only: false,
                    source: PreventionSourceSpec::Filter(PreventionSourceFilter {
                        creatures_only: true,
                        controller: spellwake_engine::program::ControllerFilter::Opponent,
                        ..Default::default()
                    }),
                    recipient: PreventionRecipientSpec::Any,
                    both_directions: false,
                }
            }
            _ => return None,
        };
        return Some((
            NoTarget,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(spec),
        ));
    }
    if line == "you may choose new targets for target spell or ability" {
        return Some((
            BodyTarget::SINGLE_STACK_ENTRY,
            Instruction::StackOp(Box::new(spellwake_engine::toolbox::StackOpSpec::retarget(
                false,
            ))),
        ));
    }
    if let Some((objects, destination)) = line
        .strip_prefix("destroy all ")
        .map(|objects| (objects, None))
        .or_else(|| {
            line.strip_prefix("return all ")?
                .strip_suffix(" to their owners' hands")
                .map(|objects| {
                    (
                        objects,
                        Some(spellwake_engine::program::PublicZoneDestination::Hand),
                    )
                })
        })
        .or_else(|| {
            line.strip_prefix("exile all ").map(|objects| {
                (
                    objects,
                    Some(spellwake_engine::program::PublicZoneDestination::Exile),
                )
            })
        })
    {
        let group = match objects {
            "creatures" => ObjectFilter::creature(),
            "artifacts and enchantments" => ObjectFilter::artifact_or_enchantment(),
            "artifacts, creatures, and enchantments" => ObjectFilter::any_types(
                CardTypeMask::ARTIFACT
                    .union(CardTypeMask::CREATURE)
                    .union(CardTypeMask::ENCHANTMENT),
            ),
            "nonland permanents that aren't legendary" => ObjectFilter::nonland().nonlegendary(),
            "nonland permanents" => ObjectFilter::nonland(),
            "nonenchantment creatures" => ObjectFilter::creature().nonenchantment(),
            "creatures with power 4 or greater" => {
                ObjectFilter::creature().with_power(PowerFilter::AtLeast(4))
            }
            _ => return None,
        };
        let group = Box::new(group);
        return Some((
            NoTarget,
            destination.map_or(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                    *(group.clone()),
                    true,
                ),
                |destination| {
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::move_group(
                        *(group),
                        destination,
                    )
                },
            ),
        ));
    }
    if let Some(selection) = player_selection(line) {
        return Some(selection);
    }
    if let Some(group) = group_modifier(line) {
        return Some((NoTarget, group));
    }
    if let Some(what) = line
        .strip_prefix("return target ")
        .and_then(|rest| rest.strip_suffix(" from your graveyard to your hand"))
    {
        let target = match what {
            "card" => BodyTarget::SINGLE_OWN_GRAVEYARD_CARD,
            "creature card" => BodyTarget::filtered_own_graveyard(ObjectFilter::creature()),
            "creature or land card" => {
                BodyTarget::filtered_own_graveyard(ObjectFilter::creature_or_land())
            }
            "artifact or enchantment card" => {
                BodyTarget::filtered_own_graveyard(ObjectFilter::artifact_or_enchantment())
            }
            "instant or sorcery card" => {
                BodyTarget::filtered_own_graveyard(ObjectFilter::instant_or_sorcery())
            }
            "enchantment card" => BodyTarget::filtered_own_graveyard(ObjectFilter::enchantment()),
            "land card" => BodyTarget::filtered_own_graveyard(ObjectFilter::land()),
            "permanent card" => BodyTarget::filtered_own_graveyard(ObjectFilter::permanent()),
            _ => return None,
        };
        return Some((
            target,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand(),
        ));
    }
    if let Some(what) = line.strip_prefix("return target ").and_then(|rest| {
        rest.strip_suffix(" from your graveyard to the battlefield")
            .map(|what| (what, false))
            .or_else(|| {
                rest.strip_suffix(" from your graveyard to the battlefield tapped")
                    .map(|what| (what, true))
            })
    }) {
        let (what, tapped) = what;
        let target = match what {
            "creature card" => ObjectFilter::creature(),
            "legendary creature card" => ObjectFilter::creature().legendary(),
            "artifact card" => ObjectFilter::artifact(),
            "enchantment card" => ObjectFilter::enchantment(),
            "land card" => ObjectFilter::land(),
            "planeswalker card" => ObjectFilter::planeswalker(),
            "creature or land card" => ObjectFilter::creature_or_land(),
            "creature card with mana value 3 or less" => {
                ObjectFilter::creature().with_mana_value(ManaValueFilter::AtMost(3))
            }
            "permanent card with mana value 3 or less" => {
                ObjectFilter::permanent().with_mana_value(ManaValueFilter::AtMost(3))
            }
            "permanent card" => ObjectFilter::permanent(),
            _ => return None,
        };
        let op = if !tapped && target == ObjectFilter::creature() {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(
                false,
            )
        } else {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(
                tapped,
            )
        };
        return Some((BodyTarget::filtered_own_graveyard(target), op));
    }
    if let Some(target) = line.strip_prefix("exile target ").and_then(filtered_target) {
        return Some((
            target,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target(),
        ));
    }
    if let Some(target) = line
        .strip_prefix("destroy target ")
        .and_then(filtered_target)
    {
        return Some((
            target,
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
        ));
    }
    if let Some(target) = line
        .strip_prefix("return target ")
        .and_then(|s| s.strip_suffix(" to its owner's hand"))
        .and_then(filtered_target)
    {
        return Some((
            target,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand(),
        ));
    }
    if let Some(keyword) = line
        .strip_prefix("target creature gains ")
        .and_then(|s| s.strip_suffix(" until end of turn"))
    {
        use spellwake_engine::characteristics::{
            AbilityChange, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        };
        return Some((
            BodyTarget::SINGLE_CREATURE,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                    AbilityChange::grant_keywords([super::super::static_keyword(keyword)?]),
                )]),
                ResolvedDuration::UntilEndOfTurn,
            ),
        ));
    }
    if let Some(change) = line.strip_prefix("target creature gets ") {
        let stats = change.strip_suffix(" until end of turn")?;
        let (stats, keyword) = match stats.split_once(" and gains ") {
            Some((stats, keyword)) => (stats, Some(super::super::static_keyword(keyword)?)),
            None => (stats, None),
        };
        let (power, toughness) = stats.split_once('/')?;
        return Some((
            BodyTarget::SINGLE_CREATURE,
            pump(signed_stat(power)?, signed_stat(toughness)?, keyword),
        ));
    }
    if let Some(rest) = line
        .strip_prefix("cardname deals ")
        .or_else(|| line.strip_prefix("this creature deals "))
        .or_else(|| line.strip_prefix("this artifact deals "))
        .or_else(|| line.strip_prefix("this enchantment deals "))
    {
        let (amount, recipient) = rest.split_once(" damage to ")?;
        use spellwake_engine::program::DamageGroup;
        let group = match recipient {
            "each opponent" => Some(DamageGroup::Opponents),
            "each player" => Some(DamageGroup::Players),
            "each creature" => Some(DamageGroup::Creatures),
            "each creature and each player" => Some(DamageGroup::CreaturesAndPlayers),
            "each creature and each planeswalker" => Some(DamageGroup::CreaturesAndPlaneswalkers),
            _ => None,
        };
        if let Some(group) = group {
            return Some((
                NoTarget,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                        spellwake_engine::program::DamageRecipientsRef::Group(group),
                    )),
                    spellwake_engine::program::ValueExpr::literal(number(amount)?),
                    false,
                ),
            ));
        }
        let target = match recipient {
            "any target" => BodyTarget::SINGLE_ANY_TARGET,
            "target player or planeswalker" => BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER,
            "target creature" => BodyTarget::SINGLE_CREATURE,
            "target player" => BodyTarget::SINGLE_PLAYER,
            _ => return None,
        };
        return Some((
            target,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                spellwake_engine::toolbox::DamageTargetQuery::Targets,
                spellwake_engine::program::ValueExpr::literal(number(amount)?),
                false,
            ),
        ));
    }
    if line == "put a +1/+1 counter on this creature" {
        return Some((
            NoTarget,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                spellwake_engine::effects::CounterKind::PlusOne,
                spellwake_engine::program::ValueExpr::literal(1),
            ),
        ));
    }
    if let Some(rest) = line.strip_prefix("draw a card for each ") {
        let amount = match rest {
            "creature you control with a +1/+1 counter on it" => {
                spellwake_engine::program::ValueExpr::count_battlefield(
                    spellwake_engine::program::ObjectFilter::creature()
                        .controlled()
                        .with_flags(
                            spellwake_engine::program::ObjectFilterFlags::WITH_PLUS_ONE_COUNTER,
                        ),
                )
            }
            "green creature you control" => {
                spellwake_engine::program::ValueExpr::count_battlefield(
                    spellwake_engine::program::ObjectFilter::creature()
                        .controlled()
                        .with_color(spellwake_engine::program::ObjectColorFilter::has(
                            spellwake_engine::characteristics::Color::Green,
                        )),
                )
            }
            "creature you control" => {
                spellwake_engine::program::ValueExpr::controlled_type(CardType::Creature)
            }
            "artifact you control" => {
                spellwake_engine::program::ValueExpr::controlled_type(CardType::Artifact)
            }
            "enchantment you control" => {
                spellwake_engine::program::ValueExpr::controlled_type(CardType::Enchantment)
            }
            _ => return None,
        };
        return Some((
            NoTarget,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(amount),
        ));
    }
    if let Some(rest) = line
        .strip_prefix("target player draws ")
        .or_else(|| line.strip_prefix("each player draws "))
    {
        let amount = spellwake_engine::program::ValueExpr::Literal(
            number(
                rest.strip_suffix(" cards")
                    .or_else(|| rest.strip_suffix(" card"))?,
            )?
            .into(),
        );
        return Some(if line.starts_with("each") {
            (
                NoTarget,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_each_player(amount),
            )
        } else {
            (
                BodyTarget::SINGLE_PLAYER,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_target_player(amount),
            )
        });
    }
    if let Some(rest) = line.strip_prefix("discard ") {
        let count = number(
            rest.strip_suffix(" cards")
                .or_else(|| rest.strip_suffix(" card"))?,
        )?;
        return Some((
            NoTarget,
            Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                spellwake_engine::program::AffectedPlayers::One(ResolvingPlayer::Controller),
                spellwake_engine::program::ValueExpr::Literal(count.into()),
            ))),
        ));
    }
    if let Some(rest) = line
        .strip_prefix("draw ")
        .or_else(|| line.strip_prefix("you draw "))
    {
        let count = rest
            .strip_suffix(" cards")
            .or_else(|| rest.strip_suffix(" card"))?;
        return Some((NoTarget, draw(number(count)?)));
    }
    if let Some(rest) = line.strip_prefix("you gain ") {
        return controller_life(rest, true);
    }
    if let Some(rest) = line.strip_prefix("you lose ") {
        return controller_life(rest, false);
    }
    if let Some(rest) = line.strip_prefix("target player loses ") {
        return Some((
            BodyTarget::SINGLE_PLAYER,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer),
                spellwake_engine::program::ValueExpr::literal(number(rest.strip_suffix(" life")?)?),
            ),
        ));
    }
    let amount = |value: &str| {
        Some(spellwake_engine::program::ValueExpr::Literal(
            number(value)?.into(),
        ))
    };
    let controller = ResolvingPlayer::Controller;
    Some(match line {
        "counter target noncreature spell" => (
            BodyTarget::single_spell(spellwake_engine::program::SpellFilter::noncreature()),
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                ),
            )),
        ),
        "counter target sorcery spell" => (
            BodyTarget::single_spell(spellwake_engine::program::SpellFilter::of_type(
                spellwake_engine::program::CardType::Sorcery,
            )),
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                ),
            )),
        ),
        "counter target creature spell" => (
            BodyTarget::single_spell(spellwake_engine::program::SpellFilter::creature()),
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                ),
            )),
        ),
        "counter target spell" => (
            BodyTarget::single_spell(spellwake_engine::program::SpellFilter::any()),
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                ),
            )),
        ),
        "destroy target creature" => (
            BodyTarget::SINGLE_CREATURE,
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
        ),
        "destroy target permanent" => (
            BodyTarget::SINGLE_PERMANENT,
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
        ),
        "return target creature to its owner's hand" => (
            BodyTarget::SINGLE_CREATURE,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand(),
        ),
        "return target permanent to its owner's hand" => (
            BodyTarget::SINGLE_PERMANENT,
            crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand(),
        ),
        _ if line.starts_with("scry ") => (
            NoTarget,
            Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::scry(
                controller,
                amount(line.strip_prefix("scry ")?)?,
            ))),
        ),
        _ if line.starts_with("surveil ") => (
            NoTarget,
            Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::surveil(
                    controller,
                    amount(line.strip_prefix("surveil ")?)?,
                ),
            )),
        ),
        _ => return None,
    })
}

/// Create blockers based on attacks against the caster and install the printed prevention.
pub(super) fn combat_defense(
    text: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ExecutableBody> {
    let (template, excluded) = text.strip_prefix("Create X ")?.split_once(
        ", where X is the number of creatures attacking you. Prevent all combat damage that would be dealt this turn by non-"
    )?;
    let subtype = vocabulary.resolve_label(excluded.strip_suffix(" creatures.")?)?;
    let Instruction::CreateTokens { mut creations } =
        ordinary_text::create_tokens(&format!("one {template}"), vocabulary)?
    else {
        return None;
    };
    let [creation] = creations.as_mut_slice() else {
        return None;
    };
    creation.amount = spellwake_engine::program::ValueExpr::CountAttackingController;
    Some(ExecutableBody::new(
        BodyTarget::None,
        vec![
            Instruction::CreateTokens { creations },
            crate::keyword_creatures::oracle_lowering::toolbox_emit::prevent_damage_spec(
                spellwake_engine::damage::TurnPreventionSpec {
                    combat_only: true,
                    source: spellwake_engine::damage::PreventionSourceSpec::Filter(
                        spellwake_engine::damage::PreventionSourceFilter {
                            creatures_only: true,
                            forbidden_subtypes: [Some(subtype), None],
                            ..Default::default()
                        },
                    ),
                    recipient: spellwake_engine::damage::PreventionRecipientSpec::Any,
                    both_directions: false,
                },
            ),
        ],
    ))
}

/// Resolve a mass destruction effect, with additional tokens only at threshold.
pub(super) fn threshold_destruction(
    text: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ExecutableBody> {
    let replacement = text.trim()
        .strip_prefix("Destroy all creatures. They can't be regenerated.")?
        .trim_start()
        .strip_prefix("Threshold — If there are seven or more cards in your graveyard, instead destroy all creatures, then create ")?
        .strip_suffix(". Creatures destroyed this way can't be regenerated.")?;
    let destroy = crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
        ObjectFilter::creature(),
        false,
    );
    Some(ExecutableBody::new(
        BodyTarget::None,
        vec![
            Instruction::BranchCondition {
                condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                    Box::new(ResolutionCondition {
                        left: spellwake_engine::program::ValueExpr::count_graveyard(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                            spellwake_engine::program::ObjectFilter::any(),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: spellwake_engine::program::ValueExpr::Literal(7.into()),
                    }),
                )),
                destination: 4,
            },
            destroy.clone(),
            super::super::ordinary_text::create_tokens(replacement, vocabulary)?,
            Instruction::Jump { destination: 5 },
            destroy,
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_independent_effects_without_narrowing_targets_or_dropping_conditions() {
        let body = body(
            "Target creature gets -2/+1 until end of turn. Scry 1. Draw a card.",
            "Example",
        )
        .unwrap();
        assert_eq!(body.target, BodyTarget::SINGLE_CREATURE);
        assert_eq!(
            body.instructions,
            vec![
                pump(-2, 1, None),
                Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::scry(
                        ResolvingPlayer::Controller,
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                )),
                draw(1)
            ]
        );
        let damage = super::body("Example deals 3 damage to any target.", "Example").unwrap();
        assert_eq!(damage.target, BodyTarget::SINGLE_ANY_TARGET);
        assert_eq!(
            damage.instructions,
            [
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::literal(3),
                    false,
                )
            ]
        );
        for text in [
            "Target creature gets +2/+2 until end of turn. Destroy target creature.",
            "Target creature you control gets +2/+2 until end of turn.",
            "Target creature gets +2/+2 until end of turn. If it dies, draw a card.",
            "Example deals 3 damage to any target that was dealt damage this turn.",
        ] {
            assert!(super::body(text, "Example").is_none(), "{text}");
        }
    }
}
