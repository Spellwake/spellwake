//! Real SQL identities and permissions; no hosted bearer or endpoint qualification.
use super::{TestResult, account, connection, lobbies, set_auth_claims};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

async fn identity(c: &Client, actor: Uuid) -> Result<(Uuid, Uuid), tokio_postgres::Error> {
    let row = c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1", &[&actor]).await?;
    Ok((row.get(0), row.get(1)))
}

async fn visible(c: &Client, room: Uuid) -> Result<i64, tokio_postgres::Error> {
    c.query_one(
        "SELECT count(*)::bigint FROM app.room WHERE id=$1",
        &[&room],
    )
    .await
    .map(|r| r.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn lobby_rls_distinguishes_private_members_public_active_rooms_and_closed_history()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let (member, member_subject) = account(&c).await?;
    let (_, outsider) = account(&c).await?;
    let (host, _) = identity(&c, f.host).await?;
    c.execute("INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'human',false)", &[&f.room,&member]).await?;
    for subject in [host, member_subject] {
        set_auth_claims(&c, subject).await?;
        assert_eq!(visible(&c, f.room).await?, 1);
        assert_eq!(
            c.query_one(
                "SELECT count(id)::bigint FROM app.room_slot WHERE room_id=$1",
                &[&f.room]
            )
            .await?
            .get::<_, i64>(0),
            2
        );
    }
    set_auth_claims(&c, outsider).await?;
    assert_eq!(visible(&c, f.room).await?, 0);
    assert_eq!(
        c.query_one(
            "SELECT count(id)::bigint FROM app.room_slot WHERE room_id=$1",
            &[&f.room]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute(
        "UPDATE app.room SET visibility='public' WHERE id=$1",
        &[&f.room],
    )
    .await?;
    set_auth_claims(&c, outsider).await?;
    assert_eq!(visible(&c, f.room).await?, 1);
    for private in ["deck_revision_id", "sponsor_id", "team_ref"] {
        let sql = format!("SELECT {private} FROM app.room_slot WHERE room_id=$1");
        assert_eq!(
            c.query(&sql, &[&f.room]).await.unwrap_err().code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
    }
    assert_eq!(
        c.execute(
            "UPDATE app.room SET host_id=$2 WHERE id=$1",
            &[&f.room, &member]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    assert_eq!(
        c.execute("DELETE FROM app.room_slot WHERE room_id=$1", &[&f.room])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.room SET state='closed' WHERE id=$1", &[&f.room])
        .await?;
    set_auth_claims(&c, outsider).await?;
    assert_eq!(visible(&c, f.room).await?, 0);
    set_auth_claims(&c, member_subject).await?;
    assert_eq!(visible(&c, f.room).await?, 1);
    c.batch_execute("RESET ROLE; SET ROLE anon").await?;
    assert_eq!(
        visible(&c, f.room).await.unwrap_err().code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn live_room_facades_bind_subject_session_and_preserve_pending_account_release() -> TestResult
{
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let (subject, session) = identity(&c, f.host).await?;
    let (_, foreign_subject) = account(&c).await?;
    let forbidden = json!({"status":"fail","data":{"session":"unauthorized"}});
    c.batch_execute("SET ROLE service_role").await?;
    let create = "SELECT app.create_room_command($1,$2,$3,$4,$5,'collection',false,'private')";
    let bad: Value = c
        .query_one(
            create,
            &[
                &foreign_subject,
                &session,
                &Uuid::new_v4(),
                &f.format,
                &f.profile,
            ],
        )
        .await?
        .get(0);
    assert_eq!(bad, forbidden);
    let request = Uuid::new_v4();
    let accepted: Value = c
        .query_one(
            create,
            &[&subject, &session, &request, &f.format, &f.profile],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    assert_eq!(
        c.query_one(
            create,
            &[&subject, &session, &request, &f.format, &f.profile]
        )
        .await?
        .get::<_, Value>(0),
        accepted
    );
    let selected: Value = c
        .query_one(
            "SELECT app.select_room_deck_command($1,$2,$3,$4,0,$5)",
            &[&subject, &session, &Uuid::new_v4(), &f.room, &f.revision],
        )
        .await?
        .get(0);
    assert_eq!(selected["status"], "success");
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.host]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            create,
            &[&subject, &session, &Uuid::new_v4(), &f.format, &f.profile]
        )
        .await?
        .get::<_, Value>(0)["data"]["account"],
        "unavailable"
    );
    assert_eq!(
        c.query_one(
            "SELECT app.select_room_deck_command($1,$2,$3,$4,1,NULL)",
            &[&subject, &session, &Uuid::new_v4(), &f.room]
        )
        .await?
        .get::<_, Value>(0)["data"]["account"],
        "unavailable"
    );
    let released: Value = c
        .query_one(
            "SELECT app.leave_room_command($1,$2,$3,$4,1)",
            &[&subject, &session, &Uuid::new_v4(), &f.room],
        )
        .await?
        .get(0);
    assert_eq!(released["status"], "success");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn expired_room_session_cannot_read_public_lobbies_or_execute_new_or_replayed_commands()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let (subject, session) = identity(&c, f.host).await?;
    let request = Uuid::new_v4();
    c.execute(
        "UPDATE app.room SET visibility='public' WHERE id=$1",
        &[&f.room],
    )
    .await?;
    c.batch_execute("SET ROLE service_role").await?;
    let sql = "SELECT app.select_room_deck_command($1,$2,$3,$4,0,$5)";
    assert_eq!(
        c.query_one(sql, &[&subject, &session, &request, &f.room, &f.revision])
            .await?
            .get::<_, Value>(0)["status"],
        "success"
    );
    c.batch_execute("RESET ROLE").await?;
    set_auth_claims(&c, subject).await?;
    assert_eq!(visible(&c, f.room).await?, 1);
    c.batch_execute("RESET ROLE").await?;
    c.execute(
        "UPDATE auth.sessions SET not_after=statement_timestamp() WHERE id=$1",
        &[&session],
    )
    .await?;
    // Retain the same request.jwt.claims; expiry must be checked in canonical SQL authority.
    c.batch_execute("SET ROLE authenticated").await?;
    assert_eq!(visible(&c, f.room).await?, 0);
    c.batch_execute("RESET ROLE; SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(sql, &[&subject, &session, &request, &f.room, &f.revision])
            .await?
            .get::<_, Value>(0),
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(
        c.query_one(
            sql,
            &[&subject, &session, &Uuid::new_v4(), &f.room, &f.revision]
        )
        .await?
        .get::<_, Value>(0),
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn only_service_facades_receive_execute_and_no_runtime_role_receives_raw_lobby_writes()
-> TestResult {
    let c = connection().await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for signature in [
            "app.create_room_command(uuid,uuid,uuid,uuid,uuid,text,boolean,text)",
            "app.select_room_deck_command(uuid,uuid,uuid,uuid,numeric,uuid)",
            "app.leave_room_command(uuid,uuid,uuid,uuid,numeric)",
        ] {
            assert_eq!(
                c.query_one(
                    "SELECT has_function_privilege($1::name,$2,'EXECUTE')",
                    &[&role, &signature]
                )
                .await?
                .get::<_, bool>(0),
                role == "service_role"
            );
        }
        for table in ["app.room", "app.room_slot"] {
            for action in ["INSERT", "UPDATE", "DELETE", "TRUNCATE"] {
                assert!(
                    !c.query_one(
                        "SELECT has_table_privilege($1::name,$2,$3)",
                        &[&role, &table, &action]
                    )
                    .await?
                    .get::<_, bool>(0)
                );
            }
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn live_lobby_facades_reject_null_and_zero_request_ids_without_creating_commands()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let (subject, session) = identity(&c, f.host).await?;
    let before: i64 = c
        .query_one(
            "SELECT count(*)::bigint FROM app.command_receipt WHERE account_id=$1",
            &[&f.host],
        )
        .await?
        .get(0);
    c.batch_execute("SET ROLE service_role").await?;
    let invalid = json!({"status":"fail","data":{"request":"invalid"}});
    for request in [None, Some(Uuid::nil())] {
        let created: Value = c
            .query_one(
                "SELECT app.create_room_command($1,$2,$3,$4,$5,'collection',false,'private')",
                &[&subject, &session, &request, &f.format, &f.profile],
            )
            .await?
            .get(0);
        assert_eq!(created, invalid);
        let selected: Value = c
            .query_one(
                "SELECT app.select_room_deck_command($1,$2,$3,$4,0,$5)",
                &[&subject, &session, &request, &f.room, &f.revision],
            )
            .await?
            .get(0);
        assert_eq!(selected, invalid);
        let left: Value = c
            .query_one(
                "SELECT app.leave_room_command($1,$2,$3,$4,0)",
                &[&subject, &session, &request, &f.room],
            )
            .await?
            .get(0);
        assert_eq!(left, invalid);
    }
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.command_receipt WHERE account_id=$1",
            &[&f.host]
        )
        .await?
        .get::<_, i64>(0),
        before
    );
    Ok(())
}
