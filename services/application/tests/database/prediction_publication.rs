//! Authenticated publication consumes existing synthetic qualified source records, never invents a live price.
use super::{
    TestResult, connection,
    prediction_commands::execute_for_actor,
    pricing::{buyer, product},
    set_auth_claims,
};
use serde_json::{Value, json};
use uuid::Uuid;
struct PublicationFixture {
    actor: Uuid,
    subject: Uuid,
    printing: Uuid,
    probe: Uuid,
    input: Value,
    source: Value,
}
async fn fixture(
    client: &tokio_postgres::Client,
) -> Result<PublicationFixture, Box<dyn std::error::Error + Send + Sync>> {
    let (actor, subject) = buyer(client).await?;
    let product = product(client, false).await?;
    let row=client.query_one("SELECT id,external_printing_ref FROM app.printing WHERE id=(SELECT printing_id FROM app.asset_definition WHERE id=$1)",&[&product.cards[0]]).await?;
    let printing: Uuid = row.get(0);
    let external: String = row.get(1);
    set_auth_claims(client, subject).await?;
    let unavailable: Value = client
        .query_one(
            "SELECT app.read_my_prediction_publication_source($1)",
            &[&printing],
        )
        .await?
        .get(0);
    assert_eq!(unavailable["data"]["price"], "unavailable");
    client.batch_execute("RESET ROLE").await?;
    let slot: String = client
        .query_one(
            "SELECT ((clock_timestamp() AT TIME ZONE 'UTC'-interval '12 hours')::date)::text",
            &[],
        )
        .await?
        .get(0);
    let manifest = super::prediction_resolution::manifest(client, &slot, true).await?;
    // An accepted retained artifact is sufficient to request extraction, before any probe exists.
    set_auth_claims(client, subject).await?;
    let basis: Value = client
        .query_one(
            "SELECT app.read_my_prediction_probe_basis($1)",
            &[&printing],
        )
        .await?
        .get(0);
    assert_eq!(basis["status"], "success");
    let parsed: spellwake_application::prediction_publication::OwnedProbeBasis =
        serde_json::from_value(basis["data"].clone())?;
    parsed.validate(printing)?;
    assert_eq!(parsed.account_id, actor);
    assert_eq!(parsed.basis.external_printing_id.to_string(), external);
    assert_eq!(parsed.basis.slot.to_string(), slot);
    client.batch_execute("RESET ROLE").await?;
    let card = json!({"object":"card","id":external,"finishes":["nonfoil","foil"],"prices":{"usd":"2.37","usd_foil":null,"usd_etched":null}});
    client.batch_execute("SET ROLE service_role").await?;
    let probe: Uuid = client
        .query_one("SELECT app.record_price_probe($1,$2)", &[&manifest, &card])
        .await?
        .get(0);
    client.batch_execute("RESET ROLE").await?;
    set_auth_claims(client, subject).await?;
    let source: Value = client
        .query_one(
            "SELECT app.read_my_prediction_publication_source($1)",
            &[&printing],
        )
        .await?
        .get(0);
    assert_eq!(source["status"], "success");
    assert_eq!(source["data"]["accountId"], actor.to_string());
    assert_eq!(source["data"]["source"]["probeId"], probe.to_string());
    assert_eq!(source["data"]["source"]["currentUsd"], "2.37");
    for forbidden in ["artifactRef", "manifestId", "prices", "evidence", "hash"] {
        assert!(source["data"]["source"].get(forbidden).is_none());
    }
    client.batch_execute("RESET ROLE").await?;
    let command:Value=client.query_one("SELECT jsonb_build_object('requestId',$1::uuid,'printingId',$2::uuid,'probeId',$3::uuid,'targetUsd','10.00','condition','reached_by','tradingClosesAt',clock_timestamp()+interval '1 hour','observationStartsAt',date_trunc('day',clock_timestamp() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'+interval '2 days 12 hours','deadline',date_trunc('day',clock_timestamp() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'+interval '2 days 13 hours')",&[&Uuid::new_v4(),&printing,&probe]).await?.get(0);
    let input = json!({"kind":"publish_market","command":command});
    Ok(PublicationFixture {
        actor,
        subject,
        printing,
        probe,
        input,
        source,
    })
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn publication_preflight_and_session_writer_share_exact_source_and_recover_normalized_terms()
-> TestResult {
    let client = connection().await?;
    let PublicationFixture {
        actor,
        printing,
        probe,
        input,
        source,
        ..
    } = fixture(&client).await?;
    let parsed: spellwake_application::prediction_publication::OwnedPublicationSource =
        serde_json::from_value(source["data"].clone())?;
    parsed.validate(printing)?;
    assert_eq!(
        execute_for_actor(&client, actor, &input, true).await?["data"]["state"],
        "unknown"
    );
    let first = execute_for_actor(&client, actor, &input, false).await?;
    assert_eq!(first["status"], "success");
    let market: Uuid = first["data"]["resourceId"].as_str().unwrap().parse()?;
    assert_eq!(
        client
            .query_one(
                "SELECT publication_probe_id FROM app.prediction_market WHERE id=$1",
                &[&market]
            )
            .await?
            .get::<_, Uuid>(0),
        probe
    );
    client
        .batch_execute("SET TimeZone='America/New_York'; SET DateStyle='ISO, MDY'")
        .await?;
    assert_eq!(
        execute_for_actor(&client, actor, &input, true).await?["data"]["state"],
        "accepted"
    );
    assert_eq!(
        execute_for_actor(&client, actor, &input, false).await?,
        first
    );
    client
        .batch_execute("RESET TimeZone; RESET DateStyle")
        .await?;
    let mut changed = input.clone();
    changed["command"]["targetUsd"] = json!("11.00");
    assert_eq!(
        execute_for_actor(&client, actor, &changed, true).await?["data"]["request"],
        "conflict"
    );
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&actor]).await?;
    let mut fresh = input;
    fresh["command"]["requestId"] = json!(Uuid::new_v4());
    assert_eq!(
        execute_for_actor(&client, actor, &fresh, false).await?["data"]["account"],
        "account unavailable"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn publication_source_and_writer_capabilities_are_explicit() -> TestResult {
    let client = connection().await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for signature in [
            "app.prediction_publication_price(uuid,uuid)",
            "app.prediction_order_terms(jsonb)",
            "app.prediction_command_terms(jsonb)",
            "app.execute_prediction_order(uuid,uuid,jsonb)",
            "app.execute_prediction_command(uuid,uuid,jsonb)",
            "app.read_my_prediction_publication_source(uuid)",
            "app.read_my_prediction_probe_basis(uuid)",
            "app.publish_prediction(uuid,uuid,text,text,timestamptz,timestamptz,timestamptz,uuid,uuid)",
        ] {
            let allowed: bool = client
                .query_one(
                    "SELECT has_function_privilege($1,$2,'EXECUTE')",
                    &[&role, &signature],
                )
                .await?
                .get(0);
            assert_eq!(
                allowed,
                (role == "service_role" && signature.starts_with("app.execute_prediction_command"))
                    || (role == "authenticated"
                        && (signature.starts_with("app.read_my_prediction_publication_source")
                            || signature.starts_with("app.read_my_prediction_probe_basis"))),
                "{role}:{signature}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn publication_auth_revocation_after_printing_wait_rolls_back_market_and_receipt()
-> TestResult {
    let locker = connection().await?;
    let PublicationFixture {
        actor,
        subject,
        printing,
        input,
        ..
    } = fixture(&locker).await?;
    let session: Uuid = locker
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0);
    let request = input["command"]["requestId"].as_str().unwrap().to_owned();
    let writer = connection().await?;
    let pid: i32 = writer
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    writer.batch_execute("SET ROLE service_role").await?;
    locker.batch_execute("BEGIN").await?;
    locker
        .query_one(
            "SELECT id FROM app.printing WHERE id=$1 FOR UPDATE",
            &[&printing],
        )
        .await?;
    let task = tokio::spawn(async move {
        Ok::<Value, tokio_postgres::Error>(
            writer
                .query_one(
                    "SELECT app.execute_prediction_command($1,$2,$3)",
                    &[&subject, &session, &input],
                )
                .await?
                .get(0),
        )
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if locker
                .query_one("SELECT cardinality(pg_blocking_pids($1))>0", &[&pid])
                .await
                .unwrap()
                .get::<_, bool>(0)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    locker
        .execute(
            "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 day' WHERE id=$1",
            &[&subject],
        )
        .await?;
    locker.batch_execute("COMMIT").await?;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(10), task).await???,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(
        locker
            .query_one(
                "SELECT count(*) FROM app.prediction_market WHERE creator_id=$1",
                &[&actor]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    assert_eq!(
        locker
            .query_one(
                "SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND request_ref=$2",
                &[&actor, &request]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn staged_catalogue_printing_cannot_supply_prediction_publication() -> TestResult {
    use super::catalogue_publication as catalogue;
    let client = connection().await?;
    let (actor, subject) = buyer(&client).await?;
    let external = Uuid::new_v4();
    let batch = json!([catalogue::metadata(
        Uuid::new_v4(),
        Uuid::new_v4(),
        external,
        "Pending prediction card"
    )]);
    let terms = catalogue::source_terms(std::slice::from_ref(&batch));
    client.batch_execute("SET ROLE service_role").await?;
    let publication = catalogue::begin(&client, &terms).await?;
    catalogue::append(&client, publication, 1, &batch).await?;
    client.batch_execute("RESET ROLE").await?;
    let printing: Uuid = client
        .query_one(
            "SELECT id FROM app.printing WHERE external_printing_ref=$1 AND finish='nonfoil'",
            &[&external.to_string()],
        )
        .await?
        .get(0);
    set_auth_claims(&client, subject).await?;
    let value: Value = client
        .query_one(
            "SELECT app.read_my_prediction_publication_source($1)",
            &[&printing],
        )
        .await?
        .get(0);
    assert_eq!(value["data"]["printing"], "unavailable");
    let basis: Value = client
        .query_one(
            "SELECT app.read_my_prediction_probe_basis($1)",
            &[&printing],
        )
        .await?
        .get(0);
    assert_eq!(basis["data"]["printing"], "unavailable");
    client.batch_execute("RESET ROLE").await?;
    let command:Value=client.query_one("SELECT jsonb_build_object('requestId',$1::uuid,'printingId',$2::uuid,'probeId',$3::uuid,'targetUsd','10.00','condition','at_deadline','tradingClosesAt',clock_timestamp()+interval '1 hour','observationStartsAt',clock_timestamp()+interval '2 days','deadline',clock_timestamp()+interval '4 days')",&[&Uuid::new_v4(),&printing,&Uuid::new_v4()]).await?.get(0);
    let input = json!({"kind":"publish_market","command":command});
    assert_eq!(
        execute_for_actor(&client, actor, &input, false).await?["data"]["printing"],
        "printing unavailable"
    );
    client.batch_execute("SET ROLE service_role").await?;
    catalogue::finish(&client, publication).await?;
    client.batch_execute("RESET ROLE").await?;
    set_auth_claims(&client, subject).await?;
    let value: Value = client
        .query_one(
            "SELECT app.read_my_prediction_publication_source($1)",
            &[&printing],
        )
        .await?
        .get(0);
    assert_eq!(value["data"]["price"], "unavailable");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn source_preparation_basis_is_read_only_and_requires_current_active_authority() -> TestResult
{
    let client = connection().await?;
    let f = fixture(&client).await?;
    // Transaction-local counters detect any receipt write without observing parallel tests.
    client.batch_execute("BEGIN").await?;
    let before: i64 = client
        .query_one("SELECT coalesce(sum(n_tup_ins+n_tup_upd+n_tup_del),0)::bigint FROM pg_stat_xact_user_tables WHERE relid='app.command_receipt'::regclass", &[])
        .await?
        .get(0);
    set_auth_claims(&client, f.subject).await?;
    let basis: Value = client
        .query_one(
            "SELECT app.read_my_prediction_probe_basis($1)",
            &[&f.printing],
        )
        .await?
        .get(0);
    assert_eq!(basis["data"]["accountId"], f.actor.to_string());
    let missing: Value = client
        .query_one(
            "SELECT app.read_my_prediction_probe_basis($1)",
            &[&Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(missing["data"]["printing"], "unavailable");
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(
        client
            .query_one("SELECT coalesce(sum(n_tup_ins+n_tup_upd+n_tup_del),0)::bigint FROM pg_stat_xact_user_tables WHERE relid='app.command_receipt'::regclass", &[])
            .await?
            .get::<_, i64>(0),
        before
    );
    client.batch_execute("COMMIT").await?;
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.actor]).await?;
    set_auth_claims(&client, f.subject).await?;
    let pending: Value = client
        .query_one(
            "SELECT app.read_my_prediction_probe_basis($1)",
            &[&f.printing],
        )
        .await?
        .get(0);
    assert_eq!(pending["data"]["account"], "unavailable");
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 day' WHERE id=$1",
            &[&f.subject],
        )
        .await?;
    set_auth_claims(&client, f.subject).await?;
    let banned: Value = client
        .query_one(
            "SELECT app.read_my_prediction_probe_basis($1)",
            &[&f.printing],
        )
        .await?
        .get(0);
    assert_eq!(banned["data"]["session"], "unauthorized");
    Ok(())
}
