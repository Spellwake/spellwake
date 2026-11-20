//! Fault controls use actual issued frames and otherwise valid native replies.
use super::*;
const D: SeatId = SeatId(77);
fn selected_cut() -> GameState {
    selected_cut_with_replacement(false)
}
fn selected_cut_with_replacement(replacement: bool) -> GameState {
    let mut cards: Vec<SeedCard> = [
        (10, A),
        (11, A),
        (20, B),
        (21, B),
        (30, C),
        (31, C),
        (40, D),
        (41, D),
    ]
    .into_iter()
    .map(|(id, owner)| {
        seed(
            id,
            owner,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        )
    })
    .collect();
    if replacement {
        use crate::program::{
            PublicZoneDestination, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
            ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
            ZoneReplacementSubject,
        };
        let mut source = Program::enchantment(None);
        source.zone_replacements.push(ZoneReplacementAbility {
            id: ZoneReplacementAbilityId(0),
            actions: ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectDiscard])),
            objects: ZoneObjectKind::Card,
            subject: ZoneReplacementSubject::AnyObject,
            destination: PublicZoneDestination::Graveyard,
            replace_with: PublicZoneDestination::Exile,
            choice: ZoneReplacementChoice::OptionalAffectedPlayer,
        });
        cards.push(seed(
            50,
            D,
            ObjectDestination::Battlefield { controller: D },
            source,
        ));
    }
    let state = fixture(
        BodyTarget::None,
        vec![
            gain(2),
            Instruction::Discard {
                players: AffectedPlayers::EachPlayer,
                amount: InstructionAmount::Literal(2.into()),
            },
            gain(3),
        ],
        vec![A, B, C, D],
        A,
        cards,
    );
    let state = pass_to_boundary(cast(&state, A, 1, None));
    let state = reply(
        &state,
        DecisionReply::SelectResolution(vec![id(&state, 10), id(&state, 11)]),
    )
    .into_state();
    advance(&state, B, GameCommand::Concede).into_state()
}
#[test]
fn rebased_selection_operand_and_choice_corruption_rejects_valid_next_reply_atomically() {
    let state = selected_cut();
    let actual = state.execution();
    let reply = DecisionReply::SelectResolution(vec![id(&state, 30), id(&state, 31)]);
    let frame = actual.selection_frame().unwrap();
    assert!(
        actual
            .resume_resolution_selection(actual.revision(), &frame, C, reply.clone())
            .is_ok()
    );
    for mutation in 0..7 {
        let mut bad = actual.clone();
        let Some(PendingWork::ResolutionSelection(work)) = &mut bad.pending_work else {
            panic!("actual selection")
        };
        match mutation {
            // All original domains clamp to two, so cardinality checks alone cannot detect this.
            0 => work.materialized.requested = 3.into(),
            1 => work.materialized.players.reverse(),
            2 => work.materialized.operands = None,
            3 => work.materialized.instruction = gain(99),
            4 => work.cursor.instruction += 1,
            5 => work.materialized.certificates[0]
                .objects
                .pop()
                .map(|_| ())
                .unwrap(),
            6 => work.materialized.certificates[0].before.reverse(),
            _ => unreachable!(),
        }
        let saved = bad.clone();
        assert_eq!(
            bad.resume_resolution_selection(bad.revision(), &frame, C, reply.clone()),
            Err(EffectError::InconsistentSelectionWork),
            "mutation {mutation}"
        );
        assert_eq!(bad, saved);
    }
}

#[test]
fn later_targeted_discard_skips_known_departed_recipient_and_runs_tail_once() {
    let cards = [(10, A), (20, B), (30, C), (40, D)]
        .into_iter()
        .map(|(card, owner)| {
            seed(
                card,
                owner,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            )
        })
        .collect();
    let state = fixture(
        BodyTarget::SinglePlayer,
        vec![
            gain(2),
            Instruction::Discard {
                players: AffectedPlayers::EachPlayer,
                amount: InstructionAmount::Literal(1.into()),
            },
            Instruction::Discard {
                players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                amount: InstructionAmount::Literal(1.into()),
            },
            gain(3),
        ],
        vec![A, B, C, D],
        A,
        cards,
    );
    let state = pass_to_boundary(cast(&state, A, 1, Some(TargetRef::Player(B))));
    let state = reply(
        &state,
        DecisionReply::SelectResolution(vec![id(&state, 10)]),
    )
    .into_state();
    let state = advance(&state, B, GameCommand::Concede).into_state();
    let state = reply(
        &state,
        DecisionReply::SelectResolution(vec![id(&state, 30)]),
    )
    .into_state();
    let candidate = reply(
        &state,
        DecisionReply::SelectResolution(vec![id(&state, 40)]),
    );
    let discards=events(&candidate).iter().flat_map(|event| &event.occurrences)
        .filter(|occurrence|matches!(occurrence.value,RulesOccurrence::ZoneChange(ref change) if matches!(change.reason,ZoneReason::Discard {..}))).count();
    assert_eq!(discards, 3);
    assert_eq!(
        candidate.state().execution().life_total(A),
        Some(&25.into())
    );
    assert!(candidate.state().execution().window().entries().is_empty());
    assert!(candidate.state().pending_decision().is_none());
}

#[test]
fn selected_replacement_retained_history_and_effective_prefix_cannot_be_corrupted() {
    let state = selected_cut_with_replacement(true);
    let state = reply(
        &state,
        DecisionReply::SelectResolution(vec![id(&state, 30), id(&state, 31)]),
    )
    .into_state();
    let state = reply(
        &state,
        DecisionReply::SelectResolution(vec![id(&state, 40), id(&state, 41)]),
    )
    .into_state();
    let RuleFrame::ZoneReplacement(frame) = state.pending_decision().unwrap().frame() else {
        panic!("actual applications")
    };
    let ZoneReplacementRequest::Select { applications } = frame.request() else {
        panic!("two A opportunities")
    };
    let state = reply(
        &state,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Select(applications[0])),
    )
    .into_state();
    let state = reply(
        &state,
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline)),
    )
    .into_state();
    let state = advance(&state, C, GameCommand::Concede).into_state();
    let actual = state.execution();
    let frame = actual.selection_frame().unwrap();
    let response =
        DecisionReply::ZoneReplacement(ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline));
    assert!(
        actual
            .resume_resolution_selection(actual.revision(), &frame, A, response.clone())
            .is_ok()
    );
    use crate::effects::zone_replacement::SelectionReplacementFault;
    for fault in [
        SelectionReplacementFault::CandidateDestination,
        SelectionReplacementFault::CurrentSourceDefinition,
        SelectionReplacementFault::OriginalUniverse,
        SelectionReplacementFault::EventNamespace,
        SelectionReplacementFault::ApplicationSourceWitness,
        SelectionReplacementFault::RetainedDestination,
        SelectionReplacementFault::RetainedApplicationWitness,
        SelectionReplacementFault::RetainedFirstSource,
    ] {
        let mut bad = actual.clone();
        let Some(PendingWork::ResolutionSelection(work)) = &mut bad.pending_work else {
            panic!("selection")
        };
        let ResolutionSelectionStage::Replacing { batch, .. } = &mut work.stage else {
            panic!("replacing")
        };
        batch.qualification_corrupt_selection(fault);
        let saved = bad.clone();
        assert_eq!(
            bad.resume_resolution_selection(bad.revision(), &frame, A, response.clone()),
            Err(EffectError::ZoneReplacement(
                ZoneReplacementError::InconsistentBatch
            )),
            "{fault:?}"
        );
        assert_eq!(bad, saved);
    }
    for corrupt_history in [true, false] {
        let mut bad = actual.clone();
        let Some(PendingWork::ResolutionSelection(work)) = &mut bad.pending_work else {
            panic!("selection")
        };
        let ResolutionSelectionStage::Replacing { batch, selections } = &mut work.stage else {
            panic!("replacing")
        };
        let expected = if corrupt_history {
            batch.qualification_corrupt_history();
            EffectError::ZoneReplacement(ZoneReplacementError::InconsistentBatch)
        } else {
            selections[0].objects.pop().unwrap();
            EffectError::InconsistentSelectionWork
        };
        let saved = bad.clone();
        assert_eq!(
            bad.resume_resolution_selection(bad.revision(), &frame, A, response.clone()),
            Err(expected)
        );
        assert_eq!(bad, saved);
    }
}
