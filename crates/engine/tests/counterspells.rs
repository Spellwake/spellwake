//! Countering in the restricted native world (CR 701.6 and 608.2b).
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    effects::{EffectError, EffectWindow, Instruction, Program, SeedCard},
    mana::{FloatingMana, ManaCost, ManaType},
    priority::SeatId,
    stack_window::{StackEntryId, StackKind},
    targets::TargetRef,
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn setup(blue: RulesInteger, cost: RulesInteger) -> EffectWindow {
    EffectWindow::new(
        vec![A, B],
        A,
        vec![
            SeedCard {
                card: CardId(1),
                owner: A,
                destination: CardDestination::Hand,
                program: Program::Instant {
                    cost: ManaCost::one(ManaType::Red),
                    instructions: vec![Instruction::Damage { amount: 3.into() }],
                },
            },
            SeedCard {
                card: CardId(2),
                owner: B,
                destination: CardDestination::Hand,
                program: Program::Instant {
                    cost: ManaCost::specific(ManaType::Blue, cost.clone()),
                    instructions: vec![Instruction::CounterSpell],
                },
            },
            SeedCard {
                card: CardId(3),
                owner: B,
                destination: CardDestination::Battlefield { controller: B },
                program: Program::VanillaCreature {
                    power: 2.into(),
                    toughness: 2.into(),
                },
            },
            SeedCard {
                card: CardId(4),
                owner: A,
                destination: CardDestination::Hand,
                program: Program::Instant {
                    cost: ManaCost::specific(ManaType::Blue, cost),
                    instructions: vec![Instruction::CounterSpell],
                },
            },
        ],
        vec![
            FloatingMana {
                player: A,
                mana_type: ManaType::Red,
                amount: 1.into(),
            },
            FloatingMana {
                player: B,
                mana_type: ManaType::Blue,
                amount: blue.clone(),
            },
            FloatingMana {
                player: A,
                mana_type: ManaType::Blue,
                amount: blue,
            },
        ],
    )
    .unwrap()
}
fn card(state: &EffectWindow, key: u64) -> CardObjectId {
    state
        .window()
        .cards()
        .current_identity(CardId(key))
        .unwrap()
}
fn entry(state: &EffectWindow, key: u64) -> StackEntryId {
    state.window().entries().iter().find(|entry| matches!(entry.kind(), StackKind::CardSpell(object) if state.window().cards().get(*object).unwrap().card() == CardId(key))).unwrap().id()
}
fn ready(state: EffectWindow) -> EffectWindow {
    state.settle_checks(state.revision()).unwrap()
}
fn pass(state: EffectWindow, actor: SeatId) -> EffectWindow {
    let state = ready(state);
    state.pass(state.revision(), actor).unwrap()
}
fn bolt() -> EffectWindow {
    let state = ready(setup(2.into(), 2.into()));
    state
        .cast(
            state.revision(),
            A,
            card(&state, 1),
            TargetRef::Card(card(&state, 3)),
        )
        .unwrap()
}
fn counter() -> EffectWindow {
    let state = ready(pass(bolt(), A));
    state
        .cast(
            state.revision(),
            B,
            card(&state, 2),
            TargetRef::Stack(entry(&state, 1)),
        )
        .unwrap()
}
fn zone(state: &EffectWindow, key: u64) -> CardZone {
    state.window().cards().get(card(state, key)).unwrap().zone()
}

#[test]
fn countered_spell_never_executes_and_paid_costs_are_not_refunded() {
    let state = pass(pass(counter(), B), A);
    let before = state.clone();
    let state = state.resolve_top(state.revision()).unwrap();
    assert_eq!(zone(&state, 1), CardZone::Graveyard(A));
    assert_eq!(zone(&state, 2), CardZone::Graveyard(B));
    assert_eq!(state.creature(card(&state, 3)).unwrap().damage, 0.into());
    assert!(state.window().entries().is_empty());
    assert_eq!(state.mana(A, ManaType::Red), 0.into());
    assert_eq!(state.mana(B, ManaType::Blue), 0.into());
    assert_eq!(before.window().entries().len(), 2);
}

#[test]
fn countering_the_counter_preserves_the_original_spell_until_it_resolves() {
    let state = ready(pass(counter(), B));
    let state = state
        .cast(
            state.revision(),
            A,
            card(&state, 4),
            TargetRef::Stack(entry(&state, 2)),
        )
        .unwrap();
    let state = pass(pass(state, A), B);
    let state = state.resolve_top(state.revision()).unwrap();
    assert_eq!(zone(&state, 2), CardZone::Graveyard(B));
    assert_eq!(zone(&state, 4), CardZone::Graveyard(A));
    assert_eq!(state.window().entries().len(), 1);
    assert_eq!(zone(&state, 1), CardZone::Stack);
    let state = pass(pass(state, A), B);
    let state = state.resolve_top(state.revision()).unwrap();
    assert_eq!(state.creature(card(&state, 3)).unwrap().damage, 3.into());
    let state = ready(state);
    assert_eq!(zone(&state, 3), CardZone::Graveyard(B));
    assert_eq!(state.mana(A, ManaType::Blue), 0.into());
}

#[test]
fn a_counter_with_a_departed_spell_target_fails_to_resolve_without_following_card_identity() {
    let state = ready(pass(counter(), B));
    let state = state
        .cast(
            state.revision(),
            A,
            card(&state, 4),
            TargetRef::Stack(entry(&state, 1)),
        )
        .unwrap();
    let state = pass(pass(state, A), B);
    let state = state.resolve_top(state.revision()).unwrap();
    assert_eq!(zone(&state, 1), CardZone::Graveyard(A));
    assert_eq!(state.window().entries().len(), 1);
    let state = pass(pass(state, A), B);
    let state = state.resolve_top(state.revision()).unwrap();
    assert_eq!(zone(&state, 2), CardZone::Graveyard(B));
    assert_eq!(zone(&state, 3), CardZone::Battlefield);
    assert!(state.window().entries().is_empty());
}

#[test]
fn spell_and_creature_target_domains_are_distinct_and_invalid_announcements_are_atomic() {
    let state = ready(pass(bolt(), A));
    let before = state.clone();
    for target in [
        TargetRef::Card(card(&state, 3)),
        TargetRef::Card(card(&state, 1)),
        TargetRef::Player(A),
    ] {
        assert_eq!(
            state.cast(state.revision(), B, card(&state, 2), target),
            Err(EffectError::IllegalTarget)
        );
    }
    assert_eq!(state, before);
    assert_eq!(state.mana(B, ManaType::Blue), 2.into());
}

#[test]
fn a_two_blue_cost_cannot_be_paid_with_only_one_blue() {
    let state = ready(setup(1.into(), 2.into()));
    let state = state
        .cast(
            state.revision(),
            A,
            card(&state, 1),
            TargetRef::Card(card(&state, 3)),
        )
        .unwrap();
    let state = ready(pass(state, A));
    let before = state.clone();
    assert_eq!(
        state.cast(
            state.revision(),
            B,
            card(&state, 2),
            TargetRef::Stack(entry(&state, 1))
        ),
        Err(EffectError::InsufficientMana)
    );
    assert_eq!(state, before);
    assert_eq!(zone(&state, 2), CardZone::Hand(B));
}

#[test]
fn homogeneous_cost_subtraction_stays_exact_for_huge_quantities() {
    let cost: RulesInteger = "900719925474099312345678901234567890"
        .to_owned()
        .try_into()
        .unwrap();
    let state = ready(setup(cost.plus(&1.into()), cost));
    let state = state
        .cast(
            state.revision(),
            A,
            card(&state, 1),
            TargetRef::Card(card(&state, 3)),
        )
        .unwrap();
    let state = ready(pass(state, A));
    let state = state
        .cast(
            state.revision(),
            B,
            card(&state, 2),
            TargetRef::Stack(entry(&state, 1)),
        )
        .unwrap();
    assert_eq!(state.mana(B, ManaType::Blue), 1.into());
}

#[test]
fn negative_specific_cost_programs_are_rejected_at_setup() {
    for amount in [(-1).into(), (-2).into()] {
        let result = EffectWindow::new(
            vec![A, B],
            A,
            vec![SeedCard {
                card: CardId(1),
                owner: A,
                destination: CardDestination::Hand,
                program: Program::Instant {
                    cost: ManaCost::specific(ManaType::Blue, amount),
                    instructions: vec![Instruction::CounterSpell],
                },
            }],
            vec![],
        );
        assert_eq!(result, Err(EffectError::InvalidSetup));
    }
}
