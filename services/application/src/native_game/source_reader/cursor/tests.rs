//! Strict private source decoding and correlation; loopback replies are not SQL/Auth proof.
use super::*;
use crate::test_http::{Mock, Step, positional};
use serde_json::Value;
fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
fn fixture() -> Value {
    json!({"status":"success","data":{"version":"native-cursor-v1",
        "basis":{"gameId":id(4),"contextId":id(5),"epoch":"42","revision":"0"},
        "viewerSeatId":id(6),"throughEventSequence":"0","purpose":"snapshot",
        "sourceReceiptId":id(8),"disclosure":"live","token":"n1.12345678-1234-4234-9234-123456789abc"}})
}
fn repository(mock: &Mock) -> NativeSourceRepository {
    NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "cursor-test",
            Duration::from_secs(2),
        )
        .unwrap(),
    )
}
fn access() -> NativeHumanAccess {
    NativeHumanAccess::fixture_for_outcome_test(0)
}
fn cursor() -> PlayerCursor {
    serde_json::from_value::<Record>(fixture()["data"].clone())
        .unwrap()
        .cursor()
}
const PATH: &str = "/rest/v1/rpc/read_native_cursor_source";
#[test]
fn cursor_uses_derived_viewer_and_requires_exact_independent_issued_coordinates() {
    let mut step = Step::json(PATH, fixture());
    step.expect = Some(
        serde_json::to_vec(&json!({"p_game":id(4),"p_viewer":id(6),"p_token":cursor().token}))
            .unwrap(),
    );
    let mut history = fixture();
    history["data"]["purpose"] = json!("history");
    let mock = Mock::new(vec![step, Step::json(PATH, history)]);
    let repo = repository(&mock);
    for _ in 0..2 {
        let issued = repo
            .read_human_cursor(&access(), &cursor())
            .unwrap()
            .unwrap();
        assert!(issued.cursor() == cursor());
        issued.validate_current(&access()).unwrap();
    }
    assert_eq!(mock.finish().len(), 2);
}
#[test]
fn cursor_rejects_unknown_foreign_progress_and_non_live_issuance_without_fallback() {
    let mut sources = vec![json!({"status":"fail","data":{"cursor":"unavailable"}})];
    for (path, value) in [
        ("/data/basis/gameId", json!(id(99))),
        ("/data/basis/contextId", json!(id(99))),
        ("/data/basis/epoch", json!("1")),
        ("/data/basis/revision", json!("1")),
        ("/data/viewerSeatId", json!(id(99))),
        ("/data/throughEventSequence", json!("1")),
        ("/data/disclosure", json!("completed")),
        ("/data/disclosure", json!("operational-prefix")),
        (
            "/data/token",
            json!("n1.12345678-1234-4234-9234-123456789abd"),
        ),
    ] {
        let mut v = fixture();
        *v.pointer_mut(path).unwrap() = value;
        sources.push(v);
    }
    let count = sources.len();
    let mock = Mock::new(sources.into_iter().map(|v| Step::json(PATH, v)).collect());
    let repo = repository(&mock);
    for _ in 0..count {
        assert!(
            repo.read_human_cursor(&access(), &cursor())
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(mock.finish().len(), count);
    let mock = Mock::new(vec![]);
    let repo = repository(&mock);
    for (field, value) in [
        ("viewerSeatId", json!(id(99))),
        ("token", json!("not-issued")),
    ] {
        let mut public = serde_json::to_value(cursor()).unwrap();
        public[field] = value;
        let public = serde_json::from_value(public).unwrap();
        assert!(
            repo.read_human_cursor(&access(), &public)
                .unwrap()
                .is_none()
        );
    }
    assert!(mock.finish().is_empty());
}
#[test]
fn cursor_strictly_decodes_the_complete_private_contract() {
    let good = fixture();
    let mut cases = vec![];
    for key in good["data"].as_object().unwrap().keys() {
        let mut v = good.clone();
        v["data"].as_object_mut().unwrap().remove(key);
        cases.push(v);
    }
    for (field, value) in [
        ("version", json!("other")),
        ("sourceReceiptId", json!(Uuid::nil())),
        ("purpose", json!("other")),
        ("purpose", json!({"snapshot":null})),
        ("disclosure", json!({"live":null})),
        ("disclosure", json!("other")),
        ("throughEventSequence", json!("01")),
        ("scan", Value::Null),
    ] {
        let mut v = good.clone();
        v["data"][field] = value;
        cases.push(v);
    }
    for token in [
        "n1.12345678-1234-1234-9234-123456789abc",
        "n1.12345678-1234-4234-7234-123456789abc",
        "n1.12345678-1234-4234-9234-123456789ABC",
        "n1.{12345678-1234-4234-9234-123456789abc}",
        "n1.00000000-0000-0000-0000-000000000000",
    ] {
        let mut v = good.clone();
        v["data"]["token"] = json!(token);
        cases.push(v);
    }
    let fields: Vec<_> = good["data"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut v = good.clone();
    v["data"] = positional(&good["data"], &fields);
    cases.push(v);
    let count = cases.len();
    let mut steps: Vec<_> = cases.into_iter().map(|v| Step::json(PATH, v)).collect();
    let mut duplicate = Step::json(PATH, good.clone());
    duplicate.body = serde_json::to_string(&good)
        .unwrap()
        .replacen(
            "\"purpose\":\"snapshot\"",
            "\"purpose\":\"snapshot\",\"purpose\":\"history\"",
            1,
        )
        .into_bytes();
    steps.push(duplicate);
    steps.push(Step::json(
        PATH,
        json!({"status":"fail","data":{"game":"unavailable"}}),
    ));
    let mock = Mock::new(steps);
    let repo = repository(&mock);
    for _ in 0..count + 2 {
        assert!(repo.read_human_cursor(&access(), &cursor()).is_err());
    }
    assert_eq!(mock.finish().len(), count + 2);
}

#[test]
fn independently_matching_cursor_still_requires_current_epoch_and_frontier() {
    let mut sources = vec![];
    for (pointer, value) in [
        ("/data/basis/epoch", json!("43")),
        ("/data/basis/revision", json!("1")),
        ("/data/throughEventSequence", json!("1")),
    ] {
        let mut v = fixture();
        *v.pointer_mut(pointer).unwrap() = value;
        sources.push(v);
    }
    let requests: Vec<_> = sources
        .iter()
        .map(|v| {
            serde_json::from_value::<Record>(v["data"].clone())
                .unwrap()
                .cursor()
        })
        .collect();
    let mock = Mock::new(sources.into_iter().map(|v| Step::json(PATH, v)).collect());
    let repo = repository(&mock);
    for request in requests {
        assert!(
            repo.read_human_cursor(&access(), &request)
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(mock.finish().len(), 3);
}

// These cuts carry cursor coordinates and distinct View receipts only. They do
// not represent native projections, accepted SQL history, or qualified rules.
struct CursorCut {
    snapshot: crate::player_wire::PlayerSnapshot,
    receipt: Uuid,
}
impl super::super::cuts::HistoryCut for CursorCut {
    fn snapshot(&self) -> &crate::player_wire::PlayerSnapshot {
        &self.snapshot
    }
    fn cut_facts<'a>(
        &'a self,
        _vocabulary: &'a spellwake_contracts::creature_types::CreatureTypeVocabulary,
    ) -> Result<crate::player_wire::PlayerProjectionFacts<'a>, NativeSourceError> {
        panic!("cursor verification must not request projection facts")
    }
    fn cut_terminal(&self) -> Option<&crate::player_wire::PlayerTerminal> {
        panic!("cursor verification must not request terminal facts")
    }
    fn view_receipt(&self) -> Uuid {
        self.receipt
    }
    fn lineage(&self) -> super::super::cuts::CutLineage<'_> {
        panic!("cursor verification must not request accepted lineage")
    }
}
fn cursor_cuts() -> Vec<CursorCut> {
    [0_u64, 10, 20, 30, 40]
        .into_iter()
        .enumerate()
        .map(|(index, sequence)| {
            let mut basis = cursor().basis;
            // Raw sequence intentionally differs from accepted revision.
            basis.revision = NativeRevision::from(u64::try_from(index).unwrap());
            CursorCut {
                snapshot: crate::player_wire::PlayerSnapshot {
                    basis,
                    viewer_seat_id: id(6),
                    players: vec![],
                    turn: None,
                    visible_objects: vec![],
                    zones: vec![],
                    stack: vec![],
                    decisions: vec![],
                    through_event_sequence: Amount::from(sequence),
                },
                receipt: id(100 + u128::try_from(index).unwrap()),
            }
        })
        .collect()
}
fn history_record() -> Value {
    let mut record = fixture();
    record["data"]["basis"]["revision"] = json!("1");
    record["data"]["throughEventSequence"] = json!("10");
    record["data"]["purpose"] = json!("history");
    record["data"]["sourceReceiptId"] = json!(id(103));
    record
}
fn history_cursor() -> PlayerCursor {
    serde_json::from_value::<Record>(history_record()["data"].clone())
        .unwrap()
        .cursor()
}
fn cursor_vocabulary() -> spellwake_contracts::creature_types::CreatureTypeVocabulary {
    // Required by CutSource's shape but never read by this cursor verifier.
    spellwake_contracts::creature_types::CreatureTypeVocabulary::new(
        "synthetic-cursor-only".into(),
        [0; 32],
        vec!["Human".into()],
    )
    .unwrap()
}
fn cursor_source<'a>(
    cuts: &'a [CursorCut],
    vocabulary: &'a spellwake_contracts::creature_types::CreatureTypeVocabulary,
    disclosure: NativeDisclosure,
) -> super::super::cuts::CutSource<'a, CursorCut> {
    super::super::cuts::CutSource {
        cuts,
        disclosure,
        opening_receipt_id: id(90),
        vocabulary,
    }
}
fn history_step(record: Value) -> Step {
    let mut step = Step::json(PATH, record);
    step.expect = Some(
        serde_json::to_vec(&json!({
            "p_game":id(4),"p_viewer":id(6),"p_token":history_cursor().token
        }))
        .unwrap(),
    );
    step
}
#[test]
fn cut_history_receipt_interval_includes_end_interior_and_coordinate_only() {
    // E = cut1, A = cut3. All five receipts are independently distinct.
    let cuts = cursor_cuts();
    let vocabulary = cursor_vocabulary();
    let source = cursor_source(&cuts, &vocabulary, NativeDisclosure::LiveParticipant);
    let cases = [
        (101, true),  // E
        (102, true),  // strictly between E and A
        (103, true),  // A
        (100, false), // E-1
        (103, true),  // restore unchanged source after rejection
        (104, false), // belongs to collection, but after A
        (103, true),
        (999, false), // foreign nonnil receipt
        (103, true),
    ];
    let steps = cases
        .iter()
        .map(|(receipt, _)| {
            let mut record = history_record();
            record["data"]["sourceReceiptId"] = json!(id(*receipt));
            history_step(record)
        })
        .collect();
    let mock = Mock::new(steps);
    let repo = repository(&mock);
    for (receipt, expected) in cases {
        let result = repo
            .read_cut_history_cursor(&source, 3, &history_cursor(), None)
            .unwrap();
        assert_eq!(result.is_some(), expected, "issuer receipt {receipt}");
        if let Some(issued) = result {
            assert!(issued.cursor() == history_cursor());
        }
    }
    assert_eq!(mock.finish().len(), cases.len());
}
#[test]
fn cut_history_rejects_issuer_purpose_disclosure_and_coordinate_substitution() {
    let cuts = cursor_cuts();
    let vocabulary = cursor_vocabulary();
    for disclosure in [
        NativeDisclosure::LiveParticipant,
        NativeDisclosure::CompletedParticipant,
    ] {
        let source = cursor_source(&cuts, &vocabulary, disclosure);
        let mut good = history_record();
        good["data"]["disclosure"] = json!(match disclosure {
            NativeDisclosure::LiveParticipant => "live",
            NativeDisclosure::CompletedParticipant => "completed",
        });
        for (pointer, value) in [
            ("/data/purpose", json!("snapshot")),
            ("/data/disclosure", json!("operational-prefix")),
            (
                "/data/disclosure",
                json!(if disclosure == NativeDisclosure::LiveParticipant {
                    "completed"
                } else {
                    "live"
                }),
            ),
            ("/data/basis/gameId", json!(id(99))),
            ("/data/basis/contextId", json!(id(99))),
            ("/data/basis/epoch", json!("43")),
            ("/data/basis/revision", json!("2")),
            ("/data/viewerSeatId", json!(id(99))),
            ("/data/throughEventSequence", json!("20")),
            (
                "/data/token",
                json!("n1.12345678-1234-4234-9234-123456789abd"),
            ),
        ] {
            let mut altered = good.clone();
            *altered.pointer_mut(pointer).unwrap() = value;
            let mock = Mock::new(vec![
                history_step(good.clone()),
                history_step(altered),
                history_step(good.clone()),
            ]);
            let repo = repository(&mock);
            for expected in [true, false, true] {
                let result = repo
                    .read_cut_history_cursor(&source, 3, &history_cursor(), None)
                    .unwrap();
                assert_eq!(result.is_some(), expected, "issuer field {pointer}");
                if let Some(issued) = result {
                    assert!(issued.cursor() == history_cursor());
                }
            }
            assert_eq!(mock.finish().len(), 3);
        }
    }
}
#[test]
fn cut_history_invalid_public_coordinates_do_not_read_issuer_or_charge_budget() {
    let cuts = cursor_cuts();
    let vocabulary = cursor_vocabulary();
    let source = cursor_source(&cuts, &vocabulary, NativeDisclosure::LiveParticipant);
    let mock = Mock::new(vec![]);
    let repo = repository(&mock);
    for (pointer, value) in [
        ("/token", json!("not-issued")),
        ("/viewerSeatId", json!(id(99))),
        ("/basis/gameId", json!(id(99))),
        ("/basis/contextId", json!(id(99))),
        ("/basis/epoch", json!("43")),
        ("/basis/revision", json!("4")),
        ("/throughEventSequence", json!("40")),
        ("/throughEventSequence", json!("11")), // not an existing cut
    ] {
        let mut value_of_request = serde_json::to_value(history_cursor()).unwrap();
        *value_of_request.pointer_mut(pointer).unwrap() = value;
        let requested = serde_json::from_value(value_of_request).unwrap();
        let mut budget = NativeRecoveryBudget::new(1, 1_024).unwrap();
        assert!(
            repo.read_cut_history_cursor(&source, 3, &requested, Some(&mut budget))
                .unwrap()
                .is_none(),
            "public field {pointer}"
        );
        assert_eq!(budget.remaining(), (1, 1_024));
    }
    // An otherwise valid end still cannot lie beyond the requested coordinate.
    assert!(
        repo.read_cut_history_cursor(&source, 0, &history_cursor(), None)
            .unwrap()
            .is_none()
    );
    assert!(mock.finish().is_empty());
}
#[test]
fn cut_history_exhausted_record_or_byte_budget_is_typed_and_pre_io() {
    let cuts = cursor_cuts();
    let vocabulary = cursor_vocabulary();
    let source = cursor_source(&cuts, &vocabulary, NativeDisclosure::LiveParticipant);
    let mock = Mock::new(vec![]);
    let repo = repository(&mock);
    for (records, bytes) in [(1, 0), (0, 1_024)] {
        let mut budget = NativeRecoveryBudget::new(1, 1_024).unwrap();
        budget.charge_work(records, bytes).unwrap();
        let result = repo.read_cut_history_cursor(&source, 3, &history_cursor(), Some(&mut budget));
        assert!(matches!(
            result,
            Err(NativeSourceError(Failure::Checkpoint(
                NativeCheckpointError::Budget
            )))
        ));
        assert_eq!(budget.remaining(), (1 - records, 1_024 - bytes));
    }
    assert!(mock.finish().is_empty());
}
