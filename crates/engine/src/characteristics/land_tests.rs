//! CR 205.3d/i, 305.6-8 and 613: pure layer fixtures with source-qualified V.
//! Prepared physical worlds do not qualify root stack restoration or full cards.
use super::*;
use crate::{
    land::{BasicLandType, LandType},
    mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput, ManaType},
};

fn creator() -> StackEntryId {
    // Allocate a genuine entry ID; the layer-only fixture does not resolve it.
    let cards = GameObjects::new(0)
        .seed_card(0, CardId(999), P0, ObjectDestination::Hand)
        .unwrap()
        .next;
    let card = cards.current_identity(CardId(999)).unwrap();
    let window = crate::stack_window::StackWindow::new(vec![P0, SeatId(1)], P0, cards).unwrap();
    let window = window.finish_checks(window.revision()).unwrap();
    window
        .record_card_spell(window.revision(), P0, card)
        .unwrap()
        .entries()[0]
        .id()
}

fn land(types: impl IntoIterator<Item = LandType>) -> Program {
    Program::land(types.into_iter().collect(), vec![])
}
fn change(operation: SubtypeOperation<BTreeSet<LandType>, LandType>) -> ResolvedPart {
    ResolvedPart::Type(TypeChange {
        supertypes: TypeOperation::Keep,
        types: TypeOperation::Keep,
        subtypes: SubtypeOperation::Keep,
        land_subtypes: operation,
    })
}
fn apply(fixture: &mut Fixture, parts: Vec<ResolvedPart>) {
    fixture
        .state
        .add_effect(
            creator(),
            fixture.object("target"),
            ResolvedParts(parts),
            ResolvedDuration::NoStatedDuration,
        )
        .unwrap();
}
fn fixture(program: Program) -> Fixture {
    Fixture::new(
        vocabulary(),
        vec![("target", battlefield(), program)],
        &["target"],
    )
}
fn evaluated(fixture: &Fixture) -> ObjectCharacteristics {
    let saved = fixture.clone();
    let result = evaluate(fixture.world())
        .unwrap()
        .get(fixture.object("target"))
        .unwrap()
        .clone();
    assert_eq!(fixture, &saved);
    result
}
fn printed_land() -> Program {
    let mut program =
        land([LandType::Forest, LandType::Gate]).with_printed_keywords([KeywordAbility::Shroud]);
    program.mana = [0, u64::MAX]
        .map(|id| ManaAbility {
            id: ManaAbilityId(id),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())])),
        })
        .into();
    program.activated = vec![crate::activated::ActivatedAbility {
        id: crate::activated::ActivatedAbilityId(2),
        mana_cost: ManaCost::free(),
        source_cost: crate::activated::SourceCost::None,
        body: crate::program::ExecutableBody::new(
            crate::program::BodyTarget::SinglePermanent,
            vec![crate::program::Instruction::ReturnToOwnersHand],
        ),
    }];
    program.triggered = vec![crate::program::TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: crate::program::TriggeredAbilityId(3),
        condition: crate::program::TriggerSpec::becomes_tapped(
            crate::program::ObjectFilter::self_object(),
        ),
        body: crate::program::ExecutableBody::new(
            crate::program::BodyTarget::None,
            vec![crate::program::Instruction::DrawController {
                amount: crate::program::ValueExpr::Literal(0.into()),
            }],
        ),
    }];
    program.continuous = ability(
        &[CardType::Land],
        &[],
        false,
        vec![StaticPart::Color(ColorOperation::Add(
            StaticColorOperand::Literal(BTreeSet::from([Color::Green])),
        ))],
    );
    program.supertypes = BTreeSet::from([
        crate::program::Supertype::Legendary,
        crate::program::Supertype::Snow,
    ]);
    program
}

#[test]
fn all_basic_subsets_derive_in_every_physical_zone_independently_of_basic_supertype() {
    for mask in 0_u8..32 {
        let basics: BTreeSet<_> = BasicLandType::ALL
            .into_iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, value)| value)
            .collect();
        for basic_supertype in [false, true] {
            for destination in [
                ObjectDestination::LibraryTop,
                ObjectDestination::Hand,
                ObjectDestination::Graveyard,
                ObjectDestination::Exile,
                ObjectDestination::Command,
                ObjectDestination::Stack { controller: P0 },
                battlefield(),
            ] {
                let mut program = land(basics.iter().map(|value| value.land_type()));
                if basic_supertype {
                    program.supertypes.insert(crate::program::Supertype::Basic);
                }
                let order: &[&str] = if destination == battlefield() {
                    &["target"]
                } else {
                    &[]
                };
                let fixture =
                    Fixture::new(vocabulary(), vec![("target", destination, program)], order);
                let current = evaluated(&fixture);
                assert_eq!(current.intrinsic_mana, basics);
                assert_eq!(current.defined.land_subtypes, current.current_land_subtypes);
                assert_eq!(
                    current.mana_abilities(),
                    basics
                        .iter()
                        .copied()
                        .map(ManaAbilityRef::BasicLand)
                        .collect()
                );
                assert!(current.abilities.mana.is_empty());
                assert!(current.printed_text.mana.is_empty());
            }
        }
    }
    for subtype in LandType::ALL {
        let current = evaluated(&fixture(land([subtype])));
        assert_eq!(
            current.intrinsic_mana,
            subtype.basic().into_iter().collect()
        );
        assert_eq!(current.current_land_subtypes, BTreeSet::from([subtype]));
    }
}

#[test]
fn setting_basic_types_removes_printed_availability_but_add_remove_and_nonbasic_sets_preserve_it() {
    for operation in [
        SubtypeOperation::Keep,
        SubtypeOperation::Add(BTreeSet::from([LandType::Island])),
        SubtypeOperation::Remove(BTreeSet::from([LandType::Forest])),
        SubtypeOperation::Set(BTreeSet::new()),
        SubtypeOperation::Set(BTreeSet::from([LandType::Cave])),
    ] {
        let mut fixture = fixture(printed_land());
        let original = evaluated(&fixture);
        apply(&mut fixture, vec![change(operation.clone())]);
        let current = evaluated(&fixture);
        assert_eq!(current.abilities, original.abilities, "{operation:?}");
        assert!(current.has_keyword(KeywordAbility::Shroud));
        assert_eq!(current.printed_text, original.printed_text);
        assert_eq!(
            current.intrinsic_mana,
            current
                .current_land_subtypes
                .iter()
                .copied()
                .filter_map(LandType::basic)
                .collect()
        );
    }
    let mut fixture = fixture(printed_land());
    let original = evaluated(&fixture);
    let new_types = BTreeSet::from([LandType::Mountain, LandType::Island]);
    apply(
        &mut fixture,
        vec![
            change(SubtypeOperation::Set(new_types.clone())),
            ResolvedPart::Abilities(AbilityChange::grant_keywords([KeywordAbility::Haste])),
        ],
    );
    let current = evaluated(&fixture);
    assert_eq!(current.current_land_subtypes, new_types);
    assert_eq!(
        current.intrinsic_mana,
        BTreeSet::from([BasicLandType::Mountain, BasicLandType::Island])
    );
    assert!(current.abilities.identities().is_empty());
    assert_eq!(current.printed_text, original.printed_text);
    assert_eq!(current.defined, original.defined);
    assert_eq!(current.current_supertypes, original.current_supertypes);
    assert_eq!(current.current_types, original.current_types);
    assert_eq!(current.keywords, BTreeSet::from([KeywordAbility::Haste]));
    assert_eq!(
        current.printed_text.printed_keywords,
        BTreeSet::from([KeywordAbility::Shroud])
    );
    assert!(
        current.current_colors.is_empty(),
        "unstarted later color ability was removed in layer 4"
    );
}

#[test]
fn later_all_removes_intrinsic_mana_while_nonmana_keeps_it_and_printed_ids_cannot_collide() {
    for removal in [AbilityRemoval::All, AbilityRemoval::NonMana] {
        let mut fixture = fixture(printed_land());
        let original = evaluated(&fixture);
        assert_eq!(
            original.mana_abilities(),
            BTreeSet::from([
                ManaAbilityRef::Printed(ManaAbilityId(0)),
                ManaAbilityRef::Printed(ManaAbilityId(u64::MAX)),
                ManaAbilityRef::BasicLand(BasicLandType::Forest)
            ])
        );
        apply(
            &mut fixture,
            vec![
                change(SubtypeOperation::Add(BTreeSet::from([LandType::Swamp]))),
                ResolvedPart::Abilities(AbilityChange::remove(removal)),
            ],
        );
        let current = evaluated(&fixture);
        assert_eq!(
            current.current_land_subtypes,
            BTreeSet::from([LandType::Forest, LandType::Gate, LandType::Swamp])
        );
        if removal == AbilityRemoval::All {
            assert!(current.intrinsic_mana.is_empty());
            assert!(current.mana_abilities().is_empty());
        } else {
            assert_eq!(
                current.intrinsic_mana,
                BTreeSet::from([BasicLandType::Forest, BasicLandType::Swamp])
            );
            assert_eq!(current.abilities.mana, original.abilities.mana);
        }
        assert!(current.abilities.activated.is_empty());
        assert!(current.abilities.triggered.is_empty());
        assert!(current.abilities.continuous.is_none());
    }
}

#[test]
fn land_family_and_creature_family_have_independent_carriers_before_and_after_operations() {
    let v = vocabulary();
    let elf = v.resolve_label("Elf").unwrap();
    let mut program = printed_land();
    program
        .card_types
        .extend([CardType::Creature, CardType::Kindred]);
    program.creature_subtypes.insert(elf);
    let mut fixture = fixture(program);
    let mut lose_land = match change(SubtypeOperation::Add(BTreeSet::from([LandType::Plains]))) {
        ResolvedPart::Type(value) => value,
        _ => unreachable!(),
    };
    lose_land.types = TypeOperation::Remove(BTreeSet::from([CardType::Land, CardType::Creature]));
    apply(&mut fixture, vec![ResolvedPart::Type(lose_land)]);
    let current = evaluated(&fixture);
    assert_eq!(current.current_creature_subtypes, BTreeSet::from([elf]));
    assert!(current.current_land_subtypes.is_empty());
    assert!(current.intrinsic_mana.is_empty());
    assert!(!current.is_creature());
    assert!(current.power_toughness.is_none());
    let ResolvedPart::Type(mut restore) =
        change(SubtypeOperation::Add(BTreeSet::from([LandType::Island])))
    else {
        unreachable!()
    };
    restore.types = TypeOperation::Add(BTreeSet::from([CardType::Land]));
    apply(&mut fixture, vec![ResolvedPart::Type(restore)]);
    assert_eq!(
        evaluated(&fixture).current_land_subtypes,
        BTreeSet::from([LandType::Island])
    );
    assert_eq!(
        evaluated(&fixture).intrinsic_mana,
        BTreeSet::from([BasicLandType::Island])
    );
}

#[test]
fn setting_basic_subtypes_without_the_land_carrier_cannot_remove_printed_text() {
    let mut program = printed_land();
    program.card_types = BTreeSet::from([CardType::Artifact]);
    program.land_subtypes.clear();
    let mut fixture = fixture(program);
    let original = evaluated(&fixture);
    apply(
        &mut fixture,
        vec![change(SubtypeOperation::Set(BTreeSet::from([
            LandType::Island,
        ])))],
    );
    let current = evaluated(&fixture);
    assert!(current.current_land_subtypes.is_empty());
    assert!(current.intrinsic_mana.is_empty());
    assert_eq!(current.abilities, original.abilities);
    assert_eq!(current.printed_text, original.printed_text);
}

#[test]
fn source_current_land_operands_regenerate_after_a_dependency_changes_the_source() {
    let mut donor = land([LandType::Forest]);
    donor.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Land]),
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
            land_subtypes: SubtypeOperation::Add(
                StaticLandSubtypeOperand::SourceCurrentLandSubtypes,
            ),
        })]),
    });
    let mut fixture = Fixture::new(
        vocabulary(),
        vec![
            ("donor", battlefield(), donor),
            ("target", battlefield(), land([LandType::Cave])),
        ],
        &["donor", "target"],
    );
    let effect = fixture
        .state
        .add_effect(
            creator(),
            fixture.object("donor"),
            ResolvedParts(vec![change(SubtypeOperation::Add(BTreeSet::from([
                LandType::Island,
            ])))]),
            ResolvedDuration::NoStatedDuration,
        )
        .unwrap();
    let result = evaluate(fixture.world()).unwrap();
    let current = result.get(fixture.object("target")).unwrap();
    assert_eq!(
        current.current_land_subtypes,
        BTreeSet::from([LandType::Cave, LandType::Forest, LandType::Island])
    );
    assert_eq!(
        current.intrinsic_mana,
        BTreeSet::from([BasicLandType::Forest, BasicLandType::Island])
    );
    let type_steps: Vec<_> = result
        .trace
        .applied
        .iter()
        .filter(|part| part.bucket == CharacteristicBucket::Type)
        .map(|part| part.key)
        .collect();
    assert_eq!(
        type_steps,
        vec![EffectInstanceKey::Resolved(effect.id), fixture.key("donor")]
    );
    assert_eq!(current.abilities.mana.len(), 0);
}

#[test]
fn basic_type_text_loss_preserves_effects_that_already_started_in_an_earlier_layer() {
    let mut program = land([LandType::Forest]);
    program.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector::default(),
        parts: StaticParts(vec![
            StaticPart::Color(ColorOperation::Add(StaticColorOperand::Literal(
                BTreeSet::from([Color::Red]),
            ))),
            StaticPart::Names(NameOperation::Set(StaticNameOperand::Literal(
                spellwake_contracts::names::ObjectNames::nameless(),
            ))),
        ]),
    });
    let mut fixture = fixture(program);
    apply(
        &mut fixture,
        vec![change(SubtypeOperation::Set(BTreeSet::from([
            LandType::Mountain,
        ])))],
    );
    let result = evaluate(fixture.world()).unwrap();
    let current = result.get(fixture.object("target")).unwrap();
    assert_eq!(current.abilities.continuous, None);
    assert_eq!(current.current_colors, BTreeSet::from([Color::Red]));
    assert_eq!(
        result.trace.bindings[&fixture.key("target")],
        PassBinding::Processed(fixture.affected(&["target"]))
    );
    assert!(result.trace.applied.iter().any(
        |part| part.key == fixture.key("target") && part.bucket == CharacteristicBucket::Color
    ));
    assert!(current.printed_text.continuous.is_some());
}

#[test]
fn land_filter_counts_current_controller_battlefield_and_intersects_creature_filter_for_affine_pt()
{
    let v = vocabulary();
    let goblin = v.resolve_label("Goblin").unwrap();
    let forests = PermanentSelector {
        land_subtype: Some(LandType::Forest),
        controller: ControllerSelector::SourceController,
        ..PermanentSelector::default()
    };
    let mut generator = Program::artifact(None, vec![]);
    generator.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::SetPT(NumericPair {
            power: NumericExpression::Count {
                selector: forests.clone(),
                multiplier: 2.into(),
                offset: 1.into(),
            },
            toughness: NumericExpression::Count {
                selector: PermanentSelector {
                    creature_subtype: Some(goblin),
                    ..forests
                },
                multiplier: 3.into(),
                offset: 1.into(),
            },
        })]),
    });
    let mut target = Program::creature(None, 2.into(), 3.into(), BTreeSet::from([goblin]));
    target.card_types.insert(CardType::Land);
    target.land_subtypes.insert(LandType::Forest);
    let mut fixture = Fixture::new(
        v,
        vec![
            ("generator", battlefield(), generator),
            ("target", battlefield(), target),
            ("own", battlefield(), land([LandType::Forest])),
            ("hand", ObjectDestination::Hand, land([LandType::Forest])),
            (
                "other",
                ObjectDestination::Battlefield {
                    controller: SeatId(1),
                },
                land([LandType::Forest]),
            ),
        ],
        &["generator", "target", "own", "other"],
    );
    assert_eq!(
        evaluated(&fixture).power_toughness.unwrap().derived,
        pair(5, 4)
    );
    apply(
        &mut fixture,
        vec![change(SubtypeOperation::Set(BTreeSet::from([
            LandType::Island,
        ])))],
    );
    assert_eq!(
        evaluated(&fixture).power_toughness.unwrap().derived,
        pair(3, 1)
    );
}

#[test]
fn creature_text_substitution_rewrites_creature_words_and_preserves_land_operands_and_filter() {
    let v = vocabulary();
    let elf = v.resolve_label("Elf").unwrap();
    let goblin = v.resolve_label("Goblin").unwrap();
    let mut source = Program::artifact(None, vec![]);
    source.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            creature_subtype: Some(elf),
            land_subtype: Some(LandType::Forest),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
            land_subtypes: SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(
                BTreeSet::from([LandType::Island]),
            )),
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Add(StaticSubtypeOperand::Literal(BTreeSet::from([elf]))),
        })]),
    });
    let mut target = Program::creature(None, 2.into(), 3.into(), BTreeSet::from([goblin]));
    target.card_types.insert(CardType::Land);
    target.land_subtypes.insert(LandType::Forest);
    let mut fixture = Fixture::new(
        v,
        vec![
            ("source", battlefield(), source),
            ("target", battlefield(), target),
        ],
        &["source", "target"],
    );
    fixture
        .state
        .add_effect(
            creator(),
            fixture.object("source"),
            ResolvedParts(vec![ResolvedPart::Text(CreatureTypeTextChange {
                from: elf,
                to: goblin,
            })]),
            ResolvedDuration::NoStatedDuration,
        )
        .unwrap();
    let result = evaluate(fixture.world()).unwrap();
    let Some(StaticContinuousAbility::Battlefield {
        selector, parts, ..
    }) = &result
        .get(fixture.object("source"))
        .unwrap()
        .printed_text
        .continuous
    else {
        panic!()
    };
    assert_eq!(selector.creature_subtype, Some(goblin));
    assert_eq!(selector.land_subtype, Some(LandType::Forest));
    let StaticPart::Type(part) = &parts.0[0] else {
        panic!()
    };
    assert_eq!(
        part.subtypes,
        SubtypeOperation::Add(StaticSubtypeOperand::Literal(BTreeSet::from([goblin])))
    );
    assert_eq!(
        part.land_subtypes,
        SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(BTreeSet::from([
            LandType::Island
        ])))
    );
    let current = result.get(fixture.object("target")).unwrap();
    assert_eq!(current.current_creature_subtypes, BTreeSet::from([goblin]));
    assert_eq!(
        current.current_land_subtypes,
        BTreeSet::from([LandType::Forest, LandType::Island])
    );
}

#[test]
fn copied_printed_keywords_survive_in_text_after_removal_and_later_grants() {
    use KeywordAbility::{Flying, Reach, Shroud};
    for removal in [AbilityRemoval::All, AbilityRemoval::NonMana] {
        let mut fixture = fixture(land([]).with_printed_keywords([Shroud]));
        let donor = land([]).with_printed_keywords([Flying]);
        assert_eq!(evaluated(&fixture).keywords, BTreeSet::from([Shroud]));
        apply(&mut fixture, vec![ResolvedPart::Copy(Box::new(donor))]);
        assert_eq!(evaluated(&fixture).keywords, BTreeSet::from([Flying]));
        apply(
            &mut fixture,
            vec![ResolvedPart::Abilities(AbilityChange::remove(removal))],
        );
        assert!(evaluated(&fixture).keywords.is_empty());
        apply(
            &mut fixture,
            vec![ResolvedPart::Abilities(AbilityChange::grant_keywords([
                Reach,
            ]))],
        );
        let current = evaluated(&fixture);
        assert_eq!(current.keywords, BTreeSet::from([Reach]));
        assert_eq!(
            current.printed_text.printed_keywords,
            BTreeSet::from([Flying])
        );
    }
}
