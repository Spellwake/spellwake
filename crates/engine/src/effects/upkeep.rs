//! Upkeep payments use a normal stack trigger and a normal payment window.
use super::*;
use crate::stack_window::StackEntry;
use crate::{
    occurrence::{EventCause, RulesOccurrence},
    program::{TriggerEvent, TriggerKeywordEvent, UpkeepKeyword},
};

/// A token may enter with echo only after earlier upkeeps have passed. Inspect
/// the admitted templates as well as the physical cards when enabling history.
pub(super) fn needs_echo_history(program: &Program) -> bool {
    super::entry_counters::definition_or_token_matches(program, |p| {
        p.triggered.iter().any(|a| {
            a.condition.event
                == TriggerEvent::Keyword(TriggerKeywordEvent::Upkeep(UpkeepKeyword::Echo))
        })
    })
}
impl EffectWindow {
    pub(super) fn upkeep_price(
        &mut self,
        keyword: UpkeepKeyword,
        cost: &ManaCost,
        entry: &StackEntry,
        instruction: usize,
        append: &mut RulesJournal,
    ) -> Result<Option<ManaCost>, EffectError> {
        let trigger = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let source = trigger.source.object.id();
        if keyword == UpkeepKeyword::Echo {
            // This is historical acquisition by the captured player, including
            // when the source later changes controllers. A different incarnation
            // cannot be sacrificed on the unpaid branch.
            return Ok(trigger.source.echo_condition().then(|| cost.clone()));
        }
        if !self
            .window
            .cards()
            .get(source)
            .is_some_and(|o| o.zone() == ObjectZone::Battlefield)
        {
            return Ok(None);
        }
        if keyword == UpkeepKeyword::OtherCreaturesUpkeep {
            let evaluated = self.evaluated_characteristics()?;
            let count = evaluated
                .objects
                .values()
                .filter(|permanent| {
                    permanent.object != source
                        && permanent.zone == ObjectZone::Battlefield
                        && permanent.controller == Some(entry.controller())
                        && permanent.is_creature()
                })
                .count();
            let count = RulesInteger::from_usize(count);
            return Ok(Some(ManaCost {
                generic: cost.generic.times(&count),
                required: cost
                    .required
                    .iter()
                    .map(|(kind, n)| (*kind, n.times(&count)))
                    .collect(),
            }));
        }
        let snapshot = self.rules_snapshot(None)?;
        let before = self
            .characteristics
            .age
            .get(&source)
            .cloned()
            .unwrap_or_else(|| 0.into());
        let added = super::permanent_counters::effect_counter_multiplier(
            &snapshot.view().evaluated,
            source,
        );
        let after = before.plus(&added);
        self.characteristics.age.insert(source, after.clone());
        let price = ManaCost {
            generic: cost.generic.times(&after),
            required: cost
                .required
                .iter()
                .map(|(kind, n)| (*kind, n.times(&after)))
                .collect(),
        };
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &snapshot,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::AgeCounterPlaced {
                    object: source,
                    before,
                    after,
                }])
            },
        )?;
        Ok(Some(price))
    }
}
