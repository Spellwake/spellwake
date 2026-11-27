//! Typed references issued within one borrowed native source/viewer scope.
use super::*;
use spellwake_contracts::creature_types::CreatureTypeId;
use spellwake_engine::program::TriggeredAbilityId;
use spellwake_engine::{
    activated::ActivatedAbilityId,
    characteristics::{EvaluatedCharacteristics, ObjectCharacteristics, StaticAbilityId},
    checks::{CheckDecision, LegendConflictId},
    damage::{DamageApplication, DamageAttemptId, DamageSourceId},
    effects::{CopyTargetChoice, DamageDecision, OptionalEffectChoice},
    game::{DecisionId, RuleFrame},
    mana::{ManaAbilityRef, ManaType},
    object_identity::{ObjectId, ObjectZone},
    occurrence::TriggerInstanceId,
    opening::{MulliganChoice, OpeningDecision},
    stack_window::StackEntryId,
    token::TokenBatchMember,
};

/// Trusted native identities/options, not a serializable client grammar.
/// The issuer must choose player-visible/interactable references. Registration
/// proves current presence/frame membership, not disclosure or full legality.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NativeReference {
    Object(ObjectId),
    Zone(ObjectZone),
    Stack(StackEntryId),
    GrantedAbility {
        source: ObjectId,
        effect: spellwake_engine::characteristics::EffectInstanceKey,
    },
    ActivatedAbility {
        source: ObjectId,
        ability: ActivatedAbilityId,
    },
    ManaAbility {
        source: ObjectId,
        ability: ManaAbilityRef,
    },
    TriggeredAbility {
        source: ObjectId,
        ability: TriggeredAbilityId,
    },
    ContinuousAbility {
        source: ObjectId,
        ability: StaticAbilityId,
    },
    Decision(DecisionId),
    Legend(LegendConflictId),
    Trigger(TriggerInstanceId),
    TokenMember(TokenBatchMember),
    DamageApplication(DamageApplication),
    DamageAttempt(DamageAttemptId),
    DamageSource(DamageSourceId),
    DrawLandKind(Option<bool>),
    MiracleReveal(bool),
    PlaneswalkerType(spellwake_engine::program::PlaneswalkerSubtype),
    OptionalEffect(OptionalEffectChoice),
    EntryPayLife(bool),
    OptionIndex(u8),
    ZoneReplacement(usize),
    Mulligan(MulliganChoice),
    CopyTargets(CopyTargetChoice),
    ManaColor(ManaType),
    CreatureType(CreatureTypeId),
    CardName(spellwake_engine::land::CardNameIndex),
}
impl NativeScope<'_> {
    /// Issue an opaque handle only after current native identity/frame checks.
    /// Repeated issuance of the same typed value in this scope preserves its token.
    /// # Errors
    /// Rejects missing/stale objects/abilities and foreign decision/frame domains;
    /// rejected registration adds no registry entry. Native read errors stay typed.
    pub fn issue(&mut self, reference: NativeReference) -> Result<PlayerHandle, NativeBridgeError> {
        self.validate_reference(reference)?;
        self.register_validated(reference)
    }

    /// Return the already-issued opaque handle for `id` in this scope, if any.
    ///
    /// Never issues a new handle or widens snapshot visibility.
    pub(crate) fn handle_of_object(&self, id: ObjectId) -> Option<PlayerHandle> {
        self.references
            .iter()
            .find(|(_, value)| *value == NativeReference::Object(id))
            .map(|(handle, _)| handle.clone())
    }
    /// The projector owns this fresh evaluated cut for the same immutable source.
    /// No externally supplied or cached evaluation reaches this private method.
    pub(super) fn issue_evaluated(
        &mut self,
        reference: NativeReference,
        evaluated: &EvaluatedCharacteristics,
    ) -> Result<PlayerHandle, NativeBridgeError> {
        match reference {
            NativeReference::GrantedAbility { source, .. }
            | NativeReference::ActivatedAbility { source, .. }
            | NativeReference::ManaAbility { source, .. }
            | NativeReference::TriggeredAbility { source, .. }
            | NativeReference::ContinuousAbility { source, .. } => {
                if !evaluated
                    .get(source)
                    .is_some_and(|value| ability_matches(reference, value))
                {
                    return Err(NativeBridgeError::InvalidReference);
                }
            }
            _ => self.validate_reference(reference)?,
        }
        self.register_validated(reference)
    }
    fn register_validated(
        &mut self,
        reference: NativeReference,
    ) -> Result<PlayerHandle, NativeBridgeError> {
        if let Some(handle) = self
            .references
            .iter()
            .find(|(_, value)| *value == reference)
            .map(|(handle, _)| handle.clone())
        {
            if let Some(used) = &mut self.used_handles {
                used.insert(handle.clone());
            }
            return Ok(handle);
        }
        loop {
            let text = format!("n1.{}", Uuid::new_v4());
            let handle =
                PlayerHandle::try_from(text).map_err(|_| NativeBridgeError::InvalidReference)?;
            if self
                .references
                .iter()
                .any(|(existing, _)| *existing == handle)
            {
                continue;
            }
            self.references.push((handle.clone(), reference));
            if let Some(used) = &mut self.used_handles {
                used.insert(handle.clone());
            }
            return Ok(handle);
        }
    }
    pub(super) fn reference(
        &self,
        handle: &PlayerHandle,
    ) -> Result<NativeReference, NativeBridgeError> {
        self.references
            .iter()
            .find(|(issued, _)| issued == handle)
            .map(|(_, value)| *value)
            .ok_or(NativeBridgeError::InvalidReference)
    }
    pub(super) fn object(&self, handle: &PlayerHandle) -> Result<ObjectId, NativeBridgeError> {
        match self.reference(handle)? {
            NativeReference::Object(id) => Ok(id),
            _ => Err(NativeBridgeError::WrongReferenceKind),
        }
    }
    pub(super) fn stack(&self, handle: &PlayerHandle) -> Result<StackEntryId, NativeBridgeError> {
        match self.reference(handle)? {
            NativeReference::Stack(id) => Ok(id),
            _ => Err(NativeBridgeError::WrongReferenceKind),
        }
    }
    pub(super) fn frame(&self) -> Result<&RuleFrame, NativeBridgeError> {
        let decision = self
            .source
            .pending_decision()
            .ok_or(NativeBridgeError::WrongDecision)?;
        if decision.chooser() != self.binding.actor || decision.basis() != self.source.basis() {
            return Err(NativeBridgeError::WrongDecision);
        }
        Ok(decision.frame())
    }
    fn target_is_current(&self, target: spellwake_engine::targets::TargetRef) -> bool {
        use spellwake_engine::targets::TargetRef;
        match target {
            TargetRef::Object(id) => self.source.execution().window().cards().get(id).is_some(),
            TargetRef::Player(seat) => self
                .source
                .execution()
                .window()
                .participants()
                .contains(&seat),
            TargetRef::Stack(id) => self
                .source
                .execution()
                .window()
                .entries()
                .iter()
                .any(|entry| entry.id() == id),
        }
    }
    pub(super) fn validate_reference(
        &self,
        value: NativeReference,
    ) -> Result<(), NativeBridgeError> {
        let valid = match value {
            NativeReference::Object(id) => {
                self.source.execution().window().cards().get(id).is_some()
            }
            NativeReference::Zone(zone) => match zone {
                ObjectZone::Library(seat)
                | ObjectZone::Hand(seat)
                | ObjectZone::Graveyard(seat) => self
                    .binding
                    .seats
                    .iter()
                    .any(|entry| entry.native_seat == seat),
                ObjectZone::Battlefield
                | ObjectZone::Stack
                | ObjectZone::Exile
                | ObjectZone::Command => true,
            },
            NativeReference::Stack(id) => self
                .source
                .execution()
                .window()
                .entries()
                .iter()
                .any(|entry| entry.id() == id),
            NativeReference::GrantedAbility { source, .. }
            | NativeReference::ActivatedAbility { source, .. }
            | NativeReference::ManaAbility { source, .. }
            | NativeReference::TriggeredAbility { source, .. }
            | NativeReference::ContinuousAbility { source, .. } => self
                .source
                .execution()
                .current_characteristics(source)
                .map_err(|cause| NativeBridgeError::Observation(Box::new(cause)))?
                .is_some_and(|current| ability_matches(value, &current)),
            NativeReference::Decision(id) => {
                self.frame()?;
                self.source
                    .pending_decision()
                    .is_some_and(|decision| decision.id() == id)
            }
            NativeReference::Legend(id) => {
                matches!(self.frame()?, RuleFrame::Checks(CheckDecision::Legend(frame)) if frame.conflicts().iter().any(|conflict| conflict.id() == id))
            }
            NativeReference::Trigger(id) => {
                matches!(self.frame()?, RuleFrame::Triggers(frame) if frame.instances().contains(&id))
            }
            NativeReference::TokenMember(member) => {
                matches!(self.frame()?, RuleFrame::TimestampOrder(frame) if frame.members().iter().any(|entry| entry.member == member))
            }
            NativeReference::DamageApplication(application) => {
                matches!(self.frame()?, RuleFrame::Damage(DamageDecision::Order(frame)) if frame.applications().contains(&application))
            }
            NativeReference::DamageSource(source) => {
                matches!(self.frame()?, RuleFrame::DamageSourceChoice(frame) if frame.sources().contains(&source))
            }
            NativeReference::DamageAttempt(attempt) => {
                matches!(self.frame()?, RuleFrame::Damage(DamageDecision::Allocation(frame)) if frame.eligible().iter().any(|domain| domain.attempt() == attempt))
            }
            NativeReference::ZoneReplacement(index) => {
                matches!(self.frame()?,RuleFrame::ZoneReplacement(frame) if frame.reply_at(index).is_some())
            }
            NativeReference::MiracleReveal(_) => {
                matches!(self.frame()?, RuleFrame::DrawReplacement(frame)
                if matches!(frame.domain, spellwake_engine::effects::DrawReplacementDomain::Miracle { .. }))
            }
            NativeReference::DrawLandKind(_) => {
                matches!(self.frame()?,RuleFrame::DrawReplacement(frame) if matches!(frame.domain,spellwake_engine::effects::DrawReplacementDomain::LandKind))
            }
            NativeReference::PlaneswalkerType(_) => {
                matches!(
                    self.frame()?,
                    RuleFrame::PromptChoice(frame)
                        if matches!(
                            &frame.domain,
                            spellwake_engine::toolbox::ResolvedPromptDomain::PlaneswalkerType { .. }
                        )
                )
            }
            NativeReference::OptionalEffect(choice) => {
                matches!(self.frame()?, RuleFrame::OptionalEffect(frame) if frame.options().contains(&choice))
            }
            NativeReference::EntryPayLife(_) => {
                matches!(
                    self.frame()?,
                    RuleFrame::EntryChoice(frame) if matches!(frame.rule(), spellwake_engine::land::EntryTap::UnlessPayLife(_))
                ) || matches!(
                    self.frame()?,
                    RuleFrame::TokenEntryChoice(frame) if matches!(frame.rule(), spellwake_engine::land::EntryTap::UnlessPayLife(_))
                )
            }
            NativeReference::OptionIndex(index) => {
                matches!(self.frame()?, RuleFrame::OptionIndex(frame) if index < frame.count())
                    || matches!(self.frame()?, RuleFrame::PromptChoice(frame) if match &frame.domain {
                        spellwake_engine::toolbox::ResolvedPromptDomain::Options { labels, .. } => usize::from(index) < labels.len(),
                        spellwake_engine::toolbox::ResolvedPromptDomain::CardType { allowed } => usize::from(index) < allowed.len(),
                        spellwake_engine::toolbox::ResolvedPromptDomain::BasicLandType { allowed } => usize::from(index) < allowed.len(),
                        _ => false,
                    })
                    || matches!(self.frame()?, RuleFrame::EntryChoice(frame) if frame.rule().option_labels().is_some_and(|labels| usize::from(index) < labels.len()))
                    || matches!(self.frame()?, RuleFrame::TokenEntryChoice(frame) if frame.rule().option_labels().is_some_and(|labels| usize::from(index) < labels.len()))
            }
            NativeReference::Mulligan(choice) => {
                matches!(self.frame()?, RuleFrame::Opening(OpeningDecision::Declare { can_mulligan, .. }) if choice == MulliganChoice::Keep || *can_mulligan)
            }
            NativeReference::CopyTargets(choice) => {
                matches!(self.frame()?, RuleFrame::CopyTargets(_))
                    && match choice {
                        CopyTargetChoice::Retain => true,
                        CopyTargetChoice::Change(target) => self.target_is_current(target),
                    }
            }
            NativeReference::ManaColor(color) => {
                matches!(self.frame()?, RuleFrame::ManaColor(frame) if frame.colors().contains(&color))
                    || matches!(self.frame()?, RuleFrame::PromptChoice(frame) if matches!(&frame.domain, spellwake_engine::toolbox::ResolvedPromptDomain::Color { allowed } if allowed.contains(&color)))
                    || matches!(self.frame()?, RuleFrame::EntryChoice(frame) if match frame.rule() {
                        spellwake_engine::land::EntryTap::AlwaysChooseColorExcept(excluded)
                        | spellwake_engine::land::EntryTap::ChooseColorExcept(excluded) => color != excluded && color != ManaType::Colourless,
                        spellwake_engine::land::EntryTap::ChooseColor => color != ManaType::Colourless,
                        _ => false,
                    })
                    || matches!(self.frame()?, RuleFrame::TokenEntryChoice(frame) if match frame.rule() {
                        spellwake_engine::land::EntryTap::AlwaysChooseColorExcept(excluded)
                        | spellwake_engine::land::EntryTap::ChooseColorExcept(excluded) => color != excluded && color != ManaType::Colourless,
                        spellwake_engine::land::EntryTap::ChooseColor => color != ManaType::Colourless,
                        _ => false,
                    })
            }
            NativeReference::CardName(index) => {
                matches!(self.frame()?, RuleFrame::EntryChoice(frame) if frame.card_names().get(index.0).is_some())
                    || matches!(self.frame()?, RuleFrame::TokenEntryChoice(frame) if frame.card_names().get(index.0).is_some())
            }
            NativeReference::CreatureType(member) => {
                matches!(self.frame()?, RuleFrame::CreatureType(frame) if frame.vocabulary() == member.vocabulary() && !frame.excluding().contains(&member) && self.source.execution().creature_vocabulary().label(member).is_some())
                    || matches!(self.frame()?, RuleFrame::PromptChoice(frame) if matches!(&frame.domain, spellwake_engine::toolbox::ResolvedPromptDomain::CreatureType { allowed } if allowed.contains(&member)))
                    || matches!(self.frame()?, RuleFrame::EntryChoice(frame) if matches!(frame.rule(), spellwake_engine::land::EntryTap::ChooseCreatureType) && self.source.execution().creature_vocabulary().label(member).is_some())
                    || matches!(self.frame()?, RuleFrame::TokenEntryChoice(frame) if matches!(frame.rule(), spellwake_engine::land::EntryTap::ChooseCreatureType) && self.source.execution().creature_vocabulary().label(member).is_some())
            }
        };
        if valid {
            Ok(())
        } else {
            Err(NativeBridgeError::InvalidReference)
        }
    }
}

fn ability_matches(reference: NativeReference, current: &ObjectCharacteristics) -> bool {
    match reference {
        NativeReference::GrantedAbility { source, effect } => {
            current.object == source && current.granted_activated.contains_key(&effect)
        }
        NativeReference::ActivatedAbility { source, ability } => {
            current.object == source && current.abilities.activated.contains(&ability)
        }
        NativeReference::ManaAbility { source, ability } => {
            current.object == source && current.mana_abilities().contains(&ability)
        }
        NativeReference::TriggeredAbility { source, ability } => {
            current.object == source && current.abilities.triggered.contains(&ability)
        }
        NativeReference::ContinuousAbility { source, ability } => {
            current.object == source
                && (current.abilities.continuous == Some(ability)
                    || current
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ability))
        }
        _ => false,
    }
}
