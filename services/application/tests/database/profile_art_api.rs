//! Profile reference transactions and visibility; fixtures do not qualify loadable artwork.
use super::{TestResult, account, connection, decks, set_auth_claims};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
async fn session(c: &Client, subject: Uuid) -> Result<Uuid, tokio_postgres::Error> {
    Ok(c.query_one(
        "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
        &[&subject],
    )
    .await?
    .get(0))
}
async fn printing(c: &Client, definition: Uuid) -> Result<Uuid, tokio_postgres::Error> {
    let set:Uuid=c.query_one("INSERT INTO app.card_set(external_set_ref,name) VALUES($1,'Profile fixture') RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    Ok(c.query_one("INSERT INTO app.printing(definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES($1,$2,$3,'common','nonfoil','fixture:reference-only') RETURNING id",&[&definition,&set,&Uuid::new_v4().to_string()]).await?.get(0))
}
async fn change(
    c: &Client,
    subject: Uuid,
    session: Uuid,
    request: Uuid,
    expected: &Value,
    art: &Value,
) -> Result<Value, tokio_postgres::Error> {
    Ok(c.query_one(
        "SELECT app.change_profile_art_command($1,$2,$3,$4,$5)",
        &[&subject, &session, &request, expected, art],
    )
    .await?
    .get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn profile_art_exact_receipts_cas_reset_and_pending_reads_preserve_other_preferences()
-> TestResult {
    let c = connection().await?;
    let (actor, subject, _, definition, _) = decks::library(&c).await?;
    let session = session(&c, subject).await?;
    let printing = printing(&c, definition).await?;
    let before:Value=c.query_one("SELECT to_jsonb(p)-ARRAY['printing_id','crop_x','crop_y','crop_scale'] FROM app.profile p WHERE account_id=$1",&[&actor]).await?.get(0);
    let art = json!({"printingId":printing,"cropX":0.1,"cropY":0.3333333333333333,"scale":1.0000000000000002});
    let request = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            "SELECT app.read_profile_art_command($1,$2,$3,'null',$4)",
            &[&subject, &session, &request, &art]
        )
        .await?
        .get::<_, Value>(0)["data"]["state"],
        "unknown"
    );
    let accepted = change(&c, subject, session, request, &Value::Null, &art).await?;
    assert_eq!(
        accepted,
        json!({"status":"success","data":{"resourceId":actor}})
    );
    assert_eq!(
        change(&c, subject, session, request, &Value::Null, &art).await?,
        accepted
    );
    assert_eq!(
        change(&c, subject, session, Uuid::new_v4(), &Value::Null, &art).await?["data"]["profile"],
        "profile art changed; reload before saving"
    );
    c.batch_execute("RESET ROLE; SET extra_float_digits=0")
        .await?;
    set_auth_claims(&c, subject).await?;
    let read: Value = c
        .query_one("SELECT app.read_my_profile_art()", &[])
        .await?
        .get(0);
    assert_eq!(read["data"]["cardArtProfile"], art);
    assert_eq!(read["data"]["supporter"], false);
    c.batch_execute("RESET ROLE").await?;
    // Losing the old image reference never prevents clearing that selection.
    c.execute(
        "UPDATE app.printing SET image_ref=NULL WHERE id=$1",
        &[&printing],
    )
    .await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        change(&c, subject, session, Uuid::new_v4(), &art, &Value::Null).await?["status"],
        "success"
    );
    assert_eq!(
        change(&c, subject, session, Uuid::new_v4(), &Value::Null, &art).await?["data"]["art"],
        "catalogue art unavailable"
    );
    assert_eq!(
        c.query_one(
            "SELECT app.read_profile_art_command($1,$2,$3,'null',$4)",
            &[&subject, &session, &request, &art]
        )
        .await?
        .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    c.batch_execute("RESET ROLE").await?;
    super::deletion::request(&c, actor, Uuid::new_v4()).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        change(&c, subject, session, request, &Value::Null, &art).await?,
        accepted
    );
    assert_eq!(
        change(
            &c,
            subject,
            session,
            Uuid::new_v4(),
            &Value::Null,
            &Value::Null
        )
        .await?["data"]["account"],
        "account unavailable"
    );
    c.batch_execute("RESET ROLE").await?;
    set_auth_claims(&c, subject).await?;
    assert_eq!(
        c.query_one("SELECT app.read_my_profile_art()", &[])
            .await?
            .get::<_, Value>(0)["status"],
        "success"
    );
    c.batch_execute("RESET ROLE").await?;
    let after:Value=c.query_one("SELECT to_jsonb(p)-ARRAY['printing_id','crop_x','crop_y','crop_scale'] FROM app.profile p WHERE account_id=$1",&[&actor]).await?.get(0);
    assert_eq!(before, after);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn profile_art_normalization_preserves_float_bits_and_rejects_invalid_shapes() -> TestResult {
    let c = connection().await?;
    let id = Uuid::new_v4();
    for value in [
        0.1,
        0.3333333333333333,
        1.0000000000000002,
        f64::MAX,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
    ] {
        let input = json!({"printingId":id,"cropX":0.1,"cropY":-0.0,"scale":value});
        let row=c.query_one("SELECT encode(float8send((app.normalize_profile_art($1)->>'scale')::float8),'hex'),app.normalize_profile_art($1)",&[&input]).await?;
        let bits: String = row.get(0);
        assert_eq!(bits, format!("{:016x}", value.to_bits()));
        let normalized: Value = row.get(1);
        assert_eq!(normalized["cropY"], 0);
    }
    for input in [
        json!([]),
        json!(true),
        json!(123),
        json!("invalid"),
        json!({}),
        json!({"printingId":id,"cropX":"0.1","cropY":0,"scale":1}),
        json!({"printingId":id,"cropX":0,"cropY":0,"scale":0}),
        json!({"printingId":id,"cropX":1.1,"cropY":0,"scale":1}),
        json!({"printingId":Uuid::nil(),"cropX":0,"cropY":0,"scale":1}),
    ] {
        assert!(
            c.query_one("SELECT app.normalize_profile_art($1)", &[&input])
                .await?
                .get::<_, Option<Value>>(0)
                .is_none()
        );
    }
    assert!(c.query_one("SELECT app.normalize_profile_art('{\"printingId\":\"11111111-1111-4111-8111-111111111111\",\"cropX\":0,\"cropY\":0,\"scale\":1e400}'::jsonb)",&[]).await?.get::<_,Option<Value>>(0).is_none());
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn public_profile_uses_existing_active_or_retained_identity_scope() -> TestResult {
    let c = connection().await?;
    let (viewer, subject) = account(&c).await?;
    let (target, target_subject, _, definition, _) = decks::library(&c).await?;
    let target_session = session(&c, target_subject).await?;
    let image = printing(&c, definition).await?;
    let art = json!({"printingId":image,"cropX":0.25,"cropY":0.5,"scale":2});
    assert_eq!(
        change(
            &c,
            target_subject,
            target_session,
            Uuid::new_v4(),
            &Value::Null,
            &art
        )
        .await?["status"],
        "success"
    );
    set_auth_claims(&c, subject).await?;
    let active: Value = c
        .query_one("SELECT app.read_public_profile_art($1)", &[&target])
        .await?
        .get(0);
    assert_eq!(active["data"]["viewerAccountId"], viewer.to_string());
    assert_eq!(active["data"]["person"]["id"], target.to_string());
    assert_eq!(active["data"]["cardArtProfile"], art);
    c.batch_execute("RESET ROLE").await?;
    super::deletion::request(&c, target, Uuid::new_v4()).await?;
    set_auth_claims(&c, target_subject).await?;
    assert_eq!(
        c.query_one("SELECT app.read_my_profile_art()", &[])
            .await?
            .get::<_, Value>(0)["data"]["cardArtProfile"],
        art
    );
    set_auth_claims(&c, subject).await?;
    assert_eq!(
        c.query_one("SELECT app.read_public_profile_art($1)", &[&target])
            .await?
            .get::<_, Value>(0)["status"],
        "fail"
    );
    c.batch_execute("RESET ROLE").await?;
    // Retained own block is an existing078 identity source, not contact permission.
    c.execute(
        "INSERT INTO app.block(blocker_id,blocked_id,created_at) VALUES($1,$2,clock_timestamp())",
        &[&viewer, &target],
    )
    .await?;
    set_auth_claims(&c, subject).await?;
    let retained: Value = c
        .query_one("SELECT app.read_public_profile_art($1)", &[&target])
        .await?
        .get(0);
    assert_eq!(retained["status"], "success");
    assert!(retained["data"]["cardArtProfile"].is_null());
    assert_eq!(retained["data"]["supporter"], false);
    c.batch_execute("RESET ROLE").await?;
    // Structural retirement fixture verifies projection masking, not cleanup delivery.
    let op = super::operation(&c, target).await?;
    c.execute(
        "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
        &[&op],
    )
    .await?;
    c.execute("UPDATE app.account SET state='deleted',auth_subject=NULL,deletion_requested_at=clock_timestamp(),deletion_finalized_at=clock_timestamp(),retirement_operation_id=$2 WHERE id=$1",&[&target,&op]).await?;
    set_auth_claims(&c, subject).await?;
    let deleted: Value = c
        .query_one("SELECT app.read_public_profile_art($1)", &[&target])
        .await?
        .get(0);
    assert_eq!(deleted["data"]["person"]["username"], "Deleted player");
    assert_eq!(deleted["data"]["person"]["isDeleted"], true);
    assert!(deleted["data"]["cardArtProfile"].is_null());
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn competing_profile_art_edits_serialize_and_reject_the_stale_expected_crop() -> TestResult {
    let c = connection().await?;
    let other = connection().await?;
    let (_, subject, _, definition, _) = decks::library(&c).await?;
    let session = session(&c, subject).await?;
    let image = printing(&c, definition).await?;
    let art = json!({"printingId":image,"cropX":0.1,"cropY":0.2,"scale":1});
    let replacement = json!({"printingId":image,"cropX":0.3,"cropY":0.4,"scale":2});
    let request = Uuid::new_v4();
    let pid: i32 = other
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    let accepted = change(&c, subject, session, request, &Value::Null, &art).await?;
    assert_eq!(accepted["status"], "success");
    let task = tokio::spawn(async move {
        other.batch_execute("SET ROLE service_role").await?;
        change(
            &other,
            subject,
            session,
            Uuid::new_v4(),
            &Value::Null,
            &replacement,
        )
        .await
    });
    super::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await??["data"]["profile"],
        "profile art changed; reload before saving"
    );
    let changed = json!({"printingId":image,"cropX":0.9,"cropY":0.2,"scale":1});
    assert_eq!(
        change(&c, subject, session, request, &Value::Null, &changed).await?["data"]["request"],
        "conflict"
    );
    assert_eq!(
        c.query_one(
            "SELECT app.read_profile_art_command($1,$2,$3,'null',$4)",
            &[&subject, &session, &request, &changed]
        )
        .await?
        .get::<_, Value>(0)["data"]["request"],
        "conflict"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn profile_art_commands_reject_stale_isolation_and_recheck_expired_after_wait() -> TestResult
{
    let c = connection().await?;
    let waiting = connection().await?;
    let (actor, subject, _, definition, _) = decks::library(&c).await?;
    let session = session(&c, subject).await?;
    let image = printing(&c, definition).await?;
    let art = json!({"printingId":image,"cropX":0.1,"cropY":0.2,"scale":1});
    for sql in [
        "SELECT app.change_profile_art_command($1,$2,$3,'null',$4)",
        "SELECT app.read_profile_art_command($1,$2,$3,'null',$4)",
    ] {
        c.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ; SET LOCAL ROLE service_role")
            .await?;
        assert_eq!(
            c.query_one(sql, &[&subject, &session, &Uuid::new_v4(), &art])
                .await
                .unwrap_err()
                .code(),
            Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
        );
        c.batch_execute("ROLLBACK").await?;
    }
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let request = Uuid::new_v4();
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
        &[&actor],
    )
    .await?;
    let task = tokio::spawn(async move {
        waiting.batch_execute("SET ROLE service_role").await?;
        change(&waiting, subject, session, request, &Value::Null, &art).await
    });
    super::wait_for_database_lock(&c, pid).await?;
    c.execute(
        "UPDATE auth.sessions SET not_after=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&session],
    )
    .await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await??,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE account_id=$1 AND scope='profile_art'",&[&actor]).await?.get::<_,i64>(0),0);
    Ok(())
}
