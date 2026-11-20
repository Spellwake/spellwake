//! Synthetic opening/history manifests qualify SQL policy and cutoff fences, not real activity/physical evidence.
use super::{
    TestResult, account, connection, game_scope, host_integrity, pricing, wait_for_database_lock,
};
use serde_json::{Value, json};
use spellwake_application::supply_source_ports::{FinalizeQuarterlySupply, SupplyBallot};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const HASH: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) async fn manifest(c: &Client) -> FixtureResult<(Value, Vec<Uuid>)> {
    let mut candidates = Vec::new();
    let mut sets = Vec::new();
    for _ in 0..6 {
        let p = pricing::product(c, true).await?;
        let set: Uuid = c
            .query_one(
                "SELECT set_id FROM app.booster_product WHERE id=$1",
                &[&p.product],
            )
            .await?
            .get(0);
        sets.push(set);
        candidates.push(json!({"setId":set,"products":[{"recipeId":p.recipe,"weight":"1","normalQuantity":"100","modifierN":"1","modifierD":"1","modifierRef":HASH}]}));
    }
    Ok((
        json!({"version":"supply-ballot-v1","supplyCoefficient":"60","sourceRef":HASH,"tieSeed":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","candidates":candidates}),
        sets,
    ))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn current_quarter_ballot_freezes_candidates_and_replaces_only_qualified_voters_approvals()
-> TestResult {
    let c = connection().await?;
    let (manifest, sets) = manifest(&c).await?;
    let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) SELECT extract(year FROM start_value)::bigint,extract(quarter FROM start_value)::bigint,start_value AT TIME ZONE 'UTC',(start_value+interval '3 months') AT TIME ZONE 'UTC','fixture','planned' FROM(SELECT date_trunc('quarter',clock_timestamp() AT TIME ZONE 'UTC')+interval '3 months' start_value) q RETURNING id", &[]).await?.get(0);
    assert_eq!(c.execute("INSERT INTO app.supply_drop(season_id,set_id,population_snapshot_ref,quantity,policy_ref) VALUES($1,$2,'fixture',1,'bypass')", &[&season,&sets[0]]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    let opened: Value = c
        .query_one(
            "SELECT app.open_supply_ballot($1,$2)",
            &[&season, &manifest],
        )
        .await?
        .get(0);
    SupplyBallot::from_bytes(season, &serde_json::to_vec(&manifest)?)?
        .decode_response(&serde_json::to_vec(&opened)?, None)?;
    assert_eq!(opened["status"], "success");
    assert_eq!(
        c.query_one(
            "SELECT app.open_supply_ballot($1,$2)",
            &[&season, &manifest]
        )
        .await?
        .get::<_, Value>(0),
        opened
    );
    let no_game = account(&c).await?.0;
    let denied: Value = c
        .query_one(
            "SELECT app.replace_supply_ballot($1,$2,$3,$4)",
            &[&no_game, &season, &vec![sets[0]], &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(denied["status"], "fail");
    let f = game_scope::seed(&c).await?;
    c.execute(
        "UPDATE app.game SET state='completed' WHERE id=$1",
        &[&f.game],
    )
    .await?;
    c.execute("INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision) VALUES($1,$2,$3,3,true,'fixture',clock_timestamp(),0)", &[&f.game,&f.seat,&f.account]).await?;
    let request = Uuid::new_v4();
    let approvals = vec![sets[0], sets[1]];
    let first: Value = c
        .query_one(
            "SELECT app.replace_supply_ballot($1,$2,$3,$4)",
            &[&f.account, &season, &approvals, &request],
        )
        .await?
        .get(0);
    assert_eq!(first["status"], "success");
    assert_eq!(
        c.query_one(
            "SELECT app.replace_supply_ballot($1,$2,$3,$4)",
            &[&f.account, &season, &approvals, &request]
        )
        .await?
        .get::<_, Value>(0),
        first
    );
    c.query_one(
        "SELECT app.replace_supply_ballot($1,$2,$3,$4)",
        &[&f.account, &season, &vec![sets[2]], &Uuid::new_v4()],
    )
    .await?;
    assert_eq!(
        c.query_one(
            "SELECT array_agg(set_id) FROM app.set_ballot WHERE season_id=$1 AND account_id=$2",
            &[&season, &f.account]
        )
        .await?
        .get::<_, Vec<Uuid>>(0),
        vec![sets[2]]
    );
    let tallies: Value = c
        .query_one("SELECT app.supply_ballot_tallies($1)", &[&season])
        .await?
        .get(0);
    assert_eq!(tallies.as_array().unwrap().len(), 6);
    assert_eq!(tallies[0]["setId"], sets[2].to_string());
    assert_eq!(tallies[0]["approvals"], "1");
    assert!(
        tallies.as_array().unwrap()[1..]
            .iter()
            .all(|x| x["approvals"] == "0")
    );
    let mut changed = manifest.clone();
    changed["supplyCoefficient"] = json!("61");
    assert_eq!(
        c.query_one("SELECT app.open_supply_ballot($1,$2)", &[&season, &changed])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT app.replace_supply_ballot($1,$2,$3,$4)",
            &[
                &f.account,
                &season,
                &vec![sets[0], sets[0]],
                &Uuid::new_v4()
            ]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    super::human_foundation::verify_supply_ballot_boundaries(&c, season, &sets).await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn quarterly_cutoff_waits_for_source_commit_counts_direct_activity_and_publishes_five_conserved_drops_once()
-> TestResult {
    let c = connection().await?;
    let (manifest, _) = manifest(&c).await?;
    let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(2009,2,'2009-04-01T00:00:00Z','2009-07-01T00:00:00Z','fixture','planned') RETURNING id", &[]).await?.get(0);
    // Accepted historical opening is a deliberately privileged normalized fixture.
    // Current-quarter opening clock gates are tested separately above.
    let receipt:Uuid=c.query_one("INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source) VALUES('supply-season:'||$1::uuid::text,'supply_ballot_open_v1',$1::uuid::text,'initial',app.auth_json_hash($2),'pending','',$2::jsonb||jsonb_build_object('openedAt','2009-01-01T00:00:00.000000Z')) RETURNING id", &[&season,&manifest]).await?.get(0);
    c.query_one(
        "SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid))",
        &[&receipt, &season],
    )
    .await?;
    let f = game_scope::seed(&c).await?;
    c.execute(
        "UPDATE app.game SET state='completed' WHERE id=$1",
        &[&f.game],
    )
    .await?;
    let waiting = connection().await?;
    waiting
        .batch_execute("SET TIME ZONE 'Europe/Berlin'")
        .await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let host = host_integrity::fixture(&c).await?;
    let mut terminal = host_integrity::candidate(&host);
    terminal["terminal"] =
        json!({"outcome":"draw","winnerSeatIds":[],"winnersRef":"fixture:none","reason":"fixture"});
    terminal["participation"] = json!([{"seatId":host.seat,"ownTurns":"3","reason":"fixture"}]);
    c.batch_execute("BEGIN").await?;
    assert_eq!(
        host_integrity::accept(&c, host.game, "1", "0", Uuid::new_v4(), &terminal).await?["status"],
        "success"
    );
    c.execute("INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision) VALUES($1,$2,$3,3,true,'fixture','2009-01-02T00:00:00Z',0)", &[&f.game,&f.seat,&f.account]).await?;
    let task = tokio::spawn(async move {
        waiting
            .query_one("SELECT app.finalize_quarterly_supply($1)", &[&season])
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    let result = task.await??;
    FinalizeQuarterlySupply { season_id: season }.decode_response(&serde_json::to_vec(&result)?)?;
    assert_eq!(result["status"], "success");
    assert_eq!(result["data"]["quantity"], "20");
    assert_eq!(
        c.query_one("SELECT app.finalize_quarterly_supply($1)", &[&season])
            .await?
            .get::<_, Value>(0),
        result
    );
    let totals=c.query_one("SELECT count(*)::bigint,sum(quantity)::text,(SELECT sum(starting_quantity)::text FROM app.supply_stock s JOIN app.supply_drop d ON d.id=s.drop_id WHERE d.season_id=$1) FROM app.supply_drop WHERE season_id=$1", &[&season]).await?;
    assert_eq!(totals.get::<_, i64>(0), 5);
    assert_eq!(totals.get::<_, String>(1), "20");
    assert_eq!(totals.get::<_, String>(2), "20");
    assert_eq!(c.query_one("SELECT normalized_source->>'cutoff' FROM app.command_receipt WHERE principal_key='supply-season:'||$1::uuid::text AND scope='supply_ballot_result_v1'", &[&season]).await?.get::<_,String>(0),"2009-03-25T00:00:00.000000Z");
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.monthly_account_activity WHERE account_id=$1",
            &[&f.account]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn zero_population_creates_no_stock_and_weighted_products_conserve_each_selected_sets_share()
-> TestResult {
    let c = connection().await?;
    for (year, populated) in [(1970_i32, false), (1971_i32, true)] {
        let (mut source, _) = manifest(&c).await?;
        let mut recipes = Vec::new();
        for candidate in source["candidates"].as_array_mut().unwrap() {
            let set: Uuid = candidate["setId"].as_str().unwrap().parse()?;
            let extra = pricing::product_in_set(&c, true, Some(set)).await?;
            recipes.push(
                candidate["products"][0]["recipeId"]
                    .as_str()
                    .unwrap()
                    .parse::<Uuid>()?,
            );
            recipes.push(extra.recipe);
            candidate["products"].as_array_mut().unwrap().push(json!({"recipeId":extra.recipe,"weight":"2","normalQuantity":"100","modifierN":"1","modifierD":"1","modifierRef":HASH}));
        }
        let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES($1::integer,2,make_timestamptz($1::integer,4,1,0,0,0,'UTC'),make_timestamptz($1::integer,7,1,0,0,0,'UTC'),'fixture','planned') RETURNING id", &[&year]).await?.get(0);
        let opened_at = format!("{year}-01-01T00:00:00.000000Z");
        let opening:Uuid=c.query_one("INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source) VALUES('supply-season:'||$1::uuid::text,'supply_ballot_open_v1',$1::uuid::text,'initial',app.auth_json_hash($2),'pending','',$2::jsonb||jsonb_build_object('openedAt',$3::text)) RETURNING id", &[&season,&source,&opened_at]).await?.get(0);
        c.query_one(
            "SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid))",
            &[&opening, &season],
        )
        .await?;
        if populated {
            let f = game_scope::seed(&c).await?;
            c.execute(
                "UPDATE app.game SET state='completed' WHERE id=$1",
                &[&f.game],
            )
            .await?;
            let completed = format!("{year}-01-02T00:00:00.000000Z");
            c.execute("INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision) VALUES($1,$2,$3,3,true,'fixture',$4::text::timestamptz,0)", &[&f.game,&f.seat,&f.account,&completed]).await?;
        }
        let result: Value = c
            .query_one("SELECT app.finalize_quarterly_supply($1)", &[&season])
            .await?
            .get(0);
        FinalizeQuarterlySupply { season_id: season }
            .decode_response(&serde_json::to_vec(&result)?)?;
        assert_eq!(result["status"], "success");
        assert_eq!(
            result["data"]["quantity"],
            if populated { "20" } else { "0" }
        );
        assert_eq!(
            c.query_one(
                "SELECT count(*)::bigint FROM app.supply_drop WHERE season_id=$1",
                &[&season]
            )
            .await?
            .get::<_, i64>(0),
            5
        );
        if populated {
            for row in c.query("SELECT d.set_id,d.quantity::text,p.id,r.id,coalesce(sum(stock.starting_quantity),0)::text FROM app.supply_drop d JOIN app.booster_product p ON p.set_id=d.set_id JOIN app.booster_recipe r ON r.product_id=p.id LEFT JOIN app.supply_stock stock ON stock.drop_id=d.id AND stock.recipe_id=r.id WHERE d.season_id=$1 GROUP BY d.id,p.id,r.id ORDER BY d.set_id,p.id", &[&season]).await? {
 let set:Uuid=row.get(0);assert_eq!(row.get::<_,String>(1),"4");let product:Uuid=row.get(2);let recipe:Uuid=row.get(3);
 let candidate=source["candidates"].as_array().unwrap().iter().find(|x|x["setId"].as_str()==Some(set.to_string().as_str())).unwrap();
 let weight=candidate["products"].as_array().unwrap().iter().find(|x|x["recipeId"].as_str()==Some(recipe.to_string().as_str())).unwrap()["weight"].as_str().unwrap();
 let first:Uuid=c.query_one("SELECT min(id::text)::uuid FROM app.booster_product WHERE set_id=$1", &[&set]).await?.get(0);
 let expected=if weight=="1" {1}else{2}+i64::from(product==first);
 assert_eq!(row.get::<_,String>(4),expected.to_string());
 }
        } else {
            assert_eq!(
                c.query_one(
                    "SELECT count(*)::bigint FROM app.supply_stock WHERE recipe_id=ANY($1)",
                    &[&recipes]
                )
                .await?
                .get::<_, i64>(0),
                0
            );
        }
        assert_eq!(
            c.query_one("SELECT app.finalize_quarterly_supply($1)", &[&season])
                .await?
                .get::<_, Value>(0),
            result
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn future_quarter_cannot_issue_stock_or_reprice_products_before_its_start() -> TestResult {
    let c = connection().await?;
    let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(2198,2,'2198-04-01T00:00:00Z','2198-07-01T00:00:00Z','fixture','planned') RETURNING id",&[]).await?.get(0);
    let error = c
        .query_one("SELECT app.finalize_quarterly_supply($1)", &[&season])
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        error.as_db_error().unwrap().message(),
        "supply quarter has not started"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.supply_drop WHERE season_id=$1",
            &[&season]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='supply_ballot_result_v1' AND target_ref=$1",&[&season.to_string()]).await?.get::<_,i64>(0),0);
    Ok(())
}
