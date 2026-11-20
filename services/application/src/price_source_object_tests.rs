//! Retained index decoding; no live source request is made.
use super::*;
use serde_json::json;

fn selected() -> Value {
    json!({"object":"bulk_data","id":ALL_CARDS_ID,"type":"all_cards","updated_at":"2026-11-01T11:00:00Z","jsonl_download_uri":"https://data.scryfall.io/all-cards/all-cards-20261101110000.jsonl.gz","compressed_size":100})
}

#[test]
fn source_objects_index_keeps_unrelated_products_and_raw_selected_fields() {
    let item = selected();
    let raw = format!(
        r#"{{"object":"list","has_more":false,"data":[null,[],17,{{"type":17}},{{"type":"oracle_cards","unrelated":true}},{item}],"extension":true}}"#
    );
    let actual = parse_index(raw.as_bytes()).unwrap();
    assert_eq!(actual.id.to_string(), ALL_CARDS_ID);
    assert_eq!(actual.compressed_size, 100);
    let selected = item.to_string();
    for field in [
        "object",
        "id",
        "type",
        "updated_at",
        "jsonl_download_uri",
        "compressed_size",
    ] {
        let key = json!(field).to_string();
        let duplicate =
            selected.replace(&format!("{key}:"), &format!("{key}:{},{key}:", item[field]));
        let raw = format!(r#"{{"object":"list","has_more":false,"data":[{duplicate}]}}"#);
        assert_eq!(
            parse_index(raw.as_bytes()).unwrap_err(),
            SourceError::InvalidIndex,
            "{field}"
        );
    }
}

#[test]
fn source_objects_index_envelope_and_selection_cannot_be_ambiguous() {
    let item = selected();
    let index = json!({"object":"list","has_more":false,"data":[item]});
    let array = crate::test_http::positional(&index, &["object", "has_more", "data"]);
    assert!(parse_index(array.to_string().as_bytes()).is_err());
    for field in ["object", "has_more", "data"] {
        let key = json!(field).to_string();
        let duplicate = format!(
            r#"{{{key}:{},"object":"list","has_more":false,"data":[{item}]}}"#,
            index[field]
        );
        assert_eq!(
            parse_index(duplicate.as_bytes()).unwrap_err(),
            SourceError::InvalidIndex
        );
    }
    let array = crate::test_http::positional(
        &item,
        &[
            "object",
            "id",
            "type",
            "updated_at",
            "jsonl_download_uri",
            "compressed_size",
        ],
    );
    for data in [json!([item, item]), json!([array]), json!([])] {
        assert!(
            parse_index(
                json!({"object":"list","has_more":false,"data":data})
                    .to_string()
                    .as_bytes()
            )
            .is_err()
        );
    }
    // A duplicated discriminator is ambiguous even on an unrelated product.
    let raw = format!(
        r#"{{"object":"list","has_more":false,"data":[{{"type":"oracle_cards","type":"other"}},{item}]}}"#
    );
    assert!(parse_index(raw.as_bytes()).is_err());
}
