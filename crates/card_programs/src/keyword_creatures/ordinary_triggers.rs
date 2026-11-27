//! Complete ordinary permanents with self-entry, death and departure triggers.
use super::{
    ordinary_text::{amount, create_tokens},
    *,
};
use spellwake_engine::program::{
    AffectedPlayers, BodyTarget, EffectSubject, ExecutableBody, Instruction, ManaValueFilter,
    ObjectFilter, PermanentTypeFilter, PlayerRelation, ResolvingPlayer, TriggerGrouping,
    TriggerSpec, ValueExpr,
};

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    if card.layout != "normal" || card.name != face.name {
        return None;
    }
    let types = face.type_line.as_deref()?.split(" — ").next()?;
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut triggers = Vec::new();
    let mut once_each_turn = BTreeSet::new();
    let mut other = Vec::new();
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .peekable();
    if let Some((kicker, remaining)) = text.trim().split_once('\n')
        && let Some(cost) = kicker.trim().strip_prefix("Kicker ")
        && let Some((counters, target_trigger)) = remaining.split_once('\n')
        && counters.trim() == "If this creature was kicked, it enters with a +1/+1 counter on it for each nonbasic land your opponents control."
        && let Some(template) = target_trigger.strip_prefix("When this creature becomes the target of a spell, sacrifice it and create a number of ")
            .and_then(|text| text.strip_suffix(" tokens equal to its power."))
    {
        use spellwake_engine::program::{AdditionalCastCost, EntryCounterAbility};
        let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
        program.additional_cast_costs.push(AdditionalCastCost::Kicker(fixed_cost(cost)?));
        program.entry_counter_abilities.push(EntryCounterAbility::KickedOpposingNonbasicLands(0));
        let Instruction::CreateTokens { mut creations } = create_tokens(&format!("a {template} token"), vocabulary)? else { return None; };
        let [creation] = creations.as_mut_slice() else { return None; };
        creation.amount = spellwake_engine::program::ValueExpr::object_stat(spellwake_engine::program::ValueObjectRef::TriggerSource, spellwake_engine::program::ObjectStatKind::Power);
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Targeted { subject: spellwake_engine::program::ObjectFilter::self_object(), by_opponent: false, spell_only: true, instant_or_sorcery_only: false }),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, vec![
                Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::sacrifice_state_trigger_source(),
                )),
                Instruction::CreateTokens { creations },
            ]),
        });
        return Some(program);
    }
    if text.trim()
        == "Flying\nAs this creature enters, choose two players.\nWhenever damage is dealt to one of the chosen players, the other chosen player also loses that much life."
    {
        let mut program =
            ordinary_creature(card, vocabulary, Profile::Current, Some("Flying")).ok()?;
        let spellwake_engine::program::ProgramKind::Permanent(definition) = &mut program.kind
        else {
            return None;
        };
        definition.entry_tap = Some(spellwake_engine::land::EntryTap::ChooseTwoPlayers);
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::new(
                spellwake_engine::program::TriggerEvent::DealtDamage {
                    subject: spellwake_engine::program::ObjectFilter::any(),
                    linked_player: true,
                },
            ),
            grouping: TriggerGrouping::OneOrMoreByPlayer,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::UniqueRelatedPlayer,
                        ),
                        spellwake_engine::program::ValueExpr::event(
                            spellwake_engine::program::EventValueKind::DamageReceived,
                        ),
                    ),
                ],
            ),
        });
        return Some(program);
    }
    while let Some(line) = lines.next() {
        let line = line
            .strip_prefix("Undergrowth — ")
            .or_else(|| line.strip_prefix("Constellation — "))
            .unwrap_or(line);
        let modal = trigger(line, &card.name, types).and_then(|(condition, effect)| {
            let (minimum, maximum, distinct_targets) = match effect {
                "choose one —" => (1, Some(1), false),
                "choose any number —" => (0, None, false),
                "choose one or more. Each mode must target a different player." => (1, None, true),
                _ => return None,
            };
            Some((condition, minimum, maximum, distinct_targets))
        });
        if line=="Myriad" {
            triggers.push((spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::self_object(), spellwake_engine::program::AttackTriggerMode::Individual { capture_defender: true }),ExecutableBody::new(BodyTarget::None,vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::myriad())),Instruction::ScheduleDelayedStepTrigger { step: spellwake_engine::turn::Step::EndCombat, own_step_only: false, body: Box::new(ExecutableBody::new(BodyTarget::None, vec![Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(spellwake_engine::toolbox::ObjectQuery::CreatedTokens, spellwake_engine::toolbox::ZoneDest::exile())))])) }])));
        } else if line == "Whenever you cast a planeswalker spell, you may return this card from your graveyard to the battlefield." {
            triggers.push((spellwake_engine::program::TriggerSpec::spell_cast(spellwake_engine::program::PlayerRelation::Controller, spellwake_engine::program::SpellFilter::of_type(spellwake_engine::program::CardType::Planeswalker)).in_zone(spellwake_engine::program::TriggerZone::Graveyard),ExecutableBody::new(BodyTarget::None,vec![
                Instruction::OptionalSequence { chooser:ResolvingPlayer::Controller, instructions: 1 },crate::keyword_creatures::oracle_lowering::toolbox_emit::return_source_graveyard_to_battlefield(false),
            ])));
        } else if line == "Whenever the final chapter ability of a Saga you control resolves, each opponent loses X life and you gain X life, where X is that Saga's mana value." {
            triggers.push((
                spellwake_engine::program::TriggerSpec::player_action(
                    spellwake_engine::program::PlayerRelation::Controller,
                    spellwake_engine::program::PlayerTriggerAction::SagaFinalChapterResolved,
                ),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::drain_life(
                            spellwake_engine::toolbox::PlayerSelector::Group(
                                AffectedPlayers::EachOpponent,
                            ),
                            spellwake_engine::program::ValueExpr::object_stat(
                                spellwake_engine::program::ValueObjectRef::FinalSaga,
                                spellwake_engine::program::ObjectStatKind::ManaValue,
                            ),
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            false,
                        ),
                    ],
                ),
            ));
        } else if line == "Whenever a planeswalker you control enters, if this artifact has loyalty counters on it, you may move a loyalty counter from this artifact onto that planeswalker." {
            triggers.push((spellwake_engine::program::TriggerSpec::enters(spellwake_engine::program::ObjectFilter::any_types(spellwake_engine::program::CardTypeMask::from_card_type(CardType::Planeswalker)).controlled()).only_if(spellwake_engine::program::TriggerCondition::SourceHasLoyalty),
                ExecutableBody::new(BodyTarget::None, vec![
                    Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::require_condition(spellwake_engine::program::InterveningCondition::SourceLoyalty))),
                    Instruction::OptionalSequence { chooser: ResolvingPlayer::Controller, instructions: 1 },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::move_counters(
                        spellwake_engine::toolbox::ObjectQuery::Source,
                        spellwake_engine::toolbox::ObjectQuery::TriggerSubject,
                        Some(spellwake_engine::effects::CounterKind::Loyalty),
                        Some(spellwake_engine::program::ValueExpr::literal(1)),
                    ),
                ])));
        } else if let Some((quantity, subject)) = line.strip_prefix("When you control ").and_then(|text| text.split_once(" or more "))
            && let Some((kind, reference)) = subject.split_once(", sacrifice ")
            && let Some(reference) = reference.strip_suffix('.')
            && (reference == face.name || reference == "this creature" || Some(reference) == face.name.split(',').next())
        {
            let subtype = vocabulary.resolve_label(kind.strip_suffix('s')?)?;
            let minimum = u16::try_from(amount(quantity)?).ok()?;
            triggers.push((spellwake_engine::program::TriggerSpec::state_threshold(spellwake_engine::program::StateThresholdKind::ControlledSubtype { subtype, minimum }),
                ExecutableBody::new(BodyTarget::None, vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec::sacrifice_state_trigger_source()))])));
        } else if let Some(subtype) = line.strip_prefix("Whenever a ")
            .or_else(|| line.strip_prefix("Whenever an "))
            .and_then(|text| text.strip_suffix(" deals damage to you, destroy it."))
        {
            triggers.push((spellwake_engine::program::TriggerSpec::deals_damage(spellwake_engine::program::ObjectFilter::any().with_subtype(vocabulary.resolve_label(subtype)?), spellwake_engine::program::DamageTriggerRecipient::Controller, false),
                ExecutableBody::new(BodyTarget::None, vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_subject(spellwake_engine::program::EffectSubject::TriggerSubject, false)])));
        } else if let Some((subtype, effect)) = line.strip_prefix("Whenever a nontoken ")
            .and_then(|text| text.split_once(" you control dies, "))
            && let Some(subtype) = vocabulary.resolve_label(subtype)
        {
            triggers.push((spellwake_engine::program::TriggerSpec::dies(spellwake_engine::program::ObjectFilter::creature().controlled().nontoken().with_subtype(subtype)),
                body(effect, &card.name, vocabulary)?));
        } else if let Some((subtype, threshold)) = line.strip_prefix("Whenever a ")
            .and_then(|text| text.strip_suffix(" or greater attacks, it gains lifelink until end of turn."))
            .and_then(|text| text.split_once(" token you control with power "))
        {
            let subtype = vocabulary.resolve_label(subtype)?;
            let minimum_power = threshold.parse::<u16>().ok()?;
            triggers.push((spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::creature().controlled().token().with_subtype(subtype).with_power(spellwake_engine::program::PowerFilter::AtLeast((minimum_power) as i16)), spellwake_engine::program::AttackTriggerMode::Individual { capture_defender: false }),
                ExecutableBody::new(BodyTarget::None, vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                    EffectSubject::TriggeringAttacker,
                    spellwake_engine::program::ValueExpr::literal(0),
                    spellwake_engine::program::ValueExpr::literal(0),
                    [KeywordAbility::Lifelink].into(),
                    BTreeSet::new(),
                )])));
        } else if let Some((threshold,consequence))=line.strip_prefix("At the beginning of each end step, if an opponent lost ").and_then(|text|text.split_once(" or more life this turn, "))
            && consequence=="you may put a quest counter on this enchantment." {
            let n=threshold.parse::<u8>().ok().filter(|v|*v>0)?;
            let mut effect=body(consequence,&card.name,vocabulary)?;
            effect.instructions.insert(0,Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::require_condition(spellwake_engine::program::InterveningCondition::OpponentLifeLoss(n)))));
            triggers.push((spellwake_engine::program::TriggerSpec::step_began(spellwake_engine::turn::Step::End, spellwake_engine::program::PlayerRelation::Controller).only_if(spellwake_engine::program::TriggerCondition::OpponentLostLifeAtLeast(n)),effect));
        } else if let Some((threshold,consequence))=line.strip_prefix("Whenever a card is put into an opponent's graveyard from anywhere, if this enchantment has ")
            .and_then(|text|text.split_once(" or more quest counters on it, you may have that player lose ")) {
            let threshold=u8::try_from(amount(threshold)?).ok().filter(|n|*n>0)?;
            let (lose,gain)=consequence.split_once(" life. If you do, you gain ")?;
            let lose=lose.parse::<u8>().ok().filter(|n|*n>0)?;
            let gain=gain.strip_suffix(" life.")?.parse::<u8>().ok().filter(|n|*n>0)?;
            triggers.push((spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::PutIntoGraveyard { subject: spellwake_engine::program::ObjectFilter::any(), player: spellwake_engine::program::PlayerRelation::Opponent, from_library: false, own_turn_only: false }).only_if(spellwake_engine::program::TriggerCondition::SourceCounterAtLeast { kind: spellwake_engine::effects::CounterKind::Quest, minimum: threshold }),ExecutableBody::new(BodyTarget::None,vec![
                Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::require_condition(spellwake_engine::program::InterveningCondition::SourceQuestCounters(threshold)))),Instruction::OptionalSequence {chooser:ResolvingPlayer::Controller,instructions:2},
                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::UniqueRelatedPlayer,
                    ),
                    spellwake_engine::program::ValueExpr::literal(i64::from(lose)),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::literal(i64::from(gain)),
                ),
            ])));
        } else if let Some((minimum,amount))=line.strip_prefix("When there are ").and_then(|text|text.split_once(" or more page counters on this artifact, exile it. If you do, you gain ")) {
            let minimum=u8::try_from(super::ordinary_text::amount(minimum)?).ok().filter(|value|*value>0)?;
            let amount=amount.strip_suffix(" life.")?.parse::<u8>().ok().filter(|value|*value>0)?;
            triggers.push((
                spellwake_engine::program::TriggerSpec::state_threshold(
                    spellwake_engine::program::StateThresholdKind::SourceCounters {
                        kind: spellwake_engine::effects::CounterKind::Page,
                        comparison: spellwake_engine::program::Comparison::GreaterOrEqual,
                        amount: minimum,
                    },
                ),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec {
                                source: spellwake_engine::toolbox::ObjectQuery::Source,
                                selection: spellwake_engine::toolbox::ZoneSelection::All,
                                destination: spellwake_engine::toolbox::ZoneDest::exile(),
                                bind_moved: None,
                                bind_snapshots: None,
                                bind_unchosen: None,
                                bind_succeeded: Some(spellwake_engine::toolbox::BindingId(1)),
                                tag_moved: None,
                                shuffle_library: None,
                            },
                        ),
                        Instruction::BranchCondition {
                            condition: Box::new(spellwake_engine::toolbox::ConditionExpr::BoundBool(
                                spellwake_engine::toolbox::BindingId(1),
                            )),
                            destination: 3,
                        },
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            spellwake_engine::program::ValueExpr::literal(i64::from(amount)),
                        ),
                    ],
                ),
            ));
        } else if line == "Whenever an opponent sacrifices a nontoken permanent, put that card onto the battlefield under your control." {
            triggers.push((spellwake_engine::program::TriggerSpec::player_action(PlayerRelation::Opponent, spellwake_engine::program::PlayerTriggerAction::Sacrificed(spellwake_engine::program::ObjectFilter::any().nontoken())),
                ExecutableBody::new(BodyTarget::None, vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Intersect(
                                Box::new(spellwake_engine::toolbox::ObjectQuery::TriggerSubject),
                                Box::new(spellwake_engine::toolbox::ObjectQuery::Union(vec![
                                    spellwake_engine::toolbox::ObjectQuery::zone(
                                        spellwake_engine::toolbox::QueryZone::Graveyard(Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Group(
                                                spellwake_engine::program::AffectedPlayers::EachPlayer,
                                            ),
                                        )),
                                        spellwake_engine::program::ObjectFilter::permanent(),
                                    ),
                                    spellwake_engine::toolbox::ObjectQuery::zone(
                                        spellwake_engine::toolbox::QueryZone::Exile,
                                        spellwake_engine::program::ObjectFilter::permanent(),
                                    ),
                                    spellwake_engine::toolbox::ObjectQuery::zone(
                                        spellwake_engine::toolbox::QueryZone::Command,
                                        spellwake_engine::program::ObjectFilter::permanent(),
                                    ),
                                ])),
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
                ])));
        } else if let Some(token) = line.strip_prefix("Whenever one or more creature cards are put into your graveyard from anywhere during your turn, create ")
            .and_then(|effect| effect.strip_suffix(". This ability triggers only once each turn."))
        {
            once_each_turn.insert(triggers.len());
            triggers.push((spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::PutIntoGraveyard { subject: spellwake_engine::program::ObjectFilter::creature(), player: spellwake_engine::program::PlayerRelation::Controller, from_library: false, own_turn_only: true }),
                ExecutableBody::new(BodyTarget::None, vec![create_tokens(token, vocabulary)?])));
        } else if let Some(effect) = line.strip_prefix("Whenever one or more artifact creatures you control enter, ")
            .and_then(|text| text.strip_suffix(" This ability triggers only once each turn."))
        {
            once_each_turn.insert(triggers.len());
            triggers.push((spellwake_engine::program::TriggerSpec::enters(spellwake_engine::program::ObjectFilter::creature().with_required_type(CardType::Artifact).controlled()),
                body(effect, &card.name, vocabulary)?));
        } else if let Some((condition, minimum, maximum, distinct_targets)) = modal {
            let mut modes = Vec::new();
            while lines.peek().is_some_and(|line| line.starts_with("• ")) {
                let effect = lines.next()?.strip_prefix("• ")?;
                modes.push(modal_body(effect, &card.name, vocabulary)?);
            }
            let maximum = maximum.or_else(|| u8::try_from(modes.len()).ok())?;
            if modes.len() < 2 {
                return None;
            }
            triggers.push((condition, ExecutableBody::with_triggered_modes(spellwake_engine::program::TriggeredModes {
                minimum, maximum, distinct_targets, allow_repeat: false, uniqueness: spellwake_engine::program::ModalUniqueness::None, prefix: None, suffix: None, options: modes,
            })));
        } else if let Some(effect) = line.strip_prefix("Magecraft — Whenever you cast or copy an instant or sorcery spell, ") {
            triggers.push((spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::SpellCast { caster: spellwake_engine::program::TriggerPlayerScope::Relation(spellwake_engine::program::PlayerRelation::Controller), filter: spellwake_engine::program::SpellFilter::instant_or_sorcery(), self_cast: false, include_copy: true, min_mana_value: None }), body(effect, &card.name, vocabulary)?));
        } else if let Some((threshold, effect)) = line.strip_prefix("Formidable — At the beginning of combat on your turn, if creatures you control have total power ")
            .and_then(|text| text.split_once(" or greater, "))
        {
            use spellwake_engine::characteristics::{AbilityChange, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec};
            use spellwake_engine::program::{Comparison, ResolutionCondition};
            let minimum = threshold.parse::<u16>().ok().filter(|value| *value > 0)?;
            let keyword = static_keyword(effect.strip_prefix("target creature you control gains ")?.strip_suffix(" until end of turn.")?)?;
            triggers.push((spellwake_engine::program::TriggerSpec::step_began(spellwake_engine::turn::Step::BeginningCombat, spellwake_engine::program::PlayerRelation::Controller).only_if(spellwake_engine::program::TriggerCondition::TotalControlledPowerAtLeast(minimum)),
                ExecutableBody::new(BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled()), vec![
                    Instruction::BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(ResolutionCondition {
                        left: spellwake_engine::program::ValueExpr::aggregate(spellwake_engine::program::ObjectFilter::creature().controlled(), spellwake_engine::program::ObjectStatKind::Power, spellwake_engine::program::StatAggregate::Sum),
                        comparison: Comparison::GreaterOrEqual,
                        right: spellwake_engine::program::ValueExpr::Literal(i64::from(minimum).into()),
                    }))), destination: 2 },
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([keyword]))]),
                        ResolvedDuration::UntilEndOfTurn,
                    ),
                ])));
        } else if let Some((kind, cost)) = line.strip_prefix("Whenever a ")
            .and_then(|rule| rule.split_once(" creature you control becomes the target of a spell or ability an opponent controls, counter that spell or ability unless its controller pays "))
        {
            let ability = TriggeredAbility::protect_controlled_subtype(
                TriggeredAbilityId(0), vocabulary.resolve_label(kind)?, fixed_cost(cost.strip_suffix('.')?)?,
            );
            triggers.push((ability.condition, ability.body));
        } else if let Some(effect)=line.strip_prefix("Whenever an opponent attacks with creatures, if two or more of those creatures ")
            .and_then(|clause|clause.strip_prefix("are attacking you and/or planeswalkers you control, ")) {
            use spellwake_engine::program::{Comparison,ResolutionCondition};
            let mut consequence=body(effect,&card.name,vocabulary)?;
            if consequence.target!=BodyTarget::None || !matches!(consequence.instructions.as_slice(),[Instruction::DrawCards(_)]) {return None;}
            consequence.instructions.insert(0,Instruction::BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(ResolutionCondition {
                left:spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::DeclaredAttackersAtController),comparison:Comparison::GreaterOrEqual,
                right:spellwake_engine::program::ValueExpr::Literal(2.into()),
            }))), destination:2 });
            triggers.push((spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::any(), spellwake_engine::program::AttackTriggerMode::OpponentsAtController { minimum: 2 }),consequence));
        } else if let Some((position,effect))=line.strip_prefix("Whenever an opponent casts their ")
            .and_then(|clause|clause.split_once(" spell each turn, ")) {
            let ordinal=match position {"first"=>1,"second"=>2,"third"=>3,"fourth"=>4,_=>return None};
            triggers.push((spellwake_engine::program::TriggerSpec::spell_cast(PlayerRelation::Opponent, spellwake_engine::program::SpellFilter::any().ordinal_this_turn(std::num::NonZeroU32::from(std::num::NonZeroU16::new(ordinal)?))),body(effect,&card.name,vocabulary)?));
        } else if let Some(keywords)=line.strip_prefix("Whenever one or more creatures attack one of your opponents ")
            .and_then(|rule|rule.strip_prefix("or a planeswalker they control, those creatures gain "))
            .and_then(|rule|rule.strip_suffix(" until end of turn.")) {
            let keywords=keywords.split(" and ").map(static_keyword).collect::<Option<BTreeSet<_>>>()?;
            triggers.push((spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::any(), spellwake_engine::program::AttackTriggerMode::AtOpponentOrPlaneswalker),ExecutableBody::new(BodyTarget::None,
                vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(EffectSubject::TriggeringAttackGroup,spellwake_engine::program::ValueExpr::literal(0),spellwake_engine::program::ValueExpr::literal(0),keywords,BTreeSet::new())])));
        } else if let Some((ordinal,effect))=line.strip_prefix("Whenever you cast your ").and_then(|clause|clause.split_once(" spell each turn, ")) {
            let ordinal=match ordinal {"first"=>1,"second"=>2,"third"=>3,"fourth"=>4,_=>return None};
            let effect=effect.strip_prefix("put a +1/+1 counter on ")?;
            let (subject,damage)=effect.split_once(", then it deals damage equal to its power to ")?;
            if (subject!=card.name && Some(subject)!=card.name.split(',').next() && subject!="this creature") || damage!="any target." {return None;}
            triggers.push((spellwake_engine::program::TriggerSpec::spell_cast(PlayerRelation::Controller, spellwake_engine::program::SpellFilter::any().ordinal_this_turn(std::num::NonZeroU32::from(std::num::NonZeroU16::new(ordinal)?))),
                ExecutableBody::new(BodyTarget::SingleAnyTarget,vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(spellwake_engine::effects::CounterKind::PlusOne,spellwake_engine::program::ValueExpr::literal(1)),
                    Instruction::DealDamage(Box::new(spellwake_engine::toolbox::DealDamageSpec::from_power(spellwake_engine::program::PowerDamageSource::AbilitySource)))])));
        } else if let Some((kind,effect)) = line.strip_prefix("Whenever one or more +1/+1 counters are put on another non-").and_then(|s|s.split_once(" creature you control, ")) {
            triggers.push((spellwake_engine::program::TriggerSpec::counters_changed(spellwake_engine::program::ObjectFilter::creature().controlled().other_than_ability_source().without_subtype(vocabulary.resolve_label(kind)?), spellwake_engine::program::CounterTriggerKind::PlusOnePlaced { by_controller_only: false }),body(effect,&card.name,vocabulary)?));
        } else if line=="Whenever a commander you control attacks, put a +1/+1 counter on it." {
            triggers.push((
                spellwake_engine::program::TriggerSpec::attacks(
                    spellwake_engine::program::ObjectFilter::any().controlled().commander(),
                    spellwake_engine::program::AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
                ),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                            spellwake_engine::toolbox::ObjectQuery::Intersect(
                                Box::new(spellwake_engine::toolbox::ObjectQuery::TriggerSubject),
                                Box::new(spellwake_engine::toolbox::ObjectQuery::zone(
                                    spellwake_engine::toolbox::QueryZone::Battlefield,
                                    spellwake_engine::program::ObjectFilter::any(),
                                )),
                            ),
                            spellwake_engine::effects::CounterKind::PlusOne,
                            spellwake_engine::program::ValueExpr::literal(1),
                        ),
                    ],
                ),
            ));
        } else if let Some((condition,words)) = [
            ("Whenever a creature you control attacks alone, it gains ",spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::any(), spellwake_engine::program::AttackTriggerMode::Alone)),
            ("Whenever another creature you control attacks, it gains ",spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::creature().controlled().other_than_ability_source(), spellwake_engine::program::AttackTriggerMode::Individual { capture_defender: false })),
        ].into_iter().find_map(|(prefix,condition)|line.strip_prefix(prefix).and_then(|s|s.strip_suffix(" until end of turn.")).map(|s|(condition,s))) {
            let keywords=words.split(" and ").map(static_keyword).collect::<Option<BTreeSet<_>>>()?;
            triggers.push((condition,ExecutableBody::new(BodyTarget::None,vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                EffectSubject::TriggeringAttacker,spellwake_engine::program::ValueExpr::literal(0),spellwake_engine::program::ValueExpr::literal(0),keywords,BTreeSet::new())])));
        } else if let Some((kind, action)) = line.strip_prefix("Whenever you cast an instant, sorcery, or ").and_then(|s|s.split_once(" spell, ")) {
            triggers.push((spellwake_engine::program::TriggerSpec::spell_cast(PlayerRelation::Controller, spellwake_engine::program::SpellFilter::instant_sorcery_or_subtype(vocabulary.resolve_label(kind)?)),
                body(action,&card.name,vocabulary)?));
        } else if line == "When this creature enters, it deals X damage to target creature an opponent controls, where X is the number of instant and sorcery cards in your graveyard." {
            triggers.push((
                spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                ExecutableBody::new(
                    BodyTarget::FilteredPermanent(ObjectFilter::creature().opposing()),
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Targets,
                            spellwake_engine::program::ValueExpr::count_graveyard(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                                spellwake_engine::program::ObjectFilter::instant_or_sorcery(),
                            ),
                            false,
                        ),
                    ],
                ),
            ));
        } else if line == "When this creature enters, it deals 1 damage to target opponent for each creature card in your graveyard." {
            triggers.push((
                spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                ExecutableBody::new(
                    BodyTarget::SingleOpponent,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                            spellwake_engine::toolbox::DamageTargetQuery::Targets,
                            spellwake_engine::program::ValueExpr::count_graveyard(
                                spellwake_engine::program::ResolvingPlayer::Controller,
                                spellwake_engine::program::ObjectFilter::creature(),
                            ),
                            false,
                        ),
                    ],
                ),
            ));
        } else if let Some(action) = line.strip_prefix("Whenever you cast an artifact spell, you may pay ") {
            let (price, action) = action.split_once(". If you do, create ")?;
            let creation = create_tokens(action.strip_suffix('.')?,vocabulary)?;
            triggers.push((spellwake_engine::program::TriggerSpec::spell_cast(PlayerRelation::Controller, spellwake_engine::program::SpellFilter::of_type(CardType::Artifact)),
                ExecutableBody::new(BodyTarget::None, vec![Instruction::PromptChoice(Box::new(
                    spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                        ResolvingPlayer::Controller,
                        fixed_cost(price)?,
                        spellwake_engine::program::ValueExpr::Literal(0.into()),
                        1,
                        2,
                    ),
                )), creation])));
        } else if let Some((subject, effect)) = line.strip_prefix("Whenever ").and_then(|s|s.split_once(" or another nontoken creature you control dies, create "))
            && (subject == face.name || Some(subject) == face.name.split(',').next()) {
            let (template, threshold) = effect.split_once(" If the creature had power ")?;
            let minimum = threshold.strip_suffix(" or greater, create two of those tokens instead.")?.parse::<u16>().ok()?;
            let Instruction::CreateTokens { mut creations } = create_tokens(template, vocabulary)? else {return None;};
            let [creation] = creations.as_mut_slice() else {return None;};
            if creation.amount != spellwake_engine::program::ValueExpr::Literal(1.into()) {return None;}
            creation.amount = spellwake_engine::program::ValueExpr::conditional(spellwake_engine::program::ValueExpr::object_stat(spellwake_engine::program::ValueObjectRef::DyingCreature, spellwake_engine::program::ObjectStatKind::Power), spellwake_engine::program::Comparison::GreaterOrEqual, spellwake_engine::program::ValueExpr::literal(i64::from(minimum)), spellwake_engine::program::ValueExpr::literal(2), spellwake_engine::program::ValueExpr::literal(1));
            triggers.push((spellwake_engine::program::TriggerSpec::dies(spellwake_engine::program::ObjectFilter::creature().controlled().nontoken().or_self()),
                ExecutableBody::new(BodyTarget::None, vec![Instruction::CreateTokens {creations}])));
        } else if let Some((kind,effect))=line.strip_prefix("Whenever a ").and_then(|s|s.split_once(" deals combat damage to a player, its controller may ")) {
            let kind=vocabulary.resolve_label(kind)?;
            let chooser=ResolvingPlayer::CombatDamageSourceController;
            let consequence = if effect == "draw a card." {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_resolving_player(
                    ResolvingPlayer::CombatDamageSourceController,
                    spellwake_engine::program::ValueExpr::literal(1),
                )
            } else {
                let Instruction::CreateTokens {mut creations}=create_tokens(effect.strip_prefix("create ")?.strip_suffix('.')?,vocabulary)? else {return None;};
                for spec in &mut creations {spec.recipients=spellwake_engine::token::TokenRecipients::CombatDamageSourceController;}
                Instruction::CreateTokens {creations}
            };
            triggers.push((spellwake_engine::program::TriggerSpec::deals_damage(spellwake_engine::program::ObjectFilter::any().with_subtype(kind), spellwake_engine::program::DamageTriggerRecipient::AnyPlayer, true),ExecutableBody::new(BodyTarget::None,vec![Instruction::OptionalSequence { chooser, instructions: 1 },consequence])));
        } else if let Some(reference)=line.strip_prefix("Whenever you put one or more +1/+1 counters on another colorless creature, you may put a +1/+1 counter on ").and_then(|s|s.strip_suffix('.')) && (reference==face.name || Some(reference)==face.name.split_once(", ").map(|(name,_)|name)) {
            triggers.push((spellwake_engine::program::TriggerSpec::counters_changed(spellwake_engine::program::ObjectFilter::creature().with_color(spellwake_engine::program::ObjectColorFilter::Colorless).other_than_ability_source(), spellwake_engine::program::CounterTriggerKind::PlusOnePlaced { by_controller_only: true }), ExecutableBody::new(BodyTarget::None,vec![Instruction::OptionalSequence { chooser:ResolvingPlayer::Controller, instructions: 1 },crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(spellwake_engine::effects::CounterKind::PlusOne,spellwake_engine::program::ValueExpr::literal(1))])));
        } else if line=="Whenever you cast a colorless creature spell, manifest the top card of your library." {
            triggers.push((spellwake_engine::program::TriggerSpec::spell_cast(PlayerRelation::Controller, spellwake_engine::program::SpellFilter::creature().with_colorless()), ExecutableBody::new(BodyTarget::None,vec![Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::manifest_top(ResolvingPlayer::Controller, spellwake_engine::program::ValueExpr::Literal(1.into()))))])));
        } else if let Some(minimum)=line.strip_prefix("Whenever you attack with creatures with total power ").and_then(|s|s.strip_suffix(" or greater, you may turn a face-down creature you control face up.")) {
            triggers.push((spellwake_engine::program::TriggerSpec::attacks(spellwake_engine::program::ObjectFilter::any(), spellwake_engine::program::AttackTriggerMode::GroupTotalPowerAtLeast(minimum.parse().ok()?)),ExecutableBody::new(BodyTarget::None,vec![Instruction::MutateObject(Box::new(spellwake_engine::toolbox::MutateObjectSpec {
                source: spellwake_engine::toolbox::ObjectQuery::zone(
                    spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                    )),
                    spellwake_engine::program::ObjectFilter::creature().with_flags(
                        spellwake_engine::program::ObjectFilterFlags::FACE_DOWN,
                    ),
                ),
                selection: spellwake_engine::toolbox::ZoneSelection::Prompt {
                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                    min: Box::new(spellwake_engine::program::ValueExpr::literal(0)),
                    max: Box::new(spellwake_engine::program::ValueExpr::literal(1)),
                    constraint: None,
                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                    hidden: false,
                },
                op: spellwake_engine::toolbox::ObjectMutation::Orientation(spellwake_engine::toolbox::OrientationOp::TurnFaceUp),
                bind_mutated: None,
                bind_succeeded: None,
            }))])));
        } else if let Some((kind,effect))=line.strip_prefix("Whenever a ").and_then(|s|s.split_once(" you control dies, "))
            && let Some(kind)=vocabulary.resolve_label(kind) {
            triggers.push((spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::Dies { subject: spellwake_engine::program::ObjectFilter::any().controlled().with_subtype(kind), mode: spellwake_engine::program::DiesTriggerMode::Permanent }),body(effect,&card.name,vocabulary)?));
        } else if let Some((condition, effect)) = trigger(line, &card.name, types) {
            let body = if condition==spellwake_engine::program::TriggerSpec::deals_damage(spellwake_engine::program::ObjectFilter::self_object(), spellwake_engine::program::DamageTriggerRecipient::PlayerOrPlaneswalker, true) && effect=="copy the next loyalty ability you activate this turn when you activate it. You may choose new targets for the copy." {
                ExecutableBody::new(BodyTarget::None,vec![Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::watch_next_spell_or_loyalty(true)))])
            } else if condition == (spellwake_engine::program::TriggerSpec::status_changed(spellwake_engine::program::ObjectFilter::self_object(), spellwake_engine::program::StatusTriggerKind::Phased { out: false })) && effect == "put a +1/+1 counter on it." {
                ExecutableBody::new(BodyTarget::None, vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(spellwake_engine::effects::CounterKind::PlusOne, spellwake_engine::program::ValueExpr::literal(1))])
            } else if let Some(token) = effect.strip_prefix("create that many ") {
                let amount = if condition == spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player()
                    || condition == spellwake_engine::program::TriggerSpec::deals_damage(
                        spellwake_engine::program::ObjectFilter::creature().controlled(),
                        spellwake_engine::program::DamageTriggerRecipient::AnyPlayer,
                        true,
                    )
                {
                    spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::CombatDamageToPlayer)
                } else if condition == spellwake_engine::program::TriggerSpec::new(spellwake_engine::program::TriggerEvent::DealtDamage {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    linked_player: false,
                }) {
                    spellwake_engine::program::ValueExpr::event(spellwake_engine::program::EventValueKind::DamageReceived)
                } else {
                    return None;
                };
                let template = format!("a {}",token.strip_suffix('.').unwrap_or(token));
                let Instruction::CreateTokens { mut creations } = create_tokens(&template,vocabulary)? else {return None;};
                let [group] = creations.as_mut_slice() else {return None;};
                group.amount = amount;
                ExecutableBody::new(BodyTarget::None,vec![Instruction::CreateTokens {creations}])
            } else {body(effect, &card.name, vocabulary)?};
            triggers.push((condition, body));
        } else {
            other.push(line);
        }
    }
    if triggers.is_empty() {
        return None;
    }
    let mut reduced = card.clone();
    reduced.faces[0].oracle_text = Some(other.join("\n"));
    let mut program = if !other.is_empty() {
        super::compile(&reduced, vocabulary, Profile::Current)
            .ok()
            .or_else(|| {
                let cost = Some(fixed_cost(face.mana_cost.as_deref()?)?);
                let mut base = match face.type_line.as_deref()? {
                    "Artifact" => Program::artifact(cost, vec![]),
                    "Enchantment" => Program::enchantment(cost),
                    _ => return None,
                };
                for word in &other {
                    base.printed_keywords
                        .insert(static_keyword(&word.to_ascii_lowercase())?);
                }
                Some(base)
            })?
    } else if types.split_whitespace().any(|w| w == "Creature") {
        let remaining = other.join("\n");
        ordinary_creature(card, vocabulary, Profile::Current, Some(&remaining))
            .ok()
            .or_else(|| super::source_boost::compile(card, vocabulary, Some(&remaining)))
            .or_else(|| {
                let mut base = card.clone();
                base.faces[0].oracle_text = Some(remaining);
                reductions::battlefield(&base, vocabulary)
                    .or_else(|| flash_permissions::compile(&base, vocabulary))
            })?
    } else {
        // An additional subtype, static line or activated ability needs its own definition.
        if !other.is_empty() || types != face.type_line.as_deref()? {
            return None;
        }
        let mana = Some(fixed_cost(face.mana_cost.as_deref()?)?);
        let mut permanent = match types {
            "Artifact" | "Legendary Artifact" | "Snow Artifact" => Program::artifact(mana, vec![]),
            "Enchantment" | "Legendary Enchantment" | "Snow Enchantment" => {
                Program::enchantment(mana)
            }
            _ => return None,
        };
        if types.starts_with("Legendary ") {
            permanent.supertypes.insert(Supertype::Legendary);
        }
        if types.starts_with("Snow ") {
            permanent.supertypes.insert(Supertype::Snow);
        }
        permanent
    };
    let mut id = program
        .triggered
        .iter()
        .map(|t| t.id.0)
        .max()
        .map_or(Some(0), |v| v.checked_add(1))?;
    for (index, (condition, body)) in triggers.into_iter().enumerate() {
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            grouping: if once_each_turn.contains(&index) {
                TriggerGrouping::OneOrMoreOnceEachTurn
            } else if matches!(
                condition.event,
                spellwake_engine::program::TriggerEvent::DealtDamage {
                    subject,
                    linked_player: false,
                } if subject == spellwake_engine::program::ObjectFilter::self_object()
            ) || matches!(
                condition.event,
                spellwake_engine::program::TriggerEvent::PutIntoGraveyard {
                    player: PlayerRelation::Controller,
                    from_library: true,
                    ..
                }
            ) {
                TriggerGrouping::OneOrMoreTogether
            } else {
                TriggerGrouping::EachOccurrence
            },
            condition,
            body,
        });
        id = id.checked_add(1)?;
    }
    Some(program)
}

pub(super) fn trigger<'a>(
    line: &'a str,
    name: &str,
    types: &str,
) -> Option<(TriggerSpec, &'a str)> {
    let line = line
        .strip_prefix("Constellation — ")
        .or_else(|| line.strip_prefix("Landfall — "))
        .unwrap_or(line);
    for (prefix, out) in [
        ("Whenever this creature phases out, ", true),
        ("Whenever this creature phases in, ", false),
    ] {
        if let Some(effect) = line.strip_prefix(prefix) {
            return Some((
                spellwake_engine::program::TriggerSpec::status_changed(
                    spellwake_engine::program::ObjectFilter::self_object(),
                    spellwake_engine::program::StatusTriggerKind::Phased { out },
                ),
                effect,
            ));
        }
    }
    if let Some(effect) =
        line.strip_prefix("Whenever a creature you control with a +1/+1 counter on it attacks, ")
    {
        return Some((
            spellwake_engine::program::TriggerSpec::attacks(
                spellwake_engine::program::ObjectFilter::creature()
                    .controlled()
                    .with_flags(
                        spellwake_engine::program::ObjectFilterFlags::WITH_PLUS_ONE_COUNTER,
                    ),
                spellwake_engine::program::AttackTriggerMode::Individual {
                    capture_defender: false,
                },
            ),
            effect,
        ));
    }
    if let Some((who, effect)) = line
        .strip_prefix("Whenever ")
        .and_then(|s| s.split_once(" enters or attacks, "))
        && (who == name || Some(who) == name.split(',').next() || who == "this creature")
    {
        return Some((
            spellwake_engine::program::TriggerSpec::any_of(vec![
                spellwake_engine::program::TriggerEvent::Enters {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    from_zone: spellwake_engine::program::TriggerFromZone::Any,
                    cast_origin: None,
                    attached_to_creature: false,
                },
                spellwake_engine::program::TriggerEvent::Attacks {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    mode: spellwake_engine::program::AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
                },
            ]),
            effect,
        ));
    }
    if let Some((cardinal, effect)) = line
        .strip_prefix("Whenever you draw your ")
        .and_then(|clause| clause.split_once(" card each turn, "))
    {
        let ordinal = match cardinal {
            "first" => 1,
            "second" => 2,
            "third" => 3,
            "fourth" => 4,
            _ => return None,
        };
        return Some((
            spellwake_engine::program::TriggerSpec::player_action(
                spellwake_engine::program::PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::DrewCard {
                    nth_each_turn: Some(std::num::NonZeroU16::new(ordinal)?),
                },
            ),
            effect,
        ));
    }
    if let Some(effect) = line.strip_prefix(
        "Whenever one or more creature cards are put into your graveyard from your library, ",
    ) {
        return Some((
            spellwake_engine::program::TriggerSpec::new(
                spellwake_engine::program::TriggerEvent::PutIntoGraveyard {
                    subject: spellwake_engine::program::ObjectFilter::creature(),
                    player: PlayerRelation::Controller,
                    from_library: true,
                    own_turn_only: false,
                },
            ),
            effect,
        ));
    }
    if let Some(effect) =
        line.strip_prefix("When you cast this spell, if you have fewer than seven cards in hand, ")
    {
        return Some((
            spellwake_engine::program::TriggerSpec::self_cast().only_if(
                spellwake_engine::program::TriggerCondition::HandSizeBelow(7),
            ),
            effect,
        ));
    }
    if let Some(effect) = line.strip_prefix("When you cast this spell, ") {
        return Some((spellwake_engine::program::TriggerSpec::self_cast(), effect));
    }
    if let Some(effect) = line.strip_prefix("When you cycle this card, ") {
        return Some((
            spellwake_engine::program::TriggerSpec::player_action(
                spellwake_engine::program::PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::Cycled { self_only: true },
            )
            .in_zone(spellwake_engine::program::TriggerZone::Any),
            effect,
        ));
    }
    for (prefix, step, player) in [
        (
            "At the beginning of combat on your turn, ",
            spellwake_engine::turn::Step::BeginningCombat,
            PlayerRelation::Controller,
        ),
        (
            "At the beginning of combat on each turn, ",
            spellwake_engine::turn::Step::BeginningCombat,
            PlayerRelation::AnyParticipant,
        ),
        (
            "At the beginning of your upkeep, ",
            spellwake_engine::turn::Step::Upkeep,
            PlayerRelation::Controller,
        ),
        (
            "At the beginning of each player's upkeep, ",
            spellwake_engine::turn::Step::Upkeep,
            PlayerRelation::AnyParticipant,
        ),
        (
            "At the beginning of each upkeep, ",
            spellwake_engine::turn::Step::Upkeep,
            PlayerRelation::AnyParticipant,
        ),
        (
            "At the beginning of each player's draw step, ",
            spellwake_engine::turn::Step::Draw,
            PlayerRelation::AnyParticipant,
        ),
        (
            "At the beginning of each end step, ",
            spellwake_engine::turn::Step::End,
            PlayerRelation::AnyParticipant,
        ),
        (
            "At the beginning of each of your postcombat main phases, ",
            spellwake_engine::turn::Step::PostcombatMain,
            PlayerRelation::Controller,
        ),
        (
            "At the beginning of your end step, ",
            spellwake_engine::turn::Step::End,
            PlayerRelation::Controller,
        ),
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some((
                spellwake_engine::program::TriggerSpec::step_began(step, player),
                rest,
            ));
        }
    }
    for (pattern, relation) in [
        (
            "Whenever a land you control enters, ",
            spellwake_engine::program::ObjectFilter::any_types(
                spellwake_engine::program::CardTypeMask::from_card_type(CardType::Land),
            )
            .controlled(),
        ),
        (
            "Whenever an enchantment you control enters, ",
            spellwake_engine::program::ObjectFilter::any_types(
                spellwake_engine::program::CardTypeMask::from_card_type(CardType::Enchantment),
            )
            .controlled(),
        ),
        (
            "Whenever this enchantment or another enchantment you control enters, ",
            spellwake_engine::program::ObjectFilter::any_types(
                spellwake_engine::program::CardTypeMask::from_card_type(CardType::Enchantment),
            )
            .controlled()
            .or_self(),
        ),
        (
            "Whenever this creature or another enchantment you control enters, ",
            spellwake_engine::program::ObjectFilter::any_types(
                spellwake_engine::program::CardTypeMask::from_card_type(CardType::Enchantment),
            )
            .controlled()
            .or_self(),
        ),
        (
            "Whenever an Equipment you control enters, ",
            spellwake_engine::program::ObjectFilter::any()
                .controlled()
                .with_artifact_subtype(spellwake_engine::program::ArtifactSubtype::Equipment),
        ),
        (
            "Whenever an artifact you control enters, ",
            spellwake_engine::program::ObjectFilter::any_types(
                spellwake_engine::program::CardTypeMask::from_card_type(CardType::Artifact),
            )
            .controlled(),
        ),
        (
            "Whenever another nontoken creature you control enters, ",
            spellwake_engine::program::ObjectFilter::creature()
                .controlled()
                .nontoken()
                .other_than_ability_source(),
        ),
        (
            "Whenever a nontoken creature you control enters, ",
            spellwake_engine::program::ObjectFilter::creature()
                .controlled()
                .nontoken(),
        ),
    ] {
        if let Some(effect) = line.strip_prefix(pattern) {
            return Some((
                spellwake_engine::program::TriggerSpec::enters(relation),
                effect,
            ));
        }
    }
    if let Some(effect) = line
        .strip_prefix("Whenever a creature an opponent controls attacks, ")
        .and_then(|s| s.strip_prefix("if you're the defending player, "))
    {
        return Some((
            spellwake_engine::program::TriggerSpec::attacks(
                spellwake_engine::program::ObjectFilter::any(),
                spellwake_engine::program::AttackTriggerMode::AtControllerOrPlaneswalker,
            ),
            effect,
        ));
    }
    if let Some(effect) = line.strip_prefix("Whenever an opponent taps an artifact for mana, ") {
        return Some((
            spellwake_engine::program::TriggerSpec::player_action(
                spellwake_engine::program::PlayerRelation::Opponent,
                spellwake_engine::program::PlayerTriggerAction::ArtifactTappedForMana,
            ),
            effect,
        ));
    }
    if let Some(effect) = line.strip_prefix("Whenever an opponent draws a card, ") {
        return Some((
            spellwake_engine::program::TriggerSpec::player_action(
                PlayerRelation::Opponent,
                spellwake_engine::program::PlayerTriggerAction::DrewCard {
                    nth_each_turn: None,
                },
            ),
            effect,
        ));
    }
    for (prefix, player) in [
        ("Whenever you cast ", PlayerRelation::Controller),
        ("Whenever an opponent casts ", PlayerRelation::Opponent),
        ("Whenever a player casts ", PlayerRelation::AnyParticipant),
    ] {
        if let Some((class, effect)) = line.strip_prefix(prefix).and_then(|s| s.split_once(", ")) {
            let filter = match class {
                "a spell" => spellwake_engine::program::SpellFilter::any(),
                "a noncreature spell" => spellwake_engine::program::SpellFilter::noncreature(),
                "a historic spell" => spellwake_engine::program::SpellFilter::historic(),
                "an instant or sorcery spell" => {
                    spellwake_engine::program::SpellFilter::instant_or_sorcery()
                }
                "an instant or sorcery spell with mana value greater than the number of experience counters you have" => {
                    spellwake_engine::program::SpellFilter::instant_or_sorcery().above_experience()
                }
                "an artifact spell" => {
                    spellwake_engine::program::SpellFilter::of_type(CardType::Artifact)
                }
                "an enchantment spell" => {
                    spellwake_engine::program::SpellFilter::of_type(CardType::Enchantment)
                }
                "a creature spell" => {
                    spellwake_engine::program::SpellFilter::of_type(CardType::Creature)
                }
                "an instant spell" => {
                    spellwake_engine::program::SpellFilter::of_type(CardType::Instant)
                }
                "a sorcery spell" => {
                    spellwake_engine::program::SpellFilter::of_type(CardType::Sorcery)
                }
                _ => continue,
            };
            return Some((
                spellwake_engine::program::TriggerSpec::spell_cast(player, filter),
                effect,
            ));
        }
    }
    if let Some(effect) = line.strip_prefix("Whenever another nontoken creature you control dies, ")
    {
        return Some((
            spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::creature()
                    .controlled()
                    .nontoken()
                    .other_than_ability_source(),
            ),
            effect,
        ));
    }
    if let Some(effect) = line.strip_prefix("Whenever a nontoken creature you control dies, ") {
        return Some((
            spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::creature()
                    .controlled()
                    .nontoken(),
            ),
            effect,
        ));
    }
    if let Some(effect) = line.strip_prefix("Whenever a creature you control dies, ") {
        return Some((
            spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::creature().controlled(),
            ),
            effect,
        ));
    }
    for (prefix, condition) in [
        (
            "Whenever an enchantment you control enters, ",
            spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::any_types(
                    spellwake_engine::program::CardTypeMask::from_card_type(CardType::Enchantment),
                )
                .controlled(),
            ),
        ),
        (
            "Whenever you sacrifice an enchantment, ",
            spellwake_engine::program::TriggerSpec::player_action(
                PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::Sacrificed(
                    spellwake_engine::program::ObjectFilter::any_types(
                        spellwake_engine::program::CardTypeMask::from_card_type(
                            CardType::Enchantment,
                        ),
                    ),
                ),
            ),
        ),
        (
            "Whenever you sacrifice a permanent, ",
            spellwake_engine::program::TriggerSpec::player_action(
                PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::Sacrificed(
                    spellwake_engine::program::ObjectFilter::any(),
                ),
            ),
        ),
        (
            "Whenever a creature you control deals combat damage to a player, ",
            spellwake_engine::program::TriggerSpec::deals_damage(
                spellwake_engine::program::ObjectFilter::creature().controlled(),
                spellwake_engine::program::DamageTriggerRecipient::AnyPlayer,
                true,
            ),
        ),
        (
            "Whenever a creature an opponent controls dies, ",
            spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::creature().opposing(),
            ),
        ),
        (
            "Whenever another nontoken creature dies, ",
            spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::creature()
                    .nontoken()
                    .other_than_ability_source(),
            ),
        ),
        (
            "Whenever a token you control leaves the battlefield, ",
            spellwake_engine::program::TriggerSpec::leaves_battlefield(
                spellwake_engine::program::ObjectFilter::any()
                    .controlled()
                    .token(),
            ),
        ),
        (
            "Whenever a nontoken creature you control with a +1/+1 counter on it dies, ",
            spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::creature()
                    .controlled()
                    .nontoken()
                    .with_flags(
                        spellwake_engine::program::ObjectFilterFlags::WITH_PLUS_ONE_COUNTER,
                    ),
            ),
        ),
    ] {
        if let Some(effect) = line.strip_prefix(prefix) {
            return Some((condition, effect));
        }
    }
    if let Some(effect) = line.strip_prefix("Whenever another creature dies, ") {
        return Some((
            spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::creature().other_than_ability_source(),
            ),
            effect,
        ));
    }
    if let Some(effect) = line.strip_prefix("Whenever another creature you control dies, ") {
        return Some((
            spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::creature()
                    .controlled()
                    .other_than_ability_source(),
            ),
            effect,
        ));
    }
    // The independently parsed effect must consume the complete printed clause.
    for (prefix, relation) in [
        (
            "Whenever another creature you control enters, ",
            spellwake_engine::program::ObjectFilter::creature()
                .controlled()
                .other_than_ability_source(),
        ),
        (
            "Whenever another creature enters, ",
            spellwake_engine::program::ObjectFilter::creature().other_than_ability_source(),
        ),
        (
            "Whenever this creature or another creature you control enters, ",
            spellwake_engine::program::ObjectFilter::creature().controlled(),
        ),
    ] {
        if let Some(effect) = line.strip_prefix(prefix) {
            return Some((
                spellwake_engine::program::TriggerSpec::enters(relation),
                effect,
            ));
        }
    }
    let line = line
        .strip_prefix("When ")
        .or_else(|| line.strip_prefix("Whenever "))?;
    let rest = line
        .strip_prefix(name)
        .or_else(|| {
            types
                .split_whitespace()
                .any(|ty| ty == "Legendary")
                .then(|| {
                    name.split_once(',')
                        .and_then(|(short, _)| line.strip_prefix(short))
                })
                .flatten()
        })
        .and_then(|s| s.strip_prefix(' '))
        .or_else(|| {
            let rest = line.strip_prefix("this ")?;
            let (noun, rest) = rest.split_once(' ')?;
            (noun == "permanent"
                || types
                    .split_whitespace()
                    .any(|t| t.eq_ignore_ascii_case(noun)))
            .then_some(rest)
        })?;
    if let Some((minimum, effect)) = rest
        .strip_prefix("enters and whenever you cast a spell with mana value ")
        .and_then(|text| text.split_once(" or greater, "))
    {
        return Some((
            spellwake_engine::program::TriggerSpec::any_of(vec![
                spellwake_engine::program::TriggerEvent::Enters {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    from_zone: spellwake_engine::program::TriggerFromZone::Any,
                    cast_origin: None,
                    attached_to_creature: false,
                },
                spellwake_engine::program::TriggerEvent::SpellCast {
                    caster: spellwake_engine::program::TriggerPlayerScope::Relation(
                        spellwake_engine::program::PlayerRelation::Controller,
                    ),
                    filter: spellwake_engine::program::SpellFilter::any(),
                    self_cast: false,
                    include_copy: false,
                    min_mana_value: Some(minimum.parse().ok()?),
                },
            ]),
            effect,
        ));
    }
    if let Some(effect) = rest.strip_prefix("attacks, ").filter(|s| {
        s.contains("defending player controls") || s.starts_with("defending player exiles ")
    }) {
        return Some((
            spellwake_engine::program::TriggerSpec::attacks(
                spellwake_engine::program::ObjectFilter::self_object(),
                spellwake_engine::program::AttackTriggerMode::Individual {
                    capture_defender: true,
                },
            ),
            effect,
        ));
    }
    if let Some(effect) = rest.strip_prefix("attacks, ")
        && effect.ends_with("there is an additional combat phase.")
    {
        return Some((
            spellwake_engine::program::TriggerSpec::self_attacks(),
            effect,
        ));
    }
    if let Some(effect) = rest.strip_prefix("attacks, ")
        && spells::ordinary_effects::group_modifier(effect.strip_suffix('.')?).is_some()
    {
        return Some((
            spellwake_engine::program::TriggerSpec::self_attacks(),
            effect,
        ));
    }
    for (start, condition) in [
        (
            "enters, if you cast it from your hand, ",
            spellwake_engine::program::TriggerSpec::new(
                spellwake_engine::program::TriggerEvent::Enters {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    from_zone: spellwake_engine::program::TriggerFromZone::Any,
                    cast_origin: Some(spellwake_engine::program::CastEntryKind::Cast {
                        from_hand: true,
                    }),
                    attached_to_creature: false,
                },
            ),
        ),
        (
            "enters, if you cast it, ",
            spellwake_engine::program::TriggerSpec::new(
                spellwake_engine::program::TriggerEvent::Enters {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    from_zone: spellwake_engine::program::TriggerFromZone::Any,
                    cast_origin: Some(spellwake_engine::program::CastEntryKind::Cast {
                        from_hand: false,
                    }),
                    attached_to_creature: false,
                },
            ),
        ),
        (
            "deals damage to an opponent, ",
            spellwake_engine::program::TriggerSpec::deals_damage(
                spellwake_engine::program::ObjectFilter::self_object(),
                spellwake_engine::program::DamageTriggerRecipient::Opponent,
                false,
            ),
        ),
        (
            "deals combat damage to a player or planeswalker, ",
            spellwake_engine::program::TriggerSpec::deals_damage(
                spellwake_engine::program::ObjectFilter::self_object(),
                spellwake_engine::program::DamageTriggerRecipient::PlayerOrPlaneswalker,
                true,
            ),
        ),
        (
            "becomes tapped, ",
            spellwake_engine::program::TriggerSpec::becomes_tapped(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
        ),
        (
            "attacks, ",
            spellwake_engine::program::TriggerSpec::self_attacks(),
        ),
        (
            "deals combat damage to a player who controls more lands than you, ",
            spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player()
                .only_if(spellwake_engine::program::TriggerCondition::DefendingPlayerHasMoreLands),
        ),
        (
            "deals combat damage to a player, ",
            spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
        ),
        (
            "is dealt damage, ",
            spellwake_engine::program::TriggerSpec::new(
                spellwake_engine::program::TriggerEvent::DealtDamage {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    linked_player: false,
                },
            ),
        ),
        (
            "enters, ",
            spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
        ),
        (
            "dies, ",
            spellwake_engine::program::TriggerSpec::dies(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
        ),
        (
            "leaves the battlefield, ",
            spellwake_engine::program::TriggerSpec::leaves_battlefield(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
        ),
        (
            "is put into a graveyard from the battlefield, ",
            spellwake_engine::program::TriggerSpec::new(
                spellwake_engine::program::TriggerEvent::Dies {
                    subject: spellwake_engine::program::ObjectFilter::self_object(),
                    mode: spellwake_engine::program::DiesTriggerMode::Permanent,
                },
            ),
        ),
    ] {
        if let Some(effect) = rest.strip_prefix(start) {
            return Some((condition, effect));
        }
    }
    None
}

fn count(s: &str) -> Option<ValueExpr> {
    Some(spellwake_engine::program::ValueExpr::Literal(
        amount(s)?.into(),
    ))
}
fn cards(s: &str) -> Option<ValueExpr> {
    count(
        s.strip_suffix(" cards")
            .or_else(|| s.strip_suffix(" card"))?,
    )
}
pub(super) fn body(
    text: &str,
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ExecutableBody> {
    if let Some(cost) = text
        .strip_prefix("this creature phases out unless you pay ")
        .and_then(|rest| rest.strip_suffix('.'))
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::PromptChoice(Box::new(
                    spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                        ResolvingPlayer::Controller,
                        fixed_cost(cost)?,
                        spellwake_engine::program::ValueExpr::Literal(0.into()),
                        2,
                        1,
                    ),
                )),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::phase_out_query(
                    spellwake_engine::toolbox::ObjectQuery::Source,
                ),
            ],
        ));
    }
    if text == "target creature phases out." {
        return Some(ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::phase_out_query(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                ),
            ],
        ));
    }
    if text == "all lands you control phase out." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::phase_out_query(
                    spellwake_engine::toolbox::ObjectQuery::zone(
                        spellwake_engine::toolbox::QueryZone::Battlefield,
                        ObjectFilter::land().controlled(),
                    ),
                ),
            ],
        ));
    }
    if text
        == "each opponent loses X life and you scry X, where X is the number of Zombies you control."
    {
        let zombie = vocabulary.resolve_label("Zombie")?;
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::BranchCondition {
                    condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Compare {
                        left: Box::new(spellwake_engine::program::ValueExpr::controlled_subtype(
                            zombie,
                        )),
                        op: spellwake_engine::program::Comparison::Greater,
                        right: Box::new(spellwake_engine::program::ValueExpr::literal(0)),
                    }),
                    destination: 3,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                    spellwake_engine::toolbox::PlayerSelector::Group(
                        spellwake_engine::program::AffectedPlayers::EachOpponent,
                    ),
                    spellwake_engine::program::ValueExpr::controlled_subtype(zombie),
                ),
                Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::scry(
                        ResolvingPlayer::Controller,
                        spellwake_engine::program::ValueExpr::controlled_subtype(zombie),
                    ),
                )),
            ],
        ));
    }
    if text
        == "gain control of target creature until end of turn. Untap that creature. Until end of turn, it has base power and toughness 10/10 and gains trample, annihilator 2, and haste."
    {
        use spellwake_engine::characteristics::{
            AbilityChange, ControlRecipient, PowerToughness, ResolvedDuration, ResolvedPartSpec,
            ResolvedPartsSpec,
        };
        use spellwake_engine::program::{TriggeredAbility, TriggeredAbilityId};
        let until_end = ResolvedDuration::UntilEndOfTurn;
        let grant = TriggeredAbility::annihilator(TriggeredAbilityId(0), 2.into());
        return Some(ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                        ControlRecipient::ResolvingController,
                    )]),
                    until_end,
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(false),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![
                        ResolvedPartSpec::Abilities(AbilityChange {
                            granted_keywords: BTreeSet::from([
                                KeywordAbility::Trample,
                                KeywordAbility::Haste,
                            ]),
                            granted_triggers: vec![grant],
                            ..Default::default()
                        }),
                        ResolvedPartSpec::SetPT(PowerToughness {
                            power: 10.into(),
                            toughness: 10.into(),
                        }),
                    ]),
                    until_end,
                ),
            ],
        ));
    }
    if let Some(number) = text.trim().strip_prefix("look at the top ").and_then(|s| {
        s.strip_suffix(
            " cards of your library. Put one of them into your hand and the rest into your graveyard.",
        )
    }) {
        let count = u8::try_from(amount(number)?).ok().filter(|&n| n > 0)?;
        return Some(ExecutableBody::new(
            BodyTarget::None,
            spellwake_engine::toolbox::ZoneOpSpec::look_top_choose_instructions(
                ResolvingPlayer::Controller,
                ResolvingPlayer::Controller,
                spellwake_engine::program::ValueExpr::literal(i64::from(count)),
                None,
                1,
                false,
                spellwake_engine::program::LookTopKeepDest::Hand,
                spellwake_engine::program::LookTopRestDest::Graveyard,
                false,
                false,
                false,
                None,
            ),
        ));
    }

    if let Some(rest) =
        text.strip_prefix("you may put a land card from your hand onto the battlefield tapped.")
    {
        use spellwake_engine::program::{Comparison, ResolutionCondition};
        let put = |types| {
            Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::put_hand_permanents_onto_battlefield(
                    ResolvingPlayer::Controller,
                    types,
                    1,
                    true,
                ),
            ))
        };
        let instructions = if rest.is_empty() {
            vec![put([CardType::Land].into())]
        } else {
            let minimum=amount(rest.strip_prefix(" If you control ")?.strip_suffix(" or more enchantments, instead you may put a creature or land card from your hand onto the battlefield tapped.")?)?;
            vec![
                Instruction::BranchCondition {
                    condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                        Box::new(ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::controlled_type(
                                CardType::Enchantment,
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: spellwake_engine::program::ValueExpr::Literal(minimum.into()),
                        }),
                    )),
                    destination: 3,
                },
                put([CardType::Creature, CardType::Land].into()),
                Instruction::Jump { destination: 4 },
                put([CardType::Land].into()),
            ]
        };
        return Some(ExecutableBody::new(BodyTarget::None, instructions));
    }
    if text == "goad target creature that player controls." {
        return Some(ExecutableBody::new(
            BodyTarget::FilteredPermanent(ObjectFilter::creature().of_related_player()),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::goad_target()],
        ));
    }
    if text
        == "exile the top card of that player's library. Until end of turn, you may cast that card and you may spend mana as though it were mana of any color to cast that spell."
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_library_top(
                    ResolvingPlayer::UniqueRelatedPlayer,
                    spellwake_engine::program::ValueExpr::literal(1),
                    false,
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::grant_play_permission(
                    spellwake_engine::toolbox::ObjectQuery::filtered(
                        spellwake_engine::toolbox::ObjectQuery::LastMoved,
                        ObjectFilter::nonland(),
                    ),
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                    true,
                    false,
                    true,
                ),
            ],
        ));
    }
    if text == "return target land card from your graveyard to the battlefield tapped." {
        return Some(ExecutableBody::new(
            BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::land()),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(true)],
        ));
    }
    if let Some(description) = text.strip_prefix(
        "choose any number of target planeswalkers you control. Until end of turn, they become ",
    ) {
        use spellwake_engine::characteristics::{
            AbilityChange, Color, ColorOperation, InstructionSubtypeOperand, PowerToughness,
            ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, SubtypeOperation,
            TypeChangeSpec, TypeOperation,
        };
        use spellwake_engine::program::{CardType, TriggeredAbility, TriggeredAbilityId};
        let (characteristics, abilities) = description.split_once(" creatures with ")?;
        let mut words = characteristics.split_whitespace();
        let (power, toughness) = words.next()?.split_once('/')?;
        let color = match words.next()? {
            "white" => Color::White,
            "blue" => Color::Blue,
            "black" => Color::Black,
            "red" => Color::Red,
            "green" => Color::Green,
            _ => return None,
        };
        let subtype = vocabulary.resolve_label(words.next()?)?;
        if words.next().is_some() {
            return None;
        }
        let (keywords, quoted) = abilities.split_once(", and \"")?;
        let keywords = keywords
            .split(", ")
            .map(super::static_keyword)
            .collect::<Option<std::collections::BTreeSet<_>>>()?;
        let quoted = quoted
            .strip_suffix('"')?
            .strip_prefix("Whenever this creature deals combat damage to a player, ")?;
        let grant = TriggeredAbility {
            id: TriggeredAbilityId(0),
            condition: spellwake_engine::program::TriggerSpec::self_deals_combat_damage_to_player(),
            grouping: TriggerGrouping::EachOccurrence,
            body: body(quoted, name, vocabulary)?,
        };
        let parts = ResolvedPartsSpec(vec![
            ResolvedPartSpec::Type(TypeChangeSpec {
                types: TypeOperation::Set([CardType::Creature].into()),
                supertypes: TypeOperation::Keep,
                subtypes: SubtypeOperation::Set(InstructionSubtypeOperand::Literal(
                    [subtype].into(),
                )),
                land_subtypes: SubtypeOperation::Keep,
            }),
            ResolvedPartSpec::Color(ColorOperation::Set([color].into())),
            ResolvedPartSpec::Abilities(AbilityChange {
                granted_keywords: keywords,
                granted_triggers: vec![grant],
                ..Default::default()
            }),
            ResolvedPartSpec::SetPT(PowerToughness {
                power: power.parse::<i64>().ok()?.into(),
                toughness: toughness.parse::<i64>().ok()?.into(),
            }),
        ]);
        return Some(ExecutableBody::new(
            BodyTarget::FilteredPermanent(ObjectFilter::planeswalker().controlled()),
            vec![Instruction::ApplyContinuous(Box::new(
                spellwake_engine::toolbox::ApplyContinuousSpec::modify_any_number(
                    parts,
                    ResolvedDuration::UntilEndOfTurn,
                ),
            ))],
        ));
    }
    if let Some((source, rest)) = text
        .strip_prefix("for each Aura and Equipment attached to ")
        .and_then(|s| s.split_once(", create "))
        && (source == name || name.split(',').next() == Some(source))
        && let Some(template) =
            rest.strip_suffix(". Exile those tokens at the beginning of the next end step.")
    {
        let Instruction::CreateTokens { mut creations } = create_tokens(template, vocabulary)?
        else {
            return None;
        };
        let [creation] = creations.as_mut_slice() else {
            return None;
        };
        if creation.amount != spellwake_engine::program::ValueExpr::Literal(1.into()) {
            return None;
        }
        creation.amount = spellwake_engine::program::ValueExpr::AttachedCount(
            spellwake_engine::program::AttachedCountKind::AurasAndEquipmentOnSource,
        );
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::CreateTokens { creations },
                Instruction::ScheduleDelayedStepTrigger {
                    step: spellwake_engine::turn::Step::End,
                    own_step_only: false,
                    body: Box::new(ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::ZoneOp(Box::new(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                                spellwake_engine::toolbox::ZoneDest::exile(),
                            ),
                        ))],
                    )),
                },
            ],
        ));
    }
    if text == "tap target creature defending player controls." {
        return Some(ExecutableBody::new(
            BodyTarget::FilteredPermanent(ObjectFilter::creature().of_related_player()),
            vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(true)],
        ));
    }
    if let Some(template) = text
        .strip_prefix("create X ")
        .and_then(|effect| effect.strip_suffix(", where X is that spell's mana value."))
    {
        let Instruction::CreateTokens { mut creations } =
            create_tokens(&format!("one {template}"), vocabulary)?
        else {
            return None;
        };
        let [creation] = creations.as_mut_slice() else {
            return None;
        };
        creation.amount = spellwake_engine::program::ValueExpr::object_stat(
            spellwake_engine::program::ValueObjectRef::TriggeringSpell,
            spellwake_engine::program::ObjectStatKind::ManaValue,
        );
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens { creations }],
        ));
    }
    if let Some(instructions) = super::ordinary_text::amass(text, vocabulary) {
        return Some(ExecutableBody::new(BodyTarget::None, instructions.to_vec()));
    }
    if let Some((life, effect)) = text
        .strip_prefix("you lose ")
        .and_then(|text| text.split_once(" life and "))
        && let Some(instructions) = super::ordinary_text::amass(effect, vocabulary)
    {
        let amount = life.parse::<i64>().ok().filter(|value| *value > 0)?;
        let mut body = vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                spellwake_engine::program::ValueExpr::literal(amount),
            ),
        ];
        body.extend(instructions);
        return Some(ExecutableBody::new(BodyTarget::None, body));
    }
    if let Some((source, subtype)) = text
        .strip_prefix("put a +1/+1 counter on ")
        .and_then(|effect| effect.split_once(". If that creature was a "))
        .and_then(|(source, effect)| {
            effect
                .strip_suffix(", you may draw a card. If you do, you lose 1 life.")
                .map(|subtype| (source, subtype))
        })
    {
        if source != name
            && Some(source) != name.split_once(',').map(|(short, _)| short)
            && source != "this creature"
        {
            return None;
        }
        use spellwake_engine::program::{Comparison, ResolutionCondition};
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                    spellwake_engine::effects::CounterKind::PlusOne,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
                Instruction::BranchCondition {
                    condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                        Box::new(ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::object_matches(
                                spellwake_engine::program::MatchObjectRef::DyingCreature,
                                spellwake_engine::program::ObjectFilter::creature()
                                    .with_subtype(vocabulary.resolve_label(subtype)?),
                            ),
                            comparison: Comparison::Equal,
                            right: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        }),
                    )),
                    destination: 5,
                },
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 2,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ],
        ));
    }
    if text == "you may put a quest counter on this enchantment." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                    spellwake_engine::effects::CounterKind::Quest,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ],
        ));
    }
    if text == "draw cards equal to the difference." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::PlayerStat(
                        spellwake_engine::program::PlayerStatExpr::CardsShortOfHandSize(7),
                    ),
                ),
            ],
        ));
    }

    if let Some((price, effect)) = text.strip_prefix("you may pay ").and_then(|clause| clause.split_once(". If you do, tap up to "))
        && let Some(subjects) = effect.strip_suffix(" target permanents. Those permanents don't untap during their controllers' next untap steps.")
    {
        let maximum = u16::try_from(amount(subjects)?).ok()?;
        return Some(
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![
                    Instruction::PromptChoice(Box::new(
                        spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                            ResolvingPlayer::Controller,
                            fixed_cost(price)?,
                            spellwake_engine::program::ValueExpr::Literal(0.into()),
                            1,
                            3,
                        ),
                    )),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(true),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::skip_next_untap_targets(),
                ],
            )
            .with_permanent_target_bounds(0, maximum),
        );
    }
    if let Some((price, paid)) = text
        .strip_prefix("you may pay ")
        .and_then(|s| s.split_once(". If you do, "))
        && paid.strip_prefix("untap all attacking creatures and after this phase, ")
            == Some("there is an additional combat phase.")
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::PromptChoice(Box::new(
                    spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                        ResolvingPlayer::Controller,
                        fixed_cost(price)?,
                        spellwake_engine::program::ValueExpr::Literal(0.into()),
                        1,
                        3,
                    ),
                )),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::set_group_tapped(
                    ObjectFilter::creature().attacking(),
                    false,
                ),
                Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::additional_combat(
                        spellwake_engine::program::CombatUntapGroup::None,
                    ),
                )),
            ],
        ));
    }
    if let Some(amount) = text
        .strip_prefix("bolster ")
        .and_then(|s| s.strip_suffix('.'))
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|v| *v > 0)
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::bolster(amount),
            ))],
        ));
    }
    if text.strip_prefix("gain control of that artifact ")
        == Some("until the end of your next turn.")
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_control(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        spellwake_engine::program::EffectSubject::TriggerSubject,
                    )),
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::characteristics::ResolvedDuration::ThroughControllerNextTurn,
                ),
            ],
        ));
    }
    if text == "you get an experience counter." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::add_player_counter(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::effects::CounterKind::Experience,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ],
        ));
    }
    if let Some(rest) = text
        .strip_prefix("choose target creature card in your graveyard. ")
        .and_then(|s| s.strip_prefix("If that card's mana value is less than or equal to "))
        .and_then(|s| s.strip_prefix("the number of experience counters you have, "))
    {
        if rest != "return it to the battlefield. Otherwise, put it into your hand." {
            return None;
        }
        use spellwake_engine::program::{Comparison, ResolutionCondition};
        return Some(ExecutableBody::new(
            BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature()),
            vec![
                Instruction::BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(ResolutionCondition {
                        left: spellwake_engine::program::ValueExpr::object_stat(
                            spellwake_engine::program::ValueObjectRef::TargetObject,
                            spellwake_engine::program::ObjectStatKind::ManaValue,
                        ),
                        comparison: Comparison::LessOrEqual,
                        right: spellwake_engine::program::ValueExpr::counters(
                            spellwake_engine::program::CounterSubjectRef::ControllerExperience,
                            None,
                        ),
                    }))), destination: 3 },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::return_target_to_battlefield(false),
                Instruction::Jump { destination: 4 },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand(),
            ],
        ));
    }
    if text == "that player draws an additional card." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_resolving_player(
                    ResolvingPlayer::UniqueRelatedPlayer,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ],
        ));
    }
    if let Some((source, amount)) = text
        .strip_suffix(" damage to that player.")
        .and_then(|clause| clause.split_once(" deals "))
        && (source == name || Some(source) == name.split(',').next() || source == "this creature")
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                        spellwake_engine::program::DamageRecipientsRef::Group(
                            spellwake_engine::program::DamageGroup::RelatedPlayer,
                        ),
                    )),
                    spellwake_engine::program::ValueExpr::literal(ordinary_text::amount(amount)?),
                    false,
                ),
            ],
        ));
    }
    if let Some((template, cost)) = text
        .strip_prefix("create ")
        .and_then(|s| s.strip_suffix('.'))
        .and_then(|s| s.split_once(" unless that creature's controller pays "))
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::PromptChoice(Box::new(
                    spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                        ResolvingPlayer::TriggeringAttackerController,
                        fixed_cost(cost)?,
                        spellwake_engine::program::ValueExpr::Literal(0.into()),
                        2,
                        1,
                    ),
                )),
                create_tokens(template, vocabulary)?,
            ],
        ));
    }
    if let Some((cost, template)) = text
        .strip_prefix("that player may pay ")
        .and_then(|clause| clause.split_once(". If the player doesn't, you create "))
    {
        let creation = create_tokens(template.strip_suffix('.')?, vocabulary)?;
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::PromptChoice(Box::new(
                    spellwake_engine::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                        ResolvingPlayer::UniqueRelatedPlayer,
                        fixed_cost(cost)?,
                        spellwake_engine::program::ValueExpr::Literal(0.into()),
                        2,
                        1,
                    ),
                )),
                creation,
            ],
        ));
    }
    if let Some(symbol) = text.strip_prefix("add that much ").and_then(|s| {
        s.strip_suffix(". Until end of turn, you don't lose this mana as steps and phases end.")
    }) {
        let price = fixed_cost(symbol)?;
        if price.generic != 0.into() || price.required.len() != 1 || symbol.len() != 3 {
            return None;
        }
        let (kind, _) = price.required.into_iter().next()?;
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                    ResolvingPlayer::Controller,
                    spellwake_engine::program::EffectManaYield::CombatDamageUntilCleanup(kind),
                ),
            ))],
        ));
    }
    if let Some(description) = text.strip_prefix("create ").and_then(|s| {
        s.strip_suffix(r#" with "This token gets +1/+1 for each artifact you control.""#)
    }) {
        use spellwake_engine::characteristics::{
            ControllerSelector, NumericExpression, NumericPair, PermanentSelector, StaticPart,
            StaticParts,
        };
        let Instruction::CreateTokens { mut creations } = create_tokens(description, vocabulary)?
        else {
            return None;
        };
        let [spec] = creations.as_mut_slice() else {
            return None;
        };
        let mut program = spec.definition.program().clone();
        let bonus = NumericExpression::count(PermanentSelector {
            controller: ControllerSelector::SourceController,
            required_types: [CardType::Artifact].into(),
            ..Default::default()
        });
        super::army_grants::add_battlefield_ability(
            &mut program,
            PermanentSelector {
                controller: ControllerSelector::SourceItself,
                ..Default::default()
            },
            StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                power: bonus.clone(),
                toughness: bonus,
            })]),
        )?;
        spec.definition = spellwake_engine::token::TokenDefinition::new(
            program,
            spec.definition.name().clone(),
            vocabulary,
        )
        .ok()?;
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens { creations }],
        ));
    }
    if text
        == "its controller may draw a card if its power is greater than each other creature's power."
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::HighestEntrantController,
                    instructions: 1,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_resolving_player(
                    ResolvingPlayer::HighestEntrantController,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ],
        ));
    }
    if text == "add {R} for each 1 life your opponents have lost this turn." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                    ResolvingPlayer::Controller,
                    spellwake_engine::program::EffectManaYield::Counted {
                        kind: spellwake_engine::mana::ManaType::Red,
                        amount: spellwake_engine::program::ValueExpr::PlayerStat(
                            spellwake_engine::program::PlayerStatExpr::OpponentsLifeLostThisTurn,
                        ),
                    },
                ),
            ))],
        ));
    }
    if let Some((source, damage)) = text
        .strip_prefix("you may have ")
        .and_then(|s| s.strip_suffix(" damage to each creature."))
        .and_then(|s| s.split_once(" deal "))
        && (source == name || source == "it")
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                        spellwake_engine::program::DamageRecipientsRef::Group(
                            spellwake_engine::program::DamageGroup::Creatures,
                        ),
                    )),
                    spellwake_engine::program::ValueExpr::literal(amount(damage)?),
                    false,
                ),
            ],
        ));
    }
    if text
        == "choose an opponent. If that player has more cards in hand than you, draw cards equal to the difference."
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::PromptChoice(Box::new(spellwake_engine::toolbox::PromptChoiceSpec {
                    chooser: spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    domain: spellwake_engine::toolbox::PromptDomain::Players {
                        selector: spellwake_engine::toolbox::PlayerSelector::Group(
                            spellwake_engine::program::AffectedPlayers::EachOpponent,
                        ),
                        min: 1,
                        max: 1,
                        random: false,
                    },
                    category: spellwake_engine::toolbox::ChoiceCategory::Generic,
                    bind_to: spellwake_engine::toolbox::BindingId(0),
                    bind_unchosen: None,
                    tag_partition: None,
                    auto_resolve_trivial: true,
                    option_branches: Vec::new(),
                })),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Sub(
                        Box::new(spellwake_engine::program::ValueExpr::QueryCount(Box::new(
                            spellwake_engine::toolbox::ObjectQuery::zone(
                                spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                                    spellwake_engine::toolbox::PlayerSelector::Bound(
                                        spellwake_engine::toolbox::BindingId(0),
                                    ),
                                )),
                                spellwake_engine::program::ObjectFilter::any(),
                            ),
                        ))),
                        Box::new(spellwake_engine::program::ValueExpr::PlayerStat(
                            spellwake_engine::program::PlayerStatExpr::PlayerHandSize(
                                ResolvingPlayer::Controller,
                            ),
                        )),
                    ),
                ),
            ],
        ));
    }
    if let Some(kinds) = text
        .strip_prefix("that player may put an ")
        .and_then(|s| s.strip_suffix(" card from their hand onto the battlefield."))
    {
        let types = kinds
            .replace(", or ", ", ")
            .split(", ")
            .map(|kind| match kind {
                "artifact" => Some(CardType::Artifact),
                "creature" => Some(CardType::Creature),
                "land" => Some(CardType::Land),
                _ => None,
            })
            .collect::<Option<BTreeSet<_>>>()?;
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::put_hand_permanents_onto_battlefield(
                    ResolvingPlayer::UniqueRelatedPlayer,
                    types,
                    1,
                    false,
                ),
            ))],
        ));
    }
    if let Some(count)=text.strip_prefix("exile the top ").and_then(|s|s.strip_suffix(" cards of your library. Until end of turn, you may cast noncreature spells from among those cards without paying their mana costs.")) {
        let count: i64 = match count {"two"=>2,"three"=>3,"four"=>4,"five"=>5,_=>return None};
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_library_top(
                    ResolvingPlayer::Controller,
                    spellwake_engine::program::ValueExpr::literal(count),
                    false,
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::grant_play_permission(
                    spellwake_engine::toolbox::ObjectQuery::filtered(
                        spellwake_engine::toolbox::ObjectQuery::LastMoved,
                        ObjectFilter::from_clause(
                            spellwake_engine::program::ObjectClause::any()
                                .noncreature()
                                .nonland(),
                        ),
                    ),
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn,
                    true,
                    true,
                    false,
                ),
            ],
        ));
    }
    if text
        == "manifest a number of cards from the top of your library equal to the number of counters on it."
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::manifest_top(
                    ResolvingPlayer::Controller,
                    spellwake_engine::program::ValueExpr::counters(
                        spellwake_engine::program::CounterSubjectRef::DyingSource,
                        None,
                    ),
                ),
            ))],
        ));
    }
    if let Some(kind) = text
        .strip_prefix(
            "return it to its owner's hand. If you do, you may search your library for a ",
        )
        .and_then(|s| s.strip_suffix(" card, put it onto the battlefield tapped, then shuffle."))
    {
        use spellwake_engine::program::{Comparison, LibrarySearch, ResolutionCondition};
        let subtype = spellwake_engine::land::LandType::from_wire_label(kind)?;
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::BranchCondition { condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(Box::new(ResolutionCondition {
                        left: spellwake_engine::program::ValueExpr::event(
                            spellwake_engine::program::EventValueKind::TriggerSourceOnBattlefield,
                        ),
                        comparison: Comparison::Equal,
                        right: spellwake_engine::program::ValueExpr::Literal(1.into()),
                    }))), destination: 4 },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::return_source_to_hand(),
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                Instruction::SearchLibrary {
                    filter: Box::new(LibrarySearch::land_subtypes_to_battlefield(
                        [subtype].into(),
                        true,
                    )),
                },
            ],
        ));
    }
    if text == "search your library for up to three instant cards, exile them, then shuffle." {
        use spellwake_engine::program::{
            LibrarySearch, SearchCardMatcher, SearchCount, SearchDestination, SearchExileLink,
        };
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::SearchLibrary {
                filter: Box::new(
                    LibrarySearch::to_hand(SearchCardMatcher::CardTypes(
                        [CardType::Instant].into(),
                    ))
                    .with_count(SearchCount::UpTo(3))
                    .with_destination(SearchDestination::Exile {
                        link: SearchExileLink::LinkedToTriggerSource,
                    })
                    .with_reveal(false),
                ),
            }],
        ));
    }
    if text
        .strip_prefix("you may search your library for an ")
        .and_then(|s| s.strip_suffix(", exile it, then shuffle."))
        == Some("artifact card")
    {
        use spellwake_engine::program::{
            LibrarySearch, SearchCardMatcher, SearchDestination, SearchExileLink,
        };
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                Instruction::SearchLibrary {
                    filter: Box::new(
                        LibrarySearch::to_hand(SearchCardMatcher::CardTypes(
                            [CardType::Artifact].into(),
                        ))
                        .with_destination(SearchDestination::Exile {
                            link: SearchExileLink::LinkedToTriggerSource,
                        })
                        .with_reveal(false),
                    ),
                },
            ],
        ));
    }
    if text == "it deals that much damage to each creature that player controls." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Objects(
                        spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::BattlefieldControlledBy(
                                Box::new(spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    ResolvingPlayer::UniqueRelatedPlayer,
                                )),
                            ),
                            ObjectFilter::creature(),
                        ),
                    ),
                    spellwake_engine::program::ValueExpr::event(
                        spellwake_engine::program::EventValueKind::CombatDamageToPlayer,
                    ),
                    false,
                ),
            ],
        ));
    }
    if text == "draw three cards if three or more creatures died this turn." {
        use spellwake_engine::program::{Comparison, ResolutionCondition};
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::BranchCondition {
                    condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
                        Box::new(ResolutionCondition {
                            left: spellwake_engine::program::ValueExpr::event(
                                spellwake_engine::program::EventValueKind::CreaturesDiedThisTurn,
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: spellwake_engine::program::ValueExpr::Literal(3.into()),
                        }),
                    )),
                    destination: 2,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::Literal(3.into()),
                ),
            ],
        ));
    }
    if text == "you gain life and draw cards equal to its power." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::object_stat(
                        spellwake_engine::program::ValueObjectRef::DyingSource,
                        spellwake_engine::program::ObjectStatKind::Power,
                    ),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                    spellwake_engine::program::ValueExpr::object_stat(
                        spellwake_engine::program::ValueObjectRef::DyingSource,
                        spellwake_engine::program::ObjectStatKind::Power,
                    ),
                ),
            ],
        ));
    }
    if text == "untap all lands you control." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::set_group_tapped(
                    ObjectFilter::land().controlled(),
                    false,
                ),
            ],
        ));
    }
    if text
        == "until end of turn, this enchantment becomes a legendary 6/6 Nightmare God creature with menace and deathtouch. It's still an enchantment."
    {
        use spellwake_engine::characteristics::{
            AbilityChange, InstructionSubtypeOperand, PowerToughness, ResolvedDuration,
            ResolvedPartSpec, ResolvedPartsSpec, SubtypeOperation, TypeChangeSpec, TypeOperation,
        };
        let parts = ResolvedPartsSpec(vec![
            ResolvedPartSpec::Type(TypeChangeSpec {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Add([Supertype::Legendary].into()),
                types: TypeOperation::Add([CardType::Creature].into()),
                subtypes: SubtypeOperation::Add(InstructionSubtypeOperand::Literal(
                    [
                        vocabulary.resolve_label("Nightmare")?,
                        vocabulary.resolve_label("God")?,
                    ]
                    .into(),
                )),
            }),
            ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                KeywordAbility::Menace,
                KeywordAbility::Deathtouch,
            ])),
            ResolvedPartSpec::SetPT(PowerToughness {
                power: 6.into(),
                toughness: 6.into(),
            }),
        ]);
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Source,
                    parts,
                    ResolvedDuration::UntilEndOfTurn,
                ),
            ],
        ));
    }
    if let Some(subtype) = text
        .strip_prefix("destroy all non-")
        .and_then(|s| s.strip_suffix(" creatures."))
    {
        let kind = vocabulary.resolve_label(subtype)?;
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::destroy_group(
                    ObjectFilter::creature().without_subtype(kind),
                    true,
                ),
            ],
        ));
    }
    if text.ends_with('"')
        && let Some(template) = text.strip_prefix("create ")
        && let Some(create) = create_tokens(template, vocabulary)
    {
        return Some(ExecutableBody::new(BodyTarget::None, vec![create]));
    }
    if text.eq_ignore_ascii_case("investigate.") {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![create_tokens("a Clue token", vocabulary)?],
        ));
    }
    if let Some((action, gain)) = text
        .strip_suffix('.')
        .and_then(|s| s.split_once(" and you gain "))
    {
        let gain = amount(gain.strip_suffix(" life")?)?;
        let first = if let Some(template) = action.strip_prefix("create ") {
            Some(create_tokens(template, vocabulary)?)
        } else {
            action.strip_prefix("mill ").and_then(cards).map(|amount| {
                Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::mill(
                        ResolvingPlayer::Controller,
                        amount,
                    ),
                ))
            })
        };
        if let Some(first) = first {
            return Some(ExecutableBody::new(
                BodyTarget::None,
                vec![
                    first,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::literal(gain),
                    ),
                ],
            ));
        }
    }
    if let Some(exile) = text
        .strip_prefix("defending player exiles the top ")
        .and_then(|rest| rest.strip_suffix(" cards of their library."))
    {
        let amount = u16::try_from(amount(exile)?).ok()?;
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_library_top(
                    ResolvingPlayer::UniqueRelatedPlayer,
                    spellwake_engine::program::ValueExpr::literal(i64::from(amount)),
                    false,
                ),
            ],
        ));
    }
    if let Some(amount) = text
        .strip_prefix("defending player exiles ")
        .and_then(|s| s.strip_suffix(" permanents they control."))
        .and_then(count)
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_chosen_permanents(
                    AffectedPlayers::One(ResolvingPlayer::UniqueRelatedPlayer),
                    amount,
                    PermanentTypeFilter {
                        any_types: Default::default(),
                        required_types: BTreeSet::new(),
                        forbidden_types: BTreeSet::new(),
                        object_filter: None,
                    },
                ),
            ],
        ));
    }
    if text == "scry X, where X is the number of planeswalkers you control." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::scry(
                    ResolvingPlayer::Controller,
                    spellwake_engine::program::ValueExpr::controlled_type(CardType::Planeswalker),
                ),
            ))],
        ));
    }
    if let Some(limit) = text
        .strip_prefix("you may return target creature card with mana value ")
        .and_then(|s| s.strip_suffix(" or less from your graveyard to your hand."))
        .and_then(|s| s.parse::<u32>().ok())
    {
        return Some(ExecutableBody::new(
            BodyTarget::FilteredOwnGraveyardCard(
                ObjectFilter::creature().with_mana_value(ManaValueFilter::AtMost(limit)),
            ),
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand(),
            ],
        ));
    }
    if text == "other creatures you control get +1/+0 and gain menace until end of turn." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                    EffectSubject::Matching(
                        ObjectFilter::creature()
                            .controlled()
                            .other_than_ability_source(),
                    ),
                    spellwake_engine::program::ValueExpr::literal(1),
                    spellwake_engine::program::ValueExpr::literal(0),
                    [KeywordAbility::Menace].into(),
                    BTreeSet::new(),
                ),
            ],
        ));
    }
    if text == "put a +1/+1 counter on each other creature you control." {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                    spellwake_engine::toolbox::ObjectQuery::zone(
                        spellwake_engine::toolbox::QueryZone::Battlefield,
                        ObjectFilter::creature()
                            .controlled()
                            .other_than_ability_source(),
                    ),
                    spellwake_engine::effects::CounterKind::PlusOne,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ],
        ));
    }
    if let Some((description, source)) = text
        .strip_prefix("create ")
        .and_then(|s| s.strip_suffix('.'))
        .and_then(|s| s.split_once(" for each Equipment attached to "))
    {
        if source != name
            && Some(source) != name.split_once(',').map(|(short, _)| short)
            && source != "this creature"
        {
            return None;
        }
        let Instruction::CreateTokens { mut creations } = create_tokens(description, vocabulary)?
        else {
            return None;
        };
        let [token] = creations.as_mut_slice() else {
            return None;
        };
        if token.amount != spellwake_engine::program::ValueExpr::Literal(1.into()) {
            return None;
        }
        token.amount = spellwake_engine::program::ValueExpr::AttachedCount(
            spellwake_engine::program::AttachedCountKind::EquipmentOnSource,
        );
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens { creations }],
        ));
    }
    if let Some(recipient) = text
        .strip_prefix("it deals that much damage to ")
        .or_else(|| text.strip_prefix("this creature deals that much damage to "))
        .and_then(|s| s.strip_suffix('.'))
    {
        let target = match recipient {
            "target player or planeswalker" => BodyTarget::SinglePlayerOrPlaneswalker,
            "any target" => BodyTarget::SingleAnyTarget,
            "target player" => BodyTarget::SinglePlayer,
            _ => return None,
        };
        return Some(ExecutableBody::new(
            target,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                    spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    spellwake_engine::program::ValueExpr::event(
                        spellwake_engine::program::EventValueKind::DamageReceived,
                    ),
                    false,
                ),
            ],
        ));
    }
    if text
        == "you may exile target creature an opponent controls until this creature leaves the battlefield."
    {
        return Some(ExecutableBody::new(
            BodyTarget::FilteredPermanent(ObjectFilter::creature().opposing()),
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::exile_target_until_source_leaves(),
            ],
        ));
    }
    if let Some((draw, discard)) = text
        .strip_prefix("you may draw ")
        .and_then(|s| s.strip_suffix('.'))
        .and_then(|s| s.split_once(". If you do, discard "))
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 2,
                },
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(cards(
                    draw,
                )?),
                Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                    AffectedPlayers::One(ResolvingPlayer::Controller),
                    cards(discard)?,
                ))),
            ],
        ));
    }
    if let Some(template) = text
        .strip_prefix("you may create ")
        .and_then(|s| s.strip_suffix('.'))
    {
        let create = super::ordinary_text::create_tokens(template, vocabulary)?;
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                create,
            ],
        ));
    }
    if let Some(template) = text
        .strip_prefix("create ")
        .and_then(|s| s.strip_suffix(" for each +1/+1 counter on this creature."))
    {
        let Instruction::CreateTokens { mut creations } =
            super::ordinary_text::create_tokens(template, vocabulary)?
        else {
            return None;
        };
        let [creation] = creations.as_mut_slice() else {
            return None;
        };
        if creation.amount != spellwake_engine::program::ValueExpr::Literal(1.into()) {
            return None;
        }
        creation.amount = spellwake_engine::program::ValueExpr::counters(
            spellwake_engine::program::CounterSubjectRef::TriggerSource,
            Some(spellwake_engine::effects::CounterKind::PlusOne),
        );
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens { creations }],
        ));
    }
    if let Some(amount) = text
        .strip_prefix("you may mill ")
        .and_then(|text| text.strip_suffix('.'))
        .and_then(cards)
    {
        return Some(ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
                Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::mill(
                        ResolvingPlayer::Controller,
                        amount,
                    ),
                )),
            ],
        ));
    }
    if let Some(remainder) = text.strip_prefix("you may ") {
        if let Some(mut search) = super::basic_land_search::body(remainder)
            && matches!(search.instructions.as_slice(), [Instruction::SearchLibrary { filter }] if filter.supports_optional_wrapper())
        {
            search.instructions.insert(
                0,
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 1,
                },
            );
            return Some(search);
        }
        let mut effect = spells::ordinary_effects::body(remainder, name)?;
        if !matches!(
            effect.instructions.as_slice(),
            [Instruction::DrawCards(_) | Instruction::MutateObject(_) | Instruction::ZoneOp(_)]
        ) {
            return None;
        }
        effect.instructions.insert(
            0,
            Instruction::OptionalSequence {
                chooser: ResolvingPlayer::Controller,
                instructions: 1,
            },
        );
        return Some(effect);
    }
    let mut normalized = match name
        .split_once(',')
        .and_then(|(short, _)| text.strip_prefix(short))
        .filter(|tail| tail.starts_with(" deals "))
    {
        Some(tail) => format!("{name}{tail}"),
        None => text.to_owned(),
    };
    normalized.get_mut(..1)?.make_ascii_uppercase();
    if let Some(search) = super::basic_land_search::body(&normalized) {
        return Some(search);
    }
    if let Some(body) = spells::ordinary_effects::body(&normalized, name) {
        return Some(body);
    }
    let raw = text.strip_suffix('.')?;
    let player = ResolvingPlayer::Controller;
    let self_cards = AffectedPlayers::One(player);
    let (target, instructions) = if let Some(rest) = raw.strip_prefix("each opponent loses ") {
        let (loss, gain) = if let Some((loss, gain)) = rest.split_once(" life and you gain ") {
            (loss, Some(gain))
        } else {
            (rest.strip_suffix(" life")?, None)
        };
        let loss_amt = amount(loss)?;
        let ops = match gain {
            Some(gain) => {
                let gain_amt = amount(gain.strip_suffix(" life")?)?;
                if gain_amt == loss_amt {
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::drain_life(
                            spellwake_engine::toolbox::PlayerSelector::Group(
                                AffectedPlayers::EachOpponent,
                            ),
                            spellwake_engine::program::ValueExpr::literal(loss_amt),
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            false,
                        ),
                    ]
                } else {
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                            spellwake_engine::toolbox::PlayerSelector::Group(
                                AffectedPlayers::EachOpponent,
                            ),
                            spellwake_engine::program::ValueExpr::literal(loss_amt),
                        ),
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::Controller,
                            ),
                            spellwake_engine::program::ValueExpr::literal(gain_amt),
                        ),
                    ]
                }
            }
            None => vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                    spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::EachOpponent),
                    spellwake_engine::program::ValueExpr::literal(loss_amt),
                ),
            ],
        };
        (BodyTarget::None, ops)
    } else if let Some(rest) = raw
        .strip_prefix("this creature deals ")
        .or_else(|| raw.strip_prefix("it deals "))
        .or_else(|| {
            name.split_once(',')
                .and_then(|(short, _)| raw.strip_prefix(short)?.strip_prefix(" deals "))
        })
        .or_else(|| raw.strip_prefix(name)?.strip_prefix(" deals "))
    {
        let (damage, gain) = if let Some((damage, gain)) =
            rest.split_once(" damage to each opponent and you gain ")
        {
            (damage, Some(gain))
        } else {
            (rest.strip_suffix(" damage to each opponent")?, None)
        };
        let mut instructions = vec![
            crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                spellwake_engine::toolbox::DamageTargetQuery::Relative(Box::new(
                    spellwake_engine::program::DamageRecipientsRef::Group(
                        spellwake_engine::program::DamageGroup::Opponents,
                    ),
                )),
                spellwake_engine::program::ValueExpr::literal(amount(damage)?),
                false,
            ),
        ];
        if let Some(gain) = gain {
            instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::literal(amount(
                        gain.strip_suffix(" life")?,
                    )?),
                ),
            );
        }
        (BodyTarget::None, instructions)
    } else if raw == "put a +1/+1 counter on this creature"
        || raw
            .strip_prefix("put a +1/+1 counter on ")
            .is_some_and(|who| {
                who == name || Some(who) == name.split_once(',').map(|(short, _)| short)
            })
    {
        (
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                    spellwake_engine::effects::CounterKind::PlusOne,
                    spellwake_engine::program::ValueExpr::literal(1),
                ),
            ],
        )
    } else if let Some((cost, discarded)) = raw
        .strip_prefix("draw ")
        .and_then(|s| s.split_once(", then discard "))
    {
        (
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(cards(
                    cost,
                )?),
                Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::discard(
                    self_cards,
                    cards(discarded)?,
                ))),
            ],
        )
    } else if matches!(
        raw,
        "each player sacrifices a creature of their choice" | "each opponent sacrifices a creature"
    ) {
        (
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                    if raw.starts_with("each opponent") {
                        AffectedPlayers::EachOpponent
                    } else {
                        AffectedPlayers::EachPlayer
                    },
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                    PermanentTypeFilter {
                        any_types: Default::default(),
                        required_types: BTreeSet::from([CardType::Creature]),
                        forbidden_types: BTreeSet::new(),
                        object_filter: None,
                    },
                ),
            ))],
        )
    } else if let Some(n) = raw.strip_prefix("draw ") {
        (
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(cards(n)?),
            ],
        )
    } else if let Some(n) = raw.strip_prefix("mill ") {
        (
            BodyTarget::None,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::mill(player, cards(n)?),
            ))],
        )
    } else if let Some(n) = raw.strip_prefix("surveil ") {
        (
            BodyTarget::None,
            vec![Instruction::MutatePlayer(Box::new(
                spellwake_engine::toolbox::MutatePlayerSpec::surveil(player, count(n)?),
            ))],
        )
    } else if let Some(n) = raw.strip_prefix("discard ") {
        (
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::discard(self_cards, cards(n)?),
            ))],
        )
    } else if let Some(n) = raw.strip_prefix("put ").and_then(|s| {
        s.strip_suffix(" +1/+1 counter on target creature")
            .or_else(|| s.strip_suffix(" +1/+1 counters on target creature"))
    }) {
        (
            BodyTarget::SingleCreature,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                    spellwake_engine::effects::CounterKind::PlusOne,
                    spellwake_engine::program::ValueExpr::Literal(amount(n)?.into()),
                ),
            ],
        )
    } else {
        let line = raw.strip_prefix("create ")?;
        (BodyTarget::None, vec![create_tokens(line, vocabulary)?])
    };
    Some(ExecutableBody::new(target, instructions))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod watcher_games;

fn modal_body(
    effect: &str,
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ExecutableBody> {
    let one = || spellwake_engine::program::ValueExpr::Literal(1.into());
    let creature = || PermanentTypeFilter {
        required_types: [CardType::Creature].into(),
        any_types: BTreeSet::new(),
        forbidden_types: BTreeSet::new(),
        object_filter: None,
    };
    let (target, ops) = match effect {
        "Put a +1/+1 counter on each creature you control. Those creatures gain vigilance until end of turn." => {
            (
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
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                        EffectSubject::Matching(ObjectFilter::creature().controlled()),
                        spellwake_engine::program::ValueExpr::literal(0),
                        spellwake_engine::program::ValueExpr::literal(0),
                        [KeywordAbility::Vigilance].into(),
                        BTreeSet::new(),
                    ),
                ],
            )
        }
        "Each player discards a card." => (
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::discard(AffectedPlayers::EachPlayer, one()),
            ))],
        ),
        "Each player sacrifices a creature of their choice." => (
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                    AffectedPlayers::EachPlayer,
                    one(),
                    creature(),
                ),
            ))],
        ),
        "Each player loses 1 life and draws a card." => (
            BodyTarget::None,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                    spellwake_engine::toolbox::PlayerSelector::Group(AffectedPlayers::EachPlayer),
                    one(),
                ),
                crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_each_player(one()),
            ],
        ),
        "Target opponent sacrifices a creature of their choice." => (
            BodyTarget::SingleOpponent,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    one(),
                    creature(),
                ),
            ))],
        ),
        "Target opponent discards two cards." => (
            BodyTarget::SingleOpponent,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::discard(
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    spellwake_engine::program::ValueExpr::Literal(2.into()),
                ),
            ))],
        ),
        "Target opponent loses 5 life." => (
            BodyTarget::SingleOpponent,
            vec![
                crate::keyword_creatures::oracle_lowering::toolbox_emit::lose_life(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        ResolvingPlayer::TargetPlayer,
                    ),
                    spellwake_engine::program::ValueExpr::literal(5),
                ),
            ],
        ),
        _ => {
            let mut lower = effect.to_owned();
            lower.get_mut(..1)?.make_ascii_lowercase();
            return body(&lower, name, vocabulary);
        }
    };
    Some(ExecutableBody::new(target, ops))
}
