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
        BodyTarget::SinglePermanent,
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
                BodyTarget::SinglePermanent,
                vec![Instruction::CopyTargetToSource {
                    duration: no_duration(),
                    power_toughness: None,
                }],
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

#[test]
fn permanent_copies_capture_printed_supertypes_while_receiver_effects_and_exit_remain_independent()
{
    use Supertype::*;
    let mut donor = Program::creature(None, 2.into(), 3.into(), BTreeSet::new());
    donor.supertypes = set(&[Basic]);
    let game = world(
        vocabulary(),
        vec![
            card(1, copier(), true),
            card(2, copier(), true),
            card(3, donor.clone(), true),
            change(4, TypeOperation::Set(set(&[Snow])), no_duration()),
            change(
                5,
                TypeOperation::Add(set(&[Snow])),
                ResolvedDuration::UntilEndOfTurn,
            ),
            spell(
                6,
                BodyTarget::SinglePermanent,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ],
        vec![CardId(1), CardId(2), CardId(3)],
    );
    let (changed_donor, _) = resolve(&game, 4, object(&game, 3));
    let (changed_receiver, _) = resolve(changed_donor.state(), 5, object(&game, 1));
    let copied = copy(changed_receiver.state(), 1, 3);
    let receiver = object(copied.state(), 1);
    assert_eq!(
        current(copied.state(), receiver).defined.supertypes,
        set(&[Basic])
    );
    assert_eq!(
        current(copied.state(), receiver).current_supertypes,
        set(&[Basic, Snow])
    );
    assert_eq!(
        current(copied.state(), object(&game, 3)).current_supertypes,
        set(&[Snow])
    );
    assert_eq!(
        copied.state().execution().copiable_program(receiver),
        Some(&donor)
    );
    let old_donor = object(copied.state(), 3);
    let (departed, _) = resolve(copied.state(), 6, old_donor);
    assert!(
        departed
            .state()
            .execution()
            .window()
            .cards()
            .get(old_donor)
            .is_none()
    );
    assert_eq!(
        departed
            .state()
            .execution()
            .window()
            .cards()
            .get(object(departed.state(), 3))
            .unwrap()
            .zone(),
        ObjectZone::Hand(P0)
    );
    let second = copy(departed.state(), 2, 1);
    assert_eq!(
        current(second.state(), object(second.state(), 2)).current_supertypes,
        set(&[Basic])
    );
    let cleaned = cleanup(second.state());
    assert_eq!(
        current(cleaned.state(), receiver).current_supertypes,
        set(&[Basic])
    );
}
