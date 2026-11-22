//! P01-P08: lower-owner actual cuts and explicit native malformed-context probes.
//! Overlay probes qualify participant capture, not entry replacement or full cards.
use super::*;
use crate::{
    life::{LifeAdjustment, LifeChangeKind},
    occurrence::{
        EventCause, RulesOccurrence, RulesRecord, StatusReason, TriggerRelated, TriggerState,
    },
    program::{PlayerRelation, TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId},
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const C: SeatId = SeatId(2);
fn base(conditions: &[TriggerSpec], target_destination: ObjectDestination) -> EffectWindow {
    base_with_grouping(
        conditions,
        target_destination,
        TriggerGrouping::OneOrMoreByPlayer,
    )
}
fn base_with_grouping(
    conditions: &[TriggerSpec],
    target_destination: ObjectDestination,
    grouping: TriggerGrouping,
) -> EffectWindow {
    let mut source = Program::artifact(None, vec![]);
    source.triggered = conditions
        .iter()
        .enumerate()
        .map(|(id, condition)| TriggeredAbility {
            id: TriggeredAbilityId(u64::try_from(id).unwrap()),
            condition: condition.clone(),
            grouping,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        })
        .collect();
    EffectWindow::new(GameSeed::with_uniform_life(
        vec![A, B, C],
        A,
        vec![
            SeedCard {
                card: CardId(1),
                owner: A,
                destination: ObjectDestination::Battlefield { controller: A },
                program: source,
            },
            SeedCard {
                card: CardId(2),
                owner: C,
                destination: target_destination,
                program: Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            },
        ],
        vec![CardId(1)]
            .into_iter()
            .chain(
                matches!(target_destination, ObjectDestination::Battlefield { .. })
                    .then_some(CardId(2)),
            )
            .collect(),
        vec![],
        20.into(),
        crate::combat::CombatRules::UnrestrictedFreeForAll,
        crate::test_support::creature_types::selected_vocabulary().unwrap(),
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    ))
    .unwrap()
}
fn captured(append: &RulesJournal) -> Vec<&crate::occurrence::WaitingInstance> {
    append
        .records
        .iter()
        .filter_map(|r| {
            if let RulesRecord::TriggerDiscovered(w) = r {
                Some(w.as_ref())
            } else {
                None
            }
        })
        .collect()
}
fn assert_roles(append: &RulesJournal, expected: &[SeatId]) {
    let values = captured(append);
    assert_eq!(values.len(), expected.len());
    for (capture, player) in values.into_iter().zip(expected) {
        assert_eq!(capture.controller, A);
        let wire = crate::test_support::projection::waiting(capture);
        assert!(wire.get("occurrence").is_none() && wire.get("related").is_none());
        assert_eq!(
            wire["causes"]["event"],
            capture.causes.event().sequence().to_string()
        );
        assert_eq!(
            wire["causes"]["key"],
            serde_json::json!({"tag":"Player","player":player.0})
        );
        let encoded_members = wire["causes"]["members"].as_array().unwrap();
        assert_eq!(encoded_members.len(), capture.causes.len());
        for (encoded, native) in encoded_members.iter().zip(capture.causes.members()) {
            assert_eq!(
                encoded["occurrence"]["event"],
                native.occurrence.event.sequence().to_string()
            );
            assert_eq!(
                encoded["occurrence"]["index"],
                native.occurrence.index.to_string()
            );
            assert_eq!(encoded["related_player"], native.related_player.unwrap().0);
        }
        let encoded_journal = crate::test_support::projection::journal(append);
        assert!(
            encoded_journal
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["tag"] == "TriggerDiscovered" && r["value"] == wire)
        );

        assert_eq!(capture.causes.len(), 1);
        assert_eq!(capture.causes.members()[0].related_player, Some(*player));
        assert_eq!(
            capture.causes.key(),
            crate::occurrence::TriggerCauseKey::Player(*player)
        );
    }
}
#[test]
fn p01_p05_token_creator_and_after_entry_controller_are_different_roles() {
    let state = base(
        &[
            crate::program::TriggerSpec::player_action(
                PlayerRelation::AnyParticipant,
                crate::program::PlayerTriggerAction::TokenCreated,
            ),
            crate::program::TriggerSpec::enters(crate::program::ObjectFilter::creature()),
        ],
        ObjectDestination::Hand,
    );
    let snapshot = state.rules_snapshot(None).unwrap();
    let before = snapshot.view();
    let born = before
        .cards
        .insert_token(
            before.cards.revision(),
            C,
            crate::token::TokenDefinition::new(
                Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
                crate::token::TokenName::Default {
                    ordered_subtypes: vec![],
                },
                &state.creature_vocabulary,
            )
            .unwrap()
            .materialize(&state.creature_vocabulary)
            .unwrap(),
            &state.creature_vocabulary,
        )
        .unwrap();
    let controlled = born
        .next
        .apply_controller(born.next.revision(), born.after.id(), B)
        .unwrap();
    let mut successor_snapshot = state.rules_snapshot(None).unwrap();
    successor_snapshot
        .characteristics
        .register_battlefield_entry(born.after.id())
        .unwrap();
    let after = successor_snapshot
        .view_with_cards(&controlled.next)
        .unwrap();
    let appearance = after.appearance(born.after.id()).unwrap();
    assert!(appearance.object.is_token());
    let result = TriggerState::default()
        .publish(
            EventCause::MechanismUnit,
            vec![RulesOccurrence::TokenCreated {
                creator: C,
                after: Box::new(appearance),
            }],
            &before,
            &after,
        )
        .unwrap();
    assert_roles(result.append(), &[C, B]);
}
#[test]
fn p02_life_changed_player_is_captured_from_the_actual_life_owner() {
    let state = base(
        &[crate::program::TriggerSpec::life_changed(
            LifeChangeKind::Gain,
            PlayerRelation::AnyParticipant,
        )],
        ObjectDestination::Hand,
    );
    let before = state.rules_snapshot(None).unwrap();
    let mut changed = state.clone();
    let changes = changed
        .life
        .apply_adjustments(&BTreeMap::from([(C, LifeAdjustment::Gain(2.into()))]))
        .unwrap();
    let after = changed.rules_snapshot(None).unwrap();
    let result = state
        .triggers
        .clone()
        .publish(
            EventCause::MechanismUnit,
            changes
                .into_iter()
                .map(RulesOccurrence::LifeChanged)
                .collect(),
            &before.view(),
            &after.view(),
        )
        .unwrap();
    assert_roles(result.append(), &[C]);
}
#[test]
fn p03_p05_p06_p07_actual_movement_uses_discard_or_correct_side_controller() {
    for (condition, start, destination, discarding, expected) in [
        (
            crate::program::TriggerSpec::player_action(
                PlayerRelation::AnyParticipant,
                crate::program::PlayerTriggerAction::Discarded,
            ),
            ObjectDestination::Hand,
            ObjectDestination::Graveyard,
            true,
            C,
        ),
        (
            crate::program::TriggerSpec::enters(crate::program::ObjectFilter::creature()),
            ObjectDestination::Hand,
            ObjectDestination::Battlefield { controller: B },
            false,
            B,
        ),
        (
            crate::program::TriggerSpec::leaves_battlefield(
                crate::program::ObjectFilter::creature(),
            ),
            ObjectDestination::Battlefield { controller: B },
            ObjectDestination::Hand,
            false,
            B,
        ),
        (
            crate::program::TriggerSpec::dies(crate::program::ObjectFilter::creature()),
            ObjectDestination::Battlefield { controller: B },
            ObjectDestination::Graveyard,
            false,
            B,
        ),
    ] {
        let state = base(&[condition], start);
        let id = state.window.cards().current_identity(CardId(2)).unwrap();
        let result = if discarding {
            state.qualification_discard(state.revision(), C, id)
        } else {
            state.qualification_zone_moves(state.revision(), &[(id, destination)])
        }
        .unwrap();
        assert_roles(result.append(), &[expected]);
    }
}

#[test]
fn p04_actual_step_occurrence_retains_active_player() {
    let state = base(
        &[crate::program::TriggerSpec::step_began(
            crate::turn::Step::Upkeep,
            PlayerRelation::AnyParticipant,
        )],
        ObjectDestination::Hand,
    );
    let snapshot = state.rules_snapshot(None).unwrap();
    let view = snapshot.view();
    let position = crate::turn::TurnPosition {
        number: 4,
        step: crate::turn::Step::Upkeep,
        cleanup_iteration: 0,
        additional_damage: false,
    };
    let result = state
        .triggers
        .clone()
        .publish(
            EventCause::StepBegan { position },
            vec![RulesOccurrence::StepBegan {
                position,
                active: C,
            }],
            &view,
            &view,
        )
        .unwrap();
    assert_roles(result.append(), &[C]);
}
#[test]
fn p08_tap_role_reads_after_view_and_missing_required_context_rejects_atomically() {
    let state = base(
        &[crate::program::TriggerSpec::becomes_tapped(
            crate::program::ObjectFilter::creature(),
        )],
        ObjectDestination::Battlefield { controller: B },
    );
    let snapshot = state.rules_snapshot(None).unwrap();
    let before = snapshot.view();
    let id = before.cards.current_identity(CardId(2)).unwrap();
    let tapped = before
        .cards
        .set_tapped(before.cards.revision(), id, true)
        .unwrap();
    let change = crate::object_identity::ObjectStatusChange {
        before: tapped.before.clone().unwrap(),
        after: tapped.after.clone(),
    };
    let controlled = tapped
        .next
        .apply_controller(tapped.next.revision(), id, C)
        .unwrap();
    let after = snapshot.view_with_cards(&controlled.next).unwrap();
    let occurrence = RulesOccurrence::StatusChange {
        change,
        reason: StatusReason::MechanismUnit,
    };
    let saved = state.triggers.clone();
    let result = saved
        .clone()
        .publish(
            EventCause::MechanismUnit,
            vec![occurrence.clone()],
            &before,
            &after,
        )
        .unwrap();
    assert_roles(result.append(), &[C]);
    let TriggerRelated::StatusChange(receipt) =
        &captured(result.append())[0].causes.members()[0].related
    else {
        panic!("status receipt");
    };
    assert_eq!(receipt.after.controller(), Some(B));
    // A deliberately inconsistent lower-owner status event has no AFTER appearance.
    let moved = controlled
        .next
        .apply_zone_move(controlled.next.revision(), id, ObjectDestination::Hand)
        .unwrap();
    let missing = snapshot.view_with_cards(&moved.next).unwrap();
    let error = saved
        .clone()
        .publish(
            EventCause::MechanismUnit,
            vec![occurrence],
            &before,
            &missing,
        )
        .unwrap_err();
    assert_eq!(error, crate::occurrence::TriggerError::InvalidWorld);
    assert_eq!(state.triggers, saved);
    assert_eq!(state.window.cards(), before.cards);
}

#[test]
fn p08_entry_and_departure_missing_participant_context_reject_without_adoption() {
    for (condition, start, destination, corrupt_after) in [
        (
            crate::program::TriggerSpec::enters(crate::program::ObjectFilter::creature()),
            ObjectDestination::Hand,
            ObjectDestination::Battlefield { controller: B },
            true,
        ),
        (
            crate::program::TriggerSpec::leaves_battlefield(
                crate::program::ObjectFilter::creature(),
            ),
            ObjectDestination::Battlefield { controller: B },
            ObjectDestination::Hand,
            false,
        ),
        (
            crate::program::TriggerSpec::dies(crate::program::ObjectFilter::creature()),
            ObjectDestination::Battlefield { controller: B },
            ObjectDestination::Graveyard,
            false,
        ),
    ] {
        let state = base(&[condition], start);
        let id = state.window.cards().current_identity(CardId(2)).unwrap();
        let transition = state
            .qualification_zone_moves(state.revision(), &[(id, destination)])
            .unwrap();
        let before = state.rules_snapshot(None).unwrap();
        let after = transition.state().rules_snapshot(None).unwrap();
        let mut occurrence = transition
            .append()
            .records
            .iter()
            .find_map(|record| match record {
                RulesRecord::Event(event) => {
                    event
                        .occurrences
                        .iter()
                        .find_map(|occurrence| match &occurrence.value {
                            RulesOccurrence::ZoneChange(_) => Some(occurrence.value.clone()),
                            _ => None,
                        })
                }
                _ => None,
            })
            .unwrap();
        let RulesOccurrence::ZoneChange(change) = &mut occurrence else {
            unreachable!();
        };
        // Keep the actual registry cuts intact; only the supplied appearance is malformed.
        let malformed = if corrupt_after {
            &mut change.after
        } else {
            &mut change.before
        };
        assert_eq!(malformed.object.controller(), Some(B));
        malformed.object = malformed.object.qualification_without_controller();
        let saved = state.clone();
        let error = state
            .triggers
            .clone()
            .publish(
                EventCause::MechanismUnit,
                vec![occurrence],
                &before.view(),
                &after.view(),
            )
            .unwrap_err();
        assert_eq!(error, crate::occurrence::TriggerError::InvalidWorld);
        assert_eq!(state, saved);
        assert!(state.triggers.waiting().is_empty());
    }
}

#[test]
fn matched_count_rejects_missing_or_inconsistent_actual_installed_bindings() {
    use crate::test_support::characteristic_trajectories as t;
    use crate::{
        game::{DecisionReply, RuleFrame},
        program::CreatureTypeSlot,
        token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients},
    };
    let vocabulary = t::vocabulary();
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(7),
        condition: crate::program::TriggerSpec::enters(crate::program::ObjectFilter::creature()),
        grouping: TriggerGrouping::OneOrMoreTogether,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::ChooseCreatureType {
                    into: CreatureTypeSlot::new(0),
                    excluding: BTreeSet::new(),
                },
                Instruction::DrawController {
                    amount: crate::program::ValueExpr::event(
                        crate::program::EventValueKind::MatchedOccurrences,
                    ),
                },
            ],
        ),
    });
    let game = t::world(
        vocabulary.clone(),
        vec![
            t::card(2, watcher, true),
            t::spell(
                1,
                BodyTarget::None,
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        definition: TokenDefinition::new(
                            Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
                            TokenName::Default {
                                ordered_subtypes: vec![],
                            },
                            &vocabulary,
                        )
                        .unwrap(),
                        amount: crate::program::ValueExpr::Literal(2.into()),
                        base_size: None,
                        enters_tapped: false,
                        recipients: TokenRecipients::Controller,
                    }],
                }],
            ),
        ],
        vec![CardId(2)],
    );
    let cast = t::submit(
        &game,
        A,
        crate::game::GameCommand::CastUntargeted {
            spell: t::object(&game, 1),
            payment: None,
        },
    );
    let pending = t::pass_pair(cast.state());
    let RuleFrame::TimestampOrder(frame) = pending.state().pending_decision().unwrap().frame()
    else {
        panic!("timestamp");
    };
    let ordered = t::reply(
        pending.state(),
        A,
        DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
    );
    let resolving = t::pass_pair(ordered.state());
    assert!(matches!(
        resolving.state().pending_decision().unwrap().frame(),
        RuleFrame::CreatureType(_)
    ));
    let execution = resolving.state().execution();
    let entry = execution.window.resolving().unwrap().id();
    let saved = execution.clone();
    assert_eq!(
        execution.resolve_value_expr(
            A,
            &crate::program::ValueExpr::event(crate::program::EventValueKind::MatchedOccurrences)
        ),
        Ok(2.into())
    );
    assert_eq!(
        execution.resolve_value_expr(
            B,
            &crate::program::ValueExpr::event(crate::program::EventValueKind::MatchedOccurrences)
        ),
        Err(EffectError::InconsistentProgram)
    );
    for missing in [false, true] {
        let mut malformed = execution.clone();
        let alternate_source = malformed
            .window
            .cards()
            .iter()
            .find(|o| o.is_token())
            .unwrap()
            .clone();
        let StackPayload::Instructions(payload) = malformed.payloads.get_mut(&entry).unwrap()
        else {
            panic!("instructions");
        };
        if missing {
            payload.bindings.trigger = None;
        } else {
            payload.bindings.trigger.as_mut().unwrap().source.object = alternate_source;
        }
        let malformed_saved = malformed.clone();
        assert_eq!(
            malformed.resolve_value_expr(
                A,
                &crate::program::ValueExpr::event(
                    crate::program::EventValueKind::MatchedOccurrences
                )
            ),
            Err(EffectError::InconsistentProgram)
        );
        assert_eq!(malformed, malformed_saved);
    }
    assert_eq!(execution, &saved);
}

#[test]
fn actual_discovery_selection_counts_instances_once_and_rejects_distinct_matches() {
    for policy in [
        TriggerGrouping::OneOrMoreTogether,
        TriggerGrouping::EachOccurrence,
    ] {
        let state = base_with_grouping(
            &[crate::program::TriggerSpec::player_action(
                PlayerRelation::AnyParticipant,
                crate::program::PlayerTriggerAction::TokenCreated,
            )],
            ObjectDestination::Hand,
            policy,
        );
        let snapshot = state.rules_snapshot(None).unwrap();
        let before = snapshot.view();
        let definition = crate::token::TokenDefinition::new(
            Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            crate::token::TokenName::Default {
                ordered_subtypes: vec![],
            },
            &state.creature_vocabulary,
        )
        .unwrap()
        .materialize(&state.creature_vocabulary)
        .unwrap();
        let inserted = before
            .cards
            .insert_tokens(
                before.cards.revision(),
                vec![(C, definition.clone()), (C, definition)],
                &state.creature_vocabulary,
            )
            .unwrap();
        let mut successor_snapshot = state.rules_snapshot(None).unwrap();
        for object in &inserted.objects {
            successor_snapshot
                .characteristics
                .register_battlefield_entry(object.id())
                .unwrap();
        }
        let after = successor_snapshot.view_with_cards(&inserted.next).unwrap();
        let occurrences = inserted
            .objects
            .iter()
            .map(|object| RulesOccurrence::TokenCreated {
                creator: C,
                after: Box::new(after.appearance(object.id()).unwrap()),
            })
            .collect();
        let result = state
            .triggers
            .clone()
            .publish(EventCause::MechanismUnit, occurrences, &before, &after)
            .unwrap();
        let source = before.cards.current_identity(CardId(1)).unwrap();
        let captures = captured(result.append());
        let selected = crate::test_support::receipt_joins::discovery(
            result.append(),
            source,
            0,
            "ambiguous",
            |_| Ok(true),
        );
        if policy == TriggerGrouping::OneOrMoreTogether {
            assert_eq!(captures.len(), 1);
            assert_eq!(captures[0].causes.len(), 2);
            assert_eq!(selected, Ok(Some(captures[0].id)));
            let mut duplicated = result.append().clone();
            duplicated
                .records
                .push(crate::occurrence::RulesRecord::TriggerDiscovered(Box::new(
                    captures[0].clone(),
                )));
            assert_eq!(
                crate::test_support::receipt_joins::discovery(
                    &duplicated,
                    source,
                    0,
                    "ambiguous",
                    |_| Ok(true)
                ),
                Err("ambiguous".into())
            );

            for object in &inserted.objects {
                assert_eq!(
                    crate::test_support::receipt_joins::birth_discovery(
                        result.append(),
                        source,
                        0,
                        object.id()
                    ),
                    selected
                );
            }
        } else {
            assert_eq!(captures.len(), 2);
            assert_eq!(selected, Err("ambiguous".into()));
        }
    }
}
