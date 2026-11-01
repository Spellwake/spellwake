//! Narrow executable kernel for vanilla creatures and exact-cost, single-target
//! instant damage/finite prevention/multiplication/bounce/counter/copy and literal layer-7 programs. This is not the general Magic interpreter.
//! The closed world has no dynamic selectors/dependencies, CDAs, other layers, durations,
//! other abilities, general replacement effects, triggers,
//! teams, planeswalkers, battles, equipment, combat, phase changes or player elimination.
//! Fixed noncreature Land tap mana abilities share the pool and ordinary priority
//! path. Nested payment windows, mana replacements/triggers and turns are absent.
use crate::{
    TargetId,
    card_identity::{CardDestination, CardId, CardObjectId, CardObjects, CardZone},
    characteristics::{
        CharacteristicsError, ContinuousEffectId, PowerToughness, PowerToughnessCounters,
        PowerToughnessEffect, PowerToughnessEvaluation, PowerToughnessOperation,
        PowerToughnessState,
    },
    damage::{
        ActiveDamageAttempt, DamageAttemptId, DamageEffectCreation, DamageEffectId, DamageError,
        DamageJournal, DamageMultiplier, DamageMultiplierId, DamageState, PreventionShield,
        PreventionShieldId,
    },
    mana::{
        FixedManaAbility, FloatingMana, ManaAbilityId, ManaCost, ManaPayment, ManaPaymentError,
        ManaPaymentRecord, ManaProductionRecord, ManaType, prepare_payment,
    },
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackKind, StackWindow, StackWindowError, WindowPhase},
    targets::{
        ResolutionPermission, TargetCheck, TargetChoice, TargetClause, TargetClauseId, TargetRef,
    },
};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    Damage { amount: RulesInteger },
    CreatePreventionShield { amount: RulesInteger },
    CreateDamageMultiplier { factor: RulesInteger },
    ReturnToOwnersHand,
    CounterSpell,
    CopySpell,
    ContinuousPowerToughness { operation: PowerToughnessOperation },
    PutPowerToughnessCounters { counters: PowerToughnessCounters },
}
/// One trusted resolution-time choice: retaining is distinct from fresh capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyTargetChoice {
    Retain,
    Change(TargetRef),
}
/// Owned post-target-check continuation. Only the executor constructs this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyResolution {
    revision: u64,
    entry: StackEntryId,
    original: StackEntryId,
    chooser: SeatId,
    instruction: usize,
}
impl CopyResolution {
    pub fn instruction_index(&self) -> usize {
        self.instruction
    }
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    pub fn original(&self) -> StackEntryId {
        self.original
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
}
/// Immutable domain/token into the executor-owned active attempt. Remaining and
/// used quantities live only in DamageState; this is not a second continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageChoice {
    revision: u64,
    attempt: DamageAttemptId,
    stage: u64,
    source: StackEntryId,
    chooser: SeatId,
    options: Vec<DamageEffectId>,
    instruction: usize,
}
impl DamageChoice {
    pub fn instruction_index(&self) -> usize {
        self.instruction
    }
    pub fn attempt(&self) -> DamageAttemptId {
        self.attempt
    }
    pub fn source(&self) -> StackEntryId {
        self.source
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn options(&self) -> &[DamageEffectId] {
        &self.options
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionFrame {
    CopyTargets(CopyResolution),
    DamageOrder(DamageChoice),
}
impl ResolutionFrame {
    pub fn chooser(&self) -> SeatId {
        match self {
            Self::CopyTargets(frame) => frame.chooser(),
            Self::DamageOrder(frame) => frame.chooser(),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionReply {
    CopyTargets(CopyTargetChoice),
    DamageOrder(DamageEffectId),
}
impl From<CopyTargetChoice> for DecisionReply {
    fn from(choice: CopyTargetChoice) -> Self {
        Self::CopyTargets(choice)
    }
}
/// Shared begin/resume progress with only this native operation's damage append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionProgress {
    Complete {
        state: EffectWindow,
        journal: DamageJournal,
    },
    Pending {
        state: EffectWindow,
        frame: ResolutionFrame,
        journal: DamageJournal,
    },
}
/// Restricted authored programs do not certify the corresponding complete cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    Land {
        abilities: Vec<FixedManaAbility>,
    },
    VanillaCreature {
        power: RulesInteger,
        toughness: RulesInteger,
    },
    Instant {
        cost: ManaCost,
        instructions: Vec<Instruction>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedCard {
    pub card: CardId,
    pub owner: SeatId,
    pub destination: CardDestination,
    pub program: Program,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectError {
    Window(StackWindowError),
    InvalidSetup,
    UnknownCard,
    NotInstant,
    NotManaSource,
    UnknownManaAbility,
    NotOwnHand,
    IllegalTarget,
    InsufficientMana,
    ManaPaymentRequired,
    InvalidManaPayment(ManaPaymentError),
    WrongPhase,
    InconsistentProgram,
    CopyChoiceRequired,
    UnexpectedCopyChoice,
    WrongChooser,
    WrongResolutionFrame,
    Characteristics(CharacteristicsError),
    Damage(DamageError),
    WrongDecisionKind,
    DamageChoiceRequired,
}
impl From<StackWindowError> for EffectError {
    fn from(error: StackWindowError) -> Self {
        Self::Window(error)
    }
}
impl From<CharacteristicsError> for EffectError {
    fn from(error: CharacteristicsError) -> Self {
        Self::Characteristics(error)
    }
}
impl From<DamageError> for EffectError {
    fn from(error: DamageError) -> Self {
        Self::Damage(error)
    }
}
/// Immutable, canonical native state. This is not a safe network projection and
/// cannot be opened over arbitrary existing games: every object has a closed-world program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectWindow {
    revision: u64,
    window: StackWindow,
    programs: BTreeMap<CardId, Program>,
    payloads: BTreeMap<StackEntryId, Vec<Instruction>>,
    resolution: Option<ResolutionCursor>,
    damage: BTreeMap<CardObjectId, RulesInteger>,
    mana: BTreeMap<(SeatId, ManaType), RulesInteger>,
    characteristics: PowerToughnessState,
    damage_state: DamageState,
}
/// Owned native cast result; a receipt exists only when this transition succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastTransition {
    state: EffectWindow,
    payment: ManaPaymentRecord,
}
impl CastTransition {
    pub fn state(&self) -> &EffectWindow {
        &self.state
    }
    pub fn payment(&self) -> &ManaPaymentRecord {
        &self.payment
    }
    pub fn into_state(self) -> EffectWindow {
        self.state
    }
}
/// Successful native activation with its immediate production evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaActivationTransition {
    state: EffectWindow,
    production: ManaProductionRecord,
}
impl ManaActivationTransition {
    pub fn state(&self) -> &EffectWindow {
        &self.state
    }
    pub fn production(&self) -> &ManaProductionRecord {
        &self.production
    }
    pub fn into_state(self) -> EffectWindow {
        self.state
    }
}
/// Points into the owned payload. Tokens never own another mutable program counter.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolutionCursor {
    entry: StackEntryId,
    instruction: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureView {
    pub object: CardObjectId,
    pub power: RulesInteger,
    pub toughness: RulesInteger,
    pub damage: RulesInteger,
}

impl EffectWindow {
    pub fn new(
        order: Vec<SeatId>,
        active: SeatId,
        cards: Vec<SeedCard>,
        mana: Vec<FloatingMana>,
    ) -> Result<Self, EffectError> {
        let mut objects = CardObjects::new(0);
        let mut programs = BTreeMap::new();
        for seed in cards {
            match &seed.program {
                Program::Land { abilities } => {
                    let ids: BTreeSet<_> = abilities.iter().map(|ability| ability.id).collect();
                    if abilities.is_empty()
                        || ids.len() != abilities.len()
                        || abilities.iter().any(|ability| !ability.valid())
                        || matches!(seed.destination, CardDestination::Stack { .. })
                    {
                        return Err(EffectError::InvalidSetup);
                    }
                }
                Program::VanillaCreature { .. } => {}
                Program::Instant { cost, instructions } => {
                    if !cost.valid()
                        || !Self::valid_program(instructions)
                        || matches!(seed.destination, CardDestination::Battlefield { .. })
                    {
                        return Err(EffectError::InvalidSetup);
                    }
                }
            }
            if programs.insert(seed.card, seed.program).is_some() {
                return Err(EffectError::InvalidSetup);
            }
            objects = objects
                .seed_card(objects.revision(), seed.card, seed.owner, seed.destination)
                .map_err(StackWindowError::from)?
                .next;
        }
        let mut pools = BTreeMap::new();
        for pool in mana {
            if !order.contains(&pool.player)
                || pool.amount < 0.into()
                || pools
                    .insert((pool.player, pool.mana_type), pool.amount)
                    .is_some()
            {
                return Err(EffectError::InvalidSetup);
            }
        }
        Ok(Self {
            revision: 0,
            window: StackWindow::new(order, active, objects)?,
            programs,
            payloads: BTreeMap::new(),
            resolution: None,
            damage: BTreeMap::new(),
            mana: pools,
            characteristics: PowerToughnessState::default(),
            damage_state: DamageState::default(),
        })
    }
    pub fn window(&self) -> &StackWindow {
        &self.window
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn next_revision(&self, expected: u64) -> Result<u64, EffectError> {
        if expected != self.revision {
            return Err(StackWindowError::StaleRevision.into());
        }
        self.revision
            .checked_add(1)
            .ok_or(StackWindowError::RevisionExhausted.into())
    }
    pub fn prevention_shields(&self) -> impl Iterator<Item = &PreventionShield> {
        self.damage_state.shields()
    }
    pub fn prevention_shield(&self, id: PreventionShieldId) -> Option<&PreventionShield> {
        self.damage_state.shield(id)
    }
    pub fn damage_multipliers(&self) -> impl Iterator<Item = &DamageMultiplier> {
        self.damage_state.multipliers()
    }
    pub fn damage_multiplier(&self, id: DamageMultiplierId) -> Option<&DamageMultiplier> {
        self.damage_state.multiplier(id)
    }
    pub fn pending_damage(&self) -> Option<&ActiveDamageAttempt> {
        self.damage_state.active()
    }
    pub fn mana(&self, player: SeatId, mana_type: ManaType) -> RulesInteger {
        self.mana
            .get(&(player, mana_type))
            .cloned()
            .unwrap_or_else(|| 0.into())
    }
    /// Copiable PT equals defined vanilla PT in this subset, in any supported zone.
    /// This does not implement copy effects or all copiable characteristics.
    pub fn copiable_power_toughness(&self, object: CardObjectId) -> Option<PowerToughness> {
        let card = self.window.cards().get(object)?;
        let Program::VanillaCreature { power, toughness } = self.programs.get(&card.card())? else {
            return None;
        };
        Some(PowerToughness {
            power: power.clone(),
            toughness: toughness.clone(),
        })
    }
    /// Exact current battlefield evaluation shared by legality and supported SBA.
    /// No cached display value is authoritative.
    pub fn power_toughness(&self, object: CardObjectId) -> Option<PowerToughnessEvaluation> {
        if self.window.cards().get(object)?.zone() != CardZone::Battlefield {
            return None;
        }
        Some(
            self.characteristics
                .evaluate(object, self.copiable_power_toughness(object)?),
        )
    }
    /// Privileged read-only physical counter evidence, not a network projection.
    pub fn power_toughness_counters(&self, object: CardObjectId) -> Option<PowerToughnessCounters> {
        self.power_toughness(object)?;
        Some(self.characteristics.counters(object))
    }
    pub fn power_toughness_effect(&self, id: ContinuousEffectId) -> Option<&PowerToughnessEffect> {
        self.characteristics.effect(id)
    }
    pub fn creature(&self, object: CardObjectId) -> Option<CreatureView> {
        let evaluation = self.power_toughness(object)?;
        Some(CreatureView {
            object,
            power: evaluation.derived.power,
            toughness: evaluation.derived.toughness,
            damage: self
                .damage
                .get(&object)
                .cloned()
                .unwrap_or_else(|| 0.into()),
        })
    }
    /// Settle this universe's supported simultaneous SBA: copy cessation,
    /// zero/lethal derived toughness and opposing +1/+1/-1/-1 counters.
    /// Prevention settles before this boundary. Indestructibility, regeneration
    /// and other state-based exceptions remain unsupported.
    pub fn settle_checks(&self, expected: u64) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        if !matches!(
            self.window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            return Err(EffectError::WrongPhase);
        }
        let mut next = self.clone();
        loop {
            // Both lists use the same pre-check state. Counters are not canceled
            // before deciding which creatures die in this simultaneous batch.
            let dead: Vec<_> = next
                .window
                .cards()
                .iter()
                .filter_map(|card| next.creature(card.id()))
                .filter(|creature| {
                    creature.toughness <= 0.into() || creature.damage >= creature.toughness
                })
                .map(|creature| (creature.object, CardDestination::Graveyard))
                .collect();
            let cancellations = next.characteristics.counter_cancellations();
            let departed_copies = next
                .window
                .spell_copies()
                .values()
                .any(|copy| copy.zone != CardZone::Stack);
            if dead.is_empty() && cancellations.is_empty() && !departed_copies {
                break;
            }
            if !dead.is_empty() {
                next.window = next
                    .window
                    .record_check_moves(next.window.revision(), dead)?;
            }
            next.characteristics
                .apply_counter_cancellations(cancellations);
            if departed_copies {
                next.window = next.window.cease_departed_copies(next.window.revision())?;
            }
            next.prune_creature_state();
        }
        next.prune_payloads();
        next.window = next.window.finish_checks(next.window.revision())?;
        next.revision = revision;
        Ok(next)
    }
    pub fn pass(&self, expected: u64, player: SeatId) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        Ok(Self {
            revision,
            window: self.window.pass(self.window.revision(), player)?,
            ..self.clone()
        })
    }
    /// Fixed noncreature-land mana ability in an ordinary priority window only.
    /// The authored yield, tap cost and pool credit form one native successor.
    pub fn activate_mana(
        &self,
        expected: u64,
        actor: SeatId,
        source: CardObjectId,
        ability: ManaAbilityId,
    ) -> Result<ManaActivationTransition, EffectError> {
        let revision = self.next_revision(expected)?;
        let card = self
            .window
            .cards()
            .get(source)
            .ok_or(EffectError::UnknownCard)?;
        let Some(Program::Land { abilities }) = self.programs.get(&card.card()) else {
            return Err(EffectError::NotManaSource);
        };
        let selected = abilities
            .iter()
            .find(|entry| entry.id == ability)
            .ok_or(EffectError::UnknownManaAbility)?;
        let window = self
            .window
            .record_tap_action(self.window.revision(), actor, source)?;
        let mut next = self.clone();
        next.revision = revision;
        next.window = window;
        for (mana_type, amount) in &selected.produced {
            next.mana.insert(
                (actor, *mana_type),
                self.mana(actor, *mana_type).plus(amount),
            );
        }
        Ok(ManaActivationTransition {
            state: next,
            production: ManaProductionRecord {
                source,
                ability,
                controller: actor,
                produced: selected.produced.clone(),
            },
        })
    }
    /// Cast with the unique mandatory payment for a cost containing no generic mana.
    /// Explicit generic allocation uses the same cast_with_payment path.
    pub fn cast(
        &self,
        expected: u64,
        actor: SeatId,
        spell: CardObjectId,
        target: TargetRef,
    ) -> Result<Self, EffectError> {
        Ok(self
            .cast_with_payment(expected, actor, spell, target, None)?
            .into_state())
    }
    /// Atomically validate the authored cost and exact selected spend, then admit
    /// the supported spell. No general announcement or mana-ability flow is implied.
    pub fn cast_with_payment(
        &self,
        expected: u64,
        actor: SeatId,
        spell: CardObjectId,
        target: TargetRef,
        selected: Option<ManaPayment>,
    ) -> Result<CastTransition, EffectError> {
        let revision = self.next_revision(expected)?;
        let source = self
            .window
            .cards()
            .get(spell)
            .ok_or(EffectError::UnknownCard)?;
        if source.zone() != CardZone::Hand(actor) {
            return Err(EffectError::NotOwnHand);
        }
        let Some(Program::Instant { cost, instructions }) = self.programs.get(&source.card())
        else {
            return Err(EffectError::NotInstant);
        };
        if !self.legal_target(
            instructions
                .first()
                .ok_or(EffectError::InconsistentProgram)?,
            target,
        ) {
            return Err(EffectError::IllegalTarget);
        }
        let spend = prepare_payment(cost, selected.as_ref(), |mana_type| {
            self.mana(actor, mana_type)
        })
        .map_err(|error| match error {
            ManaPaymentError::InsufficientMana => EffectError::InsufficientMana,
            ManaPaymentError::PaymentRequired => EffectError::ManaPaymentRequired,
            other => EffectError::InvalidManaPayment(other),
        })?;
        let window = self.window.record_targeted_card_spell(
            self.window.revision(),
            actor,
            spell,
            vec![TargetClause {
                id: TargetClauseId(0),
                minimum: 1,
                maximum: 1,
            }],
            vec![TargetChoice {
                id: TargetId(0),
                clause: TargetClauseId(0),
                target,
            }],
        )?;
        let entry = window
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?
            .id();
        let mut next = self.clone();
        next.revision = revision;
        next.payloads.insert(entry, instructions.clone());
        next.window = window;
        for payment in &spend {
            next.mana.insert(
                (actor, payment.mana_type),
                self.mana(actor, payment.mana_type).minus(&payment.amount),
            );
        }
        Ok(CastTransition {
            state: next,
            payment: ManaPaymentRecord {
                source: entry,
                payer: actor,
                cost: cost.clone(),
                spend,
            },
        })
    }
    /// Cross-layer overflow fault injection; absent from production builds.
    #[cfg(test)]
    pub(crate) fn set_test_revision(&mut self, revision: u64) {
        self.revision = revision;
    }
    #[cfg(test)]
    pub(crate) fn set_test_window_revision(&mut self, revision: u64) {
        self.window.set_test_revision(revision);
    }
    /// Execute exactly the selected top instant. Identity and creature-type checks
    /// are computed natively, not supplied as caller booleans. Leaves CR 117.5
    /// pending so lethal creatures remain until settle_checks, not mid-resolution.
    pub fn resolve_top(&self, expected: u64) -> Result<Self, EffectError> {
        self.resolve_with_choice(expected, None)
    }
    /// Explicit resolution-time target decision for a CopySpell instruction.
    /// This trusted native continuation is not a browser-facing decision API.
    pub fn resolve_top_with_copy_choice(
        &self,
        expected: u64,
        actor: SeatId,
        choice: CopyTargetChoice,
    ) -> Result<Self, EffectError> {
        self.resolve_with_choice(expected, Some((actor, choice)))
    }
    fn resolve_with_choice(
        &self,
        expected: u64,
        copy_choice: Option<(SeatId, CopyTargetChoice)>,
    ) -> Result<Self, EffectError> {
        let progress = self.begin_resolution(expected)?;
        match progress {
            ResolutionProgress::Complete { state, .. } => {
                if copy_choice.is_some() {
                    return Err(EffectError::UnexpectedCopyChoice);
                }
                Ok(state)
            }
            ResolutionProgress::Pending {
                state,
                frame: ResolutionFrame::CopyTargets(frame),
                ..
            } => {
                let (actor, choice) = copy_choice.ok_or(EffectError::CopyChoiceRequired)?;
                match state.resume_resolution(
                    state.revision(),
                    &ResolutionFrame::CopyTargets(frame),
                    actor,
                    DecisionReply::CopyTargets(choice),
                )? {
                    ResolutionProgress::Complete { state, .. } => Ok(state),
                    ResolutionProgress::Pending {
                        frame: ResolutionFrame::CopyTargets(_),
                        ..
                    } => Err(EffectError::CopyChoiceRequired),
                    ResolutionProgress::Pending { .. } => Err(EffectError::DamageChoiceRequired),
                }
            }
            ResolutionProgress::Pending { .. } => Err(EffectError::DamageChoiceRequired),
        }
    }
    /// Run the target gate once for the whole program, then execute to a real boundary.
    pub fn begin_resolution(&self, expected: u64) -> Result<ResolutionProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.resolution.is_some() || self.damage_state.active().is_some() {
            return Err(EffectError::WrongPhase);
        }
        let entry = self
            .window
            .checking_targets()
            .ok_or(EffectError::WrongPhase)?
            .clone();
        let program = self
            .payloads
            .get(&entry.id())
            .ok_or(EffectError::InconsistentProgram)?;
        let first = program.first().ok_or(EffectError::InconsistentProgram)?;
        let [bound] = entry.targets().targets() else {
            return Err(EffectError::InconsistentProgram);
        };
        let mut next = self.clone();
        next.revision = revision;
        next.window = next.window.check_resolution_targets(
            next.window.revision(),
            entry.id(),
            vec![TargetCheck {
                target: bound.choice().id,
                legal: self.legal_target(first, bound.choice().target),
            }],
        )?;
        if next
            .window
            .target_assessment()
            .ok_or(EffectError::InconsistentProgram)?
            .permission()
            == ResolutionPermission::AllTargetsIllegal
        {
            next.window = next.window.finish_unresolved(
                next.window.revision(),
                entry.id(),
                Some(CardDestination::Graveyard),
            )?;
            return Ok(ResolutionProgress::Complete {
                state: next,
                journal: DamageJournal::default(),
            });
        }
        next.resolution = Some(ResolutionCursor {
            entry: entry.id(),
            instruction: 0,
        });
        next.progress_resolution(DamageJournal::default())
    }
    /// Privileged native continuation evidence, not a stepping or wire API.
    pub fn instruction_index(&self) -> Option<usize> {
        self.resolution.as_ref().map(|cursor| cursor.instruction)
    }
    fn current_instruction(&self) -> Result<&Instruction, EffectError> {
        let cursor = self
            .resolution
            .as_ref()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if self.window.resolving().map(|entry| entry.id()) != Some(cursor.entry) {
            return Err(EffectError::WrongResolutionFrame);
        }
        self.payloads
            .get(&cursor.entry)
            .and_then(|program| program.get(cursor.instruction))
            .ok_or(EffectError::InconsistentProgram)
    }
    fn advance_instruction(&mut self) -> Result<(), EffectError> {
        let cursor = self
            .resolution
            .as_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        cursor.instruction = cursor
            .instruction
            .checked_add(1)
            .ok_or(EffectError::InconsistentProgram)?;
        Ok(())
    }
    fn copy_resolution_frame(&self) -> Result<CopyResolution, EffectError> {
        let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
        if self.damage_state.active().is_some()
            || self.current_instruction()? != &Instruction::CopySpell
        {
            return Err(EffectError::WrongResolutionFrame);
        }
        let [bound] = entry.targets().targets() else {
            return Err(EffectError::InconsistentProgram);
        };
        let TargetRef::Stack(original) = bound.choice().target else {
            return Err(EffectError::InconsistentProgram);
        };
        Ok(CopyResolution {
            revision: self.revision,
            entry: entry.id(),
            original,
            chooser: entry.controller(),
            instruction: self
                .instruction_index()
                .ok_or(EffectError::WrongResolutionFrame)?,
        })
    }
    /// Resume the exact current instruction, then continue the rest of its program.
    /// One supplied copy choice; additional choices return an atomic required-input error.
    pub fn resume_copy_resolution(
        &self,
        expected: u64,
        frame: &CopyResolution,
        actor: SeatId,
        choice: CopyTargetChoice,
    ) -> Result<Self, EffectError> {
        match self.resume_resolution(
            expected,
            &ResolutionFrame::CopyTargets(frame.clone()),
            actor,
            DecisionReply::CopyTargets(choice),
        )? {
            ResolutionProgress::Complete { state, .. } => Ok(state),
            ResolutionProgress::Pending {
                frame: ResolutionFrame::CopyTargets(_),
                ..
            } => Err(EffectError::CopyChoiceRequired),
            ResolutionProgress::Pending { .. } => Err(EffectError::DamageChoiceRequired),
        }
    }
    /// Shared typed continuation used by the game driver and convenience methods.
    pub fn resume_resolution(
        &self,
        expected: u64,
        frame: &ResolutionFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<ResolutionProgress, EffectError> {
        match (frame, reply) {
            (ResolutionFrame::CopyTargets(frame), DecisionReply::CopyTargets(choice)) => {
                let revision = self.next_revision(expected)?;
                if &self.copy_resolution_frame()? != frame {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != frame.chooser {
                    return Err(EffectError::WrongChooser);
                }
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongPhase)?
                    .clone();
                let mut next = self.clone();
                next.revision = revision;
                let mut journal = DamageJournal::default();
                next.apply_instruction(
                    entry,
                    Instruction::CopySpell,
                    Some((actor, choice)),
                    &mut journal,
                )?;
                next.advance_instruction()?;
                next.progress_resolution(journal)
            }
            (ResolutionFrame::DamageOrder(frame), DecisionReply::DamageOrder(effect)) => {
                let revision = self.next_revision(expected)?;
                if &self.damage_choice()? != frame {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != frame.chooser {
                    return Err(EffectError::WrongChooser);
                }
                let mut next = self.clone();
                next.revision = revision;
                let mut journal = DamageJournal::default();
                journal.record(next.damage_state.apply(effect)?);
                next.progress_resolution(journal)
            }
            _ => Err(EffectError::WrongDecisionKind),
        }
    }
    fn damage_choice(&self) -> Result<DamageChoice, EffectError> {
        if !matches!(self.current_instruction()?, Instruction::Damage { .. }) {
            return Err(EffectError::WrongResolutionFrame);
        }
        let active = self
            .damage_state
            .active()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
        if entry.id() != active.attempt().source {
            return Err(EffectError::WrongResolutionFrame);
        }
        let card = self
            .window
            .cards()
            .get(active.attempt().target)
            .ok_or(EffectError::InconsistentProgram)?;
        let options = self.damage_state.applicable();
        if options.len() < 2 {
            return Err(EffectError::WrongPhase);
        }
        Ok(DamageChoice {
            revision: self.revision,
            attempt: active.attempt().id,
            stage: active.stage(),
            source: entry.id(),
            chooser: card.controller().unwrap_or(card.owner()),
            options,
            instruction: self
                .instruction_index()
                .ok_or(EffectError::WrongResolutionFrame)?,
        })
    }
    fn progress_resolution(
        mut self,
        mut journal: DamageJournal,
    ) -> Result<ResolutionProgress, EffectError> {
        loop {
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongPhase)?
                .clone();
            let cursor = self
                .resolution
                .as_ref()
                .ok_or(EffectError::WrongResolutionFrame)?;
            if cursor.entry != entry.id() {
                return Err(EffectError::WrongResolutionFrame);
            }
            let program = self
                .payloads
                .get(&entry.id())
                .ok_or(EffectError::InconsistentProgram)?;
            if cursor.instruction == program.len() {
                if self.damage_state.active().is_some() {
                    return Err(EffectError::InconsistentProgram);
                }
                self.window = self.window.finish_resolution(
                    self.window.revision(),
                    entry.id(),
                    Some(CardDestination::Graveyard),
                )?;
                self.resolution = None;
                return Ok(ResolutionProgress::Complete {
                    state: self,
                    journal,
                });
            }
            let instruction = program
                .get(cursor.instruction)
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            if self.damage_state.active().is_some() {
                if !matches!(instruction, Instruction::Damage { .. }) {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if let Some(frame) = self.progress_damage(&mut journal)? {
                    return Ok(ResolutionProgress::Pending {
                        state: self,
                        frame,
                        journal,
                    });
                }
                continue;
            }
            let [bound] = entry.targets().targets() else {
                return Err(EffectError::InconsistentProgram);
            };
            let target = bound.choice().target;
            // This is operand availability, not another resolution target gate.
            if !self.legal_target(&instruction, target) {
                self.advance_instruction()?;
                continue;
            }
            match instruction {
                Instruction::CopySpell => {
                    let frame = ResolutionFrame::CopyTargets(self.copy_resolution_frame()?);
                    return Ok(ResolutionProgress::Pending {
                        state: self,
                        frame,
                        journal,
                    });
                }
                Instruction::Damage { amount } => {
                    let TargetRef::Card(target) = target else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    journal
                        .attempts
                        .push(self.damage_state.begin(entry.id(), target, amount)?);
                }
                other => {
                    self.apply_instruction(entry, other, None, &mut journal)?;
                    self.advance_instruction()?;
                }
            }
        }
    }
    fn progress_damage(
        &mut self,
        journal: &mut DamageJournal,
    ) -> Result<Option<ResolutionFrame>, EffectError> {
        loop {
            let options = self.damage_state.applicable();
            match options.as_slice() {
                [] => {
                    let source = self
                        .damage_state
                        .active()
                        .ok_or(EffectError::WrongPhase)?
                        .attempt()
                        .source;
                    if self.window.resolving().map(|entry| entry.id()) != Some(source) {
                        return Err(EffectError::WrongResolutionFrame);
                    }
                    let (applied, completion) = self.damage_state.finish()?;
                    if let Some(fact) = applied {
                        let previous = self
                            .damage
                            .get(&fact.target)
                            .cloned()
                            .unwrap_or_else(|| 0.into());
                        self.damage.insert(fact.target, previous.plus(&fact.amount));
                        journal.applied.push(fact);
                    }
                    journal.completions.push(completion);
                    self.advance_instruction()?;
                    return Ok(None);
                }
                [id] => journal.record(self.damage_state.apply(*id)?),
                _ => return Ok(Some(ResolutionFrame::DamageOrder(self.damage_choice()?))),
            }
        }
    }
    fn apply_instruction(
        &mut self,
        entry: crate::stack_window::StackEntry,
        instruction: Instruction,
        copy_choice: Option<(SeatId, CopyTargetChoice)>,
        journal: &mut DamageJournal,
    ) -> Result<(), EffectError> {
        let index = self
            .instruction_index()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let [bound] = entry.targets().targets() else {
            return Err(EffectError::InconsistentProgram);
        };
        let choice = bound.choice();
        let target = choice.target;
        match instruction {
            Instruction::Damage { .. } => return Err(EffectError::InconsistentProgram),
            Instruction::CreateDamageMultiplier { factor } => {
                let TargetRef::Card(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let multiplier = self
                    .damage_state
                    .add_multiplier(entry.id(), target, factor)?;
                journal.created.push(DamageEffectCreation::Multiplier {
                    multiplier,
                    instruction: index,
                });
            }
            Instruction::CreatePreventionShield { amount } => {
                let TargetRef::Card(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let shield = self.damage_state.add_shield(entry.id(), target, amount)?;
                journal.created.push(DamageEffectCreation::Prevention {
                    shield,
                    instruction: index,
                });
            }
            Instruction::CounterSpell => {
                self.window = self.window.record_targeted_resolution_move(
                    self.window.revision(),
                    choice.id,
                    CardDestination::Graveyard,
                )?;
            }
            Instruction::CopySpell => {
                let (actor, selected) = copy_choice.ok_or(EffectError::CopyChoiceRequired)?;
                if actor != entry.controller() {
                    return Err(EffectError::WrongChooser);
                }
                let TargetRef::Stack(original) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let original_entry = self
                    .window
                    .entries()
                    .iter()
                    .find(|item| item.id() == original)
                    .ok_or(EffectError::InconsistentProgram)?;
                let payload = self
                    .payloads
                    .get(&original)
                    .ok_or(EffectError::InconsistentProgram)?
                    .clone();
                let changes = match selected {
                    CopyTargetChoice::Retain => Vec::new(),
                    CopyTargetChoice::Change(target) => {
                        if !self.legal_target(
                            payload.first().ok_or(EffectError::InconsistentProgram)?,
                            target,
                        ) {
                            return Err(EffectError::IllegalTarget);
                        }
                        let [bound] = original_entry.targets().targets() else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let mut replacement = bound.choice();
                        replacement.target = target;
                        vec![replacement]
                    }
                };
                self.window = self.window.record_spell_copy(
                    self.window.revision(),
                    original,
                    actor,
                    changes,
                )?;
                let copy = self
                    .window
                    .entries()
                    .last()
                    .ok_or(EffectError::InconsistentProgram)?
                    .id();
                self.payloads.insert(copy, payload);
            }
            Instruction::ContinuousPowerToughness { operation } => {
                let TargetRef::Card(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                self.characteristics
                    .add_effect(entry.id(), target, operation)?;
            }
            Instruction::PutPowerToughnessCounters { counters } => {
                let TargetRef::Card(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                self.characteristics.place_counters(target, &counters)?;
            }
            Instruction::ReturnToOwnersHand => {
                self.window = self.window.record_targeted_resolution_move(
                    self.window.revision(),
                    choice.id,
                    CardDestination::Hand,
                )?;
                self.prune_creature_state();
            }
        }
        Ok(())
    }
    fn spell_target(instruction: &Instruction) -> bool {
        matches!(
            instruction,
            Instruction::CounterSpell | Instruction::CopySpell
        )
    }
    fn valid_program(program: &[Instruction]) -> bool {
        let Some(first) = program.first() else {
            return false;
        };
        program.iter().all(|instruction| {
            if Self::spell_target(first) != Self::spell_target(instruction) {
                return false;
            }
            match instruction {
                Instruction::Damage { amount } | Instruction::CreatePreventionShield { amount } => {
                    amount.is_positive()
                }
                Instruction::CreateDamageMultiplier { factor } => factor.is_positive(),
                Instruction::PutPowerToughnessCounters { counters } => counters.valid_placement(),
                _ => true,
            }
        })
    }
    fn legal_target(&self, instruction: &Instruction, target: TargetRef) -> bool {
        match (instruction, target) {
            (Instruction::CounterSpell | Instruction::CopySpell, TargetRef::Stack(id)) => {
                self.window.entries().iter().any(|entry| {
                    entry.id() == id
                        && matches!(
                            entry.kind(),
                            StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                        )
                })
            }
            (
                Instruction::Damage { .. }
                | Instruction::CreatePreventionShield { .. }
                | Instruction::CreateDamageMultiplier { .. }
                | Instruction::ReturnToOwnersHand
                | Instruction::ContinuousPowerToughness { .. }
                | Instruction::PutPowerToughnessCounters { .. },
                TargetRef::Card(card),
            ) => self.creature(card).is_some(),
            _ => false,
        }
    }
    fn prune_payloads(&mut self) {
        let live: BTreeSet<_> = self
            .window
            .entries()
            .iter()
            .map(|entry| entry.id())
            .collect();
        self.payloads.retain(|id, _| live.contains(id));
    }
    fn prune_creature_state(&mut self) {
        let current: BTreeSet<_> = self
            .window
            .cards()
            .iter()
            .filter(|card| card.zone() == CardZone::Battlefield)
            .map(|card| card.id())
            .collect();
        self.damage.retain(|object, _| current.contains(object));
        self.characteristics.retain_battlefield(&current);
        self.damage_state.retain_battlefield(&current);
    }
}

#[cfg(test)]
mod revision_tests {
    use super::*;
    #[test]
    fn mana_activation_operational_failures_and_no_stack_allocation() {
        let a = SeatId(0);
        let b = SeatId(1);
        let initial = EffectWindow::new(
            vec![a, b],
            a,
            vec![SeedCard {
                card: CardId(1),
                owner: b,
                destination: CardDestination::Battlefield { controller: a },
                program: Program::Land {
                    abilities: vec![FixedManaAbility {
                        id: ManaAbilityId(4),
                        produced: BTreeMap::from([(ManaType::Green, 3.into())]),
                    }],
                },
            }],
            vec![
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Green,
                    amount: 7.into(),
                },
                FloatingMana {
                    player: b,
                    mana_type: ManaType::Blue,
                    amount: 11.into(),
                },
            ],
        )
        .unwrap();
        let initial = initial.settle_checks(initial.revision()).unwrap();
        let source = initial.window.cards().current_identity(CardId(1)).unwrap();
        for fault in 0..4 {
            let mut state = initial.clone();
            let expected = match fault {
                0 => {
                    state.revision = u64::MAX;
                    EffectError::Window(StackWindowError::RevisionExhausted)
                }
                1 => {
                    state.window.set_test_revision(u64::MAX);
                    EffectError::Window(StackWindowError::RevisionExhausted)
                }
                2 => {
                    state.window.set_test_card_revision(u64::MAX);
                    EffectError::Window(StackWindowError::Cards(
                        crate::card_identity::CardIdentityError::RevisionExhausted,
                    ))
                }
                _ => {
                    state.window.set_test_priority_revision(u64::MAX);
                    EffectError::Window(StackWindowError::Priority(
                        crate::priority::PriorityError::RevisionExhausted,
                    ))
                }
            };
            let before = state.clone();
            assert_eq!(
                state.activate_mana(state.revision(), a, source, ManaAbilityId(4)),
                Err(expected)
            );
            assert_eq!(state, before);
            assert_eq!(state.mana(a, ManaType::Green), 7.into());
            assert_eq!(state.mana(b, ManaType::Blue), 11.into());
            assert_eq!(
                state.window.cards().get(source).unwrap().tapped(),
                Some(false)
            );
        }
        let mut state = initial;
        state.window.set_test_next_entry(u64::MAX);
        let activated = state
            .activate_mana(state.revision(), a, source, ManaAbilityId(4))
            .unwrap();
        assert_eq!(activated.state().mana(a, ManaType::Green), 10.into());
        assert_eq!(
            activated
                .state()
                .window
                .cards()
                .get(source)
                .unwrap()
                .tapped(),
            Some(true)
        );
        assert_eq!(activated.production().source, source);
        assert!(activated.state().window.entries().is_empty());
        assert_eq!(state.mana(a, ManaType::Green), 7.into());
    }
    #[test]
    fn paid_cast_operational_failures_preserve_nonzero_pools_and_source() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut state = EffectWindow::new(
            vec![a, b],
            a,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: a,
                    destination: CardDestination::Hand,
                    program: Program::Instant {
                        cost: ManaCost::one(ManaType::Red),
                        instructions: vec![Instruction::Damage { amount: 1.into() }],
                    },
                },
                SeedCard {
                    card: CardId(2),
                    owner: b,
                    destination: CardDestination::Battlefield { controller: b },
                    program: Program::VanillaCreature {
                        power: 1.into(),
                        toughness: 10.into(),
                    },
                },
            ],
            vec![
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 2.into(),
                },
                FloatingMana {
                    player: b,
                    mana_type: ManaType::Red,
                    amount: 99.into(),
                },
            ],
        )
        .unwrap();
        state = state.settle_checks(state.revision()).unwrap();
        let spell = state.window.cards().current_identity(CardId(1)).unwrap();
        let target = TargetRef::Card(state.window.cards().current_identity(CardId(2)).unwrap());
        state.revision = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.cast_with_payment(state.revision(), a, spell, target, None),
            Err(EffectError::Window(StackWindowError::RevisionExhausted))
        );
        assert_eq!(state, saved);
        state.revision = 1;
        state.window.set_test_revision(u64::MAX);
        let saved = state.clone();
        // Exact payment validates before the child window fails to allocate its successor.
        assert_eq!(
            state.cast_with_payment(state.revision(), a, spell, target, None),
            Err(EffectError::Window(StackWindowError::RevisionExhausted))
        );
        assert_eq!(state, saved);
        assert_eq!(state.mana(a, ManaType::Red), 2.into());
        assert_eq!(state.mana(b, ManaType::Red), 99.into());
        state.window.set_test_revision(1);
        state.window.set_test_next_entry(u64::MAX);
        let saved = state.clone();
        assert_eq!(
            state.cast_with_payment(state.revision(), a, spell, target, None),
            Err(EffectError::Window(
                StackWindowError::EntryIdentityExhausted
            ))
        );
        assert_eq!(state, saved);
        assert_eq!(state.mana(a, ManaType::Red), 2.into());
        assert_eq!(
            state.window.cards().get(spell).unwrap().zone(),
            CardZone::Hand(a)
        );
    }
    #[test]
    fn instruction_index_alone_cannot_redirect_a_copy_continuation() {
        use crate::game::{GameCommand, GameContext, GameId, GameInput, GameState};
        let a = SeatId(0);
        let b = SeatId(1);
        let instant = |card, owner, instructions| SeedCard {
            card: CardId(card),
            owner,
            destination: CardDestination::Hand,
            program: Program::Instant {
                cost: ManaCost::one(ManaType::Blue),
                instructions,
            },
        };
        let mut game = GameState::initialize(
            GameId(30),
            GameContext::new(
                "test".into(),
                "rules".into(),
                "cards".into(),
                "programs".into(),
                "profile".into(),
            )
            .unwrap(),
            vec![a, b],
            a,
            vec![
                instant(1, a, vec![Instruction::Damage { amount: 1.into() }]),
                instant(2, b, vec![Instruction::CopySpell, Instruction::CopySpell]),
                SeedCard {
                    card: CardId(3),
                    owner: b,
                    destination: CardDestination::Battlefield { controller: b },
                    program: Program::VanillaCreature {
                        power: 1.into(),
                        toughness: 10.into(),
                    },
                },
            ],
            vec![
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Blue,
                    amount: 1.into(),
                },
                FloatingMana {
                    player: b,
                    mana_type: ManaType::Blue,
                    amount: 1.into(),
                },
            ],
        )
        .unwrap()
        .into_state();
        let target = game
            .execution()
            .window()
            .cards()
            .current_identity(CardId(3))
            .unwrap();
        let spell = game
            .execution()
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        for (actor, command) in [
            (
                a,
                GameCommand::Cast {
                    spell,
                    target: TargetRef::Card(target),
                },
            ),
            (a, GameCommand::Pass),
        ] {
            game = game
                .advance(GameInput {
                    basis: game.basis().clone(),
                    actor,
                    command,
                })
                .unwrap()
                .into_state();
        }
        let original = game.execution().window().entries()[0].id();
        let spell = game
            .execution()
            .window()
            .cards()
            .current_identity(CardId(2))
            .unwrap();
        for (actor, command) in [
            (
                b,
                GameCommand::Cast {
                    spell,
                    target: TargetRef::Stack(original),
                },
            ),
            (b, GameCommand::Pass),
            (a, GameCommand::Pass),
        ] {
            game = game
                .advance(GameInput {
                    basis: game.basis().clone(),
                    actor,
                    command,
                })
                .unwrap()
                .into_state();
        }
        let ResolutionFrame::CopyTargets(frame) = game.pending_decision().unwrap().frame() else {
            panic!("copy choice required");
        };
        let mut wrong = frame.clone();
        wrong.instruction = 1;
        let state = game.execution();
        let saved = state.clone();
        assert_eq!(
            state.resume_resolution(
                state.revision(),
                &ResolutionFrame::CopyTargets(wrong),
                b,
                DecisionReply::CopyTargets(CopyTargetChoice::Retain)
            ),
            Err(EffectError::WrongResolutionFrame)
        );
        assert_eq!(state, &saved);
        assert_eq!(frame.instruction_index(), 0);
    }
    #[test]
    fn executor_revision_exhaustion_is_checked_without_mutating_child_or_resources() {
        let a = SeatId(0);
        let mut state = EffectWindow::new(vec![a, SeatId(1)], a, vec![], vec![]).unwrap();
        state = state.settle_checks(state.revision()).unwrap();
        state.revision = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.pass(u64::MAX, a),
            Err(EffectError::Window(StackWindowError::RevisionExhausted))
        );
        assert_eq!(state, saved);
        assert_eq!(
            state.pass(0, a),
            Err(EffectError::Window(StackWindowError::StaleRevision))
        );
        assert_eq!(state, saved);
    }
}
