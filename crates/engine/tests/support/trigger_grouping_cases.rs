//! Actual root grouping/resolution cases reuse the simultaneous-birth trajectory.
use super::*;
use spellwake_engine::{
    occurrence::{TriggerCauseKey, WaitingInstance},
    program::{DefinitionError, TriggerGrouping},
};

fn discoveries(candidate: &NativeCandidate) -> Vec<&WaitingInstance> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerDiscovered(value) => Some(value.as_ref()),
            _ => None,
        })
        .collect()
}
fn grouped_watcher(grouping: TriggerGrouping, body: Vec<Instruction>) -> Program {
    let mut program = Program::artifact(None, vec![]);
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(7),
        condition: OrdinaryCondition::Enters(ObjectRelation::AnyCreature),
        grouping,
        body: ExecutableBody::new(BodyTarget::None, body),
    });
    program
}
fn draw_count() -> Instruction {
    Instruction::DrawController {
        amount: InstructionAmount::MatchedOccurrences,
    }
}

#[test]
fn mixed_publication_filters_members_before_grouping_and_empty_match_has_no_group() {
    for include_creature in [false, true] {
        let mut groups = vec![group(
            template(Program::artifact(None, vec![])),
            2,
            TokenRecipients::Controller,
        )];
        if include_creature {
            groups.push(group(creature(), 1, TokenRecipients::Controller));
        }
        let game = start(
            vec![create(groups)],
            vec![card(
                2,
                grouped_watcher(TriggerGrouping::OneOrMoreTogether, vec![draw_count()]),
                true,
            )],
        );
        let pending = resolve(&cast(&game));
        let committed = order(pending.state(), false);
        assert_eq!(
            births(&committed).len(),
            if include_creature { 3 } else { 2 }
        );
        let captures = discoveries(&committed);
        if !include_creature {
            assert!(captures.is_empty());
            assert!(committed.state().execution().window().entries().is_empty());
            continue;
        }
        assert_eq!(captures.len(), 1);
        let capture = captures[0];
        assert_eq!(capture.causes.key(), TriggerCauseKey::Together);
        assert_eq!(capture.causes.len(), 1);
        let member = &capture.causes.members()[0];
        assert_eq!(member.related_player, Some(P0));
        assert_eq!(member.occurrence.event, capture.causes.event());
        assert_eq!(member.occurrence.index, 2);
        let TriggerRelated::TokenCreated { after, .. } = &member.related else {
            panic!("birth");
        };
        assert!(after.types.contains(&CardType::Creature));
        assert_eq!(after.object.id(), births(&committed)[2].object.id());
    }
}

#[test]
fn copied_grouping_uses_current_text_and_excludes_retained_removed_roster_entries() {
    use spellwake_engine::{
        activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
        characteristics::{AbilityChange, AbilityRemoval, ResolvedDuration, ResolvedPartSpec},
        mana::ManaCost,
        program::PlayerRelation,
        targets::TargetRef,
    };
    for remove_receiver in [false, true] {
        let receiver = Program::artifact(
            None,
            vec![ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SinglePermanent,
                    vec![Instruction::CopyTargetToSource {
                        duration: ResolvedDuration::UntilEndOfTurn,
                        power_toughness: None,
                    }],
                ),
            }],
        );
        let mut donor = Program::creature(None, 1.into(), 1.into(), BTreeSet::new());
        donor.triggered =
            grouped_watcher(TriggerGrouping::OneOrMoreTogether, vec![draw_count()]).triggered;
        donor.triggered[0].condition =
            OrdinaryCondition::TokenCreated(PlayerRelation::AnyParticipant);
        let game = start(
            vec![create(vec![group(
                creature(),
                2,
                TokenRecipients::Controller,
            )])],
            vec![
                card(2, receiver, true),
                card(3, donor, true),
                spell(
                    4,
                    BodyTarget::SinglePermanent,
                    vec![trajectories::effect(
                        vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                            AbilityRemoval::All,
                        ))],
                        ResolvedDuration::UntilEndOfTurn,
                    )],
                ),
            ],
        );
        let receiver_id = object(&game, 2);
        let donor_id = object(&game, 3);
        let activated = submit(
            &game,
            P0,
            GameCommand::ActivateAbility {
                source: receiver_id,
                ability: ActivatedAbilityId(1),
                target: TargetRef::Object(donor_id),
                payment: None,
            },
        );
        let copied = resolve(activated.state());
        let copied_text = trajectories::current(copied.state(), receiver_id);
        assert_eq!(
            copied_text.printed_text.triggered[0].grouping,
            TriggerGrouping::OneOrMoreTogether
        );
        assert_eq!(copied_text.abilities.triggered, vec![TriggeredAbilityId(7)]);
        let ready = if remove_receiver {
            let (cast_removal, _) = trajectories::cast(copied.state(), 4, receiver_id);
            resolve(cast_removal.state()).into_state()
        } else {
            copied.into_state()
        };
        let current = trajectories::current(&ready, receiver_id);
        assert_eq!(
            current.printed_text.triggered[0].grouping,
            TriggerGrouping::OneOrMoreTogether
        );
        assert_eq!(current.abilities.triggered.is_empty(), remove_receiver);
        let pending = resolve(&cast(&ready));
        let committed = order(pending.state(), false);
        let captures = discoveries(&committed);
        let expected = if remove_receiver {
            BTreeSet::from([donor_id])
        } else {
            BTreeSet::from([receiver_id, donor_id])
        };
        assert_eq!(
            captures
                .iter()
                .map(|w| w.source.object.id())
                .collect::<BTreeSet<_>>(),
            expected
        );
        for capture in captures {
            assert_eq!(capture.definition, TriggeredAbilityId(7));
            assert_eq!(capture.controller, P0);
            assert_eq!(capture.causes.key(), TriggerCauseKey::Together);
            assert_eq!(capture.causes.len(), 2);
        }
    }
}

#[test]
fn simultaneous_deaths_keep_complete_before_causes_after_watcher_itself_dies() {
    use spellwake_engine::{
        characteristics::ControllerSelector, mana::ManaCost, object_identity::ObjectZone,
    };
    let p2 = SeatId(2);
    let mut watcher = Program::creature(None, 1.into(), 1.into(), BTreeSet::new());
    watcher.triggered =
        grouped_watcher(TriggerGrouping::OneOrMoreTogether, vec![draw_count()]).triggered;
    watcher.triggered[0].condition = OrdinaryCondition::Dies(ObjectRelation::AnyCreature);
    let mut shrinker = Program::enchantment(Some(ManaCost::free()));
    shrinker.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            controller: ControllerSelector::Any,
            required_types: BTreeSet::from([CardType::Creature]),
            ..Default::default()
        },
        parts: StaticParts(vec![trajectories::static_set(0, 0)]),
    });
    let mut cards = vec![
        card(1, shrinker, false),
        card(2, watcher, true),
        trajectories::controlled(3, P1, P1, creature().program().clone()),
        trajectories::controlled(4, p2, p2, creature().program().clone()),
    ];
    cards.extend((10..15).map(|id| trajectories::library(id, P0, &vocabulary())));
    let game = root(cards, vec![P0, P1, p2], P0, vec![]);
    let dead_ids = [object(&game, 2), object(&game, 3), object(&game, 4)];
    let committed = resolve(&cast(&game));
    let captures = discoveries(&committed);
    assert_eq!(captures.len(), 1);
    let capture = captures[0];
    assert_eq!(capture.source.object.id(), dead_ids[0]);
    assert_eq!(capture.source.object.controller(), Some(P0));
    assert_eq!(capture.controller, P0);
    assert_eq!(capture.causes.key(), TriggerCauseKey::Together);
    assert_eq!(capture.causes.len(), 3);
    let mut seen = BTreeMap::new();
    for member in capture.causes.members() {
        assert_eq!(member.occurrence.event, capture.causes.event());
        let TriggerRelated::ZoneChange { before, after } = &member.related else {
            panic!("death");
        };
        assert_eq!(before.object.zone(), ObjectZone::Battlefield);
        assert_eq!(
            after.object.zone(),
            ObjectZone::Graveyard(before.object.owner())
        );
        assert_eq!(member.related_player, before.object.controller());
        seen.insert(before.object.id(), member.related_player.unwrap());
        assert!(
            committed
                .state()
                .execution()
                .window()
                .cards()
                .get(before.object.id())
                .is_none()
        );
    }
    assert_eq!(
        seen,
        BTreeMap::from([(dead_ids[0], P0), (dead_ids[1], P1), (dead_ids[2], p2)])
    );
    let entry = committed.state().execution().window().entries()[0].id();
    assert_eq!(
        committed
            .state()
            .execution()
            .execution_bindings(entry)
            .unwrap()
            .trigger
            .as_ref()
            .unwrap()
            .causes,
        capture.causes
    );
    let resolved = resolve(committed.state());
    assert_eq!(resolved.draw_journal().events.len(), 3);
    assert!(resolved.state().execution().window().entries().is_empty());
}

#[test]
fn grouped_instances_from_two_source_controllers_are_placed_in_apnap_cohorts() {
    let watcher = grouped_watcher(
        TriggerGrouping::OneOrMoreByPlayer,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
    );
    let game = root(
        vec![
            spell(
                1,
                BodyTarget::None,
                vec![create(vec![group(
                    creature(),
                    2,
                    TokenRecipients::EachPlayer,
                )])],
            ),
            card(2, watcher.clone(), true),
            trajectories::controlled(3, P1, P1, watcher),
        ],
        vec![P0, P1],
        P1,
        vec![],
    );
    // The non-first participant is active; P0 receives priority to cast its instant.
    let priority = submit(&game, P1, GameCommand::Pass);
    let pending = resolve(&cast(priority.state()));
    let first = order(pending.state(), false);
    let second = order(first.state(), false);
    let captures = discoveries(&second);
    assert_eq!(captures.len(), 4);
    let expected = captures
        .iter()
        .map(|w| (w.id, (w.controller, w.causes.clone())))
        .collect::<BTreeMap<_, _>>();
    for capture in &captures {
        assert_eq!(capture.causes.len(), 2);
        let TriggerCauseKey::Player(player) = capture.causes.key() else {
            panic!("player group");
        };
        assert!(
            capture
                .causes
                .members()
                .iter()
                .all(|m| m.related_player == Some(player))
        );
    }
    let mut current = second.into_state();
    let mut chosen = vec![];
    let mut placements = vec![];
    for actor in [P1, P0] {
        let RuleFrame::Triggers(frame) = current.pending_decision().unwrap().frame() else {
            panic!("APNAP cohort");
        };
        assert_eq!(frame.chooser(), actor);
        let mut ids = frame.instances().to_vec();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().all(|id| expected[id].0 == actor));
        ids.reverse();
        chosen.extend(ids.iter().copied());
        let placed = reply(&current, actor, DecisionReply::OrderTriggers(ids));
        placements.extend(placed.rules_journal().records.iter().filter_map(
            |record| match record {
                RulesRecord::TriggerPlaced(value) => Some(value.clone()),
                _ => None,
            },
        ));
        current = placed.into_state();
    }
    let entries = current.execution().window().entries();
    assert_eq!(
        entries.iter().map(|e| e.controller()).collect::<Vec<_>>(),
        vec![P1, P1, P0, P0]
    );
    assert_eq!(
        placements.iter().map(|p| p.instance).collect::<Vec<_>>(),
        chosen
    );
    assert_eq!(
        placements.iter().map(|p| p.entry).collect::<Vec<_>>(),
        entries.iter().map(|e| e.id()).collect::<Vec<_>>()
    );
    assert!(
        placements
            .windows(2)
            .all(|pair| pair[0].wave == pair[1].wave)
    );
    for ((entry, instance), receipt) in entries.iter().zip(chosen).zip(placements) {
        let binding = current
            .execution()
            .execution_bindings(entry.id())
            .unwrap()
            .trigger
            .as_ref()
            .unwrap();
        assert_eq!(binding.instance, instance);
        assert_eq!(binding.causes, expected[&instance].1);
        assert_eq!(entry.controller(), expected[&instance].0);
        assert_eq!(receipt.controller, entry.controller());
        assert_eq!(receipt.instance, binding.instance);
    }
}

#[test]
fn together_retains_both_real_births_in_one_installed_trigger() {
    let watcher = grouped_watcher(
        TriggerGrouping::OneOrMoreTogether,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
    );
    let game = start(
        vec![create(vec![group(
            creature(),
            2,
            TokenRecipients::Controller,
        )])],
        vec![card(2, watcher, true)],
    );
    let source = object(&game, 2);
    let pending = resolve(&cast(&game));
    let committed = order(pending.state(), true);
    let found = discoveries(&committed);
    assert_eq!(found.len(), 1);
    let capture = found[0];
    assert_eq!(capture.source.object.id(), source);
    assert_eq!(capture.controller, P0);
    assert_eq!(capture.causes.key(), TriggerCauseKey::Together);
    assert_eq!(capture.causes.len(), 2);
    let actual_births = births(&committed)
        .iter()
        .map(|a| a.object.id())
        .collect::<BTreeSet<_>>();
    let cause_births = capture
        .causes
        .members()
        .iter()
        .map(|member| {
            assert_eq!(member.occurrence.event, capture.causes.event());
            assert_eq!(member.related_player, Some(P0));
            let TriggerRelated::TokenCreated { after, .. } = &member.related else {
                panic!("actual birth cause");
            };
            after.object.id()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(cause_births, actual_births);
    let entry = committed.state().execution().window().entries()[0].id();
    let binding = committed
        .state()
        .execution()
        .execution_bindings(entry)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.instance, capture.id);
    assert_eq!(binding.causes, capture.causes);
    let settled = resolve(committed.state());
    assert_eq!(settled.state().execution().life_total(P0), Some(&21.into()));
}

#[test]
fn player_groups_keep_participant_keys_separate_from_one_source_controller() {
    let p2 = SeatId(2);
    let mut watcher = grouped_watcher(
        TriggerGrouping::OneOrMoreByPlayer,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
    );
    watcher.triggered[0].condition =
        OrdinaryCondition::TokenCreated(spellwake_engine::program::PlayerRelation::AnyParticipant);
    let game = root(
        vec![
            spell(
                1,
                BodyTarget::None,
                vec![create(vec![group(
                    creature(),
                    2,
                    TokenRecipients::EachPlayer,
                )])],
            ),
            card(2, watcher, true),
        ],
        vec![p2, P1, P0],
        P0,
        vec![],
    );
    let mut pending = resolve(&cast(&game));
    for _ in 0..3 {
        pending = order(pending.state(), false);
    }
    let found = discoveries(&pending);
    assert_eq!(found.len(), 3);
    assert_eq!(
        found.iter().map(|w| w.causes.key()).collect::<Vec<_>>(),
        vec![
            TriggerCauseKey::Player(p2),
            TriggerCauseKey::Player(P1),
            TriggerCauseKey::Player(P0)
        ]
    );
    for capture in found {
        assert_eq!(capture.controller, P0);
        assert_eq!(capture.causes.len(), 2);
        let TriggerCauseKey::Player(player) = capture.causes.key() else {
            panic!("participant key");
        };
        for member in capture.causes.members() {
            assert_eq!(member.related_player, Some(player));
            assert!(
                matches!(&member.related, TriggerRelated::TokenCreated {creator,..} if *creator==player)
            );
        }
    }
    let RuleFrame::Triggers(frame) = pending.state().pending_decision().unwrap().frame() else {
        panic!("real group order");
    };
    assert_eq!(frame.chooser(), P0);
    assert_eq!(frame.instances().len(), 3);
}

#[test]
fn matched_count_keeps_original_binding_across_nested_token_order_and_later_draw() {
    let watcher = grouped_watcher(
        TriggerGrouping::OneOrMoreTogether,
        vec![
            create(vec![TokenCreation {
                definition: template(Program::artifact(None, vec![])),
                amount: InstructionAmount::MatchedOccurrences,
                recipients: TokenRecipients::Controller,
            }]),
            draw_count(),
        ],
    );
    let mut others = vec![card(2, watcher, true)];
    others.extend((10..14).map(|id| trajectories::library(id, P0, &vocabulary())));
    let game = start(
        vec![create(vec![group(
            creature(),
            2,
            TokenRecipients::Controller,
        )])],
        others,
    );
    let pending = resolve(&cast(&game));
    let committed = order(pending.state(), false);
    let original = discoveries(&committed)[0].causes.clone();
    let nested = resolve(committed.state());
    assert!(matches!(
        nested.state().pending_decision().unwrap().frame(),
        RuleFrame::TimestampOrder(_)
    ));
    let current_entry = nested
        .state()
        .execution()
        .window()
        .resolving()
        .unwrap()
        .id();
    assert_eq!(
        nested
            .state()
            .execution()
            .execution_bindings(current_entry)
            .unwrap()
            .trigger
            .as_ref()
            .unwrap()
            .causes,
        original
    );
    let settled = order(nested.state(), true);
    assert_eq!(births(&settled).len(), 2);
    assert_eq!(settled.draw_journal().events.len(), 2);
    assert!(
        births(&settled)
            .iter()
            .all(|a| a.types.contains(&CardType::Artifact))
    );
    assert!(discoveries(&settled).is_empty());
    assert_eq!(original.len(), 2);
}

#[test]
fn trigger_dependent_amount_is_admitted_only_in_trigger_bodies() {
    let body = ExecutableBody::new(BodyTarget::None, vec![draw_count()]);
    assert_eq!(
        body.validate(&vocabulary()),
        Err(DefinitionError::UnboundTriggerAmount)
    );
    assert_eq!(
        Program::instant(spellwake_engine::mana::ManaCost::free(), body.clone())
            .validate(&vocabulary()),
        Err(DefinitionError::UnboundTriggerAmount)
    );
    let mut activated = Program::artifact(None, vec![]);
    activated
        .activated
        .push(spellwake_engine::activated::ActivatedAbility {
            id: spellwake_engine::activated::ActivatedAbilityId(1),
            mana_cost: spellwake_engine::mana::ManaCost::free(),
            source_cost: spellwake_engine::activated::SourceCost::None,
            body: body.clone(),
        });
    assert_eq!(
        activated.validate(&vocabulary()),
        Err(DefinitionError::UnboundTriggerAmount)
    );
    assert!(
        grouped_watcher(TriggerGrouping::OneOrMoreTogether, body.instructions)
            .validate(&vocabulary())
            .is_ok()
    );
}

#[test]
fn complete_newcomer_causes_resolve_after_sources_and_related_tokens_cease() {
    let mut newcomer = Program::creature(None, 0.into(), 0.into(), BTreeSet::new());
    newcomer.triggered =
        grouped_watcher(TriggerGrouping::OneOrMoreTogether, vec![draw_count()]).triggered;
    let libraries = (10..16)
        .map(|id| trajectories::library(id, P0, &vocabulary()))
        .collect();
    let game = start(
        vec![create(vec![group(
            template(newcomer),
            2,
            TokenRecipients::Controller,
        )])],
        libraries,
    );
    let pending = resolve(&cast(&game));
    let committed = order(pending.state(), false);
    let captures = discoveries(&committed);
    assert_eq!(captures.len(), 2);
    for capture in &captures {
        assert_eq!(capture.causes.len(), 2);
        assert!(
            committed
                .state()
                .execution()
                .window()
                .cards()
                .get(capture.source.object.id())
                .is_none()
        );
        for member in capture.causes.members() {
            let TriggerRelated::TokenCreated { after, .. } = &member.related else {
                panic!("birth");
            };
            assert!(
                committed
                    .state()
                    .execution()
                    .window()
                    .cards()
                    .get(after.object.id())
                    .is_none()
            );
        }
    }
    let RuleFrame::Triggers(frame) = committed.state().pending_decision().unwrap().frame() else {
        panic!("real ordering");
    };
    let placed = reply(
        committed.state(),
        P0,
        DecisionReply::OrderTriggers(frame.instances().to_vec()),
    );
    let first = resolve(placed.state());
    assert_eq!(first.draw_journal().events.len(), 2);
    let second = resolve(first.state());
    assert_eq!(second.draw_journal().events.len(), 2);
    assert!(second.state().execution().window().entries().is_empty());
}

#[test]
fn separate_creation_instructions_never_merge_their_trigger_cause_sets() {
    let watcher = grouped_watcher(
        TriggerGrouping::OneOrMoreTogether,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
    );
    let instruction = create(vec![group(creature(), 2, TokenRecipients::Controller)]);
    let game = start(
        vec![
            create(vec![group(creature(), 0, TokenRecipients::Controller)]),
            instruction.clone(),
            instruction,
        ],
        vec![card(2, watcher, true)],
    );
    let pending = resolve(&cast(&game));
    assert!(discoveries(&pending).is_empty());
    let first = order(pending.state(), false);
    let first_causes = discoveries(&first)[0].causes.clone();
    assert!(matches!(
        first.state().pending_decision().unwrap().frame(),
        RuleFrame::TimestampOrder(_)
    ));
    let second = order(first.state(), false);
    let second_causes = discoveries(&second)[0].causes.clone();
    assert_eq!((first_causes.len(), second_causes.len()), (2, 2));
    assert_ne!(first_causes.event(), second_causes.event());
    assert!(first_causes.members().iter().all(|m| {
        !second_causes
            .members()
            .iter()
            .any(|n| n.occurrence == m.occurrence)
    }));
    let RuleFrame::Triggers(frame) = second.state().pending_decision().unwrap().frame() else {
        panic!("two real groups");
    };
    assert_eq!(frame.instances().len(), 2);
}

#[test]
fn grouped_order_reply_rejects_wrong_actor_and_duplicate_instances_atomically() {
    let watcher = grouped_watcher(
        TriggerGrouping::OneOrMoreByPlayer,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
    );
    let game = start(
        vec![create(vec![group(
            creature(),
            2,
            TokenRecipients::EachPlayer,
        )])],
        vec![card(2, watcher, true)],
    );
    let pending = resolve(&cast(&game));
    let first = order(pending.state(), false);
    let second = order(first.state(), false);
    let state = second.state();
    let saved = state.clone();
    let decision = state.pending_decision().unwrap();
    let RuleFrame::Triggers(frame) = decision.frame() else {
        panic!("group order");
    };
    let ids = frame.instances().to_vec();
    assert_eq!(ids.len(), 2);
    for (actor, choice) in [
        (P1, DecisionReply::OrderTriggers(ids.clone())),
        (P0, DecisionReply::OrderTriggers(vec![ids[0], ids[0]])),
    ] {
        assert!(
            state
                .advance(GameInput {
                    basis: state.basis().clone(),
                    actor,
                    command: GameCommand::Reply {
                        decision: decision.id(),
                        choice
                    }
                })
                .is_err()
        );
        assert_eq!(state, &saved);
    }
}
