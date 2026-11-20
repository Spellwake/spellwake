//! Real pinned SDK codecs, without starting a Temporal server or executing worker effects.
use crate::{
    deletion_delivery::DeletionBatch,
    price_collector::maintenance::MaintenanceBatch,
    price_workflow::{PriceDrainBatch, probe::ProbeInput},
    stripe_workflow::{StripeBatch, StripeCursor, StripeWork},
    temporal_runtime::RecordPayload,
};
use chrono::NaiveDate;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::HashMap;
use temporalio_common::{
    Memo, MemoValues,
    data_converters::{
        ActivitySerializationContext, DataConverter, SerializationContextData,
        WorkflowSerializationContext,
    },
    protos::temporal::api::common::v1::Memo as RawMemo,
};
use uuid::Uuid;

async fn record<T: Serialize + DeserializeOwned + 'static>(value: Value, fields: &[&str]) {
    let converter = DataConverter::default();
    for context in [
        SerializationContextData::Workflow(WorkflowSerializationContext::new()),
        SerializationContextData::Activity(ActivitySerializationContext::new()),
    ] {
        let legacy: T = serde_json::from_value(value.clone()).unwrap();
        let before = converter.to_payload(&context, &legacy).await.unwrap();
        let wrapped = converter
            .to_payload(&context, &RecordPayload(legacy))
            .await
            .unwrap();
        assert_eq!(wrapped.metadata, before.metadata);
        assert_eq!(
            wrapped.data, before.data,
            "transparent serialization must preserve history bytes"
        );
        let decoded = converter
            .from_payload::<RecordPayload<T>>(&context, before.clone())
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(decoded.0).unwrap(), value);
        let old_reader = converter
            .from_payload::<T>(&context, wrapped)
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(old_reader).unwrap(), value);

        let positional = crate::test_http::positional(&value, fields);
        let payload = converter.to_payload(&context, &positional).await.unwrap();
        // Prove this is a complete otherwise-decodable sequence, not an unrelated failure.
        assert!(
            converter
                .from_payload::<T>(&context, payload.clone())
                .await
                .is_ok()
        );
        assert!(
            converter
                .from_payload::<RecordPayload<T>>(&context, payload)
                .await
                .is_err()
        );

        for bad in [
            Value::Null,
            json!(true),
            json!(1),
            json!("record"),
            json!([]),
        ] {
            let payload = converter.to_payload(&context, &bad).await.unwrap();
            assert!(
                converter
                    .from_payload::<RecordPayload<T>>(&context, payload)
                    .await
                    .is_err()
            );
        }

        for field in fields {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(*field);
            let payload = converter.to_payload(&context, &missing).await.unwrap();
            let previous = converter
                .from_payload::<T>(&context, payload.clone())
                .await
                .is_ok();
            assert_eq!(
                converter
                    .from_payload::<RecordPayload<T>>(&context, payload)
                    .await
                    .is_ok(),
                previous,
                "missing {field}"
            );
        }
        let mut extra = value.clone();
        extra["unrecognized"] = json!(true);
        let payload = converter.to_payload(&context, &extra).await.unwrap();
        let previous = converter
            .from_payload::<T>(&context, payload.clone())
            .await
            .is_ok();
        assert_eq!(
            converter
                .from_payload::<RecordPayload<T>>(&context, payload)
                .await
                .is_ok(),
            previous
        );

        let key = fields[0];
        let raw = format!(
            "{{{}:{},{}",
            json!(key),
            value[key],
            &value.to_string()[1..]
        );
        let mut payload = before;
        payload.data = raw.into_bytes();
        // The pinned SDK's generic reader normalizes these duplicate keys away.
        assert!(
            converter
                .from_payload::<T>(&context, payload.clone())
                .await
                .is_ok()
        );
        assert!(
            converter
                .from_payload::<RecordPayload<T>>(&context, payload)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn temporal_objects_keep_seven_record_encodings_and_reject_positional_payloads() {
    let id = Uuid::new_v4();
    record::<StripeCursor>(
        json!({"kind":"purchase","after":[null,null,null],"tails":[false,false,false]}),
        &["kind", "after", "tails"],
    )
    .await;
    record::<StripeWork>(json!({"kind":"event","after":null}), &["kind", "after"]).await;
    record::<StripeBatch>(
        json!({"cursor":id,"completed":1,"tail":false}),
        &["cursor", "completed", "tail"],
    )
    .await;
    record::<DeletionBatch>(
        json!({"attempted":1,"acknowledged":1,"nextCursor":id,"more":true,"retryRequired":false}),
        &[
            "attempted",
            "acknowledged",
            "nextCursor",
            "more",
            "retryRequired",
        ],
    )
    .await;
    record::<MaintenanceBatch>(
        json!({"processed":1,"settled":1,"nextCursor":null,"more":false}),
        &["processed", "settled", "nextCursor", "more"],
    )
    .await;
    // Existing price-drain history uses these snake_case field names.
    record::<PriceDrainBatch>(
        json!({"manifest_id":id,"more":false}),
        &["manifest_id", "more"],
    )
    .await;
    record::<ProbeInput>(
        json!({"slot":"2026-11-02","externalPrintingId":id}),
        &["slot", "externalPrintingId"],
    )
    .await;
}

#[tokio::test]
async fn temporal_objects_memo_decodes_legacy_records_and_rejects_array_sources() {
    let converter = DataConverter::default();
    let context = SerializationContextData::Workflow(WorkflowSerializationContext::new());
    let input = ProbeInput {
        slot: NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
        external_printing_id: Uuid::new_v4(),
    };
    let before = converter.to_payload(&context, &input).await.unwrap();
    let mut values = MemoValues::new();
    values.insert("source", RecordPayload(input));
    let produced = converter
        .to_payload(&context, values.get("source").unwrap())
        .await
        .unwrap();
    assert_eq!(before.metadata, produced.metadata);
    assert_eq!(before.data, produced.data);
    for payload in [before, produced] {
        let memo = Memo::from_raw(
            Some(RawMemo {
                fields: HashMap::from([("source".into(), payload)]),
            }),
            converter.payload_converter().clone(),
            context.clone(),
        );
        assert_eq!(
            memo.get::<RecordPayload<ProbeInput>>("source")
                .unwrap()
                .unwrap()
                .0,
            input
        );
        assert!(
            memo.get::<RecordPayload<ProbeInput>>("absent")
                .unwrap()
                .is_none()
        );
    }
    let array = json!([input.slot, input.external_printing_id]);
    let payload = converter.to_payload(&context, &array).await.unwrap();
    let memo = Memo::from_raw(
        Some(RawMemo {
            fields: HashMap::from([("source".into(), payload)]),
        }),
        converter.payload_converter().clone(),
        context,
    );
    assert!(memo.get::<RecordPayload<ProbeInput>>("source").is_err());

    let mut duplicate = converter
        .to_payload(&SerializationContextData::None, &input)
        .await
        .unwrap();
    duplicate.data = format!(
        r#"{{"slot":"2026-11-02","slot":"2026-11-02","externalPrintingId":"{}"}}"#,
        input.external_printing_id
    )
    .into_bytes();
    let memo = Memo::from_raw(
        Some(RawMemo {
            fields: HashMap::from([("source".into(), duplicate)]),
        }),
        converter.payload_converter().clone(),
        SerializationContextData::None,
    );
    assert!(memo.get::<RecordPayload<ProbeInput>>("source").is_err());
}

#[tokio::test]
async fn temporal_objects_reject_bad_encoding_malformed_json_and_extra_arguments() {
    let converter = DataConverter::default();
    let context = SerializationContextData::None;
    let input = ProbeInput {
        slot: NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
        external_printing_id: Uuid::new_v4(),
    };
    let original = converter
        .to_payload(&context, &RecordPayload(input))
        .await
        .unwrap();
    for raw in [
        b"{".to_vec(),
        [original.data.as_slice(), b" trailing"].concat(),
        [original.data.as_slice(), original.data.as_slice()].concat(),
    ] {
        let mut payload = original.clone();
        payload.data = raw;
        assert!(
            converter
                .from_payload::<RecordPayload<ProbeInput>>(&context, payload)
                .await
                .is_err()
        );
    }
    for encoding in [
        None,
        Some(b"binary/plain".to_vec()),
        Some(b"json/plain ".to_vec()),
    ] {
        let mut payload = original.clone();
        if let Some(encoding) = encoding {
            payload.metadata.insert("encoding".into(), encoding);
        } else {
            payload.metadata.remove("encoding");
        }
        assert!(
            converter
                .from_payload::<RecordPayload<ProbeInput>>(&context, payload)
                .await
                .is_err()
        );
    }
    assert_eq!(
        converter
            .from_payloads::<RecordPayload<ProbeInput>>(&context, vec![original.clone()])
            .await
            .unwrap()
            .0,
        input
    );
    for payloads in [vec![], vec![original.clone(), original]] {
        assert!(
            converter
                .from_payloads::<RecordPayload<ProbeInput>>(&context, payloads)
                .await
                .is_err()
        );
    }
}

async fn scalar<T: Serialize + DeserializeOwned + 'static>(value: T) {
    let converter = DataConverter::default();
    let context = SerializationContextData::None;
    let payload = converter.to_payload(&context, &value).await.unwrap();
    let decoded: T = converter.from_payload(&context, payload).await.unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::to_value(value).unwrap()
    );
}

#[tokio::test]
async fn temporal_objects_preserve_real_scalar_nullable_unit_and_fixed_lists() {
    let id = Uuid::new_v4();
    scalar(()).await;
    scalar(NaiveDate::from_ymd_opt(2026, 11, 2).unwrap()).await;
    scalar(id).await;
    scalar(Some(id)).await;
    scalar(None::<Uuid>).await;
    scalar([Some(id), None, None]).await;
    scalar([true, false, true]).await;
}

#[tokio::test]
async fn stripe_history_literal_bytes() {
    use crate::stripe_workflow::StripeWorkKind;
    assert!(matches!(
        StripeCursor::default().kind,
        StripeWorkKind::Purchase
    ));
    let converter = DataConverter::default();
    for (kind, name) in [
        (StripeWorkKind::Purchase, "purchase"),
        (StripeWorkKind::Event, "event"),
        (StripeWorkKind::Evidence, "evidence"),
    ] {
        // Fixed wire literals, retained before String decoder production edits.
        let cursor_bytes =
            format!(r#"{{"kind":"{name}","after":[null,null,null],"tails":[false,false,false]}}"#)
                .into_bytes();
        let work_bytes = format!(r#"{{"kind":"{name}","after":null}}"#).into_bytes();
        for context in [
            SerializationContextData::Workflow(WorkflowSerializationContext::new()),
            SerializationContextData::Activity(ActivitySerializationContext::new()),
        ] {
            let cursor = StripeCursor {
                kind,
                after: [None; 3],
                tails: [false; 3],
            };
            let work = StripeWork { kind, after: None };
            let mut cursor_payload = converter
                .to_payload(&context, &RecordPayload(cursor))
                .await
                .unwrap();
            let mut work_payload = converter
                .to_payload(&context, &RecordPayload(work))
                .await
                .unwrap();
            assert_eq!(cursor_payload.data, cursor_bytes);
            assert_eq!(work_payload.data, work_bytes);
            assert_eq!(
                cursor_payload.metadata.get("encoding"),
                Some(&b"json/plain".to_vec())
            );
            assert_eq!(
                work_payload.metadata.get("encoding"),
                Some(&b"json/plain".to_vec())
            );
            cursor_payload.data = cursor_bytes.clone();
            work_payload.data = work_bytes.clone();
            let decoded = converter
                .from_payload::<RecordPayload<StripeCursor>>(&context, cursor_payload)
                .await
                .unwrap();
            assert_eq!(serde_json::to_vec(&decoded.0).unwrap(), cursor_bytes);
            let decoded = converter
                .from_payload::<RecordPayload<StripeWork>>(&context, work_payload)
                .await
                .unwrap();
            assert_eq!(serde_json::to_vec(&decoded.0).unwrap(), work_bytes);
        }
    }
}

#[tokio::test]
async fn stripe_history_rejects_nested_nonstring_kinds() {
    let converter = DataConverter::default();
    for context in [
        SerializationContextData::Workflow(WorkflowSerializationContext::new()),
        SerializationContextData::Activity(ActivitySerializationContext::new()),
    ] {
        for token in ["purchase", "event", "evidence"] {
            for bad in [
                json!({token:null}),
                json!({token:{}}),
                json!([token]),
                json!(null),
                json!("unknown"),
            ] {
                let cursor =
                    json!({"kind":bad,"after":[null,null,null],"tails":[false,false,false]});
                let work = json!({"kind":bad,"after":null});
                let payload = converter.to_payload(&context, &cursor).await.unwrap();
                assert!(
                    converter
                        .from_payload::<RecordPayload<StripeCursor>>(&context, payload)
                        .await
                        .is_err()
                );
                let payload = converter.to_payload(&context, &work).await.unwrap();
                assert!(
                    converter
                        .from_payload::<RecordPayload<StripeWork>>(&context, payload)
                        .await
                        .is_err()
                );
            }
        }
        // Rust Default is not permission to accept a missing required JSON field.
        let payload = converter
            .to_payload(
                &context,
                &json!({"after":[null,null,null],"tails":[false,false,false]}),
            )
            .await
            .unwrap();
        assert!(
            converter
                .from_payload::<RecordPayload<StripeCursor>>(&context, payload)
                .await
                .is_err()
        );
        let payload = converter
            .to_payload(&context, &json!({"after":null}))
            .await
            .unwrap();
        assert!(
            converter
                .from_payload::<RecordPayload<StripeWork>>(&context, payload)
                .await
                .is_err()
        );
    }
}
