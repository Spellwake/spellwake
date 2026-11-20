//! Retained synthetic stop-source projection, not provider/controller qualification.
use super::{TestResult, connection};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

async fn retained_stop(
    c: &Client,
    binding: Uuid,
    epoch: &str,
) -> Result<Uuid, tokio_postgres::Error> {
    // Model accepted retained source rows, including nullable historical process
    // fields. No trigger is disabled and no native/provider authority is asserted.
    let source = json!({"bindingId":binding,"epoch":epoch,"providerStartId":null,"processEpoch":null,"providerRef":null});
    let id: Uuid = c.query_one("INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source) VALUES('ai-runtime','ai_stop_request_v1',$1,$2,app.auth_json_hash($3),'pending','',$3) RETURNING id", &[&binding.to_string(),&Uuid::new_v4().to_string(),&source]).await?.get(0);
    c.query_one(
        "SELECT app.finish_account_command($1,true,$2)",
        &[&id, &json!({"resourceId":binding})],
    )
    .await?;
    Ok(id)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn bounded_stop_pages_preserve_all_retained_epochs_and_exclusive_cursor() -> TestResult {
    let c = connection().await?;
    let binding = Uuid::new_v4();
    let mut expected = Vec::new();
    let mut epochs = std::collections::BTreeMap::new();
    for epoch in ["1", "2", "3"] {
        let id = retained_stop(&c, binding, epoch).await?;
        expected.push(id);
        epochs.insert(id, epoch);
    }
    expected.sort_unstable();
    let foreign = Uuid::new_v4();
    retained_stop(&c, foreign, "9").await?;
    let mut after: Option<Uuid> = None;
    let mut actual = Vec::new();
    for _ in 0..3 {
        let page: Value = c
            .query_one(
                "SELECT app.read_ai_stop_work_page($1,$2,1)",
                &[&binding, &after],
            )
            .await?
            .get(0);
        let decoded = spellwake_application::controller_ports::ReadStopWork {
            binding_id: binding,
            after,
            limit: 1,
        }
        .decode_page(page.to_string().as_bytes())?;
        assert_eq!(decoded.items.len(), 1);
        assert_eq!(page.as_object().unwrap().len(), 3);
        assert_eq!(page["bindingId"], binding.to_string());
        let rows = page["items"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["bindingId"], binding.to_string());
        assert_eq!(row.as_object().unwrap().len(), 6);
        for field in ["startId", "processEpoch", "providerRef"] {
            assert!(row.get(field).is_some_and(Value::is_null));
        }
        let id: Uuid = row["stopId"].as_str().unwrap().parse()?;
        assert_eq!(row["epoch"], epochs[&id]);
        assert!(after.is_none_or(|previous| id > previous));
        assert_eq!(page["nextCursor"], id.to_string());
        actual.push(id);
        after = Some(id);
    }
    assert_eq!(actual, expected);
    let empty: Value = c
        .query_one(
            "SELECT app.read_ai_stop_work_page($1,$2,1)",
            &[&binding, &after],
        )
        .await?
        .get(0);
    assert_eq!(
        empty,
        json!({"bindingId":binding,"items":[],"nextCursor":null})
    );
    let page: Value = c
        .query_one(
            "SELECT app.read_ai_stop_work_page($1,NULL,100)",
            &[&binding],
        )
        .await?
        .get(0);
    assert!(
        spellwake_application::controller_ports::ReadStopWork {
            binding_id: binding,
            after: None,
            limit: 100
        }
        .decode_page(page.to_string().as_bytes())
        .is_ok()
    );
    assert_eq!(page["items"].as_array().unwrap().len(), 3);
    assert!(page["nextCursor"].is_null());
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.read_ai_stop_work($1)",
            &[&binding]
        )
        .await?
        .get::<_, i64>(0),
        3
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn bounded_stop_projection_preserves_original_nonnull_process_generation() -> TestResult {
    let c = connection().await?;
    let room = super::room_start_integrity::room(&c, false).await?;
    let before = c.query_one("SELECT b.id,b.provider_start_receipt_id,b.provider_runner_ref,b.binding_epoch::text FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1", &[&room.id]).await?;
    let binding: Uuid = before.get(0);
    let start: Uuid = before.get(1);
    let provider: String = before.get(2);
    let epoch: String = before.get(3);
    let rebind: Value = c
        .query_one(
            "SELECT app.rebind_ai_runner($1,$2::text::numeric,'fixture:paged-stop-rebind',$3)",
            &[&binding, &epoch, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(rebind["status"], "success");
    let stop: Uuid = rebind["data"]["previousStopId"].as_str().unwrap().parse()?;
    let page: Value = c
        .query_one(
            "SELECT app.read_ai_stop_work_page($1,NULL,100)",
            &[&binding],
        )
        .await?
        .get(0);
    let decoded = spellwake_application::controller_ports::ReadStopWork {
        binding_id: binding,
        after: None,
        limit: 100,
    }
    .decode_page(page.to_string().as_bytes())?;
    assert_eq!(decoded.items[0].start_id, Some(start));
    assert_eq!(
        decoded.items[0].provider_ref.as_deref(),
        Some(provider.as_str())
    );
    let expected = json!({"bindingId":binding,"stopId":stop,"epoch":epoch,"startId":start,"processEpoch":epoch,"providerRef":provider});
    assert_eq!(
        page,
        json!({"bindingId":binding,"items":[expected.clone()],"nextCursor":null})
    );
    let original: Value = c
        .query_one(
            "SELECT work FROM app.read_ai_stop_work($1) work",
            &[&binding],
        )
        .await?
        .get(0);
    assert_eq!(original, expected);
    assert!(
        c.query_one(
            "SELECT binding_epoch::text<>$2 FROM app.ai_runner_binding WHERE id=$1",
            &[&binding, &epoch]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn bounded_stop_discovery_rejects_invalid_envelopes_and_is_service_only() -> TestResult {
    let c = connection().await?;
    let binding = Uuid::new_v4();
    for (target, after, limit) in [
        (Some(binding), None, Some(0_i32)),
        (Some(binding), None, Some(101)),
        (Some(binding), None, None),
        (None, None, Some(1)),
        (Some(Uuid::nil()), None, Some(1)),
        (Some(binding), Some(Uuid::nil()), Some(1)),
    ] {
        assert_eq!(
            c.query_one(
                "SELECT app.read_ai_stop_work_page($1,$2,$3)",
                &[&target, &after, &limit]
            )
            .await
            .unwrap_err()
            .code(),
            Some(&SqlState::INVALID_PARAMETER_VALUE)
        );
    }
    let empty: Value = c
        .query_one(
            "SELECT app.read_ai_stop_work_page($1,NULL,100)",
            &[&binding],
        )
        .await?
        .get(0);
    assert_eq!(
        empty,
        json!({"bindingId":binding,"items":[],"nextCursor":null})
    );
    for role in ["anon", "authenticated", "supabase_auth_admin"] {
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        assert_eq!(
            c.query_one("SELECT app.read_ai_stop_work_page($1,NULL,1)", &[&binding])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        c.batch_execute("RESET ROLE").await?;
    }
    c.batch_execute("SET ROLE service_role").await?;
    let service_page: Value = c
        .query_one("SELECT app.read_ai_stop_work_page($1,NULL,1)", &[&binding])
        .await?
        .get(0);
    assert_eq!(service_page, empty);
    assert_eq!(
        c.query_one("SELECT app.read_ai_stop_work($1)", &[&binding])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
