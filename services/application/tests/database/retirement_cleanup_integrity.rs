//! Canonical deletion intent drives private domain cleanup; no remote deletion is asserted.
use super::{TestResult, connection, deletion, room_start_integrity};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
async fn cleanup(
    c: &Client,
    actor: Uuid,
    kind: &str,
    target: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let response: Value = c
        .query_one(
            "SELECT app.cancel_deletion_work($1,$2,$3)",
            &[&actor, &kind, &target],
        )
        .await?
        .get(0);
    if response["status"] == "success" {
        let original = spellwake_application::deletion::CleanupWork {
            kind: serde_json::from_value(json!(kind)).unwrap(),
            id: target,
        };
        let parsed: spellwake_application::ApiResponse<
            spellwake_application::deletion::CleanupAck,
        > = spellwake_application::decode_wire_object(&serde_json::to_vec(&response).unwrap())
            .unwrap();
        assert!(
            matches!(parsed, spellwake_application::ApiResponse::Success { data } if data.matches(&original))
        );
    }
    Ok(response)
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn anchored_host_cleanup_preserves_history_and_leaves_unknown_provider_stop_outstanding()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let basis = room_start_integrity::basis(&c, f.id).await?;
    let invite: Value = c
        .query_one(
            "SELECT app.issue_room_invitation($1,$2,$3::text::numeric,encode(sha256(convert_to($4::uuid::text,'UTF8')),'hex'),$4)",
            &[&f.source.actor, &f.id, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(invite["status"], "success");
    let invitation: Uuid = invite["data"]["resourceId"].as_str().unwrap().parse()?;
    let row=c.query_one("SELECT s.id,b.id FROM app.room_slot s JOIN app.ai_runner_binding b ON b.room_slot_id=s.id WHERE s.room_id=$1", &[&f.id]).await?;
    let slot: Uuid = row.get(0);
    let binding: Uuid = row.get(1);
    assert_eq!(
        deletion::request(&c, f.source.actor, Uuid::new_v4()).await?["status"],
        "success"
    );
    let work: Value = c
        .query_one("SELECT app.read_deletion_work($1,100)", &[&f.source.actor])
        .await?
        .get(0);
    let typed_work: Vec<spellwake_application::deletion::CleanupWork> = work
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            spellwake_application::decode_wire_object(&serde_json::to_vec(item).unwrap()).unwrap()
        })
        .collect();
    spellwake_application::deletion::validate_cleanup_work(&typed_work, 100).unwrap();
    for kind in ["room", "invitation", "ai_slot"] {
        assert!(
            work.as_array().unwrap().iter().any(|w| w["kind"] == kind),
            "{work}"
        );
    }
    c.batch_execute("BEGIN").await?;
    assert_eq!(
        cleanup(&c, f.source.actor, "ai_slot", slot).await?["status"],
        "success"
    );
    c.batch_execute("ROLLBACK").await?;
    assert!(
        c.query_one(
            "SELECT retired_at IS NULL FROM app.room_slot WHERE id=$1",
            &[&slot]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(
        cleanup(&c, f.source.actor, "ai_slot", slot).await?["status"],
        "success"
    );
    assert_eq!(
        cleanup(&c, f.source.actor, "room", f.id).await?["status"],
        "success"
    );
    assert_eq!(
        cleanup(&c, f.source.actor, "invitation", invitation).await?["status"],
        "success"
    );
    assert_eq!(
        c.query_one("SELECT app.read_deletion_work($1,100)", &[&f.source.actor])
            .await?
            .get::<_, Value>(0),
        json!([])
    );
    assert!(
        c.query_one("SELECT state='closed' FROM app.room WHERE id=$1", &[&f.id])
            .await?
            .get::<_, bool>(0)
    );
    assert!(
        c.query_one(
            "SELECT state='canceled' FROM app.invitation WHERE id=$1",
            &[&invitation]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert!(c.query_one("SELECT desired_state='stopped' AND observed_state<>'stopped' FROM app.ai_runner_binding WHERE id=$1", &[&binding]).await?.get::<_,bool>(0));
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.read_ai_stop_work($1)",
            &[&binding]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt r WHERE r.account_id=$1 AND r.scope='deletion_cleanup_v2' AND r.state='accepted' AND r.normalized_source->>'intentId'=app.accepted_deletion_intent($1)::text", &[&f.source.actor]).await?.get::<_,i64>(0),3);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pending_guest_cleanup_vacates_only_the_guest_and_requires_actual_deletion_intent()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, true).await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.other]).await?;
    assert_eq!(
        c.query_one("SELECT app.read_deletion_work($1,100)", &[&f.other])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    assert_eq!(
        cleanup(&c, f.other, "room", f.id).await.unwrap_err().code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    assert_eq!(
        deletion::request(&c, f.other, Uuid::new_v4()).await?["status"],
        "success"
    );
    assert_eq!(
        cleanup(&c, f.other, "room", f.id).await?["status"],
        "success"
    );
    assert!(
        c.query_one(
            "SELECT state='lobby' AND host_id=$2 FROM app.room WHERE id=$1",
            &[&f.id, &f.source.actor]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
            &[&f.id, &f.other]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(
        c.query_one("SELECT app.read_deletion_work($1,100)", &[&f.other])
            .await?
            .get::<_, Value>(0),
        json!([])
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deletion_cancels_offered_ante_but_preserves_the_already_admitted_match() -> TestResult {
    let c = connection().await?;
    let (f, m) = super::ante_transaction_integrity::reserve(&c, true).await?;
    let coin = super::ante_transaction_integrity::fund(&c, f.source.actor).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&m, &f.source.actor],
        )
        .await?
        .get(0);
    let offered = super::ante_transaction_integrity::offer(
        &c,
        m,
        seat,
        "0",
        json!([{"assetId":coin,"quantity":"10"}]),
        None,
    )
    .await?;
    assert_eq!(offered["status"], "success");
    let commitment: Uuid = offered["data"]["resourceId"].as_str().unwrap().parse()?;
    assert_eq!(
        deletion::request(&c, f.source.actor, Uuid::new_v4()).await?["status"],
        "success"
    );
    assert_eq!(
        cleanup(&c, f.source.actor, "ante", commitment).await?["status"],
        "success"
    );
    assert!(
        c.query_one(
            "SELECT state='canceled' FROM app.ante_commitment WHERE id=$1",
            &[&commitment]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert!(
        c.query_one(
            "SELECT state='ready' AND reserved_game_id IS NOT NULL FROM app.match WHERE id=$1",
            &[&m]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.asset_hold WHERE ante_commitment_id=$1 AND state='held'", &[&commitment]).await?.get::<_,i64>(0),0);
    Ok(())
}
