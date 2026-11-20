//! Original private discovery bytes and original query scope, not worker qualification.
use super::*;
use serde_json::json;
fn query() -> ReadStopWork {
    ReadStopWork {
        binding_id: Uuid::new_v4(),
        after: None,
        limit: 2,
    }
}
fn row(binding: Uuid, stop: Uuid) -> serde_json::Value {
    json!({"bindingId":binding,"stopId":stop,"epoch":"2","startId":null,"processEpoch":null,"providerRef":null})
}
#[test]
fn stop_page_scope_cursor_and_historical_nullable_fields_are_exact() {
    let mut q = query();
    let mut ids = [Uuid::new_v4(), Uuid::new_v4()];
    ids.sort_unstable();
    let page = json!({"bindingId":q.binding_id,"items":[row(q.binding_id,ids[0]),row(q.binding_id,ids[1])],"nextCursor":ids[1]});
    let decoded = q.decode_page(page.to_string().as_bytes()).unwrap();
    assert_eq!(decoded.items[0].epoch, Amount::from(2));
    assert!(
        decoded.items[0].start_id.is_none()
            && decoded.items[0].process_epoch.is_none()
            && decoded.items[0].provider_ref.is_none()
    );
    for (field, value) in [
        ("bindingId", json!(Uuid::new_v4())),
        ("nextCursor", json!(null)),
    ] {
        let mut bad = page.clone();
        bad[field] = value;
        assert!(q.decode_page(bad.to_string().as_bytes()).is_err());
    }
    for rows in [
        json!([row(q.binding_id, ids[1]), row(q.binding_id, ids[0])]),
        json!([row(q.binding_id, ids[0]), row(q.binding_id, ids[0])]),
        json!([row(Uuid::new_v4(), ids[0]), row(q.binding_id, ids[1])]),
    ] {
        let mut bad = page.clone();
        bad["items"] = rows;
        assert!(q.decode_page(bad.to_string().as_bytes()).is_err());
    }
    let short =
        json!({"bindingId":q.binding_id,"items":[row(q.binding_id,ids[1])],"nextCursor":null});
    q.after = Some(ids[0]);
    assert!(q.decode_page(short.to_string().as_bytes()).is_ok());
    q.after = Some(ids[1]);
    assert!(q.decode_page(short.to_string().as_bytes()).is_err());
    q.after = None;
    q.limit = 1;
    assert!(q.decode_page(page.to_string().as_bytes()).is_err());
    for limit in [0, 101] {
        q.limit = limit;
        assert!(q.decode_page(short.to_string().as_bytes()).is_err());
    }
}
#[test]
fn stop_page_original_objects_and_required_nulls_never_normalize() {
    let q = query();
    let stop = Uuid::new_v4();
    let page = json!({"bindingId":q.binding_id,"items":[row(q.binding_id,stop)],"nextCursor":null});
    let text = page.to_string();
    for field in ["startId", "processEpoch", "providerRef"] {
        let mut bad = page.clone();
        bad["items"][0].as_object_mut().unwrap().remove(field);
        assert!(q.decode_page(bad.to_string().as_bytes()).is_err());
    }
    let mut bad = page.clone();
    bad.as_object_mut().unwrap().remove("nextCursor");
    assert!(q.decode_page(bad.to_string().as_bytes()).is_err());
    let duplicate = text.replacen("\"epoch\":\"2\"", "\"epoch\":\"2\",\"epoch\":\"2\"", 1);
    assert!(q.decode_page(duplicate.as_bytes()).is_err());
    let escaped = text.replacen(
        "\"epoch\":\"2\"",
        "\"epoch\":\"2\",\"ep\\u006fch\":\"2\"",
        1,
    );
    assert!(q.decode_page(escaped.as_bytes()).is_err());
    let mut bad = page.clone();
    bad["items"] = json!([[q.binding_id, stop, "2", null, null, null]]);
    assert!(q.decode_page(bad.to_string().as_bytes()).is_err());
    assert!(
        q.decode_page(json!([q.binding_id, [], null]).to_string().as_bytes())
            .is_err()
    );
}
#[test]
fn stop_page_original_byte_budget_is_enforced_before_typed_parse() {
    let q = query();
    let mut bytes = json!({"bindingId":q.binding_id,"items":[],"nextCursor":null})
        .to_string()
        .into_bytes();
    bytes.resize(MAX_STOP_WORK_PAGE_BYTES, b' ');
    assert!(q.decode_page(&bytes).is_ok());
    bytes.push(b' ');
    assert!(q.decode_page(&bytes).is_err());
}
