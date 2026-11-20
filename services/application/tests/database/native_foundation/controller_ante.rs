//! Canonical private AI ante RPC/role/transaction controls over normalized SQL
//! admission/provider observations. No credential possession/native/format truth.
use super::*;
const READ: &str = "SELECT app.read_controller_ai_ante_original($1,$2::text::numeric,$3,$4)";
const SUBMIT: &str = "SELECT app.submit_controller_ai_ante($1,$2::text::numeric,$3,$4)";
async fn fixture(c: &Client) -> FixtureResult<(Native, Controller)> {
    let f = reserved_native(c, "bo1", false).await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    let actor = controller(c, &f).await?;
    Ok((f, actor))
}
fn source(f: &Native, actor: &Controller, revision: &str, coins: &str) -> Value {
    json!({"requestId":Uuid::new_v4(),"matchId":f.matched,"seatId":actor.seat,"expectedTermsRevision":revision,"coins":coins})
}
async fn rpc(c: &Client, actor: &Controller, original: &Value, read: bool) -> FixtureResult<Value> {
    c.batch_execute("SET ROLE service_role").await?;
    let value = c
        .query_one(
            if read { READ } else { SUBMIT },
            &[&actor.grant, &actor.epoch, &actor.secret, original],
        )
        .await;
    c.batch_execute("RESET ROLE").await?;
    Ok(value?.get(0))
}
async fn context(c: &Client, f: &Native, actor: &Controller) -> FixtureResult<Value> {
    c.batch_execute("SET ROLE service_role").await?;
    let value = c
        .query_one(
            "SELECT app.read_controller_ai_ante_context($1,$2::text::numeric,$3,$4)",
            &[&actor.grant, &actor.epoch, &actor.secret, &f.matched],
        )
        .await;
    c.batch_execute("RESET ROLE").await?;
    Ok(value?.get(0))
}
async fn fingerprint(c: &Client, f: &Native) -> FixtureResult<Value> {
    Ok(c.query_one("SELECT jsonb_build_object('originals',(SELECT count(*) FROM app.command_receipt WHERE scope='ante_offer_v1' AND target_ref=($1::uuid)::text),'stakes',(SELECT count(*) FROM app.ante_stake s JOIN app.ante_commitment a ON a.id=s.commitment_id WHERE a.match_id=$1::uuid),'exposures',(SELECT count(*) FROM app.ai_loss_exposure WHERE match_id=$1::uuid),'operations',(SELECT count(*) FROM app.ledger_operation l JOIN app.command_receipt r ON r.operation_id=l.id WHERE l.source_scope='ante_offer_v1' AND r.target_ref=($1::uuid)::text),'terms',(SELECT terms_revision::text FROM app.ante_commitment WHERE match_id=$1::uuid),'nativeRevision',accepted_revision::text) FROM app.game WHERE id=$2",&[&f.matched,&f.game]).await?.get(0))
}
fn unavailable(value: &Value) {
    assert_eq!(
        value,
        &json!({"status":"fail","data":{"actor":"unavailable"}})
    );
}
async fn rebind(c: &Client, actor: &Controller) -> FixtureResult<Value> {
    Ok(c.query_one(
        "SELECT app.rebind_ai_runner($1,$2::text::numeric,'fixture:ante-rebind',$3)",
        &[&actor.binding, &actor.epoch, &Uuid::new_v4()],
    )
    .await?
    .get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn private_roles_volatility_and_canonical_context_are_explicit() -> TestResult {
    let c = connection().await?;
    let (f, actor) = fixture(&c).await?;
    for name in [
        "read_controller_ai_ante_context",
        "read_controller_ai_ante_original",
        "submit_controller_ai_ante",
    ] {
        let row=c.query_one("SELECT p.provolatile::text,p.prosecdef,p.proconfig,has_function_privilege('service_role',p.oid,'EXECUTE'),has_function_privilege('anon',p.oid,'EXECUTE'),has_function_privilege('authenticated',p.oid,'EXECUTE'),has_function_privilege('supabase_auth_admin',p.oid,'EXECUTE') FROM pg_proc p WHERE p.pronamespace='app'::regnamespace AND p.proname=$1",&[&name]).await?;
        assert_eq!(row.get::<_, String>(0), "v");
        assert!(row.get::<_, bool>(1));
        assert!(
            row.get::<_, Vec<String>>(2)
                .iter()
                .any(|x| x == "search_path=\"\"")
        );
        assert!(row.get::<_, bool>(3));
        for i in 4..7 {
            assert!(!row.get::<_, bool>(i));
        }
    }
    let value = context(&c, &f, &actor).await?;
    assert_eq!(value["status"], "success");
    assert_eq!(value["data"]["actor"]["bindingId"], json!(actor.binding));
    assert_eq!(value["data"]["actor"]["accountId"], Value::Null);
    assert_eq!(value["data"]["context"]["matchId"], json!(f.matched));
    assert_eq!(value["data"]["context"]["seatId"], json!(actor.seat));
    assert_eq!(value["data"]["context"]["commitmentId"], Value::Null);
    assert_eq!(value["data"]["context"]["exposureId"], Value::Null);
    let original = source(&f, &actor, "0", "100");
    for role in ["anon", "authenticated", "supabase_auth_admin"] {
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        let e = c
            .query_one(
                SUBMIT,
                &[&actor.grant, &actor.epoch, &actor.secret, &original],
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), Some(&SqlState::INSUFFICIENT_PRIVILEGE));
        c.batch_execute("RESET ROLE").await?;
    }
    assert_eq!(
        rpc(&c, &actor, &original, true).await?["data"]["disposition"],
        json!({"state":"unknown","response":null})
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn original_change_withdraw_reoffer_and_historical_recovery_use_one_exposure() -> TestResult {
    let c = connection().await?;
    let (f, actor) = fixture(&c).await?;
    let original = source(&f, &actor, "0", "100");
    let first = rpc(&c, &actor, &original, false).await?;
    assert_eq!(first["status"], "success");
    assert_eq!(first["data"]["original"], original);
    assert_eq!(first["data"]["disposition"]["state"], "accepted");
    assert_eq!(
        first["data"]["disposition"]["response"]["data"]["termsRevision"],
        "1"
    );
    let resolved = context(&c, &f, &actor).await?;
    assert!(resolved["data"]["context"]["commitmentId"].is_string());
    assert!(resolved["data"]["context"]["exposureId"].is_string());
    for (revision, coins) in [("1", "50"), ("2", "0"), ("3", "300")] {
        let reply = rpc(&c, &actor, &source(&f, &actor, revision, coins), false).await?;
        assert_eq!(reply["data"]["disposition"]["state"], "accepted");
    }
    let before = fingerprint(&c, &f).await?;
    assert_eq!(before["originals"], 4);
    assert_eq!(before["exposures"], 1);
    assert_eq!(before["terms"], "4");
    assert_eq!(rpc(&c, &actor, &original, true).await?, first);
    assert_eq!(rpc(&c, &actor, &original, false).await?, first);
    let mut changed = original.clone();
    changed["coins"] = json!("101");
    assert_eq!(
        rpc(&c, &actor, &changed, true).await?,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    assert_eq!(
        rpc(&c, &actor, &changed, false).await?,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    assert_eq!(fingerprint(&c, &f).await?, before);
    let exposure=c.query_one("SELECT x.reserved_coins::text,b.utc_day,to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD') FROM app.ai_loss_exposure x JOIN app.ai_loss_bucket b ON b.id=x.bucket_id JOIN app.match m ON m.id=x.match_id WHERE x.match_id=$1",&[&f.matched]).await?;
    assert_eq!(exposure.get::<_, String>(0), "300");
    assert_eq!(exposure.get::<_, String>(1), exposure.get::<_, String>(2));
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn foreign_grant_seat_epoch_and_invalid_original_never_write() -> TestResult {
    let c = connection().await?;
    let (f, actor) = fixture(&c).await?;
    let (_, foreign) = fixture(&c).await?;
    let original = source(&f, &actor, "0", "100");
    let before = fingerprint(&c, &f).await?;
    unavailable(&rpc(&c, &foreign, &original, false).await?);
    let wrong = Controller {
        binding: actor.binding,
        grant: actor.grant,
        epoch: "999".to_owned(),
        secret: actor.secret.clone(),
        seat: actor.seat,
    };
    unavailable(&rpc(&c, &wrong, &original, false).await?);
    let mut human = original.clone();
    human["seatId"] = json!(f.seat);
    unavailable(&rpc(&c, &actor, &human, false).await?);
    for bad in [
        json!(null),
        json!([]),
        {
            let mut v = original.clone();
            v["coins"] = json!("301");
            v
        },
        {
            let mut v = original.clone();
            v["sponsorId"] = json!(Uuid::new_v4());
            v
        },
        {
            let mut v = original.clone();
            v["expectedTermsRevision"] = json!("00");
            v
        },
        {
            let mut v = original.clone();
            v["requestId"] = json!(Uuid::nil());
            v
        },
    ] {
        assert_eq!(
            rpc(&c, &actor, &bad, false).await?,
            json!({"status":"fail","data":{"request":"invalid"}})
        );
    }
    assert_eq!(fingerprint(&c, &f).await?, before);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn rebind_and_stop_winning_account_wait_roll_back_new_original_and_all_funding() -> TestResult
{
    for stop in [false, true] {
        let c = connection().await?;
        let (f, actor) = fixture(&c).await?;
        let original = source(&f, &actor, "0", "100");
        let before = fingerprint(&c, &f).await?;
        c.batch_execute("BEGIN").await?;
        if stop {
            let _:Uuid=c.query_one("SELECT app.request_ai_stop($1,(SELECT bootstrap_receipt_id FROM app.ai_runner_binding WHERE id=$1))",&[&actor.binding]).await?.get(0);
        } else {
            assert_eq!(rebind(&c, &actor).await?["status"], "success");
        }
        let (waiting, pid) = worker().await?;
        let grant = actor.grant;
        let epoch = actor.epoch.clone();
        let reference = actor.secret.clone();
        let task = tokio::spawn(async move {
            waiting
                .query_one(SUBMIT, &[&grant, &epoch, &reference, &original])
                .await
                .map(|r| r.get::<_, Value>(0))
        });
        crate::wait_for_database_lock(&c, pid).await?;
        c.batch_execute("COMMIT").await?;
        unavailable(&task.await??);
        assert_eq!(fingerprint(&c, &f).await?, before);
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn committed_writer_precedes_rebind_and_funds_exactly_once() -> TestResult {
    for stop in [false, true] {
        let c = connection().await?;
        let (f, actor) = fixture(&c).await?;
        let original = source(&f, &actor, "0", "100");
        c.batch_execute("BEGIN").await?;
        let accepted = rpc(&c, &actor, &original, false).await?;
        assert_eq!(accepted["data"]["disposition"]["state"], "accepted");
        let waiting = connection().await?;
        let pid = waiting
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        let binding = actor.binding;
        let epoch = actor.epoch.clone();
        let task = tokio::spawn(async move {
            if stop {
                waiting.query_one("SELECT app.request_ai_stop($1,(SELECT bootstrap_receipt_id FROM app.ai_runner_binding WHERE id=$1))",&[&binding]).await.map(|r|json!(r.get::<_,Uuid>(0)))
            } else {
                waiting.query_one("SELECT app.rebind_ai_runner($1,$2::text::numeric,'fixture:ante-after-write',$3)",&[&binding,&epoch,&Uuid::new_v4()]).await.map(|r|r.get::<_,Value>(0))
            }
        });
        crate::wait_for_database_lock(&c, pid).await?;
        c.batch_execute("COMMIT").await?;
        let stopped = task.await??;
        if stop {
            assert!(stopped.is_string());
        } else {
            assert_eq!(stopped["status"], "success");
        }
        let after = fingerprint(&c, &f).await?;
        assert_eq!(after["originals"], 1);
        assert_eq!(after["exposures"], 1);
        assert_eq!(after["operations"], 1);
        assert_eq!(after["terms"], "1");
        unavailable(&rpc(&c, &actor, &original, true).await?);
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn changed_source_after_receipt_wait_returns_canonical_conflict_without_second_write()
-> TestResult {
    let c = connection().await?;
    let (f, actor) = fixture(&c).await?;
    let original = source(&f, &actor, "0", "100");
    c.batch_execute("BEGIN").await?;
    let accepted = rpc(&c, &actor, &original, false).await?;
    assert_eq!(accepted["data"]["disposition"]["state"], "accepted");
    let before = fingerprint(&c, &f).await?;
    let mut changed = original.clone();
    changed["coins"] = json!("101");
    let (waiting, pid) = worker().await?;
    let grant = actor.grant;
    let epoch = actor.epoch.clone();
    let reference = actor.secret.clone();
    let task = tokio::spawn(async move {
        waiting
            .query_one(SUBMIT, &[&grant, &epoch, &reference, &changed])
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    crate::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await??,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    assert_eq!(fingerprint(&c, &f).await?, before);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deletion_continuation_preserves_old_original_and_persists_only_fresh_window_rejection()
-> TestResult {
    let c = connection().await?;
    let (f, actor) = fixture(&c).await?;
    let original = source(&f, &actor, "0", "100");
    let accepted = rpc(&c, &actor, &original, false).await?;
    assert_eq!(accepted["data"]["disposition"]["state"], "accepted");
    let deleted: Value = c
        .query_one(
            "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
            &[&f.subject, &f.session, &Uuid::new_v4(), &"1".repeat(64)],
        )
        .await?
        .get(0);
    assert_eq!(deleted["status"], "success");
    assert_eq!(rpc(&c, &actor, &original, true).await?, accepted);
    let before = fingerprint(&c, &f).await?;
    let rejected = rpc(&c, &actor, &source(&f, &actor, "1", "50"), false).await?;
    assert_eq!(
        rejected["data"]["disposition"],
        json!({"state":"rejected","response":{"status":"fail","data":{"match":"unavailable initial ante window"}}})
    );
    let mut expected = before;
    expected["originals"] = json!(2);
    assert_eq!(fingerprint(&c, &f).await?, expected);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn opening_first_keeps_actor_live_but_stores_one_economic_rejection() -> TestResult {
    let c = connection().await?;
    let (f, actor) = fixture(&c).await?;
    let opened = accept_opening_fixture(&c, &f, &f.opening).await?;
    assert_eq!(opened["status"], "success");
    let original = source(&f, &actor, "0", "100");
    let before = fingerprint(&c, &f).await?;
    let rejected = rpc(&c, &actor, &original, false).await?;
    assert_eq!(
        rejected["data"]["disposition"],
        json!({"state":"rejected","response":{"status":"fail","data":{"match":"unavailable initial ante window"}}})
    );
    assert_eq!(rpc(&c, &actor, &original, true).await?, rejected);
    assert_eq!(rpc(&c, &actor, &original, false).await?, rejected);
    let mut expected = before;
    expected["originals"] = json!(1);
    assert_eq!(fingerprint(&c, &f).await?, expected);
    Ok(())
}

type SubmitTask = tokio::task::JoinHandle<Result<Value, tokio_postgres::Error>>;
async fn pending_submit(actor: &Controller, original: Value) -> FixtureResult<(SubmitTask, i32)> {
    let (waiting, pid) = worker().await?;
    let grant = actor.grant;
    let epoch = actor.epoch.clone();
    let reference = actor.secret.clone();
    Ok((
        tokio::spawn(async move {
            waiting
                .query_one(SUBMIT, &[&grant, &epoch, &reference, &original])
                .await
                .map(|r| r.get::<_, Value>(0))
        }),
        pid,
    ))
}
fn window_rejection(value: &Value) {
    assert_eq!(value["status"], "success");
    assert_eq!(
        value["data"]["disposition"],
        json!({"state":"rejected","response":{"status":"fail","data":{"match":"unavailable initial ante window"}}})
    );
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deletion_and_writer_account_waits_preserve_canonical_rejection_or_original() -> TestResult
{
    for deletion_first in [true, false] {
        let c = connection().await?;
        let (f, actor) = fixture(&c).await?;
        let original = source(&f, &actor, "0", "100");
        if deletion_first {
            c.batch_execute("BEGIN").await?;
            let deleted: Value = c
                .query_one(
                    "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
                    &[&f.subject, &f.session, &Uuid::new_v4(), &"2".repeat(64)],
                )
                .await?
                .get(0);
            assert_eq!(deleted["status"], "success");
            let mut expected = fingerprint(&c, &f).await?;
            let (task, pid) = pending_submit(&actor, original.clone()).await?;
            crate::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            let rejected = task.await??;
            window_rejection(&rejected);
            assert_eq!(rpc(&c, &actor, &original, true).await?, rejected);
            expected["originals"] = json!(1);
            assert_eq!(fingerprint(&c, &f).await?, expected);
        } else {
            c.batch_execute("BEGIN").await?;
            let accepted = rpc(&c, &actor, &original, false).await?;
            assert_eq!(accepted["data"]["disposition"]["state"], "accepted");
            let expected = fingerprint(&c, &f).await?;
            let waiting = connection().await?;
            let pid = waiting
                .query_one("SELECT pg_backend_pid()", &[])
                .await?
                .get(0);
            let subject = f.subject;
            let session = f.session;
            let task = tokio::spawn(async move {
                waiting
                    .query_one(
                        "SELECT app.request_account_deletion_command($1,$2,$3,$4)",
                        &[&subject, &session, &Uuid::new_v4(), &"3".repeat(64)],
                    )
                    .await
                    .map(|r| r.get::<_, Value>(0))
            });
            crate::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "success");
            assert_eq!(rpc(&c, &actor, &original, true).await?, accepted);
            assert_eq!(fingerprint(&c, &f).await?, expected);
        }
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn opening_and_writer_match_waits_preserve_ante_gate_in_both_orders() -> TestResult {
    for opening_first in [true, false] {
        let c = connection().await?;
        let (f, actor) = fixture(&c).await?;
        let original = source(&f, &actor, "0", "100");
        if opening_first {
            c.batch_execute("BEGIN").await?;
            assert_eq!(
                accept_opening_fixture(&c, &f, &f.opening).await?["status"],
                "success"
            );
            let mut expected = fingerprint(&c, &f).await?;
            let (task, pid) = pending_submit(&actor, original.clone()).await?;
            crate::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            let rejected = task.await??;
            window_rejection(&rejected);
            assert_eq!(rpc(&c, &actor, &original, true).await?, rejected);
            expected["originals"] = json!(1);
            assert_eq!(fingerprint(&c, &f).await?, expected);
        } else {
            c.batch_execute("BEGIN").await?;
            let accepted = rpc(&c, &actor, &original, false).await?;
            assert_eq!(accepted["data"]["disposition"]["state"], "accepted");
            let expected = fingerprint(&c, &f).await?;
            let (waiting, pid) = worker().await?;
            let matched = f.matched;
            let basis = f.original_basis.clone();
            let game = f.game;
            let opening = f.opening.clone();
            let task = tokio::spawn(async move {
                waiting
                    .query_one(
                        "SELECT app.accept_native_opening($1,$2::text::numeric,$3,$4,$5,'0',$6,$7)",
                        &[
                            &matched,
                            &basis,
                            &game,
                            &OWNER,
                            &opening,
                            &"1".repeat(64),
                            &"2".repeat(64),
                        ],
                    )
                    .await
                    .map(|r| r.get::<_, Value>(0))
            });
            crate::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            let error = task.await?.unwrap_err();
            assert_eq!(error.code(), Some(&SqlState::T_R_SERIALIZATION_FAILURE));
            assert_eq!(
                error.as_db_error().unwrap().message(),
                "ante source is not committed; opening remains reserved"
            );
            assert_eq!(fingerprint(&c, &f).await?, expected);
        }
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn terminal_match_fence_rolls_back_ante_rejection_when_authority_ends() -> TestResult {
    let c = connection().await?;
    let (f, actor) = fixture(&c).await?;
    assert_eq!(
        accept_opening_fixture(&c, &f, &f.opening).await?["status"],
        "success"
    );
    let original = source(&f, &actor, "0", "100");
    c.batch_execute("BEGIN").await?;
    finish_opening_game_as_draw(&c, &f).await?;
    let expected = fingerprint(&c, &f).await?;
    let (task, pid) = pending_submit(&actor, original).await?;
    crate::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    unavailable(&task.await??);
    assert_eq!(fingerprint(&c, &f).await?, expected);
    Ok(())
}
async fn three_participant_fixture(c: &Client) -> FixtureResult<(Native, Controller, Uuid)> {
    let room = rooms::room_with_capacity(c, true, false, 3).await?;
    let accounts = c
        .query(
            "SELECT id FROM app.account WHERE id IN($1,$2) ORDER BY id",
            &[&room.source.actor, &room.other],
        )
        .await?;
    let lower: Uuid = accounts[0].get(0);
    let sponsor: Uuid = accounts[1].get(0);
    let revision = if sponsor == room.source.actor {
        room.source.revision
    } else {
        room.other_revision
    };
    let basis = rooms::basis(c, room.id).await?;
    let added: Value = c
        .query_one(
            "SELECT app.add_room_ai_slot($1,$2,$3::text::numeric,$4)",
            &[&sponsor, &room.id, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(added["status"], "success");
    let slot: Uuid = added["data"]["resourceId"].as_str().unwrap().parse()?;
    let grant = rooms::bootstrap_ai(c, slot, sponsor, revision).await?;
    let ready:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:ante-three-deck','fixture:ante-three-authentication',(SELECT configuration_id FROM app.room WHERE id=$3),$4)",&[&grant,&revision,&room.id,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(ready["status"], "success");
    let basis = rooms::basis(c, room.id).await?;
    let countdown:Value=c.query_one("SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:ante-three-source',$4)",&[&room.source.actor,&room.id,&basis,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(countdown["status"], "success");
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&room.id],
    )
    .await?;
    let started = rooms::start(c, &room, Uuid::new_v4()).await?;
    assert_eq!(started["status"], "success");
    let matched = started["data"]["resourceId"].as_str().unwrap().parse()?;
    let f = reserved_native_match(c, "bo1", &room, matched).await?;
    let actor = controller(c, &f).await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING",&[]).await?;
    Ok((f, actor, lower))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn lower_sorted_other_participant_wait_does_not_lock_controller_sponsor_first() -> TestResult
{
    let c = connection().await?;
    let (f, actor, lower) = three_participant_fixture(&c).await?;
    let sponsor: Uuid = c
        .query_one(
            "SELECT sponsor_id FROM app.match_seat WHERE id=$1",
            &[&actor.seat],
        )
        .await?
        .get(0);
    assert_ne!(sponsor, lower);
    let original = source(&f, &actor, "0", "100");
    let expected = fingerprint(&c, &f).await?;
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
        &[&lower],
    )
    .await?;
    let (task, pid) = pending_submit(&actor, original).await?;
    crate::wait_for_database_lock(&c, pid).await?;
    // Actual canonical rebind can take the higher sponsor while the writer waits
    // on the lower participant. A sponsor-first wrapper would block or deadlock.
    let rebinder = connection().await?;
    rebinder.batch_execute("SET lock_timeout='1s'").await?;
    assert_eq!(rebind(&rebinder, &actor).await?["status"], "success");
    c.batch_execute("COMMIT").await?;
    unavailable(&task.await??);
    assert_eq!(fingerprint(&c, &f).await?, expected);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn known_pending_and_same_original_wait_never_repeat_the_economic_write() -> TestResult {
    let c = connection().await?;
    let (f, actor) = fixture(&c).await?;
    let original = source(&f, &actor, "0", "100");
    c.batch_execute("BEGIN").await?;
    let accepted = rpc(&c, &actor, &original, false).await?;
    let expected = fingerprint(&c, &f).await?;
    let (task, pid) = pending_submit(&actor, original.clone()).await?;
    crate::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??, accepted);
    assert_eq!(fingerprint(&c, &f).await?, expected);
    let pending = source(&f, &actor, "1", "50");
    c.execute("INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref) VALUES('ante-seat:'||$1::uuid::text,'ante_offer_v1',$2::uuid::text,$3,app.auth_json_hash(jsonb_build_object('match',$2::uuid,'seat',$1::uuid,'basis','1','assets','[]'::jsonb,'aiCoins','50')),'pending','')",&[&actor.seat,&f.matched,&pending["requestId"].as_str().unwrap()]).await?;
    let before = fingerprint(&c, &f).await?;
    let observed = rpc(&c, &actor, &pending, true).await?;
    assert_eq!(
        observed["data"]["disposition"],
        json!({"state":"pending","response":null})
    );
    assert_eq!(rpc(&c, &actor, &pending, false).await?, observed);
    assert_eq!(fingerprint(&c, &f).await?, before);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn replacement_current_generation_recovers_old_ante_without_current_terms_repreparation()
-> TestResult {
    let c = connection().await?;
    let (f, actor) = fixture(&c).await?;
    let original = source(&f, &actor, "0", "100");
    let accepted = rpc(&c, &actor, &original, false).await?;
    assert_eq!(accepted["data"]["disposition"]["state"], "accepted");
    assert_eq!(
        rpc(&c, &actor, &source(&f, &actor, "1", "50"), false).await?["data"]["disposition"]["state"],
        "accepted"
    );
    let expected = fingerprint(&c, &f).await?;
    let rebound = rebind(&c, &actor).await?;
    assert_eq!(rebound["status"], "success");
    let start: Uuid = rebound["data"]["startKey"].as_str().unwrap().parse()?;
    let stop: Uuid = rebound["data"]["previousStopId"]
        .as_str()
        .unwrap()
        .parse()?;
    let epoch = rebound["data"]["epoch"].as_str().unwrap().to_owned();
    let stopped:Value=c.query_one("SELECT app.observe_ai_stop($1,$2::text::numeric,$3,'fixture:observed-ante-process-absence',$4)",&[&actor.binding,&actor.epoch,&stop,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(stopped["status"], "success");
    let provider = format!("fixture:ante-replacement:{}:{epoch}", actor.binding);
    let running:Value=c.query_one("SELECT app.observe_ai_provider($1,$2::text::numeric,$3,'running',$4,'fixture:ante-provider-observation',$5)",&[&actor.binding,&epoch,&start,&provider,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(running["status"], "success");
    let reference = format!("fixture:ante-secret-generation:{}", Uuid::new_v4());
    let issued: Value = c
        .query_one(
            "SELECT app.issue_ai_grant($1,$2::text::numeric,$3,$4)",
            &[&actor.binding, &epoch, &reference, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(issued["status"], "success");
    let grant: Uuid = issued["data"]["resourceId"].as_str().unwrap().parse()?;
    let delivered:Value=c.query_one("SELECT app.observe_ai_grant_delivery($1,0,'delivered','fixture:ante-delivery-observation',$2)",&[&grant,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(delivered["status"], "success");
    let meta=c.query_one("SELECT slot.deck_revision_id,room.configuration_id FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id JOIN app.room room ON room.id=slot.room_id WHERE b.id=$1",&[&actor.binding]).await?;
    let revision: Uuid = meta.get(0);
    let configuration: Uuid = meta.get(1);
    let ready:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,$2::text::numeric,$3,'fixture:ante-deck-observation','fixture:ante-authentication-observation',$4,$5)",&[&grant,&epoch,&revision,&configuration,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(ready["status"], "success");
    let replacement = controller(&c, &f).await?;
    assert_ne!(replacement.grant, actor.grant);
    assert_eq!(replacement.binding, actor.binding);
    assert_eq!(replacement.seat, actor.seat);
    assert_eq!(replacement.epoch, epoch);
    unavailable(&rpc(&c, &actor, &original, true).await?);
    let recovered = rpc(&c, &replacement, &original, true).await?;
    assert_eq!(recovered["status"], "success");
    assert_eq!(recovered["data"]["original"], original);
    assert_eq!(
        recovered["data"]["disposition"],
        accepted["data"]["disposition"]
    );
    assert_eq!(rpc(&c, &replacement, &original, false).await?, recovered);
    assert_eq!(fingerprint(&c, &f).await?, expected);
    Ok(())
}
