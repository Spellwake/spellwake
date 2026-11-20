//! Loopback controls of the production original-source verifier; no runtime qualification.
use super::*;
use crate::{
    native_game::source_reader::{
        tests::{fixture, game, repository},
        view::ArchivedViewClaim,
    },
    player_wire::PlayerBasis,
    test_http::{Mock, Step},
};
use serde_json::Value;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
fn reference(role: NativeArtifactRole) -> NativeArtifactRef {
    NativeArtifactRef::for_bytes(game(), id(0x125), role, b"source-control").unwrap()
}
fn basis() -> PlayerBasis {
    PlayerBasis {
        game_id: game(),
        context_id: id(0x125),
        epoch: 42.into(),
        revision: 0.into(),
    }
}
fn budget() -> NativeRecoveryBudget {
    NativeRecoveryBudget::new(8, 100_000).unwrap()
}
fn view_record() -> Value {
    json!({"status":"success","data":{
        "version":"native-view-v1", "basis":basis(), "viewerSeatId":id(1),
        "disclosure":"live", "openingReceiptId":id(30), "checkpointId":id(31),
        "throughEventSequence":"0", "viewRef":reference(NativeArtifactRole::View).as_str(),
        "receiptId":id(32)
    }})
}
fn view_claim<'a>(
    b: &'a PlayerBasis,
    r: &'a NativeArtifactRef,
    through: &'a Amount,
) -> ArchivedViewClaim<'a> {
    ArchivedViewClaim {
        basis: b,
        viewer: id(1),
        disclosure: NativeDisclosure::LiveParticipant,
        opening: id(30),
        checkpoint: id(31),
        through,
        receipt: id(32),
        reference: r,
    }
}
fn view_step(value: Value) -> Step {
    let mut step = Step::json("/rest/v1/rpc/read_native_view_source", value);
    step.expect = Some(
        serde_json::to_vec(&json!({"p_game":game(),"p_context":id(0x125),
        "p_epoch":"42","p_revision":"0","p_viewer":id(1),"p_disclosure":"live"}))
        .unwrap(),
    );
    step
}

#[test]
fn archived_view_source_checks_every_sql_identity_field_and_restores() {
    let good = view_record();
    for (pointer, value) in [
        ("/data/version", json!("native-view-v2")),
        ("/data/basis/gameId", json!(id(99))),
        ("/data/basis/contextId", json!(id(99))),
        ("/data/basis/epoch", json!("43")),
        ("/data/basis/revision", json!("1")),
        ("/data/viewerSeatId", json!(id(2))),
        ("/data/disclosure", json!("completed")),
        ("/data/openingReceiptId", json!(id(99))),
        ("/data/checkpointId", json!(id(99))),
        ("/data/throughEventSequence", json!("1")),
        ("/data/receiptId", json!(id(99))),
        (
            "/data/viewRef",
            json!(reference(NativeArtifactRole::State).as_str()),
        ),
    ] {
        let mut bad = good.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        let mock = Mock::new(vec![
            view_step(good.clone()),
            view_step(bad),
            view_step(good.clone()),
        ]);
        let repo = repository(&mock);
        let b = basis();
        let r = reference(NativeArtifactRole::View);
        let through = Amount::from(0);
        for accepted in [true, false, true] {
            assert_eq!(
                repo.archived_view_source(view_claim(&b, &r, &through), &mut budget())
                    .is_ok(),
                accepted,
                "{pointer}"
            );
        }
        assert_eq!(mock.finish().len(), 3);
    }
}

#[test]
fn archived_view_scope_and_exhausted_work_or_bytes_refuse_before_io() {
    let mock = Mock::new(vec![]);
    let repo = repository(&mock);
    let b = basis();
    let r = reference(NativeArtifactRole::View);
    let through = Amount::from(0);
    let mut nil = view_claim(&b, &r, &through);
    nil.viewer = Uuid::nil();
    assert!(repo.archived_view_source(nil, &mut budget()).is_err());
    let wrong_role = reference(NativeArtifactRole::State);
    assert!(
        repo.archived_view_source(view_claim(&b, &wrong_role, &through), &mut budget())
            .is_err()
    );
    for changed in [
        PlayerBasis {
            game_id: id(99),
            ..b.clone()
        },
        PlayerBasis {
            context_id: id(99),
            ..b.clone()
        },
        PlayerBasis {
            revision: 1.into(),
            ..b.clone()
        },
    ] {
        assert!(
            repo.archived_view_source(view_claim(&changed, &r, &through), &mut budget())
                .is_err()
        );
    }
    for bytes in [false, true] {
        let mut exhausted = NativeRecoveryBudget::new(1, 1).unwrap();
        exhausted
            .charge_work(usize::from(!bytes), u64::from(bytes))
            .unwrap();
        let result = repo.archived_view_source(view_claim(&b, &r, &through), &mut exhausted);
        assert!(matches!(
            result,
            Err(NativeSourceError(Failure::Checkpoint(
                NativeCheckpointError::Budget
            )))
        ));
    }
    assert!(mock.finish().is_empty());
}

fn index() -> EncodedIndex {
    EncodedIndex {
        version: REPLAY_SCHEMA.into(),
        lineage: EncodedLineage {
            native_epoch: 42.into(),
            runtime_id: "b".repeat(64),
            program_source_id: "c".repeat(64),
            qualification_ref: "fixture:source".into(),
            initial_ref: reference(NativeArtifactRole::Initial),
            initial_state_ref: reference(NativeArtifactRole::State),
        },
        game_id: game(),
        context_id: id(0x125),
        release_id: id(0x127),
        opening_receipt_id: id(30),
        initial_checkpoint_id: id(31),
        through_event_sequence: 70.into(),
        through_revision: 12.into(),
        vocabulary_ref: reference(NativeArtifactRole::Replay),
        vocabulary_id: serde_json::from_value(json!("e".repeat(64))).unwrap(),
        program_identity_ref: reference(NativeArtifactRole::Replay),
        conclusion: NATIVE_CONCLUSION.into(),
        perspectives: vec![],
    }
}
fn result_rows() -> Value {
    json!([{"id":id(40),"game_id":game(),"source_event_id":id(41),"outcome":"win","winner_seat_ids":[id(1)]}])
}
fn event_rows() -> Value {
    json!([{"id":id(41),"game_id":game(),"context_id":id(0x125),"sequence":"70",
        "accepted_revision":"12","source_receipt_id":id(0x130)}])
}
fn rows_step(table: &str, filters: &[(&str, String)], value: Value) -> Step {
    let mut url = reqwest::Url::parse(&format!("http://localhost/rest/v1/{table}")).unwrap();
    url.query_pairs_mut()
        .extend_pairs(filters.iter().map(|(key, value)| (*key, value.as_str())));
    Step::get(
        format!("{}?{}", url.path(), url.query().unwrap()),
        200,
        serde_json::to_vec(&value).unwrap(),
    )
}
fn result_step(value: Value) -> Step {
    rows_step(
        "game_result",
        &[
            (
                "select",
                "id,game_id,source_event_id,outcome,winner_seat_ids".into(),
            ),
            ("game_id", format!("eq.{}", game())),
            ("limit", "2".into()),
        ],
        value,
    )
}
fn event_step(value: Value, event: Uuid) -> Step {
    rows_step(
        "game_event",
        &[
            (
                "select",
                "id,game_id,context_id,sequence::text,accepted_revision::text,source_receipt_id"
                    .into(),
            ),
            ("id", format!("eq.{event}")),
            ("game_id", format!("eq.{}", game())),
            ("context_id", format!("eq.{}", id(0x125))),
            ("limit", "2".into()),
        ],
        value,
    )
}
fn check_terminal(rows: Value, events: Option<Value>, conclusion: bool, accepted: bool) {
    let event = rows
        .get(0)
        .and_then(|row| row.get("source_event_id"))
        .and_then(Value::as_str)
        .map(|text| Uuid::parse_str(text).unwrap())
        .unwrap_or(id(41));
    let mut steps = vec![
        Step::json("/rest/v1/rpc/read_native_game_source", fixture()),
        result_step(rows),
    ];
    if let Some(events) = events {
        steps.push(event_step(events, event));
    }
    let count = steps.len();
    let mock = Mock::new(steps);
    let repo = repository(&mock);
    let source = repo.read(game()).unwrap().unwrap();
    let result = repo.portable_terminal_source(&source, &index(), conclusion, &mut budget());
    assert_eq!(result.is_ok(), accepted);
    if accepted && conclusion {
        let terminal = result.unwrap().unwrap();
        assert_eq!(terminal.event, id(41));
        assert_eq!(terminal.value.outcome, Outcome::Win);
        assert_eq!(terminal.value.winner_seat_ids, vec![id(1)]);
    }
    assert_eq!(mock.finish().len(), count);
}

#[test]
fn terminal_original_result_fields_cardinality_and_incident_absence_are_binding() {
    check_terminal(result_rows(), Some(event_rows()), true, true);
    for rows in [json!([]), json!([result_rows()[0], result_rows()[0]])] {
        check_terminal(rows, None, true, false);
    }
    for (field, value) in [
        ("id", json!(Uuid::nil())),
        ("game_id", json!(id(99))),
        ("outcome", json!("no_contest")),
        ("outcome", json!("draw")),
        ("winner_seat_ids", json!([])),
        ("winner_seat_ids", json!([id(1), id(1)])),
        ("winner_seat_ids", json!([id(99)])),
        ("winner_seat_ids", json!(vec![id(1); 7])),
    ] {
        let mut rows = result_rows();
        rows[0][field] = value;
        check_terminal(rows, None, true, false);
        check_terminal(result_rows(), Some(event_rows()), true, true);
    }
    check_terminal(json!([]), None, false, true);
    check_terminal(result_rows(), None, false, false);
    // Draw with no winner is accepted through the same production method.
    let mut draw = result_rows();
    draw[0]["outcome"] = json!("draw");
    draw[0]["winner_seat_ids"] = json!([]);
    let mock = Mock::new(vec![
        Step::json("/rest/v1/rpc/read_native_game_source", fixture()),
        result_step(draw),
        event_step(event_rows(), id(41)),
    ]);
    let repo = repository(&mock);
    let source = repo.read(game()).unwrap().unwrap();
    let terminal = repo
        .portable_terminal_source(&source, &index(), true, &mut budget())
        .unwrap()
        .unwrap();
    assert_eq!(terminal.value.outcome, Outcome::Draw);
    assert!(terminal.value.winner_seat_ids.is_empty());
    assert_eq!(mock.finish().len(), 3);
}

#[test]
fn terminal_original_event_joins_frontier_and_creator_are_binding() {
    for events in [json!([]), json!([event_rows()[0], event_rows()[0]])] {
        check_terminal(result_rows(), Some(events), true, false);
    }
    for (field, value) in [
        ("id", json!(id(99))),
        ("game_id", json!(id(99))),
        ("context_id", json!(id(99))),
        ("sequence", json!("69")),
        ("accepted_revision", json!("11")),
        ("source_receipt_id", json!(id(99))),
    ] {
        let mut events = event_rows();
        events[0][field] = value;
        check_terminal(result_rows(), Some(events), true, false);
        check_terminal(result_rows(), Some(event_rows()), true, true);
    }
    let mut rows = result_rows();
    rows[0]["source_event_id"] = json!(id(99));
    check_terminal(rows, Some(event_rows()), true, false);
}

#[test]
fn terminal_value_only_compares_a_consumed_final_cut() {
    let index = index();
    let terminal = TerminalSource {
        event: id(41),
        value: PlayerTerminal {
            outcome: Outcome::Win,
            winner_seat_ids: vec![id(1)],
        },
    };
    let mismatch = PlayerTerminal {
        outcome: Outcome::Draw,
        winner_seat_ids: vec![],
    };
    assert!(
        verify_terminal_value(
            &70.into(),
            id(41),
            Some(&terminal.value),
            &index,
            Some(&terminal)
        )
        .is_ok()
    );
    for (event, value, source) in [
        (id(99), Some(&terminal.value), Some(&terminal)),
        (id(41), Some(&mismatch), Some(&terminal)),
        (id(41), None, Some(&terminal)),
        (id(41), Some(&terminal.value), None),
    ] {
        assert!(verify_terminal_value(&70.into(), event, value, &index, source).is_err());
        assert!(verify_terminal_value(&63.into(), event, value, &index, source).is_ok());
    }
    assert!(
        verify_terminal_value(
            &70.into(),
            id(41),
            Some(&terminal.value),
            &index,
            Some(&terminal)
        )
        .is_ok()
    );
    assert!(verify_terminal_value(&70.into(), id(41), None, &index, None).is_err());
    let mut incident = index;
    incident.conclusion = OPERATIONAL_INCIDENT.into();
    assert!(verify_terminal_value(&70.into(), id(41), None, &incident, None).is_ok());
    assert!(verify_terminal_value(&70.into(), id(41), Some(&mismatch), &incident, None).is_err());
}

#[test]
fn terminal_rows_exhausted_work_or_bytes_refuse_before_io() {
    let mock = Mock::new(vec![Step::json(
        "/rest/v1/rpc/read_native_game_source",
        fixture(),
    )]);
    let repo = repository(&mock);
    let source = repo.read(game()).unwrap().unwrap();
    for bytes in [false, true] {
        let mut exhausted = NativeRecoveryBudget::new(1, 1).unwrap();
        exhausted
            .charge_work(usize::from(!bytes), u64::from(bytes))
            .unwrap();
        let result = repo.portable_terminal_source(&source, &index(), true, &mut exhausted);
        assert!(matches!(
            result,
            Err(NativeSourceError(Failure::Checkpoint(
                NativeCheckpointError::Budget
            )))
        ));
    }
    assert_eq!(mock.finish().len(), 1);
}
