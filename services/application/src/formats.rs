//! Application-layer format classification and preview-format legality isolation.
//!
//! Reuses `spellwake_content`'s canonical ordinary (`FORMATS`) vs preview (`PREVIEW_FORMATS`)
//! split so preview eligibility (`future` / Future Standard) can never leak into ordinary
//! constructed/commander legality or competitive queues, and invented overlays like
//! `futuremodern` are rejected rather than treated as legal.

use serde::{Deserialize, Serialize};
use spellwake_content::{FORMATS, PREVIEW_FORMATS};
pub use spellwake_content::{FormatClass, Legality, classify_format};
use std::{collections::BTreeMap, error::Error, fmt};

/// Canonical ordinary base format keys (`commander`, `standard`, `pioneer`, `modern`, `legacy`, `vintage`).
pub const ORDINARY_FORMATS: [&str; 6] = FORMATS;

/// Canonical isolated preview format keys (`future` = Future Standard only).
pub const ISOLATED_PREVIEW_FORMATS: [&str; 1] = PREVIEW_FORMATS;

/// Typed rejection when evaluating a card's format legality across ordinary vs preview scopes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatLegalityError {
    /// Format key is neither a supported ordinary base format nor a supported preview format.
    UnknownFormat(String),
    /// A preview legality map accidentally contained an ordinary format key or vice versa.
    ScopeContamination {
        scope: FormatClass,
        foreign_key: String,
    },
}

impl fmt::Display for FormatLegalityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFormat(key) => write!(f, "unsupported format key: {key}"),
            Self::ScopeContamination { scope, foreign_key } => {
                write!(
                    f,
                    "format scope {scope:?} contains foreign key {foreign_key}"
                )
            }
        }
    }
}

impl Error for FormatLegalityError {}

/// Evaluated legality decision for a single card in one explicitly classified format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatLegalityDecision {
    pub format_key: String,
    pub format_class: FormatClass,
    pub status: Legality,
    pub allowed: bool,
    pub competitive_queue_allowed: bool,
}

/// Returns `true` only for ordinary base formats; preview formats cannot enter competitive queues.
#[must_use]
pub fn is_competitive_eligible_format(format_key: &str) -> bool {
    classify_format(format_key) == Some(FormatClass::Ordinary)
}

/// Evaluate whether a card is legal in `requested_format`, strictly isolating ordinary
/// legalities from preview (`future`) legalities.
///
/// # Errors
/// Returns [`FormatLegalityError::UnknownFormat`] if `requested_format` is unmapped (including
/// invented keys such as `futuremodern` or `futurelegacy`), or
/// [`FormatLegalityError::ScopeContamination`] if either legality map carries keys from the
/// opposite format class.
pub fn evaluate_card_legality(
    requested_format: &str,
    ordinary_legalities: &BTreeMap<String, Legality>,
    preview_legalities: &BTreeMap<String, Legality>,
) -> Result<FormatLegalityDecision, FormatLegalityError> {
    for key in ordinary_legalities.keys() {
        if classify_format(key) == Some(FormatClass::Preview) {
            return Err(FormatLegalityError::ScopeContamination {
                scope: FormatClass::Ordinary,
                foreign_key: key.clone(),
            });
        }
    }
    for key in preview_legalities.keys() {
        if classify_format(key) == Some(FormatClass::Ordinary) {
            return Err(FormatLegalityError::ScopeContamination {
                scope: FormatClass::Preview,
                foreign_key: key.clone(),
            });
        }
    }

    let format_class = classify_format(requested_format)
        .ok_or_else(|| FormatLegalityError::UnknownFormat(requested_format.to_owned()))?;

    let status = match format_class {
        FormatClass::Ordinary => ordinary_legalities
            .get(requested_format)
            .cloned()
            .unwrap_or(Legality::Missing),
        FormatClass::Preview => preview_legalities
            .get(requested_format)
            .cloned()
            .unwrap_or(Legality::Missing),
    };

    let allowed = status.is_eligible();
    let competitive_queue_allowed = allowed && format_class == FormatClass::Ordinary;

    Ok(FormatLegalityDecision {
        format_key: requested_format.to_owned(),
        format_class,
        status,
        allowed,
        competitive_queue_allowed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordinary_map(entries: &[(&str, Legality)]) -> BTreeMap<String, Legality> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn preview_future_legality_never_leaks_into_ordinary_standard_or_competitive_queues() {
        let ordinary = ordinary_map(&[
            ("standard", Legality::NotLegal),
            ("modern", Legality::Legal),
            ("commander", Legality::Legal),
        ]);
        let preview = ordinary_map(&[("future", Legality::Legal)]);

        // Preview-set reprint: legal in Future Standard, NOT legal in ordinary Standard.
        let std_decision = evaluate_card_legality("standard", &ordinary, &preview).unwrap();
        assert_eq!(std_decision.format_class, FormatClass::Ordinary);
        assert_eq!(std_decision.status, Legality::NotLegal);
        assert!(!std_decision.allowed);
        assert!(!std_decision.competitive_queue_allowed);

        // Existing ordinary Modern legality is preserved while a new printing is previewed.
        let mod_decision = evaluate_card_legality("modern", &ordinary, &preview).unwrap();
        assert!(mod_decision.allowed);
        assert!(mod_decision.competitive_queue_allowed);

        // Preview format allows preview play only, never competitive queues.
        let future_decision = evaluate_card_legality("future", &ordinary, &preview).unwrap();
        assert_eq!(future_decision.format_class, FormatClass::Preview);
        assert!(future_decision.allowed);
        assert!(!future_decision.competitive_queue_allowed);
        assert!(!is_competitive_eligible_format("future"));
        assert!(is_competitive_eligible_format("standard"));
    }

    #[test]
    fn rejects_invented_future_overlays_and_cross_scope_contamination() {
        let ordinary = ordinary_map(&[("standard", Legality::Legal)]);
        let preview = ordinary_map(&[("future", Legality::Legal)]);

        assert_eq!(
            evaluate_card_legality("futuremodern", &ordinary, &preview),
            Err(FormatLegalityError::UnknownFormat("futuremodern".into()))
        );
        assert_eq!(
            evaluate_card_legality("futurelegacy", &ordinary, &preview),
            Err(FormatLegalityError::UnknownFormat("futurelegacy".into()))
        );

        let contaminated_ordinary =
            ordinary_map(&[("standard", Legality::Legal), ("future", Legality::Legal)]);
        assert_eq!(
            evaluate_card_legality("standard", &contaminated_ordinary, &preview),
            Err(FormatLegalityError::ScopeContamination {
                scope: FormatClass::Ordinary,
                foreign_key: "future".into(),
            })
        );

        let contaminated_preview =
            ordinary_map(&[("future", Legality::Legal), ("modern", Legality::Legal)]);
        assert_eq!(
            evaluate_card_legality("future", &ordinary, &contaminated_preview),
            Err(FormatLegalityError::ScopeContamination {
                scope: FormatClass::Preview,
                foreign_key: "modern".into(),
            })
        );
    }

    #[test]
    fn missing_or_unknown_legality_is_never_silently_allowed() {
        let ordinary = ordinary_map(&[("standard", Legality::Unknown("unmapped".into()))]);
        let preview = BTreeMap::new();

        let unknown = evaluate_card_legality("standard", &ordinary, &preview).unwrap();
        assert!(!unknown.allowed);

        let missing = evaluate_card_legality("pioneer", &ordinary, &preview).unwrap();
        assert_eq!(missing.status, Legality::Missing);
        assert!(!missing.allowed);

        let missing_preview = evaluate_card_legality("future", &ordinary, &preview).unwrap();
        assert_eq!(missing_preview.status, Legality::Missing);
        assert!(!missing_preview.allowed);
    }
}
