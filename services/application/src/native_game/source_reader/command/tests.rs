//! Raw canonical receipt -> public correlated receipt, with no native execution.
use super::*;
use serde_json::Value;
mod uncertain;
fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
fn original(n: u128) -> PlayerCommand {
    PlayerCommand {
        request_id: id(n),
        basis: PlayerBasis {
            game_id: id(1),
            context_id: id(2),
            epoch: 0.into(),
            revision: 3.into(),
        },
        action: crate::player_wire::PlayerAction::Pass {},
    }
}
fn query() -> PlayerReceiptQuery {
    PlayerReceiptQuery {
        game_id: id(1),
        context_id: id(2),
        outstanding: vec![original(10), original(11)],
    }
}
fn ack(n: u128) -> Value {
    json!({"requestId":id(n),"actorSeatId":id(100),"gameId":id(1),"contextId":id(2),"epoch":"0","revision":"4"})
}
fn fixture() -> Value {
    json!({"status":"success","data":{"receipts":[
        {"actorSeatId":id(100),"original":original(10),"state":"accepted","response":{"status":"success","data":ack(10)}},
        {"actorSeatId":id(100),"original":original(11),"state":"rejected","response":{"status":"fail","data":{"basis":"stale"}}}
    ]}})
}
fn decode(
    bytes: &[u8],
    query: &PlayerReceiptQuery,
) -> Result<ApiResponse<Vec<PlayerReceipt>>, NativeSourceError> {
    let response: ObjectResponse<Batch> =
        crate::decode_wire_object(bytes).map_err(|error| backend_error("receipt", error))?;
    decode_batch(response.0, query)
}
#[test]
fn historical_flat_ack_and_rejection_preserve_original_intents_without_latest_basis() {
    let q = query();
    let ApiResponse::Success { data } =
        decode(&serde_json::to_vec(&fixture()).unwrap(), &q).unwrap()
    else {
        panic!("receipt data required")
    };
    q.validate_receipts(&data, id(100)).unwrap();
    let CommandDisposition::Accepted { ack } = &data[0].disposition else {
        panic!("accepted original required")
    };
    assert_eq!(ack.basis.revision.get(), 4);
    assert_eq!(ack.request_id, id(10));
    assert!(matches!(
        data[1].disposition,
        CommandDisposition::Rejected { .. }
    ));
    let unknown = json!({"status":"success","data":{"receipts":q.outstanding.iter().map(|original|json!({"actorSeatId":id(100),"original":original,"state":"unknown","response":null})).collect::<Vec<_>>()}});
    let ApiResponse::Success { data } = decode(&serde_json::to_vec(&unknown).unwrap(), &q).unwrap()
    else {
        panic!("unknown originals required")
    };
    assert!(
        data.iter()
            .all(|receipt| matches!(receipt.disposition, CommandDisposition::Unknown))
    );
    let empty = PlayerReceiptQuery {
        game_id: id(1),
        context_id: id(2),
        outstanding: vec![],
    };
    assert!(
        matches!(decode(br#"{"status":"success","data":{"receipts":[]}}"#,&empty).unwrap(),ApiResponse::Success {data} if data.is_empty())
    );
}
#[test]
fn historical_receipts_reject_state_response_original_order_actor_and_ack_drift() {
    let good = fixture();
    let q = query();
    let mut cases = vec![];
    for (pointer, value) in [
        ("/data/receipts/0/state", json!("pending")),
        ("/data/receipts/0/state", json!("unknown")),
        ("/data/receipts/0/response", Value::Null),
        ("/data/receipts/0/response/status", json!("fail")),
        ("/data/receipts/0/response/data/requestId", json!(id(11))),
        ("/data/receipts/0/response/data/actorSeatId", json!(id(101))),
        ("/data/receipts/0/response/data/gameId", json!(id(99))),
        ("/data/receipts/0/response/data/contextId", json!(id(99))),
        ("/data/receipts/0/response/data/epoch", json!("1")),
        ("/data/receipts/0/response/data/revision", json!("3")),
        ("/data/receipts/0/response/data/revision", json!("04")),
        (
            "/data/receipts/0/response/data/revision",
            json!("18446744073709551616"),
        ),
        ("/data/receipts/0/original/action/kind", json!("concede")),
        ("/data/receipts/1/actorSeatId", json!(id(101))),
        (
            "/data/receipts/1/response/data/basis",
            json!("private diagnostics"),
        ),
    ] {
        let mut case = good.clone();
        *case.pointer_mut(pointer).unwrap() = value;
        cases.push(case);
    }
    let mut reordered = good.clone();
    reordered["data"]["receipts"]
        .as_array_mut()
        .unwrap()
        .reverse();
    cases.push(reordered);
    let mut missing = good.clone();
    missing["data"]["receipts"].as_array_mut().unwrap().pop();
    cases.push(missing);
    let mut duplicate = good.clone();
    duplicate["data"]["receipts"][1] = good["data"]["receipts"][0].clone();
    cases.push(duplicate);
    for field in ["actorSeatId", "original", "state", "response"] {
        let mut case = good.clone();
        case["data"]["receipts"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        cases.push(case);
    }
    for case in cases {
        assert!(decode(&serde_json::to_vec(&case).unwrap(), &q).is_err());
    }
    let raw = serde_json::to_string(&good).unwrap();
    let repeated = raw.replacen(
        "\"state\":\"accepted\"",
        "\"state\":\"accepted\",\"state\":\"accepted\"",
        1,
    );
    assert_ne!(raw, repeated);
    assert!(decode(repeated.as_bytes(), &q).is_err());
}
#[test]
fn historical_receipts_only_forward_declared_ingress_denials() {
    let q = query();
    for fields in [
        json!({"session":"unauthorized"}),
        json!({"game":"unavailable"}),
        json!({"request":"conflict"}),
    ] {
        assert!(matches!(
            decode(
                &serde_json::to_vec(&json!({"status":"fail","data":fields})).unwrap(),
                &q
            )
            .unwrap(),
            ApiResponse::Fail { .. }
        ));
    }
    for value in [
        json!({"status":"fail","data":{"actor":"unavailable"}}),
        json!({"status":"fail","data":{"request":"conflict","session":"unauthorized"}}),
        json!({"status":"error","message":"internal"}),
    ] {
        assert!(decode(&serde_json::to_vec(&value).unwrap(), &q).is_err());
    }
    assert!(
        decode(
            br#"{"status":"fail","data":{"request":"conflict","request":"conflict"}}"#,
            &q
        )
        .is_err()
    );
}
