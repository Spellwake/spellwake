//! Shared exact three-input natural-pool intake; no synthetic or subset fallback.
use sha2::{Digest, Sha256};
use spellwake_card_programs::TEACHING_SOURCE_SHA256;
use spellwake_content::Inventory;
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_reference_tools::{creature_vocabulary, packet};
use std::path::PathBuf;

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn read_input(name: &str) -> Vec<u8> {
    let path = PathBuf::from(std::env::var_os(name).expect("explicit genuine input path required"));
    std::fs::read(path).unwrap()
}
pub fn inputs() -> (Inventory, CreatureTypeVocabulary, Vec<u8>) {
    let metadata = read_input("SPELLWAKE_CATALOGUE_METADATA");
    let bytes = read_input("SPELLWAKE_CATALOGUE_INPUT");
    let vocabulary = read_input("SPELLWAKE_CREATURE_VOCABULARY_INPUT");
    assert_eq!(
        digest(&metadata),
        "77a7ffc2250306197da78c0c4faf35fb4fae01b4dbdd50bf8eb45f502e4737ad"
    );
    assert_eq!(digest(&bytes), TEACHING_SOURCE_SHA256);
    assert_eq!(
        digest(&vocabulary),
        "dd16c29df30b69a72ea9021f0e60429a8537914e9e85187d36559e42ce59cd3c"
    );
    let vocabulary =
        creature_vocabulary::vocabulary(packet::unique_json(&vocabulary).unwrap()).unwrap();
    let inventory = spellwake_content::snapshot::decode(&metadata, bytes.clone()).unwrap();
    (inventory, vocabulary, bytes)
}
