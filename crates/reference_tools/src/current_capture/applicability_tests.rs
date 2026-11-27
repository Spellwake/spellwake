//! Native calls still run when metadata denies the predecessor epoch.
use super::super::{
    current_profile::applicability::{ApplicabilityError, tests as fixture},
    native_calls,
};
use super::*;
use serde_json::json;
use spellwake_engine::{
    game::RuleFrame,
    object_identity::ObjectDestination,
    program::{BodyTarget, ExecutableBody, TriggerGrouping, TriggeredAbility, TriggeredAbilityId},
};

pub(crate) struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.borrow_mut().take();
        });
    }
}
pub(crate) fn start() {
    let recorder = fixture::recorder(CaptureScope::Current10);
    ACTIVE.with(|active| {
        assert!(active.borrow().is_none());
        *active.borrow_mut() = Some(ActiveCapture {
            identity: capture_identity().unwrap(),
            profile: "applicability-control".into(),
            recorder,
            points: vec![],
            calls: vec![],
            observations: BTreeMap::new(),
            record_pointers: vec![],
            sequence: 0,
            failure: None,
        });
    });
}
pub(crate) fn denied() {
    assert_eq!(
        finish_record("applicability-control", json!({})).unwrap_err(),
        ApplicabilityError.to_string()
    );
}

#[test]
fn applicability_effect_initializer_executes_and_preserves_ok_and_original_err_after_denial() {
    let _reset = Reset;
    for invalid in [false, true] {
        let (_, mut seed) = fixture::seed(fixture::selection_program(false));
        if invalid {
            seed.seats.clear();
        }
        let expected = spellwake_engine::effects::EffectWindow::new(seed.clone());
        assert_eq!(expected.is_err(), invalid);
        start();
        let (result, point) = native_calls::effect_initialize_recorded(seed, true);
        assert_eq!(result, expected);
        assert!(
            point.is_some(),
            "allocated evidence label carries no authority after denial"
        );
        denied();
    }
}

#[test]
fn applicability_root_initializer_and_advance_keep_exact_native_results() {
    let _reset = Reset;
    let (context, seed) = fixture::seed(fixture::selection_program(false));
    let expected = GameState::initialize(GameId(95111), context.clone(), seed.clone());
    start();
    assert_eq!(initialize(GameId(95111), context, seed), expected);
    denied();
    let source = fixture::pending();
    let retained = source.clone();
    let pending = source.pending_decision().unwrap();
    let RuleFrame::ResolutionSelect(frame) = pending.frame() else {
        unreachable!()
    };
    let input = GameInput {
        basis: source.basis().clone(),
        actor: pending.chooser(),
        command: spellwake_engine::game::GameCommand::Reply {
            decision: pending.id(),
            choice: spellwake_engine::effects::DecisionReply::SelectResolution(vec![
                frame.domain()[0],
            ]),
        },
    };
    let expected = source.advance(input.clone());
    start();
    assert_eq!(advance(&source, input), expected);
    assert_eq!(source, retained);
    denied();
}

fn token_objects() -> (GameObjects, ObjectId) {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let mut token = fixture::creature();
    token.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::enters(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![fixture::selection(false)]),
    });
    let token = spellwake_engine::token::TokenDefinition::new(
        token,
        spellwake_engine::token::TokenName::Default {
            ordered_subtypes: vec![],
        },
        &vocabulary,
    )
    .unwrap()
    .materialize(&vocabulary)
    .unwrap();
    let inserted = GameObjects::new(0)
        .insert_token(0, fixture::A, token, &vocabulary)
        .unwrap();
    (inserted.next, inserted.after.id())
}

#[test]
fn applicability_control_start_and_controller_adapters_preserve_native_execution_after_source_denial()
 {
    let _reset = Reset;
    let (objects, object) = token_objects();
    let retained = objects.clone();
    let seats = [fixture::A, fixture::B];
    start();
    assert_eq!(
        native_calls::card_turn_start(&objects, objects.revision(), fixture::A, &seats),
        objects.record_turn_start(objects.revision(), fixture::A)
    );
    denied();
    for revision in [objects.revision(), objects.revision() + 1] {
        start();
        assert_eq!(
            native_calls::card_controller(&objects, revision, object, fixture::B, &seats),
            objects.apply_controller(revision, object, fixture::B)
        );
        denied();
    }
    assert_eq!(objects, retained);
}

#[test]
fn applicability_generic_card_adapter_does_not_skip_native_call_on_denied_source() {
    let _reset = Reset;
    let (objects, object) = token_objects();
    start();
    assert_eq!(
        card_move(
            &objects,
            objects.revision(),
            object,
            ObjectDestination::Graveyard,
            &[fixture::A, fixture::B]
        ),
        objects.apply_zone_move(objects.revision(), object, ObjectDestination::Graveyard)
    );
    denied();
}

#[test]
fn applicability_first_denial_acknowledges_metadata_labels_but_cannot_seal_an_old_prefix() {
    let _reset = Reset;
    start();
    let objects = GameObjects::new(0);
    native_snapshot(
        Phase::BeforeCall,
        "old-prefix",
        SnapshotOwner::GameObjects {
            cards: &objects,
            participants: &[fixture::A, fixture::B],
        },
        &AliasContext::default(),
    )
    .unwrap();
    let program = fixture::selection_program(false);
    let denied_label = native_snapshot(
        Phase::PureInput,
        "denied-before-native",
        SnapshotOwner::ProgramInput {
            program: &program,
            source_ref: "actual-native-control",
        },
        &AliasContext::default(),
    )
    .unwrap();
    bind_observation(&denied_label, "/ignored").unwrap();
    reference_record_point(&denied_label, "/ignored").unwrap();
    snapshot_at(
        "also-denied",
        &[],
        Phase::Observation,
        "denied",
        SnapshotOwner::ProgramInput {
            program: &program,
            source_ref: "actual-native-control",
        },
        &AliasContext::default(),
    )
    .unwrap();
    ACTIVE.with(|active| {
        let active = active.borrow();
        let active = active.as_ref().unwrap();
        assert_eq!(active.points.len(), 1);
        assert!(active.calls.is_empty());
        assert!(active.recorder.applicability_rejected());
    });
    denied();
}

#[test]
fn applicability_poison_preserves_the_first_unrelated_metadata_failure() {
    let _reset = Reset;
    start();
    assert!(bind_observation("missing", "/unused").is_err());
    remember(Err("first unrelated metadata failure".into()));
    let (_, seed) = fixture::seed(fixture::selection_program(false));
    let direct = spellwake_engine::effects::EffectWindow::new(seed.clone());
    assert_eq!(native_calls::effect_initialize(seed), direct);
    assert_eq!(
        finish_record("applicability-control", json!({})).unwrap_err(),
        "first unrelated metadata failure"
    );
}

#[test]
fn library_profile_denial_preserves_actual_initializer_cast_and_completed_native_results() {
    use spellwake_engine::{effects::Instruction, mana::ManaCost, program::ResolvingPlayer};
    let _reset = Reset;
    for instruction in [
        Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::scry(
            ResolvingPlayer::Controller,
            spellwake_engine::program::ValueExpr::Literal(1.into()),
        ))),
        Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::surveil(
                ResolvingPlayer::Controller,
                spellwake_engine::program::ValueExpr::Literal(1.into()),
            ),
        )),
        Instruction::MutatePlayer(Box::new(spellwake_engine::toolbox::MutatePlayerSpec::mill(
            ResolvingPlayer::Controller,
            spellwake_engine::program::ValueExpr::Literal(1.into()),
        ))),
    ] {
        let program = spellwake_engine::program::Program::instant(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::None, vec![instruction]),
        );
        let (context, seed) = fixture::seed(program);
        let expected = GameState::initialize(GameId(95331), context.clone(), seed.clone());
        start();
        assert_eq!(initialize(GameId(95331), context, seed), expected);
        denied();
        let mut state = expected.unwrap().into_state();
        let spell = state
            .execution()
            .window()
            .cards()
            .current_identity(spellwake_engine::object_identity::CardId(1))
            .unwrap();
        let input = GameInput {
            basis: state.basis().clone(),
            actor: fixture::A,
            command: spellwake_engine::game::GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
        };
        let expected = state.advance(input.clone());
        start();
        assert_eq!(advance(&state, input), expected);
        denied();
        state = expected.unwrap().into_state();
        for _ in 0..2 {
            let spellwake_engine::stack_window::WindowPhase::Priority(
                spellwake_engine::priority::PriorityPhase::Ready { holder },
            ) = state.execution().window().phase()
            else {
                panic!("actual priority")
            };
            let input = GameInput {
                basis: state.basis().clone(),
                actor: holder,
                command: spellwake_engine::game::GameCommand::Pass,
            };
            let expected = state.advance(input.clone());
            start();
            assert_eq!(advance(&state, input), expected);
            denied();
            state = expected.unwrap().into_state();
        }
        assert!(state.pending_decision().is_none());
        let retained = state.clone();
        let mut basis = state.basis().clone();
        basis.revision += 1;
        let input = GameInput {
            basis,
            actor: fixture::A,
            command: spellwake_engine::game::GameCommand::Pass,
        };
        let expected = state.advance(input.clone());
        assert!(expected.is_err());
        start();
        assert_eq!(advance(&state, input), expected);
        denied();
        assert_eq!(state, retained);
    }
}
