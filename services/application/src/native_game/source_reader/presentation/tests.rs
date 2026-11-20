//! Private response boundary tests, not executable SQL/Auth/retained native proof.
use super::*;
use crate::test_http::{Mock, Step, positional};
use serde_json::Value;
fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
const PATH: &str = "/rest/v1/rpc/read_native_human_presentation";
fn free() -> AppearanceSelection {
    AppearanceSelection {
        back_id: id(10),
        sleeve_id: id(11),
        dice_id: id(12),
        playmat_id: None,
    }
}
fn fixture() -> Value {
    json!({"status":"success","data":{"version":"native-presentation-v1",
        "access":access::tests::fixture()["data"],
        "presentation":{"seats":[{"seatId":id(6),"appearance":free()},{"seatId":id(9),"appearance":free()}]}}})
}
fn repository(mock: &Mock) -> NativeSourceRepository {
    NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "presentation-test",
            Duration::from_secs(2),
        )
        .unwrap(),
    )
}
fn read(repo: &NativeSourceRepository) -> Result<NativePresentationOutcome, NativeSourceError> {
    repo.presentation_identity(id(1), id(2), id(3), id(4), id(5), &free())
}
#[test]
fn presentation_requires_full_access_and_independent_roster() {
    let mut step = Step::json(PATH, fixture());
    step.expect=Some(serde_json::to_vec(&json!({"p_subject":id(1),"p_session":id(2),"p_game":id(4),"p_context":id(5),"p_free":free()})).unwrap());
    let mock = Mock::new(vec![step]);
    let repo = repository(&mock);
    let NativePresentationOutcome::Observed(observation) = read(&repo).unwrap() else {
        panic!("expected observation")
    };
    let access = NativeHumanAccess::fixture_for_outcome_test(0);
    observation
        .validate_against(&access, &[id(6), id(9)])
        .unwrap();
    assert!(
        observation
            .validate_against(&access, &[id(9), id(6)])
            .is_err()
    );
    assert!(
        observation
            .validate_against(
                &NativeHumanAccess::fixture_for_outcome_test(1),
                &[id(6), id(9)]
            )
            .is_err()
    );
    assert_eq!(mock.finish().len(), 1);
}
#[test]
fn presentation_rejects_malformed_scope_rows_and_partial_private_access() {
    let good = fixture();
    let mut cases = vec![];
    for (pointer, value) in [
        ("/data/version", json!("other")),
        ("/data/access/actor/accountId", json!(id(99))),
        ("/data/access/source/gameId", json!(id(99))),
        ("/data/access/disclosure/live", json!(false)),
        ("/data/presentation/seats/0/seatId", json!(id(99))),
        ("/data/presentation/seats/1/seatId", json!(id(6))),
        (
            "/data/presentation/seats/1/appearance/backId",
            json!(Uuid::nil()),
        ),
    ] {
        let mut v = good.clone();
        *v.pointer_mut(pointer).unwrap() = value;
        cases.push(v);
    }
    for pointer in [
        "/data",
        "/data/access",
        "/data/access/actor",
        "/data/access/source",
        "/data/access/disclosure",
        "/data/presentation",
        "/data/presentation/seats/0/appearance",
    ] {
        let object = good.pointer(pointer).unwrap();
        let fields: Vec<_> = object
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut v = good.clone();
        *v.pointer_mut(pointer).unwrap() = positional(object, &fields);
        cases.push(v);
        for key in &fields {
            let mut v = good.clone();
            v.pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(*key);
            cases.push(v);
        }
        let mut v = good.clone();
        v.pointer_mut(pointer).unwrap()["extra"] = json!(true);
        cases.push(v);
    }
    let count = cases.len();
    let mut steps: Vec<_> = cases.into_iter().map(|v| Step::json(PATH, v)).collect();
    let mut duplicate = Step::json(PATH, good.clone());
    duplicate.body = serde_json::to_string(&good)
        .unwrap()
        .replacen("\"live\":true", "\"live\":true,\"live\":false", 1)
        .into_bytes();
    steps.push(duplicate);
    let mock = Mock::new(steps);
    let repo = repository(&mock);
    for _ in 0..count + 1 {
        assert!(read(&repo).is_err());
    }
    assert_eq!(mock.finish().len(), count + 1);
}
#[test]
fn presentation_preserves_declared_failures_and_rejects_unknown_business_vocabulary() {
    let mock = Mock::new(vec![
        Step::json(
            PATH,
            json!({"status":"fail","data":{"session":"unauthorized"}}),
        ),
        Step::json(PATH, json!({"status":"fail","data":{"game":"unavailable"}})),
        Step::json(
            PATH,
            json!({"status":"fail","data":{"presentation":"unavailable"}}),
        ),
        Step::json(
            PATH,
            json!({"status":"fail","data":{"presentation":"unavailable","game":"unavailable"}}),
        ),
        Step::json(
            PATH,
            json!({"status":"fail","data":{"presentation":"other"}}),
        ),
    ]);
    let repo = repository(&mock);
    assert!(matches!(
        read(&repo).unwrap(),
        NativePresentationOutcome::Unauthorized
    ));
    assert!(matches!(
        read(&repo).unwrap(),
        NativePresentationOutcome::GameUnavailable
    ));
    assert!(matches!(
        read(&repo).unwrap(),
        NativePresentationOutcome::PresentationUnavailable
    ));
    assert!(read(&repo).is_err());
    assert!(read(&repo).is_err());
    assert_eq!(mock.finish().len(), 5);
}
