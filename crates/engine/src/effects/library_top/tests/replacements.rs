//! Mechanism controls with retained vocabulary and real root input/result replay.
//! Synthetic typed replacement definitions do not qualify a catalogue card.
use super::*;
use crate::{
    object_identity::ObjectIdentityError,
    program::{
        DefinitionError, PublicZoneDestination as D, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementSubject,
    },
    test_support::program as codec,
};
fn rule(id: u64, action: ZoneActionKind, from: D, to: D, optional: bool) -> ZoneReplacementAbility {
    ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(id),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([action])),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::AnyObject,
        destination: from,
        replace_with: to,
        choice: if optional {
            ZoneReplacementChoice::OptionalAffectedPlayer
        } else {
            ZoneReplacementChoice::Mandatory
        },
    }
}
fn source(rules: Vec<ZoneReplacementAbility>) -> Program {
    let mut p = Program::enchantment(None);
    p.zone_replacements = rules;
    p
}
fn start(
    kind: LibraryActionKind,
    count: u64,
    amount: i64,
    rules: Vec<ZoneReplacementAbility>,
) -> Run {
    let mut run = Run::new(fixture_with_sources(
        vec![
            operation(kind, ResolvingPlayer::Controller, amount.into()),
            gain(3),
        ],
        BodyTarget::None,
        A,
        count,
        None,
        false,
        if rules.is_empty() {
            vec![]
        } else {
            vec![source(rules)]
        },
    ));
    run.cast(1, None);
    run.pass_round();
    run
}
fn owned(effect: &EffectWindow) -> &LibraryTopWork {
    let Some(PendingWork::LibraryTop(work)) = &effect.pending_work else {
        panic!("actual library work")
    };
    work
}
fn owned_mut(effect: &mut EffectWindow) -> &mut LibraryTopWork {
    let Some(PendingWork::LibraryTop(work)) = &mut effect.pending_work else {
        panic!("actual library work")
    };
    work
}
fn batch(effect: &EffectWindow) -> &ZoneReplacementBatch {
    let LibraryStage::Replacing { batch, .. } = &owned(effect).stage else {
        panic!("actual replacing stage")
    };
    batch
}
fn replacement(run: &Run) -> ZoneReplacementFrame {
    let RuleFrame::ZoneReplacement(frame) = run.state.pending_decision().unwrap().frame() else {
        panic!("replacement frame")
    };
    frame.clone()
}
fn select(run: &mut Run, before: ObjectId, ability: u64) {
    let frame = replacement(run);
    let ZoneReplacementRequest::Select { applications } = frame.request() else {
        panic!("actual selection")
    };
    let application = *applications
        .iter()
        .find(|a| {
            a.event.before() == before && a.effect.ability == ZoneReplacementAbilityId(ability)
        })
        .unwrap();
    run.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Select(application),
    ));
}
fn confirm(run: &mut Run, answer: ZoneOptionalAnswer) {
    run.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Optional(answer),
    ));
}
fn choose_surveillance(run: &mut Run, top: Vec<ObjectId>, selected: Vec<ObjectId>) {
    run.reply(DecisionReply::LibraryTop(LibraryTopReply::Surveil {
        top_first: top,
        to_graveyard: selected,
    }));
}
fn actions(run: &Run) -> Vec<&LibraryAction> {
    run.facts()
        .into_iter()
        .filter_map(|fact| match fact {
            RulesOccurrence::LibraryAction(a) => Some(a),
            _ => None,
        })
        .collect()
}
fn assert_rejected(
    effect: &EffectWindow,
    frame: &EffectDecision,
    actor: SeatId,
    reply: DecisionReply,
    error: EffectError,
) {
    let before = effect.clone();
    assert_eq!(
        effect.resume_library_top(effect.revision(), frame, actor, reply.clone()),
        Err(error.clone())
    );
    assert_eq!(
        *effect, before,
        "all work, history, journals, identities and counters preserved"
    );
    assert_eq!(
        effect.resume_library_top(effect.revision(), frame, actor, reply),
        Err(error)
    );
    assert_eq!(*effect, before);
}
#[test]
fn no_source_and_empty_selected_use_none_and_preserve_completion_distinctions() {
    for kind in [LibraryActionKind::Mill, LibraryActionKind::Surveil] {
        let mut run = start(kind, 4, 2, vec![]);
        let before = owned(run.state.execution()).domain.clone();
        if kind == LibraryActionKind::Surveil {
            choose_surveillance(&mut run, vec![], vec![before[1], before[0]]);
        }
        assert!(matches!(
            &owned(run.state.execution()).stage,
            LibraryStage::Ordering {
                replacements: None,
                ..
            }
        ));
        run.reply(DecisionReply::OrderResolutionGraveyard(vec![
            before[1], before[0],
        ]));
        let a = actions(&run)[0];
        assert_eq!(a.selected_before_top_first, before);
        assert_eq!(a.public_zone_moves.len(), 2);
        assert!(
            a.public_zone_moves
                .iter()
                .all(|m| m.destination == D::Graveyard && m.before != m.after)
        );
        assert_eq!(library(&run.state, A), vec![CardId(12), CardId(13)]);
        assert_eq!(run.state.execution().life_total(A), Some(&23.into()));
        run.replay();
    }
    // A nonempty roster with no matching action must retain a complete batch.
    let mut unmatched = start(
        LibraryActionKind::Surveil,
        3,
        2,
        vec![rule(
            0,
            ZoneActionKind::EffectMill,
            D::Graveyard,
            D::Exile,
            false,
        )],
    );
    let domain = frame(&unmatched.state).domain_top_first().to_vec();
    choose_surveillance(&mut unmatched, vec![], domain.clone());
    assert!(
        matches!(&owned(unmatched.state.execution()).stage,LibraryStage::Ordering {replacements:Some(batch),..} if batch.candidates().len()==2&&batch.history().is_empty())
    );
    unmatched.reply(DecisionReply::OrderResolutionGraveyard(domain));
    unmatched.replay();
    // A live source does not manufacture a batch for a keep-all selection.
    let mut run = start(
        LibraryActionKind::Surveil,
        2,
        2,
        vec![rule(
            0,
            ZoneActionKind::EffectSurveil,
            D::Graveyard,
            D::Exile,
            false,
        )],
    );
    let domain = frame(&run.state).domain_top_first().to_vec();
    choose_surveillance(&mut run, domain.clone(), vec![]);
    let a = actions(&run)[0];
    assert!(a.selected_before_top_first.is_empty() && a.public_zone_moves.is_empty());
    assert_eq!(a.top_first, domain);
    run.replay();
    for kind in [LibraryActionKind::Mill, LibraryActionKind::Surveil] {
        for amount in [0, 1] {
            let run = start(
                kind,
                0,
                amount,
                vec![rule(
                    0,
                    ZoneActionKind::EffectMill,
                    D::Graveyard,
                    D::Exile,
                    false,
                )],
            );
            assert_eq!(actions(&run).len(), usize::try_from(amount).unwrap());
            assert!(
                run.calls
                    .iter()
                    .filter_map(|(_, r)| r.as_ref().ok())
                    .all(|c| c.draw_journal().events.is_empty())
            );
            assert!(run.state.outcome().is_none());
            run.replay();
        }
    }
}
#[test]
fn copied_surveil_body_redirects_then_continues_once_and_completes_surveilled() {
    let mut run = Run::new(fixture_with_sources(
        vec![
            operation(
                LibraryActionKind::Surveil,
                ResolvingPlayer::Controller,
                1.into(),
            ),
            gain(3),
        ],
        BodyTarget::None,
        A,
        3,
        Some(crate::program::TriggerSpec::player_action(
            PlayerRelation::Controller,
            crate::program::PlayerTriggerAction::Surveilled,
        )),
        true,
        vec![source(vec![rule(
            0,
            ZoneActionKind::EffectSurveil,
            D::Graveyard,
            D::Exile,
            false,
        )])],
    ));
    run.cast(1, None);
    let original = run.state.execution().window().entries()[0].id();
    run.cast(2, Some(TargetRef::Stack(original)));
    run.pass_round();
    for _ in 0..2 {
        run.pass_round();
        let domain = frame(&run.state).domain_top_first().to_vec();
        choose_surveillance(&mut run, vec![], domain);
        assert_eq!(
            run.state.execution().window().entries().len(),
            if actions(&run).len() == 1 { 2 } else { 1 }
        );
        run.pass_round();
    }
    assert!(run.state.execution().window().entries().is_empty());
    assert_eq!(run.state.execution().life_total(A), Some(&28.into()));
    let a = actions(&run);
    assert_eq!(a.len(), 2);
    assert!(a.iter().all(|a| a.public_zone_moves.len() == 1
        && a.public_zone_moves[0].destination == D::Exile
        && a.graveyard_before_top_first.is_empty()));
    assert_ne!(
        a[0].selected_before_top_first,
        a[1].selected_before_top_first
    );
    let entries: BTreeSet<_> = run
        .calls
        .iter()
        .filter_map(|(_, r)| r.as_ref().ok())
        .flat_map(|c| &c.rules_journal().records)
        .filter_map(|r| match r {
            RulesRecord::Event(e)
                if e.occurrences
                    .iter()
                    .any(|o| matches!(o.value, RulesOccurrence::LibraryAction(_))) =>
            {
                Some(e.cause.clone())
            }
            _ => None,
        })
        .map(|c| format!("{c:?}"))
        .collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(library(&run.state, A), vec![CardId(12)]);
    run.replay();
}
#[test]
fn mill_chain_reevaluates_each_lineage_once_with_optional_history_separate() {
    let mut run = start(
        LibraryActionKind::Mill,
        3,
        2,
        vec![
            rule(0, ZoneActionKind::EffectMill, D::Graveyard, D::Exile, false),
            rule(1, ZoneActionKind::EffectMill, D::Exile, D::Graveyard, true),
        ],
    );
    let domain = owned(run.state.execution()).domain.clone();
    select(&mut run, domain[0], 0);
    select(&mut run, domain[0], 1);
    confirm(&mut run, ZoneOptionalAnswer::Apply);
    let b = batch(run.state.execution());
    assert_eq!(b.history().len(), 3);
    assert_eq!(b.applied().values().map(BTreeSet::len).sum::<usize>(), 3);
    assert!(
        b.candidates()
            .values()
            .all(|c| c.action == ZoneActionKind::EffectMill
                && matches!(c.reason,ZoneReason::Mill {player} if player==A))
    );
    assert_eq!(b.optional().len(), 1);
    assert_eq!(
        library(&run.state, A),
        vec![CardId(10), CardId(11), CardId(12)]
    );
    assert_eq!(
        [object(&run.state, 10), object(&run.state, 11)],
        domain.as_slice()
    );
    assert!(actions(&run).is_empty());
    assert!(
        matches!(replacement(&run).request(),ZoneReplacementRequest::ConfirmOptional {application} if application.event.before()==domain[1])
    );
    confirm(&mut run, ZoneOptionalAnswer::Decline);
    let a = actions(&run)[0];
    assert_eq!(
        a.public_zone_moves
            .iter()
            .map(|m| m.destination)
            .collect::<Vec<_>>(),
        vec![D::Graveyard, D::Exile]
    );
    assert_eq!(a.graveyard_before_top_first, vec![domain[0]]);
    let actual=run.facts().into_iter().filter_map(|f|match f {RulesOccurrence::ZoneChange(z) if matches!(z.reason,ZoneReason::Mill {player} if player==A)=>Some(z),_=>None}).collect::<Vec<_>>();
    assert_eq!(actual.len(), 2);
    assert!(actual.iter().all(|z| domain.contains(&z.before.object.id())
        && z.before.object.zone() == ObjectZone::Library(A)));
    run.replay();
    let v = creature_types::selected_vocabulary().unwrap();
    let mut p = source(vec![
        rule(0, ZoneActionKind::EffectMill, D::Graveyard, D::Exile, false),
        rule(
            1,
            ZoneActionKind::EffectSurveil,
            D::Graveyard,
            D::Exile,
            true,
        ),
    ]);
    assert_eq!(codec::decode(codec::view(&p), &v).unwrap(), p);
    p.zone_replacements[0].actions = ZoneActionPredicate::Any;
    assert_eq!(p.validate(&v), Ok(()));
    for unsupported in [
        ZoneActionKind::Mill,
        ZoneActionKind::Surveil,
        ZoneActionKind::Draw,
    ] {
        p.zone_replacements[0].actions =
            ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectMill, unsupported]));
        assert_eq!(
            p.validate(&v),
            Err(DefinitionError::UnsupportedZoneReplacementScope)
        );
    }
}
#[test]
fn malformed_frame_domain_amount_and_stage_controls_preserve_whole_owned_work() {
    for with_source in [false, true] {
        let mut run = start(
            LibraryActionKind::Surveil,
            4,
            2,
            if with_source {
                vec![rule(
                    0,
                    ZoneActionKind::EffectSurveil,
                    D::Graveyard,
                    D::Exile,
                    true,
                )]
            } else {
                vec![]
            },
        );
        let domain = frame(&run.state).domain_top_first().to_vec();
        let stale = run.state.execution().library_top_frame().unwrap();
        let effect = run.state.execution();
        let choice = DecisionReply::LibraryTop(LibraryTopReply::Surveil {
            top_first: vec![],
            to_graveyard: domain.clone(),
        });
        assert_rejected(effect, &stale, B, choice.clone(), EffectError::WrongChooser);
        for mutation in 0..3 {
            let mut bad = effect.clone();
            let work = owned_mut(&mut bad);
            match mutation {
                0 => work.requested = 3.into(),
                1 => work.domain.pop().map(|_| ()).unwrap(),
                _ => work.domain.swap(0, 1),
            };
            assert_rejected(
                &bad,
                &stale,
                A,
                choice.clone(),
                EffectError::InconsistentLibraryTopWork,
            );
        }
        let mut invalid = domain.clone();
        invalid[1] = invalid[0];
        assert_rejected(
            effect,
            &stale,
            A,
            DecisionReply::LibraryTop(LibraryTopReply::Surveil {
                top_first: vec![],
                to_graveyard: invalid,
            }),
            EffectError::InvalidLibraryPartition,
        );
        choose_surveillance(&mut run, vec![], domain);
        assert_rejected(
            run.state.execution(),
            &stale,
            A,
            choice,
            EffectError::WrongLibraryTopFrame,
        );
        if with_source {
            let frame = replacement(&run);
            let effect = run.state.execution();
            let ZoneReplacementRequest::Select { applications } = frame.request() else {
                panic!("select")
            };
            let application = applications[0];
            let reply = DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(application));
            assert_rejected(
                effect,
                &EffectDecision::ZoneReplacement(frame.qualification_with_stage(frame.stage() + 1)),
                A,
                reply.clone(),
                EffectError::WrongLibraryTopFrame,
            );
            let mut wrong = application;
            wrong.event = wrong
                .event
                .qualification_with_before(object(&run.state, 90));
            assert_rejected(
                effect,
                &EffectDecision::ZoneReplacement(frame.clone()),
                A,
                DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(wrong)),
                EffectError::ZoneReplacement(ZoneReplacementError::InapplicableApplication),
            );
            let mut exhausted = effect.clone();
            let LibraryStage::Replacing { batch, .. } = &mut owned_mut(&mut exhausted).stage else {
                unreachable!()
            };
            batch.qualification_exhaust_stage();
            let current = exhausted.library_top_frame().unwrap();
            assert_rejected(
                &exhausted,
                &current,
                A,
                reply,
                EffectError::ZoneReplacement(ZoneReplacementError::StageExhausted),
            );
            while run.state.pending_decision().is_some() {
                let f = run.state.pending_decision().unwrap().frame();
                match f {
                    RuleFrame::ZoneReplacement(f) => match f.request() {
                        ZoneReplacementRequest::Select { applications } => {
                            run.reply(DecisionReply::ZoneReplacement(
                                ZoneReplacementReply::Select(applications[0]),
                            ));
                        }
                        ZoneReplacementRequest::Devour { .. }
                        | ZoneReplacementRequest::DevourGraveyardOrder { .. }
                        | ZoneReplacementRequest::EntryCopy { .. }
                        | ZoneReplacementRequest::EntryController { .. } => {
                            panic!("fixture has no controller-changing entrants")
                        }
                        ZoneReplacementRequest::ConfirmOptional { .. } => {
                            confirm(&mut run, ZoneOptionalAnswer::Decline)
                        }
                    },
                    RuleFrame::ResolutionGraveyardOrder(f) => {
                        run.reply(DecisionReply::OrderResolutionGraveyard(f.domain().to_vec()));
                    }
                    _ => panic!("expected replacement/order"),
                }
            }
        } else {
            let effect = run.state.execution();
            let bound = effect.library_top_frame().unwrap();
            let mut bad = effect.clone();
            owned_mut(&mut bad).requested = 0.into();
            assert_rejected(
                &bad,
                &bound,
                A,
                DecisionReply::OrderResolutionGraveyard(owned(effect).domain.clone()),
                EffectError::InconsistentLibraryTopWork,
            );
            let RuleFrame::ResolutionGraveyardOrder(f) =
                run.state.pending_decision().unwrap().frame()
            else {
                panic!("order")
            };
            run.reply(DecisionReply::OrderResolutionGraveyard(f.domain().to_vec()));
        }
        run.replay();
    }
}
#[test]
fn real_late_identity_and_after_movement_reorder_exhaustion_are_atomic() {
    let mut run = start(
        LibraryActionKind::Mill,
        4,
        3,
        vec![rule(
            0,
            ZoneActionKind::EffectMill,
            D::Graveyard,
            D::Exile,
            true,
        )],
    );
    let domain = owned(run.state.execution()).domain.clone();
    select(&mut run, domain[0], 0);
    confirm(&mut run, ZoneOptionalAnswer::Apply);
    select(&mut run, domain[1], 0);
    confirm(&mut run, ZoneOptionalAnswer::Decline);
    confirm(&mut run, ZoneOptionalAnswer::Decline);
    let effect = run.state.execution();
    let frame = effect.library_top_frame().unwrap();
    let order = vec![domain[2], domain[1]];
    let reply = DecisionReply::OrderResolutionGraveyard(order.clone());
    assert!(matches!(
        &owned(effect).stage,
        LibraryStage::Ordering {
            replacements: Some(_),
            ..
        }
    ));
    for change in 0..2 {
        let mut bad = effect.clone();
        let work = owned_mut(&mut bad);
        if change == 0 {
            work.requested = 0.into();
        } else {
            work.domain.pop();
        }
        assert_rejected(
            &bad,
            &frame,
            A,
            reply.clone(),
            EffectError::InconsistentLibraryTopWork,
        );
    }
    assert_rejected(
        effect,
        &frame,
        A,
        DecisionReply::OrderResolutionGraveyard(vec![domain[1], domain[1]]),
        EffectError::InvalidGraveyardPermutation,
    );
    assert_rejected(
        effect,
        &frame,
        A,
        DecisionReply::LibraryTop(LibraryTopReply::Surveil {
            top_first: vec![],
            to_graveyard: domain.clone(),
        }),
        EffectError::WrongDecisionKind,
    );
    let moves = vec![
        (domain[1], ObjectDestination::Graveyard),
        (domain[2], ObjectDestination::Graveyard),
        (domain[0], ObjectDestination::Exile),
    ];
    let mut identity = effect.clone();
    identity.window.set_test_next_object_identity(u64::MAX - 1);
    let cards = identity.window.cards();
    assert!(
        cards
            .apply_zone_moves(cards.revision(), &moves[..1])
            .is_ok()
    );
    assert_eq!(
        cards
            .apply_zone_moves(cards.revision(), &moves)
            .unwrap_err(),
        ObjectIdentityError::IdentityExhausted
    );
    assert_rejected(
        &identity,
        &frame,
        A,
        reply.clone(),
        EffectError::Window(StackWindowError::Objects(
            ObjectIdentityError::IdentityExhausted,
        )),
    );
    let mut reorder = effect.clone();
    reorder
        .window
        .set_test_object_revision(u64::MAX - u64::try_from(moves.len()).unwrap());
    let cards = reorder.window.cards();
    let (after, receipts) = cards.apply_zone_moves(cards.revision(), &moves).unwrap();
    assert_eq!(receipts.len(), 3);
    assert_eq!(after.revision(), u64::MAX);
    assert_eq!(after.library(A), &[object(&run.state, 13)]);
    assert_eq!(
        after.reorder_library(after.revision(), A, after.library(A).to_vec()),
        Err(ObjectIdentityError::RevisionExhausted)
    );
    assert_rejected(
        &reorder,
        &frame,
        A,
        reply,
        EffectError::Window(StackWindowError::Objects(
            ObjectIdentityError::RevisionExhausted,
        )),
    );
    run.reply(DecisionReply::OrderResolutionGraveyard(order));
    assert_eq!(actions(&run)[0].public_zone_moves.len(), 3);
    run.replay();
}

/// Join privileged metadata to the actual movement facts in its own publication.
/// Final current-card lookup cannot substitute for this incarnation-exact join.
pub(super) fn assert_publication_receipts(run: &Run) {
    for candidate in run.calls.iter().filter_map(|(_, r)| r.as_ref().ok()) {
        for record in &candidate.rules_journal().records {
            let RulesRecord::Event(event) = record else {
                continue;
            };
            for fact in &event.occurrences {
                let RulesOccurrence::LibraryAction(action) = &fact.value else {
                    continue;
                };
                let changes=event.occurrences.iter().filter_map(|o|match &o.value {
                    RulesOccurrence::ZoneChange(z) if matches!((action.kind,z.reason),
                        (LibraryActionKind::Mill,ZoneReason::Mill {player}) | (LibraryActionKind::Surveil,ZoneReason::Surveil {player}) if player==action.player)=>Some(z.as_ref()),
                    _=>None,
                }).collect::<Vec<_>>();
                assert_eq!(changes.len(), action.public_zone_moves.len());
                assert_eq!(
                    action.selected_before_top_first,
                    action
                        .public_zone_moves
                        .iter()
                        .map(|m| m.before)
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    action
                        .selected_before_top_first
                        .iter()
                        .copied()
                        .collect::<BTreeSet<_>>()
                        .len(),
                    action.selected_before_top_first.len()
                );
                for movement in &action.public_zone_moves {
                    let same = changes
                        .iter()
                        .filter(|z| z.before.object.id() == movement.before)
                        .collect::<Vec<_>>();
                    assert_eq!(same.len(), 1);
                    let actual = same[0];
                    assert_eq!(actual.after.object.id(), movement.after);
                    assert_ne!(movement.before, movement.after);
                    assert_eq!(
                        actual.before.object.zone(),
                        ObjectZone::Library(action.player)
                    );
                    assert_eq!(
                        actual.after.object.zone(),
                        match movement.destination {
                            D::Hand => panic!(
                                "library-top replacement unexpectedly returned a card to hand"
                            ),
                            D::Graveyard => ObjectZone::Graveyard(action.player),
                            D::Exile => ObjectZone::Exile,
                            D::Battlefield =>
                                panic!("library-top replacement unexpectedly entered battlefield"),
                            D::Command => ObjectZone::Command,
                            D::Library =>
                                panic!("library-top replacement unexpectedly returned to library"),
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn real_chosen_type_and_captured_trigger_counts_reauthorize_the_owned_library_cursor() {
    use crate::token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients};
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    for captured in [false, true] {
        let amount = if captured {
            crate::program::ValueExpr::event(crate::program::EventValueKind::MatchedOccurrences)
        } else {
            crate::program::ValueExpr::count_battlefield(
                crate::program::ObjectFilter::any()
                    .controlled()
                    .of_chosen_slot(CreatureTypeSlot::new(0)),
            )
        };
        let body = vec![
            Instruction::MutatePlayer(Box::new(crate::toolbox::MutatePlayerSpec::mill(
                ResolvingPlayer::Controller,
                amount.clone(),
            ))),
            Instruction::MutatePlayer(Box::new(crate::toolbox::MutatePlayerSpec::surveil(
                ResolvingPlayer::Controller,
                amount,
            ))),
            gain(3),
        ];
        let redirect = source(vec![
            rule(0, ZoneActionKind::EffectMill, D::Graveyard, D::Exile, false),
            rule(
                1,
                ZoneActionKind::EffectSurveil,
                D::Graveyard,
                D::Exile,
                false,
            ),
        ]);
        let (main, extra) = if captured {
            let mut watcher = Program::artifact(None, vec![]);
            watcher.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(7),
                condition: crate::program::TriggerSpec::enters(
                    crate::program::ObjectFilter::creature(),
                ),
                grouping: TriggerGrouping::OneOrMoreTogether,
                body: ExecutableBody::new(BodyTarget::None, body),
            });
            let token = TokenDefinition::new(
                Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
                TokenName::Default {
                    ordered_subtypes: vec![],
                },
                &vocabulary,
            )
            .unwrap();
            (
                vec![Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        definition: token,
                        amount: crate::program::ValueExpr::Literal(2.into()),
                        base_size: None,
                        enters_tapped: false,
                        recipients: TokenRecipients::Controller,
                        attacking_defender: None,
                    }],
                }],
                vec![redirect, watcher],
            )
        } else {
            let creature = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([elf]));
            let mut main = vec![Instruction::ChooseCreatureType {
                into: CreatureTypeSlot::new(0),
                excluding: BTreeSet::new(),
            }];
            main.extend(body);
            (main, vec![redirect, creature.clone(), creature])
        };
        let mut run = Run::new(fixture_with_sources(
            main,
            BodyTarget::None,
            A,
            5,
            None,
            false,
            extra,
        ));
        run.cast(1, None);
        run.pass_round();
        if captured {
            let RuleFrame::TimestampOrder(frame) = run.state.pending_decision().unwrap().frame()
            else {
                panic!("actual simultaneous token entries")
            };
            run.reply(DecisionReply::TimestampOrder(
                frame.members().iter().map(|m| m.member).collect(),
            ));
            assert_eq!(
                run.state.execution().window().entries().len(),
                1,
                "actual grouped trigger placed after source spell completes"
            );
            run.pass_round();
            assert!(
                matches!(run.state.execution().window().resolving().unwrap().kind(),StackKind::TriggeredAbility {source} if source.id()==object(&run.state,201))
            );
        } else {
            assert!(matches!(
                run.state.pending_decision().unwrap().frame(),
                RuleFrame::CreatureType(_)
            ));
            run.reply(DecisionReply::CreatureType(elf));
        }
        let before = owned(run.state.execution()).domain.clone();
        assert_eq!(before.len(), 2);
        assert_eq!(owned(run.state.execution()).requested, 2.into());
        assert!(
            batch(run.state.execution())
                .candidates()
                .values()
                .all(|c| c.action == ZoneActionKind::EffectMill)
        );
        select(&mut run, before[0], 0);
        let next = frame(&run.state).domain_top_first().to_vec();
        assert_eq!(next.len(), 2);
        assert_eq!(owned(run.state.execution()).requested, 2.into());
        choose_surveillance(&mut run, vec![], next.clone());
        select(&mut run, next[0], 1);
        let actions = actions(&run);
        assert_eq!(actions.len(), 2);
        assert!(actions.iter().all(|a| {
            a.requested == 2.into()
                && a.selected_before_top_first.len() == 2
                && a.public_zone_moves
                    .iter()
                    .all(|m| m.destination == D::Exile)
        }));
        assert_eq!(library(&run.state, A), vec![CardId(14)]);
        assert_eq!(run.state.execution().life_total(A), Some(&23.into()));
        run.replay();
    }
}
