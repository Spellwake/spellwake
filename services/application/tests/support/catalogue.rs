//! Shared synthetic retained-source fixture, never upstream qualification evidence.
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::PathBuf};
use uuid::Uuid;

pub struct Fixture {
    pub root: PathBuf,
    pub qualification: PathBuf,
    pub artifact: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
pub fn gzip(records: &[Value]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    for record in records {
        writeln!(encoder, "{record}").unwrap();
    }
    encoder.finish().unwrap()
}
pub fn card(oracle: Uuid, set: Uuid) -> Value {
    json!({"object":"card","id":Uuid::new_v4(),"oracle_id":oracle,"name":"A literal ★ card","set_id":set,"set":"tst","set_name":"Test Set","collector_number":"123a★/b","lang":"en","rarity":"rare","finishes":["foil","nonfoil"],"layout":"normal","prices":{"usd":null}})
}
fn hash(bytes: &[u8]) -> String {
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}
pub fn fixture(records: &[Value]) -> Fixture {
    let root = std::env::temp_dir().join(format!("spellwake-catalogue-test-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let artifact = root.join("all-cards.jsonl.gz");
    let compressed = gzip(records);
    fs::write(&artifact, &compressed).unwrap();
    let metadata = json!({"object":"bulk_data","id":"922288cb-4bef-45e1-bb30-0c2bd3d3534f","type":"all_cards","updated_at":"2026-11-01T10:22:43.931Z","jsonl_download_uri":"https://data.scryfall.io/all-cards/all-cards-20261101102243.jsonl.gz","compressed_size":compressed.len()});
    let index = root.join("bulk-index.json");
    let index_bytes = json!({"object":"list","has_more":false,"data":[metadata]}).to_string();
    fs::write(&index, &index_bytes).unwrap();
    let qualification = root.join("qualification.json");
    let evidence = json!({"metadata":metadata,"acquiredAt":"2026-11-01T12:01:29.765119Z","artifactPath":artifact,"metadataPath":index,"hash":hash(&compressed),"metadataHash":hash(index_bytes.as_bytes()),"qualificationPath":qualification,"compressedBytes":compressed.len(),"decompressedBytes":records.iter().map(|record|record.to_string().len()+1).sum::<usize>(),"recordCount":records.len()});
    fs::write(&qualification, evidence.to_string()).unwrap();
    Fixture {
        root,
        qualification,
        artifact,
    }
}
