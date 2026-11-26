#![allow(dead_code, unused_imports)]
//! Prepared public-boundary mechanism checks. These are not whole-card or format qualification.
use spellwake_engine::{
    characteristics::{PowerToughness, PowerToughnessCounters, ResolvedPartSpec},
    combat::CombatRules,
    effects::{EffectError, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    occurrence::{
        EntryCompletion, EventCause, RulesEvent, RulesOccurrence, RulesRecord, ZoneReason,
    },
    priority::{PriorityPhase, SeatId},
    program::{
        BodyTarget, ExecutableBody, Instruction, Program, TriggerSpec, TriggeredAbility,
        TriggeredAbilityId,
    },
    setup::GameSeed,
    stack_window::WindowPhase,
    turn::{Step, TurnStart},
};
use std::collections::BTreeSet;

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn seed(card: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination,
        program,
    }
}
fn library(card: u64, owner: SeatId) -> SeedCard {
    seed(
        card,
        owner,
        ObjectDestination::LibraryBottom,
        Program::creature(None, 1.into(), 1.into(), std::collections::BTreeSet::new()),
    )
}
fn draw() -> Instruction {
    Instruction::DrawController {
        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
    }
}
fn context() -> GameContext {
    GameContext::new(
        "trigger-tests".into(),
        "cr-20260925".into(),
        "literal".into(),
        "ordinary-trigger".into(),
        "ffa".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn game(cards: Vec<SeedCard>, scheduled: bool) -> GameState {
    let seed = {
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
    };
    if scheduled {
        GameState::initialize_scheduled(
            GameId(1701),
            context(),
            seed,
            TurnStart {
                number: 2,
                step: Step::PrecombatMain,
            },
        )
    } else {
        GameState::initialize(GameId(1701), context(), seed)
    }
    .unwrap()
    .into_state()
}
fn object(game: &GameState, card: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn submit(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    })
    .unwrap()
}
fn resolve(game: GameState) -> NativeCandidate {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected priority")
    };
    let other = if holder == A { B } else { A };
    let first = submit(&game, holder, GameCommand::Pass).into_state();
    submit(&first, other, GameCommand::Pass)
}
fn events(candidate: &NativeCandidate) -> Vec<&RulesEvent> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::Event(event) = record {
                Some(event)
            } else {
                None
            }
        })
        .collect()
}
fn watched(mut program: Program, condition: TriggerSpec, id: u64) -> Program {
    program.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(id),
        condition,
        body: ExecutableBody::new(BodyTarget::None, vec![draw()]),
    });
    program
}

#[test]
fn paid_mana_choice_captures_the_actual_tap_and_reply_emits_only_production() {
    let land = watched(
        Program::land(
            std::collections::BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(7),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: BTreeSet::from([ManaType::Blue, ManaType::Green]),
                },
            }],
        ),
        spellwake_engine::program::TriggerSpec::becomes_tapped(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        4,
    );
    // Owner differs from controller. The captured stack controller will draw.
    let initial = game(
        vec![
            seed(1, B, ObjectDestination::Battlefield { controller: A }, land),
            library(2, A),
        ],
        false,
    );
    let source = object(&initial, 1);
    let pending = submit(
        &initial,
        A,
        GameCommand::ActivateMana {
            source,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    );
    let cuts = events(&pending);
    assert_eq!(cuts.len(), 2);
    assert!(matches!(cuts[0].cause, EventCause::ManaAbilityCost { .. }));
    assert!(matches!(
        cuts[1].cause,
        EventCause::ManaAbilityActivated { .. }
    ));
    let state = pending.into_state();
    assert!(state.execution().window().entries().is_empty());
    let waiting = state.execution().waiting_triggers().next().unwrap();
    assert_eq!(waiting.source.object.owner(), B);
    assert_eq!(waiting.controller, A);
    assert_eq!(waiting.source.object.tapped(), Some(true));
    assert_eq!(state.execution().mana(A, ManaType::Blue), 0.into());
    let decision = state.pending_decision().unwrap();
    assert!(matches!(decision.frame(), RuleFrame::ManaColor(_)));
    let saved = state.clone();
    assert_eq!(
        state.advance(GameInput {
            basis: state.basis().clone(),
            actor: A,
            command: GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::ManaColor(ManaType::Red)
            }
        }),
        Err(GameError::Effect(EffectError::IllegalManaColor))
    );
    assert_eq!(state, saved);
    let produced = submit(
        &state,
        A,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::ManaColor(ManaType::Blue),
        },
    );
    let cuts = events(&produced);
    assert_eq!(cuts.len(), 1);
    let RulesOccurrence::ManaProduced { before, after, .. } = &cuts[0].occurrences[0].value else {
        panic!("production cut")
    };
    assert_eq!(before[&ManaType::Blue], 0.into());
    assert_eq!(after[&ManaType::Blue], 1.into());
    assert_eq!(before.len(), 6);
    assert_eq!(produced.state().execution().window().entries().len(), 1);
    let drawn = resolve(produced.into_state());
    assert_eq!(
        drawn
            .state()
            .execution()
            .window()
            .cards()
            .get(object(drawn.state(), 2))
            .unwrap()
            .zone(),
        ObjectZone::Hand(A)
    );
    assert!(drawn.state().execution().life_total(B).is_some());
}

#[test]
fn entry_and_death_capture_distinct_events_before_a_zero_toughness_source_leaves() {
    let creature = watched(
        watched(
            Program::creature(
                Some(ManaCost::free()),
                0.into(),
                0.into(),
                std::collections::BTreeSet::new(),
            ),
            spellwake_engine::program::TriggerSpec::enters(
                spellwake_engine::program::ObjectFilter::self_object(),
            ),
            1,
        ),
        spellwake_engine::program::TriggerSpec::dies(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        2,
    );
    let initial = game(
        vec![
            seed(1, A, ObjectDestination::Hand, creature),
            library(2, A),
            library(3, A),
        ],
        true,
    );
    let cast = submit(
        &initial,
        A,
        GameCommand::CastUntargeted {
            spell: object(&initial, 1),
            payment: None,
        },
    );
    let cast_cuts = events(&cast);
    assert_eq!(cast_cuts.len(), 2); // Announcement and completion; no free-cost debit.
    let resolved = resolve(cast.into_state());
    let discoveries: Vec<_> = resolved
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::TriggerDiscovered(waiting) = record {
                Some(waiting)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(discoveries.len(), 2);
    assert_ne!(discoveries[0].causes.event(), discoveries[1].causes.event());
    let death = events(&resolved)
        .into_iter()
        .find(|event| event.id == discoveries[1].causes.event())
        .unwrap();
    let RulesOccurrence::ZoneChange(change) = &death.occurrences[0].value else {
        panic!("actual death");
    };
    assert_eq!(
        change.reason,
        ZoneReason::StateBased(spellwake_engine::occurrence::StateBasedMoveReasons::new(
            spellwake_engine::occurrence::StateBasedMoveReason::ZeroToughness
        ))
    );
    assert_eq!(discoveries[0].source.object, discoveries[1].source.object);
    assert_eq!(discoveries[0].source.object.zone(), ObjectZone::Battlefield);
    assert_eq!(
        discoveries[1]
            .source
            .current_power_toughness
            .as_ref()
            .unwrap()
            .clone()
            .toughness,
        0.into()
    );
    assert_ne!(
        discoveries[0].source.object.id(),
        object(resolved.state(), 1)
    );
    assert_eq!(
        resolved
            .state()
            .execution()
            .window()
            .cards()
            .get(object(resolved.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    assert!(matches!(
        resolved.state().pending_decision().unwrap().frame(),
        RuleFrame::Triggers(_)
    ));
}

#[test]
fn targetless_draw_executes_without_a_target_and_completion_has_one_exit() {
    let initial = game(
        vec![
            seed(
                1,
                A,
                ObjectDestination::Hand,
                Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(BodyTarget::None, vec![draw()]),
                ),
            ),
            library(2, A),
        ],
        false,
    );
    let cast = submit(
        &initial,
        A,
        GameCommand::CastUntargeted {
            spell: object(&initial, 1),
            payment: None,
        },
    )
    .into_state();
    let resolved = resolve(cast);
    assert_eq!(resolved.draw_journal().events.len(), 1);
    let cuts = events(&resolved);
    assert_eq!(cuts.len(), 2);
    assert!(matches!(cuts[0].cause, EventCause::Draw { .. }));
    assert!(matches!(
        cuts[1].cause,
        EventCause::ResolutionCompleted {
            completion: EntryCompletion::Resolved,
            ..
        }
    ));
    assert_eq!(cuts[1].occurrences.len(), 1);
    let RulesOccurrence::ZoneChange(exit) = &cuts[1].occurrences[0].value else {
        panic!("actual spell exit")
    };
    assert_eq!(exit.reason, ZoneReason::ResolutionExit);
    assert_eq!(exit.before.object.zone(), ObjectZone::Stack);
    assert_eq!(exit.after.object.zone(), ObjectZone::Graveyard(A));
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr-20260925")
}
