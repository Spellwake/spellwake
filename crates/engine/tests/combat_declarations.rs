#![allow(dead_code, unused_imports)]
//! Ordinary declarations and damage through the exact native root. These literal
//! programs do not qualify keywords, complete cards or games.
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::{
    combat::{
        AssignmentOrigin, AttackDeclaration, BlockDeclaration, BlockStatus, CombatDamageAssignment,
        CombatDecision, CombatDeclarationRecord, CombatError, CombatRules, DeclarationOrigin,
    },
    damage::{
        DamageAllocation, DamageError, DamageKind, DamageOrigin, DamageRecipient, DamageSourceId,
    },
    effects::{DamageDecision, EffectError, Instruction, Program, SeedCard},
    game::{
        DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput,
        GameState, NativeCandidate, OpeningSetup, RuleFrame,
    },
    mana::{ManaCost, ManaType},
    object_identity::{CardId, GameObject, ObjectDestination, ObjectZone},
    opening::MulliganChoice,
    priority::{PriorityPhase, SeatId},
    setup::GameSeed,
    shuffle::EntropyTape,
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStage, TurnStart, TurnWork},
};

const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);
const C: SeatId = SeatId(90);
const D: SeatId = SeatId(2);

fn context() -> GameContext {
    GameContext::new(
        "declarations".into(),
        "cr-20260925".into(),
        "authored".into(),
        "ordinary-creatures".into(),
        "explicit-ffa".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn creature(id: u64, player: SeatId, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: player,
        destination,
        program: Program::creature(
            Some(ManaCost::free()),
            2.into(),
            4.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}
fn battlefield(id: u64, player: SeatId) -> SeedCard {
    creature(
        id,
        player,
        ObjectDestination::Battlefield { controller: player },
    )
}
fn instant(id: u64, player: SeatId, instruction: Instruction) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: player,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::SINGLE_CREATURE, vec![instruction]),
        ),
    }
}
fn setup(seats: Vec<SeatId>, mut cards: Vec<SeedCard>, step: Step) -> NativeCandidate {
    for player in &seats {
        cards.push(creature(
            1000 + u64::from(player.0),
            *player,
            ObjectDestination::LibraryBottom,
        ));
    }
    GameState::initialize_scheduled(
        GameId(508),
        context(),
        {
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
                seats,
                A,
                cards,
                initial_battlefield_order,
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
        TurnStart { number: 1, step },
    )
    .unwrap()
}
fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected actual ready priority")
    };
    holder
}
fn input(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    }
}
fn act(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    game.advance(input(game, actor, command)).unwrap()
}
fn reply(game: &GameState, choice: DecisionReply) -> NativeCandidate {
    let pending = game.pending_decision().unwrap();
    act(
        game,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
}
fn round(game: &GameState) -> NativeCandidate {
    let mut state = game.clone();
    for _ in 1..game.execution().window().participants().len() {
        state = act(&state, holder(&state), GameCommand::Pass).into_state();
    }
    act(&state, holder(&state), GameCommand::Pass)
}
fn attackers(game: &GameState) -> GameState {
    let mut next = game.clone();
    for _ in 0..32 {
        if next.turn().unwrap().position().step == Step::DeclareAttackers {
            return next;
        }
        next = act(&next, holder(&next), GameCommand::Pass).into_state();
    }
    panic!("declaration boundary was not reached")
}
fn reject(game: &GameState, incoming: GameInput, expected: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(incoming), Err(expected));
    assert_eq!(game, &saved);
}

#[test]
fn opening_and_entropy_candidates_keep_all_facts_current_and_preserve_bound_choices() {
    let mut seeds = vec![];
    for player in [A, B] {
        for index in 0..8 {
            seeds.push(creature(
                u64::from(player.0) * 100 + index,
                player,
                ObjectDestination::LibraryBottom,
            ));
        }
    }
    let initial = GameState::initialize_opening(
        GameId(509),
        context(),
        OpeningSetup {
            seed: {
                let cards: Vec<SeedCard> = seeds;
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
            },
            entropy: EntropyTape::new(vec![0; 64]),
        },
    )
    .unwrap();
    assert!(!initial.shuffle_journal().is_empty());
    assert!(!initial.draw_journal().events.is_empty());
    assert!(initial.combat_declarations().is_empty());
    let supplied = initial
        .state()
        .supply_entropy(initial.state().basis(), vec![1, 2])
        .unwrap();
    assert!(supplied.shuffle_journal().is_empty());
    assert!(supplied.draw_journal().events.is_empty());
    assert!(supplied.turn_transitions().is_empty());
    assert!(supplied.combat_declarations().is_empty());
    assert!(supplied.damage_journal().attempts.is_empty());
    assert!(supplied.permanent_entry().is_none());
    assert!(supplied.land_play().is_none());
    assert!(supplied.mana_payment().is_none());
    assert!(supplied.ability_activation().is_none());
    assert!(supplied.mana_production().is_none());
    assert_eq!(
        supplied.state().pending_decision().unwrap().basis(),
        supplied.state().basis()
    );
    let accepted = reply(
        supplied.state(),
        DecisionReply::Mulligan(MulliganChoice::Keep),
    );
    assert!(accepted.shuffle_journal().is_empty());
    assert!(accepted.draw_journal().events.is_empty());
    assert!(accepted.combat_declarations().is_empty());
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr-20260925")
}

#[path = "combat_keywords/damage.rs"]
mod keyword_damage;

#[path = "combat_keywords/blocking.rs"]
mod keyword_blocking;

#[path = "combat_keywords/triggers.rs"]
mod keyword_triggers;

#[path = "combat_keywords/poison.rs"]
mod keyword_poison;

#[path = "combat_keywords/advanced.rs"]
mod advanced_combat_keywords;

#[path = "combat_keywords/protection.rs"]
mod protection_damage_keywords;

#[path = "upkeep_keywords/behavior.rs"]
mod upkeep_keywords;

#[path = "upkeep_keywords/counters.rs"]
mod counter_keywords;

#[path = "combat_keywords/growth.rs"]
mod growth_keywords;

#[path = "combat_keywords/transfers.rs"]
mod transfer_keywords;

#[path = "combat_keywords/attacking_tokens.rs"]
mod attacking_tokens;

fn cards() -> Vec<SeedCard> {
    let bounce = || {
        Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
            spellwake_engine::toolbox::ObjectQuery::Targets,
            spellwake_engine::toolbox::ZoneDest::Hand,
        )))
    };
    vec![
        battlefield(1, A),
        battlefield(2, A),
        battlefield(3, B),
        battlefield(4, B),
        SeedCard {
            card: CardId(20),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: Program::artifact(None, vec![]),
        },
        instant(30, A, bounce()),
        instant(31, B, bounce()),
        instant(
            32,
            A,
            spellwake_engine::program::Instruction::DealDamage(Box::new(
                spellwake_engine::toolbox::DealDamageSpec {
                    source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                    recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(
                        spellwake_engine::program::ValueExpr::Literal(4.into()),
                    ),
                    exile_if_dies: false,
                },
            )),
        ),
        creature(40, A, ObjectDestination::Hand),
    ]
}
fn object(game: &GameState, id: u64) -> &GameObject {
    let cards = game.execution().window().cards();
    cards
        .get(cards.current_identity(CardId(id)).unwrap())
        .unwrap()
}
fn declared(game: &GameState, selected: &[(u64, SeatId)]) -> NativeCandidate {
    reply(
        game,
        DecisionReply::DeclareAttackers(
            selected
                .iter()
                .map(|&(id, defender)| AttackDeclaration {
                    attacker: object(game, id).id(),
                    defender,
                    defending_planeswalker: None,
                })
                .collect(),
        ),
    )
}
fn blocked(game: &GameState, selected: &[(u64, u64)]) -> NativeCandidate {
    reply(
        game,
        DecisionReply::DeclareBlockers(
            selected
                .iter()
                .map(|&(blocker, attacker)| BlockDeclaration {
                    blocker: object(game, blocker).id(),
                    attacker: object(game, attacker).id(),
                })
                .collect(),
        ),
    )
}
