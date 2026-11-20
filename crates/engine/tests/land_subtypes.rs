//! Land-family mechanism contracts, not complete card or format qualification.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    land::LandType,
    mana::ManaCost,
    program::{
        BodyTarget, CardType, DefinitionError, ExecutableBody, Instruction, InstructionAmount,
        Program, Supertype,
    },
};
use std::collections::BTreeSet;

fn land_static(
    operation: spellwake_engine::characteristics::SubtypeOperation<
        spellwake_engine::characteristics::StaticLandSubtypeOperand,
        LandType,
    >,
    nonbasic_only: bool,
) -> spellwake_engine::characteristics::StaticContinuousAbility {
    use spellwake_engine::characteristics::*;
    StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Land]),
            forbidden_supertypes: if nonbasic_only {
                BTreeSet::from([Supertype::Basic])
            } else {
                BTreeSet::new()
            },
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
            land_subtypes: operation,
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
        })]),
    }
}

// Full expected trace derives from CR613.8: Urborg depends on Moon because Moon
// removes its source ability; Moon does not depend on adding Swamp. No loop or
// timestamp-derived edge is expected. After Moon leaves only Urborg generates.
fn assert_moon_urborg_trace(
    game: &spellwake_engine::game::GameState,
    moon: spellwake_engine::object_identity::ObjectId,
    urborg: spellwake_engine::object_identity::ObjectId,
    forest: spellwake_engine::object_identity::ObjectId,
    moon_first: bool,
    moon_present: bool,
) {
    use spellwake_engine::characteristics::{
        AppliedComponent, CharacteristicBucket, ComponentSelection, ComponentWait,
        EffectInstanceKey, EvaluationTrace, GeneratedComponent, PassBinding, ResolvedPart,
        StaticAbilityId, SubtypeOperation, TypeChange, TypeOperation,
    };
    use std::collections::BTreeMap;
    let moon_key = EffectInstanceKey::Static {
        source: moon,
        ability: StaticAbilityId(1),
    };
    let urborg_key = EffectInstanceKey::Static {
        source: urborg,
        ability: StaticAbilityId(1),
    };
    let type_part = |land_subtypes| {
        ResolvedPart::Type(TypeChange {
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
            land_subtypes,
        })
    };
    let mountain = type_part(SubtypeOperation::Set(BTreeSet::from([LandType::Mountain])));
    let swamp = type_part(SubtypeOperation::Add(BTreeSet::from([LandType::Swamp])));
    let generated = |key, affected: BTreeSet<_>, part: ResolvedPart| GeneratedComponent {
        key,
        bucket: CharacteristicBucket::Type,
        present: true,
        generated: affected
            .iter()
            .map(|object| (*object, part.clone()))
            .collect(),
        affected,
    };
    let moon_component = generated(moon_key, BTreeSet::from([urborg]), mountain);
    let urborg_component = generated(urborg_key, BTreeSet::from([urborg, forest]), swamp);
    let applied = |value: &GeneratedComponent| AppliedComponent {
        key: value.key,
        bucket: value.bucket,
        affected: value.affected.clone(),
        generated: value.generated.clone(),
    };
    let expected = if moon_present {
        let waits = BTreeSet::from([ComponentWait {
            waiting: urborg_key,
            prerequisite: moon_key,
        }]);
        let ordered = if moon_first {
            vec![moon_component.clone(), urborg_component]
        } else {
            vec![urborg_component, moon_component.clone()]
        };
        let components = ordered
            .iter()
            .map(|value| BTreeSet::from([value.key]))
            .collect();
        EvaluationTrace {
            selections: vec![
                ComponentSelection {
                    bucket: CharacteristicBucket::Type,
                    generated: ordered,
                    original_waits: waits.clone(),
                    strongly_connected: components,
                    derived_timestamp_waits: BTreeSet::new(),
                    retained_outside_waits: waits,
                    eligible: vec![moon_key],
                    chosen: moon_key,
                },
                ComponentSelection {
                    bucket: CharacteristicBucket::Type,
                    generated: vec![GeneratedComponent {
                        key: urborg_key,
                        bucket: CharacteristicBucket::Type,
                        present: false,
                        affected: BTreeSet::new(),
                        generated: BTreeMap::new(),
                    }],
                    original_waits: BTreeSet::new(),
                    strongly_connected: vec![BTreeSet::from([urborg_key])],
                    derived_timestamp_waits: BTreeSet::new(),
                    retained_outside_waits: BTreeSet::new(),
                    eligible: vec![urborg_key],
                    chosen: urborg_key,
                },
            ],
            applied: vec![applied(&moon_component)],
            counter_applications: vec![],
            bindings: BTreeMap::from([
                (moon_key, PassBinding::Processed(BTreeSet::from([urborg]))),
                (urborg_key, PassBinding::NotProcessed),
            ]),
        }
    } else {
        EvaluationTrace {
            selections: vec![ComponentSelection {
                bucket: CharacteristicBucket::Type,
                generated: vec![urborg_component.clone()],
                original_waits: BTreeSet::new(),
                strongly_connected: vec![BTreeSet::from([urborg_key])],
                derived_timestamp_waits: BTreeSet::new(),
                retained_outside_waits: BTreeSet::new(),
                eligible: vec![urborg_key],
                chosen: urborg_key,
            }],
            applied: vec![applied(&urborg_component)],
            counter_applications: vec![],
            bindings: BTreeMap::from([(
                urborg_key,
                PassBinding::Processed(BTreeSet::from([urborg, forest])),
            )]),
        }
    };
    let canonical_before = game.clone();
    let observed = game.execution().evaluated_characteristics().unwrap();
    assert_eq!(observed.trace, expected);
    assert_eq!(
        game, &canonical_before,
        "pure trace query must preserve every canonical owner/history/resource field"
    );
}

#[test]
fn moon_urborg_dependency_ignores_timestamp_order_and_actual_source_departure_restores() {
    use spellwake_engine::{
        characteristics::{StaticLandSubtypeOperand, SubtypeOperation},
        land::BasicLandType,
        object_identity::{CardId, ObjectZone},
    };
    use trajectories::*;
    for moon_first in [false, true] {
        let mut moon = Program::enchantment(None);
        moon.continuous = Some(land_static(
            SubtypeOperation::Set(StaticLandSubtypeOperand::Literal(BTreeSet::from([
                LandType::Mountain,
            ]))),
            true,
        ));
        let mut urborg = Program::land(BTreeSet::new(), vec![]);
        urborg.supertypes.insert(Supertype::Legendary);
        urborg.continuous = Some(land_static(
            SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(BTreeSet::from([
                LandType::Swamp,
            ]))),
            false,
        ));
        let mut forest = Program::land(BTreeSet::from([LandType::Forest]), vec![]);
        forest.supertypes.insert(Supertype::Basic);
        let game = world(
            vocabulary(),
            vec![
                card(1, moon, true),
                card(2, urborg, true),
                card(3, forest, true),
                spell(
                    4,
                    BodyTarget::SinglePermanent,
                    vec![Instruction::ReturnToOwnersHand],
                ),
            ],
            if moon_first {
                vec![CardId(1), CardId(2), CardId(3)]
            } else {
                vec![CardId(2), CardId(1), CardId(3)]
            },
        );
        let moon_id = object(&game, 1);
        let urborg_id = object(&game, 2);
        let forest_id = object(&game, 3);
        assert_moon_urborg_trace(&game, moon_id, urborg_id, forest_id, moon_first, true);
        let before = current(&game, urborg_id);
        assert_eq!(
            before.current_land_subtypes,
            BTreeSet::from([LandType::Mountain])
        );
        assert_eq!(
            before.intrinsic_mana,
            BTreeSet::from([BasicLandType::Mountain])
        );
        assert_eq!(before.abilities.continuous, None);
        assert!(before.printed_text.continuous.is_some());
        assert_eq!(
            current(&game, forest_id).current_land_subtypes,
            BTreeSet::from([LandType::Forest])
        );
        let saved = game.clone();
        let (bounced, _) = resolve(&game, 4, moon_id);
        assert_eq!(game, saved);
        assert_moon_urborg_trace(
            bounced.state(),
            moon_id,
            urborg_id,
            forest_id,
            moon_first,
            false,
        );
        let after = current(bounced.state(), urborg_id);
        assert_eq!(
            after.current_land_subtypes,
            BTreeSet::from([LandType::Swamp])
        );
        assert_eq!(after.intrinsic_mana, BTreeSet::from([BasicLandType::Swamp]));
        assert!(after.abilities.continuous.is_some());
        assert_eq!(after.defined, before.defined);
        assert_eq!(after.printed_text, before.printed_text);
        let forest = current(bounced.state(), forest_id);
        assert_eq!(
            forest.current_land_subtypes,
            BTreeSet::from([LandType::Forest, LandType::Swamp])
        );
        assert_eq!(
            forest.intrinsic_mana,
            BTreeSet::from([BasicLandType::Forest, BasicLandType::Swamp])
        );
        assert_eq!(
            bounced
                .state()
                .execution()
                .window()
                .cards()
                .get(object(bounced.state(), 1))
                .unwrap()
                .zone(),
            ObjectZone::Hand(P0)
        );
        assert_ne!(object(bounced.state(), 1), moon_id);
        assert!(
            bounced
                .state()
                .execution()
                .window()
                .cards()
                .get(moon_id)
                .is_none()
        );
        assert!(!bounced.rules_journal().records.is_empty());
    }
}

#[test]
fn ordinary_play_then_intrinsic_tap_uses_exact_source_cost_pool_publication_and_root_receipt() {
    use spellwake_engine::{
        game::{GameCommand, GameInput},
        land::BasicLandType,
        mana::ManaAbilityRef,
        occurrence::{EventCause, RulesOccurrence, RulesRecord, StatusReason},
    };
    use trajectories::*;
    for basic in BasicLandType::ALL {
        for is_basic in [false, true] {
            let mut program = Program::land(BTreeSet::from([basic.land_type()]), vec![]);
            if is_basic {
                program.supertypes.insert(Supertype::Basic);
            }
            let game = world(vocabulary(), vec![card(1, program, false)], vec![]);
            let hand = object(&game, 1);
            let played = submit(&game, P0, GameCommand::PlayLand { land: hand });
            let source = object(played.state(), 1);
            assert_ne!(source, hand);
            assert_eq!(played.land_play().unwrap().after.id(), source);
            let state = played.state();
            assert_eq!(
                current(state, source).intrinsic_mana,
                BTreeSet::from([basic])
            );
            assert!(current(state, source).abilities.mana.is_empty());
            assert!(current(state, source).printed_text.mana.is_empty());
            let saved = state.clone();
            assert!(
                state
                    .advance(GameInput {
                        basis: state.basis().clone(),
                        actor: P0,
                        command: GameCommand::ActivateMana {
                            source: hand,
                            ability: ManaAbilityRef::BasicLand(basic)
                        }
                    })
                    .is_err()
            );
            assert_eq!(state, &saved);
            let candidate = submit(
                state,
                P0,
                GameCommand::ActivateMana {
                    source,
                    ability: ManaAbilityRef::BasicLand(basic),
                },
            );
            let production = candidate.mana_production().unwrap();
            assert_eq!(production.source, source);
            assert_eq!(production.ability, ManaAbilityRef::BasicLand(basic));
            assert_eq!(production.controller, P0);
            assert_eq!(
                production.produced,
                std::collections::BTreeMap::from([(basic.mana_type(), 1.into())])
            );
            assert!(candidate.mana_payment().is_none());
            assert!(candidate.ability_activation().is_none());
            assert!(candidate.land_play().is_none());
            assert_eq!(
                candidate
                    .state()
                    .execution()
                    .window()
                    .cards()
                    .get(source)
                    .unwrap()
                    .tapped(),
                Some(true)
            );
            assert!(candidate.state().execution().window().entries().is_empty());
            for mana in spellwake_engine::mana::ManaType::ALL {
                assert_eq!(
                    candidate.state().execution().mana(P0, mana),
                    if mana == basic.mana_type() {
                        1.into()
                    } else {
                        0.into()
                    }
                );
                assert_eq!(candidate.state().execution().mana(P1, mana), 0.into());
            }
            let events: Vec<_> = candidate
                .rules_journal()
                .records
                .iter()
                .filter_map(|record| match record {
                    RulesRecord::Event(event) => Some(event),
                    _ => None,
                })
                .collect();
            assert_eq!(events.len(), 3);
            assert_eq!(
                events[0].cause,
                EventCause::ManaAbilityCost {
                    source,
                    ability: production.ability
                }
            );
            assert_eq!(
                events[1].cause,
                EventCause::ManaAbilityActivated {
                    source,
                    ability: production.ability
                }
            );
            assert_eq!(
                events[2].cause,
                EventCause::ManaProduced {
                    source,
                    ability: production.ability
                }
            );
            let facts = all_occurrences(&candidate);
            assert_eq!(facts.len(), 3);
            assert!(matches!(
                facts[0],
                RulesOccurrence::StatusChange {
                    reason: StatusReason::ManaAbilityCost,
                    ..
                }
            ));
            assert!(
                matches!(facts[1],RulesOccurrence::ManaAbility {ability,source:card,controller} if *ability==production.ability && card.id()==source && *controller==P0)
            );
            assert!(
                matches!(facts[2],RulesOccurrence::ManaProduced {production:p,before,after} if p==production && before.values().all(|amount|*amount==0.into()) && after[&basic.mana_type()]==1.into())
            );
            let tapped = candidate.state();
            let saved = tapped.clone();
            assert!(
                tapped
                    .advance(GameInput {
                        basis: tapped.basis().clone(),
                        actor: P0,
                        command: GameCommand::ActivateMana {
                            source,
                            ability: ManaAbilityRef::BasicLand(basic)
                        }
                    })
                    .is_err()
            );
            assert_eq!(tapped, &saved);
        }
    }
}

#[test]
fn every_basic_subset_activates_each_intrinsic_ability_separately() {
    use spellwake_engine::{
        game::GameCommand, land::BasicLandType, mana::ManaAbilityRef, object_identity::CardId,
    };
    use trajectories::*;
    for mask in 0_u8..32 {
        let selected: Vec<_> = BasicLandType::ALL
            .into_iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, value)| value)
            .collect();
        let game = world(
            vocabulary(),
            vec![card(
                1,
                Program::land(
                    selected.iter().map(|value| value.land_type()).collect(),
                    vec![],
                ),
                true,
            )],
            vec![CardId(1)],
        );
        let source = object(&game, 1);
        assert_eq!(
            current(&game, source).mana_abilities(),
            selected
                .iter()
                .copied()
                .map(ManaAbilityRef::BasicLand)
                .collect()
        );
        for kind in selected {
            let result = submit(
                &game,
                P0,
                GameCommand::ActivateMana {
                    source,
                    ability: ManaAbilityRef::BasicLand(kind),
                },
            );
            assert_eq!(
                result.mana_production().unwrap().produced,
                std::collections::BTreeMap::from([(kind.mana_type(), 1.into())])
            );
            assert_eq!(
                result.mana_production().unwrap().ability,
                ManaAbilityRef::BasicLand(kind)
            );
            assert_eq!(
                game.execution()
                    .window()
                    .cards()
                    .get(source)
                    .unwrap()
                    .tapped(),
                Some(false)
            );
        }
    }
}

#[test]
fn printed_zero_and_max_ids_are_distinct_from_intrinsic_and_bad_requests_are_atomic() {
    use spellwake_engine::{
        game::{GameCommand, GameInput},
        land::BasicLandType,
        mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput, ManaType},
        object_identity::CardId,
    };
    use trajectories::*;
    let mut program = Program::land(BTreeSet::from([LandType::Forest]), vec![]);
    program.mana = vec![
        ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::Fixed(std::collections::BTreeMap::from([(
                ManaType::Colourless,
                1.into(),
            )])),
        },
        ManaAbility {
            id: ManaAbilityId(u64::MAX),
            output: ManaOutput::Fixed(std::collections::BTreeMap::from([(
                ManaType::Colourless,
                2.into(),
            )])),
        },
    ];
    let game = world(
        vocabulary(),
        vec![card(1, program.clone(), true), card(2, program, false)],
        vec![CardId(1)],
    );
    let source = object(&game, 1);
    for (ability, kind, amount) in [
        (
            ManaAbilityRef::Printed(ManaAbilityId(0)),
            ManaType::Colourless,
            1,
        ),
        (
            ManaAbilityRef::Printed(ManaAbilityId(u64::MAX)),
            ManaType::Colourless,
            2,
        ),
        (
            ManaAbilityRef::BasicLand(BasicLandType::Forest),
            ManaType::Green,
            1,
        ),
    ] {
        let candidate = submit(&game, P0, GameCommand::ActivateMana { source, ability });
        assert_eq!(candidate.mana_production().unwrap().ability, ability);
        assert_eq!(
            candidate.mana_production().unwrap().produced,
            std::collections::BTreeMap::from([(kind, amount.into())])
        );
    }
    for (actor, selected_source, ability, stale) in [
        (P0, source, ManaAbilityRef::Printed(ManaAbilityId(5)), false),
        (
            P0,
            source,
            ManaAbilityRef::BasicLand(BasicLandType::Mountain),
            false,
        ),
        (
            P1,
            source,
            ManaAbilityRef::BasicLand(BasicLandType::Forest),
            false,
        ),
        (
            P0,
            object(&game, 2),
            ManaAbilityRef::BasicLand(BasicLandType::Forest),
            false,
        ),
        (
            P0,
            source,
            ManaAbilityRef::BasicLand(BasicLandType::Forest),
            true,
        ),
    ] {
        let saved = game.clone();
        let mut basis = game.basis().clone();
        if stale {
            basis.revision = basis.revision.checked_sub(1).unwrap();
        }
        assert!(
            game.advance(GameInput {
                basis,
                actor,
                command: GameCommand::ActivateMana {
                    source: selected_source,
                    ability
                }
            })
            .is_err()
        );
        assert_eq!(game, saved);
    }
}

#[test]
fn newly_played_land_creature_requires_haste_and_all_removal_disables_intrinsic_mana() {
    use spellwake_engine::{
        characteristics::{
            AbilityChange, AbilityRemoval, KeywordAbility, PermanentSelector, ResolvedDuration,
            ResolvedPartSpec, StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
        },
        game::{GameCommand, GameInput},
        land::BasicLandType,
        mana::ManaAbilityRef,
        object_identity::CardId,
    };
    use trajectories::*;
    for removal in [
        None,
        Some(AbilityRemoval::NonMana),
        Some(AbilityRemoval::All),
    ] {
        let mut program = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
        program.card_types.insert(CardType::Land);
        program.land_subtypes.insert(LandType::Forest);
        let mut cards = vec![
            card(1, program, false),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::Abilities(AbilityChange::grant_keywords(
                        [KeywordAbility::Haste],
                    ))],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ];
        if let Some(removal) = removal {
            let mut remover = Program::artifact(None, vec![]);
            remover.continuous = Some(StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(1),
                selector: PermanentSelector {
                    required_types: BTreeSet::from([CardType::Land]),
                    ..PermanentSelector::default()
                },
                parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::remove(removal))]),
            });
            cards.push(card(3, remover, true));
        }
        let game = world(
            vocabulary(),
            cards,
            if removal.is_some() {
                vec![CardId(3)]
            } else {
                vec![]
            },
        );
        let played = submit(
            &game,
            P0,
            GameCommand::PlayLand {
                land: object(&game, 1),
            },
        );
        let source = object(played.state(), 1);
        let state = played.state();
        let saved = state.clone();
        assert_eq!(
            state
                .execution()
                .window()
                .cards()
                .get(source)
                .unwrap()
                .has_continuous_control_since_turn_start(),
            Some(false)
        );
        assert!(
            state
                .advance(GameInput {
                    basis: state.basis().clone(),
                    actor: P0,
                    command: GameCommand::ActivateMana {
                        source,
                        ability: ManaAbilityRef::BasicLand(BasicLandType::Forest)
                    }
                })
                .is_err()
        );
        assert_eq!(state, &saved);
        let (haste, _) = resolve(state, 2, source);
        assert!(current(haste.state(), source).has_keyword(KeywordAbility::Haste));
        assert_eq!(
            haste
                .state()
                .execution()
                .window()
                .cards()
                .get(source)
                .unwrap()
                .has_continuous_control_since_turn_start(),
            Some(false)
        );
        if removal == Some(AbilityRemoval::All) {
            let state = haste.state();
            let saved = state.clone();
            assert!(current(state, source).mana_abilities().is_empty());
            assert!(
                state
                    .advance(GameInput {
                        basis: state.basis().clone(),
                        actor: P0,
                        command: GameCommand::ActivateMana {
                            source,
                            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest)
                        }
                    })
                    .is_err()
            );
            assert_eq!(state, &saved);
        } else {
            let tapped = submit(
                haste.state(),
                P0,
                GameCommand::ActivateMana {
                    source,
                    ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
                },
            );
            assert_eq!(
                tapped.mana_production().unwrap().produced,
                std::collections::BTreeMap::from([(BasicLandType::Forest.mana_type(), 1.into())])
            );
        }
    }
}

#[test]
fn counted_mana_uses_the_shared_current_land_filter_and_excludes_hand_and_opponents() {
    use spellwake_engine::{
        characteristics::{
            ControllerSelector, PermanentSelector, ResolvedDuration, ResolvedPartSpec,
            SubtypeOperation, TypeChangeSpec, TypeOperation,
        },
        game::GameCommand,
        mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput, ManaType},
        object_identity::CardId,
    };
    use trajectories::*;
    let mut source = Program::artifact(None, vec![]);
    source.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Counted {
            mana_type: ManaType::Green,
            permanents: PermanentSelector {
                land_subtype: Some(LandType::Forest),
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            },
        },
    });
    let game = world(
        vocabulary(),
        vec![
            card(1, source, true),
            card(
                2,
                Program::land(BTreeSet::from([LandType::Forest]), vec![]),
                true,
            ),
            card(
                3,
                Program::land(BTreeSet::from([LandType::Forest]), vec![]),
                true,
            ),
            controlled(
                4,
                P1,
                P1,
                Program::land(BTreeSet::from([LandType::Forest]), vec![]),
            ),
            card(
                5,
                Program::land(BTreeSet::from([LandType::Forest]), vec![]),
                false,
            ),
            spell(
                6,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Set(BTreeSet::from([LandType::Island])),
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Keep,
                    })],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );
    let source = object(&game, 1);
    let first = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        first.mana_production().unwrap().produced,
        std::collections::BTreeMap::from([(ManaType::Green, 2.into())])
    );
    let (changed, _) = resolve(&game, 6, object(&game, 2));
    let second = submit(
        changed.state(),
        P0,
        GameCommand::ActivateMana {
            source,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        second.mana_production().unwrap().produced,
        std::collections::BTreeMap::from([(ManaType::Green, 1.into())])
    );
}

#[test]
fn physical_land_copy_retains_authored_family_and_text_through_type_changes_and_fresh_reentry() {
    use spellwake_engine::{
        activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
        characteristics::{
            ResolvedDuration, ResolvedPartSpec, SubtypeOperation, TypeChangeSpec, TypeOperation,
        },
        game::GameCommand,
        land::BasicLandType,
        mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput, ManaPayment, ManaType},
        object_identity::CardId,
    };
    use trajectories::*;
    let donor = Program::land(
        BTreeSet::from([LandType::Forest]),
        vec![ManaAbility {
            id: ManaAbilityId(7),
            output: ManaOutput::Fixed(std::collections::BTreeMap::from([(
                ManaType::Colourless,
                1.into(),
            )])),
        }],
    );
    let receiver = Program::artifact(
        Some(ManaCost::free()),
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::CopyTargetToSource {
                    duration: ResolvedDuration::NoStatedDuration,
                    power_toughness: None,
                }],
            ),
        }],
    );
    let change = |kind, duration| {
        vec![effect(
            vec![ResolvedPartSpec::Type(TypeChangeSpec {
                land_subtypes: SubtypeOperation::Set(BTreeSet::from([kind])),
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Keep,
                subtypes: SubtypeOperation::Keep,
            })],
            duration,
        )]
    };
    let game = world(
        vocabulary(),
        vec![
            card(1, donor, true),
            card(2, receiver, true),
            spell(
                3,
                BodyTarget::SinglePermanent,
                change(LandType::Island, ResolvedDuration::NoStatedDuration),
            ),
            spell(
                4,
                BodyTarget::SinglePermanent,
                change(LandType::Mountain, ResolvedDuration::UntilEndOfTurn),
            ),
            spell(
                5,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
            spell(
                6,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let donor = object(&game, 1);
    let receiver = object(&game, 2);
    let (changed, _) = resolve(&game, 3, donor);
    assert_eq!(
        current(changed.state(), donor).current_land_subtypes,
        BTreeSet::from([LandType::Island])
    );
    assert!(current(changed.state(), donor).abilities.mana.is_empty());
    let activated = activate(changed.state(), receiver, donor, false);
    let copied = pass_pair(activated.state());
    let selected = current(copied.state(), receiver);
    assert_eq!(
        selected.current_land_subtypes,
        BTreeSet::from([LandType::Forest])
    );
    assert_eq!(
        selected.defined.land_subtypes,
        selected.current_land_subtypes
    );
    assert_eq!(
        selected.mana_abilities(),
        BTreeSet::from([
            ManaAbilityRef::Printed(ManaAbilityId(7)),
            ManaAbilityRef::BasicLand(BasicLandType::Forest)
        ])
    );
    for (ability, mana_type) in [
        (
            ManaAbilityRef::Printed(ManaAbilityId(7)),
            ManaType::Colourless,
        ),
        (
            ManaAbilityRef::BasicLand(BasicLandType::Forest),
            ManaType::Green,
        ),
    ] {
        let tapped = submit(
            copied.state(),
            P0,
            GameCommand::ActivateMana {
                source: receiver,
                ability,
            },
        );
        assert_eq!(
            tapped.mana_production().unwrap().produced,
            std::collections::BTreeMap::from([(mana_type, 1.into())])
        );
        assert_eq!(tapped.mana_production().unwrap().ability, ability);
    }
    let (donor_left, _) = resolve(copied.state(), 5, donor);
    assert_ne!(object(donor_left.state(), 1), donor);
    assert_eq!(current(donor_left.state(), receiver), selected);
    let (mountain, _) = resolve(donor_left.state(), 4, receiver);
    let altered = current(mountain.state(), receiver);
    assert_eq!(
        altered.current_land_subtypes,
        BTreeSet::from([LandType::Mountain])
    );
    assert_eq!(
        altered.defined.land_subtypes,
        BTreeSet::from([LandType::Forest])
    );
    assert!(altered.abilities.mana.is_empty());
    assert_eq!(altered.printed_text.mana.len(), 1);
    assert_eq!(
        altered.intrinsic_mana,
        BTreeSet::from([BasicLandType::Mountain])
    );
    let state = mountain.state();
    let saved = state.clone();
    assert!(
        state
            .advance(spellwake_engine::game::GameInput {
                basis: state.basis().clone(),
                actor: P0,
                command: GameCommand::ActivateMana {
                    source: receiver,
                    ability: ManaAbilityRef::Printed(ManaAbilityId(7))
                }
            })
            .is_err()
    );
    assert_eq!(state, &saved);
    let tapped = submit(
        state,
        P0,
        GameCommand::ActivateMana {
            source: receiver,
            ability: ManaAbilityRef::BasicLand(BasicLandType::Mountain),
        },
    );
    assert_eq!(
        tapped.mana_production().unwrap().produced,
        std::collections::BTreeMap::from([(ManaType::Red, 1.into())])
    );
    let cleaned = cleanup(mountain.state());
    let restored = current(cleaned.state(), receiver);
    assert_eq!(
        restored.current_land_subtypes,
        BTreeSet::from([LandType::Forest])
    );
    assert_eq!(restored.abilities.mana, vec![ManaAbilityId(7)]);
    assert_eq!(restored.printed_text, selected.printed_text);
    let (left, _) = resolve(mountain.state(), 6, receiver);
    let hand = object(left.state(), 2);
    assert_ne!(hand, receiver);
    let cast = submit(
        left.state(),
        P0,
        GameCommand::CastUntargeted {
            spell: hand,
            payment: Some(ManaPayment::default()),
        },
    );
    let reentered = pass_pair(cast.state());
    let fresh = object(reentered.state(), 2);
    assert_ne!(fresh, receiver);
    assert_ne!(fresh, hand);
    let original = current(reentered.state(), fresh);
    assert_eq!(original.current_types, BTreeSet::from([CardType::Artifact]));
    assert!(original.defined.land_subtypes.is_empty());
    assert!(original.current_land_subtypes.is_empty());
    assert!(original.intrinsic_mana.is_empty());
    assert_eq!(original.abilities.activated, vec![ActivatedAbilityId(1)]);
}

#[test]
fn authored_land_family_requires_land_independently_of_name_basic_and_creature_family() {
    let path = std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .expect("explicit source-qualified creature vocabulary");
    let vocabulary = creature_vocabulary::load_vocabulary(std::path::Path::new(&path)).unwrap();
    assert_eq!(
        vocabulary.id().to_string(),
        "39721c01eef9fd3f869588b097651bb6115f9cdde5d61bf822d65a8e5435c792"
    );
    let elf = vocabulary.resolve_label("Elf").unwrap();
    for subtype in LandType::ALL {
        let mut land = Program::land(BTreeSet::from([subtype]), vec![]);
        assert_eq!(land.validate(&vocabulary), Ok(()));
        for basic in [false, true] {
            land.supertypes = if basic {
                BTreeSet::from([Supertype::Basic])
            } else {
                BTreeSet::new()
            };
            assert_eq!(land.validate(&vocabulary), Ok(()));
            land.card_types.insert(CardType::Creature);
            land.creature_subtypes.insert(elf);
            assert_eq!(land.validate(&vocabulary), Ok(()));
            land.card_types.remove(&CardType::Land);
            assert_eq!(
                land.validate(&vocabulary),
                Err(DefinitionError::InvalidLandSubtypes)
            );
            land.card_types.insert(CardType::Land);
        }
    }
    let mut named = Program::artifact(None, vec![]);
    named.supertypes.insert(Supertype::Basic);
    named.names = spellwake_contracts::names::ObjectNames::nameless();
    assert!(named.land_subtypes.is_empty());
    assert_eq!(named.validate(&vocabulary), Ok(()));
    named.land_subtypes.insert(LandType::Plains);
    assert_eq!(
        named.validate(&vocabulary),
        Err(DefinitionError::InvalidLandSubtypes)
    );

    let mut spell = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: InstructionAmount::Literal(0.into()),
            }],
        ),
    );
    spell.land_subtypes.insert(LandType::Island);
    assert_eq!(
        spell.validate(&vocabulary),
        Err(DefinitionError::InvalidLandSubtypes)
    );
    spell.card_types.insert(CardType::Land);
    assert_eq!(spell.validate(&vocabulary), Err(DefinitionError::CardTypes));
}
