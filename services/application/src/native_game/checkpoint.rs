//! Private linked reconstruction manifests, never database acceptance or saved states.
use super::*;
use crate::player_wire::NativeRevision;
use serde::{Deserialize, Serialize};
const MAX_MANIFEST_BYTES: u64 = 32_768;
/// Safe checkpoint failure with original typed causes kept private.
pub enum NativeCheckpointError {
    /// Shape, linkage, original scope or basis is inconsistent.
    Invalid,
    /// Explicit per-attempt work budget is exhausted.
    Budget,
    /// Original manifest JSON is malformed.
    Json(serde_json::Error),
    /// Exact private artifact could not be read/retained.
    Artifact(NativeArtifactError),
    /// Original source/setup cannot be reconstructed.
    Initial(NativeInitialError),
    /// Retained original input cannot be reconstructed.
    Input(NativeCodecError),
    /// Actual state and accepted roster disagree.
    Binding(NativeBridgeError),
}
impl fmt::Display for NativeCheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "invalid native checkpoint",
            Self::Budget => "native checkpoint recovery budget exhausted",
            Self::Json(_) => "native checkpoint cannot be decoded",
            Self::Artifact(_) => "native checkpoint artifact is unavailable",
            Self::Initial(_) => "native checkpoint initial source is unavailable",
            Self::Input(_) => "native checkpoint input cannot be reconstructed",
            Self::Binding(_) => "native checkpoint binding is inconsistent",
        })
    }
}
impl fmt::Debug for NativeCheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for NativeCheckpointError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(e) => Some(e),
            Self::Artifact(e) => Some(e),
            Self::Initial(e) => Some(e),
            Self::Input(e) => Some(e),
            Self::Binding(e) => Some(e),
            _ => None,
        }
    }
}
impl From<NativeArtifactError> for NativeCheckpointError {
    fn from(e: NativeArtifactError) -> Self {
        Self::Artifact(e)
    }
}
impl From<NativeInitialError> for NativeCheckpointError {
    fn from(e: NativeInitialError) -> Self {
        Self::Initial(e)
    }
}
impl From<NativeCodecError> for NativeCheckpointError {
    fn from(e: NativeCodecError) -> Self {
        Self::Input(e)
    }
}
impl From<NativeBridgeError> for NativeCheckpointError {
    fn from(e: NativeBridgeError) -> Self {
        Self::Binding(e)
    }
}
/// Explicit trusted retained-input kind; records do not grant durable authority.
#[derive(Clone, Copy)]
pub enum NativeCheckpointInput {
    Player,
    Entropy,
    /// Private bank-expiry replay; SQL expiry authorization is separately required.
    BankExpiry,
    /// Private deterministic replay of a multiplayer response timeout.
    ResponseTimeout,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    Initial,
    Player,
    Entropy,
    BankExpiry,
    /// Private deterministic replay of a multiplayer response timeout.
    ResponseTimeout,
}
impl Kind {
    fn version(self) -> &'static str {
        match self {
            Self::BankExpiry => "native-bank-checkpoint-v1",
            Self::ResponseTimeout => "native-response-timeout-checkpoint-v1",
            Self::Initial | Self::Player | Self::Entropy => "native-checkpoint-v1",
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    version: String,
    initial_ref: NativeArtifactRef,
    #[serde(deserialize_with = "crate::provider_json::object")]
    basis: PlayerBasis,
    kind: Kind,
    #[serde(deserialize_with = "Option::deserialize")]
    parent_ref: Option<NativeArtifactRef>,
    #[serde(deserialize_with = "Option::deserialize")]
    input_ref: Option<NativeArtifactRef>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    viewer_seat_id: Option<Uuid>,
}
fn scoped(reference: &NativeArtifactRef, basis: &PlayerBasis, role: NativeArtifactRole) -> bool {
    reference.game_id() == basis.game_id
        && reference.context_id() == basis.context_id
        && reference.role() == role
}
impl Manifest {
    fn valid(&self) -> bool {
        self.version == self.kind.version()
            && scoped(&self.initial_ref, &self.basis, NativeArtifactRole::Initial)
            && match self.kind {
                Kind::Initial => {
                    self.basis.revision.get() == 0
                        && self.parent_ref.is_none()
                        && self.input_ref.is_none()
                        && self.viewer_seat_id.is_none()
                }
                Kind::Player | Kind::Entropy | Kind::BankExpiry | Kind::ResponseTimeout => {
                    self.basis.revision.get() > 0
                        && self
                            .parent_ref
                            .as_ref()
                            .is_some_and(|r| scoped(r, &self.basis, NativeArtifactRole::State))
                        && self
                            .input_ref
                            .as_ref()
                            .is_some_and(|r| scoped(r, &self.basis, NativeArtifactRole::Input))
                        && self.viewer_seat_id.is_some_and(|id| !id.is_nil())
                }
            }
    }
}
/// Independently accepted exact State artifact and native basis, not a host fence.
pub struct NativeCheckpointExpected {
    reference: NativeArtifactRef,
    basis: PlayerBasis,
    original_state: Option<NativeArtifactRef>,
}
impl NativeCheckpointExpected {
    /// Validate consistency only. Caller must resolve actual database acceptance.
    /// # Errors
    /// Rejects foreign scope/role and over-bound manifest references.
    pub fn new(
        reference: NativeArtifactRef,
        basis: PlayerBasis,
    ) -> Result<Self, NativeCheckpointError> {
        if !scoped(&reference, &basis, NativeArtifactRole::State)
            || reference.size() > MAX_MANIFEST_BYTES
        {
            return Err(NativeCheckpointError::Invalid);
        }
        Ok(Self {
            reference,
            basis,
            original_state: None,
        })
    }
    /// Bind the exact revision-zero State selected by an accepted original receipt.
    /// Callers with original receipt authority must use this anchor, rather than
    /// accepting an equivalent but independently selected initial manifest.
    /// # Errors
    /// Rejects wrong game/context/role or an over-bound original State reference.
    pub fn with_original_state(
        mut self,
        reference: NativeArtifactRef,
    ) -> Result<Self, NativeCheckpointError> {
        if !scoped(&reference, &self.basis, NativeArtifactRole::State)
            || reference.size() > MAX_MANIFEST_BYTES
        {
            return Err(NativeCheckpointError::Invalid);
        }
        self.original_state = Some(reference);
        Ok(self)
    }
}
/// Per-attempt work bounds, not a game-length policy. Counts initial State manifest.
/// Clone an unused template to start an independent request's recovery budget.
#[derive(Clone)]
pub struct NativeRecoveryBudget {
    records: usize,
    bytes: u64,
}
impl NativeRecoveryBudget {
    /// Construct explicit positive attempt limits.
    /// # Errors
    /// Rejects zero record/byte bounds.
    pub fn new(max_records: usize, max_total_bytes: u64) -> Result<Self, NativeCheckpointError> {
        if max_records == 0 || max_total_bytes == 0 {
            return Err(NativeCheckpointError::Budget);
        }
        Ok(Self {
            records: max_records,
            bytes: max_total_bytes,
        })
    }
    fn charge(
        &mut self,
        reference: &NativeArtifactRef,
        state: bool,
    ) -> Result<(), NativeCheckpointError> {
        self.charge_work(usize::from(state), reference.size())
    }
    /// One aggregate budget for `factor` independent attempts of this template.
    /// # Errors
    /// Rejects a zero factor and arithmetic overflow.
    pub(crate) fn scaled(&self, factor: usize) -> Result<Self, NativeCheckpointError> {
        let records = self
            .records
            .checked_mul(factor)
            .ok_or(NativeCheckpointError::Budget)?;
        let bytes = u64::try_from(factor)
            .ok()
            .and_then(|factor| self.bytes.checked_mul(factor))
            .ok_or(NativeCheckpointError::Budget)?;
        Self::new(records, bytes)
    }
    /// Remaining (records, bytes), for owned budget measurements.
    #[cfg(test)]
    pub(crate) fn remaining(&self) -> (usize, u64) {
        (self.records, self.bytes)
    }
    pub(super) fn remaining_bytes(&self) -> u64 {
        self.bytes
    }
    pub(super) fn charge_work(
        &mut self,
        records: usize,
        bytes: u64,
    ) -> Result<(), NativeCheckpointError> {
        let Some(remaining_records) = self.records.checked_sub(records) else {
            #[cfg(test)]
            println!("NATIVE_RECOVERY_BUDGET_EXHAUSTED records");
            return Err(NativeCheckpointError::Budget);
        };
        let Some(remaining_bytes) = self.bytes.checked_sub(bytes) else {
            #[cfg(test)]
            println!("NATIVE_RECOVERY_BUDGET_EXHAUSTED bytes");
            return Err(NativeCheckpointError::Budget);
        };
        self.records = remaining_records;
        self.bytes = remaining_bytes;
        Ok(())
    }
}
/// Actual fully reconstructed or prepared checkpoint. No DB acceptance is implied.
/// Original manifest bytes remain private; no Debug/Serialize is supplied.
pub struct NativeCheckpoint {
    bytes: Vec<u8>,
    reference: NativeArtifactRef,
    manifest: Manifest,
    candidate: NativeCandidate,
    pub(super) roster: Vec<NativeSeatBinding>,
    pub(super) activity: results::NativeActivity,
    provenance: Option<Arc<NativeInitialProvenance>>,
}
impl NativeCheckpoint {
    fn seal(
        manifest: Manifest,
        candidate: NativeCandidate,
        roster: Vec<NativeSeatBinding>,
        provenance: Option<Arc<NativeInitialProvenance>>,
        activity: results::NativeActivity,
    ) -> Result<Self, NativeCheckpointError> {
        if !manifest.valid() {
            return Err(NativeCheckpointError::Invalid);
        }
        let bytes = serde_json::to_vec(&manifest).map_err(NativeCheckpointError::Json)?;
        if u64::try_from(bytes.len()).map_err(|_| NativeCheckpointError::Invalid)?
            > MAX_MANIFEST_BYTES
        {
            return Err(NativeCheckpointError::Invalid);
        }
        let reference = NativeArtifactRef::for_bytes(
            manifest.basis.game_id,
            manifest.basis.context_id,
            NativeArtifactRole::State,
            &bytes,
        )?;
        Ok(Self {
            bytes,
            reference,
            manifest,
            candidate,
            roster,
            provenance,
            activity,
        })
    }
    /// Derive initial State manifest from the actual original opening proposal.
    /// # Errors
    /// Rejects inconsistent initial basis/manifest; no state is installed or retained.
    pub fn initial(opening: PreparedNativeOpening) -> Result<Self, NativeCheckpointError> {
        let provenance = opening.shared_provenance();
        let (_, initial_ref, candidate, roster) = opening.into_parts();
        let first = roster
            .first()
            .ok_or(NativeCheckpointError::Invalid)?
            .seat_id;
        let binding = NativeBinding::new(
            initial_ref.context_id(),
            candidate.state().basis().clone(),
            roster.clone(),
            first,
        )?;
        let activity = results::NativeActivity::initial(&candidate, &roster)?;
        Self::seal(
            Manifest {
                version: "native-checkpoint-v1".into(),
                initial_ref,
                basis: binding.public_basis,
                kind: Kind::Initial,
                parent_ref: None,
                input_ref: None,
                viewer_seat_id: None,
            },
            candidate,
            roster,
            provenance,
            activity,
        )
    }
    /// Frozen original physical-card provenance; absent for historical v1 roots.
    /// This does not authorize disclosure or replace native effective evaluation.
    pub fn provenance(&self) -> Option<&NativeInitialProvenance> {
        self.provenance.as_deref()
    }
    /// Exact private State content reference; not proof of database acceptance.
    pub fn reference(&self) -> &NativeArtifactRef {
        &self.reference
    }
    /// Original initialization root retained by this reconstruction manifest.
    pub fn initial_reference(&self) -> &NativeArtifactRef {
        &self.manifest.initial_ref
    }
    /// Exact retained parent/input coordinates for an executed linked proposal.
    pub fn retained_input(
        &self,
    ) -> Option<(
        NativeCheckpointInput,
        &NativeArtifactRef,
        &NativeArtifactRef,
    )> {
        let kind = match self.manifest.kind {
            Kind::Initial => return None,
            Kind::Player => NativeCheckpointInput::Player,
            Kind::Entropy => NativeCheckpointInput::Entropy,
            Kind::BankExpiry => NativeCheckpointInput::BankExpiry,
            Kind::ResponseTimeout => NativeCheckpointInput::ResponseTimeout,
        };
        Some((
            kind,
            self.manifest.parent_ref.as_ref()?,
            self.manifest.input_ref.as_ref()?,
        ))
    }
    /// Actual current native state, never a seat-safe public projection.
    pub fn state(&self) -> &GameState {
        self.candidate.state()
    }
    /// Actual last native candidate for trusted downstream fact processing.
    pub fn candidate(&self) -> &NativeCandidate {
        &self.candidate
    }
    /// Exact native basis derived from actual reconstruction.
    pub fn basis(&self) -> &PlayerBasis {
        &self.manifest.basis
    }
    /// Exact original manifest bytes for private retention only.
    pub fn original_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Bind one independently authenticated seat to the actual reconstructed state.
    /// # Errors
    /// Rejects foreign viewers or inconsistent roster.
    pub fn binding(&self, viewer: Uuid) -> Result<NativeBinding, NativeCheckpointError> {
        Ok(NativeBinding::new(
            self.manifest.basis.context_id,
            self.state().basis().clone(),
            self.roster.clone(),
            viewer,
        )?)
    }
    /// Retain exact State bytes with existing authenticated readback mechanics.
    /// # Errors
    /// Fails on missing private bucket, transport or exact-byte disagreement.
    pub fn retain(
        &self,
        store: &NativeArtifactStore,
    ) -> Result<NativeArtifactRef, NativeCheckpointError> {
        Ok(store.retain(
            self.manifest.basis.game_id,
            self.manifest.basis.context_id,
            NativeArtifactRole::State,
            &self.bytes,
        )?)
    }
    /// Construct next proposal by executing an exact already-retained original input.
    /// Caller still must atomically accept this proposal under actual SQL authority.
    /// # Errors
    /// Rejects wrong-role/scope/viewer, malformed/stale input and nonsequential native basis.
    pub fn append(
        &self,
        store: &NativeArtifactStore,
        input_ref: NativeArtifactRef,
        kind: NativeCheckpointInput,
        viewer: Uuid,
    ) -> Result<Self, NativeCheckpointError> {
        let (kind, candidate) = apply(store, self, &input_ref, kind, viewer)?;
        let binding = NativeBinding::new(
            self.manifest.basis.context_id,
            candidate.state().basis().clone(),
            self.roster.clone(),
            viewer,
        )?;
        let basis = binding.public_basis;
        if !next_basis(&self.manifest.basis, &basis) {
            return Err(NativeCheckpointError::Invalid);
        }
        let activity = self.activity.advance(self.state(), &candidate)?;
        Self::seal(
            Manifest {
                version: kind.version().into(),
                initial_ref: self.manifest.initial_ref.clone(),
                basis,
                kind,
                parent_ref: Some(self.reference.clone()),
                input_ref: Some(input_ref),
                viewer_seat_id: Some(viewer),
            },
            candidate,
            self.roster.clone(),
            self.provenance.clone(),
            activity,
        )
    }
}
fn next_basis(prior: &PlayerBasis, next: &PlayerBasis) -> bool {
    prior.game_id == next.game_id
        && prior.context_id == next.context_id
        && prior.epoch == next.epoch
        && prior.revision.get().checked_add(1) == Some(next.revision.get())
}
fn apply(
    store: &NativeArtifactStore,
    prior: &NativeCheckpoint,
    input: &NativeArtifactRef,
    kind: NativeCheckpointInput,
    viewer: Uuid,
) -> Result<(Kind, NativeCandidate), NativeCheckpointError> {
    if !scoped(input, prior.basis(), NativeArtifactRole::Input)
        || input.size()
            > u64::try_from(input_codec::MAX_RECORD_BYTES)
                .map_err(|_| NativeCheckpointError::Invalid)?
    {
        return Err(NativeCheckpointError::Invalid);
    }
    let binding = prior.binding(viewer)?;
    let artifact = store.read(
        input,
        prior.basis().game_id,
        prior.basis().context_id,
        NativeArtifactRole::Input,
    )?;
    match kind {
        NativeCheckpointInput::Player => Ok((
            Kind::Player,
            restore_input(prior.state(), binding, artifact.bytes())?
                .into_parts()
                .3,
        )),
        NativeCheckpointInput::Entropy => Ok((
            Kind::Entropy,
            restore_entropy(prior.state(), binding, artifact.bytes())?,
        )),
        NativeCheckpointInput::ResponseTimeout => Ok((
            Kind::ResponseTimeout,
            restore_response_timeout(prior.state(), binding, artifact.bytes())?,
        )),
        NativeCheckpointInput::BankExpiry => Ok((
            Kind::BankExpiry,
            restore_bank_expiry(prior.state(), binding, artifact.bytes())?,
        )),
    }
}
/// Recover the exact accepted tip from original setup and retained input chain.
/// No partial success, epoch rewriting, latest-source substitution or DB mutation.
/// # Errors
/// Rejects budget exhaustion, corrupt/missing/foreign artifacts, malformed/gapped/
/// cyclic manifests and actual source/input/basis inconsistencies.
pub fn load_checkpoint(
    store: &NativeArtifactStore,
    expected: &NativeCheckpointExpected,
    initial: &NativeInitialExpected,
    source: &NativeProgramSource,
    mut budget: NativeRecoveryBudget,
) -> Result<NativeCheckpoint, NativeCheckpointError> {
    let chain = read_state_chain(store, expected, initial, &mut budget)?;
    budget.charge(initial.reference(), false)?;
    let opening = load_initial(store, initial, source)?;
    replay_state_chain(store, expected, chain, opening, &mut budget, &mut |_, _| {
        Ok(())
    })
}
struct RetainedState {
    reference: NativeArtifactRef,
    bytes: Vec<u8>,
    manifest: Manifest,
}
fn read_state_chain(
    store: &NativeArtifactStore,
    expected: &NativeCheckpointExpected,
    initial: &NativeInitialExpected,
    budget: &mut NativeRecoveryBudget,
) -> Result<Vec<RetainedState>, NativeCheckpointError> {
    let mut reference = expected.reference.clone();
    let mut basis = expected.basis.clone();
    let mut seen = BTreeSet::new();
    let mut chain = Vec::new();
    loop {
        if reference.size() > MAX_MANIFEST_BYTES
            || !scoped(&reference, &basis, NativeArtifactRole::State)
            || !seen.insert(reference.as_str().to_owned())
        {
            return Err(NativeCheckpointError::Invalid);
        }
        budget.charge(&reference, true)?;
        let artifact = store.read(
            &reference,
            basis.game_id,
            basis.context_id,
            NativeArtifactRole::State,
        )?;
        let manifest: Manifest =
            crate::decode_wire_object(artifact.bytes()).map_err(NativeCheckpointError::Json)?;
        if !manifest.valid()
            || manifest.basis != basis
            || &manifest.initial_ref != initial.reference()
        {
            return Err(NativeCheckpointError::Invalid);
        }
        if manifest.kind == Kind::Initial {
            if expected
                .original_state
                .as_ref()
                .is_some_and(|original| original != &reference)
            {
                return Err(NativeCheckpointError::Invalid);
            }
            chain.push(RetainedState {
                reference,
                bytes: artifact.into_bytes(),
                manifest,
            });
            break;
        }
        reference = manifest
            .parent_ref
            .as_ref()
            .ok_or(NativeCheckpointError::Invalid)?
            .clone();
        basis.revision = NativeRevision::from(
            basis
                .revision
                .get()
                .checked_sub(1)
                .ok_or(NativeCheckpointError::Invalid)?,
        );
        chain.push(RetainedState {
            reference: artifact.reference().clone(),
            bytes: artifact.into_bytes(),
            manifest,
        });
    }
    Ok(chain)
}
fn replay_state_chain<F>(
    store: &NativeArtifactStore,
    expected: &NativeCheckpointExpected,
    mut chain: Vec<RetainedState>,
    opening: PreparedNativeOpening,
    budget: &mut NativeRecoveryBudget,
    observer: &mut F,
) -> Result<NativeCheckpoint, NativeCheckpointError>
where
    F: FnMut(&NativeCheckpoint, &mut NativeRecoveryBudget) -> Result<(), NativeCheckpointError>,
{
    let mut checkpoint = NativeCheckpoint::initial(opening)?;
    let RetainedState {
        reference: root_ref,
        bytes: root_bytes,
        manifest: root_manifest,
    } = chain.pop().ok_or(NativeCheckpointError::Invalid)?;
    if checkpoint.basis() != &root_manifest.basis {
        return Err(NativeCheckpointError::Invalid);
    }
    // Preserve original verified bytes/ref, including harmless JSON whitespace.
    checkpoint.reference = root_ref;
    checkpoint.bytes = root_bytes;
    checkpoint.manifest = root_manifest;
    observer(&checkpoint, budget)?;
    for RetainedState {
        reference,
        bytes,
        manifest,
    } in chain.into_iter().rev()
    {
        let input = manifest
            .input_ref
            .as_ref()
            .ok_or(NativeCheckpointError::Invalid)?;
        budget.charge(input, false)?;
        let kind = match manifest.kind {
            Kind::Player => NativeCheckpointInput::Player,
            Kind::Entropy => NativeCheckpointInput::Entropy,
            Kind::BankExpiry => NativeCheckpointInput::BankExpiry,
            Kind::ResponseTimeout => NativeCheckpointInput::ResponseTimeout,
            Kind::Initial => return Err(NativeCheckpointError::Invalid),
        };
        let viewer = manifest
            .viewer_seat_id
            .ok_or(NativeCheckpointError::Invalid)?;
        let (_, candidate) = apply(store, &checkpoint, input, kind, viewer)?;
        let actual = NativeBinding::new(
            manifest.basis.context_id,
            candidate.state().basis().clone(),
            checkpoint.roster.clone(),
            viewer,
        )?;
        if actual.public_basis != manifest.basis
            || manifest.parent_ref.as_ref() != Some(&checkpoint.reference)
            || !next_basis(checkpoint.basis(), &manifest.basis)
        {
            return Err(NativeCheckpointError::Invalid);
        }
        let activity = checkpoint
            .activity
            .advance(checkpoint.state(), &candidate)?;
        checkpoint = NativeCheckpoint {
            activity,
            bytes,
            reference,
            manifest,
            candidate,
            roster: checkpoint.roster,
            provenance: checkpoint.provenance,
        };
        observer(&checkpoint, budget)?;
    }
    if checkpoint.reference != expected.reference || checkpoint.basis() != &expected.basis {
        return Err(NativeCheckpointError::Invalid);
    }
    Ok(checkpoint)
}

/// Reuse one already-loaded, independently checked initialization and one replay
/// walk; observers see original State bytes/refs before the next candidate.
/// The caller charges the initial artifact before loading it.
pub(super) fn load_checkpoint_observed<F>(
    store: &NativeArtifactStore,
    expected: &NativeCheckpointExpected,
    initial: &NativeInitialExpected,
    opening: PreparedNativeOpening,
    budget: &mut NativeRecoveryBudget,
    observer: &mut F,
) -> Result<NativeCheckpoint, NativeCheckpointError>
where
    F: FnMut(&NativeCheckpoint, &mut NativeRecoveryBudget) -> Result<(), NativeCheckpointError>,
{
    if opening.reference() != initial.reference() {
        return Err(NativeCheckpointError::Invalid);
    }
    let chain = read_state_chain(store, expected, initial, budget)?;
    replay_state_chain(store, expected, chain, opening, budget, observer)
}

#[cfg(test)]
mod tests;
