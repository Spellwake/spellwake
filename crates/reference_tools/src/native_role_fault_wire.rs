//! Generated finite role-fault wire v1 from retained contracts/wire-contract.json.
//! Only reachable guards are emitted; full native/identity/semantic joins remain required.
use serde_json::Value;

fn valid_text(value: &Value) -> bool {
    value.as_str().is_some_and(|text| !text.is_empty())
}

fn valid_digest(value: &Value) -> bool {
    value.as_str().is_some_and(|text| {
        text.len() == 64
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn valid_seat(value: &Value) -> bool {
    value
        .as_f64()
        .is_some_and(|number| (0.0..=4294967295.0).contains(&number) && number.fract() == 0.0)
}

fn valid_bytes(value: &Value) -> bool {
    value
        .as_f64()
        .is_some_and(|number| (0.0..=9007199254740991.0).contains(&number) && number.fract() == 0.0)
}

fn valid_boolean(value: &Value) -> bool {
    value.is_boolean()
}

fn valid_native_object(value: &Value) -> bool {
    value.is_object()
}

fn valid_source_ref(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 3
            && part0.get("path").is_some_and(valid_text)
            && part0.get("sha256").is_some_and(valid_digest)
            && part0.get("bytes").is_some_and(valid_bytes)
    })
}

fn valid_journal(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|part0| part0.iter().all(valid_native_object))
}

fn valid_root_segment(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0
                .get("tag")
                .is_some_and(|field| field.as_str() == Some("ActualNativeInitializerRoot"))
            && part0.get("ordinal").is_some_and(valid_bytes)
            && part0.get("label").is_some_and(valid_text)
            && part0.get("helper").is_some_and(|field| {
                field.as_str() == Some("tests.initialize")
                    || field.as_str() == Some("native_mechanisms.world")
            })
            && part0.get("initial_state_point").is_some_and(valid_text)
            && part0.get("role_context").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 2
                        && part1.get("participants").is_some_and(|field| {
                            field.as_array().is_some_and(|part2| {
                                part2.len() >= 2 && part2.iter().all(valid_seat)
                            })
                        })
                        && part1.get("active").is_some_and(valid_seat)
                })
            })
            && part0.get("initial_state").is_some_and(valid_native_object)
            && part0.get("initial_journal").is_some_and(valid_journal)
    })
}

fn valid_point(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 12
            && part0.get("point_id").is_some_and(valid_text)
            && part0.get("observation_pointers").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.iter().all(valid_text))
            })
            && part0.get("phase").is_some_and(|field| {
                field.as_str() == Some("InitializeInput")
                    || field.as_str() == Some("BeforeCall")
                    || field.as_str() == Some("AfterCall")
                    || field.as_str() == Some("Observation")
                    || field.as_str() == Some("PureInput")
                    || field.as_str() == Some("PureResult")
            })
            && part0.get("owner").is_some_and(valid_text)
            && part0.get("owner_source").is_some_and(valid_text)
            && part0.get("policy_vocabulary").is_some_and(valid_text)
            && part0
                .get("native_vocabulary")
                .is_some_and(valid_native_object)
            && part0.get("native_context").is_some_and(valid_native_object)
            && part0.get("program_source").is_some_and(valid_native_object)
            && part0.get("projection").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 2
                        && part1
                            .get("tag")
                            .is_some_and(|field| field.as_str() == Some("Inline"))
                        && part1.get("value").is_some_and(valid_native_object)
                })
            })
            && part0.get("resolution").is_some_and(valid_native_object)
            && part0.get("damage_parent").is_some_and(valid_native_object)
    })
}

fn valid_call(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 9
            && part0.get("call_id").is_some_and(valid_text)
            && part0
                .get("before_point")
                .is_some_and(|field| valid_text(field) || field.is_null())
            && part0
                .get("after_point")
                .is_some_and(|field| valid_text(field) || field.is_null())
            && part0.get("owner_method").is_some_and(valid_text)
            && part0.get("input").is_some_and(valid_native_object)
            && part0.get("result").is_some_and(valid_native_object)
            && part0.get("adopted").is_some_and(valid_boolean)
            && part0.get("append").is_some_and(valid_native_object)
            && part0.get("replay").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 5
                        && part1
                            .get("source_eq")
                            .is_some_and(|field| field.as_bool() == Some(true) || field.is_null())
                        && part1
                            .get("full_result_eq")
                            .is_some_and(|field| field.as_bool() == Some(true))
                        && part1
                            .get("full_record_eq")
                            .is_some_and(|field| field.as_bool() == Some(true))
                        && part1.get("input_storage").is_some_and(|field| {
                            field.as_object().is_some_and(|part2| {
                                part2.len() == 2
                                    && part2.get("tag").is_some_and(|field| {
                                        field.as_str() == Some("SavedTypedCall")
                                    })
                                    && part2.get("call_id").is_some_and(valid_text)
                            })
                        })
                        && part1
                            .get("source_snapshot_before_invocation")
                            .is_some_and(|field| {
                                field.as_bool() == Some(true)
                                    || field.as_str() == Some("InputOnlyForInitializer")
                            })
                })
            })
    })
}

fn valid_coverage(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 6
            && part0.get("required_points").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.iter().all(valid_text))
            })
            && part0.get("required_calls").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.iter().all(valid_text))
            })
            && part0.get("observation_to_points").is_some_and(|field| {
                field.as_array().is_some_and(|part1| {
                    part1.iter().all(|item| {
                        item.as_object().is_some_and(|part2| {
                            part2.len() == 2
                                && part2.get("pointer").is_some_and(valid_text)
                                && part2.get("points").is_some_and(|field| {
                                    field.as_array().is_some_and(|part3| {
                                        !part3.is_empty() && part3.iter().all(valid_text)
                                    })
                                })
                        })
                    })
                })
            })
            && part0.get("record_pointers").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.iter().all(valid_text))
            })
            && part0.get("uncovered").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_text))
            })
            && part0
                .get("complete")
                .is_some_and(|field| field.as_bool() == Some(true))
    })
}

fn valid_witness(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 7
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.current-type-context-witness.v8")
            })
            && part0.get("vocabulary").is_some_and(valid_native_object)
            && part0.get("source_inputs").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.iter().all(valid_source_ref))
            })
            && part0.get("registry").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 2
                        && part1
                            .get("tag")
                            .is_some_and(|field| field.as_str() == Some("NotApplicable"))
                        && part1
                            .get("reason")
                            .is_some_and(|field| field.as_str() == Some("NoRegistryConstruction"))
                })
            })
            && part0.get("snapshots").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.iter().all(valid_point))
            })
            && part0.get("calls").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.iter().all(valid_call))
            })
            && part0.get("coverage").is_some_and(valid_coverage)
    })
}

fn valid_role_context(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 4
            && part0.get("participants").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 3 && part1.iter().all(valid_seat))
            })
            && part0.get("active").is_some_and(valid_seat)
            && part0.get("captured_controller").is_some_and(valid_seat)
            && part0.get("related_player").is_some_and(valid_seat)
    })
}

fn valid_state_observation(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 2
            && part0.get("point").is_some_and(valid_text)
            && part0.get("state").is_some_and(valid_native_object)
    })
}

fn valid_clean_gate(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 3
            && part0.get("point").is_some_and(valid_text)
            && part0.get("state").is_some_and(valid_native_object)
            && part0.get("role_context").is_some_and(valid_role_context)
    })
}

fn valid_clean_branch(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 4
            && part0.get("pay_input").is_some_and(valid_native_object)
            && part0.get("role_state").is_some_and(valid_state_observation)
            && part0
                .get("completion_input")
                .is_some_and(valid_native_object)
            && part0
                .get("final_state")
                .is_some_and(valid_state_observation)
    })
}

fn valid_supplied_root(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 7
            && part0.get("tag").is_some_and(|field| {
                field.as_str() == Some("NativeOnlySuppliedGameStateAfterCauseFault")
            })
            && part0
                .get("owner")
                .is_some_and(|field| field.as_str() == Some("GameState"))
            && part0.get("point").is_some_and(valid_text)
            && part0.get("state").is_some_and(valid_native_object)
            && part0
                .get("initializer_root_ordinal")
                .is_some_and(|field| field.as_f64() == Some(0.0))
            && part0.get("fault_call_id").is_some_and(valid_text)
            && part0
                .get("ordinary_root_continuity")
                .is_some_and(|field| field.as_str() == Some("NotClaimed"))
    })
}

fn valid_rejected_attempt(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 3
            && part0.get("pay_input").is_some_and(valid_native_object)
            && part0
                .get("retained_after_attempt")
                .is_some_and(valid_state_observation)
            && part0
                .get("witness_join")
                .is_some_and(|field| field.as_str() == Some("UniqueCompleteSourceAndInput"))
    })
}

fn valid_valid_continuation(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 3
            && part0.get("decline_input").is_some_and(valid_native_object)
            && part0
                .get("final_state")
                .is_some_and(valid_state_observation)
            && part0
                .get("witness_join")
                .is_some_and(|field| field.as_str() == Some("UniqueCompleteSourceAndInput"))
    })
}

fn valid_provenance(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 5
            && part0
                .get("tag")
                .is_some_and(|field| field.as_str() == Some("SyntheticNativeDisclosedCauseFault"))
            && part0
                .get("ordinary_credit")
                .is_some_and(|field| field.as_bool() == Some(false))
            && part0
                .get("source_card_credit")
                .is_some_and(|field| field.as_bool() == Some(false))
            && part0
                .get("full_game_credit")
                .is_some_and(|field| field.as_bool() == Some(false))
            && part0
                .get("admitted_malformed_publication")
                .is_some_and(|field| field.as_bool() == Some(false))
    })
}

fn valid_scenario_0(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 14
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-role-faults.v1")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10"))
            && part0
                .get("scenario_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10-CHOOSER-EMPTYCAUSESET"))
            && part0
                .get("lane")
                .is_some_and(|field| field.as_str() == Some("Chooser"))
            && part0.get("fault").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 1
                        && part1
                            .get("kind")
                            .is_some_and(|field| field.as_str() == Some("EmptyCauseSet"))
                })
            })
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("clean_gate").is_some_and(valid_clean_gate)
            && part0.get("clean_branch").is_some_and(valid_clean_branch)
            && part0.get("fault_setup").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 6
                        && part1.get("call_id").is_some_and(valid_text)
                        && part1.get("fault").is_some_and(|field| {
                            field.as_object().is_some_and(|part2| {
                                part2.len() == 1
                                    && part2.get("kind").is_some_and(|field| {
                                        field.as_str() == Some("EmptyCauseSet")
                                    })
                            })
                        })
                        && part1.get("before_point").is_some_and(valid_text)
                        && part1.get("after_point").is_some_and(valid_text)
                        && part1.get("selection").is_some_and(|field| {
                            field.as_str() == Some("SuppliedQualificationSource")
                        })
                        && part1
                            .get("gameplay_transition")
                            .is_some_and(|field| field.as_str() == Some("NotClaimed"))
                })
            })
            && part0.get("supplied_root").is_some_and(valid_supplied_root)
            && part0
                .get("rejected_attempt")
                .is_some_and(valid_rejected_attempt)
            && part0
                .get("valid_continuation")
                .is_some_and(valid_valid_continuation)
    })
}

fn valid_scenario_1(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 14
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-role-faults.v1")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10"))
            && part0
                .get("scenario_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10-CHOOSER-MISSINGRELATEDPLAYER"))
            && part0
                .get("lane")
                .is_some_and(|field| field.as_str() == Some("Chooser"))
            && part0.get("fault").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 1
                        && part1
                            .get("kind")
                            .is_some_and(|field| field.as_str() == Some("MissingRelatedPlayer"))
                })
            })
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("clean_gate").is_some_and(valid_clean_gate)
            && part0.get("clean_branch").is_some_and(valid_clean_branch)
            && part0.get("fault_setup").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 6
                        && part1.get("call_id").is_some_and(valid_text)
                        && part1.get("fault").is_some_and(|field| {
                            field.as_object().is_some_and(|part2| {
                                part2.len() == 1
                                    && part2.get("kind").is_some_and(|field| {
                                        field.as_str() == Some("MissingRelatedPlayer")
                                    })
                            })
                        })
                        && part1.get("before_point").is_some_and(valid_text)
                        && part1.get("after_point").is_some_and(valid_text)
                        && part1.get("selection").is_some_and(|field| {
                            field.as_str() == Some("SuppliedQualificationSource")
                        })
                        && part1
                            .get("gameplay_transition")
                            .is_some_and(|field| field.as_str() == Some("NotClaimed"))
                })
            })
            && part0.get("supplied_root").is_some_and(valid_supplied_root)
            && part0
                .get("rejected_attempt")
                .is_some_and(valid_rejected_attempt)
            && part0
                .get("valid_continuation")
                .is_some_and(valid_valid_continuation)
    })
}

fn valid_scenario_2(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 14
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-role-faults.v1")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10"))
            && part0
                .get("scenario_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10-CHOOSER-FOREIGNPARTICIPANT"))
            && part0
                .get("lane")
                .is_some_and(|field| field.as_str() == Some("Chooser"))
            && part0.get("fault").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 1
                        && part1
                            .get("kind")
                            .is_some_and(|field| field.as_str() == Some("ForeignParticipant"))
                })
            })
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("clean_gate").is_some_and(valid_clean_gate)
            && part0.get("clean_branch").is_some_and(valid_clean_branch)
            && part0.get("fault_setup").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 6
                        && part1.get("call_id").is_some_and(valid_text)
                        && part1.get("fault").is_some_and(|field| {
                            field.as_object().is_some_and(|part2| {
                                part2.len() == 1
                                    && part2.get("kind").is_some_and(|field| {
                                        field.as_str() == Some("ForeignParticipant")
                                    })
                            })
                        })
                        && part1.get("before_point").is_some_and(valid_text)
                        && part1.get("after_point").is_some_and(valid_text)
                        && part1.get("selection").is_some_and(|field| {
                            field.as_str() == Some("SuppliedQualificationSource")
                        })
                        && part1
                            .get("gameplay_transition")
                            .is_some_and(|field| field.as_str() == Some("NotClaimed"))
                })
            })
            && part0.get("supplied_root").is_some_and(valid_supplied_root)
            && part0
                .get("rejected_attempt")
                .is_some_and(valid_rejected_attempt)
            && part0
                .get("valid_continuation")
                .is_some_and(valid_valid_continuation)
    })
}

fn valid_scenario_3(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 14
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-role-faults.v1")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10"))
            && part0
                .get("scenario_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10-CHOOSER-AMBIGUOUSPARTICIPANTS"))
            && part0
                .get("lane")
                .is_some_and(|field| field.as_str() == Some("Chooser"))
            && part0.get("fault").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 1
                        && part1
                            .get("kind")
                            .is_some_and(|field| field.as_str() == Some("AmbiguousParticipants"))
                })
            })
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("clean_gate").is_some_and(valid_clean_gate)
            && part0.get("clean_branch").is_some_and(valid_clean_branch)
            && part0.get("fault_setup").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 6
                        && part1.get("call_id").is_some_and(valid_text)
                        && part1.get("fault").is_some_and(|field| {
                            field.as_object().is_some_and(|part2| {
                                part2.len() == 1
                                    && part2.get("kind").is_some_and(|field| {
                                        field.as_str() == Some("AmbiguousParticipants")
                                    })
                            })
                        })
                        && part1.get("before_point").is_some_and(valid_text)
                        && part1.get("after_point").is_some_and(valid_text)
                        && part1.get("selection").is_some_and(|field| {
                            field.as_str() == Some("SuppliedQualificationSource")
                        })
                        && part1
                            .get("gameplay_transition")
                            .is_some_and(|field| field.as_str() == Some("NotClaimed"))
                })
            })
            && part0.get("supplied_root").is_some_and(valid_supplied_root)
            && part0
                .get("rejected_attempt")
                .is_some_and(valid_rejected_attempt)
            && part0
                .get("valid_continuation")
                .is_some_and(valid_valid_continuation)
    })
}

fn valid_scenario_4(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 14
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-role-faults.v1")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10"))
            && part0
                .get("scenario_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10-PAYER-EMPTYCAUSESET"))
            && part0
                .get("lane")
                .is_some_and(|field| field.as_str() == Some("Payer"))
            && part0.get("fault").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 1
                        && part1
                            .get("kind")
                            .is_some_and(|field| field.as_str() == Some("EmptyCauseSet"))
                })
            })
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("clean_gate").is_some_and(valid_clean_gate)
            && part0.get("clean_branch").is_some_and(valid_clean_branch)
            && part0.get("fault_setup").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 6
                        && part1.get("call_id").is_some_and(valid_text)
                        && part1.get("fault").is_some_and(|field| {
                            field.as_object().is_some_and(|part2| {
                                part2.len() == 1
                                    && part2.get("kind").is_some_and(|field| {
                                        field.as_str() == Some("EmptyCauseSet")
                                    })
                            })
                        })
                        && part1.get("before_point").is_some_and(valid_text)
                        && part1.get("after_point").is_some_and(valid_text)
                        && part1.get("selection").is_some_and(|field| {
                            field.as_str() == Some("SuppliedQualificationSource")
                        })
                        && part1
                            .get("gameplay_transition")
                            .is_some_and(|field| field.as_str() == Some("NotClaimed"))
                })
            })
            && part0.get("supplied_root").is_some_and(valid_supplied_root)
            && part0
                .get("rejected_attempt")
                .is_some_and(valid_rejected_attempt)
            && part0
                .get("valid_continuation")
                .is_some_and(valid_valid_continuation)
    })
}

fn valid_scenario_5(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 14
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-role-faults.v1")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10"))
            && part0
                .get("scenario_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10-PAYER-MISSINGRELATEDPLAYER"))
            && part0
                .get("lane")
                .is_some_and(|field| field.as_str() == Some("Payer"))
            && part0.get("fault").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 1
                        && part1
                            .get("kind")
                            .is_some_and(|field| field.as_str() == Some("MissingRelatedPlayer"))
                })
            })
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("clean_gate").is_some_and(valid_clean_gate)
            && part0.get("clean_branch").is_some_and(valid_clean_branch)
            && part0.get("fault_setup").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 6
                        && part1.get("call_id").is_some_and(valid_text)
                        && part1.get("fault").is_some_and(|field| {
                            field.as_object().is_some_and(|part2| {
                                part2.len() == 1
                                    && part2.get("kind").is_some_and(|field| {
                                        field.as_str() == Some("MissingRelatedPlayer")
                                    })
                            })
                        })
                        && part1.get("before_point").is_some_and(valid_text)
                        && part1.get("after_point").is_some_and(valid_text)
                        && part1.get("selection").is_some_and(|field| {
                            field.as_str() == Some("SuppliedQualificationSource")
                        })
                        && part1
                            .get("gameplay_transition")
                            .is_some_and(|field| field.as_str() == Some("NotClaimed"))
                })
            })
            && part0.get("supplied_root").is_some_and(valid_supplied_root)
            && part0
                .get("rejected_attempt")
                .is_some_and(valid_rejected_attempt)
            && part0
                .get("valid_continuation")
                .is_some_and(valid_valid_continuation)
    })
}

fn valid_scenario_6(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 14
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-role-faults.v1")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10"))
            && part0
                .get("scenario_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10-PAYER-FOREIGNPARTICIPANT"))
            && part0
                .get("lane")
                .is_some_and(|field| field.as_str() == Some("Payer"))
            && part0.get("fault").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 1
                        && part1
                            .get("kind")
                            .is_some_and(|field| field.as_str() == Some("ForeignParticipant"))
                })
            })
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("clean_gate").is_some_and(valid_clean_gate)
            && part0.get("clean_branch").is_some_and(valid_clean_branch)
            && part0.get("fault_setup").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 6
                        && part1.get("call_id").is_some_and(valid_text)
                        && part1.get("fault").is_some_and(|field| {
                            field.as_object().is_some_and(|part2| {
                                part2.len() == 1
                                    && part2.get("kind").is_some_and(|field| {
                                        field.as_str() == Some("ForeignParticipant")
                                    })
                            })
                        })
                        && part1.get("before_point").is_some_and(valid_text)
                        && part1.get("after_point").is_some_and(valid_text)
                        && part1.get("selection").is_some_and(|field| {
                            field.as_str() == Some("SuppliedQualificationSource")
                        })
                        && part1
                            .get("gameplay_transition")
                            .is_some_and(|field| field.as_str() == Some("NotClaimed"))
                })
            })
            && part0.get("supplied_root").is_some_and(valid_supplied_root)
            && part0
                .get("rejected_attempt")
                .is_some_and(valid_rejected_attempt)
            && part0
                .get("valid_continuation")
                .is_some_and(valid_valid_continuation)
    })
}

fn valid_scenario_7(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 14
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-role-faults.v1")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10"))
            && part0
                .get("scenario_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10-PAYER-AMBIGUOUSPARTICIPANTS"))
            && part0
                .get("lane")
                .is_some_and(|field| field.as_str() == Some("Payer"))
            && part0.get("fault").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 1
                        && part1
                            .get("kind")
                            .is_some_and(|field| field.as_str() == Some("AmbiguousParticipants"))
                })
            })
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("clean_gate").is_some_and(valid_clean_gate)
            && part0.get("clean_branch").is_some_and(valid_clean_branch)
            && part0.get("fault_setup").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 6
                        && part1.get("call_id").is_some_and(valid_text)
                        && part1.get("fault").is_some_and(|field| {
                            field.as_object().is_some_and(|part2| {
                                part2.len() == 1
                                    && part2.get("kind").is_some_and(|field| {
                                        field.as_str() == Some("AmbiguousParticipants")
                                    })
                            })
                        })
                        && part1.get("before_point").is_some_and(valid_text)
                        && part1.get("after_point").is_some_and(valid_text)
                        && part1.get("selection").is_some_and(|field| {
                            field.as_str() == Some("SuppliedQualificationSource")
                        })
                        && part1
                            .get("gameplay_transition")
                            .is_some_and(|field| field.as_str() == Some("NotClaimed"))
                })
            })
            && part0.get("supplied_root").is_some_and(valid_supplied_root)
            && part0
                .get("rejected_attempt")
                .is_some_and(valid_rejected_attempt)
            && part0
                .get("valid_continuation")
                .is_some_and(valid_valid_continuation)
    })
}

fn valid_role_fault_artifact(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 2
            && part0.get("scenario_id").is_some_and(|field| {
                field.as_str() == Some("OP-N10-CHOOSER-EMPTYCAUSESET")
                    || field.as_str() == Some("OP-N10-CHOOSER-MISSINGRELATEDPLAYER")
                    || field.as_str() == Some("OP-N10-CHOOSER-FOREIGNPARTICIPANT")
                    || field.as_str() == Some("OP-N10-CHOOSER-AMBIGUOUSPARTICIPANTS")
                    || field.as_str() == Some("OP-N10-PAYER-EMPTYCAUSESET")
                    || field.as_str() == Some("OP-N10-PAYER-MISSINGRELATEDPLAYER")
                    || field.as_str() == Some("OP-N10-PAYER-FOREIGNPARTICIPANT")
                    || field.as_str() == Some("OP-N10-PAYER-AMBIGUOUSPARTICIPANTS")
            })
            && part0.get("artifact").is_some_and(valid_source_ref)
    })
}

fn valid_index(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 4
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-role-fault-index.v1")
            })
            && part0
                .get("profile")
                .is_some_and(|field| field.as_str() == Some("optional-payment-native-role-fault"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10"))
            && part0.get("artifacts").is_some_and(|field| {
                field.as_array().is_some_and(|part1| {
                    part1.len() == 8 && part1.iter().all(valid_role_fault_artifact)
                })
            })
    })
}

fn valid_observations(value: &Value) -> bool {
    valid_scenario_0(value)
        || valid_scenario_1(value)
        || valid_scenario_2(value)
        || valid_scenario_3(value)
        || valid_scenario_4(value)
        || valid_scenario_5(value)
        || valid_scenario_6(value)
        || valid_scenario_7(value)
}

fn valid_packet(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 4
            && part0
                .get("schema_version")
                .is_some_and(|field| field.as_f64() == Some(10.0))
            && part0
                .get("profile")
                .is_some_and(|field| field.as_str() == Some("optional-payment-native-role-fault"))
            && part0.get("observations").is_some_and(valid_observations)
            && part0.get("type_context_witness").is_some_and(valid_witness)
    })
}

/// Validate only the finite role-fault observations shape.
///
/// # Errors
/// Rejects missing/unknown/wrong-version finite fields, without proving native semantics.
pub(super) fn validate_observations(value: &Value) -> Result<(), String> {
    if valid_observations(value) {
        Ok(())
    } else {
        Err("role-fault observations differs from its separate finite v1 contract".into())
    }
}

/// Validate only the finite role-fault packet shape.
///
/// # Errors
/// Rejects missing/unknown/wrong-version finite fields, without proving native semantics.
pub(super) fn validate_packet(value: &Value) -> Result<(), String> {
    if valid_packet(value) {
        Ok(())
    } else {
        Err("role-fault packet differs from its separate finite v1 contract".into())
    }
}

/// Validate only the finite role-fault index shape.
///
/// # Errors
/// Rejects missing/unknown/wrong-version finite fields, without proving native semantics.
pub(super) fn validate_index(value: &Value) -> Result<(), String> {
    if valid_index(value) {
        Ok(())
    } else {
        Err("role-fault index differs from its separate finite v1 contract".into())
    }
}
