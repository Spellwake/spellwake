//! Counter replacement choices are keyed to the exact would-be placement cut.
use super::*;

/// An affected controller must choose where the additional +1/+1 counter
/// is inserted among the applicable doubling replacements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterReplacementChoice {
    pub(super) chooser: SeatId,
    pub(super) object: ObjectId,
    pub(super) original: PowerToughnessCounters,
    pub(super) doublers: usize,
    pub(super) ordinal: usize,
}

impl CounterReplacementChoice {
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn object(&self) -> ObjectId {
        self.object
    }
    pub fn doublers(&self) -> usize {
        self.doublers
    }
    pub fn original(&self) -> &PowerToughnessCounters {
        &self.original
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CounterOrderCursor {
    choices: Vec<(CounterReplacementChoice, usize)>,
    consumed: usize,
}

impl EffectWindow {
    pub(crate) fn prepare_counter_replacements(
        &mut self,
        choices: Vec<(CounterReplacementChoice, usize)>,
    ) {
        self.counter_order = CounterOrderCursor {
            choices,
            consumed: 0,
        };
    }

    pub(crate) fn finish_counter_replacements(&mut self) -> Result<(), EffectError> {
        if self.counter_order.consumed != self.counter_order.choices.len() {
            return Err(EffectError::InconsistentProgram);
        }
        self.counter_order = CounterOrderCursor::default();
        Ok(())
    }

    pub(super) fn counter_replacement_order(
        &mut self,
        chooser: SeatId,
        object: ObjectId,
        original: PowerToughnessCounters,
        doublers: usize,
    ) -> Result<usize, EffectError> {
        let ordinal = self.counter_order.consumed;
        let frame = CounterReplacementChoice {
            chooser,
            object,
            original,
            doublers,
            ordinal,
        };
        let Some((expected, before_addition)) = self.counter_order.choices.get(ordinal) else {
            return Err(EffectError::CounterReplacementRequired(frame));
        };
        if expected != &frame || *before_addition > doublers {
            return Err(EffectError::InconsistentProgram);
        }
        self.counter_order.consumed = ordinal
            .checked_add(1)
            .ok_or(EffectError::InconsistentProgram)?;
        Ok(*before_addition)
    }
}
