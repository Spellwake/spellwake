use super::*;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BaselineProduct {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    recipe_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    product_id: Uuid,
    eligible_at: String,
    #[serde(rename = "evidenceRef")]
    _evidence_ref: EvidenceHash,
    modifier_n: Amount,
    modifier_d: Amount,
    #[serde(rename = "modifierRef")]
    _modifier_ref: EvidenceHash,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Baseline {
    version: String,
    forecast: Amount,
    cutoff: String,
    #[serde(rename = "configurationRef")]
    _configuration_ref: EvidenceHash,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    products: Vec<BaselineProduct>,
}
///120 original complete baseline manifest. Clock/recipe qualification stays in SQL.
pub struct BaselineManifest {
    value: Baseline,
    raw: Box<RawValue>,
}
impl BaselineManifest {
    /// Parse the complete reviewed product list, keeping its original JSON identity.
    /// # Errors
    /// Rejects shape, digest, timestamp, quantity, duplicate identity or budget violations.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SourceResponseError> {
        let (crate::provider_json::Object(value), raw) =
            original_source_terms::<crate::provider_json::Object<Baseline>>(bytes, 1_048_576)?;
        let cutoff =
            parse_utc_microsecond(&value.cutoff).ok_or_else(SourceResponseError::context)?;
        let valid = value.version == "launch-baseline-v1"
            && bounded(&value.forecast, 30, false)
            && (1..=10000).contains(&value.products.len())
            && unique(value.products.iter().map(|x| x.recipe_id))
            && unique(value.products.iter().map(|x| x.product_id))
            && value.products.iter().all(|x| {
                bounded(&x.modifier_n, 30, true)
                    && bounded(&x.modifier_d, 30, true)
                    && parse_utc_microsecond(&x.eligible_at).is_some_and(|t| t <= cutoff)
            });
        checked(valid, Self { value, raw })
    }
    /// Original source only, never reconstructed with reordered/defaulted fields.
    pub fn original_json(&self) -> &str {
        self.raw.get()
    }
    /// Original PostgreSQL JSONB text size for the supported string-only source grammar.
    pub fn original_terms_octets(&self) -> Option<usize> {
        crate::source_protocol::original_source_octets(&self.raw)
    }
    /// Check the historical receipt against the exact original product mapping.
    /// # Errors
    /// Rejects incomplete, reordered, duplicated or incorrectly sized allocations.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<BaselineAck, SourceResponseError> {
        let ack: BaselineAck = accepted_response(bytes)?;
        let count = num_bigint::BigUint::from(self.value.products.len());
        let computed = (self.value.forecast.integer() * 120u32 + &count - 1u32) / &count;
        let expected = Amount::from_biguint(computed.max(20u32.into()))
            .map_err(|_| SourceResponseError::context())?;
        let mut products: Vec<_> = self.value.products.iter().collect();
        products.sort_by_key(|x| x.product_id);
        let valid = ack.quantity_per_product == expected
            && ack.windows.len() == products.len()
            && unique(ack.windows.iter().map(|x| x.window_id))
            && ack.windows.iter().zip(products).all(|(window, product)| {
                window.product_id == product.product_id
                    && window.recipe_id == product.recipe_id
                    && window.quantity == expected
            });
        checked(valid, ack)
    }
}
#[derive(Deserialize, PartialEq, Eq)]
#[serde(try_from = "String")]
enum Qualification {
    Exact,
    Unavailable,
    Bounded,
}
impl TryFrom<String> for Qualification {
    type Error = SourceResponseError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "exact" => Ok(Self::Exact),
            "unavailable" => Ok(Self::Unavailable),
            "bounded" => Ok(Self::Bounded),
            _ => Err(SourceResponseError::context()),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Physical {
    version: String,
    #[serde(deserialize_with = "crate::identifiers::uuid", rename = "recipeId")]
    _recipe_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid", rename = "productId")]
    _product_id: Uuid,
    run_ref: String,
    language: String,
    territory: String,
    unit: String,
    qualification: Qualification,
    #[serde(deserialize_with = "crate::provider_json::optional_string")]
    physical_quantity: Option<String>,
    #[serde(deserialize_with = "crate::provider_json::optional_string")]
    evidence_ref: Option<String>,
    modifier_n: Amount,
    modifier_d: Amount,
    #[serde(rename = "modifierRef")]
    _modifier_ref: EvidenceHash,
    normal_quantity: Amount,
}
///120 physical observation. An unfamiliar unit is preserved as unavailable stock.
pub struct PhysicalIntroduction {
    raw: Box<RawValue>,
    eligible_quantity: Option<Amount>,
}
impl PhysicalIntroduction {
    /// Parse exact evidence conditionally; nonexact opaque string/null observations stay valid.
    /// # Errors
    /// Rejects malformed/missing fields, identifiers, exact quantities or source bounds.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SourceResponseError> {
        let (crate::provider_json::Object(value), raw) =
            original_source_terms::<crate::provider_json::Object<Physical>>(bytes, 65_536)?;
        let quantity = if value.qualification == Qualification::Exact {
            let q: Amount = value
                .physical_quantity
                .as_deref()
                .ok_or_else(SourceResponseError::context)?
                .parse()
                .map_err(|_| SourceResponseError::context())?;
            checked(bounded(&q, 30, false), ())?;
            EvidenceHash::try_from(
                value
                    .evidence_ref
                    .clone()
                    .ok_or_else(SourceResponseError::context)?,
            )
            .map_err(|_| SourceResponseError::context())?;
            Some(q)
        } else {
            None
        };
        let valid = value.version == "physical-introduction-v1"
            && (1..=256).contains(&value.run_ref.chars().count())
            && value.run_ref.trim_matches(' ') == value.run_ref
            && (2..=3).contains(&value.language.len())
            && value.language.bytes().all(|b| b.is_ascii_lowercase())
            && (value.territory == "GLOBAL"
                || value.territory.len() == 2
                    && value.territory.bytes().all(|b| b.is_ascii_uppercase()))
            && bounded(&value.modifier_n, 30, true)
            && bounded(&value.modifier_d, 30, true)
            && bounded(&value.normal_quantity, 30, true);
        let eligible_quantity = if value.unit == "physical_pack" {
            quantity
                .map(|q| Amount::from_biguint(q.integer() / 100u32))
                .transpose()
                .map_err(|_| SourceResponseError::context())?
        } else {
            None
        };
        checked(
            valid,
            Self {
                raw,
                eligible_quantity,
            },
        )
    }
    /// Unmodified original observation, not evidence acquisition or generated stock.
    pub fn original_json(&self) -> &str {
        self.raw.get()
    }
    /// Original PostgreSQL JSONB text size for the supported string-only source grammar.
    pub fn original_terms_octets(&self) -> Option<usize> {
        crate::source_protocol::original_source_octets(&self.raw)
    }
    /// Bind the complete disposition/quantity/window tuple to the original observation.
    /// Supply an original retained coverage when known, never a freshly invented hash.
    /// # Errors
    /// Rejects bad digest, foreign coverage or contradictory stock/window tuples.
    pub fn decode_response(
        &self,
        bytes: &[u8],
        original_coverage: Option<&str>,
    ) -> Result<PhysicalAck, SourceResponseError> {
        let ack: PhysicalAck = accepted_response(bytes)?;
        let valid = valid_digest(&ack.coverage)
            && original_coverage.is_none_or(|x| x == ack.coverage)
            && match &self.eligible_quantity {
                None => {
                    ack.disposition == PhysicalDisposition::Unavailable
                        && ack.quantity.is_zero()
                        && ack.window_id.is_none()
                }
                Some(q) if q.is_zero() => {
                    ack.disposition == PhysicalDisposition::Zero
                        && ack.quantity.is_zero()
                        && ack.window_id.is_none()
                }
                Some(q) => {
                    ack.disposition == PhysicalDisposition::Published
                        && ack.quantity == *q
                        && ack.window_id.is_some()
                }
            };
        checked(valid, ack)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WeightedProduct {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    recipe_id: Uuid,
    weight: Amount,
    normal_quantity: Amount,
    modifier_n: Amount,
    modifier_d: Amount,
    #[serde(rename = "modifierRef")]
    _modifier_ref: EvidenceHash,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Candidate {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    set_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    products: Vec<WeightedProduct>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ballot {
    version: String,
    supply_coefficient: Amount,
    #[serde(rename = "sourceRef")]
    _source_ref: EvidenceHash,
    tie_seed: String,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    candidates: Vec<Candidate>,
}
///128 immutable season plus original reviewed ballot, never a public vote payload.
pub struct SupplyBallot {
    season_id: Uuid,
    value: Ballot,
    raw: Box<RawValue>,
}
impl SupplyBallot {
    /// Parse distinct candidate sets and weighted recipe revisions; SQL owns eligibility.
    /// # Errors
    /// Rejects malformed IDs, quantities, hashes, duplicates or oversized original JSONB.
    pub fn from_bytes(season_id: Uuid, bytes: &[u8]) -> Result<Self, SourceResponseError> {
        let (crate::provider_json::Object(value), raw) =
            original_source_terms::<crate::provider_json::Object<Ballot>>(bytes, 1_048_576)?;
        let valid = !season_id.is_nil()
            && value.version == "supply-ballot-v1"
            && bounded(&value.supply_coefficient, 9, true)
            && valid_digest(&value.tie_seed)
            && (5..=10000).contains(&value.candidates.len())
            && unique(value.candidates.iter().map(|x| x.set_id))
            && value.candidates.iter().all(|x| {
                (1..=1000).contains(&x.products.len())
                    && unique(x.products.iter().map(|p| p.recipe_id))
                    && x.products.iter().all(|p| {
                        bounded(&p.weight, 9, true)
                            && bounded(&p.normal_quantity, 30, true)
                            && bounded(&p.modifier_n, 30, true)
                            && bounded(&p.modifier_d, 30, true)
                    })
            });
        checked(
            valid,
            Self {
                season_id,
                value,
                raw,
            },
        )
    }
    /// Immutable trusted season scope, not an arbitrary result target.
    pub fn season_id(&self) -> Uuid {
        self.season_id
    }
    /// Original manifest with no generated default or observed clock fields added.
    pub fn original_json(&self) -> &str {
        self.raw.get()
    }
    /// Original PostgreSQL JSONB text size for the supported string-only source grammar.
    pub fn original_terms_octets(&self) -> Option<usize> {
        crate::source_protocol::original_source_octets(&self.raw)
    }
    /// Decode historical opening, optionally checking an independently known cutoff.
    /// # Errors
    /// Rejects foreign season, count, timestamp or original cutoff mismatch.
    pub fn decode_response(
        &self,
        bytes: &[u8],
        original_cutoff: Option<&str>,
    ) -> Result<BallotAck, SourceResponseError> {
        let ack: BallotAck = accepted_response(bytes)?;
        let valid = ack.resource_id == self.season_id
            && ack.candidates == Amount::from(self.value.candidates.len() as u64)
            && parse_utc_microsecond(&ack.cutoff).is_some()
            && original_cutoff.is_none_or(|x| x == ack.cutoff);
        checked(valid, ack)
    }
}
