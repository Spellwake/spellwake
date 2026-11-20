//! Failure-map decoding preserves arbitrary unique field names without normalizing duplicates.
use super::*;
use serde_json::json;

#[test]
fn consumed_json_failure_maps_reject_raw_duplicates_and_keep_other_wire_shapes() {
    for raw in [
        r#"{"status":"fail","data":{"request":"conflict","request":"conflict"}}"#,
        r#"{"status":"fail","data":{"request":"invalid","request":"conflict"}}"#,
        r#"{"status":"fail","data":{"\u0072equest":"conflict","request":"conflict"}}"#,
        r#"{"status":"fail","data":[]}"#,
        r#"{"status":"fail","data":null}"#,
        r#"{"status":"fail","data":{"request":true}}"#,
    ] {
        assert!(serde_json::from_str::<ApiResponse<()>>(raw).is_err());
    }
    for data in [
        json!({}),
        json!({"":"","first":"reason","second":"another"}),
    ] {
        let value = json!({"status":"fail","data":data});
        let parsed: ApiResponse<()> = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
    assert!(
        serde_json::from_value::<ApiResponse<bool>>(json!({"status":"success","data":true}))
            .is_ok()
    );
    assert!(
        serde_json::from_value::<ApiResponse<Vec<String>>>(
            json!({"status":"success","data":["value"]})
        )
        .is_ok()
    );
    assert!(
        serde_json::from_value::<ApiResponse<()>>(json!({"status":"success","data":null})).is_ok()
    );
}
