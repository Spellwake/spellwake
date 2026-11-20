//! Explicit synthetic test domain. It makes no complete-source vocabulary claim.
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use std::sync::Arc;
pub fn for_rules(rules: &str) -> Arc<CreatureTypeVocabulary> {
    Arc::new(
        CreatureTypeVocabulary::new(
            rules.into(),
            [0; 32],
            vec![
                "Cat".into(),
                "Elf".into(),
                "Goblin".into(),
                "Human".into(),
                "Monk".into(),
                "Shaman".into(),
                "Time Lord".into(),
                "Wall".into(),
            ],
        )
        .unwrap(),
    )
}
