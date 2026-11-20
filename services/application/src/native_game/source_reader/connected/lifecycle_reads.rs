//! Canonical lifecycle changes between actual read composition and publication.
use super::delivery::source_fingerprint;
use super::*;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio_postgres::{NoTls, types::ToSql};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Active,
    Completed,
    Incident,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Pending,
    Requester,
    Peer,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Read {
    History,
    Own,
    Full,
}
impl Read {
    fn tag(self) -> &'static str {
        match self {
            Self::History => "history",
            Self::Own => "own-replay",
            Self::Full => "full-replay",
        }
    }
    fn body(self, r: &Reservation, viewer: usize) -> Value {
        let replay = super::replay_frames::request(
            r,
            &json!("0"),
            r.seat_ids[if self == Self::Full {
                1 - viewer
            } else {
                viewer
            }],
            if self == Self::Full {
                "completed-full-information"
            } else {
                "historical-seat"
            },
            &Value::Null,
            3,
        );
        if self == Self::History {
            replay["history"].clone()
        } else {
            replay
        }
    }
    fn read(
        self,
        http: &http::HttpProbe,
        token: &str,
        game: Uuid,
        body: &Value,
        status: u16,
    ) -> Value {
        if self == Self::History {
            http.history(Some(token), game, body, status)
        } else {
            http.replay(Some(token), game, body, status)
        }
    }
}
struct Barrier {
    armed: AtomicBool,
    entered: std::sync::mpsc::Sender<Read>,
    released: Mutex<bool>,
    wake: Condvar,
}
impl Barrier {
    fn park(&self, read: Read) {
        if !self.armed.load(Ordering::SeqCst) {
            return;
        }
        self.entered.send(read).unwrap();
        let (released, timed) = self
            .wake
            .wait_timeout_while(
                self.released
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                Duration::from_secs(5),
                |released| !*released,
            )
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(released);
        assert!(
            !timed.timed_out(),
            "canonical lifecycle action must release composed readers"
        );
    }
    fn release(&self) {
        *self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.wake.notify_all();
    }
}
struct ReleaseOnDrop(Arc<Barrier>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}
pub(super) fn sql(database: &str, query: &str, parameters: &[&(dyn ToSql + Sync)]) -> Value {
    assert_eq!(
        reqwest::Url::parse(database).unwrap().host_str(),
        Some("127.0.0.1")
    );
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (client, connection) = tokio_postgres::connect(database, NoTls).await.unwrap();
        let task = tokio::spawn(connection);
        let value: Value = client.query_one(query, parameters).await.unwrap().get(0);
        drop(client);
        task.await.unwrap().unwrap();
        value
    })
}
fn reconcile(database: &str, r: &Reservation, origin: Origin) {
    if origin == Origin::Completed {
        let consumed = sql(
            database,
            "SELECT app.consume_game_outcome(r.id,m.revision) FROM app.game_result r JOIN app.game g ON g.id=r.game_id JOIN app.match m ON m.id=g.match_id WHERE g.id=$1",
            &[&r.game_id],
        );
        assert_eq!(consumed["status"], "success");
        assert_eq!(consumed["data"]["phase"], "terminal");
    }
    let drained = sql(
        database,
        "SELECT app.consume_quest_game_activity($1,100)",
        &[&r.game_id],
    );
    assert_eq!(drained["status"], "success");
    let ready = sql(
        database,
        "SELECT jsonb_build_object('ready',app.retirement_match_ready($2),'covered',EXISTS(SELECT 1 FROM app.activity_consumer_cursor c WHERE c.game_id=$1 AND c.consumer_key='quests-v1' AND c.initialized AND c.last_sequence>=(SELECT max(sequence) FROM app.game_event WHERE game_id=$1)))",
        &[&r.game_id, &r.match_id],
    );
    assert_eq!(ready, json!({"ready":true,"covered":true}));
}
fn retired(database: &str, account: Uuid) {
    let ready = sql(
        database,
        "SELECT jsonb_build_object('unblocked',NOT app.account_retirement_blocked($1,o.id),'intent',app.accepted_deletion_intent($1) IS NOT NULL) FROM app.asset_owner o WHERE o.account_id=$1",
        &[&account],
    );
    assert_eq!(ready, json!({"unblocked":true,"intent":true}));
    let value = sql(database, "SELECT app.retire_account($1)", &[&account]);
    assert_eq!(value["status"], "success");
    assert_eq!(value["data"]["state"], "retired");
    assert!(
        value["data"]["retirementOperationId"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .is_ok()
    );
}
fn deleted_identity(
    http: &http::HttpProbe,
    token: &str,
    r: &Reservation,
    seat: Uuid,
    account: Uuid,
) {
    let seats = http.match_seats(token, r.match_id);
    let row = seats["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(seat))
        .unwrap();
    assert_eq!(
        row["person"],
        json!({"id":account,"username":"Deleted player","isDeleted":true})
    );
}
fn fixture_accounts(database: &str, r: &Reservation) -> (Uuid, Uuid) {
    let fixture = sql(
        database,
        "SELECT jsonb_build_object('shape',m.structure='bo1' AND m.mode='open' AND NOT m.competitive AND (SELECT count(*) FROM app.match_seat WHERE match_id=m.id)=2 AND NOT EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=m.id AND controller_kind<>'human'),'owner',(SELECT account_id FROM app.match_seat WHERE id=$2),'peer',(SELECT account_id FROM app.match_seat WHERE id=$3)) FROM app.match m WHERE m.id=$1",
        &[&r.match_id, &r.seat_ids[0], &r.seat_ids[1]],
    );
    assert_eq!(fixture["shape"], true);
    let other = sql(
        database,
        "SELECT jsonb_build_object('trades',(SELECT count(*) FROM app.trade_offer),'purchases',(SELECT count(*) FROM app.purchase),'predictions',(SELECT count(*) FROM app.prediction_order),'listings',(SELECT count(*) FROM app.market_listing),'heldAssets',(SELECT count(*) FROM app.asset_hold WHERE state='held'),'aiBindings',(SELECT count(*) FROM app.ai_runner_binding),'aiSlots',(SELECT count(*) FROM app.room_slot WHERE controller_kind='ai'),'ante',(SELECT count(*) FROM app.ante_commitment WHERE state NOT IN('canceled','settled','refunded')),'ranks',(SELECT count(*) FROM app.rank),'competitiveResults',(SELECT count(*) FROM app.competitive_result),'unrelatedMatches',(SELECT count(*) FROM app.match WHERE id<>$1))",
        &[&r.match_id],
    );
    assert_eq!(
        other,
        json!({"trades":0,"purchases":0,"predictions":0,"listings":0,"heldAssets":0,"aiBindings":0,"aiSlots":0,"ante":0,"ranks":0,"competitiveResults":0,"unrelatedMatches":0})
    );
    let owner: Uuid = fixture["owner"].as_str().unwrap().parse().unwrap();
    let peer: Uuid = fixture["peer"].as_str().unwrap().parse().unwrap();
    (owner, peer)
}
/// Canonical terminal producer, obligation reconciliation and actual archive publication.
pub(super) fn prepare_archive(r: &Reservation, incident: bool) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let public = std::env::var("SPELLWAKE_SUPABASE_PUBLISHABLE_KEY").unwrap();
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    fixture_accounts(&database, r);
    let commands = http::HttpProbe::owned(&base, &key, "fixture:source-reader");
    if incident {
        super::no_contest::terminalize(&repository, r);
    } else {
        let token = history_delivery::fresh_owner(&base, &key, &public);
        commands.concede(&token, r.game_id, r.context_id);
    }
    reconcile(
        &database,
        r,
        if incident {
            Origin::Incident
        } else {
            Origin::Completed
        },
    );
    let token = history_delivery::fresh_owner(&base, &key, &public);
    let request = super::replay_frames::request(
        r,
        &json!("0"),
        r.seat_ids[0],
        "historical-seat",
        &Value::Null,
        3,
    );
    commands.replay(Some(&token), r.game_id, &request, 200);
    if incident {
        let token = history_delivery::fresh_owner(&base, &key, &public);
        for (seat, mode) in [
            (r.seat_ids[0], "completed-full-information"),
            (r.seat_ids[1], "completed-full-information"),
            (r.seat_ids[1], "historical-seat"),
        ] {
            let denied = super::replay_frames::request(r, &json!("0"), seat, mode, &Value::Null, 3);
            assert_eq!(
                commands.replay(Some(&token), r.game_id, &denied, 409)["data"]["mode"],
                "unavailable"
            );
        }
    }
}
pub(super) fn run(r: &Reservation, phase: &str) {
    let (origin, action) = phase
        .strip_prefix("lifecycle-read-")
        .unwrap()
        .split_once('-')
        .unwrap();
    let origin = match origin {
        "active" => Origin::Active,
        "completed" => Origin::Completed,
        "incident" => Origin::Incident,
        _ => panic!("unknown lifecycle origin"),
    };
    let action = match action {
        "pending" => Action::Pending,
        "requester" => Action::Requester,
        "peer" => Action::Peer,
        _ => panic!("unknown lifecycle action"),
    };
    assert!(origin != Origin::Active || action == Action::Pending);
    let repository = NativeSourceRepository::from_env().unwrap();
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let owner_token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let peer_token = std::env::var("SPELLWAKE_NATIVE_SOURCE_PEER_BEARER").unwrap();
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    let (owner, peer) = fixture_accounts(&database, r);
    let commands = http::HttpProbe::owned(&base, &key, "fixture:source-reader");
    if origin != Origin::Active {
        prepare_archive(r, origin == Origin::Incident);
    }
    if action == Action::Requester {
        commands.request_deletion(&owner_token);
    }
    if action == Action::Peer {
        commands.request_deletion(&peer_token);
    }
    let before = source_fingerprint(&repository, r.game_id);
    let selected = if origin == Origin::Completed {
        vec![Read::History, Read::Own, Read::Full]
    } else {
        vec![Read::History, Read::Own]
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let barrier = Arc::new(Barrier {
        armed: AtomicBool::new(false),
        entered: tx,
        released: Mutex::new(false),
        wake: Condvar::new(),
    });
    let probes: Vec<_> = selected
        .iter()
        .map(|&read| {
            let gate = barrier.clone();
            let archive = origin != Origin::Active && read != Read::History;
            let origin_url = base.clone();
            let secret = key.clone();
            let stage = if read == Read::History {
                TestDeliveryStage::HistoryCompose
            } else if archive {
                TestDeliveryStage::ArchiveReplayCompose
            } else {
                TestDeliveryStage::LiveReplayCompose
            };
            http::HttpProbe::configure(&base, &key, move |runtime| {
                let runtime = if archive {
                    NativeGameRuntime::with_pool(&origin_url, &secret, "resolution-selection")
                        .unwrap()
                } else {
                    runtime
                };
                runtime.with_read_delivery_hook(stage, move |_, recovered, _, _| {
                    assert_eq!(
                        recovered.is_none(),
                        archive,
                        "archive reader must not recover with current incompatible programs"
                    );
                    gate.park(read);
                })
            })
        })
        .collect();
    let positives: Vec<_> = selected
        .iter()
        .zip(&probes)
        .map(|(&read, http)| read.read(http, &owner_token, r.game_id, &read.body(r, 0), 200))
        .collect();
    barrier.armed.store(true, Ordering::SeqCst);
    let responses = std::thread::scope(|scope| {
        let _release = ReleaseOnDrop(barrier.clone());
        let requests: Vec<_> = selected
            .iter()
            .zip(&probes)
            .map(|(&read, http)| {
                let token = &owner_token;
                let body = read.body(r, 0);
                scope.spawn(move || {
                    read.read(
                        http,
                        token,
                        r.game_id,
                        &body,
                        if action == Action::Requester {
                            401
                        } else {
                            200
                        },
                    )
                })
            })
            .collect();
        let mut composed = BTreeSet::new();
        for _ in &selected {
            assert!(composed.insert(rx.recv_timeout(Duration::from_secs(5)).unwrap()));
        }
        assert_eq!(composed, selected.iter().copied().collect());
        match action {
            Action::Pending => commands.request_deletion(&owner_token),
            Action::Requester => retired(&database, owner),
            Action::Peer => retired(&database, peer),
        }
        barrier.release();
        requests
            .into_iter()
            .map(|request| request.join().unwrap())
            .collect::<Vec<_>>()
    });
    barrier.armed.store(false, Ordering::SeqCst);
    assert_eq!(source_fingerprint(&repository, r.game_id), before);
    if action == Action::Requester {
        for response in &responses {
            assert_eq!(
                response,
                &json!({"status":"fail","data":{"session":"unauthorized"}})
            );
        }
        for (&read, http) in selected.iter().zip(&probes) {
            read.read(http, &peer_token, r.game_id, &read.body(r, 1), 200);
        }
        deleted_identity(&commands, &peer_token, r, r.seat_ids[0], owner);
    } else {
        assert_eq!(
            responses, positives,
            "lifecycle must preserve exact retained public gameplay"
        );
        if action == Action::Peer {
            deleted_identity(&commands, &owner_token, r, r.seat_ids[1], peer);
        }
    }
    if origin == Origin::Incident {
        for (seat, mode) in [
            (r.seat_ids[0], "completed-full-information"),
            (r.seat_ids[1], "completed-full-information"),
            (r.seat_ids[1], "historical-seat"),
        ] {
            let token = if action == Action::Requester {
                &peer_token
            } else {
                &owner_token
            };
            let perspective = if action == Action::Requester && mode == "historical-seat" {
                r.seat_ids[0]
            } else {
                seat
            };
            let denied =
                super::replay_frames::request(r, &json!("0"), perspective, mode, &Value::Null, 3);
            assert_eq!(
                commands.replay(Some(token), r.game_id, &denied, 409)["data"]["mode"],
                "unavailable"
            );
        }
    }
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({"phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,"readPaths":selected.iter().map(|read|read.tag()).collect::<Vec<_>>(),"concurrentComposedReads":selected.len(),"actualPendingRead":action==Action::Pending,"actualRequesterRetirement":action==Action::Requester,"actualPeerRetirement":action==Action::Peer,"sourceUnchanged":true,"fixtureObligationsEmpty":true,"incidentDeniedModesChecked":if origin==Origin::Incident {3} else {0},"incompatibleProgramsArchive":origin!=Origin::Active,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false})
    );
}
