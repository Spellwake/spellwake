//! Authenticated-hint/fresh-source SQL contracts; no live webhook or Patreon GET is asserted.
use super::{TestResult, connection, deletion, patreon_association as patreon};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn capture(c: &Client, proof: &Value) -> FixtureResult<Uuid> {
    let event = Uuid::new_v4();
    let data = json!({"version":1,"kind":"webhook","source":"patreon-v2-member-notification-v1","campaignId":proof["campaignId"],"userId":proof["userId"],"memberId":proof["memberId"],"bodyHash":patreon::hash()});
    c.query_one(
        "SELECT app.capture_patreon_notification($1,$2)",
        &[&event, &data],
    )
    .await?;
    Ok(event)
}
async fn process(c: &Client, event: Uuid) -> Result<Value, tokio_postgres::Error> {
    let value: Value = c
        .query_one("SELECT app.process_patreon_notification($1)", &[&event])
        .await?
        .get(0);
    let original =
        spellwake_application::patreon_source_ports::ProcessNotification { event_id: event };
    let decoded = original
        .decode_response(&serde_json::to_vec(&value).unwrap())
        .unwrap();
    let retained=c.query_one("SELECT (effect->>'associationId')::uuid,(effect->>'refreshSourceId')::uuid FROM app.provider_event WHERE id=$1",&[&event]).await?;
    assert!(decoded.matches_source(
        &spellwake_application::patreon_source_ports::NotificationContext {
            association_id: retained.get(0),
            refresh_source_id: retained.get(1)
        }
    ));
    Ok(value)
}
async fn route(c: &Client, event: Uuid) -> FixtureResult<Value> {
    Ok(
        c.query_one("SELECT app.read_patreon_notification_route($1)", &[&event])
            .await?
            .get(0),
    )
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn active_notification_requires_its_fresh_creator_source_and_competing_completion_replays_once()
-> TestResult {
    let c = connection().await?;
    let i = patreon::identity(&c).await?;
    let (member, tier, proof) =
        super::retirement_minimization_integrity::link(&c, i, false).await?;
    let event = capture(&c, &proof).await?;
    let work = route(&c, event).await?;
    assert_eq!(work["action"], "refresh");
    assert_eq!(work["requestId"], event.to_string());
    assert_eq!(
        process(&c, event).await.unwrap_err().code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    let current = json!({"version":1,"source":"patreon-v2-current-member-v1","campaignId":proof["campaignId"],"userId":proof["userId"],"memberId":proof["memberId"],"memberHash":patreon::hash(),"entitledTierIds":[tier.to_string()]});
    let accepted: Value = c
        .query_one(
            "SELECT app.commit_patreon_refresh($1,$2,$3)",
            &[&event, &work["ticket"], &current],
        )
        .await?
        .get(0);
    assert_eq!(accepted["outcome"], "applied");
    assert_eq!(route(&c, event).await?["action"], "finish");
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    let result = process(&c, event).await?;
    assert_eq!(result["disposition"], "refreshed");
    assert_eq!(result["refreshSourceId"], accepted["sourceEventId"]);
    let retained:Uuid=c.query_one("SELECT r.id FROM app.provider_event r JOIN app.provider_event n ON n.id=$2 WHERE r.provider='patreon' AND r.external_event_ref='refresh:'||$1::uuid::text||':'||$2::uuid::text AND r.event_type='api.membership_refresh' AND r.processing_state='processed' AND r.received_at>=n.received_at AND r.capture->>'memberId'=n.capture->>'memberId' AND r.capture->>'userId'=n.capture->>'userId' AND r.capture->>'campaignId'=n.capture->>'campaignId'",&[&member,&event]).await?.get(0);
    let decoded =
        spellwake_application::patreon_source_ports::ProcessNotification { event_id: event }
            .decode_response(&serde_json::to_vec(&result)?)?;
    assert!(decoded.matches_source(
        &spellwake_application::patreon_source_ports::NotificationContext {
            association_id: Some(member),
            refresh_source_id: Some(retained)
        }
    ));
    let task = tokio::spawn(async move { process(&waiting, event).await });
    super::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??, result);
    assert_eq!(process(&c, event).await?, result);
    assert_eq!(route(&c, event).await?["action"], "done");
    assert!(
        c.query_one(
            "SELECT active AND active_tier_id=$2 FROM app.patreon_membership WHERE id=$1",
            &[&member, &tier]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.patreon_payment WHERE membership_id=$1",
            &[&member]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.cosmetic_grant WHERE account_id=$1 AND source_kind='patreon_payment'", &[&i.actor]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn received_account_null_hint_blocks_retirement_until_canonical_inactive_disposition()
-> TestResult {
    let c = connection().await?;
    let i = patreon::identity(&c).await?;
    let (association, _, proof) =
        super::retirement_minimization_integrity::link(&c, i, true).await?;
    let event = capture(&c, &proof).await?;
    assert!(c.query_one("SELECT account_id IS NULL AND processing_state='received' FROM app.provider_event WHERE id=$1", &[&event]).await?.get::<_,bool>(0));
    assert_eq!(
        deletion::request(&c, i.actor, Uuid::new_v4()).await?["status"],
        "success"
    );
    assert_eq!(
        c.query_one("SELECT app.retire_account($1)", &[&i.actor])
            .await?
            .get::<_, Value>(0)["data"]["state"],
        "waiting"
    );
    assert_eq!(route(&c, event).await?["action"], "finish");
    let value = process(&c, event).await?;
    assert_eq!(value["disposition"], "ignored_inactive");
    assert!(
        spellwake_application::patreon_source_ports::ProcessNotification { event_id: event }
            .decode_response(&serde_json::to_vec(&value)?)?
            .matches_source(
                &spellwake_application::patreon_source_ports::NotificationContext {
                    association_id: Some(association),
                    refresh_source_id: None
                }
            )
    );
    let retired: Value = c
        .query_one("SELECT app.retire_account($1)", &[&i.actor])
        .await?
        .get(0);
    assert_eq!(retired["data"]["state"], "retired", "{retired}");
    let late = capture(&c, &proof).await?;
    let value = process(&c, late).await?;
    assert_eq!(value["disposition"], "ignored_inactive");
    assert!(
        spellwake_application::patreon_source_ports::ProcessNotification { event_id: late }
            .decode_response(&serde_json::to_vec(&value)?)?
            .matches_source(
                &spellwake_application::patreon_source_ports::NotificationContext {
                    association_id: Some(association),
                    refresh_source_id: None
                }
            )
    );
    assert_eq!(
        c.query_one("SELECT app.retire_account($1)", &[&i.actor])
            .await?
            .get::<_, Value>(0),
        retired
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.cosmetic_grant WHERE account_id=$1 AND state='active'", &[&i.actor]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unlinked_hint_finishes_without_binding_or_money_and_later_association_preserves_its_history()
-> TestResult {
    let c = connection().await?;
    let i = patreon::identity(&c).await?;
    let terms = patreon::terms();
    let proof = patreon::proof(&terms);
    let event = capture(&c, &proof).await?;
    let value = process(&c, event).await?;
    assert_eq!(value["disposition"], "ignored_unlinked");
    assert!(
        spellwake_application::patreon_source_ports::ProcessNotification { event_id: event }
            .decode_response(&serde_json::to_vec(&value)?)?
            .matches_source(
                &spellwake_application::patreon_source_ports::NotificationContext {
                    association_id: None,
                    refresh_source_id: None
                }
            )
    );
    let request = Uuid::new_v4();
    let state = patreon::hash();
    let code = patreon::hash();
    let nonce = patreon::hash();
    assert_eq!(
        patreon::start(&c, i, request, &state, &terms).await?["status"],
        "success"
    );
    assert_eq!(
        patreon::claim(&c, i, request, &state, &code, &nonce).await?["data"]["mayExchange"],
        true
    );
    assert_eq!(
        patreon::finish(&c, i, request, &state, &code, &nonce, &proof).await?["status"],
        "success"
    );
    c.query_one(
        "SELECT app.check_patreon_notification_processing($1)",
        &[&event],
    )
    .await?;
    let value = process(&c, event).await?;
    assert_eq!(value["disposition"], "ignored_unlinked");
    assert!(
        spellwake_application::patreon_source_ports::ProcessNotification { event_id: event }
            .decode_response(&serde_json::to_vec(&value)?)?
            .matches_source(
                &spellwake_application::patreon_source_ports::NotificationContext {
                    association_id: None,
                    refresh_source_id: None
                }
            )
    );
    assert!(
        c.query_one(
            "SELECT account_id IS NULL FROM app.provider_event WHERE id=$1",
            &[&event]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn raw_processing_cannot_discharge_a_hint_or_forge_inactive_lifecycle() -> TestResult {
    let c = connection().await?;
    let i = patreon::identity(&c).await?;
    let (_, _, proof) = super::retirement_minimization_integrity::link(&c, i, false).await?;
    let event = capture(&c, &proof).await?;
    for deferred in [false, true] {
        c.batch_execute("BEGIN").await?;
        if deferred {
            c.batch_execute("SET CONSTRAINTS ALL DEFERRED").await?;
        }
        c.execute("UPDATE app.provider_event SET processing_state='processed',effect_key='forged',effect='{}' WHERE id=$1", &[&event]).await?;
        assert_eq!(
            c.batch_execute("COMMIT").await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
        assert!(c.query_one("SELECT processing_state='received' AND effect IS NULL FROM app.provider_event WHERE id=$1", &[&event]).await?.get::<_,bool>(0));
    }
    c.batch_execute("BEGIN").await?;
    c.execute("UPDATE app.provider_event e SET account_id=m.account_id,processing_state='processed',effect_key='patreon-notification:'||e.id::text,effect=jsonb_build_object('kind','patreon_notification_v1','processedAt',to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),'associationId',m.id,'associationSourceId',m.source_event_id,'accountState','deletion_pending','memberRef',m.patreon_member_ref,'disposition','ignored_inactive','refreshSourceId',NULL,'result',jsonb_build_object('eventId',e.id,'disposition','ignored_inactive','associationId',m.id,'refreshSourceId',NULL)) FROM app.patreon_membership m WHERE e.id=$1 AND m.account_id=$2", &[&event,&i.actor]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    deletion::request(&c, i.actor, Uuid::new_v4()).await?;
    assert_eq!(
        c.query_one("SELECT app.retire_account($1)", &[&i.actor])
            .await?
            .get::<_, Value>(0)["data"]["state"],
        "waiting"
    );
    assert_eq!(process(&c, event).await?["disposition"], "ignored_inactive");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn first_link_and_unlinked_processing_serialize_in_both_orders() -> TestResult {
    for link_first in [false, true] {
        let c = connection().await?;
        let other = connection().await?;
        let i = patreon::identity(&c).await?;
        let terms = patreon::terms();
        let proof = patreon::proof(&terms);
        let event = capture(&c, &proof).await?;
        let request = Uuid::new_v4();
        let state = patreon::hash();
        let code = patreon::hash();
        let nonce = patreon::hash();
        patreon::start(&c, i, request, &state, &terms).await?;
        patreon::claim(&c, i, request, &state, &code, &nonce).await?;
        let pid: i32 = other
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        c.batch_execute("BEGIN").await?;
        if link_first {
            assert_eq!(
                patreon::finish(&c, i, request, &state, &code, &nonce, &proof).await?["status"],
                "success"
            );
            let task = tokio::spawn(async move { process(&other, event).await });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(
                task.await?.unwrap_err().code(),
                Some(&SqlState::T_R_SERIALIZATION_FAILURE)
            );
            assert_eq!(route(&c, event).await?["action"], "refresh");
        } else {
            assert_eq!(process(&c, event).await?["disposition"], "ignored_unlinked");
            let task = tokio::spawn(async move {
                patreon::finish(&other, i, request, &state, &code, &nonce, &proof).await
            });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "success");
            assert_eq!(process(&c, event).await?["disposition"], "ignored_unlinked");
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stale_member_notification_keeps_original_nullable_context_and_replays_without_refresh()
-> TestResult {
    use spellwake_application::patreon_source_ports::{NotificationContext, ProcessNotification};
    let c = connection().await?;
    let i = patreon::identity(&c).await?;
    let (association, _, mut proof) =
        super::retirement_minimization_integrity::link(&c, i, false).await?;
    proof["memberId"] = json!(format!("old-member-{}", Uuid::new_v4()));
    let event = capture(&c, &proof).await?;
    let value = process(&c, event).await?;
    assert_eq!(value["disposition"], "ignored_stale_member");
    let original = ProcessNotification { event_id: event };
    let decoded = original.decode_response(&serde_json::to_vec(&value)?)?;
    assert!(decoded.matches_source(&NotificationContext {
        association_id: Some(association),
        refresh_source_id: None
    }));
    assert_eq!(process(&c, event).await?, value);
    assert_eq!(c.query_one("SELECT count(*) FROM app.provider_event WHERE provider='patreon' AND external_event_ref='refresh:'||$1::uuid::text||':'||$2::uuid::text",&[&association,&event]).await?.get::<_,i64>(0),0);
    Ok(())
}
