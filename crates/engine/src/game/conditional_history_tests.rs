//! Real recorder/candidate propagation through a choice and rejected reply.
//! Synthetic provenance labels below are unit inputs, never source qualification.
use super::*;
use crate::{
    combat::CombatRules,
    effects::SeedCard,
    mana::ManaCost,
    object_identity::{CardId, ObjectDestination},
    program::{Comparison, CreatureTypeSlot, Instruction, Program, ResolutionCondition},
    test_support::{
        creature_types,
        current_profile::{
            Append, CallPoint, ObservationJoin, Phase, Recorder, RegistryWitness, ReplayEvidence,
            SnapshotOwner, SourceCapture, SourceRef,
        },
        packet::AliasContext,
        projection,
    },
};
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn before_resolution() -> GameState {
    let gain = |n: i64| {
        crate::program::Instruction::AdjustLife(Box::new(crate::toolbox::AdjustLifeSpec::gain(
            crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            crate::program::ValueExpr::Literal(n.into()),
        )))
    };
    let ops = vec![
        Instruction::BranchCondition {
            condition: Box::new(crate::toolbox::ConditionExpr::Resolution(Box::new(
                ResolutionCondition {
                    left: crate::program::ValueExpr::PlayerStat(
                        crate::program::PlayerStatExpr::ControllerLife,
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: crate::program::ValueExpr::Literal(20.into()),
                },
            ))),
            destination: 3,
        },
        Instruction::PromptChoice(Box::new(crate::toolbox::PromptChoiceSpec {
            chooser: crate::toolbox::PlayerSelector::Resolving(
                crate::program::ResolvingPlayer::Controller,
            ),
            domain: crate::toolbox::PromptDomain::Subtype {
                kind: crate::toolbox::SubtypeChoiceKind::Creature,
                excluding: BTreeSet::new(),
                legacy_slot: Some(CreatureTypeSlot::new(0)),
            },
            category: crate::toolbox::ChoiceCategory::Generic,
            bind_to: crate::toolbox::BindingId(0),
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: false,
            option_branches: Vec::new(),
        })),
        Instruction::Jump { destination: 4 },
        gain(99),
        gain(1),
    ];
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        vec![SeedCard {
            card: CardId(1),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(ManaCost::free(), ExecutableBody::new(BodyTarget::None, ops)),
        }],
        vec![],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        creature_types::selected_vocabulary().unwrap(),
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let context = GameContext::new(
        "conditional-history-unit".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "literal partial".into(),
        "unit".into(),
        "unit".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    let mut game = GameState::initialize_scheduled(
        GameId(6052),
        context,
        seed,
        TurnStart {
            number: 3,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let source = game
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    for (actor, command) in [
        (
            A,
            GameCommand::CastUntargeted {
                spell: source,
                payment: None,
            },
        ),
        (A, GameCommand::Pass),
    ] {
        game = game
            .advance(GameInput {
                basis: game.basis().clone(),
                actor,
                command,
            })
            .unwrap()
            .into_state();
    }
    game
}
#[test]
fn actual_recorder_retains_control_facts_before_pause_after_reply_and_rejected_call() {
    let source = before_resolution();
    let saved = source.clone();
    let aliases = AliasContext::default();
    let input = GameInput {
        basis: source.basis().clone(),
        actor: B,
        command: GameCommand::Pass,
    };
    let result = source.advance(input.clone());
    let repeated = source.advance(input.clone());
    assert_eq!(source, saved);
    assert_eq!(result, repeated);
    let pending = result.as_ref().unwrap();
    assert_eq!(pending.state().execution().instruction_index(), Some(1));
    let provenance = SourceRef {
        path: "synthetic-unit-source".into(),
        sha256: "0".repeat(64),
        bytes: 1,
    };
    let mut recorder = Recorder::new(
        source.execution().creature_vocabulary(),
        provenance.clone(),
        provenance,
        vec![],
        RegistryWitness::NotApplicable,
    )
    .unwrap();
    let mut observations = serde_json::Map::new();
    for (id, phase, state) in [
        ("before", Phase::BeforeCall, &source),
        ("pending", Phase::AfterCall, pending.state()),
        ("reply_before", Phase::BeforeCall, pending.state()),
    ] {
        observations.insert(id.into(), projection::state(state, &aliases).unwrap());
        recorder
            .snapshot(
                id,
                &[format!("/observations/{id}")],
                phase,
                "actual native root",
                SnapshotOwner::GameState(state),
                &aliases,
            )
            .unwrap();
    }
    recorder
        .record_call(
            CallPoint {
                call_id: "resolve",
                before_point: Some("before"),
                after_point: Some("pending"),
                adopted: true,
            },
            &projection::QualificationCall::GameInput(&input),
            &result,
            &repeated,
            |r| {
                r.as_ref()
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|c| projection::candidate(c, Some(&source), &aliases))
            },
            Append::RulesJournal(pending.rules_journal()),
            ReplayEvidence {
                source_eq: Some(source == saved),
                source_capture: SourceCapture::BeforeInvocation,
            },
            &aliases,
            Some(source.execution()),
        )
        .unwrap();
    let human = source
        .execution()
        .creature_vocabulary()
        .resolve_label("Human")
        .unwrap();
    let decision = pending.state().pending_decision().unwrap().id();
    let bad = GameInput {
        basis: pending.state().basis().clone(),
        actor: B,
        command: GameCommand::Reply {
            decision,
            choice: DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(
                human,
            )),
        },
    };
    let unchanged = pending.state().clone();
    let rejected = pending.state().advance(bad.clone());
    let retry = pending.state().advance(bad.clone());
    assert!(rejected.is_err());
    assert_eq!(pending.state(), &unchanged);
    recorder
        .record_call(
            CallPoint {
                call_id: "reject",
                before_point: Some("reply_before"),
                after_point: None,
                adopted: false,
            },
            &projection::QualificationCall::GameInput(&bad),
            &rejected,
            &retry,
            |r| {
                Ok(match r {
                    Err(e) => projection::game_error(e),
                    Ok(_) => panic!("expected actual rejection"),
                })
            },
            Append::NotApplicable,
            ReplayEvidence {
                source_eq: Some(pending.state() == &unchanged),
                source_capture: SourceCapture::BeforeInvocation,
            },
            &aliases,
            Some(pending.state().execution()),
        )
        .unwrap();
    let reply = GameInput {
        basis: pending.state().basis().clone(),
        actor: A,
        command: GameCommand::Reply {
            decision,
            choice: DecisionReply::PromptChoice(crate::toolbox::PromptChoiceReply::CreatureType(
                human,
            )),
        },
    };
    let completed = pending.state().advance(reply.clone());
    let replayed = pending.state().advance(reply.clone());
    let done = completed.as_ref().unwrap();
    assert_eq!(done.state().execution().life_total(A), Some(&21.into()));
    observations.insert(
        "done".into(),
        projection::state(done.state(), &aliases).unwrap(),
    );
    recorder
        .snapshot(
            "done",
            &["/observations/done".into()],
            Phase::AfterCall,
            "actual accepted root",
            SnapshotOwner::GameState(done.state()),
            &aliases,
        )
        .unwrap();
    recorder
        .record_call(
            CallPoint {
                call_id: "reply",
                before_point: Some("reply_before"),
                after_point: Some("done"),
                adopted: true,
            },
            &projection::QualificationCall::GameInput(&reply),
            &completed,
            &replayed,
            |r| {
                r.as_ref()
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|c| projection::candidate(c, Some(pending.state()), &aliases))
            },
            Append::RulesJournal(done.rules_journal()),
            ReplayEvidence {
                source_eq: Some(pending.state() == &unchanged),
                source_capture: SourceCapture::BeforeInvocation,
            },
            &aliases,
            Some(pending.state().execution()),
        )
        .unwrap();
    let ids = ["before", "pending", "reply_before", "done"];
    recorder
        .finish_coverage(
            ids.iter().map(|s| (*s).into()).collect(),
            vec!["resolve".into(), "reject".into(), "reply".into()],
            ids.iter()
                .map(|s| ObservationJoin {
                    pointer: format!("/observations/{s}"),
                    points: vec![(*s).into()],
                })
                .collect(),
            vec![],
        )
        .unwrap();
    let witness = recorder
        .witness(&json!({"observations":observations}))
        .unwrap();
    assert_eq!(
        witness["schema"],
        "spellwake.current-type-context-witness.v8"
    );
    let calls = witness["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0]["append"]["value"][0]["tag"], "InstructionBranch");
    assert_eq!(calls[0]["append"]["value"][0]["instruction"], "0");
    assert_eq!(calls[0]["result"]["append"], calls[0]["append"]["value"]);
    assert_eq!(calls[1]["append"]["tag"], "NoReturnedTransition");
    assert_eq!(calls[1]["adopted"], false);
    assert!(
        calls[2]["append"]["value"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["tag"] == "InstructionJump"
                && row["instruction"] == "2"
                && row["next"] == "4")
    );
    assert_eq!(calls[2]["result"]["append"], calls[2]["append"]["value"]);
}
