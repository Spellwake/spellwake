//! Production loaded-cut closure over explicit synthetic SQL/projection controls.
//! These tests establish verifier mechanics, never native execution qualification.
use super::*;
use crate::{
    native_game::source_reader::cuts::HistoryCut,
    player_wire::{PlayerEventFacts, PlayerVisibleEvent},
    test_http::{Mock, Step},
};
use serde_json::Value;

use super::super::projection_fixture::{id, inputs};

fn scan(cuts: &[ArchivedCut], start: usize, end: usize) -> Value {
    let tip = cuts.last().unwrap().snapshot();
    let items: Vec<_> = cuts[start..end].iter().map(|cut| json!({
        "eventId":cut.event_id,"sequence":cut.snapshot().through_event_sequence,
        "acceptedRevision":cut.snapshot().basis.revision,"eventRef":cut.event_ref.as_str(),
        "creatorReceiptId":cut.creator_receipt_id,"checkpointId":cut.checkpoint_id,"stateRef":cut.state_ref.as_str()
    })).collect();
    json!({"gameId":tip.basis.game_id,"contextId":tip.basis.context_id,"viewerSeatId":tip.viewer_seat_id,
        "disclosure":"live","afterSequence":start.checked_sub(1).map(|n|n.to_string()),"limit":100,
        "sourceViewReceiptId":cuts.last().unwrap().view_receipt(),"openingReceiptId":id(7000),"items":items,
        "scannedThrough":(end-1).to_string(),"nextAfter":(end<cuts.len()).then(||(end-1).to_string()),"replay":null,"sourceHash":"a".repeat(64)})
}
fn step(cuts: &[ArchivedCut], after: Option<&str>, value: Value) -> Step {
    let tip = cuts.last().unwrap().snapshot();
    let mut step = Step::json("/rest/v1/rpc/read_native_archive_source", value);
    step.expect = Some(
        serde_json::to_vec(
            &json!({"p_game":tip.basis.game_id,"p_context":tip.basis.context_id,
        "p_viewer":tip.viewer_seat_id,"p_after_sequence":after,"p_limit":100,"p_disclosure":"live",
        "p_source_view_receipt":cuts.last().unwrap().view_receipt()}),
        )
        .unwrap(),
    );
    step
}
fn check(
    cuts: &[ArchivedCut],
    vocabulary: &CreatureTypeVocabulary,
    responses: Vec<Value>,
    accepted: bool,
) {
    let count = responses.len();
    let steps = responses
        .into_iter()
        .enumerate()
        .map(|(i, value)| step(cuts, if i == 0 { None } else { Some("99") }, value))
        .collect();
    let mock = Mock::new(steps);
    let repository = super::super::super::tests::repository(&mock);
    let source = CutSource {
        cuts,
        disclosure: NativeDisclosure::LiveParticipant,
        opening_receipt_id: id(7000),
        vocabulary,
    };
    let mut budget = NativeRecoveryBudget::new(10, 500_000).unwrap();
    take_test_fault();
    let result = repository.verify_portable_cut_sources(&source, &mut budget);
    assert_eq!(result.is_ok(), accepted);
    assert_eq!(
        take_test_fault(),
        if accepted {
            None
        } else {
            Some("accepted cut source")
        }
    );
    assert_eq!(budget.remaining().0, 10 - count);
    assert_eq!(mock.finish().len(), count);
}
#[test]
fn closure_authenticates_baseline_and_silent_cuts_across_all_loaded_chunks() {
    let (cuts, vocabulary) = inputs(103, None);
    for pair in cuts.windows(2) {
        assert!(
            PlayerVisibleEvent::from_projections(&PlayerEventFacts {
                before: pair[0].snapshot(),
                before_projection: pair[0].cut_facts(&vocabulary).unwrap(),
                after: pair[1].snapshot(),
                after_projection: pair[1].cut_facts(&vocabulary).unwrap(),
                terminal: None,
            })
            .unwrap()
            .is_none()
        );
    }
    check(
        &cuts,
        &vocabulary,
        vec![scan(&cuts, 0, 100), scan(&cuts, 100, 103)],
        true,
    );
}
#[test]
fn closure_refuses_changed_baseline_silent_and_last_loaded_lineage_then_restores() {
    let (cuts, vocabulary) = inputs(103, None);
    let first = scan(&cuts, 0, 100);
    let second = scan(&cuts, 100, 103);
    for index in [0, 1, 102] {
        for field in [
            "eventId",
            "eventRef",
            "stateRef",
            "checkpointId",
            "creatorReceiptId",
            "acceptedRevision",
        ] {
            let (mut page, row) = if index < 100 {
                (first.clone(), index)
            } else {
                (second.clone(), index - 100)
            };
            let other = if index == 0 { 1 } else { 0 };
            assert_ne!(page["items"][row][field], first["items"][other][field]);
            page["items"][row][field] = first["items"][other][field].clone();
            let responses = if index < 100 {
                vec![page]
            } else {
                vec![first.clone(), page]
            };
            check(&cuts, &vocabulary, responses, false);
            check(
                &cuts,
                &vocabulary,
                vec![first.clone(), second.clone()],
                true,
            );
        }
    }
}
