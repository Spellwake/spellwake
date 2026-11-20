//! Finite name semantics, CR 201.2/201.3, 612.7 and 613.8.
use super::*;
use spellwake_contracts::names::{EnglishCardName, NameRules, ObjectNames};

fn names(values: &[&str]) -> ObjectNames {
    ObjectNames::new(
        values
            .iter()
            .map(|value| EnglishCardName::new((*value).into()).unwrap())
            .collect(),
    )
    .unwrap()
}
fn named(values: &[&str]) -> Program {
    let mut program = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
    program.names = names(values);
    program
}
fn bf() -> ObjectDestination {
    ObjectDestination::Battlefield { controller: P0 }
}
fn source(label: &str, filter: NameFilter, parts: Vec<StaticPart>) -> Program {
    let mut program = named(&[label]);
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            name: filter,
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(parts),
    });
    program
}

#[test]
fn names_are_exact_in_every_physical_zone_and_queries_are_pure() {
    let zones = [
        ("library", ObjectDestination::LibraryTop),
        ("hand", ObjectDestination::Hand),
        ("graveyard", ObjectDestination::Graveyard),
        ("stack", ObjectDestination::Stack { controller: P0 }),
        ("exile", ObjectDestination::Exile),
        ("command", ObjectDestination::Command),
        ("battlefield", bf()),
    ];
    let fixture = Fixture::new(
        vocabulary(),
        zones
            .iter()
            .map(|(alias, zone)| (*alias, *zone, named(&["SP//dr, Piloted by Peni"])))
            .collect(),
        &["battlefield"],
    );
    let before = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    for (alias, _) in zones {
        let current = result.get(fixture.object(alias)).unwrap();
        assert_eq!(current.current_names, names(&["SP//dr, Piloted by Peni"]));
        assert_eq!(current.defined.names, current.current_names);
    }
    assert_eq!(result, evaluate(fixture.world()).unwrap());
    assert_eq!(fixture, before);
}

#[test]
fn names_create_layer_three_dependencies_and_later_components_keep_the_started_set() {
    let rename = source(
        "renamer",
        NameFilter::Any,
        vec![StaticPart::Names(NameOperation::Set(
            StaticNameOperand::Literal(names(&["new"])),
        ))],
    );
    let dependent = source(
        "reader",
        NameFilter::Named(EnglishCardName::new("new".into()).unwrap()),
        vec![
            StaticPart::Names(NameOperation::Set(StaticNameOperand::Literal(names(&[
                "extra",
            ])))),
            StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(1.into()),
                toughness: NumericExpression::Literal(1.into()),
            }),
        ],
    );
    // The reader is older, but its affected set changes if the renamer is applied.
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            ("reader", bf(), dependent),
            ("renamer", bf(), rename),
            ("subject", bf(), named(&["old"])),
        ],
        &["reader", "renamer", "subject"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let first = result
        .trace
        .selections
        .iter()
        .find(|selection| selection.bucket == CharacteristicBucket::Text)
        .unwrap();
    assert!(first.original_waits.contains(&ComponentWait {
        waiting: fixture.key("reader"),
        prerequisite: fixture.key("renamer")
    }));
    assert_eq!(first.chosen, fixture.key("renamer"));
    let subject = result.get(fixture.object("subject")).unwrap();
    assert_eq!(subject.current_names, names(&["extra"]));
    assert_eq!(subject.defined.names, names(&["old"]));
    assert_eq!(
        subject.power_toughness.as_ref().unwrap().derived,
        pair(3, 4)
    );
}

#[test]
fn live_source_names_and_interchangeable_filters_use_the_supplied_relation() {
    let mut fixture = Fixture::new(
        vocabulary(),
        vec![
            (
                "source",
                bf(),
                source(
                    "Eleven, the Mage",
                    NameFilter::SharesWithSource,
                    vec![StaticPart::Names(NameOperation::Add(
                        StaticNameOperand::SourceCurrentNames,
                    ))],
                ),
            ),
            ("subject", bf(), named(&["Cecily, Haunted Mage"])),
            ("unrelated", bf(), named(&["Cosmetic title"])),
        ],
        &["source", "subject", "unrelated"],
    );
    fixture.name_rules =
        NameRules::new(vec![names(&["Eleven, the Mage", "Cecily, Haunted Mage"])]).unwrap();
    let result = evaluate(fixture.world()).unwrap();
    assert_eq!(
        result.get(fixture.object("subject")).unwrap().current_names,
        names(&["Cecily, Haunted Mage", "Eleven, the Mage"])
    );
    assert_eq!(
        result
            .get(fixture.object("unrelated"))
            .unwrap()
            .current_names,
        names(&["Cosmetic title"])
    );
}

#[test]
fn different_from_source_preserves_nameless_direction_and_feeds_defining_pt_counts() {
    let mut defining = named(&[]);
    let count = NumericExpression::count(PermanentSelector {
        name: NameFilter::DifferentFromSource,
        ..PermanentSelector::default()
    });
    defining.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(vec![DefiningPart::PowerToughness {
            power: Some(Box::new(count)),
            toughness: None,
        }]),
    });
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            ("source", bf(), defining),
            ("named", bf(), named(&["A"])),
            ("nameless", bf(), named(&[])),
        ],
        &["source", "named", "nameless"],
    );
    let result = evaluate(fixture.world()).unwrap();
    assert_eq!(
        result
            .get(fixture.object("source"))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pair(1, 3)
    );
}

#[test]
fn proper_name_literals_are_protected_from_creature_type_word_changes() {
    let v = vocabulary();
    let elf = v.resolve_label("Elf").unwrap();
    let vampire = v.resolve_label("Vampire").unwrap();
    let mut renamed = source(
        "Elf",
        NameFilter::Named(EnglishCardName::new("Elf".into()).unwrap()),
        vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(1.into()),
            toughness: NumericExpression::Literal(0.into()),
        })],
    );
    renamed.creature_subtypes.insert(elf);
    let text = source(
        "rewriter",
        NameFilter::Any,
        vec![StaticPart::Text(CreatureTypeTextChange {
            from: elf,
            to: vampire,
        })],
    );
    let fixture = Fixture::new(
        v,
        vec![
            ("rewriter", bf(), text),
            ("source", bf(), renamed),
            ("subject", bf(), named(&["Elf"])),
        ],
        &["rewriter", "source", "subject"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let current = result.get(fixture.object("source")).unwrap();
    assert_eq!(current.current_names, names(&["Elf"]));
    assert_eq!(current.current_creature_subtypes, BTreeSet::from([vampire]));
    assert_eq!(
        result
            .get(fixture.object("subject"))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pair(3, 3)
    );
}

#[test]
fn source_current_names_operand_waits_for_a_source_rename_in_the_same_layer() {
    let mut reader = source(
        "Original",
        NameFilter::Any,
        vec![StaticPart::Names(NameOperation::Set(
            StaticNameOperand::SourceCurrentNames,
        ))],
    );
    if let Some(StaticContinuousAbility::Battlefield { selector, .. }) = &mut reader.continuous {
        selector.required_types.insert(CardType::Creature);
    }
    let replacement = Program::artifact(None, vec![]);
    reader.card_types = replacement.card_types;
    reader.creature_subtypes = replacement.creature_subtypes;
    reader.kind = replacement.kind;
    let mut renamer = Program::artifact(None, vec![]);
    renamer.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_supertypes: std::collections::BTreeSet::new(),
            forbidden_supertypes: std::collections::BTreeSet::new(),
            required_types: BTreeSet::from([CardType::Artifact]),
            name: NameFilter::Named(EnglishCardName::new("Original".into()).unwrap()),
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Names(NameOperation::Set(
            StaticNameOperand::Literal(names(&["Changed"])),
        ))]),
    });
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            ("reader", bf(), reader),
            ("renamer", bf(), renamer),
            ("subject", bf(), named(&["Subject"])),
        ],
        &["reader", "renamer", "subject"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let first = result
        .trace
        .selections
        .iter()
        .find(|selection| selection.bucket == CharacteristicBucket::Text)
        .unwrap();
    assert!(first.original_waits.contains(&ComponentWait {
        waiting: fixture.key("reader"),
        prerequisite: fixture.key("renamer")
    }));
    assert_eq!(first.chosen, fixture.key("renamer"));
    assert_eq!(
        result.get(fixture.object("subject")).unwrap().current_names,
        names(&["Changed"])
    );
}

#[test]
fn static_name_change_feeds_later_name_qualified_power_matching_the_forge_probe() {
    let filter = |value: &str| NameFilter::Named(EnglishCardName::new(value.into()).unwrap());
    let booster = source(
        "Spellwake Name Booster",
        filter("Spellwake New"),
        vec![StaticPart::ModifyPT(NumericPair {
            power: NumericExpression::Literal(1.into()),
            toughness: NumericExpression::Literal(1.into()),
        })],
    );
    let renamer = source(
        "Spellwake Renamer",
        filter("Spellwake Old"),
        vec![StaticPart::Names(NameOperation::Set(
            StaticNameOperand::Literal(names(&["Spellwake New"])),
        ))],
    );
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            ("booster", bf(), booster),
            ("renamer", bf(), renamer),
            ("subject", bf(), named(&["Spellwake Old"])),
        ],
        &["booster", "renamer", "subject"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let subject = result.get(fixture.object("subject")).unwrap();
    assert_eq!(subject.current_names, names(&["Spellwake New"]));
    assert_eq!(
        subject.power_toughness.as_ref().unwrap().derived,
        pair(3, 4)
    );
    // Only the renamed target's name/PT fields are common with the prepared Forge
    // static probe. Native host types/full lifecycle are not compared here.
}
