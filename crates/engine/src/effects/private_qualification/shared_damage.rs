//! The explicitly selected shared-processor unit has a real gate and private cursor.
use super::*;

pub(crate) struct QualificationDamageComponent {
    pub source: ObjectId,
    pub recipient: DamageRecipient,
    pub amount: RulesInteger,
}

impl EffectWindow {
    pub(crate) fn qualification_prepare_resolution_cursor(
        &self,
        entry: StackEntryId,
        instruction: usize,
    ) -> Result<(Self, StackWindow), EffectError> {
        if self.pending_work.is_some() || instruction != 0 {
            return Err(EffectError::WrongPhase);
        }
        let current = self
            .window
            .checking_targets()
            .ok_or(EffectError::WrongPhase)?;
        if current.id() != entry {
            return Err(EffectError::WrongResolutionFrame);
        }
        let body = self
            .payloads
            .get(&entry)
            .and_then(StackPayload::body)
            .ok_or(EffectError::InconsistentProgram)?;
        let [bound] = current.targets().targets() else {
            return Err(EffectError::InconsistentProgram);
        };
        let gate = self.window.check_resolution_targets(
            self.window.revision(),
            entry,
            vec![TargetCheck {
                target: bound.choice().id,
                legal: self.legal_body_target(body, bound.choice().target, current.controller())?,
            }],
        )?;
        if gate
            .target_assessment()
            .ok_or(EffectError::InconsistentProgram)?
            .permission()
            != ResolutionPermission::SomeTargetsLegal
        {
            return Err(EffectError::IllegalTarget);
        }
        // Only this disclosed test fixture installs an intermediate cursor. The
        // public driver continues through begin_resolution without exposing it.
        let mut next = self.clone();
        next.window = gate.clone();
        next.pending_work = Some(PendingWork::Resolution(ResolutionCursor {
            revealed_piles: None,
            chosen_opponent: None,
            chosen_damage_recipient: None,
            damage_offer_opponents: None,
            vowed_creatures: Default::default(),
            meld_pair: None,
            manifest_remaining: None,
            random_selection: None,
            results: ResolutionResults::default(),
            chosen_effect_color: None,
            chosen_option_index: None,
            chosen_distinct_colors: Vec::new(),
            chosen_mana_split: None,
            tempting_token_offer: None,
            destroyed_permanents: None,
            chosen_loyalty_removals: None,
            discarded_nonland_mana_value: None,
            last_created_tokens: Vec::new(),
            chosen_token_equipment: None,
            found_spells: Vec::new(),
            largest_discard: None,
            impulse: ImpulseState::default(),
            entry,
            instruction,
            exiled_target: None,
            remembered_permanent: None,
            values: BTreeMap::new(),
        }));
        Ok((next, gate))
    }
    pub(crate) fn qualification_begin_shared_damage(
        &self,
        entry: StackEntryId,
        instruction: usize,
        components: &[QualificationDamageComponent],
    ) -> Result<RuleProgress, EffectError> {
        let Some(PendingWork::Resolution(cursor)) = &self.pending_work else {
            return Err(EffectError::WrongPhase);
        };
        if cursor.entry != entry || cursor.instruction != instruction {
            return Err(EffectError::WrongResolutionFrame);
        }
        let inputs = components
            .iter()
            .map(|value| {
                Ok(DamageInput {
                    source: self.combat_damage_source(value.source)?,
                    target: value.recipient,
                    amount: value.amount.clone(),
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        self.clone().begin_damage_batch(
            DamageOrigin::Resolution { entry, instruction },
            DamageKind::Noncombat,
            inputs,
            (DamageJournal::default(), DrawJournal::default()),
            vec![],
            vec![],
        )
    }
}
