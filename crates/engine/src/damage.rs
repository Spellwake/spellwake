//! Shared simultaneous source/recipient damage math and finite modifications.
//!
//! The executor owns the active batch, validates actual world/source context, and
//! commits final results to the sole marked-damage and life state. This module
//! grants no priority and owns no cards, scheduler or active continuation.
#[cfg(test)]
pub(crate) mod private_qualification;
use crate::{
    object_identity::{GameObject, ObjectId},
    priority::SeatId,
    stack_window::StackEntryId,
    turn::TurnPosition,
};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PreventionShieldId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DamageMultiplierId(u64);
/// Effect families retain distinct identities with equal numeric ordinals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageEffectId {
    /// Noncombat prevention supplied by an actual current permanent incarnation.
    StaticPrevention(ObjectId),
    StaticPreventionWithExile(ObjectId, crate::characteristics::StaticAbilityId),
    CombatDamageToMill(ObjectId, crate::characteristics::StaticAbilityId),
    RemovePlusOneToPreventDamage(ObjectId, crate::characteristics::StaticAbilityId),
    /// Each eligible component is doubled once by this source's effect.
    StaticDouble(ObjectId),
    /// One of two independently ordered replacement effects from an Equipment.
    DoubleDamageFromEquippedCreature(ObjectId),
    DoubleDamageToEquippedCreature(ObjectId),
    /// Round up half of one source's damage to one protected recipient.
    StaticHalfPrevention {
        permanent: ObjectId,
        source: DamageSourceId,
    },
    /// An emblem caps one source's damage to a particular recipient.
    EmblemCap {
        emblem: crate::emblem::EmblemId,
        source: DamageSourceId,
    },
    /// One lasting prevention effect created by a resolving instruction.
    UntilEndOfTurn {
        creator: StackEntryId,
        instruction: usize,
    },
    /// Static protection of the affected permanent from the stated color.
    Protection(crate::characteristics::Color),
    ProtectionMulticolored,
    ProtectionSubtype(crate::characteristics::SubtypeProtection),
    Prevention(PreventionShieldId),
    Multiplier(DamageMultiplierId),
}
impl From<PreventionShieldId> for DamageEffectId {
    fn from(id: PreventionShieldId) -> Self {
        Self::Prevention(id)
    }
}
impl From<DamageMultiplierId> for DamageEffectId {
    fn from(id: DamageMultiplierId) -> Self {
        Self::Multiplier(id)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DamageAttemptId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DamageBatchId(u64);

/// Actual source identity, distinct from an instruction's execution origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageSourceId {
    Emblem(crate::emblem::EmblemId),
    Object(ObjectId),
    SpellCopy(StackEntryId),
}
/// Source incarnation and evaluated damage keywords at its actual battlefield departure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepartedDamageSource {
    /// Equipment attached at this exact permanent's last battlefield appearance.
    pub attached_equipment: RulesInteger,
    pub attached_auras_and_equipment: RulesInteger,
    /// Power at the last battlefield cut, if the object had power.
    pub power: Option<RulesInteger>,
    /// Evaluated creature status immediately before leaving the battlefield.
    pub was_creature: bool,
    /// Last evaluated creature subtypes before leaving the battlefield.
    pub creature_subtypes: BTreeSet<spellwake_contracts::creature_types::CreatureTypeId>,
    /// Last evaluated source colors at its battlefield departure.
    pub colors: std::collections::BTreeSet<crate::characteristics::Color>,
    /// Last battlefield object, including the controller when it left.
    pub object: GameObject,
    /// Keywords relevant to dealing damage from the departed incarnation.
    pub keywords: std::collections::BTreeSet<crate::characteristics::KeywordAbility>,
}
/// Bounded readonly source evidence, not complete characteristics or LKI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageSource {
    /// The actual source is a command-zone emblem, not its former creating permanent.
    Emblem {
        emblem: crate::emblem::EmblemId,
        controller: SeatId,
    },
    Object {
        object: GameObject,
        controller: SeatId,
    },
    SpellCopy {
        entry: StackEntryId,
        controller: SeatId,
    },
}
impl DamageSource {
    pub fn id(&self) -> DamageSourceId {
        match self {
            Self::Emblem { emblem, .. } => DamageSourceId::Emblem(*emblem),
            Self::Object { object, .. } => DamageSourceId::Object(object.id()),
            Self::SpellCopy { entry, .. } => DamageSourceId::SpellCopy(*entry),
        }
    }
    pub fn controller(&self) -> SeatId {
        match self {
            Self::Emblem { controller, .. }
            | Self::Object { controller, .. }
            | Self::SpellCopy { controller, .. } => *controller,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageRecipient {
    Battle(ObjectId),
    Creature(ObjectId),
    Planeswalker(ObjectId),
    Player(SeatId),
}
/// Fixed identity for a future effect, independent of current creature status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageEffectTarget {
    Permanent(ObjectId),
    Player(SeatId),
    /// A player and all current permanents they control share one capacity.
    PlayerAndPermanents(SeatId),
}
impl DamageEffectTarget {
    /// Match identity after the executor admits an actual damage recipient.
    pub fn matches(self, recipient: DamageRecipient) -> bool {
        match (self, recipient) {
            (
                Self::Permanent(expected),
                DamageRecipient::Creature(actual)
                | DamageRecipient::Planeswalker(actual)
                | DamageRecipient::Battle(actual),
            ) => expected == actual,
            (Self::Player(expected), DamageRecipient::Player(actual)) => expected == actual,
            _ => false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Combat,
    Noncombat,
}
/// Sole parent metadata after its cursor moves into the active batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageOrigin {
    /// Noncombat damage from the immediate consequence of an earlier prevention.
    PreventionFollowup {
        batch: DamageBatchId,
        creator: StackEntryId,
    },
    /// Noncombat damage caused by an immediately resolving mana ability.
    ManaAbility {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
    },
    Resolution {
        entry: StackEntryId,
        instruction: usize,
    },
    Combat {
        position: TurnPosition,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSourceFilter {
    Any,
    Exact(DamageSourceId),
}
impl DamageSourceFilter {
    fn matches(self, source: DamageSourceId) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => expected == source,
        }
    }
}
/// The damage a resolving effect prevents through the cleanup step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnPreventionFilter {
    ToAbilitySource,
    CombatToOrFromAbilitySource,
    ToTarget,
    CombatFromCreaturesOtherThanTarget,
    CombatToControlledCreatures,
    CombatFromCreaturesWithoutSubtype(spellwake_contracts::creature_types::CreatureTypeId),
    CombatFromCreaturesWithoutEitherSubtype(
        spellwake_contracts::creature_types::CreatureTypeId,
        spellwake_contracts::creature_types::CreatureTypeId,
    ),
    Combat,
    CreaturesControlledByOpponents,
}
/// Creator provenance survives creator exit and never supplies source matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreventionShield {
    pub id: PreventionShieldId,
    pub creator: StackEntryId,
    pub target: DamageEffectTarget,
    pub remaining: RulesInteger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DamageReflection {
    pub(crate) source: DamageSource,
    pub(crate) colors: BTreeSet<crate::characteristics::Color>,
    pub(crate) keywords: BTreeSet<crate::characteristics::KeywordAbility>,
    pub(crate) creature_types: BTreeSet<spellwake_contracts::creature_types::CreatureTypeId>,
    pub(crate) is_creature: bool,
    pub(crate) target: crate::targets::TargetRef,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageMultiplier {
    pub id: DamageMultiplierId,
    pub creator: StackEntryId,
    pub target: DamageEffectTarget,
    pub factor: RulesInteger,
    pub filter: DamageSourceFilter,
}
/// Current-operation birth evidence, including effects later consumed or pruned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageEffectCreation {
    Prevention {
        shield: PreventionShield,
        instruction: usize,
    },
    Multiplier {
        multiplier: DamageMultiplier,
        instruction: usize,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageInput {
    pub source: DamageSource,
    pub target: DamageRecipient,
    pub amount: RulesInteger,
}
/// A positive original contribution, not an occurrence of damage dealt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageAttempt {
    pub id: DamageAttemptId,
    pub batch: DamageBatchId,
    pub source: DamageSource,
    pub target: DamageRecipient,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageBatchStarted {
    pub id: DamageBatchId,
    pub origin: DamageOrigin,
    pub kind: DamageKind,
    pub components: Vec<DamageAttemptId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageScope {
    /// One shield shared by all damage recipients currently controlled by this seat.
    ControlledRecipients(SeatId),
    Component(DamageAttemptId),
    Recipient(DamageRecipient),
}
/// An effect and the exact represented event scope it would modify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DamageApplication {
    pub effect: DamageEffectId,
    pub scope: DamageScope,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageAllocation {
    pub attempt: DamageAttemptId,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageComponent {
    attempt: DamageAttempt,
    remaining: RulesInteger,
    preventable: bool,
    recipient_unavailable: bool,
    affected_player: Option<SeatId>,
    used_multipliers: BTreeSet<DamageMultiplierId>,
}
impl DamageComponent {
    /// Whether the current amount can be reduced by an ordinary prevention effect.
    pub fn can_prevent_damage(&self) -> bool {
        self.preventable && self.remaining.is_positive()
    }

    pub fn attempt(&self) -> &DamageAttempt {
        &self.attempt
    }
    pub fn remaining(&self) -> &RulesInteger {
        &self.remaining
    }
    pub fn used_multipliers(&self) -> &BTreeSet<DamageMultiplierId> {
        &self.used_multipliers
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageBatchPhase {
    Selecting,
    Allocating { application: DamageApplication },
}
/// The executor's sole active value. Frames and journals cannot resume it alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveDamageBatch {
    started: DamageBatchStarted,
    components: BTreeMap<DamageAttemptId, DamageComponent>,
    consumed_shields: BTreeSet<DamageApplication>,
    stage: u64,
    phase: DamageBatchPhase,
}
impl ActiveDamageBatch {
    pub(crate) fn disallow_prevention_from(&mut self, sources: &BTreeSet<DamageSourceId>) {
        for component in self.components.values_mut() {
            component.preventable = !sources.contains(&component.attempt.source.id());
        }
    }

    pub(crate) fn prevent_by_protection(
        &mut self,
        application: DamageApplication,
        eligible: Vec<DamageAttemptId>,
    ) -> Result<ProtectionPrevention, DamageError> {
        let DamageScope::Recipient(recipient) = application.scope else {
            return Err(DamageError::InapplicableEffect);
        };
        let quality = match application.effect {
            DamageEffectId::Protection(color) => {
                ProtectionQuality::Color(crate::characteristics::ColorFilter::Has(color))
            }
            DamageEffectId::ProtectionMulticolored => {
                ProtectionQuality::Color(crate::characteristics::ColorFilter::Multicolored)
            }
            DamageEffectId::ProtectionSubtype(quality) => ProtectionQuality::Subtype(quality),
            _ => return Err(DamageError::InapplicableEffect),
        };
        let allocations = self.prevent_all(recipient, eligible)?;
        Ok(ProtectionPrevention {
            batch: self.id(),
            stage: self.stage,
            quality,
            recipient,
            allocations,
        })
    }
    /// Use one side-effect-bearing prevention exactly once, retaining unpreventable damage.
    pub(crate) fn prevent_all_with_followup(
        &mut self,
        application: DamageApplication,
        eligible: &[DamageAttemptId],
    ) -> Result<(Vec<DamageAllocation>, RulesInteger), DamageError> {
        let DamageScope::Recipient(recipient) = application.scope else {
            return Err(DamageError::InapplicableEffect);
        };
        if self.phase != DamageBatchPhase::Selecting
            || eligible.is_empty()
            || self.consumed_shields.contains(&application)
        {
            return Err(DamageError::InapplicableEffect);
        }
        let mut preventable = Vec::new();
        let mut amount = 0.into();
        for &id in eligible {
            let component = self
                .component(id)
                .filter(|c| c.attempt().target == recipient && c.remaining().is_positive())
                .ok_or(DamageError::InapplicableEffect)?;
            amount = RulesInteger::plus(&amount, component.remaining());
            if component.can_prevent_damage() {
                preventable.push(id);
            }
        }
        let allocations = if preventable.is_empty() {
            self.stage = self
                .stage
                .checked_add(1)
                .ok_or(DamageError::StageExhausted)?;
            Vec::new()
        } else {
            self.prevent_all(recipient, preventable)?
        };
        self.consumed_shields.insert(application);
        Ok((allocations, amount))
    }

    /// Prevent the full remaining amounts of all eligible components for one recipient.
    pub(crate) fn prevent_all(
        &mut self,
        recipient: DamageRecipient,
        eligible: Vec<DamageAttemptId>,
    ) -> Result<Vec<DamageAllocation>, DamageError> {
        self.remove_damage(recipient, eligible, true)
    }

    pub(crate) fn replace_damage_with_library_move(
        &mut self,
        recipient: DamageRecipient,
        eligible: Vec<DamageAttemptId>,
    ) -> Result<Vec<DamageAllocation>, DamageError> {
        self.remove_damage(recipient, eligible, false)
    }

    fn remove_damage(
        &mut self,
        recipient: DamageRecipient,
        eligible: Vec<DamageAttemptId>,
        prevention: bool,
    ) -> Result<Vec<DamageAllocation>, DamageError> {
        if self.phase != DamageBatchPhase::Selecting || eligible.is_empty() {
            return Err(DamageError::InapplicableEffect);
        }
        let stage = self
            .stage
            .checked_add(1)
            .ok_or(DamageError::StageExhausted)?;
        let mut allocations = Vec::with_capacity(eligible.len());
        for id in eligible {
            let component = self
                .components
                .get(&id)
                .filter(|c| {
                    (!prevention || c.can_prevent_damage()) && c.attempt.target == recipient
                })
                .ok_or(DamageError::InapplicableEffect)?;
            allocations.push(DamageAllocation {
                attempt: id,
                amount: component.remaining.clone(),
            });
        }
        for row in &allocations {
            if let Some(component) = self.components.get_mut(&row.attempt) {
                component.remaining = 0.into();
            }
        }
        self.stage = stage;
        Ok(allocations)
    }
    pub(crate) fn double_static_component(
        &mut self,
        application: DamageApplication,
    ) -> Result<(RulesInteger, RulesInteger), DamageError> {
        if self.phase != DamageBatchPhase::Selecting || self.consumed_shields.contains(&application)
        {
            return Err(DamageError::InapplicableEffect);
        }
        let (
            DamageEffectId::StaticDouble(_)
            | DamageEffectId::DoubleDamageFromEquippedCreature(_)
            | DamageEffectId::DoubleDamageToEquippedCreature(_),
            DamageScope::Component(attempt),
        ) = (application.effect, application.scope)
        else {
            return Err(DamageError::InapplicableEffect);
        };
        let stage = self
            .stage
            .checked_add(1)
            .ok_or(DamageError::StageExhausted)?;
        let component = self
            .components
            .get_mut(&attempt)
            .filter(|component| component.remaining.is_positive())
            .ok_or(DamageError::InapplicableEffect)?;
        let before = component.remaining.clone();
        let after = before.plus(&before);
        component.remaining = after.clone();
        self.consumed_shields.insert(application);
        self.stage = stage;
        Ok((before, after))
    }

    pub(crate) fn prevent_all_but_one(
        &mut self,
        application: DamageApplication,
        eligible: Vec<DamageAttemptId>,
    ) -> Result<Vec<DamageAllocation>, DamageError> {
        self.limit_damage(application, eligible, 1.into())
    }

    pub(crate) fn limit_damage(
        &mut self,
        application: DamageApplication,
        eligible: Vec<DamageAttemptId>,
        mut leave: RulesInteger,
    ) -> Result<Vec<DamageAllocation>, DamageError> {
        if self.phase != DamageBatchPhase::Selecting
            || eligible.is_empty()
            || self.consumed_shields.contains(&application)
            || leave < 0.into()
        {
            return Err(DamageError::InapplicableEffect);
        }
        let stage = self
            .stage
            .checked_add(1)
            .ok_or(DamageError::StageExhausted)?;
        let mut allocations = Vec::new();
        for id in eligible {
            let component = self
                .components
                .get(&id)
                .filter(|c| c.remaining.is_positive())
                .ok_or(DamageError::InapplicableEffect)?;
            let keep = leave.clone().min(component.remaining.clone());
            let prevented = component.remaining.minus(&keep);
            leave = leave.minus(&keep);
            if prevented.is_positive() {
                allocations.push(DamageAllocation {
                    attempt: id,
                    amount: prevented,
                });
            }
        }
        if allocations.is_empty() {
            return Err(DamageError::InapplicableEffect);
        }
        for row in &allocations {
            let component = self
                .components
                .get_mut(&row.attempt)
                .ok_or(DamageError::InapplicableEffect)?;
            component.remaining = component.remaining.minus(&row.amount);
        }
        self.consumed_shields.insert(application);
        self.stage = stage;
        Ok(allocations)
    }
    pub(crate) fn set_recipient_controllers(
        &mut self,
        controllers: &BTreeMap<DamageRecipient, SeatId>,
    ) {
        for component in self.components.values_mut() {
            if let Some(&player) = controllers.get(&component.attempt.target) {
                component.affected_player = Some(player);
            }
        }
    }

    /// Reconcile an in-progress event after a player leaves. Present components retain
    /// their identities and all accepted modifications. Damage that can no longer be
    /// dealt leaves the choice domain without debiting any prevention effect.
    pub(crate) fn reconcile_departure(
        &mut self,
        controllers: &BTreeMap<DamageRecipient, SeatId>,
    ) -> Result<(), DamageError> {
        let stage = self
            .stage
            .checked_add(1)
            .ok_or(DamageError::StageExhausted)?;
        for component in self.components.values_mut() {
            component.affected_player = controllers.get(&component.attempt.target).copied();
            if component.affected_player.is_none() && component.remaining.is_positive() {
                component.recipient_unavailable = true;
                component.remaining = 0.into();
            }
        }
        self.stage = stage;
        Ok(())
    }

    pub fn started(&self) -> &DamageBatchStarted {
        &self.started
    }
    pub fn id(&self) -> DamageBatchId {
        self.started.id
    }
    pub fn origin(&self) -> DamageOrigin {
        self.started.origin
    }
    pub fn kind(&self) -> DamageKind {
        self.started.kind
    }
    pub fn components(&self) -> impl Iterator<Item = &DamageComponent> {
        self.components.values()
    }
    pub fn component(&self, id: DamageAttemptId) -> Option<&DamageComponent> {
        self.components.get(&id)
    }
    pub fn consumed_shields(&self) -> &BTreeSet<DamageApplication> {
        &self.consumed_shields
    }
    pub fn stage(&self) -> u64 {
        self.stage
    }
    pub fn phase(&self) -> &DamageBatchPhase {
        &self.phase
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreventionApplication {
    pub batch: DamageBatchId,
    pub shield: PreventionShieldId,
    pub creator: StackEntryId,
    pub target: DamageRecipient,
    pub stage: u64,
    pub allocations: Vec<DamageAllocation>,
    pub amount: RulesInteger,
    pub damage_remaining: BTreeMap<DamageAttemptId, RulesInteger>,
    pub shield_remaining: RulesInteger,
}
/// Damage prevented by one protection quality on a permanent or player (CR 702.16e).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectionQuality {
    Color(crate::characteristics::ColorFilter),
    Subtype(crate::characteristics::SubtypeProtection),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectionPrevention {
    pub batch: DamageBatchId,
    pub stage: u64,
    pub quality: ProtectionQuality,
    pub recipient: DamageRecipient,
    pub allocations: Vec<DamageAllocation>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageMultiplication {
    pub batch: DamageBatchId,
    pub attempt: DamageAttemptId,
    pub multiplier: DamageMultiplierId,
    pub creator: StackEntryId,
    pub target: DamageRecipient,
    pub stage: u64,
    pub before: RulesInteger,
    pub factor: RulesInteger,
    pub after: RulesInteger,
}
/// Only strictly positive applied quantities are emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedDamage {
    pub batch: DamageBatchId,
    pub attempt: DamageAttemptId,
    pub source: DamageSource,
    pub target: DamageRecipient,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageOutcome {
    Dealt(RulesInteger),
    FullyPrevented,
    /// The remaining damage could not be dealt because its recipient ceased to be available.
    RecipientUnavailable,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageCompletion {
    pub batch: DamageBatchId,
    pub attempt: DamageAttemptId,
    pub outcome: DamageOutcome,
}
/// Appends for one accepted operation; not durable replay or resumption state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DamageJournal {
    pub batches: Vec<DamageBatchStarted>,
    pub created: Vec<DamageEffectCreation>,
    pub attempts: Vec<DamageAttempt>,
    pub preventions: Vec<PreventionApplication>,
    pub protections: Vec<ProtectionPrevention>,
    pub multiplications: Vec<DamageMultiplication>,
    pub applied: Vec<AppliedDamage>,
    pub completions: Vec<DamageCompletion>,
}
impl DamageJournal {
    pub(crate) fn append(&mut self, mut other: Self) {
        self.batches.append(&mut other.batches);
        self.created.append(&mut other.created);
        self.attempts.append(&mut other.attempts);
        self.preventions.append(&mut other.preventions);
        self.protections.append(&mut other.protections);
        self.multiplications.append(&mut other.multiplications);
        self.applied.append(&mut other.applied);
        self.completions.append(&mut other.completions);
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageError {
    InvalidAmount,
    InvalidFactor,
    EmptyBatch,
    DuplicateSourceRecipient,
    UnsupportedSimultaneousScope,
    ShieldIdentityExhausted,
    MultiplierIdentityExhausted,
    BatchIdentityExhausted,
    AttemptIdentityExhausted,
    StageExhausted,
    WrongPhase,
    InapplicableEffect,
    InvalidAllocation,
    EffectsStillApplicable,
}
/// Effect registries and checked allocators. Active work lives in PendingWork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DamageState {
    next_shield: u64,
    next_multiplier: u64,
    next_batch: u64,
    next_attempt: u64,
    pub(crate) turn_preventions: BTreeMap<
        (StackEntryId, usize),
        (
            SeatId,
            TurnPreventionFilter,
            Option<crate::object_identity::ObjectId>,
        ),
    >,
    shields: BTreeMap<PreventionShieldId, PreventionShield>,
    reflections: BTreeMap<PreventionShieldId, DamageReflection>,
    multipliers: BTreeMap<DamageMultiplierId, DamageMultiplier>,
}
impl Default for DamageState {
    fn default() -> Self {
        Self {
            next_shield: 1,
            next_multiplier: 1,
            next_batch: 1,
            next_attempt: 1,
            turn_preventions: BTreeMap::new(),
            shields: BTreeMap::new(),
            reflections: BTreeMap::new(),
            multipliers: BTreeMap::new(),
        }
    }
}
impl DamageState {
    pub(crate) fn attack_allocation_mark(&self) -> [u64; 4] {
        [
            self.next_shield,
            self.next_multiplier,
            self.next_batch,
            self.next_attempt,
        ]
    }
    pub(crate) fn retain_attack_mark(&mut self, mark: [u64; 4]) {
        self.next_shield = self.next_shield.max(mark[0]);
        self.next_multiplier = self.next_multiplier.max(mark[1]);
        self.next_batch = self.next_batch.max(mark[2]);
        self.next_attempt = self.next_attempt.max(mark[3]);
    }

    pub(crate) fn counters(&self) -> (u64, u64, u64, u64) {
        (
            self.next_shield,
            self.next_multiplier,
            self.next_batch,
            self.next_attempt,
        )
    }
    pub(crate) fn add_multiplier(
        &mut self,
        creator: StackEntryId,
        target: DamageEffectTarget,
        factor: RulesInteger,
        filter: DamageSourceFilter,
    ) -> Result<DamageMultiplier, DamageError> {
        if !factor.is_positive() {
            return Err(DamageError::InvalidFactor);
        }
        let next = self
            .next_multiplier
            .checked_add(1)
            .ok_or(DamageError::MultiplierIdentityExhausted)?;
        let effect = DamageMultiplier {
            id: DamageMultiplierId(self.next_multiplier),
            creator,
            target,
            factor,
            filter,
        };
        self.multipliers.insert(effect.id, effect.clone());
        self.next_multiplier = next;
        Ok(effect)
    }
    pub(crate) fn multiplier(&self, id: DamageMultiplierId) -> Option<&DamageMultiplier> {
        self.multipliers.get(&id)
    }
    pub(crate) fn multipliers(&self) -> impl Iterator<Item = &DamageMultiplier> {
        self.multipliers.values()
    }
    pub(crate) fn add_shield(
        &mut self,
        creator: StackEntryId,
        target: DamageEffectTarget,
        amount: RulesInteger,
    ) -> Result<PreventionShield, DamageError> {
        if !amount.is_positive() {
            return Err(DamageError::InvalidAmount);
        }
        let next = self
            .next_shield
            .checked_add(1)
            .ok_or(DamageError::ShieldIdentityExhausted)?;
        let shield = PreventionShield {
            id: PreventionShieldId(self.next_shield),
            creator,
            target,
            remaining: amount,
        };
        self.shields.insert(shield.id, shield.clone());
        self.next_shield = next;
        Ok(shield)
    }
    pub(crate) fn add_reflecting_shield(
        &mut self,
        creator: StackEntryId,
        player: SeatId,
        amount: RulesInteger,
        reflection: DamageReflection,
    ) -> Result<PreventionShield, DamageError> {
        let shield = self.add_shield(
            creator,
            DamageEffectTarget::PlayerAndPermanents(player),
            amount,
        )?;
        self.reflections.insert(shield.id, reflection);
        Ok(shield)
    }

    pub(crate) fn reflection(&self, id: PreventionShieldId) -> Option<&DamageReflection> {
        self.reflections.get(&id)
    }

    pub(crate) fn shield(&self, id: PreventionShieldId) -> Option<&PreventionShield> {
        self.shields.get(&id)
    }
    pub(crate) fn shields(&self) -> impl Iterator<Item = &PreventionShield> {
        self.shields.values()
    }

    /// Validate the proposal and every checked allocation before counters move.
    pub(crate) fn begin_batch(
        &mut self,
        origin: DamageOrigin,
        kind: DamageKind,
        inputs: Vec<DamageInput>,
    ) -> Result<(ActiveDamageBatch, DamageJournal), DamageError> {
        if inputs.is_empty() {
            return Err(DamageError::EmptyBatch);
        }
        let mut pairs = BTreeSet::new();
        for input in &inputs {
            if !input.amount.is_positive() {
                return Err(DamageError::InvalidAmount);
            }
            if !pairs.insert((input.source.id(), input.target)) {
                return Err(DamageError::DuplicateSourceRecipient);
            }
        }
        self.validate_supported_scope(&inputs)?;
        let next_batch = self
            .next_batch
            .checked_add(1)
            .ok_or(DamageError::BatchIdentityExhausted)?;
        let id = DamageBatchId(self.next_batch);
        let mut next_attempt = self.next_attempt;
        let mut components = BTreeMap::new();
        let mut attempts = Vec::with_capacity(inputs.len());
        for input in inputs {
            let attempt_id = DamageAttemptId(next_attempt);
            next_attempt = next_attempt
                .checked_add(1)
                .ok_or(DamageError::AttemptIdentityExhausted)?;
            let attempt = DamageAttempt {
                id: attempt_id,
                batch: id,
                source: input.source,
                target: input.target,
                amount: input.amount,
            };
            components.insert(
                attempt_id,
                DamageComponent {
                    remaining: attempt.amount.clone(),
                    preventable: true,
                    recipient_unavailable: false,
                    affected_player: match attempt.target {
                        DamageRecipient::Player(player) => Some(player),
                        _ => None,
                    },
                    attempt: attempt.clone(),
                    used_multipliers: BTreeSet::new(),
                },
            );
            attempts.push(attempt);
        }
        let started = DamageBatchStarted {
            id,
            origin,
            kind,
            components: attempts.iter().map(|a| a.id).collect(),
        };
        let batch = ActiveDamageBatch {
            started: started.clone(),
            components,
            consumed_shields: BTreeSet::new(),
            stage: 0,
            phase: DamageBatchPhase::Selecting,
        };
        let journal = DamageJournal {
            batches: vec![started],
            attempts,
            ..Default::default()
        };
        self.next_batch = next_batch;
        self.next_attempt = next_attempt;
        Ok((batch, journal))
    }
    fn validate_supported_scope(&self, inputs: &[DamageInput]) -> Result<(), DamageError> {
        let mut recipients: BTreeMap<DamageRecipient, BTreeSet<DamageSourceId>> = BTreeMap::new();
        for input in inputs {
            recipients
                .entry(input.target)
                .or_default()
                .insert(input.source.id());
        }
        for (recipient, sources) in recipients {
            if sources.len() < 2 {
                continue;
            }
            let shields = self
                .shields
                .values()
                .filter(|s| s.target.matches(recipient) && s.remaining.is_positive())
                .count();
            let multiplier = self.multipliers.values().any(|m| {
                m.target.matches(recipient)
                    && sources.iter().any(|&source| m.filter.matches(source))
            });
            if shields > 1 || (shields > 0 && multiplier) {
                return Err(DamageError::UnsupportedSimultaneousScope);
            }
        }
        Ok(())
    }
    /// Current applications retain each source lineage and recipient event scope.
    pub(crate) fn applicable(&self, batch: &ActiveDamageBatch) -> Vec<DamageApplication> {
        if let DamageBatchPhase::Allocating { application } = batch.phase {
            return vec![application];
        }
        let mut applications = Vec::new();
        for shield in self.shields.values() {
            let Some(component) = batch.components.values().find(|component| {
                shield_applies_to(shield.target, component) && component.can_prevent_damage()
            }) else {
                continue;
            };
            let scope = match shield.target {
                DamageEffectTarget::PlayerAndPermanents(player) => {
                    DamageScope::ControlledRecipients(player)
                }
                _ => DamageScope::Recipient(component.attempt.target),
            };
            let application = DamageApplication {
                effect: shield.id.into(),
                scope,
            };
            if shield.remaining.is_positive() && !batch.consumed_shields.contains(&application) {
                applications.push(application);
            }
        }
        for effect in self.multipliers.values() {
            for component in batch.components.values() {
                if effect.target.matches(component.attempt.target)
                    && component.remaining.is_positive()
                    && effect.filter.matches(component.attempt.source.id())
                    && !component.used_multipliers.contains(&effect.id)
                {
                    applications.push(DamageApplication {
                        effect: effect.id.into(),
                        scope: DamageScope::Component(component.attempt.id),
                    });
                }
            }
        }
        applications
    }
    /// Select without debiting capacity or advancing the modification stage.
    pub(crate) fn select_prevention(
        &self,
        batch: &mut ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<(), DamageError> {
        if batch.phase != DamageBatchPhase::Selecting {
            return Err(DamageError::WrongPhase);
        }
        self.prevention_domain(batch, application)?;
        batch.phase = DamageBatchPhase::Allocating { application };
        Ok(())
    }
    /// A singleton eligible component or covering capacity fixes the partition.
    pub(crate) fn forced_allocation(
        &self,
        batch: &ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<Option<Vec<DamageAllocation>>, DamageError> {
        let (shield, eligible, required) = self.prevention_domain(batch, application)?;
        let total = eligible
            .values()
            .fold(0.into(), |sum: RulesInteger, amount| sum.plus(amount));
        if eligible.len() == 1 {
            return Ok(Some(vec![DamageAllocation {
                attempt: *eligible
                    .keys()
                    .next()
                    .ok_or(DamageError::InapplicableEffect)?,
                amount: required,
            }]));
        }
        if shield.remaining >= total {
            return Ok(Some(
                eligible
                    .into_iter()
                    .map(|(attempt, amount)| DamageAllocation { attempt, amount })
                    .collect(),
            ));
        }
        Ok(None)
    }

    /// Keep the already selected prevention effect if some valid capacity remains
    /// after departure. An empty domain reopens ordering without spending capacity.
    pub(crate) fn refresh_prevention_after_departure(
        &self,
        batch: &mut ActiveDamageBatch,
    ) -> Result<(), DamageError> {
        if let DamageBatchPhase::Allocating { application } = *batch.phase() {
            match self.prevention_domain(batch, application) {
                Ok(_) => {}
                Err(DamageError::InapplicableEffect) => batch.phase = DamageBatchPhase::Selecting,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
    /// Reconstruct the exact eligible quantities and required total from owned state.
    pub(crate) fn prevention_domain(
        &self,
        batch: &ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<
        (
            &PreventionShield,
            BTreeMap<DamageAttemptId, RulesInteger>,
            RulesInteger,
        ),
        DamageError,
    > {
        match batch.phase {
            DamageBatchPhase::Selecting => {}
            DamageBatchPhase::Allocating {
                application: selected,
            } if selected == application => {}
            DamageBatchPhase::Allocating { .. } => return Err(DamageError::WrongPhase),
        }
        let DamageEffectId::Prevention(id) = application.effect else {
            return Err(DamageError::InapplicableEffect);
        };
        let shield = self
            .shields
            .get(&id)
            .ok_or(DamageError::InapplicableEffect)?;
        let scope_matches = match (application.scope, shield.target) {
            (
                DamageScope::ControlledRecipients(player),
                DamageEffectTarget::PlayerAndPermanents(beneficiary),
            ) => player == beneficiary,
            (DamageScope::Recipient(recipient), target) => target.matches(recipient),
            _ => false,
        };
        if !scope_matches
            || !shield.remaining.is_positive()
            || batch.consumed_shields.contains(&application)
        {
            return Err(DamageError::InapplicableEffect);
        }
        let eligible: BTreeMap<_, _> = batch
            .components
            .values()
            .filter(|c| shield_applies_to(shield.target, c) && c.can_prevent_damage())
            .map(|c| (c.attempt.id, c.remaining.clone()))
            .collect();
        if eligible.is_empty() {
            return Err(DamageError::InapplicableEffect);
        }
        let total = eligible
            .values()
            .fold(0.into(), |sum: RulesInteger, amount| sum.plus(amount));
        let required = std::cmp::min(&shield.remaining, &total).clone();
        Ok((shield, eligible, required))
    }
    /// Validate every sparse row and late error before changing either owner.
    pub(crate) fn apply_prevention(
        &mut self,
        batch: &mut ActiveDamageBatch,
        application: DamageApplication,
        allocations: Vec<DamageAllocation>,
    ) -> Result<PreventionApplication, DamageError> {
        let (shield, mut remaining, required) = self.prevention_domain(batch, application)?;
        let target = match application.scope {
            DamageScope::Recipient(target) => target,
            DamageScope::ControlledRecipients(player) => DamageRecipient::Player(player),
            DamageScope::Component(_) => return Err(DamageError::InapplicableEffect),
        };
        let stage = batch
            .stage
            .checked_add(1)
            .ok_or(DamageError::StageExhausted)?;
        let mut selected = BTreeMap::new();
        let mut total = RulesInteger::from(0);
        for allocation in allocations {
            if !allocation.amount.is_positive() {
                return Err(DamageError::InvalidAllocation);
            }
            let current = remaining
                .get(&allocation.attempt)
                .ok_or(DamageError::InvalidAllocation)?;
            if allocation.amount > *current
                || selected
                    .insert(allocation.attempt, allocation.amount.clone())
                    .is_some()
            {
                return Err(DamageError::InvalidAllocation);
            }
            total = total.plus(&allocation.amount);
        }
        if total != required {
            return Err(DamageError::InvalidAllocation);
        }
        for (attempt, amount) in &selected {
            let current = remaining
                .get_mut(attempt)
                .ok_or(DamageError::InvalidAllocation)?;
            *current = current.minus(amount);
        }
        let fact = PreventionApplication {
            batch: batch.id(),
            shield: shield.id,
            creator: shield.creator,
            target,
            stage,
            allocations: selected
                .into_iter()
                .map(|(attempt, amount)| DamageAllocation { attempt, amount })
                .collect(),
            amount: total,
            damage_remaining: remaining.clone(),
            shield_remaining: shield.remaining.minus(&required),
        };
        // No fallible operation follows the first mutation of either owner.
        for (attempt, amount) in remaining {
            if let Some(component) = batch.components.get_mut(&attempt) {
                component.remaining = amount;
            }
        }
        if fact.shield_remaining.is_positive() {
            if let Some(shield) = self.shields.get_mut(&fact.shield) {
                shield.remaining = fact.shield_remaining.clone();
            }
        } else {
            self.shields.remove(&fact.shield);
            self.reflections.remove(&fact.shield);
        }
        batch.consumed_shields.insert(application);
        batch.stage = stage;
        batch.phase = DamageBatchPhase::Selecting;
        Ok(fact)
    }
    pub(crate) fn apply_multiplier(
        &self,
        batch: &mut ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<DamageMultiplication, DamageError> {
        if batch.phase != DamageBatchPhase::Selecting {
            return Err(DamageError::WrongPhase);
        }
        let (DamageEffectId::Multiplier(id), DamageScope::Component(attempt)) =
            (application.effect, application.scope)
        else {
            return Err(DamageError::InapplicableEffect);
        };
        let effect = self
            .multipliers
            .get(&id)
            .ok_or(DamageError::InapplicableEffect)?;
        let component = batch
            .components
            .get(&attempt)
            .ok_or(DamageError::InapplicableEffect)?;
        if !effect.target.matches(component.attempt.target)
            || !component.remaining.is_positive()
            || component.used_multipliers.contains(&id)
            || !effect.filter.matches(component.attempt.source.id())
        {
            return Err(DamageError::InapplicableEffect);
        }
        let stage = batch
            .stage
            .checked_add(1)
            .ok_or(DamageError::StageExhausted)?;
        let fact = DamageMultiplication {
            batch: batch.id(),
            attempt,
            multiplier: id,
            creator: effect.creator,
            target: component.attempt.target,
            stage,
            before: component.remaining.clone(),
            factor: effect.factor.clone(),
            after: component.remaining.times(&effect.factor),
        };
        if let Some(component) = batch.components.get_mut(&attempt) {
            component.remaining = fact.after.clone();
            component.used_multipliers.insert(id);
        }
        batch.stage = stage;
        Ok(fact)
    }
    /// Prepare component results without mutating marks, life or parent state.
    pub(crate) fn finish_batch(
        &self,
        batch: &ActiveDamageBatch,
    ) -> Result<(Vec<AppliedDamage>, Vec<DamageCompletion>), DamageError> {
        if batch.phase != DamageBatchPhase::Selecting {
            return Err(DamageError::WrongPhase);
        }
        if !self.applicable(batch).is_empty() {
            return Err(DamageError::EffectsStillApplicable);
        }
        let mut applied = Vec::new();
        let mut completions = Vec::new();
        for component in batch.components.values() {
            let attempt = &component.attempt;
            let outcome = if component.remaining.is_positive() {
                applied.push(AppliedDamage {
                    batch: batch.id(),
                    attempt: attempt.id,
                    source: attempt.source.clone(),
                    target: attempt.target,
                    amount: component.remaining.clone(),
                });
                DamageOutcome::Dealt(component.remaining.clone())
            } else if component.recipient_unavailable {
                DamageOutcome::RecipientUnavailable
            } else {
                DamageOutcome::FullyPrevented
            };
            completions.push(DamageCompletion {
                batch: batch.id(),
                attempt: attempt.id,
                outcome,
            });
        }
        Ok((applied, completions))
    }
    pub(crate) fn expire_reflecting_shields(&mut self) {
        self.shields
            .retain(|id, _| !self.reflections.contains_key(id));
        self.reflections.clear();
    }

    pub(crate) fn retain_recipients(&mut self, live_battlefield: &BTreeSet<ObjectId>) {
        let live = |target: DamageEffectTarget| match target {
            DamageEffectTarget::Permanent(object) => live_battlefield.contains(&object),
            DamageEffectTarget::Player(_) | DamageEffectTarget::PlayerAndPermanents(_) => true,
        };
        self.shields.retain(|_, shield| live(shield.target));
        self.reflections
            .retain(|id, _| self.shields.contains_key(id));
        self.multipliers
            .retain(|_, multiplier| live(multiplier.target));
    }
}

fn shield_applies_to(target: DamageEffectTarget, component: &DamageComponent) -> bool {
    match target {
        DamageEffectTarget::PlayerAndPermanents(player) => {
            component.affected_player == Some(player)
        }
        _ => target.matches(component.attempt.target),
    }
}

#[cfg(test)]
mod tests;
