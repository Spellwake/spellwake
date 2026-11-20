//! Projection/composer mechanics over strict synthetic trusted-RPC controls.
//! These are not native execution, real SQL cursor or HTTP authority qualification.
use super::super::replay_archive::{
    ArchivedCut,
    projection_fixture::{id, inputs},
};
use super::*;
use crate::{
    api::NativeStreamProbe,
    player_wire::{PlayerReplayFacts, PlayerReplayFrame, PlayerReplayMode, PlayerReplaySource},
    test_http::{Mock, Step},
};
use serde_json::Value;
use spellwake_contracts::creature_types::CreatureTypeVocabulary;

fn query(
    cuts: &[ArchivedCut],
    cursor: Option<crate::player_wire::PlayerCursor>,
    limit: u32,
) -> PlayerHistoryQuery {
    let snapshot = cuts.last().unwrap().snapshot();
    crate::decode_wire_object(&serde_json::to_vec(&json!({"gameId":snapshot.basis.game_id,"contextId":snapshot.basis.context_id,"cursor":cursor,"limit":limit.to_string()})).unwrap()).unwrap()
}
fn raw(cuts: &[ArchivedCut], anchor: usize, after: Option<usize>, limit: usize) -> Value {
    let snapshot = cuts[anchor].snapshot();
    let start = after.map_or(0, |n| n + 1);
    let end = (start + limit).min(anchor + 1);
    assert!(start < end);
    let items:Vec<_>=cuts[start..end].iter().map(|cut| {
        let line=cut.lineage();
        json!({"eventId":line.event_id,"sequence":cut.snapshot().through_event_sequence,
            "acceptedRevision":cut.snapshot().basis.revision,"eventRef":line.event_ref.as_str(),
            "creatorReceiptId":line.creator_receipt_id,"checkpointId":line.checkpoint_id,"stateRef":line.state_ref.as_str()})
    }).collect();
    json!({"gameId":snapshot.basis.game_id,"contextId":snapshot.basis.context_id,"viewerSeatId":snapshot.viewer_seat_id,
        "disclosure":"live","afterSequence":after.map(|n|n.to_string()),"limit":limit,
        "sourceViewReceiptId":cuts[anchor].view_receipt(),"openingReceiptId":id(7000),"items":items,
        "scannedThrough":(end-1).to_string(),"nextAfter":(end<=anchor).then(||(end-1).to_string()),"replay":null,"sourceHash":"a".repeat(64)})
}
fn scan(cuts: &[ArchivedCut], anchor: usize, after: Option<usize>, limit: usize) -> Step {
    let snapshot = cuts[anchor].snapshot();
    let mut step = Step::json(
        "/rest/v1/rpc/read_native_archive_source",
        raw(cuts, anchor, after, limit),
    );
    step.expect=Some(serde_json::to_vec(&json!({"p_game":snapshot.basis.game_id,"p_context":snapshot.basis.context_id,
        "p_viewer":snapshot.viewer_seat_id,"p_after_sequence":after.map(|n|n.to_string()),"p_limit":limit,
        "p_disclosure":"live","p_source_view_receipt":cuts[anchor].view_receipt()})).unwrap());
    step
}
fn issuer(
    cuts: &[ArchivedCut],
    anchor: usize,
    after: Option<usize>,
    limit: usize,
    token: Uuid,
) -> Step {
    let snapshot = cuts[anchor].snapshot();
    let record = json!({"version":"native-cursor-v1","basis":snapshot.basis,"viewerSeatId":snapshot.viewer_seat_id,
        "throughEventSequence":anchor.to_string(),"purpose":"history","sourceReceiptId":cuts[anchor].view_receipt(),
        "disclosure":"live","token":format!("n1.{token}")});
    let mut step = Step::json(
        "/rest/v1/rpc/issue_native_cursor_source",
        json!({"status":"success","data":record}),
    );
    step.expect=Some(serde_json::to_vec(&json!({"p_game":snapshot.basis.game_id,"p_context":snapshot.basis.context_id,
        "p_epoch":snapshot.basis.epoch,"p_revision":snapshot.basis.revision,"p_viewer":snapshot.viewer_seat_id,
        "p_through_sequence":anchor.to_string(),"p_purpose":"history","p_source_receipt":cuts[anchor].view_receipt(),
        "p_scan":raw(cuts,anchor,after,limit)})).unwrap());
    step
}
fn token(n: u8) -> Uuid {
    match n {
        0 => "70f16899-bd63-4fbc-bd00-d50bf7f9d8dc",
        1 => "13a5d50a-cfb3-47ee-b7cd-e482b7dcda6b",
        _ => "44ba8af8-445f-42c6-bd8a-06bd5fc5f57c",
    }
    .parse()
    .unwrap()
}
fn source<'a>(
    cuts: &'a [ArchivedCut],
    vocabulary: &'a CreatureTypeVocabulary,
) -> CutSource<'a, ArchivedCut> {
    CutSource {
        cuts,
        disclosure: NativeDisclosure::LiveParticipant,
        opening_receipt_id: id(7000),
        vocabulary,
    }
}
fn cost(steps: &[Step]) -> (usize, u64) {
    (
        steps.len(),
        steps
            .iter()
            .map(|step| u64::try_from(step.body.len()).unwrap())
            .sum(),
    )
}
fn page(
    repository: &NativeSourceRepository,
    source: &CutSource<'_, ArchivedCut>,
    query: &PlayerHistoryQuery,
    previous: Option<&IssuedNativeCursor>,
    reserved: usize,
    budget: &mut NativeRecoveryBudget,
) -> Result<(PlayerHistoryPage, Vec<usize>), NativeSourceError> {
    repository.compose_cut_page(
        source,
        PageScope {
            at_index: source.cuts.len() - 1,
            access: None,
            reserved,
        },
        query,
        previous,
        budget,
    )
}
fn resolver(cuts: &[ArchivedCut], anchor: usize, token: Uuid) -> Step {
    let mut step = issuer(cuts, anchor, anchor.checked_sub(1), 1, token);
    step.path = "/rest/v1/rpc/read_native_cursor_source".into();
    let snapshot = cuts[anchor].snapshot();
    step.expect=Some(serde_json::to_vec(&json!({"p_game":snapshot.basis.game_id,"p_viewer":snapshot.viewer_seat_id,"p_token":format!("n1.{token}")})).unwrap());
    step
}
fn resolve(
    repository: &NativeSourceRepository,
    source: &CutSource<'_, ArchivedCut>,
    cursor: &crate::player_wire::PlayerCursor,
    budget: &mut NativeRecoveryBudget,
) -> IssuedNativeCursor {
    repository
        .read_cut_history_cursor(source, source.cuts.len() - 1, cursor, Some(budget))
        .unwrap()
        .unwrap()
}

#[test]
fn silent_pages_stop_after_sixteen_primary_scans_then_resume_exact_raw_position() {
    let (cuts, vocabulary) = inputs(103, None);
    let source = source(&cuts, &vocabulary);
    let mut steps = Vec::new();
    for start in [0_usize, 16] {
        for position in start..start + 16 {
            steps.push(scan(&cuts, 102, position.checked_sub(1), 1));
        }
        steps.push(scan(&cuts, start + 15, Some(start + 14), 1));
        steps.push(issuer(
            &cuts,
            start + 15,
            Some(start + 14),
            1,
            token(u8::from(start > 0)),
        ));
        if start == 0 {
            steps.push(resolver(&cuts, 15, token(0)));
        }
    }
    let (records, bytes) = cost(&steps);
    assert_eq!(records, 37);
    let mock = Mock::new(steps);
    let repository = super::super::tests::repository(&mock);
    let mut budget = NativeRecoveryBudget::new(records, bytes).unwrap();
    let first_query = query(&cuts, None, 1);
    let (first, indexes) = page(&repository, &source, &first_query, None, 0, &mut budget).unwrap();
    assert!(first.events.is_empty() && indexes.is_empty() && first.has_more);
    assert_eq!(
        first.scanned_through.through_event_sequence,
        Amount::from(15)
    );
    assert_eq!(first.scanned_through.basis, cuts[15].snapshot().basis);
    assert_eq!(budget.remaining().0, 19);
    let previous = resolve(&repository, &source, &first.scanned_through, &mut budget);
    assert!(previous.cursor() == first.scanned_through);
    let next_query = query(&cuts, Some(first.scanned_through), 1);
    let (next, indexes) = page(
        &repository,
        &source,
        &next_query,
        Some(&previous),
        0,
        &mut budget,
    )
    .unwrap();
    assert!(next.events.is_empty() && indexes.is_empty() && next.has_more);
    assert_eq!(
        next.scanned_through.through_event_sequence,
        Amount::from(31)
    );
    assert_eq!(next.scanned_through.basis, cuts[31].snapshot().basis);
    assert_eq!(budget.remaining(), (0, 0));
    assert_eq!(mock.finish().len(), 37);
}

fn silent_steps(cuts: &[ArchivedCut]) -> Vec<Step> {
    let mut steps: Vec<_> = (0_usize..16)
        .map(|n| scan(cuts, 102, n.checked_sub(1), 1))
        .collect();
    steps.push(scan(cuts, 15, Some(14), 1));
    steps.push(issuer(cuts, 15, Some(14), 1, token(0)));
    steps
}
#[test]
fn record_starvation_is_causal_before_primary_anchor_and_issuer_transport() {
    let (cuts, vocabulary) = inputs(103, None);
    let source = source(&cuts, &vocabulary);
    for allowed in [1, 15, 16, 17] {
        let mut steps = silent_steps(&cuts);
        steps.truncate(allowed);
        let (_, spent_bytes) = cost(&steps);
        let mock = Mock::new(steps);
        let repository = super::super::tests::repository(&mock);
        let mut budget = NativeRecoveryBudget::new(allowed, 1_000_000).unwrap();
        let error = page(
            &repository,
            &source,
            &query(&cuts, None, 1),
            None,
            0,
            &mut budget,
        )
        .err()
        .unwrap();
        assert!(matches!(
            error.0,
            Failure::Checkpoint(NativeCheckpointError::Budget)
        ));
        assert_eq!(budget.remaining(), (0, 1_000_000 - spent_bytes));
        assert_eq!(
            mock.finish().len(),
            allowed,
            "failed record reservation performs no next request"
        );
    }
}
#[test]
fn one_byte_short_complete_page_refuses_in_transport_after_reserving_last_record() {
    let (cuts, vocabulary) = inputs(103, None);
    let source = source(&cuts, &vocabulary);
    let steps = silent_steps(&cuts);
    let (records, bytes) = cost(&steps);
    let last_bytes = u64::try_from(steps.last().unwrap().body.len()).unwrap();
    let mock = Mock::new(steps);
    let repository = super::super::tests::repository(&mock);
    let mut budget = NativeRecoveryBudget::new(records, bytes - 1).unwrap();
    let error = page(
        &repository,
        &source,
        &query(&cuts, None, 1),
        None,
        0,
        &mut budget,
    )
    .err()
    .unwrap();
    assert!(matches!(
        error.0,
        Failure::Backend(BackendError::RequestCause { stage: "body", .. })
    ));
    assert_eq!(
        budget.remaining(),
        (0, last_bytes - 1),
        "last record was reserved; oversized reply bytes were never charged"
    );
    assert_eq!(mock.finish().len(), 18);
}

fn frame(
    source: &CutSource<'_, ArchivedCut>,
    page: PlayerHistoryPage,
    indexes: &[usize],
    previous: Option<&crate::player_wire::PlayerCursor>,
) -> PlayerReplayFrame {
    let snapshot = source.cuts.last().unwrap().snapshot();
    let request = crate::player_wire::PlayerReplayRequest {
        game_id: snapshot.basis.game_id,
        context_id: snapshot.basis.context_id,
        at_event_sequence: snapshot.through_event_sequence.clone(),
        perspective_seat_id: snapshot.viewer_seat_id,
        mode: PlayerReplayMode::HistoricalSeat,
        history: page.query.clone(),
    };
    let result = PlayerReplayFrame {
        request: request.clone(),
        release_id: id(9000),
        schema_revision: "synthetic-page-controls-v1".to_owned().try_into().unwrap(),
        snapshot: Box::new(snapshot.clone()),
        history: page,
    };
    let facts = indexes
        .iter()
        .map(|index| cut_event_facts(source, *index).unwrap())
        .collect::<Vec<_>>();
    let account = id(9001);
    result
        .validate(
            &request,
            &PlayerReplayFacts {
                requester_account_id: account,
                participant_account_ids: &[account],
                historical_seat_id: snapshot.viewer_seat_id,
                historical_seat_owner: Some(account),
                completed: false,
                full_information_eligible: false,
                source: PlayerReplaySource {
                    game_id: request.game_id,
                    context_id: request.context_id,
                    release_id: result.release_id,
                    schema_revision: &result.schema_revision,
                },
                projection: source
                    .cuts
                    .last()
                    .unwrap()
                    .cut_facts(source.vocabulary)
                    .unwrap(),
                history: PlayerHistoryFacts {
                    viewer: snapshot.viewer_seat_id,
                    requested_cursor: previous,
                    scanned_cursor: &result.history.scanned_through,
                    has_more: result.history.has_more,
                    events: &facts,
                },
            },
        )
        .unwrap();
    result
}
fn encoded<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(&ApiResponse::Success { data: value }).unwrap()
}
fn response_bytes(response: axum::response::Response) -> Vec<u8> {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        axum::body::to_bytes(response.into_body(), 2 * MAX_PLAYER_WIRE_BYTES)
            .await
            .unwrap()
            .to_vec()
    })
}

#[test]
fn refit_repeats_exact_fitting_prefix_and_complete_frames_remain_bounded() {
    let (cuts, vocabulary) = inputs(4, Some(MAX_PLAYER_WIRE_BYTES / 4 + 128));
    let source = source(&cuts, &vocabulary);
    let snapshot_bytes = serde_json::to_vec(cuts.last().unwrap().snapshot())
        .unwrap()
        .len();
    let first_event = PlayerVisibleEvent::from_projections(&cut_event_facts(&source, 1).unwrap())
        .unwrap()
        .unwrap();
    let event_bytes = serde_json::to_vec(&first_event).unwrap().len();
    assert!(snapshot_bytes + PAGE_ENVELOPE_BYTES + event_bytes < MAX_PLAYER_WIRE_BYTES);
    assert!(PAGE_ENVELOPE_BYTES + 2 * event_bytes > MAX_PLAYER_WIRE_BYTES);
    for reserved in [0, snapshot_bytes] {
        let steps = vec![
            scan(&cuts, 3, None, 3),
            scan(&cuts, 3, None, 2),
            scan(&cuts, 1, None, 2),
            issuer(&cuts, 1, None, 2, token(0)),
            resolver(&cuts, 1, token(0)),
            scan(&cuts, 3, Some(1), 3),
            scan(&cuts, 3, Some(1), 1),
            scan(&cuts, 2, Some(1), 1),
            issuer(&cuts, 2, Some(1), 1, token(1)),
            resolver(&cuts, 2, token(1)),
            scan(&cuts, 3, Some(2), 3),
            scan(&cuts, 3, Some(2), 1),
            issuer(&cuts, 3, Some(2), 1, token(2)),
        ];
        let mock = Mock::new(steps);
        let repository = super::super::tests::repository(&mock);
        let mut budget = NativeRecoveryBudget::new(100, 1_000_000).unwrap();
        let mut previous: Option<IssuedNativeCursor> = None;
        let mut order = Vec::new();
        for index in 1_usize..=3 {
            let public = previous.as_ref().map(IssuedNativeCursor::cursor);
            let query = query(&cuts, public.clone(), 3);
            let (page, indexes) = page(
                &repository,
                &source,
                &query,
                previous.as_ref(),
                reserved,
                &mut budget,
            )
            .unwrap();
            assert_eq!(indexes, vec![index]);
            assert_eq!(page.events.len(), 1);
            assert_eq!(
                page.events[0].event_sequence,
                Amount::from(u64::try_from(index).unwrap())
            );
            assert_eq!(page.scanned_through.basis, cuts[index].snapshot().basis);
            assert_eq!(page.has_more, index < 3);
            assert!(encoded(&page).len() <= MAX_PLAYER_WIRE_BYTES);
            order.extend(page.events.iter().map(|event| event.event_sequence.clone()));
            let cursor = page.scanned_through.clone();
            let frame = frame(&source, page, &indexes, public.as_ref());
            let expected = encoded(&frame);
            assert!(expected.len() <= MAX_PLAYER_WIRE_BYTES);
            let response = NativeStreamProbe::bounded_candidate(&frame);
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            assert_eq!(response_bytes(response), expected);
            if index < 3 {
                previous = Some(resolve(&repository, &source, &cursor, &mut budget));
            }
        }
        assert_eq!(
            order,
            vec![Amount::from(1), Amount::from(2), Amount::from(3)]
        );
        assert_eq!(mock.finish().len(), 13);
    }
}

#[test]
fn indivisible_reserved_event_refuses_without_invalid_partial_frame_and_serializer_caps_full_envelope()
 {
    let (cuts, vocabulary) = inputs(2, Some(MAX_PLAYER_WIRE_BYTES / 2 - 20_000));
    let source = source(&cuts, &vocabulary);
    let reserved = serde_json::to_vec(cuts[1].snapshot()).unwrap().len();
    let event = PlayerVisibleEvent::from_projections(&cut_event_facts(&source, 1).unwrap())
        .unwrap()
        .unwrap();
    let bytes = serde_json::to_vec(&event).unwrap().len();
    assert!(bytes + PAGE_ENVELOPE_BYTES < MAX_PLAYER_WIRE_BYTES);
    assert!(bytes + reserved > MAX_PLAYER_WIRE_BYTES);
    let steps = vec![
        scan(&cuts, 0, None, 1),
        scan(&cuts, 0, None, 1),
        issuer(&cuts, 0, None, 1, token(0)),
        resolver(&cuts, 0, token(0)),
        scan(&cuts, 1, Some(0), 1),
        scan(&cuts, 1, Some(0), 1),
        scan(&cuts, 1, Some(0), 1),
        issuer(&cuts, 1, Some(0), 1, token(1)),
    ];
    let mock = Mock::new(steps);
    let repository = super::super::tests::repository(&mock);
    let mut budget = NativeRecoveryBudget::new(100, 1_000_000).unwrap();
    let (baseline, indexes) = page(
        &repository,
        &self::source(&cuts[..1], &vocabulary),
        &query(&cuts, None, 1),
        None,
        reserved,
        &mut budget,
    )
    .unwrap();
    assert!(baseline.events.is_empty() && indexes.is_empty() && !baseline.has_more);
    assert_eq!(
        baseline.scanned_through.through_event_sequence,
        Amount::from(0)
    );
    let previous = resolve(&repository, &source, &baseline.scanned_through, &mut budget);
    let query = query(&cuts, Some(previous.cursor()), 1);
    let error = page(
        &repository,
        &source,
        &query,
        Some(&previous),
        reserved,
        &mut budget,
    )
    .err()
    .unwrap();
    assert!(matches!(error.0, Failure::Invalid));
    // Standalone history can fit this individually valid event. Combining it
    // with the individually valid snapshot must still obey the final API cap.
    let (history, indexes) = page(
        &repository,
        &source,
        &query,
        Some(&previous),
        0,
        &mut budget,
    )
    .unwrap();
    assert!(encoded(&history).len() < MAX_PLAYER_WIRE_BYTES);
    let frame = frame(&source, history, &indexes, Some(&previous.cursor()));
    assert!(encoded(&frame).len() > MAX_PLAYER_WIRE_BYTES);
    let response = NativeStreamProbe::bounded_candidate(&frame);
    assert_eq!(
        response.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    let response: Value = serde_json::from_slice(&response_bytes(response)).unwrap();
    assert_eq!(
        response,
        json!({"status":"error","message":"Application service is temporarily unavailable","code":"unavailable"})
    );
    assert_eq!(mock.finish().len(), 8);
}

#[test]
fn replay_output_mutations_are_checked_against_frozen_original_source_and_issuer_facts() {
    let (cuts, vocabulary) = inputs(4, Some(2));
    let source = source(&cuts, &vocabulary);
    let mock = Mock::new(vec![
        scan(&cuts, 3, None, 3),
        scan(&cuts, 3, Some(2), 1),
        scan(&cuts, 3, Some(2), 1),
        issuer(&cuts, 3, Some(2), 1, token(0)),
    ]);
    let repository = super::super::tests::repository(&mock);
    let mut budget = NativeRecoveryBudget::new(100, 1_000_000).unwrap();
    let (history, indexes) = page(
        &repository,
        &source,
        &query(&cuts, None, 3),
        None,
        0,
        &mut budget,
    )
    .unwrap();
    assert_eq!(indexes, vec![1, 2, 3]);
    let original = frame(&source, history, &indexes, None);
    let request = original.request.clone();
    let snapshot = cuts.last().unwrap().snapshot();
    let schema = original.schema_revision.clone();
    let cursor = original.history.scanned_through.clone();
    let events = indexes
        .iter()
        .map(|index| cut_event_facts(&source, *index).unwrap())
        .collect::<Vec<_>>();
    let account = id(9001);
    // Capture every expected operand before mutating any output. No helper
    // recomputes the request, hasMore, issuer or event facts from a candidate.
    let facts = PlayerReplayFacts {
        requester_account_id: account,
        participant_account_ids: &[account],
        historical_seat_id: snapshot.viewer_seat_id,
        historical_seat_owner: Some(account),
        completed: false,
        full_information_eligible: false,
        source: PlayerReplaySource {
            game_id: request.game_id,
            context_id: request.context_id,
            release_id: id(9000),
            schema_revision: &schema,
        },
        projection: cuts.last().unwrap().cut_facts(&vocabulary).unwrap(),
        history: PlayerHistoryFacts {
            viewer: snapshot.viewer_seat_id,
            requested_cursor: None,
            scanned_cursor: &cursor,
            has_more: false,
            events: &events,
        },
    };
    assert!(original.validate(&request, &facts).is_ok());
    let encoded = serde_json::to_value(&original).unwrap();
    for (path, value) in [
        (
            "/request/perspectiveSeatId",
            json!(cuts[0].snapshot().players[1].seat_id),
        ),
        ("/request/atEventSequence", json!("2")),
        ("/snapshot/basis/gameId", json!(id(9991))),
        ("/history/hasMore", json!(true)),
        ("/history/scannedThrough/throughEventSequence", json!("2")),
        ("/history/events/0/changes/0/after", json!("999")),
        ("/history/events/1/eventSequence", json!("1")),
    ] {
        let mut candidate = encoded.clone();
        *candidate.pointer_mut(path).expect("declared output field") = value;
        let candidate: PlayerReplayFrame =
            crate::decode_wire_object(&serde_json::to_vec(&candidate).unwrap()).unwrap();
        assert!(candidate.validate(&request, &facts).is_err(), "{path}");
        assert!(
            original.validate(&request, &facts).is_ok(),
            "unchanged adjacent control {path}"
        );
    }
    assert_eq!(mock.finish().len(), 4);
}
