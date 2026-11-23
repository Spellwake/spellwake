//! Synthetic trusted captures qualify SQL intake/retention, not HMAC, actual Storage or payments.
use super::{TestResult, connection};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
fn capture() -> Value {
    json!({"version":1,"kind":"webhook","source":"patreon-v2-member-notification-v1","campaignId":"campaign_test","userId":format!("user-{}",Uuid::new_v4()),"memberId":format!("member-{}",Uuid::new_v4()),"bodyHash":Uuid::new_v4().simple().to_string().repeat(2)})
}
async fn ingest(c: &Client, id: Uuid, value: &Value) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.capture_patreon_notification($1,$2)",
        &[&id, value],
    )
    .await
    .map(|r| r.get(0))
}
async fn seed(
    c: &Client,
    provider: &str,
    state: &str,
    raw: &str,
    kind: &str,
) -> Result<(Uuid, Value, String), tokio_postgres::Error> {
    let id = Uuid::new_v4();
    let mut value = capture();
    value["kind"] = json!(kind);
    let hash = value["bodyHash"].as_str().unwrap();
    // Stripe captures are content-addressed; Patreon additionally scopes an
    // occurrence UUID. Keep aged synthetic rows faithful to each actual writer.
    let path = if provider == "stripe" {
        format!("payments/stripe/{hash}.json")
    } else {
        format!("payments/{provider}/{id}/{hash}.json")
    };
    c.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state) VALUES($1::uuid,$2,CASE WHEN $2='patreon' THEN 'notification:'||$1::uuid::text ELSE $1::uuid::text END,clock_timestamp()-interval '31 days','member.notification',$3,$4,$5,$6)",&[&id,&provider,&path,&if provider == "patreon" && state == "processed" { "received" } else { state },&value,&raw]).await?;
    if provider == "patreon" && state == "processed" {
        c.query_one("SELECT app.process_patreon_notification($1)", &[&id])
            .await?;
    }
    Ok((id, value, path))
}
async fn work(
    c: &Client,
    kind: &str,
    after: Option<Uuid>,
    limit: i32,
    bucket: Option<&str>,
) -> Result<Vec<Uuid>, tokio_postgres::Error> {
    let value: Value = c
        .query_one(
            "SELECT app.read_patreon_work($1,$2,$3,$4)",
            &[&kind, &after, &limit, &bucket],
        )
        .await?
        .get(0);
    Ok(serde_json::from_value(value).unwrap())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_intake_replays_one_occurrence_without_merging_equal_bodies_or_creating_benefits()
-> TestResult {
    let c = connection().await?;
    let value = capture();
    let id = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    let first = ingest(&c, id, &value).await?;
    let again = ingest(&c, id, &value).await?;
    assert_eq!(first, again);
    assert_eq!(first["id"], json!(id));
    assert_eq!(first["evidenceState"], "pending");
    assert_eq!(first["processed"], false);
    assert_eq!(
        first["payloadRef"],
        format!(
            "payments/patreon/{id}/{}.json",
            value["bodyHash"].as_str().unwrap()
        )
    );
    let other = ingest(&c, Uuid::new_v4(), &value).await?;
    assert_ne!(first["id"], other["id"]);
    assert_ne!(first["payloadRef"], other["payloadRef"]);
    let mut changed = value.clone();
    changed["memberId"] = json!("changed_member");
    assert!(ingest(&c, id, &changed).await.is_err());
    c.batch_execute("RESET ROLE").await?;
    let r=c.query_one("SELECT account_id IS NULL AND first_delivery_at IS NULL AND effect IS NULL AND effect_key IS NULL AND processing_state='received', (SELECT count(*) FROM app.patreon_membership WHERE source_event_id=$1), (SELECT count(*) FROM app.patreon_payment WHERE provider_event_id=$1), (SELECT count(*) FROM app.cosmetic_grant WHERE provider_event_id=$1) FROM app.provider_event WHERE id=$1",&[&id]).await?;
    assert!(r.get::<_, bool>(0));
    for i in 1..4 {
        assert_eq!(r.get::<_, i64>(i), 0);
    }
    assert!(
        c.execute(
            "UPDATE app.provider_event SET capture=$2 WHERE id=$1",
            &[&id, &changed]
        )
        .await
        .is_err()
    );
    assert!(c.execute("UPDATE app.provider_event SET received_at=clock_timestamp()-interval '1 day' WHERE id=$1",&[&id]).await.is_err());
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_intake_rejects_malformed_trusted_assertions_and_cross_provider_collisions()
-> TestResult {
    let c = connection().await?;
    let good = capture();
    let collision = seed(&c, "stripe", "received", "pending", "webhook")
        .await?
        .0;
    c.batch_execute("SET ROLE service_role").await?;
    let mut cases = vec![Value::Null, json!([]), json!({})];
    for (field, bad) in [
        ("version", json!("1")),
        ("version", json!(2)),
        ("kind", json!("api_read")),
        ("source", json!("arbitrary")),
        ("campaignId", json!("bad/id")),
        ("memberId", json!("a".repeat(129))),
        ("userId", Value::Null),
        ("bodyHash", json!("A".repeat(64))),
        ("bodyHash", json!("short")),
    ] {
        let mut v = good.clone();
        v[field] = bad;
        cases.push(v);
    }
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("userId");
    cases.push(missing);
    let mut extra = good.clone();
    extra["accountId"] = json!(Uuid::new_v4());
    cases.push(extra);
    let mut oversized = good.clone();
    oversized["memberId"] = json!("a".repeat(17000));
    cases.push(oversized);
    for value in cases {
        let id = Uuid::new_v4();
        assert!(ingest(&c, id, &value).await.is_err());
        assert_eq!(
            c.query_one(
                "SELECT count(*) FROM app.provider_event WHERE id=$1",
                &[&id]
            )
            .await?
            .get::<_, i64>(0),
            0
        );
    }
    assert!(ingest(&c, Uuid::nil(), &good).await.is_err());
    assert!(
        c.query_one("SELECT app.capture_patreon_notification(NULL,$1)", &[&good])
            .await
            .is_err()
    );
    assert!(ingest(&c, collision, &good).await.is_err());
    assert_eq!(
        c.query_one(
            "SELECT provider FROM app.provider_event WHERE id=$1",
            &[&collision]
        )
        .await?
        .get::<_, String>(0),
        "stripe"
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_evidence_confirmation_and_expiry_are_monotonic_and_do_not_process_notifications()
-> TestResult {
    let c = connection().await?;
    let value = capture();
    let id = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    ingest(&c, id, &value).await?;
    assert!(
        !c.query_one(
            "SELECT app.confirm_patreon_evidence($1,$2)",
            &[&id, &"f".repeat(64)]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert!(
        c.query_one(
            "SELECT app.confirm_patreon_evidence($1,$2)",
            &[&id, &value["bodyHash"].as_str().unwrap()]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(ingest(&c, id, &value).await?["evidenceState"], "retained");
    assert!(
        c.query_one("SELECT app.begin_patreon_evidence_expiry($1)", &[&id])
            .await?
            .get::<_, Option<Value>>(0)
            .is_none()
    );
    assert!(
        c.query_one("SELECT app.finish_patreon_evidence_expiry($1)", &[&id])
            .await
            .is_err()
    );
    c.batch_execute("RESET ROLE").await?;
    for (state, raw) in [
        ("received", "pending"),
        ("verified", "retained"),
        ("rejected", "pending"),
        ("processed", "retained"),
    ] {
        let (due, source, path) = seed(&c, "patreon", state, raw, "webhook").await?;
        c.batch_execute("SET ROLE service_role").await?;
        if raw == "pending" {
            assert!(
                c.query_one(
                    "SELECT app.confirm_patreon_evidence($1,$2)",
                    &[&due, &source["bodyHash"].as_str().unwrap()]
                )
                .await?
                .get::<_, bool>(0)
            );
            assert_eq!(
                c.query_one(
                    "SELECT raw_evidence_state FROM app.provider_event WHERE id=$1",
                    &[&due]
                )
                .await?
                .get::<_, String>(0),
                "expired"
            );
        }
        let ticket: Value = c
            .query_one("SELECT app.begin_patreon_evidence_expiry($1)", &[&due])
            .await?
            .get(0);
        assert_eq!(
            ticket,
            json!({"payloadRef":path,"bodyHash":source["bodyHash"]})
        );
        assert!(
            c.query_one(
                "SELECT app.confirm_patreon_evidence($1,$2)",
                &[&due, &source["bodyHash"].as_str().unwrap()]
            )
            .await?
            .get::<_, bool>(0)
        );
        c.query_one("SELECT app.finish_patreon_evidence_expiry($1)", &[&due])
            .await?;
        let first: Value = c
            .query_one(
                "SELECT to_jsonb(raw_deleted_at) FROM app.provider_event WHERE id=$1",
                &[&due],
            )
            .await?
            .get(0);
        c.query_one("SELECT app.finish_patreon_evidence_expiry($1)", &[&due])
            .await?;
        let row=c.query_one("SELECT processing_state,raw_evidence_state,to_jsonb(raw_deleted_at) FROM app.provider_event WHERE id=$1",&[&due]).await?;
        assert_eq!(row.get::<_, String>(0), state);
        assert_eq!(row.get::<_, String>(1), "expired");
        assert_eq!(row.get::<_, Value>(2), first);
        c.batch_execute("RESET ROLE").await?;
    }
    let (cross, source, _) = seed(&c, "stripe", "received", "pending", "webhook").await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert!(
        !c.query_one(
            "SELECT app.confirm_patreon_evidence($1,$2)",
            &[&cross, &source["bodyHash"].as_str().unwrap()]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert!(
        c.query_one("SELECT app.begin_patreon_evidence_expiry($1)", &[&cross])
            .await?
            .get::<_, Option<Value>>(0)
            .is_none()
    );
    assert!(
        c.query_one("SELECT app.finish_patreon_evidence_expiry($1)", &[&cross])
            .await
            .is_err()
    );
    assert!(
        c.query_one("SELECT app.confirm_patreon_evidence($1,NULL)", &[&id])
            .await
            .is_err()
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_work_is_bounded_ordered_and_pending_means_received_independent_of_raw_state()
-> TestResult {
    let c = connection().await?;
    let mut expected = Vec::new();
    for raw in ["pending", "retained", "expired"] {
        expected.push(seed(&c, "patreon", "received", raw, "webhook").await?.0);
    }
    let processed = seed(&c, "patreon", "processed", "expired", "webhook")
        .await?
        .0;
    let other_source = seed(&c, "patreon", "received", "pending", "api_read")
        .await?
        .0;
    c.batch_execute("SET ROLE service_role").await?;
    let ids = work(&c, "event", None, 100, None).await?;
    for id in &expected {
        assert!(ids.contains(id));
    }
    assert!(!ids.contains(&processed));
    assert!(!ids.contains(&other_source));
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    let page = work(&c, "event", None, 1, None).await?;
    assert_eq!(page.len(), 1);
    let next = work(&c, "event", Some(page[0]), 100, None).await?;
    assert!(next.iter().all(|id| *id > page[0]));
    for id in expected {
        let v: Value = c
            .query_one("SELECT app.read_patreon_notification($1)", &[&id])
            .await?
            .get(0);
        assert_eq!(v["id"], json!(id));
        assert_eq!(v.as_object().unwrap().len(), 6);
    }
    for id in [processed, other_source, Uuid::new_v4()] {
        assert!(
            c.query_one("SELECT app.read_patreon_notification($1)", &[&id])
                .await?
                .get::<_, Option<Value>>(0)
                .is_none()
        );
    }
    for (kind, after, limit, bucket) in [
        ("purchase", None, 1, None),
        ("event", Some(Uuid::nil()), 1, None),
        ("event", None, 0, None),
        ("event", None, 101, None),
        ("event", None, 1, Some("bad/bucket")),
        ("evidence", None, 1, None),
    ] {
        assert!(work(&c, kind, after, limit, bucket).await.is_err());
    }
    assert!(
        c.query_one("SELECT app.read_patreon_notification($1)", &[&Uuid::nil()])
            .await
            .is_err()
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn shared_evidence_scan_rediscovers_late_objects_and_preserves_stripe_nonwebhook_rules()
-> TestResult {
    let c = connection().await?;
    // Transaction-local metadata is synthetic Storage shape, not actual Storage HTTP.
    c.batch_execute("BEGIN; CREATE SCHEMA storage; CREATE TABLE storage.objects(bucket_id text NOT NULL,name text NOT NULL,PRIMARY KEY(bucket_id,name))").await?;
    let (id, _, path) = seed(&c, "patreon", "received", "pending", "webhook").await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert!(
        work(&c, "evidence", None, 100, Some("private_test"))
            .await?
            .contains(&id)
    );
    c.query_one("SELECT app.begin_patreon_evidence_expiry($1)", &[&id])
        .await?;
    c.query_one("SELECT app.finish_patreon_evidence_expiry($1)", &[&id])
        .await?;
    assert!(
        !work(&c, "evidence", None, 100, Some("private_test"))
            .await?
            .contains(&id)
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute(
        "INSERT INTO storage.objects VALUES('private_test',$1)",
        &[&path],
    )
    .await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert!(
        work(&c, "evidence", None, 100, Some("private_test"))
            .await?
            .contains(&id)
    );
    assert!(
        !work(&c, "evidence", None, 100, Some("different_bucket"))
            .await?
            .contains(&id)
    );
    c.batch_execute("RESET ROLE").await?;
    let (legacy, source, _) = seed(&c, "stripe", "received", "pending", "api_read").await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert!(
        c.query_one(
            "SELECT app.confirm_stripe_evidence($1,$2)",
            &[&legacy, &source["bodyHash"].as_str().unwrap()]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert!(
        c.query_one("SELECT app.begin_stripe_evidence_expiry($1)", &[&legacy])
            .await?
            .get::<_, Option<Value>>(0)
            .is_none()
    );
    c.query_one("SELECT app.finish_stripe_evidence_expiry($1)", &[&legacy])
        .await?;
    c.batch_execute("RESET ROLE; ROLLBACK").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_intake_capabilities_are_service_only_and_private_helpers_cannot_be_called()
-> TestResult {
    let c = connection().await?;
    let public = [
        "capture_patreon_notification(uuid,jsonb)",
        "confirm_patreon_evidence(uuid,text)",
        "read_patreon_work(text,uuid,integer,text)",
        "read_patreon_notification(uuid)",
        "begin_patreon_evidence_expiry(uuid)",
        "finish_patreon_evidence_expiry(uuid)",
    ];
    let private = [
        "provider_evidence_matches(text,jsonb,text)",
        "confirm_provider_evidence(text,uuid,text)",
        "begin_provider_evidence_expiry(text,uuid)",
        "finish_provider_evidence_expiry(text,uuid)",
        "read_provider_evidence_work(text,uuid,integer,text)",
        "patreon_notification_id(uuid)",
        "patreon_notification_capture(jsonb)",
    ];
    for role in [
        "anon",
        "authenticated",
        "supabase_auth_admin",
        "service_role",
    ] {
        for function in public.iter().chain(private.iter()) {
            let allowed: bool = c
                .query_one(
                    "SELECT has_function_privilege($1,$2,'EXECUTE')",
                    &[&role, &format!("app.{function}")],
                )
                .await?
                .get(0);
            assert_eq!(
                allowed,
                role == "service_role" && public.contains(function),
                "{role} {function}"
            );
        }
    }
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        assert!(
            c.execute("DELETE FROM app.provider_event WHERE false", &[])
                .await
                .is_err()
        );
        if role != "service_role" {
            assert!(
                c.query_one(
                    "SELECT app.read_patreon_notification($1)",
                    &[&Uuid::new_v4()]
                )
                .await
                .is_err()
            );
        }
        c.batch_execute("RESET ROLE").await?;
    }
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM pg_class WHERE relnamespace='app'::regnamespace AND relkind='r'",
            &[]
        )
        .await?
        .get::<_, i64>(0),
        111
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_notification_wrappers_exclude_oauth_and_foreign_sources_and_reject_required_nulls()
-> TestResult {
    let c = connection().await?;
    for (event_type, source, kind) in [
        (
            "oauth.association",
            "patreon-v2-member-notification-v1",
            "webhook",
        ),
        ("member.notification", "foreign-source", "webhook"),
        (
            "member.notification",
            "patreon-v2-member-notification-v1",
            "api_read",
        ),
    ] {
        let id = Uuid::new_v4();
        let mut value = capture();
        value["source"] = json!(source);
        value["kind"] = json!(kind);
        c.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state) VALUES($1::uuid,'patreon',CASE WHEN $2='patreon' THEN 'notification:'||$1::uuid::text ELSE $1::uuid::text END,clock_timestamp()-interval '31 days',$2,'synthetic-private-path','received',$3,'expired')",&[&id,&event_type,&value]).await?;
        c.batch_execute("SET ROLE service_role").await?;
        assert!(
            !c.query_one(
                "SELECT app.confirm_patreon_evidence($1,$2)",
                &[&id, &value["bodyHash"].as_str().unwrap()]
            )
            .await?
            .get::<_, bool>(0)
        );
        assert!(
            c.query_one("SELECT app.begin_patreon_evidence_expiry($1)", &[&id])
                .await?
                .get::<_, Option<Value>>(0)
                .is_none()
        );
        assert!(
            c.query_one("SELECT app.read_patreon_notification($1)", &[&id])
                .await?
                .get::<_, Option<Value>>(0)
                .is_none()
        );
        assert!(
            c.query_one("SELECT app.finish_patreon_evidence_expiry($1)", &[&id])
                .await
                .is_err()
        );
        assert!(!work(&c, "event", None, 100, None).await?.contains(&id));
        assert!(
            !work(&c, "evidence", None, 100, Some("private_test"))
                .await?
                .contains(&id)
        );
        c.batch_execute("RESET ROLE").await?;
    }
    let delivery = Uuid::new_v4();
    let other = Uuid::new_v4();
    let value = capture();
    c.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state) VALUES($1,'patreon',$2,clock_timestamp(),'member.notification','synthetic-private-path','received',$3,'pending')",&[&other,&format!("notification:{delivery}"),&value]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert!(ingest(&c, delivery, &value).await.is_err());
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.provider_event WHERE id=$1",
            &[&delivery]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    for sql in [
        "SELECT app.capture_patreon_notification(NULL,NULL)",
        "SELECT app.read_patreon_work(NULL,NULL,1,NULL)",
        "SELECT app.read_patreon_work('event',NULL,NULL,NULL)",
        "SELECT app.read_patreon_notification(NULL)",
        "SELECT app.begin_patreon_evidence_expiry(NULL)",
        "SELECT app.finish_patreon_evidence_expiry(NULL)",
        "SELECT app.confirm_patreon_evidence(NULL,NULL)",
    ] {
        assert!(c.query_one(sql, &[]).await.is_err(), "{sql}");
    }
    for field in ["campaignId", "memberId", "userId"] {
        let mut bad = value.clone();
        bad[field] = json!("é");
        assert!(ingest(&c, Uuid::new_v4(), &bad).await.is_err());
    }
    assert!(work(&c, "event", None, 1, Some("é")).await.is_err());
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stripe_shared_evidence_wrappers_preserve_work_nulls_timezone_and_late_object_recovery()
-> TestResult {
    let c = connection().await?;
    // This transaction sees no Storage schema until its own metadata shape is added.
    c.batch_execute("BEGIN; SET LOCAL TIME ZONE 'America/New_York'")
        .await?;
    let (id, value, path) = seed(&c, "stripe", "received", "pending", "webhook").await?;
    c.batch_execute("SET ROLE service_role").await?;
    let initial: Value = c
        .query_one("SELECT app.read_stripe_notification($1)", &[&id])
        .await?
        .get(0);
    assert!(!initial["expiresAt"].as_str().unwrap().ends_with("+00:00"));
    assert!(
        !c.query_one("SELECT app.confirm_stripe_evidence(NULL,NULL)", &[])
            .await?
            .get::<_, bool>(0)
    );
    assert!(
        c.query_one("SELECT app.begin_stripe_evidence_expiry(NULL)", &[])
            .await?
            .get::<_, Option<Value>>(0)
            .is_none()
    );
    c.batch_execute("SAVEPOINT null_finish").await?;
    assert!(
        c.query_one("SELECT app.finish_stripe_evidence_expiry(NULL)", &[])
            .await
            .is_err()
    );
    c.batch_execute("ROLLBACK TO SAVEPOINT null_finish; RELEASE SAVEPOINT null_finish")
        .await?;
    let scan: Value = c
        .query_one(
            "SELECT app.read_stripe_work('evidence',NULL,100,'private_test')",
            &[],
        )
        .await?
        .get(0);
    assert!(scan.as_array().unwrap().contains(&json!(id)));
    assert!(
        c.query_one(
            "SELECT app.confirm_stripe_evidence($1,$2)",
            &[&id, &value["bodyHash"].as_str().unwrap()]
        )
        .await?
        .get::<_, bool>(0)
    );
    let ticket: Value = c
        .query_one("SELECT app.begin_stripe_evidence_expiry($1)", &[&id])
        .await?
        .get(0);
    assert_eq!(
        ticket,
        json!({"payloadRef":path,"bodyHash":value["bodyHash"]})
    );
    assert!(
        spellwake_application::stripe::notification::evidence_locator_matches(
            ticket["payloadRef"].as_str().unwrap(),
            ticket["bodyHash"].as_str().unwrap()
        )
    );
    c.query_one("SELECT app.finish_stripe_evidence_expiry($1)", &[&id])
        .await?;
    let scan: Value = c
        .query_one(
            "SELECT app.read_stripe_work('evidence',NULL,100,'private_test')",
            &[],
        )
        .await?
        .get(0);
    assert!(!scan.as_array().unwrap().contains(&json!(id)));
    for sql in [
        "SELECT app.read_stripe_work('evidence',NULL,1,NULL)",
        "SELECT app.read_stripe_work('evidence',NULL,1,'bad/bucket')",
    ] {
        c.batch_execute("SAVEPOINT bad_argument").await?;
        assert!(c.query_one(sql, &[]).await.is_err());
        c.batch_execute("ROLLBACK TO SAVEPOINT bad_argument; RELEASE SAVEPOINT bad_argument")
            .await?;
    }
    c.batch_execute("RESET ROLE; CREATE SCHEMA storage; CREATE TABLE storage.objects(bucket_id text NOT NULL,name text NOT NULL,PRIMARY KEY(bucket_id,name))").await?;
    c.execute(
        "INSERT INTO storage.objects VALUES('private_test',$1)",
        &[&path],
    )
    .await?;
    c.batch_execute("SET ROLE service_role").await?;
    let scan: Value = c
        .query_one(
            "SELECT app.read_stripe_work('evidence',NULL,100,'private_test')",
            &[],
        )
        .await?
        .get(0);
    assert!(scan.as_array().unwrap().contains(&json!(id)));
    let scan: Value = c
        .query_one("SELECT app.read_stripe_work('event',NULL,100,NULL)", &[])
        .await?
        .get(0);
    assert!(scan.as_array().unwrap().contains(&json!(id)));
    let _: Value = c
        .query_one("SELECT app.read_stripe_work('purchase',NULL,1,NULL)", &[])
        .await?
        .get(0);
    c.batch_execute("RESET ROLE; ROLLBACK").await?;
    Ok(())
}

async fn wait_for_block(c: &Client, pid: i32) -> TestResult {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if c.query_one("SELECT cardinality(pg_blocking_pids($1))>0", &[&pid])
            .await?
            .get::<_, bool>(0)
        {
            return Ok(());
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "expected actual database lock wait"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_capture_concurrent_replay_waits_for_one_source_and_rejects_changed_terms()
-> TestResult {
    for changed in [false, true] {
        let first = connection().await?;
        let second = connection().await?;
        let observer = connection().await?;
        let id = Uuid::new_v4();
        let value = capture();
        first
            .batch_execute("BEGIN; SET LOCAL ROLE service_role")
            .await?;
        let accepted = ingest(&first, id, &value).await?;
        let pid: i32 = second
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        second.batch_execute("SET ROLE service_role").await?;
        let mut competing = value.clone();
        if changed {
            competing["userId"] = json!("different_user");
        }
        let task = tokio::spawn(async move { ingest(&second, id, &competing).await });
        wait_for_block(&observer, pid).await?;
        first.batch_execute("COMMIT").await?;
        let result = task.await?;
        if changed {
            assert!(result.is_err());
        } else {
            assert_eq!(result?, accepted);
        }
        let row = observer
            .query_one(
                "SELECT count(*),bool_and(capture=$2) FROM app.provider_event WHERE id=$1",
                &[&id, &value],
            )
            .await?;
        assert_eq!(row.get::<_, i64>(0), 1);
        assert!(row.get::<_, bool>(1));
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_evidence_confirmation_refreshes_due_time_after_an_observed_row_wait() -> TestResult
{
    let owner = connection().await?;
    let worker = connection().await?;
    let observer = connection().await?;
    let id = Uuid::new_v4();
    let value = capture();
    owner.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state) VALUES($1::uuid,'patreon',$1::uuid::text,clock_timestamp()-interval '30 days'+interval '1 second','member.notification','synthetic-private-path','received',$2,'pending')",&[&id,&value]).await?;
    owner.batch_execute("BEGIN").await?;
    owner
        .query_one(
            "SELECT id FROM app.provider_event WHERE id=$1 FOR UPDATE",
            &[&id],
        )
        .await?;
    let pid: i32 = worker
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    worker.batch_execute("SET ROLE service_role").await?;
    let hash = value["bodyHash"].as_str().unwrap().to_owned();
    let task = tokio::spawn(async move {
        worker
            .query_one("SELECT app.confirm_patreon_evidence($1,$2)", &[&id, &hash])
            .await
    });
    wait_for_block(&observer, pid).await?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if observer.query_one("SELECT clock_timestamp()>=received_at+interval '30 days' FROM app.provider_event WHERE id=$1",&[&id]).await?.get::<_,bool>(0){break;}
        assert!(
            tokio::time::Instant::now() < deadline,
            "expected source expiry"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    owner.batch_execute("COMMIT").await?;
    assert!(task.await??.get::<_, bool>(0));
    assert_eq!(
        observer
            .query_one(
                "SELECT raw_evidence_state FROM app.provider_event WHERE id=$1",
                &[&id]
            )
            .await?
            .get::<_, String>(0),
        "expired"
    );
    Ok(())
}
