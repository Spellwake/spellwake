//! Actual HTTP participant replay over a real terminal native game: first
//! request builds and canonically publishes the portable archive; later frames
//! are read from it. Expected values come from fresh verified original walks.
use super::*;
use crate::player_wire::{PlayerEventFacts, PlayerVisibleEvent};
use serde_json::Value;

fn walk(
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    access: &NativeHumanAccess,
    seat: Uuid,
    full: bool,
) -> NativeHumanHistory {
    let budget = NativeRecoveryBudget::new(10_000, 100_000_000).unwrap();
    if full {
        repository.project_completed_history(store, programs, access, seat, budget)
    } else {
        repository.project_retained_seat_history(store, programs, access, seat, budget)
    }
    .unwrap()
    .unwrap()
}
/// Visible events of the walk up to and including `at`.
pub(super) fn events_through(history: &NativeHumanHistory, at: usize) -> Vec<Value> {
    let vocabulary = history
        .recovered()
        .checkpoint()
        .state()
        .execution()
        .creature_vocabulary();
    history.observations()[..=at]
        .windows(2)
        .filter_map(|pair| {
            PlayerVisibleEvent::from_projections(&PlayerEventFacts {
                before: pair[0].projection().snapshot(),
                before_projection: pair[0].projection().facts(vocabulary).unwrap(),
                after: pair[1].projection().snapshot(),
                after_projection: pair[1].projection().facts(vocabulary).unwrap(),
                terminal: pair[1].terminal(),
            })
            .unwrap()
        })
        .map(|event| serde_json::to_value(event).unwrap())
        .collect()
}
pub(super) fn request(
    reservation: &Reservation,
    at: &Value,
    seat: Uuid,
    mode: &str,
    cursor: &Value,
    limit: u64,
) -> Value {
    json!({"gameId":reservation.game_id,"contextId":reservation.context_id,"atEventSequence":at,
        "perspectiveSeatId":seat,"mode":mode,
        "history":{"gameId":reservation.game_id,"contextId":reservation.context_id,"cursor":cursor,"limit":limit.to_string()}})
}
pub(super) fn sequence(history: &NativeHumanHistory, index: usize) -> Value {
    serde_json::to_value(
        &history.observations()[index]
            .projection()
            .snapshot()
            .through_event_sequence,
    )
    .unwrap()
}
/// Returns the number of frames checked against expected original walks and
/// the measured publication walk budget.
pub(super) fn run(
    http: &http::HttpProbe,
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    reservation: &Reservation,
    tokens: &[String; 2],
    actors: &[crate::actor::RequestActor; 2],
) -> (usize, Value) {
    let outsider = std::env::var("SPELLWAKE_NATIVE_SOURCE_OUTSIDER_BEARER").unwrap();
    let game = reservation.game_id;
    let access = |index: usize| {
        let NativeHumanAccessOutcome::Granted(access) = repository
            .human_access(&actors[index], game, reservation.context_id)
            .unwrap()
        else {
            panic!("actual retained participant required")
        };
        access
    };
    assert!(
        access(0).replay().0.is_none(),
        "no publication before replay"
    );
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let runtime = NativeGameRuntime::from_env(&base, &key).unwrap().unwrap();
    // Direct stages first, so a failure names its step before the HTTP path.
    {
        use super::history_pages::failure;
        let template = NativeRecoveryBudget::new(10_000, 10_000_000).unwrap();
        // Two concurrent builders converge on one canonical publication; a later
        // identical retry (as after a lost response) returns the same manifest.
        let owner_access = access(0);
        let publish = || {
            repository
                .publish_portable_replay(store, programs, &owner_access, &template)
                .unwrap_or_else(|e| panic!("{}", failure("publish", &e)))
                .expect("completed game must publish")
        };
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(publish);
            let second = scope.spawn(publish);
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!(first, second, "concurrent builders must converge");
        let published = publish();
        assert_eq!(
            published, first,
            "retry must return the original publication"
        );
        eprintln!("REPLAY_PUBLISHED");
        assert_eq!(access(0).replay().0, Some(published));
        for seat in &reservation.seat_ids {
            for disclosure in [
                NativeDisclosure::LiveParticipant,
                NativeDisclosure::CompletedParticipant,
            ] {
                let mut budget = template.clone();
                repository
                    .read_portable_replay(
                        store,
                        crate::native_game::source_reader::replay_archive::ReplayTarget {
                            game,
                            context: reservation.context_id,
                            manifest: published,
                        },
                        *seat,
                        disclosure,
                        None,
                        &mut budget,
                    )
                    .unwrap_or_else(|e| panic!("{}", failure("read archive", &e)))
                    .expect("retained seat");
            }
        }
        let target = |manifest| crate::native_game::source_reader::replay_archive::ReplayTarget {
            game,
            context: reservation.context_id,
            manifest,
        };
        let mut budget = template.clone();
        // Unknown manifest and foreign context are refused, never substituted.
        assert!(
            repository
                .read_portable_replay(
                    store,
                    target(Uuid::new_v4()),
                    reservation.seat_ids[0],
                    NativeDisclosure::LiveParticipant,
                    None,
                    &mut budget,
                )
                .is_err()
        );
        let mut foreign = target(published);
        foreign.context = Uuid::new_v4();
        assert!(
            repository
                .read_portable_replay(
                    store,
                    foreign,
                    reservation.seat_ids[0],
                    NativeDisclosure::LiveParticipant,
                    None,
                    &mut budget,
                )
                .is_err()
        );
        // A seat that is not in the archive is absent, not an error.
        assert!(
            repository
                .read_portable_replay(
                    store,
                    target(published),
                    Uuid::new_v4(),
                    NativeDisclosure::CompletedParticipant,
                    None,
                    &mut budget,
                )
                .unwrap()
                .is_none()
        );
        // An exhausted budget refuses before reading the archive.
        let mut empty = NativeRecoveryBudget::new(1, 1).unwrap();
        assert!(
            repository
                .read_portable_replay(
                    store,
                    target(published),
                    reservation.seat_ids[0],
                    NativeDisclosure::LiveParticipant,
                    None,
                    &mut empty,
                )
                .is_err()
        );
        eprintln!("REPLAY_ARCHIVE_READ");
        let direct: crate::player_wire::PlayerReplayRequest = serde_json::from_value(request(
            reservation,
            &json!("0"),
            reservation.seat_ids[0],
            "historical-seat",
            &Value::Null,
            5,
        ))
        .unwrap();
        match runtime
            .replay_frame(repository, &actors[0], &direct)
            .unwrap_or_else(|e| panic!("{}", failure("runtime replay", &e)))
        {
            crate::native_game::NativeReadOutcome::Ready(
                crate::native_game::NativeReplayRead::Frame(_),
            ) => {}
            crate::native_game::NativeReadOutcome::Ready(_) => panic!("runtime replay refusal"),
            crate::native_game::NativeReadOutcome::Refused(_) => panic!("runtime access refusal"),
        }
        eprintln!("REPLAY_RUNTIME_FRAME");
    }
    let release = repository.read(game).unwrap().unwrap().context_ids().1;
    let [owner_seat, peer_seat] = [reservation.seat_ids[0], reservation.seat_ids[1]];
    let mut frames = 0;
    let mut manifest = None;
    let started = std::time::Instant::now();
    // (requester, perspective, mode) cases that must succeed.
    for (requester, seat, full) in [
        (0, owner_seat, false),
        (0, peer_seat, true),
        (0, owner_seat, true),
        (1, peer_seat, false),
    ] {
        let expected = walk(repository, store, programs, &access(requester), seat, full);
        let last = expected.observations().len() - 1;
        let mode = if full {
            "completed-full-information"
        } else {
            "historical-seat"
        };
        for at in [0, last / 2, last] {
            let body = request(
                reservation,
                &sequence(&expected, at),
                seat,
                mode,
                &Value::Null,
                100,
            );
            let frame = http.replay(Some(&tokens[requester]), game, &body, 200);
            let data = &frame["data"];
            assert_eq!(data["request"], body);
            assert_eq!(data["releaseId"], json!(release));
            assert_eq!(data["schemaRevision"], "spellwake-replay-v2");
            assert_eq!(
                data["snapshot"],
                serde_json::to_value(expected.observations()[at].projection().snapshot()).unwrap()
            );
            // The response bound may truncate the page; it must then be an
            // exact prefix with truthful progress and hasMore.
            let events = events_through(&expected, at);
            let page = data["history"]["events"].as_array().unwrap();
            assert!(page.len() <= events.len() && page[..] == events[..page.len()]);
            assert!(!page.is_empty() || events.is_empty());
            assert!(serde_json::to_vec(&frame).unwrap().len() <= 262_144);
            if page.len() == events.len() {
                assert_eq!(data["history"]["hasMore"], json!(false));
                assert_eq!(
                    data["history"]["scannedThrough"]["throughEventSequence"],
                    sequence(&expected, at)
                );
            } else {
                assert_eq!(data["history"]["hasMore"], json!(true));
            }
            if manifest.is_none() {
                manifest = access(0).replay().0;
                assert!(manifest.is_some(), "replay must read a published archive");
                // A repeated request reads the same publication, unchanged.
                assert_eq!(
                    http.replay(Some(&tokens[requester]), game, &body, 200),
                    frame
                );
            }
            frames += 1;
        }
        eprintln!(
            "REPLAY_CASE_FRAMES_DONE seat_full={full} elapsed={:?}",
            started.elapsed()
        );
        if requester != 0 || full {
            continue;
        }
        // Page the owner's own-seat history at the final coordinate.
        let mut cursor = Value::Null;
        let mut paged = Vec::new();
        for _ in 0..1_000 {
            let body = request(
                reservation,
                &sequence(&expected, last),
                seat,
                mode,
                &cursor,
                1,
            );
            let frame = http.replay(Some(&tokens[requester]), game, &body, 200);
            let history = &frame["data"]["history"];
            paged.extend(history["events"].as_array().unwrap().iter().cloned());
            assert_ne!(history["scannedThrough"], cursor);
            cursor = history["scannedThrough"].clone();
            if history["hasMore"] == json!(false) {
                break;
            }
        }
        assert_eq!(paged, events_through(&expected, last));
        // A cursor past the requested coordinate is unavailable, never truncated.
        if last > 0 {
            let body = request(reservation, &sequence(&expected, 0), seat, mode, &cursor, 1);
            assert_eq!(
                http.replay(Some(&tokens[requester]), game, &body, 409)["data"]["cursor"],
                "unavailable"
            );
        }
    }
    assert_eq!(access(0).replay().0, manifest);
    assert_eq!(access(1).replay().0, manifest);

    // Reader independence: the archive reader takes no program source and
    // does not recover the native game.
    let full_peer = walk(repository, store, programs, &access(0), peer_seat, true);
    // Measured work of one publication walk on this real game, against the
    // runtime's configured per-walk recovery template.
    let (left_records, left_bytes) = full_peer.remaining_budget().remaining();
    let configured = |name: &str| -> u64 { std::env::var(name).unwrap().parse().unwrap() };
    let (walk_records, walk_bytes) = (10_000 - left_records, 100_000_000 - left_bytes);
    let mut budget_metrics = json!({
        "cuts": full_peer.observations().len(),
        "walkRecords": walk_records,
        "walkBytes": walk_bytes,
        "configuredRecords": configured("SPELLWAKE_NATIVE_RECOVERY_MAX_RECORDS"),
        "configuredBytes": configured("SPELLWAKE_NATIVE_RECOVERY_MAX_BYTES"),
        "plannedWalks": 2 * reservation.seat_ids.len(),
    });
    eprintln!("REPLAY_BUDGET {budget_metrics}");
    let mut budget = NativeRecoveryBudget::new(10_000, 100_000_000).unwrap();
    let archive = repository
        .read_portable_replay(
            store,
            crate::native_game::source_reader::replay_archive::ReplayTarget {
                game,
                context: reservation.context_id,
                manifest: manifest.unwrap(),
            },
            peer_seat,
            NativeDisclosure::CompletedParticipant,
            None,
            &mut budget,
        )
        .unwrap()
        .unwrap();
    // A coordinate-bounded read loads only the pages through that cut.
    let prefix = repository
        .read_portable_replay(
            store,
            crate::native_game::source_reader::replay_archive::ReplayTarget {
                game,
                context: reservation.context_id,
                manifest: manifest.unwrap(),
            },
            peer_seat,
            NativeDisclosure::CompletedParticipant,
            Some(&Amount::from(0)),
            &mut NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    let loaded = prefix.cut_source().cuts.len();
    assert!(loaded >= 1 && loaded <= 64.min(full_peer.observations().len()));
    let source = archive.cut_source();
    budget_metrics["archiveControls"] =
        super::archive_integrity::run(repository, store, reservation, manifest.unwrap());
    assert_eq!(source.cuts.len(), full_peer.observations().len());
    for (archived, original) in source.cuts.iter().zip(full_peer.observations()) {
        use super::super::cuts::HistoryCut;
        assert!(archived.snapshot() == original.projection().snapshot());
        assert_eq!(archived.view_receipt(), original.receipt_id());
    }

    // Refusals.
    let tip = sequence(&full_peer, full_peer.observations().len() - 1);
    let owner = &tokens[0];
    let own = request(
        reservation,
        &tip,
        owner_seat,
        "historical-seat",
        &Value::Null,
        5,
    );
    let other_seat = request(
        reservation,
        &tip,
        peer_seat,
        "historical-seat",
        &Value::Null,
        5,
    );
    assert_eq!(
        http.replay(Some(owner), game, &other_seat, 409)["data"]["mode"],
        "unavailable"
    );
    let mut stranger = own.clone();
    stranger["perspectiveSeatId"] = json!(Uuid::new_v4());
    stranger["mode"] = json!("completed-full-information");
    assert_eq!(
        http.replay(Some(owner), game, &stranger, 409)["data"]["mode"],
        "unavailable"
    );
    let mut future = own.clone();
    future["atEventSequence"] = json!("1000000");
    assert_eq!(
        http.replay(Some(owner), game, &future, 409)["data"]["atEventSequence"],
        "unavailable"
    );
    // A live history-route cursor is the wrong disclosure for full replay.
    let live = http.history(
        Some(owner),
        game,
        &json!({"gameId":game,"contextId":reservation.context_id,"cursor":null,"limit":"1"}),
        200,
    );
    let mut wrong_disclosure = request(
        reservation,
        &tip,
        owner_seat,
        "completed-full-information",
        &live["data"]["scannedThrough"],
        5,
    );
    assert_eq!(
        http.replay(Some(owner), game, &wrong_disclosure, 409)["data"]["cursor"],
        "unavailable"
    );
    wrong_disclosure["history"]["cursor"]["token"] = json!(format!("n1.{}", Uuid::new_v4()));
    assert_eq!(
        http.replay(Some(owner), game, &wrong_disclosure, 409)["data"]["cursor"],
        "unavailable"
    );
    assert_eq!(
        http.replay(None, game, &own, 401)["data"]["session"],
        "unauthorized"
    );
    assert_eq!(
        http.replay(Some(&outsider), game, &own, 404)["data"]["game"],
        "unavailable"
    );
    // A server whose program source differs cannot recover this game, yet
    // still serves the identical replay frame from the published archive.
    let other_pool = http::HttpProbe::configure(&base, &key, |_| {
        NativeGameRuntime::with_pool(&base, &key, "resolution-selection").unwrap()
    });
    let view_status = other_pool.status(
        Some(owner),
        &format!("/v1/games/{game}/view"),
        &json!({"gameId":game,"contextId":reservation.context_id}),
    );
    assert_ne!(view_status, 200, "other runtime must not recover the game");
    assert_eq!(
        other_pool.replay(Some(owner), game, &own, 200),
        http.replay(Some(owner), game, &own, 200)
    );
    eprintln!("REPLAY_OTHER_RUNTIME view_status={view_status}");
    // Too small a recovery budget refuses with the safe 503, never a partial frame.
    let starved = http::HttpProbe::configure(&base, &key, |runtime| {
        runtime.with_test_recovery(NativeRecoveryBudget::new(1, 1).unwrap())
    });
    assert_eq!(
        starved.replay(Some(owner), game, &own, 503)["code"],
        "unavailable"
    );
    let mut bad_mode = own.clone();
    bad_mode["mode"] = json!("spectator");
    let mut numeric = own.clone();
    numeric["atEventSequence"] = json!(0);
    let mut mismatched = own.clone();
    mismatched["history"]["gameId"] = json!(Uuid::new_v4());
    let mut extra = own.clone();
    extra["extra"] = json!(1);
    for body in [bad_mode, numeric, mismatched, extra] {
        http.replay(Some(owner), game, &body, 400);
    }
    (frames, budget_metrics)
}
