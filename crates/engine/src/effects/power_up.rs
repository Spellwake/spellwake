//! Fixed-symbol Power-up activation prices (CR 702.193 and 118.7).
use super::*;

impl EffectWindow {
    /// Quote a printed fixed Power-up price for this exact permanent incarnation.
    /// The ordinary activation command rechecks the current ability and uses this
    /// same price to validate the payment and record the receipt.
    pub fn power_up_mana_cost(&self, source: ObjectId, printed: &ManaCost) -> ManaCost {
        if source < self.turn_start_incarnation
            || !self.window.cards().get(source).is_some_and(|card| {
                card.zone() == ObjectZone::Battlefield && card.face_down().is_none()
            })
        {
            return printed.clone();
        }
        let Some(reduction) = self
            .copiable_program(source)
            .and_then(Program::permanent)
            .and_then(|p| p.casting_cost.as_ref())
        else {
            return printed.clone();
        };
        reduce_power_up(printed, reduction)
    }
}
fn reduce_power_up(printed: &ManaCost, reduction: &ManaCost) -> ManaCost {
    let mut due = printed.clone();
    let mut generic_discount = reduction.generic.clone();
    for (color, offered) in &reduction.required {
        let required = due.required.remove(color).unwrap_or_else(|| 0.into());
        if &required > offered {
            due.required.insert(*color, required.minus(offered));
        } else {
            generic_discount = generic_discount.plus(&offered.minus(&required));
        }
    }
    due.generic = std::cmp::max(0.into(), due.generic.minus(&generic_discount));
    due
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn power_up_matching_colors_and_colorless_reduce_first_then_excess_reduces_only_generic() {
        use ManaType::{Blue, Colourless, Green, Red, White};
        let price = ManaCost {
            generic: 6.into(),
            required: [(Blue, 2.into()), (Colourless, 1.into()), (Red, 2.into())].into(),
        };
        let reducer = ManaCost {
            generic: 1.into(),
            required: [
                (Blue, 3.into()),
                (Colourless, 2.into()),
                (Green, 1.into()),
                (White, 1.into()),
            ]
            .into(),
        };
        assert_eq!(
            reduce_power_up(&price, &reducer),
            ManaCost {
                generic: 1.into(),
                required: [(Red, 2.into())].into()
            }
        );
        assert_eq!(
            reduce_power_up(&ManaCost::one(Red), &reducer),
            ManaCost::one(Red)
        );
        assert_eq!(
            reduce_power_up(&ManaCost::generic(3.into()), &reducer),
            ManaCost::free()
        );
    }
}
