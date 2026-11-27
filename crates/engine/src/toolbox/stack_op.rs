//! Universal composable `StackOp` primitive.
//!
//! Unifies counterspells (to graveyard, exile, library top/bottom, ward source,
//! triggering stack object, or all matching stack objects), stack object copies
//! (interactive target reselection or batch copy), target retargeting, and
//! gaining control of spells on the stack into one orthogonal primitive.

use crate::{
    object_identity::ObjectId,
    program::{
        CastCopyKeyword, DefinitionError, EndStepCast, SpellFilter, StackCopyTarget, ValueExpr,
    },
};
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
    /// Resolving source or self-driven keyword cast/copy action (cascade, discover, madness, encore, etc.).
    Source,
}

impl StackTargetSelector {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Targeted => true,
            Self::Triggering | Self::Matching { .. } | Self::Source => false,
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
    /// Encore (CR 702.141): create attacking token copies for each opponent.
    Encore,
    /// Cast a defeated Siege battle transformed from exile without paying its mana cost.
    CastDefeatedSiege,
    /// Copy the triggering spell on cast (storm, replicate, storm-like copy keywords).
    CopyOnCast(CastCopyKeyword),
    /// Copy a captured ability from resolution state.
    CopyCapturedAbility,
    /// Choose among found exiled spells (e.g. hideaway/cascade/discover flow).
    ChooseFoundSpell,
    /// Cast a specific found exiled spell by object ID.
    CastFoundSpell(ObjectId),
    /// Delayed end-step cast trigger (e.g. suspend/rebound/flashback-like end-step cast).
    EndStepCast {
        kind: EndStepCast,
        permanent: ObjectId,
        turn: u64,
    },
    /// Rebound cast (CR 702.88) from exile at upkeep.
    ReboundCast(ObjectId),
    /// Madness cast (CR 702.35) from exile.
    MadnessCast,
    /// Miracle cast (CR 702.94) of revealed drawn card.
    MiracleCast(ObjectId),
    /// Cascade (CR 702.85): exile until cheaper nonland spell, optionally cast free.
    Cascade,
    /// Cascade bottom step: put remaining exiled cards on bottom in random order.
    CascadeBottom,
    /// Discover N (CR 701.57): exile until nonland MV <= N, optionally cast free or put into hand.
    Discover { value: ValueExpr },
}

impl StackOpKind {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Copy { count, .. } | Self::Discover { value: count } => count.uses_target(),
            Self::Counter { .. }
            | Self::Retarget { .. }
            | Self::GainControl
            | Self::Encore
            | Self::CastDefeatedSiege
            | Self::CopyOnCast(_)
            | Self::CopyCapturedAbility
            | Self::ChooseFoundSpell
            | Self::CastFoundSpell(_)
            | Self::EndStepCast { .. }
            | Self::ReboundCast(_)
            | Self::MadnessCast
            | Self::MiracleCast(_)
            | Self::Cascade
            | Self::CascadeBottom => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        if let Self::Copy { count, .. } | Self::Discover { value: count } = self {
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

    /// True if this `StackOp` executes through the progress/choice/suspend/cascade runtime handlers.
    #[must_use]
    pub fn is_keyword_cast_or_copy_op(&self) -> bool {
        matches!(
            self.op,
            StackOpKind::Encore
                | StackOpKind::CastDefeatedSiege
                | StackOpKind::CopyOnCast(_)
                | StackOpKind::CopyCapturedAbility
                | StackOpKind::ChooseFoundSpell
                | StackOpKind::CastFoundSpell(_)
                | StackOpKind::EndStepCast { .. }
                | StackOpKind::ReboundCast(_)
                | StackOpKind::MadnessCast
                | StackOpKind::MiracleCast(_)
                | StackOpKind::Cascade
                | StackOpKind::CascadeBottom
                | StackOpKind::Discover { .. }
        )
    }

    #[must_use]
    pub fn encore() -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::Encore,
        }
    }

    #[must_use]
    pub fn cast_defeated_siege() -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::CastDefeatedSiege,
        }
    }

    #[must_use]
    pub fn copy_on_cast(kind: CastCopyKeyword) -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::CopyOnCast(kind),
        }
    }

    #[must_use]
    pub fn copy_captured_ability() -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::CopyCapturedAbility,
        }
    }

    #[must_use]
    pub fn choose_found_spell() -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::ChooseFoundSpell,
        }
    }

    #[must_use]
    pub fn cast_found_spell(object: ObjectId) -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::CastFoundSpell(object),
        }
    }

    #[must_use]
    pub fn end_step_cast(kind: EndStepCast, permanent: ObjectId, turn: u64) -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::EndStepCast {
                kind,
                permanent,
                turn,
            },
        }
    }

    #[must_use]
    pub fn rebound_cast(object: ObjectId) -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::ReboundCast(object),
        }
    }

    #[must_use]
    pub fn madness_cast() -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::MadnessCast,
        }
    }

    #[must_use]
    pub fn miracle_cast(object: ObjectId) -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::MiracleCast(object),
        }
    }

    #[must_use]
    pub fn cascade() -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::Cascade,
        }
    }

    #[must_use]
    pub fn cascade_bottom() -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::CascadeBottom,
        }
    }

    #[must_use]
    pub fn discover(value: ValueExpr) -> Self {
        Self {
            target: StackTargetSelector::Source,
            op: StackOpKind::Discover { value },
        }
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
