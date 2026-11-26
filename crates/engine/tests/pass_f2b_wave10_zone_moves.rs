#![forbid(unsafe_code)]
//! Combined-zone exile keeps target scope and a single simultaneous movement batch.

#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;

use spellwake_engine::{
    effects::SeedCard,
    game::{GameCommand, GameState},
    mana::{ManaCost, ManaPayment},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::{RulesOccurrence, RulesRecord},
    priority::SeatId,
    program::{
        AffectedPlayers, BodyTarget, Instruction, ObjectFilter, Program, ResolvingPlayer,
        SourceZoneFilter,
    },
    targets::TargetRef,
};
use trajectories::*;

fn filler(id: u64, owner: SeatId, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program: Program::artifact(Some(ManaCost::free()), vec![]),
    }
}

fn combined_exile() -> SeedCard {
    spell(
        1,
        BodyTarget::SinglePlayer,
        vec![Instruction::ExileGroupFromZone {
            players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
            zone: SourceZoneFilter::HandOrGraveyard,
            filter: Box::new(ObjectFilter::any()),
        }],
    )
}

fn zone(game: &GameState, id: u64) -> ObjectZone {
    game.execution()
        .window()
        .cards()
        .get(object(game, id))
        .unwrap()
        .zone()
}

#[test]
fn combined_exile_moves_only_targets_hand_and_graveyard_in_one_batch() {
    let game = world(
        vocabulary(),
        vec![
            combined_exile(),
            filler(2, P1, ObjectDestination::Hand),
            filler(3, P1, ObjectDestination::Graveyard),
            filler(4, P1, ObjectDestination::LibraryBottom),
            filler(5, P1, ObjectDestination::Battlefield { controller: P1 }),
            filler(6, P0, ObjectDestination::Hand),
            filler(7, P0, ObjectDestination::Graveyard),
        ],
        vec![CardId(5)],
    );
    let cast = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 1),
            target: TargetRef::Player(P1),
            payment: ManaPayment::default(),
        },
    );
    let resolved = pass_pair(cast.state());
    assert_eq!(zone(resolved.state(), 2), ObjectZone::Exile);
    assert_eq!(zone(resolved.state(), 3), ObjectZone::Exile);
    assert_eq!(zone(resolved.state(), 4), ObjectZone::Library(P1));
    assert_eq!(zone(resolved.state(), 5), ObjectZone::Battlefield);
    assert_eq!(zone(resolved.state(), 6), ObjectZone::Hand(P0));
    assert_eq!(zone(resolved.state(), 7), ObjectZone::Graveyard(P0));
    let exile_batches: Vec<usize> = resolved.rules_journal().records.iter().filter_map(|record| {
        let RulesRecord::Event(event) = record else { return None; };
        let count = event.occurrences.iter().filter(|row| matches!(
            &row.value, RulesOccurrence::ZoneChange(change) if change.after.object.zone() == ObjectZone::Exile
        )).count();
        (count > 0).then_some(count)
    }).collect();
    assert_eq!(exile_batches, vec![2]);
}

#[test]
fn combined_exile_of_empty_target_zones_resolves_without_touching_others() {
    let game = world(
        vocabulary(),
        vec![
            combined_exile(),
            filler(2, P0, ObjectDestination::Hand),
            filler(3, P0, ObjectDestination::Graveyard),
            filler(4, P1, ObjectDestination::Battlefield { controller: P1 }),
        ],
        vec![CardId(4)],
    );
    let cast = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 1),
            target: TargetRef::Player(P1),
            payment: ManaPayment::default(),
        },
    );
    let resolved = pass_pair(cast.state());
    assert_eq!(zone(resolved.state(), 1), ObjectZone::Graveyard(P0));
    assert_eq!(zone(resolved.state(), 2), ObjectZone::Hand(P0));
    assert_eq!(zone(resolved.state(), 3), ObjectZone::Graveyard(P0));
    assert_eq!(zone(resolved.state(), 4), ObjectZone::Battlefield);
}
