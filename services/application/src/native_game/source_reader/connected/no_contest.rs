//! Actual operational no-contest: the hosted game is paused on a real incident,
//! the match is finalized as unrecoverable, then each participant's own-seat
//! prefix replay is served from the canonically published archive.
use super::*;
use crate::provider_json::ObjectResponse;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct MatchRow {
    revision: String,
    availability_revision: String,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    incident_receipt_id: Option<Uuid>,
    availability: String,
}
fn request(reservation: &Reservation, at: &Value, seat: Uuid, mode: &str) -> Value {
    json!({"gameId":reservation.game_id,"contextId":reservation.context_id,"atEventSequence":at,
        "perspectiveSeatId":seat,"mode":mode,
        "history":{"gameId":reservation.game_id,"contextId":reservation.context_id,"cursor":null,"limit":"100"}})
}
// Replay reserves space for its snapshot; the same cursor and visible-event
// limit can therefore yield a shorter page than the standalone history route.
// Check each route against the independently reconstructed exact event interval.
fn assert_interval(page: &Value, after: &Value, tip: &Value, events: &[Value]) {
    let sequence = |value: &Value| value.as_str().unwrap().parse::<u64>().unwrap();
    let start = sequence(&after["throughEventSequence"]);
    let end = sequence(&page["scannedThrough"]["throughEventSequence"]);
    assert!(start < end && end <= sequence(tip));
    let expected: Vec<_> = events
        .iter()
        .filter(|event| {
            let at = sequence(&event["eventSequence"]);
            start < at && at <= end
        })
        .collect();
    let actual: Vec<_> = page["events"].as_array().unwrap().iter().collect();
    assert!(
        actual == expected,
        "exact visible events in the scanned interval"
    );
    assert_eq!(page["hasMore"], json!(end < sequence(tip)));
}
/// Produce the genuine operational prefix before any replay reader publishes.
pub(super) fn terminalize(repository: &NativeSourceRepository, reservation: &Reservation) {
    let game = reservation.game_id;
    // 1. Real incident pause of the hosted game at its current cut.
    let source = repository.read(game).unwrap().unwrap();
    let (owner, epoch, revision, _, _) = source.operational_basis();
    let at = source.clocks().map(|row| row.4).max().unwrap();
    let ObjectResponse(ApiResponse::Success { .. }): ObjectResponse<Value> = repository
        .backend
        .rpc(
            "pause_game_incident",
            &json!({"p_game":game,"p_owner":owner,"p_epoch":epoch,"p_operational_revision":revision,
                "p_incident_at":at,"p_kind":"storage","p_proof":"fixture:owned-no-contest-incident",
                "p_request":Uuid::new_v4()}),
        )
        .unwrap()
    else {
        panic!("actual incident pause required");
    };
    // 2. Finalize the recovering match as unrecoverable at its exact basis.
    let bytes = repository
        .backend
        .select_bytes_bounded(
            "match",
            &[
                (
                    "select",
                    "revision::text,availability_revision::text,incident_receipt_id,availability"
                        .to_owned(),
                ),
                ("id", format!("eq.{}", source.match_id())),
            ],
            4_096,
        )
        .unwrap();
    let [row]: [MatchRow; 1] = serde_json::from_slice::<Vec<MatchRow>>(&bytes)
        .unwrap()
        .try_into()
        .ok()
        .unwrap();
    assert_eq!(row.availability, "recovering");
    let incident = row.incident_receipt_id.expect("incident receipt");
    let ObjectResponse(ApiResponse::Success { .. }): ObjectResponse<Value> = repository
        .backend
        .rpc(
            "finalize_unrecoverable_match",
            &json!({"p_match":source.match_id(),"p_expected":row.revision,
                "p_availability":row.availability_revision,"p_incident":incident,
                "p_failure_ref":"fixture:owned-no-contest-unrecoverable"}),
        )
        .unwrap()
    else {
        panic!("actual unrecoverable terminalization required");
    };
    let terminal = repository.read(game).unwrap().unwrap();
    assert_eq!(
        terminal.phases().0,
        crate::match_history::GamePhase::NoContest
    );
    assert_eq!(terminal.last_event_sequence(), source.last_event_sequence());
}
/// Returns the stage metrics for the qualifier.
pub(super) fn run(
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    reservation: &Reservation,
    tokens: &[String; 2],
    actors: &[crate::actor::RequestActor; 2],
) -> Value {
    let game = reservation.game_id;
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    terminalize(repository, reservation);
    let access = |index: usize| {
        let NativeHumanAccessOutcome::Granted(access) = repository
            .human_access(&actors[index], game, reservation.context_id)
            .unwrap()
        else {
            panic!("actual retained participant required")
        };
        access
    };
    assert!(access(0).operational_no_contest());
    assert!(access(0).completed_disclosure().is_none());
    assert_eq!(access(0).replay(), (None, false));
    // No native conclusion exists, so completion authority must be refused.
    assert!(
        repository
            .project_completed_history(
                store,
                programs,
                &access(0),
                reservation.seat_ids[1],
                NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
            )
            .is_err()
    );

    let http = http::HttpProbe::new(&base, &key);
    let mut frames = 0;
    for (index, token) in tokens.iter().enumerate() {
        let seat = reservation.seat_ids[index];
        let expected = repository
            .project_human_history(
                store,
                programs,
                &access(index),
                NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
            )
            .unwrap()
            .unwrap();
        let observations = expected.observations();
        let last = observations.len() - 1;
        assert!(observations.iter().all(|cut| cut.terminal().is_none()));
        for position in [0, last / 2, last] {
            let at = serde_json::to_value(
                &observations[position]
                    .projection()
                    .snapshot()
                    .through_event_sequence,
            )
            .unwrap();
            let body = request(reservation, &at, seat, "historical-seat");
            let frame = http.replay(Some(token), game, &body, 200);
            assert_eq!(frame["data"]["schemaRevision"], "spellwake-replay-v2");
            assert_eq!(
                frame["data"]["snapshot"],
                serde_json::to_value(observations[position].projection().snapshot()).unwrap()
            );
            frames += 1;
        }
        // The tip frame's history is the exact own-seat event sequence, and
        // paging resumes through issued cursors without gaps.
        let tip_at = super::replay_frames::sequence(&expected, last);
        let events = super::replay_frames::events_through(&expected, last);
        let mut cursor = Value::Null;
        let mut seen = Vec::new();
        let mut earlier = None;
        for _ in 0..=events.len() {
            let body = super::replay_frames::request(
                reservation,
                &tip_at,
                seat,
                "historical-seat",
                &cursor,
                3,
            );
            let data = http.replay(Some(token), game, &body, 200)["data"].clone();
            seen.extend(
                data["history"]["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
            cursor = data["history"]["scannedThrough"].clone();
            assert!(!cursor.is_null(), "replay must issue a real cursor");
            if earlier.is_none() && data["history"]["hasMore"] == json!(true) {
                earlier = Some((cursor.clone(), seen.len()));
            }
            frames += 1;
            if data["history"]["hasMore"] == json!(false) {
                break;
            }
        }
        assert_eq!(seen, events);
        assert_eq!(cursor["throughEventSequence"], tip_at);
        assert_ne!(tip_at, json!("0"), "incident must retain an actual prefix");
        let before_tip = super::replay_frames::request(
            reservation,
            &json!("0"),
            seat,
            "historical-seat",
            &cursor,
            3,
        );
        assert_eq!(
            http.replay(Some(token), game, &before_tip, 409)["data"]["cursor"],
            "unavailable"
        );

        // An issued earlier replay cursor also resumes the original own-seat
        // history route. Compare raw scan coordinates independently of the
        // number of visible events, since private cuts may be silent.
        let (earlier, consumed) = earlier.expect("actual prefix must span multiple pages");
        let mut resumed = earlier.clone();
        let mut remaining = Vec::new();
        for _ in 0..=events.len() {
            // The preceding loop covers three-event paging; use a bounded wide
            // page for exact cross-route continuation from the earlier cursor.
            let replay = super::replay_frames::request(
                reservation,
                &tip_at,
                seat,
                "historical-seat",
                &resumed,
                100,
            );
            let replay_data = http.replay(Some(token), game, &replay, 200)["data"].clone();
            let history = http.history(Some(token), game, &replay["history"], 200);
            assert_interval(&history["data"], &resumed, &tip_at, &events);
            assert_interval(&replay_data["history"], &resumed, &tip_at, &events);
            remaining.extend(
                history["data"]["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
            let next = history["data"]["scannedThrough"].clone();
            assert_ne!(next, resumed, "resumed page must advance");
            resumed = next;
            frames += 1;
            if history["data"]["hasMore"] == json!(false) {
                break;
            }
        }
        assert_eq!(remaining, events[consumed..]);
        assert_eq!(resumed["throughEventSequence"], tip_at);

        let positive = super::replay_frames::request(
            reservation,
            &tip_at,
            seat,
            "historical-seat",
            &earlier,
            3,
        );
        let restored = http.replay(Some(token), game, &positive, 200);
        let restored_history = http.history(Some(token), game, &positive["history"], 200);
        // Another participant's issued cursor is foreign to that seat. An
        // unissued token with valid grammar is also refused, never reset.
        let mut foreign = positive.clone();
        foreign["perspectiveSeatId"] = json!(reservation.seat_ids[1 - index]);
        assert_eq!(
            http.replay(Some(&tokens[1 - index]), game, &foreign, 409)["data"]["cursor"],
            "unavailable"
        );
        assert_eq!(
            http.history(Some(&tokens[1 - index]), game, &foreign["history"], 409)["data"]["cursor"],
            "unavailable"
        );
        let mut unknown = positive.clone();
        unknown["history"]["cursor"]["token"] = json!(format!("n1.{}", Uuid::new_v4()));
        assert_eq!(
            http.replay(Some(token), game, &unknown, 409)["data"]["cursor"],
            "unavailable"
        );
        assert_eq!(
            http.history(Some(token), game, &unknown["history"], 409)["data"]["cursor"],
            "unavailable"
        );
        assert_eq!(http.replay(Some(token), game, &positive, 200), restored);
        assert_eq!(
            http.history(Some(token), game, &positive["history"], 200),
            restored_history
        );
        // The live history route serves the same own-seat events.
        let live = http.history(
            Some(token),
            game,
            &json!({"gameId":game,"contextId":reservation.context_id,"cursor":null,"limit":"100"}),
            200,
        );
        let live_events = live["data"]["events"].as_array().unwrap();
        assert!(
            live_events.len() <= events.len() && live_events[..] == events[..live_events.len()]
        );
        // Full reveal and another seat stay refused for an operational prefix.
        let tip = serde_json::to_value(
            &observations[last]
                .projection()
                .snapshot()
                .through_event_sequence,
        )
        .unwrap();
        let other = reservation.seat_ids[1 - index];
        for body in [
            request(reservation, &tip, seat, "completed-full-information"),
            request(reservation, &tip, other, "completed-full-information"),
            request(reservation, &tip, other, "historical-seat"),
        ] {
            assert_eq!(
                http.replay(Some(token), game, &body, 409)["data"]["mode"],
                "unavailable"
            );
        }
    }
    let manifest = access(0).replay();
    assert!(manifest.0.is_some(), "first prefix replay must publish");
    assert!(
        manifest.1,
        "publication must grant operational-prefix disclosure"
    );
    assert_eq!(access(1).replay(), manifest);
    // The live history route stays available after the incident.
    http.history(
        Some(&tokens[0]),
        game,
        &json!({"gameId":game,"contextId":reservation.context_id,"cursor":null,"limit":"5"}),
        200,
    );
    // A server that cannot recover the game serves the same prefix frame.
    let own = request(
        reservation,
        &json!("0"),
        reservation.seat_ids[0],
        "historical-seat",
    );
    let other_pool = http::HttpProbe::configure(&base, &key, |_| {
        NativeGameRuntime::with_pool(&base, &key, "resolution-selection").unwrap()
    });
    assert_eq!(
        other_pool.replay(Some(&tokens[0]), game, &own, 200),
        http.replay(Some(&tokens[0]), game, &own, 200)
    );
    json!({"phase":"results","actualNoContestReplayHttp":true,"noContestFramesChecked":frames,
        "operationalPrefixPublished":true,"actualNativeTransitionEvidence":true,
        "actualClockIntentDerived":true,"initialVersion2":true,"selectedPrintingProvenance":true,
        "selectedSourceActualSql":true,"immutableSourceDisplay":true,"fullyQualifiedCards":0,
        "semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
}
