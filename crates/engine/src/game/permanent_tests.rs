//! Operational failures before and after actual permanent entry remain atomic.
use super::*;
use crate::combat::CombatRules;
use crate::setup::GameSeed;
use crate::{
    effects::Program,
    mana::{ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination},
    turn::Step,
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn fixture(toughness: i64) -> GameState {
    GameState::initialize_scheduled(
        GameId(8),
        GameContext::new(
            "r".into(),
            "cr".into(),
            "cat".into(),
            "p".into(),
            "individual".into(),
            crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap(),
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![SeedCard {
                card: CardId(1),
                owner: A,
                destination: ObjectDestination::Hand,
                program: Program::creature(
                    Some(ManaCost::one(ManaType::Green)),
                    8.into(),
                    toughness.into(),
                    std::collections::BTreeSet::new(),
                ),
            }],
            vec![],
            vec![
                FloatingMana {
                    player: A,
                    mana_type: ManaType::Green,
                    amount: 7.into(),
                },
                FloatingMana {
                    player: B,
                    mana_type: ManaType::Blue,
                    amount: 11.into(),
                },
            ],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ),
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}
fn cast(s: &GameState) -> GameInput {
    GameInput {
        basis: s.basis.clone(),
        actor: A,
        command: GameCommand::CastUntargeted {
            spell: s
                .execution
                .window()
                .cards()
                .current_identity(CardId(1))
                .unwrap(),
            payment: None,
        },
    }
}
fn pass(s: &GameState, actor: SeatId) -> GameInput {
    GameInput {
        basis: s.basis.clone(),
        actor,
        command: GameCommand::Pass,
    }
}
#[test]
fn failed_permanent_cast_preserves_hand_resources_stack_and_count() {
    for fault in 0..9 {
        let mut s = fixture(4);
        match fault {
            0 => s.basis.revision = u64::MAX,
            1 => s.execution.set_test_revision(u64::MAX),
            2 => s.execution.set_test_window_revision(u64::MAX),
            3 => s.execution.set_test_priority_revision(u64::MAX),
            4 => s.execution.set_test_object_allocator(u64::MAX, false),
            5 => s.execution.set_test_object_allocator(u64::MAX, true),
            6 => s.execution.set_test_stack_next_entry(u64::MAX),
            7 => s.execution.set_test_revision(u64::MAX - 1),
            8 => s.execution.set_test_window_revision(u64::MAX - 1),
            _ => unreachable!(),
        }
        let before = s.clone();
        assert!(s.advance(cast(&s)).is_err(), "fault {fault}");
        assert_eq!(s, before);
        assert_eq!(s.execution.mana(A, ManaType::Green), 7.into());
        assert_eq!(s.execution.mana(B, ManaType::Blue), 11.into());
        assert_eq!(s.turn().unwrap().lands_played(), &0.into());
        assert!(s.execution.window().entries().is_empty());
    }
}
fn paid(toughness: i64) -> GameState {
    let s = fixture(toughness);
    let s = s.advance(cast(&s)).unwrap().into_state();
    s.advance(pass(&s, A)).unwrap().into_state()
}
#[test]
fn failed_permanent_resolution_retains_previously_paid_spell_and_no_entry_candidate() {
    for fault in 0..9 {
        let mut s = paid(4);
        match fault {
            0 => s.basis.revision = u64::MAX,
            1 => s.execution.set_test_revision(u64::MAX),
            2 => s.execution.set_test_revision(u64::MAX - 1),
            3 => s.execution.set_test_revision(u64::MAX - 2),
            4 => s.execution.set_test_window_revision(u64::MAX),
            5 => s.execution.set_test_window_revision(u64::MAX - 2),
            6 => s.execution.set_test_object_allocator(u64::MAX, false),
            7 => s.execution.set_test_object_allocator(u64::MAX, true),
            8 => s.execution.set_test_priority_revision(u64::MAX),
            _ => unreachable!(),
        }
        let before = s.clone();
        assert!(s.advance(pass(&s, B)).is_err(), "fault {fault}");
        assert_eq!(s, before);
        assert_eq!(s.execution.mana(A, ManaType::Green), 6.into());
        assert_eq!(s.execution.mana(B, ManaType::Blue), 11.into());
        let id = s
            .execution
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        assert_eq!(
            s.execution.window().cards().get(id).unwrap().zone(),
            ObjectZone::Stack
        );
        assert_eq!(s.execution.window().entries().len(), 1);
    }
}
#[test]
fn late_zero_toughness_death_fault_discards_physical_entry_and_fact_together() {
    for identity in [false, true] {
        let mut s = paid(0);
        s.execution
            .set_test_object_allocator(u64::MAX - 1, identity);
        let before = s.clone();
        assert!(s.advance(pass(&s, B)).is_err());
        assert_eq!(s, before);
        assert_eq!(s.execution.window().entries().len(), 1);
        assert_eq!(s.execution.mana(A, ManaType::Green), 6.into());
    }
}
#[test]
fn resolving_a_paid_permanent_does_not_allocate_another_stack_entry_or_pay_again() {
    let mut s = paid(4);
    s.execution.set_test_stack_next_entry(u64::MAX);
    let done = s.advance(pass(&s, B)).unwrap();
    let record = done.permanent_entry().unwrap();
    assert_eq!(record.before.zone(), ObjectZone::Stack);
    assert_eq!(record.after.zone(), ObjectZone::Battlefield);
    assert_ne!(record.before.id(), record.after.id());
    assert_eq!(done.state().execution.mana(A, ManaType::Green), 6.into());
    assert!(done.mana_payment().is_none());
    assert!(done.state().execution.window().entries().is_empty());
}
