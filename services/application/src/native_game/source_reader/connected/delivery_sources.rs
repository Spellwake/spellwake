//! Canonical host incidents and a real terminal crossing prepared delivery.
use super::delivery::{Calls, cut, json_response, request, source_fingerprint};
use super::delivery_presentation::{frame, open};
use super::*;
use crate::{
    api::NativeStreamProbe,
    player_wire::{PlayerAction, PlayerCommand},
};
use axum::{http::StatusCode, response::Response};
use futures::StreamExt;
use std::{sync::Arc, time::Duration};
use tokio_postgres::{Client, NoTls};

async fn pause(client: &Client, game: Uuid) {
    let result: serde_json::Value = client.query_one(
        "SELECT app.pause_game_incident(g.id,h.owner_instance_ref,h.fence_epoch,h.revision,clock_timestamp(),'server_process','test-only:controlled-owned-host-loss',$2) FROM app.game g JOIN app.operational_state h ON h.game_id=g.id WHERE g.id=$1",
        &[&game,&Uuid::new_v4()],
    ).await.unwrap().get(0);
    assert_eq!(result["status"], "success");
}
async fn restore(client: &Client, game: Uuid) {
    let result: serde_json::Value = client.query_one(
        "SELECT app.restore_game_checkpoint(g.id,h.fence_epoch,h.revision,h.recovery_checkpoint_id,h.owner_instance_ref,'test-only:controlled-owned-restore',$2) FROM app.game g JOIN app.operational_state h ON h.game_id=g.id WHERE g.id=$1",
        &[&game,&Uuid::new_v4()],
    ).await.unwrap().get(0);
    assert_eq!(result["status"], "success");
}
async fn stale_response(response: Response) {
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_response(response).await,
        json!({"status":"fail","data":{"basis":"stale"}})
    );
}
pub(super) fn run(r: &Reservation, store: &NativeArtifactStore, programs: &NativeProgramSource) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let recovered = repository
        .recover_with_history(
            r.game_id,
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
        r,
    );
    let base = std::env::var("SPELLWAKE_SUPABASE_URL").unwrap();
    let key = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap();
    let bearer = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    assert_eq!(
        reqwest::Url::parse(&database).unwrap().host_str(),
        Some("127.0.0.1")
    );
    let calls = Arc::new(Calls::default());
    let proxy = fault_proxy::Proxy::new(&base, None, calls.clone());
    let probe = Arc::new(NativeStreamProbe::new(http::configuration(
        proxy.url(),
        &key,
        &base,
        |runtime| runtime,
        None,
    )));
    let commands = NativeStreamProbe::new(http::configuration(
        &base,
        &key,
        &base,
        |runtime| runtime,
        Some(NativeHostOwner::new("fixture:source-reader").unwrap()),
    ));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (client,connection)=tokio_postgres::connect(&database,NoTls).await.unwrap();
        let connection=tokio::spawn(connection);
        for phase in ["reconnect","stream-preflight","stream-first","stream-later"] {
            eprintln!("canonical late-source boundary: {phase}");
            // Every boundary has a positive actual delivery before the canonical
            // fixture host incident. No fake access/source response is substituted.
            let mut positive=open(&probe,&bearer,r).await;
            assert_eq!(frame(&mut positive).await["status"],"success");
            drop(positive);
            let before=cut(r.game_id).await;
            if matches!(phase,"reconnect"|"stream-preflight") {
                let entered=if phase=="reconnect" {calls.arm_after_receipts()} else {calls.arm_after_presentation()};
                let task_probe=probe.clone();let token=bearer.clone();let game=r.game_id;let context=r.context_id;
                let task=tokio::spawn(async move {
                    let (operation,payload)=if phase=="reconnect" {
                        ("reconnect",json!({"receipts":{"gameId":game,"contextId":context,"outstanding":[]},"cursor":null}))
                    } else {("stream",json!({"gameId":game,"contextId":context,"cursor":null}))};
                    task_probe.request(request(&token,game,operation,&payload)).await
                });
                tokio::time::timeout(Duration::from_secs(4),entered).await.unwrap().unwrap();
                pause(&client,r.game_id).await;let paused=cut(r.game_id).await;assert_ne!(before,paused);
                calls.resume();stale_response(task.await.unwrap()).await;
                assert_eq!(cut(r.game_id).await,paused);
            } else {
                let mut body=open(&probe,&bearer,r).await;
                if phase=="stream-later" {assert_eq!(frame(&mut body).await["status"],"success");}
                let entered=calls.arm_after_presentation();
                let task=tokio::spawn(async move {let value=frame(&mut body).await;(body,value)});
                tokio::time::timeout(Duration::from_secs(4),entered).await.unwrap().unwrap();
                pause(&client,r.game_id).await;let paused=cut(r.game_id).await;assert_ne!(before,paused);
                calls.resume();let (mut body,value)=task.await.unwrap();
                assert_eq!(value,json!({"status":"fail","data":{"basis":"stale"}}));
                assert!(body.next().await.is_none());drop(body);
                assert_eq!(cut(r.game_id).await,paused);
            }
            assert_eq!(probe.available(),(16,16));
            restore(&client,r.game_id).await;
            let restored=cut(r.game_id).await;
            let mut positive=open(&probe,&bearer,r).await;
            assert_eq!(frame(&mut positive).await["status"],"success");drop(positive);
            assert_eq!(cut(r.game_id).await,restored);
        }
        let terminal_before:bool=client.query_one("SELECT (app.native_disclosure_source($1,$2)->>'completed')::boolean",&[&r.game_id,&r.context_id]).await.unwrap().get(0);
        assert!(!terminal_before);
        let view=commands.request(request(&bearer,r.game_id,"view",&json!({"gameId":r.game_id,"contextId":r.context_id}))).await;
        assert_eq!(view.status(),StatusCode::OK);let view=json_response(view).await;
        let command=PlayerCommand {request_id:Uuid::new_v4(),basis:serde_json::from_value(view["data"]["snapshot"]["basis"].clone()).unwrap(),action:PlayerAction::Concede {}};
        let mut prepared=open(&probe,&bearer,r).await;
        let accepted=commands.request(request(&bearer,r.game_id,"commands",&command)).await;
        assert_eq!(accepted.status(),StatusCode::ACCEPTED);assert_eq!(json_response(accepted).await["status"],"success");
        let terminal_after:bool=client.query_one("SELECT (app.native_disclosure_source($1,$2)->>'completed')::boolean",&[&r.game_id,&r.context_id]).await.unwrap().get(0);
        assert!(terminal_after);
        let terminal=cut(r.game_id).await;
        assert_eq!(frame(&mut prepared).await,json!({"status":"fail","data":{"basis":"stale"}}));
        assert!(prepared.next().await.is_none());drop(prepared);
        assert_eq!(cut(r.game_id).await,terminal);
        assert_eq!(probe.available(),(16,16));assert_eq!(commands.available(),(16,16));
        drop(client);connection.await.unwrap().unwrap();
    });
    let source = repository.read(r.game_id).unwrap().unwrap();
    assert_eq!(source.accepted_revision(), &Amount::from(1));
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({
            "phase":"sources","actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,
            "selectedSourceActualSql":true,"immutableSourceDisplay":true,"actualCanonicalIncidentCases":4,
            "actualCanonicalRestorations":4,"actualTerminalDisclosureCrossing":true,"sourceUnchangedAcrossReads":true,
            "fullyQualifiedCards":0,"semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false,
            "finalSourceFingerprint":source_fingerprint(&repository,r.game_id).iter().map(|v|format!("{v:02x}")).collect::<String>()
        })
    );
}
