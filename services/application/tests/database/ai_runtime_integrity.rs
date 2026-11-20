//! Source-qualified SQL runner lifecycle; no real provider, secret store or browser is activated.
use super::{TestResult, connection, match_lifecycle_integrity, room_start_integrity};
use serde_json::Value;
use tokio_postgres::error::SqlState;
use uuid::Uuid;
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_adoption_revokes_old_grant_and_opening_waits_for_new_delivery_and_readiness()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let old=c.query_one("SELECT b.id,g.id,b.binding_epoch::text FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.controller_grant g ON g.binding_id=b.id WHERE s.room_id=$1 AND g.state='active'", &[&f.id]).await?;
    let binding: Uuid = old.get(0);
    let old_grant: Uuid = old.get(1);
    assert_eq!(old.get::<_, String>(2), "1");
    let basis = room_start_integrity::basis(&c, f.id).await?;
    let forced: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:qualified',$4)",
            &[&f.source.actor, &f.id, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(forced["status"], "success");
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&f.id],
    )
    .await?;
    let started = room_start_integrity::start(&c, &f, Uuid::new_v4()).await?;
    assert_eq!(started["status"], "success", "{started}");
    let m: Uuid = started["data"]["resourceId"].as_str().unwrap().parse()?;
    let g: Uuid = started["data"]["gameId"].as_str().unwrap().parse()?;
    let proof = serde_json::json!({"contextId":g,"snapshotRef":"fixture:context","contextRevision":"fixture:v1","stateRef":"fixture:state","factsRef":"fixture:facts","eventRef":"fixture:opening","qualificationRef":"fixture:normalized-only"});
    let basis: String = c
        .query_one("SELECT revision::text FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    assert_eq!(
        c.query_one(
            "SELECT app.accept_reserved_game($1,$2::text::numeric,$3,'fixture:lifecycle',$4)",
            &[&m, &basis, &g, &proof]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    let row=c.query_one("SELECT g.id,g.delivery_state,b.binding_epoch::text,b.ready_epoch IS NULL FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id WHERE b.id=$1 AND g.state='active'", &[&binding]).await?;
    let grant: Uuid = row.get(0);
    assert_eq!(row.get::<_, String>(1), "pending");
    assert_eq!(row.get::<_, String>(2), "2");
    assert!(row.get::<_, bool>(3));
    assert_eq!(
        c.query_one(
            "SELECT state FROM app.controller_grant WHERE id=$1",
            &[&old_grant]
        )
        .await?
        .get::<_, String>(0),
        "revoked"
    );
    let early_request = Uuid::new_v4();
    let configuration: Uuid = c
        .query_one(
            "SELECT configuration_id FROM app.room WHERE id=$1",
            &[&f.id],
        )
        .await?
        .get(0);
    let early:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,2,$2,'fixture:deck','fixture:browser',(SELECT configuration_id FROM app.room WHERE id=$3),$4)", &[&grant,&f.source.revision,&f.id,&early_request]).await?.get(0);
    assert_eq!(early["status"], "fail");
    assert!(
        spellwake_application::controller_ports::ReadyConfirmation {
            grant_id: grant,
            epoch: 2.into(),
            revision_id: f.source.revision,
            qualification_ref: "fixture:deck".parse()?,
            authentication_ref: "fixture:browser".parse()?,
            configuration_id: configuration,
            request_id: early_request
        }
        .decode_response(early.to_string().as_bytes(), binding)
        .is_ok()
    );
    let request = Uuid::new_v4();
    let unknown: Value = c
        .query_one(
            "SELECT app.observe_ai_grant_delivery($1,0,'unknown','fixture:lost-ack',$2)",
            &[&grant, &request],
        )
        .await?
        .get(0);
    assert_eq!(unknown["status"], "success");
    assert!(
        spellwake_application::controller_ports::GrantDeliveryObservation {
            grant_id: grant,
            basis: 0.into(),
            outcome: spellwake_application::controller_ports::DeliveryOutcome::Unknown,
            proof_ref: "fixture:lost-ack".parse()?,
            request_id: request
        }
        .decode_response(unknown.to_string().as_bytes())
        .is_ok()
    );
    assert_eq!(
        c.query_one(
            "SELECT app.observe_ai_grant_delivery($1,0,'unknown','fixture:lost-ack',$2)",
            &[&grant, &request]
        )
        .await?
        .get::<_, Value>(0),
        unknown
    );
    let delivered: Value = c
        .query_one(
            "SELECT app.observe_ai_grant_delivery($1,1,'delivered','fixture:reconciled',$2)",
            &[&grant, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(delivered["status"], "success");
    let confirmed:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,2,$2,'fixture:deck','fixture:browser',(SELECT configuration_id FROM app.room WHERE id=$3),$4)", &[&grant,&f.source.revision,&f.id,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(confirmed["status"], "success", "{confirmed}");
    assert_eq!(
        match_lifecycle_integrity::open(&c, m, g).await?["status"],
        "success"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn removed_ai_frees_capacity_keeps_stop_work_and_old_callbacks_cannot_control_replacement()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let row=c.query_one("SELECT s.id,b.id,b.bootstrap_receipt_id,g.id FROM app.room_slot s JOIN app.ai_runner_binding b ON b.room_slot_id=s.id JOIN app.controller_grant g ON g.binding_id=b.id WHERE s.room_id=$1 AND s.controller_kind='ai' AND g.state='active'", &[&f.id]).await?;
    let slot: Uuid = row.get(0);
    let binding: Uuid = row.get(1);
    let start: Uuid = row.get(2);
    let grant: Uuid = row.get(3);
    let basis = room_start_integrity::basis(&c, f.id).await?;
    let request = Uuid::new_v4();
    let removed: Value = c
        .query_one(
            "SELECT app.remove_ai_slot($1,$2,$3::text::numeric,$4)",
            &[&f.source.actor, &slot, &basis, &request],
        )
        .await?
        .get(0);
    assert_eq!(removed["status"], "success", "{removed}");
    assert_eq!(
        c.query_one(
            "SELECT app.remove_ai_slot($1,$2,$3::text::numeric,$4)",
            &[&f.source.actor, &slot, &basis, &request]
        )
        .await?
        .get::<_, Value>(0),
        removed
    );
    let stop=c.query_one("SELECT stop_receipt_id,desired_state,observed_state FROM app.ai_runner_binding WHERE id=$1", &[&binding]).await?;
    let stop_id: Uuid = stop.get(0);
    assert_eq!(stop.get::<_, String>(1), "stopped");
    assert_ne!(stop.get::<_, String>(2), "stopped");
    let replacement:Uuid=c.query_one("INSERT INTO app.room_slot(room_id,sponsor_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'ai',false) RETURNING id", &[&f.id,&f.source.actor]).await?.get(0);
    let replacement_grant =
        room_start_integrity::bootstrap_ai(&c, replacement, f.source.actor, f.source.revision)
            .await?;
    let late:Value=c.query_one("SELECT app.observe_ai_provider($1,1,$2,'running','fixture:late-provider','fixture:late-proof',$3)", &[&binding,&start,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(late["status"], "fail");
    let late:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck','fixture:browser',(SELECT configuration_id FROM app.room WHERE id=$3),$4)", &[&grant,&f.source.revision,&f.id,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(late["status"], "fail");
    let observed: Value = c
        .query_one(
            "SELECT app.observe_ai_stop($1,1,$2,'fixture:verified-stop',$3)",
            &[&binding, &stop_id, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(observed["status"], "success");
    assert_eq!(
        c.query_one(
            "SELECT state FROM app.controller_grant WHERE id=$1",
            &[&replacement_grant]
        )
        .await?
        .get::<_, String>(0),
        "active"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND retired_at IS NULL",
            &[&f.id]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1",
            &[&f.id]
        )
        .await?
        .get::<_, i64>(0),
        3
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn bootstrap_rechecks_sponsor_after_waiting_for_the_original_account_fence() -> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let old:Uuid=c.query_one("SELECT id FROM app.room_slot WHERE room_id=$1 AND controller_kind='ai' AND retired_at IS NULL", &[&f.id]).await?.get(0);
    let basis = room_start_integrity::basis(&c, f.id).await?;
    let removed: Value = c
        .query_one(
            "SELECT app.remove_ai_slot($1,$2,$3::text::numeric,$4)",
            &[&f.source.actor, &old, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(removed["status"], "success");
    let slot:Uuid=c.query_one("INSERT INTO app.room_slot(room_id,sponsor_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'ai',false) RETURNING id", &[&f.id,&f.source.actor]).await?.get(0);
    let (other, _) = super::account(&c).await?;
    let basis = room_start_integrity::basis(&c, f.id).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
        &[&f.source.actor],
    )
    .await?;
    let actor = f.source.actor;
    let revision = f.source.revision;
    let task = tokio::spawn(async move {
        waiting
            .query_one(
                "SELECT app.bootstrap_ai_slot($1,$2,$3::text::numeric,$4,$5)",
                &[&actor, &slot, &basis, &revision, &Uuid::new_v4()],
            )
            .await
    });
    super::wait_for_database_lock(&c, pid).await?;
    c.execute(
        "UPDATE app.room_slot SET sponsor_id=$1 WHERE id=$2",
        &[&other, &slot],
    )
    .await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert!(
        c.query_opt(
            "SELECT id FROM app.ai_runner_binding WHERE room_slot_id=$1",
            &[&slot]
        )
        .await?
        .is_none()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn adoption_cannot_replace_missing_room_start_cause_with_an_accepted_unrelated_receipt()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let row=c.query_one("SELECT b.id,b.bootstrap_receipt_id FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1", &[&f.id]).await?;
    let binding: Uuid = row.get(0);
    let unrelated: Uuid = row.get(1);
    for cause in [None, Some(unrelated)] {
        c.batch_execute("BEGIN").await?;
        let m:Uuid=c.query_one("INSERT INTO app.match(room_id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) SELECT id,format_id,profile_id,$2,mode,competitive,structure,'admitting',0 FROM app.room WHERE id=$1 RETURNING id", &[&f.id,&f.legality]).await?.get(0);
        c.execute("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,1,'ai')", &[&m,&f.source.actor]).await?;
        c.query_one("SELECT app.adopt_room_ai($1,$2,$3)", &[&f.id, &m, &cause])
            .await?;
        assert_eq!(
            c.batch_execute("COMMIT").await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
        assert_eq!(
            c.query_one(
                "SELECT binding_epoch::text FROM app.ai_runner_binding WHERE id=$1",
                &[&binding]
            )
            .await?
            .get::<_, String>(0),
            "1"
        );
        assert!(
            c.query_opt("SELECT id FROM app.match WHERE id=$1", &[&m])
                .await?
                .is_none()
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pending_sponsor_can_replace_revoked_authority_and_finish_an_already_admitted_match()
-> TestResult {
    let c = connection().await?;
    let (f, m) = super::ante_transaction_integrity::reserve(&c, false).await?;
    let row=c.query_one("SELECT b.id,g.id FROM app.ai_runner_binding b JOIN app.match_seat seat ON seat.id=b.match_seat_id JOIN app.controller_grant g ON g.binding_id=b.id WHERE seat.match_id=$1 AND g.state='active'", &[&m]).await?;
    let binding: Uuid = row.get(0);
    let old: Uuid = row.get(1);
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.source.actor]).await?;
    c.execute(
        "UPDATE app.controller_grant SET state='revoked' WHERE id=$1",
        &[&old],
    )
    .await?;
    let issued: Value = c
        .query_one(
            "SELECT app.issue_ai_grant($1,2,'fixture:continuing-match-secret-reference:'||$1::uuid::text,$2)",
            &[&binding, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(issued["status"], "success", "{issued}");
    let grant: Uuid = issued["data"]["resourceId"].as_str().unwrap().parse()?;
    let delivered:Value=c.query_one("SELECT app.observe_ai_grant_delivery($1,0,'delivered','fixture:continuing-match-delivery',$2)", &[&grant,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(delivered["status"], "success");
    let ready:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,2,$2,'fixture:deck','fixture:browser',(SELECT configuration_id FROM app.room WHERE id=$3),$4)", &[&grant,&f.source.revision,&f.id,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(ready["status"], "success", "{ready}");
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    assert_eq!(
        match_lifecycle_integrity::open(&c, m, g).await?["status"],
        "success"
    );
    Ok(())
}
