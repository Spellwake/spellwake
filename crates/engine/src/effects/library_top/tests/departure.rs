//! Actual four-seat library cuts and deterministic complete original replay.
use super::*;
use crate::program::{
    CreatureTypeSlot, ZoneActionPredicate, ZoneObjectKind, ZoneReplacementAbility,
    ZoneReplacementAbilityId, ZoneReplacementChoice, ZoneReplacementSubject,
};
const C: SeatId = SeatId(31);
const D: SeatId = SeatId(47);
#[derive(Default)]
struct Extras {
    counted: bool,
    ability: bool,
    lower: bool,
    suppressed: bool,
    existing: bool,
}
fn start(kind: LibraryActionKind, recipient: SeatId, replacement: bool, repeated: bool) -> Run {
    start_extended(kind, recipient, replacement, repeated, Extras::default())
}
fn start_extended(
    kind: LibraryActionKind,
    recipient: SeatId,
    replacement: bool,
    repeated: bool,
    extras: Extras,
) -> Run {
    let Extras {
        counted,
        ability,
        lower,
        suppressed,
        existing,
    } = extras;
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "library-departure".into(),
        vocabulary.rules_revision().into(),
        "synthetic".into(),
        "library-departure".into(),
        "private".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let role = if recipient == A {
        ResolvingPlayer::Controller
    } else {
        ResolvingPlayer::TargetPlayer
    };
    let mut body = vec![gain(2), operation(kind, role, 2.into())];
    if repeated {
        body.push(operation(LibraryActionKind::Scry, role, 2.into()));
    }
    body.push(gain(3));
    if counted {
        body.insert(
            0,
            Instruction::ChooseCreatureType {
                into: CreatureTypeSlot::new(0),
                excluding: BTreeSet::new(),
            },
        );
        body[2] = Instruction::Scry {
            player: role,
            amount: InstructionAmount::ControlledPermanentsOfChosenType {
                chosen: CreatureTypeSlot::new(0),
            },
        };
    }
    let mut cards = vec![seed(
        1,
        A,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                if recipient == A {
                    BodyTarget::None
                } else {
                    BodyTarget::SinglePlayer
                },
                body.clone(),
            ),
        ),
    )];
    if ability {
        use crate::activated::{ActivatedAbility, ActivatedAbilityId, SourceCost};
        cards[0] = seed(
            1,
            B,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ActivatedAbilityId(1),
                    mana_cost: ManaCost::free(),
                    source_cost: SourceCost::None,
                    body: ExecutableBody::new(BodyTarget::SinglePlayer, {
                        // The ability declares its real target in the tail while Scry uses
                        // its already bound controller recipient.
                        let mut body = body;
                        *body.last_mut().unwrap() = Instruction::AdjustTargetLife {
                            adjustment: LifeAdjustment::Gain(3.into()),
                        };
                        body
                    }),
                }],
            ),
        );
    }
    if counted {
        let mut human = Program::creature(None, 1.into(), 2.into(), BTreeSet::new());
        human
            .creature_subtypes
            .insert(vocabulary.resolve_label("Human").unwrap());
        cards.push(seed(
            201,
            C,
            ObjectDestination::Battlefield { controller: A },
            human.clone(),
        ));
        cards.push(seed(
            202,
            A,
            ObjectDestination::Battlefield { controller: A },
            human,
        ));
    }
    for id in (10..13).rev() {
        cards.push(seed(
            id,
            recipient,
            ObjectDestination::LibraryTop,
            Program::artifact(None, vec![]),
        ));
    }
    if replacement {
        let mut source = Program::enchantment(None);
        source.zone_replacements = vec![ZoneReplacementAbility {
            id: ZoneReplacementAbilityId(0),
            actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                ZoneActionKind::EffectMill,
                ZoneActionKind::EffectSurveil,
            ])),
            objects: ZoneObjectKind::Card,
            subject: ZoneReplacementSubject::AnyObject,
            destination: PublicZoneDestination::Graveyard,
            replace_with: PublicZoneDestination::Exile,
            choice: ZoneReplacementChoice::OptionalAffectedPlayer,
        }];
        if existing {
            let mut active = Program::artifact(None, vec![]);
            active.zone_replacements = source.zone_replacements.clone();
            cards.push(seed(
                203,
                D,
                ObjectDestination::Battlefield { controller: D },
                active,
            ));
        }
        cards.push(seed(
            200,
            B,
            ObjectDestination::Battlefield { controller: B },
            source,
        ));
    }
    if suppressed {
        use crate::characteristics::{
            AbilityChange, AbilityRemoval, PermanentSelector, StaticAbilityId,
            StaticContinuousAbility, StaticPart, StaticParts,
        };
        let mut suppressor = Program::artifact(None, vec![]);
        suppressor.continuous = Some(StaticContinuousAbility::Battlefield {
            id: StaticAbilityId(0),
            selector: PermanentSelector {
                required_types: BTreeSet::from([CardType::Enchantment]),
                ..PermanentSelector::default()
            },
            parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::remove(
                AbilityRemoval::All,
            ))]),
        });
        cards.push(seed(
            204,
            C,
            ObjectDestination::Battlefield { controller: C },
            suppressor,
        ));
    }
    if lower {
        cards.push(seed(
            2,
            D,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(BodyTarget::None, vec![gain(5)]),
            ),
        ));
    }
    let order = cards
        .iter()
        .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
        .map(|c| c.card)
        .collect();
    let state = GameState::initialize(
        GameId(88412),
        context,
        GameSeed::with_uniform_life(
            vec![A, B, C, D],
            A,
            cards,
            order,
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            names,
        ),
    )
    .unwrap()
    .into_state();
    let mut run = Run::new(state);
    if lower {
        for actor in [A, B, C] {
            run.command(actor, GameCommand::Pass);
        }
        run.command(
            D,
            GameCommand::CastUntargeted {
                spell: object(&run.state, 2),
                payment: None,
            },
        );
        run.command(D, GameCommand::Pass);
    }
    if ability {
        run.command(
            A,
            GameCommand::ActivateAbility {
                source: object(&run.state, 1),
                ability: crate::activated::ActivatedAbilityId(1),
                target: TargetRef::Player(A),
                payment: None,
            },
        );
    } else {
        run.cast(
            1,
            if recipient == A {
                None
            } else {
                Some(TargetRef::Player(recipient))
            },
        );
    }
    for actor in [A, B, C, D] {
        run.command(actor, GameCommand::Pass);
    }
    if counted {
        let human = run
            .state
            .execution()
            .creature_vocabulary()
            .resolve_label("Human")
            .unwrap();
        run.reply(DecisionReply::CreatureType(human));
    }
    run
}
fn replacement(run: &Run) -> ZoneReplacementFrame {
    let RuleFrame::ZoneReplacement(frame) = run.state.pending_decision().unwrap().frame() else {
        panic!("replacement")
    };
    frame.clone()
}
fn select_first(run: &mut Run) {
    let frame = replacement(run);
    if let ZoneReplacementRequest::Select { applications } = frame.request() {
        run.reply(DecisionReply::ZoneReplacement(
            ZoneReplacementReply::Select(applications[0]),
        ));
    }
}
fn finish_order(run: &mut Run) {
    let RuleFrame::ResolutionGraveyardOrder(frame) = run.state.pending_decision().unwrap().frame()
    else {
        panic!("owner order")
    };
    run.reply(DecisionReply::OrderResolutionGraveyard(
        frame.domain().iter().rev().copied().collect(),
    ));
}
fn no_early_moves(run: &Run) {
    assert!(graveyard(&run.state, A).is_empty());
    assert!(
        run.state
            .execution()
            .window()
            .cards()
            .iter()
            .all(|o| o.zone() != ObjectZone::Exile)
    );
}
#[test]
fn unrelated_departure_retains_private_scry_partition_and_rejects_old_capability() {
    let mut run = start(LibraryActionKind::Scry, A, false, false);
    let old = run.state.pending_decision().unwrap().clone();
    let domain = frame(&run.state).domain_top_first().to_vec();
    run.command(C, GameCommand::Concede);
    assert_eq!(frame(&run.state).domain_top_first(), domain);
    let choice = DecisionReply::LibraryTop(LibraryTopReply::Scry {
        top_first: vec![domain[1]],
        bottom_top_first: vec![domain[0]],
    });
    let input = GameInput {
        basis: run.state.basis().clone(),
        actor: A,
        command: GameCommand::Reply {
            decision: old.id(),
            choice: choice.clone(),
        },
    };
    assert!(run.call(input).is_err());
    run.reply(choice);
    assert_eq!(
        library(&run.state, A),
        vec![CardId(11), CardId(12), CardId(10)]
    );
    assert_eq!(run.state.execution().life_total(A), Some(&25.into()));
    run.replay();
}
#[test]
fn departed_library_recipient_omits_pending_and_later_operation_without_action_receipt() {
    let mut run = start(LibraryActionKind::Surveil, B, false, true);
    run.command(B, GameCommand::Concede);
    assert!(run.state.pending_decision().is_none());
    assert!(library(&run.state, B).is_empty());
    assert_eq!(run.state.execution().life_total(A), Some(&25.into()));
    assert!(
        !run.facts()
            .iter()
            .any(|o| matches!(o, RulesOccurrence::LibraryAction(_)))
    );
    run.replay();
}
#[test]
fn replacement_source_departure_keeps_applied_prefix_and_drops_only_unanswered_confirmation() {
    let mut run = start(LibraryActionKind::Mill, A, true, false);
    no_early_moves(&run);
    select_first(&mut run);
    run.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Optional(ZoneOptionalAnswer::Apply),
    ));
    assert!(matches!(
        replacement(&run).request(),
        ZoneReplacementRequest::ConfirmOptional { .. }
    ));
    no_early_moves(&run);
    run.command(B, GameCommand::Concede);
    assert!(run.state.pending_decision().is_none());
    assert_eq!(graveyard(&run.state, A), vec![CardId(1), CardId(10)]);
    let cards = run.state.execution().window().cards();
    assert_eq!(
        cards
            .get(cards.current_identity(CardId(11)).unwrap())
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    assert_eq!(run.state.execution().life_total(A), Some(&25.into()));
    run.replay();
}
#[test]
fn declined_prefix_and_surveillance_partition_survive_last_source_departure_until_real_owner_order()
{
    let mut run = start(LibraryActionKind::Surveil, A, true, false);
    let domain = frame(&run.state).domain_top_first().to_vec();
    run.reply(DecisionReply::LibraryTop(LibraryTopReply::Surveil {
        top_first: vec![],
        to_graveyard: domain,
    }));
    select_first(&mut run);
    run.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline),
    ));
    run.command(B, GameCommand::Concede);
    no_early_moves(&run);
    finish_order(&mut run);
    assert_eq!(
        graveyard(&run.state, A),
        vec![CardId(1), CardId(11), CardId(10)]
    );
    assert_eq!(library(&run.state, A), vec![CardId(12)]);
    run.replay();
}
#[test]
fn owner_order_departures_keep_prefix_with_or_without_replacement_batch() {
    for replacements in [false, true] {
        let mut run = start(LibraryActionKind::Mill, A, replacements, false);
        if replacements {
            for _ in 0..2 {
                select_first(&mut run);
                run.reply(DecisionReply::ZoneReplacement(
                    ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline),
                ));
            }
        }
        assert!(matches!(
            run.state.pending_decision().unwrap().frame(),
            RuleFrame::ResolutionGraveyardOrder(_)
        ));
        run.command(B, GameCommand::Concede);
        run.command(C, GameCommand::Concede);
        no_early_moves(&run);
        finish_order(&mut run);
        assert_eq!(
            graveyard(&run.state, A),
            vec![CardId(1), CardId(11), CardId(10)]
        );
        run.replay();
    }
}
#[test]
fn rebased_prefix_corruption_rejects_without_moving_candidates() {
    let mut run = start(LibraryActionKind::Mill, A, true, false);
    select_first(&mut run);
    run.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline),
    ));
    run.command(C, GameCommand::Concede);
    let mut execution = run.state.execution().clone();
    let Some(PendingWork::LibraryTop(work)) = &mut execution.pending_work else {
        panic!("library")
    };
    let LibraryStage::Replacing { batch, .. } = &mut work.stage else {
        panic!("replacing")
    };
    batch.qualification_corrupt_history();
    let frame = execution.library_top_frame().unwrap();
    let saved = execution.clone();
    assert!(
        execution
            .resume_library_top(
                execution.revision(),
                &frame,
                A,
                DecisionReply::ZoneReplacement(ZoneReplacementReply::Optional(
                    ZoneOptionalAnswer::Decline
                ))
            )
            .is_err()
    );
    assert_eq!(execution, saved);
    no_early_moves(&run);
    run.replay();
}

#[test]
fn captured_library_count_survives_contributor_departure_and_source_owned_ability_continues() {
    for ability in [false, true] {
        let mut run = start_extended(
            LibraryActionKind::Scry,
            A,
            false,
            false,
            Extras {
                counted: true,
                ability,
                ..Extras::default()
            },
        );
        let domain = frame(&run.state).domain_top_first().to_vec();
        assert_eq!(domain.len(), 2);
        run.command(C, GameCommand::Concede);
        if ability {
            run.command(B, GameCommand::Concede);
        }
        assert_eq!(frame(&run.state).domain_top_first(), domain);
        run.reply(DecisionReply::LibraryTop(LibraryTopReply::Scry {
            top_first: domain,
            bottom_top_first: vec![],
        }));
        assert_eq!(run.state.execution().life_total(A), Some(&25.into()));
        let actions = run
            .facts()
            .into_iter()
            .filter_map(|f| {
                if let RulesOccurrence::LibraryAction(a) = f {
                    Some(a)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].requested, 2.into());
        run.replay();
    }
}
#[test]
fn requiring_controller_departure_ceases_library_without_completion_or_reply() {
    for kind in [LibraryActionKind::Scry, LibraryActionKind::Mill] {
        let mut run = start(kind, A, false, false);
        run.command(A, GameCommand::Concede);
        assert!(run.state.pending_decision().is_none());
        assert!(run.state.execution().window().entries().is_empty());
        assert_eq!(run.state.execution().life_total(A), Some(&22.into()));
        assert!(
            !run.facts()
                .iter()
                .any(|f| matches!(f, RulesOccurrence::LibraryAction(_)))
        );
        run.replay();
    }
}

#[test]
fn library_controller_departure_leaves_real_lower_spell_to_resolve() {
    let mut run = start_extended(
        LibraryActionKind::Scry,
        A,
        false,
        false,
        Extras {
            lower: true,
            ..Extras::default()
        },
    );
    run.command(A, GameCommand::Concede);
    while !run.state.execution().window().entries().is_empty() {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            run.state.execution().window().phase()
        else {
            panic!("priority")
        };
        run.command(holder, GameCommand::Pass);
    }
    assert_eq!(run.state.execution().life_total(D), Some(&25.into()));
    assert_eq!(run.state.execution().life_total(A), Some(&22.into()));
    run.replay();
}
#[test]
fn departure_commit_and_reissued_library_decision_failures_leave_whole_root_unchanged() {
    use crate::effects::private_qualification::QualificationFault;
    let mut run = start(LibraryActionKind::Mill, A, true, false);
    select_first(&mut run);
    run.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Optional(ZoneOptionalAnswer::Apply),
    ));
    for fault in [
        QualificationFault::ObjectIdentity(u64::MAX),
        QualificationFault::ObjectRevision(u64::MAX),
        QualificationFault::Revision(u64::MAX),
    ] {
        let mut bad = run.state.clone();
        bad.set_test_check_effect_fault(fault);
        let before = bad.clone();
        let input = GameInput {
            basis: bad.basis().clone(),
            actor: B,
            command: GameCommand::Concede,
        };
        let result = bad.advance(input.clone());
        assert!(result.is_err());
        assert_eq!(bad.advance(input), result);
        assert_eq!(bad, before);
    }
    let mut bad = start(LibraryActionKind::Scry, A, false, false).state;
    bad.set_test_next_decision(u64::MAX);
    let before = bad.clone();
    assert!(
        bad.advance(GameInput {
            basis: bad.basis().clone(),
            actor: C,
            command: GameCommand::Concede
        })
        .is_err()
    );
    assert_eq!(bad, before);
    no_early_moves(&run);
    run.replay();
}

#[test]
fn live_confirmation_keeps_selection_across_unrelated_departure_and_answers_once() {
    let mut run = start(LibraryActionKind::Mill, A, true, false);
    select_first(&mut run);
    let old = replacement(&run);
    run.command(C, GameCommand::Concede);
    assert_eq!(replacement(&run).request(), old.request());
    assert_eq!(replacement(&run).stage(), old.stage());
    run.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline),
    ));
    select_first(&mut run);
    run.reply(DecisionReply::ZoneReplacement(
        ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline),
    ));
    finish_order(&mut run);
    assert_eq!(
        graveyard(&run.state, A),
        vec![CardId(1), CardId(11), CardId(10)]
    );
    run.replay();
}
#[test]
fn departure_exposes_new_replacements_during_owner_order_with_or_without_prior_batch() {
    for existing in [false, true] {
        let mut run = start_extended(
            LibraryActionKind::Mill,
            A,
            true,
            false,
            Extras {
                suppressed: true,
                existing,
                ..Extras::default()
            },
        );
        if existing {
            for _ in 0..2 {
                select_first(&mut run);
                run.reply(DecisionReply::ZoneReplacement(
                    ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline),
                ));
            }
        }
        assert!(matches!(
            run.state.pending_decision().unwrap().frame(),
            RuleFrame::ResolutionGraveyardOrder(_)
        ));
        run.command(C, GameCommand::Concede);
        no_early_moves(&run);
        for _ in 0..2 {
            select_first(&mut run);
            run.reply(DecisionReply::ZoneReplacement(
                ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline),
            ));
        }
        run.command(B, GameCommand::Concede);
        finish_order(&mut run);
        assert_eq!(
            graveyard(&run.state, A),
            vec![CardId(1), CardId(11), CardId(10)]
        );
        run.replay();
    }
}
#[test]
fn sealed_library_operands_and_full_cut_proof_cannot_be_replaced_by_recomputed_values() {
    let mut run = start(LibraryActionKind::Scry, A, false, false);
    run.command(C, GameCommand::Concede);
    let actual = run.state.execution();
    let original_domain = actual
        .library_top_frame()
        .and_then(|f| {
            if let EffectDecision::LibraryTop(f) = f {
                Some(f.domain_top_first().to_vec())
            } else {
                None
            }
        })
        .unwrap();
    for mutation in 0..4 {
        let mut bad = actual.clone();
        let Some(PendingWork::LibraryTop(work)) = &mut bad.pending_work else {
            panic!("library")
        };
        match mutation {
            0 => work.requested = 1.into(),
            1 => work.player = B,
            2 => work.domain.reverse(),
            _ => work.operands.as_mut().unwrap().instruction = gain(99),
        }
        let frame = bad.library_top_frame().unwrap();
        let saved = bad.clone();
        assert_eq!(
            bad.resume_library_top(
                bad.revision(),
                &frame,
                A,
                DecisionReply::LibraryTop(LibraryTopReply::Scry {
                    top_first: original_domain.clone(),
                    bottom_top_first: vec![]
                })
            ),
            Err(EffectError::InconsistentLibraryTopWork)
        );
        assert_eq!(bad, saved);
    }
    let saved = actual.clone();
    assert!(matches!(
        actual.depart_players(&BTreeSet::from([B]), None, None),
        Err(EffectError::UnsupportedDepartureState)
    ));
    assert_eq!(*actual, saved);
    run.replay();
}
