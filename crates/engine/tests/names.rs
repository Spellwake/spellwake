#![allow(dead_code, unused_imports)]
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
            mana_type: Some(ManaType::Green),
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
