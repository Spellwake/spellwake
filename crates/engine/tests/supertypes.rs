#![allow(dead_code, unused_imports)]
//! Supertype traits through actual root actions, occurrences, copying and cleanup.
//! Associated World/legend state actions and Snow costs qualify separately.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        PermanentSelector, ResolvedDuration, ResolvedPartSpec, SubtypeOperation, TypeChangeSpec,
        TypeOperation,
    },
    game::{GameCommand, GameState, NativeCandidate},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectZone},
    occurrence::RulesOccurrence,
    program::{BodyTarget, ExecutableBody, Instruction, Program, Supertype},
};
use std::collections::BTreeSet;
use trajectories::*;

fn set(values: &[Supertype]) -> BTreeSet<Supertype> {
    values.iter().copied().collect()
}
fn part(operation: TypeOperation<Supertype>) -> ResolvedPartSpec {
    ResolvedPartSpec::Type(TypeChangeSpec {
        land_subtypes: SubtypeOperation::Keep,
        supertypes: operation,
        types: TypeOperation::Keep,
        subtypes: SubtypeOperation::Keep,
    })
}
fn change(
    id: u64,
    operation: TypeOperation<Supertype>,
    duration: ResolvedDuration,
) -> spellwake_engine::effects::SeedCard {
    spell(
        id,
        BodyTarget::SINGLE_PERMANENT,
        vec![effect(vec![part(operation)], duration)],
    )
}
fn no_duration() -> ResolvedDuration {
    ResolvedDuration::NoStatedDuration
}
fn copier() -> Program {
    Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SINGLE_PERMANENT,
                vec![Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::become_copy(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            spellwake_engine::program::EffectSubject::AbilitySource,
                        )),
                        spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                            spellwake_engine::program::MoveSubjectRef::TargetObject,
                        ),
                        no_duration(),
                        spellwake_engine::program::CopyExceptions {
                            override_pt: None,
                            ..Default::default()
                        },
                    ),
                ))],
            ),
        }],
    )
}
fn copy(game: &GameState, receiver: u64, donor: u64) -> NativeCandidate {
    let candidate = activate(game, object(game, receiver), object(game, donor), false);
    let ability = candidate
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    assert_eq!(
        candidate.state().execution().stack_copiable_names(ability),
        None
    );
    assert_eq!(
        candidate
            .state()
            .execution()
            .stack_copiable_supertypes(ability),
        None
    );
    pass_pair(candidate.state())
}

#[test]
fn set_add_remove_and_cleanup_publish_current_traits_without_rewriting_copy_values() {
    use Supertype::*;
    let mut original = Program::artifact(None, vec![]);
    original.supertypes = set(&[Basic]);
    let game = world(
        vocabulary(),
        vec![
            card(1, original, true),
            change(
                2,
                TypeOperation::Add(set(&[Snow])),
                ResolvedDuration::UntilEndOfTurn,
            ),
            change(
                3,
                TypeOperation::Remove(set(&[Basic])),
                ResolvedDuration::UntilEndOfTurn,
            ),
            change(
                4,
                TypeOperation::Set(BTreeSet::new()),
                ResolvedDuration::UntilEndOfTurn,
            ),
        ],
        vec![CardId(1)],
    );
    let id = object(&game, 1);
    let before = game.clone();
    let (added, _) = resolve(&game, 2, id);
    assert_eq!(game, before);
    assert_eq!(
        current(added.state(), id).current_supertypes,
        set(&[Basic, Snow])
    );
    let (removed, _) = resolve(added.state(), 3, id);
    assert_eq!(
        current(removed.state(), id).current_supertypes,
        set(&[Snow])
    );
    assert!(all_occurrences(&removed).iter().any(|event| matches!(event,
        RulesOccurrence::ContinuousEffectCreated { appearances, .. } if appearances.iter().any(|change|
            change.before.current_supertypes == set(&[Basic, Snow]) && change.after.current_supertypes == set(&[Snow]) && change.after.copiable_supertypes == set(&[Basic])))));
    let (empty, _) = resolve(removed.state(), 4, id);
    assert!(current(empty.state(), id).current_supertypes.is_empty());
    assert_eq!(
        empty
            .state()
            .execution()
            .copiable_program(id)
            .unwrap()
            .supertypes,
        set(&[Basic])
    );
    let cleaned = cleanup(empty.state());
    assert_eq!(
        current(cleaned.state(), id).current_supertypes,
        set(&[Basic])
    );
    assert_eq!(object(cleaned.state(), 1), id);
}

#[test]
fn counted_mana_reads_current_required_and_forbidden_supertypes() {
    use Supertype::*;
    let mut source = Program::artifact(None, vec![]);
    source.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Counted {
            mana_type: Some(ManaType::Green),
            permanents: PermanentSelector {
                required_supertypes: set(&[Snow]),
                forbidden_supertypes: set(&[Legendary]),
                ..PermanentSelector::default()
            },
        },
    });
    let mut excluded = Program::artifact(None, vec![]);
    excluded.supertypes = set(&[Legendary, Snow]);
    let game = world(
        vocabulary(),
        vec![
            card(1, source, true),
            card(2, Program::artifact(None, vec![]), true),
            card(3, excluded, true),
            change(4, TypeOperation::Add(set(&[Snow])), no_duration()),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let (changed, _) = resolve(&game, 4, object(&game, 2));
    let produced = submit(
        changed.state(),
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
    assert!(
        current(produced.state(), object(&game, 2))
            .defined
            .supertypes
            .is_empty()
    );
}
