//! Actual HTTP participant history pages over a real terminal native game.
//! Pages are compared with the same verified walk's consecutive projections,
//! so paging cannot skip, repeat or reorder visible observations.
use super::*;
use crate::player_wire::{PlayerEventFacts, PlayerPublicChange, PlayerVisibleEvent};
use serde_json::Value;

/// Every visible event of one perspective, from the verified original walk.
fn expected_events(
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    reservation: &Reservation,
    actor: &crate::actor::RequestActor,
) -> (Vec<PlayerVisibleEvent>, Value) {
    let NativeHumanAccessOutcome::Granted(access) = repository
        .human_access(actor, reservation.game_id, reservation.context_id)
        .unwrap()
    else {
        panic!("actual retained participant required")
    };
    let history = repository
        .project_human_history(
            store,
            programs,
            &access,
            NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    let vocabulary = history
        .recovered()
        .checkpoint()
        .state()
        .execution()
        .creature_vocabulary();
    let observations = history.observations();
    assert!(observations[0].terminal().is_none());
    let events: Vec<_> = observations
        .windows(2)
        .filter_map(|pair| {
            assert!(pair[0].terminal().is_none());
            PlayerVisibleEvent::from_projections(&PlayerEventFacts {
                before: pair[0].projection().snapshot(),
                before_projection: pair[0].projection().facts(vocabulary).unwrap(),
                after: pair[1].projection().snapshot(),
                after_projection: pair[1].projection().facts(vocabulary).unwrap(),
                terminal: pair[1].terminal(),
            })
            .unwrap()
        })
        .collect();
    let tip = observations.last().unwrap();
    assert!(tip.terminal().is_some());
    (
        events,
        serde_json::to_value(&tip.projection().snapshot().through_event_sequence).unwrap(),
    )
}
fn query(reservation: &Reservation, cursor: &Value, limit: u64) -> Value {
    // The shared contract encodes the limit as a canonical decimal string.
    json!({"gameId":reservation.game_id,"contextId":reservation.context_id,"cursor":cursor,"limit":limit.to_string()})
}
/// Page through all history at one limit; returns events and the final cursor.
fn collect(
    http: &http::HttpProbe,
    token: &str,
    reservation: &Reservation,
    limit: u64,
) -> (Vec<Value>, Value) {
    let mut cursor = Value::Null;
    let mut events = Vec::new();
    for _ in 0..1_000 {
        let page = http.history(
            Some(token),
            reservation.game_id,
            &query(reservation, &cursor, limit),
            200,
        );
        let data = &page["data"];
        assert_eq!(data["query"], query(reservation, &cursor, limit));
        let batch = data["events"].as_array().unwrap();
        assert!(batch.len() <= usize::try_from(limit).unwrap());
        events.extend(batch.iter().cloned());
        // Every page makes real progress; scannedThrough is an issued cursor.
        assert_ne!(data["scannedThrough"], cursor);
        cursor = data["scannedThrough"].clone();
        if data["hasMore"] == json!(false) {
            return (events, cursor);
        }
        assert_eq!(data["hasMore"], json!(true));
    }
    panic!("history paging did not terminate")
}
pub(super) fn failure(stage: &str, error: &dyn std::error::Error) -> String {
    let mut text = format!("{stage}: {error}");
    let mut next = error.source();
    while let Some(cause) = next {
        text.push_str(&format!(" <- {cause}"));
        next = cause.source();
    }
    text
}
/// Direct repository composition in labelled stages, before the HTTP boundary.
fn direct(
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    reservation: &Reservation,
    actor: &crate::actor::RequestActor,
) {
    let NativeHumanAccessOutcome::Granted(access) = repository
        .human_access(actor, reservation.game_id, reservation.context_id)
        .unwrap()
    else {
        panic!("actual retained participant required")
    };
    let history = repository
        .project_human_history(
            store,
            programs,
            &access,
            NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    let at = history
        .observations()
        .last()
        .unwrap()
        .projection()
        .snapshot()
        .through_event_sequence
        .clone();
    let walked = 100_000_000 - history.remaining_budget().remaining_bytes();
    eprintln!(
        "HISTORY_WALK_BYTES {walked} cuts {}",
        history.observations().len()
    );
    // The runtime template is one recovery's bound; a page walk must fit it too.
    let template = NativeRecoveryBudget::new(10_000, 10_000_000).unwrap();
    if let Err(e) = repository.project_human_history(store, programs, &access, template) {
        panic!(
            "{}",
            failure(
                &format!("walk within runtime template ({walked} bytes used at 100MB)"),
                &e
            )
        );
    }
    let mut budget = history.remaining_budget();
    let scan = repository
        .read_human_archive(&history, &at, None, 1, &mut budget)
        .unwrap_or_else(|e| panic!("{}", failure("baseline scan", &e)));
    assert_eq!(scan.item_count(), 1);
    assert!(scan.has_more());
    let issued = repository
        .issue_human_history_cursor(&scan, &mut budget)
        .unwrap_or_else(|e| panic!("{}", failure("baseline cursor", &e)));
    issued
        .validate_human_history(&access)
        .unwrap_or_else(|e| panic!("{}", failure("baseline cursor scope", &e)));
    let read = repository
        .read_verified_history_cursor(&history, &issued.cursor())
        .unwrap_or_else(|e| panic!("{}", failure("cursor read", &e)))
        .expect("issued history cursor must resolve");
    let next = repository
        .read_human_archive(
            &history,
            &at,
            Some(&read.cursor().through_event_sequence),
            1,
            &mut budget,
        )
        .unwrap_or_else(|e| panic!("{}", failure("second scan", &e)));
    repository
        .issue_human_history_cursor(&next, &mut budget)
        .unwrap_or_else(|e| panic!("{}", failure("second cursor", &e)));
    // The exact runtime wrapper the HTTP route calls, built like the HTTP probe.
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let runtime = NativeGameRuntime::from_env(&base, &key).unwrap().unwrap();
    let query_one: crate::player_wire::PlayerHistoryQuery =
        serde_json::from_value(query(reservation, &Value::Null, 1)).unwrap();
    match runtime
        .history_page(repository, actor, &query_one)
        .unwrap_or_else(|e| panic!("{}", failure("runtime history page", &e)))
    {
        crate::native_game::NativeReadOutcome::Ready(
            crate::native_game::NativeHistoryRead::Page(_),
        ) => {}
        crate::native_game::NativeReadOutcome::Ready(_) => panic!("runtime cursor refusal"),
        crate::native_game::NativeReadOutcome::Refused(_) => panic!("runtime access refusal"),
    }
    // Page through with issued cursors exactly as HTTP does, labelling each page.
    let mut cursor = Value::Null;
    for page_index in 0..1_000 {
        let page_query: crate::player_wire::PlayerHistoryQuery =
            serde_json::from_value(query(reservation, &cursor, 1)).unwrap();
        let previous = page_query.cursor.as_ref().map(|requested| {
            repository
                .read_verified_history_cursor(&history, requested)
                .unwrap_or_else(|e| {
                    panic!("{}", failure(&format!("page {page_index} cursor read"), &e))
                })
                .unwrap_or_else(|| panic!("page {page_index} cursor unavailable"))
        });
        let mut budget = history.remaining_budget();
        let page = repository
            .compose_human_history_page(
                &access,
                &history,
                &page_query,
                previous.as_ref(),
                &mut budget,
            )
            .unwrap_or_else(|e| {
                panic!(
                    "{}",
                    failure(&format!("compose page {page_index} after {cursor}"), &e)
                )
            });
        cursor = serde_json::to_value(&page.scanned_through).unwrap();
        if !page.has_more {
            break;
        }
    }
    for limit in [1_u64, 100] {
        let query: crate::player_wire::PlayerHistoryQuery =
            serde_json::from_value(query(reservation, &Value::Null, limit)).unwrap();
        let mut budget = history.remaining_budget();
        repository
            .compose_human_history_page(&access, &history, &query, None, &mut budget)
            .unwrap_or_else(|e| panic!("{}", failure(&format!("compose limit {limit}"), &e)));
    }
}
/// Returns the owner's visible event count.
pub(super) fn run(
    http: &http::HttpProbe,
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    reservation: &Reservation,
    tokens: &[String; 2],
    actors: &[crate::actor::RequestActor; 2],
) -> usize {
    let outsider = std::env::var("SPELLWAKE_NATIVE_SOURCE_OUTSIDER_BEARER").unwrap();
    for actor in actors {
        direct(repository, store, programs, reservation, actor);
    }
    let mut finals = Vec::new();
    let mut owner_events = 0;
    for (index, token) in tokens.iter().enumerate() {
        let (expected, tip) =
            expected_events(repository, store, programs, reservation, &actors[index]);
        let expected: Vec<Value> = expected
            .iter()
            .map(|event| serde_json::to_value(event).unwrap())
            .collect();
        assert!(!expected.is_empty());
        assert!(
            expected
                .iter()
                .all(|event| event["viewerSeatId"] == json!(reservation.seat_ids[index]))
        );
        let last = expected.last().unwrap();
        assert_eq!(last["eventSequence"], tip);
        assert!(
            serde_json::from_value::<PlayerVisibleEvent>(last.clone())
                .unwrap()
                .changes
                .iter()
                .any(|change| matches!(change, PlayerPublicChange::Terminal { .. }))
        );
        let mut cursors = Vec::new();
        for limit in [1, 2, 100] {
            let (events, cursor) = collect(http, token, reservation, limit);
            assert_eq!(
                events, expected,
                "page size {limit} changed visible history"
            );
            assert_eq!(cursor["throughEventSequence"], tip);
            assert_eq!(cursor["viewerSeatId"], json!(reservation.seat_ids[index]));
            cursors.push(cursor);
        }
        // SQL cursor identity is positional: page size cannot change the final token.
        assert!(cursors.windows(2).all(|pair| pair[0] == pair[1]));
        let first = query(reservation, &Value::Null, 2);
        assert_eq!(
            http.history(Some(token), reservation.game_id, &first, 200),
            http.history(Some(token), reservation.game_id, &first, 200)
        );
        let caught_up = http.history(
            Some(token),
            reservation.game_id,
            &query(reservation, &cursors[0], 5),
            200,
        );
        assert_eq!(caught_up["data"]["events"], json!([]));
        assert_eq!(caught_up["data"]["hasMore"], json!(false));
        assert_eq!(caught_up["data"]["scannedThrough"], cursors[0]);
        if index == 0 {
            owner_events = expected.len();
        }
        finals.push(cursors.swap_remove(0));
    }
    let owner = &tokens[0];
    let game = reservation.game_id;
    let fresh = query(reservation, &Value::Null, 5);
    // Another participant's issued cursor is foreign, never a reset.
    let foreign = http.history(Some(owner), game, &query(reservation, &finals[1], 5), 409);
    assert_eq!(foreign["data"]["cursor"], "unavailable");
    let mut forged = finals[0].clone();
    // Canonical grammar, never issued by SQL.
    forged["token"] = json!(format!("n1.{}", Uuid::new_v4()));
    let forged = http.history(Some(owner), game, &query(reservation, &forged, 5), 409);
    assert_eq!(forged["data"]["cursor"], "unavailable");
    // A snapshot-purpose cursor from reconnect is not a history position.
    let reconnect = http.reconnect(
        Some(owner),
        game,
        &json!({"receipts":{"gameId":game,"contextId":reservation.context_id,"outstanding":[]},"cursor":null}),
        200,
    );
    let snapshot = &reconnect["data"]["cursor"];
    assert_eq!(
        snapshot["throughEventSequence"],
        finals[0]["throughEventSequence"]
    );
    let wrong_purpose = http.history(Some(owner), game, &query(reservation, snapshot, 5), 409);
    assert_eq!(wrong_purpose["data"]["cursor"], "unavailable");
    // Authentication, participation and scope.
    assert_eq!(
        http.history(None, game, &fresh, 401)["data"]["session"],
        "unauthorized"
    );
    assert_eq!(
        http.history(Some(&outsider), game, &fresh, 404)["data"]["game"],
        "unavailable"
    );
    let mut other_context = fresh.clone();
    other_context["contextId"] = json!(Uuid::new_v4());
    assert_eq!(
        http.history(Some(owner), game, &other_context, 404)["data"]["game"],
        "unavailable"
    );
    // Strict request bodies.
    for body in [
        query(reservation, &Value::Null, 0),
        query(reservation, &Value::Null, 101),
        json!({"gameId":game,"contextId":reservation.context_id,"limit":"5"}),
        json!({"gameId":game,"contextId":reservation.context_id,"cursor":null,"limit":5}),
        json!({"gameId":game,"contextId":reservation.context_id,"cursor":null,"limit":"05"}),
        json!({"gameId":game,"contextId":reservation.context_id,"cursor":null,"limit":"5","extra":1}),
        json!({"gameId":game,"contextId":reservation.context_id,"cursor":[],"limit":"5"}),
        json!({"gameId":Uuid::new_v4(),"contextId":reservation.context_id,"cursor":null,"limit":"5"}),
    ] {
        http.history(Some(owner), game, &body, 400);
    }
    owner_events
}
