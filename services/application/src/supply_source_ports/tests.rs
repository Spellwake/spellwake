use super::*;
use serde_json::{Value, json};
const HASH: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
fn baseline() -> Value {
    json!({"version":"launch-baseline-v1","forecast":"1","cutoff":"2026-10-02T00:00:00.000000Z","configurationRef":HASH,"products":[{"recipeId":id(1),"productId":id(2),"eligibleAt":"2026-10-01T00:00:00.000000Z","evidenceRef":HASH,"modifierN":"1","modifierD":"1","modifierRef":HASH}]})
}
fn physical() -> Value {
    json!({"version":"physical-introduction-v1","recipeId":id(1),"productId":id(2),"runRef":"initial","language":"en","territory":"GLOBAL","unit":"physical_pack","qualification":"exact","physicalQuantity":"12345","evidenceRef":HASH,"modifierN":"1","modifierD":"1","modifierRef":HASH,"normalQuantity":"100"})
}
fn ballot() -> Value {
    json!({"version":"supply-ballot-v1","supplyCoefficient":"90","sourceRef":HASH,"tieSeed":"a".repeat(64),"candidates":(1..=5).map(|n|json!({"setId":id(n),"products":[{"recipeId":id(n+10),"weight":"1","normalQuantity":"100","modifierN":"1","modifierD":"1","modifierRef":HASH}]})).collect::<Vec<_>>()})
}
fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}
fn success(v: Value) -> Vec<u8> {
    bytes(&json!({"status":"success","data":v}))
}
fn physical_ack(disposition: &str, quantity: &str, window: Value) -> Vec<u8> {
    success(
        json!({"coverage":"a".repeat(64),"quantity":quantity,"windowId":window,"disposition":disposition}),
    )
}
#[test]
fn baseline_preserves_original_and_checks_complete_exact_mapping() {
    let v = baseline();
    let raw = format!(
        " {{  {}  }} ",
        serde_json::to_string(&v)
            .unwrap()
            .trim_start_matches('{')
            .trim_end_matches('}')
    );
    let input = BaselineManifest::from_bytes(raw.as_bytes()).unwrap();
    assert_eq!(input.original_json(), raw.trim());
    let output = |q: &str| {
        success(
            json!({"quantityPerProduct":q,"windows":json!([{"recipeId":id(1),"productId":id(2),"windowId":id(3),"quantity":q}]).to_string()}),
        )
    };
    assert!(input.decode_response(&output("120")).is_ok());
    assert!(input.decode_response(&output("119")).is_err());
    for v in [
        json!({"quantityPerProduct":"120","windows":[]}),
        json!({"quantityPerProduct":"120","windows":"[]"}),
    ] {
        assert!(input.decode_response(&success(v)).is_err());
    }
    let mut multi = baseline();
    let duplicate = multi["products"][0].clone();
    multi["products"].as_array_mut().unwrap().push(duplicate);
    assert!(BaselineManifest::from_bytes(&bytes(&multi)).is_err());
}
#[test]
fn baseline_rejects_unknown_duplicates_noncanonical_and_future_eligibility() {
    for (field, value) in [
        ("forecast", json!(1)),
        ("forecast", json!("01")),
        ("forecast", json!("1".repeat(31))),
        ("cutoff", json!("2026-10-02T00:00:00Z")),
        ("configurationRef", json!("a".repeat(64))),
        ("version", json!({"launch-baseline-v1":null})),
        ("extra", Value::Null),
    ] {
        let mut v = baseline();
        v[field] = value;
        assert!(BaselineManifest::from_bytes(&bytes(&v)).is_err(), "{field}");
    }
    let v = String::from_utf8(bytes(&baseline())).unwrap().replace(
        "\"forecast\":\"1\"",
        "\"forecast\":\"1\",\"forecast\":\"1\"",
    );
    assert!(BaselineManifest::from_bytes(v.as_bytes()).is_err());
    let mut v = baseline();
    v["products"][0]["eligibleAt"] = json!("2026-10-03T00:00:00.000000Z");
    assert!(BaselineManifest::from_bytes(&bytes(&v)).is_err());
    v = baseline();
    v["products"][0]["recipeId"] = json!(Uuid::nil());
    assert!(BaselineManifest::from_bytes(&bytes(&v)).is_err());
}
#[test]
fn physical_result_is_the_original_eligible_tuple_not_merely_well_typed() {
    let input = PhysicalIntroduction::from_bytes(&bytes(&physical())).unwrap();
    assert!(
        input
            .decode_response(&physical_ack("published", "123", json!(id(3))), None)
            .is_ok()
    );
    for (d, q, w) in [
        ("published", "124", json!(id(3))),
        ("published", "123", json!("")),
        ("zero", "0", json!("")),
        ("unavailable", "0", json!("")),
        ("published", "123", Value::Null),
    ] {
        assert!(input.decode_response(&physical_ack(d, q, w), None).is_err());
    }
    assert!(
        input
            .decode_response(
                &physical_ack("published", "123", json!(id(3))),
                Some(&"b".repeat(64))
            )
            .is_err()
    );
    let v = String::from_utf8(physical_ack("published", "123", json!(id(3))))
        .unwrap()
        .replace("\"published\"", "{\"published\":null}");
    assert!(input.decode_response(v.as_bytes(), None).is_err());
}
#[test]
fn physical_nonexact_fields_are_required_but_opaque_and_never_mint_stock() {
    for qualification in ["bounded", "unavailable"] {
        for quantity in [Value::Null, json!("roughly a lot")] {
            for unit in ["physical_pack", "box", ""] {
                let mut v = physical();
                v["qualification"] = json!(qualification);
                v["physicalQuantity"] = quantity.clone();
                v["evidenceRef"] = json!("unavailable");
                v["unit"] = json!(unit);
                let input = PhysicalIntroduction::from_bytes(&bytes(&v)).unwrap();
                assert!(
                    input
                        .decode_response(&physical_ack("unavailable", "0", json!("")), None)
                        .is_ok()
                );
                assert!(
                    input
                        .decode_response(&physical_ack("published", "1", json!(id(3))), None)
                        .is_err()
                );
            }
        }
    }
    for field in ["physicalQuantity", "evidenceRef"] {
        let mut v = physical();
        v["qualification"] = json!("unavailable");
        v.as_object_mut().unwrap().remove(field);
        assert!(PhysicalIntroduction::from_bytes(&bytes(&v)).is_err());
    }
    let mut v = physical();
    v["physicalQuantity"] = json!("99");
    let input = PhysicalIntroduction::from_bytes(&bytes(&v)).unwrap();
    assert!(
        input
            .decode_response(&physical_ack("zero", "0", json!("")), None)
            .is_ok()
    );
    v["unit"] = json!("");
    let input = PhysicalIntroduction::from_bytes(&bytes(&v)).unwrap();
    assert!(
        input
            .decode_response(&physical_ack("unavailable", "0", json!("")), None)
            .is_ok()
    );
}
#[test]
fn physical_exact_validation_and_original_budget_are_not_bypassed() {
    for (field, value) in [
        ("physicalQuantity", Value::Null),
        ("physicalQuantity", json!("01")),
        ("evidenceRef", Value::Null),
        ("modifierN", json!("0")),
        ("language", json!("EN")),
        ("territory", json!("global")),
        ("runRef", json!(" initial ")),
    ] {
        let mut v = physical();
        v[field] = value;
        assert!(
            PhysicalIntroduction::from_bytes(&bytes(&v)).is_err(),
            "{field}"
        );
    }
    let mut v = physical();
    v["qualification"] = json!("unavailable");
    v["evidenceRef"] = json!("x".repeat(66000));
    assert!(PhysicalIntroduction::from_bytes(&bytes(&v)).is_err());
}
#[test]
fn ballot_season_count_cutoff_and_nested_product_constraints() {
    let v = ballot();
    let input = SupplyBallot::from_bytes(id(100), &bytes(&v)).unwrap();
    let ack = success(
        json!({"resourceId":id(100),"candidates":"5","cutoff":"2026-12-25T00:00:00.000000Z"}),
    );
    assert!(input.decode_response(&ack, None).is_ok());
    assert!(
        input
            .decode_response(&ack, Some("2026-12-24T00:00:00.000000Z"))
            .is_err()
    );
    assert!(
        SupplyBallot::from_bytes(id(101), &bytes(&v))
            .unwrap()
            .decode_response(&ack, None)
            .is_err()
    );
    assert!(SupplyBallot::from_bytes(Uuid::nil(), &bytes(&v)).is_err());
    for (field, value) in [
        ("supplyCoefficient", json!("1000000000")),
        ("tieSeed", json!(HASH)),
        ("candidates", json!([])),
    ] {
        let mut v = ballot();
        v[field] = value;
        assert!(SupplyBallot::from_bytes(id(100), &bytes(&v)).is_err());
    }
    let mut v = ballot();
    v["candidates"][1]["setId"] = v["candidates"][0]["setId"].clone();
    assert!(SupplyBallot::from_bytes(id(100), &bytes(&v)).is_err());
}
#[test]
fn quarterly_conserves_values_without_demanding_recipe_uuid_order() {
    let input = FinalizeQuarterlySupply { season_id: id(100) };
    let windows = json!([{"recipeId":id(3),"windowId":id(7),"quantity":"2"},{"recipeId":id(1),"windowId":id(8),"quantity":"3"}]);
    let ack = |q: &str, w: &Value| {
        success(
            json!({"resourceId":id(100),"allocationRef":HASH,"quantity":q,"windows":w.to_string()}),
        )
    };
    assert!(input.decode_response(&ack("5", &windows)).is_ok());
    assert!(input.decode_response(&ack("6", &windows)).is_err());
    assert!(input.decode_response(&ack("0", &json!([]))).is_ok());
    let mut w = windows.clone();
    w[1]["windowId"] = w[0]["windowId"].clone();
    assert!(input.decode_response(&ack("5", &w)).is_err());
    w = windows;
    w[1]["recipeId"] = w[0]["recipeId"].clone();
    assert!(input.decode_response(&ack("5", &w)).is_err());
    let duplicate = format!(
        r#"{{"status":"success","data":{{"resourceId":"{}","allocationRef":"{}","quantity":"1","windows":"[{{\"recipeId\":\"{}\",\"recipeId\":\"{}\",\"windowId\":\"{}\",\"quantity\":\"1\"}}]"}}}}"#,
        id(100),
        HASH,
        id(1),
        id(1),
        id(2)
    );
    assert!(input.decode_response(duplicate.as_bytes()).is_err());
}
fn opening() -> Value {
    json!({"version":"pack-opening-v1","groups":[{"collationRef":"joint","outcomes":[{"weight":"1","cards":[{"slotId":id(1),"printingId":id(2),"quantity":"1"}]}]}]})
}
#[test]
fn opening_conditional_fields_preserve_omission_and_reject_explicit_null() {
    let v = opening();
    let a = BoosterOpening::from_bytes(&bytes(&v)).unwrap();
    let mut explicit = v.clone();
    explicit["groups"][0]["kind"] = json!("joint");
    let b = BoosterOpening::from_bytes(&bytes(&explicit)).unwrap();
    assert_ne!(a.original_json(), b.original_json());
    for (field, value) in [
        ("kind", Value::Null),
        ("kind", json!({"joint":null})),
        ("outcomes", Value::Null),
    ] {
        let mut v = opening();
        v["groups"][0][field] = value;
        assert!(BoosterOpening::from_bytes(&bytes(&v)).is_err());
    }
    let v = json!({"version":"pack-opening-v1","groups":[{"collationRef":"pool","kind":"multiset-without-replacement"}]});
    assert!(BoosterOpening::from_bytes(&bytes(&v)).is_ok());
    for value in [Value::Null, json!([])] {
        let mut v = v.clone();
        v["groups"][0]["outcomes"] = value;
        assert!(BoosterOpening::from_bytes(&bytes(&v)).is_err());
    }
}
#[test]
fn opening_nested_constraints_and_qualifier_targets() {
    for (field, value) in [
        ("weight", json!("0")),
        ("weight", json!("1".repeat(79))),
        ("cards", json!([])),
    ] {
        let mut v = opening();
        v["groups"][0]["outcomes"][0][field] = value;
        assert!(BoosterOpening::from_bytes(&bytes(&v)).is_err());
    }
    let mut v = opening();
    let card = v["groups"][0]["outcomes"][0]["cards"][0].clone();
    v["groups"][0]["outcomes"][0]["cards"]
        .as_array_mut()
        .unwrap()
        .push(card);
    assert!(BoosterOpening::from_bytes(&bytes(&v)).is_err());
    assert!(QualifyBoosterRecipe::new(id(1), HASH.to_owned().try_into().unwrap()).is_ok());
    assert!(QualifyBoosterRecipe::new(Uuid::nil(), HASH.to_owned().try_into().unwrap()).is_err());
    assert!(
        QualifyBoosterOpening::new(
            id(1),
            BoosterOpening::from_bytes(&bytes(&opening())).unwrap(),
            HASH.to_owned().try_into().unwrap()
        )
        .is_ok()
    );
}
#[test]
fn source_success_decoders_do_not_invent_void_or_failure_receipts() {
    let baseline = BaselineManifest::from_bytes(&bytes(&baseline())).unwrap();
    for bytes in [
        b"".as_slice(),
        b"null",
        b"{\"status\":\"fail\",\"data\":{\"source\":\"no\"}}",
        b"{\"status\":\"error\",\"message\":\"no\"}",
    ] {
        assert!(baseline.decode_response(bytes).is_err());
    }
}
