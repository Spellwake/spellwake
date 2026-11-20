//! Genuine native bytes over canonical mixed roster with controlled provider observations.
//! No model provider, controller SecretStore possession or AI command is qualified.
use super::delivery::source_fingerprint;
use super::*;
use crate::actor::{ActorClient, ActorConfig};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn originals(
    repository: &NativeSourceRepository,
    store: &NativeArtifactStore,
    r: &Reservation,
) -> (Value, Value, Vec<u8>) {
    let source: Value = repository
        .backend
        .rpc("read_native_game_source", &json!({"p_game":r.game_id}))
        .unwrap();
    let opening: Value = repository
        .backend
        .rpc("read_native_opening_source", &json!({"p_game":r.game_id}))
        .unwrap();
    assert_eq!(source["status"], "success");
    assert_eq!(opening["status"], "success");
    let reference = archive_integrity::reference(&source["data"]["initialRef"]);
    let bytes = store
        .read(
            &reference,
            r.game_id,
            r.context_id,
            NativeArtifactRole::Initial,
        )
        .unwrap()
        .bytes()
        .to_vec();
    (source, opening, bytes)
}
fn persisted(database: &str, r: &Reservation) -> Value {
    lifecycle_reads::sql(
        database,
        "SELECT jsonb_build_object('originals',(SELECT jsonb_agg(to_jsonb(c) ORDER BY c.id) FROM app.command_receipt c WHERE c.scope='native_player_v1'),'results',(SELECT jsonb_agg(to_jsonb(g) ORDER BY g.id) FROM app.game_result g WHERE g.game_id=$1),'manifests',(SELECT jsonb_agg(to_jsonb(m) ORDER BY m.id) FROM app.replay_manifest m WHERE m.game_id=$1))",
        &[&r.game_id],
    )
}
pub(super) fn run(r: &Reservation, store: &NativeArtifactStore) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let peer = std::env::var("SPELLWAKE_NATIVE_SOURCE_PEER_BEARER").unwrap();
    let outside = std::env::var("SPELLWAKE_NATIVE_SOURCE_OUTSIDER_BEARER").unwrap();
    let actor =
        ActorClient::new(ActorConfig::new(&base, &public, &format!("{base}/auth/v1")).unwrap())
            .unwrap()
            .resolve(&token)
            .unwrap();
    let account = actor.account().unwrap();
    let source = repository.read(r.game_id).unwrap().unwrap();
    let controllers: Vec<_> = source.controllers().collect();
    assert_eq!(controllers.len(), 2);
    assert_eq!(
        controllers[0],
        (r.seat_ids[0], Some(account), ControllerKind::Human)
    );
    assert_eq!(controllers[1], (r.seat_ids[1], None, ControllerKind::Ai));
    assert_eq!(
        source.human_seats().collect::<Vec<_>>(),
        vec![r.seat_ids[0]]
    );
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    let ready = lifecycle_reads::sql(
        &database,
        "SELECT jsonb_build_object('sponsor',s.sponsor_id,'humanAccount',(SELECT account_id FROM app.match_seat WHERE id=$2),'aiAccount',seat.account_id,'bindingEpoch',b.binding_epoch::text,'readyEpoch',b.ready_epoch::text,'ready',app.ai_slot_ready(s.id),'bindingState',b.observed_state,'grantState',g.state,'delivery',g.delivery_state,'grantEpoch',g.binding_epoch::text,'grantSeat',g.match_seat_id,'oldRevoked',(SELECT count(*) FROM app.controller_grant old WHERE old.binding_id=b.id AND old.id<>g.id AND old.state='revoked'),'provider',b.provider_runner_ref,'deck',(b.ready_deck_revision_id=s.deck_revision_id),'configuration',(b.ready_configuration_id=r.configuration_id)) FROM app.match_seat seat JOIN app.ai_runner_binding b ON b.match_seat_id=seat.id JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.room r ON r.id=s.room_id JOIN app.controller_grant g ON g.binding_id=b.id AND g.binding_epoch=b.binding_epoch AND g.state='active' WHERE seat.id=$1",
        &[&r.seat_ids[1], &r.seat_ids[0]],
    );
    assert_eq!(ready["sponsor"], json!(account));
    assert_eq!(ready["humanAccount"], json!(account));
    assert_eq!(ready["aiAccount"], Value::Null);
    assert_eq!(ready["bindingEpoch"], "2");
    assert_eq!(ready["readyEpoch"], ready["bindingEpoch"]);
    assert_eq!(ready["grantEpoch"], ready["bindingEpoch"]);
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["bindingState"], "ready");
    assert_eq!(ready["grantState"], "active");
    assert_eq!(ready["delivery"], "delivered");
    assert_eq!(ready["grantSeat"], json!(r.seat_ids[1]));
    assert_eq!(ready["oldRevoked"], 1);
    assert_eq!(ready["provider"], "fixture:mixed-replay-process");
    assert_eq!(ready["deck"], true);
    assert_eq!(ready["configuration"], true);
    for bearer in [&peer, &outside] {
        let outsider_actor =
            ActorClient::new(ActorConfig::new(&base, &public, &format!("{base}/auth/v1")).unwrap())
                .unwrap()
                .resolve(bearer)
                .unwrap();
        assert!(matches!(
            repository
                .human_access(&outsider_actor, r.game_id, r.context_id)
                .unwrap(),
            NativeHumanAccessOutcome::Unavailable
        ));
    }
    let commands = http::HttpProbe::owned(&base, &key, "fixture:source-reader");
    let query = json!({"gameId":r.game_id,"contextId":r.context_id,"cursor":null,"limit":"3"});
    let body = |seat, mode| replay_frames::request(r, &json!("0"), seat, mode, &Value::Null, 3);
    commands.history(Some(&token), r.game_id, &query, 200);
    let live = commands.replay(
        Some(&token),
        r.game_id,
        &body(r.seat_ids[0], "historical-seat"),
        200,
    );
    assert_eq!(
        live["data"]["snapshot"]["players"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let projected: BTreeSet<Uuid> = live["data"]["snapshot"]["players"]
        .as_array()
        .unwrap()
        .iter()
        .map(|seat| seat["seatId"].as_str().unwrap().parse().unwrap())
        .collect();
    assert_eq!(projected, r.seat_ids.iter().copied().collect());
    assert_eq!(
        commands.replay(
            Some(&token),
            r.game_id,
            &body(r.seat_ids[1], "historical-seat"),
            409
        )["data"]["mode"],
        "unavailable"
    );
    assert_eq!(
        commands.replay(
            Some(&token),
            r.game_id,
            &body(r.seat_ids[1], "completed-full-information"),
            409
        )["data"]["mode"],
        "unavailable"
    );
    for outsider in [&peer, &outside] {
        assert_eq!(
            commands.history(Some(outsider), r.game_id, &query, 404),
            json!({"status":"fail","data":{"game":"unavailable"}})
        );
        for seat in &r.seat_ids {
            for mode in ["historical-seat", "completed-full-information"] {
                assert_eq!(
                    commands.replay(Some(outsider), r.game_id, &body(*seat, mode), 404),
                    json!({"status":"fail","data":{"game":"unavailable"}})
                );
            }
        }
    }
    commands.concede(&token, r.game_id, r.context_id);
    let before = source_fingerprint(&repository, r.game_id);
    let original_before = originals(&repository, store, r);
    let before_publish = persisted(&database, r);
    assert_eq!(before_publish["originals"].as_array().unwrap().len(), 1);
    assert_eq!(before_publish["results"].as_array().unwrap().len(), 1);
    assert_eq!(before_publish["manifests"], Value::Null);
    commands.replay(
        Some(&token),
        r.game_id,
        &body(r.seat_ids[0], "historical-seat"),
        200,
    );
    let published = persisted(&database, r);
    assert_eq!(published["originals"], before_publish["originals"]);
    assert_eq!(published["results"], before_publish["results"]);
    assert_eq!(published["manifests"].as_array().unwrap().len(), 1);
    commands.history(Some(&token), r.game_id, &query, 200);
    let NativeHumanAccessOutcome::Granted(access) = repository
        .human_access(&actor, r.game_id, r.context_id)
        .unwrap()
    else {
        panic!("actual retained human required")
    };
    let manifest = access.replay().0.unwrap();
    let rows: Value = serde_json::from_slice(
        &repository
            .backend
            .select_bytes_bounded(
                "replay_manifest",
                &[
                    ("select", "artifact_ref".into()),
                    ("id", format!("eq.{manifest}")),
                    ("limit", "2".into()),
                ],
                16384,
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    let header = archive_integrity::read(store, r, &rows[0]["artifact_ref"]);
    let index = archive_integrity::read(store, r, &header["indexRef"]);
    let perspective_set: BTreeSet<_> = index["perspectives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|perspective| {
            (
                perspective["seatId"]
                    .as_str()
                    .unwrap()
                    .parse::<Uuid>()
                    .unwrap(),
                perspective["disclosure"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(index["perspectives"].as_array().unwrap().len(), 3);
    assert_eq!(
        perspective_set,
        BTreeSet::from([
            (r.seat_ids[0], "live".to_owned()),
            (r.seat_ids[0], "completed".to_owned()),
            (r.seat_ids[1], "completed".to_owned())
        ])
    );
    let hits = Arc::new(AtomicUsize::new(0));
    let observed = hits.clone();
    let (origin, secret) = (base.clone(), key.clone());
    let archive = http::HttpProbe::configure(&base, &key, move |_| {
        NativeGameRuntime::with_pool(&origin, &secret, "resolution-selection")
            .unwrap()
            .with_read_delivery_hook(
                TestDeliveryStage::ArchiveReplayCompose,
                move |_, recovered, _, _| {
                    assert!(recovered.is_none());
                    observed.fetch_add(1, Ordering::SeqCst);
                },
            )
    });
    for (seat, mode) in [
        (r.seat_ids[0], "historical-seat"),
        (r.seat_ids[0], "completed-full-information"),
        (r.seat_ids[1], "completed-full-information"),
    ] {
        let frame = archive.replay(Some(&token), r.game_id, &body(seat, mode), 200);
        assert_eq!(frame["data"]["snapshot"]["viewerSeatId"], json!(seat));
    }
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    assert_eq!(
        archive.replay(
            Some(&token),
            r.game_id,
            &body(r.seat_ids[1], "historical-seat"),
            409
        )["data"]["mode"],
        "unavailable"
    );
    for outsider in [&peer, &outside] {
        assert_eq!(
            archive.history(Some(outsider), r.game_id, &query, 404),
            json!({"status":"fail","data":{"game":"unavailable"}})
        );
        for seat in &r.seat_ids {
            for mode in ["historical-seat", "completed-full-information"] {
                assert_eq!(
                    archive.replay(Some(outsider), r.game_id, &body(*seat, mode), 404),
                    json!({"status":"fail","data":{"game":"unavailable"}})
                );
            }
        }
    }
    assert_eq!(source_fingerprint(&repository, r.game_id), before);
    assert_eq!(
        originals(&repository, store, r),
        original_before,
        "original source, ordered roster, opening/program/vocabulary and exact Initial bytes remain unchanged"
    );
    assert_eq!(
        persisted(&database, r),
        published,
        "actual accepted result/original/manifest rows remain byte-for-byte equivalent JSON"
    );
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":"mixed-replay","actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"sourceHumanSeats":1,"sourceAiSeats":1,"archivePerspectives":3,"filteredHumanPerspectives":1,"completedPerspectives":2,"sponsorAiHistoricalDenials":2,"outsideRouteDenials":20,"outsideAccessDenials":2,"authorizedArchiveReads":3,"sourceUnchanged":true,"originalIdentityUnchanged":true,"canonicalRowsUnchanged":true,"completedHistoryPositive":true,"incompatibleArchiveNoRecovery":true,"canonicalGrantAdopted":true,"providerActivated":false,"controllerPossessionQualified":false,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
