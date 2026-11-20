use super::{TestResult, connection, packs, predictions, pricing::buyer, social::friend, trades};
use serde_json::{Value, json};
use spellwake_application::deletion::DeletionStatus;
use spellwake_application::deletion::{CleanupWork, validate_cleanup_work};
use uuid::Uuid;

/// Resolve synthetic fixture credentials as owner, then invoke the real command
/// under the original role. This helper is never installed in the application.
pub(super) async fn request<C: tokio_postgres::GenericClient + Sync>(
    client: &C,
    actor: Uuid,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let role: String = client
        .query_one("SELECT current_user::text", &[])
        .await?
        .get(0);
    assert!(matches!(
        role.as_str(),
        "postgres" | "service_role" | "anon" | "authenticated"
    ));
    client.batch_execute("RESET ROLE").await?;
    let source = client.query_one("SELECT a.auth_subject,(test_fixture.claims_for_subject(a.auth_subject)->>'session_id')::uuid FROM app.account a WHERE id=$1", &[&actor]).await;
    client.batch_execute(&format!("SET ROLE {role}")).await?;
    let source = source?;
    let subject: Uuid = source.get(0);
    let session: Uuid = source.get(1);
    Ok(client
        .query_one(
            "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
            &[&subject, &session, &request, &"a".repeat(64)],
        )
        .await?
        .get(0))
}

fn resource(value: &Value) -> Uuid {
    value["data"]["resourceId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deletion_observer_is_capability_bound_and_cleanup_alone_cannot_claim_finality()
-> TestResult {
    let mut client = connection().await?;
    let (actor, subject) = super::account(&client).await?;
    let session: Uuid = client
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0);
    let request = Uuid::new_v4();
    let hash = "a".repeat(64);
    let unavailable = json!({"status":"fail","data":{"observer":"unavailable"}});
    client.batch_execute("SET ROLE service_role").await?;
    let denied: Value = client
        .query_one(
            "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
            &[&subject, &Uuid::new_v4(), &request, &hash],
        )
        .await?
        .get(0);
    assert_eq!(
        denied,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    let accepted: Value = client
        .query_one(
            "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
            &[&subject, &session, &request, &hash],
        )
        .await?
        .get(0);
    assert_eq!(
        accepted,
        json!({"status":"success","data":{"resourceId":actor}})
    );
    let repeated: Value = client
        .query_one(
            "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
            &[&subject, &session, &request, &hash],
        )
        .await?
        .get(0);
    assert_eq!(repeated, accepted);
    let conflict: Value = client
        .query_one(
            "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
            &[&subject, &session, &request, &"b".repeat(64)],
        )
        .await?
        .get(0);
    assert_eq!(
        conflict,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    let requested: String = client
        .query_one(
            "SELECT deletion_requested_at::text FROM app.account WHERE id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    let fresh: Value = client
        .query_one(
            "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
            &[&subject, &session, &Uuid::new_v4(), &"b".repeat(64)],
        )
        .await?
        .get(0);
    assert_eq!(fresh, accepted);
    assert_eq!(
        client
            .query_one(
                "SELECT deletion_requested_at::text FROM app.account WHERE id=$1",
                &[&actor]
            )
            .await?
            .get::<_, String>(0),
        requested
    );
    let observed: Value = client
        .query_one(
            "SELECT app.observe_account_deletion($1,$2,$3)",
            &[&actor, &request, &hash],
        )
        .await?
        .get(0);
    assert_eq!(
        observed,
        json!({"status":"success","data":{"accountId":actor,"requestId":request,"phase":"pending"}})
    );
    for (account, intent, key) in [
        (Uuid::new_v4(), request, hash.clone()),
        (actor, Uuid::new_v4(), hash.clone()),
        (actor, request, "c".repeat(64)),
    ] {
        assert_eq!(
            client
                .query_one(
                    "SELECT app.observe_account_deletion($1,$2,$3)",
                    &[&account, &intent, &key]
                )
                .await?
                .get::<_, Value>(0),
            unavailable
        );
    }
    for key in ["A".repeat(64), "a".repeat(63)] {
        assert_eq!(
            client
                .query_one(
                    "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
                    &[&subject, &session, &Uuid::new_v4(), &key]
                )
                .await?
                .get::<_, Value>(0),
            json!({"status":"fail","data":{"request":"invalid"}})
        );
    }
    client.batch_execute("RESET ROLE").await?;
    for role in ["anon", "authenticated"] {
        client.batch_execute(&format!("SET ROLE {role}")).await?;
        assert_eq!(
            client
                .query_one(
                    "SELECT app.observe_account_deletion($1,$2,$3)",
                    &[&actor, &request, &hash]
                )
                .await
                .unwrap_err()
                .code(),
            Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
        );
        client.batch_execute("RESET ROLE").await?;
    }
    // Source fixture only: even real guard cleanup is insufficient without the
    // canonical finalizer's minimization/retirement receipt, which is not forged.
    let operation = super::auth_identity::mark_deleted(&client, actor).await?;
    client
        .query_one(
            "SELECT app.record_registered_auth_retirement($1,$2,$3)",
            &[&subject, &actor, &operation],
        )
        .await?;
    client.execute("UPDATE app.account SET auth_cleanup_subject=auth_subject,auth_subject=NULL WHERE id=$1", &[&actor]).await?;
    let tx = client.transaction().await?;
    super::auth_identity::canonical_delete(&tx, subject).await?;
    tx.commit().await?;
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        client
            .query_one(
                "SELECT app.observe_account_deletion($1,$2,$3)",
                &[&actor, &request, &hash]
            )
            .await?
            .get::<_, Value>(0),
        json!({"status":"success","data":{"accountId":actor,"requestId":request,"phase":"finalizing","retirementOperationId":operation}})
    );
    assert_eq!(
        client
            .query_one(
                "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
                &[&subject, &session, &Uuid::new_v4(), &hash]
            )
            .await?
            .get::<_, Value>(0),
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deletion_request_cancels_only_uncommitted_work_and_retains_filled_backing_and_payments()
-> TestResult {
    let client = connection().await?;
    let (_, actor, inventory) = packs::purchased(&client, 2).await?;
    let (bob, _) = buyer(&client).await?;
    friend(&client, actor, bob).await?;
    let outgoing = resource(&trades::offer(&client, actor, bob, 100, 100, Uuid::new_v4()).await?);
    let incoming = resource(&trades::offer(&client, bob, actor, 100, 100, Uuid::new_v4()).await?);
    let pack_asset: Uuid = client
        .query_one(
            "SELECT asset_definition_id FROM app.inventory_asset WHERE id=$1",
            &[&inventory],
        )
        .await?
        .get(0);
    client.batch_execute("SET ROLE service_role").await?;
    let listing = super::economy_commands::execute_for_actor(&client,actor,&json!({"kind":"listing_create","command":{"requestId":Uuid::new_v4(),"lot":{"assetId":pack_asset,"quantity":"1"},"totalPrice":"1000"}}),false).await?;
    assert_eq!(listing["status"], "success");
    let listing = resource(&listing);
    client.batch_execute("RESET ROLE").await?;
    let market = predictions::market(&client, bob, 3600).await?;
    let order = resource(
        &predictions::place(&client, actor, market, "yes", "60", "3", Uuid::new_v4()).await?,
    );
    let other_order =
        resource(&predictions::place(&client, bob, market, "no", "40", "1", Uuid::new_v4()).await?);
    let cosmetic = Uuid::new_v4();
    let purchase = Uuid::new_v4();
    client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state) VALUES($1,'back','test-only:paid','active')",&[&cosmetic]).await?;
    client.execute("INSERT INTO app.purchase(id,account_id,cosmetic_id,provider_request_ref,requested_at,state) VALUES($1,$2,$3,$1::uuid::text,clock_timestamp(),'creating')",&[&purchase,&actor,&cosmetic]).await?;
    let source=client.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1", &[&actor]).await?;
    let actor_subject: Uuid = source.get(0);
    let actor_session: Uuid = source.get(1);
    let source=client.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1", &[&bob]).await?;
    let bob_subject: Uuid = source.get(0);
    let bob_session: Uuid = source.get(1);
    client.batch_execute("SET ROLE service_role").await?;
    let blocked: Value = client
        .query_one(
            "SELECT app.execute_social_command($1,$2,$3)",
            &[&bob_subject, &bob_session, &json!({"kind":"block","command":{"requestId":Uuid::new_v4(),"otherAccountId":actor,"blocked":true}})],
        )
        .await?
        .get(0);
    assert_eq!(blocked["status"], "success");
    let request = Uuid::new_v4();
    let deleted: Value = super::deletion::request(&client, actor, request).await?;
    assert_eq!(deleted["status"], "success");
    assert_eq!(resource(&deleted), actor);
    let repeated: Value = super::deletion::request(&client, actor, request).await?;
    assert_eq!(deleted, repeated);
    let fresh: Value = super::deletion::request(&client, actor, Uuid::new_v4()).await?;
    assert_eq!(fresh, deleted);
    for input in [
        json!({"kind":"listing_create","command":{"requestId":Uuid::new_v4(),"lot":{"assetId":pack_asset,"quantity":"1"},"totalPrice":"1000"}}),
        json!({"kind":"pack_open","command":{"requestId":Uuid::new_v4(),"inventoryAssetId":inventory}}),
        json!({"kind":"trade_response","command":{"requestId":Uuid::new_v4(),"tradeId":incoming,"action":"accept"}}),
    ] {
        let result =
            super::economy_commands::execute_for_actor(&client, actor, &input, false).await?;
        assert_eq!(result["status"], "fail");
    }
    assert!(
        client
            .execute(
                "UPDATE app.account SET state='active',deletion_requested_at=NULL WHERE id=$1",
                &[&actor]
            )
            .await
            .is_err()
    );
    let blocked_change: Value = client
        .query_one(
            "SELECT app.execute_social_command($1,$2,$3)",
            &[&actor_subject, &actor_session, &json!({"kind":"block","command":{"requestId":Uuid::new_v4(),"otherAccountId":bob,"blocked":true}})],
        )
        .await?
        .get(0);
    assert_eq!(blocked_change["status"], "fail");
    assert!(
        client
            .query_one(
                "SELECT app.lock_contact_pair($1,$2,false,true)",
                &[&actor, &bob]
            )
            .await
            .is_err()
    );
    let work: Vec<CleanupWork> = serde_json::from_value(
        client
            .query_one("SELECT app.read_deletion_work($1,100)", &[&actor])
            .await?
            .get::<_, Value>(0),
    )?;
    validate_cleanup_work(&work, 100)?;
    assert_eq!(work.len(), 4);
    let first: Vec<CleanupWork> = serde_json::from_value(
        client
            .query_one("SELECT app.read_deletion_work($1,1)", &[&actor])
            .await?
            .get::<_, Value>(0),
    )?;
    validate_cleanup_work(&first, 1)?;
    assert_eq!(first.len(), 1);
    for item in &work {
        let kind = serde_json::to_value(item.kind)?;
        let result: Value = client
            .query_one(
                "SELECT app.cancel_deletion_work($1,$2,$3)",
                &[&actor, &kind.as_str().unwrap(), &item.id],
            )
            .await?
            .get(0);
        assert_eq!(result["status"], "success");
        let parsed: spellwake_application::ApiResponse<
            spellwake_application::deletion::CleanupAck,
        > = spellwake_application::decode_wire_object(&serde_json::to_vec(&result)?)?;
        assert!(
            matches!(parsed,spellwake_application::ApiResponse::Success{data} if data.matches(item))
        );
        let retry: Value = client
            .query_one(
                "SELECT app.cancel_deletion_work($1,$2,$3)",
                &[&actor, &kind.as_str().unwrap(), &item.id],
            )
            .await?
            .get(0);
        assert_eq!(retry, result);
    }
    let empty: Value = client
        .query_one("SELECT app.read_deletion_work($1,100)", &[&actor])
        .await?
        .get(0);
    assert_eq!(empty, json!([]));
    let status: DeletionStatus = serde_json::from_value(
        client
            .query_one("SELECT app.read_account_deletion_status($1)", &[&actor])
            .await?
            .get::<_, Value>(0),
    )?;
    assert_eq!(status.obligations.uncommitted_work.to_string(), "0");
    assert_eq!(
        status.obligations.unsettled_prediction_markets.to_string(),
        "1"
    );
    assert_eq!(status.obligations.purchases_in_flight.to_string(), "1");
    assert_eq!(status.obligations.held_commitments.to_string(), "1");
    assert_eq!(status.obligations.pending_commands.to_string(), "0");
    assert!(
        client
            .query_one("SELECT app.read_account_deletion_status($1)", &[&bob])
            .await
            .is_err()
    );
    assert!(
        client
            .query_one(
                "SELECT app.cancel_deletion_work($1,'prediction',$2)",
                &[&actor, &other_order]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(
        client
            .query_one(
                "SELECT state FROM app.trade_offer WHERE id=$1",
                &[&outgoing]
            )
            .await?
            .get::<_, String>(0),
        "canceled"
    );
    assert_eq!(
        client
            .query_one(
                "SELECT state FROM app.trade_offer WHERE id=$1",
                &[&incoming]
            )
            .await?
            .get::<_, String>(0),
        "rejected"
    );
    assert_eq!(
        client
            .query_one(
                "SELECT state FROM app.market_listing WHERE id=$1",
                &[&listing]
            )
            .await?
            .get::<_, String>(0),
        "canceled"
    );
    let backing=client.query_one("SELECT o.state,o.filled_quantity::text,l.quantity::text,h.state FROM app.prediction_order o JOIN app.asset_hold h ON h.id=o.hold_id JOIN app.hold_line l ON l.hold_id=h.id WHERE o.id=$1",&[&order]).await?;
    assert_eq!(
        (
            backing.get::<_, String>(0),
            backing.get::<_, String>(1),
            backing.get::<_, String>(2),
            backing.get::<_, String>(3)
        ),
        ("canceled".into(), "1".into(), "60".into(), "held".into())
    );
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.prediction_pair WHERE market_id=$1",
                &[&market]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    assert_eq!(
        client
            .query_one("SELECT state FROM app.purchase WHERE id=$1", &[&purchase])
            .await?
            .get::<_, String>(0),
        "creating"
    );
    let state=client.query_one("SELECT state,retirement_operation_id IS NULL,deletion_finalized_at IS NULL FROM app.account WHERE id=$1",&[&actor]).await?;
    assert_eq!(state.get::<_, String>(0), "deletion_pending");
    assert!(state.get::<_, bool>(1));
    assert!(state.get::<_, bool>(2));
    assert_eq!(client.query_one("SELECT owned::text,held::text,available::text FROM app.asset_availability WHERE id=$1",&[&inventory]).await?.get::<_,String>(2),"2");
    for role in ["anon", "authenticated"] {
        client.batch_execute(&format!("SET ROLE {role}")).await?;
        assert!(
            super::deletion::request(&client, actor, Uuid::new_v4())
                .await
                .is_err()
        );
        assert!(
            client
                .query_one("SELECT app.read_account_deletion_status($1)", &[&actor])
                .await
                .is_err()
        );
        assert!(
            client
                .query_one("SELECT app.read_deletion_work($1,100)", &[&actor])
                .await
                .is_err()
        );
        assert!(
            client
                .query_one(
                    "SELECT app.cancel_deletion_work($1,'trade',$2)",
                    &[&actor, &outgoing]
                )
                .await
                .is_err()
        );
        assert!(
            client
                .query_one("SELECT app.read_deletion_account_page(NULL,100)", &[])
                .await
                .is_err()
        );
        client.batch_execute("RESET ROLE").await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deletion_marker_serializes_with_waiting_market_purchase_before_cleanup() -> TestResult {
    let mut client = connection().await?;
    let (_, actor, inventory) = packs::purchased(&client, 1).await?;
    let (bob, _) = buyer(&client).await?;
    let asset: Uuid = client
        .query_one(
            "SELECT asset_definition_id FROM app.inventory_asset WHERE id=$1",
            &[&inventory],
        )
        .await?
        .get(0);
    client.batch_execute("SET ROLE service_role").await?;
    let listed = super::economy_commands::execute_for_actor(&client,actor,&json!({"kind":"listing_create","command":{"requestId":Uuid::new_v4(),"lot":{"assetId":asset,"quantity":"1"},"totalPrice":"1000"}}),false).await?;
    assert_eq!(listed["status"], "success");
    let listing = resource(&listed);
    client.batch_execute("RESET ROLE").await?;
    let transaction = client.transaction().await?;
    let deletion: Value = super::deletion::request(&transaction, actor, Uuid::new_v4()).await?;
    assert_eq!(deletion["status"], "success");
    let writer = connection().await?;
    let observer = connection().await?;
    let name = format!("deletion-buy-wait-{}", Uuid::new_v4());
    writer
        .query_one("SELECT set_config('application_name',$1,false)", &[&name])
        .await?;
    let task = tokio::spawn(async move {
        writer.batch_execute("SET ROLE service_role").await?;
        super::economy_commands::execute_for_actor(&writer,bob,&json!({"kind":"listing_buy","command":{"requestId":Uuid::new_v4(),"listingId":listing,"expectedRevision":"1","expectedTotal":"1000"}}),false).await
    });
    let mut waiting = false;
    for _ in 0..100 {
        waiting=observer.query_one("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock')",&[&name]).await?.get(0);
        if waiting {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        waiting,
        "buyer must actually wait on deletion's account fence"
    );
    transaction.commit().await?;
    let denied = task.await??;
    assert_eq!(denied["status"], "fail");
    assert_eq!(
        observer
            .query_one(
                "SELECT state FROM app.market_listing WHERE id=$1",
                &[&listing]
            )
            .await?
            .get::<_, String>(0),
        "listed"
    );
    let timestamp: String = observer
        .query_one(
            "SELECT deletion_requested_at::text FROM app.account WHERE id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    observer.batch_execute("SET ROLE service_role").await?;
    let (left, right) = tokio::join!(
        async { super::deletion::request(&observer, actor, Uuid::new_v4()).await },
        async { super::deletion::request(&client, actor, Uuid::new_v4()).await }
    );
    assert_eq!(left?, right?);
    let cleaned: Value = observer
        .query_one(
            "SELECT app.cancel_deletion_work($1,'listing',$2)",
            &[&actor, &listing],
        )
        .await?
        .get(0);
    assert_eq!(cleaned["status"], "success");
    observer.batch_execute("RESET ROLE").await?;
    assert_eq!(
        observer
            .query_one(
                "SELECT deletion_requested_at::text FROM app.account WHERE id=$1",
                &[&actor]
            )
            .await?
            .get::<_, String>(0),
        timestamp
    );
    assert_eq!(
        observer
            .query_one(
                "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                &[&inventory]
            )
            .await?
            .get::<_, String>(0),
        "1"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stale_account_snapshot_cannot_start_a_new_commitment_after_deletion_marker() -> TestResult
{
    let client = connection().await?;
    let (_, actor, inventory) = packs::purchased(&client, 1).await?;
    let mut stale = connection().await?;
    let transaction = stale
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    transaction
        .query_one("SELECT state FROM app.account WHERE id=$1", &[&actor])
        .await?;
    client.batch_execute("SET ROLE service_role").await?;
    let requested: Value = super::deletion::request(&client, actor, Uuid::new_v4()).await?;
    assert_eq!(requested["status"], "success");
    transaction.batch_execute("RESET ROLE").await?;
    let rejected = transaction
        .query_one(
            "SELECT app.open_pack($1,$2,$3)",
            &[&actor, &inventory, &Uuid::new_v4()],
        )
        .await
        .unwrap_err();
    assert_eq!(
        rejected.code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    transaction.rollback().await?;
    for limit in [0i32, 101] {
        assert!(
            client
                .query_one("SELECT app.read_deletion_work($1,$2)", &[&actor, &limit])
                .await
                .is_err()
        );
        assert!(
            client
                .query_one("SELECT app.read_deletion_account_page(NULL,$1)", &[&limit])
                .await
                .is_err()
        );
    }
    let page: Value = client
        .query_one("SELECT app.read_deletion_account_page(NULL,100)", &[])
        .await?
        .get(0);
    let ids: Vec<Uuid> = serde_json::from_value(page["items"].clone())?;
    assert!(ids.contains(&actor));
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        client
            .query_one(
                "SELECT app.cancel_deletion_work($1,'match',$2)",
                &[&actor, &Uuid::new_v4()]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(
        client
            .query_one(
                "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                &[&inventory]
            )
            .await?
            .get::<_, String>(0),
        "1"
    );
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.pack_open_receipt WHERE inventory_asset_id=$1",
                &[&inventory]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deletion_status_distinguishes_known_proofs_and_does_not_infer_match_finality() -> TestResult
{
    let client = connection().await?;
    let (actor, _) = buyer(&client).await?;
    let (bob, _) = buyer(&client).await?;
    let requested: Value = super::deletion::request(&client, actor, Uuid::new_v4()).await?;
    assert_eq!(requested["status"], "success");
    super::social::participation_pair(&client, actor, bob, "open", 0, 3).await?;
    let match_id: Uuid = client
        .query_one(
            "SELECT match_id FROM app.match_seat WHERE account_id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    client.execute("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,2,'ai')",&[&match_id,&actor]).await?;
    let verified = Uuid::new_v4();
    let received = Uuid::new_v4();
    for (id, state) in [(verified, "verified"), (received, "received")] {
        client.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state) VALUES($1,'patreon',$1::uuid::text,$2,clock_timestamp(),'test-only:payment','test-only:proof',$3)",&[&id,&actor,&state]).await?;
    }
    let request = Uuid::new_v4();
    let pending: Uuid = client
        .query_one(
            "SELECT (app.begin_account_command($1,$2,'test_pending',$1::uuid::text,'{}')).id",
            &[&actor, &request],
        )
        .await?
        .get(0);
    client.batch_execute("SET ROLE service_role").await?;
    let status: Value = client
        .query_one("SELECT app.read_account_deletion_status($1)", &[&actor])
        .await?
        .get(0);
    let parsed: DeletionStatus = serde_json::from_value(status.clone())?;
    assert_eq!(parsed.obligations.matches_without_result.to_string(), "1");
    assert_eq!(parsed.obligations.verified_patreon_events.to_string(), "1");
    assert_eq!(parsed.obligations.pending_commands.to_string(), "1");
    assert_eq!(parsed.obligations.uncommitted_work.to_string(), "0");
    assert!(status.get("readyToFinalize").is_none());
    assert!(status.get("authSubject").is_none());
    client.batch_execute("RESET ROLE").await?;
    // Source fixture only: the runtime host must provide its actual terminal proof.
    client.execute("INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at) VALUES($1,$1::uuid::text,'draw','test-only:none',clock_timestamp())",&[&match_id]).await?;
    client
        .execute(
            "UPDATE app.provider_event SET processing_state='processed' WHERE id=$1",
            &[&verified],
        )
        .await?;
    client
        .query_one(
            "SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid))",
            &[&pending, &actor],
        )
        .await?;
    client.batch_execute("SET ROLE service_role").await?;
    let status: DeletionStatus = serde_json::from_value(
        client
            .query_one("SELECT app.read_account_deletion_status($1)", &[&actor])
            .await?
            .get::<_, Value>(0),
    )?;
    assert_eq!(status.obligations.matches_without_result.to_string(), "0");
    assert_eq!(status.obligations.verified_patreon_events.to_string(), "0");
    assert_eq!(status.obligations.pending_commands.to_string(), "0");
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(
        client
            .query_one("SELECT state FROM app.account WHERE id=$1", &[&actor])
            .await?
            .get::<_, String>(0),
        "deletion_pending"
    );
    Ok(())
}
