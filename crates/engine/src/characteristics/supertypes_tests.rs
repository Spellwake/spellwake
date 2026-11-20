//! CR 205.4a-c/g, 613.1d and 613.8: finite traits, not World/legend qualification.
use super::*;
use crate::program::Supertype;

fn set(values: &[Supertype]) -> BTreeSet<Supertype> {
    values.iter().copied().collect()
}
fn bf() -> ObjectDestination {
    ObjectDestination::Battlefield { controller: P0 }
}
fn subject(supertypes: &[Supertype]) -> Program {
    let mut program = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
    program.supertypes = set(supertypes);
    program
}
fn source(selector: PermanentSelector, parts: Vec<StaticPart>) -> Program {
    let mut program = Program::artifact(None, vec![]);
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector,
        parts: StaticParts(parts),
    });
    program
}
fn supertype_part(operation: TypeOperation<Supertype>) -> StaticPart {
    StaticPart::Type(StaticTypeChange {
        land_subtypes: SubtypeOperation::Keep,
        supertypes: operation,
        types: TypeOperation::Keep,
        subtypes: SubtypeOperation::Keep,
    })
}

#[test]
fn all_five_printed_supertypes_are_exact_in_every_zone_and_queries_are_pure() {
    use Supertype::*;
    let all = [Basic, Legendary, Ongoing, Snow, World];
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
            .map(|(alias, zone)| (*alias, *zone, subject(&all)))
            .collect(),
        &["battlefield"],
    );
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    for (alias, _) in zones {
        let value = result.get(fixture.object(alias)).unwrap();
        assert_eq!(value.defined.supertypes, set(&all));
        assert_eq!(value.current_supertypes, set(&all));
        assert!(value.is_creature());
    }
    assert_eq!(evaluate(fixture.world()).unwrap(), result);
    assert_eq!(fixture, saved);
}

#[test]
fn replacing_types_preserves_supertypes_and_replacing_supertypes_preserves_subtypes() {
    use Supertype::*;
    let v = vocabulary();
    let elf = v.resolve_label("Elf").unwrap();
    let mut printed = subject(&[Legendary, Snow]);
    printed.creature_subtypes.insert(elf);
    let only_creatures = || PermanentSelector {
        required_types: BTreeSet::from([CardType::Creature]),
        ..PermanentSelector::default()
    };
    let fixture = Fixture::new(
        v.clone(),
        vec![
            (
                "source",
                bf(),
                source(
                    only_creatures(),
                    vec![StaticPart::Type(StaticTypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Set(BTreeSet::from([CardType::Artifact])),
                        subtypes: SubtypeOperation::Keep,
                    })],
                ),
            ),
            ("subject", bf(), printed.clone()),
        ],
        &["source", "subject"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let value = result.get(fixture.object("subject")).unwrap();
    assert_eq!(value.current_supertypes, set(&[Legendary, Snow]));
    assert_eq!(value.current_types, BTreeSet::from([CardType::Artifact]));
    assert!(value.current_creature_subtypes.is_empty());
    assert_eq!(value.defined.creature_subtypes, BTreeSet::from([elf]));
    let fixture = Fixture::new(
        v.clone(),
        vec![
            (
                "source",
                bf(),
                source(
                    only_creatures(),
                    vec![supertype_part(TypeOperation::Set(set(&[Basic])))],
                ),
            ),
            ("subject", bf(), printed.clone()),
        ],
        &["source", "subject"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let value = result.get(fixture.object("subject")).unwrap();
    assert_eq!(value.current_supertypes, set(&[Basic]));
    assert_eq!(value.defined.supertypes, set(&[Legendary, Snow]));
    assert!(value.is_creature());
    assert_eq!(value.current_creature_subtypes, BTreeSet::from([elf]));
    assert_eq!(value.power_toughness.as_ref().unwrap().derived, pair(2, 3));
    // Exact native counterpart to the prepared Forge removal probe.
    let fixture = Fixture::new(
        v,
        vec![
            (
                "source",
                bf(),
                source(
                    only_creatures(),
                    vec![supertype_part(TypeOperation::Remove(set(&[Snow])))],
                ),
            ),
            ("subject", bf(), printed),
        ],
        &["source", "subject"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let value = result.get(fixture.object("subject")).unwrap();
    assert_eq!(value.current_supertypes, set(&[Legendary]));
    assert_eq!(value.current_creature_subtypes, BTreeSet::from([elf]));
    assert!(value.is_creature());
}

#[test]
fn supertype_and_type_operations_share_one_layer_four_bucket() {
    let program = source(
        PermanentSelector::default(),
        vec![
            supertype_part(TypeOperation::Add(set(&[Supertype::Snow]))),
            StaticPart::Type(StaticTypeChange {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                subtypes: SubtypeOperation::Keep,
            }),
        ],
    );
    assert_eq!(
        program.validate(&vocabulary()),
        Err(crate::program::DefinitionError::ContinuousEffect)
    );
}

#[test]
fn creature_word_rewriting_preserves_populated_supertype_operations_and_filters() {
    use Supertype::*;
    let v = vocabulary();
    let elf = v.resolve_label("Elf").unwrap();
    let vampire = v.resolve_label("Vampire").unwrap();
    let mut generator = subject(&[Snow]);
    generator.creature_subtypes.insert(elf);
    generator.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_supertypes: set(&[Snow]),
            forbidden_supertypes: set(&[World]),
            creature_subtype: Some(elf),
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Add(set(&[Basic])),
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Add(StaticSubtypeOperand::Literal(BTreeSet::from([elf]))),
        })]),
    });
    let mut target = subject(&[Snow]);
    target.creature_subtypes.insert(elf);
    let rewriter = source(
        PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        },
        vec![StaticPart::Text(CreatureTypeTextChange {
            from: elf,
            to: vampire,
        })],
    );
    let fixture = Fixture::new(
        v,
        vec![
            ("generator", bf(), generator),
            ("rewriter", bf(), rewriter),
            ("target", bf(), target),
        ],
        &["generator", "rewriter", "target"],
    );
    let result = evaluate(fixture.world()).unwrap();
    let current = result.get(fixture.object("generator")).unwrap();
    let Some(StaticContinuousAbility::Battlefield {
        selector, parts, ..
    }) = &current.printed_text.continuous
    else {
        panic!("rewritten static")
    };
    assert_eq!(selector.creature_subtype, Some(vampire));
    assert_eq!(selector.required_supertypes, set(&[Snow]));
    assert_eq!(selector.forbidden_supertypes, set(&[World]));
    assert_eq!(
        parts.0,
        vec![StaticPart::Type(StaticTypeChange {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Add(set(&[Basic])),
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Add(StaticSubtypeOperand::Literal(BTreeSet::from([
                vampire
            ])))
        })]
    );
    let target = result.get(fixture.object("target")).unwrap();
    assert_eq!(target.current_supertypes, set(&[Basic, Snow]));
    assert_eq!(target.current_creature_subtypes, BTreeSet::from([vampire]));
}

#[test]
fn populated_supertype_filters_obey_source_suppression_and_earlier_layer_start() {
    use Supertype::*;
    for started in [false, true] {
        let mut parts = vec![StaticPart::SetPT(NumericPair {
            power: NumericExpression::Literal(4.into()),
            toughness: NumericExpression::Literal(5.into()),
        })];
        if started {
            parts.insert(0, supertype_part(TypeOperation::Add(set(&[Basic]))));
        }
        let generator = source(
            PermanentSelector {
                required_supertypes: set(&[Snow]),
                forbidden_supertypes: set(&[Legendary]),
                ..PermanentSelector::default()
            },
            parts,
        );
        let remover = source(
            PermanentSelector {
                required_types: BTreeSet::from([CardType::Artifact]),
                exclude_source: true,
                ..PermanentSelector::default()
            },
            vec![StaticPart::Abilities(AbilityChange::remove(
                AbilityRemoval::All,
            ))],
        );
        let fixture = Fixture::new(
            vocabulary(),
            vec![
                ("generator", bf(), generator),
                ("remover", bf(), remover),
                ("target", bf(), subject(&[Snow])),
            ],
            &["generator", "remover", "target"],
        );
        let saved = fixture.clone();
        let result = evaluate(fixture.world()).unwrap();
        assert_eq!(
            result
                .get(fixture.object("generator"))
                .unwrap()
                .abilities
                .continuous,
            None
        );
        let target = result.get(fixture.object("target")).unwrap();
        assert_eq!(
            target.current_supertypes,
            if started {
                set(&[Basic, Snow])
            } else {
                set(&[Snow])
            }
        );
        assert_eq!(
            target.power_toughness.as_ref().unwrap().derived,
            if started { pair(4, 5) } else { pair(2, 3) }
        );
        assert_eq!(
            result.trace.bindings[&fixture.key("generator")],
            if started {
                PassBinding::Processed(fixture.affected(&["target"]))
            } else {
                PassBinding::NotProcessed
            }
        );
        assert_eq!(evaluate(fixture.world()).unwrap(), result);
        assert_eq!(fixture, saved);
    }
}

#[test]
fn supertype_filters_create_layer_four_dependencies_and_preserve_started_recipients() {
    use Supertype::*;
    let reader = source(
        PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            required_supertypes: set(&[Snow]),
            forbidden_supertypes: set(&[World]),
            ..PermanentSelector::default()
        },
        vec![
            supertype_part(TypeOperation::Remove(set(&[Snow]))),
            StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(1.into()),
                toughness: NumericExpression::Literal(1.into()),
            }),
        ],
    );
    let maker = source(
        PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        },
        vec![supertype_part(TypeOperation::Add(set(&[Snow])))],
    );
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            ("reader", bf(), reader),
            ("maker", bf(), maker),
            ("subject", bf(), subject(&[])),
            ("excluded", bf(), subject(&[World])),
        ],
        &["reader", "maker", "subject", "excluded"],
    );
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    let first = result
        .trace
        .selections
        .iter()
        .find(|selection| selection.bucket == CharacteristicBucket::Type)
        .unwrap();
    assert!(first.original_waits.contains(&ComponentWait {
        waiting: fixture.key("reader"),
        prerequisite: fixture.key("maker")
    }));
    assert_eq!(first.chosen, fixture.key("maker"));
    let value = result.get(fixture.object("subject")).unwrap();
    assert!(value.current_supertypes.is_empty());
    // Removing Snow makes the selector false, but layer 7 retains the earlier-started set.
    assert_eq!(value.power_toughness.as_ref().unwrap().derived, pair(3, 4));
    let excluded = result.get(fixture.object("excluded")).unwrap();
    assert_eq!(excluded.current_supertypes, set(&[Snow, World]));
    assert_eq!(
        excluded.power_toughness.as_ref().unwrap().derived,
        pair(2, 3)
    );
    assert_eq!(evaluate(fixture.world()).unwrap(), result);
    assert_eq!(fixture, saved);
}

#[test]
fn defining_counts_use_current_supertypes_and_never_infer_basic_or_snow_from_names() {
    use Supertype::*;
    let mut counted = subject(&[]);
    let ProgramKind::Permanent(definition) = &mut counted.kind else {
        panic!("creature")
    };
    definition.printed_pt = Some(PrintedPowerToughness {
        power: PrintedStat::Star {
            multiplier: 1.into(),
            offset: 0.into(),
        },
        toughness: PrintedStat::Number(3.into()),
    });
    counted.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(vec![DefiningPart::PowerToughness {
            power: Some(Box::new(NumericExpression::count(PermanentSelector {
                required_supertypes: set(&[Snow]),
                forbidden_supertypes: set(&[Legendary]),
                ..PermanentSelector::default()
            }))),
            toughness: None,
        }]),
    });
    let mut misleading = Program::land(std::collections::BTreeSet::new(), vec![]);
    misleading.names = spellwake_contracts::names::ObjectNames::single(
        spellwake_contracts::names::EnglishCardName::new("Snow-Covered Plains".into()).unwrap(),
    );
    let mut plains_named = Program::land(std::collections::BTreeSet::new(), vec![]);
    plains_named.names = spellwake_contracts::names::ObjectNames::single(
        spellwake_contracts::names::EnglishCardName::new("Plains".into()).unwrap(),
    );
    let fixture = Fixture::new(
        vocabulary(),
        vec![
            ("counted", ObjectDestination::Hand, counted),
            ("snow", bf(), subject(&[Snow])),
            ("legendary-snow", bf(), subject(&[Legendary, Snow])),
            ("misleading", bf(), misleading),
            ("plains-named", bf(), plains_named),
        ],
        &["snow", "legendary-snow", "misleading", "plains-named"],
    );
    let result = evaluate(fixture.world()).unwrap();
    assert_eq!(
        result
            .get(fixture.object("counted"))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pair(1, 3)
    );
    assert!(
        result
            .get(fixture.object("misleading"))
            .unwrap()
            .current_supertypes
            .is_empty()
    );
    assert!(
        result
            .get(fixture.object("plains-named"))
            .unwrap()
            .current_supertypes
            .is_empty()
    );
}
