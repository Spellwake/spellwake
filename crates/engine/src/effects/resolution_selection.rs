//! One resolution cursor owns all affected-player choices, owner ordering and commit.
//! These native observations are privileged, including hidden hand domains.
use super::*;
mod departure;
use crate::occurrence::{EventCause, ZoneReason};
use crate::program::{AffectedPlayers, PermanentTypeFilter};
use crate::stack_window::ResolutionSelectionMove;
#[cfg(test)]
mod tests;

/// The kind of an executor-issued selected-object decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionSelectionKind {
    /// A controlled current permanent moves to its owner's graveyard.
    Sacrifice,
    /// A physical card moves from its owner's hand to that graveyard.
    Discard,
}

/// The original operation retained by the sole work owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionSelectionOperation {
    /// Evaluated types filter a separately checked physical permanent domain.
    Sacrifice { filter: PermanentTypeFilter },
    /// The domain and commit both require physical-card origin and exact Hand.
    Discard,
}
impl ResolutionSelectionOperation {
    /// Observe the operation discriminant without supplying any work authority.
    pub fn kind(&self) -> ResolutionSelectionKind {
        match self {
            Self::Sacrifice { .. } => ResolutionSelectionKind::Sacrifice,
            Self::Discard => ResolutionSelectionKind::Discard,
        }
    }
    fn movement(
        &self,
        selected: &ResolutionSelectedObject,
        destination: crate::program::PublicZoneDestination,
    ) -> ResolutionSelectionMove {
        match self {
            Self::Sacrifice { .. } => ResolutionSelectionMove::Sacrifice {
                player: selected.player,
                object: selected.object,
                destination,
            },
            Self::Discard => ResolutionSelectionMove::Discard {
                player: selected.player,
                object: selected.object,
                destination,
            },
        }
    }
    pub(super) fn reason(&self, player: SeatId) -> ZoneReason {
        match self {
            Self::Sacrifice { .. } => ZoneReason::Sacrifice { player },
            Self::Discard => ZoneReason::Discard { player },
        }
    }
}

/// A complete frozen current-incarnation domain, in actual affected-player order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSelectionRequirement {
    /// Actual participant required to choose this row's objects.
    pub player: SeatId,
    /// Exact current identities frozen before any participant's selection.
    pub domain: Vec<ObjectId>,
    /// Nonnegative authored amount clamped to this domain before conversion.
    pub required: usize,
}
/// One accepted affected-player answer, retained before any physical movement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionAcceptedSelection {
    /// Actual affected chooser, independent of the selected cards' owners.
    pub player: SeatId,
    /// Distinct exact members of that player's frozen requirement.
    pub objects: Vec<ObjectId>,
}
/// Existing resolution-facing name for the neutral physical owner group.
pub type ResolutionGraveyardGroup = ZoneGraveyardGroup;
/// Existing resolution-facing name for a neutral owner permutation.
pub type ResolutionAcceptedGraveyardOrder = ZoneOwnerOrder;
/// A selected exact incarnation and the player performing this operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSelectedObject {
    /// Player sacrificing or discarding this object, not inferred from owner.
    pub player: SeatId,
    /// Actual before incarnation, never rebound through a later CardId lookup.
    pub object: ObjectId,
}
/// Complete internal batch. Application order differs from logical publication order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSelectionBatch {
    /// Original affected-player choices, owned here rather than by the generic reducer.
    pub selections: Vec<ResolutionAcceptedSelection>,
    /// Complete settled replacement plan, absent for historical no-replacement work.
    pub replacements: Option<Box<ZoneReplacementBatch>>,
    /// Publication order: owner APNAP, chosen physical order, then token departures.
    pub logical: Vec<ResolutionSelectedObject>,
    /// Physical top-insertion order, reversing each owner's physical entrants.
    pub application: Vec<ResolutionSelectedObject>,
    /// Complete owner permutations, including internally determined singletons.
    pub owner_orders: Vec<ResolutionAcceptedGraveyardOrder>,
    /// Current Battlefield tokens selected for sacrifice; always empty for Discard.
    pub token_departures: Vec<ObjectId>,
}
/// Mutually exclusive owned stages, never a reply-supplied mutation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionSelectionStage {
    /// Every domain is frozen; accepted rows form the exact consumed APNAP prefix.
    Choosing {
        /// All affected players, including zero-count requirements.
        requirements: Vec<ResolutionSelectionRequirement>,
        /// First unconsumed requirement; equals accepted.len().
        next_player: usize,
        /// Effective members of consumed choices, including forced empty rows.
        /// Departure provenance separately retains their original offers/answers.
        accepted: Vec<ResolutionAcceptedSelection>,
    },
    /// Complete precommit candidate reducer, retaining this owner's sole cursor.
    Replacing {
        /// Original selections retained by this resolution owner.
        selections: Vec<ResolutionAcceptedSelection>,
        /// Sole generic replacement reducer.
        batch: Box<ZoneReplacementBatch>,
    },
    /// All selections are complete before any owner ordering or physical move.
    Ordering {
        /// Complete affected-player selections retained in actual APNAP order.
        selections: Vec<ResolutionAcceptedSelection>,
        /// Settled final destinations/history; sources still belong to the before-world.
        replacements: Option<Box<ZoneReplacementBatch>>,
        /// Nonempty physical-card groups in owner APNAP order.
        groups: Vec<ResolutionGraveyardGroup>,
        /// First unordered owner group; equals orders.len().
        next_owner: usize,
        /// Accepted owner-order prefix, including internal singleton orders.
        orders: Vec<ResolutionAcceptedGraveyardOrder>,
    },
    /// Internal validated-shape batch awaiting one final revalidation and commit.
    Ready(ResolutionSelectionBatch),
}
/// A real offered choice, retained separately from later action feasibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSelectionCertificate {
    /// Exact actor/domain/cardinality that originally authorized this choice.
    pub requirement: ResolutionSelectionRequirement,
    /// Original answer order, before later action feasibility changes.
    pub objects: Vec<ObjectId>,
    /// Actual selected incarnations at choice time, not current movement snapshots.
    pub before: Vec<GameObject>,
    /// A zero requirement was determined internally without a player reply.
    pub forced: bool,
}
/// Privileged evidence of one actual physical owner's ordering choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSelectionOrderCertificate {
    /// Complete original arrival domain for this real owner reply.
    pub group: ResolutionGraveyardGroup,
    /// Actual accepted full order, distinct from later filtered inherited orders.
    pub order: ResolutionAcceptedGraveyardOrder,
}
/// Private operand witness installed only after validating the actual pre-departure owner.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectionOperands {
    cursor: ResolutionCursor,
    instruction: Instruction,
    requested: RulesInteger,
    players: Vec<SeatId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectionMaterialized {
    instruction: Instruction,
    requested: RulesInteger,
    players: Vec<SeatId>,
    certificates: Vec<ResolutionSelectionCertificate>,
    operands: Option<SelectionOperands>,
    held_orders: BTreeMap<SeatId, ResolutionAcceptedGraveyardOrder>,
    order_certificates: Vec<ResolutionSelectionOrderCertificate>,
}
impl SelectionMaterialized {
    fn validate_certificate(
        &self,
        index: usize,
        value: &ResolutionSelectionCertificate,
    ) -> Result<(), EffectError> {
        let requirement = &value.requirement;
        let required = std::cmp::min(
            &self.requested,
            &RulesInteger::from_usize(requirement.domain.len()),
        )
        .to_usize()
        .ok_or(EffectError::InconsistentSelectionWork)?;
        if self.players.get(index) != Some(&requirement.player)
            || requirement.required != required
            || value.forced != (required == 0)
            || value.objects.len() != required
            || requirement
                .domain
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != requirement.domain.len()
            || value.objects.iter().copied().collect::<BTreeSet<_>>().len() != value.objects.len()
            || !value
                .objects
                .iter()
                .all(|object| requirement.domain.contains(object))
        {
            return Err(EffectError::InconsistentSelectionWork);
        }
        if value.before.len() != value.objects.len()
            || value.before.iter().zip(&value.objects).any(|(before, id)| {
                before.id() != *id
                    || match &self.instruction {
                        Instruction::Sacrifice { .. } => {
                            before.zone() != ObjectZone::Battlefield
                                || before.controller() != Some(requirement.player)
                        }
                        Instruction::Discard { .. } => {
                            before.zone() != ObjectZone::Hand(requirement.player)
                                || before.owner() != requirement.player
                                || before.card_id().is_none()
                        }
                        _ => true,
                    }
            })
        {
            return Err(EffectError::InconsistentSelectionWork);
        }
        Ok(())
    }
    fn refresh_held_orders(
        &mut self,
        groups: &[ResolutionGraveyardGroup],
    ) -> Result<(), EffectError> {
        self.held_orders.clear();
        for certificate in &self.order_certificates {
            if certificate.group.owner != certificate.order.owner {
                return Err(EffectError::InconsistentSelectionWork);
            }
            validate_graveyard_order(&certificate.group, &certificate.order.top_to_bottom)
                .map_err(|_| EffectError::InconsistentSelectionWork)?;
        }
        for group in groups {
            if group.entrants.len() < 2 {
                continue;
            }
            if let Some(previous) = self
                .order_certificates
                .iter()
                .rev()
                .find(|value| value.group.owner == group.owner)
                && group
                    .entrants
                    .iter()
                    .all(|object| previous.group.entrants.contains(object))
            {
                let order = ResolutionAcceptedGraveyardOrder {
                    owner: group.owner,
                    top_to_bottom: previous
                        .order
                        .top_to_bottom
                        .iter()
                        .copied()
                        .filter(|object| group.entrants.contains(object))
                        .collect(),
                };
                validate_graveyard_order(group, &order.top_to_bottom)?;
                self.held_orders.insert(group.owner, order);
            }
        }
        Ok(())
    }
    fn record(
        &mut self,
        requirement: &ResolutionSelectionRequirement,
        objects: &[ObjectId],
        forced: bool,
        cards: &GameObjects,
    ) -> Result<(), EffectError> {
        let value = ResolutionSelectionCertificate {
            requirement: requirement.clone(),
            objects: objects.to_vec(),
            before: objects
                .iter()
                .map(|id| {
                    cards
                        .get(*id)
                        .cloned()
                        .ok_or(EffectError::InconsistentSelectionWork)
                })
                .collect::<Result<_, _>>()?,
            forced,
        };
        self.validate_certificate(self.certificates.len(), &value)?;
        self.certificates.push(value);
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolutionSelectionWork {
    pub(super) cursor: ResolutionCursor,
    operation: ResolutionSelectionOperation,
    stage: ResolutionSelectionStage,
    materialized: SelectionMaterialized,
}
/// Borrowed complete state of the one owner. This is not a public seat-safe view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionSelectionState<'a> {
    /// Complete borrowed cursor, including body-local values.
    pub cursor: ResolutionState<'a>,
    /// The sole owner's original operation and exact filter.
    pub operation: &'a ResolutionSelectionOperation,
    /// Complete current stage and accepted prefixes; hidden data is privileged.
    pub stage: &'a ResolutionSelectionStage,
    /// Original choices and frozen operands for a trusted departure epoch only.
    /// This borrowed evidence supplies no client authority or prefix reconstruction.
    pub departure: Option<ResolutionSelectionDepartureState<'a>>,
}
/// Borrowed provenance kept apart from current effective action members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionSelectionDepartureState<'a> {
    /// Original nonnegative amount before domain clamping.
    pub requested: &'a RulesInteger,
    /// Original affected-player order, including departed participants.
    pub players: &'a [SeatId],
    /// Real original answers and explicit internally forced zero rows.
    pub choices: &'a [ResolutionSelectionCertificate],
    /// Actual owner replies, retaining superseded domains as historical evidence.
    pub owner_choices: &'a [ResolutionSelectionOrderCertificate],
    /// Certified prior orders still usable when their new group is reached.
    pub held_orders: &'a BTreeMap<SeatId, ResolutionAcceptedGraveyardOrder>,
}

/// An exact immutable positive selection request issued by the executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSelectionFrame {
    pub(crate) revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    operation: ResolutionSelectionKind,
    domain: Vec<ObjectId>,
    required: usize,
}
impl ResolutionSelectionFrame {
    /// Exact executor revision at which this immutable frame is issued.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Actual resolving entry whose captured body owns this work.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Absolute authored instruction index retained by the sole cursor.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// Affected participant authorized to submit the selection.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Operation kind; replies cannot change it or its retained filter.
    pub fn operation(&self) -> ResolutionSelectionKind {
        self.operation
    }
    /// Exact frozen current-incarnation domain, potentially private hand data.
    pub fn domain(&self) -> &[ObjectId] {
        &self.domain
    }
    /// Required distinct member count, clamped by the executor to domain size.
    pub fn required(&self) -> usize {
        self.required
    }
}
/// An exact immutable owner permutation request, distinct from a check decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionGraveyardOrderFrame {
    pub(crate) revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    domain: Vec<ObjectId>,
}
impl ResolutionGraveyardOrderFrame {
    pub(super) fn for_group(
        revision: u64,
        entry: StackEntryId,
        instruction: usize,
        group: &ResolutionGraveyardGroup,
    ) -> Self {
        Self {
            revision,
            entry,
            instruction,
            chooser: group.owner,
            domain: group.entrants.clone(),
        }
    }
    /// Exact executor revision required when resuming this owner choice.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Actual resolving entry, not a checks or cleanup authority.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Absolute selected-object instruction retained by the cursor.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// Physical owner authorized to order all of these entrants.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Complete exact physical group, possibly selected by several controllers.
    pub fn domain(&self) -> &[ObjectId] {
        &self.domain
    }
}
impl ResolutionSelectionWork {
    fn frame(&self, revision: u64) -> Option<EffectDecision> {
        match &self.stage {
            ResolutionSelectionStage::Choosing {
                requirements,
                next_player,
                ..
            } => {
                let requirement = requirements.get(*next_player)?;
                (requirement.required > 0).then(|| {
                    EffectDecision::ResolutionSelect(ResolutionSelectionFrame {
                        revision,
                        entry: self.cursor.entry,
                        instruction: self.cursor.instruction,
                        chooser: requirement.player,
                        operation: self.operation.kind(),
                        domain: requirement.domain.clone(),
                        required: requirement.required,
                    })
                })
            }
            ResolutionSelectionStage::Ordering {
                groups, next_owner, ..
            } => {
                let group = groups.get(*next_owner)?;
                (group.entrants.len() > 1).then(|| {
                    EffectDecision::ResolutionGraveyardOrder(ResolutionGraveyardOrderFrame {
                        revision,
                        entry: self.cursor.entry,
                        instruction: self.cursor.instruction,
                        chooser: group.owner,
                        domain: group.entrants.clone(),
                    })
                })
            }
            ResolutionSelectionStage::Replacing { batch: work, .. } => {
                work.frame(revision).map(EffectDecision::ZoneReplacement)
            }
            ResolutionSelectionStage::Ready(_) => None,
        }
    }
}

impl EffectWindow {
    /// Inspect every live requirement, accepted prefix and order without cloning a game.
    /// Hidden domains remain privileged; this accessor supplies no resume authority.
    pub fn resolution_selection_state(&self) -> Option<ResolutionSelectionState<'_>> {
        match self.pending_work.as_ref() {
            Some(PendingWork::ResolutionSelection(work)) => Some(ResolutionSelectionState {
                cursor: (&work.cursor).into(),
                operation: &work.operation,
                stage: &work.stage,
                departure: work.materialized.operands.is_some().then_some(
                    ResolutionSelectionDepartureState {
                        requested: &work.materialized.requested,
                        players: &work.materialized.players,
                        choices: &work.materialized.certificates,
                        owner_choices: &work.materialized.order_certificates,
                        held_orders: &work.materialized.held_orders,
                    },
                ),
            }),
            None
            | Some(
                PendingWork::LibraryTop(_)
                | PendingWork::Resolution(_)
                | PendingWork::OptionalEffect(_)
                | PendingWork::ResolutionPayment(_)
                | PendingWork::ManaColor(_)
                | PendingWork::TokenEntry(_)
                | PendingWork::Damage(_)
                | PendingWork::Checks(_)
                | PendingWork::Combat(_),
            ) => None,
        }
    }
    /// Derive only the positive current request from the sole owned stage.
    pub(super) fn selection_frame(&self) -> Option<EffectDecision> {
        match self.pending_work.as_ref() {
            Some(PendingWork::ResolutionSelection(work)) => work.frame(self.revision),
            None
            | Some(
                PendingWork::LibraryTop(_)
                | PendingWork::Resolution(_)
                | PendingWork::OptionalEffect(_)
                | PendingWork::ResolutionPayment(_)
                | PendingWork::ManaColor(_)
                | PendingWork::TokenEntry(_)
                | PendingWork::Damage(_)
                | PendingWork::Checks(_)
                | PendingWork::Combat(_),
            ) => None,
        }
    }
    /// Freeze every affected domain before transferring the existing cursor.
    ///
    /// # Errors
    /// Rejects missing resolving ownership, an invalid captured player/amount,
    /// inconsistent bounded conversion or a failed characteristic evaluation.
    pub(super) fn begin_resolution_selection(
        &mut self,
        instruction: &Instruction,
    ) -> Result<bool, EffectError> {
        let (players, amount, operation) = match instruction {
            Instruction::Sacrifice {
                players,
                amount,
                filter,
            } => (
                *players,
                amount,
                ResolutionSelectionOperation::Sacrifice {
                    filter: filter.clone(),
                },
            ),
            Instruction::Discard { players, amount } => {
                (*players, amount, ResolutionSelectionOperation::Discard)
            }
            Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OfferManaPayment { .. }
            | Instruction::CreateTokens { .. }
            | Instruction::AdjustControllerLife { .. }
            | Instruction::AdjustTargetLife { .. }
            | Instruction::ChooseCreatureType { .. }
            | Instruction::Damage { .. }
            | Instruction::CreatePreventionShield { .. }
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::ReturnToOwnersHand
            | Instruction::CounterSpell
            | Instruction::CopySpell
            | Instruction::CopyTargetToSource { .. }
            | Instruction::CreateContinuousEffect { .. }
            | Instruction::PutPowerToughnessCounters { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::DrawController { .. } => return Ok(false),
        };
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let amount = self.resolve_instruction_amount(entry.controller(), amount)?;
        if amount < 0.into() {
            return Err(EffectError::InconsistentSelectionWork);
        }
        let players = match players {
            AffectedPlayers::One(role) => {
                let player = self.resolving_player_identity(role)?;
                if !self.window.participants().contains(&player) {
                    self.advance_instruction()?;
                    return Ok(true);
                }
                vec![player]
            }
            AffectedPlayers::EachPlayer => self.window.apnap_players()?.collect(),
        };
        let evaluated = self.evaluated_characteristics()?;
        let mut requirements = Vec::with_capacity(players.len());
        for &player in &players {
            requirements
                .push(self.selection_requirement_at(&operation, &amount, player, &evaluated)?);
        }
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::ResolutionSelection(Box::new(
            ResolutionSelectionWork {
                cursor,
                operation,
                stage: ResolutionSelectionStage::Choosing {
                    requirements,
                    next_player: 0,
                    accepted: vec![],
                },
                materialized: SelectionMaterialized {
                    instruction: instruction.clone(),
                    requested: amount,
                    players,
                    certificates: vec![],
                    operands: None,
                    held_orders: BTreeMap::new(),
                    order_certificates: vec![],
                },
            },
        )));
        Ok(true)
    }
    /// Advance only this instruction's owned stages. No player choice changes cards.
    ///
    /// # Errors
    /// Rejects an absent/inconsistent owner or stage and propagates final batch,
    /// physical movement, publisher and cursor failures. Call only on an owned
    /// tentative successor; no failed prefix is an adoptable game result.
    pub(super) fn progress_resolution_selection(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::ResolutionSelection(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongSelectionFrame);
        };
        let ResolutionSelectionWork {
            cursor,
            operation,
            mut stage,
            mut materialized,
        } = *work;
        loop {
            stage = match stage {
                ResolutionSelectionStage::Choosing {
                    requirements,
                    mut next_player,
                    mut accepted,
                } => {
                    if accepted.len() != next_player || next_player > requirements.len() {
                        return Err(EffectError::InconsistentSelectionWork);
                    }
                    while let Some(requirement) = requirements.get(next_player) {
                        if requirement.required > 0 {
                            break;
                        }
                        materialized.record(requirement, &[], true, self.window.cards())?;
                        accepted.push(ResolutionAcceptedSelection {
                            player: requirement.player,
                            objects: vec![],
                        });
                        next_player += 1;
                    }
                    if next_player < requirements.len() {
                        let work = ResolutionSelectionWork {
                            cursor,
                            operation,
                            materialized,
                            stage: ResolutionSelectionStage::Choosing {
                                requirements,
                                next_player,
                                accepted,
                            },
                        };
                        let frame = work
                            .frame(self.revision)
                            .ok_or(EffectError::InconsistentSelectionWork)?;
                        self.pending_work = Some(PendingWork::ResolutionSelection(Box::new(work)));
                        return Ok(Some(frame));
                    }
                    let replacements = self.selection_replacements(
                        &cursor,
                        &operation,
                        &materialized,
                        accepted.clone(),
                    )?;
                    if !replacements.sources().is_empty() && !replacements.candidates().is_empty() {
                        ResolutionSelectionStage::Replacing {
                            selections: accepted,
                            batch: Box::new(replacements),
                        }
                    } else {
                        let groups = self.selection_owner_groups(&accepted)?;
                        materialized.refresh_held_orders(&groups)?;
                        ResolutionSelectionStage::Ordering {
                            replacements: None,
                            selections: accepted,
                            groups,
                            next_owner: 0,
                            orders: vec![],
                        }
                    }
                }
                ResolutionSelectionStage::Replacing {
                    selections,
                    batch: mut replacements,
                } => {
                    if let Some(frame) = replacements.progress(self.revision)? {
                        self.pending_work = Some(PendingWork::ResolutionSelection(Box::new(
                            ResolutionSelectionWork {
                                cursor,
                                operation,
                                materialized,
                                stage: ResolutionSelectionStage::Replacing {
                                    selections,
                                    batch: replacements,
                                },
                            },
                        )));
                        return Ok(Some(frame));
                    }
                    let groups = self.selection_arrival_groups(&selections, Some(&replacements))?;
                    materialized.refresh_held_orders(&groups)?;
                    ResolutionSelectionStage::Ordering {
                        selections,
                        replacements: Some(replacements),
                        groups,
                        next_owner: 0,
                        orders: vec![],
                    }
                }
                ResolutionSelectionStage::Ordering {
                    replacements,
                    selections,
                    groups,
                    mut next_owner,
                    mut orders,
                } => {
                    if orders.len() != next_owner || next_owner > groups.len() {
                        return Err(EffectError::InconsistentSelectionWork);
                    }
                    while let Some(group) = next_graveyard_group(&groups, &mut orders)? {
                        let Some(held) = materialized.held_orders.remove(&group.owner) else {
                            break;
                        };
                        validate_graveyard_order(&group, &held.top_to_bottom)?;
                        orders.push(held);
                    }
                    next_owner = orders.len();
                    if next_owner < groups.len() {
                        let work = ResolutionSelectionWork {
                            cursor,
                            operation,
                            materialized,
                            stage: ResolutionSelectionStage::Ordering {
                                replacements,
                                selections,
                                groups,
                                next_owner,
                                orders,
                            },
                        };
                        let frame = work
                            .frame(self.revision)
                            .ok_or(EffectError::InconsistentSelectionWork)?;
                        self.pending_work = Some(PendingWork::ResolutionSelection(Box::new(work)));
                        return Ok(Some(frame));
                    }
                    ResolutionSelectionStage::Ready(self.selection_batch_replaced(
                        &selections,
                        orders,
                        replacements,
                    )?)
                }
                ResolutionSelectionStage::Ready(batch) => {
                    self.commit_resolution_selection(
                        &cursor,
                        &operation,
                        &materialized,
                        &batch,
                        append,
                    )?;
                    #[cfg(test)]
                    let observed = (cursor.entry, cursor.instruction);
                    self.pending_work = Some(PendingWork::Resolution(cursor));
                    self.advance_instruction()?;
                    #[cfg(test)]
                    super::characteristic_observations::instruction(
                        self,
                        "Completed",
                        observed.0,
                        observed.1,
                        None,
                    );
                    return Ok(None);
                }
            };
        }
    }
    /// Accept one exact executor frame on a tentative clone of this accepted state.
    ///
    /// # Errors
    /// Rejects stale/exhausted revisions, wrong frames/actors/reply kinds,
    /// incorrect cardinality, duplicate/outside members and incomplete owner
    /// permutations. Propagates later stage/commit failures without mutating self.
    pub(super) fn resume_resolution_selection(
        &self,
        expected: u64,
        frame: &EffectDecision,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if let Some(PendingWork::ResolutionSelection(work)) = &self.pending_work {
            if work.materialized.operands.is_some() {
                self.validate_selection_work(work)?;
            } else {
                self.validate_selection_header(work)?;
            }
        }
        if self.selection_frame().as_ref() != Some(frame) {
            return Err(EffectError::WrongSelectionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let mut next = self.clone();
        let Some(PendingWork::ResolutionSelection(work)) = next.pending_work.as_mut() else {
            return Err(EffectError::WrongSelectionFrame);
        };
        match (&mut work.stage, reply) {
            (
                ResolutionSelectionStage::Replacing {
                    batch: replacements,
                    ..
                },
                DecisionReply::ZoneReplacement(reply),
            ) => {
                let EffectDecision::ZoneReplacement(bound) = frame else {
                    return Err(ZoneReplacementError::WrongFrame.into());
                };
                replacements.accept(self.revision, bound, actor, reply)?;
            }
            (
                ResolutionSelectionStage::Choosing {
                    requirements,
                    next_player,
                    accepted,
                },
                DecisionReply::SelectResolution(objects),
            ) => {
                let requirement = requirements
                    .get(*next_player)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                if accepted.len() != *next_player {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                if objects.len() != requirement.required {
                    return Err(EffectError::InvalidSelectionCount);
                }
                let unique: BTreeSet<_> = objects.iter().copied().collect();
                if unique.len() != objects.len() {
                    return Err(EffectError::DuplicateSelection);
                }
                if !objects
                    .iter()
                    .all(|object| requirement.domain.contains(object))
                {
                    return Err(EffectError::SelectionOutsideDomain);
                }
                work.materialized
                    .record(requirement, &objects, false, self.window.cards())?;
                accepted.push(ResolutionAcceptedSelection {
                    player: actor,
                    objects,
                });
                *next_player = next_player
                    .checked_add(1)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
            }
            (
                ResolutionSelectionStage::Ordering {
                    groups,
                    next_owner,
                    orders,
                    ..
                },
                DecisionReply::OrderResolutionGraveyard(top_to_bottom),
            ) => {
                let group = groups
                    .get(*next_owner)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                if orders.len() != *next_owner {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                validate_graveyard_order(group, &top_to_bottom)?;
                let order = ResolutionAcceptedGraveyardOrder {
                    owner: actor,
                    top_to_bottom,
                };
                work.materialized
                    .order_certificates
                    .push(ResolutionSelectionOrderCertificate {
                        group: group.clone(),
                        order: order.clone(),
                    });
                orders.push(order);
                *next_owner = next_owner
                    .checked_add(1)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
            }
            (_, _) => return Err(EffectError::WrongDecisionKind),
        }
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
    pub(super) fn selection_owner_groups(
        &self,
        selections: &[ResolutionAcceptedSelection],
    ) -> Result<Vec<ResolutionGraveyardGroup>, EffectError> {
        self.zone_arrival_groups(
            &selections
                .iter()
                .flat_map(|s| s.objects.iter().copied())
                .collect::<Vec<_>>(),
            None,
        )
    }
    pub(super) fn selection_arrival_groups(
        &self,
        selections: &[ResolutionAcceptedSelection],
        replacements: Option<&ZoneReplacementBatch>,
    ) -> Result<Vec<ResolutionGraveyardGroup>, EffectError> {
        self.zone_arrival_groups(
            &selections
                .iter()
                .flat_map(|s| s.objects.iter().copied())
                .collect::<Vec<_>>(),
            replacements,
        )
    }
    pub(super) fn selection_batch_replaced(
        &self,
        selections: &[ResolutionAcceptedSelection],
        orders: Vec<ResolutionAcceptedGraveyardOrder>,
        replacements: Option<Box<ZoneReplacementBatch>>,
    ) -> Result<ResolutionSelectionBatch, EffectError> {
        let mut players = BTreeMap::new();
        let mut objects = vec![];
        for selection in selections {
            for &id in &selection.objects {
                if players.insert(id, selection.player).is_some() {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                objects.push(id);
            }
        }
        let plan = self.zone_move_plan(&objects, &orders, replacements.as_deref())?;
        let rows = |ids: &[ObjectId]| {
            ids.iter()
                .map(|id| {
                    Ok(ResolutionSelectedObject {
                        object: *id,
                        player: *players
                            .get(id)
                            .ok_or(EffectError::InconsistentSelectionWork)?,
                    })
                })
                .collect::<Result<Vec<_>, EffectError>>()
        };
        Ok(ResolutionSelectionBatch {
            selections: selections.to_vec(),
            logical: rows(&plan.logical)?,
            application: rows(
                &plan
                    .application
                    .iter()
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>(),
            )?,
            owner_orders: orders,
            token_departures: plan.token_departures,
            replacements,
        })
    }
    fn commit_resolution_selection(
        &mut self,
        cursor: &ResolutionCursor,
        operation: &ResolutionSelectionOperation,
        materialized: &SelectionMaterialized,
        batch: &ResolutionSelectionBatch,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if entry.id() != cursor.entry {
            return Err(EffectError::WrongResolutionFrame);
        }
        let actual = self
            .payloads
            .get(&cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|body| body.get(cursor.instruction))
            .ok_or(EffectError::InconsistentSelectionWork)?;
        let affected = match (operation, actual) {
            (
                ResolutionSelectionOperation::Sacrifice { filter },
                Instruction::Sacrifice {
                    players,
                    filter: actual,
                    ..
                },
            ) if filter == actual => *players,
            (ResolutionSelectionOperation::Discard, Instruction::Discard { players, .. }) => {
                *players
            }
            _ => return Err(EffectError::InconsistentSelectionWork),
        };
        if materialized.instruction != *actual
            || materialized.requested < 0.into()
            || materialized.certificates.len() != materialized.players.len()
        {
            return Err(EffectError::InconsistentSelectionWork);
        }
        for (index, value) in materialized.certificates.iter().enumerate() {
            materialized.validate_certificate(index, value)?;
        }
        let players: BTreeSet<_> = materialized.players.iter().copied().collect();
        if materialized.operands.is_none() {
            let current: BTreeSet<_> = match affected {
                AffectedPlayers::One(role) => [self.resolving_player(role)?].into_iter().collect(),
                AffectedPlayers::EachPlayer => self.window.apnap_players()?.collect(),
            };
            if players != current {
                return Err(EffectError::InconsistentSelectionWork);
            }
        }
        if batch.selections != self.selection_effective(operation, materialized) {
            return Err(EffectError::InconsistentSelectionWork);
        }
        let evaluated = self.evaluated_characteristics()?;
        let mut unique = BTreeSet::new();
        for selected in &batch.logical {
            let object = self
                .window
                .cards()
                .get(selected.object)
                .ok_or(EffectError::InconsistentSelectionWork)?;
            if !unique.insert(selected.object)
                || !players.contains(&selected.player)
                || if materialized.operands.is_some() {
                    !self.window.participants().contains(&selected.player)
                        || !selection_action_possible(operation, selected.player, object)
                } else {
                    !selection_eligible(operation, selected.player, object, &evaluated)
                }
            {
                return Err(EffectError::InconsistentSelectionWork);
            }
        }
        let application: BTreeMap<_, _> = batch
            .application
            .iter()
            .map(|row| (row.object, row.player))
            .collect();
        let logical: BTreeMap<_, _> = batch
            .logical
            .iter()
            .map(|row| (row.object, row.player))
            .collect();
        if application != logical || application.len() != batch.application.len() {
            return Err(EffectError::InconsistentSelectionWork);
        }
        let mut selections = BTreeMap::<SeatId, Vec<ObjectId>>::new();
        for row in &batch.logical {
            selections.entry(row.player).or_default().push(row.object);
        }
        let selections: Vec<_> = selections
            .into_iter()
            .map(|(player, objects)| ResolutionAcceptedSelection { player, objects })
            .collect();
        if let Some(replacements) = &batch.replacements {
            replacements.verify_initial(self.selection_replacements(
                cursor,
                operation,
                materialized,
                batch.selections.clone(),
            )?)?;
        }
        let expected_groups =
            self.selection_arrival_groups(&selections, batch.replacements.as_deref())?;
        if expected_groups.len() != batch.owner_orders.len() {
            return Err(EffectError::InconsistentSelectionWork);
        }
        for (group, order) in expected_groups.iter().zip(&batch.owner_orders) {
            if group.owner != order.owner
                || group.entrants.len() != order.top_to_bottom.len()
                || group.entrants.iter().copied().collect::<BTreeSet<_>>()
                    != order.top_to_bottom.iter().copied().collect::<BTreeSet<_>>()
            {
                return Err(EffectError::InconsistentSelectionWork);
            }
        }
        let rebuilt = self.selection_batch_replaced(
            &batch.selections,
            batch.owner_orders.clone(),
            batch.replacements.clone(),
        )?;
        if rebuilt != *batch
            || matches!(operation, ResolutionSelectionOperation::Discard)
                && !batch.token_departures.is_empty()
        {
            return Err(EffectError::InconsistentSelectionWork);
        }
        if batch.logical.is_empty() {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let (window, movements) = self.window.record_resolution_selection_moves(
            self.window.revision(),
            cursor.entry,
            batch
                .application
                .iter()
                .map(|row| {
                    Ok(operation.movement(
                        row,
                        batch.replacements.as_ref().map_or(
                            Ok(crate::program::PublicZoneDestination::Graveyard),
                            |work| work.destination(row.object),
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, EffectError>>()?,
        )?;
        let mut movements: BTreeMap<_, _> = movements
            .into_iter()
            .map(|movement| (movement.before.id(), movement))
            .collect();
        let mut ordered = Vec::with_capacity(batch.logical.len());
        for row in &batch.logical {
            ordered.push((
                movements
                    .remove(&row.object)
                    .ok_or(EffectError::InconsistentSelectionWork)?,
                operation.reason(row.player),
            ));
        }
        if !movements.is_empty() {
            return Err(EffectError::InconsistentSelectionWork);
        }
        self.window = window;
        self.emit_from(
            EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            &before,
            None,
            append,
            |previous, current, controllers| {
                ordered
                    .iter()
                    .map(|(movement, reason)| {
                        zone_occurrence_at(previous, current, movement, *reason, controllers)
                    })
                    .collect()
            },
        )
    }
}

fn selection_action_possible(
    operation: &ResolutionSelectionOperation,
    player: SeatId,
    object: &GameObject,
) -> bool {
    match operation {
        ResolutionSelectionOperation::Sacrifice { .. } => {
            object.zone() == ObjectZone::Battlefield && object.controller() == Some(player)
        }
        ResolutionSelectionOperation::Discard => {
            object.card_id().is_some()
                && object.owner() == player
                && object.zone() == ObjectZone::Hand(player)
        }
    }
}
fn selection_eligible(
    operation: &ResolutionSelectionOperation,
    player: SeatId,
    object: &GameObject,
    evaluated: &EvaluatedCharacteristics,
) -> bool {
    match operation {
        ResolutionSelectionOperation::Sacrifice { filter } => {
            object.zone() == ObjectZone::Battlefield
                && object.controller() == Some(player)
                && evaluated
                    .get(object.id())
                    .is_some_and(|value| filter.matches_types(&value.current_types))
        }
        ResolutionSelectionOperation::Discard => {
            object.card_id().is_some() && object.zone() == ObjectZone::Hand(player)
        }
    }
}

/// One owner of singleton elision and the next physical graveyard permutation.
pub(super) fn next_graveyard_group(
    groups: &[ResolutionGraveyardGroup],
    orders: &mut Vec<ResolutionAcceptedGraveyardOrder>,
) -> Result<Option<ResolutionGraveyardGroup>, EffectError> {
    if orders.len() > groups.len() {
        return Err(EffectError::InconsistentSelectionWork);
    }
    while let Some(group) = groups.get(orders.len()) {
        if group.entrants.len() > 1 {
            return Ok(Some(group.clone()));
        }
        orders.push(ResolutionAcceptedGraveyardOrder {
            owner: group.owner,
            top_to_bottom: group.entrants.clone(),
        });
    }
    Ok(None)
}
pub(super) fn validate_graveyard_order(
    group: &ResolutionGraveyardGroup,
    order: &[ObjectId],
) -> Result<(), EffectError> {
    let actual: BTreeSet<_> = order.iter().copied().collect();
    let expected: BTreeSet<_> = group.entrants.iter().copied().collect();
    if order.len() != group.entrants.len() || actual.len() != order.len() || actual != expected {
        return Err(EffectError::InvalidGraveyardPermutation);
    }
    Ok(())
}
