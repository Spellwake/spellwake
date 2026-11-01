//! Restricted executable interactions, not full-card or full-game certification.
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    effects::{EffectError, EffectWindow, Instruction, Program, SeedCard},
    mana::{FloatingMana, ManaCost, ManaType},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackWindowError, WindowPhase},
    targets::TargetRef,
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn setup(amount: RulesInteger, toughness: RulesInteger) -> EffectWindow {
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
                    instructions: vec![Instruction::Damage { amount }],
                },
            },
            SeedCard {
                card: CardId(2),
                owner: B,
                destination: CardDestination::Hand,
                program: Program::Instant {
                    cost: ManaCost::one(ManaType::Blue),
                    instructions: vec![Instruction::ReturnToOwnersHand],
                },
            },
            SeedCard {
                card: CardId(3),
                owner: B,
                destination: CardDestination::Battlefield { controller: B },
                program: Program::VanillaCreature {
                    power: 2.into(),
                    toughness,
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
                amount: 1.into(),
            },
        ],
    )
    .unwrap()
}
fn identity(state: &EffectWindow, card: u64) -> CardObjectId {
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
            TargetRef::Card(identity(&state, 3)),
        )
        .unwrap()
}
fn close_round(state: EffectWindow) -> EffectWindow {
    pass(pass(state, A), B)
}

#[test]
fn damage_is_marked_before_lethal_state_based_action_and_priority_stays_blocked() {
    let state = close_round(cast_bolt(setup(3.into(), 2.into())));
    let lion = identity(&state, 3);
    let state = state.resolve_top(state.revision()).unwrap();
    assert_eq!(state.creature(lion).unwrap().damage, 3.into());
    assert!(matches!(
        state.window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
    ));
    assert!(state.pass(state.revision(), A).is_err());
    let state = ready(state);
    assert!(state.creature(lion).is_none());
    assert_ne!(identity(&state, 3), lion);
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 3))
            .unwrap()
            .zone(),
        CardZone::Graveyard(B)
    );
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 1))
            .unwrap()
            .zone(),
        CardZone::Graveyard(A)
    );
    assert_eq!(state.mana(A, ManaType::Red), 0.into());
    assert!(state.window().entries().is_empty());
}

#[test]
fn bounce_response_invalidates_the_original_target_and_never_damages_the_returned_card() {
    let state = pass(cast_bolt(setup(3.into(), 2.into())), A);
    let state = ready(state);
    let original = identity(&state, 3);
    let state = state
        .cast(
            state.revision(),
            B,
            identity(&state, 2),
            TargetRef::Card(original),
        )
        .unwrap();
    let state = pass(pass(state, B), A);
    let state = state.resolve_top(state.revision()).unwrap();
    assert_ne!(identity(&state, 3), original);
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 3))
            .unwrap()
            .zone(),
        CardZone::Hand(B)
    );
    assert_eq!(state.window().entries().len(), 1);
    let state = close_round(state);
    let state = state.resolve_top(state.revision()).unwrap();
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 3))
            .unwrap()
            .zone(),
        CardZone::Hand(B)
    );
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 1))
            .unwrap()
            .zone(),
        CardZone::Graveyard(A)
    );
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 2))
            .unwrap()
            .zone(),
        CardZone::Graveyard(B)
    );
    assert!(state.window().entries().is_empty());
    assert_eq!(state.mana(B, ManaType::Blue), 0.into());
}

#[test]
fn nonlethal_damage_remains_until_later_effects_and_bounce_clears_incarnation_damage() {
    let state = close_round(cast_bolt(setup(1.into(), 2.into())));
    let state = ready(state.resolve_top(state.revision()).unwrap());
    assert_eq!(
        state.creature(identity(&state, 3)).unwrap().damage,
        1.into()
    );
    let state = state.pass(state.revision(), A).unwrap();
    let state = ready(state);
    let state = state
        .cast(
            state.revision(),
            B,
            identity(&state, 2),
            TargetRef::Card(identity(&state, 3)),
        )
        .unwrap();
    let state = pass(pass(state, B), A);
    let state = state.resolve_top(state.revision()).unwrap();
    assert!(state.creature(identity(&state, 3)).is_none());
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 3))
            .unwrap()
            .zone(),
        CardZone::Hand(B)
    );
}

#[test]
fn illegal_casts_leave_mana_card_identities_stack_and_revision_unchanged() {
    let state = ready(setup(3.into(), 2.into()));
    let before = state.clone();
    let bolt = identity(&state, 1);
    let bounce = identity(&state, 2);
    let lion = identity(&state, 3);
    assert_eq!(
        state.cast(state.revision() - 1, A, bolt, TargetRef::Card(lion)),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(
        state.cast(state.revision(), A, bounce, TargetRef::Card(lion)),
        Err(EffectError::NotOwnHand)
    );
    assert_eq!(
        state.cast(state.revision(), A, bolt, TargetRef::Card(bounce)),
        Err(EffectError::IllegalTarget)
    );
    assert_eq!(
        state.cast(state.revision(), B, bounce, TargetRef::Card(lion)),
        Err(EffectError::Window(StackWindowError::Priority(
            spellwake_engine::priority::PriorityError::WrongPlayer
        )))
    );
    assert_eq!(state, before);
    let cast = state
        .cast(state.revision(), A, bolt, TargetRef::Card(lion))
        .unwrap();
    assert_eq!(cast.mana(A, ManaType::Red), 0.into());
    assert_eq!(state.mana(A, ManaType::Red), 1.into());
    assert!(
        cast.cast(cast.revision(), A, bolt, TargetRef::Card(lion))
            .is_err()
    );
}

#[test]
fn inadequate_mana_rejects_before_any_spell_move() {
    // No floating red mana is available in this setup.
    let state = EffectWindow::new(
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
                card: CardId(3),
                owner: B,
                destination: CardDestination::Battlefield { controller: B },
                program: Program::VanillaCreature {
                    power: 2.into(),
                    toughness: 2.into(),
                },
            },
        ],
        vec![],
    )
    .unwrap();
    let state = ready(state);
    assert_eq!(
        state.cast(
            state.revision(),
            A,
            identity(&state, 1),
            TargetRef::Card(identity(&state, 3))
        ),
        Err(EffectError::InsufficientMana)
    );
    assert!(state.window().entries().is_empty());
}

#[test]
fn huge_damage_and_toughness_stay_exact_without_machine_integer_overflow() {
    let amount: RulesInteger = "900719925474099312345678901234567890"
        .to_owned()
        .try_into()
        .unwrap();
    let state = close_round(cast_bolt(setup(amount.clone(), amount.plus(&1.into()))));
    let state = ready(state.resolve_top(state.revision()).unwrap());
    assert_eq!(state.creature(identity(&state, 3)).unwrap().damage, amount);
}

#[test]
fn zero_toughness_dies_during_initial_checks_before_anyone_can_act() {
    let state = ready(setup(3.into(), 0.into()));
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 3))
            .unwrap()
            .zone(),
        CardZone::Graveyard(B)
    );
    assert_eq!(
        state.cast(
            state.revision(),
            A,
            identity(&state, 1),
            TargetRef::Card(identity(&state, 3))
        ),
        Err(EffectError::IllegalTarget)
    );
}

#[test]
fn invalid_closed_world_setup_is_rejected_instead_of_silently_omitting_rules() {
    let instant = SeedCard {
        card: CardId(1),
        owner: A,
        destination: CardDestination::Battlefield { controller: A },
        program: Program::Instant {
            cost: ManaCost::one(ManaType::Red),
            instructions: vec![Instruction::ReturnToOwnersHand],
        },
    };
    assert_eq!(
        EffectWindow::new(vec![A, B], A, vec![instant], vec![]),
        Err(EffectError::InvalidSetup)
    );
    assert_eq!(
        EffectWindow::new(
            vec![A, B],
            A,
            vec![],
            vec![FloatingMana {
                player: A,
                mana_type: ManaType::Red,
                amount: (-1).into()
            }]
        ),
        Err(EffectError::InvalidSetup)
    );
    assert_eq!(
        EffectWindow::new(
            vec![A, B],
            A,
            vec![],
            vec![FloatingMana {
                player: SeatId(9),
                mana_type: ManaType::Red,
                amount: 1.into()
            }]
        ),
        Err(EffectError::InvalidSetup)
    );
}

#[test]
fn state_based_move_batch_rejects_partial_failure_without_moving_any_card() {
    let state = setup(3.into(), 2.into());
    let window = state.window();
    let saved = window.clone();
    let lion = identity(&state, 3);
    let bolt = identity(&state, 1);
    assert_eq!(
        window.record_check_moves(window.revision(), vec![]),
        Err(StackWindowError::EmptyCheckMoves)
    );
    assert_eq!(
        window.record_check_moves(
            window.revision(),
            vec![
                (lion, CardDestination::Graveyard),
                (bolt, CardDestination::Graveyard)
            ]
        ),
        Err(StackWindowError::InvalidCheckMove)
    );
    assert_eq!(
        window.record_check_moves(
            window.revision(),
            vec![
                (lion, CardDestination::Graveyard),
                (lion, CardDestination::Hand)
            ]
        ),
        Err(StackWindowError::InvalidCheckMove)
    );
    assert_eq!(window, &saved);
    let ready = ready(state);
    assert_eq!(
        ready
            .window()
            .record_check_moves(ready.revision(), vec![(lion, CardDestination::Graveyard)]),
        Err(StackWindowError::ChecksNotPending)
    );
}

#[test]
fn multiple_zero_toughness_creatures_leave_before_priority_and_keep_their_owners() {
    let state = EffectWindow::new(
        vec![A, B],
        A,
        vec![A, B]
            .into_iter()
            .enumerate()
            .map(|(index, owner)| SeedCard {
                card: CardId(index as u64),
                owner,
                destination: CardDestination::Battlefield { controller: A },
                program: Program::VanillaCreature {
                    power: 5.into(),
                    toughness: 0.into(),
                },
            })
            .collect(),
        vec![],
    )
    .unwrap();
    let saved = state.clone();
    let next = ready(state.clone());
    for card in next.window().cards().iter() {
        assert_eq!(card.zone(), CardZone::Graveyard(card.owner()));
        assert_eq!(card.controller(), None);
    }
    assert_eq!(next.revision(), state.revision() + 1);
    assert_eq!(next.window().revision(), 2);
    assert_eq!(state, saved);
}
