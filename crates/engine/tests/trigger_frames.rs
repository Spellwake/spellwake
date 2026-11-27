//! Prepared lethal setup reaches real root checks and issued ordinary trigger frames.
//! This is a mechanism fixture, not a cast/opening or complete-card trajectory.
use spellwake_engine::{
    checks::CheckBoundary,
    combat::CombatRules,
    effects::{EffectDecision, EffectError, SeedCard},
    game::{
        DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput,
        GameState, NativeCandidate, RuleFrame,
    },
    mana::ManaType,
    object_identity::{CardId, ObjectDestination},
    occurrence::{RulesRecord, TriggerError},
    priority::{PriorityPhase, SeatId},
    program::{BodyTarget, ExecutableBody, Program, TriggeredAbility, TriggeredAbilityId},
    setup::GameSeed,
    stack_window::WindowPhase,
};
use std::collections::BTreeSet;

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn issued() -> NativeCandidate {
    let mut observer = Program::artifact(None, vec![]);
    observer.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::dies(
            spellwake_engine::program::ObjectFilter::creature(),
        ),
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![spellwake_engine::program::Instruction::DrawCards(Box::new(
                spellwake_engine::toolbox::DrawCardsSpec::simple(
                    spellwake_engine::toolbox::PlayerSelector::Resolving(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                ),
            ))],
        ),
    });
    GameState::initialize(
        GameId(787),
        GameContext::new(
            "trigger-frame-tests".into(),
            "cr-20260925".into(),
            "literal-fixtures".into(),
            "ordinary-trigger-frame".into(),
            "unrestricted-ffa".into(),
            test_vocabulary().id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap(),
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![
                SeedCard {
                    card: CardId(10),
                    owner: A,
                    destination: ObjectDestination::Battlefield { controller: A },
                    program: observer,
                },
                SeedCard {
                    card: CardId(20),
                    owner: A,
                    destination: ObjectDestination::Battlefield { controller: A },
                    program: Program::creature(
                        None,
                        0.into(),
                        0.into(),
                        std::collections::BTreeSet::new(),
                    ),
                },
                SeedCard {
                    card: CardId(30),
                    owner: B,
                    destination: ObjectDestination::Battlefield { controller: B },
                    program: Program::creature(
                        None,
                        0.into(),
                        0.into(),
                        std::collections::BTreeSet::new(),
                    ),
                },
            ],
            vec![CardId(10), CardId(20), CardId(30)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ),
    )
    .unwrap()
}

fn reject(
    state: &GameState,
    actor: SeatId,
    decision: DecisionId,
    choice: DecisionReply,
    error: GameError,
) {
    let saved = state.clone();
    assert_eq!(
        state.advance(GameInput {
            basis: state.basis().clone(),
            actor,
            command: GameCommand::Reply { decision, choice }
        }),
        Err(error)
    );
    assert_eq!(state, &saved);
}

#[test]
fn root_exposes_the_exact_issued_trigger_frame_and_owned_capture() {
    let candidate = issued();
    let state = candidate.state();
    let pending = state.pending_decision().unwrap();
    let RuleFrame::Triggers(frame) = pending.frame() else {
        panic!("real ordering frame required")
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(pending.chooser(), A);
    assert_eq!(frame.revision(), state.execution().revision());
    assert_eq!(frame.boundary(), CheckBoundary::Priority);
    assert!(frame.event().started_revision() <= frame.revision());
    assert!(frame.wave().sequence() > 0);
    assert_eq!(frame.instances().len(), 2);
    assert_eq!(frame.instances().iter().collect::<BTreeSet<_>>().len(), 2);
    assert_eq!(
        RuleFrame::from(EffectDecision::Triggers(frame.clone())),
        *pending.frame()
    );
    let discoveries = candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::TriggerDiscovered(value) => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        discoveries.iter().map(|row| row.id).collect::<Vec<_>>(),
        frame.instances()
    );
    let observer = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(10))
        .unwrap();
    assert!(
        discoveries
            .iter()
            .all(|row| row.source.object.id() == observer && row.controller == A)
    );
    assert_ne!(
        discoveries[0].causes.members()[0].occurrence,
        discoveries[1].causes.members()[0].occurrence
    );
    assert!(state.execution().window().entries().is_empty());
}

#[test]
fn root_reply_guards_preserve_issued_state_and_valid_order_keeps_exact_entry_associations() {
    let candidate = issued();
    let state = candidate.state();
    let pending = state.pending_decision().unwrap();
    let RuleFrame::Triggers(frame) = pending.frame() else {
        panic!()
    };
    let ids = frame.instances().to_vec();
    let mut stale_basis = state.basis().clone();
    stale_basis.revision += 1;
    let saved = state.clone();
    assert_eq!(
        state.advance(GameInput {
            basis: stale_basis,
            actor: A,
            command: GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::OrderTriggers(ids.clone()),
            },
        }),
        Err(GameError::StaleRevision)
    );
    assert_eq!(state, &saved);
    reject(
        state,
        B,
        pending.id(),
        DecisionReply::OrderTriggers(ids.clone()),
        GameError::WrongChooser,
    );
    reject(
        state,
        A,
        DecisionId(pending.id().0 + 1),
        DecisionReply::OrderTriggers(ids.clone()),
        GameError::WrongDecision,
    );
    reject(
        state,
        A,
        pending.id(),
        DecisionReply::ManaColor(ManaType::Blue),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    for invalid in [vec![], vec![ids[0]], vec![ids[0], ids[0]]] {
        reject(
            state,
            A,
            pending.id(),
            DecisionReply::OrderTriggers(invalid),
            GameError::Effect(EffectError::Trigger(TriggerError::InvalidTriggerOrder)),
        );
    }
    let selected = vec![ids[1], ids[0]];
    let accepted = state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor: A,
            command: GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::OrderTriggers(selected.clone()),
            },
        })
        .unwrap();
    let placements = accepted
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::TriggerPlaced(value) => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        placements
            .iter()
            .map(|row| row.instance)
            .collect::<Vec<_>>(),
        selected
    );
    assert_eq!(
        placements.iter().map(|row| row.entry).collect::<Vec<_>>(),
        accepted
            .state()
            .execution()
            .window()
            .entries()
            .iter()
            .map(|row| row.id())
            .collect::<Vec<_>>()
    );
    assert!(
        accepted
            .rules_journal()
            .records
            .iter()
            .all(|row| !matches!(row, RulesRecord::TriggerDiscovered(_)))
    );
    assert!(accepted.state().pending_decision().is_none());
    assert_eq!(
        accepted.state().execution().window().phase(),
        WindowPhase::Priority(PriorityPhase::Ready { holder: A })
    );
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr-20260925")
}
