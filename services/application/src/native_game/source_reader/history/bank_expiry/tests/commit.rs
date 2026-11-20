//! Actual native bank proposal and strict private transport mocks, not SQL due authorization.
use super::*;
use crate::admission_ports::HostCandidate;
use crate::test_http::Step;
const DISPOSITION: &str = "/rest/v1/rpc/read_native_system_disposition";
fn repo(mock: &Mock) -> NativeSourceRepository {
    NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "bank-commit-test",
            Duration::from_secs(2),
        )
        .unwrap(),
    )
}
fn fixture() -> (RecoveredNativeGame, NativeCheckpoint, NativeSystemOriginal) {
    let f = checkpoint_fixture(None);
    let recovered = super::state_only::recover(&f).unwrap().unwrap();
    let (next, _, _) = cut_from(&f);
    let original =
        NativeSystemOriginal::prepare_bank_expiry(Uuid::from_u128(0xb001), &recovered, &next)
            .unwrap();
    (recovered, next, original)
}
fn unknown() -> Step {
    Step::json(
        DISPOSITION,
        json!({"status":"success","data":{"state":"unknown","response":null}}),
    )
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; bank frame and mismatch mechanics only"]
fn bank_commit_frame_copies_source_and_rejects_mutations_before_artifact_io() {
    let (recovered, next, original) = fixture();
    let intent = recovered.bank_expiry_intent(&next).unwrap();
    let frame = recovered.prepare_bank_candidate(&next).unwrap();
    assert!(frame.matches_native_bank_clocks(&intent));
    assert!(recovered.clock_intent(&next).is_err());
    assert!(recovered.prepare_candidate(&next).is_err());
    assert!(
        NativeSystemOriginal::prepare_entropy(original.request_id(), &recovered, &next).is_err()
    );
    assert!(original.matches_bank_checkpoint(&next));
    let good: Value = serde_json::from_str(frame.original_json()).unwrap();
    assert_eq!(
        good["bankTimeout"]["proofRef"],
        json!(next.retained_input().unwrap().2)
    );
    assert_eq!(good["bankTimeout"]["clockRevision"], "4");
    assert!(
        good["clocks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["paused"] == true
                && v.get("eligibility").is_none()
                && v["remainingMs"] == "1700000")
    );
    for (path, value) in [
        (
            "/bankTimeout/seatId",
            json!(next.terminal().unwrap().unwrap().winner_seat_ids()[0]),
        ),
        ("/bankTimeout/clockRevision", json!("5")),
        ("/bankTimeout/proofRef", json!("fixture:unqualified-proof")),
        ("/clocks/0/remainingMs", json!("0")),
        ("/clocks/0/clockRevision", json!("5")),
        ("/clocks/0/paused", json!(false)),
        ("/terminal/reason", json!("native_concession")),
    ] {
        let mut bad = good.clone();
        *bad.pointer_mut(path).unwrap() = value;
        let candidate = HostCandidate::from_bytes(&serde_json::to_vec(&bad).unwrap()).unwrap();
        let mock = Mock::new(vec![unknown()]);
        assert!(
            repo(&mock)
                .commit_bank_expiry(
                    &checkpoint_store(&mock),
                    &recovered,
                    original.request_id(),
                    &next,
                    &candidate,
                    &original
                )
                .is_err(),
            "accepted {path}"
        );
        assert_eq!(mock.finish().len(), 1);
    }
    let mut bad = good.clone();
    bad["nativeEliminations"] = json!([]);
    let candidate = HostCandidate::from_bytes(&serde_json::to_vec(&bad).unwrap()).unwrap();
    assert!(!candidate.matches_native_bank_clocks(&intent));
    let mut identity: Value = serde_json::from_str(original.original_json()).unwrap();
    identity["kind"] = json!("entropy");
    let other = NativeSystemOriginal::from_bytes(
        original.request_id(),
        &serde_json::to_vec(&identity).unwrap(),
    )
    .unwrap();
    let mock = Mock::new(vec![]);
    assert!(
        repo(&mock)
            .commit_bank_expiry(
                &checkpoint_store(&mock),
                &recovered,
                original.request_id(),
                &next,
                &frame,
                &other
            )
            .is_err()
    );
    assert!(mock.finish().is_empty());
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; clock source eligibility mechanics only"]
fn bank_commit_refuses_paused_positive_and_unsupported_or_unavailable_sources() {
    let f = checkpoint_fixture(None);
    let (next, _, _) = cut_from(&f);
    for case in 0..5 {
        let recovered = super::state_only::recover_metadata(&f, |body| match case {
            0 => body["data"]["matchClocks"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .for_each(|v| v["paused"] = json!(true)),
            1 => body["data"]["matchClocks"][0]["policyRef"] = json!("future-policy"),
            2 => body["data"]["matchAvailability"] = json!("recovering"),
            3 => body["data"]["operational"]["availability"] = json!("recovering"),
            _ => body["data"]["matchState"] = json!("between_games"),
        })
        .unwrap()
        .unwrap();
        assert!(
            recovered.bank_expiry_intent(&next).is_err(),
            "accepted source {case}"
        );
    }
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; original-first bank transport only"]
fn bank_commit_exact_ack_recovers_before_fresh_frame_checks_and_conflicts_stay_closed() {
    let (recovered, next, original) = fixture();
    let frame = recovered.prepare_bank_candidate(&next).unwrap();
    let mut bad: Value = serde_json::from_str(frame.original_json()).unwrap();
    bad["bankTimeout"]["clockRevision"] = json!("999");
    let bad = HostCandidate::from_bytes(&serde_json::to_vec(&bad).unwrap()).unwrap();
    let response = json!({"status":"success","data":{"resourceId":Uuid::from_u128(0xb003),"revision":next.basis().revision}});
    let mock = Mock::new(vec![Step::json(
        DISPOSITION,
        json!({"status":"success","data":{"state":"accepted","response":response}}),
    )]);
    let ack = repo(&mock)
        .commit_bank_expiry(
            &checkpoint_store(&mock),
            &recovered,
            original.request_id(),
            &next,
            &bad,
            &original,
        )
        .unwrap();
    assert_eq!(ack.revision, Amount::from(next.basis().revision.get()));
    assert_eq!(mock.finish().len(), 1);
    let mock = Mock::new(vec![Step::json(
        DISPOSITION,
        json!({"status":"fail","data":{"request":"conflict"}}),
    )]);
    assert!(
        repo(&mock)
            .commit_bank_expiry(
                &checkpoint_store(&mock),
                &recovered,
                original.request_id(),
                &next,
                &frame,
                &original
            )
            .is_err()
    );
    assert_eq!(mock.finish().len(), 1);
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; exact prepared bank RPC transport only"]
fn bank_commit_unknown_checks_retained_event_results_then_sends_exact_clock_original() {
    let (recovered, next, original) = fixture();
    let frame = recovered.prepare_bank_candidate(&next).unwrap();
    let event = next.prepare_event().unwrap();
    let results = next.prepare_results().unwrap();
    let mut steps = vec![unknown()];
    steps.extend(checkpoint_read_steps(
        event.reference(),
        event.original_bytes(),
    ));
    steps.extend(checkpoint_read_steps(
        results.reference(),
        results.original_bytes(),
    ));
    let mut commit = Step::json(
        "/rest/v1/rpc/commit_native_host_input",
        json!({"status":"success","data":{"resourceId":Uuid::from_u128(0xb004),"revision":next.basis().revision}}),
    );
    commit.expect=Some(serde_json::to_vec(&json!({"p_game":next.basis().game_id,"p_request":original.request_id(),"p_original":serde_json::from_str::<Value>(original.original_json()).unwrap(),"p_prepared":{"owner":"fixture:owner","fenceEpoch":"1","candidate":serde_json::from_str::<Value>(frame.original_json()).unwrap()}})).unwrap());
    steps.push(commit);
    let mock = Mock::new(steps);
    let ack = repo(&mock)
        .commit_bank_expiry(
            &checkpoint_store(&mock),
            &recovered,
            original.request_id(),
            &next,
            &frame,
            &original,
        )
        .unwrap();
    assert_eq!(ack.revision, Amount::from(next.basis().revision.get()));
    assert_eq!(mock.finish().len(), 6);
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; inherited-zero bank mechanics only"]
fn bank_commit_zero_is_allowed_only_for_the_actual_loser_and_sql_still_owns_its_source() {
    let f = checkpoint_fixture(None);
    let (next, _, _) = cut_from(&f);
    let loser = next.terminal().unwrap().unwrap().losing_seat_ids()[0];
    let recovered = super::state_only::recover_metadata(&f, |body| {
        for row in body["data"]["matchClocks"].as_array_mut().unwrap() {
            if row["seatId"] == json!(loser) {
                row["remainingMs"] = json!("0");
                row["paused"] = json!(true);
            }
        }
    })
    .unwrap()
    .unwrap();
    assert!(recovered.bank_expiry_intent(&next).is_ok());
    assert!(recovered.prepare_bank_candidate(&next).is_ok());
    let both = super::state_only::recover_metadata(&f, |body| {
        for row in body["data"]["matchClocks"].as_array_mut().unwrap() {
            row["remainingMs"] = json!("0");
            row["paused"] = json!(true);
        }
    })
    .unwrap()
    .unwrap();
    assert!(both.bank_expiry_intent(&next).is_err());
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; bank retention refusal before mutation RPC"]
fn bank_commit_refuses_missing_or_corrupt_result_artifacts_before_mutation_rpc() {
    let (recovered, next, original) = fixture();
    let frame = recovered.prepare_bank_candidate(&next).unwrap();
    let event = next.prepare_event().unwrap();
    let results = next.prepare_results().unwrap();
    for corrupt_results in [false, true] {
        let mut steps = vec![unknown()];
        if corrupt_results {
            steps.extend(checkpoint_read_steps(
                event.reference(),
                event.original_bytes(),
            ));
        }
        let (reference, bytes) = if corrupt_results {
            (results.reference(), results.original_bytes())
        } else {
            (event.reference(), event.original_bytes())
        };
        let mut corrupt = bytes.to_vec();
        corrupt[0] ^= 1;
        steps.extend(checkpoint_read_steps(reference, &corrupt));
        let expected = steps.len();
        let mock = Mock::new(steps);
        assert!(
            repo(&mock)
                .commit_bank_expiry(
                    &checkpoint_store(&mock),
                    &recovered,
                    original.request_id(),
                    &next,
                    &frame,
                    &original
                )
                .is_err()
        );
        assert_eq!(mock.finish().len(), expected);
    }
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; lost bank acknowledgment private transport only"]
fn bank_commit_lost_ack_retries_the_exact_original_without_a_second_mutation() {
    let (recovered, next, original) = fixture();
    let frame = recovered.prepare_bank_candidate(&next).unwrap();
    let event = next.prepare_event().unwrap();
    let results = next.prepare_results().unwrap();
    let mut steps = vec![unknown()];
    steps.extend(checkpoint_read_steps(
        event.reference(),
        event.original_bytes(),
    ));
    steps.extend(checkpoint_read_steps(
        results.reference(),
        results.original_bytes(),
    ));
    let mut lost = Step::json(
        "/rest/v1/rpc/commit_native_host_input",
        json!({"status":"success","data":{"resourceId":Uuid::from_u128(0xb005),"revision":next.basis().revision}}),
    );
    lost.drop_ack = true;
    steps.push(lost);
    steps.push(Step::json(DISPOSITION,json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":Uuid::from_u128(0xb005),"revision":next.basis().revision}}}})));
    let mock = Mock::new(steps);
    let repo = repo(&mock);
    let store = checkpoint_store(&mock);
    assert!(
        repo.commit_bank_expiry(
            &store,
            &recovered,
            original.request_id(),
            &next,
            &frame,
            &original
        )
        .is_err()
    );
    let ack = repo
        .commit_bank_expiry(
            &store,
            &recovered,
            original.request_id(),
            &next,
            &frame,
            &original,
        )
        .unwrap();
    assert_eq!(ack.resource_id, Uuid::from_u128(0xb005));
    assert_eq!(mock.finish().len(), 7);
}
