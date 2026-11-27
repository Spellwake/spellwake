//! Faults at the actual root boundary, inaccessible to production callers.
use super::*;
use crate::combat::CombatRules;
use crate::program::{BodyTarget, ExecutableBody};
use crate::setup::GameSeed;
use crate::{
    effects::Program,
    mana::{ManaAbility, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination},
    turn::Step,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn fixture() -> GameState {
    let mut cards: Vec<_> = (1..=2)
        .map(|id| SeedCard {
            card: CardId(id),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::land(
                std::collections::BTreeSet::new(),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Red, 1.into())])),
                }],
            ),
        })
        .collect();
    cards.push(SeedCard {
        card: CardId(3),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::sorcery(
            ManaCost::one(ManaType::Red),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![crate::program::Instruction::DealDamage(Box::new(
                    crate::toolbox::DealDamageSpec {
                        source: crate::toolbox::DamageSourceQuery::EntrySource,
                        recipients: crate::toolbox::DamageTargetQuery::Targets,
                        amount: crate::program::DamageAmountSpec::Expr(
                            crate::program::ValueExpr::Literal(1.into()),
                        ),
                        exile_if_dies: false,
                    },
                ))],
            ),
        ),
    });
    cards.push(SeedCard {
        card: CardId(4),
        owner: B,
        destination: ObjectDestination::Battlefield { controller: B },
        program: Program::creature(None, 3.into(), 3.into(), std::collections::BTreeSet::new()),
    });
    GameState::initialize_scheduled(
        GameId(7),
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
            cards,
            vec![CardId(4)],
            vec![
                FloatingMana {
                    player: A,
                    mana_type: ManaType::Red,
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
fn object(state: &GameState, id: u64) -> ObjectId {
    state
        .execution
        .window()
        .cards()
        .current_identity(CardId(id))
        .unwrap()
}
fn play(state: &GameState) -> GameInput {
    GameInput {
        basis: state.basis.clone(),
        actor: A,
        command: GameCommand::PlayLand {
            land: object(state, 1),
        },
    }
}
#[test]
fn land_immediate_and_late_faults_preserve_cards_count_priority_and_resources() {
    for fault in 0..9 {
        let mut state = fixture();
        match fault {
            0 => state.basis.revision = u64::MAX,
            1 => state.execution.set_test_revision(u64::MAX),
            2 => state.execution.set_test_window_revision(u64::MAX),
            3 => state.execution.set_test_priority_revision(u64::MAX),
            4 => state.execution.set_test_object_allocator(u64::MAX, false),
            5 => state.execution.set_test_object_allocator(u64::MAX, true),
            6 => state.execution.set_test_revision(u64::MAX - 1),
            7 => state.execution.set_test_window_revision(u64::MAX - 1),
            8 => state.execution.set_test_priority_revision(u64::MAX - 1),
            _ => unreachable!(),
        }
        let before = state.clone();
        assert!(state.advance(play(&state)).is_err(), "fault {fault}");
        assert_eq!(state, before, "fault {fault}");
        assert_eq!(state.turn().unwrap().lands_played(), &0.into());
        assert_eq!(state.execution.mana(A, ManaType::Red), 7.into());
        assert_eq!(state.execution.mana(B, ManaType::Blue), 11.into());
        assert!(state.execution.window().entries().is_empty());
    }
}
#[test]
fn land_does_not_need_a_stack_entry_identity() {
    let mut state = fixture();
    state.execution.set_test_stack_next_entry(u64::MAX);
    let played = state.advance(play(&state)).unwrap();
    assert!(played.land_play().is_some());
    assert!(played.mana_payment().is_none());
    assert!(played.state().execution.window().entries().is_empty());
    assert_eq!(played.state().turn().unwrap().lands_played(), &1.into());
}
#[test]
fn failed_later_sorcery_retains_accepted_land_and_count_without_debit() {
    let state = fixture();
    let paid = state.advance(play(&state)).unwrap().into_state();
    for fault in 0..4 {
        let mut state = paid.clone();
        match fault {
            0 => state.execution.set_test_stack_next_entry(u64::MAX),
            1 => state.execution.set_test_revision(u64::MAX - 1),
            2 => state.execution.set_test_window_revision(u64::MAX - 1),
            3 => state.execution.set_test_priority_revision(u64::MAX - 1),
            _ => unreachable!(),
        }
        let before = state.clone();
        let input = GameInput {
            basis: state.basis.clone(),
            actor: A,
            command: GameCommand::Cast {
                spell: object(&state, 3),
                target: TargetRef::Object(object(&state, 4)),
            },
        };
        assert!(state.advance(input).is_err(), "fault {fault}");
        assert_eq!(state, before);
        assert_eq!(state.turn().unwrap().lands_played(), &1.into());
        assert_eq!(state.execution.mana(A, ManaType::Red), 7.into());
        assert_eq!(
            state
                .execution
                .window()
                .cards()
                .get(object(&state, 1))
                .unwrap()
                .zone(),
            ObjectZone::Battlefield
        );
    }
}
