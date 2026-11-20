//! Private wire/ref and acknowledgment controls, not retained native byte truth.
use super::*;
use crate::test_http::{Mock, Step};
use serde_json::Value;
const PATH: &str = "/rest/v1/rpc/read_native_system_disposition";
fn game() -> Uuid {
    Uuid::from_u128(701)
}
fn context() -> Uuid {
    Uuid::from_u128(702)
}
fn reference(role: NativeArtifactRole) -> NativeArtifactRef {
    NativeArtifactRef::for_bytes(game(), context(), role, b"SQL-independent original fixture")
        .unwrap()
}
fn opening() -> Value {
    json!({"version":"native-opening-v1","matchId":Uuid::from_u128(703),"gameId":game(),"owner":"fixture:old-owner","originalBasis":"9","opening":{"contextId":context(),"snapshotRef":reference(NativeArtifactRole::Initial),"contextRevision":"fixture:context","stateRef":reference(NativeArtifactRole::State),"factsRef":reference(NativeArtifactRole::Facts),"eventRef":reference(NativeArtifactRole::Event),"qualificationRef":"fixture:unqualified"},"nativeEpoch":"0","runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)})
}
fn host(kind: &str) -> Value {
    json!({"version":"native-host-input-v1","basis":{"gameId":game(),"contextId":context(),"epoch":"0","revision":"3"},"kind":kind,"inputRef":reference(NativeArtifactRole::Input),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)})
}
fn parse(request: Uuid, value: &Value) -> Result<NativeSystemOriginal, NativeSourceError> {
    NativeSystemOriginal::from_bytes(request, &serde_json::to_vec(value).unwrap())
}
fn repo(mock: &Mock) -> NativeSourceRepository {
    NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "system-original-test",
            Duration::from_secs(1),
        )
        .unwrap(),
    )
}
fn read(value: &Value, response: Value) -> Result<NativeSystemDisposition, NativeSourceError> {
    let request = if value["version"] == "native-opening-v1" {
        game()
    } else {
        Uuid::from_u128(704)
    };
    let original = parse(request, value)?;
    let mut step = Step::json(PATH, response);
    step.expect = Some(
        serde_json::to_vec(&json!({"p_game":game(),"p_request":request,"p_original":value}))
            .unwrap(),
    );
    let mock = Mock::new(vec![step]);
    let result = repo(&mock).read_system_disposition(&original);
    mock.finish();
    result
}
#[test]
fn complete_original_parser_keeps_old_absence_and_existing_host_vocabulary() {
    let value = opening();
    let original = parse(game(), &value).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(original.original_json()).unwrap(),
        value
    );
    assert!(
        original
            .opening_fields()
            .unwrap()
            .artifacts
            .event_schema_revision()
            .is_none()
    );
    assert_eq!(
        original.opening_fields().unwrap().original_basis,
        &Amount::from(9)
    );
    assert!(original.host_fields().is_none());
    for (name, kind) in [
        ("entropy", NativeHostInputKind::Entropy),
        ("clock", NativeHostInputKind::Clock),
        ("native-work", NativeHostInputKind::NativeWork),
    ] {
        let original = parse(Uuid::from_u128(704), &host(name)).unwrap();
        assert_eq!(original.host_fields().unwrap().kind, kind);
        assert!(original.opening_fields().is_none());
    }
    let mut wrong = host("entropy");
    wrong["inputRef"] = json!(reference(NativeArtifactRole::Event));
    assert!(parse(Uuid::from_u128(704), &wrong).is_err());
    assert!(parse(Uuid::from_u128(704), &value).is_err());
    let mut wrong = value.clone();
    wrong["opening"]["eventSchemaRevision"] = Value::Null;
    assert!(parse(game(), &wrong).is_err());
}
#[test]
fn retained_opening_disposition_needs_no_current_runtime_lease_or_storage() {
    let value = opening();
    let response = json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":game(),"contextId":context(),"revision":"10"}}}});
    match read(&value, response).unwrap() {
        NativeSystemDisposition::Accepted(NativeSystemAck::Opening(ack)) => {
            assert_eq!(ack.revision, Amount::from(10))
        }
        _ => panic!("expected original opening acknowledgment"),
    }
    assert!(matches!(
        read(
            &value,
            json!({"status":"success","data":{"state":"unknown","response":null}})
        )
        .unwrap(),
        NativeSystemDisposition::Unknown
    ));
    assert!(matches!(
        read(
            &value,
            json!({"status":"fail","data":{"request":"conflict"}})
        )
        .unwrap(),
        NativeSystemDisposition::Conflict
    ));
}
#[test]
fn disposition_rejects_missing_or_substituted_original_acknowledgments() {
    let value = host("entropy");
    for response in [
        json!({"status":"success","data":{"state":"unknown"}}),
        json!({"status":"success","data":{"state":"unknown","response":{}}}),
        json!({"status":"success","data":{"state":"accepted","response":null}}),
        json!({"status":"success","data":{"state":"pending","response":null}}),
        json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":Uuid::from_u128(705),"revision":"5"}}}}),
        json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"secret":"do not publish"}}}}),
    ] {
        assert!(read(&value, response).is_err());
    }
    let response = json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":Uuid::from_u128(705),"revision":"4"}}}});
    assert!(matches!(
        read(&value, response).unwrap(),
        NativeSystemDisposition::Accepted(NativeSystemAck::Host(_))
    ));
}

#[test]
fn saved_original_comparison_uses_jsonb_intent_and_checks_command_scope_without_current_runtime() {
    use crate::admission_ports::{AcceptReservedGame, OpeningArtifacts};
    let value = opening();
    let original = parse(game(), &value).unwrap();
    let pretty = NativeSystemOriginal::from_bytes(
        game(),
        serde_json::to_string_pretty(&value).unwrap().as_bytes(),
    )
    .unwrap();
    assert!(original.matches_original(&pretty).unwrap());
    let mut changed = value.clone();
    changed["runtimeId"] = json!("3".repeat(64));
    assert!(
        !original
            .matches_original(&parse(game(), &changed).unwrap())
            .unwrap()
    );
    let mut command = AcceptReservedGame {
        match_id: Uuid::from_u128(703),
        expected_revision: 9.into(),
        game_id: game(),
        owner: "fixture:old-owner".parse().unwrap(),
        original_accepted_basis: Some(9.into()),
        opening: OpeningArtifacts::from_bytes(&serde_json::to_vec(&value["opening"]).unwrap())
            .unwrap(),
    };
    assert!(original.matches_opening_command(&command).unwrap());
    command.game_id = Uuid::new_v4();
    assert!(!original.matches_opening_command(&command).unwrap());
    command.game_id = game();
    command.expected_revision = 10.into();
    assert!(!original.matches_opening_command(&command).unwrap());
    command.expected_revision = 9.into();
    command.original_accepted_basis = Some(10.into());
    assert!(!original.matches_opening_command(&command).unwrap());
    command.original_accepted_basis = Some(9.into());
    let mut changed = value["opening"].clone();
    changed["contextRevision"] = json!("changed");
    command.opening = OpeningArtifacts::from_bytes(&serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(!original.matches_opening_command(&command).unwrap());
}
