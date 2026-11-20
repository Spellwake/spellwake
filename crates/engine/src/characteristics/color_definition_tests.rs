//! Literal self-only color CDAs: CR 113.6a, 604.3, 613.3 and 702.114a.
use super::*;
use crate::mana::ManaType;

fn definition(colors: &[Color]) -> Program {
    let mut program = Program::creature(
        Some(ManaCost {
            generic: 2.into(),
            required: BTreeMap::from([(ManaType::Blue, 1.into()), (ManaType::Red, 1.into())]),
        }),
        3.into(),
        3.into(),
        BTreeSet::new(),
    );
    program.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(9),
        parts: DefiningParts(vec![DefiningPart::Colors(colors.iter().copied().collect())]),
    });
    program
}
fn battlefield() -> ObjectDestination {
    ObjectDestination::Battlefield { controller: P0 }
}

#[test]
fn literal_definition_works_in_every_physical_zone_without_allocating_or_selecting() {
    let rows = [
        ("library", ObjectDestination::LibraryTop),
        ("hand", ObjectDestination::Hand),
        ("graveyard", ObjectDestination::Graveyard),
        ("battlefield", battlefield()),
        ("stack", ObjectDestination::Stack { controller: P0 }),
        ("exile", ObjectDestination::Exile),
        ("command", ObjectDestination::Command),
    ];
    let fixture = Fixture::new(
        vocabulary(),
        rows.iter()
            .map(|(name, zone)| (*name, *zone, definition(&[])))
            .collect(),
        &["battlefield"],
    );
    let saved = fixture.clone();
    let evaluated = evaluate(fixture.world()).unwrap();
    for (name, _) in rows {
        let current = evaluated.get(fixture.object(name)).unwrap();
        assert_eq!(
            current.defined.colors,
            BTreeSet::from([Color::Blue, Color::Red])
        );
        assert!(current.current_colors.is_empty());
        assert_eq!(current.abilities.continuous, Some(StaticAbilityId(9)));
        assert!(evaluated.trace.applied.iter().any(|applied| {
            applied.key
                == EffectInstanceKey::Static {
                    source: current.object,
                    ability: StaticAbilityId(9),
                }
                && applied.bucket == CharacteristicBucket::Color
                && applied.affected == BTreeSet::from([current.object])
        }));
    }
    assert_eq!(evaluated.trace.applied.len(), 7);
    assert!(evaluated.trace.selections.is_empty());
    assert_eq!(fixture, saved);
    assert_eq!(evaluate(fixture.world()).unwrap(), evaluated);
}

#[test]
fn newer_definition_precedes_older_ordinary_color_effect() {
    let lord = permanent(
        &[CardType::Artifact],
        (0, 0),
        &[],
        None,
        ability(
            &[CardType::Creature],
            &[],
            false,
            vec![StaticPart::Color(ColorOperation::Set(
                StaticColorOperand::Literal(BTreeSet::from([Color::Red])),
            ))],
        ),
    );
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            ("lord", battlefield(), lord),
            ("subject", battlefield(), definition(&[Color::Blue])),
        ],
        &["lord", "subject"],
    );
    let result = evaluate(fixture.world()).unwrap();
    assert_eq!(
        result
            .get(fixture.object("subject"))
            .unwrap()
            .current_colors,
        BTreeSet::from([Color::Red])
    );
    assert_eq!(
        result.trace.applied[0].key,
        EffectInstanceKey::Static {
            source: fixture.object("subject"),
            ability: StaticAbilityId(9),
        }
    );
    assert_eq!(result.trace.applied[1].key, fixture.key("lord"));
}

#[test]
fn later_ability_removal_keeps_already_applied_color_definition() {
    let remover = permanent(
        &[CardType::Artifact],
        (0, 0),
        &[],
        None,
        ability(
            &[CardType::Creature],
            &[],
            false,
            vec![StaticPart::Abilities(AbilityChange::remove(
                AbilityRemoval::All,
            ))],
        ),
    );
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            ("remover", battlefield(), remover),
            ("subject", battlefield(), definition(&[])),
        ],
        &["remover", "subject"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let current = result.get(fixture.object("subject")).unwrap();
    assert_eq!(current.abilities.continuous, None);
    assert!(current.current_colors.is_empty());
    assert_eq!(
        current.defined.colors,
        BTreeSet::from([Color::Blue, Color::Red])
    );
}

#[test]
fn definitions_change_only_their_own_sources_and_preserve_indicator_baseline() {
    let mut blue = definition(&[Color::Blue]);
    blue.color_indicator = Some(BTreeSet::from([Color::Green]));
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            ("blue", ObjectDestination::Hand, blue),
            ("red", ObjectDestination::Hand, definition(&[Color::Red])),
            (
                "plain",
                ObjectDestination::Hand,
                Program::creature(zero_cost(), 3.into(), 3.into(), BTreeSet::new()),
            ),
        ],
        &[],
    );
    let result = evaluate(fixture.world()).unwrap();
    let blue = result.get(fixture.object("blue")).unwrap();
    assert_eq!(blue.defined.colors, BTreeSet::from([Color::Green]));
    assert_eq!(blue.current_colors, BTreeSet::from([Color::Blue]));
    assert_eq!(
        result.get(fixture.object("red")).unwrap().current_colors,
        BTreeSet::from([Color::Red])
    );
    assert!(
        result
            .get(fixture.object("plain"))
            .unwrap()
            .current_colors
            .is_empty()
    );
}
