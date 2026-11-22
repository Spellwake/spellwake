//! CR 109.5, 205.3d, 208.2a/208.3, 613.4a/613.6 and 702.73.
use super::*;
use spellwake_contracts::RulesInteger;

fn star(offset: i64) -> PrintedStat {
    PrintedStat::Star {
        multiplier: 1.into(),
        offset: offset.into(),
    }
}
fn box_star() -> PrintedPowerToughness {
    PrintedPowerToughness {
        power: star(0),
        toughness: star(1),
    }
}
fn defining(parts: Vec<DefiningPart>) -> Program {
    let mut p = Program::creature(zero_cost(), 0.into(), 1.into(), BTreeSet::new());
    let ProgramKind::Permanent(d) = &mut p.kind else {
        unreachable!()
    };
    d.printed_pt = Some(box_star());
    p.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(parts),
    });
    p
}
// Prepared reference witnesses share these printed boxes and formulas.
fn all_types() -> Program {
    let mut p = defining(vec![DefiningPart::CreatureTypes(
        DefiningCreatureTypes::All,
    )]);
    let ProgramKind::Permanent(d) = &mut p.kind else {
        unreachable!()
    };
    d.printed_pt = Some(pair(3, 3).into());
    p
}
fn equal_lands() -> Program {
    let mut p = defining(vec![DefiningPart::PowerToughness {
        power: Some(Box::new(lands(0))),
        toughness: Some(Box::new(lands(0))),
    }]);
    let ProgramKind::Permanent(d) = &mut p.kind else {
        unreachable!()
    };
    d.printed_pt = Some(PrintedPowerToughness {
        power: star(0),
        toughness: star(0),
    });
    p
}
fn lands(offset: i64) -> NumericExpression {
    NumericExpression::Count {
        selector: PermanentSelector {
            required_supertypes: std::collections::BTreeSet::new(),
            forbidden_supertypes: std::collections::BTreeSet::new(),
            required_types: BTreeSet::from([CardType::Land]),
            controller: ControllerSelector::SourceController,
            ..PermanentSelector::default()
        },
        multiplier: 1.into(),
        offset: offset.into(),
    }
}
fn count_part() -> DefiningPart {
    DefiningPart::PowerToughness {
        power: Some(Box::new(lands(0))),
        toughness: Some(Box::new(lands(1))),
    }
}
fn remover() -> Program {
    permanent(
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
    )
}
fn zones() -> [(&'static str, ObjectDestination); 7] {
    [
        ("library", ObjectDestination::LibraryTop),
        ("hand", ObjectDestination::Hand),
        ("graveyard", ObjectDestination::Graveyard),
        ("battlefield", battlefield()),
        ("stack", ObjectDestination::Stack { controller: P0 }),
        ("exile", ObjectDestination::Exile),
        ("command", ObjectDestination::Command),
    ]
}

#[test]
fn all_types_use_the_exact_world_vocabulary_in_every_zone_and_queries_are_pure() {
    let v = vocabulary();
    let expected = v
        .members()
        .iter()
        .map(|name| v.resolve_label(name).unwrap())
        .collect::<BTreeSet<_>>();
    let fixture = Fixture::new(
        v,
        zones()
            .into_iter()
            .map(|(alias, zone)| (alias, zone, all_types()))
            .collect(),
        &["battlefield"],
    );
    let saved = fixture.clone();
    let result = evaluate(fixture.world()).unwrap();
    for (alias, _) in zones() {
        let value = result.get(fixture.object(alias)).unwrap();
        assert_eq!(value.current_creature_subtypes, expected);
        assert_eq!(value.power_toughness.as_ref().unwrap().derived, pair(3, 3));
        assert_eq!(value.zone, fixture.cards.get(value.object).unwrap().zone());
    }
    assert_eq!(fixture, saved);
    assert_eq!(evaluate(fixture.world()).unwrap(), result);
    assert!(result.trace.selections.is_empty());
}

#[test]
fn current_pt_in_all_zones_uses_controller_when_present_and_owner_otherwise() {
    let mut rows = zones()
        .into_iter()
        .map(|(alias, zone)| (alias, zone, equal_lands()))
        .collect::<Vec<_>>();
    rows.push((
        "own1",
        battlefield(),
        Program::land(std::collections::BTreeSet::new(), vec![]),
    ));
    rows.push((
        "own2",
        battlefield(),
        Program::land(std::collections::BTreeSet::new(), vec![]),
    ));
    rows.push((
        "other",
        ObjectDestination::Battlefield {
            controller: SeatId(1),
        },
        Program::land(std::collections::BTreeSet::new(), vec![]),
    ));
    rows.push((
        "controlled_stack",
        ObjectDestination::Stack {
            controller: SeatId(1),
        },
        equal_lands(),
    ));
    rows.push((
        "controlled_bf",
        ObjectDestination::Battlefield {
            controller: SeatId(1),
        },
        equal_lands(),
    ));
    let f = Fixture::new(
        vocabulary(),
        rows,
        &["battlefield", "own1", "own2", "other", "controlled_bf"],
    );
    let result = evaluate(f.world()).unwrap();
    for (alias, _) in zones() {
        let pt = result
            .get(f.object(alias))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap();
        assert_eq!(pt.derived, pair(2, 2));
        assert_eq!(
            pt.copiable,
            Some(PrintedPowerToughness {
                power: star(0),
                toughness: star(0)
            })
        );
    }
    for alias in ["controlled_stack", "controlled_bf"] {
        assert_eq!(
            result
                .get(f.object(alias))
                .unwrap()
                .power_toughness
                .as_ref()
                .unwrap()
                .derived,
            pair(1, 1)
        );
    }
}

#[test]
fn printed_noncreature_pt_exists_outside_battlefield_only() {
    let p = permanent(&[CardType::Artifact], (4, 5), &[], None, None);
    let f = Fixture::new(
        vocabulary(),
        zones()
            .into_iter()
            .map(|(alias, zone)| (alias, zone, p.clone()))
            .collect(),
        &["battlefield"],
    );
    let result = evaluate(f.world()).unwrap();
    for (alias, zone) in zones() {
        let current = result.get(f.object(alias)).unwrap();
        assert_eq!(
            current.power_toughness.as_ref().map(|p| p.derived.clone()),
            if matches!(zone, ObjectDestination::Battlefield { .. }) {
                None
            } else {
                Some(pair(4, 5))
            }
        );
    }
}

#[test]
fn printed_affine_baseline_and_copiable_formula_are_distinct() {
    let mut p = defining(vec![count_part()]);
    p.continuous = None;
    let ProgramKind::Permanent(d) = &mut p.kind else {
        unreachable!()
    };
    d.printed_pt = Some(PrintedPowerToughness {
        power: PrintedStat::Star {
            multiplier: (-1).into(),
            offset: 7.into(),
        },
        toughness: star(2),
    });
    let f = Fixture::new(
        vocabulary(),
        vec![("subject", ObjectDestination::Hand, p)],
        &[],
    );
    let current = evaluate(f.world())
        .unwrap()
        .objects
        .remove(&f.object("subject"))
        .unwrap();
    let pt = current.power_toughness.unwrap();
    assert_eq!(pt.base, pair(7, 2));
    assert_eq!(pt.copiable, current.defined.printed_pt);
}

#[test]
fn pt_only_removal_falls_back_but_earlier_type_part_continues_to_seven_a() {
    for starts_earlier in [false, true] {
        let mut parts = vec![count_part()];
        if starts_earlier {
            parts.insert(0, DefiningPart::CreatureTypes(DefiningCreatureTypes::All));
        }
        let f = Fixture::new(
            vocabulary(),
            vec![
                ("subject", battlefield(), defining(parts)),
                ("remove", battlefield(), remover()),
                (
                    "land1",
                    battlefield(),
                    Program::land(std::collections::BTreeSet::new(), vec![]),
                ),
                (
                    "land2",
                    battlefield(),
                    Program::land(std::collections::BTreeSet::new(), vec![]),
                ),
            ],
            &["subject", "remove", "land1", "land2"],
        );
        let result = evaluate(f.world()).unwrap();
        let current = result.get(f.object("subject")).unwrap();
        assert_eq!(current.abilities.continuous, None);
        assert_eq!(
            current.power_toughness.as_ref().unwrap().derived,
            if starts_earlier {
                pair(2, 3)
            } else {
                pair(0, 1)
            }
        );
        assert_eq!(
            result.trace.bindings[&f.key("subject")],
            if starts_earlier {
                PassBinding::Processed(BTreeSet::from([f.object("subject")]))
            } else {
                PassBinding::NotProcessed
            }
        );
        assert_eq!(
            current.power_toughness.as_ref().unwrap().copiable,
            Some(box_star())
        );
    }
}

#[test]
fn older_ordinary_type_setter_overrides_newer_all_types_definition() {
    let v = vocabulary();
    let goblin = v.resolve_label("Goblin").unwrap();
    let field = permanent(
        &[CardType::Artifact],
        (0, 0),
        &[],
        None,
        ability(
            &[CardType::Creature],
            &[],
            false,
            vec![StaticPart::Type(StaticTypeChange {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Keep,
                subtypes: SubtypeOperation::Set(StaticSubtypeOperand::Literal(BTreeSet::from([
                    goblin,
                ]))),
            })],
        ),
    );
    let f = Fixture::new(
        v,
        vec![
            ("field", battlefield(), field),
            ("subject", battlefield(), all_types()),
        ],
        &["field", "subject"],
    );
    let result = evaluate(f.world()).unwrap();
    assert_eq!(
        result
            .get(f.object("subject"))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pair(3, 3)
    );
    assert_eq!(
        result
            .get(f.object("subject"))
            .unwrap()
            .current_creature_subtypes,
        BTreeSet::from([goblin])
    );
}

#[test]
fn a_one_stat_definition_keeps_the_other_printed_baseline() {
    let mut p = defining(vec![DefiningPart::PowerToughness {
        power: Some(Box::new(NumericExpression::Literal(9.into()))),
        toughness: None,
    }]);
    let ProgramKind::Permanent(d) = &mut p.kind else {
        unreachable!()
    };
    d.printed_pt = Some(pair(0, 4).into());
    let f = Fixture::new(
        vocabulary(),
        vec![("subject", ObjectDestination::Hand, p)],
        &[],
    );
    assert_eq!(
        evaluate(f.world())
            .unwrap()
            .get(f.object("subject"))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pair(9, 4)
    );
}

#[test]
fn seven_a_precedes_ordinary_set_modify_and_switch() {
    let mut p = defining(vec![count_part()]);
    let ProgramKind::Permanent(d) = &mut p.kind else {
        unreachable!()
    };
    d.printed_pt = Some(box_star());
    let ordinary = permanent(
        &[CardType::Artifact],
        (0, 0),
        &[],
        None,
        ability(
            &[CardType::Creature],
            &[],
            false,
            vec![
                StaticPart::SetPT(NumericPair {
                    power: NumericExpression::Literal(4.into()),
                    toughness: NumericExpression::Literal(5.into()),
                }),
                StaticPart::ModifyPT(NumericPair {
                    power: NumericExpression::Literal(1.into()),
                    toughness: NumericExpression::Literal(2.into()),
                }),
                StaticPart::SwitchPT,
            ],
        ),
    );
    let f = Fixture::new(
        vocabulary(),
        vec![
            ("ordinary", battlefield(), ordinary),
            ("subject", battlefield(), p),
        ],
        &["ordinary", "subject"],
    );
    let result = evaluate(f.world()).unwrap();
    let pt = result
        .get(f.object("subject"))
        .unwrap()
        .power_toughness
        .as_ref()
        .unwrap();
    assert_eq!(pt.base, pair(4, 5));
    assert_eq!(pt.derived, pair(7, 5));
    assert_eq!(
        result.trace.applied[0].bucket,
        CharacteristicBucket::DefinePT
    );
}

#[test]
fn affine_count_preserves_signed_and_huge_exact_arithmetic() {
    let factor: RulesInteger =
        RulesInteger::try_from("90071992547409931234567890".to_owned()).unwrap();
    let expr = NumericExpression::Count {
        selector: PermanentSelector {
            required_supertypes: std::collections::BTreeSet::new(),
            forbidden_supertypes: std::collections::BTreeSet::new(),
            required_types: BTreeSet::from([CardType::Land]),
            ..PermanentSelector::default()
        },
        multiplier: factor.clone(),
        offset: (-1).into(),
    };
    let f = Fixture::new(
        vocabulary(),
        vec![
            (
                "subject",
                ObjectDestination::Hand,
                defining(vec![DefiningPart::PowerToughness {
                    power: Some(Box::new(expr)),
                    toughness: Some(Box::new(NumericExpression::Literal((-3).into()))),
                }]),
            ),
            (
                "land1",
                battlefield(),
                Program::land(std::collections::BTreeSet::new(), vec![]),
            ),
            (
                "land2",
                battlefield(),
                Program::land(std::collections::BTreeSet::new(), vec![]),
            ),
        ],
        &["land1", "land2"],
    );
    let result = evaluate(f.world()).unwrap();
    let pt = &result
        .get(f.object("subject"))
        .unwrap()
        .power_toughness
        .as_ref()
        .unwrap()
        .derived;
    assert_eq!(pt.power, factor.times(&2.into()).minus(&1.into()));
    assert_eq!(pt.toughness, (-3).into());
}

#[test]
fn a_type_definition_cannot_give_a_noncreature_invalid_subtypes_or_erase_its_off_field_box() {
    let mut p = Program::artifact(None, vec![]);
    let ProgramKind::Permanent(d) = &mut p.kind else {
        unreachable!()
    };
    d.printed_pt = Some(pair(4, 5).into());
    p.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(vec![DefiningPart::CreatureTypes(
            DefiningCreatureTypes::All,
        )]),
    });
    let f = Fixture::new(
        vocabulary(),
        vec![
            ("hand", ObjectDestination::Hand, p.clone()),
            ("battlefield", battlefield(), p),
        ],
        &["battlefield"],
    );
    let result = evaluate(f.world()).unwrap();
    for alias in ["hand", "battlefield"] {
        let c = result.get(f.object(alias)).unwrap();
        assert!(!c.is_creature());
        assert!(c.current_creature_subtypes.is_empty());
    }
    assert_eq!(
        result
            .get(f.object("hand"))
            .unwrap()
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pair(4, 5)
    );
    assert!(
        result
            .get(f.object("battlefield"))
            .unwrap()
            .power_toughness
            .is_none()
    );
}

#[test]
fn printed_changeling_and_devoid_define_every_physical_zone_before_ability_removal() {
    let v = vocabulary();
    let all: BTreeSet<_> = v
        .members()
        .iter()
        .map(|name| v.resolve_label(name).unwrap())
        .collect();
    let mut card = Program::creature(None, 2.into(), 3.into(), BTreeSet::new())
        .with_printed_keywords([KeywordAbility::Changeling, KeywordAbility::Devoid]);
    card.color_indicator = Some(BTreeSet::from([Color::Red]));
    let mut rows: Vec<_> = zones()
        .into_iter()
        .map(|(alias, zone)| (alias, zone, card.clone()))
        .collect();
    rows.push(("remover", battlefield(), remover()));
    let fixture = Fixture::new(v, rows, &["battlefield", "remover"]);
    let result = evaluate(fixture.world()).unwrap();
    for (alias, _) in zones() {
        let current = result.get(fixture.object(alias)).unwrap();
        assert_eq!(current.current_creature_subtypes, all);
        assert!(current.current_colors.is_empty());
        assert_eq!(current.defined.colors, BTreeSet::from([Color::Red]));
        assert!(current.defined.creature_subtypes.is_empty());
        assert_eq!(
            current.has_keyword(KeywordAbility::Changeling),
            alias != "battlefield"
        );
        assert_eq!(
            current.has_keyword(KeywordAbility::Devoid),
            alias != "battlefield"
        );
    }
}
