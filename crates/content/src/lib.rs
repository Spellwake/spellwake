//! Dated Oracle-catalogue inventory, not card compilation or engine qualification.
//! Additional provider fields are deliberately ignored; required identity/name/layout
//! and the six format statuses are validated/normalized rather than inferred legal.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::BufRead,
};

pub mod snapshot;

pub const FORMATS: [&str; 6] = [
    "commander",
    "standard",
    "pioneer",
    "modern",
    "legacy",
    "vintage",
];
#[derive(Debug, Deserialize, Serialize)]
pub struct BulkMetadata {
    pub object: String,
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub updated_at: String,
    pub jsonl_download_uri: String,
    pub compressed_size: u64,
}
#[derive(Debug, Deserialize)]
struct RawFace {
    name: String,
    #[serde(default)]
    type_line: Option<String>,
    #[serde(default)]
    oracle_text: Option<String>,
    #[serde(default)]
    mana_cost: Option<String>,
    #[serde(default)]
    power: Option<String>,
    #[serde(default)]
    toughness: Option<String>,
}
#[derive(Debug, Deserialize)]
struct RawCard {
    object: String,
    id: String,
    #[serde(default)]
    oracle_id: Option<String>,
    name: String,
    layout: String,
    #[serde(default)]
    type_line: Option<String>,
    #[serde(default)]
    oracle_text: Option<String>,
    #[serde(default)]
    mana_cost: Option<String>,
    #[serde(default)]
    power: Option<String>,
    #[serde(default)]
    toughness: Option<String>,
    #[serde(default)]
    card_faces: Option<Vec<RawFace>>,
    legalities: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "value", rename_all = "kebab-case")]
pub enum Legality {
    Legal,
    Restricted,
    Banned,
    NotLegal,
    Unknown(String),
    Missing,
}
impl Legality {
    fn read(value: Option<&String>) -> Self {
        match value.map(String::as_str) {
            Some("legal") => Self::Legal,
            Some("restricted") => Self::Restricted,
            Some("banned") => Self::Banned,
            Some("not_legal") => Self::NotLegal,
            Some(value) => Self::Unknown(value.into()),
            None => Self::Missing,
        }
    }
    fn eligible(&self) -> bool {
        matches!(self, Self::Legal | Self::Restricted)
    }
}
#[derive(Debug, Serialize)]
pub struct CatalogFace {
    pub name: String,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub mana_cost: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryCard {
    pub scryfall_id: String,
    pub oracle_id: Option<String>,
    pub name: String,
    pub layout: String,
    pub faces: Vec<CatalogFace>,
    pub legalities: BTreeMap<String, Legality>,
    pub eligible_formats: Vec<String>,
    pub required_by_base_formats: bool,
    pub program_evidence: &'static str,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub source: BulkMetadata,
    pub compressed_sha256: String,
    pub source_records: usize,
    pub unique_oracle_ids: usize,
    pub missing_oracle_ids: usize,
    pub duplicate_oracle_ids: Vec<String>,
    pub unknown_legality_records: usize,
    pub missing_required_oracle_ids: usize,
    pub base_format_inventory_resolved: bool,
    pub eligible_by_format: BTreeMap<String, usize>,
    pub required_records: usize,
    pub required_faces: usize,
    pub required_without_program_evidence: usize,
    pub qualified_by_this_report: usize,
    pub executable_registry: &'static str,
    pub scope: &'static str,
    pub cards: Vec<InventoryCard>,
}
#[derive(Debug)]
pub enum CatalogError {
    Invalid(String),
    Io(std::io::Error),
}
impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::Io(error) => std::fmt::Display::fmt(error, f),
        }
    }
}
impl std::error::Error for CatalogError {}

pub fn inventory(
    reader: impl BufRead,
    source: BulkMetadata,
    compressed_sha256: String,
) -> Result<Inventory, CatalogError> {
    if source.object != "bulk_data"
        || source.kind != "oracle_cards"
        || source.id.is_empty()
        || source.updated_at.is_empty()
        || !source
            .jsonl_download_uri
            .starts_with("https://data.scryfall.io/oracle-cards/")
        || source.compressed_size == 0
    {
        return Err(CatalogError::Invalid(
            "invalid Oracle snapshot metadata".into(),
        ));
    }
    if compressed_sha256.len() != 64
        || !compressed_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(CatalogError::Invalid("invalid snapshot SHA-256".into()));
    }
    let mut cards = Vec::new();
    let mut record_ids = BTreeSet::new();
    let mut oracle_ids = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    let mut missing = 0;
    let mut unknown = 0;
    let mut eligible_by_format: BTreeMap<_, _> = FORMATS
        .into_iter()
        .map(|format| (format.to_owned(), 0))
        .collect();
    let mut required_records = 0;
    let mut missing_required_oracle_ids = 0;
    let mut required_faces = 0;
    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(CatalogError::Io)?;
        let raw: RawCard = serde_json::from_str(&line).map_err(|error| {
            CatalogError::Invalid(format!("line {}: invalid card: {error}", index + 1))
        })?;
        if raw.object != "card"
            || raw.id.trim().is_empty()
            || raw.name.trim().is_empty()
            || raw.layout.trim().is_empty()
        {
            return Err(CatalogError::Invalid(format!(
                "line {}: missing card identity/name/layout",
                index + 1
            )));
        }
        if !record_ids.insert(raw.id.clone()) {
            return Err(CatalogError::Invalid(format!(
                "line {}: duplicate Scryfall identity",
                index + 1
            )));
        }
        if let Some(id) = &raw.oracle_id {
            if id.trim().is_empty() {
                return Err(CatalogError::Invalid(format!(
                    "line {}: empty Oracle identity",
                    index + 1
                )));
            }
            if !oracle_ids.insert(id.clone()) {
                duplicates.insert(id.clone());
            }
        } else {
            missing += 1;
        }
        let legalities: BTreeMap<_, _> = FORMATS
            .into_iter()
            .map(|format| {
                (
                    format.to_owned(),
                    Legality::read(raw.legalities.get(format)),
                )
            })
            .collect();
        if legalities
            .values()
            .any(|status| matches!(status, Legality::Missing | Legality::Unknown(_)))
        {
            unknown += 1;
        }
        let eligible_formats: Vec<_> = legalities
            .iter()
            .filter(|(_, status)| status.eligible())
            .map(|(format, _)| format.clone())
            .collect();
        for format in &eligible_formats {
            *eligible_by_format
                .get_mut(format)
                .expect("known format key") += 1;
        }
        let faces = match raw.card_faces {
            Some(faces) if !faces.is_empty() => faces
                .into_iter()
                .map(|face| CatalogFace {
                    name: face.name,
                    type_line: face.type_line,
                    oracle_text: face.oracle_text,
                    mana_cost: face.mana_cost,
                    power: face.power,
                    toughness: face.toughness,
                })
                .collect(),
            _ => vec![CatalogFace {
                name: raw.name.clone(),
                type_line: raw.type_line,
                oracle_text: raw.oracle_text,
                mana_cost: raw.mana_cost,
                power: raw.power,
                toughness: raw.toughness,
            }],
        };
        if faces.iter().any(|face| face.name.trim().is_empty()) {
            return Err(CatalogError::Invalid(format!(
                "line {}: empty face name",
                index + 1
            )));
        }
        let required_by_base_formats = !eligible_formats.is_empty();
        if required_by_base_formats {
            if raw.oracle_id.is_none() {
                missing_required_oracle_ids += 1;
            }
            required_records += 1;
            required_faces += faces.len();
        }
        cards.push(InventoryCard {
            scryfall_id: raw.id,
            oracle_id: raw.oracle_id,
            name: raw.name,
            layout: raw.layout,
            faces,
            legalities,
            eligible_formats,
            required_by_base_formats,
            program_evidence: "missing",
        });
    }
    if cards.is_empty() {
        return Err(CatalogError::Invalid("empty Oracle snapshot".into()));
    }
    Ok(Inventory {
        source,
        compressed_sha256,
        source_records: cards.len(),
        unique_oracle_ids: oracle_ids.len(),
        missing_oracle_ids: missing,
        duplicate_oracle_ids: duplicates.iter().cloned().collect(),
        unknown_legality_records: unknown,
        missing_required_oracle_ids,
        base_format_inventory_resolved: unknown == 0
            && duplicates.is_empty()
            && missing_required_oracle_ids == 0,
        eligible_by_format,
        required_records,
        required_faces,
        required_without_program_evidence: required_records,
        qualified_by_this_report: 0,
        executable_registry: "not-connected",
        scope: "Dated catalogue inventory only; Planechase pool and Treachery role qualification are separate. No executable or behavior evidence is consumed.",
        cards,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn metadata() -> BulkMetadata {
        BulkMetadata {
            object: "bulk_data".into(),
            id: "fixture".into(),
            kind: "oracle_cards".into(),
            updated_at: "fixture-date".into(),
            jsonl_download_uri: "https://data.scryfall.io/oracle-cards/fixture.jsonl.gz".into(),
            compressed_size: 1,
        }
    }
    fn raw(id: &str) -> serde_json::Value {
        serde_json::json!({"object":"card","id":id,"oracle_id":id,"name":"Fixture","layout":"normal","type_line":"Land","oracle_text":"","legalities":{"commander":"legal","standard":"banned","pioneer":"not_legal","modern":"not_legal","legacy":"legal","vintage":"restricted"}})
    }
    fn run(input: String) -> Result<Inventory, CatalogError> {
        inventory(Cursor::new(input), metadata(), "a".repeat(64))
    }
    #[test]
    fn separates_inventory_from_program_and_qualification_evidence() {
        let report = run(raw("1").to_string()).unwrap();
        assert_eq!(report.required_records, 1);
        assert_eq!(report.required_faces, 1);
        assert_eq!(report.required_without_program_evidence, 1);
        assert_eq!(report.qualified_by_this_report, 0);
        assert_eq!(report.eligible_by_format["vintage"], 1);
        assert_eq!(report.eligible_by_format["standard"], 0);
    }
    #[test]
    fn retains_all_faces_and_flags_unknown_or_missing_statuses() {
        let mut card = raw("1");
        card["legalities"]["standard"] = serde_json::json!("unrecognized");
        card["legalities"].as_object_mut().unwrap().remove("modern");
        card["card_faces"] = serde_json::json!([{"name":"Front","oracle_text":"front","power":"1","toughness":"2"},{"name":"Back","oracle_text":"back","power":"3","toughness":"4"}]);
        let report = run(card.to_string()).unwrap();
        assert_eq!(report.required_faces, 2);
        assert_eq!(report.cards[0].faces[0].power.as_deref(), Some("1"));
        assert_eq!(report.cards[0].faces[1].toughness.as_deref(), Some("4"));
        assert_eq!(report.unknown_legality_records, 1);
        assert_eq!(report.cards[0].legalities["modern"], Legality::Missing);
        assert_eq!(
            report.cards[0].legalities["standard"],
            Legality::Unknown("unrecognized".into())
        );
    }
    #[test]
    fn retains_outside_scope_and_unresolved_identities_without_calling_them_supported() {
        let mut first = raw("1");
        first["oracle_id"] = serde_json::Value::Null;
        for status in first["legalities"].as_object_mut().unwrap().values_mut() {
            *status = serde_json::json!("not_legal");
        }
        let second = raw("2");
        let mut third = raw("3");
        third["oracle_id"] = serde_json::json!("2");
        let report = run(format!("{first}\n{second}\n{third}")).unwrap();
        assert_eq!(report.source_records, 3);
        assert_eq!(report.required_records, 2);
        assert_eq!(report.missing_oracle_ids, 1);
        assert_eq!(report.duplicate_oracle_ids, vec!["2"]);
        assert!(!report.cards[0].required_by_base_formats);
    }
    #[test]
    fn rejects_empty_malformed_and_duplicate_records() {
        assert!(run("".into()).is_err());
        assert!(run("{}".into()).is_err());
        let card = raw("1");
        assert!(run(format!("{card}\n{card}")).is_err());
    }
}
