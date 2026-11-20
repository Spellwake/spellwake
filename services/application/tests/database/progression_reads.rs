//! Caller-RLS JSON projections over real owned normalized progression.
use super::{TestResult, account, connection, rank_source_integrity, set_auth_claims};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn quests_preserve_explicit_day_nullable_objective_and_scoped_allowance_without_generating()
-> TestResult {
    let c = connection().await?;
    let (owner, subject) = account(&c).await?;
    let (_, stranger) = account(&c).await?;
    let def:Uuid=c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref) VALUES($1,'fixture','fixture:objective',3,500,'fixture:casts','fixture:v1','fixture:qualified') RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    let generated:Value=c.query_one("SELECT app.generate_daily_quest($1,$2,to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD'),$3)",&[&owner,&def,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(generated["status"], "success");
    let assignment: Uuid = generated["data"]["resourceId"].as_str().unwrap().parse()?;
    let unqualified:Uuid=c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins) VALUES($1,'fixture','fixture:unavailable',3,500) RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    // Explicit reference-only retained data cannot acquire qualified objective claims.
    let retained:Uuid=c.query_one("INSERT INTO app.quest_assignment(account_id,definition_id,source_day,assignment_ref,progress) VALUES($1,$2,'2026-01-01',$3,0) RETURNING id",&[&owner,&unqualified,&Uuid::new_v4().to_string()]).await?.get(0);
    set_auth_claims(&c, subject).await?;
    let response: Value = c
        .query_one("SELECT app.read_my_quests(NULL,100)", &[])
        .await?
        .get(0);
    let typed: spellwake_application::ApiResponse<spellwake_application::progression::QuestPage> =
        serde_json::from_value(response.clone())?;
    if let spellwake_application::ApiResponse::Success { data } = typed {
        data.validate(owner, None, 100)?;
    } else {
        panic!("missing own quest page");
    }
    assert_eq!(
        response["data"]["daily"]["generatedAssignmentId"],
        assignment.to_string()
    );
    let items = response["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .find(|v| v["id"] == retained.to_string())
            .unwrap()["objective"]
            .is_null()
    );
    assert_eq!(
        items
            .iter()
            .find(|v| v["id"] == assignment.to_string())
            .unwrap()["objective"]["target"],
        "3"
    );
    let page: Value = c
        .query_one("SELECT app.read_my_quests(NULL,1)", &[])
        .await?
        .get(0);
    assert_eq!(page["data"]["nextCursor"], page["data"]["items"][0]["id"]);
    set_auth_claims(&c, stranger).await?;
    let foreign: Value = c
        .query_one("SELECT app.read_my_quests(NULL,100)", &[])
        .await?
        .get(0);
    assert_eq!(foreign["data"]["items"], json!([]));
    assert!(foreign["data"]["daily"].is_null());
    assert!(foreign["data"]["utcDay"].is_string());
    assert_eq!(
        c.query_one("SELECT app.read_my_quests(NULL,101)", &[])
            .await?
            .get::<_, Value>(0)["data"]["query"],
        "invalid"
    );
    c.query_one("SELECT set_config('request.jwt.claims','{}',false)", &[])
        .await?;
    assert_eq!(
        c.query_one("SELECT app.read_my_quests(NULL,100)", &[])
            .await?
            .get::<_, Value>(0)["data"]["session"],
        "unauthorized"
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn rank_filters_calendar_and_history_are_owner_bound_and_keep_exact_wire_types() -> TestResult
{
    let c = connection().await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING",&[]).await?;
    let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(1981,1,'1981-01-01T00:00:00Z','1981-04-01T00:00:00Z','fixture','closed') RETURNING id",&[]).await?.get(0);
    let (result, format, owner, _) =
        rank_source_integrity::source(&c, None, 1, "1981-02-01T00:00:00Z").await?;
    assert_eq!(
        rank_source_integrity::consume(&c, result).await?["status"],
        "success"
    );
    let subject: Uuid = c
        .query_one(
            "SELECT auth_subject FROM app.account WHERE id=$1",
            &[&owner],
        )
        .await?
        .get(0);
    let (_, other) = account(&c).await?;
    set_auth_claims(&c, subject).await?;
    let value: Value = c
        .query_one(
            "SELECT app.read_my_ranks(NULL,100,$1,$2)",
            &[&format, &season],
        )
        .await?
        .get(0);
    let parsed: spellwake_application::ApiResponse<spellwake_application::progression::RankPage> =
        serde_json::from_value(value.clone())?;
    if let spellwake_application::ApiResponse::Success { data } = parsed {
        data.validate(owner, None, 100, Some(format), Some(season))?;
    } else {
        panic!("missing own ranks");
    }
    assert_eq!(value["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        c.query_one(
            "SELECT app.read_my_ranks(NULL,100,$1,NULL)",
            &[&Uuid::new_v4()]
        )
        .await?
        .get::<_, Value>(0)["data"]["items"],
        json!([])
    );
    let calendar: Value = c
        .query_one("SELECT app.read_my_seasons(NULL,100)", &[])
        .await?
        .get(0);
    let parsed: spellwake_application::ApiResponse<spellwake_application::progression::SeasonPage> =
        serde_json::from_value(calendar)?;
    if let spellwake_application::ApiResponse::Success { data } = parsed {
        data.validate(owner, None, 100)?;
        assert!(
            data.items
                .iter()
                .any(|item| item.id == season && item.state == "closed")
        );
    } else {
        panic!("missing season calendar");
    }
    set_auth_claims(&c, other).await?;
    assert_eq!(
        c.query_one(
            "SELECT app.read_my_ranks(NULL,100,$1,$2)",
            &[&format, &season]
        )
        .await?
        .get::<_, Value>(0)["data"]["items"],
        json!([])
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
