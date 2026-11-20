//! Names through real candidate, occurrence, count and cleanup owners.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_contracts::names::{EnglishCardName, NameRules, ObjectNames};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        NameFilter, NameOperation, PermanentSelector, ResolvedDuration, ResolvedPartSpec,
    },
    combat::CombatRules,
    game::{GameCommand, GameContext, GameId, GameState},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    object_identity::CardId,
    occurrence::RulesOccurrence,
    program::{BodyTarget, ExecutableBody, Instruction, Program},
    setup::GameSeed,
};
use std::{collections::BTreeSet, sync::Arc};
use trajectories::*;

fn names(labels: &[&str]) -> ObjectNames {
    ObjectNames::new(
        labels
            .iter()
            .map(|label| EnglishCardName::new((*label).into()).unwrap())
            .collect(),
    )
    .unwrap()
}
fn named(mut program: Program, label: &str) -> Program {
    program.names = names(&[label]);
    program
}
fn rename(
    id: u64,
    change: NameOperation<ObjectNames>,
    duration: ResolvedDuration,
) -> spellwake_engine::effects::SeedCard {
    spell(
        id,
        BodyTarget::SinglePermanent,
        vec![effect(vec![ResolvedPartSpec::Names(change)], duration)],
    )
}
fn creature(label: &str) -> Program {
    named(
        Program::creature(None, 2.into(), 3.into(), BTreeSet::new()),
        label,
    )
}

#[test]
fn set_add_nameless_and_cleanup_preserve_copiable_names_and_emit_actual_appearances() {
    let game = world(
        vocabulary(),
        vec![
            card(1, creature("Original"), true),
            rename(
                2,
                NameOperation::Set(names(&["Renamed"])),
                ResolvedDuration::UntilEndOfTurn,
            ),
            rename(
                3,
                NameOperation::Add(names(&["Extra"])),
                ResolvedDuration::UntilEndOfTurn,
            ),
            rename(
                4,
                NameOperation::Set(ObjectNames::nameless()),
                ResolvedDuration::UntilEndOfTurn,
            ),
        ],
        vec![CardId(1)],
    );
    let id = object(&game, 1);
    let (set, _) = resolve(&game, 2, id);
    assert_eq!(current(set.state(), id).current_names, names(&["Renamed"]));
    let (add, _) = resolve(set.state(), 3, id);
    assert_eq!(
        current(add.state(), id).current_names,
        names(&["Renamed", "Extra"])
    );
    assert_eq!(
        add.state().execution().copiable_program(id).unwrap().names,
        names(&["Original"])
    );
    assert!(all_occurrences(&add).iter().any(|event|matches!(event,RulesOccurrence::ContinuousEffectCreated {appearances,..} if appearances.iter().any(|appearance|appearance.before.current_names==names(&["Renamed"])&&appearance.after.current_names==names(&["Renamed","Extra"])&&appearance.after.copiable_names==names(&["Original"])))));
    let (empty, _) = resolve(add.state(), 4, id);
    assert!(current(empty.state(), id).current_names.is_empty());
    let cleaned = cleanup(empty.state());
    assert_eq!(
        current(cleaned.state(), id).current_names,
        names(&["Original"])
    );
    assert_eq!(object(cleaned.state(), 1), id);
}

#[test]
fn counted_mana_reads_current_names_with_the_same_filter_as_static_counts() {
    let mut source = creature("Source");
    source.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Counted {
            mana_type: ManaType::Green,
            permanents: PermanentSelector {
                name: NameFilter::Named(EnglishCardName::new("Elf".into()).unwrap()),
                ..PermanentSelector::default()
            },
        },
    });
    let game = world(
        vocabulary(),
        vec![
            card(1, source, true),
            card(2, creature("Elf"), true),
            card(3, creature("Other"), true),
            rename(
                4,
                NameOperation::Set(names(&["Elf"])),
                ResolvedDuration::NoStatedDuration,
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let (renamed, _) = resolve(&game, 4, object(&game, 3));
    let done = submit(
        renamed.state(),
        P0,
        GameCommand::ActivateMana {
            source: object(&game, 1),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    assert_eq!(done.state().execution().mana(P0, ManaType::Green), 2.into());
    assert_eq!(
        current(done.state(), object(&game, 3)).defined.names,
        names(&["Other"])
    );
}

#[test]
fn permanent_copy_and_copy_of_copy_capture_names_before_temporary_renames() {
    let copier = |label| {
        named(
            Program::artifact(
                None,
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
            ),
            label,
        )
    };
    let game = world(
        vocabulary(),
        vec![
            card(1, copier("First"), true),
            card(2, copier("Second"), true),
            card(3, creature("Donor"), true),
            rename(
                6,
                NameOperation::Set(names(&["Renamed receiver"])),
                ResolvedDuration::UntilEndOfTurn,
            ),
            rename(
                4,
                NameOperation::Set(names(&["Temporary"])),
                ResolvedDuration::NoStatedDuration,
            ),
            spell(
                5,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let donor = object(&game, 3);
    let (renamed, _) = resolve(&game, 4, donor);
    let (receiver_renamed, _) = resolve(renamed.state(), 6, object(&game, 1));
    let activated = activate(receiver_renamed.state(), object(&game, 1), donor, false);
    let copied = pass_pair(activated.state());
    assert_eq!(
        current(copied.state(), object(&game, 1)).current_names,
        names(&["Renamed receiver"])
    );
    assert_eq!(
        current(copied.state(), donor).current_names,
        names(&["Temporary"])
    );
    let activated = activate(copied.state(), object(&game, 2), object(&game, 1), false);
    let second = pass_pair(activated.state());
    let (departed, _) = resolve(second.state(), 5, donor);
    assert_ne!(object(departed.state(), 3), donor);
    assert_eq!(
        current(departed.state(), object(&game, 1)).current_names,
        names(&["Renamed receiver"])
    );
    assert_eq!(
        current(departed.state(), object(&game, 2)).current_names,
        names(&["Donor"])
    );
    assert_eq!(
        departed
            .state()
            .execution()
            .copiable_program(object(&game, 1))
            .unwrap()
            .names,
        names(&["Donor"])
    );
    assert_eq!(
        current(departed.state(), object(departed.state(), 3)).current_names,
        names(&["Donor"])
    );
}

#[test]
fn game_context_binds_the_actual_supplied_name_relation_before_initialization() {
    let v = vocabulary();
    let relation = Arc::new(
        NameRules::new(vec![names(&["Eleven, the Mage", "Cecily, Haunted Mage"])]).unwrap(),
    );
    let context = |id| {
        GameContext::new(
            "names".into(),
            v.rules_revision().into(),
            "fixture".into(),
            "partial".into(),
            "names".into(),
            v.id(),
            id,
        )
        .unwrap()
    };
    let mut seed = GameSeed::with_uniform_life(
        vec![P0, P1],
        P0,
        vec![],
        vec![],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        v.clone(),
        relation.clone(),
    );
    assert!(
        GameState::initialize(
            GameId(1),
            context(NameRules::literal_names().id()),
            seed.clone()
        )
        .is_err()
    );
    let game = GameState::initialize(GameId(1), context(relation.id()), seed.clone()).unwrap();
    assert_eq!(game.state().execution().name_rules(), relation.as_ref());
    seed.name_rules = Arc::new(NameRules::literal_names());
    assert_eq!(game.state().execution().name_rules(), relation.as_ref());
    // A nameless spell is Some(empty); an activated ability is None.
    let game = world(
        vocabulary(),
        vec![
            card(
                1,
                Program::artifact(
                    None,
                    vec![ActivatedAbility {
                        id: ActivatedAbilityId(1),
                        mana_cost: ManaCost::free(),
                        source_cost: SourceCost::None,
                        body: ExecutableBody::new(
                            BodyTarget::SinglePermanent,
                            vec![effect(
                                vec![ResolvedPartSpec::Names(NameOperation::Add(names(&[
                                    "extra",
                                ])))],
                                ResolvedDuration::NoStatedDuration,
                            )],
                        ),
                    }],
                ),
                true,
            ),
            card(2, creature("Target"), true),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ],
        vec![CardId(1), CardId(2)],
    );
    let active = activate(&game, object(&game, 1), object(&game, 2), false);
    assert_eq!(
        active
            .state()
            .execution()
            .stack_copiable_names(active.state().execution().window().entries()[0].id()),
        None
    );
    let (casted, _) = cast(&game, 3, object(&game, 2));
    assert_eq!(
        casted
            .state()
            .execution()
            .stack_copiable_names(casted.state().execution().window().entries()[0].id()),
        Some(&ObjectNames::nameless())
    );
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(object(&game, 3))
            .unwrap()
            .zone(),
        spellwake_engine::object_identity::ObjectZone::Hand(P0)
    );
}

#[test]
fn renamed_self_trigger_keeps_old_appearance_after_later_rename_exit_and_stale_rejection() {
    use spellwake_engine::program::{
        InstructionAmount, ObjectRelation, OrdinaryCondition, TriggeredAbility, TriggeredAbilityId,
    };
    let mut source = creature("Original");
    source.supertypes = BTreeSet::from([spellwake_engine::program::Supertype::Snow]);
    source.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(std::collections::BTreeMap::from([(
            ManaType::Green,
            1.into(),
        )])),
    });
    source.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition: OrdinaryCondition::BecomesTapped(ObjectRelation::SelfObject),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: InstructionAmount::Literal(0.into()),
            }],
        ),
    });
    let game = world(
        vocabulary(),
        vec![
            card(1, source, true),
            spell(
                2,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![
                        ResolvedPartSpec::Names(NameOperation::Set(names(&["First"]))),
                        ResolvedPartSpec::Type(spellwake_engine::characteristics::TypeChangeSpec {
                            land_subtypes:
                                spellwake_engine::characteristics::SubtypeOperation::Keep,
                            supertypes: spellwake_engine::characteristics::TypeOperation::Add(
                                BTreeSet::from([spellwake_engine::program::Supertype::Basic]),
                            ),
                            types: spellwake_engine::characteristics::TypeOperation::Keep,
                            subtypes: spellwake_engine::characteristics::SubtypeOperation::Keep,
                        }),
                    ],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            spell(
                3,
                BodyTarget::SinglePermanent,
                vec![effect(
                    vec![
                        ResolvedPartSpec::Names(NameOperation::Set(names(&["Second"]))),
                        ResolvedPartSpec::Type(spellwake_engine::characteristics::TypeChangeSpec {
                            land_subtypes:
                                spellwake_engine::characteristics::SubtypeOperation::Keep,
                            supertypes: spellwake_engine::characteristics::TypeOperation::Set(
                                BTreeSet::new(),
                            ),
                            types: spellwake_engine::characteristics::TypeOperation::Keep,
                            subtypes: spellwake_engine::characteristics::SubtypeOperation::Keep,
                        }),
                    ],
                    ResolvedDuration::NoStatedDuration,
                )],
            ),
            spell(
                4,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
            rename(
                5,
                NameOperation::Set(names(&["Stale"])),
                ResolvedDuration::NoStatedDuration,
            ),
        ],
        vec![CardId(1)],
    );
    let id = object(&game, 1);
    let (renamed, _) = resolve(&game, 2, id);
    let tapped = submit(
        renamed.state(),
        P0,
        GameCommand::ActivateMana {
            source: id,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
    );
    let trigger = tapped.state().execution().window().entries()[0].id();
    let captured = tapped
        .state()
        .execution()
        .execution_bindings(trigger)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap()
        .source
        .clone();
    assert_eq!(captured.object.id(), id);
    assert_eq!(captured.current_names, names(&["First"]));
    assert_eq!(captured.copiable_names, names(&["Original"]));
    assert_eq!(
        captured.copiable_supertypes,
        BTreeSet::from([spellwake_engine::program::Supertype::Snow])
    );
    assert_eq!(
        captured.current_supertypes,
        BTreeSet::from([
            spellwake_engine::program::Supertype::Basic,
            spellwake_engine::program::Supertype::Snow
        ])
    );
    assert_eq!(captured.object.tapped(), Some(true));
    let (again, _) = resolve(tapped.state(), 3, id);
    assert_eq!(current(again.state(), id).current_names, names(&["Second"]));
    assert!(current(again.state(), id).current_supertypes.is_empty());
    let (departed, _) = resolve(again.state(), 4, id);
    assert_ne!(object(departed.state(), 1), id);
    assert_eq!(
        departed
            .state()
            .execution()
            .execution_bindings(trigger)
            .unwrap()
            .trigger
            .as_ref()
            .unwrap()
            .source,
        captured
    );
    let saved = departed.state().clone();
    let rejected = departed.state().advance(spellwake_engine::game::GameInput {
        basis: departed.state().basis().clone(),
        actor: P0,
        command: GameCommand::CastPaid {
            spell: object(departed.state(), 5),
            target: spellwake_engine::targets::TargetRef::Object(id),
            payment: spellwake_engine::mana::ManaPayment::default(),
        },
    });
    assert!(rejected.is_err());
    assert_eq!(departed.state(), &saved);
    let done = pass_pair(departed.state());
    assert!(done.state().execution().window().entries().is_empty());
}
