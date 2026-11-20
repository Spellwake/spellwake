//! Shared strict SDK record tests for both member source lanes.
use crate::temporal_runtime::RecordPayload;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use temporalio_common::data_converters::{
    ActivitySerializationContext, DataConverter, SerializationContextData,
    WorkflowSerializationContext,
};
pub(super) async fn codec<T: Serialize + DeserializeOwned + 'static>(
    value: Value,
    fields: &[&str],
) {
    let converter = DataConverter::default();
    for context in [
        SerializationContextData::Workflow(WorkflowSerializationContext::new()),
        SerializationContextData::Activity(ActivitySerializationContext::new()),
    ] {
        let typed: T = serde_json::from_value(value.clone()).unwrap();
        let payload = converter
            .to_payload(&context, &RecordPayload(typed))
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&payload.data).unwrap(),
            value
        );
        let decoded = converter
            .from_payload::<RecordPayload<T>>(&context, payload.clone())
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(decoded.0).unwrap(), value);
        for bad in [
            crate::test_http::positional(&value, fields),
            Value::Null,
            json!([]),
            json!("bad"),
            json!(1),
        ] {
            let bad = converter.to_payload(&context, &bad).await.unwrap();
            assert!(
                converter
                    .from_payload::<RecordPayload<T>>(&context, bad)
                    .await
                    .is_err()
            );
        }
        for field in fields {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(*field);
            let bad = converter.to_payload(&context, &missing).await.unwrap();
            assert!(
                converter
                    .from_payload::<RecordPayload<T>>(&context, bad)
                    .await
                    .is_err(),
                "missing{field}"
            );
            let mut duplicate = payload.clone();
            duplicate.data = format!(
                "{{\"{field}\":{},{}",
                value[field],
                String::from_utf8(payload.data.clone())
                    .unwrap()
                    .trim_start_matches('{')
            )
            .into_bytes();
            assert!(
                converter
                    .from_payload::<RecordPayload<T>>(&context, duplicate)
                    .await
                    .is_err(),
                "duplicate{field}"
            );
        }
        let mut extra = value.clone();
        extra["providerToken"] = json!("forbidden");
        let extra = converter.to_payload(&context, &extra).await.unwrap();
        assert!(
            converter
                .from_payload::<RecordPayload<T>>(&context, extra)
                .await
                .is_err()
        );
    }
}
