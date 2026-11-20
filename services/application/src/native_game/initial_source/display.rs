//! Immutable physical provenance and guarded printed descriptions for player views.
//! This adapter supplies descriptions only; projection owns visibility and values.
use super::*;
use spellwake_engine::{
    characteristics::ObjectCharacteristics,
    object_identity::GameObject,
    occurrence::{ExecutionBindings, WaitingInstance},
    program::{ExecutableBody, Program},
    stack_window::{StackEntry, StackKind},
    token::TokenEntryPreview,
};

/// Game-scoped immutable descriptions borrowed from an accepted Initial and source.
/// Native roles are labeled as roles, never reconstructed English ability text.
/// Unknown copiable definitions fail rather than receiving a guessed card record.
pub struct NativeImmutableDisplay<'a> {
    programs: &'a NativeProgramSource,
    provenance: &'a NativeInitialProvenance,
    context: spellwake_engine::game::GameContext,
}
impl<'a> NativeImmutableDisplay<'a> {
    /// Compose the exact frozen rules source and physical Initial provenance.
    /// The projector independently checks the resulting game and rules context.
    /// This constructor is not admission, participant Auth or card qualification.
    ///
    /// # Errors
    /// Rejects an invalid retained release/profile or native source context.
    pub fn new(
        programs: &'a NativeProgramSource,
        provenance: &'a NativeInitialProvenance,
    ) -> Result<Self, NativeInitialError> {
        Ok(Self {
            programs,
            provenance,
            context: provenance.display_context(programs)?,
        })
    }
    fn description(
        &self,
        definition: &Program,
    ) -> Result<crate::player_wire::PlayerCharacteristics, NativeProjectionError> {
        if let Some(printed) = self
            .programs
            .printed_card(definition)
            .map_err(NativeProjectionError::metadata)?
        {
            return Ok(printed.characteristics().clone());
        }
        self.programs
            .token_description(definition)
            .map_err(NativeProjectionError::metadata)?
            .ok_or(NativeProjectionError::MissingDisplay)
    }
}
impl NativeDisplaySource for NativeImmutableDisplay<'_> {
    fn context(&self) -> &spellwake_engine::game::GameContext {
        &self.context
    }
    fn scope(&self) -> NativeDisplayScope {
        let (game_id, context_id) = self.provenance.scope();
        NativeDisplayScope::Game {
            game_id,
            context_id,
        }
    }
    fn object(
        &self,
        object: &GameObject,
        definition: &Program,
    ) -> Result<NativeObjectDisplay, NativeProjectionError> {
        let characteristics = self.description(definition)?;
        let physical = self.provenance.physical_card(object);
        // A physical card lacking its retained source is corruption, not a token.
        if object.card_id().is_some() && physical.is_none() {
            return Err(NativeProjectionError::MissingDisplay);
        }
        Ok(NativeObjectDisplay {
            definition_id: physical
                .as_ref()
                .map(NativePhysicalCardSource::definition_id),
            printing_id: physical
                .as_ref()
                .and_then(NativePhysicalCardSource::printing_id),
            characteristics,
        })
    }
    fn ability(
        &self,
        _: &GameObject,
        _: &ObjectCharacteristics,
        ability: NativeAbilityReference,
    ) -> Result<String, NativeProjectionError> {
        Ok(match ability {
            NativeAbilityReference::Mana(_) => "Mana ability",
            NativeAbilityReference::Activated(_) => "Activated ability",
            NativeAbilityReference::Triggered(_) => "Triggered ability",
            NativeAbilityReference::Continuous(_) => "Static ability",
        }
        .into())
    }
    fn stack(
        &self,
        entry: &StackEntry,
        _: Option<&ExecutableBody>,
        _: Option<&ExecutionBindings>,
    ) -> Result<String, NativeProjectionError> {
        Ok(match entry.kind() {
            StackKind::CardSpell(_) => "Card spell",
            StackKind::SpellCopy { .. } => "Spell copy",
            StackKind::ActivatedAbility { .. } => "Activated ability",
            StackKind::TriggeredAbility { .. } => "Triggered ability",
        }
        .into())
    }
    fn token_preview(&self, preview: &TokenEntryPreview) -> Result<String, NativeProjectionError> {
        // This is an ordering role, not an allocated object or a printing claim.
        Ok(self.description(&preview.definition)?.name)
    }
    fn trigger(&self, _: &WaitingInstance) -> Result<String, NativeProjectionError> {
        Ok("Triggered ability".into())
    }
    fn prevention_attempt(
        &self,
        attempt: &spellwake_engine::effects::DamageAllocationDomain,
    ) -> Result<String, NativeProjectionError> {
        // Only the permitted occurrence amount is rendered, never retained IDs.
        Ok(format!("{} damage", String::from(attempt.amount().clone())))
    }
}
