//! Universal composable `StackOp` primitive.
//!
//! Unifies counterspells (to graveyard, exile, library top/bottom, ward source,
//! triggering stack object, or all matching stack objects), stack object copies
//! (interactive target reselection or batch copy), target retargeting, and
//! gaining control of spells on the stack into one orthogonal primitive.

use crate::program::{DefinitionError, SpellFilter, StackCopyTarget, ValueExpr};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;

/// Selects which stack object(s) a [`StackOpSpec`] operates on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackTargetSelector {
    /// The stack target announced for the resolving spell or ability.
    Targeted,
    /// The stack object that caused this triggered ability to trigger.
    Triggering,
    /// Every stack object matching the filter (spells and/or abilities).
    Matching {
        filter: Option<Box<SpellFilter>>,
        include_spells: bool,
        include_abilities: bool,
        opponents_only: bool,
    },
    /// Legacy/general copy selector (`TargetSpell`, `TriggeringSpell`, `ResolvingSpell`, etc.).
    CopyTarget(StackCopyTarget),
}

impl StackTargetSelector {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Targeted => true,
            Self::Triggering | Self::Matching { .. } => false,
            Self::CopyTarget(target) => matches!(
                target,
                StackCopyTarget::TargetSpell
                    | StackCopyTarget::TargetAbility
                    | StackCopyTarget::TargetSpellOrAbility
            ),
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        if let Self::Matching {
            filter: Some(spell_filter),
            ..
        } = self
        {
            spell_filter.object.validate_vocabulary(vocabulary)?;
        }
        Ok(())
    }
}

/// Destination for countered card spells (CR 701.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterDestination {
    Graveyard,
    Exile,
    Library { top: bool },
}

/// Orthogonal stack manipulation operation executed by [`StackOpSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackOpKind {
    /// Counter selected stack object(s), moving countered card spells to `destination`.
    /// When `ward_source` is true, counters the triggering spell/ability unless ward was paid.
    Counter {
        destination: CounterDestination,
        ward_source: bool,
    },
    /// Copy selected stack object(s) `count` times.
    /// When `interactive_retarget` is true, prompts for new targets via `CopyResolution`.
    Copy {
        count: ValueExpr,
        may_choose_new_targets: bool,
        interactive_retarget: bool,
        ability_mode: bool,
    },
    /// Change targets of a targeted stack object.
    /// When `single_target_only` is true, only applies if the stack object has a single target.
    Retarget { single_target_only: bool },
    /// Controller of the resolving effect gains control of the targeted spell on the stack.
    GainControl,
}

impl StackOpKind {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Copy { count, .. } => count.uses_target(),
            Self::Counter { .. } | Self::Retarget { .. } | Self::GainControl => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        if let Self::Copy { count, .. } = self {
            count
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction)?;
        }
        Ok(())
    }
}

/// Complete universal stack-operation specification (`Instruction::StackOp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackOpSpec {
    pub target: StackTargetSelector,
    pub op: StackOpKind,
}

impl StackOpSpec {
    /// Counter target spell or ability, moving countered spells to `destination`.
    #[must_use]
    pub fn counter_targeted(destination: CounterDestination) -> Self {
        Self {
            target: StackTargetSelector::Targeted,
            op: StackOpKind::Counter {
                destination,
                ward_source: false,
            },
        }
    }

    /// Counter every stack object matching the given filter criteria.
    #[must_use]
    pub fn counter_matching(
        filter: Option<Box<SpellFilter>>,
        include_spells: bool,
        include_abilities: bool,
        opponents_only: bool,
    ) -> Self {
        Self {
            target: StackTargetSelector::Matching {
                filter,
                include_spells,
                include_abilities,
                opponents_only,
            },
            op: StackOpKind::Counter {
                destination: CounterDestination::Graveyard,
                ward_source: false,
            },
        }
    }

    /// Counter the stack object that caused this triggered ability to trigger.
    #[must_use]
    pub fn counter_triggering() -> Self {
        Self {
            target: StackTargetSelector::Triggering,
            op: StackOpKind::Counter {
                destination: CounterDestination::Graveyard,
                ward_source: false,
            },
        }
    }

    /// Counter the triggering spell/ability unless its controller pays the ward cost.
    #[must_use]
    pub fn counter_ward_source() -> Self {
        Self {
            target: StackTargetSelector::Triggering,
            op: StackOpKind::Counter {
                destination: CounterDestination::Graveyard,
                ward_source: true,
            },
        }
    }

    /// Copy target spell with interactive target reselection.
    #[must_use]
    pub fn copy_targeted_spell() -> Self {
        Self {
            target: StackTargetSelector::Targeted,
            op: StackOpKind::Copy {
                count: ValueExpr::Literal(1.into()),
                may_choose_new_targets: true,
                interactive_retarget: true,
                ability_mode: false,
            },
        }
    }

    /// Copy target activated or triggered ability with interactive target reselection.
    #[must_use]
    pub fn copy_targeted_ability() -> Self {
        Self {
            target: StackTargetSelector::Targeted,
            op: StackOpKind::Copy {
                count: ValueExpr::Literal(1.into()),
                may_choose_new_targets: true,
                interactive_retarget: true,
                ability_mode: true,
            },
        }
    }

    /// Copy a stack object `count` times (batch copy without interactive retarget frame).
    #[must_use]
    pub fn copy_stack(
        target: StackCopyTarget,
        count: ValueExpr,
        may_choose_new_targets: bool,
    ) -> Self {
        Self {
            target: StackTargetSelector::CopyTarget(target),
            op: StackOpKind::Copy {
                count,
                may_choose_new_targets,
                interactive_retarget: false,
                ability_mode: false,
            },
        }
    }

    /// Choose new targets for target spell or ability (`single_target_only` = ChangeOnlyTarget).
    #[must_use]
    pub fn retarget(single_target_only: bool) -> Self {
        Self {
            target: StackTargetSelector::Targeted,
            op: StackOpKind::Retarget { single_target_only },
        }
    }

    /// Gain control of target spell on the stack.
    #[must_use]
    pub fn gain_control() -> Self {
        Self {
            target: StackTargetSelector::Targeted,
            op: StackOpKind::GainControl,
        }
    }

    /// True if this `StackOp` uses the interactive `CopyResolution` target-reselection frame.
    #[must_use]
    pub fn is_interactive_copy_or_retarget(&self) -> bool {
        matches!(
            (&self.target, &self.op),
            (
                StackTargetSelector::Targeted,
                StackOpKind::Copy {
                    interactive_retarget: true,
                    ..
                } | StackOpKind::Retarget { .. }
            )
        )
    }

    /// True if this `StackOp` requires a bound `TargetRef::Stack` on the resolving entry.
    #[must_use]
    pub fn requires_stack_target(&self) -> bool {
        matches!(self.target, StackTargetSelector::Targeted)
    }

    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.target.uses_target() || self.op.uses_target()
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.target.validate_vocabulary(vocabulary)?;
        self.op.validate_vocabulary(vocabulary)
    }
}
