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
fn setup(amount: RulesInteger, toughness: RulesInteger) -> EffectWindow {
    EffectWindow::new(GameSeed::with_uniform_life(
        vec![A, B],
        A,
        vec![
            SeedCard {
                card: CardId(1),
                owner: A,
                destination: ObjectDestination::Hand,
                program: Program::instant(
                    ManaCost::one(ManaType::Red),
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::Damage { amount }],
                    ),
                ),
            },
            SeedCard {
                card: CardId(2),
                owner: B,
                destination: ObjectDestination::Hand,
                program: Program::instant(
                    ManaCost::one(ManaType::Blue),
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::ReturnToOwnersHand],
                    ),
                ),
            },
            SeedCard {
                card: CardId(3),
                owner: B,
                destination: ObjectDestination::Battlefield { controller: B },
                program: Program::creature(
                    None,
                    2.into(),
                    toughness,
                    std::collections::BTreeSet::new(),
                ),
            },
        ],
        vec![CardId(3)],
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
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        test_vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    ))
    .unwrap()
}
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
fn damage_is_marked_before_lethal_state_based_action_and_priority_stays_blocked() {
    let state = close_round(cast_bolt(setup(3.into(), 2.into())));
    let lion = identity(&state, 3);
    let state = state.resolve_top(state.revision()).unwrap();
    assert_eq!(state.creature(lion).unwrap().unwrap().damage, 3.into());
    assert!(matches!(
        state.window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
    ));
    assert!(state.pass(state.revision(), A).is_err());
    let state = ready(state);
    assert!(state.creature(lion).unwrap().is_none());
    assert_ne!(identity(&state, 3), lion);
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 3))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(B)
    );
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
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
            TargetRef::Object(original),
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
        ObjectZone::Hand(B)
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
        ObjectZone::Hand(B)
    );
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 2))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(B)
    );
    assert!(state.window().entries().is_empty());
    assert_eq!(state.mana(B, ManaType::Blue), 0.into());
}

#[test]
fn nonlethal_damage_remains_until_later_effects_and_bounce_clears_incarnation_damage() {
    let state = close_round(cast_bolt(setup(1.into(), 2.into())));
    let state = ready(state.resolve_top(state.revision()).unwrap());
    assert_eq!(
        state.creature(identity(&state, 3)).unwrap().unwrap().damage,
        1.into()
    );
    let state = state.pass(state.revision(), A).unwrap();
    let state = ready(state);
    let state = state
        .cast(
            state.revision(),
            B,
            identity(&state, 2),
            TargetRef::Object(identity(&state, 3)),
        )
        .unwrap();
    let state = pass(pass(state, B), A);
    let state = state.resolve_top(state.revision()).unwrap();
    assert!(state.creature(identity(&state, 3)).unwrap().is_none());
    assert_eq!(
        state
            .window()
            .cards()
            .get(identity(&state, 3))
            .unwrap()
            .zone(),
        ObjectZone::Hand(B)
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
        state.cast(state.revision() - 1, A, bolt, TargetRef::Object(lion)),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(
        state.cast(state.revision(), A, bounce, TargetRef::Object(lion)),
        Err(EffectError::NotOwnHand)
    );
    assert_eq!(
        state.cast(state.revision(), A, bolt, TargetRef::Object(bounce)),
        Err(EffectError::IllegalTarget)
    );
    assert_eq!(
        state.cast(state.revision(), B, bounce, TargetRef::Object(lion)),
        Err(EffectError::Window(StackWindowError::Priority(
            spellwake_engine::priority::PriorityError::WrongPlayer
        )))
    );
    assert_eq!(state, before);
    let cast = state
        .cast(state.revision(), A, bolt, TargetRef::Object(lion))
        .unwrap();
    assert_eq!(cast.mana(A, ManaType::Red), 0.into());
    assert_eq!(state.mana(A, ManaType::Red), 1.into());
    assert!(
        cast.cast(cast.revision(), A, bolt, TargetRef::Object(lion))
            .is_err()
    );
}

#[test]
fn inadequate_mana_rejects_before_any_spell_move() {
    // No floating red mana is available in this setup.
    let state = EffectWindow::new(GameSeed::with_uniform_life(
        vec![A, B],
        A,
        vec![
            SeedCard {
                card: CardId(1),
                owner: A,
                destination: ObjectDestination::Hand,
                program: Program::instant(
                    ManaCost::one(ManaType::Red),
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::Damage { amount: 3.into() }],
                    ),
                ),
            },
            SeedCard {
                card: CardId(3),
                owner: B,
                destination: ObjectDestination::Battlefield { controller: B },
                program: Program::creature(
                    None,
                    2.into(),
                    2.into(),
                    std::collections::BTreeSet::new(),
                ),
            },
        ],
        vec![CardId(3)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        test_vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    ))
    .unwrap();
    let state = ready(state);
    assert_eq!(
        state.cast(
            state.revision(),
            A,
            identity(&state, 1),
            TargetRef::Object(identity(&state, 3))
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
    assert_eq!(
        state.creature(identity(&state, 3)).unwrap().unwrap().damage,
        amount
    );
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
        ObjectZone::Graveyard(B)
    );
    assert_eq!(
        state.cast(
            state.revision(),
            A,
            identity(&state, 1),
            TargetRef::Object(identity(&state, 3))
        ),
        Err(EffectError::IllegalTarget)
    );
}

#[test]
fn invalid_closed_world_setup_is_rejected_instead_of_silently_omitting_rules() {
    let instant = SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: Program::instant(
            ManaCost::one(ManaType::Red),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ),
    };
    assert_eq!(
        EffectWindow::new({
            let cards: Vec<SeedCard> = vec![instant];
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
        }),
        Err(EffectError::InvalidSetup)
    );
    assert_eq!(
        EffectWindow::new(GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![],
            vec![],
            vec![FloatingMana {
                player: A,
                mana_type: ManaType::Red,
                amount: (-1).into()
            }],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )),
        Err(EffectError::InvalidSetup)
    );
    assert_eq!(
        EffectWindow::new(GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![],
            vec![],
            vec![FloatingMana {
                player: SeatId(9),
                mana_type: ManaType::Red,
                amount: 1.into()
            }],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )),
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
                (lion, ObjectDestination::Graveyard),
                (bolt, ObjectDestination::Graveyard)
            ]
        ),
        Err(StackWindowError::InvalidCheckMove)
    );
    assert_eq!(
        window.record_check_moves(
            window.revision(),
            vec![
                (lion, ObjectDestination::Graveyard),
                (lion, ObjectDestination::Hand)
            ]
        ),
        Err(StackWindowError::InvalidCheckMove)
    );
    assert_eq!(window, &saved);
    let ready = ready(state);
    assert_eq!(
        ready
            .window()
            .record_check_moves(ready.revision(), vec![(lion, ObjectDestination::Graveyard)]),
        Err(StackWindowError::ChecksNotPending)
    );
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
