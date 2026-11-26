//! Individual-turn control flow (CR 103.8, 500–514).
//!
//! Outcomes are trusted kernel notifications, never player commands or proof that
//! drawing/combat/trigger processing happened. Active seat and order stay in StackWindow.
use spellwake_contracts::RulesInteger;

/// A main phase has no steps; these positions include those two phase boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    Untap,
    Upkeep,
    Draw,
    PrecombatMain,
    BeginningCombat,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
    EndCombat,
    PostcombatMain,
    End,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Beginning,
    PrecombatMain,
    Combat,
    PostcombatMain,
    Ending,
}
impl Step {
    pub fn phase(self) -> Phase {
        match self {
            Self::Untap | Self::Upkeep | Self::Draw => Phase::Beginning,
            Self::PrecombatMain => Phase::PrecombatMain,
            Self::BeginningCombat
            | Self::DeclareAttackers
            | Self::DeclareBlockers
            | Self::CombatDamage
            | Self::EndCombat => Phase::Combat,
            Self::PostcombatMain => Phase::PostcombatMain,
            Self::End | Self::Cleanup => Phase::Ending,
        }
    }
}

/// Required rule work before priority or before the next no-priority step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnWork {
    Untap,
    Draw,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage { additional: bool },
    Cleanup,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStage {
    Work(TurnWork),
    Priority,
}

/// Prepared native fixture entry, not deck setup or a recovery checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnStart {
    pub number: u64,
    pub step: Step,
}
impl Default for TurnStart {
    fn default() -> Self {
        Self {
            number: 1,
            step: Step::Untap,
        }
    }
}

/// A current boundary identity; repeated damage and cleanup steps stay distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnPosition {
    pub number: u64,
    pub step: Step,
    pub cleanup_iteration: u64,
    pub additional_damage: bool,
}

/// Report actual work, not a permission to skip it. Cleanup includes discard,
/// simultaneous damage/EOT cleanup, then repeated SBA and waiting-trigger handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkCompleted {
    Untap,
    Draw,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage { additional_step_required: bool },
    Cleanup { priority_required: bool },
}

/// The attacker fact is combat history through this priority round, including
/// creatures put onto the battlefield attacking, even if they have since left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityCompleted {
    Ordinary,
    Attackers { declared_or_put_attacking: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnError {
    LandLimitReached,
    InvalidStart,
    WorkRequired,
    PriorityRequired,
    WrongWork,
    WrongPriorityOutcome,
    TurnExhausted,
    CleanupExhausted,
}

/// Only turn control state is stored here; no players, objects, mana or stack copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnScheduler {
    lands_played: RulesInteger,
    position: TurnPosition,
    stage: TurnStage,
    skip_first_draw: bool,
    additional_damage_required: bool,
}
impl TurnScheduler {
    /// Private operational fault at an already reached cleanup-priority cut.
    #[cfg(test)]
    pub(crate) fn qualification_set_cleanup_iteration(
        &mut self,
        value: u64,
    ) -> Result<(), TurnError> {
        if self.position.step != Step::Cleanup || self.stage != TurnStage::Priority {
            return Err(TurnError::WrongPriorityOutcome);
        }
        self.position.cleanup_iteration = value;
        Ok(())
    }
    pub fn new(start: TurnStart, skip_first_draw: bool) -> Result<Self, TurnError> {
        if start.number == 0 || (start.number == 1 && start.step == Step::Draw && skip_first_draw) {
            return Err(TurnError::InvalidStart);
        }
        let mut state = Self {
            lands_played: 0.into(),
            position: TurnPosition {
                number: start.number,
                step: start.step,
                cleanup_iteration: 0,
                additional_damage: false,
            },
            stage: TurnStage::Priority,
            skip_first_draw,
            additional_damage_required: false,
        };
        state.enter(start.step);
        Ok(state)
    }
    /// CR 724.1d skips all intervening steps and can begin a repeat cleanup.
    pub(crate) fn jump_to_cleanup(&self) -> Result<Self, TurnError> {
        let mut next = self.clone();
        if self.position.step == Step::Cleanup {
            next.position.cleanup_iteration = self
                .position
                .cleanup_iteration
                .checked_add(1)
                .ok_or(TurnError::CleanupExhausted)?;
        }
        next.additional_damage_required = false;
        next.position.additional_damage = false;
        next.enter(Step::Cleanup);
        Ok(next)
    }
    pub(crate) fn begin_additional_combat(mut self) -> Self {
        self.enter(Step::BeginningCombat);
        self
    }
    pub fn position(&self) -> TurnPosition {
        self.position
    }
    pub fn stage(&self) -> TurnStage {
        self.stage
    }
    /// Actual special-action plays in this individual turn, not battlefield entries.
    pub fn lands_played(&self) -> &RulesInteger {
        &self.lands_played
    }
    /// The root separately establishes ordinary timing and the physical land play.
    pub(crate) fn record_land_play(&self, allowance: &RulesInteger) -> Result<Self, TurnError> {
        if &self.lands_played >= allowance {
            return Err(TurnError::LandLimitReached);
        }
        let mut next = self.clone();
        next.lands_played = self.lands_played.plus(&1.into());
        Ok(next)
    }
    fn enter(&mut self, step: Step) {
        self.position.step = step;
        self.stage = match step {
            Step::Untap => TurnStage::Work(TurnWork::Untap),
            Step::Draw => TurnStage::Work(TurnWork::Draw),
            Step::DeclareAttackers => TurnStage::Work(TurnWork::DeclareAttackers),
            Step::DeclareBlockers => TurnStage::Work(TurnWork::DeclareBlockers),
            Step::CombatDamage => TurnStage::Work(TurnWork::CombatDamage {
                additional: self.position.additional_damage,
            }),
            Step::Cleanup => TurnStage::Work(TurnWork::Cleanup),
            _ => TurnStage::Priority,
        };
    }
    /// Trusted continuation after actual mandatory work; rejection preserves self.
    pub fn complete_work(&self, result: WorkCompleted) -> Result<Self, TurnError> {
        let TurnStage::Work(work) = self.stage else {
            return Err(TurnError::PriorityRequired);
        };
        let mut next = self.clone();
        match (work, result) {
            (TurnWork::Untap, WorkCompleted::Untap) => next.enter(Step::Upkeep),
            (TurnWork::Draw, WorkCompleted::Draw)
            | (TurnWork::DeclareAttackers, WorkCompleted::DeclareAttackers)
            | (TurnWork::DeclareBlockers, WorkCompleted::DeclareBlockers) => {
                next.stage = TurnStage::Priority
            }
            (
                TurnWork::CombatDamage { additional },
                WorkCompleted::CombatDamage {
                    additional_step_required,
                },
            ) => {
                if additional && additional_step_required {
                    return Err(TurnError::WrongWork);
                }
                next.additional_damage_required = additional_step_required;
                next.stage = TurnStage::Priority;
            }
            (
                TurnWork::Cleanup,
                WorkCompleted::Cleanup {
                    priority_required: true,
                },
            ) => next.stage = TurnStage::Priority,
            (
                TurnWork::Cleanup,
                WorkCompleted::Cleanup {
                    priority_required: false,
                },
            ) => {
                next.position.number = self
                    .position
                    .number
                    .checked_add(1)
                    .ok_or(TurnError::TurnExhausted)?;
                next.position.cleanup_iteration = 0;
                next.position.additional_damage = false;
                next.lands_played = 0.into();
                next.enter(Step::Untap);
            }
            _ => return Err(TurnError::WrongWork),
        }
        Ok(next)
    }
    /// Advance past priority, skipping any consecutive steps for which `should_skip` returns true.
    pub(crate) fn complete_priority_with_step_skip(
        &self,
        result: PriorityCompleted,
        skip_draw: bool,
        mut should_skip: impl FnMut(Step) -> bool,
    ) -> Result<Self, TurnError> {
        let mut next = self.complete_priority_with_draw_skip(result, skip_draw)?;
        for _ in 0..8 {
            let step = next.position.step;
            if matches!(step, Step::Cleanup) || !should_skip(step) {
                break;
            }
            let successor = match step {
                Step::Untap => Step::Upkeep,
                Step::Upkeep => Step::Draw,
                Step::Draw => Step::PrecombatMain,
                Step::PrecombatMain => Step::BeginningCombat,
                Step::BeginningCombat
                | Step::DeclareAttackers
                | Step::DeclareBlockers
                | Step::CombatDamage
                | Step::EndCombat => Step::PostcombatMain,
                Step::PostcombatMain => Step::End,
                Step::End => Step::Cleanup,
                Step::Cleanup => break,
            };
            next.enter(successor);
        }
        Ok(next)
    }

    /// Called only after an actual empty-stack consecutive-pass round. Resolving
    /// an entry never calls this method. The enclosing GameState enforces that gate.
    pub fn complete_priority(&self, result: PriorityCompleted) -> Result<Self, TurnError> {
        self.complete_priority_with_draw_skip(result, false)
    }
    pub(crate) fn complete_priority_with_draw_skip(
        &self,
        result: PriorityCompleted,
        skip_draw: bool,
    ) -> Result<Self, TurnError> {
        if self.stage != TurnStage::Priority {
            return Err(TurnError::WorkRequired);
        }
        if (self.position.step == Step::DeclareAttackers)
            != matches!(result, PriorityCompleted::Attackers { .. })
        {
            return Err(TurnError::WrongPriorityOutcome);
        }
        let mut next = self.clone();
        match self.position.step {
            Step::Upkeep if skip_draw || self.position.number == 1 && self.skip_first_draw => {
                next.enter(Step::PrecombatMain)
            }
            Step::Upkeep => next.enter(Step::Draw),
            Step::Draw => next.enter(Step::PrecombatMain),
            Step::PrecombatMain => next.enter(Step::BeginningCombat),
            Step::BeginningCombat => next.enter(Step::DeclareAttackers),
            Step::DeclareAttackers => match result {
                PriorityCompleted::Attackers {
                    declared_or_put_attacking: true,
                } => next.enter(Step::DeclareBlockers),
                _ => next.enter(Step::EndCombat),
            },
            Step::DeclareBlockers => next.enter(Step::CombatDamage),
            Step::CombatDamage if self.additional_damage_required => {
                next.position.additional_damage = true;
                next.additional_damage_required = false;
                next.enter(Step::CombatDamage);
            }
            Step::CombatDamage => {
                next.position.additional_damage = false;
                next.enter(Step::EndCombat);
            }
            Step::EndCombat => next.enter(Step::PostcombatMain),
            Step::PostcombatMain => next.enter(Step::End),
            Step::End => next.enter(Step::Cleanup),
            Step::Cleanup => {
                next.position.cleanup_iteration = self
                    .position
                    .cleanup_iteration
                    .checked_add(1)
                    .ok_or(TurnError::CleanupExhausted)?;
                next.enter(Step::Cleanup);
            }
            Step::Untap => return Err(TurnError::WrongPriorityOutcome),
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_counter_exhaustion_keeps_the_entire_schedule() {
        let mut schedule = TurnScheduler::new(
            TurnStart {
                number: 3,
                step: Step::Cleanup,
            },
            false,
        )
        .unwrap();
        schedule = schedule
            .complete_work(WorkCompleted::Cleanup {
                priority_required: true,
            })
            .unwrap();
        schedule.position.cleanup_iteration = u64::MAX;
        let before = schedule.clone();
        assert_eq!(
            schedule.complete_priority(PriorityCompleted::Ordinary),
            Err(TurnError::CleanupExhausted)
        );
        assert_eq!(schedule, before);
    }

    #[test]
    fn additional_damage_cannot_recursively_request_another_damage_step() {
        let schedule = TurnScheduler::new(
            TurnStart {
                number: 3,
                step: Step::CombatDamage,
            },
            false,
        )
        .unwrap()
        .complete_work(WorkCompleted::CombatDamage {
            additional_step_required: true,
        })
        .unwrap()
        .complete_priority(PriorityCompleted::Ordinary)
        .unwrap();
        let before = schedule.clone();
        assert_eq!(
            schedule.complete_work(WorkCompleted::CombatDamage {
                additional_step_required: true
            }),
            Err(TurnError::WrongWork)
        );
        assert_eq!(schedule, before);
    }
}
