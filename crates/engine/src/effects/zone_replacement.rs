//! One bounded precommit reducer under the existing selected-effect cursor.
use super::*;
use crate::program::{
    PublicZoneDestination, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
    ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
    ZoneReplacementSubject,
};
use crate::{
    checks::CheckEventId,
    occurrence::{EventCause, ZoneReason},
};

/// Actual source incarnation plus its copiable definition-local identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZoneEffectInstance {
    pub source: ObjectId,
    pub ability: ZoneReplacementAbilityId,
}
/// Original selected event lineage, unchanged by destination transformations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoneEventKey {
    /// A real resolving body instruction.
    Instruction {
        entry: StackEntryId,
        instruction: usize,
        before: ObjectId,
    },
    /// A held selected action after departure, distinct for each original actor.
    /// The same incarnation may have been chosen by different actors at different cuts.
    Selection {
        entry: StackEntryId,
        instruction: usize,
        before: ObjectId,
        player: SeatId,
    },
    /// A real reserved event/pass within the Checks owner.
    Check {
        event: CheckEventId,
        before: ObjectId,
    },
}
impl ZoneEventKey {
    /// Exact original incarnation, unchanged throughout this lineage.
    pub fn before(self) -> ObjectId {
        match self {
            Self::Instruction { before, .. }
            | Self::Selection { before, .. }
            | Self::Check { before, .. } => before,
        }
    }
    #[cfg(test)]
    pub(crate) fn qualification_with_before(self, before: ObjectId) -> Self {
        match self {
            Self::Instruction {
                entry, instruction, ..
            } => Self::Instruction {
                entry,
                instruction,
                before,
            },
            Self::Selection {
                entry,
                instruction,
                player,
                ..
            } => Self::Selection {
                entry,
                instruction,
                before,
                player,
            },
            Self::Check { event, .. } => Self::Check { event, before },
        }
    }
}
/// One offered application, never a caller-supplied transformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZoneApplication {
    pub event: ZoneEventKey,
    pub effect: ZoneEffectInstance,
}
/// Optional opportunity outcome, distinct from successful effect application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneOptionalAnswer {
    /// Use the offered replacement.
    Apply,
    /// Spend this lineage's opportunity without applying it.
    Decline,
}
/// A response to an exact executor-issued replacement request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneReplacementReply {
    /// Select one event/effect pair.
    Select(ZoneApplication),
    /// Answer the specific optional confirmation.
    Optional(ZoneOptionalAnswer),
}
/// Immutable request payload; a singleton optional effect still needs confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneReplacementRequest {
    /// Earliest affected player's applicable pairs.
    Select { applications: Vec<ZoneApplication> },
    /// One already selected optional effect.
    ConfirmOptional { application: ZoneApplication },
}
/// Exact private native request. Hidden source domains remain privileged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneReplacementFrame {
    pub(crate) revision: u64,
    cause: EventCause,
    stage: u64,
    chooser: SeatId,
    request: ZoneReplacementRequest,
}
impl ZoneReplacementFrame {
    #[cfg(test)]
    pub(crate) fn qualification_with_stage(&self, stage: u64) -> Self {
        Self {
            stage,
            ..self.clone()
        }
    }
    #[cfg(test)]
    pub(crate) fn qualification_with_cause(&self, cause: EventCause) -> Self {
        Self {
            cause,
            ..self.clone()
        }
    }
    /// Accepted executor revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Exact existing cause bound to the actual owning process, never a synthetic entry.
    pub fn cause(&self) -> &EventCause {
        &self.cause
    }
    /// Modification/confirmation stage.
    pub fn stage(&self) -> u64 {
        self.stage
    }
    /// Actual affected object's controller, otherwise owner.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Complete offered request.
    pub fn request(&self) -> &ZoneReplacementRequest {
        &self.request
    }
}
/// Recoverable reducer or internally inconsistent retained-work failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneReplacementError {
    /// The exact current request was not supplied.
    WrongFrame,
    /// An effect is unavailable or has spent this lineage's opportunity.
    InapplicableApplication,
    /// Owned physical/definition/work data disagree.
    InconsistentBatch,
    /// Operational stage exhaustion, never a game loss.
    StageExhausted,
}
impl std::fmt::Display for ZoneReplacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "zone replacement: {self:?}")
    }
}
impl std::error::Error for ZoneReplacementError {}
impl From<ZoneReplacementError> for EffectError {
    fn from(value: ZoneReplacementError) -> Self {
        Self::ZoneReplacement(value)
    }
}
/// One selected attempted movement and its modified destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneCandidate {
    pub key: ZoneEventKey,
    pub before: GameObject,
    pub action: ZoneActionKind,
    pub reason: crate::occurrence::ZoneReason,
    pub original: PublicZoneDestination,
    pub current: PublicZoneDestination,
}
/// Source evidence from the complete unchanged before-world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneReplacementSource {
    pub instance: ZoneEffectInstance,
    pub before: GameObject,
    pub definition: ZoneReplacementAbility,
}
/// One consumed opportunity in actual application order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneReplacementStep {
    pub application: ZoneApplication,
    pub optional: Option<ZoneOptionalAnswer>,
}
/// Mutually exclusive reducer states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneReplacementPhase {
    /// Determine current eligible pairs.
    Selecting,
    /// Await this exact selected optional opportunity.
    Confirming { application: ZoneApplication },
    /// No applications remain.
    Settled,
}
/// Trusted consumed prefix established only after validating the previous physical cut.
/// It stores reducer facts and consumed source evidence, never a former world snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ZoneReplacementPrefix {
    destinations: BTreeMap<ZoneEventKey, PublicZoneDestination>,
    applied: BTreeMap<ZoneEventKey, BTreeSet<ZoneEffectInstance>>,
    optional: BTreeMap<ZoneApplication, ZoneOptionalAnswer>,
    history: Vec<ZoneReplacementStep>,
    application_sources: BTreeMap<ZoneApplication, ZoneReplacementSource>,
    sources: BTreeMap<ZoneEffectInstance, ZoneReplacementSource>,
    stage: u64,
    phase: ZoneReplacementPhase,
}
/// Sole precommit value. The parent cursor remains in ResolutionSelectionWork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneReplacementBatch {
    cause: EventCause,
    intents: Vec<ZoneMoveIntent>,
    window_revision: u64,
    evaluated: Arc<EvaluatedCharacteristics>,
    apnap: Vec<SeatId>,
    candidates: BTreeMap<ZoneEventKey, ZoneCandidate>,
    selection_origins: Option<BTreeMap<ZoneEventKey, ZoneCandidate>>,
    application_sources: BTreeMap<ZoneApplication, ZoneReplacementSource>,
    sources: BTreeMap<ZoneEffectInstance, ZoneReplacementSource>,
    applied: BTreeMap<ZoneEventKey, BTreeSet<ZoneEffectInstance>>,
    optional: BTreeMap<ZoneApplication, ZoneOptionalAnswer>,
    history: Vec<ZoneReplacementStep>,
    stage: u64,
    phase: ZoneReplacementPhase,
    retained_prefix: Option<Box<ZoneReplacementPrefix>>,
}
/// Single-field controls applied only to genuinely reached retained work.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum SelectionReplacementFault {
    CandidateDestination,
    CurrentSourceDefinition,
    OriginalUniverse,
    EventNamespace,
    ApplicationSourceWitness,
    RetainedDestination,
    RetainedApplicationWitness,
    RetainedFirstSource,
}
impl ZoneReplacementBatch {
    #[cfg(test)]
    pub(crate) fn qualification_corrupt_selection(&mut self, fault: SelectionReplacementFault) {
        match fault {
            SelectionReplacementFault::CandidateDestination => {
                self.candidates
                    .first_entry()
                    .expect("actual candidates")
                    .get_mut()
                    .current = PublicZoneDestination::Exile
            }
            SelectionReplacementFault::CurrentSourceDefinition => {
                self.sources
                    .first_entry()
                    .expect("actual current source")
                    .get_mut()
                    .definition
                    .replace_with = PublicZoneDestination::Graveyard
            }
            SelectionReplacementFault::OriginalUniverse => {
                let key = self
                    .selection_origins
                    .as_ref()
                    .expect("trusted selection universe")
                    .keys()
                    .find(|key| !self.candidates.contains_key(key))
                    .copied()
                    .expect("actual retired original action");
                self.selection_origins
                    .as_mut()
                    .expect("trusted selection universe")
                    .remove(&key)
                    .expect("actual retired original action");
            }
            SelectionReplacementFault::EventNamespace => {
                let (key, mut candidate) = self.candidates.pop_first().expect("actual candidate");
                let ZoneEventKey::Selection {
                    entry,
                    instruction,
                    before,
                    ..
                } = key
                else {
                    panic!("selection namespace")
                };
                candidate.key = ZoneEventKey::Instruction {
                    entry,
                    instruction,
                    before,
                };
                self.candidates.insert(candidate.key, candidate);
            }
            SelectionReplacementFault::ApplicationSourceWitness => {
                self.application_sources
                    .first_entry()
                    .expect("actual consumed witness")
                    .get_mut()
                    .definition
                    .replace_with = PublicZoneDestination::Graveyard
            }
            SelectionReplacementFault::RetainedDestination => {
                *self
                    .retained_prefix
                    .as_mut()
                    .expect("trusted cut")
                    .destinations
                    .first_entry()
                    .expect("actual retained destination")
                    .get_mut() = PublicZoneDestination::Exile
            }
            SelectionReplacementFault::RetainedApplicationWitness => {
                self.retained_prefix
                    .as_mut()
                    .expect("trusted cut")
                    .application_sources
                    .first_entry()
                    .expect("actual retained witness")
                    .get_mut()
                    .definition
                    .replace_with = PublicZoneDestination::Graveyard
            }
            SelectionReplacementFault::RetainedFirstSource => {
                self.retained_prefix
                    .as_mut()
                    .expect("trusted cut")
                    .sources
                    .first_entry()
                    .expect("actual first witness")
                    .get_mut()
                    .definition
                    .replace_with = PublicZoneDestination::Graveyard
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn qualification_exhaust_stage(&mut self) {
        self.stage = u64::MAX;
    }
    #[cfg(test)]
    pub(crate) fn qualification_corrupt_history(&mut self) {
        let step = self.history.last_mut().expect("test consumed opportunity");
        step.application.effect.ability = ZoneReplacementAbilityId(u64::MAX);
    }
    /// Exact frozen complete characteristic cut, including simultaneously departing sources.
    pub fn evaluated(&self) -> &EvaluatedCharacteristics {
        &self.evaluated
    }
    /// Physical revision held across all selection/replacement/order replies.
    pub fn window_revision(&self) -> u64 {
        self.window_revision
    }
    /// Retained actual active-player-first participant order.
    pub fn apnap(&self) -> &[SeatId] {
        &self.apnap
    }
    /// Complete neutral original movements, in the owning adapter's bound order.
    pub fn intents(&self) -> &[ZoneMoveIntent] {
        &self.intents
    }
    /// Existing event cause authenticated by the owning adapter.
    pub fn cause(&self) -> &EventCause {
        &self.cause
    }
    /// All candidate lineages.
    pub fn candidates(&self) -> &BTreeMap<ZoneEventKey, ZoneCandidate> {
        &self.candidates
    }
    /// Frozen actual source instances/definitions.
    pub fn sources(&self) -> &BTreeMap<ZoneEffectInstance, ZoneReplacementSource> {
        &self.sources
    }
    /// Successfully applied effects per lineage.
    pub fn applied(&self) -> &BTreeMap<ZoneEventKey, BTreeSet<ZoneEffectInstance>> {
        &self.applied
    }
    /// Optional outcomes; Decline never appears in applied history.
    pub fn optional(&self) -> &BTreeMap<ZoneApplication, ZoneOptionalAnswer> {
        &self.optional
    }
    /// Complete consumed-opportunity chronology.
    pub fn history(&self) -> &[ZoneReplacementStep] {
        &self.history
    }
    /// Current checked stage.
    pub fn stage(&self) -> u64 {
        self.stage
    }
    /// Sole current reducer phase.
    pub fn phase(&self) -> &ZoneReplacementPhase {
        &self.phase
    }
    /// Actual immutable source snapshot at each consumed event/effect opportunity.
    /// Unlike an aggregate source-instance lookup, this preserves earlier cuts.
    pub fn consumed_sources(&self) -> &BTreeMap<ZoneApplication, ZoneReplacementSource> {
        &self.application_sources
    }
    /// Original certified selected actions, including currently infeasible ones.
    /// These privileged choice snapshots are not proposed movements or client authority.
    pub fn selection_origins(&self) -> Option<&BTreeMap<ZoneEventKey, ZoneCandidate>> {
        self.selection_origins.as_ref()
    }
    /// Source evidence for consumed opportunities accepted before a trusted departure.
    /// Current eligibility uses `sources()` instead.
    pub fn departed_prefix_sources(
        &self,
    ) -> Option<&BTreeMap<ZoneEffectInstance, ZoneReplacementSource>> {
        self.retained_prefix.as_ref().map(|prefix| &prefix.sources)
    }
    fn restore_prefix(&mut self) -> Result<(), EffectError> {
        let Some(prefix) = &self.retained_prefix else {
            return Ok(());
        };
        if Self::first_consumed_sources(&prefix.history, &prefix.application_sources)?
            != prefix.sources
        {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        let expected: BTreeSet<_> = self
            .selection_origins
            .as_ref()
            .unwrap_or(&self.candidates)
            .keys()
            .copied()
            .collect();
        let keys = prefix.destinations.keys().copied().collect::<BTreeSet<_>>();
        if !expected.is_subset(&keys)
            || (keys != expected && !matches!(self.cause, EventCause::StateBasedActions { .. }))
        {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        for (key, candidate) in &mut self.candidates {
            candidate.current = *prefix
                .destinations
                .get(key)
                .ok_or(ZoneReplacementError::InconsistentBatch)?;
        }
        self.applied = prefix.applied.clone();
        self.optional = prefix.optional.clone();
        self.history = prefix.history.clone();
        self.application_sources = prefix.application_sources.clone();
        self.stage = prefix.stage;
        self.phase = prefix.phase.clone();
        Ok(())
    }
    /// Reauthorize unchanged original movement lineages across a trusted departure.
    pub(super) fn rebase_departure(&self, before: Self, after: Self) -> Result<Self, EffectError> {
        self.verify_state(before, false)?;
        if self.cause != after.cause
            || self.intents != after.intents
            || self.candidates.len() != after.candidates.len()
        {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        for (key, held) in &self.candidates {
            let mut original = held.clone();
            original.current = original.original;
            if after.candidates.get(key) != Some(&original) {
                return Err(ZoneReplacementError::InconsistentBatch.into());
            }
        }
        let destinations = self
            .candidates
            .iter()
            .map(|(key, candidate)| (*key, candidate.current))
            .collect();
        self.install_departure_prefix(after, destinations)
    }
    /// Re-evaluate one state-action event after a departure. Removed candidates
    /// have no physical destination, but their consumed opportunities stay in history.
    pub(super) fn rebase_state_actions(
        &self,
        before: Self,
        after: Self,
    ) -> Result<Self, EffectError> {
        self.verify_state(before, false)?;
        if self.cause != after.cause || !matches!(self.cause, EventCause::StateBasedActions { .. })
        {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        let mut destinations = self
            .retained_prefix
            .as_ref()
            .map(|prefix| prefix.destinations.clone())
            .unwrap_or_default();
        destinations.extend(
            self.candidates
                .iter()
                .map(|(key, item)| (*key, item.current)),
        );
        for (key, item) in &after.candidates {
            destinations.entry(*key).or_insert(item.original);
        }
        self.install_departure_prefix(after, destinations)
    }
    fn first_consumed_sources(
        history: &[ZoneReplacementStep],
        application_sources: &BTreeMap<ZoneApplication, ZoneReplacementSource>,
    ) -> Result<BTreeMap<ZoneEffectInstance, ZoneReplacementSource>, EffectError> {
        let mut sources = BTreeMap::new();
        if application_sources.len() != history.len() {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        for step in history {
            let source = application_sources
                .get(&step.application)
                .ok_or(ZoneReplacementError::InconsistentBatch)?;
            if source.instance != step.application.effect {
                return Err(ZoneReplacementError::InconsistentBatch.into());
            }
            sources
                .entry(source.instance)
                .or_insert_with(|| source.clone());
        }
        Ok(sources)
    }
    fn install_departure_prefix(
        &self,
        mut after: Self,
        destinations: BTreeMap<ZoneEventKey, PublicZoneDestination>,
    ) -> Result<Self, EffectError> {
        let sources = Self::first_consumed_sources(&self.history, &self.application_sources)?;
        after.retained_prefix = Some(Box::new(ZoneReplacementPrefix {
            destinations,
            applied: self.applied.clone(),
            optional: self.optional.clone(),
            history: self.history.clone(),
            application_sources: self.application_sources.clone(),
            sources,
            stage: self.stage,
            phase: ZoneReplacementPhase::Selecting,
        }));
        after.restore_prefix()?;
        if let ZoneReplacementPhase::Confirming { application } = self.phase
            && after.applicable(application)
            && after
                .sources
                .get(&application.effect)
                .is_some_and(|source| {
                    source.definition.choice == ZoneReplacementChoice::OptionalAffectedPlayer
                })
        {
            after.phase = ZoneReplacementPhase::Confirming { application };
            after
                .retained_prefix
                .as_mut()
                .ok_or(ZoneReplacementError::InconsistentBatch)?
                .phase = after.phase.clone();
        }
        Ok(after)
    }
    fn selection_key(key: ZoneEventKey, reason: ZoneReason) -> Result<ZoneEventKey, EffectError> {
        let player = match reason {
            ZoneReason::Sacrifice { player } | ZoneReason::Discard { player } => player,
            _ => return Err(ZoneReplacementError::InconsistentBatch.into()),
        };
        match key {
            ZoneEventKey::Instruction {
                entry,
                instruction,
                before,
            } => Ok(ZoneEventKey::Selection {
                entry,
                instruction,
                before,
                player,
            }),
            ZoneEventKey::Selection { player: actual, .. } if actual == player => Ok(key),
            _ => Err(ZoneReplacementError::InconsistentBatch.into()),
        }
    }
    fn same_selection_lineage(origin: &ZoneCandidate, current: &ZoneCandidate) -> bool {
        origin.key == current.key
            && origin.action == current.action
            && origin.reason == current.reason
            && origin.original == current.original
            && origin.before.id() == current.before.id()
            && origin.before.origin() == current.before.origin()
            && origin.before.owner() == current.before.owner()
            && origin.before.zone() == current.before.zone()
            && origin.before.controller() == current.before.controller()
    }
    /// Authorize the complete original choice universe and normalize trusted legacy keys.
    /// The caller supplies actual held choice snapshots, not a player payload.
    pub(super) fn with_selection_origins(
        mut self,
        values: Vec<ZoneCandidate>,
    ) -> Result<Self, EffectError> {
        let mut origins = BTreeMap::new();
        for value in values {
            if Self::selection_key(value.key, value.reason)? != value.key
                || value.before.id() != value.key.before()
                || value.original != PublicZoneDestination::Graveyard
                || value.current != value.original
                || origins.insert(value.key, value).is_some()
            {
                return Err(ZoneReplacementError::InconsistentBatch.into());
            }
        }
        if let Some(held) = &self.selection_origins {
            if held != &origins {
                return Err(ZoneReplacementError::InconsistentBatch.into());
            }
            return Ok(self);
        }
        if self.retained_prefix.is_some() {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        let keys = self
            .candidates
            .iter()
            .map(|(key, value)| Ok((*key, Self::selection_key(*key, value.reason)?)))
            .collect::<Result<BTreeMap<_, _>, EffectError>>()?;
        let event = |key: ZoneEventKey| {
            keys.get(&key).copied().ok_or(EffectError::ZoneReplacement(
                ZoneReplacementError::InconsistentBatch,
            ))
        };
        let application = |value: ZoneApplication| {
            Ok::<_, EffectError>(ZoneApplication {
                event: event(value.event)?,
                effect: value.effect,
            })
        };
        self.candidates = self
            .candidates
            .into_values()
            .map(|mut value| {
                value.key = event(value.key)?;
                Ok((value.key, value))
            })
            .collect::<Result<_, EffectError>>()?;
        self.applied = self
            .applied
            .into_iter()
            .map(|(key, value)| Ok((event(key)?, value)))
            .collect::<Result<_, EffectError>>()?;
        self.optional = self
            .optional
            .into_iter()
            .map(|(key, value)| Ok((application(key)?, value)))
            .collect::<Result<_, EffectError>>()?;
        self.application_sources = self
            .application_sources
            .into_iter()
            .map(|(key, value)| Ok((application(key)?, value)))
            .collect::<Result<_, EffectError>>()?;
        for step in &mut self.history {
            step.application = application(step.application)?;
        }
        if let ZoneReplacementPhase::Confirming { application: value } = &mut self.phase {
            *value = application(*value)?;
        }
        for (key, current) in &self.candidates {
            if origins
                .get(key)
                .is_none_or(|origin| !Self::same_selection_lineage(origin, current))
            {
                return Err(ZoneReplacementError::InconsistentBatch.into());
            }
        }
        self.selection_origins = Some(origins);
        Ok(self)
    }
    /// Reconcile feasibility only within the exact certified original actor/action universe.
    pub(super) fn rebase_selection_departure(
        &self,
        before: Self,
        after: Self,
    ) -> Result<Self, EffectError> {
        self.verify_state(before, false)?;
        let origins = after
            .selection_origins
            .as_ref()
            .ok_or(ZoneReplacementError::InconsistentBatch)?;
        let held = self
            .clone()
            .with_selection_origins(origins.values().cloned().collect())?;
        if held.cause != after.cause || held.selection_origins != after.selection_origins {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        for (key, current) in &after.candidates {
            if origins
                .get(key)
                .is_none_or(|origin| !Self::same_selection_lineage(origin, current))
            {
                return Err(ZoneReplacementError::InconsistentBatch.into());
            }
        }
        let destinations = origins
            .iter()
            .map(|(key, origin)| {
                let destination = held
                    .candidates
                    .get(key)
                    .map(|candidate| candidate.current)
                    .or_else(|| {
                        held.retained_prefix
                            .as_ref()
                            .and_then(|prefix| prefix.destinations.get(key).copied())
                    })
                    .unwrap_or(origin.original);
                (*key, destination)
            })
            .collect();
        held.install_departure_prefix(after, destinations)
    }
    pub(super) fn destination(&self, id: ObjectId) -> Result<PublicZoneDestination, EffectError> {
        self.candidates
            .values()
            .find(|c| c.before.id() == id)
            .map(|c| c.current)
            .ok_or(ZoneReplacementError::InconsistentBatch.into())
    }
    fn chooser(candidate: &ZoneCandidate) -> SeatId {
        candidate
            .before
            .controller()
            .unwrap_or(candidate.before.owner())
    }
    fn applicable(&self, application: ZoneApplication) -> bool {
        let Some(candidate) = self.candidates.get(&application.event) else {
            return false;
        };
        let Some(source) = self.sources.get(&application.effect) else {
            return false;
        };
        let definition = &source.definition;
        !self
            .applied
            .get(&application.event)
            .is_some_and(|used| used.contains(&application.effect))
            && !self.optional.contains_key(&application)
            && definition.destination == candidate.current
            && matches!(&definition.actions,ZoneActionPredicate::OneOf(actions) if actions.contains(&candidate.action))
            && match definition.objects {
                ZoneObjectKind::Either => true,
                ZoneObjectKind::Card => candidate.before.card_id().is_some(),
                ZoneObjectKind::Token => candidate.before.is_token(),
            }
            && match definition.subject {
                ZoneReplacementSubject::AnyObject => true,
                ZoneReplacementSubject::SourceSelf => {
                    candidate.before.id() == source.instance.source
                }
            }
    }
    fn applications(&self) -> Option<(SeatId, Vec<ZoneApplication>)> {
        for &actor in &self.apnap {
            let applications = self
                .candidates
                .iter()
                .filter(|(_, candidate)| Self::chooser(candidate) == actor)
                .flat_map(|(&event, _)| {
                    self.sources
                        .keys()
                        .map(move |&effect| ZoneApplication { event, effect })
                })
                .filter(|application| self.applicable(*application))
                .collect::<Vec<_>>();
            if !applications.is_empty() {
                return Some((actor, applications));
            }
        }
        None
    }
    fn bump(&mut self) -> Result<(), ZoneReplacementError> {
        self.stage = self
            .stage
            .checked_add(1)
            .ok_or(ZoneReplacementError::StageExhausted)?;
        Ok(())
    }
    fn confirm(&mut self, application: ZoneApplication) -> Result<(), ZoneReplacementError> {
        self.bump()?;
        self.phase = ZoneReplacementPhase::Confirming { application };
        Ok(())
    }
    fn apply(
        &mut self,
        application: ZoneApplication,
        answer: Option<ZoneOptionalAnswer>,
    ) -> Result<(), ZoneReplacementError> {
        if !self
            .applications()
            .is_some_and(|(_, domain)| domain.contains(&application))
        {
            return Err(ZoneReplacementError::InapplicableApplication);
        }
        let source = self
            .sources
            .get(&application.effect)
            .ok_or(ZoneReplacementError::InconsistentBatch)?;
        if (source.definition.choice == ZoneReplacementChoice::OptionalAffectedPlayer)
            != answer.is_some()
        {
            return Err(ZoneReplacementError::InconsistentBatch);
        }
        let destination = source.definition.replace_with;
        let witness = source.clone();
        self.bump()?;
        if let Some(answer) = answer {
            self.optional.insert(application, answer);
        }
        if answer != Some(ZoneOptionalAnswer::Decline) {
            self.candidates
                .get_mut(&application.event)
                .ok_or(ZoneReplacementError::InconsistentBatch)?
                .current = destination;
            self.applied
                .entry(application.event)
                .or_default()
                .insert(application.effect);
        }
        self.history.push(ZoneReplacementStep {
            application,
            optional: answer,
        });
        if self
            .application_sources
            .insert(application, witness)
            .is_some()
        {
            return Err(ZoneReplacementError::InconsistentBatch);
        }
        self.phase = ZoneReplacementPhase::Selecting;
        Ok(())
    }
    pub(super) fn frame(&self, revision: u64) -> Option<ZoneReplacementFrame> {
        let (chooser, request) = match self.phase {
            ZoneReplacementPhase::Settled => return None,
            ZoneReplacementPhase::Confirming { application } => (
                Self::chooser(self.candidates.get(&application.event)?),
                ZoneReplacementRequest::ConfirmOptional { application },
            ),
            ZoneReplacementPhase::Selecting => {
                let (chooser, applications) = self.applications()?;
                (chooser, ZoneReplacementRequest::Select { applications })
            }
        };
        Some(ZoneReplacementFrame {
            revision,
            cause: self.cause.clone(),
            stage: self.stage,
            chooser,
            request,
        })
    }
    pub(super) fn progress(
        &mut self,
        revision: u64,
    ) -> Result<Option<EffectDecision>, EffectError> {
        loop {
            if matches!(self.phase, ZoneReplacementPhase::Confirming { .. }) {
                return Ok(Some(EffectDecision::ZoneReplacement(
                    self.frame(revision)
                        .ok_or(ZoneReplacementError::InconsistentBatch)?,
                )));
            }
            let Some((_, applications)) = self.applications() else {
                self.phase = ZoneReplacementPhase::Settled;
                return Ok(None);
            };
            if applications.len() > 1 {
                return Ok(Some(EffectDecision::ZoneReplacement(
                    self.frame(revision)
                        .ok_or(ZoneReplacementError::InconsistentBatch)?,
                )));
            }
            let application = applications[0];
            match self.sources[&application.effect].definition.choice {
                ZoneReplacementChoice::Mandatory => self.apply(application, None)?,
                ZoneReplacementChoice::OptionalAffectedPlayer => self.confirm(application)?,
            }
        }
    }
    pub(super) fn accept(
        &mut self,
        revision: u64,
        frame: &ZoneReplacementFrame,
        actor: SeatId,
        reply: ZoneReplacementReply,
    ) -> Result<(), EffectError> {
        if self.frame(revision).as_ref() != Some(frame) {
            return Err(ZoneReplacementError::WrongFrame.into());
        }
        if actor != frame.chooser {
            return Err(EffectError::WrongChooser);
        }
        match (&frame.request, reply) {
            (
                ZoneReplacementRequest::Select { applications },
                ZoneReplacementReply::Select(application),
            ) if applications.contains(&application) => {
                match self.sources[&application.effect].definition.choice {
                    ZoneReplacementChoice::Mandatory => self.apply(application, None)?,
                    ZoneReplacementChoice::OptionalAffectedPlayer => self.confirm(application)?,
                }
            }
            (
                ZoneReplacementRequest::ConfirmOptional { application },
                ZoneReplacementReply::Optional(answer),
            ) => self.apply(*application, Some(answer))?,
            (ZoneReplacementRequest::Select { .. }, ZoneReplacementReply::Select(_)) => {
                return Err(ZoneReplacementError::InapplicableApplication.into());
            }
            _ => return Err(EffectError::WrongDecisionKind),
        }
        Ok(())
    }
    pub(super) fn verify_cut(&self, fresh: &Self) -> Result<(), EffectError> {
        if self.cause != fresh.cause
            || self.intents != fresh.intents
            || self.window_revision != fresh.window_revision
            || self.evaluated != fresh.evaluated
            || self.sources != fresh.sources
            || self.apnap != fresh.apnap
            || self.candidates.len() != fresh.candidates.len()
            || self.selection_origins != fresh.selection_origins
        {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        for (key, held) in &self.candidates {
            let mut original = held.clone();
            original.current = original.original;
            if fresh.candidates.get(key) != Some(&original) {
                return Err(ZoneReplacementError::InconsistentBatch.into());
            }
        }
        Ok(())
    }
    fn verify_state(&self, mut replay: Self, settled: bool) -> Result<(), EffectError> {
        self.verify_cut(&replay)?;
        if settled && self.phase != ZoneReplacementPhase::Settled {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        replay.retained_prefix = self.retained_prefix.clone();
        replay.restore_prefix()?;
        if !self.history.starts_with(&replay.history) {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        for step in &self.history[replay.history.len()..] {
            if step.optional.is_some()
                && !matches!(replay.phase, ZoneReplacementPhase::Confirming { application } if application == step.application)
            {
                replay.confirm(step.application)?;
            }
            replay.apply(step.application, step.optional)?;
        }
        match self.phase {
            ZoneReplacementPhase::Settled => {
                if replay.applications().is_some() {
                    return Err(ZoneReplacementError::InconsistentBatch.into());
                }
                replay.phase = ZoneReplacementPhase::Settled;
            }
            ZoneReplacementPhase::Confirming { application } if replay.phase != self.phase => {
                if !replay
                    .applications()
                    .is_some_and(|(_, domain)| domain.contains(&application))
                    || replay.sources.get(&application.effect).is_none_or(|s| {
                        s.definition.choice != ZoneReplacementChoice::OptionalAffectedPlayer
                    })
                {
                    return Err(ZoneReplacementError::InconsistentBatch.into());
                }
                replay.confirm(application)?;
            }
            _ => {}
        }
        if replay != *self {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        Ok(())
    }
    pub(super) fn verify_pending(&self, replay: Self) -> Result<(), EffectError> {
        self.verify_state(replay, false)
    }
    pub(super) fn verify_initial(&self, replay: Self) -> Result<(), EffectError> {
        self.verify_state(replay, true)
    }
}
/// One neutral original attempted movement. The owning adapter authenticates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneMoveIntent {
    /// Original current incarnation.
    pub before: ObjectId,
    /// Original action reason, retained when its destination changes.
    pub reason: ZoneReason,
}
pub(super) struct ZoneReplacementSources {
    evaluated: Arc<EvaluatedCharacteristics>,
    pub(super) sources: BTreeMap<ZoneEffectInstance, ZoneReplacementSource>,
}
/// Internal owner-authorized inputs, not player assertions or an instruction language.
pub(super) struct AuthorizedZoneBatch {
    pub(super) cause: EventCause,
    pub(super) action: ZoneActionKind,
    pub(super) intents: Vec<ZoneMoveIntent>,
}
impl EffectWindow {
    pub(super) fn zone_replacement_sources(&self) -> Result<ZoneReplacementSources, EffectError> {
        let evaluated = Arc::new(self.evaluated_characteristics()?);
        let mut sources = BTreeMap::new();
        for before in self
            .window
            .cards()
            .iter()
            .filter(|object| object.zone() == ObjectZone::Battlefield)
        {
            let current = evaluated
                .get(before.id())
                .ok_or(ZoneReplacementError::InconsistentBatch)?;
            for &ability in &current.abilities.zone_replacements {
                let definition = current
                    .printed_text
                    .zone_replacements
                    .iter()
                    .find(|value| value.id == ability)
                    .ok_or(ZoneReplacementError::InconsistentBatch)?
                    .clone();
                let instance = ZoneEffectInstance {
                    source: before.id(),
                    ability,
                };
                if sources
                    .insert(
                        instance,
                        ZoneReplacementSource {
                            instance,
                            before: before.clone(),
                            definition,
                        },
                    )
                    .is_some()
                {
                    return Err(ZoneReplacementError::InconsistentBatch.into());
                }
            }
        }
        Ok(ZoneReplacementSources { evaluated, sources })
    }
    pub(super) fn begin_selection_replacements(
        &self,
        cursor: &ResolutionCursor,
        operation: &ResolutionSelectionOperation,
        selections: Vec<ResolutionAcceptedSelection>,
        retain_candidates: bool,
    ) -> Result<ZoneReplacementBatch, EffectError> {
        let action = match operation {
            ResolutionSelectionOperation::Sacrifice { .. } => ZoneActionKind::EffectSacrifice,
            ResolutionSelectionOperation::Discard => ZoneActionKind::EffectDiscard,
        };
        self.begin_zone_replacements_authorized(
            AuthorizedZoneBatch {
                cause: EventCause::Instruction {
                    entry: cursor.entry,
                    instruction: cursor.instruction,
                },
                action,
                intents: selections
                    .into_iter()
                    .flat_map(|row| {
                        row.objects.into_iter().map(move |before| ZoneMoveIntent {
                            before,
                            reason: operation.reason(row.player),
                        })
                    })
                    .collect(),
            },
            retain_candidates,
        )
    }
    pub(super) fn begin_zone_replacements(
        &self,
        input: AuthorizedZoneBatch,
    ) -> Result<ZoneReplacementBatch, EffectError> {
        self.begin_zone_replacements_authorized(input, false)
    }
    pub(super) fn begin_zone_replacements_authorized(
        &self,
        input: AuthorizedZoneBatch,
        retain_candidates: bool,
    ) -> Result<ZoneReplacementBatch, EffectError> {
        let ZoneReplacementSources { evaluated, sources } = self.zone_replacement_sources()?;
        let mut candidates = BTreeMap::new();
        let action = input.action;
        match (&input.cause, action) {
            (
                EventCause::Instruction { entry, .. },
                ZoneActionKind::EffectSacrifice
                | ZoneActionKind::EffectDiscard
                | ZoneActionKind::EffectMill
                | ZoneActionKind::EffectSurveil,
            ) if self
                .window
                .resolving()
                .is_some_and(|actual| actual.id() == *entry) => {}
            (EventCause::CleanupDiscard { .. }, ZoneActionKind::CleanupDiscard)
            | (EventCause::StateBasedActions { .. }, ZoneActionKind::StateBased) => {
                self.authorize_check_zone_input(&input.cause, action, &input.intents)?;
            }
            _ => return Err(ZoneReplacementError::InconsistentBatch.into()),
        }
        // Preserve the existing selection error owner when no replacement exists.
        if retain_candidates || !sources.is_empty() {
            for intent in &input.intents {
                let object = intent.before;
                let reason_ok = matches!(
                    (action, intent.reason),
                    (
                        ZoneActionKind::EffectSacrifice,
                        ZoneReason::Sacrifice { .. }
                    ) | (
                        ZoneActionKind::EffectDiscard | ZoneActionKind::CleanupDiscard,
                        ZoneReason::Discard { .. }
                    ) | (ZoneActionKind::EffectMill, ZoneReason::Mill { .. })
                        | (ZoneActionKind::EffectSurveil, ZoneReason::Surveil { .. })
                        | (ZoneActionKind::StateBased, ZoneReason::StateBased(_))
                );
                if !reason_ok {
                    return Err(ZoneReplacementError::InconsistentBatch.into());
                }
                let before = self
                    .window
                    .cards()
                    .get(object)
                    .ok_or(ZoneReplacementError::InconsistentBatch)?
                    .clone();
                let key = match input.cause {
                    EventCause::Instruction { entry, instruction } => ZoneEventKey::Instruction {
                        entry,
                        instruction,
                        before: object,
                    },
                    EventCause::CleanupDiscard { event, .. }
                    | EventCause::StateBasedActions { event, .. } => ZoneEventKey::Check {
                        event,
                        before: object,
                    },
                    _ => return Err(ZoneReplacementError::InconsistentBatch.into()),
                };
                let candidate = ZoneCandidate {
                    key,
                    before,
                    action,
                    reason: intent.reason,
                    original: PublicZoneDestination::Graveyard,
                    current: PublicZoneDestination::Graveyard,
                };
                if candidates.insert(key, candidate).is_some() {
                    return Err(ZoneReplacementError::InconsistentBatch.into());
                }
            }
        }
        Ok(ZoneReplacementBatch {
            cause: input.cause,
            intents: input.intents,
            window_revision: self.window.revision(),
            evaluated,
            apnap: self.window.apnap_players()?.collect(),
            candidates,
            selection_origins: None,
            application_sources: BTreeMap::new(),
            sources,
            applied: BTreeMap::new(),
            optional: BTreeMap::new(),
            history: vec![],
            stage: 0,
            phase: ZoneReplacementPhase::Selecting,
            retained_prefix: None,
        })
    }
}

/// Physical-card owner domain shared by movement owners; tokens do not require ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneGraveyardGroup {
    /// Physical owner, independently of the replacement chooser.
    pub owner: SeatId,
    /// Complete final Graveyard card entrants.
    pub entrants: Vec<ObjectId>,
}
/// Exact accepted final top-first arrangement above an owner's old pile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneOwnerOrder {
    /// Owner authorized to arrange these physical cards.
    pub owner: SeatId,
    /// Complete final entrant permutation.
    pub top_to_bottom: Vec<ObjectId>,
}
pub(super) struct ZoneMovePlan {
    pub(super) logical: Vec<ObjectId>,
    pub(super) application: Vec<(ObjectId, PublicZoneDestination)>,
    pub(super) token_departures: Vec<ObjectId>,
}
impl EffectWindow {
    pub(super) fn zone_arrival_groups(
        &self,
        objects: &[ObjectId],
        replacements: Option<&ZoneReplacementBatch>,
    ) -> Result<Vec<ZoneGraveyardGroup>, EffectError> {
        let mut owners = BTreeMap::<SeatId, Vec<ObjectId>>::new();
        let mut unique = BTreeSet::new();
        for &id in objects {
            if !unique.insert(id) {
                return Err(EffectError::InconsistentSelectionWork);
            }
            let object = self
                .window
                .cards()
                .get(id)
                .ok_or(EffectError::InconsistentSelectionWork)?;
            let destination =
                replacements.map_or(Ok(PublicZoneDestination::Graveyard), |b| b.destination(id))?;
            if object.card_id().is_some() && destination == PublicZoneDestination::Graveyard {
                owners.entry(object.owner()).or_default().push(id);
            }
        }
        let mut groups = vec![];
        for owner in self.window.apnap_players()? {
            if let Some(entrants) = owners.remove(&owner) {
                groups.push(ZoneGraveyardGroup { owner, entrants });
            }
        }
        if !owners.is_empty() {
            return Err(EffectError::InconsistentSelectionWork);
        }
        Ok(groups)
    }
    pub(super) fn zone_move_plan(
        &self,
        objects: &[ObjectId],
        orders: &[ZoneOwnerOrder],
        replacements: Option<&ZoneReplacementBatch>,
    ) -> Result<ZoneMovePlan, EffectError> {
        let groups = self.zone_arrival_groups(objects, replacements)?;
        if orders.len() != groups.len() {
            return Err(EffectError::InconsistentSelectionWork);
        }
        let mut plan = ZoneMovePlan {
            logical: vec![],
            application: vec![],
            token_departures: vec![],
        };
        let mut groups = groups.iter().zip(orders).peekable();
        for owner in self.window.apnap_players()? {
            if groups.peek().is_some_and(|(g, _)| g.owner == owner) {
                let (group, order) = groups
                    .next()
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                if order.owner != owner
                    || order.top_to_bottom.len() != group.entrants.len()
                    || order.top_to_bottom.iter().copied().collect::<BTreeSet<_>>()
                        != group.entrants.iter().copied().collect()
                {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                plan.logical.extend(&order.top_to_bottom);
                plan.application.extend(
                    order
                        .top_to_bottom
                        .iter()
                        .rev()
                        .map(|id| (*id, PublicZoneDestination::Graveyard)),
                );
            }
            for &id in objects {
                let object = self
                    .window
                    .cards()
                    .get(id)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                if object.owner() == owner
                    && object.card_id().is_some()
                    && replacements
                        .is_some_and(|b| b.destination(id) == Ok(PublicZoneDestination::Exile))
                {
                    plan.logical.push(id);
                    plan.application.push((id, PublicZoneDestination::Exile));
                }
            }
            for id in objects.iter().copied().collect::<BTreeSet<_>>() {
                let object = self
                    .window
                    .cards()
                    .get(id)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                if object.owner() == owner && object.is_token() {
                    let destination = replacements
                        .map_or(Ok(PublicZoneDestination::Graveyard), |b| b.destination(id))?;
                    plan.logical.push(id);
                    plan.application.push((id, destination));
                    plan.token_departures.push(id);
                }
            }
        }
        if groups.next().is_some()
            || plan.logical.len() != objects.len()
            || plan.application.len() != objects.len()
        {
            return Err(EffectError::InconsistentSelectionWork);
        }
        Ok(plan)
    }
}
