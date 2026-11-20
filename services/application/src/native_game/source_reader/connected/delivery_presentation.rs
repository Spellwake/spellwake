//! Real mutable presentation observations around unchanged native snapshots/cursors.
use super::delivery::{Calls, record, request, source_fingerprint};
use super::*;
use crate::api::NativeStreamProbe;
use futures::StreamExt;
use std::{sync::Arc, time::Duration};
use tokio_postgres::{Client, NoTls};
#[path = "../../../../tests/support/cosmetic_sources.rs"]
mod cosmetic_sources;

struct Owner {
    seat: Uuid,
    account: Uuid,
    deck: Uuid,
    free: Uuid,
}
#[derive(Clone, Copy)]
enum Mutation {
    Header,
    Grant,
    Catalogue,
}
#[derive(Clone, Copy)]
enum Timing {
    Before,
    During,
    After,
}
struct Setup {
    old: Uuid,
    new: Uuid,
    purchases: Vec<Uuid>,
}
async fn choose(client: &Client, owner: &Owner, back: Uuid) {
    let ack:serde_json::Value=client.query_one("SELECT app.change_deck_appearance($1,$2,$3,sleeve_id,dice_id,NULL,$4) FROM app.deck WHERE id=$2",
        &[&owner.account,&owner.deck,&back,&Uuid::new_v4()]).await.unwrap().get(0);
    assert_eq!(ack["status"], "success");
}
async fn setup(client: &Client, owners: &[Owner]) -> Setup {
    let old = cosmetic_sources::cosmetic(client, "back").await.unwrap();
    let new = cosmetic_sources::cosmetic(client, "back").await.unwrap();
    let mut purchases = Vec::new();
    for owner in owners {
        purchases.push(
            cosmetic_sources::purchase(client, owner.account, old)
                .await
                .unwrap(),
        );
        cosmetic_sources::purchase(client, owner.account, new)
            .await
            .unwrap();
        choose(client, owner, old).await;
    }
    Setup {
        old,
        new,
        purchases,
    }
}
async fn mutate(client: &Client, owners: &[Owner], data: &Setup, kind: Mutation) {
    match kind {
        Mutation::Header => {
            for owner in owners {
                choose(client, owner, data.new).await;
            }
        }
        Mutation::Grant => {
            for purchase in &data.purchases {
                client
                    .execute(
                        "UPDATE app.purchase SET state='refunded' WHERE id=$1",
                        &[purchase],
                    )
                    .await
                    .unwrap();
            }
        }
        Mutation::Catalogue => {
            client
                .execute(
                    "UPDATE app.cosmetic SET state='inactive' WHERE id=$1",
                    &[&data.old],
                )
                .await
                .unwrap();
        }
    }
}
fn expected(owners: &[Owner], data: &Setup, kind: Mutation) -> Vec<Uuid> {
    owners
        .iter()
        .map(|owner| {
            if matches!(kind, Mutation::Header) {
                data.new
            } else {
                owner.free
            }
        })
        .collect()
}
fn appearance(frame: &serde_json::Value, owners: &[Owner], expected: &[Uuid]) {
    assert_eq!(frame["status"], "success");
    assert_eq!(frame["data"]["kind"], "snapshot");
    let seats = frame["data"]["presentation"]["seats"].as_array().unwrap();
    assert_eq!(seats.len(), owners.len());
    for ((seat, owner), back) in seats.iter().zip(owners).zip(expected) {
        assert_eq!(seat["seatId"], json!(owner.seat));
        assert_eq!(seat["appearance"]["backId"], json!(back));
    }
}
fn same_native(frame: &serde_json::Value, previous: &serde_json::Value) {
    assert_eq!(frame["data"]["snapshot"], previous["data"]["snapshot"]);
    assert_eq!(frame["data"]["cursor"], previous["data"]["cursor"]);
}
pub(super) async fn frame(body: &mut axum::body::BodyDataStream) -> serde_json::Value {
    record(&body.next().await.unwrap().unwrap())
}
pub(super) async fn open(
    probe: &NativeStreamProbe,
    token: &str,
    r: &Reservation,
) -> axum::body::BodyDataStream {
    let response = probe
        .request(request(
            token,
            r.game_id,
            "stream",
            &json!({"gameId":r.game_id,"contextId":r.context_id,"cursor":null}),
        ))
        .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    response.into_body().into_data_stream()
}
pub(super) async fn wait_for_real_observation(client: &Client) {
    tokio::time::timeout(Duration::from_secs(4),async {
        loop {
            // pg_stat_activity can cache a transaction snapshot; invalidate it on
            // each poll. The real relation waiter proves statement entry, not time.
            client.query_one("SELECT pg_stat_clear_snapshot()",&[]).await.unwrap();
            let waiting:bool=client.query_one("SELECT EXISTS(SELECT 1 FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE l.relation='app.cosmetic'::regclass AND NOT l.granted AND a.query LIKE '%read_native_human_presentation%')",&[]).await.unwrap().get(0);
            if waiting {break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("real presentation statement must wait on cosmetic relation lock");
}

pub(super) fn run(
    reservation: &Reservation,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let recovered = repository
        .recover_with_history(
            reservation.game_id,
            store,
            programs,
            NativeRecoveryBudget::new(100, 4_000_000).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered.history_evidence(),
        NativeHistoryEvidence::TransitionV1
    );
    super::selected_origin(
        recovered.checkpoint().provenance(),
        recovered.checkpoint().state(),
        reservation,
    );
    let original = source_fingerprint(&repository, reservation.game_id);
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    let url = reqwest::Url::parse(&database).unwrap();
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert!(matches!(url.scheme(), "postgresql" | "postgres"));
    let calls = Arc::new(Calls::default());
    let proxy = fault_proxy::Proxy::new(&base, None, calls.clone());
    let probe = NativeStreamProbe::new(http::configuration(
        proxy.url(),
        &key,
        &base,
        |runtime| runtime,
        None,
    ));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (client,connection)=tokio_postgres::connect(&database,NoTls).await.unwrap();
        let connection=tokio::spawn(connection);
        let rows=client.query("SELECT s.id,s.account_id,r.deck_id,app.free_cosmetic(s.account_id,'back') FROM app.match_seat s JOIN app.deck_revision r ON r.id=s.source_revision_id WHERE s.match_id=$1 ORDER BY s.seat_index",&[&reservation.match_id]).await.unwrap();
        let owners:Vec<_>=rows.iter().map(|row|Owner {seat:row.get(0),account:row.get(1),deck:row.get(2),free:row.get(3)}).collect();
        assert_eq!(owners.iter().map(|o|o.seat).collect::<Vec<_>>(),reservation.seat_ids);
        let data=setup(&client,&owners).await;
        let other=cosmetic_sources::purchase(&client,owners[0].account,data.old).await.unwrap();
        let mut body=open(&probe,&token,reservation).await;
        let baseline=frame(&mut body).await;
        appearance(&baseline,&owners,&[data.old,data.old]);
        client.execute("UPDATE app.purchase SET state='refunded' WHERE id=$1",&[&data.purchases[0]]).await.unwrap();
        let retained=frame(&mut body).await;appearance(&retained,&owners,&[data.old,data.old]);same_native(&retained,&baseline);
        client.execute("UPDATE app.purchase SET state='reversed' WHERE id=$1",&[&other]).await.unwrap();
        let revoked=frame(&mut body).await;appearance(&revoked,&owners,&[owners[0].free,data.old]);same_native(&revoked,&baseline);
        drop(body);assert_eq!(probe.available(),(16,16));

        for kind in [Mutation::Header,Mutation::Grant,Mutation::Catalogue] {
            for timing in [Timing::Before,Timing::During,Timing::After] {
                let data=setup(&client,&owners).await;
                let mut body=open(&probe,&token,reservation).await;
                let old=vec![data.old;owners.len()];
                let new=expected(&owners,&data,kind);
                let current=match timing {
                    Timing::Before => {
                        mutate(&client,&owners,&data,kind).await;
                        let current=frame(&mut body).await;appearance(&current,&owners,&new);current
                    }
                    Timing::During => {
                        client.batch_execute("BEGIN; LOCK TABLE app.cosmetic IN ACCESS EXCLUSIVE MODE").await.unwrap();
                        let task=tokio::spawn(async move {let value=frame(&mut body).await;(body,value)});
                        wait_for_real_observation(&client).await;
                        mutate(&client,&owners,&data,kind).await;
                        client.batch_execute("COMMIT").await.unwrap();
                        let (returned,current)=task.await.unwrap();body=returned;
                        appearance(&current,&owners,&old);current
                    }
                    Timing::After => {
                        let entered=calls.arm_after_presentation();
                        let task=tokio::spawn(async move {let value=frame(&mut body).await;(body,value)});
                        tokio::time::timeout(Duration::from_secs(4),entered).await.unwrap().unwrap();
                        mutate(&client,&owners,&data,kind).await;
                        calls.resume();
                        let (returned,current)=task.await.unwrap();body=returned;
                        appearance(&current,&owners,&old);current
                    }
                };
                let next=frame(&mut body).await;appearance(&next,&owners,&new);
                same_native(&current,&baseline);same_native(&next,&baseline);
                drop(body);assert_eq!(probe.available(),(16,16));
            }
        }
        drop(client);connection.await.unwrap().unwrap();
    });
    assert_eq!(
        source_fingerprint(&repository, reservation.game_id),
        original
    );
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({
            "phase":"presentation","actualNativeTransitionEvidence":true,"initialVersion2":true,
            "selectedPrintingProvenance":true,"selectedSourceActualSql":true,"immutableSourceDisplay":true,
            "actualRemainingGrant":true,"actualLastGrantRefresh":true,"actualEqualCursorAppearanceRefresh":true,
            "actualBeforeObservation":true,"actualDuringObservation":true,"actualAfterObservation":true,
            "actualHeaderCatalogueGrantMatrix":true,"coherentCases":9,"deletedOwnerQualified":false,
            "sourceUnchanged":true,"fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false,
            "paymentFixtureScope":"normalized verified provider_event/purchase and actual triggers; not external Stripe settlement",
        })
    );
}
