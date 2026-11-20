//! Genuine manual shared-policy qualification. No ordinary Report5 runner or projection.
use super::*;
use crate::ordinary_game::{fixture::Fixture, policy::Policy};
use spellwake_engine::{
    game::{
        DecisionReply, GameCommand, GameError, GameInput, GameState, NativeCandidate, RuleFrame,
    },
    object_identity::ObjectDestination,
};
use std::{path::PathBuf, sync::OnceLock};
fn required(key: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit genuine source input required: {key}"))
}
pub(crate) fn genuine() -> &'static SelectionSource {
    static SOURCE: OnceLock<SelectionSource> = OnceLock::new();
    SOURCE.get_or_init(|| {
        let vocabulary = crate::creature_vocabulary::load_vocabulary(&required(
            "SPELLWAKE_CREATURE_VOCABULARY_INPUT",
        ))
        .unwrap();
        SelectionSource::load(
            &required("SPELLWAKE_CATALOGUE_METADATA"),
            &required("SPELLWAKE_CATALOGUE_INPUT"),
            vocabulary,
        )
        .unwrap()
    })
}
#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub source: GameState,
    pub input: GameInput,
    pub result: Result<NativeCandidate, GameError>,
    pub installed: bool,
}
pub(crate) struct Run {
    pub fixture: Fixture,
    pub initialized: NativeCandidate,
    pub state: GameState,
    pub policy: Policy,
    pub calls: Vec<Call>,
    pub binding: SelectionBindingEvidence,
}
pub(crate) fn begin(case: SelectionCase) -> Run {
    let source = genuine();
    let fixture = source.fixture(case, source.vocabulary().clone()).unwrap();
    let initialized = authority::initialize(&fixture).unwrap();
    let state = initialized.state().clone();
    let mut policy = Policy::new(&fixture).unwrap();
    let binding = policy
        .bind_selection_initializer(source, &state, &initialized)
        .unwrap();
    assert_eq!(binding.validation.as_ref().unwrap(), &initialized);
    Run {
        fixture,
        initialized,
        state,
        policy,
        calls: vec![],
        binding,
    }
}
pub(crate) fn accept_next(run: &mut Run, exercise_drop: bool) -> bool {
    let Some(prepared) = run.policy.prepare(&run.state).unwrap() else {
        return false;
    };
    let retained_prepared = prepared.selection_metadata().map(|metadata| {
        let work = run.state.execution().resolution_selection_state().unwrap();
        assert_eq!(
            &metadata.reached.decision,
            run.state.pending_decision().unwrap()
        );
        assert_eq!(metadata.reached.work.entry, work.cursor.entry());
        assert_eq!(metadata.reached.work.instruction, work.cursor.instruction());
        assert_eq!(&metadata.reached.work.locals, work.cursor.locals());
        assert_eq!(&metadata.reached.work.operation, work.operation);
        assert_eq!(&metadata.reached.work.stage, work.stage);
        assert!(
            metadata
                .receipts
                .iter()
                .all(|r| r.reached == *metadata.reached)
        );
        assert!(
            metadata
                .pending
                .iter()
                .all(|r| r.reached == *metadata.reached)
        );
        assert_eq!(
            metadata
                .receipts
                .iter()
                .map(|r| &r.probe)
                .collect::<Vec<_>>(),
            prepared.recipe_probes().iter().collect::<Vec<_>>()
        );
        (
            metadata.reached.clone(),
            metadata.receipts.to_vec(),
            metadata.pending.to_vec(),
        )
    });
    let before = run.policy.selection_observation().unwrap();
    let old_count = run.policy.consumed();
    let probes = prepared.recipe_probes().to_vec();
    for probe in probes {
        let source = run.state.clone();
        let result = run.state.advance(probe.input.clone());
        assert_eq!(
            result.as_ref().unwrap_err(),
            &probe.expected,
            "{} at {}",
            probe.id,
            run.fixture.case.as_str()
        );
        assert_eq!(run.state, source);
        assert_eq!(run.policy.selection_observation().unwrap(), before);
        run.calls.push(Call {
            source,
            input: probe.input,
            result,
            installed: false,
        });
    }
    let input = run.policy.issue(&run.state, prepared).unwrap();
    let source = run.state.clone();
    let mut result = run.state.advance(input.clone());
    let candidate = result.as_ref().unwrap_or_else(|e| {
        panic!(
            "{} shared policy input {input:?}: {e:?}",
            run.fixture.case.as_str()
        )
    });
    if exercise_drop {
        let prepared_acceptance = run.policy.prepare_accept(&input, candidate).unwrap();
        drop(prepared_acceptance);
        assert_eq!(run.policy.selection_observation().unwrap(), before);
        assert_eq!(run.policy.consumed(), old_count);
        assert_eq!(run.state, source);
        run.calls.push(Call {
            source: source.clone(),
            input: input.clone(),
            result: result.clone(),
            installed: false,
        });
        let actual_second_call = run.state.advance(input.clone());
        assert_eq!(actual_second_call, result);
        result = actual_second_call;
    }
    let candidate = result.as_ref().unwrap();
    let acceptance = run.policy.prepare_accept(&input, candidate).unwrap();
    run.state = candidate.clone().into_state();
    acceptance.commit();
    assert_eq!(run.policy.consumed(), old_count + 1);
    if let Some((reached, receipts, pending)) = retained_prepared {
        let observed = run.policy.selection_observation().unwrap();
        let consumed = &observed.consumed[&reached.plan_id];
        assert_eq!(consumed.reached, reached);
        assert_eq!(
            consumed.accepted.ordinal,
            u64::try_from(old_count + 1).unwrap()
        );
        assert_eq!(consumed.accepted.input, input);
        assert_eq!(&consumed.accepted.after_basis, run.state.basis());
        assert!(observed.receipts.ends_with(&receipts));
        assert!(observed.pending.ends_with(&pending));
    }

    run.calls.push(Call {
        source,
        input,
        result,
        installed: true,
    });
    true
}
pub(crate) fn reached(case: SelectionCase, plan: &str) -> Run {
    let mut run = begin(case);
    for _ in 0..128 {
        let prepared = run
            .policy
            .prepare(&run.state)
            .unwrap()
            .expect("desired plan reached before stop");
        if prepared
            .policy_entry_ref
            .starts_with(&format!("{plan}.reply."))
        {
            return run;
        }
        assert!(accept_next(&mut run, false));
    }
    panic!("desired plan not reached")
}
fn replay(run: &Run) {
    assert_eq!(
        authority::initialize(&run.fixture),
        Ok(run.initialized.clone())
    );
    let mut state = run.initialized.state().clone();
    for call in &run.calls {
        assert_eq!(state, call.source);
        let actual = state.advance(call.input.clone());
        assert_eq!(actual, call.result);
        if call.installed {
            state = actual.unwrap().into_state();
        } else {
            assert_eq!(state, call.source);
        }
    }
    assert_eq!(state, run.state);
    if let Some(helper) = &run.binding.foreign_helper {
        let source = genuine();
        let fixture = source
            .fixture(SelectionCase::Sd11, source.vocabulary().clone())
            .unwrap();
        let regenerated = authority::initialize(&fixture);
        assert_eq!(regenerated, helper.initialization);
        assert_eq!(Some(&regenerated), helper.validation.as_ref());
        let mut state = helper.initialization.as_ref().unwrap().state().clone();
        assert_eq!(helper.calls.len(), 4);
        let binding = helper.stop_binding.as_ref().unwrap();
        assert_eq!(binding.case, SelectionCase::Sd11);
        assert!(!helper.consumed_plans.contains_key(&binding.plan_id));
        assert_eq!(
            helper.consumed_plans["primary.select.P1"].accepted.ordinal,
            4
        );

        for call in &helper.calls {
            assert_eq!(state, call.source);
            let actual = state.advance(call.input.clone());
            assert_eq!(actual, call.result);
            assert!(call.installed);
            state = actual.unwrap().into_state();
        }
        assert_eq!(helper.stop.as_ref(), Some(&state));
        let binding = helper.stop_binding.as_ref().unwrap();
        let work = state.execution().resolution_selection_state().unwrap();
        assert_eq!(&binding.decision, state.pending_decision().unwrap());
        assert_eq!(&binding.work.operation, work.operation);
        assert_eq!(&binding.work.stage, work.stage);
        assert_eq!(&binding.work.locals, work.cursor.locals());

        assert!(matches!(
            state.pending_decision().unwrap().frame(),
            RuleFrame::ResolutionGraveyardOrder(_)
        ));
    }
}
#[test]
#[ignore = "requires genuine full source/vocabulary and reviewed v3 probes; actual shared policy/native replay"]
fn all_twenty_selected_shared_policy_stops_and_twenty_six_probes_replay_complete_native_results() {
    let mut outputs = vec![];
    let mut probes = 0;
    let mut pending = 0;
    let mut plan_count = 0;
    let mut empty_journal_selection = 0;
    let mut zero_rows = 0;
    for case in SelectionCase::ALL {
        let mut run = begin(case);
        let mut iterations = 0;
        loop {
            if let Some(work) = run.state.execution().resolution_selection_state() {
                match work.stage {
                    spellwake_engine::effects::ResolutionSelectionStage::Choosing {
                        requirements,
                        next_player,
                        accepted,
                    } => {
                        assert_eq!(*next_player, accepted.len());
                        zero_rows += requirements.iter().filter(|r| r.required == 0).count();
                        if requirements.len() == 2 {
                            let mut seats = run.fixture.setup.seed.seats.clone();
                            let first = seats
                                .iter()
                                .position(|s| *s == run.state.execution().window().turn_owner())
                                .unwrap();
                            seats.rotate_left(first);
                            assert_eq!(
                                requirements.iter().map(|r| r.player).collect::<Vec<_>>(),
                                seats
                            );
                        }
                    }
                    spellwake_engine::effects::ResolutionSelectionStage::Ordering {
                        next_owner,
                        orders,
                        ..
                    } => {
                        assert_eq!(*next_owner, orders.len());
                    }
                    _ => {}
                }
            }
            if !accept_next(&mut run, true) {
                break;
            }
            iterations += 1;
            assert!(iterations <= 128, "{} watchdog", case.as_str());
        }
        run.policy
            .finish(&run.state)
            .unwrap_or_else(|e| panic!("{} finish: {e}", case.as_str()));
        let observed = run.policy.selection_observation().unwrap();
        probes += observed.receipts.len();
        pending += observed.pending.len();
        plan_count += observed.consumed.len();
        assert_eq!(
            observed.consumed.len(),
            recipes::recipe(case).unwrap().decision_plans.len()
        );
        for call in &run.calls {
            if call.installed
                && matches!(
                    call.input.command,
                    GameCommand::Reply {
                        choice: DecisionReply::SelectResolution(_),
                        ..
                    }
                )
                && call
                    .result
                    .as_ref()
                    .unwrap()
                    .rules_journal()
                    .records
                    .is_empty()
            {
                empty_journal_selection += 1;
            }
        }
        if matches!(
            case,
            SelectionCase::Sd04RealHistoryCompanion | SelectionCase::Sd14RealHistoryCompanion
        ) {
            assert!(run.binding.foreign_helper.is_none());
            assert!(observed.consumed.contains_key("history.select.P1"));
            assert!(observed.receipts.iter().all(|r| matches!(
                r.fulfillment,
                probes::FulfillmentDisposition::CompanionSeparateHistory {
                    original_retrocredit: false
                }
            )));
        }
        if case == SelectionCase::Sd01 {
            let object = run
                .fixture
                .current_object(&run.state, "P2-BF-BEARS-1")
                .unwrap();
            assert_eq!(
                run.state
                    .execution()
                    .window()
                    .cards()
                    .get(object)
                    .unwrap()
                    .zone(),
                spellwake_engine::object_identity::ObjectZone::Graveyard(
                    crate::ordinary_game::fixture::B
                )
            );
            assert_eq!(observed.consumed.len(), 1);
            assert!(
                run.calls
                    .iter()
                    .filter(|call| call.installed)
                    .all(|call| !matches!(
                        call.input.command,
                        GameCommand::Reply {
                            choice: DecisionReply::OrderResolutionGraveyard(_),
                            ..
                        }
                    ))
            );
        }
        if matches!(
            case,
            SelectionCase::Sd02 | SelectionCase::Sd05 | SelectionCase::Sd13
        ) {
            assert!(observed.consumed.is_empty());
            assert!(
                run.calls
                    .iter()
                    .filter(|call| call.installed)
                    .all(|call| !matches!(
                        call.input.command,
                        GameCommand::Reply {
                            choice: DecisionReply::SelectResolution(_)
                                | DecisionReply::OrderResolutionGraveyard(_),
                            ..
                        }
                    ))
            );
        }
        if case == SelectionCase::Sd06 {
            assert!(run.state.outcome().is_some());
        } else {
            assert!(run.state.outcome().is_none());
            assert!(run.policy.completed_boundary(&run.state).unwrap().is_some());
        }
        if case == SelectionCase::Sd13 {
            assert!(run.initialized.draw_journal().events.is_empty());
            assert!(
                run.calls
                    .iter()
                    .filter_map(|c| c.result.as_ref().ok())
                    .all(|c| c.draw_journal().events.is_empty())
            );
        }
        replay(&run);
        outputs.push(serde_json::json!({"case":case.as_str(),"accepted":run.policy.consumed(),"plans":observed.consumed.len(),"probes":observed.receipts.len(),"pending":observed.pending.len(),"complete_result_and_state_replay":true,"helper_calls":run.binding.foreign_helper.as_ref().map_or(0,|h|h.calls.len()),"actual_typed_call_diagnostics":format!("{:?}",run.calls),"actual_authority_annotations":format!("{observed:?}"),"actual_auxiliary_and_helper_evidence":format!("{:?}",run.binding),"scope":"manual shared Policy; no Report5/capture"}));
    }
    assert_eq!(outputs.len(), 20);
    assert_eq!(plan_count, 33);
    assert_eq!(probes, 26);
    assert_eq!(pending, 2);
    assert!(empty_journal_selection > 0);
    assert!(zero_rows > 0);

    if let Some(path) = std::env::var_os("SPELLWAKE_SELECTION_POLICY_OBSERVATIONS_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&outputs).unwrap()).unwrap();
    }
}
#[test]
#[ignore = "requires genuine source; complete same-basis initializer binding refusal controls"]
fn initializer_binding_checks_whole_fixture_seed_life_program_and_is_atomic() {
    let source = genuine();
    let fixture = source
        .fixture(SelectionCase::Sd01, source.vocabulary().clone())
        .unwrap();
    let genuine = authority::initialize(&fixture).unwrap();
    let mut policy = Policy::new(&fixture).unwrap();
    assert!(
        policy
            .prepare(genuine.state())
            .unwrap_err()
            .contains("initializer authority")
    );
    for mutation in 0..3 {
        let mut altered = fixture.clone();
        match mutation {
            0 => altered.setup.seed.life[0].total = 21.into(),
            1 => {
                let creature = altered
                    .setup
                    .seed
                    .cards
                    .iter()
                    .find(|r| {
                        r.program
                            .has_type(spellwake_engine::program::CardType::Creature)
                    })
                    .unwrap()
                    .program
                    .clone();
                altered.setup.seed.cards[0].program = creature;
            }
            2 => {
                let card = altered.card("CASE-SPELL").unwrap();
                altered
                    .setup
                    .seed
                    .cards
                    .iter_mut()
                    .find(|r| r.card == card)
                    .unwrap()
                    .destination = ObjectDestination::LibraryBottom;
            }
            _ => unreachable!(),
        }
        let alternative = authority::initialize(&altered).unwrap();
        assert_eq!(alternative.state().basis(), genuine.state().basis());
        assert_ne!(alternative, genuine);
        let error = policy
            .bind_selection_initializer(source, alternative.state(), &alternative)
            .unwrap_err();
        assert!(error.validation.as_ref().unwrap().is_ok());
        assert_eq!(policy.consumed(), 0);
        assert!(policy.selection_observation().unwrap().consumed.is_empty());
        assert!(
            policy
                .prepare(genuine.state())
                .unwrap_err()
                .contains("initializer authority")
        );
        let mut foreign_policy = Policy::new(&altered).unwrap();
        let error = foreign_policy
            .bind_selection_initializer(source, alternative.state(), &alternative)
            .unwrap_err();
        assert!(error.validation.is_none());
        assert!(error.message.contains("complete source-owned fixture"));
    }
    // A same-basis mismatch for a helper-requiring original is rejected before any helper is provisioned.
    let original = source
        .fixture(SelectionCase::Sd04, source.vocabulary().clone())
        .unwrap();
    let mut altered = original.clone();
    altered.setup.seed.life[0].total = 21.into();
    let altered_initializer = authority::initialize(&altered).unwrap();
    let mut original_policy = Policy::new(&original).unwrap();
    let error = original_policy
        .bind_selection_initializer(source, altered_initializer.state(), &altered_initializer)
        .unwrap_err();
    assert!(error.validation.as_ref().unwrap().is_ok());
    assert!(error.helper.is_none());
    assert!(
        original_policy
            .prepare(altered_initializer.state())
            .unwrap_err()
            .contains("initializer authority")
    );
    let evidence = policy
        .bind_selection_initializer(source, genuine.state(), &genuine)
        .unwrap();
    assert!(evidence.foreign_helper.is_none());
    let error = policy
        .bind_selection_initializer(source, genuine.state(), &genuine)
        .unwrap_err();
    assert!(error.validation.is_none() && error.helper.is_none());
    assert!(error.message.contains("already bound"));
    assert!(policy.prepare(genuine.state()).unwrap().is_some());
}
