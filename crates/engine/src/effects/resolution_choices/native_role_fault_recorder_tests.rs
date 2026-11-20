//! Recorder protocol tests; unit metadata below creates no qualification packet.
use super::*;
fn recorder(source: &GameState, after: bool) -> Recorder {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    // Explicit synthetic unit metadata, following the maintained Recorder tests.
    // Actual exports use current_capture::recorder's genuine input/publication guard.
    let unit_source = || SourceRef {
        path: "unit-recorder-metadata-not-qualification".into(),
        sha256: "0".repeat(64),
        bytes: 0,
    };
    let mut recorder = Recorder::new(
        &vocabulary,
        unit_source(),
        unit_source(),
        vec![],
        RegistryWitness::NotApplicable,
    )
    .unwrap();
    recorder
        .snapshot(
            "before",
            &["/observations/source".into()],
            Phase::BeforeCall,
            "GameState::qualification_with_related_player_cause_fault",
            SnapshotOwner::GameState(source),
            &AliasContext::default(),
        )
        .unwrap();
    if after {
        // Deliberate false successor metadata for a rejected-call counterexample.
        recorder
            .snapshot(
                "false-after",
                &[],
                Phase::AfterCall,
                "counterexample-only",
                SnapshotOwner::GameState(source),
                &AliasContext::default(),
            )
            .unwrap();
    }
    recorder
}
#[test]
fn related_player_fault_actual_setup_err_has_no_result_adoption_append_or_successor() {
    let source = world(vec![C, A, B], C, vec![]);
    let saved = source.clone();
    let fault = Fault::EmptyCauseSet;
    let result = source.qualification_with_related_player_cause_fault(fault);
    let replayed = source.qualification_with_related_player_cause_fault(fault);
    assert_eq!(result, Err(SetupError::WrongGate));
    assert_eq!(result, replayed);
    assert_eq!(source, saved);
    let project = |value: &Result<GameState, SetupError>| match value {
        Ok(state) => {
            Ok(json!({"tag":"Ok","state":projection::state(state,&AliasContext::default())?}))
        }
        Err(error) => Ok(projection::related_player_fault_setup_error(error)),
    };
    let call = projection::QualificationCall::GameRelatedPlayerCauseFault { fault };
    assert_eq!(call.owner_name(), "GameState");
    assert_eq!(call.result_owner_name(), "GameState");
    assert!(!call.is_pure());
    assert!(!call.is_initializer());
    assert!(!call.returns_rules_append());
    for (adopted, after, append) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let mut recorder = recorder(&source, after);
        let journal = RulesJournal::default();
        assert!(
            recorder
                .record_call(
                    CallPoint {
                        call_id: "fault",
                        before_point: Some("before"),
                        after_point: after.then_some("false-after"),
                        adopted
                    },
                    &call,
                    &result,
                    &replayed,
                    project,
                    if append {
                        Append::RulesJournal(&journal)
                    } else {
                        Append::NotApplicable
                    },
                    ReplayEvidence {
                        source_eq: Some(source == saved),
                        source_capture: SourceCapture::BeforeInvocation
                    },
                    &AliasContext::default(),
                    Some(source.execution())
                )
                .is_err()
        );
    }
    let mut recorder = recorder(&source, false);
    recorder
        .record_call(
            CallPoint {
                call_id: "fault",
                before_point: Some("before"),
                after_point: None,
                adopted: false,
            },
            &call,
            &result,
            &replayed,
            project,
            Append::NotApplicable,
            ReplayEvidence {
                source_eq: Some(source == saved),
                source_capture: SourceCapture::BeforeInvocation,
            },
            &AliasContext::default(),
            Some(source.execution()),
        )
        .unwrap();
    recorder
        .finish_coverage(
            vec!["before".into()],
            vec!["fault".into()],
            vec![ObservationJoin {
                pointer: "/observations/source".into(),
                points: vec!["before".into()],
            }],
            vec![],
        )
        .unwrap();
    let record = json!({"observations":{"source":projection::state(&source,&AliasContext::default()).unwrap()}});
    let witness = recorder.witness(&record).unwrap();
    let actual = &witness["calls"][0];
    assert_eq!(
        actual["result"],
        json!({"tag":"Err","error":{"domain":"RelatedPlayerFaultSetupError","variant":"WrongGate"}})
    );
    assert_eq!(
        actual["input"],
        json!({"tag":"GameRelatedPlayerCauseFault","fault":{"kind":"EmptyCauseSet"}})
    );
    assert_eq!(actual["after_point"], Value::Null);
    assert_eq!(actual["adopted"], false);
    assert_eq!(
        actual["append"],
        json!({"tag":"NoReturnedTransition","reason":"NativeError"})
    );
    assert_eq!(actual["replay"]["source_eq"], true);
    assert_eq!(actual["replay"]["full_result_eq"], true);
    assert_eq!(actual["replay"]["full_record_eq"], true);
    assert_eq!(source, saved);
}
