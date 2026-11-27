#![allow(dead_code, unused_imports)]
//! Restricted executable interactions, not full-card or full-game certification.
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    effects::{EffectError, EffectWindow, Instruction, Program, SeedCard},
    mana::{FloatingMana, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackWindowError, WindowPhase},
    targets::TargetRef,
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn identity(state: &EffectWindow, card: u64) -> ObjectId {
    state
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn ready(state: EffectWindow) -> EffectWindow {
    state.settle_checks(state.revision()).unwrap()
}
fn pass(state: EffectWindow, player: SeatId) -> EffectWindow {
    let state = ready(state);
    state.pass(state.revision(), player).unwrap()
}
fn cast_bolt(state: EffectWindow) -> EffectWindow {
    let state = ready(state);
    state
        .cast(
            state.revision(),
            A,
            identity(&state, 1),
            TargetRef::Object(identity(&state, 3)),
        )
        .unwrap()
}
fn close_round(state: EffectWindow) -> EffectWindow {
    pass(pass(state, A), B)
}

#[test]
fn multiple_zero_toughness_creatures_leave_before_priority_and_keep_their_owners() {
    let state = EffectWindow::new({
        let cards: Vec<SeedCard> = vec![A, B]
            .into_iter()
            .enumerate()
            .map(|(index, owner)| SeedCard {
                card: CardId(index as u64),
                owner,
                destination: ObjectDestination::Battlefield { controller: A },
                program: Program::creature(
                    None,
                    5.into(),
                    0.into(),
                    std::collections::BTreeSet::new(),
                ),
            })
            .collect();
        // Fixture premise: seeded BF timestamps follow this authored card sequence.
        let initial_battlefield_order = cards
            .iter()
            .filter(|card| {
                matches!(
                    card.destination,
                    spellwake_engine::object_identity::ObjectDestination::Battlefield { .. }
                )
            })
            .map(|card| card.card)
            .collect();
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            initial_battlefield_order,
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    })
    .unwrap();
    let saved = state.clone();
    let next = ready(state.clone());
    for card in next.window().cards().iter() {
        assert_eq!(card.zone(), ObjectZone::Graveyard(card.owner()));
        assert_eq!(card.controller(), None);
    }
    assert_eq!(next.revision(), state.revision() + 1);
    assert_eq!(next.window().revision(), 2);
    assert_eq!(state, saved);
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("synthetic-only-tests")
}
