//! Durable delivery identity survives unknown publication outcomes and source advancement.
use super::{
    TestResult, ante_transaction_integrity as ante, between_deadline_integrity as deadlines,
    connection, match_lifecycle_integrity as lifecycle, wait_for_database_lock,
};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn prepare(c: &Client, h: Uuid) -> FixtureResult<(Uuid, Value)> {
    let response: Value = c
        .query_one("SELECT app.prepare_handoff($1)", &[&h])
        .await?
        .get(0);
    assert_eq!(response["status"], "success");
    let id=c.query_one("SELECT id FROM app.command_receipt WHERE scope='handoff_prepare_v1' AND target_ref=$1 AND state='accepted'",&[&h.to_string()]).await?.get(0);
    let facade: Value = c.query_one("SELECT app.prepare_handoff_delivery((SELECT destination FROM app.durable_handoff WHERE id=$1),$1)", &[&h]).await?.get(0);
    assert_eq!(facade["data"]["handoffId"], h.to_string());
    assert_eq!(facade["data"]["preparationId"], serde_json::to_value(id)?);
    for key in ["dispatchKey", "destination", "sourceHash"] {
        assert_eq!(facade["data"][key], response["data"][key]);
    }
    let manifest: Value = c
        .query_one("SELECT app.handoff_source_manifest($1)", &[&h])
        .await?
        .get(0);
    assert_eq!(facade["data"]["manifest"], manifest);
    let typed = spellwake_application::decode_wire_object::<
        spellwake_application::handoffs::PreparedHandoff,
    >(facade["data"].to_string().as_bytes())?;
    let destination = serde_json::from_value(manifest["destination"].clone())?;
    assert!(typed.valid_for(h, destination));
    Ok((id, response))
}
async fn ack(
    c: &Client,
    h: Uuid,
    p: Uuid,
    outcome: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    Ok(c.query_one(
        "SELECT app.acknowledge_handoff_delivery((SELECT destination FROM app.durable_handoff WHERE id=$1),$1,$2,$3,'fixture:verified-publication',$4)",
        &[&h, &p, &outcome, &request],
    )
    .await?
    .get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unknown_publication_keeps_one_prepared_dispatch_and_verified_acknowledgement_is_terminal()
-> TestResult {
    let c = connection().await?;
    let (_, m) = ante::reserve(&c, true).await?;
    let h: Uuid = c
        .query_one(
            "SELECT id FROM app.durable_handoff WHERE match_id=$1 AND kind='ready_match'",
            &[&m],
        )
        .await?
        .get(0);
    let (p, prepared) = prepare(&c, h).await?;
    assert_eq!(prepare(&c, h).await?.1, prepared);
    let request = Uuid::new_v4();
    let unknown = ack(&c, h, p, "unknown", request).await?;
    assert_eq!(unknown["status"], "success");
    assert_eq!(ack(&c, h, p, "unknown", request).await?, unknown);
    assert!(c.query_one("SELECT state='pending' AND acknowledgement_receipt_id IS NULL FROM app.durable_handoff WHERE id=$1",&[&h]).await?.get::<_,bool>(0));
    assert_eq!(
        ack(&c, h, p, "published", request)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let request = Uuid::new_v4();
    let published = ack(&c, h, p, "published", request).await?;
    assert_eq!(published["status"], "success");
    assert_eq!(ack(&c, h, p, "published", request).await?, published);
    assert_eq!(
        ack(&c, h, p, "published", Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert_eq!(c.execute("UPDATE app.durable_handoff SET state='pending',acknowledgement_receipt_id=NULL WHERE id=$1",&[&h]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        c.execute("DELETE FROM app.durable_handoff WHERE id=$1", &[&h])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(prepare(&c, h).await?.1, prepared);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn all_canonical_handoff_kinds_keep_their_producer_after_the_match_advances() -> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    deadlines::arm(&c, &f, &deadlines::due(&c, "1 hour").await?, Uuid::new_v4()).await?;
    let paused: Value = c.query_one("SELECT app.pause_match_between_games($1,(SELECT revision FROM app.match WHERE id=$1),(SELECT availability_revision FROM app.match WHERE id=$1),clock_timestamp(),'storage','fixture:handoff-pause',$2)", &[&f.matched,&Uuid::new_v4()]).await?.get(0);
    let incident: Uuid = paused["data"]["incidentId"].as_str().unwrap().parse()?;
    let resumed: Value = c.query_one("SELECT app.resume_match_between_games($1,(SELECT revision FROM app.match WHERE id=$1),(SELECT availability_revision FROM app.match WHERE id=$1),$2,'fixture:handoff-resume',$3)", &[&f.matched,&incident,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(resumed["status"], "success");
    assert!(c.query_one("SELECT EXISTS(SELECT 1 FROM app.durable_handoff WHERE match_id=$1 AND kind='between_deadline' AND source_ref LIKE '%:%')", &[&f.matched]).await?.get::<_,bool>(0));
    let rows = c
        .query(
            "SELECT id,kind FROM app.durable_handoff WHERE match_id=$1 ORDER BY id",
            &[&f.matched],
        )
        .await?;
    let mut historical = Vec::new();
    for row in rows {
        let h: Uuid = row.get(0);
        let (p, response) = prepare(&c, h).await?;
        historical.push((h, response));
        assert_eq!(
            ack(&c, h, p, "published", Uuid::new_v4()).await?["status"],
            "success"
        );
    }
    for d in c.query("SELECT d.id,s.source_revision_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.match_id=$1 ORDER BY d.id",&[&f.matched]).await?{
  let id:Uuid=d.get(0);let revision:Uuid=d.get(1);
  let response:Value=c.query_one("SELECT app.reply_between_games($1,(SELECT revision FROM app.match WHERE id=$2),$3,'fixture:selected','fixture:qualified',$4)",&[&id,&f.matched,&revision,&Uuid::new_v4()]).await?.get(0);assert_eq!(response["status"],"success");
 }
    for (h, response) in historical {
        assert_eq!(prepare(&c, h).await?.1, response);
    }
    let h: Uuid = c
        .query_one(
            "SELECT id FROM app.durable_handoff WHERE match_id=$1 AND kind='reserved_game'",
            &[&f.matched],
        )
        .await?
        .get(0);
    let (p, _) = prepare(&c, h).await?;
    assert_eq!(
        ack(&c, h, p, "published", Uuid::new_v4()).await?["status"],
        "success"
    );
    let (_, m) = ante::reserve_match(&c, true, "bo1").await?;
    let game: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    lifecycle::open(&c, m, game).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY id LIMIT 1",
            &[&m],
        )
        .await?
        .get(0);
    let result = lifecycle::complete(&c, m, game, vec![seat]).await?;
    assert_eq!(
        lifecycle::consume(&c, m, result).await?["data"]["phase"],
        "terminal"
    );
    let h: Uuid = c
        .query_one(
            "SELECT id FROM app.durable_handoff WHERE match_id=$1 AND kind='match_outcome'",
            &[&m],
        )
        .await?
        .get(0);
    let (p, _) = prepare(&c, h).await?;
    assert_eq!(
        ack(&c, h, p, "published", Uuid::new_v4()).await?["status"],
        "success"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn raw_acknowledgement_and_orphan_accepted_preparation_fail_deferred_source_checks()
-> TestResult {
    let c = connection().await?;
    let (_, m) = ante::reserve(&c, true).await?;
    let h: Uuid = c
        .query_one(
            "SELECT id FROM app.durable_handoff WHERE match_id=$1 AND kind='ready_match'",
            &[&m],
        )
        .await?
        .get(0);
    let (p, _) = prepare(&c, h).await?;
    c.batch_execute("BEGIN").await?;
    c.execute("UPDATE app.durable_handoff SET state='acknowledged',acknowledgement_receipt_id=$2 WHERE id=$1",&[&h,&p]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN").await?;
    let orphan = Uuid::new_v4();
    let id:Uuid=c.query_one("SELECT (app.begin_handoff_command($1,'handoff_prepare_v1',$1,jsonb_build_object('handoffId',$1::uuid))).id",&[&orphan]).await?.get(0);
    c.query_one(
        "SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid))",
        &[&id, &orphan],
    )
    .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let page: Value = c
        .query_one(
            "SELECT app.read_pending_handoffs('game_host',NULL,100)",
            &[],
        )
        .await?
        .get(0);
    assert!(
        page.as_array()
            .unwrap()
            .iter()
            .any(|v| v["handoffId"] == h.to_string())
    );
    assert!(
        c.query_one(
            "SELECT app.read_pending_handoffs('game_host',NULL,101)",
            &[]
        )
        .await
        .is_err()
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn competing_acknowledgements_serialize_to_one_terminal_completion() -> TestResult {
    let c = connection().await?;
    let (_, m) = ante::reserve(&c, true).await?;
    let h: Uuid = c
        .query_one(
            "SELECT id FROM app.durable_handoff WHERE match_id=$1 AND kind='ready_match'",
            &[&m],
        )
        .await?
        .get(0);
    let (p, _) = prepare(&c, h).await?;
    c.batch_execute("BEGIN").await?;
    let first = ack(&c, h, p, "published", Uuid::new_v4()).await?;
    assert_eq!(first["status"], "success");
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let task = tokio::spawn(async move { ack(&waiting, h, p, "published", Uuid::new_v4()).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??["status"], "fail");
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='handoff_ack_v1' AND target_ref=$1 AND state='accepted' AND normalized_source->'input'->>'outcome'='published'",&[&h.to_string()]).await?.get::<_,i64>(0),1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unqualified_retained_handoffs_remain_history_and_do_not_poison_pending_discovery()
-> TestResult {
    let c = connection().await?;
    let fixture = super::host_integrity::fixture(&c).await?;
    // A retained unadmitted row cannot become external publication authority.
    let h:Uuid=c.query_one("INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state) VALUES($1,$2,$2::uuid::text,'game_host','reserved_game','pending') RETURNING id",&[&fixture.match_id,&fixture.game]).await?.get(0);
    assert_eq!(
        c.query_one("SELECT app.prepare_handoff($1)", &[&h])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let page: Value = c
        .query_one(
            "SELECT app.read_pending_handoffs('game_host',NULL,100)",
            &[],
        )
        .await?
        .get(0);
    assert!(
        !page
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["handoffId"] == h.to_string())
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn delivery_capabilities_are_private_and_original_observations_remain_recoverable()
-> TestResult {
    let c = connection().await?;
    let (_, m) = ante::reserve(&c, true).await?;
    let h: Uuid = c
        .query_one(
            "SELECT id FROM app.durable_handoff WHERE match_id=$1 AND kind='ready_match'",
            &[&m],
        )
        .await?
        .get(0);
    for role in ["anon", "authenticated", "supabase_auth_admin"] {
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        assert_eq!(
            c.query_one(
                "SELECT app.read_handoff_delivery_work('game_host',NULL,100)",
                &[]
            )
            .await
            .unwrap_err()
            .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        assert_eq!(
            c.query_one("SELECT app.prepare_handoff_delivery('game_host',$1)", &[&h])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        c.batch_execute("RESET ROLE").await?;
    }
    c.batch_execute("SET ROLE service_role").await?;
    for sql in [
        "SELECT app.prepare_handoff($1)",
        "SELECT app.handoff_source_manifest($1)",
    ] {
        assert_eq!(
            c.query_one(sql, &[&h]).await.unwrap_err().code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
    }
    assert_eq!(
        c.query_one(
            "SELECT app.prepare_handoff_delivery('match_settlement',$1)",
            &[&h]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    for limit in [0, 101] {
        assert_eq!(
            c.query_one(
                "SELECT app.read_handoff_delivery_work('game_host',NULL,$1)",
                &[&limit]
            )
            .await
            .unwrap_err()
            .code(),
            Some(&SqlState::INVALID_PARAMETER_VALUE)
        );
    }
    assert_eq!(
        c.query_one(
            "SELECT app.read_handoff_delivery_work('game_host',$1,100)",
            &[&Uuid::nil()]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    let prepared: Value = c
        .query_one("SELECT app.prepare_handoff_delivery('game_host',$1)", &[&h])
        .await?
        .get(0);
    let p = Uuid::parse_str(prepared["data"]["preparationId"].as_str().unwrap())?;
    let request = Uuid::new_v4();
    let r = request;
    let o = "unknown";
    let read_sql = "SELECT app.read_handoff_delivery_ack('game_host',$1,$2,$3,'fixture:verified-publication',$4)";
    let unknown: Value = c.query_one(read_sql, &[&h, &p, &o, &r]).await?.get(0);
    assert_eq!(
        unknown,
        serde_json::json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    let observed:Value=c.query_one("SELECT app.acknowledge_handoff_delivery('game_host',$1,$2,$3,'fixture:verified-publication',$4)",&[&h,&p,&o,&r]).await?.get(0);
    assert_eq!(observed["data"]["outcome"], "unknown");
    let recovered: Value = c.query_one(read_sql, &[&h, &p, &o, &r]).await?.get(0);
    assert_eq!(recovered["data"]["state"], "accepted");
    assert_eq!(recovered["data"]["response"], observed);
    let changed: Value = c
        .query_one(read_sql, &[&h, &p, &"published", &r])
        .await?
        .get(0);
    assert_eq!(
        changed,
        serde_json::json!({"status":"fail","data":{"request":"conflict"}})
    );
    assert!(
        c.query_one(
            "SELECT app.acknowledge_handoff_delivery('game_host',$1,$2,'published','proof',$3)",
            &[&h, &Uuid::new_v4(), &Uuid::new_v4()]
        )
        .await
        .is_err()
    );
    let published_request = Uuid::new_v4();
    let published:Value=c.query_one("SELECT app.acknowledge_handoff_delivery('game_host',$1,$2,'published','fixture:verified-publication',$3)",&[&h,&p,&published_request]).await?.get(0);
    assert_eq!(published["status"], "success");
    let recovered: Value = c
        .query_one(read_sql, &[&h, &p, &"published", &published_request])
        .await?
        .get(0);
    assert_eq!(recovered["data"]["response"], published);
    let rejected_request = Uuid::new_v4();
    let rejected:Value=c.query_one("SELECT app.acknowledge_handoff_delivery('game_host',$1,$2,'published','fixture:verified-publication',$3)",&[&h,&p,&rejected_request]).await?.get(0);
    assert_eq!(
        rejected,
        serde_json::json!({"status":"fail","data":{"handoff":"already acknowledged"}})
    );
    let recovered: Value = c
        .query_one(read_sql, &[&h, &p, &"published", &rejected_request])
        .await?
        .get(0);
    assert_eq!(recovered["data"]["state"], "rejected");
    assert_eq!(recovered["data"]["response"], rejected);
    let replay: Value = c
        .query_one("SELECT app.prepare_handoff_delivery('game_host',$1)", &[&h])
        .await?
        .get(0);
    assert_eq!(replay, prepared);
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn delivery_preparation_serializes_and_rejects_non_read_committed_mutation() -> TestResult {
    let c = connection().await?;
    let (_, m) = ante::reserve(&c, true).await?;
    let h: Uuid = c
        .query_one(
            "SELECT id FROM app.durable_handoff WHERE match_id=$1 AND kind='ready_match'",
            &[&m],
        )
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    let first: Value = c
        .query_one("SELECT app.prepare_handoff_delivery('game_host',$1)", &[&h])
        .await?
        .get(0);
    let other = connection().await?;
    let pid: i32 = other
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let competing = tokio::spawn(async move {
        other
            .query_one("SELECT app.prepare_handoff_delivery('game_host',$1)", &[&h])
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(competing.await??, first);
    let preparation = Uuid::parse_str(first["data"]["preparationId"].as_str().unwrap())?;
    for isolation in ["REPEATABLE READ", "SERIALIZABLE"] {
        c.batch_execute(&format!("BEGIN ISOLATION LEVEL {isolation}"))
            .await?;
        assert_eq!(
            c.query_one("SELECT app.prepare_handoff_delivery('game_host',$1)", &[&h])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::T_R_SERIALIZATION_FAILURE)
        );
        c.batch_execute("ROLLBACK").await?;
        c.batch_execute(&format!("BEGIN ISOLATION LEVEL {isolation}"))
            .await?;
        assert_eq!(c.query_one("SELECT app.acknowledge_handoff_delivery('game_host',$1,$2,'unknown','fixture:proof',$3)",&[&h,&preparation,&Uuid::new_v4()]).await.unwrap_err().code(),Some(&SqlState::T_R_SERIALIZATION_FAILURE));
        c.batch_execute("ROLLBACK").await?;
    }
    let page: Value = c
        .query_one(
            "SELECT app.read_handoff_delivery_work('game_host',NULL,2)",
            &[],
        )
        .await?
        .get(0);
    let ids: Vec<Uuid> = page
        .as_array()
        .unwrap()
        .iter()
        .map(|item| Uuid::parse_str(item["handoffId"].as_str().unwrap()).unwrap())
        .collect();
    assert!(!ids.is_empty() && ids.len() <= 2);
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    let after = *ids.last().unwrap();
    let next: Value = c
        .query_one(
            "SELECT app.read_handoff_delivery_work('game_host',$1,2)",
            &[&after],
        )
        .await?
        .get(0);
    for item in next.as_array().unwrap() {
        assert!(Uuid::parse_str(item["handoffId"].as_str().unwrap())? > after);
        assert_eq!(item["destination"], "game_host");
    }
    Ok(())
}
