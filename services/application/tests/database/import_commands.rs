//! Real normalized import facades; HTTP composition remains a separate acceptance gate.
use super::{TestResult, account, connection, decks};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
async fn call(
    c: &Client,
    subject: Uuid,
    session: Uuid,
    input: &Value,
    lookup: bool,
) -> Result<Value, tokio_postgres::Error> {
    let sql = if lookup {
        "SELECT app.read_deck_import_command($1,$2,$3)"
    } else {
        "SELECT app.import_deck_command($1,$2,$3)"
    };
    Ok(c.query_one(sql, &[&subject, &session, input]).await?.get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn import_facade_rechecks_authority_and_recovers_exact_original_two_id_receipt() -> TestResult
{
    let c = connection().await?;
    let (actor, subject, _deck, definition, format) = decks::library(&c).await?;
    let session: Uuid = c
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0);
    let input = json!({"requestId":Uuid::new_v4(),"name":"API import","mode":"open","formatId":format,"entries":[{"definitionId":definition,"printingId":null,"section":"main","quantity":"4"}]});
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        call(&c, subject, session, &input, true).await?,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    let result = call(&c, subject, session, &input, false).await?;
    assert_eq!(result["status"], "success");
    assert!(result["data"]["revisionId"].is_string());
    assert_eq!(call(&c, subject, session, &input, false).await?, result);
    let recovered = call(&c, subject, session, &input, true).await?;
    assert_eq!(
        recovered,
        json!({"status":"success","data":{"state":"accepted","response":result}})
    );
    let typed: spellwake_application::ApiResponse<
        spellwake_application::decks::import::ImportDisposition,
    > = serde_json::from_value(recovered)?;
    assert!(matches!(
        typed,
        spellwake_application::ApiResponse::Success { .. }
    ));
    let mut changed = input.clone();
    changed["name"] = json!("Changed");
    for lookup in [false, true] {
        assert_eq!(
            call(&c, subject, session, &changed, lookup).await?["data"]["request"],
            "conflict"
        );
    }
    c.batch_execute("RESET ROLE").await?;
    let (_other, foreign) = account(&c).await?;
    let foreign_session: Uuid = c
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&foreign],
        )
        .await?
        .get(0);
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        call(&c, subject, foreign_session, &input, false).await?["data"]["session"],
        "unauthorized"
    );
    assert_eq!(
        call(&c, foreign, foreign_session, &input, true).await?["data"]["state"],
        "unknown"
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&actor]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        call(&c, subject, session, &input, true).await?["data"]["state"],
        "accepted"
    );
    let mut new = input.clone();
    new["requestId"] = json!(Uuid::new_v4());
    assert_eq!(
        call(&c, subject, session, &new, false).await?["data"]["account"],
        "unavailable"
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute(
        "UPDATE auth.sessions SET not_after=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&session],
    )
    .await?;
    c.batch_execute("SET ROLE service_role").await?;
    for lookup in [false, true] {
        assert_eq!(
            call(&c, subject, session, &input, lookup).await?["data"]["session"],
            "unauthorized"
        );
    }
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn rejected_import_retains_no_partial_library_and_facades_remain_narrow() -> TestResult {
    let c = connection().await?;
    let (actor, subject, _deck, definition, format) = decks::library(&c).await?;
    let session: Uuid = c
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0);
    let before: i64 = c
        .query_one(
            "SELECT count(*) FROM app.deck WHERE account_id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    let input = json!({"requestId":Uuid::new_v4(),"name":"Rejected","mode":"open","formatId":format,"entries":[{"definitionId":definition,"printingId":null,"section":"main","quantity":"-1"}]});
    c.batch_execute("SET ROLE service_role").await?;
    let result = call(&c, subject, session, &input, false).await?;
    assert_eq!(result["status"], "fail");
    assert_eq!(
        call(&c, subject, session, &input, true).await?,
        json!({"status":"success","data":{"state":"rejected","response":result}})
    );
    for invalid in [
        Value::Null,
        json!([]),
        json!({"requestId":Uuid::new_v4()}),
        {
            let mut value = input.clone();
            value["actorId"] = json!(actor);
            value
        },
    ] {
        assert_eq!(
            call(&c, subject, session, &invalid, false).await?["data"]["request"],
            "invalid"
        );
    }
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.deck WHERE account_id=$1",
            &[&actor]
        )
        .await?
        .get::<_, i64>(0),
        before
    );
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        assert!(!c.query_one("SELECT has_function_privilege($1,'app.import_deck(uuid,text,text,uuid,jsonb,uuid)','EXECUTE')",&[&role]).await?.get::<_,bool>(0));
        for name in [
            "app.import_deck_command(uuid,uuid,jsonb)",
            "app.read_deck_import_command(uuid,uuid,jsonb)",
        ] {
            assert_eq!(
                c.query_one(
                    "SELECT has_function_privilege($1,$2,'EXECUTE')",
                    &[&role, &name]
                )
                .await?
                .get::<_, bool>(0),
                role == "service_role"
            );
        }
    }
    Ok(())
}
