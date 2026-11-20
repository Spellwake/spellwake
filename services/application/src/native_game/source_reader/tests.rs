//! Adversarial private-wire mechanics; genuine managed/native reconstruction is separate.
use super::*;
use crate::test_http::{Mock, Step};
use serde_json::Value;
const PATH: &str = "/rest/v1/rpc/read_native_game_source";
pub(super) fn game() -> Uuid {
    Uuid::from_u128(0x124)
}
pub(super) fn fixture() -> Value {
    let g = game();
    let c = Uuid::from_u128(0x125);
    let initial =
        NativeArtifactRef::for_bytes(g, c, NativeArtifactRole::Initial, b"fixture-root").unwrap();
    let state =
        NativeArtifactRef::for_bytes(g, c, NativeArtifactRole::State, b"fixture-manifest").unwrap();
    json!({"status":"success","data":{"version":"native-game-source-v1","gameId":g,"matchId":Uuid::from_u128(0x126),"contextId":c,"releaseId":Uuid::from_u128(0x127),"profileId":Uuid::from_u128(0x128),"gameState":"active","matchState":"active","matchAvailability":"available","acceptedRevision":"0","contextRevision":"fixture:v1","initialRef":initial.as_str(),"checkpoint":{"id":Uuid::from_u128(0x129),"acceptedRevision":"0","stateRef":state.as_str(),"clockRevision":"1","creatorReceiptId":Uuid::from_u128(0x130)},"operational":{"ownerRef":"fixture:owner","fenceEpoch":"999999999999999999999999999999999999999","revision":"3","availability":"available","recoveryCheckpointId":null},"seats":[{"seatId":Uuid::from_u128(1),"seatIndex":"2","accountId":Uuid::from_u128(0x131),"controllerKind":"human","sourceRevisionId":Uuid::from_u128(0x132),"deckSnapshotRef":"fixture:accepted-deck"},{"seatId":Uuid::from_u128(2),"seatIndex":"5","accountId":null,"controllerKind":"ai","sourceRevisionId":Uuid::from_u128(0x133),"deckSnapshotRef":"fixture:accepted-deck-ai"}],"matchClocks":[{"seatId":Uuid::from_u128(1),"policyRef":"bo3-overall-bank-v1","remainingMs":"120000","clockRevision":"4","paused":true,"lastAccountedAt":"2026-11-06T00:00:00.000000Z"},{"seatId":Uuid::from_u128(2),"policyRef":"bo3-overall-bank-v1","remainingMs":"110000","clockRevision":"4","paused":true,"lastAccountedAt":"2026-11-06T00:00:00.000000Z"}],"lastEventSequence":"0"}})
}
pub(super) fn repository(mock: &Mock) -> NativeSourceRepository {
    NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "source-reader-test",
            Duration::from_secs(1),
        )
        .unwrap(),
    )
}
#[test]
fn sealed_source_preserves_copied_roster_independent_fence_and_historical_clock_basis() {
    let mut earlier = fixture();
    earlier["data"]["gameState"] = json!("completed");
    earlier["data"]["matchState"] = json!("ready");
    let mut step = Step::json(PATH, earlier);
    step.expect = Some(serde_json::to_vec(&json!({"p_game":game()})).unwrap());
    let mock = Mock::new(vec![step]);
    let repo = repository(&mock);
    assert!(repo.read(Uuid::nil()).is_err()); // no extra request consumed
    let source = repo.read(game()).unwrap().unwrap();
    assert_eq!(
        source.human_seats().collect::<Vec<_>>(),
        vec![Uuid::from_u128(1)]
    );
    // The AI's seat and the human account UUID are not human seat recipients.
    assert!(
        !source
            .human_seats()
            .any(|seat| seat == Uuid::from_u128(2) || seat == Uuid::from_u128(0x131))
    );
    assert_eq!(source.game_id(), game());
    assert_eq!(source.context_revision(), "fixture:v1");
    assert_eq!(source.accepted_revision(), &Amount::from(0));
    assert_eq!(
        source.operational_basis().1.to_string(),
        "999999999999999999999999999999999999999"
    );
    assert_eq!(
        source
            .deck_sources()
            .map(|(id, _, _)| id)
            .collect::<Vec<_>>(),
        vec![Uuid::from_u128(1), Uuid::from_u128(2)]
    );
    assert!(source.initial_expected().is_ok());
    assert!(
        source
            .clocks()
            .all(|(_, _, revision, _, _)| revision == &Amount::from(4))
    );
}
#[test]
fn corruption_matrix_rejects_scope_roles_roster_clocks_and_noncanonical_scalars() {
    let good = fixture();
    let mut cases = Vec::new();
    for (pointer, value) in [
        ("/data/version", json!("native-game-source-v2")),
        ("/data/seats/1/seatIndex", json!("9223372036854775808")),
        ("/data/gameId", json!(Uuid::new_v4())),
        ("/data/contextId", json!(Uuid::new_v4())),
        ("/data/releaseId", json!(Uuid::nil())),
        ("/data/gameState", json!("reserved")),
        ("/data/matchState", json!("admitting")),
        ("/data/matchAvailability", json!("unknown")),
        ("/data/acceptedRevision", json!(0)),
        ("/data/checkpoint/acceptedRevision", json!("1")),
        ("/data/checkpoint/clockRevision", json!("01")),
        ("/data/operational/fenceEpoch", json!("0")),
        ("/data/operational/ownerRef", json!("x".repeat(257))),
        ("/data/operational/recoveryCheckpointId", json!(Uuid::nil())),
        ("/data/seats/0/controllerKind", json!("ai")),
        ("/data/seats/1/accountId", json!(Uuid::new_v4())),
        (
            "/data/seats/1/seatId",
            good["data"]["seats"][0]["seatId"].clone(),
        ),
        ("/data/seats/1/seatIndex", json!("2")),
        ("/data/seats/0/sourceRevisionId", json!(Uuid::nil())),
        (
            "/data/matchClocks/1/seatId",
            good["data"]["matchClocks"][0]["seatId"].clone(),
        ),
        ("/data/matchClocks/1/clockRevision", json!("0")),
        (
            "/data/matchClocks/1/lastAccountedAt",
            json!("2026-11-06T00:00:00Z"),
        ),
        ("/data/matchClocks/1/remainingMs", json!("-1")),
        ("/data/initialRef", json!("fixture:opaque-not-native")),
        (
            "/data/initialRef",
            good["data"]["checkpoint"]["stateRef"].clone(),
        ),
        (
            "/data/checkpoint/stateRef",
            good["data"]["initialRef"].clone(),
        ),
    ] {
        let mut changed = good.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        cases.push(changed);
    }
    for field in ["checkpoint", "operational"] {
        let mut changed = good.clone();
        changed["data"][field] = json!([]);
        cases.push(changed);
    }
    for field in ["seats", "matchClocks"] {
        let mut changed = good.clone();
        changed["data"][field] = json!([]);
        cases.push(changed);
        let mut changed = good.clone();
        let object = changed["data"][field][0].as_object().unwrap();
        changed["data"][field][0] = Value::Array(object.values().cloned().collect());
        cases.push(changed);
    }
    for pointer in ["/data/operational", "/data/seats/1"] {
        let mut changed = good.clone();
        changed
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(if pointer.ends_with('1') {
                "accountId"
            } else {
                "recoveryCheckpointId"
            });
        cases.push(changed);
    }
    let case_count = cases.len();
    let mock = Mock::new(cases.into_iter().map(|v| Step::json(PATH, v)).collect());
    let repo = repository(&mock);
    for _ in 0..case_count {
        assert!(repo.read(game()).is_err());
    }
    assert_eq!(mock.finish().len(), case_count);
}
#[test]
fn original_duplicate_fields_unknown_failures_and_provider_errors_do_not_become_unavailable() {
    let raw = serde_json::to_string(&fixture()).unwrap();
    let mut steps = Vec::new();
    for (key, value) in [
        ("version", json!("native-game-source-v1")),
        ("ownerRef", json!("fixture:owner")),
        ("creatorReceiptId", json!(Uuid::from_u128(0x130))),
        ("deckSnapshotRef", json!("fixture:accepted-deck")),
        ("remainingMs", json!("120000")),
    ] {
        let mut step = Step::json(PATH, Value::Null);
        let needle = format!("\"{key}\":");
        step.body = raw
            .replacen(&needle, &format!("{needle}{value},{needle}"), 1)
            .into_bytes();
        steps.push(step);
    }
    steps.push(Step::json(
        PATH,
        json!({"status":"fail","data":{"game":"unavailable","extra":"invalid"}}),
    ));
    steps.push(Step::json(
        PATH,
        json!({"status":"fail","data":{"game":"unknown"}}),
    ));
    steps.push(Step::json(
        PATH,
        json!({"status":"error","message":"private provider detail","code":"internal"}),
    ));
    steps.push(Step::json(
        PATH,
        json!({"status":"fail","data":{"game":"unavailable"}}),
    ));
    let mock = Mock::new(steps);
    let repo = repository(&mock);
    for _ in 0..8 {
        assert!(repo.read(game()).is_err());
    }
    assert!(repo.read(game()).unwrap().is_none());
}
#[test]
#[ignore = "requires genuine retained catalogue/vocabulary; loopback Store mechanics, not managed RPC"]
fn genuine_opening_epoch_and_exact_root_drive_checkpoint_expectation_and_reconstruction() {
    let read =
        |name| std::fs::read(std::env::var_os(name).expect("genuine input required")).unwrap();
    let program = NativeProgramSource::from_bytes(
        NativeProgramPool::ResolutionSelectionNaturalMana,
        &read("SPELLWAKE_CATALOGUE_METADATA"),
        read("SPELLWAKE_CATALOGUE_INPUT"),
        &read("SPELLWAKE_CREATURE_VOCABULARY_INPUT"),
    )
    .unwrap();
    let g = game();
    let c = Uuid::from_u128(0x125);
    let release = Uuid::from_u128(0x127);
    let profile = Uuid::from_u128(0x128);
    let ids = NativeInitialIds {
        game_id: g,
        context_id: c,
        release_id: release,
        profile_id: profile,
    };
    let spec = || NativeInitialSpec {
        first_player_seat_id: Uuid::from_u128(1),
        participants: (1..=2)
            .map(|seat| NativeInitialParticipant {
                seat_id: Uuid::from_u128(seat),
                life: 20.into(),
                cards: (0..20)
                    .map(|_| NativeInitialCard {
                        oracle_id: spellwake_card_programs::OracleId::try_from(
                            "b2c6aa39-2d2a-459c-a555-fb48ba993373".to_owned(),
                        )
                        .unwrap(),
                        zone: NativeInitialZone::Library,
                    })
                    .collect(),
            })
            .collect(),
        entropy: vec![0.into(); 2048],
    };
    let original = prepare_initial(ids, &program, spec()).unwrap();
    let checkpoint =
        NativeCheckpoint::initial(prepare_initial(ids, &program, spec()).unwrap()).unwrap();
    let mut data = fixture();
    data["data"]["initialRef"] = json!(original.reference().as_str());
    data["data"]["checkpoint"]["stateRef"] = json!(checkpoint.reference().as_str());
    let mut changed_release = data.clone();
    changed_release["data"]["releaseId"] = json!(Uuid::new_v4());
    let mut reversed = data.clone();
    reversed["data"]["seats"][0]["seatId"] = json!(Uuid::from_u128(2));
    reversed["data"]["seats"][1]["seatId"] = json!(Uuid::from_u128(1));
    let mut overflow = data.clone();
    for pointer in [
        "/data/acceptedRevision",
        "/data/checkpoint/acceptedRevision",
        "/data/lastEventSequence",
    ] {
        *overflow.pointer_mut(pointer).unwrap() = json!("18446744073709551616");
    }
    let bucket = || {
        Step::get(
            "/storage/v1/bucket/native-evidence".into(),
            200,
            br#"{"id":"native-evidence","public":false}"#.to_vec(),
        )
    };
    let object = |reference: &NativeArtifactRef, bytes: &[u8]| {
        Step::get(
            format!(
                "/storage/v1/object/authenticated/native-evidence/{}",
                reference.as_str()
            ),
            200,
            bytes.to_vec(),
        )
    };
    let mock = Mock::new(vec![
        Step::json(PATH, data),
        Step::json(PATH, changed_release),
        Step::json(PATH, reversed),
        Step::json(PATH, overflow),
        bucket(),
        object(checkpoint.reference(), checkpoint.original_bytes()),
        bucket(),
        object(original.reference(), original.original_bytes()),
    ]);
    let repo = repository(&mock);
    let accepted = repo.read(g).unwrap().unwrap();
    let expected = accepted.checkpoint_expected(&original).unwrap();
    for _ in 0..3 {
        let source = repo.read(g).unwrap().unwrap();
        assert!(source.checkpoint_expected(&original).is_err());
    }
    let store = NativeArtifactStore::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "source-reconstruction-test",
            Duration::from_secs(1),
        )
        .unwrap(),
        "native-evidence".into(),
    )
    .unwrap();
    let restored = load_checkpoint(
        &store,
        &expected,
        &accepted.initial_expected().unwrap(),
        &program,
        NativeRecoveryBudget::new(
            1,
            original.reference().size() + checkpoint.reference().size(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        restored.basis().epoch.get(),
        original.candidate().state().basis().epoch
    );
    assert_eq!(restored.candidate(), checkpoint.candidate());
}

#[test]
fn six_seat_projection_preserves_seat_order_while_clocks_join_by_uuid() {
    let mut value = fixture();
    let seats:Vec<Value>=(1..=6).rev().enumerate().map(|(index,id)|json!({
        "seatId":Uuid::from_u128(id),"seatIndex":(index*3).to_string(),"accountId":null,
        "controllerKind":"ai","sourceRevisionId":Uuid::from_u128(200+id),"deckSnapshotRef":"fixture:original-ai"})).collect();
    let clocks: Vec<Value> = (1..=6)
        .map(|id| {
            json!({"seatId":Uuid::from_u128(id),"policyRef":"bo3-overall-bank-v1","remainingMs":"120000",
        "clockRevision":"4","paused":true,"lastAccountedAt":"2026-11-06T00:00:00.000000Z"})
        })
        .collect();
    value["data"]["seats"] = json!(seats);
    value["data"]["matchClocks"] = json!(clocks);
    let mock = Mock::new(vec![Step::json(PATH, value)]);
    let repo = repository(&mock);
    let source = repo.read(game()).unwrap().unwrap();
    assert_eq!(
        source
            .controllers()
            .map(|(seat, _, _)| seat)
            .collect::<Vec<_>>(),
        (1..=6).rev().map(Uuid::from_u128).collect::<Vec<_>>()
    );
    assert_eq!(
        source
            .clocks()
            .map(|(seat, _, _, _, _)| seat)
            .collect::<Vec<_>>(),
        (1..=6).map(Uuid::from_u128).collect::<Vec<_>>()
    );
    assert_eq!(mock.finish().len(), 1);
}

#[test]
fn clock_policy_is_observed_exactly_without_balance_inference_or_foreign_lookup() {
    // The BO3-zero row is a private wire control, not real bank-expiry proof.
    for (policy, remaining) in [
        ("bo1-bank-inactive-v1", "0"),
        ("bo3-overall-bank-v1", "0"),
        ("bo3-overall-bank-v1", "1800000"),
        (" future-clock-v2 ", "0"),
    ] {
        let mut value = fixture();
        for clock in value["data"]["matchClocks"].as_array_mut().unwrap() {
            clock["policyRef"] = json!(policy);
            clock["remainingMs"] = json!(remaining);
        }
        let mock = Mock::new(vec![Step::json(PATH, value)]);
        let source = repository(&mock).read(game()).unwrap().unwrap();
        assert_eq!(source.clock_policy(Uuid::from_u128(1)), Some(policy));
        assert_eq!(source.clock_policy(Uuid::from_u128(2)), Some(policy));
        assert_eq!(source.clock_policy(Uuid::nil()), None);
        assert_eq!(source.clock_policy(Uuid::new_v4()), None);
        assert!(
            source
                .clocks()
                .all(|(_, amount, _, paused, _)| amount.to_string() == remaining && paused)
        );
        assert_eq!(mock.finish().len(), 1);
    }
}
#[test]
fn clock_policy_requires_one_nonnull_text_field_and_rejects_ambiguous_wire() {
    let mut cases = vec![];
    for invalid in [
        Value::Null,
        json!(0),
        json!(false),
        json!({}),
        json!(""),
        json!("   "),
        json!("bad\0policy"),
        json!("x".repeat(2049)),
    ] {
        let mut value = fixture();
        value["data"]["matchClocks"][0]["policyRef"] = invalid;
        cases.push(Step::json(PATH, value));
    }
    let mut missing = fixture();
    missing["data"]["matchClocks"][0]
        .as_object_mut()
        .unwrap()
        .remove("policyRef");
    cases.push(Step::json(PATH, missing));
    let mut duplicate = Step::json(PATH, Value::Null);
    duplicate.body = serde_json::to_string(&fixture())
        .unwrap()
        .replacen(
            "\"policyRef\":\"bo3-overall-bank-v1\"",
            "\"policyRef\":\"bo3-overall-bank-v1\",\"policyRef\":\"bo1-bank-inactive-v1\"",
            1,
        )
        .into_bytes();
    cases.push(duplicate);
    let count = cases.len();
    let mock = Mock::new(cases);
    let repo = repository(&mock);
    for _ in 0..count {
        assert!(repo.read(game()).is_err());
    }
    assert_eq!(mock.finish().len(), count);
}
