//! Disjoint native-only cause-fault capture. No admitted malformed publication is claimed.
use super::*;
use crate::test_support::related_player_fault::{
    RelatedPlayerCauseFault as Fault, RelatedPlayerFaultSetupError as SetupError,
};

const PROFILE: &str = "optional-payment-native-role-fault";
const SCHEMA: &str = "spellwake.optional-payment-native-role-faults.v1";
const FAULTS: [Fault; 4] = [
    Fault::EmptyCauseSet,
    Fault::MissingRelatedPlayer,
    Fault::ForeignParticipant,
    Fault::AmbiguousParticipants,
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lane {
    Chooser,
    Payer,
}
impl Lane {
    fn name(self) -> &'static str {
        match self {
            Self::Chooser => "Chooser",
            Self::Payer => "Payer",
        }
    }
    fn body(self) -> Vec<Instruction> {
        match self {
            Self::Chooser => vec![
                gate_instruction(4),
                Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::UniqueRelatedPlayer,
                },
                gain(1),
                gain(2),
            ],
            Self::Payer => vec![
                gate_instruction(3),
                Instruction::OfferDynamicManaPayment {
                    payer: ResolvingPlayer::UniqueRelatedPlayer,
                    fixed: ManaCost::free(),
                    extra_generic: crate::program::ValueExpr::Literal(0.into()),
                    paid_destination: 2,
                    declined_destination: 2,
                },
                gain(2),
            ],
        }
    }
}
fn gate_instruction(end: usize) -> Instruction {
    Instruction::OfferDynamicManaPayment {
        payer: ResolvingPlayer::Controller,
        fixed: ManaCost::free(),
        extra_generic: crate::program::ValueExpr::Literal(0.into()),
        paid_destination: 1,
        declined_destination: end,
    }
}
fn scenario(lane: Lane, fault: Fault) -> String {
    format!(
        "OP-N10-{}-{}",
        lane.name().to_uppercase(),
        projection::related_player_cause_fault(fault)["kind"]
            .as_str()
            .unwrap()
            .to_uppercase()
    )
}
fn validated(program: &Program) {
    if capture::is_active() {
        validate_program(program).unwrap();
    } else {
        program
            .validate(&creature_types::selected_vocabulary().unwrap())
            .unwrap();
    }
}

/// Actual simultaneous token publication and retained trigger; no cause ID is fabricated.
fn grouped_gate(lane: Lane, controller: SeatId) -> GameState {
    use crate::occurrence::{TriggerCauseKey, TriggerRelated};
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let definition = TokenDefinition::new(
        creature(),
        TokenName::Specified(
            spellwake_contracts::names::EnglishCardName::new("Role Fault Native Token".into())
                .unwrap(),
        ),
        &vocabulary,
    )
    .unwrap();
    let mut creation = TokenCreation::one(definition);
    creation.amount = crate::program::ValueExpr::Literal(2.into());
    let creator = hand(
        1,
        A,
        BodyTarget::None,
        vec![Instruction::CreateTokens {
            creations: vec![creation],
        }],
    );
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: crate::program::TriggerSpec::enters(crate::program::ObjectFilter::creature()),
        grouping: TriggerGrouping::OneOrMoreByPlayer,
        body: ExecutableBody::new(BodyTarget::None, lane.body()),
    });
    validated(&creator.program);
    validated(&watcher);
    let state = world(
        vec![C, A, B],
        C,
        vec![
            creator,
            seed(
                20,
                controller,
                ObjectDestination::Battlefield { controller },
                watcher,
            ),
        ],
    );
    let watcher = object(&state, 20);
    let state = candidate(&state, C, GameCommand::Pass).into_state();
    let casted = cast(&state, A, 1, None);
    let timestamp = resolve_top(casted.state());
    let RuleFrame::TimestampOrder(frame) = timestamp.state().pending_decision().unwrap().frame()
    else {
        panic!("actual token timestamp child");
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.members().len(), 2);
    let order = frame
        .members()
        .iter()
        .rev()
        .map(|member| member.member)
        .collect();
    let created = reply(timestamp.state(), DecisionReply::TimestampOrder(order));
    let events = created
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::Event(event)
                if event
                    .occurrences
                    .iter()
                    .any(|row| matches!(row.value, RulesOccurrence::TokenCreated { .. })) =>
            {
                Some(event)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 1);
    let event = events[0];
    let published = event
        .occurrences
        .iter()
        .filter_map(|row| match &row.value {
            RulesOccurrence::TokenCreated { creator, after } => {
                assert_eq!(*creator, A);
                Some((row.id, after.as_ref()))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(published.len(), 2);
    let discovered = created
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::TriggerDiscovered(value) => Some(value.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(discovered.len(), 1);
    let actual = discovered[0];
    assert_eq!(actual.source.object.id(), watcher);
    assert_eq!(actual.controller, controller);
    assert_eq!(actual.causes.key(), TriggerCauseKey::Player(A));
    assert_eq!(actual.causes.event(), event.id);
    assert_eq!(actual.causes.members().len(), 2);
    for member in actual.causes.members() {
        assert_eq!(member.occurrence.event, event.id);
        assert_eq!(member.related_player, Some(A));
        let TriggerRelated::TokenCreated { creator, after } = &member.related else {
            panic!("real token cause");
        };
        assert_eq!(*creator, A);
        assert_eq!(after, published[&member.occurrence]);
    }
    assert_eq!(
        actual
            .causes
            .members()
            .iter()
            .map(|member| member.occurrence)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    assert_eq!(created.rules_journal().records.iter().filter(|row|matches!(row, RulesRecord::TriggerPlaced(value) if value.instance==actual.id && value.controller==controller)).count(),1);
    let state = resolve_top(created.state()).into_state();
    let RuleFrame::ResolutionPayment(frame) = state.pending_decision().unwrap().frame() else {
        panic!("actual outer Controller gate");
    };
    assert_eq!(frame.instruction(), 0);
    assert_eq!(frame.payer(), controller);
    assert_eq!(frame.cost(), &ManaCost::free());
    let binding = state
        .execution()
        .execution_bindings(frame.entry())
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.causes, actual.causes);
    assert_eq!(binding.instance, actual.id);
    state
}

struct SetupCapture {
    call: String,
    before: String,
    after: Option<String>,
}
/// Actual typed source/input/Result, recorded through the maintained generic owner.
fn setup(
    source: &GameState,
    fault: Fault,
) -> (Result<GameState, SetupError>, Option<SetupCapture>) {
    let saved = source.clone();
    let aliases = AliasContext::default();
    let coordinates = if capture::is_active() {
        Some((
            capture::native_call_id().unwrap(),
            capture::native_snapshot(
                Phase::BeforeCall,
                "GameState::qualification_with_related_player_cause_fault",
                SnapshotOwner::GameState(source),
                &aliases,
            )
            .unwrap(),
        ))
    } else {
        None
    };
    let result = source.qualification_with_related_player_cause_fault(fault);
    let replayed = source.qualification_with_related_player_cause_fault(fault);
    assert_eq!(&saved, source);
    assert_eq!(result, replayed);
    let coordinates = coordinates.map(|(call, before)| {
        let after = result.as_ref().ok().map(|state| {
            capture::native_snapshot(
                Phase::AfterCall,
                "GameState::qualification_with_related_player_cause_fault",
                SnapshotOwner::GameState(state),
                &aliases,
            )
            .unwrap()
        });
        capture::record_native(
            CallPoint {
                call_id: &call,
                before_point: Some(&before),
                after_point: after.as_deref(),
                adopted: false,
            },
            &projection::QualificationCall::GameRelatedPlayerCauseFault { fault },
            &result,
            &replayed,
            |value| match value {
                Ok(state) => Ok(json!({"tag":"Ok","state":projection::state(state,&aliases)?})),
                Err(error) => Ok(projection::related_player_fault_setup_error(error)),
            },
            Append::NotApplicable,
            ReplayEvidence {
                source_eq: Some(*source == saved),
                source_capture: SourceCapture::BeforeInvocation,
            },
            &aliases,
            Some(source.execution()),
        )
        .unwrap();
        SetupCapture {
            call,
            before,
            after,
        }
    });
    (result, coordinates)
}
fn observe(state: &GameState, pointer: &str) -> Option<String> {
    if !capture::is_active() {
        return None;
    }
    let point = capture::observation(
        "NativeOnly.RoleFaultActualWholeRoot",
        SnapshotOwner::GameState(state),
        &AliasContext::default(),
    )
    .unwrap();
    capture::bind_observation(&point, pointer).unwrap();
    Some(point)
}
fn payment_input(state: &GameState, choice: ResolutionPaymentReply) -> GameInput {
    GameInput {
        basis: state.basis().clone(),
        actor: B,
        command: command_reply(state, DecisionReply::ResolutionPayment(choice)),
    }
}
fn state_observation(point: String, state: &GameState) -> Value {
    json!({"point":point,"state":projection::state(state,&AliasContext::default()).unwrap()})
}
fn exercise(lane: Lane, fault: Fault) -> Option<Value> {
    let clean = grouped_gate(lane, B);
    let clean_point = observe(&clean, "/observations/clean_gate/state");
    let clean_pay = payment_input(&clean, ResolutionPaymentReply::Pay(ManaPayment::default()));
    let role = capture::advance(&clean, clean_pay.clone()).unwrap();
    assert_eq!(role.state().pending_decision().unwrap().chooser(), A);
    match (lane, role.state().pending_decision().unwrap().frame()) {
        (Lane::Chooser, RuleFrame::OptionalEffect(frame)) => {
            assert_eq!(frame.instruction(), 1);
            assert_eq!(frame.chooser(), A);
        }
        (Lane::Payer, RuleFrame::ResolutionPayment(frame)) => {
            assert_eq!(frame.instruction(), 1);
            assert_eq!(frame.payer(), A);
            assert_eq!(frame.cost(), &ManaCost::free());
        }
        _ => panic!("actual clean role kind"),
    }
    let role_point = observe(role.state(), "/observations/clean_branch/role_state/state");
    let completion = GameInput {
        basis: role.state().basis().clone(),
        actor: A,
        command: command_reply(
            role.state(),
            match lane {
                Lane::Chooser => DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
                Lane::Payer => DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(
                    ManaPayment::default(),
                )),
            },
        ),
    };
    let completed = capture::advance(role.state(), completion.clone()).unwrap();
    assert_eq!(
        completed.state().execution().life_total(B),
        Some(&match lane {
            Lane::Chooser => 23.into(),
            Lane::Payer => 22.into(),
        })
    );
    let clean_final_point = observe(
        completed.state(),
        "/observations/clean_branch/final_state/state",
    );
    let clean_saved = clean.clone();
    let (supplied, setup_capture) = setup(&clean, fault);
    let supplied = supplied.unwrap();
    assert_eq!(clean, clean_saved);
    assert_eq!(supplied.basis(), clean.basis());
    assert_eq!(supplied.pending_decision(), clean.pending_decision());
    // Both public aliases expose the very same single actual retained payload.
    let before = projection::state(&clean, &AliasContext::default()).unwrap();
    let after = projection::state(&supplied, &AliasContext::default()).unwrap();
    let frame = clean.pending_decision().unwrap();
    let RuleFrame::ResolutionPayment(frame) = frame.frame() else {
        unreachable!();
    };
    let old = clean
        .execution()
        .execution_bindings(frame.entry())
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    let new = supplied
        .execution()
        .execution_bindings(frame.entry())
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(old.causes.event(), new.causes.event());
    assert_eq!(old.causes.key(), new.causes.key());
    let original = old.causes.members();
    let changed = new.causes.members();
    match fault {
        Fault::EmptyCauseSet => assert!(changed.is_empty()),
        Fault::MissingRelatedPlayer => {
            let mut expected = original.to_vec();
            expected[0].related_player = None;
            assert_eq!(changed, expected);
        }
        Fault::ForeignParticipant => {
            assert_eq!(changed.len(), 2);
            let foreign = changed[0].related_player.unwrap();
            assert!(!clean.execution().window().participants().contains(&foreign));
            assert_eq!(
                foreign,
                SeatId(3),
                "actual fixed roster leaves least seat3 unused"
            );
            let mut expected = original.to_vec();
            for member in &mut expected {
                member.related_player = Some(foreign);
            }
            assert_eq!(changed, expected);
        }
        Fault::AmbiguousParticipants => {
            let mut expected = original.to_vec();
            expected[1].related_player = Some(B);
            assert_eq!(changed, expected);
        }
    }
    let aliases = before["stack"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, row)| row["entry"] == before["resolving"]["entry"])
        .collect::<Vec<_>>();
    assert_eq!(aliases.len(), 1);
    let stack_index = aliases[0].0;
    assert_eq!(aliases[0].1, &before["resolving"]);
    assert_eq!(after["stack"][stack_index], after["resolving"]);
    // Comparison-only canonical clone; never restored or supplied to native execution.
    let mut comparable = after.clone();
    for pointer in [
        "/resolving/bindings/trigger/causes/members".to_string(),
        format!("/stack/{}/bindings/trigger/causes/members", stack_index),
    ] {
        *comparable.pointer_mut(&pointer).unwrap() = before.pointer(&pointer).unwrap().clone();
    }
    assert_eq!(
        comparable, before,
        "only the two projections of the actual retained cause members differ"
    );
    let supplied_point = observe(&supplied, "/observations/supplied_root/state");
    let saved = supplied.clone();
    let pay = payment_input(
        &supplied,
        ResolutionPaymentReply::Pay(ManaPayment::default()),
    );
    let failed = capture::advance(&supplied, pay.clone());
    assert_eq!(
        failed,
        Err(GameError::Effect(EffectError::InvalidResolvingPlayer))
    );
    assert_eq!(
        failed,
        capture::without_capture(|| supplied.advance(pay.clone()))
    );
    assert_eq!(supplied, saved);
    let retained_point = observe(
        &supplied,
        "/observations/rejected_attempt/retained_after_attempt/state",
    );
    let decline = payment_input(&supplied, ResolutionPaymentReply::Decline);
    let declined = capture::advance(&supplied, decline.clone()).unwrap();
    assert_eq!(supplied, saved);
    assert_eq!(
        Ok(declined.clone()),
        capture::without_capture(|| supplied.advance(decline.clone()))
    );
    assert_eq!(declined.state().execution().life_total(B), Some(&20.into()));
    for seat in [A, B, C] {
        for color in ManaType::ALL {
            assert_eq!(
                declined.state().execution().mana(seat, color),
                supplied.execution().mana(seat, color)
            );
        }
    }
    assert!(declined.state().execution().window().resolving().is_none());
    assert!(declined.state().execution().window().entries().is_empty());
    assert!(declined.state().pending_decision().is_none());
    assert_eq!(declined.rules_journal().records.iter().filter(|row|matches!(row,RulesRecord::ResolutionPaymentOutcome { instruction:0,payer,outcome:ResolutionPaymentOutcome::Declined,.. } if *payer==B)).count(),1);
    assert_eq!(
        declined
            .rules_journal()
            .records
            .iter()
            .filter(|row| matches!(
                row,
                RulesRecord::ResolutionPaymentOutcome { .. }
                    | RulesRecord::OptionalEffectOutcome { .. }
            ))
            .count(),
        1
    );
    let events = occurrences(&declined);
    assert!(!events.iter().any(|row| matches!(
        row,
        RulesOccurrence::ManaDebited { .. }
            | RulesOccurrence::LifeChanged(_)
            | RulesOccurrence::TokenCreated { .. }
    )));
    assert_eq!(
        events
            .iter()
            .filter(|row| matches!(row, RulesOccurrence::EntryCompleted { .. }))
            .count(),
        1
    );
    let final_point = observe(
        declined.state(),
        "/observations/valid_continuation/final_state/state",
    );
    if !capture::is_active() {
        return None;
    }
    let setup_capture = setup_capture.unwrap();
    let roots = ROOT_SEGMENTS.with(|roots| std::mem::take(&mut *roots.borrow_mut()));
    assert_eq!(roots.len(), 1);
    Some(
        json!({"schema":SCHEMA,"scope":"NativeOnly","case_id":"OP-N10","scenario_id":scenario(lane,fault),"lane":lane.name(),"fault":projection::related_player_cause_fault(fault),
            "provenance":{"tag":"SyntheticNativeDisclosedCauseFault","ordinary_credit":false,"source_card_credit":false,"full_game_credit":false,"admitted_malformed_publication":false},
            "root_segments":roots,
            "clean_gate":{"point":clean_point.unwrap(),"state":before,"role_context":{"participants":[C.0,A.0,B.0],"active":C.0,"captured_controller":B.0,"related_player":A.0}},
            "clean_branch":{"pay_input":projection::input(&clean_pay),"role_state":state_observation(role_point.unwrap(),role.state()),"completion_input":projection::input(&completion),"final_state":state_observation(clean_final_point.unwrap(),completed.state())},
            "fault_setup":{"call_id":setup_capture.call,"fault":projection::related_player_cause_fault(fault),"before_point":setup_capture.before,"after_point":setup_capture.after.unwrap(),"selection":"SuppliedQualificationSource","gameplay_transition":"NotClaimed"},
            "supplied_root":{"tag":"NativeOnlySuppliedGameStateAfterCauseFault","owner":"GameState","point":supplied_point.unwrap(),"state":after,"initializer_root_ordinal":0,"fault_call_id":setup_capture.call,"ordinary_root_continuity":"NotClaimed"},
            "rejected_attempt":{"pay_input":projection::input(&pay),"retained_after_attempt":state_observation(retained_point.unwrap(),&supplied),"witness_join":"UniqueCompleteSourceAndInput"},
            "valid_continuation":{"decline_input":projection::input(&decline),"final_state":state_observation(final_point.unwrap(),declined.state()),"witness_join":"UniqueCompleteSourceAndInput"}
        }),
    )
}

#[test]
fn related_player_fault_both_lanes_reject_atomically_then_decline_same_malformed_root() {
    for lane in [Lane::Chooser, Lane::Payer] {
        for fault in FAULTS {
            assert!(exercise(lane, fault).is_none());
        }
    }
}
fn deny_setup(source: GameState, error: SetupError) {
    let saved = source.clone();
    assert_eq!(setup(&source, Fault::EmptyCauseSet).0, Err(error));
    assert_eq!(source, saved);
}
#[test]
fn related_player_fault_setup_wrong_gate_retains_whole_root() {
    deny_setup(world(vec![C, A, B], C, vec![]), SetupError::WrongGate);
}
#[test]
fn related_player_fault_setup_wrong_binding_retains_whole_root() {
    deny_setup(pending(vec![gate_instruction(1)]), SetupError::WrongBinding);
}
#[test]
fn related_player_fault_setup_insufficient_real_members_retains_whole_root() {
    deny_setup(
        captured_after_source_change(false, vec![gate_instruction(1)]),
        SetupError::InsufficientCauseMembers,
    );
}
#[test]
fn related_player_fault_setup_invalid_original_retains_whole_malformed_root() {
    let clean = grouped_gate(Lane::Chooser, B);
    let supplied = setup(&clean, Fault::ForeignParticipant).0.unwrap();
    deny_setup(supplied, SetupError::InvalidOriginalCauseSet);
}
#[test]
fn related_player_fault_setup_controller_not_distinct_retains_whole_root() {
    deny_setup(
        grouped_gate(Lane::Chooser, A),
        SetupError::ControllerNotDistinctParticipant,
    );
}

#[test]
#[ignore = "explicit separate role-fault export; final qualification requires reviewed reader and frozen source/image epoch"]
fn export_optional_payment_native_role_fault_packets() -> Result<(), String> {
    let output = std::env::var_os("SPELLWAKE_ROLE_FAULT_OUTPUT")
        .ok_or("SPELLWAKE_ROLE_FAULT_OUTPUT required")?;
    let output = Path::new(&output);
    fs::create_dir(output).map_err(|error| format!("fresh role-fault directory: {error}"))?;
    let packets = output.join("packets");
    fs::create_dir(&packets).map_err(|error| error.to_string())?;
    let repo = capture::repository();
    // Complete executable/source closure belongs to the external retained producer.
    // These explicit witnessed inputs bind this new method/fixture/contract to it.
    let sources = [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "crates/engine/src/game.rs",
        "crates/engine/src/game/related_player_fault_qualification.rs",
        "crates/engine/src/effects.rs",
        "crates/engine/src/effects/private_qualification.rs",
        "crates/engine/src/effects/private_qualification/related_player_fault.rs",
        "crates/engine/src/effects/resolution_choices.rs",
        "crates/engine/src/effects/resolution_choices/tests.rs",
        "crates/engine/src/effects/resolution_choices/native_mechanisms.rs",
        "crates/engine/src/effects/resolution_choices/native_packets.rs",
        "crates/engine/src/effects/resolution_choices/native_role_fault_packets.rs",
        "crates/engine/src/occurrence/causes.rs",
        "crates/engine/test_support/mod.rs",
        "crates/engine/test_support/related_player_fault.rs",
        "crates/engine/test_support/projection.rs",
        "crates/reference_tools/src/lib.rs",
        "crates/reference_tools/src/current_profile.rs",
        "crates/reference_tools/src/current_capture.rs",
        "crates/reference_tools/src/native_role_fault_wire.rs",
    ]
    .map(|path| repo.join(path));
    let mut artifacts = Vec::new();
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for lane in [Lane::Chooser, Lane::Payer] {
        for fault in FAULTS {
            let id = scenario(lane, fault);
            assert!(ids.insert(id.clone()));
            ROOT_SEGMENTS.with(|roots| roots.borrow_mut().clear());
            ROOT_LABEL.with(|label| {
                *label.borrow_mut() = format!("{id}/actual-clean-then-disclosed-fault")
            });
            capture::start_with_registry_witness(
                PROFILE,
                &sources,
                RegistryWitness::NotApplicable,
            )?;
            let observations = exercise(lane, fault).unwrap();
            let packet = capture::finish(PROFILE, observations)?;
            let path = packets.join(format!("{id}.json"));
            assert!(paths.insert(path.clone()));
            write_new(&path, &packet)?;
            let artifact = capture::source(&path)?;
            println!(
                "ROLE_FAULT_PUBLISHED {}",
                serde_json::to_string(&artifact).map_err(|error| error.to_string())?
            );
            artifacts.push(json!({"scenario_id":id,"artifact":artifact}));
        }
    }
    assert_eq!(ids.len(), 8);
    assert_eq!(paths.len(), 8);
    assert_eq!(
        fs::read_dir(&packets)
            .map_err(|error| error.to_string())?
            .count(),
        8
    );
    let index = json!({"schema":"spellwake.optional-payment-native-role-fault-index.v1","profile":PROFILE,"case_id":"OP-N10","artifacts":artifacts});
    validate_role_fault_index(&index)?;
    let path = output.join("index.json");
    write_new(&path, &index)?;
    println!(
        "ROLE_FAULT_PUBLISHED {}",
        serde_json::to_string(&capture::source(&path)?).map_err(|error| error.to_string())?
    );
    Ok(())
}

#[path = "native_role_fault_recorder_tests.rs"]
mod recorder_tests;
