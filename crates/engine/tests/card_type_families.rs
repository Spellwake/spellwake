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
