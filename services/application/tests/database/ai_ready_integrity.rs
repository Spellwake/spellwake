//! Normalized browser/provider confirmations: no real credentials, browser or hosted process qualification.
use super::{TestResult, connection, room_start_integrity};
use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use uuid::Uuid;
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn ai_labels_are_insufficient_and_epoch_or_grant_changes_invalidate_ready_and_countdown()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let row=c.query_one("SELECT b.id,g.id,b.room_slot_id FROM app.ai_runner_binding b JOIN app.controller_grant g ON g.binding_id=b.id JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE slot.room_id=$1", &[&f.id]).await?;
    let binding: Uuid = row.get(0);
    let grant: Uuid = row.get(1);
    let slot: Uuid = row.get(2);
    assert!(
        c.query_one("SELECT app.ai_slot_ready($1)", &[&slot])
            .await?
            .get::<_, bool>(0)
    );
    let basis: String = c
        .query_one("SELECT revision::text FROM app.room WHERE id=$1", &[&f.id])
        .await?
        .get(0);
    c.query_one(
        "SELECT app.set_room_ready($1,$2,$3::text::numeric,true,$4,'fixture:human-deck',$5)",
        &[
            &f.source.actor,
            &f.id,
            &basis,
            &f.source.revision,
            &Uuid::new_v4(),
        ],
    )
    .await?;
    assert_eq!(
        c.query_one("SELECT state FROM app.room WHERE id=$1", &[&f.id])
            .await?
            .get::<_, String>(0),
        "countdown"
    );
    c.execute(
        "UPDATE app.controller_grant SET state='revoked' WHERE id=$1",
        &[&grant],
    )
    .await?;
    assert!(
        !c.query_one("SELECT app.ai_slot_ready($1)", &[&slot])
            .await?
            .get::<_, bool>(0)
    );
    assert_eq!(
        c.query_one("SELECT state FROM app.room WHERE id=$1", &[&f.id])
            .await?
            .get::<_, String>(0),
        "lobby"
    );
    c.execute("UPDATE app.room_slot SET ready=true WHERE id=$1", &[&slot])
        .await?;
    assert!(
        !c.query_one("SELECT app.ai_slot_ready($1)", &[&slot])
            .await?
            .get::<_, bool>(0)
    );
    let basis: String = c
        .query_one("SELECT revision::text FROM app.room WHERE id=$1", &[&f.id])
        .await?
        .get(0);
    let forced: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:decks',$4)",
            &[&f.source.actor, &f.id, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(forced["status"], "fail");
    assert_eq!(
        c.execute(
            "UPDATE app.ai_runner_binding SET binding_epoch=2 WHERE id=$1",
            &[&binding]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let denied:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck','fixture:authenticated',(SELECT configuration_id FROM app.room WHERE id=$4),$3)", &[&grant,&f.source.revision,&Uuid::new_v4(),&f.id]).await?.get(0);
    assert_eq!(denied["status"], "fail");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn ai_ready_confirmation_is_source_once_only_and_a_stopped_runner_cannot_keep_live_grants()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let row=c.query_one("SELECT b.id,g.id,b.room_slot_id FROM app.ai_runner_binding b JOIN app.controller_grant g ON g.binding_id=b.id JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE slot.room_id=$1", &[&f.id]).await?;
    let binding: Uuid = row.get(0);
    let grant: Uuid = row.get(1);
    let slot: Uuid = row.get(2);
    let request = Uuid::new_v4();
    let first:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck','fixture:authenticated',(SELECT configuration_id FROM app.room WHERE id=$4),$3)", &[&grant,&f.source.revision,&request,&f.id]).await?.get(0);
    assert_eq!(first["status"], "success");
    let repeated:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck','fixture:authenticated',(SELECT configuration_id FROM app.room WHERE id=$4),$3)", &[&grant,&f.source.revision,&request,&f.id]).await?.get(0);
    assert_eq!(first, repeated);
    assert_eq!(
        c.query_one(
            "SELECT app.commit_ai_ready_confirmation($1,1,$2,'changed','fixture:authenticated',(SELECT configuration_id FROM app.room WHERE id=$4),$3)",
            &[&grant, &f.source.revision, &request, &f.id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.ai_runner_binding SET desired_state='stopped' WHERE id=$1",
            &[&binding]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.execute(
        "UPDATE app.controller_grant SET state='revoked' WHERE id=$1",
        &[&grant],
    )
    .await?;
    assert_eq!(c.execute("UPDATE app.ai_runner_binding SET desired_state='stopped',observed_state='stopped' WHERE id=$1", &[&binding]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    let cause:Uuid=c.query_one("SELECT id FROM app.command_receipt WHERE controller_grant_id=$1 AND request_ref=$2 AND scope='ai_ready_v1'", &[&grant,&request.to_string()]).await?.get(0);
    let stop: Uuid = c
        .query_one("SELECT app.request_ai_stop($1,$2)", &[&binding, &cause])
        .await?
        .get(0);
    let observed: Value = c
        .query_one(
            "SELECT app.observe_ai_stop($1,1,$2,'fixture:verified-stop',$3)",
            &[&binding, &stop, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(observed["status"], "success");
    assert!(
        !c.query_one("SELECT app.ai_slot_ready($1)", &[&slot])
            .await?
            .get::<_, bool>(0)
    );
    let repeated:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck','fixture:authenticated',(SELECT configuration_id FROM app.room WHERE id=$4),$3)", &[&grant,&f.source.revision,&request,&f.id]).await?.get(0);
    assert_eq!(repeated, first);
    assert!(
        !c.query_one("SELECT app.ai_slot_ready($1)", &[&slot])
            .await?
            .get::<_, bool>(0)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn host_force_requires_every_configured_seat_even_when_profile_minimum_is_smaller()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, true).await?;
    let profile:Uuid=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref) VALUES($1,'fixture','fixture',2,4,'fixture') RETURNING id", &[&Uuid::new_v4().to_string()]).await?.get(0);
    let teams: Vec<Value> = c
        .query("SELECT id FROM app.room_slot WHERE room_id=$1", &[&f.id])
        .await?
        .iter()
        .map(|r| json!({"slotId":r.get::<_,Uuid>(0),"teamRef":null}))
        .collect();
    let basis: String = c
        .query_one("SELECT revision::text FROM app.room WHERE id=$1", &[&f.id])
        .await?
        .get(0);
    c.query_one(
        "SELECT app.configure_room($1,$2,$3::text::numeric,$4,'bo1',$5,$6)",
        &[
            &f.source.actor,
            &f.id,
            &basis,
            &profile,
            &json!(teams),
            &Uuid::new_v4(),
        ],
    )
    .await?;
    let basis: String = c
        .query_one("SELECT revision::text FROM app.room WHERE id=$1", &[&f.id])
        .await?
        .get(0);
    let forced: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:deck',$4)",
            &[&f.source.actor, &f.id, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(forced["status"], "fail");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn delayed_ready_confirmation_cannot_reuse_an_old_room_configuration() -> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let original: Uuid = c
        .query_one(
            "SELECT configuration_id FROM app.room WHERE id=$1",
            &[&f.id],
        )
        .await?
        .get(0);
    let grant: Uuid = c.query_one("SELECT g.id FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1", &[&f.id]).await?.get(0);
    c.execute(
        "UPDATE app.room SET revision=revision+1 WHERE id=$1",
        &[&f.id],
    )
    .await?;
    let unchanged: Uuid = c
        .query_one(
            "SELECT configuration_id FROM app.room WHERE id=$1",
            &[&f.id],
        )
        .await?
        .get(0);
    assert_eq!(original, unchanged);
    c.execute("UPDATE app.room_slot SET team_ref='changed-team' WHERE room_id=$1 AND controller_kind='ai'", &[&f.id]).await?;
    let current: Uuid = c
        .query_one(
            "SELECT configuration_id FROM app.room WHERE id=$1",
            &[&f.id],
        )
        .await?
        .get(0);
    assert_ne!(original, current);
    let denied: Value = c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck','fixture:authenticated',$3,$4)", &[&grant,&f.source.revision,&original,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(denied["status"], "fail");
    assert_eq!(denied["data"]["configuration"], "stale");
    let accepted: Value = c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck','fixture:authenticated',$3,$4)", &[&grant,&f.source.revision,&current,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(accepted["status"], "success");
    Ok(())
}
