#![allow(dead_code, unused_imports)]
//! Finite shared card-type/family contracts; no complete-card qualification.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    characteristics::{
        CharacteristicDefinitionError, InstructionSubtypeOperand, PermanentSelector,
        ResolvedDuration, ResolvedPart, ResolvedPartSpec, ResolvedParts, ResolvedPartsSpec,
        StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts, StaticTypeChange,
        SubtypeOperation, TypeChange, TypeChangeSpec, TypeOperation,
    },
    mana::ManaCost,
    object_identity::CardId,
    program::{BodyTarget, CardType, ExecutableBody, Instruction, Program},
};
use std::collections::BTreeSet;
use trajectories::*;

#[test]
fn every_authored_type_subset_is_checked_against_body_and_shared_family() {
    let vocabulary = vocabulary();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    let labels = [
        CardType::Land,
        CardType::Artifact,
        CardType::Creature,
        CardType::Enchantment,
        CardType::Kindred,
        CardType::Instant,
        CardType::Sorcery,
    ];
    for mask in 0_u8..128 {
        for spell_body in [false, true] {
            for with_subtype in [false, true] {
                let mut program = if spell_body {
                    Program::instant(
                        ManaCost::free(),
                        ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::DrawController {
                                amount: spellwake_engine::program::ValueExpr::Literal(0.into()),
                            }],
                        ),
                    )
                } else {
                    Program::artifact(None, vec![])
                };
                program.card_types = labels
                    .iter()
                    .enumerate()
                    .filter(|(bit, _)| mask & (1 << bit) != 0)
                    .map(|(_, label)| *label)
                    .collect();
                program.creature_subtypes = if with_subtype {
                    BTreeSet::from([elf])
                } else {
                    BTreeSet::new()
                };
                // Independent exhaustive bit-domain oracle: four permanent labels,
                // two mutually exclusive instruction labels, and one shared family.
                let expected_shape = if spell_body {
                    (mask & 0b1100000).count_ones() == 1 && mask & 0b0001111 == 0
                } else {
                    mask & 0b0001111 != 0 && mask & 0b1100000 == 0
                };
                let expected = expected_shape && (!with_subtype || mask & 0b0010100 != 0);
                assert_eq!(
                    program.validate(&vocabulary).is_ok(),
                    expected,
                    "mask={mask:07b} spell={spell_body} subtype={with_subtype}"
                );
            }
        }
    }
}

#[test]
fn instruction_types_reject_at_all_three_native_battlefield_operation_boundaries() {
    let vocabulary = vocabulary();
    for forbidden in [CardType::Instant, CardType::Sorcery] {
        for operation in [
            TypeOperation::Add(BTreeSet::from([forbidden])),
            TypeOperation::Remove(BTreeSet::from([forbidden])),
            TypeOperation::Set(BTreeSet::from([forbidden])),
        ] {
            let expected = Err(CharacteristicDefinitionError::UnsupportedTypeOperation);
            let resolved = ResolvedParts(vec![ResolvedPart::Type(TypeChange {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: operation.clone(),
                subtypes: SubtypeOperation::Keep,
            })]);
            assert_eq!(resolved.validate(), expected);
            let spec = ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: operation.clone(),
                subtypes: SubtypeOperation::Keep,
            })]);
            assert_eq!(spec.validate(&vocabulary, &BTreeSet::new()), expected);
            let printed = StaticContinuousAbility::Battlefield {
                id: StaticAbilityId(1),
                selector: PermanentSelector::default(),
                parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: operation,
                    subtypes: SubtypeOperation::Keep,
                })]),
            };
            assert_eq!(printed.validate(&vocabulary), expected);
        }
    }
}

#[test]
fn losing_one_family_carrier_retains_subtypes_and_losing_the_last_clears_them() {
    let vocabulary = vocabulary();
    let mut subject = Program::creature(
        Some(ManaCost::free()),
        2.into(),
        3.into(),
        subtypes(&vocabulary, &["Elf"]),
    );
    subject
        .card_types
        .extend([CardType::Kindred, CardType::Enchantment]);
    let change = |id, types, subtypes| {
        spell(
            id,
            BodyTarget::SinglePermanent,
            vec![effect(
                vec![type_part(types, subtypes)],
                ResolvedDuration::UntilEndOfTurn,
            )],
        )
    };
    let game = world(
        vocabulary.clone(),
        vec![
            card(1, subject, true),
            change(
                2,
                TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
                SubtypeOperation::Keep,
            ),
            change(
                3,
                TypeOperation::Remove(BTreeSet::from([CardType::Kindred])),
                SubtypeOperation::Keep,
            ),
            change(
                4,
                TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                SubtypeOperation::Add(InstructionSubtypeOperand::Literal(subtypes(
                    &vocabulary,
                    &["Goblin"],
                ))),
            ),
        ],
        vec![CardId(1)],
    );
    let subject = object(&game, 1);
    let (first, _) = resolve(&game, 2, subject);
    assert_current(
        first.state(),
        &vocabulary,
        subject,
        &[CardType::Enchantment, CardType::Kindred],
        &["Elf"],
        None,
    );
    let (second, _) = resolve(first.state(), 3, subject);
    assert_current(
        second.state(),
        &vocabulary,
        subject,
        &[CardType::Enchantment],
        &[],
        None,
    );
    let (third, _) = resolve(second.state(), 4, subject);
    assert_current(
        third.state(),
        &vocabulary,
        subject,
        &[CardType::Enchantment, CardType::Creature],
        &["Goblin"],
        Some((2, 3)),
    );
}

#[test]
fn noncreature_kindred_uses_the_same_add_remove_and_set_subtype_operations() {
    let vocabulary = vocabulary();
    let mut subject = Program::enchantment(None);
    subject.card_types.insert(CardType::Kindred);
    subject.creature_subtypes = subtypes(&vocabulary, &["Elf"]);
    let change = |id, subtypes| {
        spell(
            id,
            BodyTarget::SinglePermanent,
            vec![effect(
                vec![type_part(TypeOperation::Keep, subtypes)],
                ResolvedDuration::NoStatedDuration,
            )],
        )
    };
    let game = world(
        vocabulary.clone(),
        vec![
            card(1, subject, true),
            change(
                2,
                SubtypeOperation::Add(InstructionSubtypeOperand::Literal(subtypes(
                    &vocabulary,
                    &["Goblin"],
                ))),
            ),
            change(3, SubtypeOperation::Remove(subtypes(&vocabulary, &["Elf"]))),
            change(
                4,
                SubtypeOperation::Set(InstructionSubtypeOperand::Literal(subtypes(
                    &vocabulary,
                    &["Wizard"],
                ))),
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let (added, _) = resolve(&game, 2, target);
    assert_current(
        added.state(),
        &vocabulary,
        target,
        &[CardType::Kindred, CardType::Enchantment],
        &["Elf", "Goblin"],
        None,
    );
    let (removed, _) = resolve(added.state(), 3, target);
    assert_current(
        removed.state(),
        &vocabulary,
        target,
        &[CardType::Kindred, CardType::Enchantment],
        &["Goblin"],
        None,
    );
    let (set, _) = resolve(removed.state(), 4, target);
    assert_current(
        set.state(),
        &vocabulary,
        target,
        &[CardType::Kindred, CardType::Enchantment],
        &["Wizard"],
        None,
    );
}

#[test]
fn kindred_all_type_definition_runs_in_every_physical_zone_without_becoming_creature() {
    use spellwake_engine::{
        characteristics::{DefiningCreatureTypes, DefiningPart, DefiningParts},
        game::GameCommand,
        mana::ManaPayment,
        object_identity::{ObjectDestination, ObjectZone},
    };
    let vocabulary = vocabulary();
    let mut instruction = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::Literal(0.into()),
            }],
        ),
    );
    instruction.card_types.insert(CardType::Kindred);
    instruction.creature_subtypes = subtypes(&vocabulary, &["Elf"]);
    instruction.continuous = Some(StaticContinuousAbility::CharacteristicDefining {
        id: StaticAbilityId(1),
        parts: DefiningParts(vec![DefiningPart::CreatureTypes(
            DefiningCreatureTypes::All,
        )]),
    });
    let mut cards = vec![];
    for (index, destination) in [
        ObjectDestination::LibraryTop,
        ObjectDestination::Hand,
        ObjectDestination::Graveyard,
        ObjectDestination::Exile,
        ObjectDestination::Command,
    ]
    .into_iter()
    .enumerate()
    {
        let mut seed = card(index as u64 + 1, instruction.clone(), false);
        seed.destination = destination;
        cards.push(seed);
    }
    let mut enchantment = Program::enchantment(None);
    enchantment.card_types.insert(CardType::Kindred);
    enchantment.creature_subtypes = instruction.creature_subtypes.clone();
    enchantment.continuous = instruction.continuous.clone();
    cards.push(card(6, enchantment, true));
    let game = world(vocabulary.clone(), cards, vec![CardId(6)]);
    let assert_all = |game: &spellwake_engine::game::GameState, id| {
        let value = current(game, object(game, id));
        assert!(!value.is_creature());
        assert!(value.power_toughness.is_none());
        assert_eq!(
            value.defined.creature_subtypes,
            subtypes(&vocabulary, &["Elf"])
        );
        assert_eq!(
            value.current_creature_subtypes.len(),
            vocabulary.members().len()
        );
    };
    for id in 1..=6 {
        assert_all(&game, id);
    }
    let cast = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 2),
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(
        current(cast.state(), object(cast.state(), 2)).zone,
        ObjectZone::Stack
    );
    assert_all(cast.state(), 2);
    let completed = pass_pair(cast.state());
    assert_eq!(
        current(completed.state(), object(completed.state(), 2)).zone,
        ObjectZone::Graveyard(P0)
    );
    assert_all(completed.state(), 2);
}

#[test]
fn kindred_instruction_timing_and_paid_enchantment_entry_use_ordinary_root_transitions() {
    use spellwake_engine::{
        actions::TimingError,
        effects::EffectError,
        game::{GameCommand, GameError, GameInput},
        mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaPayment, ManaSpend, ManaType},
        object_identity::ObjectZone,
    };
    use std::collections::BTreeMap;
    let vocabulary = vocabulary();
    let family = subtypes(&vocabulary, &["Elf"]);
    let mut enchantment = Program::enchantment(Some(ManaCost::one(ManaType::Green)));
    enchantment.card_types.insert(CardType::Kindred);
    enchantment.creature_subtypes = family.clone();
    enchantment.mana.push(ManaAbility {
        id: ManaAbilityId(2),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
    });
    let body = || {
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::Literal(0.into()),
            }],
        )
    };
    let mut instant = Program::instant(ManaCost::free(), body());
    instant.card_types.insert(CardType::Kindred);
    instant.creature_subtypes = family.clone();
    let mut sorcery = Program::sorcery(ManaCost::free(), body());
    sorcery.card_types.insert(CardType::Kindred);
    sorcery.creature_subtypes = family;
    let game = world(
        vocabulary.clone(),
        vec![
            card(
                1,
                Program::land(
                    std::collections::BTreeSet::new(),
                    vec![ManaAbility {
                        id: ManaAbilityId(1),
                        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
                    }],
                ),
                true,
            ),
            card(2, enchantment, false),
            card(3, instant.clone(), false),
            card(4, instant, false),
            card(5, sorcery, false),
        ],
        vec![CardId(1)],
    );
    let produced = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 1),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        produced.state().execution().mana(P0, ManaType::Green),
        1.into()
    );
    let green = ManaPayment {
        from_lots: Default::default(),
        spend: vec![ManaSpend {
            mana_type: ManaType::Green,
            amount: 1.into(),
        }],
    };
    let cast = |game: &spellwake_engine::game::GameState, id, payment| {
        submit(
            game,
            P0,
            GameCommand::CastUntargeted {
                spell: object(game, id),
                payment: Some(payment),
            },
        )
    };
    let first = cast(produced.state(), 3, ManaPayment::default());
    let response = cast(first.state(), 4, ManaPayment::default());
    assert_eq!(response.state().execution().window().entries().len(), 2);
    for (id, payment) in [(2, green.clone()), (5, ManaPayment::default())] {
        let game = response.state();
        let saved = game.clone();
        let rejected = game.advance(GameInput {
            basis: game.basis().clone(),
            actor: P0,
            command: GameCommand::CastUntargeted {
                spell: object(game, id),
                payment: Some(payment),
            },
        });
        assert_eq!(
            rejected,
            Err(GameError::Effect(EffectError::Timing(
                TimingError::StackNotEmpty
            )))
        );
        assert_eq!(game, &saved);
        assert_eq!(game.execution().mana(P0, ManaType::Green), 1.into());
    }
    let top = pass_pair(response.state());
    let empty = pass_pair(top.state());
    let announced = cast(empty.state(), 2, green.clone());
    assert_eq!(announced.mana_payment().unwrap().spend, green.spend);
    assert_eq!(
        announced.mana_payment().unwrap().cost,
        ManaCost::one(ManaType::Green)
    );
    assert_eq!(
        announced.state().execution().mana(P0, ManaType::Green),
        0.into()
    );
    assert_eq!(
        current(announced.state(), object(announced.state(), 2)).zone,
        ObjectZone::Stack
    );
    let entered = pass_pair(announced.state());
    assert!(entered.permanent_entry().is_some());
    assert_current(
        entered.state(),
        &vocabulary,
        object(entered.state(), 2),
        &[CardType::Kindred, CardType::Enchantment],
        &["Elf"],
        None,
    );
    let fresh = object(entered.state(), 2);
    assert_eq!(
        entered
            .state()
            .execution()
            .window()
            .cards()
            .get(fresh)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(false)
    );
    let tapped = submit(
        entered.state(),
        P0,
        GameCommand::ActivateMana {
            source: fresh,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(2)),
        },
    );
    assert_eq!(
        tapped.state().execution().mana(P0, ManaType::Blue),
        1.into()
    );
    let scheduled = cast(tapped.state(), 5, ManaPayment::default());
    let completed = pass_pair(scheduled.state());
    assert_eq!(
        current(completed.state(), object(completed.state(), 5)).zone,
        ObjectZone::Graveyard(P0)
    );
}

#[test]
fn generic_mana_counts_kindred_members_but_creature_targets_reject_them() {
    use spellwake_engine::{
        effects::EffectError,
        game::{GameCommand, GameError, GameInput},
        mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaPayment, ManaType},
    };
    let vocabulary = vocabulary();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    let counted = ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Counted {
            mana_type: Some(ManaType::Green),
            permanents: PermanentSelector {
                creature_subtype: Some(elf),
                controller: spellwake_engine::characteristics::ControllerSelector::SourceController,
                ..PermanentSelector::default()
            },
        },
    };
    let mut source = Program::artifact(None, vec![]);
    source.card_types.insert(CardType::Kindred);
    source.creature_subtypes.insert(elf);
    source.mana.push(counted.clone());
    let mut other_source = Program::artifact(None, vec![]);
    other_source.mana.push(counted);
    let mut kindred = Program::enchantment(None);
    kindred.card_types.insert(CardType::Kindred);
    kindred.creature_subtypes.insert(elf);
    let game = world(
        vocabulary,
        vec![
            card(1, source, true),
            card(2, kindred, true),
            card(
                3,
                Program::creature(None, 2.into(), 3.into(), BTreeSet::from([elf])),
                true,
            ),
            card(4, other_source, true),
            spell(
                5,
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
            spell(
                6,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Remove(BTreeSet::from([CardType::Kindred])),
                        SubtypeOperation::Keep,
                    )],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );
    let saved = game.clone();
    let rejected = game.advance(GameInput {
        basis: game.basis().clone(),
        actor: P0,
        command: GameCommand::CastPaid {
            spell: object(&game, 5),
            target: spellwake_engine::targets::TargetRef::Object(object(&game, 2)),
            payment: ManaPayment::default(),
        },
    });
    assert_eq!(rejected, Err(GameError::Effect(EffectError::IllegalTarget)));
    assert_eq!(game, saved);
    let activated = submit(
        &game,
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 1),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        activated.state().execution().mana(P0, ManaType::Green),
        3.into()
    );
    assert_eq!(
        activated
            .state()
            .execution()
            .window()
            .cards()
            .get(object(activated.state(), 1))
            .unwrap()
            .tapped(),
        Some(true)
    );
    let (stripped, _) = resolve(activated.state(), 6, object(activated.state(), 2));
    let next = submit(
        stripped.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(stripped.state(), 4),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(next.state().execution().mana(P0, ManaType::Green), 5.into());
}

#[test]
fn chosen_type_draw_counts_noncreature_kindred_with_the_existing_value_and_draw_cursor() {
    use spellwake_engine::{
        game::{DecisionReply, GameCommand},
        mana::ManaPayment,
        program::CreatureTypeSlot,
    };
    let vocabulary = vocabulary();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    let mut kindred = Program::enchantment(None);
    kindred.card_types.insert(CardType::Kindred);
    kindred.creature_subtypes.insert(elf);
    let mut draw = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::ChooseCreatureType {
                    into: CreatureTypeSlot::new(0),
                    excluding: BTreeSet::new(),
                },
                Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::count_battlefield(
                        spellwake_engine::program::ObjectFilter::any()
                            .controlled()
                            .of_chosen_slot(CreatureTypeSlot::new(0)),
                    ),
                },
            ],
        ),
    );
    draw.card_types.insert(CardType::Kindred);
    draw.creature_subtypes.insert(elf);
    let game = world(
        vocabulary.clone(),
        vec![
            card(1, kindred, true),
            card(
                2,
                Program::creature(None, 2.into(), 3.into(), BTreeSet::from([elf])),
                true,
            ),
            card(3, draw, false),
            library(4, P0, &vocabulary),
            library(5, P0, &vocabulary),
            library(6, P0, &vocabulary),
        ],
        vec![CardId(1), CardId(2)],
    );
    let before = game.execution().window().cards().library(P0).to_vec();
    let cast = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let entry = cast
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let pending = pass_pair(cast.state());
    assert_type_frame(pending.state(), entry);
    assert_eq!(
        pending.state().execution().window().cards().library(P0),
        before
    );
    let completed = reply(pending.state(), P0, DecisionReply::CreatureType(elf));
    assert_eq!(
        completed
            .state()
            .execution()
            .window()
            .cards()
            .library(P0)
            .len(),
        1
    );
    for old in &before[..2] {
        let actual = zone_change(&completed, *old);
        assert_eq!(
            actual.before.object.zone(),
            spellwake_engine::object_identity::ObjectZone::Library(P0)
        );
        assert_eq!(
            actual.after.object.zone(),
            spellwake_engine::object_identity::ObjectZone::Hand(P0)
        );
        assert_ne!(actual.before.object.id(), actual.after.object.id());
    }
    assert_completion(
        &completed,
        entry,
        spellwake_engine::occurrence::EntryCompletion::Resolved,
    );
}

#[test]
fn later_current_types_may_be_kindred_only_or_typeless_without_readmitting_the_definition() {
    let vocabulary = vocabulary();
    let subject = Program::creature(None, 2.into(), 3.into(), subtypes(&vocabulary, &["Elf"]));
    let change = |id, types, subtypes| {
        spell(
            id,
            BodyTarget::SinglePermanent,
            vec![effect(
                vec![type_part(types, subtypes)],
                ResolvedDuration::NoStatedDuration,
            )],
        )
    };
    let game = world(
        vocabulary.clone(),
        vec![
            card(1, subject, true),
            change(
                2,
                TypeOperation::Set(BTreeSet::from([CardType::Kindred])),
                SubtypeOperation::Keep,
            ),
            change(
                3,
                TypeOperation::Set(BTreeSet::new()),
                SubtypeOperation::Keep,
            ),
            change(
                4,
                TypeOperation::Add(BTreeSet::from([CardType::Kindred])),
                SubtypeOperation::Set(InstructionSubtypeOperand::Literal(subtypes(
                    &vocabulary,
                    &["Goblin"],
                ))),
            ),
        ],
        vec![CardId(1)],
    );
    let target = object(&game, 1);
    let (kindred, _) = resolve(&game, 2, target);
    assert_current(
        kindred.state(),
        &vocabulary,
        target,
        &[CardType::Kindred],
        &["Elf"],
        None,
    );
    let (typeless, _) = resolve(kindred.state(), 3, target);
    assert_current(typeless.state(), &vocabulary, target, &[], &[], None);
    let (restored, _) = resolve(typeless.state(), 4, target);
    assert_current(
        restored.state(),
        &vocabulary,
        target,
        &[CardType::Kindred],
        &["Goblin"],
        None,
    );
    assert_eq!(
        current(restored.state(), target).defined.types,
        BTreeSet::from([CardType::Creature])
    );
}

#[test]
fn live_kindred_source_subtypes_regenerate_when_family_eligibility_changes() {
    use spellwake_engine::characteristics::StaticSubtypeOperand;
    let vocabulary = vocabulary();
    let mut generator = Program::enchantment(None);
    generator.card_types.insert(CardType::Kindred);
    generator.creature_subtypes = subtypes(&vocabulary, &["Elf"]);
    generator.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
            land_subtypes: SubtypeOperation::Keep,
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Add(StaticSubtypeOperand::SourceCurrentSubtypes),
        })]),
    });
    let game = world(
        vocabulary.clone(),
        vec![
            card(1, generator, true),
            card(
                2,
                Program::creature(None, 2.into(), 3.into(), subtypes(&vocabulary, &["Human"])),
                true,
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Remove(BTreeSet::from([CardType::Kindred])),
                        SubtypeOperation::Keep,
                    )],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            spell(
                4,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![type_part(
                        TypeOperation::Add(BTreeSet::from([CardType::Kindred])),
                        SubtypeOperation::Set(InstructionSubtypeOperand::Literal(subtypes(
                            &vocabulary,
                            &["Goblin"],
                        ))),
                    )],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let source = object(&game, 1);
    let target = object(&game, 2);
    assert_current(
        &game,
        &vocabulary,
        target,
        &[CardType::Creature],
        &["Human", "Elf"],
        Some((2, 3)),
    );
    let (stripped, _) = resolve(&game, 3, source);
    assert_current(
        stripped.state(),
        &vocabulary,
        target,
        &[CardType::Creature],
        &["Human"],
        Some((2, 3)),
    );
    assert_current(
        stripped.state(),
        &vocabulary,
        source,
        &[CardType::Enchantment],
        &[],
        None,
    );
    let (restored, _) = resolve(stripped.state(), 4, source);
    assert_current(
        restored.state(),
        &vocabulary,
        target,
        &[CardType::Creature],
        &["Human", "Goblin"],
        Some((2, 3)),
    );
    let counters = restored.state().execution().operational_counters();
    assert_current(
        restored.state(),
        &vocabulary,
        source,
        &[CardType::Enchantment, CardType::Kindred],
        &["Goblin"],
        None,
    );
    assert_eq!(
        restored.state().execution().operational_counters(),
        counters
    );
}

#[test]
fn kindred_text_changes_rewrite_current_family_and_printed_mana_selector_together() {
    use spellwake_engine::{
        characteristics::{ControllerSelector, CreatureTypeTextChangeSpec, CreatureTypeWord},
        game::GameCommand,
        mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaType},
    };
    let vocabulary = vocabulary();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    let goblin = vocabulary.resolve_label("Goblin").unwrap();
    let mut source = Program::enchantment(None);
    source.card_types.insert(CardType::Kindred);
    source.creature_subtypes.insert(elf);
    source.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Counted {
            mana_type: Some(ManaType::Green),
            permanents: PermanentSelector {
                creature_subtype: Some(elf),
                controller: ControllerSelector::SourceController,
                ..PermanentSelector::default()
            },
        },
    });
    let game = world(
        vocabulary.clone(),
        vec![
            card(1, source, true),
            card(
                2,
                Program::creature(None, 2.into(), 3.into(), BTreeSet::from([goblin])),
                true,
            ),
            card(
                3,
                Program::creature(None, 2.into(), 3.into(), BTreeSet::from([elf])),
                true,
            ),
            spell(
                4,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![ResolvedPartSpec::Text(CreatureTypeTextChangeSpec {
                        from: CreatureTypeWord::Literal(elf),
                        to: CreatureTypeWord::Literal(goblin),
                    })],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let source = object(&game, 1);
    let (rewritten, _) = resolve(&game, 4, source);
    let current = current(rewritten.state(), source);
    assert_eq!(current.defined.creature_subtypes, BTreeSet::from([elf]));
    assert_eq!(current.current_creature_subtypes, BTreeSet::from([goblin]));
    assert!(!current.is_creature());
    let activated = submit(
        rewritten.state(),
        P0,
        GameCommand::ActivateMana {
            source,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(
        activated.state().execution().mana(P0, ManaType::Green),
        2.into()
    );
}

#[test]
fn legendary_noncreature_enchantments_and_kindred_permanents_do_not_qualify_spell_casting() {
    use spellwake_engine::{
        effects::EffectError,
        game::{GameCommand, GameError, GameInput},
        program::Supertype,
    };
    let vocabulary = vocabulary();
    for with_kindred in [false, true] {
        let mut source = Program::enchantment(None);
        source.supertypes.insert(Supertype::Legendary);
        if with_kindred {
            source.card_types.insert(CardType::Kindred);
            source.creature_subtypes = subtypes(&vocabulary, &["Elf"]);
        }
        let mut restricted = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(0.into()),
                }],
            ),
        );
        restricted.supertypes.insert(Supertype::Legendary);
        restricted.card_types.insert(CardType::Kindred);
        restricted.creature_subtypes = subtypes(&vocabulary, &["Elf"]);
        let game = world(
            vocabulary.clone(),
            vec![card(1, source, true), card(2, restricted, false)],
            vec![CardId(1)],
        );
        let saved = game.clone();
        let rejected = game.advance(GameInput {
            basis: game.basis().clone(),
            actor: P0,
            command: GameCommand::CastUntargeted {
                spell: object(&game, 2),
                payment: None,
            },
        });
        assert_eq!(
            rejected,
            Err(GameError::Effect(EffectError::LegendarySpellPrerequisite))
        );
        assert_eq!(game, saved);
    }
}

#[test]
fn paid_kindred_instruction_spells_use_actual_mana_and_life_transition_paths() {
    use spellwake_engine::{
        game::GameCommand,
        life::LifeAdjustment,
        mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaPayment, ManaSpend, ManaType},
        object_identity::ObjectZone,
    };
    use std::collections::BTreeMap;
    let vocabulary = vocabulary();
    for kind in [CardType::Instant, CardType::Sorcery] {
        let body = ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(3.into()),
            }],
        );
        let mut paid = if kind == CardType::Instant {
            Program::instant(ManaCost::one(ManaType::Green), body)
        } else {
            Program::sorcery(ManaCost::one(ManaType::Green), body)
        };
        paid.card_types.insert(CardType::Kindred);
        paid.creature_subtypes = subtypes(&vocabulary, &["Elf"]);
        let game = world(
            vocabulary.clone(),
            vec![
                card(
                    1,
                    Program::land(
                        std::collections::BTreeSet::new(),
                        vec![ManaAbility {
                            id: ManaAbilityId(1),
                            output: ManaOutput::Fixed(BTreeMap::from([(
                                ManaType::Green,
                                1.into(),
                            )])),
                        }],
                    ),
                    true,
                ),
                card(2, paid, false),
            ],
            vec![CardId(1)],
        );
        let produced = submit(
            &game,
            P0,
            GameCommand::ActivateMana {
                source: object(&game, 1),
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
            },
        );
        let payment = ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Green,
                amount: 1.into(),
            }],
        };
        let cast = submit(
            produced.state(),
            P0,
            GameCommand::CastUntargeted {
                spell: object(produced.state(), 2),
                payment: Some(payment.clone()),
            },
        );
        assert_eq!(cast.mana_payment().unwrap().spend, payment.spend);
        assert_eq!(cast.state().execution().mana(P0, ManaType::Green), 0.into());
        let completed = pass_pair(cast.state());
        assert_eq!(
            completed.state().execution().life_total(P0),
            Some(&23.into())
        );
        assert_eq!(
            current(completed.state(), object(completed.state(), 2)).zone,
            ObjectZone::Graveyard(P0)
        );
        assert_current(
            completed.state(),
            &vocabulary,
            object(completed.state(), 2),
            &[CardType::Kindred, kind],
            &["Elf"],
            None,
        );
    }
}
