//! Spell modes are chosen on announcement and performed in printed order (CR 700.2).
use super::*;
use crate::{
    mana::CastMode,
    program::SpellDefinition,
    targets::{BoundTarget, TargetLegality},
};

/// The half-open instruction range belongs to one occurrence of one printed mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SelectedMode {
    pub printed: u8,
    reported: bool,
    start: usize,
    end: usize,
    pub target: Option<(TargetId, BodyTarget)>,
    extra_targets: Vec<(TargetId, BodyTarget)>,
    pub(super) clause: Option<TargetClause>,
}

impl SelectedMode {
    /// An instruction segment introduced by splice carries targets but has no chosen mode number.
    pub(super) fn splice(
        start: usize,
        end: usize,
        mut targets: impl Iterator<Item = (TargetId, BodyTarget)>,
    ) -> Self {
        Self {
            printed: 0,
            reported: false,
            start,
            end,
            target: targets.next(),
            extra_targets: targets.collect(),
            clause: None,
        }
    }
    pub(super) fn targets(&self) -> impl Iterator<Item = (TargetId, BodyTarget)> + '_ {
        self.target.iter().chain(&self.extra_targets).copied()
    }
}

impl EffectWindow {
    pub(super) fn ordered_permanent(&self, position: u16) -> Result<Option<ObjectId>, EffectError> {
        let assessment = self
            .window
            .target_assessment()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let Some(row) = assessment
            .get(TargetId(u32::from(position)))
            .filter(|row| row.legality == TargetLegality::Legal)
        else {
            return Ok(None);
        };
        match row.choice.target {
            TargetRef::Object(object) => Ok(Some(object)),
            _ => Err(EffectError::InconsistentProgram),
        }
    }

    /// Legal surviving objects in the original multi-target clause. CR 608.2b.
    pub(super) fn legal_permanent_targets(&self) -> Result<BTreeSet<ObjectId>, EffectError> {
        self.legal_object_targets_at(
            self.resolution_cursor()
                .map(|cursor| (cursor.entry, cursor.instruction)),
        )
    }

    pub(super) fn legal_object_targets_at(
        &self,
        position: Option<(StackEntryId, usize)>,
    ) -> Result<BTreeSet<ObjectId>, EffectError> {
        let assessment = self
            .window
            .target_assessment()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let mode = position.and_then(|(entry, index)| self.instruction_mode(entry, index));
        assessment
            .results()
            .iter()
            .filter(|row| {
                row.legality == TargetLegality::Legal
                    && mode.is_none_or(|mode| mode.targets().any(|(id, _)| id == row.choice.id))
            })
            .map(|row| match row.choice.target {
                TargetRef::Object(object) => Ok(object),
                _ => Err(EffectError::InconsistentProgram),
            })
            .collect()
    }

    pub(super) fn select_modes(
        &self,
        definition: &SpellDefinition,
        choices: &[CastMode],
        entwined: bool,
        current: &EvaluatedCharacteristics,
        source: &ObjectCharacteristics,
        actor: SeatId,
    ) -> Result<(ExecutableBody, Vec<SelectedMode>, Vec<TargetChoice>), EffectError> {
        let spec = definition
            .modes
            .as_ref()
            .ok_or(EffectError::InvalidCastResource)?;
        let bodies = std::iter::once(&definition.body)
            .chain(&spec.additional)
            .collect::<Vec<_>>();
        let bounds = if entwined {
            (bodies.len(), bodies.len())
        } else {
            (usize::from(spec.minimum), usize::from(spec.maximum))
        };
        self.compose_modes(
            &bodies,
            choices,
            bounds,
            spec.allow_repeat,
            |_, body, target| {
                !Self::protected_target(
                    target,
                    &super::protection::ProtectionSource::from(source),
                    current,
                ) && self.legal_body_target_in(body, target, actor, current)
            },
        )
    }

    pub(super) fn compose_modes(
        &self,
        bodies: &[&ExecutableBody],
        choices: &[CastMode],
        bounds: (usize, usize),
        allow_repeat: bool,
        legal: impl Fn(u8, &ExecutableBody, TargetRef) -> bool,
    ) -> Result<(ExecutableBody, Vec<SelectedMode>, Vec<TargetChoice>), EffectError> {
        if !(bounds.0..=bounds.1).contains(&choices.len()) {
            return Err(EffectError::InvalidCastResource);
        }
        let mut choices = choices.to_vec();
        // Preserve announcement order between repeated instances of the same mode.
        choices.sort_by_key(|choice| choice.mode);
        if !allow_repeat && choices.windows(2).any(|pair| pair[0].mode == pair[1].mode) {
            return Err(EffectError::InvalidCastResource);
        }
        let mut modes = Vec::new();
        let mut instructions = Vec::new();
        let mut targets = Vec::new();
        for choice in choices {
            let body = *bodies
                .get(usize::from(choice.mode))
                .ok_or(EffectError::InvalidCastResource)?;
            let mut bound = Vec::new();
            let mut clause = None;
            if body.target == BodyTarget::None {
                if choice.target.is_some() || !choice.targets.is_empty() {
                    return Err(EffectError::UnexpectedTarget);
                }
            } else {
                let (minimum, maximum, selections) =
                    if let Some((minimum, maximum)) = body.permanent_target_bounds() {
                        if choice.target.is_some() {
                            return Err(EffectError::UnexpectedTarget);
                        }
                        (minimum, maximum, choice.targets)
                    } else {
                        if !choice.targets.is_empty() {
                            return Err(EffectError::UnexpectedTarget);
                        }
                        (
                            1,
                            1,
                            vec![choice.target.ok_or(EffectError::TargetRequired)?],
                        )
                    };
                if !(minimum..=maximum).contains(&selections.len())
                    || selections.iter().collect::<BTreeSet<_>>().len() != selections.len()
                {
                    return Err(EffectError::InvalidCastResource);
                }
                let clause_id = TargetClauseId(
                    u32::try_from(modes.len()).map_err(|_| EffectError::InconsistentProgram)?,
                );
                clause = Some(TargetClause {
                    id: clause_id,
                    minimum,
                    maximum,
                });
                for target in selections {
                    if !legal(choice.mode, body, target) {
                        return Err(EffectError::IllegalTarget);
                    }
                    let id = TargetId(
                        u32::try_from(targets.len())
                            .map_err(|_| EffectError::InconsistentProgram)?,
                    );
                    targets.push(TargetChoice {
                        id,
                        clause: clause_id,
                        target,
                    });
                    bound.push((id, body.target));
                }
            }
            let mut bound = bound.into_iter();
            let target = bound.next();
            let start = instructions.len();
            for mut op in body.instructions.iter().cloned() {
                // Local jump destinations include the end-of-mode sentinel.
                match &mut op {
                    Instruction::FlipCoin {
                        lose_destination: destination,
                    }
                    | Instruction::BranchIfFalse { destination, .. }
                    | Instruction::Jump { destination } => *destination += start,
                    Instruction::OfferManaPayment {
                        paid_destination,
                        declined_destination,
                        ..
                    } => {
                        *paid_destination += start;
                        *declined_destination += start;
                    }
                    Instruction::OfferSourceCounters {
                        placed_destination,
                        declined_destination,
                        ..
                    } => {
                        *placed_destination += start;
                        *declined_destination += start;
                    }
                    _ => {}
                }
                instructions.push(op);
            }
            modes.push(SelectedMode {
                printed: choice.mode,
                reported: true,
                clause,
                extra_targets: bound.collect(),
                start,
                end: instructions.len(),
                target,
            });
        }
        Ok((
            ExecutableBody::new(BodyTarget::None, instructions),
            modes,
            targets,
        ))
    }

    pub(super) fn expand_fixed_clauses(
        &self,
        body: &ExecutableBody,
        choices: &[TargetRef],
        actor: SeatId,
        current: &EvaluatedCharacteristics,
        source: &ObjectCharacteristics,
    ) -> Result<(ExecutableBody, Vec<SelectedMode>, Vec<TargetChoice>), EffectError> {
        let copies_other = matches!(
            body.instructions.last(),
            Some(Instruction::CopyOtherTargetAsArtifact)
        );
        let fights = body.instructions.last() == Some(&Instruction::FightFixedTargets);
        if (copies_other || fights) && matches!(choices, [first, second] if first == second) {
            return Err(EffectError::IllegalTarget);
        }
        let shared_action = copies_other
            || fights
            || matches!(
                body.instructions.last(),
                Some(
                    Instruction::ReturnFixedTargets
                        | Instruction::DestroyFixedTargets
                        | Instruction::DestroyTargetAndAttachedEquipment
                        | Instruction::ExileUntilOtherTargetLeaves
                        | Instruction::CreatureDealsPowerDamageToOther { .. }
                )
            );
        let mut choices = choices.iter();
        let mut instructions = Vec::new();
        let mut segments = Vec::new();
        let mut targets = Vec::new();
        for op in &body.instructions {
            if let Instruction::FixedTargetClause(clause) = op {
                let target = *choices.next().ok_or(EffectError::TargetRequired)?;
                if Self::protected_target(
                    target,
                    &super::protection::ProtectionSource::from(source),
                    current,
                ) || !self.legal_body_target_in(clause, target, actor, current)
                {
                    return Err(EffectError::IllegalTarget);
                }
                let ordinal =
                    u32::try_from(targets.len()).map_err(|_| EffectError::InvalidCastResource)?;
                let id = TargetId(ordinal);
                targets.push(TargetChoice {
                    id,
                    clause: TargetClauseId(ordinal),
                    target,
                });
                let start = instructions.len();
                if shared_action {
                    instructions.push(Instruction::Jump {
                        destination: start + 1,
                    });
                } else {
                    instructions.extend(clause.instructions.iter().cloned());
                }
                segments.push(SelectedMode {
                    printed: 0,
                    reported: false,
                    clause: Some(TargetClause {
                        id: TargetClauseId(ordinal),
                        minimum: 1,
                        maximum: 1,
                    }),
                    extra_targets: vec![],
                    start,
                    end: instructions.len(),
                    target: Some((id, clause.target)),
                });
            } else {
                instructions.push(op.clone());
            }
        }
        if choices.next().is_some() {
            return Err(EffectError::UnexpectedTarget);
        }
        Ok((
            ExecutableBody::new(BodyTarget::None, instructions),
            segments,
            targets,
        ))
    }

    pub(super) fn legal_linked_targets(&self) -> Result<Option<(ObjectId, ObjectId)>, EffectError> {
        let assessment = self
            .window
            .target_assessment()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let find = |id| {
            assessment
                .get(TargetId(id))
                .filter(|row| row.legality == TargetLegality::Legal)
                .and_then(|row| match row.choice.target {
                    TargetRef::Object(object) => Some(object),
                    _ => None,
                })
        };
        Ok(find(0).zip(find(1)))
    }

    pub(super) fn stack_modes(&self, entry: StackEntryId) -> &[SelectedMode] {
        match self.payloads.get(&entry) {
            Some(StackPayload::Instructions(payload)) => &payload.modes,
            _ => &[],
        }
    }

    /// Printed indices in resolution order, including repeated chosen modes.
    pub fn chosen_spell_modes(&self, entry: StackEntryId) -> impl Iterator<Item = u8> + '_ {
        self.stack_modes(entry)
            .iter()
            .filter(|mode| mode.reported)
            .map(|mode| mode.printed)
    }

    pub(super) fn mode_target_contract(
        &self,
        entry: StackEntryId,
        target: TargetId,
    ) -> Option<BodyTarget> {
        self.stack_modes(entry)
            .iter()
            .flat_map(SelectedMode::targets)
            .find_map(|(id, kind)| (id == target).then_some(kind))
    }

    fn instruction_mode(&self, entry: StackEntryId, instruction: usize) -> Option<&SelectedMode> {
        self.stack_modes(entry)
            .iter()
            .find(|mode| (mode.start..mode.end).contains(&instruction))
    }

    pub(super) fn instruction_target<'a>(
        &self,
        entry: &'a crate::stack_window::StackEntry,
    ) -> Result<&'a BoundTarget, EffectError> {
        self.instruction_target_at(entry, self.instruction_index())
    }

    pub(super) fn instruction_target_at<'a>(
        &self,
        entry: &'a crate::stack_window::StackEntry,
        instruction: Option<usize>,
    ) -> Result<&'a BoundTarget, EffectError> {
        let targets = entry.targets().targets();
        if let Some(mode) = instruction.and_then(|index| self.instruction_mode(entry.id(), index)) {
            let (id, _) = mode.target.ok_or(EffectError::InconsistentProgram)?;
            return targets
                .iter()
                .find(|bound| bound.choice().id == id)
                .ok_or(EffectError::InconsistentProgram);
        }
        let [bound] = targets else {
            return Err(EffectError::InconsistentProgram);
        };
        Ok(bound)
    }

    pub(super) fn mode_target_illegal(&self, entry: StackEntryId, index: usize) -> bool {
        self.instruction_mode(entry, index)
            .filter(|mode| mode.target.is_some())
            .is_some_and(|mode| {
                mode.targets().all(|(id, _)| {
                    self.window
                        .target_assessment()
                        .and_then(|assessment| assessment.get(id))
                        .is_some_and(|result| result.legality != TargetLegality::Legal)
                })
            })
    }
}

impl InstructionPayload {
    /// A resolving instruction can expand into a different number of operations.
    /// Keep later targets, modes and branch destinations bound to their original text.
    pub(super) fn expand_instruction(
        &mut self,
        index: usize,
        keep_original: bool,
        operations: impl IntoIterator<Item = Instruction>,
    ) {
        let operations: Vec<_> = operations.into_iter().collect();
        let removed = usize::from(!keep_original);
        let rebase = |position: &mut usize| {
            if *position > index {
                *position = *position - removed + operations.len();
            }
        };
        for mode in &mut self.modes {
            rebase(&mut mode.start);
            rebase(&mut mode.end);
        }
        for instruction in &mut self.body.instructions {
            match instruction {
                Instruction::FlipCoin {
                    lose_destination: destination,
                }
                | Instruction::BranchIfFalse { destination, .. }
                | Instruction::Jump { destination } => rebase(destination),
                Instruction::OfferManaPayment {
                    paid_destination,
                    declined_destination,
                    ..
                } => {
                    rebase(paid_destination);
                    rebase(declined_destination);
                }
                Instruction::OfferSourceCounters {
                    placed_destination,
                    declined_destination,
                    ..
                } => {
                    rebase(placed_destination);
                    rebase(declined_destination);
                }
                _ => {}
            }
        }
        self.body
            .instructions
            .splice(index + usize::from(keep_original)..index + 1, operations);
    }
}

#[cfg(test)]
mod segment_tests {
    use super::*;

    #[test]
    fn expanding_then_removing_host_operations_keeps_splice_target_and_branch_boundaries() {
        let host = ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::FlipCoin {
                    lose_destination: 2,
                },
                Instruction::DrawController {
                    amount: ValueExpr::Literal(1.into()),
                },
                Instruction::Damage { amount: 2.into() },
            ],
        );
        let StackPayload::Instructions(mut payload) = StackPayload::from_body(host, None) else {
            panic!()
        };
        payload.modes = vec![
            SelectedMode::splice(0, 2, std::iter::empty()),
            SelectedMode::splice(
                2,
                3,
                std::iter::once((TargetId(0), BodyTarget::SinglePlayer)),
            ),
        ];
        let draw = payload.body.instructions[1].clone();
        payload.expand_instruction(1, true, [draw.clone(), draw]);
        assert_eq!((payload.modes[1].start, payload.modes[1].end), (4, 5));
        assert_eq!(
            payload.body.instructions[0],
            Instruction::FlipCoin {
                lose_destination: 4
            }
        );
        payload.expand_instruction(1, false, []);
        assert_eq!((payload.modes[0].start, payload.modes[0].end), (0, 3));
        assert_eq!((payload.modes[1].start, payload.modes[1].end), (3, 4));
        assert_eq!(
            payload.body.instructions[0],
            Instruction::FlipCoin {
                lose_destination: 3
            }
        );
        assert_eq!(
            payload.body.instructions[3],
            Instruction::Damage { amount: 2.into() }
        );
    }
}
