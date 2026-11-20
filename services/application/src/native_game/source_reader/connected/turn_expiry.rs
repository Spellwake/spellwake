//! Authenticated waits after all three opening choices. Every action uses its live seat's View.
use super::*;
use crate::player_wire::{PlayerAction, PlayerChoice, PlayerCommand, PlayerDomain, PlayerSnapshot};

pub(super) fn run(
    r: &Reservation,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    http: &http::HttpProbe,
    seek_combat: bool,
) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let tokens = [
        "SPELLWAKE_NATIVE_STORAGE_CALLER",
        "SPELLWAKE_NATIVE_SOURCE_PEER_BEARER",
        "SPELLWAKE_NATIVE_SOURCE_OUTSIDER_BEARER",
    ]
    .map(|name| std::env::var(name).unwrap());
    for token in &tokens {
        super::response_expiry::keep_hand(http, token, r);
    }
    let recover = || {
        repository
            .recover_with_history(
                r.game_id,
                store,
                programs,
                NativeRecoveryBudget::new(4000, 100_000_000).unwrap(),
            )
            .unwrap()
            .unwrap()
    };
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let auth = crate::actor::ActorClient::new(
        crate::actor::ActorConfig::new(
            &base,
            &std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap(),
            &format!("{}/auth/v1", base.trim_end_matches('/')),
        )
        .unwrap(),
    )
    .unwrap();
    let actors = tokens.each_ref().map(|token| auth.resolve(token).unwrap());
    let mut steps = 0;
    let (game, actor, original, wait) = loop {
        let game = recover();
        let turn = game
            .checkpoint()
            .turn_response()
            .unwrap()
            .expect("controlled turn must wait for a seat");
        let actor = r
            .seat_ids
            .iter()
            .position(|seat| *seat == turn.seat_id)
            .unwrap();
        let wait = repository
            .opening_response_wait(r.game_id)
            .unwrap()
            .unwrap();
        let public = http.read(Some(&tokens[actor]), r.game_id, r.context_id, 200);
        let snapshot: PlayerSnapshot =
            serde_json::from_value(public["data"]["snapshot"].clone()).unwrap();
        let (action, combat) = if let Some(choice) = snapshot.decisions.first() {
            assert_eq!(choice.deadline_at.as_deref(), Some(wait.expires_at()));
            let PlayerDomain::Attackers { required, .. } = &choice.domain else {
                panic!("expected first declaration of attackers")
            };
            assert!(required.is_empty());
            (
                PlayerAction::Choose {
                    decision: choice.decision.clone(),
                    response: PlayerChoice::Attackers {
                        assignments: vec![],
                    },
                },
                true,
            )
        } else {
            assert_eq!(
                snapshot
                    .turn
                    .as_ref()
                    .unwrap()
                    .priority_deadline_at
                    .as_deref(),
                Some(wait.expires_at())
            );
            let action = if seek_combat {
                let NativeHumanAccessOutcome::Granted(access) = repository
                    .human_access(&actors[actor], r.game_id, r.context_id)
                    .unwrap()
                else {
                    panic!("actual seat required")
                };
                let authority =
                    super::super::command::NativePlayerAuthority::Human(&actors[actor], &access);
                let display =
                    NativeImmutableDisplay::new(programs, game.checkpoint().provenance().unwrap())
                        .unwrap();
                let meta = NativeProjectionMetadata {
                    through_event_sequence: game.source().last_event_sequence().clone(),
                    deadline_at: None,
                    disclosure: NativeDisclosure::LiveParticipant,
                };
                let issued = authority
                    .issue(&repository, store, &game, &meta, &display)
                    .unwrap();
                super::cast_activity::choose(&snapshot, &game, &issued)
            } else {
                PlayerAction::Pass {}
            };
            (action, false)
        };
        let original = PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: snapshot.basis,
            action,
        };
        if seek_combat == combat {
            let reconnect = json!({"receipts":{"gameId":r.game_id,"contextId":r.context_id,"outstanding":[]},"cursor":null});
            assert_eq!(
                http.reconnect(Some(&tokens[actor]), r.game_id, &reconnect, 200)["data"]["snapshot"],
                public["data"]["snapshot"]
            );
            for (i, token) in tokens.iter().enumerate() {
                if i == actor {
                    continue;
                }
                let view = http.read(Some(token), r.game_id, r.context_id, 200);
                assert!(
                    view["data"]["snapshot"]["turn"]
                        .get("priorityDeadlineAt")
                        .is_none()
                );
                assert_eq!(view["data"]["snapshot"]["decisions"], json!([]));
            }
            break (game, actor, original, wait);
        }
        steps += 1;
        assert!(steps <= 1600, "combat decision must be reachable");
        let accepted = http.command(Some(&tokens[actor]), &original, 202);
        assert_eq!(http.command(Some(&tokens[actor]), &original, 202), accepted);
    };
    let late = || {
        assert_eq!(
            http.command(Some(&tokens[actor]), &original, 409),
            json!({"status":"fail","data":{"decision":"stale"}})
        )
    };
    let next = super::response_expiry::expire(
        &repository,
        store,
        r,
        &game,
        &wait,
        r.seat_ids[actor],
        late,
    );
    late();
    let after = recover();
    assert!(
        after.history_evidence().transitions_verified()
            && after.checkpoint().reference() == next.reference()
    );
    let survivors: Vec<_> = (0..tokens.len()).filter(|i| *i != actor).collect();
    http.concede(&tokens[survivors[0]], r.game_id, r.context_id);
    let complete = recover();
    assert_eq!(
        complete
            .checkpoint()
            .terminal()
            .unwrap()
            .unwrap()
            .winner_seat_ids(),
        &[r.seat_ids[survivors[1]]]
    );
    let db = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    let settle = || {
        lifecycle_reads::sql(
            &db,
            "SELECT app.consume_game_outcome((SELECT id FROM app.game_result WHERE game_id=$1),(SELECT revision FROM app.match WHERE id=$2))",
            &[&r.game_id, &r.match_id],
        )
    };
    let first = settle();
    assert_eq!(first["status"], "success");
    assert_eq!(settle(), first);
    let result = lifecycle_reads::sql(
        &db,
        "SELECT jsonb_build_object('gameResults',(SELECT count(*) FROM app.game_result WHERE game_id=$1),'winners',winner_seat_ids) FROM app.match_result WHERE match_id=$2",
        &[&r.game_id, &r.match_id],
    );
    assert_eq!(
        result,
        json!({"gameResults":1,"winners":[r.seat_ids[survivors[1]]]})
    );
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"opening-response","priorityTimeout":!seek_combat,
        "combatDecisionTimeout":seek_combat,"interveningCommands":steps,"ownerReconnectAndPeerPrivacy":true,
        "databaseDue":true,"exactRetry":true,"coldHistory":true,"matchSettled":true,"publicDeadlineAndLateReplyVerified":true,"result":result})
    );
}
