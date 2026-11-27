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
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::creature(),
        ),
        grouping,
        body: ExecutableBody::new(BodyTarget::None, body),
    });
    program
}
fn draw_count() -> Instruction {
    spellwake_engine::program::Instruction::DrawCards(Box::new(
        spellwake_engine::toolbox::DrawCardsSpec::simple(
            spellwake_engine::toolbox::PlayerSelector::Resolving(
                spellwake_engine::program::ResolvingPlayer::Controller,
            ),
            spellwake_engine::program::ValueExpr::event(
                spellwake_engine::program::EventValueKind::MatchedOccurrences,
            ),
        ),
    ))
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
                    BodyTarget::SINGLE_PERMANENT,
                    vec![Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::become_copy(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                spellwake_engine::program::EffectSubject::AbilitySource,
                            )),
                            spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                spellwake_engine::program::MoveSubjectRef::TargetObject,
                            ),
                            ResolvedDuration::UntilEndOfTurn,
                            spellwake_engine::program::CopyExceptions {
                                override_pt: None,
                                ..Default::default()
                            },
                        ),
                    ))],
                ),
            }],
        );
        let mut donor = Program::creature(None, 1.into(), 1.into(), BTreeSet::new());
        donor.triggered =
            grouped_watcher(TriggerGrouping::OneOrMoreTogether, vec![draw_count()]).triggered;
        donor.triggered[0].condition = spellwake_engine::program::TriggerSpec::player_action(
            PlayerRelation::AnyParticipant,
            spellwake_engine::program::PlayerTriggerAction::TokenCreated,
        );
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
                    BodyTarget::SINGLE_PERMANENT,
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
    watcher.triggered[0].condition = spellwake_engine::program::TriggerSpec::dies(
        spellwake_engine::program::ObjectFilter::creature(),
    );
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
fn matched_count_keeps_original_binding_across_nested_token_order_and_later_draw() {
    let watcher = grouped_watcher(
        TriggerGrouping::OneOrMoreTogether,
        vec![
            create(vec![TokenCreation {
                definition: template(Program::artifact(None, vec![])),
                amount: spellwake_engine::program::ValueExpr::event(
                    spellwake_engine::program::EventValueKind::MatchedOccurrences,
                ),
                base_size: None,
                enters_tapped: false,
                recipients: TokenRecipients::Controller,
                attacking_defender: None,
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
