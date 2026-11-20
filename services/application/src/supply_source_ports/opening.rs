use super::*;
fn present_string<'de, D: Deserializer<'de>>(input: D) -> Result<Option<String>, D::Error> {
    String::deserialize(input).map(Some)
}
fn present_objects<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    input: D,
) -> Result<Option<Vec<T>>, D::Error> {
    crate::provider_json::objects(input).map(Some)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Card {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    slot_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    printing_id: Uuid,
    quantity: Amount,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Outcome {
    weight: Amount,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    cards: Vec<Card>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Group {
    collation_ref: String,
    #[serde(default, deserialize_with = "present_string")]
    kind: Option<String>,
    #[serde(default, deserialize_with = "present_objects")]
    outcomes: Option<Vec<Outcome>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Plan {
    version: String,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    groups: Vec<Group>,
}
///046 original authored opening plan. Accepted grammar is not collation evidence.
pub struct BoosterOpening {
    raw: Box<RawValue>,
}
impl BoosterOpening {
    /// Parse bounded joint/multiset syntax, retaining omitted fields and exact source JSON.
    /// # Errors
    /// Rejects malformed, duplicated, incompatible, unknown or over-budget plan terms.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SourceResponseError> {
        let (crate::provider_json::Object(plan), raw) =
            original_source_terms::<crate::provider_json::Object<Plan>>(bytes, 1_048_576)?;
        let mut outcomes = 0usize;
        let mut lines = 0usize;
        let valid = plan.version == "pack-opening-v1"
            && plan.groups.len() <= 64
            && unique(plan.groups.iter().map(|g| &g.collation_ref))
            && plan.groups.iter().all(|g| {
                (1..=1024).contains(&g.collation_ref.chars().count())
                    && match g.kind.as_deref().unwrap_or("joint") {
                        "multiset-without-replacement" => g.outcomes.is_none(),
                        "joint" => g.outcomes.as_ref().is_some_and(|xs| {
                            outcomes += xs.len();
                            (1..=10000).contains(&xs.len())
                                && xs.iter().all(|x| {
                                    lines += x.cards.len();
                                    bounded(&x.weight, 78, true)
                                        && (1..=10000).contains(&x.cards.len())
                                        && unique(
                                            x.cards.iter().map(|c| (c.slot_id, c.printing_id)),
                                        )
                                        && x.cards.iter().all(|c| bounded(&c.quantity, 4, true))
                                })
                        }),
                        _ => false,
                    }
            })
            && outcomes <= 10000
            && lines <= 10000;
        checked(valid, Self { raw })
    }
    /// Original plan only. Its hash must come from PostgreSQL JSONB text, never compact JSON.
    pub fn original_json(&self) -> &str {
        self.raw.get()
    }
    /// Original PostgreSQL JSONB text size for the supported string-only source grammar.
    pub fn original_terms_octets(&self) -> Option<usize> {
        crate::source_protocol::original_source_octets(&self.raw)
    }
}
///024/029 qualification input. Only authored SQL checks can qualify a real recipe.
pub struct QualifyBoosterRecipe {
    recipe_id: Uuid,
    manifest: EvidenceHash,
}
impl QualifyBoosterRecipe {
    /// Bind a canonical recipe to its retained manifest; result is SQL VOID, not a receipt.
    /// # Errors
    /// Rejects nil recipe IDs. EvidenceHash already enforces the exact digest grammar.
    pub fn new(recipe_id: Uuid, manifest: EvidenceHash) -> Result<Self, SourceResponseError> {
        checked(
            !recipe_id.is_nil(),
            Self {
                recipe_id,
                manifest,
            },
        )
    }
    /// Original canonical SQL target.
    pub fn recipe_id(&self) -> Uuid {
        self.recipe_id
    }
    /// Retained evidence identity, not evidence of actual qualification.
    pub fn manifest(&self) -> &EvidenceHash {
        &self.manifest
    }
}
///046 qualification input; actual invocation returns VOID through the shared204 transport.
pub struct QualifyBoosterOpening {
    recipe_id: Uuid,
    plan: BoosterOpening,
    hash: EvidenceHash,
}
impl QualifyBoosterOpening {
    /// Bind the original plan and independently retained PostgreSQL JSONB hash.
    /// # Errors
    /// Rejects nil recipe IDs. SQL checks hash equality, lineage and authored marginals.
    pub fn new(
        recipe_id: Uuid,
        plan: BoosterOpening,
        hash: EvidenceHash,
    ) -> Result<Self, SourceResponseError> {
        checked(
            !recipe_id.is_nil(),
            Self {
                recipe_id,
                plan,
                hash,
            },
        )
    }
    /// Canonical recipe target, preserved across an exact retry.
    pub fn recipe_id(&self) -> Uuid {
        self.recipe_id
    }
    /// Original plan, retaining omitted kind versus explicitly written joint.
    pub fn plan(&self) -> &BoosterOpening {
        &self.plan
    }
    /// Hash supplied by the trusted JSONB evidence path, never recomputed here.
    pub fn hash(&self) -> &EvidenceHash {
        &self.hash
    }
}
