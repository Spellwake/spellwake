//! Global day/night designation (CR 731). It persists if every werewolf leaves.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DayNight {
    #[default]
    Neither,
    Day,
    Night,
}
impl DayNight {
    pub(crate) fn untap(self, previous_active_player_spells: u8) -> Option<Self> {
        match (self, previous_active_player_spells) {
            (Self::Day, 0) => Some(Self::Night),
            (Self::Night, 2..) => Some(Self::Day),
            _ => None,
        }
    }
}

/// A controller's choice of relative timestamps for simultaneously transforming permanents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformOrder {
    pub(crate) controller: crate::priority::SeatId,
    pub(crate) objects: Vec<crate::object_identity::ObjectId>,
    pub(crate) clock: DayNight,
}
impl TransformOrder {
    pub fn chooser(&self) -> crate::priority::SeatId {
        self.controller
    }
    pub fn objects(&self) -> &[crate::object_identity::ObjectId] {
        &self.objects
    }
    pub fn new_designation(&self) -> DayNight {
        self.clock
    }
}

#[cfg(test)]
mod tests {
    use super::DayNight::*;
    #[test]
    fn day_night_changes_only_for_zero_or_at_least_two_active_turn_casts() {
        for casts in 0..=4 {
            assert_eq!(Neither.untap(casts), None);
            assert_eq!(Day.untap(casts), (casts == 0).then_some(Night));
            assert_eq!(Night.untap(casts), (casts >= 2).then_some(Day));
        }
    }
}
