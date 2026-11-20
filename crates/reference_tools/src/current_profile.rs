//! Private qualification collection from actual borrowed native inputs and results.
//! Callers execute and retain native operations; this module only observes them.
use super::{packet::AliasContext, projection};
use serde::{Deserialize, Serialize};
#[path = "native_packet_wire.rs"]
mod native_packet_wire;
#[path = "native_role_fault_wire.rs"]
mod native_role_fault_wire;
use serde_json::{Value, json};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    effects::EffectWindow,
    game::{GameContext, GameState},
    object_identity::GameObjects,
    occurrence::RulesJournal,
    priority::SeatId,
    setup::GameSeed,
    stack_window::StackWindow,
    turn::TurnScheduler,
};
use std::collections::{BTreeMap, BTreeSet};

/// Current qualification report epoch; packet, witness and record codecs have
/// independent namespaces. Historical current9 results require retained readers.
pub const REPORT_SCHEMA_VERSION: u32 = 10;
#[path = "current_profile/applicability.rs"]
pub mod applicability;
#[path = "current_profile/caller_delivery.rs"]
mod caller_delivery;
use applicability::{ApplicabilityError, NativeCaptureValue};
pub(crate) use caller_delivery::CallerCompletion;
pub use caller_delivery::CaptureScope;
use caller_delivery::PendingCallerDelivery;

/// One independently retained file binding; the collector never reads that file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}
impl SourceRef {
    fn validate(&self) -> Result<(), String> {
        nonempty(&self.path)?;
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("source sha256 must be 64 lowercase hexadecimal characters".into());
        }
        Ok(())
    }
}

/// Crate-local seal: downstream callers cannot invent a Registry8 authority.
pub(crate) mod registry_witness_sealed {
    pub trait Sealed {}
}
/// Immutable native Registry8 coverage supplied only by a maintained source owner.
/// This does not authenticate synthetic post-intake Inventory bytes.
pub trait NativeOptionalRegistryWitness: registry_witness_sealed::Sealed {
    /// Serialize the complete actual immutable native coverage, including its
    /// per-identity keys/scopes, without another authored card table.
    ///
    /// # Errors
    /// Returns the existing native coverage serializer's error.
    fn native_coverage(&self) -> Result<Value, String>;
    /// Join the same actual owner to the final strict compiled source19 inputs.
    ///
    /// # Errors
    /// Rejects foreign source/vocabulary/identity/signature relationships or
    /// unavailable/pending compiled inputs. Genuine raw-source authentication
    /// remains with the ordinary source loader before native construction.
    fn validate_source(&self, vocabulary_id: &str) -> Result<(), String>;
}

/// Immutable authenticated source9 authority, supplied by the maintained source owner.
/// Kept independent of card_programs because engine test support shares this module.
pub trait NativeSelectionRegistryWitness: registry_witness_sealed::Sealed {
    /// Serialize complete actual native Registry9 coverage.
    ///
    /// # Errors
    /// Returns an existing native coverage serialization failure.
    fn native_coverage(&self) -> Result<Value, String>;
    /// Validate this same immutable source, native Registry9 and vocabulary owner.
    ///
    /// # Errors
    /// Rejects any foreign vocabulary, source identity, printing, signature or scope.
    fn validate_source(&self, vocabulary_id: &str) -> Result<(), String>;
}

pub enum RegistryWitness<'a> {
    Registry(&'a Value),
    /// An immutable Registry8 constructed by the maintained content owner.
    /// The ordinary source owner separately checks its compiled source19 joins.
    /// This witness does not authenticate post-intake Inventory bytes or admit
    /// an ordinary optional-payment envelope.
    OptionalPayment(&'a dyn NativeOptionalRegistryWitness),
    /// Exact immutable source9 authority, admitted only in Selection11.
    ResolutionSelection(&'a dyn NativeSelectionRegistryWitness),
    NotApplicable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Phase {
    InitializeInput,
    BeforeCall,
    AfterCall,
    Observation,
    PureInput,
    PureResult,
}
/// Actual owner references, with no route for a primitive to claim root ownership.
pub enum SnapshotOwner<'a> {
    GameState(&'a GameState),
    EffectWindow(&'a EffectWindow),
    StackWindow(&'a StackWindow),
    GameObjects {
        cards: &'a GameObjects,
        participants: &'a [SeatId],
    },
    TurnScheduler(&'a TurnScheduler),
    InitializeInput {
        context: Option<&'a GameContext>,
        seed: &'a GameSeed,
        source_ref: &'a str,
    },
    ProgramInput {
        program: &'a spellwake_engine::program::Program,
        source_ref: &'a str,
    },
    PureValue(&'a Value),
}
pub enum Append<'a> {
    RulesJournal(&'a RulesJournal),
    NotApplicable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceCapture {
    BeforeInvocation,
    InputOnlyForInitializer,
}
/// Native caller evidence for source retention; full Result equality is checked here.
pub struct ReplayEvidence {
    pub source_eq: Option<bool>,
    pub source_capture: SourceCapture,
}
pub struct CallPoint<'a> {
    pub call_id: &'a str,
    pub before_point: Option<&'a str>,
    pub after_point: Option<&'a str>,
    pub adopted: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservationJoin {
    pub pointer: String,
    pub points: Vec<String>,
}

/// A bounded recorder. Stored JSON is produced from references to actual owners.
pub struct Recorder {
    scope: CaptureScope,
    applicability_failure: Option<ApplicabilityError>,
    caller_delivery: BTreeMap<String, PendingCallerDelivery>,
    vocabulary: Value,
    vocabulary_id: String,
    source_inputs: Vec<SourceRef>,
    registry: Value,
    snapshots: BTreeMap<String, Value>,
    calls: BTreeMap<String, Value>,
    point_order: Vec<String>,
    call_order: Vec<String>,
    coverage: Option<Value>,
    native_value_pointers: BTreeSet<String>,
}
fn nonempty(value: &str) -> Result<(), String> {
    if value.is_empty() {
        Err("empty qualification label".into())
    } else {
        Ok(())
    }
}
fn pointer(value: &str) -> Result<(), String> {
    if !value.starts_with('/') {
        return Err("qualification pointer must be an absolute RFC6901 pointer".into());
    }
    let bytes = value.as_bytes();
    let mut offset = 0;
    while offset < bytes.len() {
        if bytes[offset] == b'~' {
            offset += 1;
            if offset == bytes.len() || !matches!(bytes[offset], b'0' | b'1') {
                return Err("invalid RFC6901 escape".into());
            }
        }
        offset += 1;
    }
    Ok(())
}
fn not_owned(reason: &str) -> Value {
    json!({"tag":"NotOwned","reason":reason})
}
fn owned(value: Value) -> Value {
    json!({"tag":"NativeOwned","value":value})
}
fn supplied(source: &str, value: Value) -> Value {
    json!({"tag":"SuppliedInput","source_ref":source,"value":value})
}
fn applicable(value: Option<Value>) -> Value {
    json!({"tag":"Applicable","value":value})
}
fn programs(value: &EffectWindow) -> Value {
    json!(value.programs().map(|(card,program)|json!({"card":card.0.to_string(),"program":super::program::view(program)})).collect::<Vec<_>>())
}
fn exact_object<'a>(
    value: &'a Value,
    fields: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let object = value.as_object().ok_or("registry DTO must be an object")?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err("registry DTO has missing or unknown fields".into());
    }
    Ok(object)
}
fn registry_hash(value: &Value) -> Result<(), String> {
    let hash = value.as_str().ok_or("registry digest must be a string")?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("registry digest must be canonical lowercase SHA-256".into());
    }
    Ok(())
}
fn mandatory_registry_ids() -> Result<BTreeSet<String>, String> {
    // Reuse the source projection consumed by the mandatory catalogue owner;
    // this observer does not maintain another authored-card or Program table.
    let definitions: Value =
        serde_json::from_str(include_str!("ordinary_game/mandatory-definitions-v1.json"))
            .map_err(|error| format!("mandatory registry identity source: {error}"))?;
    let definitions = definitions
        .as_array()
        .ok_or("mandatory registry identity source must be an array")?;
    let ids = definitions
        .iter()
        .map(|definition| {
            definition["oracleId"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "mandatory registry identity source lacks an Oracle ID".to_owned())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if definitions.len() != 19 || ids.len() != 19 {
        return Err(
            "mandatory registry identity source must select nineteen distinct cards".into(),
        );
    }
    Ok(ids)
}
fn validate_registry(value: &Value, vocabulary_id: &str) -> Result<(), String> {
    validate_registry_with_owner(value, vocabulary_id, None)
}
pub(crate) fn validate_registry_with_owner(
    value: &Value,
    vocabulary_id: &str,
    optional_owner: Option<&dyn NativeOptionalRegistryWitness>,
) -> Result<(), String> {
    exact_object(
        value,
        &[
            "sourceSha256",
            "sourceRecords",
            "requiredRecords",
            "requiredPartialBindings",
            "requiredWithoutBinding",
            "fullyQualifiedCards",
            "behaviorEvidence",
            "registryVersion",
            "creatureVocabularyId",
            "cards",
        ],
    )?;
    registry_hash(&value["sourceSha256"])?;
    let teaching = value["registryVersion"]
        == spellwake_contracts::teaching_profile::TEACHING_REGISTRY_VERSION;
    // This shared observer checks the DTO shape and the mandatory identity set.
    // The registry and strict reference reader bind their actual source semantics.
    let interaction = value["registryVersion"] == 6;
    let mandatory = value["registryVersion"] == 7;
    let optional_payment = value["registryVersion"] == 8;
    if optional_payment && optional_owner.is_none() {
        return Err(
            "Registry8 witness requires its immutable guarded native Registry owner".into(),
        );
    }
    if optional_owner.is_some() && !optional_payment {
        return Err("typed optional-payment witness requires Registry8".into());
    }
    let authored = teaching || interaction || mandatory || optional_payment;
    if (value["registryVersion"] != 4 && !authored)
        || value["creatureVocabularyId"] != vocabulary_id
    {
        return Err(
            "registry version or actual vocabulary identity differs from selected header".into(),
        );
    }
    for field in [
        "sourceRecords",
        "requiredRecords",
        "requiredPartialBindings",
        "requiredWithoutBinding",
        "fullyQualifiedCards",
    ] {
        value[field]
            .as_u64()
            .ok_or("registry count must be an unsigned JSON integer")?;
    }
    nonempty(
        value["behaviorEvidence"]
            .as_str()
            .ok_or("registry behavior evidence must be a string")?,
    )?;
    let cards = value["cards"]
        .as_array()
        .ok_or("registry cards must be an array")?;
    if (teaching || interaction)
        && (value["sourceSha256"] != spellwake_contracts::teaching_profile::TEACHING_SOURCE_SHA256
            || vocabulary_id != spellwake_contracts::teaching_profile::TEACHING_VOCABULARY_ID
            || cards.len() != 20
            || value["requiredRecords"] != 20
            || value["requiredPartialBindings"] != 20
            || value["requiredWithoutBinding"] != 0
            || value["fullyQualifiedCards"] != 0
            || value["sourceRecords"]
                .as_u64()
                .is_none_or(|count| count < 20)
            || (interaction && value["sourceRecords"] != 39196))
    {
        return Err("authored registry differs from its pinned finite admission scope".into());
    }
    if mandatory
        && (value["sourceSha256"] != spellwake_contracts::teaching_profile::TEACHING_SOURCE_SHA256
            || vocabulary_id != spellwake_contracts::teaching_profile::TEACHING_VOCABULARY_ID
            || value["sourceRecords"] != 39196
            || cards.len() != 19
            || value["requiredRecords"] != 19
            || value["requiredPartialBindings"] != 19
            || value["requiredWithoutBinding"] != 0
            || value["fullyQualifiedCards"] != 0)
    {
        return Err("mandatory registry differs from its pinned finite admission scope".into());
    }
    if optional_payment
        && (value["sourceSha256"] != spellwake_contracts::teaching_profile::TEACHING_SOURCE_SHA256
            || vocabulary_id != spellwake_contracts::teaching_profile::TEACHING_VOCABULARY_ID
            || value["sourceRecords"] != 39196
            || cards.len() != 19
            || value["requiredRecords"] != 19
            || value["requiredPartialBindings"] != 19
            || value["requiredWithoutBinding"] != 0
            || value["fullyQualifiedCards"] != 0)
    {
        return Err(
            "optional-payment registry differs from its pinned finite admission scope".into(),
        );
    }
    let mut authored_ids = BTreeSet::new();
    let mut teaching_lands = 0;
    let mut interaction_families = [0usize; 5];
    let mut mandatory_families = [0usize; 6];
    let mut optional_families = [0usize; 11];
    for card in cards {
        exact_object(
            card,
            &[
                "name",
                "oracleId",
                "required",
                "binding",
                "behaviorEvidence",
            ],
        )?;
        nonempty(
            card["name"]
                .as_str()
                .ok_or("registry card name must be a string")?,
        )?;
        if !card["oracleId"].is_null() {
            let id = card["oracleId"]
                .as_str()
                .ok_or("registry Oracle ID must be a string or null")?;
            if id.len() != 36
                || !id.bytes().enumerate().all(|(index, byte)| {
                    if [8, 13, 18, 23].contains(&index) {
                        byte == b'-'
                    } else {
                        byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                    }
                })
            {
                return Err("registry Oracle ID must be canonical".into());
            }
        }
        card["required"]
            .as_bool()
            .ok_or("registry required must be a boolean")?;
        nonempty(
            card["behaviorEvidence"]
                .as_str()
                .ok_or("registry card evidence must be a string")?,
        )?;
        let binding = &card["binding"];
        if authored {
            let id = card["oracleId"]
                .as_str()
                .ok_or("authored registry requires a selected Oracle identity")?;
            if card["required"] != true
                || binding["status"] != "partial"
                || !authored_ids.insert(id)
            {
                return Err(
                    "authored registry requires distinct admitted selected identities".into(),
                );
            }
        }
        match binding["status"].as_str() {
            Some("missing") => {
                exact_object(binding, &["status"])?;
            }
            Some("blocked") => {
                exact_object(binding, &["status", "reason"])?;
                nonempty(
                    binding["reason"]
                        .as_str()
                        .ok_or("registry block reason must be a string")?,
                )?;
            }
            Some("partial") => {
                exact_object(binding, &["status", "program", "scope", "semanticSha256"])?;
                registry_hash(&binding["semanticSha256"])?;
                let selected_variant = if teaching {
                    let land = binding["program"] == "teaching-basic-land-v1";
                    teaching_lands += usize::from(land);
                    (land || binding["program"] == "teaching-vanilla-creature-v1")
                        && binding["scope"] == "teaching-ordinary-card-v1"
                } else if interaction {
                    let family = match binding["program"].as_str() {
                        Some("interaction-basic-land-v1") => Some(0),
                        Some("interaction-vanilla-creature-v1") => Some(1),
                        Some("interaction-bounce-creature-v1") => Some(2),
                        Some("interaction-counter-spell-v1") => Some(3),
                        Some("interaction-draw-two-v1") => Some(4),
                        _ => None,
                    };
                    if let Some(index) = family {
                        interaction_families[index] += 1;
                    }
                    family.is_some() && binding["scope"] == "interaction-ordinary-card-v1"
                } else if mandatory {
                    let family = match binding["program"].as_str() {
                        Some("mandatory-basic-land-v1") => Some(0),
                        Some("mandatory-vanilla-creature-v1") => Some(1),
                        Some("mandatory-triggered-creature-v1") => Some(2),
                        Some("mandatory-token-creation-spell-v1") => Some(3),
                        Some("mandatory-bounce-creature-v1") => Some(4),
                        Some("mandatory-counter-spell-v1") => Some(5),
                        _ => None,
                    };
                    if let Some(index) = family {
                        mandatory_families[index] += 1;
                    }
                    family.is_some() && binding["scope"] == "mandatory-trigger-ordinary-card-v1"
                } else if optional_payment {
                    let family = match binding["program"].as_str() {
                        Some("optional-payment-basic-land-v1") => Some(0),
                        Some("optional-payment-vanilla-creature-v1") => Some(1),
                        Some("optional-payment-triggered-creature-v1") => Some(2),
                        Some("optional-payment-bounce-creature-v1") => Some(3),
                        Some("optional-payment-counter-spell-v1") => Some(4),
                        Some("optional-payment-draw-two-v1") => Some(5),
                        Some("optional-payment-token-creation-spell-v1") => Some(6),
                        Some("optional-payment-unless-counter-spell-v1") => Some(7),
                        Some("optional-payment-copy-spell-v1") => Some(8),
                        Some("optional-payment-mana-artifact-v1") => Some(9),
                        Some("optional-payment-triggered-enchantment-v1") => Some(10),
                        _ => None,
                    };
                    if let Some(index) = family {
                        optional_families[index] += 1;
                    }
                    family.is_some() && binding["scope"] == "optional-payment-ordinary-card-v1"
                } else {
                    [
                        "bolt-creature-v1",
                        "unsummon-creature-v1",
                        "lion-battlefield-v2",
                        "counterspell-supported-instant-v2",
                        "twincast-single-target-instant-v1",
                    ]
                    .iter()
                    .any(|name| binding["program"] == *name)
                        && [
                            "creature-target-only",
                            "supported-instant-spell-target-only",
                            "seeded-vanilla-battlefield-only",
                        ]
                        .iter()
                        .any(|scope| binding["scope"] == *scope)
                };
                if !selected_variant {
                    return Err("registry binding is not a selected current native variant".into());
                }
            }
            _ => return Err("unknown registry binding status".into()),
        }
    }
    if teaching && teaching_lands != 2 {
        return Err("teaching registry requires two basic lands and eighteen creatures".into());
    }
    if interaction && interaction_families != [2, 14, 1, 2, 1] {
        return Err("interaction registry differs from its selected card families".into());
    }
    if mandatory {
        if mandatory_families != [4, 4, 8, 1, 1, 1] {
            return Err("mandatory registry differs from its selected card families".into());
        }
        let selected = mandatory_registry_ids()?;
        if !authored_ids
            .iter()
            .copied()
            .eq(selected.iter().map(String::as_str))
        {
            return Err("mandatory registry differs from its selected Oracle identities".into());
        }
    }
    if optional_payment {
        if optional_families != [3, 2, 6, 1, 1, 1, 1, 1, 1, 1, 1] {
            return Err("optional-payment registry differs from its selected card families".into());
        }
        let registry = optional_owner.ok_or("Registry8 witness lacks its native owner")?;
        let native = registry.native_coverage()?;
        // Counts and source-face signatures do not bind ProgramKey per Oracle ID.
        // Compare the complete immutable owner's actual coverage instead of
        // maintaining another name-to-family table in the observer.
        if &native != value {
            return Err("Registry8 witness differs from its immutable guarded native owner".into());
        }
        // The sealed implementation joins the actual owner to the final strict
        // source inputs; a shape-only or pending-signature witness cannot pass.
        registry.validate_source(vocabulary_id)?;
    }
    Ok(())
}
/// Validate a supplied witness value against its exact authenticated native owner.
/// A raw DTO alone never supplies source9 authority.
///
/// # Errors
/// Rejects a missing owner, invalid source relationship or any changed coverage value.
pub(crate) fn validate_selection_registry_with_owner(
    value: &Value,
    vocabulary_id: &str,
    owner: Option<&dyn NativeSelectionRegistryWitness>,
) -> Result<(), String> {
    let owner =
        owner.ok_or("Registry9 witness requires its immutable authenticated source owner")?;
    owner.validate_source(vocabulary_id)?;
    if value != &owner.native_coverage()? {
        return Err(
            "Registry9 witness differs from its immutable authenticated source owner".into(),
        );
    }
    Ok(())
}

fn registry_witness_value(
    witness: RegistryWitness<'_>,
    vocabulary_id: &str,
) -> Result<Value, String> {
    Ok(match witness {
        RegistryWitness::Registry(value) => {
            validate_registry(value, vocabulary_id)?;
            json!({"tag":"Registry","coverage":value})
        }
        RegistryWitness::OptionalPayment(registry) => {
            let value = registry.native_coverage()?;
            validate_registry_with_owner(&value, vocabulary_id, Some(registry))?;
            json!({"tag":"Registry","coverage":value})
        }
        RegistryWitness::ResolutionSelection(source) => {
            let value = source.native_coverage()?;
            validate_selection_registry_with_owner(&value, vocabulary_id, Some(source))?;
            json!({"tag":"Registry","coverage":value})
        }
        RegistryWitness::NotApplicable => {
            json!({"tag":"NotApplicable","reason":"NoRegistryConstruction"})
        }
    })
}
impl Recorder {
    pub fn new(
        vocabulary: &CreatureTypeVocabulary,
        input: SourceRef,
        publication: SourceRef,
        source_inputs: Vec<SourceRef>,
        registry: RegistryWitness<'_>,
    ) -> Result<Self, String> {
        Self::new_scoped(
            CaptureScope::Current10,
            vocabulary,
            input,
            publication,
            source_inputs,
            registry,
        )
    }
    /// Construct a recorder in an explicit private observation scope.
    ///
    /// # Errors
    /// Preserves the source and native-registry validation of `new`. Selection11
    /// admits only the sealed immutable source9 witness or NotApplicable for
    /// native mechanisms. Envelope/report admission remains a separate boundary.
    pub fn new_scoped(
        scope: CaptureScope,
        vocabulary: &CreatureTypeVocabulary,
        input: SourceRef,
        publication: SourceRef,
        source_inputs: Vec<SourceRef>,
        registry: RegistryWitness<'_>,
    ) -> Result<Self, String> {
        if scope == CaptureScope::ResolutionSelection11
            && !matches!(
                registry,
                RegistryWitness::NotApplicable | RegistryWitness::ResolutionSelection(_)
            )
        {
            return Err("selection11 requires its sealed source9 witness or no registry".into());
        }
        if scope == CaptureScope::Current10
            && matches!(registry, RegistryWitness::ResolutionSelection(_))
        {
            return Err("source9 witness requires the explicit selection11 scope".into());
        }
        input.validate()?;
        publication.validate()?;
        let mut sources = BTreeSet::new();
        for source in &source_inputs {
            source.validate()?;
            if !sources.insert(source.path.clone()) {
                return Err("duplicate source input path".into());
            }
        }
        let registry = registry_witness_value(registry, &vocabulary.id().to_string())?;
        Ok(Self {
            scope,
            applicability_failure: None,
            caller_delivery: BTreeMap::new(),
            vocabulary: json!({"input":input,"publication":publication,"value":projection::vocabulary(vocabulary)}),
            vocabulary_id: vocabulary.id().to_string(),
            source_inputs,
            registry,
            snapshots: BTreeMap::new(),
            calls: BTreeMap::new(),
            point_order: Vec::new(),
            call_order: Vec::new(),
            coverage: None,
            native_value_pointers: BTreeSet::new(),
        })
    }
    fn writable(&self) -> Result<(), String> {
        if let Some(error) = self.applicability_failure {
            return Err(error.to_string());
        }
        if self.coverage.is_some() {
            Err("qualification coverage is already sealed".into())
        } else {
            Ok(())
        }
    }
    /// A denied observer has no authority to emit any later witness.
    pub(crate) fn applicability_rejected(&self) -> bool {
        self.applicability_failure.is_some()
    }
    fn admit_current10(
        &mut self,
        inspect: impl FnOnce() -> Result<(), ApplicabilityError>,
    ) -> Result<(), String> {
        if self.scope == CaptureScope::Current10
            && let Err(error) = inspect()
        {
            self.applicability_failure.get_or_insert(error);
            return Err(error.to_string());
        }
        Ok(())
    }
    /// Observe an actual native owner within the selected qualification epoch.
    ///
    /// # Errors
    /// Rejects invalid labels, phases, pointers and vocabulary relationships.
    /// Current10 rejects retained resolution-selection semantics and permanently
    /// denies this recorder's later coverage/witness publication.
    pub fn snapshot(
        &mut self,
        point_id: &str,
        observation_pointers: &[String],
        phase: Phase,
        owner_source: &str,
        owner: SnapshotOwner<'_>,
        aliases: &AliasContext,
    ) -> Result<(), String> {
        self.writable()?;
        self.admit_current10(|| applicability::snapshot(&owner))?;
        nonempty(point_id)?;
        nonempty(owner_source)?;
        if self.snapshots.contains_key(point_id) {
            return Err(format!("duplicate snapshot point {point_id}"));
        }
        let mut unique = BTreeSet::new();
        for value in observation_pointers {
            pointer(value)?;
            if !unique.insert(value) {
                return Err("duplicate snapshot observation pointer".into());
            }
        }
        let no_resolution = json!({"tag":"NotApplicable","reason":"OwnerHasNoResolution"});
        let no_parent = json!({"tag":"NotApplicable","reason":"OwnerHasNoDamageWork"});
        let (
            owner_name,
            native_vocabulary,
            native_context,
            program_source,
            value,
            resolution,
            damage_parent,
        ) = match owner {
            SnapshotOwner::GameState(value) => {
                let execution = value.execution();
                (
                    "GameState",
                    owned(json!(execution.creature_vocabulary().id().to_string())),
                    owned(projection::context(&value.basis().context)),
                    owned(programs(execution)),
                    projection::state(value, aliases)?,
                    applicable(
                        execution
                            .resolution_state()
                            .map(projection::resolution_state),
                    ),
                    applicable(execution.damage_parent().map(projection::damage_parent)),
                )
            }
            SnapshotOwner::EffectWindow(value) => (
                "EffectWindow",
                owned(json!(value.creature_vocabulary().id().to_string())),
                not_owned("OwnerHasNoContext"),
                owned(programs(value)),
                projection::effect(value, aliases)?,
                applicable(value.resolution_state().map(projection::resolution_state)),
                applicable(value.damage_parent().map(projection::damage_parent)),
            ),
            SnapshotOwner::StackWindow(value) => (
                "StackWindow",
                not_owned("OwnerHasNoVocabulary"),
                not_owned("OwnerHasNoContext"),
                not_owned("OwnerHasNoPrograms"),
                projection::stack_window(value),
                no_resolution,
                no_parent,
            ),
            SnapshotOwner::GameObjects {
                cards,
                participants,
            } => (
                "GameObjects",
                not_owned("OwnerHasNoVocabulary"),
                not_owned("OwnerHasNoContext"),
                not_owned("OwnerHasNoPrograms"),
                projection::card_objects(cards, participants),
                no_resolution,
                no_parent,
            ),
            SnapshotOwner::TurnScheduler(value) => (
                "TurnScheduler",
                not_owned("OwnerHasNoVocabulary"),
                not_owned("OwnerHasNoContext"),
                not_owned("OwnerHasNoPrograms"),
                projection::turn_scheduler(value),
                no_resolution,
                no_parent,
            ),
            SnapshotOwner::InitializeInput {
                context,
                seed,
                source_ref,
            } => {
                if phase != Phase::InitializeInput {
                    return Err("constructor operands require InitializeInput phase".into());
                }
                nonempty(source_ref)?;
                let seed_value = projection::game_seed_input(seed);
                (
                    "GameSeedInput",
                    supplied(source_ref, json!(seed.creature_vocabulary.id().to_string())),
                    context
                        .map(|value| supplied(source_ref, projection::context(value)))
                        .unwrap_or_else(|| not_owned("NoLiveStateBeforeInitialization")),
                    supplied(source_ref, seed_value["cards"].clone()),
                    json!({"context":context.map(projection::context),"seed":seed_value}),
                    no_resolution,
                    no_parent,
                )
            }
            SnapshotOwner::ProgramInput {
                program,
                source_ref,
            } => {
                nonempty(source_ref)?;
                let value = super::program::view(program);
                (
                    "PureValue",
                    not_owned("OwnerHasNoVocabulary"),
                    not_owned("OwnerHasNoContext"),
                    supplied(source_ref, value.clone()),
                    value,
                    no_resolution,
                    no_parent,
                )
            }
            SnapshotOwner::PureValue(value) => (
                "PureValue",
                not_owned("OwnerHasNoVocabulary"),
                not_owned("OwnerHasNoContext"),
                not_owned("OwnerHasNoPrograms"),
                value.clone(),
                no_resolution,
                no_parent,
            ),
        };
        if native_vocabulary["tag"] == "NativeOwned"
            && native_vocabulary["value"] != self.vocabulary_id
        {
            return Err("snapshot native vocabulary differs from witness header".into());
        }
        let value = json!({"point_id":point_id,"observation_pointers":observation_pointers,"phase":phase,"owner":owner_name,"owner_source":owner_source,"policy_vocabulary":self.vocabulary_id,"native_vocabulary":native_vocabulary,"native_context":native_context,"program_source":program_source,"projection":{"tag":"Inline","value":value},"resolution":resolution,"damage_parent":damage_parent});
        self.snapshots.insert(point_id.to_owned(), value);
        self.point_order.push(point_id.to_owned());
        Ok(())
    }
    /// Observe two Results from the caller's actual native reexecution.
    /// Full native Eq is checked before any projected comparison is accepted.
    ///
    /// # Errors
    /// Rejects invalid source/result/replay/append relationships. Current10 also
    /// rejects resolution-selection operands or retained successful native values,
    /// including unadopted Results, before invoking the result projector. Native
    /// errors remain in their original type; this method never executes a call.
    #[allow(clippy::too_many_arguments)]
    pub fn record_call<R: Eq + NativeCaptureValue, E: Eq>(
        &mut self,
        point: CallPoint<'_>,
        input: &projection::QualificationCall<'_>,
        result: &Result<R, E>,
        replayed: &Result<R, E>,
        project_result: impl Fn(&Result<R, E>) -> Result<Value, String>,
        append: Append<'_>,
        replay: ReplayEvidence,
        aliases: &AliasContext,
        execution: Option<&EffectWindow>,
    ) -> Result<(), String> {
        self.writable()?;
        self.admit_current10(|| {
            applicability::call(input)?;
            if let Some(execution) = execution {
                execution.check_current10()?;
            }
            applicability::append(&append)?;
            if let Ok(value) = result {
                value.check_current10()?;
            }
            if let Ok(value) = replayed {
                value.check_current10()?;
            }
            Ok(())
        })?;
        nonempty(point.call_id)?;
        if self.calls.contains_key(point.call_id) {
            return Err("duplicate qualification call ID".into());
        }
        let initializer = input.is_initializer();
        let before = point
            .before_point
            .map(|id| {
                self.snapshots
                    .get(id)
                    .ok_or_else(|| format!("missing before point {id}"))
            })
            .transpose()?;
        let after = point
            .after_point
            .map(|id| {
                self.snapshots
                    .get(id)
                    .ok_or_else(|| format!("missing after point {id}"))
            })
            .transpose()?;
        if before.is_some_and(|value| value["owner"] != input.owner_name()) {
            return Err("typed call owner differs from its actual source point".into());
        }
        if let Some(after) = after {
            let phase = if input.is_pure() {
                "PureResult"
            } else {
                "AfterCall"
            };
            if after["phase"] != phase {
                return Err("successor point has the wrong phase".into());
            }
            if !initializer && after["owner"] != input.result_owner_name() {
                return Err("successor point has a different owner".into());
            }
        }
        if input.is_pure() {
            let scalar = matches!(input,projection::QualificationCall::PureCall{input,..} if !input.is_object() && !input.is_array());
            if before.is_none() && !scalar {
                return Err("non-scalar pure call requires its exact input snapshot".into());
            }
            if before.is_some_and(|value| value["phase"] != "PureInput")
                || replay.source_eq != Some(true)
                || replay.source_capture != SourceCapture::BeforeInvocation
            {
                return Err("pure call requires retained typed input equality and an input snapshot before invocation".into());
            }
        }
        if initializer {
            if before.is_none_or(|value| value["phase"] != "InitializeInput")
                || replay.source_eq.is_some()
                || replay.source_capture != SourceCapture::InputOnlyForInitializer
            {
                return Err("initializer requires actual supplied input point and input-only retention evidence".into());
            }
        } else if !input.is_pure()
            && (before.is_none_or(|value| value["phase"] != "BeforeCall")
                || replay.source_eq != Some(true)
                || replay.source_capture != SourceCapture::BeforeInvocation)
        {
            return Err("live native call requires a source snapshot before invocation and unchanged full source Eq".into());
        }
        if result.is_err() && (point.adopted || after.is_some()) {
            return Err("native error cannot be adopted or have a successor point".into());
        }
        if point.adopted && after.is_none() {
            return Err("adopted native result requires its actual successor point".into());
        }
        let full_result_eq = result == replayed;
        if !full_result_eq {
            return Err("full native Result differs on reexecution".into());
        }
        let result_projection = project_result(result)?;
        let full_record_eq = result_projection == project_result(replayed)?;
        if !full_record_eq {
            return Err("full projected native record differs on reexecution".into());
        }
        if result.is_ok() && input.returns_rules_append() && matches!(append, Append::NotApplicable)
        {
            return Err("actual native transition requires its returned RulesJournal".into());
        }
        let append = match (result.is_err(), append) {
            (true, Append::RulesJournal(_)) => {
                return Err("native Err has no returned RulesJournal".into());
            }
            (true, Append::NotApplicable) => {
                json!({"tag":"NoReturnedTransition","reason":"NativeError"})
            }
            (false, Append::RulesJournal(value)) => {
                json!({"tag":"RulesJournal","value":projection::journal(value)})
            }
            (false, Append::NotApplicable) => {
                json!({"tag":"NotApplicable","reason":"OwnerHasNoRulesAppend"})
            }
        };
        let call = json!({"call_id":point.call_id,"before_point":point.before_point,"after_point":point.after_point,"owner_method":input.owner_method(),"input":projection::qualification_call(input,aliases,execution)?,"result":result_projection,"adopted":point.adopted,"append":append,"replay":{"source_eq":replay.source_eq,"full_result_eq":full_result_eq,"full_record_eq":full_record_eq,"input_storage":{"tag":"SavedTypedCall","call_id":point.call_id},"source_snapshot_before_invocation":if initializer {json!("InputOnlyForInitializer")}else{json!(replay.source_capture==SourceCapture::BeforeInvocation)}}});
        self.calls.insert(point.call_id.to_owned(), call);
        self.call_order.push(point.call_id.to_owned());
        Ok(())
    }
    /// Bind a native-only cut to its own complete emitted Inline owner value.
    /// This uses the actual insertion index and never a later scoped observation.
    pub fn bind_native_point(&mut self, point_id: &str) -> Result<String, String> {
        self.writable()?;
        let index = self
            .point_order
            .iter()
            .position(|id| id == point_id)
            .ok_or("unknown native-only point")?;
        if self.snapshots[point_id]["projection"]["tag"] != "Inline" {
            return Err("native-only value binding requires Inline projection".into());
        }
        let pointer = format!("/type_context_witness/snapshots/{index}/projection/value");
        self.bind_observation(point_id, &pointer)?;
        if !self.native_value_pointers.insert(pointer.clone()) {
            return Err("duplicate native-only value binding".into());
        }
        Ok(pointer)
    }
    /// Attach a source writer's exact observation pointer to an existing point.
    /// This changes qualification metadata only, never the saved native projection.
    pub fn bind_observation(
        &mut self,
        point_id: &str,
        observation_pointer: &str,
    ) -> Result<(), String> {
        self.writable()?;
        pointer(observation_pointer)?;
        let point = self
            .snapshots
            .get_mut(point_id)
            .ok_or("unknown observation point")?;
        let pointers = point["observation_pointers"]
            .as_array_mut()
            .ok_or("lost observation pointer list")?;
        if pointers.iter().any(|value| value == observation_pointer) {
            return Err("duplicate observation point binding".into());
        }
        pointers.push(json!(observation_pointer));
        Ok(())
    }
    /// Bind an already observed complete projection to the same R21 record field.
    /// The actual target is verified against its saved Inline value at finish time.
    pub fn reference_record_point(
        &mut self,
        point_id: &str,
        record_pointer: &str,
    ) -> Result<(), String> {
        self.writable()?;
        pointer(record_pointer)?;
        let point = self
            .snapshots
            .get_mut(point_id)
            .ok_or("unknown record point")?;
        if point["projection"]["tag"] != "Inline" {
            return Err("point already uses record storage".into());
        }
        point["projection"] = json!({"tag":"RecordPointer","pointer":record_pointer,"saved_value":point["projection"]["value"].take()});
        Ok(())
    }
    pub fn finish_coverage(
        &mut self,
        required_points: Vec<String>,
        required_calls: Vec<String>,
        observation_to_points: Vec<ObservationJoin>,
        record_pointers: Vec<String>,
    ) -> Result<(), String> {
        self.writable()?;
        let exact_set = |values: &[String]| -> Result<BTreeSet<String>, String> {
            let set = values.iter().cloned().collect::<BTreeSet<_>>();
            if set.len() != values.len() || values.iter().any(String::is_empty) {
                return Err("duplicate or empty coverage requirement".into());
            }
            Ok(set)
        };
        if exact_set(&required_points)? != self.snapshots.keys().cloned().collect()
            || exact_set(&required_calls)? != self.calls.keys().cloned().collect()
        {
            return Err("coverage inventory does not match every observed point and call".into());
        }
        let mut observations = BTreeMap::new();
        for join in &observation_to_points {
            pointer(&join.pointer)?;
            if join.points.is_empty()
                || observations
                    .insert(join.pointer.clone(), exact_set(&join.points)?)
                    .is_some()
            {
                return Err("missing/duplicate observation coverage join".into());
            }
            for id in &join.points {
                let point = self
                    .snapshots
                    .get(id)
                    .ok_or("coverage joins unknown point")?;
                if !point["observation_pointers"]
                    .as_array()
                    .ok_or("lost snapshot pointers")?
                    .iter()
                    .any(|value| value == &join.pointer)
                {
                    return Err("coverage observation does not join its snapshot".into());
                }
            }
        }
        for point in self.snapshots.values() {
            if point["observation_pointers"]
                .as_array()
                .is_none_or(Vec::is_empty)
            {
                return Err("native point has no actual emitted value binding".into());
            }
            for value in point["observation_pointers"]
                .as_array()
                .ok_or("lost snapshot pointers")?
            {
                let value = value.as_str().ok_or("invalid snapshot pointer")?;
                if !observations
                    .get(value)
                    .is_some_and(|ids| ids.contains(point["point_id"].as_str().unwrap_or_default()))
                {
                    return Err("snapshot observation is uncovered".into());
                }
            }
        }
        let supplied = exact_set(&record_pointers)?;
        for value in &supplied {
            pointer(value)?;
        }
        let actual = self
            .snapshots
            .values()
            .filter(|point| point["projection"]["tag"] == "RecordPointer")
            .map(|point| {
                point["projection"]["pointer"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect::<BTreeSet<_>>();
        let actual = actual
            .union(&self.native_value_pointers)
            .cloned()
            .collect::<BTreeSet<_>>();
        if actual != supplied {
            return Err("record pointer inventory differs from observed record storage".into());
        }
        self.coverage = Some(
            json!({"required_points":required_points,"required_calls":required_calls,"observation_to_points":observation_to_points,"record_pointers":record_pointers,"uncovered":[],"complete":true}),
        );
        Ok(())
    }
    /// Finish against the emitted record so every pointer resolves to actual bytes.
    pub fn witness(&self, record: &Value) -> Result<Value, String> {
        if let Some(error) = self.applicability_failure {
            return Err(error.to_string());
        }
        let coverage = self
            .coverage
            .as_ref()
            .ok_or("coverage has not been completed")?;
        let mut snapshots = Vec::new();
        for id in &self.point_order {
            let mut point = self.snapshots[id].clone();
            if point["projection"]["tag"] == "RecordPointer" {
                let target = point["projection"]["pointer"]
                    .as_str()
                    .ok_or("lost record pointer")?;
                if record.pointer(target) != Some(&point["projection"]["saved_value"]) {
                    return Err(format!(
                        "record projection differs from observed native point {id}"
                    ));
                }
                point["projection"]
                    .as_object_mut()
                    .ok_or("lost projection storage")?
                    .remove("saved_value");
            }
            snapshots.push(point);
        }
        let mut witness = json!({"schema":self.scope.witness_schema(),"vocabulary":self.vocabulary,"source_inputs":self.source_inputs,"registry":self.registry,"snapshots":snapshots,"calls":self.call_order.iter().map(|id|&self.calls[id]).collect::<Vec<_>>(),"coverage":coverage});
        if self.scope == CaptureScope::ResolutionSelection11 {
            witness["caller_delivery"] = self.caller_delivery_value()?;
        }
        let mut emitted = record.clone();
        emitted["type_context_witness"] = witness.clone();
        if self.scope == CaptureScope::ResolutionSelection11 {
            self.validate_caller_record_pointers(&emitted)?;
        }
        for (index, point) in witness["snapshots"]
            .as_array()
            .ok_or("lost snapshot array")?
            .iter()
            .enumerate()
        {
            for raw in point["observation_pointers"]
                .as_array()
                .ok_or("lost observation pointers")?
            {
                let raw = raw.as_str().ok_or("invalid observation pointer")?;
                let observed = emitted
                    .pointer(raw)
                    .ok_or_else(|| format!("missing actual observation pointer {raw}"))?;
                if raw.starts_with("/type_context_witness/") {
                    let expected =
                        format!("/type_context_witness/snapshots/{index}/projection/value");
                    if raw != expected
                        || point["projection"]["tag"] != "Inline"
                        || observed != &point["projection"]["value"]
                    {
                        return Err("native-only pointer does not bind its exact complete Inline owner value".into());
                    }
                }
            }
        }
        Ok(witness)
    }
}
/// The selected qualification profiles' exact four-field current envelope.
///
/// # Errors
/// Rejects an unlisted profile, a mismatched selected Registry7/8 witness,
/// non-object observations, incomplete coverage or invalid observation pointers.
/// The native-only optional profile also requires its finite packet contract.
/// That structural admission does not certify the complete native payload semantics.
pub fn envelope(profile: &str, observations: Value, recorder: &Recorder) -> Result<Value, String> {
    if recorder.scope != CaptureScope::Current10 {
        return Err("selection11 ordinary/native envelope admission is not implemented".into());
    }
    const PROFILES: [&str; 26] = [
        "original",
        "copies",
        "pt",
        "prevention",
        "multipliers",
        "programs",
        "payment",
        "mana-abilities",
        "mana-color",
        "activated",
        "scheduler",
        "library",
        "opening",
        "land",
        "permanent",
        "setup",
        "declarations",
        "shared-damage",
        "cleanup",
        "ordinary",
        "teaching",
        "interaction",
        "mandatory-trigger",
        "optional-payment",
        "optional-payment-native",
        "optional-payment-native-role-fault",
    ];
    if !PROFILES.contains(&profile) {
        return Err("unknown current qualification envelope profile".into());
    }
    let optional_registry = recorder.registry["tag"] == "Registry"
        && recorder.registry["coverage"]["registryVersion"] == 8;
    if (profile == "optional-payment") != optional_registry {
        return Err("optional-payment envelope requires its selected Registry8 witness".into());
    }
    let mandatory_registry = recorder.registry["tag"] == "Registry"
        && recorder.registry["coverage"]["registryVersion"] == 7;
    if (profile == "mandatory-trigger") != mandatory_registry {
        return Err("mandatory-trigger envelope requires its selected Registry7 witness".into());
    }
    if !observations.is_object() {
        return Err("current scoped observations must be an object".into());
    }
    if profile == "optional-payment-native" {
        native_packet_wire::validate_observations(&observations)?;
    }
    if profile == "optional-payment-native-role-fault" {
        native_role_fault_wire::validate_observations(&observations)?;
    }
    if optional_registry {
        let suite = &observations["data"];
        if !matches!(observations["status"].as_str(), Some("success" | "fail"))
            || suite["schema_version"] != 1
            || suite["profile"] != "optional-payment-v1"
            || suite["registry_version"] != 8
            || suite["catalogue_sha256"]
                != spellwake_contracts::teaching_profile::TEACHING_SOURCE_SHA256
            || !suite["runs"].as_array().is_some_and(|runs| {
                !runs.is_empty() && runs.iter().all(|run| run["schema_version"] == 4)
            })
        {
            return Err("optional-payment envelope requires its ordinary Report4 header".into());
        }
        // This header pairing does not certify any run. Native call/projection,
        // replay, source and reference qualification remain reader obligations.
    }
    let mut value = json!({"schema_version":REPORT_SCHEMA_VERSION,"profile":profile,"observations":observations,"type_context_witness":Value::Null});
    value["type_context_witness"] = recorder.witness(&value)?;
    if profile == "optional-payment-native" {
        native_packet_wire::validate_packet(&value)?;
    }
    if profile == "optional-payment-native-role-fault" {
        native_role_fault_wire::validate_packet(&value)?;
    }
    Ok(value)
}

/// Validate this disjoint index's finite fields only; exact inventory/artifact joins
/// remain writer/semantic-reader obligations.
///
/// # Errors
/// Rejects any index whose fields differ from the separate role-fault v1 contract.
pub fn validate_role_fault_index(value: &Value) -> Result<(), String> {
    native_role_fault_wire::validate_index(value)
}

#[cfg(test)]
#[path = "role_fault_projection_contract_tests.rs"]
mod role_fault_projection_contract_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use spellwake_engine::{
        game::{GameError, GameId},
        object_identity::{CardId, ObjectDestination},
    };

    fn recorder(vocabulary: &CreatureTypeVocabulary) -> Recorder {
        // Synthetic unit metadata only; these tests produce no qualification capture.
        let source = |path: &str| SourceRef {
            path: path.into(),
            sha256: "0".repeat(64),
            bytes: 0,
        };
        Recorder::new(
            vocabulary,
            source("compiled-synthetic-input"),
            source("compiled-synthetic-publication"),
            vec![],
            RegistryWitness::NotApplicable,
        )
        .unwrap()
    }
    #[test]
    fn native_optional_finite_header_rejects_reviewed_contract_counterexamples() {
        // Synthetic wire-shape guards only. These create no native qualification evidence.
        let root = json!({
            "tag":"ActualNativeInitializerRoot","ordinal":0,"label":"synthetic-header-control","helper":"tests.initialize",
            "initial_state_point":"point","role_context":{"participants":[0,1],"active":0},"initial_state":{},"initial_journal":[]
        });
        let header = json!({
            "schema":"spellwake.optional-payment-native-packets.v2","scope":"NativeOnly","case_id":"OP-N05",
            "roles":{"participants":[0,1],"active":0},"provenance":{"tag":"SyntheticNativeOnly","ordinary_credit":false,"source_card_credit":false,"full_game_credit":false},
            "root_segments":[root.clone()],"supplied_owners":[],"replay":null
        });
        assert!(native_packet_wire::validate_observations(&header).is_ok());
        for (pointer, value) in [
            ("/roles", Value::Null),
            ("/replay", json!({})),
            ("/roles/active", json!("0")),
            ("/case_id", json!("OP-N99")),
        ] {
            let mut changed = header.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                native_packet_wire::validate_observations(&changed).is_err(),
                "{pointer}"
            );
        }
        let mut receiver = header.clone();
        receiver["case_id"] = json!("OP-N24");
        receiver["root_segments"] = json!([root.clone(), root]);
        receiver["supplied_owners"] = json!([{"junk":true}]);
        assert!(native_packet_wire::validate_observations(&receiver).is_err());
        receiver["supplied_owners"] = json!([{
            "tag":"NativeOnlySuppliedOwnerAfterPrivilegedSetup","owner":"EffectWindow","point":"p","root_continuity":"NotClaimed","preceding_root_ordinal":1,
            "setup":"synthetic structural control","state":{},"setup_movements":[{"before":{},"after":{}}],"setup_journal":[]
        }]);
        assert!(native_packet_wire::validate_observations(&receiver).is_ok());
    }
    /// Synthetic DTO controls only; these rows are not catalogue evidence.
    fn teaching_registry_dto() -> Value {
        let cards: Vec<_> = (0..20)
            .map(|index| {
                json!({"name":format!("Control {index}"),
                    "oracleId":format!("00000000-0000-0000-0000-{index:012}"),
                    "required":true,"behaviorEvidence":"synthetic-unit-control",
                    "binding":{"status":"partial","program":if index<2{"teaching-basic-land-v1"}else{"teaching-vanilla-creature-v1"},
                        "scope":"teaching-ordinary-card-v1","semanticSha256":"0".repeat(64)}})
            })
            .collect();
        json!({"sourceSha256":spellwake_contracts::teaching_profile::TEACHING_SOURCE_SHA256,
            "sourceRecords":39196,"requiredRecords":20,"requiredPartialBindings":20,
            "requiredWithoutBinding":0,"fullyQualifiedCards":0,
            "behaviorEvidence":"synthetic-unit-control","registryVersion":5,
            "creatureVocabularyId":spellwake_contracts::teaching_profile::TEACHING_VOCABULARY_ID,"cards":cards})
    }
    #[test]
    fn teaching_registry_witness_rejects_wrong_pins_incomplete_pools_and_foreign_variants() {
        let valid = teaching_registry_dto();
        let vocabulary = spellwake_contracts::teaching_profile::TEACHING_VOCABULARY_ID;
        validate_registry(&valid, vocabulary).unwrap();
        for (pointer, replacement) in [
            ("/sourceSha256", json!("0".repeat(64))),
            ("/creatureVocabularyId", json!("0".repeat(64))),
            ("/registryVersion", json!(4)),
            ("/registryVersion", json!(6)),
            ("/requiredRecords", json!(19)),
            ("/requiredPartialBindings", json!(19)),
            ("/requiredWithoutBinding", json!(1)),
            ("/fullyQualifiedCards", json!(20)),
            ("/sourceRecords", json!(19)),
            ("/cards/0/required", json!(false)),
            ("/cards/0/oracleId", Value::Null),
            ("/cards/0/binding/status", json!("missing")),
            ("/cards/0/binding/program", json!("bolt-creature-v1")),
            (
                "/cards/0/binding/program",
                json!("teaching-vanilla-creature-v1"),
            ),
            ("/cards/0/binding/scope", json!("creature-target-only")),
        ] {
            let mut changed = valid.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                validate_registry(&changed, vocabulary).is_err(),
                "{pointer}"
            );
        }
        let mut duplicate = valid.clone();
        duplicate["cards"][1]["oracleId"] = duplicate["cards"][0]["oracleId"].clone();
        assert!(validate_registry(&duplicate, vocabulary).is_err());
        let mut missing = valid;
        missing["cards"].as_array_mut().unwrap().pop();
        assert!(validate_registry(&missing, vocabulary).is_err());
    }
    #[test]
    fn interaction_registry_shape_keeps_its_own_families_and_epoch() {
        // Synthetic DTO controls only. Actual identity/source joins are exercised
        // by the catalogue registry and reference consumer, not this shape gate.
        let mut valid = teaching_registry_dto();
        valid["registryVersion"] = json!(6);
        for (index, card) in valid["cards"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            card["binding"]["program"] = json!(match index {
                0..=1 => "interaction-basic-land-v1",
                2..=15 => "interaction-vanilla-creature-v1",
                16 => "interaction-bounce-creature-v1",
                17..=18 => "interaction-counter-spell-v1",
                19 => "interaction-draw-two-v1",
                _ => unreachable!(),
            });
            card["binding"]["scope"] = json!("interaction-ordinary-card-v1");
        }
        let vocabulary = spellwake_contracts::teaching_profile::TEACHING_VOCABULARY_ID;
        validate_registry(&valid, vocabulary).unwrap();
        for (pointer, replacement) in [
            ("/registryVersion", json!(5)),
            ("/sourceRecords", json!(39195)),
            ("/requiredRecords", json!(19)),
            ("/requiredPartialBindings", json!(19)),
            ("/requiredWithoutBinding", json!(1)),
            ("/fullyQualifiedCards", json!(20)),
            ("/sourceSha256", json!("0".repeat(64))),
            ("/creatureVocabularyId", json!("0".repeat(64))),
            ("/cards/0/required", json!(false)),
            ("/cards/0/oracleId", Value::Null),
            ("/cards/0/binding/status", json!("missing")),
            ("/cards/0/binding/program", json!("teaching-basic-land-v1")),
            (
                "/cards/0/binding/program",
                json!("interaction-vanilla-creature-v1"),
            ),
            ("/cards/0/binding/scope", json!("teaching-ordinary-card-v1")),
        ] {
            let mut changed = valid.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                validate_registry(&changed, vocabulary).is_err(),
                "{pointer}"
            );
        }
        let mut duplicate = valid.clone();
        duplicate["cards"][1]["oracleId"] = duplicate["cards"][0]["oracleId"].clone();
        assert!(validate_registry(&duplicate, vocabulary).is_err());
        valid["cards"].as_array_mut().unwrap().pop();
        assert!(validate_registry(&valid, vocabulary).is_err());
    }
    /// Synthetic DTO controls using the retained identity projection, not catalogue evidence.
    fn mandatory_registry_dto() -> Value {
        let definitions: Value =
            serde_json::from_str(include_str!("ordinary_game/mandatory-definitions-v1.json"))
                .unwrap();
        let cards: Vec<_> = definitions
            .as_array()
            .unwrap()
            .iter()
            .map(|definition| {
                let program = match definition["name"].as_str().unwrap() {
                    "Forest" | "Island" | "Plains" | "Swamp" => "mandatory-basic-land-v1",
                    "Centaur Courser" | "Grizzly Bears" | "Kalonian Tusker" | "Rumbling Baloth" => {
                        "mandatory-vanilla-creature-v1"
                    }
                    "Counterspell" => "mandatory-counter-spell-v1",
                    "Raise the Alarm" => "mandatory-token-creation-spell-v1",
                    "Unsummon" => "mandatory-bounce-creature-v1",
                    "Blood Artist" | "Doomed Dissenter" | "Elvish Visionary"
                    | "Impassioned Orator" | "Llanowar Visionary" | "Novice Occultist"
                    | "Phyrexian Rager" | "Soul Warden" => "mandatory-triggered-creature-v1",
                    name => panic!("unlisted synthetic Registry7 fixture name: {name}"),
                };
                json!({"name":definition["name"],"oracleId":definition["oracleId"],
                    "required":true,"behaviorEvidence":"synthetic-unit-control",
                    "binding":{"status":"partial","program":program,
                        "scope":"mandatory-trigger-ordinary-card-v1",
                        "semanticSha256":definition["registrySignatureV1Sha256"]}})
            })
            .collect();
        json!({"sourceSha256":spellwake_contracts::teaching_profile::TEACHING_SOURCE_SHA256,
            "sourceRecords":39196,"requiredRecords":19,"requiredPartialBindings":19,
            "requiredWithoutBinding":0,"fullyQualifiedCards":0,
            "behaviorEvidence":"synthetic-unit-control","registryVersion":7,
            "creatureVocabularyId":spellwake_contracts::teaching_profile::TEACHING_VOCABULARY_ID,"cards":cards})
    }
    #[test]
    fn mandatory_registry_witness_requires_its_finite_source_counts_families_and_scope() {
        let valid = mandatory_registry_dto();
        let vocabulary = spellwake_contracts::teaching_profile::TEACHING_VOCABULARY_ID;
        validate_registry(&valid, vocabulary).unwrap();
        for (pointer, replacement) in [
            ("/registryVersion", json!(3)),
            ("/registryVersion", json!(4)),
            ("/registryVersion", json!(5)),
            ("/registryVersion", json!(6)),
            ("/registryVersion", json!(8)),
            ("/registryVersion", json!(9)),
            ("/sourceSha256", json!("0".repeat(64))),
            ("/sourceRecords", json!(19)),
            ("/sourceRecords", json!(39195)),
            ("/sourceRecords", json!(39197)),
            ("/requiredRecords", json!(20)),
            ("/requiredPartialBindings", json!(18)),
            ("/requiredWithoutBinding", json!(1)),
            ("/fullyQualifiedCards", json!(1)),
            ("/creatureVocabularyId", json!("0".repeat(64))),
            ("/cards/0/required", json!(false)),
            ("/cards/0/binding/status", json!("missing")),
            (
                "/cards/0/binding/program",
                json!("teaching-vanilla-creature-v1"),
            ),
            (
                "/cards/0/binding/program",
                json!("interaction-vanilla-creature-v1"),
            ),
            (
                "/cards/0/binding/program",
                json!("mandatory-vanilla-creature-v1"),
            ),
            ("/cards/0/binding/program", json!("mandatory-unlisted-v1")),
            (
                "/cards/0/binding/scope",
                json!("interaction-ordinary-card-v1"),
            ),
            ("/cards/0/binding/semanticSha256", json!("0".repeat(63))),
        ] {
            let mut changed = valid.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                validate_registry(&changed, vocabulary).is_err(),
                "{pointer}"
            );
        }
        assert!(validate_registry(&valid, &"0".repeat(64)).is_err());
        for pointer in [
            "/sourceRecords",
            "/requiredRecords",
            "/requiredPartialBindings",
            "/requiredWithoutBinding",
            "/fullyQualifiedCards",
        ] {
            for replacement in [json!("19"), json!(-1), json!(19.0)] {
                let mut changed = valid.clone();
                *changed.pointer_mut(pointer).unwrap() = replacement;
                assert!(
                    validate_registry(&changed, vocabulary).is_err(),
                    "{pointer}"
                );
            }
        }
    }
    #[test]
    fn mandatory_registry_witness_requires_the_exact_distinct_identity_set() {
        let valid = mandatory_registry_dto();
        let vocabulary = spellwake_contracts::teaching_profile::TEACHING_VOCABULARY_ID;
        let mut reordered = valid.clone();
        reordered["cards"].as_array_mut().unwrap().reverse();
        validate_registry(&reordered, vocabulary).unwrap();
        for replacement in [
            Value::Null,
            json!("00000000-0000-0000-0000-000000000000"),
            json!("310F141c-7f37-4729-aed6-dd9c09db448d"),
            valid["cards"][1]["oracleId"].clone(),
        ] {
            let mut changed = valid.clone();
            changed["cards"][0]["oracleId"] = replacement;
            assert!(validate_registry(&changed, vocabulary).is_err());
        }
        let mut missing = valid.clone();
        missing["cards"].as_array_mut().unwrap().pop();
        assert!(validate_registry(&missing, vocabulary).is_err());
        let mut extra = valid.clone();
        extra["cards"]
            .as_array_mut()
            .unwrap()
            .push(valid["cards"][0].clone());
        assert!(validate_registry(&extra, vocabulary).is_err());
        for pointer in ["", "/cards/0", "/cards/0/binding"] {
            let mut changed = valid.clone();
            changed
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unlisted".into(), json!(true));
            assert!(
                validate_registry(&changed, vocabulary).is_err(),
                "{pointer}"
            );
        }
    }
    #[test]
    fn mandatory_envelope_pairs_registry7_with_current10_and_witness8_only() {
        // Envelope-only unit metadata; actual Registry admission is checked above.
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut recorder = recorder(&vocabulary);
        recorder.registry = json!({"tag":"Registry","coverage":mandatory_registry_dto()});
        recorder
            .finish_coverage(vec![], vec![], vec![], vec![])
            .unwrap();
        let observations = json!({"schema_version":3,"profile":"mandatory-trigger-v1"});
        let envelope = envelope("mandatory-trigger", observations.clone(), &recorder).unwrap();
        assert_eq!(envelope.as_object().unwrap().len(), 4);
        assert_eq!(envelope["schema_version"], 10);
        assert_eq!(envelope["profile"], "mandatory-trigger");
        assert_eq!(envelope["observations"], observations);
        assert_eq!(
            envelope["type_context_witness"]["schema"],
            "spellwake.current-type-context-witness.v8"
        );
        assert_eq!(
            envelope["type_context_witness"]["registry"]["coverage"]["registryVersion"],
            7
        );
        assert_eq!(
            envelope["type_context_witness"]["vocabulary"]["value"],
            projection::vocabulary(&vocabulary)
        );
        for profile in [
            "mandatory-trigger-v1",
            "mandatory",
            "interaction",
            "teaching",
            "original",
            "unlisted",
        ] {
            assert!(
                super::envelope(profile, observations.clone(), &recorder).is_err(),
                "{profile}"
            );
        }
        for registry in [
            json!({"tag":"NotApplicable","reason":"NoRegistryConstruction"}),
            json!({"tag":"Registry","coverage":teaching_registry_dto()}),
            json!({"tag":"Registry","coverage":{"registryVersion":4}}),
            json!({"tag":"Registry","coverage":{"registryVersion":6}}),
        ] {
            recorder.registry = registry;
            assert!(super::envelope("mandatory-trigger", observations.clone(), &recorder).is_err());
        }
    }
    #[test]
    fn new_current_epochs_do_not_admit_unimplemented_optional_payment_source_profiles() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut recorder = recorder(&vocabulary);
        recorder
            .finish_coverage(vec![], vec![], vec![], vec![])
            .unwrap();
        for profile in ["optional-payment", "optional-payment-v1"] {
            assert!(envelope(profile, json!({}), &recorder).is_err());
        }
        // Changing only the version cannot establish the source/profile guards
        // required by the separate Registry8 and ordinary Report4 owners.
        let mut unimplemented = mandatory_registry_dto();
        unimplemented["registryVersion"] = json!(8);
        assert!(
            validate_registry(
                &unimplemented,
                spellwake_contracts::teaching_profile::TEACHING_VOCABULARY_ID
            )
            .is_err()
        );
    }
    fn replay() -> ReplayEvidence {
        ReplayEvidence {
            source_eq: Some(true),
            source_capture: SourceCapture::BeforeInvocation,
        }
    }
    fn joins(pairs: &[(&str, &str)]) -> Vec<ObservationJoin> {
        pairs
            .iter()
            .map(|(pointer, point)| ObservationJoin {
                pointer: (*pointer).into(),
                points: vec![(*point).into()],
            })
            .collect()
    }
    #[test]
    fn primitive_result_reexecution_keeps_exact_ownership_and_record_pointer_binding() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut recorder = recorder(&vocabulary);
        let source = GameObjects::new(0);
        let seats = [SeatId(0), SeatId(1)];
        let aliases = AliasContext::default();
        recorder
            .snapshot(
                "before",
                &["/observations/before".into()],
                Phase::BeforeCall,
                "GameObjects::seed_card",
                SnapshotOwner::GameObjects {
                    cards: &source,
                    participants: &seats,
                },
                &aliases,
            )
            .unwrap();
        let result = source.seed_card(0, CardId(41), seats[0], ObjectDestination::Hand);
        let again = source.seed_card(0, CardId(41), seats[0], ObjectDestination::Hand);
        let next = &result.as_ref().unwrap().next;
        recorder
            .snapshot(
                "after",
                &["/observations/after".into()],
                Phase::AfterCall,
                "GameObjects::seed_card",
                SnapshotOwner::GameObjects {
                    cards: next,
                    participants: &seats,
                },
                &aliases,
            )
            .unwrap();
        let call =
            projection::QualificationCall::GameObjectsCall(projection::GameObjectsCall::SeedCard {
                expected: 0,
                card: CardId(41),
                owner: seats[0],
                destination: ObjectDestination::Hand,
            });
        recorder.record_call(CallPoint{call_id:"seed",before_point:Some("before"),after_point:Some("after"),adopted:true},&call,&result,&again,|result|Ok(match result{Ok(value)=>json!({"tag":"Ok","state":projection::card_objects(&value.next,&seats),"before":value.before.as_ref().map(projection::object),"after":projection::object(&value.after)}),Err(error)=>projection::object_identity_error(error)}),Append::NotApplicable,replay(),&aliases,None).unwrap();
        recorder
            .reference_record_point("before", "/observations/before")
            .unwrap();
        recorder
            .finish_coverage(
                vec!["before".into(), "after".into()],
                vec!["seed".into()],
                joins(&[
                    ("/observations/before", "before"),
                    ("/observations/after", "after"),
                ]),
                vec!["/observations/before".into()],
            )
            .unwrap();
        let observations = json!({"before":projection::card_objects(&source,&seats),"after":projection::card_objects(next,&seats)});
        let value = envelope("library", observations.clone(), &recorder).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 4);
        let point = &value["type_context_witness"]["snapshots"][0];
        assert_eq!(
            point["native_vocabulary"],
            json!({"tag":"NotOwned","reason":"OwnerHasNoVocabulary"})
        );
        assert_eq!(
            point["resolution"],
            json!({"tag":"NotApplicable","reason":"OwnerHasNoResolution"})
        );
        assert_eq!(
            point["projection"],
            json!({"tag":"RecordPointer","pointer":"/observations/before"})
        );
        let mut wrong = observations;
        wrong["before"]["revision"] = json!("900");
        assert!(envelope("library", wrong, &recorder).is_err());
    }
    #[test]
    fn actual_native_error_has_no_successor_and_never_fabricates_an_empty_append() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut recorder = recorder(&vocabulary);
        let source = GameObjects::new(0);
        let seats = [SeatId(0), SeatId(1)];
        let aliases = AliasContext::default();
        recorder
            .snapshot(
                "before",
                &["/observations/source".into()],
                Phase::BeforeCall,
                "GameObjects::seed_card",
                SnapshotOwner::GameObjects {
                    cards: &source,
                    participants: &seats,
                },
                &aliases,
            )
            .unwrap();
        let result = source.seed_card(9, CardId(1), seats[0], ObjectDestination::Hand);
        let again = source.seed_card(9, CardId(1), seats[0], ObjectDestination::Hand);
        let call =
            projection::QualificationCall::GameObjectsCall(projection::GameObjectsCall::SeedCard {
                expected: 9,
                card: CardId(1),
                owner: seats[0],
                destination: ObjectDestination::Hand,
            });
        recorder
            .record_call(
                CallPoint {
                    call_id: "reject",
                    before_point: Some("before"),
                    after_point: None,
                    adopted: false,
                },
                &call,
                &result,
                &again,
                |value| {
                    Ok(projection::object_identity_error(
                        value.as_ref().err().unwrap(),
                    ))
                },
                Append::NotApplicable,
                replay(),
                &aliases,
                None,
            )
            .unwrap();
        recorder
            .finish_coverage(
                vec!["before".into()],
                vec!["reject".into()],
                joins(&[("/observations/source", "before")]),
                vec![],
            )
            .unwrap();
        let value = envelope(
            "library",
            json!({"source":projection::card_objects(&source,&seats)}),
            &recorder,
        )
        .unwrap();
        assert_eq!(
            value["type_context_witness"]["calls"][0]["append"],
            json!({"tag":"NoReturnedTransition","reason":"NativeError"})
        );
        assert_eq!(
            value["type_context_witness"]["calls"][0]["after_point"],
            Value::Null
        );
    }
    #[test]
    fn consuming_initializer_observes_typed_input_without_inventing_a_live_source() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut recorder = recorder(&vocabulary);
        let aliases = AliasContext::default();
        let context = GameContext::new(
            "synthetic".into(),
            vocabulary.rules_revision().into(),
            "synthetic".into(),
            "synthetic".into(),
            "synthetic".into(),
            vocabulary.id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![SeatId(0), SeatId(1)],
            SeatId(0),
            vec![],
            vec![],
            vec![],
            20.into(),
            spellwake_engine::combat::CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        );
        recorder
            .snapshot(
                "input",
                &["/observations/input".into()],
                Phase::InitializeInput,
                "GameState::initialize",
                SnapshotOwner::InitializeInput {
                    context: Some(&context),
                    seed: &seed,
                    source_ref: "compiled-synthetic-input",
                },
                &aliases,
            )
            .unwrap();
        let result = GameState::initialize(GameId(9), context.clone(), seed.clone());
        let again = GameState::initialize(GameId(9), context.clone(), seed.clone());
        let candidate = result.as_ref().unwrap();
        recorder
            .snapshot(
                "after",
                &["/observations/after".into()],
                Phase::AfterCall,
                "GameState::initialize",
                SnapshotOwner::GameState(candidate.state()),
                &aliases,
            )
            .unwrap();
        let call = projection::QualificationCall::GameInitialize {
            game: GameId(9),
            context: &context,
            seed: &seed,
        };
        recorder.record_call(CallPoint{call_id:"initialize",before_point:Some("input"),after_point:Some("after"),adopted:true},&call,&result,&again,|value:&Result<_,GameError>|Ok(match value{Ok(value)=>json!({"tag":"Ok","candidate":projection::candidate(value,None,&aliases)?}),Err(error)=>projection::game_error(error)}),Append::RulesJournal(candidate.rules_journal()),ReplayEvidence{source_eq:None,source_capture:SourceCapture::InputOnlyForInitializer},&aliases,None).unwrap();
        recorder
            .finish_coverage(
                vec!["input".into(), "after".into()],
                vec!["initialize".into()],
                joins(&[
                    ("/observations/input", "input"),
                    ("/observations/after", "after"),
                ]),
                vec![],
            )
            .unwrap();
        let value=envelope("setup",json!({"input":{"context":projection::context(&context),"seed":projection::game_seed_input(&seed)},"after":projection::state(candidate.state(),&aliases).unwrap()}),&recorder).unwrap();
        assert_eq!(
            value["type_context_witness"]["snapshots"][0]["native_vocabulary"]["tag"],
            "SuppliedInput"
        );
        assert_eq!(
            value["type_context_witness"]["calls"][0]["replay"]["source_eq"],
            Value::Null
        );
        assert_eq!(
            value["type_context_witness"]["calls"][0]["replay"]["source_snapshot_before_invocation"],
            "InputOnlyForInitializer"
        );
    }
    #[test]
    fn incomplete_duplicate_and_missing_observation_bindings_cannot_finish_a_capture() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut recorder = recorder(&vocabulary);
        let aliases = AliasContext::default();
        let scalar = json!(1);
        recorder
            .snapshot(
                "point",
                &["/observations/value".into()],
                Phase::PureResult,
                "pure-synthetic",
                SnapshotOwner::PureValue(&scalar),
                &aliases,
            )
            .unwrap();
        assert!(
            recorder
                .snapshot(
                    "point",
                    &[],
                    Phase::PureResult,
                    "pure-synthetic",
                    SnapshotOwner::PureValue(&scalar),
                    &aliases
                )
                .is_err()
        );
        assert!(
            recorder
                .finish_coverage(vec![], vec![], vec![], vec![])
                .is_err()
        );
        assert!(
            recorder
                .finish_coverage(vec!["point".into()], vec![], vec![], vec![])
                .is_err()
        );
        recorder
            .finish_coverage(
                vec!["point".into()],
                vec![],
                joins(&[("/observations/value", "point")]),
                vec![],
            )
            .unwrap();
        assert!(envelope("library", json!({}), &recorder).is_err());
        assert!(envelope("unknown-profile", json!({"value":1}), &recorder).is_err());
    }
    #[test]
    fn native_only_points_bind_only_their_exact_emitted_inline_value_leaf() {
        let vocabulary = super::super::creature_types::synthetic_vocabulary();
        let mut recorder = recorder(&vocabulary);
        let aliases = AliasContext::default();
        let cards = GameObjects::new(9);
        let seats = [SeatId(0)];
        recorder
            .snapshot(
                "native",
                &[],
                Phase::Observation,
                "GameObjects::actual-read",
                SnapshotOwner::GameObjects {
                    cards: &cards,
                    participants: &seats,
                },
                &aliases,
            )
            .unwrap();
        let pointer = recorder.bind_native_point("native").unwrap();
        assert_eq!(
            pointer,
            "/type_context_witness/snapshots/0/projection/value"
        );
        recorder
            .finish_coverage(
                vec!["native".into()],
                vec![],
                joins(&[(&pointer, "native")]),
                vec![pointer.clone()],
            )
            .unwrap();
        let value = envelope("library", json!({}), &recorder).unwrap();
        assert_eq!(
            value.pointer(&pointer),
            Some(&projection::card_objects(&cards, &seats))
        );
        let mut bad = Recorder::new(
            &vocabulary,
            SourceRef {
                path: "synthetic-input".into(),
                sha256: "0".repeat(64),
                bytes: 0,
            },
            SourceRef {
                path: "synthetic-publication".into(),
                sha256: "0".repeat(64),
                bytes: 0,
            },
            vec![],
            RegistryWitness::NotApplicable,
        )
        .unwrap();
        let metadata = "/type_context_witness/snapshots/0/projection";
        bad.snapshot(
            "native",
            &[metadata.into()],
            Phase::Observation,
            "GameObjects::actual-read",
            SnapshotOwner::GameObjects {
                cards: &cards,
                participants: &seats,
            },
            &aliases,
        )
        .unwrap();
        bad.finish_coverage(
            vec!["native".into()],
            vec![],
            joins(&[(metadata, "native")]),
            vec![],
        )
        .unwrap();
        assert!(envelope("library", json!({}), &bad).is_err());
    }
}
