//! Canonical retirement after a real native terminal, including historical appearance.
use super::delivery::{Calls, cut, json_response, request, source_fingerprint};
use super::delivery_presentation::{frame, open, wait_for_real_observation};
use super::*;
use crate::{
    api::NativeStreamProbe,
    player_wire::{PlayerAction, PlayerCommand},
};
use axum::{
    body::Body,
    extract::Request,
    http::{StatusCode, header},
};
use futures::StreamExt;
use std::{sync::Arc, time::Duration};
use tokio_postgres::{Client, NoTls};
async fn deletion_request(probe: &NativeStreamProbe, token: &str) {
    let request = Request::builder()
        .method("POST")
        .uri("/v1/account/deletion")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"requestId":Uuid::new_v4(),"statusKeyHash":"a".repeat(64)}).to_string(),
        ))
        .unwrap();
    let response = probe.request(request).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(json_response(response).await["status"], "success");
}
async fn retire(client: &Client, account: Uuid) {
    let started = std::time::Instant::now();
    let retired: serde_json::Value = client
        .query_one("SELECT app.retire_account($1)", &[&account])
        .await
        .unwrap()
        .get(0);
    assert_eq!(retired["status"], "success");
    assert_eq!(retired["data"]["state"], "retired");
    eprintln!(
        "canonical fixture retirement completed in {:?}",
        started.elapsed()
    );
}
fn peer_back(value: &serde_json::Value, seat: Uuid) -> Uuid {
    assert_eq!(value["status"], "success");
    let seat = value["data"]["presentation"]["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["seatId"] == json!(seat))
        .unwrap();
    seat["appearance"]["backId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}
pub(super) fn run(
    r: &Reservation,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    phase: &str,
) {
    assert!(matches!(
        phase,
        "deletion-before" | "deletion-during" | "deletion-after"
    ));
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
    let owner_token = std::env::var("SPELLWAKE_NATIVE_STORAGE_CALLER").unwrap();
    let peer_token = std::env::var("SPELLWAKE_NATIVE_SOURCE_PEER_BEARER").unwrap();
    let database = std::env::var("SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL").unwrap();
    assert_eq!(
        reqwest::Url::parse(&database).unwrap().host_str(),
        Some("127.0.0.1")
    );
    let free: crate::cosmetics::AppearanceSelection =
        serde_json::from_str(&std::env::var("SPELLWAKE_NATIVE_PRESENTATION_FREE").unwrap())
            .unwrap();
    let calls = Arc::new(Calls::default());
    let proxy = fault_proxy::Proxy::new(&base, None, calls.clone());
    let probe = NativeStreamProbe::new(http::configuration(
        proxy.url(),
        &key,
        &base,
        |runtime| runtime,
        Some(NativeHostOwner::new("fixture:source-reader").unwrap()),
    ));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (client,connection)=tokio_postgres::connect(&database,NoTls).await.unwrap();let connection=tokio::spawn(connection);
        let row=client.query_one("SELECT s.id,s.account_id,d.id FROM app.match_seat s JOIN app.deck_revision r ON r.id=s.source_revision_id JOIN app.deck d ON d.id=r.deck_id WHERE s.match_id=$1 AND s.id<>$2",&[&r.match_id,&r.seat_ids[0]]).await.unwrap();
        let (peer_seat,peer,deck):(Uuid,Uuid,Uuid)=(row.get(0),row.get(1),row.get(2));
        let owner:Uuid=client.query_one("SELECT account_id FROM app.match_seat WHERE id=$1",&[&r.seat_ids[0]]).await.unwrap().get(0);
        let distinct=Uuid::new_v4();
        client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES($1,'back','test-only:historical-free','active',true)",&[&distinct]).await.unwrap();
        client.execute("INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,state) VALUES($1,$2,'free','active')",&[&peer,&distinct]).await.unwrap();
        let selected:serde_json::Value=client.query_one("SELECT app.change_deck_appearance($1,$2,$3,sleeve_id,dice_id,NULL,$4) FROM app.deck WHERE id=$2",&[&peer,&deck,&distinct,&Uuid::new_v4()]).await.unwrap().get(0);
        assert_eq!(selected["status"],"success");assert_ne!(distinct,free.back_id);
        let view=probe.request(request(&owner_token,r.game_id,"view",&json!({"gameId":r.game_id,"contextId":r.context_id}))).await;
        assert_eq!(view.status(),StatusCode::OK);let view=json_response(view).await;
        let command=PlayerCommand {request_id:Uuid::new_v4(),basis:serde_json::from_value(view["data"]["snapshot"]["basis"].clone()).unwrap(),action:PlayerAction::Concede {}};
        let accepted=probe.request(request(&owner_token,r.game_id,"commands",&command)).await;
        assert_eq!(accepted.status(),StatusCode::ACCEPTED);assert_eq!(json_response(accepted).await["status"],"success");
        let row=client.query_one("SELECT r.id,m.revision::text FROM app.game_result r JOIN app.game g ON g.id=r.game_id JOIN app.match m ON m.id=g.match_id WHERE g.id=$1",&[&r.game_id]).await.unwrap();
        let (result,revision):(Uuid,String)=(row.get(0),row.get(1));
        let consumed:serde_json::Value=client.query_one("SELECT app.consume_game_outcome($1,$2::text::numeric)",&[&result,&revision]).await.unwrap().get(0);
        assert_eq!(consumed["data"]["phase"],"terminal");
        let drained:serde_json::Value=client.query_one("SELECT app.consume_quest_game_activity($1,100)",&[&r.game_id]).await.unwrap().get(0);
        assert_eq!(drained["status"],"success");
        let initial_cut=cut(r.game_id).await;
        let mut body=open(&probe,&owner_token,r).await;
        let baseline=frame(&mut body).await;assert_eq!(peer_back(&baseline,peer_seat),distinct);drop(body);
        deletion_request(&probe,&peer_token).await;
        let mut body=open(&probe,&owner_token,r).await;
        let observed=match phase {
            "deletion-before"=>{retire(&client,peer).await;frame(&mut body).await}
            "deletion-during"=>{
                client.batch_execute("BEGIN; LOCK TABLE app.cosmetic IN ACCESS EXCLUSIVE MODE").await.unwrap();
                let task=tokio::spawn(async move {let value=frame(&mut body).await;(body,value)});
                wait_for_real_observation(&client).await;retire(&client,peer).await;
                client.batch_execute("COMMIT").await.unwrap();
                let (returned,value)=task.await.unwrap();body=returned;value
            }
            "deletion-after"=>{
                let entered=calls.arm_after_presentation();
                let task=tokio::spawn(async move {let value=frame(&mut body).await;(body,value)});
                tokio::time::timeout(Duration::from_secs(4),entered).await.unwrap().unwrap();
                retire(&client,peer).await;calls.resume();
                let (returned,value)=task.await.unwrap();body=returned;value
            }
            _=>unreachable!(),
        };
        assert_eq!(peer_back(&observed,peer_seat),if phase=="deletion-before" {free.back_id} else {distinct});
        let next=frame(&mut body).await;assert_eq!(peer_back(&next,peer_seat),free.back_id);
        assert_eq!(baseline["data"]["snapshot"],next["data"]["snapshot"]);
        assert_eq!(baseline["data"]["cursor"],next["data"]["cursor"]);
        drop(body);assert_eq!(probe.available(),(16,16));

        // The requesting account must lose publication rights after retirement;
        // an accepted old presentation is not a permission lease. The accepted
        // deletion intent precedes the observation; only retirement crosses it.
        deletion_request(&probe,&owner_token).await;
        let mut body=open(&probe,&owner_token,r).await;
        let entered=calls.arm_pending_after_presentation();
        let task=tokio::spawn(async move {let value=frame(&mut body).await;(body,value)});
        tokio::time::timeout(Duration::from_secs(4),entered).await.unwrap().unwrap();
        retire(&client,owner).await;calls.resume();
        let (mut body,value)=task.await.unwrap();
        assert_eq!(value,json!({"status":"fail","data":{"session":"unauthorized"}}));
        assert!(body.next().await.is_none());drop(body);assert_eq!(probe.available(),(16,16));
        assert_eq!(cut(r.game_id).await,initial_cut,"delivery and account retirement must not mutate native state or clocks");
        drop(client);connection.await.unwrap().unwrap();
    });
    let source = repository.read(r.game_id).unwrap().unwrap();
    assert_eq!(source.accepted_revision(), &Amount::from(1));
    println!(
        "OWNED_NATIVE_SOURCE_READER_OK {}",
        json!({
            "phase":phase,"actualNativeTransitionEvidence":true,"initialVersion2":true,"selectedPrintingProvenance":true,
            "selectedSourceActualSql":true,"immutableSourceDisplay":true,"actualTerminalCommand":true,
            "canonicalOutcomeConsumed":true,"peerRetiredCanonically":true,"actualDeletedOwnerReplacement":true,
            "actualCallerDeletionRefused":true,"sourceUnchangedAcrossReads":true,"fullyQualifiedCards":0,
            "semanticFactsEventsQualified":false,"deckFormatAdmissionQualified":false,
            "finalSourceFingerprint":source_fingerprint(&repository,r.game_id).iter().map(|v|format!("{v:02x}")).collect::<String>(),
        })
    );
}
