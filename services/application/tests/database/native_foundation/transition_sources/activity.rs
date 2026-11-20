//! New activity-codec SQL shape/source tests. Synthetic operands are NOT native casts.
use super::*;
const CODEC: &str = "native-transition-activity-v1";
async fn activity_candidate(c: &Client, f: &Native) -> FixtureResult<Value> {
    let mut body = candidate(c, f).await?;
    body["events"] = json!([body["events"][0]]);
    body["eventSchemaRevision"] = json!(CODEC);
    let account: Uuid = c
        .query_one(
            "SELECT account_id FROM app.match_seat WHERE id=$1",
            &[&f.seat],
        )
        .await?
        .get(0);
    body["facts"] = json!([{"eventIndex":"0","accountId":account,"kind":"native:cast-red-or-green","revision":"native-cast-colors-v1","value":"1"}]);
    Ok(body)
}
fn original(f: &Native) -> Value {
    json!({"version":"native-host-input-v1","basis":basis(f,"0"),"kind":"entropy","inputRef":artifact(f,"input",901),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)})
}
async fn commit(
    c: &Client,
    f: &Native,
    request: Uuid,
    body: &Value,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.commit_native_host_input($1,$2,$3,$4)",
        &[
            &f.game,
            &request,
            &original(f),
            &json!({"owner":OWNER,"fenceEpoch":"1","candidate":body}),
        ],
    )
    .await
    .map(|r| r.get(0))
}
#[tokio::test]
#[ignore = "requires migrated isolated Postgres; synthetic SQL source operands only"]
async fn activity_host_frame_preserves_exact_creator_human_facts_and_original_first_retry()
-> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = native(&c, "bo1").await?;
    let body = activity_candidate(&c, &f).await?;
    let request = Uuid::new_v4();
    let ack = commit(&c, &f, request, &body).await?;
    assert_eq!(ack["status"], "success", "{ack}");
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await?;
    let anchor = tip(&c, &f).await?;
    let result = page(&c, &f, anchor, Some("0"), 1).await?;
    let group = &result["data"]["groups"][0];
    assert_eq!(group["eventSchemaRevision"], CODEC);
    assert_eq!(group["events"].as_array().unwrap().len(), 1);
    assert_eq!(
        group["events"][0]["facts"],
        json!([{"accountId":body["facts"][0]["accountId"],"kind":"native:cast-red-or-green","factRevision":"native-cast-colors-v1","value":"1"}])
    );
    let retry: Value = c
        .query_one(
            "SELECT app.commit_native_host_input($1,$2,$3,NULL)",
            &[&f.game, &request, &original(&f)],
        )
        .await?
        .get(0);
    assert_eq!(retry, ack);
    assert_eq!(
        disposition(&c, f.game, request, &original(&f)).await?["data"]["response"],
        ack
    );
    let counts: (i64, i64) = {
        let row=c.query_one("SELECT (SELECT count(*) FROM app.game_activity_fact WHERE game_id=$1),(SELECT count(*) FROM app.game_checkpoint WHERE game_id=$1)",&[&f.game]).await?;
        (row.get(0), row.get(1))
    };
    assert_eq!(counts, (1, 2));
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires migrated isolated Postgres; SQL shape/rollback boundary only"]
async fn activity_closed_shape_rejects_bad_codec_counts_and_coordinates_atomically() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = native(&c, "bo1").await?;
    let good = activity_candidate(&c, &f).await?;
    let mut cases = vec![];
    // A real account outside the selected human roster, not just a nonexistent UUID.
    let outsider = crate::account(&c).await?.0;
    let mut foreign = good.clone();
    foreign["facts"][0]["accountId"] = json!(outsider);
    cases.push(foreign);
    for codec in [
        Value::Null,
        json!("native-transition-v2"),
        json!({"native-transition-activity-v1":null}),
    ] {
        let mut bad = good.clone();
        bad["eventSchemaRevision"] = codec;
        cases.push(bad);
    }
    for events in [json!([]), json!([good["events"][0], good["events"][0]])] {
        let mut bad = good.clone();
        bad["events"] = events;
        cases.push(bad);
    }
    for (field, value) in [
        ("kind", json!("native:cast-blue")),
        ("revision", json!("native-cast-colors-v2")),
        ("value", json!("0")),
        ("value", json!("01")),
        ("value", json!("18446744073709551616")),
        ("value", json!(1)),
        ("eventIndex", json!("1")),
        ("accountId", json!(Uuid::new_v4())),
    ] {
        let mut bad = good.clone();
        bad["facts"][0][field] = value;
        cases.push(bad);
    }
    let mut duplicate = good.clone();
    duplicate["facts"] = json!([good["facts"][0], good["facts"][0]]);
    cases.push(duplicate);
    for body in cases {
        c.batch_execute("SAVEPOINT invalid_activity").await?;
        assert!(
            commit(&c, &f, Uuid::new_v4(), &body).await.is_err(),
            "accepted corrupt {body}"
        );
        c.batch_execute("ROLLBACK TO SAVEPOINT invalid_activity")
            .await?;
        let row=c.query_one("SELECT accepted_revision::text,(SELECT count(*) FROM app.game_activity_fact WHERE game_id=$1),(SELECT count(*) FROM app.game_checkpoint WHERE game_id=$1) FROM app.game WHERE id=$1",&[&f.game]).await?;
        assert_eq!(row.get::<_, String>(0), "0");
        assert_eq!(row.get::<_, i64>(1), 0);
        assert_eq!(row.get::<_, i64>(2), 1);
        let parents:i64=c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='native_host_input_v1' AND target_ref=$1",&[&f.game.to_string()]).await?.get(0);
        assert_eq!(parents, 0);
    }
    c.batch_execute("SAVEPOINT positive_boundaries").await?;
    let mut positive = good.clone();
    let mut black = good["facts"][0].clone();
    black["kind"] = json!("native:cast-black-or-green");
    positive["facts"][0]["value"] = json!("18446744073709551615");
    positive["facts"].as_array_mut().unwrap().push(black);
    assert_eq!(
        commit(&c, &f, Uuid::new_v4(), &positive).await?["status"],
        "success"
    );
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE;ROLLBACK TO SAVEPOINT positive_boundaries")
        .await?;
    let mut empty = good;
    empty["facts"] = json!([]);
    assert_eq!(
        commit(&c, &f, Uuid::new_v4(), &empty).await?["status"],
        "success"
    );
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE;ROLLBACK")
        .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires migrated isolated Postgres; synthetic actor/frame source only"]
async fn activity_human_and_controller_facades_keep_original_first_dispositions() -> TestResult {
    let c = connection().await?;
    for ai in [false, true] {
        c.batch_execute("BEGIN").await?;
        let f = native_with_roster(&c, "bo1", !ai).await?;
        let (seat, controller) = actor(&c, &f, ai).await?;
        let (original, mut prepared) = player_case(&c, &f, seat, "0").await?;
        prepared["candidate"] = activity_candidate(&c, &f).await?;
        let ack = submit_actor(&c, &f, &original, Some(&prepared), controller.as_ref()).await?;
        assert_eq!(ack["status"], "success", "{ack}");
        c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await?;
        let retry = submit_actor(&c, &f, &original, None, controller.as_ref()).await?;
        assert_eq!(retry, ack);
        // The frame's selected human account is structural source membership;
        // this synthetic test cannot establish whether this AI action cast it.
        c.batch_execute("ROLLBACK").await?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires migrated isolated Postgres; closed codec expansion permission check"]
async fn activity_migration_preserves_canonical_private_exec_roles_and_v1_only_opening()
-> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    for name in [
        "check_native_creator_frame",
        "apply_accept_host_candidate",
        "submit_native_player_command",
        "commit_native_host_input",
    ] {
        let row=c.query_one("SELECT p.prosecdef,p.proconfig,has_function_privilege('service_role',p.oid,'EXECUTE'),has_function_privilege('anon',p.oid,'EXECUTE'),has_function_privilege('authenticated',p.oid,'EXECUTE'),has_function_privilege('supabase_auth_admin',p.oid,'EXECUTE') FROM pg_proc p WHERE p.pronamespace='app'::regnamespace AND p.proname=$1",&[&name]).await?;
        assert!(row.get::<_, bool>(0));
        assert!(
            row.get::<_, Vec<String>>(1)
                .iter()
                .any(|v| v == "search_path=\"\"")
        );
        assert_eq!(row.get::<_, bool>(2), name == "commit_native_host_input");
        for i in 3..6 {
            assert!(!row.get::<_, bool>(i));
        }
    }
    let f = reserved_native(&c, "bo1", true).await?;
    let mut opening = f.opening.clone();
    opening["eventSchemaRevision"] = json!(CODEC);
    c.batch_execute("SAVEPOINT invalid_opening").await?;
    assert!(accept_opening_fixture(&c, &f, &opening).await.is_err());
    c.batch_execute("ROLLBACK TO SAVEPOINT invalid_opening")
        .await?;
    assert_eq!(
        accept_opening_fixture(&c, &f, &f.opening).await?["status"],
        "success"
    );
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE;ROLLBACK")
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres; land-play actor, rollback and retry"]
async fn land_play_requires_its_player_original_and_credits_only_the_selected_human() -> TestResult
{
    let c = connection().await?;
    for ai in [false, true] {
        c.batch_execute("BEGIN").await?;
        let f = native_with_roster(&c, "bo1", !ai).await?;
        let (seat, controller) = actor(&c, &f, ai).await?;
        let (mut original, mut prepared) = player_case(&c, &f, seat, "0").await?;
        let mut body = activity_candidate(&c, &f).await?;
        body["eventSchemaRevision"] = json!("native-transition-land-play-v1");
        body["facts"][0]["kind"] = json!("native:land-play");
        body["facts"][0]["revision"] = json!("native-land-play-v1");
        let human_fact = body["facts"][0].clone();
        if ai {
            body["facts"] = json!([]);
        }
        prepared["candidate"] = body.clone();
        // A host-only entropy original and an unrelated player pass cannot create a land frame.
        c.batch_execute("SAVEPOINT invalid_land").await?;
        assert!(commit(&c, &f, Uuid::new_v4(), &body).await.is_err());
        c.batch_execute("ROLLBACK TO SAVEPOINT invalid_land")
            .await?;
        assert!(
            submit_actor(&c, &f, &original, Some(&prepared), controller.as_ref())
                .await
                .is_err()
        );
        c.batch_execute("ROLLBACK TO SAVEPOINT invalid_land")
            .await?;
        original["action"] = json!({"kind":"play-land", "source":"controlled-object"});
        for facts in [
            json!([human_fact.clone(), human_fact.clone()]),
            json!([{"value":"2"}]),
            if ai {
                json!([human_fact.clone()])
            } else {
                json!([])
            },
        ] {
            prepared["candidate"]["facts"] = facts;
            assert!(
                submit_actor(&c, &f, &original, Some(&prepared), controller.as_ref())
                    .await
                    .is_err()
            );
            c.batch_execute("ROLLBACK TO SAVEPOINT invalid_land")
                .await?;
        }
        prepared["candidate"] = body.clone();
        let ack = submit_actor(&c, &f, &original, Some(&prepared), controller.as_ref()).await?;
        assert_eq!(ack["status"], "success", "{ack}");
        c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await?;
        assert_eq!(
            submit_actor(&c, &f, &original, None, controller.as_ref()).await?,
            ack
        );
        let anchor = tip(&c, &f).await?;
        let result = page(&c, &f, anchor, Some("0"), 1).await?;
        let group = &result["data"]["groups"][0];
        assert_eq!(group["eventSchemaRevision"], body["eventSchemaRevision"]);
        let facts = &group["events"][0]["facts"];
        assert_eq!(facts.as_array().unwrap().len(), usize::from(!ai));
        if !ai {
            assert_eq!(
                facts[0],
                json!({"accountId":human_fact["accountId"], "kind":"native:land-play", "factRevision":"native-land-play-v1","value":"1"})
            );
        }
        c.batch_execute("ROLLBACK").await?;
    }
    Ok(())
}
