//! Universal `ApplyContinuous` primitive.
//!
//! Unifies temporary spell/ability continuous effects, battlefield layer grants,
//! dynamic P/T boosts, combat/cast/activation restrictions, cost modifiers,
//! damage prevention shields, turn/step skips, event replacements, and emblems
//! around a single orthogonal specification.

use crate::{
    characteristics::{
        KeywordAbility, ResolvedDuration, ResolvedPartsSpec, StaticContinuousAbility,
    },
    damage::TurnPreventionSpec,
    program::{
        BlockDeclarationRule, CombatActionKind, DefinitionError, ProtectionRecipients,
        TemporaryRuleKind, ValueExpr,
    },
    toolbox::{
        expr::ConditionExpr,
        query::{ObjectQuery, PlayerSelector},
    },
    turn::Step,
};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use std::collections::BTreeSet;

/// Orthogonal continuous, restriction, combat, prevention, or play-permission payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuousPayload {
    /// Static continuous ability (battlefield layer grant, static restriction, cost modifier, etc.).
    Static(Box<StaticContinuousAbility>),
    /// Temporary turn/duration rule (cant cast, cant activate, cost mod, zone replacement, etc.).
    TemporaryRule(Box<TemporaryRuleKind>),
    /// Resolved continuous effect parts applied to locked `selector` objects.
    ResolvedParts(Box<ResolvedPartsSpec>),
    /// Dynamic P/T modification + keyword grant/suppression evaluated at resolution time.
    DynamicBoost {
        power: ValueExpr,
        toughness: ValueExpr,
        keywords: BTreeSet<KeywordAbility>,
        suppress_keywords: BTreeSet<KeywordAbility>,
    },
    /// Double power and/or toughness of each selected creature for `duration`.
    DoublePt { power: bool, toughness: bool },
    /// Combat declaration requirement applied to selected objects.
    CombatAction(CombatActionKind),
    /// Grant play/cast permission on selected objects to `player`.
    PlayPermission {
        player: PlayerSelector,
        cast_only: bool,
        free: bool,
        any_color: bool,
    },
    /// Generalized damage-prevention shield for this turn.
    PreventDamage(Box<TurnPreventionSpec>),
    /// Next-N-damage prevention shield applied to the resolving target permanent or player.
    TargetPreventionShield {
        amount: spellwake_contracts::RulesInteger,
    },
    /// Skip `count` next turns for each player resolved by `player`.
    SkipTurns {
        player: PlayerSelector,
        count: ValueExpr,
    },
    /// Skip the next occurrence of `step` for each player resolved by `player`.
    SkipStep { player: PlayerSelector, step: Step },
    /// Create a command-zone emblem for each player selected by `as_emblem` (or controller).
    Emblem(Box<crate::emblem::EmblemDefinition>),
    /// Multiply damage dealt by sources controlled by resolving controller this turn.
    DamageMultiplier {
        factor: spellwake_contracts::RulesInteger,
    },
    /// Set block declaration constraints (e.g. only one blocker, menace, must block).
    BlockDeclarations(BlockDeclarationRule),
    /// Choose a color and grant protection from it to `recipients` for `spec.duration`.
    ChosenColorProtection { recipients: ProtectionRecipients },
    /// Prompt to modify any number of permanents matching target/filter with `parts` for `spec.duration`.
    ModifyAnyNumber { parts: Box<ResolvedPartsSpec> },
    /// Layer 1 copy effect copying `donor` onto `spec.selector` with optional CR 707.9 exceptions.
    BecomeCopy {
        donor: Box<ObjectQuery>,
        exceptions: Box<crate::program::CopyExceptions>,
    },
}

impl ContinuousPayload {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Static(_)
            | Self::ResolvedParts(_)
            | Self::DoublePt { .. }
            | Self::CombatAction(_)
            | Self::Emblem(_) => false,
            Self::TemporaryRule(rule) => rule.uses_target(),
            Self::PreventDamage(spec) => {
                use crate::damage::{PreventionRecipientSpec, PreventionSourceSpec};
                matches!(
                    spec.source,
                    PreventionSourceSpec::TargetPermanent
                        | PreventionSourceSpec::AllPermanentTargets
                        | PreventionSourceSpec::CreaturesOtherThanTarget
                        | PreventionSourceSpec::TargetSpell
                ) || matches!(spec.recipient, PreventionRecipientSpec::Target)
            }
            Self::DynamicBoost {
                power, toughness, ..
            } => power.uses_target() || toughness.uses_target(),
            Self::PlayPermission { player, .. } => player.uses_target(),
            Self::SkipTurns { player, count } => player.uses_target() || count.uses_target(),
            Self::SkipStep { player, .. } => player.uses_target(),
            Self::TargetPreventionShield { .. } | Self::ModifyAnyNumber { .. } => true,
            Self::DamageMultiplier { .. } | Self::BlockDeclarations(_) => false,
            Self::ChosenColorProtection { recipients } => match recipients {
                ProtectionRecipients::TargetPermanent => true,
                ProtectionRecipients::Matching(f) => {
                    f.uses_target_permanent() || f.has_target_player()
                }
                ProtectionRecipients::AbilitySource
                | ProtectionRecipients::ControllerAndPermanents => false,
            },
            Self::BecomeCopy { donor, .. } => donor.uses_target(),
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::DynamicBoost {
                power, toughness, ..
            } => {
                power
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                toughness
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)
            }
            Self::PlayPermission { player, .. } => player.validate_vocabulary(vocabulary),
            Self::SkipTurns { player, count } => {
                player.validate_vocabulary(vocabulary)?;
                count
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)
            }
            Self::SkipStep { player, .. } => player.validate_vocabulary(vocabulary),
            Self::Static(_)
            | Self::TemporaryRule(_)
            | Self::ResolvedParts(_)
            | Self::DoublePt { .. }
            | Self::CombatAction(_)
            | Self::PreventDamage(_)
            | Self::TargetPreventionShield { .. }
            | Self::Emblem(_)
            | Self::DamageMultiplier { .. }
            | Self::BlockDeclarations(_)
            | Self::ModifyAnyNumber { .. } => Ok(()),
            Self::ChosenColorProtection { recipients } => {
                if let ProtectionRecipients::Matching(filter) = recipients {
                    filter
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                Ok(())
            }
            Self::BecomeCopy { donor, exceptions } => {
                donor.validate_vocabulary(vocabulary)?;
                for sub in &exceptions.extra_creature_subtypes {
                    if vocabulary.label(*sub).is_none() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                }
                for tr in &exceptions.extra_triggered {
                    tr.validate_body(vocabulary)?;
                }
                for act in &exceptions.extra_activated {
                    act.body
                        .validate_in(vocabulary, crate::program::BodyContext::ACTIVATED)?;
                }
                Ok(())
            }
        }
    }
}

/// Specification for `Instruction::ApplyContinuous`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyContinuousSpec {
    /// Optional object set locked at resolution time (CR 611.2c).
    /// When `None`, the embedded rule's own selector applies dynamically.
    pub selector: Option<ObjectQuery>,
    /// The orthogonal continuous, restriction, cost, damage, or replacement payload.
    pub payload: ContinuousPayload,
    /// How long the effect persists.
    pub duration: ResolvedDuration,
    /// Optional live condition evaluated before applying the effect.
    pub condition: Option<ConditionExpr>,
    /// If true, consume this effect after its first application (one-shot shield/replacement/discount).
    pub one_shot: bool,
    /// If set, install as a permanent command-zone emblem controlled by the selected player(s).
    pub as_emblem: Option<PlayerSelector>,
    /// Human-readable emblem label when `as_emblem` is `Some`.
    pub emblem_label: Option<String>,
}

impl ApplyContinuousSpec {
    /// Construct a spec with a `StaticContinuousAbility` payload.
    #[must_use]
    pub fn static_rule(
        selector: Option<ObjectQuery>,
        rule: StaticContinuousAbility,
        duration: ResolvedDuration,
    ) -> Self {
        Self {
            selector,
            payload: ContinuousPayload::Static(Box::new(rule)),
            duration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Construct a spec with a `TemporaryRuleKind` payload.
    #[must_use]
    pub fn temporary_rule(rule: TemporaryRuleKind, duration: ResolvedDuration) -> Self {
        Self {
            selector: None,
            payload: ContinuousPayload::TemporaryRule(Box::new(rule)),
            duration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Construct a spec with a `ResolvedPartsSpec` payload on `selector`.
    #[must_use]
    pub fn resolved_parts(
        selector: ObjectQuery,
        parts: ResolvedPartsSpec,
        duration: ResolvedDuration,
    ) -> Self {
        Self {
            selector: Some(selector),
            payload: ContinuousPayload::ResolvedParts(Box::new(parts)),
            duration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Construct a dynamic P/T + keyword boost/suppression spec.
    #[must_use]
    pub fn dynamic_boost(
        selector: ObjectQuery,
        power: ValueExpr,
        toughness: ValueExpr,
        keywords: BTreeSet<KeywordAbility>,
        suppress_keywords: BTreeSet<KeywordAbility>,
        duration: ResolvedDuration,
    ) -> Self {
        Self {
            selector: Some(selector),
            payload: ContinuousPayload::DynamicBoost {
                power,
                toughness,
                keywords,
                suppress_keywords,
            },
            duration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Construct a double power and/or toughness spec.
    #[must_use]
    pub fn double_pt(
        selector: ObjectQuery,
        power: bool,
        toughness: bool,
        duration: ResolvedDuration,
    ) -> Self {
        Self {
            selector: Some(selector),
            payload: ContinuousPayload::DoublePt { power, toughness },
            duration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Construct a combat declaration requirement spec.
    #[must_use]
    pub fn combat_action(selector: ObjectQuery, kind: CombatActionKind) -> Self {
        Self {
            selector: Some(selector),
            payload: ContinuousPayload::CombatAction(kind),
            duration: ResolvedDuration::UntilEndOfTurn,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Construct a play/cast permission spec on `selector`.
    #[must_use]
    pub fn play_permission(
        selector: ObjectQuery,
        player: PlayerSelector,
        duration: ResolvedDuration,
        cast_only: bool,
        free: bool,
        any_color: bool,
    ) -> Self {
        Self {
            selector: Some(selector),
            payload: ContinuousPayload::PlayPermission {
                player,
                cast_only,
                free,
                any_color,
            },
            duration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Construct a damage-prevention shield spec.
    #[must_use]
    pub fn prevent_damage(spec: TurnPreventionSpec) -> Self {
        Self {
            selector: None,
            payload: ContinuousPayload::PreventDamage(Box::new(spec)),
            duration: ResolvedDuration::UntilEndOfTurn,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Construct a skip-turns spec.
    #[must_use]
    pub fn skip_turns(player: PlayerSelector, count: ValueExpr) -> Self {
        Self {
            selector: None,
            payload: ContinuousPayload::SkipTurns { player, count },
            duration: ResolvedDuration::NoStatedDuration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Construct a command-zone emblem creation spec for `players`.
    #[must_use]
    pub fn emblem(players: PlayerSelector, definition: crate::emblem::EmblemDefinition) -> Self {
        Self {
            selector: None,
            payload: ContinuousPayload::Emblem(Box::new(definition)),
            duration: ResolvedDuration::NoStatedDuration,
            condition: None,
            one_shot: false,
            as_emblem: Some(players),
            emblem_label: None,
        }
    }

    /// Construct a skip-step spec.
    #[must_use]
    pub fn skip_step(player: PlayerSelector, step: Step) -> Self {
        Self {
            selector: None,
            payload: ContinuousPayload::SkipStep { player, step },
            duration: ResolvedDuration::NoStatedDuration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Whether this spec depends on an announced target.
    #[must_use]
    pub fn damage_multiplier(factor: spellwake_contracts::RulesInteger) -> Self {
        Self {
            selector: None,
            payload: ContinuousPayload::DamageMultiplier { factor },
            duration: ResolvedDuration::UntilEndOfTurn,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    #[must_use]
    pub fn block_declarations(rule: BlockDeclarationRule) -> Self {
        Self {
            selector: None,
            payload: ContinuousPayload::BlockDeclarations(rule),
            duration: ResolvedDuration::UntilEndOfTurn,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    #[must_use]
    pub fn chosen_color_protection(
        recipients: ProtectionRecipients,
        duration: ResolvedDuration,
    ) -> Self {
        Self {
            selector: None,
            payload: ContinuousPayload::ChosenColorProtection { recipients },
            duration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    #[must_use]
    pub fn modify_any_number(parts: ResolvedPartsSpec, duration: ResolvedDuration) -> Self {
        Self {
            selector: None,
            payload: ContinuousPayload::ModifyAnyNumber {
                parts: Box::new(parts),
            },
            duration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    /// Construct a Layer 1 copy effect making `receiver` become a copy of `donor` for `duration`.
    #[must_use]
    pub fn become_copy(
        receiver: ObjectQuery,
        donor: ObjectQuery,
        duration: ResolvedDuration,
        exceptions: crate::program::CopyExceptions,
    ) -> Self {
        Self {
            selector: Some(receiver),
            payload: ContinuousPayload::BecomeCopy {
                donor: Box::new(donor),
                exceptions: Box::new(exceptions),
            },
            duration,
            condition: None,
            one_shot: false,
            as_emblem: None,
            emblem_label: None,
        }
    }

    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.selector.as_ref().is_some_and(ObjectQuery::uses_target)
            || self.payload.uses_target()
            || self
                .condition
                .as_ref()
                .is_some_and(ConditionExpr::uses_target)
            || self
                .as_emblem
                .as_ref()
                .is_some_and(PlayerSelector::uses_target)
    }

    /// Validate nested queries and conditions against `vocabulary`.
    ///
    /// # Errors
    /// Returns [`DefinitionError::InvalidInstruction`] if a nested query or condition is invalid.
    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        if let Some(q) = &self.selector {
            q.validate_vocabulary(vocabulary)?;
        }
        self.payload.validate_vocabulary(vocabulary)?;
        if let Some(c) = &self.condition {
            c.validate_vocabulary(vocabulary)?;
        }
        if let Some(p) = &self.as_emblem {
            p.validate_vocabulary(vocabulary)?;
        }
        Ok(())
    }
}
