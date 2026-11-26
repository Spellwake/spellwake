#![allow(dead_code, unused_imports)]
//! Independent closed-subset PT scenarios frozen in pt-layer-scenarios.md.
//! Native observations do not qualify full layers, cards, durations or reentry.
use spellwake_engine::program::{BodyTarget, ExecutableBody};
#[path = "support/damage.rs"]
pub mod damage_support;
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    characteristics::{
        ContinuousEffectId, EffectInstanceKey, PowerToughness, PowerToughnessCounters,
        PowerToughnessEvaluation, ResolvedPartSpec,
    },
    effects::{CopyTargetChoice, EffectError, EffectWindow, Instruction, Program, SeedCard},
    game::{CandidateBase, GameCommand, GameContext, GameError, GameId, GameInput, GameState},
    mana::{FloatingMana, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityError, PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackWindowError, WindowPhase},
    targets::TargetRef,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const FIRST: u64 = 100;
const SECOND: u64 = 101;
const HUGE: &str = "900719925474099312345678901234567890";
const HUGE_MINUS_ONE: &str = "900719925474099312345678901234567889";
const HUGE_PLUS_ONE: &str = "900719925474099312345678901234567891";

fn exact(value: &str) -> RulesInteger {
    value.to_owned().try_into().unwrap()
}

fn pt(power: i64, toughness: i64) -> PowerToughness {
    PowerToughness {
        power: power.into(),
        toughness: toughness.into(),
    }
}

fn counts(plus: i64, minus: i64) -> PowerToughnessCounters {
    PowerToughnessCounters {
        plus_one: plus.into(),
        minus_one: minus.into(),
    }
}

fn context() -> GameContext {
    GameContext::new(
        "pt-native-tests".into(),
        "rules-20260925".into(),
        "synthetic-definitions".into(),
        "literal-pt-programs".into(),
        "individual-turn".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}

fn instant(id: u64, owner: SeatId, instruction: ExecutableBody) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(ManaCost::one(ManaType::Blue), instruction),
    }
}

fn creature(id: u64, owner: SeatId, controller: SeatId, values: PowerToughness) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Battlefield { controller },
        program: Program::creature(
            None,
            values.power,
            values.toughness,
            std::collections::BTreeSet::new(),
        ),
    }
}

fn seeds(
    values: PowerToughness,
    instructions: Vec<(u64, SeatId, ExecutableBody)>,
) -> Vec<SeedCard> {
    let mut cards = vec![
        creature(FIRST, B, B, values.clone()),
        creature(SECOND, A, A, values),
    ];
    cards.extend(
        instructions
            .into_iter()
            .map(|(id, owner, instruction)| instant(id, owner, instruction)),
    );
    cards
}

fn initialize(cards: Vec<SeedCard>, mana: Vec<FloatingMana>) -> GameState {
    GameState::initialize(GameId(51), context(), {
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
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    })
    .unwrap()
    .into_state()
}

fn setup(values: PowerToughness, instructions: Vec<(u64, SeatId, ExecutableBody)>) -> GameState {
    initialize(
        seeds(values, instructions),
        [A, B]
            .into_iter()
            .map(|player| FloatingMana {
                player,
                mana_type: ManaType::Blue,
                amount: 100.into(),
            })
            .collect(),
    )
}

fn object(game: &GameState, card: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}

fn zone(game: &GameState, card: u64) -> ObjectZone {
    game.execution()
        .window()
        .cards()
        .get(object(game, card))
        .unwrap()
        .zone()
}

fn evaluation(game: &GameState, card: u64) -> PowerToughnessEvaluation {
    game.execution()
        .power_toughness(object(game, card))
        .unwrap()
        .unwrap()
}

fn input(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    }
}

fn advance(game: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    let before = game.clone();
    let next = game.advance(input(game, actor, command)).unwrap();
    damage_support::assert_transition_damage(game, &next);
    assert_eq!(next.base(), &CandidateBase::Advance(game.basis().clone()));
    assert_eq!(next.state().basis().revision, game.basis().revision + 1);
    assert_eq!(next.state().basis().game, game.basis().game);
    assert_eq!(next.state().basis().context, game.basis().context);
    assert_eq!(next.state().basis().epoch, game.basis().epoch);
    assert_eq!(game, &before);
    next.into_state()
}

fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected settled priority")
    };
    holder
}

fn holding(game: GameState, actor: SeatId) -> GameState {
    if holder(&game) == actor {
        game
    } else {
        let game = advance(&game, holder(&game), GameCommand::Pass);
        assert_eq!(holder(&game), actor);
        game
    }
}

fn cast(game: GameState, actor: SeatId, card: u64, target: TargetRef) -> (GameState, StackEntryId) {
    let game = holding(game, actor);
    let game = advance(
        &game,
        actor,
        GameCommand::Cast {
            spell: object(&game, card),
            target,
        },
    );
    let entry = game.execution().window().entries().last().unwrap().id();
    (game, entry)
}

fn pass_round(game: GameState) -> GameState {
    let game = advance(&game, holder(&game), GameCommand::Pass);
    advance(&game, holder(&game), GameCommand::Pass)
}

fn apply(game: GameState, card: u64, target: ObjectId) -> (GameState, StackEntryId) {
    let (game, source) = cast(game, A, card, TargetRef::Object(target));
    let game = pass_round(game);
    assert!(game.pending_decision().is_none());
    (game, source)
}

fn effect_for(state: &EffectWindow, object: ObjectId, source: StackEntryId) -> ContinuousEffectId {
    state
        .power_toughness(object)
        .unwrap()
        .unwrap()
        .applied_effects
        .into_iter()
        .map(|key| {
            let EffectInstanceKey::Resolved(id) = key else {
                panic!("this fixture has only resolved effects")
            };
            id
        })
        .find(|id| state.continuous_effect(*id).unwrap().creator == source)
        .unwrap()
}

fn reject(game: &GameState, request: GameInput, error: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(request), Err(error));
    assert_eq!(game, &saved);
}

/// Preserve the real executor's interval that GameState intentionally drives past.
/// The last pass and resolution are ordinary kernel operations, never registry edits.
fn resolve_before_checks(game: GameState) -> EffectWindow {
    let game = advance(&game, holder(&game), GameCommand::Pass);
    let execution = game
        .execution()
        .pass(game.execution().revision(), holder(&game))
        .unwrap();
    let (execution, spellwake_engine::effects::RuleResult::Complete { .. }, _) = execution
        .begin_resolution(execution.revision())
        .unwrap()
        .into_parts()
    else {
        panic!("expected a literal instruction without a resolution choice")
    };
    assert!(matches!(
        execution.window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
    ));
    execution
}

#[test]
fn untouched_queries_are_exact_read_only_and_have_no_runtime_effects() {
    let game = setup(pt(2, 3), vec![]);
    let before = game.clone();
    let expected = PowerToughnessEvaluation {
        object: object(&game, FIRST),
        copiable: Some(pt(2, 3).into()),
        base: pt(2, 3),
        derived: pt(2, 3),
        applied_effects: vec![],
    };
    assert_eq!(evaluation(&game, FIRST), expected);
    assert_eq!(evaluation(&game.clone(), FIRST), expected);
    assert_eq!(
        game.execution()
            .power_toughness_counters(object(&game, FIRST)),
        Some(counts(0, 0))
    );
    assert_eq!(game, before);
}

fn pending_copy(instruction: Instruction) -> (GameState, StackEntryId) {
    let game = setup(
        pt(2, 3),
        vec![
            (
                1,
                A,
                ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
            ),
            (
                2,
                B,
                ExecutableBody::new(
                    BodyTarget::SingleSpell(
                        spellwake_engine::program::SpellFilter::copyable_instruction(),
                    ),
                    vec![Instruction::CopySpell],
                ),
            ),
            (
                3,
                A,
                ExecutableBody::new(
                    BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                    vec![Instruction::CounterSpell],
                ),
            ),
        ],
    );
    let target = object(&game, FIRST);
    let (game, original) = cast(game, A, 1, TargetRef::Object(target));
    let (game, _) = cast(game, B, 2, TargetRef::Stack(original));
    let game = pass_round(game);
    assert!(game.pending_decision().is_some());
    (game, original)
}

fn reply_copy(game: &GameState, target: TargetRef) -> GameState {
    advance(
        game,
        B,
        GameCommand::Reply {
            decision: game.pending_decision().unwrap().id(),
            choice: (CopyTargetChoice::Change(target)).into(),
        },
    )
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("rules-20260925")
}
