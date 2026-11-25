//! One bounded precommit reducer under the existing selected-effect cursor.
pub(super) mod devour;
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

/// A resolving instruction created a temporary replacement for this exact object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CreatedZoneEffect {
    CorpseReturn,
    /// The owner’s optional Commander rule for a hand or library destination.
    Commander,
    Regenerate {
        entry: StackEntryId,
        instruction: usize,
    },
    ExileIfDies {
        entry: StackEntryId,
        instruction: usize,
    },
    /// CR 122.1c shield counter replaces destruction with removing a shield counter.
    ShieldCounter,
    /// CR 702.89 Umbra armor replaces destruction of enchanted permanent with removing damage and destroying the Aura.
    UmbraArmor {
        aura: ObjectId,
    },
    /// CR 122.1h finality counter replaces moving from battlefield to graveyard with exile.
    FinalityCounter,
    /// Duration-scoped temporary rule replacing any battlefield departure with exile.
    TemporaryLeaveExile(u64),
    /// Duration-scoped generalized zone replacement ability installed by a resolving effect.
    TemporaryZoneReplacement(u64),
}
/// Actual source incarnation plus its copiable definition-local identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZoneEffectInstance {
    pub source: ObjectId,
    pub ability: ZoneReplacementAbilityId,
    /// Distinguishes an unearth-created effect from every printed ability ID.
    pub unearth: bool,
    /// Replacement supplied by the card itself while in hand.
    pub madness: bool,
    /// Stack origin distinguishes repeated and copied temporary shields.
    pub created: Option<CreatedZoneEffect>,
}
/// Original selected event lineage, unchanged by destination transformations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoneEventKey {
    /// An already resolving permanent spell's battlefield entry.
    PermanentEntry {
        entry: StackEntryId,
        before: ObjectId,
    },
    /// A land special action, before its physical entry.
    LandPlay {
        before: ObjectId,
    },
    DamageFollowup {
        batch: crate::damage::DamageBatchId,
        before: ObjectId,
    },
    DrawReveal {
        before: ObjectId,
    },
    /// A cost paid for an already announced spell or activated ability.
    Cost {
        entry: StackEntryId,
        before: ObjectId,
    },
    /// A physical mana-ability source sacrificed without a stack entry.
    ManaCost {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        before: ObjectId,
    },
    /// Exact object in an owned linked-return batch.
    LinkedReturn {
        before: ObjectId,
    },
    /// A real resolving body instruction.
    Instruction {
        entry: StackEntryId,
        instruction: usize,
        before: ObjectId,
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
            | Self::Check { before, .. }
            | Self::LinkedReturn { before }
            | Self::Cost { before, .. }
            | Self::ManaCost { before, .. }
            | Self::PermanentEntry { before, .. }
            | Self::DrawReveal { before }
            | Self::LandPlay { before }
            | Self::DamageFollowup { before, .. } => before,
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
            Self::Check { event, .. } => Self::Check { event, before },
            Self::LinkedReturn { .. } => Self::LinkedReturn { before },
            Self::Cost { entry, .. } => Self::Cost { entry, before },
            Self::ManaCost {
                source, ability, ..
            } => Self::ManaCost {
                source,
                ability,
                before,
            },
            Self::PermanentEntry { entry, .. } => Self::PermanentEntry { entry, before },
            Self::DrawReveal { .. } => Self::DrawReveal { before },
            Self::LandPlay { .. } => Self::LandPlay { before },
            Self::DamageFollowup { batch, .. } => Self::DamageFollowup { batch, before },
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
    Devour(Option<ObjectId>),
    DevourGraveyardOrder(ObjectId),
    /// Controller selected for a permanent whose own entry effect precedes other replacements.
    EntryController(SeatId),
    /// Choose a donor without targeting it, or decline this copy replacement.
    EntryCopy(Option<ObjectId>),
    /// Use the mandatory copy replacement supplied by this exact Aura.
    EntryCopyFromPermanent(ObjectId),
    /// Select one event/effect pair.
    Select(ZoneApplication),
    /// Answer the specific optional confirmation.
    Optional(ZoneOptionalAnswer),
}
/// A live card or a not-yet-created token that can enter as a copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryCopySubject {
    Card(ObjectId),
    Token(crate::token::TokenBatchMember),
}
/// Immutable request payload; a singleton optional effect still needs confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneReplacementRequest {
    Devour {
        entrant: EntryCopySubject,
        choices: Vec<ObjectId>,
    },
    DevourGraveyardOrder {
        choices: Vec<ObjectId>,
    },
    /// Choose the opponent who will control an entering permanent (CR 616.1b).
    EntryController {
        object: ObjectId,
        opponents: Vec<SeatId>,
    },
    /// Choose whether and what this object copies as it enters (CR 616.1c).
    EntryCopy {
        object: EntryCopySubject,
        donors: Vec<ObjectId>,
        battlefield_donors: Vec<(ObjectId, ObjectId)>,
    },
    /// Earliest affected player's applicable pairs.
    Select {
        applications: Vec<ZoneApplication>,
    },
    /// One already selected optional effect.
    ConfirmOptional {
        application: ZoneApplication,
    },
}
/// Exact private native request. Hidden source domains remain privileged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneReplacementFrame {
    revision: u64,
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
    pub(super) fn token_copy(
        revision: u64,
        entry: StackEntryId,
        instruction: usize,
        chooser: SeatId,
        member: crate::token::TokenBatchMember,
        donors: Vec<ObjectId>,
        battlefield_donors: Vec<(ObjectId, ObjectId)>,
    ) -> Self {
        Self {
            revision,
            chooser,
            cause: EventCause::Instruction { entry, instruction },
            stage: revision,
            request: ZoneReplacementRequest::EntryCopy {
                object: EntryCopySubject::Token(member),
                donors,
                battlefield_donors,
            },
        }
    }
    /// Resolve a public option ordinal only in this exact native choice frame.
    pub fn reply_at(&self, index: usize) -> Option<ZoneReplacementReply> {
        match self.request() {
            ZoneReplacementRequest::Devour { choices, .. } => {
                if index == 0 {
                    Some(ZoneReplacementReply::Devour(None))
                } else {
                    choices
                        .get(index - 1)
                        .copied()
                        .map(|id| ZoneReplacementReply::Devour(Some(id)))
                }
            }
            ZoneReplacementRequest::DevourGraveyardOrder { choices } => choices
                .get(index)
                .copied()
                .map(ZoneReplacementReply::DevourGraveyardOrder),
            ZoneReplacementRequest::EntryCopy {
                donors,
                battlefield_donors,
                ..
            } => {
                let intrinsic_options = if donors.is_empty() {
                    0
                } else {
                    donors.len() + 1
                };
                if index < intrinsic_options {
                    Some(ZoneReplacementReply::EntryCopy(
                        index.checked_sub(1).map(|offset| donors[offset]),
                    ))
                } else {
                    battlefield_donors
                        .get(index - intrinsic_options)
                        .map(|&(aura, _)| ZoneReplacementReply::EntryCopyFromPermanent(aura))
                }
            }
            ZoneReplacementRequest::EntryController { opponents, .. } => opponents
                .get(index)
                .copied()
                .map(ZoneReplacementReply::EntryController),
            ZoneReplacementRequest::Select { applications } => applications
                .get(index)
                .copied()
                .map(ZoneReplacementReply::Select),
            ZoneReplacementRequest::ConfirmOptional { .. } => {
                [ZoneOptionalAnswer::Apply, ZoneOptionalAnswer::Decline]
                    .get(index)
                    .copied()
                    .map(ZoneReplacementReply::Optional)
            }
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
    /// This entrant stays in its starting zone because a rule prohibits entry.
    pub entry_prohibited: bool,
    /// Controller of a pending battlefield entrant, absent for other destinations.
    pub entering_controller: Option<SeatId>,
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
/// Sole precommit value. The parent cursor remains in ResolutionSelectionWork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneReplacementBatch {
    devour: devour::Devour,
    controller_domains: BTreeMap<ObjectId, (SeatId, Vec<SeatId>)>,
    selected_controllers: Vec<(ObjectId, SeatId)>,
    copy_rules: BTreeMap<ObjectId, crate::program::EntryCopyRule>,
    copy_pool: entry_copy::EntryCopyPool,
    eligible_nontoken_creatures: BTreeSet<ObjectId>,
    used_battlefield_copies: BTreeMap<ObjectId, BTreeSet<ObjectId>>,
    used_intrinsic_copies: BTreeMap<ObjectId, Vec<crate::program::EntryCopyRule>>,
    copies: BTreeMap<ObjectId, entry_copy::EntryCopyResult>,
    entry_opponents: BTreeMap<SeatId, Vec<SeatId>>,
    entry_history: Vec<(ObjectId, ZoneReplacementReply)>,
    regeneration_prohibited: bool,
    ordered_library_placement: Option<crate::program::LibraryPlacement>,
    cause: EventCause,
    intents: Vec<ZoneMoveIntent>,
    window_revision: u64,
    evaluated: Arc<EvaluatedCharacteristics>,
    apnap: Vec<SeatId>,
    candidates: BTreeMap<ZoneEventKey, ZoneCandidate>,
    sources: BTreeMap<ZoneEffectInstance, ZoneReplacementSource>,
    matching_subjects: BTreeSet<(ZoneEffectInstance, ObjectId)>,
    applied: BTreeMap<ZoneEventKey, BTreeSet<ZoneEffectInstance>>,
    optional: BTreeMap<ZoneApplication, ZoneOptionalAnswer>,
    history: Vec<ZoneReplacementStep>,
    stage: u64,
    phase: ZoneReplacementPhase,
}
struct PendingEntryCopy {
    object: ObjectId,
    chooser: SeatId,
    donors: Vec<ObjectId>,
    battlefield_donors: Vec<(ObjectId, ObjectId)>,
}

impl ZoneReplacementBatch {
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
    pub(super) fn used_madness(&self, id: ObjectId) -> bool {
        self.applied.iter().any(|(event, effects)| {
            event.before() == id && effects.iter().any(|effect| effect.madness)
        })
    }
    pub(super) fn destination(&self, id: ObjectId) -> Result<PublicZoneDestination, EffectError> {
        self.candidates
            .values()
            .find(|c| c.before.id() == id)
            .map(|c| c.current)
            .ok_or(ZoneReplacementError::InconsistentBatch.into())
    }
    pub(super) fn entry_prohibited(&self, id: ObjectId) -> bool {
        self.candidates
            .values()
            .any(|candidate| candidate.before.id() == id && candidate.entry_prohibited)
    }
    pub(super) fn regenerations(
        &self,
    ) -> impl Iterator<Item = (ObjectId, StackEntryId, usize)> + '_ {
        self.applied.iter().flat_map(|(event, effects)| {
            effects
                .iter()
                .filter_map(move |effect| match effect.created {
                    Some(CreatedZoneEffect::Regenerate { entry, instruction }) => {
                        Some((event.before(), entry, instruction))
                    }
                    _ => None,
                })
        })
    }
    pub(super) fn regenerated(&self, object: ObjectId) -> bool {
        self.regenerations().any(|(id, _, _)| id == object)
    }
    pub(super) fn shield_counter_saves(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.applied.iter().flat_map(|(event, effects)| {
            effects.iter().filter_map(move |effect| {
                matches!(effect.created, Some(CreatedZoneEffect::ShieldCounter))
                    .then_some(event.before())
            })
        })
    }
    pub(super) fn umbra_armor_saves(&self) -> impl Iterator<Item = (ObjectId, ObjectId)> + '_ {
        self.applied.iter().flat_map(|(event, effects)| {
            effects
                .iter()
                .filter_map(move |effect| match effect.created {
                    Some(CreatedZoneEffect::UmbraArmor { aura }) => Some((event.before(), aura)),
                    _ => None,
                })
        })
    }
    pub(super) fn saved_from_destruction(&self, object: ObjectId) -> bool {
        self.regenerated(object)
            || self.shield_counter_saves().any(|id| id == object)
            || self.umbra_armor_saves().any(|(id, _)| id == object)
    }
    pub(super) fn has_destruction_saves(&self) -> bool {
        self.regenerations().next().is_some()
            || self.shield_counter_saves().next().is_some()
            || self.umbra_armor_saves().next().is_some()
    }
    pub(super) fn entry_controller(&self, object: ObjectId) -> Option<SeatId> {
        self.selected_controllers
            .iter()
            .find(|(id, _)| *id == object)
            .map(|(_, seat)| *seat)
    }
    // Exposed for Wave 7 discard-to-battlefield commit callers outside this module.
    #[allow(dead_code)]
    pub(super) fn entering_controller_for(&self, object: ObjectId) -> Option<SeatId> {
        self.entry_controller(object).or_else(|| {
            self.candidates
                .values()
                .find(|candidate| candidate.before.id() == object)
                .and_then(|candidate| candidate.entering_controller)
        })
    }
    // Exposed for Wave 7 zone-move commit callers handling replaced Hand/Battlefield moves.
    #[allow(dead_code)]
    pub(super) fn physical_destination(
        &self,
        object: ObjectId,
    ) -> Result<ObjectDestination, EffectError> {
        let destination = self.destination(object)?;
        let controller = if destination == PublicZoneDestination::Battlefield {
            self.entering_controller_for(object).or_else(|| {
                self.candidates
                    .values()
                    .find(|candidate| candidate.before.id() == object)
                    .map(|candidate| candidate.before.owner())
            })
        } else {
            None
        };
        destination
            .physical(controller)
            .ok_or(ZoneReplacementError::InconsistentBatch.into())
    }
    pub(super) fn entry_controllers(&self) -> impl Iterator<Item = (ObjectId, SeatId)> + '_ {
        self.selected_controllers
            .iter()
            .filter(|(id, _)| self.destination(*id) == Ok(PublicZoneDestination::Battlefield))
            .copied()
    }
    pub(super) fn entry_copies(
        &self,
    ) -> impl Iterator<Item = (ObjectId, entry_copy::EntryCopyResult)> + '_ {
        self.copies
            .iter()
            .filter(|(id, _)| self.destination(**id) == Ok(PublicZoneDestination::Battlefield))
            .map(|(id, copy)| (*id, copy.clone()))
    }
    pub(super) fn copied_definition(&self, id: ObjectId) -> Option<&Program> {
        self.copies.get(&id).map(|copy| copy.definition.as_ref())
    }
    fn pending_copy(&self) -> Option<PendingEntryCopy> {
        if self.copy_rules.is_empty() && self.eligible_nontoken_creatures.is_empty() {
            return None;
        }
        for &player in &self.apnap {
            for candidate in self.candidates.values().filter(|candidate| {
                self.chooser(candidate) == player
                    && candidate.current == PublicZoneDestination::Battlefield
            }) {
                let object = candidate.before.id();
                let donors = self
                    .copy_rules
                    .get(&object)
                    .map_or_else(Vec::new, |rule| self.copy_pool.domain(rule, player));
                let creature = self.copies.get(&object).map_or_else(
                    || self.eligible_nontoken_creatures.contains(&object),
                    |copy| {
                        copy.definition
                            .card_types
                            .contains(&crate::program::CardType::Creature)
                    },
                );
                let auras = if creature {
                    self.copy_pool
                        .battlefield_donors(player)
                        .iter()
                        .copied()
                        .filter(|(aura, _)| {
                            self.used_battlefield_copies
                                .get(&object)
                                .is_none_or(|used| !used.contains(aura))
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                if !donors.is_empty() || !auras.is_empty() {
                    return Some(PendingEntryCopy {
                        object,
                        chooser: player,
                        donors: donors.to_vec(),
                        battlefield_donors: auras,
                    });
                }
            }
        }
        None
    }
    /// Replace only the entrant's own printed "as this enters" effects. The
    /// existing battlefield sources and created replacements still apply.
    fn set_entrant_replacements(
        sources: &mut BTreeMap<ZoneEffectInstance, ZoneReplacementSource>,
        before: &GameObject,
        rules: &[ZoneReplacementAbility],
    ) {
        let is_entry = |rule: &ZoneReplacementAbility| {
            rule.subject == ZoneReplacementSubject::SourceSelf
                && matches!(&rule.actions, ZoneActionPredicate::OneOf(actions) if actions.contains(&ZoneActionKind::EffectEnterBattlefield))
        };
        sources.retain(|instance, source| {
            instance.source != before.id()
                || instance.unearth
                || instance.madness
                || instance.created.is_some()
                || !is_entry(&source.definition)
        });
        for rule in rules.iter().filter(|rule| is_entry(rule)) {
            let instance = ZoneEffectInstance {
                source: before.id(),
                ability: rule.id,
                unearth: false,
                madness: false,
                created: None,
            };
            sources.insert(
                instance,
                ZoneReplacementSource {
                    instance,
                    before: before.clone(),
                    definition: rule.clone(),
                },
            );
        }
    }

    fn select_copy(&mut self, reply: ZoneReplacementReply) -> Result<(), EffectError> {
        let PendingEntryCopy {
            object,
            chooser: controller,
            donors,
            battlefield_donors,
        } = self
            .pending_copy()
            .ok_or(ZoneReplacementError::WrongFrame)?;
        let previous = self.copies.get(&object);
        let copy = match reply {
            ZoneReplacementReply::EntryCopy(donor) if !donors.is_empty() => {
                let rule = self
                    .copy_rules
                    .remove(&object)
                    .ok_or(ZoneReplacementError::InconsistentBatch)?;
                self.used_intrinsic_copies
                    .entry(object)
                    .or_default()
                    .push(rule.clone());
                donor
                    .map(|id| {
                        self.copy_pool.select_for_entrant(
                            &rule,
                            controller,
                            id,
                            previous,
                            Some(object),
                        )
                    })
                    .transpose()?
            }
            ZoneReplacementReply::EntryCopyFromPermanent(aura) => {
                let donor = battlefield_donors
                    .iter()
                    .find_map(|(source, donor)| (*source == aura).then_some(*donor))
                    .ok_or(ZoneReplacementError::InapplicableApplication)?;
                self.used_battlefield_copies
                    .entry(object)
                    .or_default()
                    .insert(aura);
                Some(self.copy_pool.battlefield_copy(donor, previous)?)
            }
            _ => return Err(ZoneReplacementError::InapplicableApplication.into()),
        };
        if let Some(copy) = copy {
            self.copy_rules.remove(&object);
            if let Some(rule) = &copy.definition.entry_copy
                && self
                    .used_intrinsic_copies
                    .get(&object)
                    .is_none_or(|used| !used.contains(rule))
            {
                self.copy_rules.insert(object, rule.clone());
            }
            if copy.definition.permanent().is_some_and(|card| {
                card.entry_tap == Some(crate::land::EntryTap::ChosenOpponentControls)
            }) && self.entry_controller(object).is_none()
                && let Some(opponents) = self.entry_opponents.get(&controller)
                && !opponents.is_empty()
            {
                self.controller_domains
                    .insert(object, (controller, opponents.clone()));
            }
            let candidate = self
                .candidates
                .values()
                .find(|candidate| candidate.before.id() == object)
                .ok_or(ZoneReplacementError::InconsistentBatch)?;
            Self::set_entrant_replacements(
                &mut self.sources,
                &candidate.before,
                &copy.definition.zone_replacements,
            );
            let subject = EntryCopySubject::Card(object);
            if let Some(n) = devour::multiplier(&copy.definition) {
                self.devour.rules.insert(subject, (controller, n));
            } else {
                self.devour.rules.remove(&subject);
            }
            self.copies.insert(object, copy);
        }

        self.bump()?;
        self.entry_history.push((object, reply));
        Ok(())
    }
    fn pending_controller(&self) -> Option<(ObjectId, SeatId, &[SeatId])> {
        for &player in &self.apnap {
            if let Some((&id, (_, opponents))) =
                self.controller_domains.iter().find(|(id, (chooser, _))| {
                    *chooser == player && self.entry_controller(**id).is_none()
                })
            {
                return Some((id, player, opponents));
            }
        }
        None
    }
    fn select_controller(&mut self, seat: SeatId) -> Result<(), ZoneReplacementError> {
        let (object, _, opponents) = self
            .pending_controller()
            .ok_or(ZoneReplacementError::WrongFrame)?;
        if !opponents.contains(&seat) {
            return Err(ZoneReplacementError::InapplicableApplication);
        }
        self.bump()?;
        self.selected_controllers.push((object, seat));
        self.entry_history
            .push((object, ZoneReplacementReply::EntryController(seat)));
        Ok(())
    }
    fn chooser(&self, candidate: &ZoneCandidate) -> SeatId {
        self.entry_controller(candidate.before.id())
            .or(candidate.entering_controller)
            .unwrap_or_else(|| {
                candidate
                    .before
                    .controller()
                    .unwrap_or(candidate.before.owner())
            })
    }
    fn applicable(&self, application: ZoneApplication) -> bool {
        let Some(candidate) = self.candidates.get(&application.event) else {
            return false;
        };
        let Some(source) = self.sources.get(&application.effect) else {
            return false;
        };
        if candidate.entry_prohibited || self.saved_from_destruction(candidate.before.id()) {
            return false;
        }
        if matches!(
            source.instance.created,
            Some(
                CreatedZoneEffect::Regenerate { .. }
                    | CreatedZoneEffect::ShieldCounter
                    | CreatedZoneEffect::UmbraArmor { .. }
            )
        ) {
            let destroyed = match candidate.reason {
                ZoneReason::Destroyed => true,
                ZoneReason::StateBased(reasons) => reasons.iter().all(|reason| {
                    matches!(
                        reason,
                        crate::occurrence::StateBasedMoveReason::LethalDestruction
                            | crate::occurrence::StateBasedMoveReason::DeathtouchDestruction
                    )
                }),
                _ => false,
            };
            let prohibited = matches!(
                source.instance.created,
                Some(CreatedZoneEffect::Regenerate { .. })
            ) && (self.regeneration_prohibited
                || self
                    .evaluated
                    .get(candidate.before.id())
                    .is_some_and(|view| {
                        view.blocking_restrictions.contains(
                            &crate::characteristics::BlockingRestriction::CannotBeRegenerated,
                        )
                    }));
            if prohibited || !destroyed {
                return false;
            }
        }
        let definition = &source.definition;
        !self
            .applied
            .get(&application.event)
            .is_some_and(|used| used.contains(&application.effect))
            && !self.optional.contains_key(&application)
            && (definition.destination == candidate.current
                || (source.instance.created == Some(CreatedZoneEffect::Commander)
                    && candidate.current == PublicZoneDestination::Library)
                || ((source.instance.unearth
                    || matches!(
                        source.instance.created,
                        Some(
                            CreatedZoneEffect::CorpseReturn
                                | CreatedZoneEffect::TemporaryLeaveExile(_)
                        )
                    ))
                    && matches!(
                        candidate.current,
                        PublicZoneDestination::Hand
                            | PublicZoneDestination::Library
                            | PublicZoneDestination::Command
                    )))
            && match &definition.actions {
                ZoneActionPredicate::Any => true,
                ZoneActionPredicate::OneOf(actions) => actions.contains(&candidate.action),
            }
            && match definition.objects {
                ZoneObjectKind::Either => true,
                ZoneObjectKind::Card => candidate.before.card_id().is_some(),
                ZoneObjectKind::Token => candidate.before.is_token(),
            }
            && match &definition.subject {
                ZoneReplacementSubject::AnyObject => true,
                ZoneReplacementSubject::SourceSelf => {
                    candidate.before.id() == source.instance.source
                }
                ZoneReplacementSubject::SourceSelfDiscardByOpponent => {
                    candidate.before.id() == source.instance.source
                        && candidate.action == ZoneActionKind::EffectDiscard
                        && self
                            .matching_subjects
                            .contains(&(application.effect, candidate.before.id()))
                }
                ZoneReplacementSubject::Matching { .. }
                | ZoneReplacementSubject::MatchingNotCast { .. }
                | ZoneReplacementSubject::DamagedBySourceThisTurn => self
                    .matching_subjects
                    .contains(&(application.effect, candidate.before.id())),
            }
    }
    fn applications(&self) -> Option<(SeatId, Vec<ZoneApplication>)> {
        for &actor in &self.apnap {
            let applications = self
                .candidates
                .iter()
                .filter(|(_, candidate)| self.chooser(candidate) == actor)
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
        self.bump()?;
        if let Some(answer) = answer {
            self.optional.insert(application, answer);
        }
        if answer != Some(ZoneOptionalAnswer::Decline) {
            let candidate = self
                .candidates
                .get_mut(&application.event)
                .ok_or(ZoneReplacementError::InconsistentBatch)?;
            candidate.current = destination;
            if destination == PublicZoneDestination::Battlefield
                && candidate.entering_controller.is_none()
                && candidate.before.zone() != ObjectZone::Battlefield
            {
                candidate.entering_controller = Some(candidate.before.owner());
            }
            self.applied
                .entry(application.event)
                .or_default()
                .insert(application.effect);
        }
        self.history.push(ZoneReplacementStep {
            application,
            optional: answer,
        });
        self.phase = ZoneReplacementPhase::Selecting;
        Ok(())
    }
    pub(super) fn frame(&self, revision: u64) -> Option<ZoneReplacementFrame> {
        let (chooser, request) = match self.phase {
            ZoneReplacementPhase::Settled => return None,
            ZoneReplacementPhase::Confirming { application } => (
                if application.effect.created == Some(CreatedZoneEffect::Commander) {
                    self.candidates.get(&application.event)?.before.owner()
                } else {
                    self.chooser(self.candidates.get(&application.event)?)
                },
                ZoneReplacementRequest::ConfirmOptional { application },
            ),
            ZoneReplacementPhase::Selecting => {
                if let Some((object, chooser, opponents)) = self.pending_controller() {
                    (
                        chooser,
                        ZoneReplacementRequest::EntryController {
                            object,
                            opponents: opponents.to_vec(),
                        },
                    )
                } else if let Some(PendingEntryCopy {
                    object,
                    chooser,
                    donors,
                    battlefield_donors,
                }) = self.pending_copy()
                {
                    (
                        chooser,
                        ZoneReplacementRequest::EntryCopy {
                            object: EntryCopySubject::Card(object),
                            donors,
                            battlefield_donors,
                        },
                    )
                } else {
                    if let Some((chooser, applications)) = self.applications() {
                        (chooser, ZoneReplacementRequest::Select { applications })
                    } else {
                        self.devour_choice()?
                    }
                }
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
            if self.pending_controller().is_none()
                && let Some(PendingEntryCopy {
                    donors,
                    battlefield_donors,
                    ..
                }) = self.pending_copy()
                && donors.is_empty()
                && let [(aura, _)] = battlefield_donors.as_slice()
            {
                self.select_copy(ZoneReplacementReply::EntryCopyFromPermanent(*aura))?;
                continue;
            }
            if self.pending_controller().is_some()
                || self.pending_copy().is_some()
                || matches!(self.phase, ZoneReplacementPhase::Confirming { .. })
            {
                return Ok(Some(EffectDecision::ZoneReplacement(
                    self.frame(revision)
                        .ok_or(ZoneReplacementError::InconsistentBatch)?,
                )));
            }
            let Some((_, applications)) = self.applications() else {
                if let Some(frame) = self.frame(revision) {
                    return Ok(Some(EffectDecision::ZoneReplacement(frame)));
                }
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
                ZoneReplacementRequest::Devour { .. }
                | ZoneReplacementRequest::DevourGraveyardOrder { .. },
                reply,
            ) => self.select_devour(reply)?,

            (
                ZoneReplacementRequest::EntryCopy { .. },
                reply @ (ZoneReplacementReply::EntryCopy(_)
                | ZoneReplacementReply::EntryCopyFromPermanent(_)),
            ) => self.select_copy(reply)?,
            (
                ZoneReplacementRequest::EntryController { .. },
                ZoneReplacementReply::EntryController(seat),
            ) => self.select_controller(seat)?,
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
    pub(super) fn verify(
        &self,
        window: &EffectWindow,
        cursor: &ResolutionCursor,
        operation: &ResolutionSelectionOperation,
        selections: Vec<ResolutionAcceptedSelection>,
    ) -> Result<(), EffectError> {
        self.verify_initial(window.begin_selection_replacements(cursor, operation, selections)?)
    }
    pub(super) fn verify_cut(&self, fresh: &Self) -> Result<(), EffectError> {
        let replay;
        let sources = if self.entry_history.is_empty() {
            &fresh.sources
        } else {
            let mut initial = fresh.clone();
            self.replay_entry_choices(&mut initial)?;
            replay = initial;
            &replay.sources
        };
        if self.regeneration_prohibited != fresh.regeneration_prohibited
            || self.ordered_library_placement != fresh.ordered_library_placement
            || self.cause != fresh.cause
            || self.intents != fresh.intents
            || self.window_revision != fresh.window_revision
            || self.evaluated != fresh.evaluated
            || &self.sources != sources
            || self.apnap != fresh.apnap
            || self.candidates.len() != fresh.candidates.len()
        {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        for (key, held) in &self.candidates {
            let mut original = held.clone();
            if !original.entry_prohibited {
                original.current = original.original;
                if original.original != PublicZoneDestination::Battlefield {
                    original.entering_controller = None;
                }
            }
            if fresh.candidates.get(key) != Some(&original) {
                return Err(ZoneReplacementError::InconsistentBatch.into());
            }
        }
        Ok(())
    }
    fn replay_entry_choices(&self, replay: &mut Self) -> Result<(), EffectError> {
        for (object, choice) in &self.entry_history {
            match choice {
                ZoneReplacementReply::EntryController(seat)
                    if replay
                        .pending_controller()
                        .is_some_and(|(next, _, _)| next == *object) =>
                {
                    replay.select_controller(*seat)?
                }
                ZoneReplacementReply::EntryCopy(_)
                | ZoneReplacementReply::EntryCopyFromPermanent(_)
                    if replay.pending_controller().is_none()
                        && replay
                            .pending_copy()
                            .is_some_and(|pending| pending.object == *object) =>
                {
                    replay.select_copy(choice.clone())?
                }
                _ => return Err(ZoneReplacementError::InconsistentBatch.into()),
            }
        }
        Ok(())
    }
    pub(super) fn verify_initial(&self, mut replay: Self) -> Result<(), EffectError> {
        if self.window_revision != replay.window_revision
            || self.phase != ZoneReplacementPhase::Settled
            || self.evaluated != replay.evaluated
        {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        self.replay_entry_choices(&mut replay)?;
        if replay.pending_copy().is_some() || replay.pending_controller().is_some() {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        let mut replies = self.devour.replies.iter().peekable();
        for (index, step) in self
            .history
            .iter()
            .map(Some)
            .chain(std::iter::once(None))
            .enumerate()
        {
            while let Some((_, reply)) = replies.next_if(|(position, _)| *position == index) {
                if replay.applications().is_some() {
                    return Err(ZoneReplacementError::InconsistentBatch.into());
                }
                replay.select_devour(reply.clone())?;
            }
            let Some(step) = step else {
                break;
            };
            if step.optional.is_some() {
                replay.confirm(step.application)?;
            }
            replay.apply(step.application, step.optional)?;
        }
        if replay.applications().is_some() || replay.devour_choice().is_some() {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        replay.phase = ZoneReplacementPhase::Settled;
        if replay != *self {
            return Err(ZoneReplacementError::InconsistentBatch.into());
        }
        Ok(())
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
    pub(super) fn graveyard_and_library_restricted(evaluated: &EvaluatedCharacteristics) -> bool {
        evaluated.objects.values().any(|object| {
            object.has_battlefield_rule(|rule| {
                matches!(
                    rule,
                    crate::characteristics::StaticContinuousAbility::Restriction {
                        rule:
                            crate::characteristics::StaticRestriction::RestrictGraveyardAndLibrary,
                        ..
                    }
                )
            })
        })
    }
    pub(super) fn zone_replacement_sources(&self) -> Result<ZoneReplacementSources, EffectError> {
        let evaluated = Arc::new(self.evaluated_characteristics()?);
        let mut sources = BTreeMap::new();
        for before in self
            .window
            .cards()
            .iter()
            .filter(|object| matches!(object.zone(), ObjectZone::Hand(_)))
        {
            let current = evaluated
                .get(before.id())
                .ok_or(ZoneReplacementError::InconsistentBatch)?;
            if current.printed_text.madness.is_some() {
                let instance = ZoneEffectInstance {
                    source: before.id(),
                    ability: ZoneReplacementAbilityId(0),
                    unearth: false,
                    madness: true,
                    created: None,
                };
                let definition = ZoneReplacementAbility {
                    id: instance.ability,
                    actions: ZoneActionPredicate::OneOf(
                        [
                            ZoneActionKind::EffectDiscard,
                            ZoneActionKind::DiscardCost,
                            ZoneActionKind::CleanupDiscard,
                        ]
                        .into(),
                    ),
                    objects: ZoneObjectKind::Card,
                    subject: ZoneReplacementSubject::SourceSelf,
                    destination: PublicZoneDestination::Graveyard,
                    replace_with: PublicZoneDestination::Exile,
                    choice: ZoneReplacementChoice::Mandatory,
                };
                sources.insert(
                    instance,
                    ZoneReplacementSource {
                        instance,
                        before: before.clone(),
                        definition,
                    },
                );
            }
            for &ability in &current.abilities.zone_replacements {
                let definition = current
                    .printed_text
                    .zone_replacements
                    .iter()
                    .find(|value| value.id == ability)
                    .ok_or(ZoneReplacementError::InconsistentBatch)?;
                if !matches!(
                    definition.subject,
                    ZoneReplacementSubject::SourceSelf
                        | ZoneReplacementSubject::SourceSelfDiscardByOpponent
                ) {
                    continue;
                }
                let instance = ZoneEffectInstance {
                    source: before.id(),
                    ability,
                    unearth: false,
                    madness: false,
                    created: None,
                };
                if sources
                    .insert(
                        instance,
                        ZoneReplacementSource {
                            instance,
                            before: before.clone(),
                            definition: definition.clone(),
                        },
                    )
                    .is_some()
                {
                    return Err(ZoneReplacementError::InconsistentBatch.into());
                }
            }
        }
        for before in self
            .window
            .cards()
            .iter()
            .filter(|object| object.zone() == ObjectZone::Battlefield)
        {
            let current = evaluated
                .get(before.id())
                .ok_or(ZoneReplacementError::InconsistentBatch)?;
            if let Some(origins) = self.damage_exile_until_cleanup.get(&before.id()) {
                for &(entry, instruction) in origins {
                    let instance = ZoneEffectInstance {
                        source: before.id(),
                        ability: ZoneReplacementAbilityId(0),
                        unearth: false,
                        madness: false,
                        created: Some(CreatedZoneEffect::ExileIfDies { entry, instruction }),
                    };
                    let definition = ZoneReplacementAbility {
                        id: instance.ability,
                        actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                            ZoneActionKind::StateBased,
                            ZoneActionKind::EffectDestroy,
                            ZoneActionKind::EffectSacrifice,
                            ZoneActionKind::SacrificeCost,
                        ])),
                        objects: ZoneObjectKind::Either,
                        subject: ZoneReplacementSubject::SourceSelf,
                        destination: PublicZoneDestination::Graveyard,
                        replace_with: PublicZoneDestination::Exile,
                        choice: ZoneReplacementChoice::Mandatory,
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
            if let Some(shields) = self.regeneration_shields.get(&before.id()) {
                for &(entry, instruction) in shields {
                    let instance = ZoneEffectInstance {
                        source: before.id(),
                        ability: ZoneReplacementAbilityId(0),
                        unearth: false,
                        madness: false,
                        created: Some(CreatedZoneEffect::Regenerate { entry, instruction }),
                    };
                    let definition = ZoneReplacementAbility {
                        id: instance.ability,
                        actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                            ZoneActionKind::StateBased,
                            ZoneActionKind::EffectDestroy,
                        ])),
                        destination: PublicZoneDestination::Graveyard,
                        replace_with: PublicZoneDestination::Battlefield,
                        objects: ZoneObjectKind::Either,
                        subject: ZoneReplacementSubject::SourceSelf,
                        choice: ZoneReplacementChoice::Mandatory,
                    };
                    sources.insert(
                        instance,
                        ZoneReplacementSource {
                            instance,
                            before: before.clone(),
                            definition,
                        },
                    );
                }
            }
            if self
                .counters_of_kind(before.id(), CounterKind::Shield)
                .is_positive()
            {
                let instance = ZoneEffectInstance {
                    source: before.id(),
                    ability: ZoneReplacementAbilityId(0),
                    unearth: false,
                    madness: false,
                    created: Some(CreatedZoneEffect::ShieldCounter),
                };
                let definition = ZoneReplacementAbility {
                    id: instance.ability,
                    actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                        ZoneActionKind::StateBased,
                        ZoneActionKind::EffectDestroy,
                    ])),
                    destination: PublicZoneDestination::Graveyard,
                    replace_with: PublicZoneDestination::Battlefield,
                    objects: ZoneObjectKind::Either,
                    subject: ZoneReplacementSubject::SourceSelf,
                    choice: ZoneReplacementChoice::Mandatory,
                };
                sources.insert(
                    instance,
                    ZoneReplacementSource {
                        instance,
                        before: before.clone(),
                        definition,
                    },
                );
            }
            for aura in evaluated.objects.values().filter(|p| {
                p.zone == ObjectZone::Battlefield
                    && p.attached_to == Some(TargetRef::Object(before.id()))
                    && p.has_keyword(KeywordAbility::UmbraArmor)
            }) {
                let instance = ZoneEffectInstance {
                    source: before.id(),
                    ability: ZoneReplacementAbilityId(0),
                    unearth: false,
                    madness: false,
                    created: Some(CreatedZoneEffect::UmbraArmor { aura: aura.object }),
                };
                let definition = ZoneReplacementAbility {
                    id: instance.ability,
                    actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                        ZoneActionKind::StateBased,
                        ZoneActionKind::EffectDestroy,
                    ])),
                    destination: PublicZoneDestination::Graveyard,
                    replace_with: PublicZoneDestination::Battlefield,
                    objects: ZoneObjectKind::Either,
                    subject: ZoneReplacementSubject::SourceSelf,
                    choice: ZoneReplacementChoice::Mandatory,
                };
                sources.insert(
                    instance,
                    ZoneReplacementSource {
                        instance,
                        before: before.clone(),
                        definition,
                    },
                );
            }
            if self
                .counters_of_kind(before.id(), CounterKind::Finality)
                .is_positive()
            {
                let instance = ZoneEffectInstance {
                    source: before.id(),
                    ability: ZoneReplacementAbilityId(0),
                    unearth: false,
                    madness: false,
                    created: Some(CreatedZoneEffect::FinalityCounter),
                };
                let definition = ZoneReplacementAbility {
                    id: instance.ability,
                    actions: ZoneActionPredicate::Any,
                    objects: ZoneObjectKind::Either,
                    subject: ZoneReplacementSubject::SourceSelf,
                    destination: PublicZoneDestination::Graveyard,
                    replace_with: PublicZoneDestination::Exile,
                    choice: ZoneReplacementChoice::Mandatory,
                };
                sources.insert(
                    instance,
                    ZoneReplacementSource {
                        instance,
                        before: before.clone(),
                        definition,
                    },
                );
            }
            if before.zone() == ObjectZone::Battlefield {
                for active in &self.temporary_rules {
                    if matches!(
                        active.rule,
                        crate::program::TemporaryRuleKind::ExileIfLeavesBattlefield { .. }
                    ) && active.resolved_objects.contains(&before.id())
                    {
                        let instance = ZoneEffectInstance {
                            source: before.id(),
                            ability: ZoneReplacementAbilityId(0),
                            unearth: false,
                            madness: false,
                            created: Some(CreatedZoneEffect::TemporaryLeaveExile(active.id)),
                        };
                        let definition = ZoneReplacementAbility {
                            id: instance.ability,
                            actions: ZoneActionPredicate::Any,
                            objects: ZoneObjectKind::Either,
                            subject: ZoneReplacementSubject::SourceSelf,
                            destination: PublicZoneDestination::Graveyard,
                            replace_with: PublicZoneDestination::Exile,
                            choice: ZoneReplacementChoice::Mandatory,
                        };
                        sources.insert(
                            instance,
                            ZoneReplacementSource {
                                instance,
                                before: before.clone(),
                                definition,
                            },
                        );
                    }
                }
            }
            if before.is_unearthed() || before.has_corpse_return() {
                let instance = ZoneEffectInstance {
                    source: before.id(),
                    ability: ZoneReplacementAbilityId(0),
                    unearth: before.is_unearthed(),
                    madness: false,
                    created: before
                        .has_corpse_return()
                        .then_some(CreatedZoneEffect::CorpseReturn),
                };
                let definition = ZoneReplacementAbility {
                    id: instance.ability,
                    actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                        ZoneActionKind::StateBased,
                        ZoneActionKind::EffectDestroy,
                        ZoneActionKind::EffectReturnSelected,
                        ZoneActionKind::EffectPutIntoLibrary,
                        ZoneActionKind::EffectSacrifice,
                        ZoneActionKind::SacrificeCost,
                    ])),
                    objects: ZoneObjectKind::Card,
                    subject: ZoneReplacementSubject::SourceSelf,
                    destination: PublicZoneDestination::Graveyard,
                    replace_with: PublicZoneDestination::Exile,
                    choice: ZoneReplacementChoice::Mandatory,
                };
                sources.insert(
                    instance,
                    ZoneReplacementSource {
                        instance,
                        before: before.clone(),
                        definition,
                    },
                );
            }
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
                    unearth: false,
                    madness: false,
                    created: None,
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
        for active in &self.temporary_rules {
            if active.consumed {
                continue;
            }
            if let crate::program::TemporaryRuleKind::TemporaryZoneReplacement(ability) =
                &active.rule
            {
                let before = active
                    .source_object
                    .and_then(|id| self.window.cards().get(id).cloned())
                    .unwrap_or_else(|| GameObject::synthetic_emblem(active.controller));
                let instance = ZoneEffectInstance {
                    source: before.id(),
                    ability: ability.id,
                    unearth: false,
                    madness: false,
                    created: Some(CreatedZoneEffect::TemporaryZoneReplacement(active.id)),
                };
                sources.insert(
                    instance,
                    ZoneReplacementSource {
                        instance,
                        before,
                        definition: (**ability).clone(),
                    },
                );
            }
        }
        Ok(ZoneReplacementSources { evaluated, sources })
    }
    pub(super) fn begin_selection_replacements(
        &self,
        cursor: &ResolutionCursor,
        operation: &ResolutionSelectionOperation,
        selections: Vec<ResolutionAcceptedSelection>,
    ) -> Result<ZoneReplacementBatch, EffectError> {
        let action = match operation {
            ResolutionSelectionOperation::ShuffleAnnouncedTargets { .. }
            | ResolutionSelectionOperation::PutInLibrary { .. }
            | ResolutionSelectionOperation::PutHandCardOnBottom => {
                ZoneActionKind::EffectPutIntoLibrary
            }
            ResolutionSelectionOperation::ReturnAnnouncedTargets { .. }
            | ResolutionSelectionOperation::ReturnSingle { .. } => {
                ZoneActionKind::EffectReturnSelected
            }
            ResolutionSelectionOperation::ChooseVow
            | ResolutionSelectionOperation::ChooseCreaturesToKeep
            | ResolutionSelectionOperation::AttachToSource { .. }
            | ResolutionSelectionOperation::EnterAurasAttachedToSource { .. }
            | ResolutionSelectionOperation::CopyDifferentlyNamedTokens
            | ResolutionSelectionOperation::Bolster { .. }
            | ResolutionSelectionOperation::Amass { .. }
            | ResolutionSelectionOperation::Untap { .. }
            | ResolutionSelectionOperation::TapOrUntapControlled { .. }
            | ResolutionSelectionOperation::Blight { .. }
            | ResolutionSelectionOperation::CollectEvidence { .. }
            | ResolutionSelectionOperation::SelectZoneCards { .. } => {
                return Err(EffectError::InconsistentSelectionWork);
            }
            ResolutionSelectionOperation::DestroyAnnouncedTargets { .. }
            | ResolutionSelectionOperation::DestroySingle { .. } => ZoneActionKind::EffectDestroy,
            ResolutionSelectionOperation::ExileAnnouncedTargets { .. }
            | ResolutionSelectionOperation::ExileLibrarySegment { .. }
            | ResolutionSelectionOperation::ExileGraveyardTargets { .. }
            | ResolutionSelectionOperation::ExileChosenGraveyardCards { .. }
            | ResolutionSelectionOperation::ExileLinkedTargets { .. }
            | ResolutionSelectionOperation::ExileChosen { .. }
            | ResolutionSelectionOperation::ExileFromZone { .. }
            | ResolutionSelectionOperation::ExileFromHand { .. }
            | ResolutionSelectionOperation::ExileSingle { .. } => ZoneActionKind::EffectExile,
            ResolutionSelectionOperation::WholeGroup { destination, .. } => match destination {
                PublicZoneDestination::Hand => ZoneActionKind::EffectReturnSelected,
                PublicZoneDestination::Exile => ZoneActionKind::EffectExile,
                PublicZoneDestination::Graveyard => ZoneActionKind::EffectDestroy,
                PublicZoneDestination::Battlefield => ZoneActionKind::EffectEnterBattlefield,
                PublicZoneDestination::Command => {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                PublicZoneDestination::Library => ZoneActionKind::EffectShuffleGraveyard,
            },
            ResolutionSelectionOperation::OtherControlledCreature {
                sacrifice: true, ..
            }
            | ResolutionSelectionOperation::SacrificeExceptVowed { .. }
            | ResolutionSelectionOperation::SacrificeContributor { .. }
            | ResolutionSelectionOperation::SacrificeColored
            | ResolutionSelectionOperation::Sacrifice { .. }
            | ResolutionSelectionOperation::SacrificeMatching { .. }
            | ResolutionSelectionOperation::RandomSacrifice { .. }
            | ResolutionSelectionOperation::PayLifeToSaveCreatures { .. } => {
                ZoneActionKind::EffectSacrifice
            }
            ResolutionSelectionOperation::MillEachPlayer { .. } => ZoneActionKind::EffectMill,
            ResolutionSelectionOperation::LibraryCohortToGraveyard { .. } => {
                ZoneActionKind::EffectRevealedToGraveyard
            }
            ResolutionSelectionOperation::RevealedHand {
                action: crate::program::ChosenHandAction::Exile,
                ..
            } => ZoneActionKind::EffectExile,
            ResolutionSelectionOperation::RevealedHand {
                action: crate::program::ChosenHandAction::Discard,
                ..
            }
            | ResolutionSelectionOperation::DiscardUpTo
            | ResolutionSelectionOperation::Discard
            | ResolutionSelectionOperation::DiscardFiltered { .. }
            | ResolutionSelectionOperation::RandomDiscard { .. } => ZoneActionKind::EffectDiscard,
            ResolutionSelectionOperation::OtherControlledCreature {
                sacrifice: false, ..
            }
            | ResolutionSelectionOperation::OwnedCommandersToHand
            | ResolutionSelectionOperation::ReturnToHand { .. }
            | ResolutionSelectionOperation::GraveyardCreatureToHand
            | ResolutionSelectionOperation::ReturnGraveyardCardsToHand { .. } => {
                ZoneActionKind::EffectReturnSelected
            }
        };
        let mut batch = self.begin_zone_replacements(AuthorizedZoneBatch {
            cause: EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            action,
            intents: selections
                .into_iter()
                .flat_map(|row| {
                    row.objects.into_iter().map(move |before| {
                        Ok(ZoneMoveIntent {
                            before,
                            reason: operation.reason(row.player)?,
                        })
                    })
                })
                .collect::<Result<Vec<_>, EffectError>>()?,
        })?;
        if let ResolutionSelectionOperation::PutInLibrary { placement, .. } = operation {
            batch.ordered_library_placement = Some(*placement);
        }
        Ok(batch)
    }
    pub(super) fn begin_zone_replacements(
        &self,
        input: AuthorizedZoneBatch,
    ) -> Result<ZoneReplacementBatch, EffectError> {
        self.begin_zone_replacements_with_entry(input, None)
    }
    pub(super) fn begin_zone_replacements_with_entry(
        &self,
        input: AuthorizedZoneBatch,
        entering_controllers: Option<&BTreeMap<ObjectId, SeatId>>,
    ) -> Result<ZoneReplacementBatch, EffectError> {
        let ZoneReplacementSources {
            evaluated,
            mut sources,
        } = self.zone_replacement_sources()?;
        for intent in &input.intents {
            if input.action == ZoneActionKind::EffectEnterBattlefield
                && self.face_down_entry_for(intent.before).is_some()
            {
                continue;
            }
            let Some(before) = self.window.cards().get(intent.before) else {
                continue;
            };
            if before.zone() == ObjectZone::Battlefield {
                continue;
            }
            let current = evaluated
                .get(before.id())
                .ok_or(ZoneReplacementError::InconsistentBatch)?;
            for &ability in &current.abilities.zone_replacements {
                let definition = current
                    .printed_text
                    .zone_replacements
                    .iter()
                    .find(|value| value.id == ability)
                    .ok_or(ZoneReplacementError::InconsistentBatch)?;
                if !matches!(
                    definition.subject,
                    ZoneReplacementSubject::SourceSelf
                        | ZoneReplacementSubject::SourceSelfDiscardByOpponent
                ) {
                    continue;
                }
                let instance = ZoneEffectInstance {
                    source: before.id(),
                    ability,
                    unearth: false,
                    madness: false,
                    created: None,
                };
                sources
                    .entry(instance)
                    .or_insert_with(|| ZoneReplacementSource {
                        instance,
                        before: before.clone(),
                        definition: definition.clone(),
                    });
            }
        }
        if input.action == ZoneActionKind::EffectEnterBattlefield {
            for intent in &input.intents {
                if self.face_down_entry_for(intent.before).is_some() {
                    continue;
                }
                let before = self
                    .window
                    .cards()
                    .get(intent.before)
                    .ok_or(ZoneReplacementError::InconsistentBatch)?;
                let face = self
                    .prospective_entry_face(intent.before)
                    .ok_or(ZoneReplacementError::InconsistentBatch)?;
                ZoneReplacementBatch::set_entrant_replacements(
                    &mut sources,
                    before,
                    &face.zone_replacements,
                );
            }
        }
        let mut candidates = BTreeMap::new();
        let action = input.action;
        if action == ZoneActionKind::EffectSacrifice
            && let Some(entry) = self.window.resolving()
        {
            for intent in &input.intents {
                if let Some(player) = evaluated
                    .get(intent.before)
                    .and_then(|card| card.controller)
                    && self.opposing_sacrifice_prohibited(&evaluated, player, entry.controller())?
                {
                    return Err(EffectError::InconsistentSelectionWork);
                }
            }
        }
        if action == ZoneActionKind::SacrificeCost
            && input.intents.iter().any(|intent| {
                evaluated.sacrifice_prohibited(intent.before)
                    || (evaluated.life_and_creature_costs_prohibited()
                        && evaluated
                            .get(intent.before)
                            .is_some_and(|card| card.is_creature()))
            })
        {
            return Err(EffectError::InvalidCastResource);
        }
        match (&input.cause, action) {
            (
                EventCause::Instruction { entry, .. },
                ZoneActionKind::EffectSacrifice
                | ZoneActionKind::EffectExile
                | ZoneActionKind::EffectDestroy
                | ZoneActionKind::EffectReturnSelected
                | ZoneActionKind::EffectLookToHand
                | ZoneActionKind::EffectDiscard
                | ZoneActionKind::EffectPutIntoLibrary
                | ZoneActionKind::EffectShuffleGraveyard
                | ZoneActionKind::EffectRevealedToGraveyard
                | ZoneActionKind::EffectSearchToGraveyard
                | ZoneActionKind::EffectMill
                | ZoneActionKind::EffectSurveil
                | ZoneActionKind::EffectEnterBattlefield
                | ZoneActionKind::Countered,
            ) if self
                .window
                .resolving()
                .is_some_and(|actual| actual.id() == *entry) => {}
            (EventCause::Draw { .. }, ZoneActionKind::EffectLookToHand) => {}
            (
                EventCause::DamageAdjusted { batch },
                ZoneActionKind::EffectExile | ZoneActionKind::EffectMill,
            ) if matches!(&self.pending_work, Some(PendingWork::Damage(work)) if work.results_committed && work.batch.id() == *batch) =>
                {}
            (EventCause::CleanupDiscard { .. }, ZoneActionKind::CleanupDiscard)
            | (EventCause::StateBasedActions { .. }, ZoneActionKind::StateBased) => {
                self.authorize_check_zone_input(&input.cause, action, &input.intents)?;
            }
            (
                EventCause::MechanismUnit | EventCause::LandPlay { .. },
                ZoneActionKind::EffectEnterBattlefield,
            ) if entering_controllers.is_some() => {}
            (
                EventCause::ResolutionCompleted { entry, .. },
                ZoneActionKind::EffectEnterBattlefield,
            ) if self
                .window
                .resolving()
                .is_some_and(|actual| actual.id() == *entry)
                && entering_controllers.is_some() => {}
            (
                EventCause::ResolutionCompleted { entry, .. },
                ZoneActionKind::ResolutionExit | ZoneActionKind::Countered,
            ) if self
                .window
                .resolving()
                .is_some_and(|actual| actual.id() == *entry) => {}
            (
                EventCause::ActivatedAbilityCost { entry, .. },
                ZoneActionKind::MillCost
                | ZoneActionKind::DiscardCost
                | ZoneActionKind::SacrificeCost
                | ZoneActionKind::ExileCost,
            )
            | (
                EventCause::CastObjectPayment { entry, .. },
                ZoneActionKind::ExileCost
                | ZoneActionKind::DiscardCost
                | ZoneActionKind::SacrificeCost,
            ) if self
                .window
                .entries()
                .iter()
                .any(|actual| actual.id() == *entry) => {}
            (EventCause::ManaAbilityCost { .. }, ZoneActionKind::SacrificeCost)
                if input.intents.len() == 1 => {}
            _ => return Err(ZoneReplacementError::InconsistentBatch.into()),
        }
        // Preserve the existing selection error owner when no replacement exists.
        let destination = match action {
            ZoneActionKind::EffectReturnSelected | ZoneActionKind::EffectLookToHand => {
                PublicZoneDestination::Hand
            }
            ZoneActionKind::EffectPutIntoLibrary | ZoneActionKind::EffectShuffleGraveyard => {
                PublicZoneDestination::Library
            }
            ZoneActionKind::EffectExile | ZoneActionKind::ExileCost => PublicZoneDestination::Exile,
            ZoneActionKind::EffectEnterBattlefield => PublicZoneDestination::Battlefield,
            _ => PublicZoneDestination::Graveyard,
        };
        let restrict_entry = action == ZoneActionKind::EffectEnterBattlefield
            && Self::graveyard_and_library_restricted(&evaluated);
        if !sources.is_empty()
            || destination != PublicZoneDestination::Graveyard
            || matches!(
                action,
                ZoneActionKind::DiscardCost
                    | ZoneActionKind::SacrificeCost
                    | ZoneActionKind::MillCost
                    | ZoneActionKind::EffectMill
                    | ZoneActionKind::EffectRevealedToGraveyard
                    | ZoneActionKind::EffectSearchToGraveyard
                    | ZoneActionKind::ResolutionExit
                    | ZoneActionKind::Countered
            )
        {
            for intent in &input.intents {
                let object = intent.before;
                let reason_ok = matches!(
                    (action, intent.reason),
                    (
                        ZoneActionKind::EffectSacrifice,
                        ZoneReason::Sacrifice { .. }
                    ) | (
                        ZoneActionKind::EffectDiscard
                            | ZoneActionKind::DiscardCost
                            | ZoneActionKind::CleanupDiscard,
                        ZoneReason::Discard { .. }
                    ) | (ZoneActionKind::EffectExile, ZoneReason::ExiledByEffect)
                        | (ZoneActionKind::EffectDestroy, ZoneReason::Destroyed)
                        | (
                            ZoneActionKind::EffectReturnSelected,
                            ZoneReason::ReturnToOwnersHand
                        )
                        | (ZoneActionKind::EffectLookToHand, ZoneReason::LookTopToHand)
                        | (
                            ZoneActionKind::EffectPutIntoLibrary,
                            ZoneReason::ShufflePermanentIntoLibrary
                                | ZoneReason::ExiledCardsToLibraryBottom
                                | ZoneReason::PutIntoOwnersLibrary(_)
                        )
                        | (
                            ZoneActionKind::EffectShuffleGraveyard,
                            ZoneReason::ShuffleGraveyardIntoLibrary
                                | ZoneReason::ShuffleHandAndGraveyardIntoLibrary
                        )
                        | (
                            ZoneActionKind::EffectRevealedToGraveyard,
                            ZoneReason::RevealedToGraveyard
                                | ZoneReason::PutLibraryCohortIntoGraveyard
                        )
                        | (
                            ZoneActionKind::EffectSearchToGraveyard,
                            ZoneReason::SearchToGraveyard
                        )
                        | (
                            ZoneActionKind::EffectMill | ZoneActionKind::MillCost,
                            ZoneReason::Mill { .. }
                        )
                        | (ZoneActionKind::EffectSurveil, ZoneReason::Surveil { .. })
                        | (
                            ZoneActionKind::EffectEnterBattlefield,
                            ZoneReason::ReturnFromLinkedExile
                                | ZoneReason::ReturnFromGraveyardToBattlefield
                                | ZoneReason::ReturnFromSacrifice
                                | ZoneReason::PermanentResolution
                                | ZoneReason::LandPlay
                                | ZoneReason::UnearthReturn
                                | ZoneReason::NinjutsuEntry
                                | ZoneReason::PersistReturn
                                | ZoneReason::UndyingReturn
                                | ZoneReason::SearchToBattlefield
                                | ZoneReason::Manifest
                                | ZoneReason::ExileToBattlefield
                                | ZoneReason::HandToBattlefield
                                | ZoneReason::LookTopToBattlefield
                                | ZoneReason::TransfigureEntry
                        )
                        | (ZoneActionKind::SacrificeCost, ZoneReason::SacrificeCost)
                        | (ZoneActionKind::ExileCost, ZoneReason::ExileCost)
                        | (ZoneActionKind::StateBased, ZoneReason::StateBased(_))
                        | (
                            ZoneActionKind::ResolutionExit,
                            ZoneReason::ResolutionExit | ZoneReason::AllTargetsIllegalExit,
                        )
                        | (ZoneActionKind::Countered, ZoneReason::Countered)
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
                    EventCause::DamageAdjusted { batch }
                        if matches!(
                            action,
                            ZoneActionKind::EffectExile | ZoneActionKind::EffectMill
                        ) =>
                    {
                        ZoneEventKey::DamageFollowup {
                            batch,
                            before: object,
                        }
                    }
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
                    EventCause::MechanismUnit
                        if action == ZoneActionKind::EffectEnterBattlefield =>
                    {
                        ZoneEventKey::LinkedReturn { before: object }
                    }
                    EventCause::Draw { .. } if action == ZoneActionKind::EffectLookToHand => {
                        ZoneEventKey::DrawReveal { before: object }
                    }
                    EventCause::LandPlay { .. }
                        if action == ZoneActionKind::EffectEnterBattlefield =>
                    {
                        ZoneEventKey::LandPlay { before: object }
                    }
                    EventCause::ResolutionCompleted { entry, .. }
                        if matches!(
                            action,
                            ZoneActionKind::EffectEnterBattlefield
                                | ZoneActionKind::ResolutionExit
                                | ZoneActionKind::Countered
                        ) =>
                    {
                        ZoneEventKey::PermanentEntry {
                            entry,
                            before: object,
                        }
                    }
                    EventCause::ActivatedAbilityCost { entry, .. }
                    | EventCause::CastObjectPayment { entry, .. }
                        if matches!(
                            action,
                            ZoneActionKind::MillCost
                                | ZoneActionKind::DiscardCost
                                | ZoneActionKind::SacrificeCost
                                | ZoneActionKind::ExileCost
                        ) =>
                    {
                        ZoneEventKey::Cost {
                            entry,
                            before: object,
                        }
                    }
                    EventCause::ManaAbilityCost { source, ability }
                        if action == ZoneActionKind::SacrificeCost =>
                    {
                        ZoneEventKey::ManaCost {
                            source,
                            ability,
                            before: object,
                        }
                    }
                    _ => return Err(ZoneReplacementError::InconsistentBatch.into()),
                };
                if matches!(
                    action,
                    ZoneActionKind::EffectReturnSelected
                        | ZoneActionKind::EffectLookToHand
                        | ZoneActionKind::EffectPutIntoLibrary
                        | ZoneActionKind::EffectShuffleGraveyard
                ) && before
                    .card_id()
                    .is_some_and(|id| self.commanders.contains_key(&id))
                {
                    let instance = ZoneEffectInstance {
                        source: object,
                        ability: ZoneReplacementAbilityId(0),
                        unearth: false,
                        madness: false,
                        created: Some(CreatedZoneEffect::Commander),
                    };
                    let definition = ZoneReplacementAbility {
                        id: instance.ability,
                        objects: ZoneObjectKind::Card,
                        subject: ZoneReplacementSubject::SourceSelf,
                        actions: ZoneActionPredicate::OneOf([action].into()),
                        destination,
                        replace_with: PublicZoneDestination::Command,
                        choice: ZoneReplacementChoice::OptionalAffectedPlayer,
                    };
                    sources.insert(
                        instance,
                        ZoneReplacementSource {
                            instance,
                            before: before.clone(),
                            definition,
                        },
                    );
                }
                let staying = if restrict_entry
                    && before.card_id().is_some()
                    && evaluated
                        .get(object)
                        .is_some_and(ObjectCharacteristics::is_creature)
                {
                    match before.zone() {
                        ObjectZone::Graveyard(_) => Some(PublicZoneDestination::Graveyard),
                        ObjectZone::Library(_) => Some(PublicZoneDestination::Library),
                        _ => None,
                    }
                } else {
                    None
                };
                let current = staying.unwrap_or(destination);
                let candidate = ZoneCandidate {
                    key,
                    before,
                    action,
                    reason: intent.reason,
                    original: destination,
                    current,
                    entry_prohibited: staying.is_some(),
                    entering_controller: entering_controllers
                        .and_then(|map| map.get(&object).copied())
                        .filter(|_| staying.is_none()),
                };
                if action == ZoneActionKind::EffectEnterBattlefield
                    && !candidate.entry_prohibited
                    && candidate.entering_controller.is_none()
                {
                    return Err(ZoneReplacementError::InconsistentBatch.into());
                }
                if candidates.insert(key, candidate).is_some() {
                    return Err(ZoneReplacementError::InconsistentBatch.into());
                }
            }
        }
        let regeneration_prohibited = match &input.cause {
            EventCause::Instruction { entry, instruction } => matches!(
                self.payloads
                    .get(entry)
                    .and_then(StackPayload::instructions)
                    .and_then(|ops| ops.get(*instruction)),
                Some(
                    Instruction::DestroyGroupWithoutRegeneration(_)
                        | Instruction::DestroyTargetWithoutRegeneration
                        | Instruction::DestroySubject {
                            no_regeneration: true,
                            ..
                        }
                        | Instruction::DestroyCapturedCombatCreature {
                            no_regeneration: true
                        }
                )
            ),
            _ => false,
        };
        let mut controller_domains = BTreeMap::new();
        for candidate in candidates.values() {
            if let Some(controller) = candidate.entering_controller
                && candidate.current == PublicZoneDestination::Battlefield
                && self.face_down_entry_for(candidate.before.id()).is_none()
                && self
                    .prospective_entry_face(candidate.before.id())
                    .and_then(Program::permanent)
                    .is_some_and(|card| {
                        card.entry_tap == Some(crate::land::EntryTap::ChosenOpponentControls)
                    })
            {
                let opponents = self
                    .combat_rules
                    .opponents(self.window.participants(), controller)?;
                if !opponents.is_empty() {
                    controller_domains.insert(candidate.before.id(), (controller, opponents));
                }
            }
        }
        let copy_rules = candidates
            .values()
            .filter(|candidate| {
                candidate.current == PublicZoneDestination::Battlefield
                    && self.face_down_entry_for(candidate.before.id()).is_none()
                    && candidate.entering_controller.is_some()
            })
            .filter_map(|candidate| {
                Some((
                    candidate.before.id(),
                    self.prospective_entry_face(candidate.before.id())?
                        .entry_copy
                        .clone()?,
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let has_battlefield_copy_rule = action == ZoneActionKind::EffectEnterBattlefield
            && entry_copy::EntryCopyPool::has_battlefield_copy_rules(&evaluated, false);
        let eligible_nontoken_creatures = if has_battlefield_copy_rule {
            candidates
                .values()
                .filter(|candidate| {
                    candidate.current == PublicZoneDestination::Battlefield
                        && candidate.before.card_id().is_some()
                        && self.face_down_entry_for(candidate.before.id()).is_none()
                        && self
                            .prospective_entry_face(candidate.before.id())
                            .is_some_and(|view| view.has_type(crate::program::CardType::Creature))
                })
                .map(|candidate| candidate.before.id())
                .collect::<BTreeSet<_>>()
        } else {
            BTreeSet::new()
        };
        let mut entry_opponents = BTreeMap::new();
        let copy_pool = if copy_rules.is_empty() && eligible_nontoken_creatures.is_empty() {
            entry_copy::EntryCopyPool::default()
        } else {
            for &seat in self.window.participants() {
                entry_opponents.insert(seat, self.opponents_of(seat)?);
            }
            entry_copy::EntryCopyPool::capture(self, &evaluated)?
        };
        let devour_rules = candidates
            .values()
            .filter(|c| {
                c.entering_controller.is_some() && self.face_down_entry_for(c.before.id()).is_none()
            })
            .filter_map(|c| {
                Some((
                    EntryCopySubject::Card(c.before.id()),
                    (
                        c.entering_controller?,
                        devour::multiplier(self.prospective_entry_face(c.before.id())?)?,
                    ),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let may_devour = !devour_rules.is_empty()
            || ((!copy_rules.is_empty() || !eligible_nontoken_creatures.is_empty())
                && self.window.cards().iter().any(|card| {
                    self.copiable_program(card.id())
                        .and_then(devour::multiplier)
                        .is_some()
                }));
        let effect_controller = match &input.cause {
            EventCause::Instruction { entry, .. } => self
                .window
                .resolving()
                .filter(|actual| actual.id() == *entry)
                .map(|actual| actual.controller()),
            _ => None,
        };
        let mut matching_subjects = BTreeSet::new();
        let empty_excluded = BTreeSet::new();
        for (&instance, source) in &sources {
            match &source.definition.subject {
                ZoneReplacementSubject::Matching {
                    filter,
                    owner_relation,
                }
                | ZoneReplacementSubject::MatchingNotCast {
                    filter,
                    owner_relation,
                } => {
                    let is_not_cast = matches!(
                        source.definition.subject,
                        ZoneReplacementSubject::MatchingNotCast { .. }
                    );
                    let source_ctrl = evaluated
                        .get(instance.source)
                        .and_then(|v| v.controller)
                        .or_else(|| source.before.controller())
                        .or_else(|| match instance.created {
                            Some(CreatedZoneEffect::TemporaryZoneReplacement(rule_id)) => self
                                .temporary_rules
                                .iter()
                                .find(|rule| rule.id == rule_id)
                                .map(|rule| rule.controller),
                            _ => None,
                        });
                    let Some(source_ctrl) = source_ctrl else {
                        continue;
                    };
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), source_ctrl)?;
                    for candidate in candidates.values() {
                        let cid = candidate.before.id();
                        if is_not_cast {
                            let was_cast = if candidate.reason == ZoneReason::PermanentResolution {
                                match &input.cause {
                                    EventCause::ResolutionCompleted { entry, .. } => self
                                        .window
                                        .entries()
                                        .iter()
                                        .find(|e| e.id() == *entry)
                                        .is_none_or(|e| {
                                            !matches!(e.kind(), StackKind::SpellCopy { .. })
                                        }),
                                    _ => true,
                                }
                            } else {
                                self.characteristics
                                    .cast_by_controller_objects
                                    .contains(&cid)
                                    || self.characteristics.cast_from_hand_objects.contains(&cid)
                            };
                            if was_cast {
                                continue;
                            }
                        }
                        let owner_ok = match owner_relation {
                            None => true,
                            Some(crate::program::PlayerRelation::Controller) => {
                                candidate.before.owner() == source_ctrl
                            }
                            Some(crate::program::PlayerRelation::Opponent) => {
                                opponents.contains(&candidate.before.owner())
                            }
                            Some(crate::program::PlayerRelation::AnyParticipant) => true,
                        };
                        if !owner_ok {
                            continue;
                        }
                        let Some(base_view) = evaluated.get(cid) else {
                            continue;
                        };
                        let adjusted_view;
                        let view = if candidate.action == ZoneActionKind::EffectEnterBattlefield {
                            let mut entry_view = base_view.clone();
                            if entry_view.controller.is_none() {
                                entry_view.controller = candidate.entering_controller;
                            }
                            if self.face_down_entry_for(cid).is_some() {
                                let two_two = crate::characteristics::PowerToughness {
                                    power: 2.into(),
                                    toughness: 2.into(),
                                };
                                entry_view.current_types = BTreeSet::from([CardType::Creature]);
                                entry_view.current_supertypes.clear();
                                entry_view.current_creature_subtypes.clear();
                                entry_view.current_colors.clear();
                                entry_view.power_toughness = Some(PowerToughnessEvaluation {
                                    object: cid,
                                    copiable: Some(two_two.clone().into()),
                                    base: two_two.clone(),
                                    derived: two_two,
                                    applied_effects: Vec::new(),
                                });
                            } else if !self.characteristics.bestowed_spells.contains(&cid)
                                && self.linked_return_work.as_ref().is_some_and(|work| {
                                    matches!(
                                        work.origin,
                                        LinkedReturnOrigin::GraveyardTransformed { .. }
                                    ) && work.objects.contains(&cid)
                                })
                                && let Some(face) = self.prospective_entry_face(cid)
                            {
                                entry_view.current_types = face.card_types.clone();
                                entry_view.current_supertypes = face.supertypes.clone();
                                entry_view.current_creature_subtypes =
                                    face.creature_subtypes.clone();
                                entry_view.current_colors = face.defined_colors();
                            }
                            adjusted_view = entry_view;
                            &adjusted_view
                        } else {
                            base_view
                        };
                        if self.matches_object_filter(
                            **filter,
                            view,
                            source_ctrl,
                            Some(instance.source),
                            &empty_excluded,
                            &evaluated,
                        ) {
                            matching_subjects.insert((instance, cid));
                        }
                    }
                }
                ZoneReplacementSubject::SourceSelfDiscardByOpponent => {
                    if action == ZoneActionKind::EffectDiscard
                        && let Some(actor) = effect_controller
                    {
                        for candidate in candidates.values() {
                            let cid = candidate.before.id();
                            if cid == instance.source {
                                let owner = candidate.before.owner();
                                let opponents = self
                                    .combat_rules
                                    .opponents(self.window.participants(), owner)?;
                                if opponents.contains(&actor) {
                                    matching_subjects.insert((instance, cid));
                                }
                            }
                        }
                    }
                }
                ZoneReplacementSubject::DamagedBySourceThisTurn => {
                    if let Some(pairs) = &self.characteristics.creature_damage_pairs {
                        for candidate in candidates.values() {
                            let cid = candidate.before.id();
                            if pairs.contains(&(instance.source, cid)) {
                                matching_subjects.insert((instance, cid));
                            }
                        }
                    }
                }
                ZoneReplacementSubject::AnyObject | ZoneReplacementSubject::SourceSelf => {}
            }
        }
        let mut batch = ZoneReplacementBatch {
            devour: devour::Devour::default(),
            copy_rules,
            copy_pool,
            eligible_nontoken_creatures,
            used_battlefield_copies: BTreeMap::new(),
            used_intrinsic_copies: BTreeMap::new(),
            copies: BTreeMap::new(),
            entry_opponents,
            entry_history: Vec::new(),
            controller_domains,
            selected_controllers: Vec::new(),
            regeneration_prohibited,
            ordered_library_placement: None,
            cause: input.cause,
            intents: input.intents,
            window_revision: self.window.revision(),
            evaluated,
            apnap: self.window.apnap_players()?.collect(),
            candidates,
            sources,
            matching_subjects,
            applied: BTreeMap::new(),
            optional: BTreeMap::new(),
            history: vec![],
            stage: 0,
            phase: ZoneReplacementPhase::Selecting,
        };
        if may_devour {
            batch.enable_devour(self, devour_rules);
        }
        Ok(batch)
    }
}

/// Physical-card owner domain shared by movement owners; tokens do not require ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneGraveyardGroup {
    /// Physical owner, independently of the replacement chooser.
    pub owner: SeatId,
    /// Complete final Graveyard card entrants.
    pub entrants: Vec<ObjectId>,
    /// Final destination. A library group retains its instruction's placement.
    pub destination: PublicZoneDestination,
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
        let mut owners = BTreeMap::<(SeatId, PublicZoneDestination), Vec<ObjectId>>::new();
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
            let order_destination = match destination {
                PublicZoneDestination::Graveyard => {
                    !matches!(object.zone(), ObjectZone::Graveyard(_))
                }
                PublicZoneDestination::Library => {
                    replacements.is_some_and(|batch| batch.ordered_library_placement.is_some())
                }
                _ => false,
            };
            if order_destination
                && !replacements.is_some_and(|b| b.saved_from_destruction(id))
                && object.card_id().is_some()
            {
                owners
                    .entry((object.owner(), destination))
                    .or_default()
                    .push(id);
            }
        }
        let mut groups = vec![];
        for owner in self.window.apnap_players()? {
            for destination in [
                PublicZoneDestination::Graveyard,
                PublicZoneDestination::Library,
            ] {
                if let Some(entrants) = owners.remove(&(owner, destination)) {
                    groups.push(ZoneGraveyardGroup {
                        owner,
                        entrants,
                        destination,
                    });
                }
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
        let moving: Vec<_> =
            objects
                .iter()
                .copied()
                .filter(|id| {
                    !replacements.is_some_and(|batch| {
                        batch.saved_from_destruction(*id)
                            || (batch.destination(*id) == Ok(PublicZoneDestination::Command)
                                && self
                                    .window
                                    .cards()
                                    .get(*id)
                                    .is_some_and(|object| object.zone() == ObjectZone::Command))
                            || (batch.destination(*id) == Ok(PublicZoneDestination::Graveyard)
                                && self.window.cards().get(*id).is_some_and(|object| {
                                    matches!(object.zone(), ObjectZone::Graveyard(_))
                                }))
                            || (batch.destination(*id) == Ok(PublicZoneDestination::Hand)
                                && self.window.cards().get(*id).is_some_and(|object| {
                                    matches!(object.zone(), ObjectZone::Hand(_))
                                }))
                    })
                })
                .collect();
        let objects = moving.as_slice();
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
            while groups.peek().is_some_and(|(g, _)| g.owner == owner) {
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
                let entries = order
                    .top_to_bottom
                    .iter()
                    .map(|id| (*id, group.destination));
                if group.destination == PublicZoneDestination::Library
                    && replacements.is_some_and(|batch| {
                        batch.ordered_library_placement
                            == Some(crate::program::LibraryPlacement::Bottom)
                    })
                {
                    plan.application.extend(entries);
                } else {
                    plan.application.extend(entries.rev());
                }
            }
            for &id in objects {
                let object = self
                    .window
                    .cards()
                    .get(id)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                if object.owner() == owner
                    && object.card_id().is_some()
                    && let Some(batch) = replacements
                    && let destination = batch.destination(id)?
                    && destination != PublicZoneDestination::Graveyard
                    && !(batch.ordered_library_placement.is_some()
                        && destination == PublicZoneDestination::Library)
                {
                    plan.logical.push(id);
                    plan.application.push((id, destination));
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

    // Exposed for Wave 7 zone-move commit hooks outside this module.
    #[allow(dead_code)]
    pub(super) fn apply_zone_replacement_extras_from_movements(
        &mut self,
        batch: Option<&ZoneReplacementBatch>,
        movements: &[crate::object_identity::ObjectMovement],
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let Some(batch) = batch else {
            return Ok(());
        };
        let moved: Vec<(ObjectId, ObjectId)> = movements
            .iter()
            .map(|movement| (movement.before.id(), movement.after.id()))
            .collect();
        self.apply_zone_replacement_extras(batch, &moved, cause, append)
    }

    /// Apply `Program.zone_replacement_extras` (exile/battlefield counters and inline followup bodies)
    /// for every zone replacement ability that actually applied in `batch`.
    // Exposed for Wave 7 zone-move commit hooks outside this module.
    #[allow(dead_code)]
    pub(super) fn apply_zone_replacement_extras(
        &mut self,
        batch: &ZoneReplacementBatch,
        moved: &[(ObjectId, ObjectId)],
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        for step in batch.history() {
            if step.optional == Some(ZoneOptionalAnswer::Decline) {
                continue;
            }
            let Some(source) = batch.sources().get(&step.application.effect) else {
                continue;
            };
            if source.instance.unearth
                || source.instance.madness
                || source.instance.created.is_some()
            {
                continue;
            }
            let extra = batch
                .evaluated()
                .get(source.instance.source)
                .and_then(|view| {
                    view.printed_text
                        .zone_replacement_extras
                        .get(&source.instance.ability)
                })
                .or_else(|| {
                    self.copiable_program(source.instance.source)
                        .and_then(|program| {
                            program
                                .zone_replacement_extras
                                .get(&source.instance.ability)
                        })
                })
                .cloned();
            let Some(extra) = extra else {
                continue;
            };
            if let Some((kind, count)) = extra.exile_counters
                && count > 0
                && let Some(after_id) = moved.iter().find_map(|&(before_id, after_id)| {
                    (before_id == step.application.event.before()).then_some(after_id)
                })
                && let Some(after_zone) = self.window.cards().get(after_id).map(GameObject::zone)
                && matches!(after_zone, ObjectZone::Exile | ObjectZone::Battlefield)
            {
                self.place_zone_replacement_counters(
                    after_id,
                    after_zone,
                    kind,
                    count,
                    cause.clone(),
                    append,
                )?;
            }
            if let Some(body) = &extra.followup_body {
                let source_controller = batch
                    .evaluated()
                    .get(source.instance.source)
                    .and_then(|view| view.controller)
                    .or_else(|| source.before.controller())
                    .unwrap_or_else(|| source.before.owner());
                let affected_player = batch
                    .candidates()
                    .get(&step.application.event)
                    .map_or(source_controller, |candidate| batch.chooser(candidate));
                let rep = super::event_replacement::EventBodyReplacement {
                    source: Some(source.instance.source),
                    ability: crate::characteristics::StaticAbilityId(
                        source.instance.ability.0 | (1_u64 << 63),
                    ),
                    controller: source_controller,
                    affected_player,
                    optional: false,
                    body: (**body).clone(),
                };
                self.execute_inline_replacement_body(&rep, cause.clone(), append)?;
            }
        }
        Ok(())
    }

    // Helper for apply_zone_replacement_extras (Wave 7 commit hooks outside this module).
    #[allow(dead_code)]
    fn place_zone_replacement_counters(
        &mut self,
        object: ObjectId,
        zone: ObjectZone,
        kind: CounterKind,
        count: u8,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let amount = RulesInteger::from(i64::from(count));
        if !amount.is_positive() {
            return Ok(());
        }
        let grant_entry = match &cause {
            EventCause::Instruction { entry, .. }
            | EventCause::ResolutionCompleted { entry, .. }
            | EventCause::ActivatedAbilityCost { entry, .. }
            | EventCause::CastObjectPayment { entry, .. } => *entry,
            _ => self
                .window
                .resolving()
                .map(crate::stack_window::StackEntry::id)
                .unwrap_or_else(StackEntryId::entry_replacement_creator),
        };
        if zone == ObjectZone::Battlefield {
            let before = self.rules_snapshot(None)?;
            if self.permanent_counter_prevented(&before.view().evaluated, object, kind) {
                return Ok(());
            }
            if kind == CounterKind::PlusOne {
                return self.place_plus_one_counter_amount(vec![object], amount, cause, append);
            }
            let mult = super::permanent_counters::effect_counter_multiplier(
                &before.view().evaluated,
                object,
            );
            let added = amount.times(&mult);
            if !added.is_positive() {
                return Ok(());
            }
            if kind == CounterKind::MinusOne {
                let prev = self.characteristics.counters(object);
                let delta = PowerToughnessCounters {
                    plus_one: 0.into(),
                    minus_one: added,
                };
                self.characteristics.place_counters(object, &delta)?;
                let after = self.characteristics.counters(object);
                let before_appearance = before
                    .view()
                    .appearance(object)
                    .ok_or(EffectError::InconsistentProgram)?;
                let after_appearance = self
                    .rules_snapshot(None)?
                    .view()
                    .appearance(object)
                    .ok_or(EffectError::InconsistentProgram)?;
                self.emit_from(cause, &before, None, append, |_, _, _| {
                    Ok(vec![crate::occurrence::RulesOccurrence::CountersPlaced {
                        object,
                        before: prev,
                        added: delta,
                        after,
                        before_appearance: Box::new(before_appearance),
                        after_appearance: Box::new(after_appearance),
                    }])
                })?;
            } else if let Some(map) = self.characteristics.named_permanent_counters_mut(kind) {
                let prev = map.get(&object).cloned().unwrap_or_else(|| 0.into());
                let after = prev.plus(&added);
                map.insert(object, after.clone());
                self.characteristics
                    .refresh_keyword_counter_grant(grant_entry, object, kind)?;
                let occurrence = match kind {
                    CounterKind::Charge => {
                        crate::occurrence::RulesOccurrence::ChargeCountersChanged {
                            object,
                            before: prev,
                            after,
                        }
                    }
                    CounterKind::Suspect => {
                        crate::occurrence::RulesOccurrence::SuspectCountersChanged {
                            object,
                            before: prev,
                            after,
                        }
                    }
                    _ => crate::occurrence::RulesOccurrence::PermanentCounterChanged {
                        object,
                        kind,
                        before: prev,
                        after,
                    },
                };
                self.emit_from(cause, &before, None, append, |_, _, _| Ok(vec![occurrence]))?;
            }
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        if kind == CounterKind::PlusOne || kind == CounterKind::MinusOne {
            let prev = self.characteristics.counters(object);
            let delta = if kind == CounterKind::PlusOne {
                PowerToughnessCounters {
                    plus_one: amount,
                    minus_one: 0.into(),
                }
            } else {
                PowerToughnessCounters {
                    plus_one: 0.into(),
                    minus_one: amount,
                }
            };
            self.characteristics.place_counters(object, &delta)?;
            let after = self.characteristics.counters(object);
            let before_appearance = before
                .view()
                .appearance(object)
                .ok_or(EffectError::InconsistentProgram)?;
            let after_appearance = self
                .rules_snapshot(None)?
                .view()
                .appearance(object)
                .ok_or(EffectError::InconsistentProgram)?;
            self.emit_from(cause, &before, None, append, |_, _, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::CountersPlaced {
                    object,
                    before: prev,
                    added: delta,
                    after,
                    before_appearance: Box::new(before_appearance),
                    after_appearance: Box::new(after_appearance),
                }])
            })?;
        } else if let Some(map) = self.characteristics.named_permanent_counters_mut(kind) {
            let prev = map.get(&object).cloned().unwrap_or_else(|| 0.into());
            let after = prev.plus(&amount);
            map.insert(object, after.clone());
            let occurrence = match kind {
                CounterKind::Charge => crate::occurrence::RulesOccurrence::ChargeCountersChanged {
                    object,
                    before: prev,
                    after,
                },
                CounterKind::Suspect => {
                    crate::occurrence::RulesOccurrence::SuspectCountersChanged {
                        object,
                        before: prev,
                        after,
                    }
                }
                _ => crate::occurrence::RulesOccurrence::PermanentCounterChanged {
                    object,
                    kind,
                    before: prev,
                    after,
                },
            };
            self.emit_from(cause, &before, None, append, |_, _, _| Ok(vec![occurrence]))?;
        }
        Ok(())
    }
}
