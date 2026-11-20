//! Actual HTTP own-seat replay of a game still in progress: frames come from a
//! fresh original walk, never publish a manifest, and refuse full-information
//! and other-seat perspectives. Expected values come from verified live walks.
use super::replay_frames::{events_through, request, sequence};
use super::*;
use serde_json::Value;

/// Returns the number of in-progress frames checked against expected walks.
pub(super) fn run(
    http: &http::HttpProbe,
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    reservation: &Reservation,
    tokens: &[String; 2],
    actors: &[crate::actor::RequestActor; 2],
) -> usize {
    let game = reservation.game_id;
    let access = |index: usize| {
        let NativeHumanAccessOutcome::Granted(access) = repository
            .human_access(&actors[index], game, reservation.context_id)
            .unwrap()
        else {
            panic!("actual active participant required")
        };
        access
    };
    let release = repository.read(game).unwrap().unwrap().context_ids().1;
    let mut frames = 0;
    for (index, token) in tokens.iter().enumerate() {
        let seat = reservation.seat_ids[index];
        let current = access(index);
        assert!(current.completed_disclosure().is_none());
        assert!(!current.operational_no_contest());
        let expected = repository
            .project_human_history(
                store,
                programs,
                &current,
                NativeRecoveryBudget::new(10_000, 100_000_000).unwrap(),
            )
            .unwrap()
            .unwrap();
        let last = expected.observations().len() - 1;
        super::history_scans::receipt_ranges(repository, &expected);
        for at in [0, last / 2, last] {
            let body = request(
                reservation,
                &sequence(&expected, at),
                seat,
                "historical-seat",
                &Value::Null,
                100,
            );
            let frame = http.replay(Some(token), game, &body, 200);
            assert!(serde_json::to_vec(&frame).unwrap().len() <= 262_144);
            let data = &frame["data"];
            assert_eq!(data["releaseId"], json!(release));
            assert_eq!(data["schemaRevision"], "spellwake-replay-v2");
            assert_eq!(
                data["snapshot"],
                serde_json::to_value(expected.observations()[at].projection().snapshot()).unwrap()
            );
            let events = events_through(&expected, at);
            let page = data["history"]["events"].as_array().unwrap();
            assert!(page.len() <= events.len() && page[..] == events[..page.len()]);
            assert!(!page.is_empty() || events.is_empty());
            assert_eq!(data["history"]["hasMore"], json!(page.len() < events.len()));
            if page.len() == events.len() {
                assert_eq!(
                    data["history"]["scannedThrough"]["throughEventSequence"],
                    sequence(&expected, at)
                );
            }
            frames += 1;
        }
        // Paging at the tip continues the same live cursor without gaps.
        let tip = sequence(&expected, last);
        let events = events_through(&expected, last);
        let mut cursor = Value::Null;
        let mut seen = Vec::new();
        for _ in 0..=events.len() {
            let body = request(reservation, &tip, seat, "historical-seat", &cursor, 3);
            let data = http.replay(Some(token), game, &body, 200)["data"].clone();
            seen.extend(
                data["history"]["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
            cursor = data["history"]["scannedThrough"].clone();
            frames += 1;
            if data["history"]["hasMore"] == json!(false) {
                break;
            }
        }
        assert_eq!(seen, events);
        // A cursor past the requested coordinate is unavailable, never truncated.
        if last > 0 {
            let body = request(
                reservation,
                &sequence(&expected, 0),
                seat,
                "historical-seat",
                &cursor,
                3,
            );
            assert_eq!(
                http.replay(Some(token), game, &body, 409)["data"]["cursor"],
                "unavailable"
            );
        }
        // A live cursor resumes on /history too: both use the same live disclosure.
        http.history(
            Some(token),
            game,
            &json!({"gameId":game,"contextId":reservation.context_id,"cursor":cursor,"limit":"5"}),
            200,
        );
        // Full information and another seat stay refused while in progress.
        let other = reservation.seat_ids[1 - index];
        for body in [
            request(
                reservation,
                &tip,
                seat,
                "completed-full-information",
                &Value::Null,
                5,
            ),
            request(reservation, &tip, other, "historical-seat", &Value::Null, 5),
        ] {
            assert_eq!(
                http.replay(Some(token), game, &body, 409)["data"]["mode"],
                "unavailable"
            );
        }
        // A coordinate past the accepted tip is not an accepted cut.
        let past = json!((tip.as_str().unwrap().parse::<u64>().unwrap() + 1_000).to_string());
        let body = request(reservation, &past, seat, "historical-seat", &Value::Null, 5);
        assert_eq!(
            http.replay(Some(token), game, &body, 409)["data"]["atEventSequence"],
            "unavailable"
        );
    }
    // Too small a recovery budget refuses the live walk with the safe 503.
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let starved = http::HttpProbe::configure(&base, &key, |runtime| {
        runtime.with_test_recovery(NativeRecoveryBudget::new(1, 1).unwrap())
    });
    let own = request(
        reservation,
        &json!("0"),
        reservation.seat_ids[0],
        "historical-seat",
        &Value::Null,
        5,
    );
    assert_eq!(
        starved.replay(Some(&tokens[0]), game, &own, 503)["code"],
        "unavailable"
    );
    // No manifest is published for a game in progress.
    assert_eq!(access(0).replay(), (None, false));
    assert_eq!(access(1).replay(), (None, false));
    frames
}
