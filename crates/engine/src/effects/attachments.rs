//! Aura targeting, attachment legality and state-based detachment.
use super::*;
use crate::attachment::{AttachmentKind, EnchantRestriction};
impl EffectWindow {
    /// Current host of this exact Aura or Equipment incarnation.
    pub fn attached_to(&self, attachment: ObjectId) -> Option<TargetRef> {
        self.characteristics.attachments.get(&attachment).copied()
    }

    /// Read the live attachment or the last host of the exact departed source.
    pub(super) fn attachment_host_for_ability(&self, source: ObjectId) -> Option<TargetRef> {
        if self
            .window
            .cards()
            .get(source)
            .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
        {
            self.attached_to(source)
        } else {
            self.growth_last_known
                .get(&source)
                .and_then(|appearance| appearance.attached_to)
        }
    }

    pub(super) fn validate_attach_activation(
        &self,
        body: &ExecutableBody,
        source: ObjectId,
        actor: SeatId,
        turn: Option<&TurnScheduler>,
        target: Option<TargetRef>,
        evaluated: &EvaluatedCharacteristics,
    ) -> Result<(), EffectError> {
        if let [Instruction::MutateObject(spec)] = body.instructions.as_slice() {
            match &spec.op {
                crate::toolbox::ObjectMutation::Attach {
                    ability: Some(kind),
                    ..
                } => {
                    validate_main_phase(&self.window, turn, actor)?;
                    if *kind == crate::attachment::AttachAbility::Reconfigure
                        && target == Some(TargetRef::Object(source))
                    {
                        return Err(EffectError::IllegalTarget);
                    }
                }
                crate::toolbox::ObjectMutation::Unattach
                    if matches!(spec.source, crate::toolbox::ObjectQuery::Source) =>
                {
                    validate_main_phase(&self.window, turn, actor)?;
                    if !matches!(self.attached_to(source),Some(TargetRef::Object(host)) if evaluated.get(host).is_some_and(|v| v.zone==ObjectZone::Battlefield && v.is_creature()))
                    {
                        return Err(EffectError::NotAbilitySource);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn has_reconfigure(source: &ObjectCharacteristics) -> bool {
        source
            .printed_text
            .activated
            .iter()
            .filter(|a| source.abilities.activated.contains(&a.id))
            .chain(source.granted_activated.values())
            .any(|ability| {
                matches!(
                    ability.body.instructions.as_slice(),
                    [Instruction::MutateObject(spec)]
                        if matches!(
                            spec.op,
                            crate::toolbox::ObjectMutation::Attach {
                                ability: Some(crate::attachment::AttachAbility::Reconfigure),
                                ..
                            }
                        )
                )
            })
    }

    /// Whether this exact spell or battlefield incarnation is currently bestowed.
    pub fn is_bestowed(&self, object: ObjectId) -> bool {
        self.characteristics.bestowed_spells.contains(&object)
            || self.characteristics.bestowed_auras.contains_key(&object)
    }
    pub(super) fn attachment_kind(&self, object: ObjectId) -> Option<AttachmentKind> {
        if self.is_bestowed(object) {
            return Some(AttachmentKind::Aura(EnchantRestriction::Creature));
        }
        self.copiable_program(object)?.permanent()?.attachment
    }
    pub(super) fn aura_on_stack(
        &self,
        entry: &crate::stack_window::StackEntry,
    ) -> Option<EnchantRestriction> {
        let object = self.window.spell_object(entry)?;
        match self.attachment_kind(object) {
            Some(AttachmentKind::Aura(enchant)) => Some(enchant),
            _ => None,
        }
    }
    pub(super) fn enchant_permits(
        &self,
        enchant: EnchantRestriction,
        host: TargetRef,
        actor: SeatId,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        match host {
            TargetRef::Player(player) => {
                enchant == EnchantRestriction::Player
                    && self.window.participants().contains(&player)
            }
            TargetRef::Object(host) => evaluated
                .get(host)
                .is_some_and(|view| enchant.permits(view, actor)),
            TargetRef::Stack(_) => false,
        }
    }
    pub(super) fn aura_target_legal(
        &self,
        enchant: EnchantRestriction,
        host: TargetRef,
        actor: SeatId,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        self.enchant_permits(enchant, host, actor, evaluated)
            && self.legal_target_kind_in(enchant.target(), host, actor, evaluated)
    }
    pub(super) fn attach_if_legal(
        &mut self,
        entry: StackEntryId,
        source: ObjectId,
        target: TargetRef,
    ) -> Result<Option<crate::occurrence::RulesOccurrence>, EffectError> {
        let current = self.evaluated_characteristics()?;
        self.attach_at_cut(entry, source, target, &current)
    }

    pub(super) fn attach_at_cut(
        &mut self,
        entry: StackEntryId,
        source: ObjectId,
        target: TargetRef,
        current: &EvaluatedCharacteristics,
    ) -> Result<Option<crate::occurrence::RulesOccurrence>, EffectError> {
        let before = self.attached_to(source);
        if before == Some(target) {
            return Ok(None);
        }
        if !current.get(source).is_some_and(|view| {
            view.zone == ObjectZone::Battlefield
                && self.attachment_permitted(view, Some(target), current)
        }) {
            return Ok(None);
        }
        let reconfigure = current.get(source).is_some_and(Self::has_reconfigure)
            && self.attachment_kind(source) == Some(AttachmentKind::Equipment);
        if !self.characteristics.bestowed_auras.contains_key(&source) {
            self.characteristics.detach(source);
        }
        self.characteristics.attach(source, target, false)?;
        if reconfigure {
            self.characteristics
                .stop_being_creature_while_attached(entry, source)?;
        }
        Ok(Some(
            crate::occurrence::RulesOccurrence::AttachmentChanged {
                source,
                before,
                after: Some(target),
            },
        ))
    }
    pub(super) fn token_equipment_selection(
        &self,
    ) -> Result<Option<ResolutionSelectionFrame>, EffectError> {
        if !matches!(
            self.current_instruction()?,
            Instruction::MutateObject(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::ObjectMutation::CreateTokenAndAttach { .. }
                )
        ) {
            return Err(EffectError::InconsistentProgram);
        }
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let controller = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?
            .controller();
        let current = self.evaluated_characteristics()?;
        let can_attach = |equipment: &ObjectCharacteristics, token: ObjectId| {
            current
                .get(token)
                .is_some_and(|card| card.zone == ObjectZone::Battlefield)
                && self.attachment_permitted(equipment, Some(TargetRef::Object(token)), &current)
        };
        let domain = if let Some(equipment) = cursor.chosen_token_equipment {
            let Some(equipment) = current.get(equipment) else {
                return Ok(None);
            };
            cursor
                .last_created_tokens
                .iter()
                .copied()
                .filter(|token| can_attach(equipment, *token))
                .collect::<Vec<_>>()
        } else {
            current
                .objects
                .values()
                .filter(|equipment| {
                    equipment.zone == ObjectZone::Battlefield
                        && equipment.controller == Some(controller)
                        && self.attachment_kind(equipment.object) == Some(AttachmentKind::Equipment)
                        && cursor
                            .last_created_tokens
                            .iter()
                            .any(|token| can_attach(equipment, *token))
                })
                .map(|equipment| equipment.object)
                .collect()
        };
        Ok((!domain.is_empty()).then(|| {
            ResolutionSelectionFrame::equipment(
                self.revision,
                cursor.entry,
                cursor.instruction,
                controller,
                domain,
                cursor.chosen_token_equipment.is_some(),
            )
        }))
    }
    pub(super) fn resume_token_equipment(
        &self,
        expected: u64,
        frame: &ResolutionSelectionFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.token_equipment_selection()?.as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if frame.chooser() != actor {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::SelectResolution(objects) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if objects.len() < frame.minimum()
            || objects.len() > 1
            || objects
                .iter()
                .any(|object| !frame.domain().contains(object))
        {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        let mut journal = RulesJournal::default();
        if let Some(equipment) = next
            .resolution_cursor()
            .and_then(|cursor| cursor.chosen_token_equipment)
        {
            let old = next.rules_snapshot(None)?;
            if let Some(change) = next.attach_at_cut(
                frame.entry(),
                equipment,
                TargetRef::Object(objects[0]),
                &old.view().evaluated,
            )? {
                next.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: frame.entry(),
                        instruction: frame.instruction(),
                    },
                    &old,
                    None,
                    &mut journal,
                    |_, _, _| Ok(vec![change]),
                )?;
            }
            let cursor = next
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?;
            cursor.chosen_token_equipment = None;
            cursor.last_created_tokens.clear();
            next.advance_instruction()?;
        } else if let Some(equipment) = objects.first() {
            next.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .chosen_token_equipment = Some(*equipment);
        } else {
            next.advance_instruction()?;
        }
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            journal,
        )
    }

    pub(super) fn attachment_permitted(
        &self,
        attachment: &ObjectCharacteristics,
        host: Option<TargetRef>,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        let Some(host) = host else { return false };
        if attachment.has_type(CardType::Battle)
            || Self::protected_target(
                host,
                &super::protection::ProtectionSource::from(attachment),
                evaluated,
            )
        {
            return false;
        }
        if host == TargetRef::Object(attachment.object)
            || (attachment.is_creature()
                && !(self.attachment_kind(attachment.object) == Some(AttachmentKind::Equipment)
                    && Self::has_reconfigure(attachment)))
        {
            return false;
        }
        match self.attachment_kind(attachment.object) {
            Some(AttachmentKind::Aura(enchant)) if attachment.has_type(CardType::Enchantment) => {
                attachment
                    .controller
                    .is_some_and(|actor| self.enchant_permits(enchant, host, actor, evaluated))
            }
            Some(kind @ (AttachmentKind::Equipment | AttachmentKind::Fortification))
                if attachment.has_type(CardType::Artifact) =>
            {
                let target = if kind == AttachmentKind::Equipment {
                    CardType::Creature
                } else {
                    CardType::Land
                };
                matches!(host,TargetRef::Object(object) if evaluated.get(object).is_some_and(|view| view.zone == ObjectZone::Battlefield && view.has_type(target)))
            }
            _ => false,
        }
    }
}
