//! Private SQL projection/ACL controls; no native elapsed-time qualification.
use super::*;
const SIGNATURE: &str = "app.read_native_transition_sources(uuid,uuid,uuid,numeric,integer)";
async fn page(c: &Client, f: &Native) -> FixtureResult<Value> {
    let anchor: Uuid = c.query_one("SELECT id FROM app.game_checkpoint WHERE game_id=$1 ORDER BY accepted_revision DESC LIMIT 1", &[&f.game]).await?.get(0);
    Ok(c.query_one(
        "SELECT app.read_native_transition_sources($1,$2,$3,NULL,32)",
        &[&f.game, &f.context, &anchor],
    )
    .await?
    .get(0))
}
#[tokio::test]
#[ignore = "requires migrated isolated Postgres; replaces history reader inside private fixture realm"]
async fn bank_history_projection_preserves_ordinary_bytes_and_private_access() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = native(&c, "bo1").await?;
    host_advance(&c, &f).await?;
    let current = page(&c, &f).await?;
    assert_eq!(current["status"], "success");
    assert_eq!(current["data"]["groups"].as_array().unwrap().len(), 2);
    assert!(
        current["data"]["groups"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g.get("bankExpiry").is_none())
    );
    let old =
        include_str!("../../../supabase/migrations/20261106023800_native_transition_sources.sql");
    let old_body = historical_function(old, "read_native_transition_sources").replacen(
        "CREATE FUNCTION",
        "CREATE OR REPLACE FUNCTION",
        1,
    );
    c.batch_execute(&old_body).await?;
    assert_eq!(page(&c, &f).await?, current);
    let new = include_str!(
        "../../../supabase/migrations/20261116024300_native_bank_history_evidence.sql"
    );
    c.batch_execute(historical_function(new, "read_native_transition_sources"))
        .await?;
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await?;
    assert_eq!(page(&c, &f).await?, current);
    let anchor: Uuid = c.query_one("SELECT id FROM app.game_checkpoint WHERE game_id=$1 ORDER BY accepted_revision DESC LIMIT 1", &[&f.game]).await?.get(0);
    for role in ["anon", "authenticated", "supabase_auth_admin"] {
        let allowed: bool = c
            .query_one(
                "SELECT has_function_privilege($1,$2,'EXECUTE')",
                &[&role, &SIGNATURE],
            )
            .await?
            .get(0);
        assert!(!allowed, "unexpected private history access for {role}");
        c.batch_execute("SAVEPOINT denied_history").await?;
        c.batch_execute(&format!("SET LOCAL ROLE {role}")).await?;
        let error = c
            .query_one(
                "SELECT app.read_native_transition_sources($1,$2,$3,NULL,32)",
                &[&f.game, &f.context, &anchor],
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::INSUFFICIENT_PRIVILEGE));
        c.batch_execute("ROLLBACK TO SAVEPOINT denied_history")
            .await?;
    }
    c.batch_execute("SET LOCAL ROLE service_role").await?;
    let service: Value = c
        .query_one(
            "SELECT app.read_native_transition_sources($1,$2,$3,NULL,32)",
            &[&f.game, &f.context, &anchor],
        )
        .await?
        .get(0);
    assert_eq!(service, current);
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires migrated isolated Postgres; rejects unsupported source claim"]
async fn native_bank_history_refuses_a_terminal_cause_without_timeout_authority() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = native(&c, "bo1").await?;
    let winner: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND id<>$2",
            &[&f.matched, &f.seat],
        )
        .await?
        .get(0);
    let mut body = candidate(&c, &f).await?;
    for clock in body["clocks"].as_array_mut().unwrap() {
        clock.as_object_mut().unwrap().remove("eligibility");
    }
    body["terminal"] = json!({"outcome":"win","winnerSeatIds":[winner],"winnersRef":body["factsRef"],"reason":"native_bank_expired"});
    let rows=c.query("SELECT id FROM app.match_seat WHERE match_id=$1 AND controller_kind='human' ORDER BY id", &[&f.matched]).await?;
    body["participation"]=json!(rows.iter().map(|r|json!({"seatId":r.get::<_,Uuid>(0),"ownTurns":"0","reason":"native_turn_starts_v1"})).collect::<Vec<_>>());
    let original = json!({"version":"native-host-input-v1","basis":basis(&f,"0"),"kind":"clock","inputRef":artifact(&f,"input",200),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
    let accepted: Value = c
        .query_one(
            "SELECT app.commit_native_host_input($1,$2,$3,$4)",
            &[
                &f.game,
                &Uuid::new_v4(),
                &original,
                &json!({"owner":OWNER,"fenceEpoch":"1","candidate":body}),
            ],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    let anchor: Uuid = c.query_one("SELECT id FROM app.game_checkpoint WHERE game_id=$1 ORDER BY accepted_revision DESC LIMIT 1", &[&f.game]).await?.get(0);
    c.batch_execute(
        "SET CONSTRAINTS ALL IMMEDIATE; SET LOCAL ROLE service_role; SAVEPOINT invalid_bank_source",
    )
    .await?;
    let error = c
        .query_one(
            "SELECT app.read_native_transition_sources($1,$2,$3,NULL,32)",
            &[&f.game, &f.context, &anchor],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
    c.batch_execute("ROLLBACK TO SAVEPOINT invalid_bank_source; ROLLBACK")
        .await?;
    Ok(())
}
