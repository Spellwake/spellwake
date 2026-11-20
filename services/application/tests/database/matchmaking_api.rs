//! HTTP-facing facades/projections over real canonical queue sources; no matcher/native qualification.
use super::{TestResult, account, connection, queue_intent_integrity as queue, set_auth_claims};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
async fn identity(c: &Client, actor: Uuid) -> Result<(Uuid, Uuid), tokio_postgres::Error> {
    let r=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1",&[&actor]).await?;
    Ok((r.get(0), r.get(1)))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn matchmaking_facades_recover_whole_intent_and_keep_normalized_proofs_out_of_owner_reads()
-> TestResult {
    let c = connection().await?;
    let mut f = queue::fixture(&c).await?;
    let (subject, session) = identity(&c, f.room.source.actor).await?;
    let (stranger, other_subject) = account(&c).await?;
    f.profile=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref,matchmaking_config) SELECT $2,'fixture','fixture',minimum_seats,seat_capacity,admission_qualification_ref,jsonb_set(matchmaking_config,'{configurations,0,key}','\"  duel  \"'::jsonb) FROM app.table_profile_revision WHERE id=$1 RETURNING id",&[&f.profile,&Uuid::new_v4().to_string()]).await?.get(0);
    let alternatives = json!([{"deckRevisionId":f.room.source.revision,"choices":[{"profileId":f.profile,"configurationKey":"team-duel","maximumAiSeats":"0","maximumSponsoredAiSeats":"0"},{"profileId":f.profile,"configurationKey":"  duel  ","maximumAiSeats":"0","maximumSponsoredAiSeats":"0"}]}]);
    let request = Uuid::new_v4();
    let lookup =
        "SELECT app.read_matchmaking_command($1,$2,'enqueue',$3,NULL,'collection',false,$4)";
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &alternatives])
            .await?
            .get::<_, Value>(0)["data"]["state"],
        "unknown"
    );
    let accepted: Value = c
        .query_one(
            "SELECT app.enqueue_matchmaking_command($1,$2,$3,'collection',false,$4)",
            &[&subject, &session, &request, &alternatives],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    let entry: Uuid = accepted["data"]["resourceId"].as_str().unwrap().parse()?;
    let intent: Uuid = accepted["data"]["intentId"].as_str().unwrap().parse()?;
    assert_eq!(accepted["data"].as_object().unwrap().len(), 2);
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &alternatives])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    let mut changed = alternatives.clone();
    changed[0]["choices"].as_array_mut().unwrap().reverse();
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &changed])
            .await?
            .get::<_, Value>(0)["data"]["request"],
        "conflict"
    );
    c.batch_execute("RESET ROLE").await?;
    set_auth_claims(&c, subject).await?;
    let detail: Value = c
        .query_one("SELECT app.read_my_queue_intent($1)", &[&intent])
        .await?
        .get(0);
    assert_eq!(detail["data"]["alternatives"], alternatives);
    assert_eq!(detail["data"]["requestId"], request.to_string());
    assert!(!detail.to_string().contains("qualificationRef"));
    assert!(!detail.to_string().contains("configuration\""));
    let page: Value = c
        .query_one("SELECT app.read_my_queue($1,NULL,1)", &[&intent])
        .await?
        .get(0);
    assert_eq!(page["data"]["items"][0]["intentId"], intent.to_string());
    assert_eq!(page["data"]["items"][0]["alternativeIndex"], "0");
    assert_eq!(page["data"]["nextCursor"], entry.to_string());
    assert_eq!(page["data"]["items"][0].as_object().unwrap().len(), 10);
    set_auth_claims(&c, other_subject).await?;
    assert_eq!(
        c.query_one("SELECT app.read_my_queue_intent($1)", &[&intent])
            .await?
            .get::<_, Value>(0)["data"]["intent"],
        "unavailable"
    );
    let empty: Value = c
        .query_one("SELECT app.read_my_queue($1,NULL,100)", &[&intent])
        .await?
        .get(0);
    assert_eq!(empty["data"]["accountId"], stranger.to_string());
    assert_eq!(empty["data"]["items"], json!([]));
    c.batch_execute("RESET ROLE; SET ROLE service_role").await?;
    let cancel = Uuid::new_v4();
    let canceled: Value = c
        .query_one(
            "SELECT app.cancel_matchmaking_command($1,$2,$3,$4)",
            &[&subject, &session, &cancel, &entry],
        )
        .await?
        .get(0);
    assert_eq!(
        canceled,
        json!({"status":"success","data":{"resourceId":entry}})
    );
    assert_eq!(
        c.query_one(
            "SELECT app.read_matchmaking_command($1,$2,'cancel',$3,$4,NULL,NULL,NULL)",
            &[&subject, &session, &cancel, &entry]
        )
        .await?
        .get::<_, Value>(0)["data"]["response"],
        canceled
    );
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &alternatives])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(
        c.query_one("SELECT state FROM app.queue_entry WHERE id=$1", &[&entry])
            .await?
            .get::<_, String>(0),
        "canceled"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn published_profile_and_configuration_reads_are_bounded_and_hide_private_source_fields()
-> TestResult {
    let c = connection().await?;
    let f = queue::fixture(&c).await?;
    let (subject, _) = identity(&c, f.room.source.actor).await?;
    let unqualified = Uuid::new_v4();
    c.execute("INSERT INTO app.table_profile_revision(id,key,revision_ref,rules_ref) VALUES($1,$1::uuid::text,'fixture','fixture')",&[&unqualified]).await?;
    set_auth_claims(&c, subject).await?;
    // The whole suite publishes more than one page of independent profiles.
    // Follow the real bounded cursor rather than assuming this random UUID is
    // among the first100 rows. Validate every actual page and retain the privacy check.
    let mut after = None;
    let mut found = false;
    loop {
        let page: Value = c
            .query_one("SELECT app.read_table_profiles($1,100)", &[&after])
            .await?
            .get(0);
        let spellwake_application::ApiResponse::Success { data } = serde_json::from_value::<
            spellwake_application::ApiResponse<
                spellwake_application::matchmaking::TableProfilePage,
            >,
        >(page.clone())?
        else {
            panic!("expected actual table profile page")
        };
        data.validate(f.room.source.actor, after, 100)?;
        found |= data
            .items
            .iter()
            .any(|p| p.id == f.profile && p.has_matchmaking);
        assert!(
            page["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p["id"] != unqualified.to_string() && p.as_object().unwrap().len() == 5)
        );
        match data.next_cursor {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert!(found);
    let first: Value = c
        .query_one(
            "SELECT app.read_table_profile_configurations($1,NULL,1)",
            &[&f.profile],
        )
        .await?
        .get(0);
    assert_eq!(first["data"]["nextIndex"], "0");
    assert_eq!(first["data"]["items"][0]["key"], "duel");
    assert_eq!(first["data"]["items"][0].as_object().unwrap().len(), 5);
    let second: Value = c
        .query_one(
            "SELECT app.read_table_profile_configurations($1,0,1)",
            &[&f.profile],
        )
        .await?
        .get(0);
    assert_eq!(second["data"]["nextIndex"], "1");
    assert_eq!(second["data"]["items"][0]["key"], "team-duel");
    assert!(!second.to_string().contains("teams"));
    assert!(!second.to_string().contains("qualificationRef"));
    assert_eq!(
        c.query_one(
            "SELECT app.read_table_profile_configurations($1,1,1)",
            &[&f.profile]
        )
        .await?
        .get::<_, Value>(0)["data"]["items"],
        json!([])
    );
    assert_eq!(
        c.query_one(
            "SELECT app.read_table_profile_configurations($1,NULL,100)",
            &[&unqualified]
        )
        .await?
        .get::<_, Value>(0)["data"]["profile"],
        "unavailable"
    );
    assert_eq!(
        c.query_one(
            "SELECT app.read_table_profile_configurations($1,-1,100)",
            &[&f.profile]
        )
        .await?
        .get::<_, Value>(0)["data"]["query"],
        "invalid"
    );
    c.batch_execute("RESET ROLE; SET ROLE anon").await?;
    assert_eq!(
        c.query_one("SELECT app.read_table_profiles(NULL,100)", &[])
            .await
            .unwrap_err()
            .code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn canonical_queue_size_limit_is_not_confused_with_compact_http_json() -> TestResult {
    let c = connection().await?;
    let f = queue::fixture(&c).await?;
    let (subject, session) = identity(&c, f.room.source.actor).await?;
    let base = json!([{"deckRevisionId":f.room.source.revision,"choices":[{"profileId":f.profile,"configurationKey":"d","maximumAiSeats":"0","maximumSponsoredAiSeats":"0"}]}]);
    let overhead:i32=c.query_one("SELECT octet_length(jsonb_build_object('mode','collection','competitive',false,'alternatives',$1::jsonb)::text)",&[&base]).await?.get(0);
    let mut under = base;
    under[0]["choices"][0]["configurationKey"] = json!(format!(
        "{}d",
        " ".repeat(262144 - usize::try_from(overhead)?)
    ));
    assert_eq!(c.query_one("SELECT octet_length(jsonb_build_object('mode','collection','competitive',false,'alternatives',$1::jsonb)::text)",&[&under]).await?.get::<_,i32>(0),262144);
    let mut over = under.clone();
    over[0]["choices"][0]["configurationKey"] = json!(format!(
        " {}",
        under[0]["choices"][0]["configurationKey"].as_str().unwrap()
    ));
    let request = Uuid::new_v4();
    let http =
        json!({"requestId":request,"mode":"collection","competitive":false,"alternatives":over});
    assert!(serde_json::to_vec(&http)?.len() < 264192);
    c.batch_execute("SET ROLE service_role").await?;
    // Within the canonical envelope, normal unsupported-choice rejection is durable.
    let within: Value = c
        .query_one(
            "SELECT app.enqueue_matchmaking_command($1,$2,$3,'collection',false,$4)",
            &[&subject, &session, &Uuid::new_v4(), &under],
        )
        .await?
        .get(0);
    assert_eq!(
        within["data"]["choice"],
        "unsupported format, profile or AI allowance"
    );
    let failed: Value = c
        .query_one(
            "SELECT app.enqueue_matchmaking_command($1,$2,$3,'collection',false,$4)",
            &[&subject, &session, &request, &over],
        )
        .await?
        .get(0);
    assert_eq!(
        failed,
        json!({"status":"fail","data":{"intent":"invalid envelope"}})
    );
    assert_eq!(
        c.query_one(
            "SELECT app.read_matchmaking_command($1,$2,'enqueue',$3,NULL,'collection',false,$4)",
            &[&subject, &session, &request, &over]
        )
        .await?
        .get::<_, Value>(0),
        failed
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE principal_key=$1 AND request_ref=$2 AND scope='queue_intent_v1'",&[&format!("human:{}",f.room.source.actor),&request.to_string()]).await?.get::<_,i64>(0),0);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn queue_cancel_rechecks_claim_scope_after_account_wait_then_retries_original_request()
-> TestResult {
    use super::wait_for_database_lock;
    let c = connection().await?;
    let f = queue::fixture(&c).await?;
    let (subject, session) = identity(&c, f.room.source.actor).await?;
    let first = queue::enqueue(
        &c,
        f.room.source.actor,
        &queue::alternatives(f.profile, f.room.source.revision, "duel", "0"),
        Uuid::new_v4(),
    )
    .await?;
    let second = queue::enqueue(
        &c,
        f.room.other,
        &queue::alternatives(f.profile, f.room.other_revision, "duel", "0"),
        Uuid::new_v4(),
    )
    .await?;
    let entry: Uuid = first["data"]["resourceId"].as_str().unwrap().parse()?;
    let other: Uuid = second["data"]["resourceId"].as_str().unwrap().parse()?;
    let request = Uuid::new_v4();
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
        &[&f.room.source.actor],
    )
    .await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    waiting.batch_execute("SET ROLE service_role").await?;
    let task = tokio::spawn(async move {
        let result = waiting
            .query_one(
                "SELECT app.cancel_matchmaking_command($1,$2,$3,$4)",
                &[&subject, &session, &request, &entry],
            )
            .await;
        (waiting, result)
    });
    wait_for_database_lock(&c, pid).await?;
    let selected = json!({"profileId":f.profile,"configurationKey":"duel","hostEntryId":entry,"humanSeats":[{"queueEntryId":entry,"seatIndex":"0"},{"queueEntryId":other,"seatIndex":"1"}],"aiSeats":[]});
    let claim: Value = c
        .query_one(
            "SELECT app.claim_matchmaking($1,$2)",
            &[&selected, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(claim["status"], "success");
    let room: Uuid = claim["data"]["resourceId"].as_str().unwrap().parse()?;
    c.batch_execute("COMMIT").await?;
    let (waiting, result) = task.await?;
    assert_eq!(
        result.unwrap_err().code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE principal_key=$1 AND request_ref=$2 AND scope='queue_cancel'",&[&format!("human:{}",f.room.source.actor),&request.to_string()]).await?.get::<_,i64>(0),0);
    let retry: Value = waiting
        .query_one(
            "SELECT app.cancel_matchmaking_command($1,$2,$3,$4)",
            &[&subject, &session, &request, &entry],
        )
        .await?
        .get(0);
    assert_eq!(
        retry,
        json!({"status":"success","data":{"resourceId":entry}})
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.queue_entry WHERE claimed_room_id=$1 AND state='canceled'",&[&room]).await?.get::<_,i64>(0),2);
    assert_eq!(
        c.query_one("SELECT state FROM app.room WHERE id=$1", &[&room])
            .await?
            .get::<_, String>(0),
        "closed"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn retained_untyped_queue_entries_keep_their_null_tuple_and_remain_releasable() -> TestResult
{
    let c = connection().await?;
    let f = queue::fixture(&c).await?;
    let (subject, session) = identity(&c, f.room.source.actor).await?;
    // The opaque writer was retired in150; seed only a retained nullable-schema row.
    let entry:Uuid=c.query_one("INSERT INTO app.queue_entry(account_id,deck_revision_id,mode,competitive,preferences_ref,state) VALUES($1,$2,'collection',false,'fixture:retained','queued') RETURNING id",&[&f.room.source.actor,&f.room.source.revision]).await?.get(0);
    set_auth_claims(&c, subject).await?;
    let page: Value = c
        .query_one("SELECT app.read_my_queue(NULL,NULL,100)", &[])
        .await?
        .get(0);
    let value = page["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == entry.to_string())
        .unwrap();
    for field in ["intentId", "alternativeIndex", "formatId"] {
        assert!(value[field].is_null());
    }
    // Other whole-tier fixtures publish profiles too. Follow the real cursor
    // rather than assuming this randomly ordered UUID is on the first page.
    let mut after: Option<Uuid> = None;
    loop {
        let profiles: Value = c
            .query_one("SELECT app.read_table_profiles($1,100)", &[&after])
            .await?
            .get(0);
        if let Some(profile) = profiles["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["id"] == f.room.source.profile.to_string())
        {
            assert_eq!(profile["hasMatchmaking"], false);
            break;
        }
        let next: Option<Uuid> = profiles["data"]["nextCursor"]
            .as_str()
            .map(str::parse)
            .transpose()?;
        assert!(next.is_some() && (after.is_none() || next > after));
        after = next;
    }
    c.batch_execute("RESET ROLE; SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            "SELECT app.cancel_matchmaking_command($1,$2,$3,$4)",
            &[&subject, &session, &Uuid::new_v4(), &entry]
        )
        .await?
        .get::<_, Value>(0)["status"],
        "success"
    );
    Ok(())
}
