//! Typed fail-closed lowering errors for `v2::ir -> spellwake-engine::Program`.

use spellwake_engine::program::DefinitionError;

/// Honest, fail-closed error returned whenever a `v2::ir` construct cannot be faithfully lowered
/// into the consolidated Universal Rules Toolbox or fails `Program::validate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweringError {
    /// Stage-1 `v2` parser did not strictly compile one or more faces.
    ParseIncomplete,
    /// A typed `v2::ir` construct has no faithful native engine representation yet.
    Unsupported(&'static str),
    /// Printed card metadata (`type_line`, `mana_cost`, `power`/`toughness`, layout) is invalid or unsupported.
    InvalidMetadata(&'static str),
    /// The lowered [`spellwake_engine::effects::Program`] failed `Program::validate`.
    EngineValidation(DefinitionError),
}

impl LoweringError {
    /// Concise, deterministic blocker tag suitable for corpus diagnostics and telemetry.
    #[must_use]
    pub const fn reason_code(&self) -> &'static str {
        match self {
            Self::ParseIncomplete => "v2:ast-parse-incomplete",
            Self::Unsupported(code) | Self::InvalidMetadata(code) => code,
            Self::EngineValidation(err) => match err {
                DefinitionError::UnsupportedPlaneswalker => "v2:validate:unsupported-planeswalker",
                DefinitionError::UnsupportedZoneReplacementScope => {
                    "v2:validate:unsupported-zone-replacement-scope"
                }
                DefinitionError::InvalidZoneReplacementDefinition => {
                    "v2:validate:invalid-zone-replacement"
                }
                DefinitionError::TokenDefinition => "v2:validate:token-definition",
                DefinitionError::ColorIndicator => "v2:validate:color-indicator",
                DefinitionError::CardTypes => "v2:validate:card-types",
                DefinitionError::CastingCost => "v2:validate:casting-cost",
                DefinitionError::ManaSource => "v2:validate:mana-source",
                DefinitionError::ActivatedSource => "v2:validate:activated-source",
                DefinitionError::TriggeredSource => "v2:validate:triggered-source",
                DefinitionError::DuplicateAbility => "v2:validate:duplicate-ability",
                DefinitionError::EmptyBody => "v2:validate:empty-body",
                DefinitionError::InvalidInstruction => "v2:validate:invalid-instruction",
                DefinitionError::InvalidControlFlow => "v2:validate:invalid-control-flow",
                DefinitionError::UnboundTriggerCondition => "v2:validate:unbound-trigger-condition",
                DefinitionError::UnboundResolvingPlayer => "v2:validate:unbound-resolving-player",
                DefinitionError::UnguaranteedRelatedPlayer => {
                    "v2:validate:unguaranteed-related-player"
                }
                DefinitionError::ResolvingPlayerTarget => "v2:validate:resolving-player-target",
                DefinitionError::MissingOptionalEffect => "v2:validate:missing-optional-effect",
                DefinitionError::UnsupportedOptionalEffect => {
                    "v2:validate:unsupported-optional-effect"
                }
                DefinitionError::InvalidResolutionPaymentCost => {
                    "v2:validate:invalid-resolution-payment-cost"
                }
                DefinitionError::OperandFamily => "v2:validate:operand-family",
                DefinitionError::PhantomTarget => "v2:validate:phantom-target",
                DefinitionError::TargetlessActivation => "v2:validate:targetless-activation",
                DefinitionError::InvalidCreatureSubtypes => "v2:validate:invalid-creature-subtypes",
                DefinitionError::InvalidLandSubtypes => "v2:validate:invalid-land-subtypes",
                DefinitionError::ForeignCreatureType => "v2:validate:foreign-creature-type",
                DefinitionError::DuplicateCreatureTypeSlot => {
                    "v2:validate:duplicate-creature-type-slot"
                }
                DefinitionError::UndefinedCreatureTypeSlot => {
                    "v2:validate:undefined-creature-type-slot"
                }
                DefinitionError::UnboundTriggerAmount => "v2:validate:unbound-trigger-amount",
                DefinitionError::EmptyCreatureTypeDomain => {
                    "v2:validate:empty-creature-type-domain"
                }
                DefinitionError::ContinuousEffect => "v2:validate:continuous-effect",
            },
        }
    }
}

impl std::fmt::Display for LoweringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason_code())
    }
}

impl std::error::Error for LoweringError {}

impl From<DefinitionError> for LoweringError {
    fn from(value: DefinitionError) -> Self {
        Self::EngineValidation(value)
    }
}

/// Result alias for `v2_lowering`.
pub type LowerResult<T> = Result<T, LoweringError>;
