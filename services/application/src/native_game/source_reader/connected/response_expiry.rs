//! Three genuine selected decks, elapsed SQL time, cold recovery and BO1 settlement.
use super::*;
use crate::player_wire::{PlayerAction, PlayerChoice, PlayerCommand, PlayerDecision, PlayerDomain};
use std::time::{Duration, Instant};

pub(super) fn run(r: &Reservation, store: &NativeArtifactStore, programs: &NativeProgramSource) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let recover = || {
        repository
            .recover_with_history(
                r.game_id,
                store,
                programs,
                NativeRecoveryBudget::new(1000, 100_000_000).unwrap(),
            )
            .unwrap()
            .unwrap()
    };
    let http = http::HttpProbe::owned(
        &std::env::var("SPELLWAKE_SUPABASE_URL").unwrap(),
        &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
        "fixture:source-reader",
    );
    let peer = std::env::var("SPELLWAKE_NATIVE_SOURCE_PEER_BEARER").unwrap();
    let third = std::env::var("SPELLWAKE_NATIVE_SOURCE_OUTSIDER_BEARER").unwrap();
    let scenario = std::env::var("SPELLWAKE_OPENING_TIMEOUT_SCENARIO").unwrap_or_default();
    if matches!(scenario.as_str(), "priority" | "combat-choice") {
        super::turn_expiry::run(r, store, programs, &http, scenario == "combat-choice");
        return;
    }
    let second_chooser = scenario == "second";
    let successive = scenario == "successive";
    let restored = scenario == "restored";
    let entropy = scenario == "entropy" || restored;
    let bystander = scenario == "bystander" || entropy;
    assert!(scenario.is_empty() || second_chooser || successive || bystander);
    let owner = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let first_deadline = repository
        .opening_response_wait(r.game_id)
        .unwrap()
        .unwrap()
        .expires_at()
        .to_owned();
    let first_view = http.read(Some(&owner), r.game_id, r.context_id, 200);
    let decisions = first_view["data"]["snapshot"]["decisions"]
        .as_array()
        .unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(
        decisions[0]["deadlineAt"].as_str(),
        Some(first_deadline.as_str())
    );
    let cached = http.read(Some(&owner), r.game_id, r.context_id, 200);
    assert_eq!(
        cached["data"]["snapshot"]["decisions"].as_array(),
        Some(decisions)
    );
    let other_view = http.read(Some(&peer), r.game_id, r.context_id, 200);
    assert!(
        other_view["data"]["snapshot"]["decisions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let first_deadline = if restored {
        let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
        let paused = lifecycle_reads::sql(
            &database,
            "SELECT app.pause_game_incident($1,'fixture:source-reader',1,0,clock_timestamp(),'storage','fixture:response-recovery',$2)",
            &[&r.game_id, &Uuid::new_v4()],
        );
        assert_eq!(paused["status"], "success", "{paused}");
        assert!(
            repository
                .opening_response_wait(r.game_id)
                .unwrap()
                .is_none()
        );
        let cp: Uuid = serde_json::from_value(paused["data"]["checkpointId"].clone()).unwrap();
        let resumed = lifecycle_reads::sql(
            &database,
            "SELECT app.restore_game_checkpoint($1,2,1,$2,'fixture:source-reader','fixture:exact-response',$3)",
            &[&r.game_id, &cp, &Uuid::new_v4()],
        );
        assert_eq!(resumed["status"], "success", "{resumed}");
        let interval = lifecycle_reads::sql(
            &database,
            "SELECT jsonb_build_array(i.normalized_source->>'incidentAt',r.normalized_source->>'resumedAt') FROM app.operational_state op JOIN app.command_receipt i ON i.id=op.incident_receipt_id JOIN app.command_receipt r ON r.id=op.restoration_receipt_id WHERE op.game_id=$1",
            &[&r.game_id],
        );
        let instant = |v: &str| chrono::DateTime::parse_from_rfc3339(v).unwrap();
        let stopped = instant(interval[0].as_str().unwrap());
        let resumed = instant(interval[1].as_str().unwrap());
        let wait = repository
            .opening_response_wait(r.game_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            instant(wait.expires_at()) - resumed,
            instant(&first_deadline) - stopped
        );
        let shown = http.read(Some(&owner), r.game_id, r.context_id, 200);
        assert_eq!(
            shown["data"]["snapshot"]["decisions"][0]["deadlineAt"],
            wait.expires_at()
        );
        wait.expires_at().to_owned()
    } else {
        first_deadline
    };
    if entropy {
        let source = recover();
        let checkpoint = source.checkpoint();
        let seat = r.seat_ids[0];
        let scope =
            NativeScope::new(checkpoint.state(), checkpoint.binding(seat).unwrap()).unwrap();
        let (bytes, expected) = scope
            .prepare_entropy(vec![1, 2, u64::MAX])
            .unwrap()
            .into_parts();
        let input = store
            .retain(r.game_id, r.context_id, NativeArtifactRole::Input, &bytes)
            .unwrap();
        let next = checkpoint
            .append(store, input, NativeCheckpointInput::Entropy, seat)
            .unwrap();
        assert_eq!(next.candidate(), &expected);
        next.retain(store).unwrap();
        next.prepare_event().unwrap().retain(store).unwrap();
        next.prepare_results().unwrap().retain(store).unwrap();
        let request = Uuid::new_v4();
        let candidate = source.prepare_candidate(&next).unwrap();
        let original = NativeSystemOriginal::prepare_entropy(request, &source, &next).unwrap();
        let submit = || {
            repository
                .commit_entropy(store, &source, request, &next, &candidate, &original)
                .unwrap()
        };
        let accepted = submit();
        let retried = submit();
        assert_eq!(accepted.resource_id, retried.resource_id);
        assert_eq!(accepted.revision, retried.revision);
        let recovered = recover();
        assert!(recovered.checkpoint().reference() == next.reference());
        let rebound = http.read(Some(&owner), r.game_id, r.context_id, 200);
        let decision = &rebound["data"]["snapshot"]["decisions"][0];
        assert_eq!(decision["deadlineAt"], first_deadline);
        assert_ne!(decision["decision"], decisions[0]["decision"]);
    }
    let timeout_index = usize::from(second_chooser);
    let winner_index = if successive {
        2
    } else {
        usize::from(!second_chooser)
    };
    if second_chooser {
        keep_hand(&http, &owner, r);
    } else if bystander {
        http.concede(&third, r.game_id, r.context_id);
    }
    let opening = recover();
    let current = opening.checkpoint();
    let first_wait = repository
        .opening_response_wait(r.game_id)
        .unwrap()
        .unwrap();
    if second_chooser {
        assert_eq!(current.basis().revision.get(), 1);
        assert!(first_deadline.as_str() < first_wait.expires_at());
    }
    if bystander {
        assert_eq!(current.basis().revision.get(), if entropy { 2 } else { 1 });
        assert_eq!(first_wait.expires_at(), first_deadline);
    }
    let responder = if second_chooser { &peer } else { &owner };
    let reconnect = json!({"receipts":{"gameId":r.game_id,"contextId":r.context_id,"outstanding":[]},"cursor":null});
    let public = http.reconnect(Some(responder), r.game_id, &reconnect, 200);
    let shown = &public["data"]["snapshot"]["decisions"];
    assert_eq!(
        shown[0]["deadlineAt"].as_str(),
        Some(first_wait.expires_at())
    );
    if second_chooser {
        let previous = http.read(Some(&owner), r.game_id, r.context_id, 200);
        assert_eq!(previous["data"]["snapshot"]["decisions"], json!([]));
    }
    let late = keep_original(&http, responder, r);
    let assert_denied = || {
        assert_eq!(
            http.command(Some(responder), &late, 409),
            json!({"status":"fail","data":{"decision":"stale"}})
        );
    };
    let next = expire(
        &repository,
        store,
        r,
        &opening,
        &first_wait,
        r.seat_ids[timeout_index],
        assert_denied,
    );
    assert_denied();
    let recovered = recover();
    assert!(recovered.history_evidence().transitions_verified());
    assert!(recovered.checkpoint().reference() == next.reference());
    if !bystander {
        let next_wait = repository
            .opening_response_wait(r.game_id)
            .unwrap()
            .unwrap();
        assert!(!next_wait.due());
        assert!(next_wait.expires_at() > first_wait.expires_at());
        if successive {
            let last = expire(
                &repository,
                store,
                r,
                &recovered,
                &next_wait,
                r.seat_ids[1],
                || {},
            );
            assert!(recover().checkpoint().reference() == last.reference());
        }
    }
    if let Some(receipts) = &r.room_receipts {
        for (operation, receipt) in [
            ("ready", &receipts.ready),
            ("force-start", &receipts.countdown),
        ] {
            let path = format!("/v1/rooms/{}/{operation}/disposition", receipts.room);
            let mut original = json!(receipt.original);
            let response = http.admission_receipt(&owner, &path, &original, 200);
            assert_eq!(
                response,
                json!({"status":"success","data":{"state":"accepted",
                "response":{"status":"success","data":receipt.ack}}})
            );
            let foreign = http.admission_receipt(&peer, &path, &original, 200);
            assert_eq!(
                foreign,
                json!({"status":"success","data":{"state":"unknown","response":null}})
            );
            original["expectedRevision"] = json!("987654321");
            assert_eq!(
                http.admission_receipt(&owner, &path, &original, 409),
                json!({"status":"fail","data":{"request":"conflict"}})
            );
        }
    }
    if !successive && !bystander {
        if !second_chooser {
            keep_hand(&http, &peer, r);
        }
        http.concede(&third, r.game_id, r.context_id);
    }
    assert!(
        repository
            .opening_response_wait(r.game_id)
            .unwrap()
            .is_none()
    );
    let finished = recover();
    let terminal = finished.checkpoint().terminal().unwrap().unwrap();
    assert_eq!(
        terminal.reason(),
        if successive || bystander {
            "native_response_timeout"
        } else {
            "native_concession"
        }
    );
    assert_eq!(terminal.winner_seat_ids(), &[r.seat_ids[winner_index]]);
    assert_eq!(terminal.losing_seat_ids().len(), 2);
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    let settle = || {
        lifecycle_reads::sql(
            &database,
            "SELECT app.consume_game_outcome((SELECT id FROM app.game_result WHERE game_id=$1),(SELECT revision FROM app.match WHERE id=$2))",
            &[&r.game_id, &r.match_id],
        )
    };
    let outcome = settle();
    assert_eq!(outcome["status"], "success");
    assert_eq!(settle(), outcome);
    let eliminated: Vec<String> = if successive {
        vec![r.seat_ids[0], r.seat_ids[1]]
    } else {
        vec![r.seat_ids[timeout_index]]
    }
    .into_iter()
    .map(|seat| seat.to_string())
    .collect();
    let result = lifecycle_reads::sql(
        &database,
        "SELECT jsonb_build_object('state',m.state,'winners',mr.winner_seat_ids,'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id=$2),'eliminations',(SELECT count(*) FROM app.command_receipt WHERE scope='native_seat_elimination_v1' AND target_ref=ANY($3::text[]) AND state='accepted')) FROM app.match m JOIN app.match_result mr ON mr.match_id=m.id WHERE m.id=$1",
        &[&r.match_id, &r.game_id, &eliminated],
    );
    assert_eq!(
        result,
        json!({"state":"completed","winners":[r.seat_ids[winner_index]],"gameResults":1,"eliminations":eliminated.len()})
    );
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"opening-response","roomOriginalsRecovered":r.room_receipts.is_some(),"secondChooser":second_chooser,"successiveTimeouts":successive,"bystanderPreservesDeadline":bystander,"entropyPreservesDeadline":entropy,"restoredRemainingTime":restored,"publicDeadlineAndLateReplyVerified":true,"databaseDue":true,"exactRetry":true,"coldHistory":true,"survivorCommand":!successive && !bystander,"matchSettled":true,"result":result})
    );
}

pub(super) fn expire(
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    r: &Reservation,
    opening: &RecoveredNativeGame,
    first_wait: &NativeOpeningResponseWait,
    seat: Uuid,
    before_expiry_commit: impl FnOnce(),
) -> NativeCheckpoint {
    let current = opening.checkpoint();
    assert!(!first_wait.due());
    assert!(first_wait.prepare(current).is_err());
    let started = Instant::now();
    let due = loop {
        let wait = repository
            .opening_response_wait(r.game_id)
            .unwrap()
            .unwrap();
        assert_eq!(wait.expires_at(), first_wait.expires_at());
        if wait.due() {
            break wait;
        }
        assert!(started.elapsed() < Duration::from_secs(135));
        std::thread::sleep(Duration::from_secs(2));
    };
    before_expiry_commit();
    let prepared = due.prepare(current).unwrap();
    let input = store
        .retain(
            r.game_id,
            r.context_id,
            NativeArtifactRole::Input,
            prepared.record(),
        )
        .unwrap();
    let next = current
        .append(store, input, NativeCheckpointInput::ResponseTimeout, seat)
        .unwrap();
    next.retain(store).unwrap();
    next.prepare_event().unwrap().retain(store).unwrap();
    next.prepare_results().unwrap().retain(store).unwrap();
    let request = Uuid::new_v4();
    let original = NativeSystemOriginal::prepare_response_timeout(request, opening, &next).unwrap();
    let commit = || {
        repository
            .commit_opening_response_timeout(store, opening, request, &next, &original)
            .unwrap()
    };
    let accepted = commit();
    let retry = commit();
    assert_eq!(retry.revision, accepted.revision);
    assert_eq!(retry.resource_id, accepted.resource_id);
    next
}

fn keep_original(http: &http::HttpProbe, player: &str, r: &Reservation) -> PlayerCommand {
    let view = http.read(Some(player), r.game_id, r.context_id, 200);
    let d: PlayerDecision =
        serde_json::from_value(view["data"]["snapshot"]["decisions"][0].clone()).unwrap();
    let PlayerDomain::Options { options, .. } = d.domain else {
        panic!("expected survivor's mulligan choice");
    };
    let option = options
        .into_iter()
        .find(|v| v.label == "Keep")
        .unwrap()
        .handle;
    PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: d.basis,
        action: PlayerAction::Choose {
            decision: d.decision,
            response: PlayerChoice::Option { option },
        },
    }
}

pub(super) fn keep_hand(http: &http::HttpProbe, player: &str, r: &Reservation) {
    let keep = keep_original(http, player, r);
    let kept = http.command(Some(player), &keep, 202);
    assert_eq!(http.command(Some(player), &keep, 202), kept);
}
