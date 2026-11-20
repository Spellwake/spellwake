//! Normalized SQL process-generation recovery; external provider truth is not qualified here.
use super::{TestResult, connection, room_start_integrity};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

async fn rebind(
    c: &Client,
    binding: Uuid,
    epoch: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let response: Value = c
        .query_one(
            "SELECT app.rebind_ai_runner($1,$2::text::numeric,'fixture:qualified-restart',$3)",
            &[&binding, &epoch, &request],
        )
        .await?
        .get(0);
    let original = spellwake_application::controller_ports::RebindRunner {
        binding_id: binding,
        basis: epoch.parse().unwrap(),
        proof_ref: "fixture:qualified-restart".parse().unwrap(),
        request_id: request,
    };
    assert!(
        original
            .decode_response(response.to_string().as_bytes())
            .is_ok()
    );
    Ok(response)
}
async fn acknowledge_stop(c: &Client, binding: Uuid, epoch: &str, stop: Uuid) -> TestResult {
    let reader = spellwake_application::controller_ports::ReadStopWork {
        binding_id: binding,
        after: None,
        limit: 100,
    };
    let mut previous = None;
    for row in c
        .query(
            "SELECT work FROM app.read_ai_stop_work($1) work",
            &[&binding],
        )
        .await?
    {
        let value: Value = row.get(0);
        let work = reader.decode_row(value.to_string().as_bytes())?;
        assert!(reader.ordered_after(&work, previous));
        previous = Some(work.stop_id);
    }
    let request = Uuid::new_v4();
    let ack: Value = c
        .query_one(
            "SELECT app.observe_ai_stop($1,$2::text::numeric,$3,'fixture:stable-start-stop',$4)",
            &[&binding, &epoch, &stop, &request],
        )
        .await?
        .get(0);
    assert_eq!(ack["status"], "success", "{ack}");
    assert!(
        spellwake_application::controller_ports::StopObservation {
            binding_id: binding,
            epoch: epoch.parse()?,
            stop_id: stop,
            proof_ref: "fixture:stable-start-stop".parse()?,
            request_id: request,
        }
        .decode_response(ack.to_string().as_bytes())
        .is_ok()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_rebind_waits_for_old_stop_before_start_and_invalidates_old_authority() -> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let row=c.query_one("SELECT b.id,b.provider_start_receipt_id,b.provider_runner_ref,g.id FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.controller_grant g ON g.binding_id=b.id WHERE s.room_id=$1 AND g.state='active'", &[&f.id]).await?;
    let binding: Uuid = row.get(0);
    let original: Uuid = row.get(1);
    let process: String = row.get(2);
    let old_grant: Uuid = row.get(3);
    let request = Uuid::new_v4();
    let result = rebind(&c, binding, "1", request).await?;
    assert_eq!(result["status"], "success", "{result}");
    assert_eq!(rebind(&c, binding, "1", request).await?, result);
    let start: Uuid = result["data"]["startKey"].as_str().unwrap().parse()?;
    let stop: Uuid = result["data"]["previousStopId"].as_str().unwrap().parse()?;
    assert!(
        c.query_one("SELECT app.read_ai_start_work($1,2) IS NULL", &[&binding])
            .await?
            .get::<_, bool>(0)
    );
    let callback = Uuid::new_v4();
    assert_eq!(
        c.query_one(
            "SELECT app.observe_ai_provider($1,2,$2,'running',$3,'fixture:new-process',$4)",
            &[&binding, &start, &process, &callback]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert!(c.query_opt("SELECT id FROM app.command_receipt WHERE scope='ai_provider_ack_v1' AND request_ref=$1", &[&callback.to_string()]).await?.is_none());
    assert_eq!(
        rebind(&c, binding, "2", Uuid::new_v4())
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    let stale_provider_request = Uuid::new_v4();
    let stale: Value = c
        .query_one(
            "SELECT app.observe_ai_provider($1,1,$2,'running',$3,'fixture:old-callback',$4)",
            &[&binding, &original, &process, &stale_provider_request],
        )
        .await?
        .get(0);
    assert_eq!(stale["status"], "fail");
    assert!(
        spellwake_application::controller_ports::ProviderObservation {
            binding_id: binding,
            epoch: 1.into(),
            start_id: original,
            outcome: spellwake_application::controller_ports::ProviderOutcome::Running,
            provider_ref: Some(process.parse()?),
            proof_ref: "fixture:old-callback".parse()?,
            request_id: stale_provider_request
        }
        .decode_response(stale.to_string().as_bytes())
        .is_ok()
    );
    acknowledge_stop(&c, binding, "1", stop).await?;
    assert!(
        !c.query_one("SELECT app.read_ai_start_work($1,2) IS NULL", &[&binding])
            .await?
            .get::<_, bool>(0)
    );
    let running: Value = c
        .query_one(
            "SELECT app.observe_ai_provider($1,2,$2,'running',$3,'fixture:new-process',$4)",
            &[&binding, &start, &process, &callback],
        )
        .await?
        .get(0);
    assert_eq!(running["status"], "success", "{running}");
    assert!(c.query_one("SELECT observed_state='running' AND stop_receipt_id IS NULL AND ready_epoch IS NULL FROM app.ai_runner_binding WHERE id=$1", &[&binding]).await?.get::<_,bool>(0));
    let stale_delivery_request = Uuid::new_v4();
    let stale: Value = c
        .query_one(
            "SELECT app.observe_ai_grant_delivery($1,0,'delivered','fixture:late-old-delivery',$2)",
            &[&old_grant, &stale_delivery_request],
        )
        .await?
        .get(0);
    assert_eq!(stale["status"], "fail");
    assert!(
        spellwake_application::controller_ports::GrantDeliveryObservation {
            grant_id: old_grant,
            basis: 0.into(),
            outcome: spellwake_application::controller_ports::DeliveryOutcome::Delivered,
            proof_ref: "fixture:late-old-delivery".parse()?,
            request_id: stale_delivery_request
        }
        .decode_response(stale.to_string().as_bytes())
        .is_ok()
    );
    let issued: Value = c
        .query_one(
            "SELECT app.issue_ai_grant($1,2,'fixture:fresh-secret-reference:'||$1::uuid::text,$2)",
            &[&binding, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(issued["status"], "success");
    let grant: Uuid = issued["data"]["resourceId"].as_str().unwrap().parse()?;
    let delivery: Value = c
        .query_one(
            "SELECT app.observe_ai_grant_delivery($1,0,'delivered','fixture:new-delivery',$2)",
            &[&grant, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(delivery["status"], "success");
    let ready:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,2,$2,'fixture:deck','fixture:browser',(SELECT configuration_id FROM app.room WHERE id=$3),$4)", &[&grant,&f.source.revision,&f.id,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(ready["status"], "success", "{ready}");
    acknowledge_stop(&c, binding, "1", stop).await?;
    assert!(
        c.query_one(
            "SELECT app.ai_slot_ready(room_slot_id) FROM app.ai_runner_binding WHERE id=$1",
            &[&binding]
        )
        .await?
        .get::<_, bool>(0)
    );
    let next = rebind(&c, binding, "2", Uuid::new_v4()).await?;
    assert_eq!(next["status"], "success", "{next}");
    assert_ne!(
        next["data"]["previousStopId"],
        result["data"]["previousStopId"]
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn adopted_rebind_preserves_seat_banks_and_admission_for_pending_sponsor() -> TestResult {
    let c = connection().await?;
    let (f, m) = super::ante_transaction_integrity::reserve(&c, false).await?;
    let row=c.query_one("SELECT b.id,b.match_seat_id,b.provider_start_receipt_id FROM app.ai_runner_binding b JOIN app.match_seat s ON s.id=b.match_seat_id WHERE s.match_id=$1", &[&m]).await?;
    let binding: Uuid = row.get(0);
    let seat: Uuid = row.get(1);
    let original: Uuid = row.get(2);
    let clocks: Value = c
        .query_one("SELECT app.match_clock_snapshot($1)", &[&m])
        .await?
        .get(0);
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.source.actor]).await?;
    let result = rebind(&c, binding, "2", Uuid::new_v4()).await?;
    assert_eq!(result["status"], "success", "{result}");
    let start: Uuid = result["data"]["startKey"].as_str().unwrap().parse()?;
    let stop: Uuid = result["data"]["previousStopId"].as_str().unwrap().parse()?;
    assert!(c.query_one("SELECT normalized_source->>'providerStartId'=$2::uuid::text AND normalized_source->>'processEpoch'='1' AND normalized_source->>'epoch'='2' FROM app.command_receipt WHERE id=$1", &[&stop,&original]).await?.get::<_,bool>(0));
    acknowledge_stop(&c, binding, "2", stop).await?;
    let running:Value=c.query_one("SELECT app.observe_ai_provider($1,3,$2,'running','fixture:rebound-provider','fixture:new-process',$3)", &[&binding,&start,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(running["status"], "success");
    assert_eq!(
        c.query_one(
            "SELECT match_seat_id FROM app.ai_runner_binding WHERE id=$1",
            &[&binding]
        )
        .await?
        .get::<_, Uuid>(0),
        seat
    );
    assert_eq!(
        c.query_one("SELECT app.match_clock_snapshot($1)", &[&m])
            .await?
            .get::<_, Value>(0),
        clocks
    );
    assert!(
        c.query_one(
            "SELECT accepted_release_id IS NOT NULL AND state='ready' FROM app.match WHERE id=$1",
            &[&m]
        )
        .await?
        .get::<_, bool>(0)
    );
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.ai_runner_binding SET epoch_receipt_id=bootstrap_receipt_id WHERE id=$1",
        &[&binding],
    )
    .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn rebind_outer_rollback_leaves_original_controller_and_no_stop_work() -> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let binding:Uuid=c.query_one("SELECT b.id FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1", &[&f.id]).await?.get(0);
    let request = Uuid::new_v4();
    c.batch_execute("BEGIN").await?;
    let result = rebind(&c, binding, "1", request).await?;
    assert_eq!(result["status"], "success");
    c.batch_execute("ROLLBACK").await?;
    assert!(c.query_one("SELECT binding_epoch=1 AND desired_state='running' AND observed_state='ready' AND stop_receipt_id IS NULL FROM app.ai_runner_binding WHERE id=$1", &[&binding]).await?.get::<_,bool>(0));
    assert!(!c.query_one("SELECT EXISTS(SELECT 1 FROM app.command_receipt WHERE scope='ai_stop_request_v1' AND target_ref=$1)", &[&binding.to_string()]).await?.get::<_,bool>(0));
    assert_eq!(
        rebind(&c, binding, "1", request).await?["status"],
        "success"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unknown_start_uses_stable_key_for_stop_and_cannot_skip_an_inherited_dependency()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let binding:Uuid=c.query_one("SELECT b.id FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1", &[&f.id]).await?.get(0);
    let first = rebind(&c, binding, "1", Uuid::new_v4()).await?;
    let old_stop: Uuid = first["data"]["previousStopId"].as_str().unwrap().parse()?;
    acknowledge_stop(&c, binding, "1", old_stop).await?;
    let start: Uuid = first["data"]["startKey"].as_str().unwrap().parse()?;
    let unknown: Value = c
        .query_one(
            "SELECT app.observe_ai_provider($1,2,$2,'unknown',NULL,'fixture:unknown-start',$3)",
            &[&binding, &start, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(unknown["status"], "success");
    let second = rebind(&c, binding, "2", Uuid::new_v4()).await?;
    assert_eq!(second["status"], "success");
    let stop: Uuid = second["data"]["previousStopId"].as_str().unwrap().parse()?;
    let work: Value = c
        .query_one(
            "SELECT work FROM app.read_ai_stop_work($1) work",
            &[&binding],
        )
        .await?
        .get(0);
    assert_eq!(work["stopId"], stop.to_string());
    assert_eq!(work["startId"], start.to_string());
    assert!(work["providerRef"].is_null());
    let typed = spellwake_application::controller_ports::ReadStopWork {
        binding_id: binding,
        after: None,
        limit: 100,
    }
    .decode_row(work.to_string().as_bytes())?;
    assert_eq!(typed.start_id, Some(start));
    assert!(typed.provider_ref.is_none());
    assert_eq!(
        rebind(&c, binding, "3", Uuid::new_v4())
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    acknowledge_stop(&c, binding, "2", stop).await?;
    assert!(
        !c.query_one(
            "SELECT EXISTS(SELECT 1 FROM app.read_ai_stop_work($1))",
            &[&binding]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert!(
        !c.query_one("SELECT app.read_ai_start_work($1,3) IS NULL", &[&binding])
            .await?
            .get::<_, bool>(0)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn terminal_and_rebind_share_parent_fences_in_both_lock_orders() -> TestResult {
    for terminal_first in [true, false] {
        let c = connection().await?;
        let (_, m) = super::ante_transaction_integrity::reserve(&c, false).await?;
        let binding:Uuid=c.query_one("SELECT b.id FROM app.ai_runner_binding b JOIN app.match_seat s ON s.id=b.match_seat_id WHERE s.match_id=$1", &[&m]).await?.get(0);
        let paused:Value=c.query_one("SELECT app.pause_match_between_games($1,(SELECT revision FROM app.match WHERE id=$1),(SELECT availability_revision FROM app.match WHERE id=$1),clock_timestamp(),'storage','fixture:incident',$2)", &[&m,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(paused["status"], "success", "{paused}");
        let incident: Uuid = paused["data"]["incidentId"].as_str().unwrap().parse()?;
        let revision = paused["data"]["revision"].as_str().unwrap().to_owned();
        let availability = paused["data"]["availabilityRevision"]
            .as_str()
            .unwrap()
            .to_owned();
        let waiting = connection().await?;
        let pid: i32 = waiting
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        c.batch_execute("BEGIN").await?;
        let task = if terminal_first {
            let result:Value=c.query_one("SELECT app.finalize_unrecoverable_match($1,$2::text::numeric,$3::text::numeric,$4,'fixture:verified-failed-restoration')", &[&m,&revision,&availability,&incident]).await?.get(0);
            assert_eq!(result["status"], "success");
            tokio::spawn(async move { rebind(&waiting, binding, "2", Uuid::new_v4()).await })
        } else {
            assert_eq!(
                rebind(&c, binding, "2", Uuid::new_v4()).await?["status"],
                "success"
            );
            tokio::spawn(async move {
                waiting.query_one("SELECT app.finalize_unrecoverable_match($1,$2::text::numeric,$3::text::numeric,$4,'fixture:verified-failed-restoration')", &[&m,&revision,&availability,&incident]).await.map(|r|r.get::<_,Value>(0))
            })
        };
        super::wait_for_database_lock(&c, pid).await?;
        c.batch_execute("COMMIT").await?;
        let result = task.await??;
        assert_eq!(
            result["status"],
            if terminal_first { "fail" } else { "success" },
            "{result}"
        );
        assert!(
            c.query_one(
                "SELECT desired_state='stopped' FROM app.ai_runner_binding WHERE id=$1",
                &[&binding]
            )
            .await?
            .get::<_, bool>(0)
        );
        assert!(c.query_one("SELECT app.read_ai_start_work($1,(SELECT binding_epoch FROM app.ai_runner_binding WHERE id=$1)) IS NULL", &[&binding]).await?.get::<_,bool>(0));
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pregame_rebind_adoption_cannot_rewind_to_a_stopped_process_with_the_same_label()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let row=c.query_one("SELECT b.id,b.provider_runner_ref,b.provider_observation_receipt_id FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1", &[&f.id]).await?;
    let binding: Uuid = row.get(0);
    let process: String = row.get(1);
    let observation: Uuid = row.get(2);
    let result = rebind(&c, binding, "1", Uuid::new_v4()).await?;
    assert_eq!(result["status"], "success");
    let start: Uuid = result["data"]["startKey"].as_str().unwrap().parse()?;
    let stop: Uuid = result["data"]["previousStopId"].as_str().unwrap().parse()?;
    acknowledge_stop(&c, binding, "1", stop).await?;
    let running:Value=c.query_one("SELECT app.observe_ai_provider($1,2,$2,'running',$3,'fixture:same-label-new-generation',$4)", &[&binding,&start,&process,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(running["status"], "success");
    let issued: Value = c
        .query_one(
            "SELECT app.issue_ai_grant($1,2,'fixture:next-secret-reference:'||$1::uuid::text,$2)",
            &[&binding, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(issued["status"], "success");
    let grant: Uuid = issued["data"]["resourceId"].as_str().unwrap().parse()?;
    let delivered: Value = c
        .query_one(
            "SELECT app.observe_ai_grant_delivery($1,0,'delivered','fixture:delivery',$2)",
            &[&grant, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(delivered["status"], "success");
    let ready:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,2,$2,'fixture:deck','fixture:browser',(SELECT configuration_id FROM app.room WHERE id=$3),$4)", &[&grant,&f.source.revision,&f.id,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(ready["status"], "success");
    let basis = room_start_integrity::basis(&c, f.id).await?;
    let countdown: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:qualified',$4)",
            &[&f.source.actor, &f.id, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(countdown["status"], "success");
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&f.id],
    )
    .await?;
    assert_eq!(
        room_start_integrity::start(&c, &f, Uuid::new_v4()).await?["status"],
        "success"
    );
    assert!(c.query_one("SELECT binding_epoch=3 AND provider_start_receipt_id=$2 FROM app.ai_runner_binding WHERE id=$1", &[&binding,&start]).await?.get::<_,bool>(0));
    assert_eq!(c.execute("UPDATE app.ai_runner_binding SET provider_start_receipt_id=bootstrap_receipt_id,provider_observation_receipt_id=$2 WHERE id=$1", &[&binding,&observation]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pending_deletion_prevents_a_new_pregame_start_but_keeps_stop_work_available() -> TestResult
{
    let c = connection().await?;
    let f = room_start_integrity::room(&c, false).await?;
    let binding:Uuid=c.query_one("SELECT b.id FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1", &[&f.id]).await?.get(0);
    let result = rebind(&c, binding, "1", Uuid::new_v4()).await?;
    let start: Uuid = result["data"]["startKey"].as_str().unwrap().parse()?;
    let stop: Uuid = result["data"]["previousStopId"].as_str().unwrap().parse()?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.source.actor]).await?;
    assert!(
        c.query_one(
            "SELECT EXISTS(SELECT 1 FROM app.read_ai_stop_work($1))",
            &[&binding]
        )
        .await?
        .get::<_, bool>(0)
    );
    acknowledge_stop(&c, binding, "1", stop).await?;
    assert!(
        c.query_one("SELECT app.read_ai_start_work($1,2) IS NULL", &[&binding])
            .await?
            .get::<_, bool>(0)
    );
    let running:Value=c.query_one("SELECT app.observe_ai_provider($1,2,$2,'running','fixture:forbidden-after-deletion','fixture:late-start',$3)", &[&binding,&start,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(running["status"], "fail");
    assert_eq!(
        rebind(&c, binding, "2", Uuid::new_v4()).await?["status"],
        "fail"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn secret_generation_cannot_be_shared_or_reused_after_authority_revocation() -> TestResult {
    let c = connection().await?;
    let a = room_start_integrity::room(&c, false).await?;
    let b = room_start_integrity::room(&c, false).await?;
    let first=c.query_one("SELECT binding.id,grant_row.id,grant_row.grant_secret_ref FROM app.ai_runner_binding binding JOIN app.room_slot slot ON slot.id=binding.room_slot_id JOIN app.controller_grant grant_row ON grant_row.binding_id=binding.id WHERE slot.room_id=$1 AND grant_row.state='active'", &[&a.id]).await?;
    let second=c.query_one("SELECT binding.id,grant_row.id FROM app.ai_runner_binding binding JOIN app.room_slot slot ON slot.id=binding.room_slot_id JOIN app.controller_grant grant_row ON grant_row.binding_id=binding.id WHERE slot.room_id=$1 AND grant_row.state='active'", &[&b.id]).await?;
    let original_binding: Uuid = first.get(0);
    let original_grant: Uuid = first.get(1);
    let reference: String = first.get(2);
    let second_binding: Uuid = second.get(0);
    let second_grant: Uuid = second.get(1);
    c.execute(
        "UPDATE app.controller_grant SET state='revoked' WHERE id=$1",
        &[&second_grant],
    )
    .await?;
    assert_eq!(
        c.query_one(
            "SELECT app.issue_ai_grant($1,1,$2,$3)",
            &[&second_binding, &reference, &Uuid::new_v4()]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::UNIQUE_VIOLATION)
    );
    c.execute(
        "UPDATE app.controller_grant SET state='revoked' WHERE id=$1",
        &[&original_grant],
    )
    .await?;
    for binding in [original_binding, second_binding] {
        assert_eq!(
            c.query_one(
                "SELECT app.issue_ai_grant($1,1,$2,$3)",
                &[&binding, &reference, &Uuid::new_v4()]
            )
            .await
            .unwrap_err()
            .code(),
            Some(&SqlState::UNIQUE_VIOLATION)
        );
    }
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let shared = format!("fixture:canonical-generation:{}", Uuid::new_v4());
    let other_reference = shared.clone();
    c.batch_execute("BEGIN").await?;
    let accepted: Value = c
        .query_one(
            "SELECT app.issue_ai_grant($1,1,$2,$3)",
            &[&original_binding, &shared, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    let task = tokio::spawn(async move {
        waiting
            .query_one(
                "SELECT app.issue_ai_grant($1,1,$2,$3)",
                &[&second_binding, &other_reference, &Uuid::new_v4()],
            )
            .await
    });
    super::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::UNIQUE_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.controller_grant WHERE grant_secret_ref=$1",
            &[&shared]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn original_runtime_jsonb_envelope_budget_matches_postgres_bytes_and_boundaries() -> TestResult
{
    use spellwake_application::{
        amount::Amount,
        controller_ports::{MAX_RUNTIME_TERMS_BYTES, ProviderObservation, ProviderOutcome},
    };
    let c = connection().await?;
    let mut original = ProviderObservation {
        binding_id: Uuid::new_v4(),
        epoch: Amount::from(1),
        start_id: Uuid::new_v4(),
        outcome: ProviderOutcome::Unknown,
        provider_ref: None,
        proof_ref: "fixture:proof".parse()?,
        request_id: Uuid::new_v4(),
    };
    let base = original.original_terms_octets().unwrap();
    let boundary = format!(
        "{}fixture:proof",
        " ".repeat(MAX_RUNTIME_TERMS_BYTES - base)
    );
    for proof in [
        "fixture:proof".to_owned(),
        " \"\\\n\t\u{0001}\u{000b}\u{000c}🂡\u{2028} ".to_owned(),
        boundary.clone(),
        format!(" {boundary}"),
    ] {
        original.proof_ref = proof.parse()?;
        let terms:Value=c.query_one("SELECT jsonb_build_object('bindingId',$1::uuid,'epoch','1','startId',$2::uuid,'outcome','unknown','providerRef',NULL::text,'proofRef',$3::text)",&[&original.binding_id,&original.start_id,&proof]).await?.get(0);
        let length: i32 = c
            .query_one("SELECT octet_length($1::jsonb::text)", &[&terms])
            .await?
            .get(0);
        assert_eq!(original.original_terms_octets(), Some(length.try_into()?));
        assert_eq!(
            original.valid(),
            usize::try_from(length)? <= MAX_RUNTIME_TERMS_BYTES
        );
        c.batch_execute("BEGIN").await?;
        let result = c
            .query_one(
                "SELECT (app.begin_ai_runtime_command('ai_provider_ack_v1',$1,$2,$3)).id",
                &[&original.binding_id, &Uuid::new_v4(), &terms],
            )
            .await;
        if usize::try_from(length)? <= MAX_RUNTIME_TERMS_BYTES {
            assert!(result.is_ok());
        } else {
            assert_eq!(
                result.unwrap_err().code(),
                Some(&SqlState::INVALID_PARAMETER_VALUE)
            );
        }
        c.batch_execute("ROLLBACK").await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn canonical_stop_scalar_reuses_existing_cause_and_rejects_foreign_stop_observation()
-> TestResult {
    use spellwake_application::controller_ports::{RequestStop, StopObservation};
    let c = connection().await?;
    let room = room_start_integrity::room(&c, false).await?;
    let source=c.query_one("SELECT b.id,b.bootstrap_receipt_id,b.binding_epoch::text FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1",&[&room.id]).await?;
    let binding: Uuid = source.get(0);
    let cause: Uuid = source.get(1);
    let epoch: String = source.get(2);
    let result: Value = c
        .query_one(
            "SELECT to_jsonb(app.request_ai_stop($1,$2))",
            &[&binding, &cause],
        )
        .await?
        .get(0);
    let stop = RequestStop {
        binding_id: binding,
        cause_id: cause,
    }
    .decode_response(result.to_string().as_bytes())?
    .0;
    let issue_request = Uuid::new_v4();
    let secret = format!("fixture:rejected-secret:{binding}");
    let issued: Value = c
        .query_one(
            "SELECT app.issue_ai_grant($1,$2::text::numeric,$3,$4)",
            &[&binding, &epoch, &secret, &issue_request],
        )
        .await?
        .get(0);
    let typed = spellwake_application::controller_ports::IssueGrant {
        binding_id: binding,
        epoch: epoch.parse()?,
        secret_ref: secret.parse()?,
        request_id: issue_request,
    }
    .decode_response(issued.to_string().as_bytes())?;
    assert!(matches!(
        typed,
        spellwake_application::ApiResponse::Fail { .. }
    ));
    let unrelated = Uuid::new_v4();
    let retry: Value = c
        .query_one(
            "SELECT to_jsonb(app.request_ai_stop($1,$2))",
            &[&binding, &unrelated],
        )
        .await?
        .get(0);
    assert_eq!(
        RequestStop {
            binding_id: binding,
            cause_id: unrelated
        }
        .decode_response(retry.to_string().as_bytes())?
        .0,
        stop
    );
    let request = Uuid::new_v4();
    let wrong_stop = Uuid::new_v4();
    let rejected: Value = c
        .query_one(
            "SELECT app.observe_ai_stop($1,$2::text::numeric,$3,'fixture:wrong-stop',$4)",
            &[&binding, &epoch, &wrong_stop, &request],
        )
        .await?
        .get(0);
    let typed = StopObservation {
        binding_id: binding,
        epoch: epoch.parse()?,
        stop_id: wrong_stop,
        proof_ref: "fixture:wrong-stop".parse()?,
        request_id: request,
    }
    .decode_response(rejected.to_string().as_bytes())?;
    assert!(matches!(
        typed,
        spellwake_application::ApiResponse::Fail { .. }
    ));
    Ok(())
}
