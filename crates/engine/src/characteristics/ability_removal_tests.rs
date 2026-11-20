//! CR 613.1f, 613.6 and 613.8: existence is part of dependency evaluation.
use super::*;
use crate::program::CardType::{Artifact, Creature};

fn source(parts: Vec<StaticPart>, required: &[CardType], exclude: bool) -> Program {
    permanent(
        &[Artifact],
        (4, 4),
        &[],
        zero_cost(),
        ability(required, &[], exclude, parts),
    )
}

#[test]
fn ability_removal_suppresses_a_pure_later_layer_static() {
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            (
                "pump",
                battlefield(),
                source(vec![set_two()], &[Creature], false),
            ),
            (
                "remove",
                battlefield(),
                source(
                    vec![StaticPart::Abilities(AbilityChange::remove(
                        AbilityRemoval::All,
                    ))],
                    &[Artifact],
                    true,
                ),
            ),
            (
                "target",
                battlefield(),
                permanent(&[Creature], (3, 3), &[], zero_cost(), None),
            ),
        ],
        &["pump", "remove", "target"],
    );
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    assert_eq!(
        result
            .get(fixture.object("target"))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pair(3, 3)
    );
    assert_eq!(
        result
            .get(fixture.object("pump"))
            .unwrap()
            .abilities
            .continuous,
        None
    );
    assert_eq!(
        result.trace.bindings[&fixture.key("pump")],
        PassBinding::NotProcessed
    );
    assert!(
        !result
            .trace
            .applied
            .iter()
            .any(|part| part.key == fixture.key("pump"))
    );
    assert_eq!(fixture, saved);
}

#[test]
fn effect_started_in_type_layer_continues_after_its_ability_is_removed() {
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            (
                "animate",
                battlefield(),
                source(
                    vec![static_types(add_type(Creature)), set_two()],
                    &[Artifact],
                    false,
                ),
            ),
            (
                "remove",
                battlefield(),
                source(
                    vec![StaticPart::Abilities(AbilityChange::remove(
                        AbilityRemoval::All,
                    ))],
                    &[Creature],
                    false,
                ),
            ),
        ],
        &["animate", "remove"],
    );
    let result = evaluate(fixture.world()).unwrap();
    for alias in ["animate", "remove"] {
        let current = result.get(fixture.object(alias)).unwrap();
        assert!(current.is_creature());
        assert_eq!(current.abilities.continuous, None);
        assert_eq!(
            current.power_toughness.as_ref().unwrap().derived,
            pair(2, 2)
        );
    }
    assert!(result.trace.applied.iter().any(
        |part| part.key == fixture.key("animate") && part.bucket == CharacteristicBucket::SetPT
    ));
    assert_eq!(result, evaluate(fixture.world()).unwrap());
}

#[test]
fn removing_the_generator_creates_a_same_layer_dependency() {
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            (
                "first",
                battlefield(),
                source(
                    vec![StaticPart::Abilities(AbilityChange::remove(
                        AbilityRemoval::All,
                    ))],
                    &[Creature],
                    false,
                ),
            ),
            (
                "second",
                battlefield(),
                source(
                    vec![StaticPart::Abilities(AbilityChange::remove(
                        AbilityRemoval::NonMana,
                    ))],
                    &[Artifact],
                    true,
                ),
            ),
            (
                "target",
                battlefield(),
                permanent(&[Creature], (3, 3), &[], zero_cost(), None),
            ),
        ],
        &["first", "second", "target"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let first_selection = result
        .trace
        .selections
        .iter()
        .find(|selection| selection.bucket == CharacteristicBucket::Abilities)
        .unwrap();
    // The newer effect goes first because it removes the older generator.
    assert_eq!(
        first_selection.original_waits,
        fixture.waits(&[("first", "second")])
    );
    assert_eq!(first_selection.chosen, fixture.key("second"));
    assert_eq!(
        result.trace.bindings[&fixture.key("first")],
        PassBinding::NotProcessed
    );
    assert_eq!(
        result
            .get(fixture.object("second"))
            .unwrap()
            .abilities
            .continuous,
        Some(StaticAbilityId(1))
    );
}

#[test]
fn mutually_removing_statics_use_timestamp_then_recompute_existence() {
    for order in [["a", "b"], ["b", "a"]] {
        let fixture = Fixture::new(
            vocabulary(),
            vec![
                (
                    "a",
                    battlefield(),
                    source(
                        vec![StaticPart::Abilities(AbilityChange::remove(
                            AbilityRemoval::All,
                        ))],
                        &[Artifact],
                        true,
                    ),
                ),
                (
                    "b",
                    battlefield(),
                    source(
                        vec![StaticPart::Abilities(AbilityChange::remove(
                            AbilityRemoval::All,
                        ))],
                        &[Artifact],
                        true,
                    ),
                ),
            ],
            &order,
        );
        let result = evaluate(fixture.world()).unwrap();
        assert_eq!(result.trace.applied.len(), 1);
        assert_eq!(result.trace.applied[0].key, fixture.key(order[0]));
        assert_eq!(
            result
                .get(fixture.object(order[0]))
                .unwrap()
                .abilities
                .continuous,
            Some(StaticAbilityId(1))
        );
        assert_eq!(
            result
                .get(fixture.object(order[1]))
                .unwrap()
                .abilities
                .continuous,
            None
        );
        let first = &result.trace.selections[0];
        assert_eq!(
            first.strongly_connected,
            vec![BTreeSet::from([fixture.key("a"), fixture.key("b")])]
        );
    }
}

#[test]
fn static_that_removes_itself_finishes_its_later_pt_component() {
    let fixture = Fixture::new(
        vocabulary(),
        vec![(
            "self",
            battlefield(),
            permanent(
                &[Creature],
                (4, 4),
                &[],
                zero_cost(),
                ability(
                    &[Creature],
                    &[],
                    false,
                    vec![
                        StaticPart::Abilities(AbilityChange::remove(AbilityRemoval::All)),
                        set_two(),
                    ],
                ),
            ),
        )],
        &["self"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let current = result.get(fixture.object("self")).unwrap();
    assert_eq!(current.abilities.continuous, None);
    assert_eq!(
        current.power_toughness.as_ref().unwrap().derived,
        pair(2, 2)
    );
    assert_eq!(result.trace.applied.len(), 2);
}
