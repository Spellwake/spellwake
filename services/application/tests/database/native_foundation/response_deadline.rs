//! Exercise the real server interval and original retry against one native SQL opening.
use super::*;

#[tokio::test]
#[ignore = "requires migrated local Postgres and a real two-minute opening wait"]
async fn opening_response_timeout_uses_the_original_server_deadline_once() -> TestResult {
    let c = connection().await?;
    let (room, matched, game) =
        super::super::native_elimination_integrity::reserve_three(&c, false).await?;
    let mut f = reserved_native_match(&c, "bo1", &room, matched).await?;
    f.opening["clockObservation"] = json!({"reason":"native_decision","seatId":f.seat,"acceptedRevision":"0","proofRef":f.opening["stateRef"]});
    assert_eq!(
        accept_opening_fixture(&c, &f, &f.opening).await?["status"],
        "success"
    );
    let read = "SELECT app.read_native_opening_response_wait($1)";
    for role in [
        "anon",
        "authenticated",
        "supabase_auth_admin",
        "service_role",
    ] {
        let row=c.query_one("SELECT has_function_privilege($1::text,'app.read_native_opening_response_wait(uuid)','execute'),has_function_privilege($1::text,'app.native_opening_response_wait(uuid)','execute'),has_function_privilege($1::text,'app.guard_native_response_deadline()','execute')",&[&role]).await?;
        assert_eq!(row.get::<_, bool>(0), role == "service_role");
        assert!(!row.get::<_, bool>(1));
        assert!(!row.get::<_, bool>(2));
    }
    c.batch_execute("SET ROLE service_role").await?;
    let observed: Value = c.query_one(read, &[&game]).await?.get(0);
    assert_eq!(observed["status"], "success", "{observed}");
    let wait = &observed["data"];
    assert_eq!(wait["due"], false);
    assert_eq!(wait["seatId"], json!(f.seat));
    assert_eq!(wait["decisionRef"], f.opening["stateRef"]);
    c.batch_execute("RESET ROLE").await?;
    let mut frame = candidate(&c, &f).await?;
    let original = json!({"version":"native-host-input-v1","basis":basis(&f,"0"),"kind":"clock","inputRef":artifact(&f,"input",40),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
    frame["nativeEliminations"] = json!([{"seatId":f.seat,"reason":"native_response_timeout","sourceRevision":"0","decisionRef":format!("multiplayer-response-v1:{}",wait["decisionRef"].as_str().unwrap()),"expiryRef":original["inputRef"],"expiredAt":wait["expiresAt"]}]);
    let prepared = json!({"owner":OWNER,"fenceEpoch":"1","candidate":frame});
    let request = Uuid::new_v4();
    let commit = "SELECT app.commit_native_host_input($1,$2,$3,$4)";
    let early = c
        .query_one(commit, &[&game, &request, &original, &prepared])
        .await
        .unwrap_err();
    assert_eq!(
        early.code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE),
        "early: {early:?}"
    );
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&game]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    let again = accept_opening_fixture(&c, &f, &f.opening).await?;
    assert_eq!(again["status"], "success");
    let unchanged: Value = c.query_one(read, &[&game]).await?.get(0);
    assert_eq!(unchanged, observed);
    let seconds:f64=c.query_one("SELECT extract(epoch FROM (app.financial_instant($1)-clock_timestamp()))::double precision",&[&wait["expiresAt"].as_str().unwrap()]).await?.get(0);
    assert!(seconds > 0.0 && seconds <= 120.0);
    tokio::time::sleep(std::time::Duration::from_secs_f64(seconds + 0.05)).await;
    let due: Value = c.query_one(read, &[&game]).await?.get(0);
    assert_eq!(due["data"]["due"], true);
    let mut changed = prepared.clone();
    changed["candidate"]["nativeEliminations"][0]["decisionRef"] =
        json!(artifact(&f, "state", 999));
    assert_eq!(
        c.query_one(commit, &[&game, &request, &original, &changed])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let accepted: Value = c
        .query_one(commit, &[&game, &request, &original, &prepared])
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    let retry: Value = c
        .query_one(commit, &[&game, &request, &original, &prepared])
        .await?
        .get(0);
    assert_eq!(retry, accepted);
    assert_eq!(
        c.query_one(read, &[&game]).await?.get::<_, Value>(0)["status"],
        "fail"
    );
    assert!(c.query_one("SELECT state='active' AND accepted_revision=1 AND (SELECT count(*) FROM app.command_receipt WHERE scope='native_seat_elimination_v1' AND target_ref=$2::uuid::text)=1 FROM app.game WHERE id=$1",&[&game,&f.seat]).await?.get::<_,bool>(0));
    let history: Value = c.query_one(
        "SELECT app.read_native_transition_sources($1,$2,id,0,1) FROM app.game_checkpoint WHERE game_id=$1 AND accepted_revision=1",
        &[&game, &f.context],
    ).await?.get(0);
    assert_eq!(history["status"], "success", "{history}");
    let proof = &history["data"]["groups"][0]["openingResponseExpiry"];
    assert_eq!(proof["originalRequestId"], json!(request));
    assert_eq!(proof["original"], original);
    assert_eq!(proof["seatId"], json!(f.seat));
    assert_eq!(proof["decisionRef"], wait["decisionRef"]);
    assert_eq!(proof["expiresAt"], wait["expiresAt"]);
    Ok(())
}
