use super::*;
use spellwake_engine::{characteristics::KeywordAbility, day_night::DayNight};
#[test]
fn day_night_survives_the_last_permanent_leaving_and_ordinary_copies_cannot_transform() {
    let mut seed = creature(
        100,
        A,
        ObjectDestination::Battlefield { controller: A },
        None,
        3,
    );
    // An ordinary, single-faced permanent can have daybound and establish the designation.
    // Its physical card has no second face to transform to.
    seed.program
        .printed_keywords
        .insert(KeywordAbility::Daybound);
    let mut cards = seeds()
        .into_iter()
        .filter(|s| matches!(s.card, CardId(13 | 98 | 99)))
        .collect::<Vec<_>>();
    cards.push(seed);
    let g = setup(Some(Step::PrecombatMain), cards);
    assert_eq!(g.execution().day_night(), DayNight::Day);
    assert!(
        g.execution()
            .opposite_physical_face(object(&g, 100))
            .is_none()
    );
    let returned = targeted(&g, A, 13, TargetRef::Object(object(&g, 100)));
    let mut g = resolve(returned.state()).into_state();
    assert_eq!(g.execution().day_night(), DayNight::Day);
    for _ in 0..140 {
        if g.execution().day_night() == DayNight::Night {
            break;
        }
        g = pass(&g).into_state();
    }
    assert_eq!(g.execution().day_night(), DayNight::Night);
    assert_eq!(
        g.execution()
            .window()
            .cards()
            .get(object(&g, 100))
            .unwrap()
            .zone(),
        ObjectZone::Hand(A)
    );
}
