//! Generated finite native-packet JSON-wire admission from the retained v2 contract.
//! Complete native payload bodies and semantic joins remain with canonical serializers/readers.
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

fn valid_roles_default(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 2
            && part0.get("participants").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 2 && part1.iter().all(valid_seat))
            })
            && part0.get("active").is_some_and(valid_seat)
    })
}

fn valid_roles_trigger(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 3
            && part0.get("active").is_some_and(valid_seat)
            && part0.get("captured_controller").is_some_and(valid_seat)
            && part0.get("caster_and_payer").is_some_and(valid_seat)
    })
}

fn valid_roles_grouped(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 3
            && part0.get("active").is_some_and(valid_seat)
            && part0.get("captured_controller").is_some_and(valid_seat)
            && part0.get("creator_and_payer").is_some_and(valid_seat)
    })
}

fn valid_roles_watcher(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 4
            && part0.get("participants").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 2 && part1.iter().all(valid_seat))
            })
            && part0.get("active").is_some_and(valid_seat)
            && part0.get("source_owner").is_some_and(valid_seat)
            && part0.get("source_controller").is_some_and(valid_seat)
    })
}

fn valid_provenance(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 4
            && part0
                .get("tag")
                .is_some_and(|field| field.as_str() == Some("SyntheticNativeOnly"))
            && part0
                .get("ordinary_credit")
                .is_some_and(|field| field.as_bool() == Some(false))
            && part0
                .get("source_card_credit")
                .is_some_and(|field| field.as_bool() == Some(false))
            && part0
                .get("full_game_credit")
                .is_some_and(|field| field.as_bool() == Some(false))
    })
}

fn valid_journal(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|part0| part0.iter().all(valid_native_object))
}

fn valid_movement(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 2
            && part0.get("before").is_some_and(valid_native_object)
            && part0.get("after").is_some_and(valid_native_object)
    })
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

fn valid_supplied_owner(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 9
            && part0.get("tag").is_some_and(|field| {
                field.as_str() == Some("NativeOnlySuppliedOwnerAfterPrivilegedSetup")
            })
            && part0
                .get("owner")
                .is_some_and(|field| field.as_str() == Some("EffectWindow"))
            && part0.get("point").is_some_and(valid_text)
            && part0
                .get("root_continuity")
                .is_some_and(|field| field.as_str() == Some("NotClaimed"))
            && part0.get("setup").is_some_and(valid_text)
            && part0.get("preceding_root_ordinal").is_some_and(valid_bytes)
            && part0.get("state").is_some_and(valid_native_object)
            && part0.get("setup_movements").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_movement))
            })
            && part0.get("setup_journal").is_some_and(valid_journal)
    })
}

fn valid_probe_changed(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 4
            && part0
                .get("kind")
                .is_some_and(|field| field.as_str() == Some("LegalChangedColor"))
            && part0.get("input").is_some_and(valid_native_object)
            && part0.get("result").is_some_and(valid_native_object)
            && part0
                .get("expected_relation")
                .is_some_and(|field| field.as_str() == Some("DifferentJournalAndWholeState"))
    })
}

fn valid_probe_wrong(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 4
            && part0
                .get("kind")
                .is_some_and(|field| field.as_str() == Some("WrongActor"))
            && part0.get("input").is_some_and(valid_native_object)
            && part0.get("error").is_some_and(valid_native_object)
            && part0
                .get("expected_relation")
                .is_some_and(|field| field.as_str() == Some("AtomicTypedErr"))
    })
}

fn valid_replay_branch(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 10
            && part0.get("optional").is_some_and(|field| {
                field.as_str() == Some("Take")
                    || field.as_str() == Some("Decline")
                    || field.is_null()
            })
            && part0.get("start").is_some_and(|field| {
                field.as_str() == Some("RetainedNativeStateAfterActualInitializer")
            })
            && part0.get("root_anchor").is_some_and(|field| {
                field.as_object().is_some_and(|part1| {
                    part1.len() == 2
                        && part1.get("ordinal").is_some_and(valid_bytes)
                        && part1.get("initial_state_point").is_some_and(valid_text)
                })
            })
            && part0.get("saved_inputs").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| !part1.is_empty() && part1.iter().all(valid_native_object))
            })
            && part0.get("saved_journals").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| !part1.is_empty() && part1.iter().all(valid_journal))
            })
            && part0.get("saved_states").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| !part1.is_empty() && part1.iter().all(valid_native_object))
            })
            && part0.get("replayed_journals").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| !part1.is_empty() && part1.iter().all(valid_journal))
            })
            && part0.get("replayed_states").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| !part1.is_empty() && part1.iter().all(valid_native_object))
            })
            && part0.get("probes").is_some_and(|field| {
                field.as_array().is_some_and(|part1| {
                    part1.len() == 2
                        && part1
                            .iter()
                            .all(|item| valid_probe_changed(item) || valid_probe_wrong(item))
                })
            })
            && part0.get("final_state").is_some_and(valid_native_object)
    })
}

fn valid_saved_replay(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 2
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.native-saved-input-replay.v2")
            })
            && part0.get("branches").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 3 && part1.iter().all(valid_replay_branch))
            })
    })
}

fn valid_op_n03(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N03"))
            && part0.get("roles").is_some_and(valid_roles_trigger)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 2 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n04(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N04"))
            && part0.get("roles").is_some_and(valid_roles_trigger)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 2 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n05(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N05"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n06(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N06"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n07(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N07"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n08(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N08"))
            && part0.get("roles").is_some_and(valid_roles_watcher)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 2 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n10(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N10"))
            && part0.get("roles").is_some_and(valid_roles_grouped)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n11(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N11"))
            && part0.get("roles").is_some_and(|field| field.is_null())
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n15(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N15"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 3 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n16(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N16"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 3 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(valid_saved_replay)
    })
}

fn valid_op_n18(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N18"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n19(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N19"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 2 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n20(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N20"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n21(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N21"))
            && part0.get("roles").is_some_and(|field| field.is_null())
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n22(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N22"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 3 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n23(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N23"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.is_empty() && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_op_n24(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 8
            && part0.get("schema").is_some_and(|field| {
                field.as_str() == Some("spellwake.optional-payment-native-packets.v2")
            })
            && part0
                .get("scope")
                .is_some_and(|field| field.as_str() == Some("NativeOnly"))
            && part0
                .get("case_id")
                .is_some_and(|field| field.as_str() == Some("OP-N24"))
            && part0.get("roles").is_some_and(valid_roles_default)
            && part0.get("provenance").is_some_and(valid_provenance)
            && part0.get("root_segments").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 2 && part1.iter().all(valid_root_segment))
            })
            && part0.get("supplied_owners").is_some_and(|field| {
                field
                    .as_array()
                    .is_some_and(|part1| part1.len() == 1 && part1.iter().all(valid_supplied_owner))
            })
            && part0.get("replay").is_some_and(|field| field.is_null())
    })
}

fn valid_observations(value: &Value) -> bool {
    valid_op_n03(value)
        || valid_op_n04(value)
        || valid_op_n05(value)
        || valid_op_n06(value)
        || valid_op_n07(value)
        || valid_op_n08(value)
        || valid_op_n10(value)
        || valid_op_n11(value)
        || valid_op_n15(value)
        || valid_op_n16(value)
        || valid_op_n18(value)
        || valid_op_n19(value)
        || valid_op_n20(value)
        || valid_op_n21(value)
        || valid_op_n22(value)
        || valid_op_n23(value)
        || valid_op_n24(value)
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

fn valid_packet(value: &Value) -> bool {
    value.as_object().is_some_and(|part0| {
        part0.len() == 4
            && part0
                .get("schema_version")
                .is_some_and(|field| field.as_f64() == Some(10.0))
            && part0
                .get("profile")
                .is_some_and(|field| field.as_str() == Some("optional-payment-native"))
            && part0.get("observations").is_some_and(valid_observations)
            && part0.get("type_context_witness").is_some_and(valid_witness)
    })
}

/// Check only the generated finite native observations JSON structure.
///
/// # Errors
/// Rejects values outside this exact finite wire contract; does not certify native semantics.
pub(super) fn validate_observations(value: &Value) -> Result<(), String> {
    if valid_observations(value) {
        Ok(())
    } else {
        Err("native-only observations differs from its finite wire contract".into())
    }
}

/// Check only the generated finite native packet JSON structure.
///
/// # Errors
/// Rejects values outside this exact finite wire contract; does not certify native semantics.
pub(super) fn validate_packet(value: &Value) -> Result<(), String> {
    if valid_packet(value) {
        Ok(())
    } else {
        Err("native-only packet differs from its finite wire contract".into())
    }
}
