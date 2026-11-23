//! One replacement-aware, simultaneous exile of all physical graveyard cards.
use super::*;
use crate::{
    occurrence::{EventCause, ZoneReason},
    program::ZoneActionKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GraveyardExileWork {
    pub(super) cursor: ResolutionCursor,
    pub(super) batch: ZoneReplacementBatch,
    objects: Vec<ObjectId>,
}
impl EffectWindow {
    pub(super) fn begin_graveyard_exile(&mut self) -> Result<(), EffectError> {
        let objects = self
            .window
            .cards()
            .iter()
            .filter(|card| {
                card.card_id().is_some() && matches!(card.zone(), ObjectZone::Graveyard(_))
            })
            .map(GameObject::id)
            .collect::<Vec<_>>();
        self.begin_selected_graveyard_exile(objects)
    }
    pub(super) fn begin_players_graveyard_exile(
        &mut self,
        players: &[SeatId],
    ) -> Result<(), EffectError> {
        let objects = self
            .window
            .cards()
            .iter()
            .filter(|card| {
                card.card_id().is_some()
                    && matches!(card.zone(), ObjectZone::Graveyard(owner) if players.contains(&owner))
            })
            .map(GameObject::id)
            .collect::<Vec<_>>();
        self.begin_selected_graveyard_exile(objects)
    }
    pub(super) fn begin_selected_graveyard_exile(
        &mut self,
        objects: Vec<ObjectId>,
    ) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        cursor.impulse.exiled_milled_power = Some(0.into());
        let cursor = cursor.clone();
        if objects.is_empty() {
            return self.advance_instruction();
        }
        let batch = self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            action: ZoneActionKind::EffectExile,
            intents: objects
                .iter()
                .map(|id| zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason: ZoneReason::ExiledByEffect,
                })
                .collect(),
        })?;
        if !matches!(self.pending_work, Some(PendingWork::Resolution(_))) {
            return Err(EffectError::WrongResolutionFrame);
        }
        self.pending_work = Some(PendingWork::GraveyardExile(Box::new(GraveyardExileWork {
            cursor,
            batch,
            objects,
        })));
        Ok(())
    }
    pub(super) fn progress_graveyard_exile(
        &mut self,
        journal: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::GraveyardExile(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if let Some(frame) = work.batch.progress(self.revision)? {
            self.pending_work = Some(PendingWork::GraveyardExile(work));
            return Ok(Some(frame));
        }
        let cause = EventCause::Instruction {
            entry: work.cursor.entry,
            instruction: work.cursor.instruction,
        };
        let fresh = self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: cause.clone(),
            action: ZoneActionKind::EffectExile,
            intents: work
                .objects
                .iter()
                .map(|id| zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason: ZoneReason::ExiledByEffect,
                })
                .collect(),
        })?;
        work.batch.verify_initial(fresh)?;
        let moves = work
            .objects
            .iter()
            .map(|id| {
                Ok((
                    *id,
                    work.batch
                        .destination(*id)?
                        .physical(None)
                        .ok_or(EffectError::InconsistentProgram)?,
                ))
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        let instruction = self
            .stack_body(work.cursor.entry)
            .and_then(|body| body.instructions.get(work.cursor.instruction));
        let copy = if matches!(
            instruction,
            Some(Instruction::ExileForBlackToken { .. } | Instruction::ExileForSubtypeToken(_))
        ) {
            let [object] = work.objects.as_slice() else {
                return Err(EffectError::InconsistentProgram);
            };
            // CR 118.12 checks whether the cost was taken, including replaced moves.
            let mut definition = self
                .copiable_program(*object)
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            match instruction {
                Some(Instruction::ExileForSubtypeToken(subtype)) => {
                    definition.creature_subtypes.insert(*subtype);
                }
                Some(Instruction::ExileForBlackToken {
                    subtype,
                    size,
                    add_types,
                }) => {
                    let ProgramKind::Permanent(permanent) = &mut definition.kind else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    permanent.printed_pt = Some(
                        crate::characteristics::PowerToughness {
                            power: i64::from(*size).into(),
                            toughness: i64::from(*size).into(),
                        }
                        .into(),
                    );
                    definition.effect_defined_colors =
                        Some([crate::characteristics::Color::Black].into());
                    definition.printed_keywords.remove(&KeywordAbility::Devoid);
                    definition.card_types.insert(CardType::Creature);
                    if !*add_types {
                        definition.creature_subtypes.clear();
                        definition
                            .printed_keywords
                            .remove(&KeywordAbility::Changeling);
                    }
                    definition.creature_subtypes.insert(*subtype);
                    // CR 707.9d omits characteristic-defining abilities for a value set by this copy exception.
                    if let Some(crate::characteristics::StaticContinuousAbility::CharacteristicDefining { parts, .. }) = &definition.continuous
                        && parts.0.iter().any(|part| !matches!(part, crate::characteristics::DefiningPart::CreatureTypes(_) if *add_types)) {
                        definition.continuous = None;
                    }
                }
                _ => return Err(EffectError::InconsistentProgram),
            }
            Some(definition)
        } else {
            None
        };
        let before = self.rules_snapshot(None)?;
        let (window, movements) = self
            .window
            .record_graveyard_exile_moves(work.cursor.entry, &moves)?;
        self.window = window;
        if matches!(
            self.stack_body(work.cursor.entry)
                .and_then(|body| body.instructions.get(work.cursor.instruction)),
            Some(Instruction::ExileMilledCreatures { .. })
        ) {
            let current = self.evaluated_characteristics()?;
            work.cursor.impulse.exiled_milled_power = Some(
                movements
                    .iter()
                    .filter(|movement| movement.after.zone() == ObjectZone::Exile)
                    .filter_map(|movement| {
                        current
                            .get(movement.after.id())
                            .and_then(|card| card.power_toughness.as_ref())
                    })
                    .fold(RulesInteger::from(0), |sum, pt| sum.plus(&pt.derived.power)),
            );
        }
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        if !movements.is_empty() {
            self.emit_from(
                cause,
                &before,
                None,
                journal,
                |old, current, controllers| {
                    movements
                        .iter()
                        .map(|movement| {
                            zone_occurrence_at(
                                old,
                                current,
                                movement,
                                ZoneReason::ExiledByEffect,
                                controllers,
                            )
                        })
                        .collect()
                },
            )?;
        }
        if let Some(definition) = copy {
            let controller = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?
                .controller();
            self.begin_materialized_token_entries(
                vec![(controller, 1, definition, false)],
                None,
                false,
            )?;
        } else {
            self.advance_instruction()?;
        }
        Ok(None)
    }
}

impl EffectWindow {
    pub(super) fn milled_creature_frame(
        &self,
        maximum: u16,
    ) -> Result<Option<ResolutionSelectionFrame>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let current = self.evaluated_characteristics()?;
        let domain = cursor
            .impulse
            .milled
            .iter()
            .copied()
            .filter(|&id| {
                current.get(id).is_some_and(|card| {
                    matches!(card.zone, ObjectZone::Graveyard(_)) && card.is_creature()
                })
            })
            .collect::<Vec<_>>();
        Ok((!domain.is_empty()).then(|| {
            ResolutionSelectionFrame::hand_entry(
                self.revision,
                entry.id(),
                cursor.instruction,
                entry.controller(),
                domain,
                usize::from(maximum),
                ResolutionSelectionKind::ExileMilledCreatures,
            )
        }))
    }
    pub(super) fn resume_milled_exile(
        &self,
        expected: u64,
        frame: &ResolutionSelectionFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let Instruction::ExileMilledCreatures { maximum } = self.current_instruction()? else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if self.milled_creature_frame(*maximum)?.as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::SelectResolution(objects) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if objects.len() > frame.required()
            || objects.iter().collect::<BTreeSet<_>>().len() != objects.len()
            || objects.iter().any(|id| !frame.domain().contains(id))
        {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        next.revision = revision;
        next.begin_selected_graveyard_exile(objects)?;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
}
