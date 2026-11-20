//! Caller-side source bindings for current native qualification captures.
//! The collector receives actual file bindings and never resolves evidence itself.
use super::{
    creature_types,
    current_profile::{CaptureScope, Recorder, RegistryWitness, SourceRef},
};
use sha2::{Digest, Sha256};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub fn source(path: &Path) -> Result<SourceRef, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(SourceRef {
        path: path.to_string_lossy().into_owned(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        bytes: bytes
            .len()
            .try_into()
            .map_err(|_| "source byte length is not representable")?,
    })
}
pub fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("qualification crate is a workspace member under crates")
        .to_owned()
}
fn validate_publication(bytes: &[u8], vocabulary: &CreatureTypeVocabulary) -> Result<(), String> {
    let value = super::packet::unique_json(bytes)?;
    let exact = |value: &Value, fields: &[&str]| -> Result<(), String> {
        let object = value
            .as_object()
            .ok_or("publication DTO must be an object")?;
        if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
            return Err("publication DTO has missing or unknown fields".into());
        }
        Ok(())
    };
    exact(&value, &["status", "data"])?;
    exact(&value["data"], &["tag", "publication"])?;
    let publication = &value["data"]["publication"];
    exact(
        publication,
        &[
            "schema_version",
            "kind",
            "source",
            "section",
            "members",
            "identity",
            "provider_subtype_aliases",
        ],
    )?;
    if value["status"] != "success"
        || value["data"]["tag"] != "Complete"
        || publication["schema_version"] != 1
        || publication["kind"] != "source-qualified-creature-type-publication"
    {
        return Err("actual Complete native creature-type publication is required".into());
    }
    let rules_sha256 = vocabulary
        .rules_sha256()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if publication["source"]["rules_revision"] != vocabulary.rules_revision()
        || publication["source"]["rules_sha256"] != rules_sha256
        || publication["identity"]["vocabulary_id"] != vocabulary.id().to_string()
    {
        return Err(
            "actual publication source or vocabulary identity differs from selected native input"
                .into(),
        );
    }
    let members = publication["members"]
        .as_array()
        .ok_or("publication members must be an array")?;
    if members.len() != vocabulary.members().len() {
        return Err("actual publication member count differs from selected native input".into());
    }
    for (index, (member, label)) in members.iter().zip(vocabulary.members()).enumerate() {
        exact(member, &["ordinal", "canonical_label"])?;
        let ordinal = index.to_string();
        if member["ordinal"].as_str() != Some(ordinal.as_str())
            || member["canonical_label"] != *label
        {
            return Err(
                "actual publication canonical members differ from selected native input".into(),
            );
        }
    }
    Ok(())
}
fn publication_source(
    path: &Path,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SourceRef, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    validate_publication(&bytes, vocabulary)?;
    Ok(SourceRef {
        path: path.to_string_lossy().into_owned(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        bytes: bytes
            .len()
            .try_into()
            .map_err(|_| "publication byte length is not representable")?,
    })
}
/// Publication is an explicit actual native D06 output, supplied by the capture caller.
/// An input candidate is never substituted for the missing publication.
pub fn recorder(
    vocabulary: &CreatureTypeVocabulary,
    source_paths: &[PathBuf],
    registry: RegistryWitness<'_>,
) -> Result<Recorder, String> {
    let input_path = creature_types::selected_vocabulary_path()?;
    let input_vocabulary = creature_types::load_vocabulary(&input_path)?;
    if *input_vocabulary != *vocabulary {
        return Err("native selected vocabulary differs from explicit captured input".into());
    }
    let publication = env::var_os("SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION").ok_or(
        "explicit actual D06 publication path SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION required",
    )?;
    let mut paths = source_paths.to_vec();
    let repo = repository();
    for path in [
        "crates/reference_tools/src/current_capture.rs",
        "crates/reference_tools/src/current_profile.rs",
        "crates/reference_tools/src/current_profile/caller_delivery.rs",
        "crates/reference_tools/src/current_profile/applicability.rs",
        "crates/reference_tools/src/current_capture/deferred.rs",
        "crates/reference_tools/src/ordinary_game/view/epoch.rs",
        "crates/reference_tools/src/ordinary_game/mandatory-definitions-v1.json",
        "crates/reference_tools/src/capture_points.rs",
        "crates/reference_tools/src/native_calls.rs",
        "crates/engine/test_support/packet.rs",
        "crates/engine/test_support/program.rs",
        "crates/engine/test_support/projection.rs",
        "crates/engine/test_support/creature_types.rs",
        "crates/engine/test_support/creature_vocabulary.rs",
        "crates/engine/test_support/check_trace.rs",
        "crates/engine/test_support/damage_trace.rs",
        "crates/engine/test_support/error.rs",
    ] {
        let path = repo.join(path);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let source_inputs = paths
        .iter()
        .map(|path| source(path))
        .collect::<Result<Vec<_>, _>>()?;
    Recorder::new(
        vocabulary,
        source(&input_path)?,
        publication_source(Path::new(&publication), vocabulary)?,
        source_inputs,
        registry,
    )
}

use super::{
    current_profile::{
        Append, CallPoint, ObservationJoin, Phase, ReplayEvidence, SnapshotOwner, SourceCapture,
    },
    packet::AliasContext,
    projection,
};
use serde_json::Value;
use spellwake_engine::{
    game::{GameContext, GameError, GameId, GameInput, GameState, NativeCandidate, OpeningSetup},
    setup::GameSeed,
    turn::TurnStart,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

struct ActiveCapture {
    identity: u64,
    profile: String,
    recorder: Recorder,
    points: Vec<String>,
    calls: Vec<String>,
    observations: BTreeMap<String, Vec<String>>,
    record_pointers: Vec<String>,
    sequence: u64,
    failure: Option<String>,
}
thread_local! {
    static ACTIVE:RefCell<Option<ActiveCapture>>=const {RefCell::new(None)};
    static SUSPENSION_DEPTH:Cell<u64>=const {Cell::new(0)};
    static NEXT_CAPTURE_ID:Cell<u64>=const {Cell::new(0)};
    #[cfg(test)]
    static EXHAUST_NEXT_OBSERVATION:Cell<bool>=const {Cell::new(false)};
}
/// Suspend metadata collection only while an existing caller validates reexecution.
/// A guard restores the same recorder even if the validation callback unwinds.
pub fn without_capture<T>(operation: impl FnOnce() -> T) -> T {
    struct Suspension(Option<ActiveCapture>);
    impl Drop for Suspension {
        fn drop(&mut self) {
            ACTIVE.with(|active| *active.borrow_mut() = self.0.take());
            SUSPENSION_DEPTH.with(|depth| depth.set(depth.get() - 1));
        }
    }
    SUSPENSION_DEPTH.with(|depth| {
        depth.set(
            depth
                .get()
                .checked_add(1)
                .expect("qualification suspension nesting exhausted"),
        )
    });
    let guard = Suspension(ACTIVE.with(|active| active.borrow_mut().take()));
    let result = operation();
    drop(guard);
    result
}
fn active<T>(operation: impl FnOnce(&mut ActiveCapture) -> Result<T, String>) -> Result<T, String> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        operation(active.as_mut().ok_or("current capture was not started")?)
    })
}
// Evidence labels returned after a typed applicability denial are coordinates
// only: they never enter native GameInput, decision or object identity domains.
// Acknowledge the first denied metadata operation as well as later ones so
// pre-native ?/unwrap adapters still execute the exact original native call.
// Recorder keeps the denial and refuses all final sealing/witness publication.
fn metadata(
    operation: impl FnOnce(&mut ActiveCapture) -> Result<(), String>,
) -> Result<(), String> {
    active(|active| {
        if active.recorder.applicability_rejected() {
            return Ok(());
        }
        let result = operation(active);
        if active.recorder.applicability_rejected() {
            Ok(())
        } else {
            result
        }
    })
}
fn remember(result: Result<(), String>) {
    if let Err(error) = result {
        ACTIVE.with(|active| {
            if let Some(active) = active.borrow_mut().as_mut()
                && active.failure.is_none()
            {
                active.failure = Some(error);
            }
        });
    }
}
/// Retain a postinstallation observer failure without changing native/caller
/// outcome. The first observer failure remains authoritative for qualification.
pub(crate) fn retain_observer_failure(error: String) {
    remember(Err(error));
}
fn token(kind: &str) -> Result<String, String> {
    active(|active| {
        active.sequence = active
            .sequence
            .checked_add(1)
            .ok_or("qualification producer label exhausted")?;
        Ok(format!("{kind}-{}", active.sequence))
    })
}
fn snapshot(
    point: &str,
    phase: Phase,
    owner_source: &str,
    owner: SnapshotOwner<'_>,
    aliases: &AliasContext,
) -> Result<(), String> {
    metadata(|active| {
        active
            .recorder
            .snapshot(point, &[], phase, owner_source, owner, aliases)?;
        active.points.push(point.into());
        if phase != Phase::Observation {
            let pointer = active.recorder.bind_native_point(point)?;
            active
                .observations
                .entry(pointer.clone())
                .or_default()
                .push(point.into());
            active.record_pointers.push(pointer);
        }
        Ok(())
    })
}
pub fn native_call_id() -> Result<String, String> {
    token("call")
}
pub fn native_snapshot(
    phase: Phase,
    owner_source: &str,
    owner: SnapshotOwner<'_>,
    aliases: &AliasContext,
) -> Result<String, String> {
    let point = token("native")?;
    snapshot(&point, phase, owner_source, owner, aliases)?;
    Ok(point)
}
fn capture_identity() -> Result<u64, String> {
    NEXT_CAPTURE_ID.with(|identity| {
        let next = identity
            .get()
            .checked_add(1)
            .ok_or("capture identity exhausted")?;
        identity.set(next);
        Ok(next)
    })
}
/// Begin one profile capture. The thread-local value stores observations only;
/// all native state remains with each existing caller.
pub fn start(
    profile: &str,
    source_paths: &[PathBuf],
    registry: Option<&Value>,
) -> Result<(), String> {
    start_with_registry_witness(
        profile,
        source_paths,
        registry
            .map(RegistryWitness::Registry)
            .unwrap_or(RegistryWitness::NotApplicable),
    )
}
/// Begin collection through the existing recorder with a sealed native witness.
/// This helper does not admit a new report/envelope profile.
///
/// # Errors
/// Preserves existing suspension/source/publication/already-active failures and
/// rejects any witness whose actual native/source relationships do not validate.
pub fn start_with_registry_witness(
    profile: &str,
    source_paths: &[PathBuf],
    registry: RegistryWitness<'_>,
) -> Result<(), String> {
    if SUSPENSION_DEPTH.with(|depth| depth.get() != 0) {
        return Err("current capture is suspended during native reexecution validation".into());
    }
    let vocabulary = creature_types::load_vocabulary(&creature_types::selected_vocabulary_path()?)?;
    let recorder = recorder(&vocabulary, source_paths, registry)?;
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        if active.is_some() {
            return Err("a current capture is already active".into());
        }
        *active = Some(ActiveCapture {
            identity: capture_identity()?,
            profile: profile.into(),
            recorder,
            points: vec![],
            calls: vec![],
            observations: BTreeMap::new(),
            record_pointers: Vec::new(),
            sequence: 0,
            failure: None,
        });
        Ok(())
    })
}
/// Observe a precise existing writer point; the caller later binds its exact DTO path.
pub fn observation(
    owner_source: &str,
    owner: SnapshotOwner<'_>,
    aliases: &AliasContext,
) -> Result<String, String> {
    // Exercise the real label-overflow error only at a committed observation.
    // Native before/after points and caller installation remain untouched.
    #[cfg(test)]
    if EXHAUST_NEXT_OBSERVATION.with(|flag| flag.replace(false)) {
        active(|capture| {
            capture.sequence = u64::MAX;
            Ok(())
        })?;
    }
    let point = token("observation")?;
    snapshot(&point, Phase::Observation, owner_source, owner, aliases)?;
    Ok(point)
}
pub fn bind_observation(point: &str, pointer: &str) -> Result<(), String> {
    metadata(|active| {
        active.recorder.bind_observation(point, pointer)?;
        active
            .observations
            .entry(pointer.into())
            .or_default()
            .push(point.into());
        Ok(())
    })
}
/// Close exact point/call inventories and validate every emitted observation path.
pub fn finish(profile: &str, observations: Value) -> Result<Value, String> {
    ACTIVE.with(|active| {
        let mut value = active
            .borrow_mut()
            .take()
            .ok_or("current capture was not started")?;
        if value.profile != profile {
            return Err("current capture profile differs from emitted profile".into());
        }
        if let Some(error) = value.failure {
            return Err(error);
        }
        let joins = value
            .observations
            .into_iter()
            .map(|(pointer, points)| ObservationJoin { pointer, points })
            .collect();
        value
            .recorder
            .finish_coverage(value.points, value.calls, joins, value.record_pointers)?;
        super::current_profile::envelope(profile, observations, &value.recorder)
    })
}
/// Record an exact source-owned point at paths already known by the caller.
pub fn snapshot_at(
    point: &str,
    pointers: &[String],
    phase: Phase,
    owner_source: &str,
    owner: SnapshotOwner<'_>,
    aliases: &AliasContext,
) -> Result<(), String> {
    metadata(|active| {
        active
            .recorder
            .snapshot(point, pointers, phase, owner_source, owner, aliases)?;
        active.points.push(point.into());
        for pointer in pointers {
            active
                .observations
                .entry(pointer.clone())
                .or_default()
                .push(point.into());
        }
        Ok(())
    })
}
/// Observe native Results already executed and retained by an existing caller.
#[allow(clippy::too_many_arguments)]
pub fn record_native<R: Eq + super::current_profile::applicability::NativeCaptureValue, E: Eq>(
    point: CallPoint<'_>,
    input: &projection::QualificationCall<'_>,
    result: &Result<R, E>,
    replayed: &Result<R, E>,
    project_result: impl Fn(&Result<R, E>) -> Result<Value, String>,
    append: Append<'_>,
    replay: ReplayEvidence,
    aliases: &AliasContext,
    execution: Option<&spellwake_engine::effects::EffectWindow>,
) -> Result<(), String> {
    metadata(|active| {
        active.recorder.record_call(
            CallPoint {
                call_id: point.call_id,
                before_point: point.before_point,
                after_point: point.after_point,
                adopted: point.adopted,
            },
            input,
            result,
            replayed,
            project_result,
            append,
            replay,
            aliases,
            execution,
        )?;
        active.calls.push(point.call_id.into());
        Ok(())
    })
}
pub fn reference_record_point(point: &str, pointer: &str) -> Result<(), String> {
    metadata(|active| {
        active.recorder.reference_record_point(point, pointer)?;
        active.record_pointers.push(pointer.into());
        Ok(())
    })
}
/// Finish the existing R21 full record, retaining its own original field layout.
pub fn finish_record(profile: &str, mut record: Value) -> Result<Value, String> {
    ACTIVE.with(|active| {
        let mut value = active
            .borrow_mut()
            .take()
            .ok_or("current capture was not started")?;
        if value.profile != profile {
            return Err("current capture profile differs from emitted profile".into());
        }
        if let Some(error) = value.failure {
            return Err(error);
        }
        let object = record
            .as_object_mut()
            .ok_or("current R21 record must be an object")?;
        object.insert(
            "schema_version".into(),
            serde_json::json!(value.recorder.scope().report_schema_version()),
        );
        object.insert(
            "record_codec".into(),
            serde_json::json!("spellwake.current-native-qualification.v2"),
        );
        let joins = value
            .observations
            .into_iter()
            .map(|(pointer, points)| ObservationJoin { pointer, points })
            .collect();
        value
            .recorder
            .finish_coverage(value.points, value.calls, joins, value.record_pointers)?;
        record["type_context_witness"] = value.recorder.witness(&record)?;
        Ok(record)
    })
}
pub fn is_active() -> bool {
    ACTIVE.with(|active| active.borrow().is_some())
}
/// Exact active observer scope; inactive execution has no implied capture epoch.
pub fn scope() -> Option<CaptureScope> {
    ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .map(|active| active.recorder.scope())
    })
}

fn result_view(
    value: &Result<NativeCandidate, GameError>,
    source: Option<&GameState>,
    aliases: &AliasContext,
) -> Result<Value, String> {
    Ok(match value {
        Ok(candidate) => {
            serde_json::json!({"tag":"Ok","candidate":projection::candidate(candidate,source,aliases)?})
        }
        Err(error) => projection::game_error(error),
    })
}
fn record_game_result(
    point: CallPoint<'_>,
    input: &projection::QualificationCall<'_>,
    result: &Result<NativeCandidate, GameError>,
    replayed: &Result<NativeCandidate, GameError>,
    source: Option<&GameState>,
    source_eq: Option<bool>,
    aliases: &AliasContext,
) -> Result<(), String> {
    metadata(|active| {
        active.recorder.record_call(
            CallPoint {
                call_id: point.call_id,
                before_point: point.before_point,
                after_point: point.after_point,
                adopted: point.adopted && result.is_ok(),
            },
            input,
            result,
            replayed,
            |value| result_view(value, source, aliases),
            result
                .as_ref()
                .map(|value| Append::RulesJournal(value.rules_journal()))
                .unwrap_or(Append::NotApplicable),
            ReplayEvidence {
                source_eq,
                source_capture: if input.is_initializer() {
                    SourceCapture::InputOnlyForInitializer
                } else {
                    SourceCapture::BeforeInvocation
                },
            },
            aliases,
            source.map(GameState::execution),
        )?;
        active.calls.push(point.call_id.into());
        Ok(())
    })
}
/// Execute the existing native command twice on the same actual borrowed source.
pub fn advance(source: &GameState, input: GameInput) -> Result<NativeCandidate, GameError> {
    advance_with_adoption(source, input, true)
}
pub fn advance_with_adoption(
    source: &GameState,
    input: GameInput,
    adopt: bool,
) -> Result<NativeCandidate, GameError> {
    reject_immediate_selection_caller();
    advance_call_recorded(source, input, adopt).0
}
fn reject_immediate_selection_caller() {
    if is_active() {
        remember(active(|active| {
            if active.recorder.scope() == CaptureScope::ResolutionSelection11 {
                Err("selection11 root calls require deferred caller completion".into())
            } else {
                Ok(())
            }
        }));
    }
}
fn advance_call_recorded(
    source: &GameState,
    input: GameInput,
    adopt: bool,
) -> (Result<NativeCandidate, GameError>, Option<String>) {
    if !is_active() {
        return (source.advance(input), None);
    }
    let mut invocation = deferred::InvocationGuard::new();
    let aliases = AliasContext::default();
    let retained = source.clone();
    let saved = input.clone();
    let call = token("call");
    let before = call.as_ref().map(|call| format!("before-{call}"));
    remember(
        before
            .as_ref()
            .map_err(|error| error.to_string())
            .and_then(|before| {
                snapshot(
                    before,
                    Phase::BeforeCall,
                    "GameState::advance",
                    SnapshotOwner::GameState(source),
                    &aliases,
                )
            }),
    );
    let result = source.advance(input);
    let replayed = source.advance(saved.clone());
    if let (Ok(call), Ok(before)) = (&call, &before) {
        let after = result.as_ref().ok().map(|_| format!("after-{call}"));
        remember(
            result
                .as_ref()
                .ok()
                .zip(after.as_deref())
                .map(|(result, after)| {
                    snapshot(
                        after,
                        Phase::AfterCall,
                        "GameState::advance",
                        SnapshotOwner::GameState(result.state()),
                        &aliases,
                    )
                })
                .unwrap_or(Ok(())),
        );
        remember(record_game_result(
            CallPoint {
                call_id: call,
                before_point: Some(before),
                after_point: after.as_deref(),
                adopted: adopt,
            },
            &projection::QualificationCall::GameInput(&saved),
            &result,
            &replayed,
            Some(source),
            Some(*source == retained),
            &aliases,
        ));
    }
    invocation.returned();
    (result, call.ok())
}
fn initialize_call(
    game: GameId,
    context: GameContext,
    seed: GameSeed,
    start: Option<TurnStart>,
    opening: Option<(spellwake_engine::shuffle::EntropyTape, Vec<u64>)>,
    adopt: bool,
) -> Result<NativeCandidate, GameError> {
    initialize_call_recorded(game, context, seed, start, opening, adopt).0
}
fn initialize_call_recorded(
    game: GameId,
    context: GameContext,
    seed: GameSeed,
    start: Option<TurnStart>,
    opening: Option<(spellwake_engine::shuffle::EntropyTape, Vec<u64>)>,
    adopt: bool,
) -> (Result<NativeCandidate, GameError>, Option<String>) {
    reject_immediate_selection_caller();
    let (result, points) = initialize_observed(game, context, seed, start, opening, adopt);
    (result, points.map(|points| points.before))
}
struct RootCallPoints {
    call: String,
    before: String,
}
fn initialize_observed(
    game: GameId,
    context: GameContext,
    seed: GameSeed,
    start: Option<TurnStart>,
    opening: Option<(spellwake_engine::shuffle::EntropyTape, Vec<u64>)>,
    adopt: bool,
) -> (Result<NativeCandidate, GameError>, Option<RootCallPoints>) {
    if !is_active() {
        return (
            if let Some((entropy, _)) = opening {
                GameState::initialize_opening(game, context, OpeningSetup { seed, entropy })
            } else if let Some(start) = start {
                GameState::initialize_scheduled(game, context, seed, start)
            } else {
                GameState::initialize(game, context, seed)
            },
            None,
        );
    }
    let mut invocation = deferred::InvocationGuard::new();
    let aliases = AliasContext::default();
    let saved_context = context.clone();
    let saved_seed = seed.clone();
    let saved_opening = opening.as_ref().map(|(entropy, _)| OpeningSetup {
        seed: saved_seed.clone(),
        entropy: entropy.clone(),
    });
    let method = if opening.is_some() {
        "GameState::initialize_opening"
    } else if start.is_some() {
        "GameState::initialize_scheduled"
    } else {
        "GameState::initialize"
    };
    let call = token("call");
    let before = call.as_ref().map(|call| format!("input-{call}"));
    remember(
        before
            .as_ref()
            .map_err(|error| error.to_string())
            .and_then(|point| {
                snapshot(
                    point,
                    Phase::InitializeInput,
                    method,
                    SnapshotOwner::InitializeInput {
                        context: Some(&saved_context),
                        seed: &saved_seed,
                        source_ref: "retained-actual-constructor-input",
                    },
                    &aliases,
                )
            }),
    );
    let result = if let Some((entropy, _)) = &opening {
        GameState::initialize_opening(
            game,
            context,
            OpeningSetup {
                seed,
                entropy: entropy.clone(),
            },
        )
    } else if let Some(start) = start {
        GameState::initialize_scheduled(game, context, seed, start)
    } else {
        GameState::initialize(game, context, seed)
    };
    let replayed = if let Some(setup) = &saved_opening {
        GameState::initialize_opening(game, saved_context.clone(), setup.clone())
    } else if let Some(start) = start {
        GameState::initialize_scheduled(game, saved_context.clone(), saved_seed.clone(), start)
    } else {
        GameState::initialize(game, saved_context.clone(), saved_seed.clone())
    };
    if let (Ok(call), Ok(before)) = (&call, &before) {
        let after = result.as_ref().ok().map(|_| format!("after-{call}"));
        remember(
            result
                .as_ref()
                .ok()
                .zip(after.as_deref())
                .map(|(result, after)| {
                    snapshot(
                        after,
                        Phase::AfterCall,
                        method,
                        SnapshotOwner::GameState(result.state()),
                        &aliases,
                    )
                })
                .unwrap_or(Ok(())),
        );
        let input = if let Some(setup) = &saved_opening {
            projection::QualificationCall::GameInitializeOpening {
                game,
                context: &saved_context,
                setup,
                entropy_words: &opening.as_ref().expect("saved opening").1,
            }
        } else if let Some(start) = start {
            projection::QualificationCall::GameInitializeScheduled {
                game,
                context: &saved_context,
                seed: &saved_seed,
                start,
            }
        } else {
            projection::QualificationCall::GameInitialize {
                game,
                context: &saved_context,
                seed: &saved_seed,
            }
        };
        remember(record_game_result(
            CallPoint {
                call_id: call,
                before_point: Some(before),
                after_point: after.as_deref(),
                adopted: adopt && result.is_ok(),
            },
            &input,
            &result,
            &replayed,
            None,
            None,
            &aliases,
        ));
    }
    let before = before.ok();
    let points = call
        .ok()
        .zip(before)
        .map(|(call, before)| RootCallPoints { call, before });
    invocation.returned();
    (result, points)
}
pub fn initialize(
    game: GameId,
    context: GameContext,
    seed: GameSeed,
) -> Result<NativeCandidate, GameError> {
    initialize_call(game, context, seed, None, None, true)
}
pub fn initialize_scheduled(
    game: GameId,
    context: GameContext,
    seed: GameSeed,
    start: TurnStart,
) -> Result<NativeCandidate, GameError> {
    initialize_call(game, context, seed, Some(start), None, true)
}
pub fn initialize_opening(
    game: GameId,
    context: GameContext,
    setup: OpeningSetup,
    entropy_words: Vec<u64>,
) -> Result<NativeCandidate, GameError> {
    initialize_call(
        game,
        context,
        setup.seed,
        None,
        Some((setup.entropy, entropy_words)),
        true,
    )
}

#[cfg(test)]
#[path = "current_capture/applicability_tests.rs"]
pub(crate) mod applicability_tests;
#[path = "current_capture/deferred.rs"]
pub mod deferred;

/// Observe an actual scheduled constructor whose result may be retained without adoption.
pub fn initialize_scheduled_with_adoption(
    game: GameId,
    context: GameContext,
    seed: GameSeed,
    start: TurnStart,
    adopt: bool,
) -> Result<NativeCandidate, GameError> {
    initialize_call(game, context, seed, Some(start), None, adopt)
}

/// Return the exact recorded constructor point to its existing scoped writer.
pub fn initialize_scheduled_recorded(
    game: GameId,
    context: GameContext,
    seed: GameSeed,
    start: TurnStart,
    adopt: bool,
) -> (Result<NativeCandidate, GameError>, Option<String>) {
    initialize_call_recorded(game, context, seed, Some(start), None, adopt)
}

/// Serialize existing typed observations, bind only explicit writer tokens and close.
pub fn finish_serializable(
    profile: &str,
    observations: &impl serde::Serialize,
) -> Result<Value, String> {
    let mut observations = serde_json::to_value(observations).map_err(|error| error.to_string())?;
    native_row_tokens(&mut observations, profile)?;
    super::capture_points::bind_value_points(&mut observations, "/observations")?;
    finish(profile, observations)
}

use spellwake_engine::{
    draw::LibraryDraw,
    object_identity::{
        CardId, GameObjects, ObjectChange, ObjectDestination, ObjectId, ObjectIdentityError,
    },
    priority::SeatId,
    shuffle::{EntropyTape, LibraryShuffle},
    turn::{PriorityCompleted, TurnError, TurnScheduler, WorkCompleted},
};
fn card_call<R: Eq + super::current_profile::applicability::NativeCaptureValue>(
    source: &GameObjects,
    input: &projection::QualificationCall<'_>,
    seats: &[SeatId],
    invoke: impl Fn(&GameObjects) -> Result<R, ObjectIdentityError>,
    successor: impl Fn(&R) -> &GameObjects,
    project: impl Fn(&R) -> Value,
    adopt: bool,
) -> Result<R, ObjectIdentityError> {
    if !is_active() {
        return invoke(source);
    }
    let aliases = AliasContext::default();
    let retained = source.clone();
    let call = native_call_id();
    let before = native_snapshot(
        Phase::BeforeCall,
        input.owner_method(),
        SnapshotOwner::GameObjects {
            cards: source,
            participants: seats,
        },
        &aliases,
    );
    let result = invoke(source);
    let replayed = invoke(source);
    if let (Ok(call), Ok(before)) = (&call, &before) {
        let after = result.as_ref().ok().map(|value| {
            native_snapshot(
                Phase::AfterCall,
                input.owner_method(),
                SnapshotOwner::GameObjects {
                    cards: successor(value),
                    participants: seats,
                },
                &aliases,
            )
        });
        let after = after.transpose();
        remember(
            after
                .as_ref()
                .map_err(|error| error.to_string())
                .and_then(|after| {
                    record_native(
                        CallPoint {
                            call_id: call,
                            before_point: Some(before),
                            after_point: after.as_deref(),
                            adopted: adopt && result.is_ok(),
                        },
                        input,
                        &result,
                        &replayed,
                        |value| {
                            Ok(match value {
                                Ok(value) => project(value),
                                Err(error) => projection::object_identity_error(error),
                            })
                        },
                        Append::NotApplicable,
                        ReplayEvidence {
                            source_eq: Some(*source == retained),
                            source_capture: SourceCapture::BeforeInvocation,
                        },
                        &aliases,
                        None,
                    )
                }),
        );
    } else {
        remember(call.map(|_| ()).and(before.map(|_| ())));
    }
    result
}
fn changed(value: &ObjectChange, seats: &[SeatId]) -> Value {
    serde_json::json!({"tag":"Ok","state":projection::card_objects(&value.next,seats),"before":value.before.as_ref().map(projection::object),"after":projection::object(&value.after)})
}
pub fn card_seed_card(
    source: &GameObjects,
    expected: u64,
    card: CardId,
    owner: SeatId,
    destination: ObjectDestination,
    seats: &[SeatId],
) -> Result<ObjectChange, ObjectIdentityError> {
    card_call(
        source,
        &projection::QualificationCall::GameObjectsCall(projection::GameObjectsCall::SeedCard {
            expected,
            card,
            owner,
            destination,
        }),
        seats,
        |source| source.seed_card(expected, card, owner, destination),
        |value| &value.next,
        |value| changed(value, seats),
        true,
    )
}
pub fn card_move(
    source: &GameObjects,
    expected: u64,
    object: ObjectId,
    destination: ObjectDestination,
    seats: &[SeatId],
) -> Result<spellwake_engine::object_identity::ZoneMoveResult, ObjectIdentityError> {
    card_call(
        source,
        &projection::QualificationCall::GameObjectsCall(
            projection::GameObjectsCall::ApplyZoneMove {
                expected,
                object,
                destination,
            },
        ),
        seats,
        |source| source.apply_zone_move(expected, object, destination),
        |value| &value.next,
        |value| serde_json::json!({"tag":"Ok","state":projection::card_objects(&value.next,seats),"outcome":projection::zone_move_outcome(&value.outcome)}),
        true,
    )
}
pub fn card_reorder(
    source: &GameObjects,
    expected: u64,
    owner: SeatId,
    order: Vec<ObjectId>,
    seats: &[SeatId],
) -> Result<GameObjects, ObjectIdentityError> {
    card_call(
        source,
        &projection::QualificationCall::GameObjectsCall(
            projection::GameObjectsCall::ReorderLibrary {
                expected,
                owner,
                order: &order,
            },
        ),
        seats,
        |source| source.reorder_library(expected, owner, order.clone()),
        |value| value,
        |value| serde_json::json!({"tag":"Ok","state":projection::card_objects(value,seats)}),
        true,
    )
}
pub fn card_draw(
    source: &GameObjects,
    expected: u64,
    player: SeatId,
    amount: spellwake_contracts::RulesInteger,
    seats: &[SeatId],
) -> Result<LibraryDraw, ObjectIdentityError> {
    card_call(
        source,
        &projection::QualificationCall::GameObjectsCall(projection::GameObjectsCall::DrawCards {
            expected,
            player,
            amount: &amount,
        }),
        seats,
        |source| source.draw_cards(expected, player, amount.clone()),
        |value| &value.next,
        |value| serde_json::json!({"tag":"Ok","state":projection::card_objects(&value.next,seats),"journal":projection::draws(&value.journal)}),
        true,
    )
}
pub fn card_shuffle(
    source: &GameObjects,
    expected: u64,
    owner: SeatId,
    entropy: &EntropyTape,
    entropy_words: &[u64],
    seats: &[SeatId],
    adopt: bool,
) -> Result<LibraryShuffle, ObjectIdentityError> {
    card_call(
        source,
        &projection::QualificationCall::GameObjectsCall(
            projection::GameObjectsCall::ShuffleLibrary {
                expected,
                owner,
                entropy,
                entropy_words,
            },
        ),
        seats,
        |source| source.shuffle_library(expected, owner, entropy),
        |value| &value.next,
        |value| serde_json::json!({"tag":"Ok","state":projection::card_objects(&value.next,seats),"record":projection::shuffle(&value.record),"entropy":{"position":value.entropy.position(),"remaining":value.entropy.remaining()}}),
        adopt,
    )
}
fn pure_construct<R: Eq + super::current_profile::applicability::NativeCaptureValue, E: Eq>(
    input: &projection::QualificationCall<'_>,
    input_value: &Value,
    invoke: impl Fn() -> Result<R, E>,
    owner: impl Fn(&R) -> Result<String, String>,
    project: impl Fn(&Result<R, E>) -> Result<Value, String>,
) -> Result<R, E> {
    if !is_active() {
        return invoke();
    }
    let aliases = AliasContext::default();
    let saved = input_value.clone();
    let call = native_call_id();
    let before = native_snapshot(
        Phase::PureInput,
        input.owner_method(),
        SnapshotOwner::PureValue(input_value),
        &aliases,
    );
    let result = invoke();
    let replayed = invoke();
    if let (Ok(call), Ok(before)) = (&call, &before) {
        let after = result.as_ref().ok().map(owner).transpose();
        remember(
            after
                .as_ref()
                .map_err(|error| error.to_string())
                .and_then(|after| {
                    record_native(
                        CallPoint {
                            call_id: call,
                            before_point: Some(before),
                            after_point: after.as_deref(),
                            adopted: result.is_ok(),
                        },
                        input,
                        &result,
                        &replayed,
                        &project,
                        Append::NotApplicable,
                        ReplayEvidence {
                            source_eq: Some(*input_value == saved),
                            source_capture: SourceCapture::BeforeInvocation,
                        },
                        &aliases,
                        None,
                    )
                }),
        );
    } else {
        remember(call.map(|_| ()).and(before.map(|_| ())));
    }
    result
}
pub fn card_new(revision: u64, seats: &[SeatId]) -> GameObjects {
    let input = serde_json::json!({"revision":revision.to_string()});
    let result: Result<GameObjects, std::convert::Infallible> = pure_construct(
        &projection::QualificationCall::GameObjectsInitialize { revision },
        &input,
        || Ok(GameObjects::new(revision)),
        |cards| {
            native_snapshot(
                Phase::PureResult,
                "GameObjects::new",
                SnapshotOwner::GameObjects {
                    cards,
                    participants: seats,
                },
                &AliasContext::default(),
            )
        },
        |result| {
            Ok(match result {
                Ok(cards) => {
                    serde_json::json!({"tag":"Ok","state":projection::card_objects(cards,seats)})
                }
                Err(error) => match *error {},
            })
        },
    );
    match result {
        Ok(cards) => cards,
        Err(error) => match error {},
    }
}
pub fn turn_new(start: TurnStart, skip_first_draw: bool) -> Result<TurnScheduler, TurnError> {
    let input = serde_json::json!({"start":{"number":start.number.to_string(),"step":projection::opaque(start.step)},"skip_first_draw":skip_first_draw});
    pure_construct(
        &projection::QualificationCall::TurnCall(projection::TurnCall::New {
            start,
            skip_first_draw,
        }),
        &input,
        || TurnScheduler::new(start, skip_first_draw),
        |state| {
            native_snapshot(
                Phase::PureResult,
                "TurnScheduler::new",
                SnapshotOwner::TurnScheduler(state),
                &AliasContext::default(),
            )
        },
        |result| {
            Ok(match result {
                Ok(value) => {
                    serde_json::json!({"tag":"Ok","state":projection::turn_scheduler(value)})
                }
                Err(error) => projection::turn_error(error),
            })
        },
    )
}
fn turn_call(
    source: &TurnScheduler,
    input: &projection::QualificationCall<'_>,
    invoke: impl Fn(&TurnScheduler) -> Result<TurnScheduler, TurnError>,
) -> Result<TurnScheduler, TurnError> {
    if !is_active() {
        return invoke(source);
    }
    let aliases = AliasContext::default();
    let retained = source.clone();
    let call = native_call_id();
    let before = native_snapshot(
        Phase::BeforeCall,
        input.owner_method(),
        SnapshotOwner::TurnScheduler(source),
        &aliases,
    );
    let result = invoke(source);
    let replayed = invoke(source);
    if let (Ok(call), Ok(before)) = (&call, &before) {
        let after = result
            .as_ref()
            .ok()
            .map(|state| {
                native_snapshot(
                    Phase::AfterCall,
                    input.owner_method(),
                    SnapshotOwner::TurnScheduler(state),
                    &aliases,
                )
            })
            .transpose();
        remember(after.as_ref().map_err(|error|error.to_string()).and_then(|after|record_native(CallPoint{call_id:call,before_point:Some(before),after_point:after.as_deref(),adopted:result.is_ok()},input,&result,&replayed,|value|Ok(match value{Ok(value)=>serde_json::json!({"tag":"Ok","state":projection::turn_scheduler(value)}),Err(error)=>projection::turn_error(error)}),Append::NotApplicable,ReplayEvidence{source_eq:Some(*source==retained),source_capture:SourceCapture::BeforeInvocation},&aliases,None)));
    } else {
        remember(call.map(|_| ()).and(before.map(|_| ())));
    }
    result
}
pub fn turn_work(
    source: &TurnScheduler,
    result: WorkCompleted,
) -> Result<TurnScheduler, TurnError> {
    turn_call(
        source,
        &projection::QualificationCall::TurnCall(projection::TurnCall::CompleteWork(result)),
        |source| source.complete_work(result),
    )
}
pub fn turn_priority(
    source: &TurnScheduler,
    result: PriorityCompleted,
) -> Result<TurnScheduler, TurnError> {
    turn_call(
        source,
        &projection::QualificationCall::TurnCall(projection::TurnCall::CompletePriority(result)),
        |source| source.complete_priority(result),
    )
}

/// Move only explicit source tokens from native subviews to the selected scoped row.
/// The finite paths are the current trace writers' declared before/after roles.
fn native_row_tokens(observations: &mut Value, profile: &str) -> Result<(), String> {
    if !["mana-abilities", "mana-color", "activated"].contains(&profile)
        || observations["status"] != "success"
    {
        return Ok(());
    }
    let probes = observations["data"]["native_observations"]
        .as_array_mut()
        .ok_or("current native probe list is missing")?;
    fn promote(row: &mut Value, children: &[&str]) -> Result<(), String> {
        let mut points = Vec::new();
        for child in children {
            let object = row
                .pointer_mut(child)
                .and_then(Value::as_object_mut)
                .ok_or("current native row child is missing")?;
            let token = object
                .remove("__current_capture_point")
                .ok_or("current native row child has no actual source token")?;
            points.push(
                token
                    .as_str()
                    .ok_or("current native child token is not a string")?
                    .to_owned(),
            );
        }
        let object = row
            .as_object_mut()
            .ok_or("current native row is not an object")?;
        if object
            .insert("__current_capture_point".into(), serde_json::json!(points))
            .is_some()
        {
            return Err("duplicate current native row source token".into());
        }
        Ok(())
    }
    for probe in probes {
        for field in if profile == "activated" {
            &["initialized", "initial"][..]
        } else {
            &["initial"][..]
        } {
            promote(
                probe
                    .get_mut(*field)
                    .ok_or("current native initial row missing")?,
                &["/snapshot"],
            )?;
        }
        let main = if profile == "mana-abilities" {
            "attempts"
        } else {
            "steps"
        };
        for row in probe[main]
            .as_array_mut()
            .ok_or("current native operation rows missing")?
        {
            promote(row, &["/before/snapshot", "/after/snapshot"])?;
        }
        if profile == "activated" {
            for row in probe["setup"]
                .as_array_mut()
                .ok_or("current native setup rows missing")?
            {
                promote(row, &["/before/snapshot", "/after/snapshot"])?;
            }
        }
        promote(
            probe
                .get_mut("followup")
                .ok_or("current native followup row missing")?,
            &["/before/snapshot", "/after/snapshot"],
        )?;
    }
    Ok(())
}

/// Arm the real observation-label overflow branch for the next recorder capture.
#[cfg(test)]
pub(crate) fn exhaust_next_observation_for_test() {
    EXHAUST_NEXT_OBSERVATION.with(|flag| assert!(!flag.replace(true)));
}
/// Inspect retained caller metadata without granting successful qualification.
#[cfg(test)]
pub(crate) fn caller_delivery_for_test() -> Value {
    active(|capture| capture.recorder.caller_delivery_value()).unwrap()
}

/// Synthetic source/publication metadata for observer mechanism tests only.
/// The caller supplies the real retained vocabulary; this grants no source-card
/// or genuine publication/image authority.
#[cfg(test)]
pub(crate) fn with_selection_observer_for_test<T>(
    vocabulary: &CreatureTypeVocabulary,
    operation: impl FnOnce() -> T,
) -> T {
    let source = |path: &str| SourceRef {
        path: path.into(),
        sha256: "0".repeat(64),
        bytes: 0,
    };
    let recorder = Recorder::new_scoped(
        CaptureScope::ResolutionSelection11,
        vocabulary,
        source("synthetic-ordinary-observer-input"),
        source("synthetic-ordinary-observer-publication"),
        vec![],
        RegistryWitness::NotApplicable,
    )
    .unwrap();
    ACTIVE.with(|active| {
        assert!(active.borrow().is_none());
        *active.borrow_mut() = Some(ActiveCapture {
            identity: capture_identity().unwrap(),
            profile: "ordinary-staged-unit".into(),
            recorder,
            points: vec![],
            calls: vec![],
            observations: BTreeMap::new(),
            record_pointers: vec![],
            sequence: 0,
            failure: None,
        });
    });
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with(|active| {
                active.borrow_mut().take();
            });
            EXHAUST_NEXT_OBSERVATION.with(|flag| flag.set(false));
        }
    }
    let _reset = Reset;
    operation()
}

#[cfg(test)]
mod publication_input_tests {
    use super::*;
    #[test]
    fn full_record_finisher_emits_current_report_epoch_and_preserves_record_layout() {
        // Synthetic metadata tests the actual finisher, not a gameplay qualification capture.
        let vocabulary = CreatureTypeVocabulary::new(
            "synthetic-full-record-envelope-unit".into(),
            [0; 32],
            vec!["Cat".into()],
        )
        .unwrap();
        let source = |path: &str| SourceRef {
            path: path.into(),
            sha256: "0".repeat(64),
            bytes: 0,
        };
        let recorder = Recorder::new(
            &vocabulary,
            source("synthetic-input"),
            source("synthetic-publication"),
            vec![],
            RegistryWitness::NotApplicable,
        )
        .unwrap();
        ACTIVE.with(|active| {
            assert!(active.borrow().is_none());
            *active.borrow_mut() = Some(ActiveCapture {
                identity: capture_identity().unwrap(),
                profile: "original".into(),
                recorder,
                points: vec![],
                calls: vec![],
                observations: BTreeMap::new(),
                record_pointers: vec![],
                sequence: 0,
                failure: None,
            });
        });
        let record = finish_record(
            "original",
            serde_json::json!({"schema_version":5,"original_field":["preserved"]}),
        )
        .unwrap();
        assert_eq!(record["schema_version"], 10);
        assert_eq!(record["original_field"], serde_json::json!(["preserved"]));
        assert_eq!(
            record["record_codec"],
            "spellwake.current-native-qualification.v2"
        );
        assert_eq!(
            record["type_context_witness"]["schema"],
            "spellwake.current-type-context-witness.v8"
        );
        assert!(!is_active());
    }
    #[test]
    fn input_candidates_and_incomplete_or_duplicate_envelopes_are_not_publications() {
        let vocabulary = CreatureTypeVocabulary::new(
            "synthetic-publication-admission-negative".into(),
            [0; 32],
            vec!["Cat".into()],
        )
        .unwrap();
        let candidate = serde_json::to_vec(&projection::vocabulary(&vocabulary)).unwrap();
        assert!(validate_publication(&candidate, &vocabulary).is_err());
        assert!(
            validate_publication(
                br#"{"status":"success","data":{"tag":"Incomplete","incomplete":{}}}"#,
                &vocabulary
            )
            .is_err()
        );
        assert!(
            validate_publication(
                br#"{"status":"success","status":"success","data":{}}"#,
                &vocabulary
            )
            .is_err()
        );
    }
    #[test]
    fn validation_suspension_restores_the_same_metadata_recorder_after_unwind() {
        let vocabulary = CreatureTypeVocabulary::new(
            "synthetic-suspension-unit".into(),
            [0; 32],
            vec!["Cat".into()],
        )
        .unwrap();
        let source = |path: &str| SourceRef {
            path: path.into(),
            sha256: "0".repeat(64),
            bytes: 0,
        };
        let recorder = Recorder::new(
            &vocabulary,
            source("synthetic-input"),
            source("synthetic-publication"),
            vec![],
            RegistryWitness::NotApplicable,
        )
        .unwrap();
        ACTIVE.with(|active| {
            assert!(active.borrow().is_none());
            *active.borrow_mut() = Some(ActiveCapture {
                identity: capture_identity().unwrap(),
                profile: "synthetic-unit".into(),
                recorder,
                points: vec![],
                calls: vec![],
                observations: BTreeMap::new(),
                record_pointers: vec![],
                sequence: 7,
                failure: None,
            });
        });
        let result = std::panic::catch_unwind(|| {
            without_capture(|| {
                assert!(!is_active());
                assert!(start("library", &[], None).is_err());
                without_capture(|| assert!(!is_active()));
                panic!("synthetic validation unwind");
            })
        });
        let recovered = ACTIVE.with(|active| active.borrow_mut().take()).unwrap();
        assert!(result.is_err());
        assert_eq!(recovered.profile, "synthetic-unit");
        assert_eq!(recovered.sequence, 7);
        assert_eq!(SUSPENSION_DEPTH.with(Cell::get), 0);
    }
}
