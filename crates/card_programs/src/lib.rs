//! Identity- and semantic-guarded bindings into the restricted native executor.
//! Authored programs are partial implementations, not inferred from names, general
//! Oracle parsing, behavioral evidence or whole-card certification.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spellwake_content::{CatalogFace, Inventory, InventoryCard};
use spellwake_engine::{
    card_identity::{CardDestination, CardId},
    effects::{Instruction, Program, SeedCard},
    mana::{ManaCost, ManaType},
    priority::SeatId,
};
use std::collections::{BTreeMap, BTreeSet};

pub const BOLT: &str = "4457ed35-7c10-48c8-9776-456485fdf070";
pub const UNSUMMON: &str = "837182db-1bf3-4a2c-bd01-1af9d9873561";
pub const COUNTERSPELL: &str = "cc187110-1148-4090-bbb8-e205694a39f5";
pub const TWINCAST: &str = "8f878efc-850f-43d2-a6fe-5ea8d1dd5afb";
pub const LION: &str = "e3c0b11e-7340-4e0a-98b7-91b38439d4f9";

/// Canonical UUID spelling for catalogue rules identities, distinct from a game
/// CardId or a printing's Scryfall identity. Invalid external spellings never bind.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OracleId(String);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidOracleId;
impl std::fmt::Display for InvalidOracleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("expected canonical lowercase Oracle UUID")
    }
}
impl std::error::Error for InvalidOracleId {}
impl TryFrom<String> for OracleId {
    type Error = InvalidOracleId;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 36
            || !value.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                }
            })
        {
            return Err(InvalidOracleId);
        }
        Ok(Self(value))
    }
}
impl From<OracleId> for String {
    fn from(value: OracleId) -> Self {
        value.0
    }
}
impl OracleId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgramKey {
    BoltCreatureV1,
    UnsummonCreatureV1,
    LionBattlefieldV1,
    CounterspellSupportedInstantV2,
    TwincastSingleTargetInstantV1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    CreatureTargetOnly,
    SupportedInstantSpellTargetOnly,
    SeededVanillaBattlefieldOnly,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum BindingStatus {
    Missing,
    Blocked {
        reason: &'static str,
    },
    Partial {
        program: ProgramKey,
        scope: Scope,
        semantic_sha256: String,
    },
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardCoverage {
    pub name: String,
    pub oracle_id: Option<String>,
    pub required: bool,
    pub binding: BindingStatus,
    pub behavior_evidence: &'static str,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub source_sha256: String,
    pub source_records: usize,
    pub required_records: usize,
    pub required_partial_bindings: usize,
    pub required_without_binding: usize,
    pub fully_qualified_cards: usize,
    pub behavior_evidence: &'static str,
    pub registry_version: u32,
    pub cards: Vec<CardCoverage>,
}
/// Private compiled instruction and guarded source identity. The caller cannot
/// alter a resolved binding through this registry.
#[derive(Debug, Clone)]
pub struct BoundProgram {
    key: ProgramKey,
    scope: Scope,
    instruction: Program,
}
impl BoundProgram {
    pub fn key(&self) -> ProgramKey {
        self.key
    }
    pub fn scope(&self) -> Scope {
        self.scope
    }
}
pub struct Registry {
    bindings: BTreeMap<OracleId, BoundProgram>,
    coverage: Coverage,
}
#[derive(Debug)]
pub enum RegistryError {
    Encoding(serde_json::Error),
    MissingBinding,
    UnsupportedPlacement,
}
impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encoding(_) => f.write_str("program signature encoding failed"),
            Self::UnsupportedPlacement => {
                f.write_str("program scope does not support this initial placement")
            }
            Self::MissingBinding => {
                f.write_str("no guarded program binding for this Oracle identity")
            }
        }
    }
}
impl std::error::Error for RegistryError {}

#[derive(Serialize)]
struct Signature<'a> {
    version: u32,
    oracle_id: &'a str,
    name: &'a str,
    layout: &'a str,
    faces: &'a [CatalogFace],
}
fn signature(card: &InventoryCard, id: &OracleId) -> Result<String, RegistryError> {
    let bytes = serde_json::to_vec(&Signature {
        version: 1,
        oracle_id: id.as_str(),
        name: &card.name,
        layout: &card.layout,
        faces: &card.faces,
    })
    .map_err(RegistryError::Encoding)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
struct Definition {
    key: ProgramKey,
    scope: Scope,
    name: &'static str,
    mana: &'static str,
    type_line: &'static str,
    text: &'static str,
    power: Option<&'static str>,
    toughness: Option<&'static str>,
    program: Program,
}
fn definition(id: &OracleId) -> Option<Definition> {
    let mut def = Definition {
        key: ProgramKey::BoltCreatureV1,
        scope: Scope::CreatureTargetOnly,
        name: "Lightning Bolt",
        mana: "{R}",
        type_line: "Instant",
        text: "Lightning Bolt deals 3 damage to any target.",
        power: None,
        toughness: None,
        program: Program::Instant {
            cost: ManaCost::one(ManaType::Red),
            instructions: vec![Instruction::Damage { amount: 3.into() }],
        },
    };
    match id.as_str() {
        BOLT => {}
        UNSUMMON => {
            def.key = ProgramKey::UnsummonCreatureV1;
            def.name = "Unsummon";
            def.mana = "{U}";
            def.text = "Return target creature to its owner's hand.";
            def.program = Program::Instant {
                cost: ManaCost::one(ManaType::Blue),
                instructions: vec![Instruction::ReturnToOwnersHand],
            };
        }
        COUNTERSPELL => {
            def.key = ProgramKey::CounterspellSupportedInstantV2;
            def.scope = Scope::SupportedInstantSpellTargetOnly;
            def.name = "Counterspell";
            def.mana = "{U}{U}";
            def.text = "Counter target spell.";
            def.program = Program::Instant {
                cost: ManaCost::specific(ManaType::Blue, 2.into()),
                instructions: vec![Instruction::CounterSpell],
            };
        }
        TWINCAST => {
            def.key = ProgramKey::TwincastSingleTargetInstantV1;
            def.scope = Scope::SupportedInstantSpellTargetOnly;
            def.name = "Twincast";
            def.mana = "{U}{U}";
            def.text =
                "Copy target instant or sorcery spell. You may choose new targets for the copy.";
            def.program = Program::Instant {
                cost: ManaCost::specific(ManaType::Blue, 2.into()),
                instructions: vec![Instruction::CopySpell],
            };
        }
        LION => {
            def.key = ProgramKey::LionBattlefieldV1;
            def.scope = Scope::SeededVanillaBattlefieldOnly;
            def.name = "Silvercoat Lion";
            def.mana = "{1}{W}";
            def.type_line = "Creature — Cat";
            def.text = "";
            def.power = Some("2");
            def.toughness = Some("2");
            def.program = Program::VanillaCreature {
                power: 2.into(),
                toughness: 2.into(),
            };
        }
        _ => return None,
    }
    Some(def)
}
fn matches(card: &InventoryCard, def: &Definition) -> bool {
    let [face] = card.faces.as_slice() else {
        return false;
    };
    card.name == def.name
        && card.layout == "normal"
        && face.name == def.name
        && face.type_line.as_deref() == Some(def.type_line)
        && face.mana_cost.as_deref() == Some(def.mana)
        && face.oracle_text.as_deref() == Some(def.text)
        && face.power.as_deref() == def.power
        && face.toughness.as_deref() == def.toughness
}
impl Registry {
    /// Bind only exact known identities whose full materialized supported signature
    /// matches. Changed text/stats/layout, missing identities and duplicates fail closed.
    pub fn from_inventory(inventory: &Inventory) -> Result<Self, RegistryError> {
        let mut seen = BTreeSet::new();
        let mut duplicate = BTreeSet::new();
        for card in &inventory.cards {
            if let Some(id) = &card.oracle_id
                && !seen.insert(id.clone())
            {
                duplicate.insert(id.clone());
            }
        }
        let mut bindings = BTreeMap::new();
        let mut cards = Vec::new();
        let mut partial = 0;
        let mut required = 0;
        for card in &inventory.cards {
            required += usize::from(card.required_by_base_formats);
            let binding = match card
                .oracle_id
                .clone()
                .and_then(|id| OracleId::try_from(id).ok())
            {
                None => BindingStatus::Blocked {
                    reason: "missing-or-invalid-oracle-id",
                },
                Some(id) if duplicate.contains(id.as_str()) => BindingStatus::Blocked {
                    reason: "ambiguous-oracle-id",
                },
                Some(id) => match definition(&id) {
                    None => BindingStatus::Missing,
                    Some(def) if !matches(card, &def) => BindingStatus::Blocked {
                        reason: "semantic-guard-mismatch",
                    },
                    Some(def) => {
                        let semantic_sha256 = signature(card, &id)?;
                        if card.required_by_base_formats {
                            partial += 1;
                        }
                        bindings.insert(
                            id,
                            BoundProgram {
                                key: def.key,
                                scope: def.scope,
                                instruction: def.program,
                            },
                        );
                        BindingStatus::Partial {
                            program: def.key,
                            scope: def.scope,
                            semantic_sha256,
                        }
                    }
                },
            };
            cards.push(CardCoverage {
                name: card.name.clone(),
                oracle_id: card.oracle_id.clone(),
                required: card.required_by_base_formats,
                binding,
                behavior_evidence: "not-evaluated-by-registry",
            });
        }
        Ok(Self {
            bindings,
            coverage: Coverage {
                source_sha256: inventory.compressed_sha256.clone(),
                source_records: inventory.cards.len(),
                required_records: required,
                required_partial_bindings: partial,
                required_without_binding: required - partial,
                fully_qualified_cards: 0,
                behavior_evidence: "not-evaluated-by-registry",
                registry_version: 3,
                cards,
            },
        })
    }
    pub fn coverage(&self) -> &Coverage {
        &self.coverage
    }
    pub fn resolve(&self, id: &OracleId) -> Option<&BoundProgram> {
        self.bindings.get(id)
    }
    /// Create an ordinary seed only through the guarded catalogue binding. This
    /// grants no collection ownership, public action authority or full card support.
    pub fn seed(
        &self,
        oracle: &OracleId,
        card: CardId,
        owner: SeatId,
        destination: CardDestination,
    ) -> Result<SeedCard, RegistryError> {
        let binding = self.resolve(oracle).ok_or(RegistryError::MissingBinding)?;
        if binding.scope == Scope::SeededVanillaBattlefieldOnly
            && !matches!(destination, CardDestination::Battlefield { .. })
        {
            return Err(RegistryError::UnsupportedPlacement);
        }
        Ok(SeedCard {
            card,
            owner,
            destination,
            program: binding.instruction.clone(),
        })
    }
}
