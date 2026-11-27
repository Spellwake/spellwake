//! Army creation and growth are separate rules events within one resolution.

use super::*;
use crate::{
    characteristics::{
        ResolvedDuration, ResolvedPart, ResolvedParts, SubtypeOperation, TypeChange, TypeOperation,
    },
    occurrence::EventCause,
};

impl EffectWindow {
    pub(super) fn controlled_armies(
        &self,
        controller: SeatId,
        army: CreatureTypeId,
    ) -> Result<BTreeSet<ObjectId>, EffectError> {
        Ok(self
            .evaluated_characteristics()?
            .objects
            .values()
            .filter(|object| {
                object.zone == ObjectZone::Battlefield
                    && object.is_creature()
                    && object.controller == Some(controller)
                    && object.current_creature_subtypes.contains(&army)
            })
            .map(|object| object.object)
            .collect())
    }

    pub(super) fn grow_army(
        &mut self,
        cursor: &ResolutionCursor,
        controller: SeatId,
        object: ObjectId,
        [army, subtype]: [CreatureTypeId; 2],
        amount: u16,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let entry = self
            .window
            .resolving()
            .filter(|entry| entry.id() == cursor.entry && entry.controller() == controller)
            .ok_or(EffectError::InconsistentSelectionWork)?
            .clone();
        let state = self.evaluated_characteristics()?;
        let current = state
            .get(object)
            .filter(|permanent| {
                permanent.zone == ObjectZone::Battlefield
                    && permanent.is_creature()
                    && permanent.controller == Some(controller)
                    && permanent.current_creature_subtypes.contains(&army)
            })
            .ok_or(EffectError::InconsistentSelectionWork)?;
        let already_subtype = current.current_creature_subtypes.contains(&subtype);
        self.place_plus_one_counter_amount(
            vec![object],
            i64::from(amount).into(),
            EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            journal,
        )?;
        if !already_subtype {
            self.modify_objects_for_duration(
                &entry,
                cursor.instruction,
                [object].into(),
                ResolvedParts(vec![ResolvedPart::Type(TypeChange {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Keep,
                    subtypes: SubtypeOperation::Add([subtype].into()),
                })]),
                ResolvedDuration::NoStatedDuration,
                journal,
            )?;
        }
        Ok(())
    }
}
