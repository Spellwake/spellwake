use super::*;
use serde_json::{Value, json};
fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn success(value: Value) -> Vec<u8> {
    bytes(&json!({"status":"success","data":value}))
}
fn frame() -> Value {
    json!({"contextId":Uuid::new_v4(),"stateRef":"fixture:state","factsRef":"fixture:facts","events":["fixture:event"],"facts":[],"clocks":[{"seatId":Uuid::new_v4(),"clockRevision":"0","remainingMs":"0","paused":true}],"participation":[]})
}
fn opening() -> Value {
    json!({"contextId":Uuid::from_u128(1),"snapshotRef":"fixture:snapshot","contextRevision":"v1","stateRef":"fixture:state","factsRef":"fixture:facts","eventRef":"fixture:event","qualificationRef":"fixture:qualification"})
}
#[test]
fn original_nested_source_terms_preserve_bytes_and_reject_ambiguous_shapes() {
    let body = frame();
    let token = serde_json::to_string_pretty(&body).unwrap();
    let original = format!("  {token}\n");
    assert_eq!(
        HostCandidate::from_bytes(original.as_bytes())
            .unwrap()
            .original_json(),
        token
    );
    for changed in [
        json!([]),
        {
            let mut x = body.clone();
            x["clocks"] = json!([[Uuid::new_v4(), "0", "0", true]]);
            x
        },
        {
            let mut x = body.clone();
            x["clocks"][0]["eligibility"] = Value::Null;
            x
        },
        {
            let mut x = body.clone();
            x["nativeEliminations"] = Value::Null;
            x
        },
        {
            let mut x = body.clone();
            x["clocks"][0]["paused"] = json!("true");
            x
        },
        {
            let mut x = body.clone();
            x["contextId"] = json!(Uuid::new_v4().to_string().to_uppercase());
            x
        },
        {
            let mut x = body.clone();
            x["events"] = json!([]);
            x
        },
        {
            let mut x = body.clone();
            x["extra"] = json!(true);
            x
        },
    ] {
        assert!(HostCandidate::from_bytes(&bytes(&changed)).is_err());
    }
    for key in ["seatId", r"s\u0065atId"] {
        let duplicate = body.to_string().replace(
            "\"seatId\":",
            &format!("\"{key}\":\"{}\",\"seatId\":", Uuid::new_v4()),
        );
        assert!(HostCandidate::from_bytes(duplicate.as_bytes()).is_err());
    }
    let mut nullable = body.clone();
    nullable["terminal"] = Value::Null;
    nullable["bankTimeout"] = Value::Null;
    nullable["nativeEliminations"] = json!([]);
    assert!(HostCandidate::from_bytes(&bytes(&nullable)).is_ok());
    let input =
        json!([{"seatId":Uuid::new_v4(),"revisionId":Uuid::new_v4(),"snapshotRef":"fixture:copy"}]);
    assert_eq!(
        AdmissionInputs::from_bytes(&bytes(&input))
            .unwrap()
            .original_json(),
        input.to_string()
    );
    assert!(RoomStartInputs::from_bytes(&bytes(&input)).is_err());
    let slot =
        json!([{"slotId":Uuid::new_v4(),"revisionId":Uuid::new_v4(),"snapshotRef":"fixture:copy"}]);
    assert!(RoomStartInputs::from_bytes(&bytes(&slot)).is_ok());
    let first = Uuid::new_v4();
    let selection = json!({"profileId":Uuid::new_v4(),"configurationKey":"fixture:duel","hostEntryId":first,"humanSeats":[{"queueEntryId":first,"seatIndex":"0"}],"aiSeats":[{"sponsorEntryId":first,"seatIndex":"1"}]});
    assert!(QueueSelection::from_bytes(&bytes(&selection)).is_ok());
    let mut hole = selection;
    hole["aiSeats"][0]["seatIndex"] = json!("5");
    assert!(QueueSelection::from_bytes(&bytes(&hole)).is_err());
    let mut changed = opening();
    changed["qualificationRef"] = Value::Null;
    assert!(OpeningArtifacts::from_bytes(&bytes(&changed)).is_err());
}
#[test]
fn source_acknowledgments_bind_original_identity_release_qualification_and_revision() {
    let room = Uuid::new_v4();
    let actor = Uuid::new_v4();
    let request = Uuid::new_v4();
    let ready = QualifiedReady {
        actor_id: actor,
        room_id: room,
        basis: 2.into(),
        ready: true,
        selected_revision_id: Some(Uuid::new_v4()),
        qualification_ref: Some("fixture:qualified".parse().unwrap()),
        request_id: request,
    };
    let countdown = QualifiedCountdown {
        actor_id: actor,
        room_id: room,
        basis: 2.into(),
        qualification_ref: "fixture:qualified".parse().unwrap(),
        request_id: request,
    };
    for revision in ["2", "3"] {
        let wire = success(json!({"resourceId":room,"revision":revision}));
        assert!(ready.decode_response(&wire).is_ok());
        assert!(countdown.decode_response(&wire).is_ok());
    }
    for data in [
        json!({"resourceId":Uuid::new_v4(),"revision":"3"}),
        json!({"resourceId":room,"revision":"4"}),
        json!({"resourceId":room,"revision":3}),
        json!([room, "3"]),
    ] {
        assert!(ready.decode_response(&success(data)).is_err());
    }
    let first = Uuid::new_v4();
    let selected=QueueSelection::from_bytes(&bytes(&json!({"profileId":Uuid::new_v4(),"configurationKey":"fixture:solo","hostEntryId":first,"humanSeats":[{"queueEntryId":first,"seatIndex":"0"}],"aiSeats":[]}))).unwrap();
    let claim = MatchmakingClaim {
        request_id: request,
        selection: selected,
    };
    assert!(
        claim
            .decode_response(&success(json!({"resourceId":room})))
            .is_ok()
    );
    assert!(
        claim
            .decode_response(br#"{"status":"fail","data":{"queue":"intent unavailable"}}"#)
            .is_ok()
    );
    assert!(
        claim
            .decode_response(br#"{"status":"fail","data":{"queue":"unknown"}}"#)
            .is_err()
    );
    let matched = Uuid::new_v4();
    let release = Uuid::new_v4();
    let admission=AcceptMatchDecks{match_id:matched,basis:0.into(),release_id:release,qualification_ref:"fixture:qualified".parse().unwrap(),inputs:AdmissionInputs::from_bytes(&bytes(&json!([{"seatId":Uuid::new_v4(),"revisionId":Uuid::new_v4(),"snapshotRef":"fixture:copy"}]))).unwrap(),request_id:request};
    let valid = json!({"resourceId":matched,"revision":"1","releaseId":release,"qualificationRef":"fixture:qualified"});
    assert!(admission.decode_response(&success(valid.clone())).is_ok());
    for key in ["resourceId", "releaseId", "qualificationRef", "revision"] {
        let mut changed = valid.clone();
        changed[key] = if key == "qualificationRef" {
            json!("fixture:other")
        } else if key == "revision" {
            json!("2")
        } else {
            json!(Uuid::new_v4())
        };
        assert!(admission.decode_response(&success(changed)).is_err());
    }
    let start=StartRoomMatch{room_id:room,basis:2.into(),release_id:release,legality_revision_id:Uuid::new_v4(),qualification_ref:"fixture:qualified".parse().unwrap(),inputs:RoomStartInputs::from_bytes(&bytes(&json!([{"slotId":Uuid::new_v4(),"revisionId":Uuid::new_v4(),"snapshotRef":"fixture:copy"}]))).unwrap(),request_id:request};
    assert!(
        start
            .decode_response(&success(
                json!({"resourceId":matched,"gameId":Uuid::new_v4()})
            ))
            .is_ok()
    );
    assert!(
        start
            .decode_response(&success(
                json!({"resourceId":matched,"gameId":Uuid::new_v4(),"revision":"1"})
            ))
            .is_err()
    );
}
#[test]
fn opening_and_outcome_replay_do_not_compare_with_a_resampled_retry_basis() {
    let matched = Uuid::new_v4();
    let game = Uuid::new_v4();
    let mut original = AcceptReservedGame {
        match_id: matched,
        expected_revision: 99.into(),
        game_id: game,
        owner: "fixture:host".parse().unwrap(),
        opening: OpeningArtifacts::from_bytes(&bytes(&opening())).unwrap(),
        original_accepted_basis: None,
    };
    let opened = success(
        json!({"resourceId":game,"contextId":original.opening.context_id(),"revision":"2"}),
    );
    assert!(original.decode_response(&opened).is_ok());
    original.original_accepted_basis = Some(1.into());
    assert!(original.decode_response(&opened).is_ok());
    original.original_accepted_basis = Some(99.into());
    assert!(original.decode_response(&opened).is_err());
    let mut outcome = ConsumeGameOutcome {
        result_id: Uuid::new_v4(),
        expected_revision: 99.into(),
        match_id: matched,
        original_accepted_basis: None,
    };
    let terminal = json!({"resourceId":matched,"revision":"2","phase":"terminal","terminalResultId":Uuid::new_v4(),"pendingDecisionIds":"[]"});
    assert!(outcome.decode_response(&success(terminal.clone())).is_ok());
    let mut decisions = vec![Uuid::new_v4(), Uuid::new_v4()];
    decisions.sort();
    let between = json!({"resourceId":matched,"revision":"2","phase":"between_games","terminalResultId":"","pendingDecisionIds":serde_json::to_string(&decisions).unwrap()});
    assert!(outcome.decode_response(&success(between.clone())).is_ok());
    for data in [
        {
            let mut x = between.clone();
            x["pendingDecisionIds"] = json!(decisions);
            x
        },
        {
            let mut x = between.clone();
            x["terminalResultId"] = Value::Null;
            x
        },
        {
            let mut x = between.clone();
            x.as_object_mut().unwrap().remove("terminalResultId");
            x
        },
        {
            let mut x = terminal;
            x["phase"] = json!({"terminal":null});
            x
        },
    ] {
        assert!(outcome.decode_response(&success(data)).is_err());
    }
    outcome.original_accepted_basis = Some(1.into());
    assert!(outcome.decode_response(&success(between.clone())).is_ok());
    outcome.original_accepted_basis = Some(99.into());
    assert!(outcome.decode_response(&success(between)).is_err());
    assert!(
        outcome
            .decode_response(br#"{"status":"fail","data":{"basis":"retry"}}"#)
            .is_err()
    );
}
#[test]
fn native_basis_checks_apply_to_success_without_hiding_stored_early_fence_rejection() {
    let mut body = frame();
    body["clocks"][0]["eligibility"] =
        json!({"reason":"native_wait","acceptedRevision":"0","proofRef":"fixture:priority"});
    let original = AcceptHostCandidate {
        game_id: Uuid::new_v4(),
        owner: "fixture:host".parse().unwrap(),
        epoch: 1.into(),
        basis: 7.into(),
        request_id: Uuid::new_v4(),
        candidate: HostCandidate::from_bytes(&bytes(&body)).unwrap(),
    };
    assert!(
        original
            .decode_response(br#"{"status":"fail","data":{"fence":"stale or unavailable"}}"#)
            .is_ok()
    );
    assert!(
        original
            .decode_response(&success(
                json!({"resourceId":Uuid::new_v4(),"revision":"8"})
            ))
            .is_err()
    );
    body["clocks"][0]["eligibility"]["reason"] = json!("native_unknown");
    assert!(HostCandidate::from_bytes(&bytes(&body)).is_err());
}

#[test]
fn candidate_nested_cardinality_literals_and_participation_rules_are_bounded() {
    let body = frame();
    for count in [1024, 1025] {
        let mut x = body.clone();
        x["events"] = json!(vec!["fixture:event"; count]);
        assert_eq!(HostCandidate::from_bytes(&bytes(&x)).is_ok(), count == 1024);
    }
    let fact = json!({"eventIndex":"0","accountId":Uuid::new_v4(),"kind":"fixture","revision":"v1","value":"0"});
    for count in [4096, 4097] {
        let mut x = body.clone();
        x["facts"] = json!(vec![fact.clone(); count]);
        assert_eq!(HostCandidate::from_bytes(&bytes(&x)).is_ok(), count == 4096);
    }
    for field in ["kind", "revision"] {
        let mut x = body.clone();
        let mut f = fact.clone();
        f[field] = json!("x".repeat(129));
        x["facts"] = json!([f]);
        assert!(HostCandidate::from_bytes(&bytes(&x)).is_err());
    }
    let mut out_of_range = body.clone();
    let mut f = fact;
    f["eventIndex"] = json!("1");
    out_of_range["facts"] = json!([f]);
    assert!(HostCandidate::from_bytes(&bytes(&out_of_range)).is_err());
    for count in [6, 7] {
        let clocks:Vec<_>=(0..count).map(|_|json!({"seatId":Uuid::new_v4(),"clockRevision":"0","remainingMs":"0","paused":true})).collect();
        let mut x = body.clone();
        x["clocks"] = json!(clocks);
        assert_eq!(HostCandidate::from_bytes(&bytes(&x)).is_ok(), count == 6);
    }
    let mut terminal = body.clone();
    terminal["terminal"] = json!({"outcome":"draw","winnerSeatIds":[],"winnersRef":"fixture:winners","reason":"fixture"});
    for count in [6, 7] {
        let mut x = terminal.clone();
        x["participation"] = json!(
            (0..count)
                .map(|_| json!({"seatId":Uuid::new_v4(),"ownTurns":"3","reason":"fixture"}))
                .collect::<Vec<_>>()
        );
        assert_eq!(HostCandidate::from_bytes(&bytes(&x)).is_ok(), count == 6);
    }
    let mut x = terminal.clone();
    x["terminal"]["reason"] = json!("x".repeat(257));
    assert!(HostCandidate::from_bytes(&bytes(&x)).is_err());
    let mut x = body.clone();
    x["participation"] = json!([{"seatId":Uuid::new_v4(),"ownTurns":"3","reason":"fixture"}]);
    assert!(HostCandidate::from_bytes(&bytes(&x)).is_err());
    let elimination = json!({"seatId":Uuid::new_v4(),"reason":"native_response_timeout","sourceRevision":"0","decisionRef":"fixture:decision","expiryRef":"fixture:expiry","expiredAt":"2026-11-04T00:00:00.000000Z"});
    let mut x = body.clone();
    x["nativeEliminations"] = json!([elimination.clone()]);
    assert!(HostCandidate::from_bytes(&bytes(&x)).is_ok());
    for (field, value) in [
        ("reason", json!("unknown")),
        ("expiredAt", json!("2026-11-04T00:00:00Z")),
        ("sourceRevision", json!(0)),
    ] {
        let mut x = body.clone();
        let mut e = elimination.clone();
        e[field] = value;
        x["nativeEliminations"] = json!([e]);
        assert!(HostCandidate::from_bytes(&bytes(&x)).is_err());
    }
    let mut x = body.clone();
    x["nativeEliminations"] = json!([elimination.clone(), elimination]);
    assert!(HostCandidate::from_bytes(&bytes(&x)).is_err());
    let mut x = body;
    x["bankTimeout"] = json!({"kind":"unknown","seatId":Uuid::new_v4(),"clockRevision":"0","proofRef":"fixture:timeout"});
    assert!(HostCandidate::from_bytes(&bytes(&x)).is_err());
}

#[test]
fn current_sql_derived_failure_vocabulary_remains_decodable() {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Case {
        source_function: String,
        field: String,
        reason: String,
    }
    // This golden vocabulary comes from canonical SQL, independently of the
    // Rust allowlists. Opening/outcome exceptions are deliberately not fail data.
    let cases: Vec<Case> = serde_json::from_str(include_str!(
        "../../tests/fixtures/admission-source-failures.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 50);
    let room = Uuid::new_v4();
    let actor = Uuid::new_v4();
    let request = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let release = Uuid::new_v4();
    let selection=QueueSelection::from_bytes(&bytes(&json!({"profileId":Uuid::new_v4(),"configurationKey":"fixture:duel","hostEntryId":actor,"humanSeats":[{"queueEntryId":actor,"seatIndex":"0"}],"aiSeats":[]}))).unwrap();
    let claim = MatchmakingClaim {
        request_id: request,
        selection,
    };
    let ready = QualifiedReady {
        actor_id: actor,
        room_id: room,
        basis: 0.into(),
        ready: true,
        selected_revision_id: Some(revision),
        qualification_ref: Some("fixture:qualification".parse().unwrap()),
        request_id: request,
    };
    let countdown = QualifiedCountdown {
        actor_id: actor,
        room_id: room,
        basis: 0.into(),
        qualification_ref: "fixture:qualification".parse().unwrap(),
        request_id: request,
    };
    let admission = AcceptMatchDecks {
        match_id: room,
        basis: 0.into(),
        release_id: release,
        qualification_ref: "fixture:qualification".parse().unwrap(),
        inputs: AdmissionInputs::from_bytes(&bytes(
            &json!([{"seatId":actor,"revisionId":revision,"snapshotRef":"fixture:snapshot"}]),
        ))
        .unwrap(),
        request_id: request,
    };
    let start = StartRoomMatch {
        room_id: room,
        basis: 0.into(),
        release_id: release,
        legality_revision_id: Uuid::new_v4(),
        qualification_ref: "fixture:qualification".parse().unwrap(),
        inputs: RoomStartInputs::from_bytes(&bytes(
            &json!([{"slotId":actor,"revisionId":revision,"snapshotRef":"fixture:snapshot"}]),
        ))
        .unwrap(),
        request_id: request,
    };
    let candidate = AcceptHostCandidate {
        game_id: room,
        owner: "fixture:host".parse().unwrap(),
        epoch: 1.into(),
        basis: 0.into(),
        request_id: request,
        candidate: HostCandidate::from_bytes(&bytes(&frame())).unwrap(),
    };
    for case in cases {
        let wire = bytes(&json!({"status":"fail","data":{case.field:case.reason}}));
        let accepted = match case.source_function.as_str() {
            "claim_matchmaking" => claim.decode_response(&wire).is_ok(),
            "set_room_ready" => ready.decode_response(&wire).is_ok(),
            "start_room_countdown" => countdown.decode_response(&wire).is_ok(),
            "accept_match_decks" => admission.decode_response(&wire).is_ok(),
            "start_room_match" => start.decode_response(&wire).is_ok(),
            "accept_host_candidate" => candidate.decode_response(&wire).is_ok(),
            _ => panic!("unexpected source family"),
        };
        assert!(accepted, "{}", case.source_function);
    }
}
#[test]
fn opening_context_is_required_original_canonical_and_correlated_with_ack() {
    let value = opening();
    let text = serde_json::to_string_pretty(&value).unwrap();
    let parsed = OpeningArtifacts::from_bytes(text.as_bytes()).unwrap();
    assert_eq!(parsed.original_json(), text);
    assert_eq!(parsed.context_id(), Uuid::from_u128(1));
    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove("contextId");
    assert!(OpeningArtifacts::from_bytes(&bytes(&missing)).is_err());
    for context in [
        Value::Null,
        json!([]),
        json!(Uuid::nil()),
        json!("AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA"),
        json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        json!("{aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa}"),
    ] {
        let mut changed = value.clone();
        changed["contextId"] = context;
        assert!(OpeningArtifacts::from_bytes(&bytes(&changed)).is_err());
    }
    let duplicate = format!(
        "{{\"contextId\":\"{}\",{}",
        parsed.context_id(),
        text.strip_prefix('{').unwrap()
    );
    assert!(OpeningArtifacts::from_bytes(duplicate.as_bytes()).is_err());
    let game = Uuid::new_v4();
    let original = AcceptReservedGame {
        match_id: Uuid::new_v4(),
        expected_revision: 1.into(),
        game_id: game,
        owner: "fixture:owner".parse().unwrap(),
        opening: parsed,
        original_accepted_basis: Some(1.into()),
    };
    assert!(
        original
            .decode_response(&success(
                json!({"resourceId":game,"contextId":original.opening.context_id(),"revision":"2"})
            ))
            .is_ok()
    );
    assert!(
        original
            .decode_response(&success(
                json!({"resourceId":game,"contextId":Uuid::new_v4(),"revision":"2"})
            ))
            .is_err()
    );
}

#[test]
fn opening_observation_is_optional_historically_but_never_nullable_or_foreign_to_state() {
    let old = opening();
    assert!(OpeningArtifacts::from_bytes(&bytes(&old)).is_ok());
    let mut new = old.clone();
    new["clockObservation"] = json!({"reason":"native_decision","seatId":Uuid::from_u128(12),"acceptedRevision":"0","proofRef":old["stateRef"]});
    let text = serde_json::to_string_pretty(&new).unwrap();
    assert_eq!(
        OpeningArtifacts::from_bytes(text.as_bytes())
            .unwrap()
            .original_json(),
        text
    );
    for value in [
        Value::Null,
        json!([]),
        json!({"reason":"native_decision","seatId":Uuid::from_u128(12),"acceptedRevision":"0","proofRef":"foreign-state"}),
    ] {
        new["clockObservation"] = value;
        assert!(OpeningArtifacts::from_bytes(&bytes(&new)).is_err());
    }
}
