//! Finite predecessor byte compatibility through actual native cleanup/cost calls.
//! The genuine vocabulary is explicit; receipt labels and fixtures are synthetic.
use serde_json::{Value, json};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    checks::CheckDecision,
    combat::CombatRules,
    effects::{DecisionReply, SeedCard},
    game::{GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
    mana::ManaCost,
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{BodyTarget, ExecutableBody, Program},
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use spellwake_reference_tools::{
    creature_types,
    current_profile::{
        Append, CallPoint, CaptureScope, ObservationJoin, Phase, Recorder, RegistryWitness,
        ReplayEvidence, SnapshotOwner, SourceCapture, SourceRef,
    },
    packet::AliasContext,
    projection::{self, QualificationCall},
};
use std::{collections::BTreeSet, sync::Arc};
const A: SeatId = SeatId(41);
const B: SeatId = SeatId(3);

fn fixture(cleanup: bool) -> GameState {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "synthetic-compatibility".into(),
        vocabulary.rules_revision().into(),
        "synthetic-not-catalogue".into(),
        "predecessor-byte-control".into(),
        "mechanism-only".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let creature = || Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let cards = if cleanup {
        (1..=8)
            .map(|id| SeedCard {
                card: CardId(id),
                owner: A,
                destination: ObjectDestination::Hand,
                program: creature(),
            })
            .collect()
    } else {
        vec![
            SeedCard {
                card: CardId(1),
                owner: A,
                destination: ObjectDestination::Battlefield { controller: A },
                program: Program::artifact(
                    None,
                    vec![ActivatedAbility {
                        id: ActivatedAbilityId(7),
                        mana_cost: ManaCost::free(),
                        source_cost: SourceCost::Sacrifice,
                        body: ExecutableBody::new(
                            BodyTarget::SINGLE_CREATURE,
                            vec![spellwake_engine::program::Instruction::ZoneOp(Box::new(
                                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    spellwake_engine::toolbox::ZoneDest::Hand,
                                ),
                            ))],
                        ),
                    }],
                ),
            },
            SeedCard {
                card: CardId(2),
                owner: B,
                destination: ObjectDestination::Battlefield { controller: B },
                program: creature(),
            },
        ]
    };
    let seed = spellwake_engine::setup::GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        if cleanup {
            vec![]
        } else {
            vec![CardId(1), CardId(2)]
        },
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    if cleanup {
        GameState::initialize_scheduled(
            GameId(95120),
            context,
            seed,
            TurnStart {
                number: 2,
                step: Step::Cleanup,
            },
        )
    } else {
        GameState::initialize(GameId(95120), context, seed)
    }
    .unwrap()
    .into_state()
}
fn emitted(cleanup: bool, default: bool) -> Value {
    let state = fixture(cleanup);
    let choice = if cleanup {
        let pending = state.pending_decision().unwrap();
        let RuleFrame::Checks(CheckDecision::CleanupDiscard(frame)) = pending.frame() else {
            panic!("actual cleanup required")
        };
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::Discard(vec![frame.domain()[0]]),
        }
    } else {
        let cards = state.execution().window().cards();
        GameCommand::ActivateAbility {
            source: cards.current_identity(CardId(1)).unwrap(),
            ability: ActivatedAbilityId(7),
            target: TargetRef::Object(cards.current_identity(CardId(2)).unwrap()),
            payment: None,
        }
    };
    let input = GameInput {
        basis: state.basis().clone(),
        actor: A,
        command: choice,
    };
    let source = state.clone();
    let result = source.advance(input.clone());
    let repeated = source.advance(input.clone());
    assert_eq!(result, repeated);
    assert_eq!(source, state);
    let candidate = result.as_ref().unwrap();
    let vocabulary = source.execution().creature_vocabulary();
    let metadata = |path: &str| SourceRef {
        path: path.into(),
        sha256: "0".repeat(64),
        bytes: 0,
    };
    let mut recorder = if default {
        Recorder::new(
            vocabulary,
            metadata("synthetic-input-receipt"),
            metadata("synthetic-publication-receipt"),
            vec![],
            RegistryWitness::NotApplicable,
        )
    } else {
        Recorder::new_scoped(
            CaptureScope::Current10,
            vocabulary,
            metadata("synthetic-input-receipt"),
            metadata("synthetic-publication-receipt"),
            vec![],
            RegistryWitness::NotApplicable,
        )
    }
    .unwrap();
    let aliases = AliasContext::default();
    recorder
        .snapshot(
            "before",
            &["/before".into()],
            Phase::BeforeCall,
            "GameState::advance",
            SnapshotOwner::GameState(&source),
            &aliases,
        )
        .unwrap();
    recorder
        .snapshot(
            "after",
            &["/after".into()],
            Phase::AfterCall,
            "GameState::advance",
            SnapshotOwner::GameState(candidate.state()),
            &aliases,
        )
        .unwrap();
    recorder.record_call(CallPoint {call_id:"native",before_point:Some("before"),after_point:Some("after"),adopted:true},
        &QualificationCall::GameInput(&input),&result,&repeated,
        |result|Ok(match result {Ok(candidate)=>json!({"tag":"Ok","candidate":projection::candidate(candidate,Some(&source),&aliases)?}),Err(error)=>projection::game_error(error)}),
        Append::RulesJournal(candidate.rules_journal()),ReplayEvidence {source_eq:Some(source==state),source_capture:SourceCapture::BeforeInvocation},&aliases,Some(source.execution())).unwrap();
    recorder
        .finish_coverage(
            vec!["before".into(), "after".into()],
            vec!["native".into()],
            vec![
                ObservationJoin {
                    pointer: "/before".into(),
                    points: vec!["before".into()],
                },
                ObservationJoin {
                    pointer: "/after".into(),
                    points: vec!["after".into()],
                },
            ],
            vec![],
        )
        .unwrap();
    let mut emitted = json!({"schema_version":10,"before":projection::state(&source,&aliases).unwrap(),"after":projection::state(candidate.state(),&aliases).unwrap()});
    emitted["type_context_witness"] = recorder.witness(&emitted).unwrap();
    emitted
}

#[test]
fn old_default_current10_cleanup_and_sacrifice_cost_bytes_are_exact() {
    let values = [emitted(true, true), emitted(false, true)];
    let explicit = [emitted(true, false), emitted(false, false)];
    let bytes = serde_json::to_vec(&values).unwrap();
    assert_eq!(bytes, serde_json::to_vec(&explicit).unwrap());
    assert!(
        values
            .iter()
            .all(|value| value["type_context_witness"]["schema"]
                == "spellwake.current-type-context-witness.v8")
    );
    if let Some(path) = std::env::var_os("SPELLWAKE_APPLICABILITY_COMPATIBILITY_OUTPUT") {
        std::fs::write(path, bytes).unwrap();
    }
}
