//! Actual account-lock waits independently exercise time freshness and Auth-row freshness.
use super::{TestResult, account, connection, lobbies, wait_for_database_lock};
use serde_json::{Value, json};
use uuid::Uuid;
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn live_auth_after_wait_observes_natural_expiry_and_revoked_amr_without_join_effects()
-> TestResult {
    for natural_expiry in [true, false] {
        let c = connection().await?;
        let f = lobbies::lobby(&c).await?;
        let (guest, _) = account(&c).await?;
        let row=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1",&[&guest]).await?;
        let subject: Uuid = row.get(0);
        let session: Uuid = row.get(1);
        let profile:Uuid=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref) VALUES($1,'fixture','fixture',2,2,'fixture:sql-only') RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
        c.execute(
            "UPDATE app.room SET visibility='public',profile_id=$1 WHERE id=$2",
            &[&profile, &f.room],
        )
        .await?;
        if natural_expiry {
            c.execute("UPDATE auth.sessions SET not_after=clock_timestamp()+interval '2 seconds' WHERE id=$1",&[&session]).await?;
        }
        c.batch_execute("BEGIN").await?;
        c.query_one(
            "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
            &[&f.host],
        )
        .await?;
        let waiting = connection().await?;
        let pid: i32 = waiting
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        waiting.batch_execute("SET ROLE service_role").await?;
        let room = f.room;
        let request = Uuid::new_v4();
        let task = tokio::spawn(async move {
            waiting
                .query_one(
                    "SELECT app.join_room_command($1,$2,$3,$4,NULL)",
                    &[&subject, &session, &request, &room],
                )
                .await
                .map(|r| r.get::<_, Value>(0))
        });
        wait_for_database_lock(&c, pid).await?;
        if natural_expiry {
            assert!(c.query_one("SELECT a.query_start<s.not_after FROM pg_stat_activity a CROSS JOIN auth.sessions s WHERE a.pid=$1 AND s.id=$2",&[&pid,&session]).await?.get::<_,bool>(0),"natural expiry must occur after the waiting statement began");
            c.query_one("SELECT pg_sleep(greatest(0,extract(epoch FROM(not_after-clock_timestamp())))::double precision+0.025) FROM auth.sessions WHERE id=$1",&[&session]).await?;
        } else {
            assert_eq!(
                c.execute(
                    "DELETE FROM auth.mfa_amr_claims WHERE session_id=$1",
                    &[&session]
                )
                .await?,
                1
            );
        }
        c.batch_execute("COMMIT").await?;
        assert_eq!(
            task.await??,
            json!({"status":"fail","data":{"session":"unauthorized"}})
        );
        assert_eq!(
            c.query_one(
                "SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
                &[&f.room, &guest]
            )
            .await?
            .get::<_, i64>(0),
            0
        );
        assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE principal_key=$1 AND scope='room_join' AND request_ref=$2",&[&format!("human:{guest}"),&request.to_string()]).await?.get::<_,i64>(0),0);
    }
    Ok(())
}
