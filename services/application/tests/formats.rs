//! Integration tests for application-layer preview-format legality isolation.

use spellwake_application::formats::{
    FormatClass, FormatLegalityError, ISOLATED_PREVIEW_FORMATS, Legality, ORDINARY_FORMATS,
    evaluate_card_legality, is_competitive_eligible_format,
};
use std::collections::BTreeMap;

fn map(entries: &[(&str, Legality)]) -> BTreeMap<String, Legality> {
    entries
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

#[test]
fn preview_format_future_is_isolated_from_ordinary_formats_and_competitive_queues() {
    assert_eq!(ORDINARY_FORMATS.len(), 6);
    assert_eq!(ISOLATED_PREVIEW_FORMATS, ["future"]);

    let ordinary = map(&[
        ("standard", Legality::NotLegal),
        ("modern", Legality::Legal),
        ("commander", Legality::Legal),
    ]);
    let preview = map(&[("future", Legality::Legal)]);

    let standard = evaluate_card_legality("standard", &ordinary, &preview).unwrap();
    assert_eq!(standard.format_class, FormatClass::Ordinary);
    assert!(!standard.allowed);
    assert!(!standard.competitive_queue_allowed);

    let modern = evaluate_card_legality("modern", &ordinary, &preview).unwrap();
    assert!(modern.allowed);
    assert!(modern.competitive_queue_allowed);

    let future = evaluate_card_legality("future", &ordinary, &preview).unwrap();
    assert_eq!(future.format_class, FormatClass::Preview);
    assert!(future.allowed);
    assert!(!future.competitive_queue_allowed);
    assert!(!is_competitive_eligible_format("future"));
}

#[test]
fn rejects_invented_future_overlays_and_cross_scope_contamination() {
    let ordinary = map(&[("standard", Legality::Legal)]);
    let preview = map(&[("future", Legality::Legal)]);

    assert_eq!(
        evaluate_card_legality("futuremodern", &ordinary, &preview),
        Err(FormatLegalityError::UnknownFormat("futuremodern".into()))
    );
    assert_eq!(
        evaluate_card_legality("futurelegacy", &ordinary, &preview),
        Err(FormatLegalityError::UnknownFormat("futurelegacy".into()))
    );

    let contaminated = map(&[("standard", Legality::Legal), ("future", Legality::Legal)]);
    assert_eq!(
        evaluate_card_legality("standard", &contaminated, &preview),
        Err(FormatLegalityError::ScopeContamination {
            scope: FormatClass::Ordinary,
            foreign_key: "future".into(),
        })
    );
}
