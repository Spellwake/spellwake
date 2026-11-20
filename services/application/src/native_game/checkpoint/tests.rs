//! Original shape and budget controls need no synthetic native source.
use super::*;
use serde_json::{Value, json};
fn basis() -> PlayerBasis {
    PlayerBasis {
        game_id: Uuid::from_u128(1),
        context_id: Uuid::from_u128(2),
        epoch: 0.into(),
        revision: 0.into(),
    }
}
fn reference(role: NativeArtifactRole) -> NativeArtifactRef {
    NativeArtifactRef::for_bytes(
        basis().game_id,
        basis().context_id,
        role,
        b"private-fixture",
    )
    .unwrap()
}
fn manifest() -> Value {
    json!({"version":"native-checkpoint-v1","initialRef":reference(NativeArtifactRole::Initial).as_str(),"basis":basis(),"kind":"initial","parentRef":null,"inputRef":null,"viewerSeatId":null})
}
fn decode(bytes: &[u8]) -> Result<Manifest, NativeCheckpointError> {
    let m: Manifest = crate::decode_wire_object(bytes).map_err(NativeCheckpointError::Json)?;
    if !m.valid() {
        return Err(NativeCheckpointError::Invalid);
    }
    Ok(m)
}
#[test]
fn original_state_anchor_rejects_wrong_role_and_foreign_game_or_context() {
    let expected =
        || NativeCheckpointExpected::new(reference(NativeArtifactRole::State), basis()).unwrap();
    assert!(
        expected()
            .with_original_state(reference(NativeArtifactRole::Initial))
            .is_err()
    );
    for (game, context) in [
        (Uuid::from_u128(3), basis().context_id),
        (basis().game_id, Uuid::from_u128(3)),
    ] {
        let foreign =
            NativeArtifactRef::for_bytes(game, context, NativeArtifactRole::State, b"fixture")
                .unwrap();
        assert!(expected().with_original_state(foreign).is_err());
    }
    assert!(
        expected()
            .with_original_state(reference(NativeArtifactRole::State))
            .unwrap()
            .original_state
            .is_some()
    );
}
#[test]
fn original_manifest_rejects_missing_null_fields_duplicates_positional_and_inconsistent_linkage() {
    let original = manifest();
    assert!(decode(&serde_json::to_vec(&original).unwrap()).is_ok());
    for field in [
        "parentRef",
        "inputRef",
        "viewerSeatId",
        "basis",
        "version",
        "kind",
        "initialRef",
    ] {
        let mut v = original.clone();
        v.as_object_mut().unwrap().remove(field);
        assert!(decode(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    for (field, value) in [
        ("extra", json!(true)),
        ("kind", json!("future")),
        (
            "parentRef",
            json!(reference(NativeArtifactRole::State).as_str()),
        ),
        ("viewerSeatId", json!(Uuid::nil())),
        (
            "inputRef",
            json!(reference(NativeArtifactRole::Input).as_str()),
        ),
        ("basis", json!([basis().game_id, basis().context_id])),
        ("version", json!("native-checkpoint-v2")),
        (
            "initialRef",
            json!(reference(NativeArtifactRole::Input).as_str()),
        ),
    ] {
        let mut v = original.clone();
        v[field] = value;
        assert!(decode(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    let raw = serde_json::to_string(&original).unwrap();
    let duplicate = raw.replacen(
        "\"version\":",
        "\"version\":\"native-checkpoint-v1\",\"version\":",
        1,
    );
    assert!(decode(duplicate.as_bytes()).is_err());
    let duplicate = raw.replacen("\"revision\":", "\"revision\":\"0\",\"revision\":", 1);
    assert!(decode(duplicate.as_bytes()).is_err());
    assert!(decode(b"[]").is_err());
    assert!(decode(b"null").is_err());
    let mut v = original.clone();
    v["basis"]["revision"] = json!("1");
    assert!(decode(&serde_json::to_vec(&v).unwrap()).is_err());
    v["kind"] = json!("player");
    v["parentRef"] = json!(reference(NativeArtifactRole::State).as_str());
    v["inputRef"] = json!(reference(NativeArtifactRole::Input).as_str());
    v["viewerSeatId"] = json!(Uuid::from_u128(3));
    assert!(decode(&serde_json::to_vec(&v).unwrap()).is_ok());
    for field in ["parentRef", "inputRef", "viewerSeatId"] {
        let mut changed = v.clone();
        changed[field] = Value::Null;
        assert!(decode(&serde_json::to_vec(&changed).unwrap()).is_err());
    }
    let mut changed = v.clone();
    changed["parentRef"] = json!(reference(NativeArtifactRole::Input).as_str());
    assert!(decode(&serde_json::to_vec(&changed).unwrap()).is_err());
    let mut changed = v.clone();
    changed["basis"]["revision"] = json!("18446744073709551616");
    assert!(decode(&serde_json::to_vec(&changed).unwrap()).is_err());
}
#[test]
fn exact_scope_expected_and_checked_work_limits_fail_closed() {
    assert!(NativeRecoveryBudget::new(0, 1).is_err());
    assert!(NativeRecoveryBudget::new(1, 0).is_err());
    assert!(NativeCheckpointExpected::new(reference(NativeArtifactRole::Input), basis()).is_err());
    let state = reference(NativeArtifactRole::State);
    let mut foreign = basis();
    foreign.context_id = Uuid::from_u128(3);
    assert!(NativeCheckpointExpected::new(state.clone(), foreign).is_err());
    let mut budget = NativeRecoveryBudget::new(1, state.size()).unwrap();
    assert!(budget.charge(&state, true).is_ok());
    assert!(matches!(
        budget.charge(&state, true),
        Err(NativeCheckpointError::Budget)
    ));
    let mut budget = NativeRecoveryBudget::new(usize::MAX, u64::MAX).unwrap();
    budget.charge(&state, true).unwrap();
    assert_eq!(budget.records, usize::MAX - 1);
    assert_eq!(budget.bytes, u64::MAX - state.size());
    let mut next = basis();
    next.revision = 1.into();
    assert!(next_basis(&basis(), &next));
    next.epoch = 1.into();
    assert!(!next_basis(&basis(), &next));
    let mut last = basis();
    last.revision = u64::MAX.into();
    assert!(!next_basis(&last, &basis()));
}
#[test]
fn checkpoint_errors_preserve_causes_without_sensitive_formatting() {
    let error = NativeCheckpointError::Json(
        serde_json::from_slice::<Value>(b"{private-checkpoint-data}").unwrap_err(),
    );
    assert!(error.source().is_some());
    assert_eq!(format!("{error:?}"), "native checkpoint cannot be decoded");
}
