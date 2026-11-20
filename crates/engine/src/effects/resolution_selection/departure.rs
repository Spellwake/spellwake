//! Trusted repair of held choices; original legality and current feasibility differ.
use super::*;
use crate::program::{PublicZoneDestination, ZoneActionKind};

impl EffectWindow {
    pub(super) fn selection_requirement_at(
        &self,
        operation: &ResolutionSelectionOperation,
        amount: &RulesInteger,
        player: SeatId,
        evaluated: &EvaluatedCharacteristics,
    ) -> Result<ResolutionSelectionRequirement, EffectError> {
        let domain = if self.window.participants().contains(&player) {
            self.window
                .cards()
                .iter()
                .filter(|object| selection_eligible(operation, player, object, evaluated))
                .map(GameObject::id)
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let required = std::cmp::min(amount, &RulesInteger::from_usize(domain.len()))
            .to_usize()
            .ok_or(EffectError::InconsistentSelectionWork)?;
        Ok(ResolutionSelectionRequirement {
            player,
            domain,
            required,
        })
    }
    pub(super) fn selection_effective(
        &self,
        operation: &ResolutionSelectionOperation,
        materialized: &SelectionMaterialized,
    ) -> Vec<ResolutionAcceptedSelection> {
        materialized
            .certificates
            .iter()
            .map(|certificate| {
                let player = certificate.requirement.player;
                let objects = certificate
                    .objects
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.window.participants().contains(&player)
                            && self.window.cards().get(*id).is_some_and(|object| {
                                selection_action_possible(operation, player, object)
                            })
                    })
                    .collect();
                ResolutionAcceptedSelection { player, objects }
            })
            .collect()
    }
    pub(super) fn selection_replacements(
        &self,
        cursor: &ResolutionCursor,
        operation: &ResolutionSelectionOperation,
        materialized: &SelectionMaterialized,
        selections: Vec<ResolutionAcceptedSelection>,
    ) -> Result<ZoneReplacementBatch, EffectError> {
        let batch = self.begin_selection_replacements(
            cursor,
            operation,
            selections,
            materialized.operands.is_some(),
        )?;
        if materialized.operands.is_none() {
            return Ok(batch);
        }
        let action = match operation {
            ResolutionSelectionOperation::Sacrifice { .. } => ZoneActionKind::EffectSacrifice,
            ResolutionSelectionOperation::Discard => ZoneActionKind::EffectDiscard,
        };
        let mut origins = vec![];
        for (index, certificate) in materialized.certificates.iter().enumerate() {
            materialized.validate_certificate(index, certificate)?;
            let player = certificate.requirement.player;
            for before in &certificate.before {
                origins.push(ZoneCandidate {
                    key: ZoneEventKey::Selection {
                        entry: cursor.entry,
                        instruction: cursor.instruction,
                        before: before.id(),
                        player,
                    },
                    before: before.clone(),
                    action,
                    reason: operation.reason(player),
                    original: PublicZoneDestination::Graveyard,
                    current: PublicZoneDestination::Graveyard,
                });
            }
        }
        batch.with_selection_origins(origins)
    }
    pub(super) fn validate_selection_header(
        &self,
        work: &ResolutionSelectionWork,
    ) -> Result<(), EffectError> {
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
            .ok_or(EffectError::InconsistentSelectionWork)?;
        let (players, amount) = match (&work.operation, actual) {
            (
                ResolutionSelectionOperation::Sacrifice { filter },
                Instruction::Sacrifice {
                    players,
                    amount,
                    filter: actual,
                },
            ) if filter == actual => (*players, amount),
            (ResolutionSelectionOperation::Discard, Instruction::Discard { players, amount }) => {
                (*players, amount)
            }
            _ => return Err(EffectError::InconsistentSelectionWork),
        };
        let materialized = &work.materialized;
        if materialized.instruction != *actual
            || materialized.requested < 0.into()
            || materialized
                .players
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != materialized.players.len()
            || materialized
                .players
                .iter()
                .any(|player| !self.window.known_player(*player))
            || materialized.certificates.len() > materialized.players.len()
        {
            return Err(EffectError::InconsistentSelectionWork);
        }
        for (index, certificate) in materialized.certificates.iter().enumerate() {
            materialized.validate_certificate(index, certificate)?;
        }
        for certificate in &materialized.order_certificates {
            if certificate.group.owner != certificate.order.owner
                || certificate.group.entrants.len() < 2
                || !self.window.known_player(certificate.order.owner)
            {
                return Err(EffectError::InconsistentSelectionWork);
            }
            validate_graveyard_order(&certificate.group, &certificate.order.top_to_bottom)
                .map_err(|_| EffectError::InconsistentSelectionWork)?;
        }
        if let Some(operands) = &materialized.operands {
            if operands.cursor != work.cursor
                || operands.instruction != materialized.instruction
                || operands.requested != materialized.requested
                || operands.players != materialized.players
            {
                return Err(EffectError::InconsistentSelectionWork);
            }
        } else {
            let actual_players = match players {
                AffectedPlayers::One(role) => vec![self.resolving_player(role)?],
                AffectedPlayers::EachPlayer => self.window.apnap_players()?.collect(),
            };
            if materialized.players != actual_players
                || materialized.requested
                    != self.resolve_instruction_amount_at(
                        entry.controller(),
                        amount,
                        Some(&work.cursor),
                    )?
            {
                return Err(EffectError::InconsistentSelectionWork);
            }
        }
        Ok(())
    }
    pub(super) fn validate_selection_work(
        &self,
        work: &ResolutionSelectionWork,
    ) -> Result<(), EffectError> {
        self.validate_selection_header(work)?;
        let materialized = &work.materialized;
        let effective = self.selection_effective(&work.operation, materialized);
        match &work.stage {
            ResolutionSelectionStage::Choosing {
                requirements,
                next_player,
                accepted,
            } => {
                if requirements.len() != materialized.players.len()
                    || *next_player != accepted.len()
                    || accepted.len() != materialized.certificates.len()
                    || *accepted != effective
                {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                let evaluated = self.evaluated_characteristics()?;
                for (index, requirement) in requirements.iter().enumerate() {
                    if materialized.players.get(index) != Some(&requirement.player) {
                        return Err(EffectError::InconsistentSelectionWork);
                    }
                    if let Some(certificate) = materialized.certificates.get(index) {
                        if certificate.requirement != *requirement {
                            return Err(EffectError::InconsistentSelectionWork);
                        }
                    } else if *requirement
                        != self.selection_requirement_at(
                            &work.operation,
                            &materialized.requested,
                            requirement.player,
                            &evaluated,
                        )?
                    {
                        return Err(EffectError::InconsistentSelectionWork);
                    }
                }
            }
            ResolutionSelectionStage::Replacing { selections, batch } => {
                if materialized.certificates.len() != materialized.players.len()
                    || *selections != effective
                {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                batch.verify_pending(self.selection_replacements(
                    &work.cursor,
                    &work.operation,
                    materialized,
                    selections.clone(),
                )?)?;
            }
            ResolutionSelectionStage::Ordering {
                selections,
                replacements,
                groups,
                next_owner,
                orders,
            } => {
                if materialized.certificates.len() != materialized.players.len()
                    || *selections != effective
                    || *next_owner != orders.len()
                    || orders.len() > groups.len()
                    || *groups
                        != self.selection_arrival_groups(selections, replacements.as_deref())?
                {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                if let Some(batch) = replacements {
                    batch.verify_initial(self.selection_replacements(
                        &work.cursor,
                        &work.operation,
                        materialized,
                        selections.clone(),
                    )?)?;
                }
                for (group, order) in groups.iter().zip(orders) {
                    if group.owner != order.owner {
                        return Err(EffectError::InconsistentSelectionWork);
                    }
                    validate_graveyard_order(group, &order.top_to_bottom)
                        .map_err(|_| EffectError::InconsistentSelectionWork)?;
                }
            }
            ResolutionSelectionStage::Ready(_) => return Err(EffectError::WrongSelectionFrame),
        }
        Ok(())
    }
    pub(in crate::effects) fn refresh_selection_after_departure(
        &mut self,
        before: &Self,
    ) -> Result<(), EffectError> {
        let Some(PendingWork::ResolutionSelection(held)) = &before.pending_work else {
            return Err(EffectError::WrongSelectionFrame);
        };
        before.validate_selection_work(held)?;
        let mut work = held.as_ref().clone();
        if self.window.resolving().map(|entry| entry.id()) != Some(work.cursor.entry) {
            if self
                .window
                .entries()
                .iter()
                .any(|entry| entry.id() == work.cursor.entry)
            {
                return Err(EffectError::InconsistentSelectionWork);
            }
            self.pending_work = None;
            return Ok(());
        }
        if work.materialized.operands.is_none() {
            work.materialized.operands = Some(SelectionOperands {
                cursor: work.cursor.clone(),
                instruction: work.materialized.instruction.clone(),
                requested: work.materialized.requested.clone(),
                players: work.materialized.players.clone(),
            });
        }
        let effective = self.selection_effective(&work.operation, &work.materialized);
        work.stage = match work.stage {
            ResolutionSelectionStage::Choosing {
                mut requirements,
                next_player,
                ..
            } => {
                let evaluated = self.evaluated_characteristics()?;
                for requirement in requirements.iter_mut().skip(next_player) {
                    *requirement = self.selection_requirement_at(
                        &work.operation,
                        &work.materialized.requested,
                        requirement.player,
                        &evaluated,
                    )?;
                }
                ResolutionSelectionStage::Choosing {
                    requirements,
                    next_player,
                    accepted: effective,
                }
            }
            ResolutionSelectionStage::Replacing { selections, batch } => {
                let old = before.selection_replacements(
                    &work.cursor,
                    &work.operation,
                    &held.materialized,
                    selections,
                )?;
                let fresh = self.selection_replacements(
                    &work.cursor,
                    &work.operation,
                    &work.materialized,
                    effective.clone(),
                )?;
                let batch = batch.rebase_selection_departure(old, fresh)?;
                work.materialized.held_orders.clear();
                ResolutionSelectionStage::Replacing {
                    selections: effective,
                    batch: Box::new(batch),
                }
            }
            ResolutionSelectionStage::Ordering {
                selections,
                replacements,
                ..
            } => {
                let fresh = self.selection_replacements(
                    &work.cursor,
                    &work.operation,
                    &work.materialized,
                    effective.clone(),
                )?;
                let batch = if let Some(held_batch) = replacements {
                    let old = before.selection_replacements(
                        &work.cursor,
                        &work.operation,
                        &held.materialized,
                        selections,
                    )?;
                    held_batch.rebase_selection_departure(old, fresh)?
                } else {
                    fresh
                };
                work.materialized.held_orders.clear();
                ResolutionSelectionStage::Replacing {
                    selections: effective,
                    batch: Box::new(batch),
                }
            }
            ResolutionSelectionStage::Ready(_) => return Err(EffectError::WrongSelectionFrame),
        };
        self.pending_work = Some(PendingWork::ResolutionSelection(Box::new(work)));
        Ok(())
    }
}
