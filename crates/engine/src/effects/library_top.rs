//! One private library action owns the existing resolution cursor until commit.
use super::zone_replacement::AuthorizedZoneBatch;
use super::*;
use crate::{
    occurrence::{EventCause, RulesOccurrence, ZoneReason},
    program::{PublicZoneDestination, ZoneActionKind},
};

#[cfg(test)]
mod tests;

/// Completed library action, distinct from physical zone movements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryActionKind {
    Scry,
    Surveil,
    Mill,
}
/// Actual one-to-one public-zone receipt; native IDs are not public transport handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPublicMove {
    /// Exact original library incarnation.
    pub before: ObjectId,
    /// Actual new incarnation from the atomic movement receipt.
    pub after: ObjectId,
    /// Actual public destination, never an intermediate candidate destination.
    pub destination: PublicZoneDestination,
}
/// Privileged action evidence. Selection/order vectors name before incarnations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryAction {
    /// Completed operation, not an inferred draw or discard.
    pub kind: LibraryActionKind,
    /// Actual recipient, independent of the resolving controller.
    pub player: SeatId,
    /// Original evaluated amount, before clamping to the actual library.
    pub requested: RulesInteger,
    /// Frozen original top segment; empty for a positive action on an empty library.
    pub looked_top_first: Vec<ObjectId>,
    /// Surviving looked cards in their chosen top-to-bottom order.
    pub top_first: Vec<ObjectId>,
    /// Scry bottom segment in final top-to-bottom order.
    pub bottom_top_first: Vec<ObjectId>,
    /// Original selected departures in frozen library top order, before replacements.
    pub selected_before_top_first: Vec<ObjectId>,
    /// Receipt-derived public movements in that same original selected order.
    pub public_zone_moves: Vec<LibraryPublicMove>,
    /// Actual owner-chosen Graveyard arrivals only, naming before incarnations.
    pub graveyard_before_top_first: Vec<ObjectId>,
}
/// Only operations that require a player partition have a library-choice frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryChoiceKind {
    Scry,
    Surveil,
}
/// Private exact partition/order request, never a production seat-safe view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryTopFrame {
    pub(crate) revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    kind: LibraryChoiceKind,
    domain_top_first: Vec<ObjectId>,
}
impl LibraryTopFrame {
    /// Executor revision that authenticates this frame.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Actual resolving entry owning the action.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Absolute instruction index in the retained body.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// Recipient who owns this private library choice.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Required reply family.
    pub fn kind(&self) -> LibraryChoiceKind {
        self.kind
    }
    /// Exact original top segment, not a public card view.
    pub fn domain_top_first(&self) -> &[ObjectId] {
        &self.domain_top_first
    }
}
/// Both Scry vectors are top-to-bottom; Surveil's graveyard vector selects only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryTopReply {
    Scry {
        top_first: Vec<ObjectId>,
        bottom_top_first: Vec<ObjectId>,
    },
    Surveil {
        top_first: Vec<ObjectId>,
        to_graveyard: Vec<ObjectId>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum LibraryStage {
    Choosing,
    Preparing {
        top_first: Vec<ObjectId>,
        bottom_top_first: Vec<ObjectId>,
        selected: Vec<ObjectId>,
    },
    Replacing {
        top_first: Vec<ObjectId>,
        bottom_top_first: Vec<ObjectId>,
        selected: Vec<ObjectId>,
        batch: Box<ZoneReplacementBatch>,
    },
    Ordering {
        top_first: Vec<ObjectId>,
        bottom_top_first: Vec<ObjectId>,
        selected: Vec<ObjectId>,
        replacements: Option<Box<ZoneReplacementBatch>>,
        groups: Vec<ResolutionGraveyardGroup>,
        orders: Vec<ResolutionAcceptedGraveyardOrder>,
    },
}
/// Materialized operands sealed only by a validated trusted departure cut.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LibraryOperands {
    cursor: ResolutionCursor,
    instruction: Instruction,
    player: SeatId,
    requested: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LibraryTopWork {
    pub(super) cursor: ResolutionCursor,
    kind: LibraryActionKind,
    player: SeatId,
    requested: RulesInteger,
    domain: Vec<ObjectId>,
    library_before: Vec<ObjectId>,
    window_revision: u64,
    stage: LibraryStage,
    operands: Option<LibraryOperands>,
}
impl LibraryTopWork {
    fn frame(&self, revision: u64) -> Option<EffectDecision> {
        match &self.stage {
            LibraryStage::Choosing => {
                let kind = match self.kind {
                    LibraryActionKind::Scry => LibraryChoiceKind::Scry,
                    LibraryActionKind::Surveil => LibraryChoiceKind::Surveil,
                    LibraryActionKind::Mill => return None,
                };
                Some(EffectDecision::LibraryTop(LibraryTopFrame {
                    revision,
                    entry: self.cursor.entry,
                    instruction: self.cursor.instruction,
                    chooser: self.player,
                    kind,
                    domain_top_first: self.domain.clone(),
                }))
            }
            LibraryStage::Preparing { .. } => None,
            LibraryStage::Replacing { batch, .. } => {
                batch.frame(revision).map(EffectDecision::ZoneReplacement)
            }
            LibraryStage::Ordering { groups, orders, .. } => {
                groups.get(orders.len()).map(|group| {
                    EffectDecision::ResolutionGraveyardOrder(
                        ResolutionGraveyardOrderFrame::for_group(
                            revision,
                            self.cursor.entry,
                            self.cursor.instruction,
                            group,
                        ),
                    )
                })
            }
        }
    }
}
impl EffectWindow {
    /// Actual pending library operation, without revealing its private cards.
    /// Describers must still authenticate the current chooser and disclosure.
    pub fn pending_library_kind(&self) -> Option<LibraryActionKind> {
        match &self.pending_work {
            Some(PendingWork::LibraryTop(work)) => Some(work.kind),
            _ => None,
        }
    }
    pub(super) fn library_top_frame(&self) -> Option<EffectDecision> {
        match self.pending_work.as_ref() {
            Some(PendingWork::LibraryTop(work)) => work.frame(self.revision),
            _ => None,
        }
    }
    /// Preserve only authenticated, materialized library work across the final cut.
    pub(super) fn refresh_library_after_departure(
        &mut self,
        before: &Self,
    ) -> Result<(), EffectError> {
        let Some(PendingWork::LibraryTop(held)) = &before.pending_work else {
            return Err(EffectError::InconsistentLibraryTopWork);
        };
        before.validate_library_authority(held)?;
        let mut work = held.as_ref().clone();
        if self.window.resolving().map(|entry| entry.id()) != Some(work.cursor.entry) {
            if self
                .window
                .entries()
                .iter()
                .any(|entry| entry.id() == work.cursor.entry)
            {
                return Err(EffectError::InconsistentLibraryTopWork);
            }
            self.pending_work = None;
            return Ok(());
        }
        if !self.window.participants().contains(&work.player) {
            self.pending_work = Some(PendingWork::Resolution(work.cursor));
            self.advance_instruction()?;
            return Ok(());
        }
        if work.operands.is_none() {
            let instruction = before
                .payloads
                .get(&work.cursor.entry)
                .and_then(StackPayload::instructions)
                .and_then(|body| body.get(work.cursor.instruction))
                .ok_or(EffectError::InconsistentLibraryTopWork)?
                .clone();
            work.operands = Some(LibraryOperands {
                cursor: work.cursor.clone(),
                instruction,
                player: work.player,
                requested: work.requested.clone(),
            });
        }
        work.window_revision = self.window.revision();
        self.validate_library_authority(&work)?;
        let stage = std::mem::replace(&mut work.stage, LibraryStage::Choosing);
        work.stage = match stage {
            LibraryStage::Choosing => LibraryStage::Choosing,
            LibraryStage::Preparing { .. } => return Err(EffectError::InconsistentLibraryTopWork),
            LibraryStage::Replacing {
                top_first,
                bottom_top_first,
                selected,
                batch,
            } => {
                before.validate_library_partition(
                    held,
                    &top_first,
                    &bottom_top_first,
                    &selected,
                )?;
                let old = before
                    .begin_library_replacements_at(held, &selected, true)?
                    .ok_or(EffectError::InconsistentLibraryTopWork)?;
                let fresh = self
                    .begin_library_replacements_at(&work, &selected, true)?
                    .ok_or(EffectError::InconsistentLibraryTopWork)?;
                LibraryStage::Replacing {
                    top_first,
                    bottom_top_first,
                    selected,
                    batch: Box::new(batch.rebase_departure(*old, *fresh)?),
                }
            }
            LibraryStage::Ordering {
                top_first,
                bottom_top_first,
                selected,
                replacements,
                groups,
                orders,
            } => {
                before.validate_library_partition(
                    held,
                    &top_first,
                    &bottom_top_first,
                    &selected,
                )?;
                let actual_groups = before.selection_arrival_groups(
                    &[ResolutionAcceptedSelection {
                        player: work.player,
                        objects: selected.clone(),
                    }],
                    replacements.as_deref(),
                )?;
                // A published library ordering has just its sole owner group,
                // and no accepted nonsingleton order before it.
                if !orders.is_empty()
                    || groups != actual_groups
                    || groups.len() != 1
                    || groups[0].owner != work.player
                    || groups[0].entrants.len() < 2
                {
                    return Err(EffectError::InconsistentLibraryTopWork);
                }
                let fresh = self
                    .begin_library_replacements_at(&work, &selected, true)?
                    .ok_or(EffectError::InconsistentLibraryTopWork)?;
                let batch = if let Some(retained) = replacements {
                    let old = before
                        .begin_library_replacements_at(held, &selected, true)?
                        .ok_or(EffectError::InconsistentLibraryTopWork)?;
                    Box::new(retained.rebase_departure(*old, *fresh)?)
                } else {
                    fresh
                };
                LibraryStage::Replacing {
                    top_first,
                    bottom_top_first,
                    selected,
                    batch,
                }
            }
        };
        self.pending_work = Some(PendingWork::LibraryTop(Box::new(work)));
        Ok(())
    }
    pub(super) fn begin_library_top(
        &mut self,
        instruction: &Instruction,
    ) -> Result<bool, EffectError> {
        let (kind, role, amount) = match instruction {
            Instruction::Scry { player, amount } => (LibraryActionKind::Scry, *player, amount),
            Instruction::Surveil { player, amount } => {
                (LibraryActionKind::Surveil, *player, amount)
            }
            Instruction::Mill { player, amount } => (LibraryActionKind::Mill, *player, amount),
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
            | Instruction::DrawController { .. }
            | Instruction::Sacrifice { .. }
            | Instruction::Discard { .. } => return Ok(false),
        };
        let controller = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?
            .controller();
        let requested = self.resolve_instruction_amount(controller, amount)?;
        if requested < 0.into() {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        let player = self.resolving_player_identity(role)?;
        if !self.window.participants().contains(&player) {
            self.advance_instruction()?;
            return Ok(true);
        }
        let library_before = self.window.cards().library(player).to_vec();
        let library = &library_before;
        let count = std::cmp::min(&requested, &RulesInteger::from_usize(library.len()))
            .to_usize()
            .ok_or(EffectError::InconsistentLibraryTopWork)?;
        let domain = library[..count].to_vec();
        let stage = if kind != LibraryActionKind::Mill && !domain.is_empty() {
            LibraryStage::Choosing
        } else {
            let selected = if kind == LibraryActionKind::Mill {
                domain.clone()
            } else {
                vec![]
            };
            LibraryStage::Preparing {
                top_first: vec![],
                bottom_top_first: vec![],
                selected,
            }
        };
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::LibraryTop(Box::new(LibraryTopWork {
            cursor,
            kind,
            player,
            requested,
            domain,
            library_before,
            window_revision: self.window.revision(),
            stage,
            operands: None,
        })));
        Ok(true)
    }
    pub(super) fn progress_library_top(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::LibraryTop(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongLibraryTopFrame);
        };
        let mut work = *work;
        loop {
            // The temporary stage is not published; the single owned work/cursor
            // is reinstalled only with the actual next stage or completed result.
            match std::mem::replace(&mut work.stage, LibraryStage::Choosing) {
                LibraryStage::Choosing => {
                    work.stage = LibraryStage::Choosing;
                }
                LibraryStage::Preparing {
                    top_first,
                    bottom_top_first,
                    selected,
                } => {
                    self.validate_library_partition(
                        &work,
                        &top_first,
                        &bottom_top_first,
                        &selected,
                    )?;
                    let replacements = self.begin_library_replacements(&work, &selected)?;
                    if let Some(batch) = replacements {
                        work.stage = LibraryStage::Replacing {
                            top_first,
                            bottom_top_first,
                            selected,
                            batch,
                        };
                    } else {
                        let groups = self.selection_arrival_groups(
                            &[ResolutionAcceptedSelection {
                                player: work.player,
                                objects: selected.clone(),
                            }],
                            None,
                        )?;
                        work.stage = LibraryStage::Ordering {
                            top_first,
                            bottom_top_first,
                            selected,
                            replacements: None,
                            groups,
                            orders: vec![],
                        };
                    }
                    continue;
                }
                LibraryStage::Replacing {
                    top_first,
                    bottom_top_first,
                    selected,
                    mut batch,
                } => {
                    let frame = batch.progress(self.revision)?;
                    if frame.is_some() {
                        work.stage = LibraryStage::Replacing {
                            top_first,
                            bottom_top_first,
                            selected,
                            batch,
                        };
                        self.pending_work = Some(PendingWork::LibraryTop(Box::new(work)));
                        return Ok(frame);
                    }
                    let groups = self.selection_arrival_groups(
                        &[ResolutionAcceptedSelection {
                            player: work.player,
                            objects: selected.clone(),
                        }],
                        Some(&batch),
                    )?;
                    work.stage = LibraryStage::Ordering {
                        top_first,
                        bottom_top_first,
                        selected,
                        replacements: Some(batch),
                        groups,
                        orders: vec![],
                    };
                    continue;
                }
                LibraryStage::Ordering {
                    top_first,
                    bottom_top_first,
                    selected,
                    replacements,
                    groups,
                    mut orders,
                } => {
                    let complete =
                        resolution_selection::next_graveyard_group(&groups, &mut orders)?.is_none();
                    work.stage = LibraryStage::Ordering {
                        top_first,
                        bottom_top_first,
                        selected,
                        replacements,
                        groups,
                        orders,
                    };
                    if complete {
                        self.commit_library_top(&work, append)?;
                        self.pending_work = Some(PendingWork::Resolution(work.cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                }
            }
            let frame = work
                .frame(self.revision)
                .ok_or(EffectError::InconsistentLibraryTopWork)?;
            self.pending_work = Some(PendingWork::LibraryTop(Box::new(work)));
            return Ok(Some(frame));
        }
    }
    /// Resume only the genuine current native library/owner frame on a tentative clone.
    ///
    /// # Errors
    /// Rejects a stale frame/revision, wrong actor/kind, incomplete or duplicated
    /// partition, or invalid owner permutation. Later commit failures are atomic.
    pub(super) fn resume_library_top(
        &self,
        expected: u64,
        frame: &EffectDecision,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let Some(PendingWork::LibraryTop(owned)) = &self.pending_work else {
            return Err(EffectError::WrongLibraryTopFrame);
        };
        self.validate_library_authority(owned)?;
        if owned.operands.is_some() {
            match &owned.stage {
                LibraryStage::Replacing {
                    selected, batch, ..
                }
                | LibraryStage::Ordering {
                    selected,
                    replacements: Some(batch),
                    ..
                } => {
                    let fresh = self
                        .begin_library_replacements_at(owned, selected, true)?
                        .ok_or(EffectError::InconsistentLibraryTopWork)?;
                    batch.verify_pending(*fresh)?;
                }
                _ => {}
            }
        }
        if self.library_top_frame().as_ref() != Some(frame) {
            return Err(EffectError::WrongLibraryTopFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let mut next = self.clone();
        let Some(PendingWork::LibraryTop(work)) = next.pending_work.take() else {
            return Err(EffectError::WrongLibraryTopFrame);
        };
        let mut work = *work;
        work.stage = match (work.stage, reply) {
            (LibraryStage::Choosing, DecisionReply::LibraryTop(reply)) => {
                let (top_first, bottom_top_first, selected) = match (work.kind, reply) {
                    (
                        LibraryActionKind::Scry,
                        LibraryTopReply::Scry {
                            top_first,
                            bottom_top_first,
                        },
                    ) => {
                        validate_partition(&work.domain, &top_first, &bottom_top_first)?;
                        (top_first, bottom_top_first, vec![])
                    }
                    (
                        LibraryActionKind::Surveil,
                        LibraryTopReply::Surveil {
                            top_first,
                            to_graveyard,
                        },
                    ) => {
                        validate_partition(&work.domain, &top_first, &to_graveyard)?;
                        (top_first, vec![], to_graveyard)
                    }
                    _ => return Err(EffectError::WrongDecisionKind),
                };
                let selected = work
                    .domain
                    .iter()
                    .copied()
                    .filter(|object| selected.contains(object))
                    .collect();
                LibraryStage::Preparing {
                    top_first,
                    bottom_top_first,
                    selected,
                }
            }
            (
                LibraryStage::Ordering {
                    top_first,
                    bottom_top_first,
                    selected,
                    replacements,
                    groups,
                    mut orders,
                },
                DecisionReply::OrderResolutionGraveyard(order),
            ) => {
                let group = groups
                    .get(orders.len())
                    .ok_or(EffectError::InconsistentLibraryTopWork)?;
                resolution_selection::validate_graveyard_order(group, &order)?;
                orders.push(ResolutionAcceptedGraveyardOrder {
                    owner: actor,
                    top_to_bottom: order,
                });
                LibraryStage::Ordering {
                    top_first,
                    bottom_top_first,
                    selected,
                    replacements,
                    groups,
                    orders,
                }
            }
            (
                LibraryStage::Replacing {
                    top_first,
                    bottom_top_first,
                    selected,
                    mut batch,
                },
                DecisionReply::ZoneReplacement(reply),
            ) => {
                let EffectDecision::ZoneReplacement(bound) = frame else {
                    return Err(EffectError::WrongLibraryTopFrame);
                };
                batch.accept(self.revision, bound, actor, reply)?;
                LibraryStage::Replacing {
                    top_first,
                    bottom_top_first,
                    selected,
                    batch,
                }
            }
            _ => return Err(EffectError::WrongDecisionKind),
        };
        next.pending_work = Some(PendingWork::LibraryTop(Box::new(work)));
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
    /// Reauthorize the exact original operation/amount/prefix in every branch.
    fn validate_library_authority(&self, work: &LibraryTopWork) -> Result<(), EffectError> {
        let entry = self
            .window
            .resolving()
            .filter(|entry| entry.id() == work.cursor.entry)
            .ok_or(EffectError::WrongResolutionFrame)?;
        let actual = self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|body| body.get(work.cursor.instruction))
            .ok_or(EffectError::InconsistentLibraryTopWork)?;
        let (kind, role, amount) = match actual {
            Instruction::Scry { player, amount } => (LibraryActionKind::Scry, *player, amount),
            Instruction::Surveil { player, amount } => {
                (LibraryActionKind::Surveil, *player, amount)
            }
            Instruction::Mill { player, amount } => (LibraryActionKind::Mill, *player, amount),
            _ => return Err(EffectError::InconsistentLibraryTopWork),
        };
        let requested = if let Some(operands) = &work.operands {
            if operands.cursor != work.cursor
                || operands.instruction != *actual
                || operands.player != work.player
                || operands.requested != work.requested
            {
                return Err(EffectError::InconsistentLibraryTopWork);
            }
            work.requested.clone()
        } else {
            let requested =
                self.resolve_instruction_amount_at(entry.controller(), amount, Some(&work.cursor))?;
            if self.resolving_player(role)? != work.player || requested != work.requested {
                return Err(EffectError::InconsistentLibraryTopWork);
            }
            requested
        };
        if kind != work.kind
            || requested < 0.into()
            || self.window.revision() != work.window_revision
            || self.window.cards().library(work.player) != work.library_before
        {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        let count = std::cmp::min(
            &requested,
            &RulesInteger::from_usize(work.library_before.len()),
        )
        .to_usize()
        .ok_or(EffectError::InconsistentLibraryTopWork)?;
        if work.domain != work.library_before[..count]
            || work.domain.iter().any(|id| {
                self.window.cards().get(*id).is_none_or(|c| {
                    c.zone() != ObjectZone::Library(work.player)
                        || c.owner() != work.player
                        || c.card_id().is_none()
                })
            })
        {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        Ok(())
    }
    fn validate_library_partition(
        &self,
        work: &LibraryTopWork,
        top: &[ObjectId],
        bottom: &[ObjectId],
        selected: &[ObjectId],
    ) -> Result<(), EffectError> {
        self.validate_library_authority(work)?;
        match work.kind {
            LibraryActionKind::Scry if selected.is_empty() => {
                validate_partition(&work.domain, top, bottom)
            }
            LibraryActionKind::Surveil if bottom.is_empty() => {
                validate_partition(&work.domain, top, selected)
            }
            LibraryActionKind::Mill
                if top.is_empty() && bottom.is_empty() && selected == work.domain =>
            {
                Ok(())
            }
            _ => Err(EffectError::InconsistentLibraryTopWork),
        }
    }
    fn begin_library_replacements(
        &self,
        work: &LibraryTopWork,
        selected: &[ObjectId],
    ) -> Result<Option<Box<ZoneReplacementBatch>>, EffectError> {
        self.begin_library_replacements_at(work, selected, work.operands.is_some())
    }
    fn begin_library_replacements_at(
        &self,
        work: &LibraryTopWork,
        selected: &[ObjectId],
        retain_candidates: bool,
    ) -> Result<Option<Box<ZoneReplacementBatch>>, EffectError> {
        self.validate_library_authority(work)?;
        if selected.is_empty() {
            return Ok(None);
        }
        if selected.iter().copied().collect::<BTreeSet<_>>().len() != selected.len()
            || !selected.iter().all(|id| work.domain.contains(id))
        {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        let (action, reason) = match work.kind {
            LibraryActionKind::Mill if selected == work.domain => (
                ZoneActionKind::EffectMill,
                ZoneReason::Mill {
                    player: work.player,
                },
            ),
            LibraryActionKind::Surveil => (
                ZoneActionKind::EffectSurveil,
                ZoneReason::Surveil {
                    player: work.player,
                },
            ),
            _ => return Err(EffectError::InconsistentLibraryTopWork),
        };
        let batch = self.begin_zone_replacements_authorized(
            AuthorizedZoneBatch {
                action,
                cause: EventCause::Instruction {
                    entry: work.cursor.entry,
                    instruction: work.cursor.instruction,
                },
                intents: selected
                    .iter()
                    .map(|before| ZoneMoveIntent {
                        before: *before,
                        reason,
                    })
                    .collect(),
            },
            retain_candidates,
        )?;
        if batch.sources().is_empty() && !retain_candidates {
            return Ok(None);
        }
        if batch.candidates().len() != selected.len() {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        Ok(Some(Box::new(batch)))
    }
    fn commit_library_top(
        &mut self,
        work: &LibraryTopWork,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.validate_library_authority(work)?;
        let LibraryStage::Ordering {
            top_first,
            bottom_top_first,
            selected,
            orders,
            replacements,
            ..
        } = &work.stage
        else {
            return Err(EffectError::InconsistentLibraryTopWork);
        };
        self.validate_library_partition(work, top_first, bottom_top_first, selected)?;
        let fresh = self.begin_library_replacements(work, selected)?;
        match (replacements, fresh) {
            (None, None) => {}
            (Some(retained), Some(initial)) => retained.verify_initial(*initial)?,
            _ => return Err(EffectError::InconsistentLibraryTopWork),
        }
        if !work.requested.is_positive() {
            return Ok(());
        }
        let batch = self.selection_batch_replaced(
            &[ResolutionAcceptedSelection {
                player: work.player,
                objects: selected.clone(),
            }],
            orders.clone(),
            replacements.clone(),
        )?;
        let before = self.rules_snapshot(None)?;
        let application = batch
            .application
            .iter()
            .map(|row| {
                Ok(crate::stack_window::LibraryTopMove {
                    before: row.object,
                    destination: batch
                        .replacements
                        .as_ref()
                        .map_or(Ok(PublicZoneDestination::Graveyard), |work| {
                            work.destination(row.object)
                        })?,
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        let (window, movements) = self.window.record_library_top(
            self.window.revision(),
            work.cursor.entry,
            crate::stack_window::LibraryTopCommit {
                player: work.player,
                frozen: &work.domain,
                top: top_first,
                bottom: bottom_top_first,
                moves_application: &application,
            },
        )?;
        let count = movements.len();
        let by_id: BTreeMap<_, _> = movements
            .into_iter()
            .map(|movement| (movement.before.id(), movement))
            .collect();
        if count != selected.len()
            || by_id.len() != count
            || by_id.keys().copied().collect::<BTreeSet<_>>() != selected.iter().copied().collect()
        {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        let public_zone_moves = selected
            .iter()
            .map(|id| {
                let movement = by_id
                    .get(id)
                    .ok_or(EffectError::InconsistentLibraryTopWork)?;
                let destination = match movement.after.zone() {
                    ObjectZone::Graveyard(owner) if owner == work.player => {
                        PublicZoneDestination::Graveyard
                    }
                    ObjectZone::Exile => PublicZoneDestination::Exile,
                    _ => return Err(EffectError::InconsistentLibraryTopWork),
                };
                if batch
                    .replacements
                    .as_ref()
                    .map_or(Ok(PublicZoneDestination::Graveyard), |work| {
                        work.destination(*id)
                    })?
                    != destination
                {
                    return Err(EffectError::InconsistentLibraryTopWork);
                }
                Ok(LibraryPublicMove {
                    before: *id,
                    after: movement.after.id(),
                    destination,
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        let action = LibraryAction {
            kind: work.kind,
            player: work.player,
            requested: work.requested.clone(),
            looked_top_first: work.domain.clone(),
            top_first: top_first.clone(),
            bottom_top_first: bottom_top_first.clone(),
            selected_before_top_first: selected.clone(),
            public_zone_moves,
            graveyard_before_top_first: batch
                .logical
                .iter()
                .filter(|row| matches!(by_id[&row.object].after.zone(), ObjectZone::Graveyard(_)))
                .map(|row| row.object)
                .collect(),
        };
        self.window = window;
        self.emit_from(
            EventCause::Instruction {
                entry: work.cursor.entry,
                instruction: work.cursor.instruction,
            },
            &before,
            None,
            append,
            |previous, current, controllers| {
                let mut values = vec![RulesOccurrence::LibraryAction(action)];
                let reason = match work.kind {
                    LibraryActionKind::Surveil => ZoneReason::Surveil {
                        player: work.player,
                    },
                    LibraryActionKind::Mill => ZoneReason::Mill {
                        player: work.player,
                    },
                    LibraryActionKind::Scry => return Ok(values),
                };
                for row in &batch.logical {
                    values.push(zone_occurrence_at(
                        previous,
                        current,
                        by_id
                            .get(&row.object)
                            .ok_or(EffectError::InconsistentLibraryTopWork)?,
                        reason,
                        controllers,
                    )?);
                }
                Ok(values)
            },
        )
    }
}
fn validate_partition(
    domain: &[ObjectId],
    first: &[ObjectId],
    second: &[ObjectId],
) -> Result<(), EffectError> {
    let actual: BTreeSet<_> = first.iter().chain(second).copied().collect();
    let expected: BTreeSet<_> = domain.iter().copied().collect();
    if first.len().checked_add(second.len()) != Some(domain.len())
        || actual.len() != domain.len()
        || actual != expected
    {
        return Err(EffectError::InvalidLibraryPartition);
    }
    Ok(())
}
