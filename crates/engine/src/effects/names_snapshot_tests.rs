//! Private actual-cut retention; context/definition replacement is fixture preparation,
//! not a public game operation or a supported way to update an active game's rules.
use super::*;
use crate::characteristics::{
    DefiningPart, DefiningParts, NameFilter, NumericExpression, PermanentSelector, StaticAbilityId,
    StaticContinuousAbility,
};
use crate::land::{BasicLandType, LandType};
use spellwake_contracts::names::{EnglishCardName, NameRules, ObjectNames};

#[test]
fn captured_name_relation_survives_later_fixture_context_and_actual_draw_overlay() {
    let a = SeatId(0);
    let b = SeatId(1);
    let name = |value: &str| EnglishCardName::new(value.into()).unwrap();
    let relation = Arc::new(
        NameRules::new(vec![ObjectNames::new(vec![name("A"), name("B")]).unwrap()]).unwrap(),
    );
    let vocabulary = crate::test_support::characteristic_trajectories::vocabulary();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    let goblin = vocabulary.resolve_label("Goblin").unwrap();
    let mut source = Program::creature(None, 2.into(), 3.into(), BTreeSet::from([elf]));
    source.card_types.insert(CardType::Land);
    source.land_subtypes.insert(LandType::Forest);
    source.names = ObjectNames::single(name("A"));
    source.supertypes = BTreeSet::from([crate::program::Supertype::Snow]);
    source.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(vec![DefiningPart::PowerToughness {
            power: Some(Box::new(NumericExpression::count(PermanentSelector {
                name: NameFilter::Named(name("B")),
                ..PermanentSelector::default()
            }))),
            toughness: None,
        }]),
    });
    let mut library = Program::artifact(None, vec![]);
    library.card_types.insert(CardType::Kindred);
    library.card_types.insert(CardType::Land);
    library.land_subtypes.insert(LandType::Plains);
    library.creature_subtypes.insert(elf);
    library.names = ObjectNames::single(name("A"));
    library.supertypes = BTreeSet::from([crate::program::Supertype::Snow]);
    let mut state = EffectWindow::new(GameSeed::with_uniform_life(
        vec![a, b],
        a,
        vec![
            SeedCard {
                card: CardId(1),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: source,
            },
            SeedCard {
                card: CardId(2),
                owner: a,
                destination: ObjectDestination::LibraryBottom,
                program: library,
            },
        ],
        vec![CardId(1)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        relation,
    ))
    .unwrap();
    let source = state.window.cards().current_identity(CardId(1)).unwrap();
    let old_library = state.window.cards().current_identity(CardId(2)).unwrap();
    let snapshot = state.rules_snapshot(None).unwrap();
    let old = snapshot.view().appearance(source).unwrap();
    assert_eq!(
        old.current_supertypes,
        BTreeSet::from([crate::program::Supertype::Snow])
    );
    assert_eq!(
        old.current_power_toughness.as_ref().unwrap().power,
        1.into()
    );
    assert_eq!(
        old.types,
        BTreeSet::from([CardType::Creature, CardType::Land])
    );
    assert_eq!(old.copiable_card_types, old.types);
    assert_eq!(old.creature_subtypes, BTreeSet::from([elf]));
    assert_eq!(old.copiable_creature_subtypes, old.creature_subtypes);
    assert_eq!(old.land_subtypes, BTreeSet::from([LandType::Forest]));
    assert_eq!(old.copiable_land_subtypes, old.land_subtypes);
    assert_eq!(old.intrinsic_mana, BTreeSet::from([BasicLandType::Forest]));
    let drawn = state
        .window
        .cards()
        .draw_cards(state.window.cards().revision(), a, 1.into())
        .unwrap();
    let hand = drawn.next.current_identity(CardId(2)).unwrap();
    assert_ne!(hand, old_library);
    state.name_rules = Arc::new(NameRules::literal_names());
    for program in Arc::make_mut(&mut state.programs).values_mut() {
        program.supertypes = BTreeSet::from([crate::program::Supertype::Basic]);
        program.card_types =
            BTreeSet::from([CardType::Creature, CardType::Kindred, CardType::Land]);
        program.creature_subtypes = BTreeSet::from([goblin]);
        program.land_subtypes = BTreeSet::from([LandType::Island]);
    }
    assert_eq!(
        state
            .evaluated_characteristics()
            .unwrap()
            .get(source)
            .unwrap()
            .current_supertypes,
        BTreeSet::from([crate::program::Supertype::Basic])
    );
    assert_eq!(
        state
            .evaluated_characteristics()
            .unwrap()
            .get(source)
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived
            .power,
        0.into()
    );
    let latest = state
        .rules_snapshot(None)
        .unwrap()
        .view()
        .appearance(source)
        .unwrap();
    assert_eq!(
        latest.types,
        BTreeSet::from([CardType::Creature, CardType::Kindred, CardType::Land])
    );
    assert_eq!(latest.copiable_card_types, latest.types);
    assert_eq!(latest.creature_subtypes, BTreeSet::from([goblin]));
    assert_eq!(latest.copiable_creature_subtypes, latest.creature_subtypes);
    assert_eq!(latest.land_subtypes, BTreeSet::from([LandType::Island]));
    assert_eq!(latest.copiable_land_subtypes, latest.land_subtypes);
    assert_eq!(
        latest.intrinsic_mana,
        BTreeSet::from([BasicLandType::Island])
    );
    let overlay = snapshot.view_with_cards(&drawn.next).unwrap();
    let old_hand = overlay.appearance(hand).unwrap();
    assert_eq!(
        old_hand.types,
        BTreeSet::from([CardType::Artifact, CardType::Kindred, CardType::Land])
    );
    assert_eq!(old_hand.copiable_card_types, old_hand.types);
    assert_eq!(old_hand.creature_subtypes, BTreeSet::from([elf]));
    assert_eq!(old_hand.land_subtypes, BTreeSet::from([LandType::Plains]));
    assert_eq!(old_hand.copiable_land_subtypes, old_hand.land_subtypes);
    assert_eq!(
        old_hand.intrinsic_mana,
        BTreeSet::from([BasicLandType::Plains])
    );
    assert_eq!(
        old_hand.copiable_creature_subtypes,
        old_hand.creature_subtypes
    );
    assert_eq!(overlay.appearance(source).unwrap(), old);
    assert!(overlay.appearance(old_library).is_none());
    assert_eq!(
        overlay.appearance(hand).unwrap().current_supertypes,
        BTreeSet::from([crate::program::Supertype::Snow])
    );
    assert_eq!(
        overlay.appearance(hand).unwrap().current_names,
        ObjectNames::single(name("A"))
    );
    assert_eq!(
        snapshot
            .view()
            .appearance(old_library)
            .unwrap()
            .object
            .zone(),
        ObjectZone::Library(a)
    );
}
