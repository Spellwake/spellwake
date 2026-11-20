//! Independent wire-name/shape oracles, compiled through both shared-module roots.
//! These are read-only contract tests, not native setup-error or capture evidence.
use super::super::{
    packet::AliasContext,
    projection::{self, QualificationCall},
    related_player_fault::{
        RelatedPlayerCauseFault as Fault, RelatedPlayerFaultSetupError as Error,
    },
};
use super::native_role_fault_wire;
use serde_json::{Value, json};

/// Finite transport only: open body objects/reference strings are intentionally
/// not native owners. No GameState, initializer, recorder or exporter is invoked.
fn finite_transport_header(lane: &str, scenario: &str, fault: &Value) -> Value {
    let observation = json!({"point":"unit-reference","state":{}});
    json!({
        "schema":"spellwake.optional-payment-native-role-faults.v1",
        "scope":"NativeOnly","case_id":"OP-N10","scenario_id":scenario,"lane":lane,"fault":fault,
        "provenance":{"tag":"SyntheticNativeDisclosedCauseFault","ordinary_credit":false,"source_card_credit":false,"full_game_credit":false,"admitted_malformed_publication":false},
        "root_segments":[{
            "tag":"ActualNativeInitializerRoot","ordinal":0,
            "label":"unit-transport-only-no-native-owner","helper":"native_mechanisms.world",
            "initial_state_point":"unit-reference","role_context":{"participants":[2,0,1],"active":2},
            "initial_state":{},"initial_journal":[]
        }],
        "clean_gate":{"point":"unit-reference","state":{},"role_context":{"participants":[2,0,1],"active":2,"captured_controller":1,"related_player":0}},
        "clean_branch":{"pay_input":{},"role_state":observation,"completion_input":{},"final_state":observation},
        "fault_setup":{"call_id":"unit-call","fault":fault,"before_point":"unit-before","after_point":"unit-after","selection":"SuppliedQualificationSource","gameplay_transition":"NotClaimed"},
        "supplied_root":{"tag":"NativeOnlySuppliedGameStateAfterCauseFault","owner":"GameState","point":"unit-reference","state":{},"initializer_root_ordinal":0,"fault_call_id":"unit-call","ordinary_root_continuity":"NotClaimed"},
        "rejected_attempt":{"pay_input":{},"retained_after_attempt":observation,"witness_join":"UniqueCompleteSourceAndInput"},
        "valid_continuation":{"decline_input":{},"final_state":observation,"witness_join":"UniqueCompleteSourceAndInput"}
    })
}

#[test]
fn all_four_fault_inputs_match_literal_shapes_and_generated_scenario_names() {
    // The names and eight IDs are independent accepted-v1 wire literals. Neither
    // Debug formatting nor a second projection call constructs the expected data.
    let cases = [
        (
            Fault::EmptyCauseSet,
            "EmptyCauseSet",
            "OP-N10-CHOOSER-EMPTYCAUSESET",
            "OP-N10-PAYER-EMPTYCAUSESET",
        ),
        (
            Fault::MissingRelatedPlayer,
            "MissingRelatedPlayer",
            "OP-N10-CHOOSER-MISSINGRELATEDPLAYER",
            "OP-N10-PAYER-MISSINGRELATEDPLAYER",
        ),
        (
            Fault::ForeignParticipant,
            "ForeignParticipant",
            "OP-N10-CHOOSER-FOREIGNPARTICIPANT",
            "OP-N10-PAYER-FOREIGNPARTICIPANT",
        ),
        (
            Fault::AmbiguousParticipants,
            "AmbiguousParticipants",
            "OP-N10-CHOOSER-AMBIGUOUSPARTICIPANTS",
            "OP-N10-PAYER-AMBIGUOUSPARTICIPANTS",
        ),
    ];
    for (native, name, chooser_scenario, payer_scenario) in cases {
        let actual = projection::related_player_cause_fault(native);
        let call = QualificationCall::GameRelatedPlayerCauseFault { fault: native };
        let actual_input =
            projection::qualification_call(&call, &AliasContext::default(), None).unwrap();

        assert_eq!(actual, json!({"kind":name}));
        assert_eq!(
            actual_input,
            json!({"tag":"GameRelatedPlayerCauseFault","fault":{"kind":name}})
        );
        assert_eq!(
            call.owner_method(),
            "GameState::qualification_with_related_player_cause_fault"
        );
        assert_eq!(call.owner_name(), "GameState");
        assert_eq!(call.result_owner_name(), "GameState");
        assert!(!call.is_pure());
        assert!(!call.is_initializer());
        assert!(!call.returns_rules_append());

        // Exercise the actual generated finite owner, so the independent names
        // must agree with both coupled lane/scenario alternatives, not only the serializer.
        for (lane, scenario) in [("Chooser", chooser_scenario), ("Payer", payer_scenario)] {
            let header = finite_transport_header(lane, scenario, &actual);
            assert!(
                native_role_fault_wire::validate_observations(&header).is_ok(),
                "{scenario}"
            );
            let mut unknown = header;
            unknown["fault"]["kind"] = json!("UnknownCauseFault");
            unknown["fault_setup"]["fault"]["kind"] = json!("UnknownCauseFault");
            assert!(native_role_fault_wire::validate_observations(&unknown).is_err());
        }
    }
}

#[test]
fn all_six_setup_error_projections_match_independent_literal_domain_and_payload_shape() {
    let cases = [
        (Error::WrongGate, "WrongGate"),
        (Error::WrongBinding, "WrongBinding"),
        (Error::InsufficientCauseMembers, "InsufficientCauseMembers"),
        (Error::InvalidOriginalCauseSet, "InvalidOriginalCauseSet"),
        (
            Error::ControllerNotDistinctParticipant,
            "ControllerNotDistinctParticipant",
        ),
        // Projection-only: this does not invoke or claim a reached NoForeignSeat failure.
        (Error::NoForeignSeat, "NoForeignSeat"),
    ];
    for (native, name) in cases {
        let actual = projection::related_player_fault_setup_error(&native);
        // Exact object equality protects tag, domain, variant and the absence of
        // extra data/state fields for these payload-free error values.
        assert_eq!(
            actual,
            json!({"tag":"Err","error":{"domain":"RelatedPlayerFaultSetupError","variant":name}})
        );
    }
}
