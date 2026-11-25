//! Source-derived participant snapshots. This is not Auth, source admission,
//! historical replay authority, persistence or HTTP delivery.
use super::*;
use crate::{amount::Amount, player_wire::*, source_protocol::parse_utc_microsecond};
use serde::{Deserialize, Serialize};
use spellwake_contracts::creature_types::{CreatureTypeVocabulary, CreatureVocabularyId};
use spellwake_engine::{
    activated::ActivatedAbilityId,
    characteristics::{EvaluatedCharacteristics, ObjectCharacteristics, StaticAbilityId},
    effects::Program,
    mana::{ManaAbilityRef, ManaType},
    object_identity::{GameObject, ObjectId, ObjectZone},
    occurrence::{ExecutionBindings, WaitingInstance},
    priority::PriorityPhase,
    program::{ExecutableBody, TriggeredAbilityId},
    stack_window::{StackEntry, StackKind, WindowPhase},
    token::TokenEntryPreview,
};
use std::collections::BTreeSet;
fn activated_kind(
    ability: Option<&spellwake_engine::activated::ActivatedAbility>,
) -> PlayerAbilityKind {
    use spellwake_engine::{
        activated::{HandDiscardKeyword, PowerAbility, SourceCost},
        program::BodyTarget,
    };
    match ability.map(|a| (a.source_cost, a.body.target)) {
        Some((
            SourceCost::Power {
                kind: PowerAbility::Crew,
                ..
            },
            _,
        )) => PlayerAbilityKind::Crew,
        Some((cost, _)) if cost.uses_contributors_origin() && cost.power_cost().is_none() => {
            PlayerAbilityKind::TapChosenPermanent
        }
        Some((
            SourceCost::Power {
                kind: PowerAbility::Station,
                ..
            },
            _,
        )) => PlayerAbilityKind::Station,
        Some((
            SourceCost::Power {
                kind: PowerAbility::Saddle,
                ..
            },
            _,
        )) => PlayerAbilityKind::Saddle,
        Some((SourceCost::HandDiscardSelf(HandDiscardKeyword::Cycling), _)) => {
            PlayerAbilityKind::Cycling
        }
        Some((SourceCost::Ninjutsu { .. }, _)) => PlayerAbilityKind::Ninjutsu,
        Some((SourceCost::HandDiscardSelf(HandDiscardKeyword::Transmute), _)) => {
            PlayerAbilityKind::Transmute
        }
        Some((_, BodyTarget::None)) => PlayerAbilityKind::ActivatedUntargeted,
        _ => PlayerAbilityKind::Activated,
    }
}
mod decisions;
mod values;
use values::*;

/// Trusted current-state disclosure, never a client-supplied authorization flag.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NativeDisclosure {
    LiveParticipant,
    /// Requires an actual current native terminal outcome as well as the binding.
    /// Historical full-reveal archives require their separate host authority.
    CompletedParticipant,
}
/// Accepted host metadata. Scalar validity does not establish its provenance.
pub struct NativeProjectionMetadata {
    pub through_event_sequence: Amount,
    pub deadline_at: Option<String>,
    pub disclosure: NativeDisclosure,
}
/// Exact borrowed accepted cut authorized by independently verified completion.
/// Private fields and constructor prevent candidate bytes from becoming a permit.
pub(super) struct HistoricalProjectionPermit<'a> {
    source: &'a GameState,
    binding: NativeBinding,
    through: Amount,
}
impl<'a> HistoricalProjectionPermit<'a> {
    pub(super) fn from_cut(
        authority: &super::source_reader::HistoricalCompletionAuthority,
        cut: &super::source_reader::HistoricalCut<'a>,
    ) -> Result<Self, NativeProjectionError> {
        if !authority.permits(cut) {
            return Err(NativeProjectionError::NotCompleted);
        }
        let (source, binding, through) = cut.projection_source();
        Ok(Self {
            source,
            binding: binding.clone(),
            through: through.clone(),
        })
    }
    fn matches(&self, scope: &NativeScope<'_>, metadata: &NativeProjectionMetadata) -> bool {
        std::ptr::eq(self.source, scope.source)
            && self.binding.basis == scope.binding.basis
            && self.binding.public_basis == scope.binding.public_basis
            && self.binding.seats == scope.binding.seats
            && self.binding.viewer == scope.binding.viewer
            && self.binding.actor == scope.binding.actor
            && self.through == metadata.through_event_sequence
            && metadata.disclosure == NativeDisclosure::CompletedParticipant
    }
}
/// Actual projection and the independent metadata used to validate it.
///
/// Constructed only by native projection, never decoded from candidate bytes.
/// The native State stays borrowed by the scope; this owner retains only public
/// values and small source-derived membership/reference metadata. It grants no
/// participant authority or archive/source qualification by itself.
pub struct NativeProjectedSnapshot {
    snapshot: PlayerSnapshot,
    roster: Vec<Uuid>,
    visible_objects: Vec<PlayerHandle>,
    known_identities: Vec<PlayerHandle>,
    ordered_zones: Vec<PlayerHandle>,
    ability_bindings: Vec<PlayerAbilityBinding>,
    full_disclosure: bool,
    vocabulary_id: CreatureVocabularyId,
}
impl NativeProjectedSnapshot {
    /// Borrow the actual projected values without mutable access to their facts.
    pub fn snapshot(&self) -> &PlayerSnapshot {
        &self.snapshot
    }
    /// Discard validation metadata and move only the public snapshot.
    pub fn into_snapshot(self) -> PlayerSnapshot {
        self.snapshot
    }
    /// Borrow independently captured facts with the original typed vocabulary.
    /// The caller must obtain that vocabulary from the qualified original source;
    /// identity equality is consistency, not evidence of rules completeness.
    /// # Errors
    /// Rejects a different vocabulary instead of reinterpreting historical labels.
    pub fn facts<'a>(
        &'a self,
        vocabulary: &'a CreatureTypeVocabulary,
    ) -> Result<PlayerProjectionFacts<'a>, NativeProjectionError> {
        if vocabulary.id() != self.vocabulary_id {
            return Err(NativeProjectionError::InvalidMetadata);
        }
        Ok(PlayerProjectionFacts {
            basis: &self.snapshot.basis,
            viewer: self.snapshot.viewer_seat_id,
            roster: &self.roster,
            through_sequence: &self.snapshot.through_event_sequence,
            visible_objects: &self.visible_objects,
            known_identities: &self.known_identities,
            ordered_zones: &self.ordered_zones,
            authorized_full_disclosure: self.full_disclosure,
            creature_vocabulary: Some(vocabulary),
            ability_bindings: &self.ability_bindings,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchivedBinding {
    source: PlayerHandle,
    handle: PlayerHandle,
    kind: PlayerAbilityKind,
}
/// Retained private encoding of one SQL-winning projection and the independent
/// metadata captured when it was projected. Only the portable replay archive
/// stores it; a decoded value is restored, never relabelled as native output.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::native_game) struct ArchivedProjection {
    #[serde(deserialize_with = "crate::provider_json::object")]
    snapshot: PlayerSnapshot,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    roster: Vec<Uuid>,
    visible_objects: Vec<PlayerHandle>,
    known_identities: Vec<PlayerHandle>,
    ordered_zones: Vec<PlayerHandle>,
    ability_bindings: Vec<ArchivedBinding>,
    full_disclosure: bool,
    vocabulary_id: CreatureVocabularyId,
}
impl ArchivedProjection {
    /// Borrow the retained public values for comparison with the independent
    /// SQL-winning original View before restoring archive validation metadata.
    pub(in crate::native_game) fn snapshot(&self) -> &PlayerSnapshot {
        &self.snapshot
    }
}
/// Archived projection decoded from hash-verified published bytes. Callers must
/// still validate it against the archive's original vocabulary via facts().
pub(in crate::native_game) struct RestoredProjection {
    snapshot: PlayerSnapshot,
    roster: Vec<Uuid>,
    visible_objects: Vec<PlayerHandle>,
    known_identities: Vec<PlayerHandle>,
    ordered_zones: Vec<PlayerHandle>,
    ability_bindings: Vec<PlayerAbilityBinding>,
    full_disclosure: bool,
    vocabulary_id: CreatureVocabularyId,
}
impl NativeProjectedSnapshot {
    /// Copy the public values and validation metadata for private retention.
    pub(in crate::native_game) fn archive(&self) -> ArchivedProjection {
        ArchivedProjection {
            snapshot: self.snapshot.clone(),
            roster: self.roster.clone(),
            visible_objects: self.visible_objects.clone(),
            known_identities: self.known_identities.clone(),
            ordered_zones: self.ordered_zones.clone(),
            ability_bindings: self
                .ability_bindings
                .iter()
                .map(|binding| ArchivedBinding {
                    source: binding.source.clone(),
                    handle: binding.handle.clone(),
                    kind: binding.kind,
                })
                .collect(),
            full_disclosure: self.full_disclosure,
            vocabulary_id: self.vocabulary_id,
        }
    }
}
impl ArchivedProjection {
    pub(in crate::native_game) fn restore(self) -> RestoredProjection {
        RestoredProjection {
            snapshot: self.snapshot,
            roster: self.roster,
            visible_objects: self.visible_objects,
            known_identities: self.known_identities,
            ordered_zones: self.ordered_zones,
            ability_bindings: self
                .ability_bindings
                .into_iter()
                .map(|binding| PlayerAbilityBinding {
                    source: binding.source,
                    handle: binding.handle,
                    kind: binding.kind,
                })
                .collect(),
            full_disclosure: self.full_disclosure,
            vocabulary_id: self.vocabulary_id,
        }
    }
}
impl RestoredProjection {
    pub(in crate::native_game) fn snapshot(&self) -> &PlayerSnapshot {
        &self.snapshot
    }
    pub(in crate::native_game) fn full_disclosure(&self) -> bool {
        self.full_disclosure
    }
    /// # Errors
    /// Rejects a vocabulary other than the one recorded at projection time.
    pub(in crate::native_game) fn facts<'a>(
        &'a self,
        vocabulary: &'a CreatureTypeVocabulary,
    ) -> Result<PlayerProjectionFacts<'a>, NativeProjectionError> {
        if vocabulary.id() != self.vocabulary_id {
            return Err(NativeProjectionError::InvalidMetadata);
        }
        Ok(PlayerProjectionFacts {
            basis: &self.snapshot.basis,
            viewer: self.snapshot.viewer_seat_id,
            roster: &self.roster,
            through_sequence: &self.snapshot.through_event_sequence,
            visible_objects: &self.visible_objects,
            known_identities: &self.known_identities,
            ordered_zones: &self.ordered_zones,
            authorized_full_disclosure: self.full_disclosure,
            creature_vocabulary: Some(vocabulary),
            ability_bindings: &self.ability_bindings,
        })
    }
}
/// Immutable source description for a currently visible object.
/// No visibility, effective numeric truth or qualification is supplied here.
pub struct NativeObjectDisplay {
    pub definition_id: Option<Uuid>,
    pub printing_id: Option<Uuid>,
    pub custom_card_revision_id: Option<Uuid>,
    pub characteristics: PlayerCharacteristics,
}
/// Typed description role; not a serializable native ID or action permission.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NativeAbilityReference {
    Mana(ManaAbilityRef),
    Activated(ActivatedAbilityId),
    Triggered(TriggeredAbilityId),
    Continuous(StaticAbilityId),
}
/// Scope of immutable descriptions, separate from participant authorization.
/// A rules context alone cannot identify selected physical cards across games.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NativeDisplayScope {
    /// Descriptions shared by the rules source, without selected row identities.
    SourceOnly,
    /// Physical identities retained by one game's immutable Initial artifact.
    Game { game_id: Uuid, context_id: Uuid },
}
/// Required immutable source descriptions. Implementations must retain the
/// accepted source context; no latest-data, Debug or guessed-text fallback.
/// These methods may fail when the admitted source lacks a required description.
/// They never decide visibility, current arithmetic or native legality.
pub trait NativeDisplaySource {
    /// Exact immutable context used by this description source. Equality is a
    /// consistency check, not admission or independent provenance qualification.
    fn context(&self) -> &spellwake_engine::game::GameContext;
    /// Explicit physical-identity scope. This is consistency, not authorization.
    fn scope(&self) -> NativeDisplayScope;
    /// Describe only the actual visible physical origin and its copiable definition.
    /// # Errors
    /// Return a typed projection failure for missing or inconsistent source metadata.
    fn object(
        &self,
        object: &GameObject,
        definition: &Program,
    ) -> Result<NativeObjectDisplay, NativeProjectionError>;
    /// Describe one currently represented ability on a visible source.
    /// # Errors
    /// Reject missing or inconsistent source labels.
    fn ability(
        &self,
        object: &GameObject,
        current: &ObjectCharacteristics,
        ability: NativeAbilityReference,
    ) -> Result<String, NativeProjectionError>;
    /// Describe an actual public stack role, including copies/ability bindings.
    /// The body/bindings are privileged inputs, never serialized by the projector.
    /// # Errors
    /// Reject unavailable source descriptions rather than inventing a label.
    fn stack(
        &self,
        entry: &StackEntry,
        body: Option<&ExecutableBody>,
        bindings: Option<&ExecutionBindings>,
    ) -> Result<String, NativeProjectionError>;
    /// Describe an actual pending newcomer without inventing a physical ObjectId.
    /// # Errors
    /// Reject missing or inconsistent preview metadata.
    fn token_preview(&self, preview: &TokenEntryPreview) -> Result<String, NativeProjectionError>;
    /// Describe an actual pending trigger and its public captured appearance/body.
    /// # Errors
    /// Reject missing or inconsistent trigger metadata.
    fn trigger(&self, instance: &WaitingInstance) -> Result<String, NativeProjectionError>;
    /// Describe one exact eligible public damage-source occurrence, including LKI
    /// or spell-copy provenance without requiring a current physical incarnation.
    /// # Errors
    /// Reject missing or inconsistent immutable source descriptions.
    fn prevention_attempt(
        &self,
        attempt: &spellwake_engine::effects::DamageAllocationDomain,
    ) -> Result<String, NativeProjectionError>;
}
/// Safe projection failure; private causes remain inspectable through Error::source.
pub enum NativeProjectionError {
    Bridge(NativeBridgeError),
    Metadata(Box<dyn Error + Send + Sync>),
    MissingDisplay,
    InvalidMetadata,
    NotCompleted,
    MissingNativeValue,
    HiddenReference,
    InvalidSnapshot,
}
impl NativeProjectionError {
    /// Preserve an underlying source failure without exposing its message.
    pub fn metadata(error: impl Error + Send + Sync + 'static) -> Self {
        Self::Metadata(Box::new(error))
    }
}
impl fmt::Display for NativeProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bridge(_) => "native projection is unavailable",
            Self::Metadata(_) | Self::MissingDisplay => "native display source is unavailable",
            Self::InvalidMetadata => "native projection metadata is invalid",
            Self::NotCompleted => "native disclosure is unavailable",
            Self::MissingNativeValue => "native projection source is inconsistent",
            Self::HiddenReference => "native reference is not visible",
            Self::InvalidSnapshot => "native projection is invalid",
        })
    }
}
impl fmt::Debug for NativeProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for NativeProjectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bridge(cause) => Some(cause),
            Self::Metadata(cause) => Some(cause.as_ref()),
            _ => None,
        }
    }
}
impl From<NativeBridgeError> for NativeProjectionError {
    fn from(value: NativeBridgeError) -> Self {
        Self::Bridge(value)
    }
}

impl NativeScope<'_> {
    /// Build a participant snapshot and atomically retain its typed handle registry.
    /// The borrowed source is never changed. Only small binding/registry metadata
    /// is cloned for staging; explicit native CopyTargets probes own ephemeral candidates.
    /// # Errors
    /// Rejects invalid host metadata, live full disclosure, missing display/native
    /// values, unsupported public references and inconsistent/oversize snapshots.
    /// Every failure preserves the original source and issued registry.
    pub fn project(
        &mut self,
        metadata: &NativeProjectionMetadata,
        display: &dyn NativeDisplaySource,
    ) -> Result<PlayerSnapshot, NativeProjectionError> {
        self.project_with_facts(metadata, display)
            .map(NativeProjectedSnapshot::into_snapshot)
    }
    /// Project and retain the actual independent validation metadata.
    /// Uses the same atomic registry staging and disclosure checks as project().
    /// # Errors
    /// Returns the same source/display/visibility/projection errors as project().
    pub fn project_with_facts(
        &mut self,
        metadata: &NativeProjectionMetadata,
        display: &dyn NativeDisplaySource,
    ) -> Result<NativeProjectedSnapshot, NativeProjectionError> {
        self.project_checked(metadata, display, None)
    }
    pub(super) fn project_historical(
        &mut self,
        metadata: &NativeProjectionMetadata,
        display: &dyn NativeDisplaySource,
        permit: &HistoricalProjectionPermit<'_>,
    ) -> Result<NativeProjectedSnapshot, NativeProjectionError> {
        self.project_checked(metadata, display, Some(permit))
    }
    fn project_checked(
        &mut self,
        metadata: &NativeProjectionMetadata,
        display: &dyn NativeDisplaySource,
        permit: Option<&HistoricalProjectionPermit<'_>>,
    ) -> Result<NativeProjectedSnapshot, NativeProjectionError> {
        if permit.is_some_and(|permit| !permit.matches(self, metadata)) {
            return Err(NativeProjectionError::InvalidMetadata);
        }
        if display.context() != &self.source.basis().context {
            return Err(NativeProjectionError::InvalidMetadata);
        }
        if let NativeDisplayScope::Game {
            game_id,
            context_id,
        } = display.scope()
            && (game_id != self.binding.public_basis.game_id
                || context_id != self.binding.public_basis.context_id)
        {
            return Err(NativeProjectionError::InvalidMetadata);
        }
        if metadata
            .deadline_at
            .as_ref()
            .is_some_and(|value| parse_utc_microsecond(value).is_none())
        {
            return Err(NativeProjectionError::InvalidMetadata);
        }
        if metadata.disclosure == NativeDisclosure::CompletedParticipant
            && self.source.outcome().is_none()
            && permit.is_none()
        {
            return Err(NativeProjectionError::NotCompleted);
        }
        let evaluated = self
            .source
            .execution()
            .evaluated_characteristics()
            .map_err(|cause| NativeBridgeError::Observation(Box::new(cause)))?;
        let mut staged = NativeScope {
            source: self.source,
            binding: self.binding.clone(),
            references: self.references.clone(),
            used_handles: self.used_handles.clone(),
            identity: std::sync::Arc::clone(&self.identity),
        };
        let builder = Projection {
            scope: &mut staged,
            evaluated,
            metadata,
            display,
            objects: vec![],
            visible: BTreeSet::new(),
            known: vec![],
            ordered: vec![],
            abilities: vec![],
            stack: vec![],
        };
        let mut projected = builder.build()?;
        populate_cast_offers(&staged, metadata, &mut projected)?;
        self.references = staged.references;
        self.used_handles = staged.used_handles;
        Ok(projected)
    }
}

#[inline(never)]
fn populate_cast_offers(
    scope: &NativeScope<'_>,
    metadata: &NativeProjectionMetadata,
    projected: &mut NativeProjectedSnapshot,
) -> Result<(), NativeProjectionError> {
    if metadata.disclosure != NativeDisclosure::LiveParticipant
        || scope.source.outcome().is_some()
        || !projected.snapshot.decisions.is_empty()
        || !projected.snapshot.turn.as_ref().is_some_and(|t| {
            t.priority_seat_ids
                .contains(&projected.snapshot.viewer_seat_id)
        })
    {
        return Ok(());
    }
    let affordances =
        crate::ai_runtime::affordances::probe_priority_affordances(scope, &projected.snapshot);
    let mut offers = Vec::with_capacity(affordances.castable_spells.len());
    for spell in affordances.castable_spells {
        let mut seen_pay = BTreeSet::new();
        let pay_sources: Vec<PlayerHandle> = spell
            .pay_sources
            .into_iter()
            .filter(|handle| seen_pay.insert(handle.clone()))
            .collect();
        offers.push(crate::player_wire::PlayerCastOffer {
            source: spell.source,
            targets: if spell.untargeted {
                None
            } else {
                Some(spell.legal_targets)
            },
            cost: spell.cost_text,
            pay_sources,
            payment: spell.payment,
        });
    }
    projected.snapshot.cast_offers = Some(offers);
    let facts = projected.facts(scope.source.execution().creature_vocabulary())?;
    projected
        .snapshot
        .validate(&facts)
        .map_err(|_| NativeProjectionError::InvalidSnapshot)
}
struct Projection<'a, 's> {
    scope: &'a mut NativeScope<'s>,
    evaluated: EvaluatedCharacteristics,
    metadata: &'a NativeProjectionMetadata,
    display: &'a dyn NativeDisplaySource,
    objects: Vec<PlayerObject>,
    visible: BTreeSet<ObjectId>,
    known: Vec<PlayerHandle>,
    ordered: Vec<PlayerHandle>,
    abilities: Vec<PlayerAbilityBinding>,
    stack: Vec<PlayerStackEntry>,
}
impl Projection<'_, '_> {
    fn issue(&mut self, reference: NativeReference) -> Result<PlayerHandle, NativeProjectionError> {
        Ok(self.scope.issue_evaluated(reference, &self.evaluated)?)
    }
    fn app_seat(&self, native: SeatId) -> Result<Uuid, NativeProjectionError> {
        self.scope
            .binding
            .seats
            .iter()
            .find(|entry| entry.native_seat == native)
            .map(|entry| entry.seat_id)
            .ok_or(NativeProjectionError::MissingNativeValue)
    }
    fn visible_object(&mut self, native: ObjectId) -> Result<PlayerHandle, NativeProjectionError> {
        if !self.visible.contains(&native) {
            return Err(NativeProjectionError::HiddenReference);
        }
        self.issue(NativeReference::Object(native))
    }
    fn target(
        &mut self,
        native: spellwake_engine::targets::TargetRef,
    ) -> Result<PlayerTarget, NativeProjectionError> {
        use spellwake_engine::targets::TargetRef;
        match native {
            TargetRef::Object(id) => Ok(PlayerTarget::Object {
                handle: self.visible_object(id)?,
            }),
            TargetRef::Stack(id) => Ok(PlayerTarget::Stack {
                handle: self.issue(NativeReference::Stack(id))?,
            }),
            TargetRef::Player(seat) => Ok(PlayerTarget::Seat {
                seat_id: self.app_seat(seat)?,
            }),
        }
    }
    fn build(mut self) -> Result<NativeProjectedSnapshot, NativeProjectionError> {
        let source = self.scope.source;
        let roster: Vec<_> = self
            .scope
            .binding
            .seats
            .iter()
            .map(|entry| entry.seat_id)
            .collect();
        let seats: Vec<_> = self
            .scope
            .binding
            .seats
            .iter()
            .map(|entry| entry.native_seat)
            .collect();
        let mut zones = vec![];
        for seat in &seats {
            for zone in [
                ObjectZone::Library(*seat),
                ObjectZone::Hand(*seat),
                ObjectZone::Graveyard(*seat),
            ] {
                zones.push(self.zone(zone)?);
            }
        }
        for zone in [
            ObjectZone::Battlefield,
            ObjectZone::Stack,
            ObjectZone::Exile,
            ObjectZone::Command,
        ] {
            zones.push(self.zone(zone)?);
        }
        self.objects
            .sort_by(|left, right| left.handle.cmp(&right.handle));
        let mut players = vec![];
        for native in seats {
            let cards = source.execution().window().cards();
            let mut search_reveals: Vec<_> = source
                .search_reveals()
                .into_iter()
                .filter(|(card, _)| card.owner() == native)
                .map(|(card, program)| {
                    self.display
                        .object(card, program)
                        .map(|display| display.characteristics)
                })
                .collect::<Result<_, _>>()?;
            let search_reveal = search_reveals.first().cloned();
            if search_reveals.len() <= 1 {
                search_reveals.clear();
            }
            let cascade_reveals = source
                .cascade_reveals()
                .filter(|(object, _)| object.owner() == native)
                .map(|(object, face)| {
                    self.display
                        .object(object, face)
                        .map(|value| value.characteristics)
                })
                .collect::<Result<_, _>>()?;
            let face_down_reveals = source
                .face_down_reveals()
                .filter(|(object, _)| object.owner() == native)
                .map(|(object, face)| {
                    self.display
                        .object(object, face)
                        .map(|value| value.characteristics)
                })
                .collect::<Result<_, _>>()?;
            let phased_out = source
                .execution()
                .phased_permanents()
                .filter(|(object, _, _)| object.controller() == Some(native))
                .map(|(object, return_seat, program)| {
                    let face_down = object.face_down().is_some();
                    Ok(crate::player_wire::PlayerPhasedPermanent {
                        names: if face_down {
                            Vec::new()
                        } else {
                            program
                                .names
                                .iter()
                                .map(|name| name.as_str().to_owned())
                                .collect()
                        },
                        face_down,
                        tapped: object.tapped() == Some(true),
                        phase_in_seat_id: self.app_seat(return_seat)?,
                        loyalty: source
                            .execution()
                            .loyalty_counters()
                            .get(&object.id())
                            .map(quantity)
                            .transpose()?,
                    })
                })
                .collect::<Result<_, NativeProjectionError>>()?;
            players.push(PlayerSeatView {
                phased_out,
                emblems: source.execution().emblems().filter(|emblem| emblem.controller == native).map(|emblem| match &emblem.definition {
                    spellwake_engine::emblem::EmblemDefinition::PreventAllButOne => PlayerEmblem::PreventAllButOne {},
                    spellwake_engine::emblem::EmblemDefinition::LoyaltyAnyTurn => PlayerEmblem::LoyaltyAnyTurn {},
                    spellwake_engine::emblem::EmblemDefinition::UnlimitedHandWithGraveyardReturn =>
                        PlayerEmblem::UnlimitedHandWithGraveyardReturn {},
                    spellwake_engine::emblem::EmblemDefinition::ReturnFallenArtifacts => PlayerEmblem::ReturnFallenArtifacts {},
                    spellwake_engine::emblem::EmblemDefinition::SacrificeCreatureGainLifeDraw => PlayerEmblem::SacrificeCreatureGainLifeDraw {},
                    spellwake_engine::emblem::EmblemDefinition::UpkeepDamage { amount } => PlayerEmblem::UpkeepDamage { amount: amount.clone() },
                    spellwake_engine::emblem::EmblemDefinition::SpellcastDamage { amount, noncreature_only } => PlayerEmblem::SpellcastDamage { amount: amount.clone(), noncreature_only: *noncreature_only },
                    spellwake_engine::emblem::EmblemDefinition::ControlledCreatureBonus { power_toughness, keywords } => PlayerEmblem::ControlledCreatureBonus {
                        power: power_toughness.power.clone(), toughness: power_toughness.toughness.clone(),
                        keywords: keywords.iter().copied().map(keyword).collect(),
                    },
                    spellwake_engine::emblem::EmblemDefinition::Authored { label, .. } => {
                        let clean: String = label
                            .chars()
                            .filter(|ch| !ch.is_control())
                            .take(512)
                            .collect();
                        PlayerEmblem::Authored {
                            label: if clean.is_empty() { "Emblem".to_owned() } else { clean },
                        }
                    }
                }).collect(),
                monarch: source.execution().monarch() == Some(native),
                city_blessing: source.execution().has_city_blessing(native),
                cascade_reveals,
                speed: source.execution().speed(native),
                face_down_reveals,
                search_reveal,
                search_reveals,
                seat_id: self.app_seat(native)?,
                life: source
                    .execution()
                    .life_total(native)
                    .ok_or(NativeProjectionError::MissingNativeValue)?
                    .clone(),
                poison: quantity(&source.execution().poison_counters(native))?,
                counters: {
                    let mut list = Vec::new();
                    let experience = source.execution().experience_counters(native);
                    if experience.is_positive() {
                        list.push(crate::player_wire::PlayerCounter {
                            name: "experience".to_owned(),
                            count: quantity(&experience)?,
                        });
                    }
                    let energy = source.execution().player_counters_of_kind(
                        native,
                        spellwake_engine::effects::CounterKind::Energy,
                    );
                    if energy.is_positive() {
                        list.push(crate::player_wire::PlayerCounter {
                            name: "energy".to_owned(),
                            count: quantity(&energy)?,
                        });
                    }
                    list
                },
                mana_pool: ManaType::ALL
                    .into_iter()
                    .map(|kind| {
                        Ok(ManaSpend {
                            color: mana(kind),
                            amount: quantity(&source.execution().mana(native, kind))?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?,
                mana_lots: source.execution().floating_mana_lots().filter(|(_,lot)|lot.player == native).map(|(id,lot)| {
                    use spellwake_engine::mana::ManaSpendEffect;
                    use crate::player_wire::{PlayerManaLot, PlayerManaSpendEffect};
                    let effect = match lot.effect {
                        ManaSpendEffect::RetainUntilCleanup => PlayerManaSpendEffect::RetainUntilCleanup,
                        ManaSpendEffect::RetainUntilEndOfCombat => PlayerManaSpendEffect::RetainUntilEndOfCombat,
                        ManaSpendEffect::CommanderEntryCounters => PlayerManaSpendEffect::CommanderEntryCounters,
                        ManaSpendEffect::SharedCommanderTypeScry => PlayerManaSpendEffect::SharedCommanderTypeScry,
                    };
                    Ok(PlayerManaLot { id, effect, color: mana(lot.mana_type), amount: quantity(&lot.remaining)? })
                }).collect::<Result<_,NativeProjectionError>>()?,
                restricted_mana: source.execution().restricted_mana().iter().filter(|((seat, _, _), amount)| *seat == native && amount.is_positive()).map(|((_, kind, restriction), amount)| {
                    Ok(crate::player_wire::PlayerRestrictedManaSpend {
                        color: mana(*kind), amount: quantity(amount)?,
                        restriction: match restriction {
                            spellwake_engine::mana::ManaRestriction::SpellWithFlashbackFromGraveyard => crate::player_wire::PlayerManaRestriction::SpellWithFlashbackFromGraveyard,
                            spellwake_engine::mana::ManaRestriction::PlaneswalkerSpell => crate::player_wire::PlayerManaRestriction::PlaneswalkerSpell,
                            spellwake_engine::mana::ManaRestriction::ColorlessSpell => crate::player_wire::PlayerManaRestriction::ColorlessSpell,
                            spellwake_engine::mana::ManaRestriction::CommanderSpell => crate::player_wire::PlayerManaRestriction::CommanderSpell,
                            spellwake_engine::mana::ManaRestriction::ColorlessEldraziSpellOrAbility => crate::player_wire::PlayerManaRestriction::ColorlessEldraziSpellOrAbility,
                            spellwake_engine::mana::ManaRestriction::NoncreatureSpell => crate::player_wire::PlayerManaRestriction::NoncreatureSpell,
                            spellwake_engine::mana::ManaRestriction::ArtifactSpellOrAbility => crate::player_wire::PlayerManaRestriction::ArtifactSpellOrAbility,
                            spellwake_engine::mana::ManaRestriction::NotNonartifactSpell => crate::player_wire::PlayerManaRestriction::NotNonartifactSpell,
                            spellwake_engine::mana::ManaRestriction::CreatureSpell => crate::player_wire::PlayerManaRestriction::CreatureSpell,
                            spellwake_engine::mana::ManaRestriction::CreatureSpellOfTypeUncounterable(_) => crate::player_wire::PlayerManaRestriction::CreatureSpellOfTypeUncounterable,
                            spellwake_engine::mana::ManaRestriction::CreatureSpellOfType(_) => crate::player_wire::PlayerManaRestriction::CreatureSpellOfType,
                            spellwake_engine::mana::ManaRestriction::CreatureSpellOrAbilityOfType(_) => crate::player_wire::PlayerManaRestriction::CreatureSpellOrAbilityOfType,
                            spellwake_engine::mana::ManaRestriction::InstantOrSorcerySpell => crate::player_wire::PlayerManaRestriction::InstantOrSorcerySpell,
                            spellwake_engine::mana::ManaRestriction::ArtifactSpell => crate::player_wire::PlayerManaRestriction::ArtifactSpell,
                            spellwake_engine::mana::ManaRestriction::ArtifactAbility => crate::player_wire::PlayerManaRestriction::ArtifactAbility,
                            spellwake_engine::mana::ManaRestriction::ArtifactSpellOrAnyAbility => crate::player_wire::PlayerManaRestriction::ArtifactSpellOrAnyAbility,
                            spellwake_engine::mana::ManaRestriction::ActivatedAbility => crate::player_wire::PlayerManaRestriction::ActivatedAbility,
                            spellwake_engine::mana::ManaRestriction::CastSpell => crate::player_wire::PlayerManaRestriction::CastSpell,
                            spellwake_engine::mana::ManaRestriction::NotSpellFromHand => crate::player_wire::PlayerManaRestriction::NotSpellFromHand,
                            spellwake_engine::mana::ManaRestriction::CostContainingX => crate::player_wire::PlayerManaRestriction::CostContainingX,
                            spellwake_engine::mana::ManaRestriction::LegendarySpell => crate::player_wire::PlayerManaRestriction::LegendarySpell,
                            spellwake_engine::mana::ManaRestriction::MulticoloredSpell => crate::player_wire::PlayerManaRestriction::MulticoloredSpell,
                            spellwake_engine::mana::ManaRestriction::EnchantmentSpell => crate::player_wire::PlayerManaRestriction::EnchantmentSpell,
                            spellwake_engine::mana::ManaRestriction::CreatureSpellOrAbility => crate::player_wire::PlayerManaRestriction::CreatureSpellOrAbility,
                            spellwake_engine::mana::ManaRestriction::ColoredSpellWithoutX => crate::player_wire::PlayerManaRestriction::ColoredSpellWithoutX,
                            spellwake_engine::mana::ManaRestriction::InstantOrSorceryOrForetell => crate::player_wire::PlayerManaRestriction::InstantOrSorceryOrForetell,
                            spellwake_engine::mana::ManaRestriction::SpellWithMinManaValue(_) => crate::player_wire::PlayerManaRestriction::SpellWithMinManaValue,
                            spellwake_engine::mana::ManaRestriction::CreatureSpellWithMinManaValue(_) => crate::player_wire::PlayerManaRestriction::CreatureSpellWithMinManaValue,
                            spellwake_engine::mana::ManaRestriction::HistoricSpell => crate::player_wire::PlayerManaRestriction::HistoricSpell,
                            spellwake_engine::mana::ManaRestriction::CreatureOrPlaneswalkerSpell => crate::player_wire::PlayerManaRestriction::CreatureOrPlaneswalkerSpell,
                            spellwake_engine::mana::ManaRestriction::CreatureAbility => crate::player_wire::PlayerManaRestriction::CreatureAbility,
                            spellwake_engine::mana::ManaRestriction::PermanentSpell => crate::player_wire::PlayerManaRestriction::PermanentSpell,
                            spellwake_engine::mana::ManaRestriction::SpellFromGraveyardOrExile => crate::player_wire::PlayerManaRestriction::SpellFromGraveyardOrExile,
                            spellwake_engine::mana::ManaRestriction::EquipmentOrEquipAbility => crate::player_wire::PlayerManaRestriction::EquipmentOrEquipAbility,
                            spellwake_engine::mana::ManaRestriction::SpellWithMinManaValueOrX(_) => crate::player_wire::PlayerManaRestriction::SpellWithMinManaValueOrX,
                            spellwake_engine::mana::ManaRestriction::CreatureSpellWithMinManaValueOrX(_) => crate::player_wire::PlayerManaRestriction::CreatureSpellWithMinManaValueOrX,
                            spellwake_engine::mana::ManaRestriction::CreatureOrEnchantmentSpell => crate::player_wire::PlayerManaRestriction::CreatureOrEnchantmentSpell,
                        },
                        creature_type: match restriction {
                            spellwake_engine::mana::ManaRestriction::CreatureSpellOfTypeUncounterable(Some(kind)) | spellwake_engine::mana::ManaRestriction::CreatureSpellOfType(Some(kind)) | spellwake_engine::mana::ManaRestriction::CreatureSpellOrAbilityOfType(Some(kind)) => Some(PlayerCreatureTypeLabel::try_from(source.execution().creature_vocabulary().label(*kind).ok_or(NativeProjectionError::MissingNativeValue)?.to_owned()).map_err(|_| NativeProjectionError::InvalidSnapshot)?),
                            _ => None,
                        },
                        min_mana_value: match restriction {
                            spellwake_engine::mana::ManaRestriction::SpellWithMinManaValue(n)
                            | spellwake_engine::mana::ManaRestriction::CreatureSpellWithMinManaValue(n)
                            | spellwake_engine::mana::ManaRestriction::SpellWithMinManaValueOrX(n)
                            | spellwake_engine::mana::ManaRestriction::CreatureSpellWithMinManaValueOrX(n) => Some(*n),
                            _ => None,
                        },
                    })
                }).collect::<Result<_, NativeProjectionError>>()?,
                hand_count: count(
                    cards
                        .iter()
                        .filter(|object| object.zone() == ObjectZone::Hand(native))
                        .count(),
                )?,
                library_count: count(cards.library(native).len())?,
                eliminated: !source.execution().window().participants().contains(&native)
                    || source.conclusion().is_some_and(|conclusion| {
                    conclusion.losing_players().any(|seat| seat == native)
                }),
            });
        }
        let turn = if let Some(turn) = source.turn() {
            let position = turn.position();
            let priority_seat_ids = match source.execution().window().phase() {
                WindowPhase::Priority(PriorityPhase::Ready { holder })
                    if source.outcome().is_none() && source.pending_decision().is_none() =>
                {
                    vec![self.app_seat(holder)?]
                }
                WindowPhase::Opening
                | WindowPhase::TurnWork
                | WindowPhase::Priority(_)
                | WindowPhase::Resolving(_)
                | WindowPhase::CheckingTargets(_)
                | WindowPhase::AllTargetsIllegal(_)
                | WindowPhase::StepEnded => vec![],
            };
            Some(PlayerTurn {
                number: RulesCount::try_from(position.number.to_string())
                    .map_err(|_| NativeProjectionError::InvalidSnapshot)?,
                day_night: match source.execution().day_night() {
                    spellwake_engine::day_night::DayNight::Neither => None,
                    spellwake_engine::day_night::DayNight::Day => Some(PlayerDayNight::Day),
                    spellwake_engine::day_night::DayNight::Night => Some(PlayerDayNight::Night),
                },
                active_seat_ids: source
                    .execution()
                    .window()
                    .active_participant()
                    .map(|seat| self.app_seat(seat))
                    .transpose()?
                    .into_iter()
                    .collect(),
                priority_seat_ids,
                phase: phase(position.step.phase()),
                step: step(position.step),
            })
        } else {
            None
        };
        self.stack = self.stack_entries()?;
        let decisions = self.decisions()?;
        let visible_objects: Vec<_> = self
            .objects
            .iter()
            .map(|object| object.handle.clone())
            .collect();
        let snapshot = PlayerSnapshot {
            basis: self.scope.basis().clone(),
            viewer_seat_id: self.scope.viewer_seat_id(),
            players,
            turn,
            visible_objects: self.objects,
            zones,
            stack: self.stack,
            decisions,
            cast_offers: None,
            through_event_sequence: self.metadata.through_event_sequence.clone(),
        };
        snapshot
            .validate(&PlayerProjectionFacts {
                basis: self.scope.basis(),
                viewer: self.scope.viewer_seat_id(),
                roster: &roster,
                through_sequence: &self.metadata.through_event_sequence,
                visible_objects: &visible_objects,
                known_identities: &self.known,
                ordered_zones: &self.ordered,
                authorized_full_disclosure: self.metadata.disclosure
                    == NativeDisclosure::CompletedParticipant,
                creature_vocabulary: Some(source.execution().creature_vocabulary()),
                ability_bindings: &self.abilities,
            })
            .map_err(|_| NativeProjectionError::InvalidSnapshot)?;
        Ok(NativeProjectedSnapshot {
            snapshot,
            roster,
            visible_objects,
            known_identities: self.known,
            ordered_zones: self.ordered,
            ability_bindings: self.abilities,
            full_disclosure: self.metadata.disclosure == NativeDisclosure::CompletedParticipant,
            vocabulary_id: source.execution().creature_vocabulary().id(),
        })
    }
    fn zone(&mut self, zone: ObjectZone) -> Result<PlayerZone, NativeProjectionError> {
        let source = self.scope.source;
        let (zone_type, owner) = match zone {
            ObjectZone::Library(seat) => (PlayerZoneKind::Library, Some(self.app_seat(seat)?)),
            ObjectZone::Hand(seat) => (PlayerZoneKind::Hand, Some(self.app_seat(seat)?)),
            ObjectZone::Graveyard(seat) => (PlayerZoneKind::Graveyard, Some(self.app_seat(seat)?)),
            ObjectZone::Battlefield => (PlayerZoneKind::Battlefield, None),
            ObjectZone::Stack => (PlayerZoneKind::Stack, None),
            ObjectZone::Exile => (PlayerZoneKind::Exile, None),
            ObjectZone::Command => (PlayerZoneKind::Command, None),
        };
        let ids: Vec<_> = match zone {
            ObjectZone::Library(seat) => source.execution().window().cards().library(seat).to_vec(),
            ObjectZone::Graveyard(seat) => {
                source.execution().window().cards().graveyard(seat).to_vec()
            }
            _ => source
                .execution()
                .window()
                .cards()
                .iter()
                .filter(|object| object.zone() == zone)
                .map(GameObject::id)
                .collect(),
        };
        let handle = self.issue(NativeReference::Zone(zone))?;
        let hidden = self.metadata.disclosure == NativeDisclosure::LiveParticipant
            && match zone {
                ObjectZone::Library(_) => true,
                ObjectZone::Hand(seat) => seat != self.scope.binding.actor,
                _ => false,
            };
        let total = ids.len();
        let ids = if hidden {
            match zone {
                ObjectZone::Library(owner) => {
                    let reveals = source.execution().draw_reveals(owner);
                    let top_visible = source
                        .execution()
                        .library_top_access(&self.evaluated, owner, self.scope.binding.actor)
                        .visible;
                    let order_domain: &[ObjectId] =
                        match source.pending_decision().map(|d| d.frame()) {
                            Some(spellwake_engine::game::RuleFrame::ResolutionGraveyardOrder(
                                frame,
                            )) if frame.chooser() == self.scope.binding.actor => frame.domain(),
                            _ => &[],
                        };
                    ids.into_iter()
                        .enumerate()
                        .filter_map(|(i, id)| {
                            (reveals.contains(&id)
                                || (i == 0 && top_visible)
                                || order_domain.contains(&id))
                            .then_some(id)
                        })
                        .collect()
                }
                ObjectZone::Hand(_) => {
                    let revealed_select: &[ObjectId] =
                        match source.pending_decision().map(|d| d.frame()) {
                            Some(spellwake_engine::game::RuleFrame::ResolutionSelect(frame))
                                if frame.chooser() == self.scope.binding.actor =>
                            {
                                frame.domain()
                            }
                            _ => &[],
                        };
                    ids.into_iter()
                        .filter(|id| {
                            source.execution().revealed_by_ninjutsu(*id)
                                || revealed_select.contains(id)
                        })
                        .collect()
                }
                _ => Vec::new(),
            }
        } else {
            ids
        };
        let concealed = total - ids.len();
        if hidden && ids.is_empty() {
            return Ok(PlayerZone::Hidden {
                handle,
                zone_type,
                owner_seat_id: owner,
                hidden_count: count(concealed)?,
            });
        }
        let order_known = matches!(zone, ObjectZone::Library(_) | ObjectZone::Graveyard(_));
        let mut objects = vec![];
        for id in ids {
            self.visible.insert(id);
            let object = self.object(id, &handle)?;
            objects.push(object.handle.clone());
            if matches!(object.identity, PlayerObjectIdentity::Known { .. }) {
                self.known.push(object.handle.clone());
            }
            self.objects.push(object);
        }
        if order_known {
            self.ordered.push(handle.clone());
        } else {
            objects.sort();
        }
        if concealed > 0 {
            return Ok(PlayerZone::Mixed {
                handle,
                zone_type,
                owner_seat_id: owner,
                objects,
                hidden_count: count(concealed)?,
                order_known,
            });
        }
        Ok(PlayerZone::Visible {
            handle,
            zone_type,
            owner_seat_id: owner,
            objects,
            order_known,
        })
    }
    fn object(
        &mut self,
        id: ObjectId,
        zone: &PlayerHandle,
    ) -> Result<PlayerObject, NativeProjectionError> {
        let source = self.scope.source;
        let physical = source
            .execution()
            .window()
            .cards()
            .get(id)
            .ok_or(NativeProjectionError::MissingNativeValue)?;
        let current = self
            .evaluated
            .get(id)
            .ok_or(NativeProjectionError::MissingNativeValue)?
            .clone();
        use spellwake_engine::face_down::FaceDownKind;
        let face_down = physical.face_down().map(|kind| match kind {
            FaceDownKind::Forest => PlayerFaceDownKind::Forest,
            FaceDownKind::Manifest => PlayerFaceDownKind::Manifest,
            FaceDownKind::Morph => PlayerFaceDownKind::Morph,
            FaceDownKind::Disguise => PlayerFaceDownKind::Disguise,
        });
        let foretold = source.execution().foretold_on(id);
        let exiled_face_down_viewer = source.execution().face_down_exile_viewer(id);
        let may_look =
            (face_down.is_none() && foretold.is_none() && exiled_face_down_viewer.is_none())
                || self.metadata.disclosure == NativeDisclosure::CompletedParticipant
                || source.conclusion().is_some()
                || (face_down.is_some() && current.controller == Some(self.scope.binding.actor))
                || (foretold.is_some() && physical.owner() == self.scope.binding.actor)
                || exiled_face_down_viewer == Some(self.scope.binding.actor);
        let face_down_order =
            if face_down.is_some() || foretold.is_some() || exiled_face_down_viewer.is_some() {
                let earlier = source
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .filter(|other| {
                        other.zone() == physical.zone()
                            && other.id() < id
                            && (other.face_down().is_some()
                                || source.execution().foretold_on(other.id()).is_some()
                                || source
                                    .execution()
                                    .face_down_exile_viewer(other.id())
                                    .is_some())
                    })
                    .count();
                Some(count(earlier + 1)?)
            } else {
                None
            };
        let mut face_up_options = Vec::new();
        if face_down.is_some()
            && current.zone == ObjectZone::Battlefield
            && current.controller == Some(self.scope.binding.actor)
            && source.conclusion().is_none()
        {
            let preview = source
                .execution()
                .face_up_preview(id)
                .map_err(|_| NativeProjectionError::MissingNativeValue)?
                .ok_or(NativeProjectionError::MissingNativeValue)?;
            let mut options = preview
                .abilities
                .face_up
                .iter()
                .map(|&index| {
                    let price = preview
                        .printed_text
                        .face_up_abilities
                        .get(index)
                        .ok_or(NativeProjectionError::MissingNativeValue)?
                        .cost();
                    Ok((index, price))
                })
                .collect::<Result<Vec<_>, NativeProjectionError>>()?;
            if let Some(price) = source.execution().manifest_face_up_cost(id) {
                options.push((preview.printed_text.face_up_abilities.len(), price));
            }
            for (index, price) in options {
                face_up_options.push(PlayerFaceUpOption {
                    ability: u16::try_from(index)
                        .map_err(|_| NativeProjectionError::InvalidSnapshot)?,
                    generic: quantity(&price.generic)?,
                    required: price
                        .required
                        .iter()
                        .map(|(kind, amount)| {
                            Ok(ManaSpend {
                                color: mana(*kind),
                                amount: quantity(amount)?,
                            })
                        })
                        .collect::<Result<_, NativeProjectionError>>()?,
                });
            }
        }
        let identity = if may_look {
            let definition =
                if face_down.is_some() || foretold.is_some() || exiled_face_down_viewer.is_some() {
                    source.execution().base_program(id)
                } else {
                    source.execution().display_program(id)
                }
                .ok_or(NativeProjectionError::MissingNativeValue)?;
            let display = self
                .display
                .object(physical, physical.displayed_face(definition))?;
            if display.definition_id.is_some_and(|id| id.is_nil())
                || display.printing_id.is_some_and(|id| id.is_nil())
                || display
                    .custom_card_revision_id
                    .is_some_and(|id| id.is_nil())
                || (display.definition_id.is_some() && display.custom_card_revision_id.is_some())
                || (self.display.scope() == NativeDisplayScope::SourceOnly
                    && (display.definition_id.is_some()
                        || display.printing_id.is_some()
                        || display.custom_card_revision_id.is_some()))
            {
                return Err(NativeProjectionError::InvalidMetadata);
            }
            PlayerObjectIdentity::Known {
                definition_id: display.definition_id,
                printing_id: display.printing_id,
                custom_card_revision_id: display.custom_card_revision_id,
                characteristics: Box::new(display.characteristics),
                current_names: if foretold.is_some() || exiled_face_down_viewer.is_some() {
                    spellwake_contracts::names::ObjectNames::nameless()
                } else {
                    current.current_names.clone()
                },
            }
        } else {
            PlayerObjectIdentity::Concealed {}
        };
        let handle = self.issue(NativeReference::Object(id))?;
        let mut abilities = vec![];
        for ability in current
            .mana_abilities()
            .into_iter()
            .map(NativeAbilityReference::Mana)
            .chain(
                current
                    .abilities
                    .activated
                    .iter()
                    .copied()
                    .map(NativeAbilityReference::Activated),
            )
            .chain(
                current
                    .abilities
                    .triggered
                    .iter()
                    .copied()
                    .map(NativeAbilityReference::Triggered),
            )
            .chain(
                current
                    .abilities
                    .continuous
                    .into_iter()
                    .chain(
                        current
                            .abilities
                            .additional_battlefield_abilities
                            .iter()
                            .copied(),
                    )
                    .map(NativeAbilityReference::Continuous),
            )
        {
            if foretold.is_some() || exiled_face_down_viewer.is_some() {
                break;
            }
            let (reference, kind) = match ability {
                NativeAbilityReference::Mana(ability) => (
                    NativeReference::ManaAbility {
                        source: id,
                        ability,
                    },
                    PlayerAbilityKind::Mana,
                ),
                NativeAbilityReference::Activated(ability) => (
                    NativeReference::ActivatedAbility {
                        source: id,
                        ability,
                    },
                    activated_kind(
                        current
                            .printed_text
                            .activated
                            .iter()
                            .find(|instance| instance.id == ability),
                    ),
                ),
                NativeAbilityReference::Triggered(ability) => (
                    NativeReference::TriggeredAbility {
                        source: id,
                        ability,
                    },
                    PlayerAbilityKind::Triggered,
                ),
                NativeAbilityReference::Continuous(ability) => (
                    NativeReference::ContinuousAbility {
                        source: id,
                        ability,
                    },
                    PlayerAbilityKind::Continuous,
                ),
            };
            let mut label = if face_down.is_some() {
                match ability {
                    NativeAbilityReference::Triggered(_) => "Triggered ability",
                    NativeAbilityReference::Activated(_) => "Activated ability",
                    NativeAbilityReference::Continuous(_) => "Static ability",
                    NativeAbilityReference::Mana(_) => "Mana ability",
                }
                .into()
            } else {
                self.display.ability(physical, &current, ability)?
            };
            if let NativeAbilityReference::Activated(number) = ability
                && let Some(cost) = current.printed_text.loyalty_costs.get(&number)
            {
                label = if current
                    .printed_text
                    .variable_loyalty_costs
                    .contains(&number)
                {
                    format!("−X: {label}")
                } else if *cost > 0 {
                    format!("+{cost}: {label}")
                } else {
                    format!("{cost}: {label}")
                };
            }
            let issued = self.issue(reference)?;
            self.abilities.push(PlayerAbilityBinding {
                source: handle.clone(),
                handle: issued.clone(),
                kind,
            });
            let power_up_cost = match ability {
                NativeAbilityReference::Activated(number)
                    if current.zone == ObjectZone::Battlefield
                        && current.abilities.activated.contains(&number)
                        && current.printed_text.activation_restrictions.get(&number)
                            == Some(
                                &spellwake_engine::activated::ActivationRestriction::PowerUp,
                            ) =>
                {
                    let definition = current
                        .printed_text
                        .activated
                        .iter()
                        .find(|a| a.id == number)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    let price = source
                        .execution()
                        .power_up_mana_cost(id, &definition.mana_cost);
                    Some(PlayerPowerUpCost {
                        generic: quantity(&price.generic)?,
                        required: price
                            .required
                            .iter()
                            .filter(|(_, n)| n.is_positive())
                            .map(|(color, amount)| {
                                Ok(ManaSpend {
                                    color: mana(*color),
                                    amount: quantity(amount)?,
                                })
                            })
                            .collect::<Result<_, NativeProjectionError>>()?,
                    })
                }
                _ => None,
            };
            abilities.push(PlayerAbilityView {
                power_up_cost,
                exhausted:matches!(ability,NativeAbilityReference::Activated(number) if self.scope.source.execution().ability_exhausted(id,number)),
                handle: issued,
                kind,
                label,
            });
        }
        if foretold.is_none() && exiled_face_down_viewer.is_none() {
            for (effect, granted) in &current.granted_activated {
                let kind = activated_kind(Some(granted));
                let issued = self.issue(NativeReference::GrantedAbility {
                    source: id,
                    effect: *effect,
                })?;
                self.abilities.push(PlayerAbilityBinding {
                    source: handle.clone(),
                    handle: issued.clone(),
                    kind,
                });
                let label = match granted.body.instructions.as_slice() {
                    [spellwake_engine::program::Instruction::Encore] => "Encore",
                    [spellwake_engine::program::Instruction::RegenerateSource] => {
                        "Regenerate this permanent"
                    }
                    [spellwake_engine::program::Instruction::RegenerateTarget] => {
                        "Regenerate target permanent"
                    }
                    [spellwake_engine::program::Instruction::DestroyTarget] => {
                        "Destroy target permanent"
                    }
                    _ => "Granted activated ability",
                };
                let terms = current.inherited_activation_terms.get(effect);
                let power_up_cost = if terms.is_some_and(|t| {
                    t.restriction
                        == Some(spellwake_engine::activated::ActivationRestriction::PowerUp)
                }) {
                    let price = source
                        .execution()
                        .power_up_mana_cost(id, &granted.mana_cost);
                    Some(PlayerPowerUpCost {
                        generic: quantity(&price.generic)?,
                        required: price
                            .required
                            .iter()
                            .filter(|(_, n)| n.is_positive())
                            .map(|(color, amount)| {
                                Ok(ManaSpend {
                                    color: mana(*color),
                                    amount: quantity(amount)?,
                                })
                            })
                            .collect::<Result<_, NativeProjectionError>>()?,
                    })
                } else {
                    None
                };
                let label = match terms.and_then(|t| t.loyalty.map(|n| (n, t.variable_loyalty))) {
                    Some((_, true)) => format!("−X: {label}"),
                    Some((n, false)) if n > 0 => format!("+{n}: {label}"),
                    Some((n, false)) => format!("{n}: {label}"),
                    None => label.into(),
                };
                let exhausted =
                    terms.is_some_and(|t| source.execution().inherited_ability_exhausted(id, t));
                abilities.push(PlayerAbilityView {
                    power_up_cost,
                    exhausted,
                    handle: issued,
                    kind,
                    label,
                });
            }
        }
        let mut counters = vec![];
        if let Some(values) = source.execution().power_toughness_counters(id) {
            for (name, value) in [("+1/+1", values.plus_one), ("-1/-1", values.minus_one)] {
                if value.is_positive() {
                    counters.push(PlayerCounter {
                        name: name.into(),
                        count: quantity(&value)?,
                    });
                }
            }
        }
        if let Some(n) = source.execution().lore_counters().get(&id) {
            counters.push(PlayerCounter {
                name: "lore".into(),
                count: quantity(n)?,
            });
        }
        if let Some(n) = source.execution().loyalty_counters().get(&id) {
            counters.push(PlayerCounter {
                name: "loyalty".into(),
                count: quantity(n)?,
            });
        }
        if let Some(n) = source.execution().charge_counters().get(&id) {
            counters.push(PlayerCounter {
                name: "charge".into(),
                count: quantity(n)?,
            });
        }
        for (name, kind) in [
            ("defense", spellwake_engine::effects::CounterKind::Defense),
            ("study", spellwake_engine::effects::CounterKind::Study),
            ("eyeball", spellwake_engine::effects::CounterKind::Eyeball),
            ("slime", spellwake_engine::effects::CounterKind::Slime),
            ("vow", spellwake_engine::effects::CounterKind::Vow),
            ("corpse", spellwake_engine::effects::CounterKind::Corpse),
            ("blessing", spellwake_engine::effects::CounterKind::Blessing),
            ("despair", spellwake_engine::effects::CounterKind::Despair),
            (
                "indestructible",
                spellwake_engine::effects::CounterKind::Indestructible,
            ),
            ("page", spellwake_engine::effects::CounterKind::Page),
            ("quest", spellwake_engine::effects::CounterKind::Quest),
            ("verse", spellwake_engine::effects::CounterKind::Verse),
        ] {
            let count = source.execution().counters_of_kind(id, kind);
            if count.is_positive() {
                counters.push(PlayerCounter {
                    name: name.into(),
                    count: quantity(&count)?,
                });
            }
        }
        for (name, counts) in [
            ("suspect", source.execution().suspect_counters()),
            ("storage", source.execution().storage_counters()),
            ("fade", source.execution().fade_counters()),
            ("time", source.execution().time_counters()),
        ] {
            if let Some(amount) = counts.get(&id) {
                counters.push(PlayerCounter {
                    name: name.into(),
                    count: quantity(amount)?,
                });
            }
        }
        if let Some(amount) = source.execution().age_counters().get(&id) {
            counters.push(PlayerCounter {
                name: "age".into(),
                count: quantity(amount)?,
            });
        }
        for (kind, amount) in source.execution().extra_named_counters(id) {
            if matches!(
                kind,
                spellwake_engine::effects::CounterKind::Study
                    | spellwake_engine::effects::CounterKind::Eyeball
                    | spellwake_engine::effects::CounterKind::Slime
            ) {
                continue;
            }
            counters.push(PlayerCounter {
                name: kind.oracle_label().into_owned(),
                count: quantity(&amount)?,
            });
        }
        let properties = PlayerPublicProperties {
            chosen_creature_type: source
                .execution()
                .entry_creature_type(id)
                .map(|kind| {
                    source
                        .execution()
                        .creature_vocabulary()
                        .label(kind)
                        .ok_or(NativeProjectionError::MissingNativeValue)
                        .and_then(|label| {
                            PlayerCreatureTypeLabel::try_from(label.to_owned())
                                .map_err(|_| NativeProjectionError::InvalidSnapshot)
                        })
                })
                .transpose()?,
            renowned: source.execution().renowned_objects().contains(&id),
            card_types: current
                .current_types
                .iter()
                .copied()
                .map(card_type)
                .collect(),
            supertypes: current
                .current_supertypes
                .iter()
                .copied()
                .map(supertype)
                .collect(),
            creature_types: current
                .current_creature_subtypes
                .iter()
                .map(|id| {
                    source
                        .execution()
                        .creature_vocabulary()
                        .label(*id)
                        .ok_or(NativeProjectionError::MissingNativeValue)
                        .and_then(|label| {
                            PlayerCreatureTypeLabel::try_from(label.to_owned())
                                .map_err(|_| NativeProjectionError::InvalidSnapshot)
                        })
                })
                .collect::<Result<_, _>>()?,
            land_types: current
                .current_land_subtypes
                .iter()
                .copied()
                .map(land_type)
                .collect(),
            planeswalker_types: current
                .current_planeswalker_subtypes
                .iter()
                .map(|kind| format!("{kind:?}"))
                .collect(),
            enchantment_types: current
                .current_enchantment_subtypes
                .iter()
                .map(|kind| kind.wire_label().to_owned())
                .collect(),
            spell_types: current
                .current_spell_subtypes
                .iter()
                .map(|kind| match kind {
                    spellwake_engine::program::SpellSubtype::Arcane => PlayerSpellSubtype::Arcane,
                    spellwake_engine::program::SpellSubtype::Adventure => {
                        PlayerSpellSubtype::Adventure
                    }
                    spellwake_engine::program::SpellSubtype::Lesson => PlayerSpellSubtype::Lesson,
                    spellwake_engine::program::SpellSubtype::Trap => PlayerSpellSubtype::Trap,
                    spellwake_engine::program::SpellSubtype::Omen => PlayerSpellSubtype::Omen,
                })
                .collect(),
            colors: current.current_colors.iter().copied().map(color).collect(),
            keywords: {
                let mut seen = std::collections::BTreeSet::new();
                current
                    .keywords
                    .iter()
                    .copied()
                    .map(keyword)
                    .chain(
                        current
                            .printed_text
                            .activation_restrictions
                            .iter()
                            .any(|(id, r)| {
                                *r == spellwake_engine::activated::ActivationRestriction::PowerUp
                                    && current.abilities.activated.contains(id)
                            })
                            .then_some(PlayerKeyword::PowerUp),
                    )
                    .chain(
                        current
                            .toxic_value
                            .is_some()
                            .then_some(PlayerKeyword::Toxic),
                    )
                    .chain(
                        current
                            .printed_text
                            .triggered
                            .iter()
                            .filter(|a| current.abilities.triggered.contains(&a.id))
                            .filter_map(|a| match &a.condition.event {
                                spellwake_engine::program::TriggerEvent::Attacks {
                                    subject,
                                    mode:
                                        spellwake_engine::program::AttackTriggerMode::Individual {
                                            capture_defender: true,
                                        },
                                } if *subject
                                    == spellwake_engine::program::ObjectFilter::self_object() =>
                                {
                                    if a.body
                                        .instructions
                                        .contains(&spellwake_engine::program::Instruction::Myriad)
                                    {
                                        Some(PlayerKeyword::Myriad)
                                    } else if a.body.instructions.iter().any(|op| {
                                        matches!(
                                            op,
                                            spellwake_engine::program::Instruction::Sacrifice { .. }
                                        )
                                    }) {
                                        Some(PlayerKeyword::Annihilator)
                                    } else {
                                        None
                                    }
                                }
                                spellwake_engine::program::TriggerEvent::Keyword(
                                    spellwake_engine::program::TriggerKeywordEvent::Counter(k),
                                ) => match k {
                                    spellwake_engine::program::CounterKeyword::Fading(_) => {
                                        Some(PlayerKeyword::Fading)
                                    }
                                    spellwake_engine::program::CounterKeyword::Vanishing(_) => {
                                        Some(PlayerKeyword::Vanishing)
                                    }
                                    spellwake_engine::program::CounterKeyword::Persist => {
                                        Some(PlayerKeyword::Persist)
                                    }
                                    spellwake_engine::program::CounterKeyword::Undying => {
                                        Some(PlayerKeyword::Undying)
                                    }
                                    _ => None,
                                },
                                spellwake_engine::program::TriggerEvent::Keyword(
                                    spellwake_engine::program::TriggerKeywordEvent::Upkeep(
                                        spellwake_engine::program::UpkeepKeyword::Echo,
                                    ),
                                ) => Some(PlayerKeyword::Echo),
                                spellwake_engine::program::TriggerEvent::Keyword(
                                    spellwake_engine::program::TriggerKeywordEvent::Upkeep(
                                        spellwake_engine::program::UpkeepKeyword::CumulativeUpkeep,
                                    ),
                                ) => Some(PlayerKeyword::CumulativeUpkeep),
                                _ => match a.body.instructions.as_slice() {
                                    [_, spellwake_engine::program::Instruction::ExtortDrain] => {
                                        Some(PlayerKeyword::Extort)
                                    }
                                    [
                                        _,
                                        spellwake_engine::program::Instruction::ResolveTransfer(
                                            key,
                                        ),
                                    ] => {
                                        use spellwake_engine::program::TransferKeyword;
                                        Some(match key {
                                            TransferKeyword::Graft(_) => PlayerKeyword::Graft,
                                            TransferKeyword::Modular(_) => PlayerKeyword::Modular,
                                            TransferKeyword::Soulshift(_) => {
                                                PlayerKeyword::Soulshift
                                            }
                                        })
                                    }
                                    [
                                        spellwake_engine::program::Instruction::ResolveGrowth(key),
                                    ] => {
                                        use spellwake_engine::program::GrowthKeyword;
                                        Some(match key {
                                            GrowthKeyword::Mentor => PlayerKeyword::Mentor,
                                            GrowthKeyword::Increment => PlayerKeyword::Increment,
                                            GrowthKeyword::Evolve => PlayerKeyword::Evolve,
                                            GrowthKeyword::Backup { .. } => PlayerKeyword::Backup,
                                        })
                                    }
                                    _ => None,
                                },
                            }),
                    )
                    .chain((current.exalted_instances() > 0).then_some(PlayerKeyword::Exalted))
                    .chain(
                        current
                            .printed_text
                            .triggered
                            .iter()
                            .filter(|a| current.abilities.triggered.contains(&a.id))
                            .filter_map(|a| {
                                use spellwake_engine::program::TokenKeyword;
                                TokenKeyword::from_ability(
                                    a,
                                    source.execution().creature_vocabulary(),
                                )
                                .map(|k| match k {
                                    TokenKeyword::Afterlife => PlayerKeyword::Afterlife,
                                    TokenKeyword::Fabricate => PlayerKeyword::Fabricate,
                                })
                            }),
                    )
                    .chain(current.combat_trigger_keywords().map(|ability| {
                        use spellwake_engine::program::CombatTriggerKeyword;
                        match ability {
                            CombatTriggerKeyword::Decayed => PlayerKeyword::Decayed,
                            CombatTriggerKeyword::Mobilize(_) => PlayerKeyword::Mobilize,
                            CombatTriggerKeyword::Ingest => PlayerKeyword::Ingest,
                            CombatTriggerKeyword::Poisonous(_) => PlayerKeyword::Poisonous,
                            CombatTriggerKeyword::Afflict(_) => PlayerKeyword::Afflict,
                            CombatTriggerKeyword::Dethrone => PlayerKeyword::Dethrone,
                            CombatTriggerKeyword::Melee => PlayerKeyword::Melee,
                            CombatTriggerKeyword::Renown(_) => PlayerKeyword::Renown,
                            CombatTriggerKeyword::Training => PlayerKeyword::Training,
                            CombatTriggerKeyword::Prowess => PlayerKeyword::Prowess,
                            CombatTriggerKeyword::BattleCry => PlayerKeyword::BattleCry,
                            CombatTriggerKeyword::Bushido(_) => PlayerKeyword::Bushido,
                            CombatTriggerKeyword::Flanking => PlayerKeyword::Flanking,
                            CombatTriggerKeyword::Frenzy(_) => PlayerKeyword::Frenzy,
                            CombatTriggerKeyword::Rampage(_) => PlayerKeyword::Rampage,
                        }
                    }))
                    .filter(|kw| seen.insert(*kw))
                    .collect()
            },
            power: current
                .power_toughness
                .as_ref()
                .map(|value| value.derived.power.clone()),
            toughness: current
                .power_toughness
                .as_ref()
                .map(|value| value.derived.toughness.clone()),
        };
        let commander = physical
            .card_id()
            .and_then(|id| source.execution().commander_record(id))
            .map(|record| {
                let combat_damage = record
                    .combat_damage()
                    .iter()
                    .map(|(&seat, amount)| {
                        Ok(PlayerCommanderDamage {
                            seat_id: self.app_seat(seat)?,
                            amount: quantity(amount)?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                Ok::<_, NativeProjectionError>(PlayerCommanderView {
                    command_zone_casts: quantity(record.command_zone_casts())?,
                    combat_damage,
                })
            })
            .transpose()?;
        let adventure_face = if may_look {
            source
                .execution()
                .printed_face(physical)
                .and_then(|program| program.adventure.as_deref())
                .map(|face| {
                    self.display
                        .object(physical, face)
                        .map(|view| view.characteristics)
                })
                .transpose()?
        } else {
            None
        };
        let split_faces = if may_look {
            source
                .execution()
                .printed_face(physical)
                .and_then(|p| p.split.as_ref())
                .map(|split| {
                    Ok::<_, NativeProjectionError>([
                        self.display.object(physical, &split.left)?.characteristics,
                        self.display.object(physical, &split.right)?.characteristics,
                    ])
                })
                .transpose()?
        } else {
            None
        };
        let transform_face = if may_look {
            source
                .execution()
                .opposite_physical_face(id)
                .map(|other| {
                    self.display
                        .object(physical, other)
                        .map(|view| PlayerTransformFace {
                            back_face_up: physical.transformed(),
                            opposite: view.characteristics,
                        })
                })
                .transpose()?
        } else {
            None
        };
        let protector_seat_id = if physical.zone() == ObjectZone::Battlefield
            && current.has_type(spellwake_engine::program::CardType::Battle)
        {
            source
                .execution()
                .battle_protector(id)
                .map(|seat| self.app_seat(seat))
                .transpose()?
        } else {
            None
        };
        Ok(PlayerObject {
            times_mutated: (physical.times_mutated() > 0)
                .then(|| count(physical.times_mutated()))
                .transpose()?,
            protector_seat_id,
            adventure_face,
            split_faces,
            transform_face,
            commander,
            exile_mana: source
                .execution()
                .flexible_exile_mana(id, self.scope.binding.actor)
                .map(|any_type| {
                    if any_type {
                        crate::player_wire::ExileManaPermission::AnyType
                    } else {
                        crate::player_wire::ExileManaPermission::AnyColor
                    }
                }),
            may_play_from_library: matches!(physical.zone(),ObjectZone::Library(owner)
                if source.execution().window().cards().library(owner).first()==Some(&id) && source.execution().library_top_access(&self.evaluated,owner,self.scope.binding.actor).playable),
            free_exile_types: source
                .execution()
                .free_exile_types(id, self.scope.binding.actor)
                .into_iter()
                .map(card_type)
                .collect(),
            may_cast_free_from_exile: source
                .execution()
                .may_cast_free_from_exile(id, self.scope.binding.actor),
            may_play_from_exile: source
                .execution()
                .may_play_from_exile(id, self.scope.binding.actor)
                || source
                    .execution()
                    .may_cast_from_granted_exile(id, self.scope.binding.actor),
            handle,
            zone: zone.clone(),
            owner_seat_id: self.app_seat(physical.owner())?,
            controller_seat_id: current
                .controller
                .map(|seat| self.app_seat(seat))
                .transpose()?,
            identity,
            face_down,
            face_down_order,
            exiled_face_down: exiled_face_down_viewer.is_some(),
            face_up_options,
            foretold_on_turn: foretold
                .map(|turn| {
                    RulesCount::try_from(turn.to_string())
                        .map_err(|_| NativeProjectionError::InvalidSnapshot)
                })
                .transpose()?,
            public_properties: (foretold.is_none() && exiled_face_down_viewer.is_none())
                .then_some(properties),
            tapped: physical.tapped().unwrap_or(false),
            counters,
            abilities,
            damage_marked: quantity(
                &source
                    .execution()
                    .battlefield_marked_damage(id)
                    .unwrap_or_else(|| 0.into()),
            )?,
            saddled: current.saddled,
            bestowed: source.execution().is_bestowed(id),
            warped_on_turn: source
                .execution()
                .warped_on(id)
                .map(|turn| {
                    RulesCount::try_from(turn.to_string())
                        .map_err(|_| NativeProjectionError::InvalidSnapshot)
                })
                .transpose()?,
            plotted_on_turn: source
                .execution()
                .plotted_on(id)
                .map(|turn| {
                    RulesCount::try_from(turn.to_string())
                        .map_err(|_| NativeProjectionError::InvalidSnapshot)
                })
                .transpose()?,
            attached_to: current
                .attached_to
                .map(|host| {
                    VisibleSubject::try_from(self.target(host)?)
                        .map_err(|_| NativeProjectionError::MissingNativeValue)
                })
                .transpose()?,
        })
    }
    fn stack_entries(&mut self) -> Result<Vec<PlayerStackEntry>, NativeProjectionError> {
        let source = self.scope.source;
        let mut result = vec![];
        for entry in source.execution().window().entries() {
            let source_id = match entry.kind() {
                StackKind::CardSpell(id) => Some(*id),
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    Some(source.id())
                }
                StackKind::Speed
                | StackKind::Monarch
                | StackKind::DelayedCopy { .. }
                | StackKind::Emblem { .. }
                | StackKind::SpellCopy { .. } => None,
            };
            let source_object = source_id
                .filter(|id| self.visible.contains(id))
                .map(|id| self.visible_object(id))
                .transpose()?;
            let mut targets = vec![];
            let mut unavailable = 0usize;
            for bound in entry.targets().targets() {
                let target = bound.choice().target;
                let visible = match target {
                    spellwake_engine::targets::TargetRef::Object(id) => self.visible.contains(&id),
                    spellwake_engine::targets::TargetRef::Stack(id) => source
                        .execution()
                        .window()
                        .entries()
                        .iter()
                        .any(|value| value.id() == id),
                    spellwake_engine::targets::TargetRef::Player(seat) => self
                        .scope
                        .binding
                        .seats
                        .iter()
                        .any(|value| value.native_seat == seat),
                };
                if visible {
                    targets.push(self.target(target)?);
                } else {
                    unavailable = unavailable
                        .checked_add(1)
                        .ok_or(NativeProjectionError::InvalidSnapshot)?;
                }
            }
            if targets.len().checked_add(unavailable) != Some(entry.targets().targets().len()) {
                return Err(NativeProjectionError::InvalidSnapshot);
            }
            let mut visible_contributors = Vec::new();
            for &member in entry.power_contributors() {
                if self.visible.contains(&member) {
                    visible_contributors.push(self.visible_object(member)?);
                }
            }
            result.push(PlayerStackEntry {
                chosen_x: entry.chosen_x().map(quantity).transpose()?,
                modes: source.execution().chosen_spell_modes(entry.id()).collect(),
                visible_contributors,
                handle: self.issue(NativeReference::Stack(entry.id()))?,
                controller_seat_id: self.app_seat(entry.controller())?,
                source_object,
                label: match entry.kind() {
                    StackKind::Speed => "Increase speed".into(),
                    StackKind::Monarch => "Monarch ability".into(),
                    StackKind::DelayedCopy { .. } => "Delayed triggered ability".into(),
                    StackKind::Emblem { .. } => "Emblem ability".into(),
                    StackKind::CardSpell(id)
                        if source
                            .execution()
                            .window()
                            .cards()
                            .get(*id)
                            .is_some_and(|o| o.face_down().is_some()) =>
                    {
                        "Face-down creature spell".into()
                    }
                    StackKind::ActivatedAbility { source } if source.face_down().is_some() => {
                        "Activated ability".into()
                    }
                    StackKind::TriggeredAbility { source } if source.face_down().is_some() => {
                        "Triggered ability".into()
                    }
                    _ => self.display.stack(
                        entry,
                        source.execution().stack_body(entry.id()),
                        source.execution().execution_bindings(entry.id()),
                    )?,
                },
                targets,
                unavailable_targets: count(unavailable)?,
            });
        }
        Ok(result)
    }
    fn object_label(&self, handle: &PlayerHandle) -> Result<String, NativeProjectionError> {
        self.objects
            .iter()
            .find(|value| &value.handle == handle)
            .map(|value| match &value.identity {
                PlayerObjectIdentity::Known {
                    characteristics, ..
                } => characteristics.name.clone(),
                PlayerObjectIdentity::Concealed {} => "Face-down card".into(),
            })
            .ok_or(NativeProjectionError::HiddenReference)
    }
}

/// Establish the supported public damage-source role before a description lookup.
/// Captured public evidence need not still have a current physical incarnation.
pub(super) fn ensure_public_damage_source(
    source: &spellwake_engine::damage::DamageSource,
    actor: SeatId,
) -> Result<(), NativeProjectionError> {
    if let spellwake_engine::damage::DamageSource::Object { object, .. } = source
        && (matches!(object.zone(), ObjectZone::Library(_))
            || matches!(object.zone(), ObjectZone::Hand(seat) if seat != actor))
    {
        return Err(NativeProjectionError::HiddenReference);
    }
    Ok(())
}
