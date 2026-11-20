//! Wire consistency, not actual caller authentication or native card qualification.
use super::*;
use crate::test_http::{Mock, Step, positional};
use serde_json::Value;
const PATH: &str = "/rest/v1/rpc/read_native_human_access";
fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
pub(in crate::native_game::source_reader) fn fixture() -> Value {
    let reference = |role: &str| {
        format!(
            "native-artifact-v1/{}/{}/{role}/1/{}.json",
            id(4),
            id(5),
            "a".repeat(64)
        )
    };
    json!({"status":"success","data":{
        "actor":{"seatId":id(6),"kind":"human","accountId":id(3),"grantId":null,"bindingId":null,"bindingEpoch":null},
        "source":{"gameId":id(4),"matchId":id(7),"contextId":id(5),"nativeEpoch":"42","nativeRevision":"0",
            "runtimeId":"b".repeat(64),"programSourceId":"c".repeat(64),"initialRef":reference("initial"),"stateRef":reference("state"),
            "checkpointId":id(8),"lastEventSequence":"0","gameState":"active","matchState":"active","availability":"available"},
        "disclosure":{"live":true,"completed":false,"operationalPrefix":false,"replayManifestId":null}
    }})
}
fn repository(mock: &Mock) -> NativeSourceRepository {
    NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "access-test",
            Duration::from_secs(2),
        )
        .unwrap(),
    )
}
fn read(repo: &NativeSourceRepository) -> Result<NativeHumanAccessOutcome, NativeSourceError> {
    repo.human_identity(id(1), id(2), id(3), id(4), id(5))
}
#[test]
fn human_source_preserves_viewer_native_cut_and_distinct_disclosure_permissions() {
    let mut completed = fixture();
    completed["data"]["source"]["gameState"] = json!("completed");
    completed["data"]["disclosure"]["completed"] = json!(true);
    let mut prefix = fixture();
    prefix["data"]["source"]["gameState"] = json!("no_contest");
    prefix["data"]["disclosure"]["operationalPrefix"] = json!(true);
    prefix["data"]["disclosure"]["replayManifestId"] = json!(id(9));
    let mut step = Step::json(PATH, fixture());
    step.expect = Some(
        serde_json::to_vec(
            &json!({"p_subject":id(1),"p_session":id(2),"p_game":id(4),"p_context":id(5)}),
        )
        .unwrap(),
    );
    let mock = Mock::new(vec![
        step,
        Step::json(PATH, completed),
        Step::json(PATH, prefix),
    ]);
    let repo = repository(&mock);
    assert!(
        repo.human_identity(Uuid::nil(), id(2), id(3), id(4), id(5))
            .is_err()
    );
    for kind in 0..3 {
        let NativeHumanAccessOutcome::Granted(access) = read(&repo).unwrap() else {
            panic!("expected human source")
        };
        assert_eq!(access.viewer_seat_id(), id(6));
        assert_eq!(access.basis().epoch.get(), 42);
        assert_eq!(access.basis().game_id, id(4));
        assert_eq!(access.basis().context_id, id(5));
        assert_eq!(access.completed_disclosure().is_some(), kind == 1);
        assert_eq!(access.replay().1, kind == 2);
        assert_eq!(
            access.replay().0,
            if kind == 2 { Some(id(9)) } else { None }
        );
    }
    assert_eq!(mock.finish().len(), 3);
}
#[test]
fn human_source_rejects_ai_authority_foreign_cut_and_contradictory_disclosure() {
    let good = fixture();
    let mut cases = vec![];
    for (pointer, value) in [
        ("/data/actor/kind", json!("ai")),
        ("/data/actor/accountId", json!(id(99))),
        ("/data/actor/grantId", json!(id(99))),
        ("/data/actor/bindingId", json!(id(99))),
        ("/data/actor/bindingEpoch", json!("1")),
        ("/data/source/gameId", json!(id(99))),
        ("/data/source/contextId", json!(id(99))),
        ("/data/source/nativeEpoch", json!("18446744073709551616")),
        ("/data/source/nativeRevision", json!("01")),
        ("/data/source/nativeRevision", json!("1")),
        ("/data/source/gameState", json!("reserved")),
        ("/data/source/matchState", json!("admitting")),
        ("/data/source/runtimeId", json!("B".repeat(64))),
        (
            "/data/source/programSourceId",
            json!(format!("sha256:{}", "c".repeat(64))),
        ),
        (
            "/data/source/stateRef",
            good["data"]["source"]["initialRef"].clone(),
        ),
        ("/data/disclosure/live", json!(false)),
        ("/data/disclosure/completed", json!(true)),
        ("/data/disclosure/operationalPrefix", json!(true)),
        ("/data/disclosure/replayManifestId", json!(id(9))),
    ] {
        let mut case = good.clone();
        *case.pointer_mut(pointer).unwrap() = value;
        cases.push(case);
    }
    for branch in ["actor", "source", "disclosure"] {
        for key in good["data"][branch].as_object().unwrap().keys() {
            let mut case = good.clone();
            case["data"][branch].as_object_mut().unwrap().remove(key);
            cases.push(case);
        }
        let fields: Vec<_> = good["data"][branch]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut case = good.clone();
        case["data"][branch] = positional(&good["data"][branch], &fields);
        cases.push(case);
        let mut case = good.clone();
        case["data"][branch]["extra"] = json!(true);
        cases.push(case);
    }
    let count = cases.len();
    let mock = Mock::new(
        cases
            .into_iter()
            .map(|case| Step::json(PATH, case))
            .collect(),
    );
    let repo = repository(&mock);
    for _ in 0..count {
        assert!(read(&repo).is_err());
    }
    assert_eq!(mock.finish().len(), count);
}
#[test]
fn human_source_distinguishes_declared_denial_from_invalid_upstream() {
    let raw = serde_json::to_string(&fixture()).unwrap();
    let duplicate = raw.replacen(
        "\"bindingEpoch\":null",
        "\"bindingEpoch\":null,\"bindingEpoch\":null",
        1,
    );
    assert_ne!(raw, duplicate);
    let mut repeated = Step::json(PATH, fixture());
    repeated.body = duplicate.into_bytes();
    let mock = Mock::new(vec![
        Step::json(
            PATH,
            json!({"status":"fail","data":{"session":"unauthorized"}}),
        ),
        Step::json(PATH, json!({"status":"fail","data":{"game":"unavailable"}})),
        Step::json(
            PATH,
            json!({"status":"fail","data":{"actor":"unavailable"}}),
        ),
        Step::json(
            PATH,
            json!({"status":"fail","data":{"game":"unavailable","session":"unauthorized"}}),
        ),
        Step::json(
            PATH,
            json!({"status":"error","message":"upstream diagnostic"}),
        ),
        repeated,
    ]);
    let repo = repository(&mock);
    assert!(matches!(
        read(&repo).unwrap(),
        NativeHumanAccessOutcome::Unauthorized
    ));
    assert!(matches!(
        read(&repo).unwrap(),
        NativeHumanAccessOutcome::Unavailable
    ));
    for _ in 0..4 {
        assert!(read(&repo).is_err());
    }
    assert_eq!(mock.finish().len(), 6);
}

#[test]
fn late_delivery_comparison_includes_disclosure_even_with_equal_native_source() {
    let mut old = fixture();
    old["data"]["source"]["gameState"] = json!("completed");
    let mut new = old.clone();
    new["data"]["disclosure"]["completed"] = json!(true);
    let old = serde_json::from_value::<Record>(old["data"].clone())
        .unwrap()
        .into_access(id(3), id(4), id(5))
        .unwrap();
    let new = serde_json::from_value::<Record>(new["data"].clone())
        .unwrap()
        .into_access(id(3), id(4), id(5))
        .unwrap();
    assert!(!old.same_delivery_source(&new));
    assert!(old.same_delivery_source(&old.clone()));
}
#[test]
fn only_a_new_replay_manifest_is_tolerated_between_observations() {
    let access = |edit: &dyn Fn(&mut Value)| {
        let mut value = fixture();
        edit(&mut value["data"]);
        NativeHumanAccess {
            record: Box::new(serde_json::from_value(value["data"].clone()).unwrap()),
        }
    };
    let completed = |d: &mut Value| {
        d["source"]["gameState"] = json!("completed");
        d["disclosure"]["completed"] = json!(true);
    };
    let manifest = |d: &mut Value| d["disclosure"]["replayManifestId"] = json!(id(9));
    let before = access(&completed);
    let published = access(&|d| {
        completed(d);
        manifest(d);
    });
    assert!(before.same_except_publication(&published));
    // Removal, no change, or any other difference is not a publication.
    assert!(!published.same_except_publication(&before));
    assert!(!before.same_except_publication(&before.clone()));
    let others: [&dyn Fn(&mut Value); 6] = [
        &|d| d["source"]["nativeRevision"] = json!("1"),
        &|d| d["source"]["lastEventSequence"] = json!("1"),
        &|d| d["source"]["checkpointId"] = json!(id(10)),
        &|d| d["actor"]["seatId"] = json!(id(11)),
        &|d| d["disclosure"]["completed"] = json!(false),
        &|d| d["disclosure"]["live"] = json!(false),
    ];
    for other in others {
        let changed = access(&|d| {
            completed(d);
            manifest(d);
            other(d);
        });
        assert!(!before.same_except_publication(&changed));
    }
    // Operational no-contest: the prefix grant arrives with the manifest only.
    let incident = |d: &mut Value| d["source"]["gameState"] = json!("no_contest");
    let prefix = access(&|d| {
        incident(d);
        d["disclosure"]["operationalPrefix"] = json!(true);
        manifest(d);
    });
    assert!(access(&incident).same_except_publication(&prefix));
    assert!(!prefix.same_except_publication(&access(&incident)));
    // A live game cannot gain the prefix grant.
    let live_prefix = access(&|d| {
        d["disclosure"]["operationalPrefix"] = json!(true);
        manifest(d);
    });
    assert!(!access(&|_| {}).same_except_publication(&live_prefix));
}
