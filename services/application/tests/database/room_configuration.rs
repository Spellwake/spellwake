//! Session-bound host configuration tests qualify SQL behavior, not native admission proofs.
use super::{TestResult, connection, room_start_integrity, set_auth_claims};
use serde_json::Value;
use uuid::Uuid;
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_configuration_facade_binds_host_and_recovers_exact_numeric_and_original_team_terms()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, true).await?;
    let row=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1",&[&f.source.actor]).await?;
    let subject: Uuid = row.get(0);
    let session: Uuid = row.get(1);
    let expected = room_start_integrity::basis(&c, f.id).await?;
    let teams:Value=c.query_one("SELECT jsonb_agg(jsonb_build_object('slotId',id,'teamRef',NULL) ORDER BY id) FROM app.room_slot WHERE room_id=$1 AND retired_at IS NULL",&[&f.id]).await?.get(0);
    let config_before: Uuid = c
        .query_one(
            "SELECT configuration_id FROM app.room WHERE id=$1",
            &[&f.id],
        )
        .await?
        .get(0);
    let request = Uuid::new_v4();
    let exec = "SELECT app.configure_room_command($1,$2,$3,$4,$5::text::numeric,$6,'bo3',$7)";
    let read =
        "SELECT app.read_room_configuration_command($1,$2,$3,$4,$5::text::numeric,$6,'bo3',$7)";
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            read,
            &[
                &subject,
                &session,
                &request,
                &f.id,
                &expected,
                &f.source.profile,
                &teams
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["state"],
        "unknown"
    );
    let accepted: Value = c
        .query_one(
            exec,
            &[
                &subject,
                &session,
                &request,
                &f.id,
                &expected,
                &f.source.profile,
                &teams,
            ],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    assert_eq!(
        accepted["data"]["revision"],
        (expected.parse::<u64>()? + 1).to_string()
    );
    assert_eq!(
        c.query_one(
            read,
            &[
                &subject,
                &session,
                &request,
                &f.id,
                &expected,
                &f.source.profile,
                &teams
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    let mut reordered = teams.clone();
    reordered.as_array_mut().unwrap().reverse();
    assert_eq!(
        c.query_one(
            read,
            &[
                &subject,
                &session,
                &request,
                &f.id,
                &expected,
                &f.source.profile,
                &reordered
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["request"],
        "conflict"
    );
    let basis = accepted["data"]["revision"].as_str().unwrap();
    let noop: Value = c
        .query_one(
            exec,
            &[
                &subject,
                &session,
                &Uuid::new_v4(),
                &f.id,
                &basis,
                &f.source.profile,
                &teams,
            ],
        )
        .await?
        .get(0);
    assert_eq!(noop["data"]["revision"], basis);
    assert_eq!(
        c.query_one(
            read,
            &[
                &subject,
                &Uuid::new_v4(),
                &request,
                &f.id,
                &expected,
                &f.source.profile,
                &teams
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["session"],
        "unauthorized"
    );
    c.batch_execute("RESET ROLE").await?;
    assert_ne!(
        c.query_one(
            "SELECT configuration_id FROM app.room WHERE id=$1",
            &[&f.id]
        )
        .await?
        .get::<_, Uuid>(0),
        config_before
    );
    let peer: Uuid = c
        .query_one(
            "SELECT auth_subject FROM app.account WHERE id=$1",
            &[&f.other],
        )
        .await?
        .get(0);
    set_auth_claims(&c, peer).await?;
    assert_eq!(
        c.query_one("SELECT app.read_host_room_configuration($1)", &[&f.id])
            .await?
            .get::<_, Value>(0)["data"]["room"],
        "unavailable"
    );
    set_auth_claims(&c, subject).await?;
    let visible: Value = c
        .query_one("SELECT app.read_host_room_configuration($1)", &[&f.id])
        .await?
        .get(0);
    assert_eq!(visible["data"]["teams"], teams);
    assert_eq!(visible["data"].as_object().unwrap().len(), 6);
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.room SET state='closed' WHERE id=$1", &[&f.id])
        .await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&f.source.actor]).await?;
    set_auth_claims(&c, subject).await?;
    assert_eq!(
        c.query_one("SELECT app.read_host_room_configuration($1)", &[&f.id])
            .await?
            .get::<_, Value>(0)["status"],
        "success"
    );
    c.batch_execute("RESET ROLE; SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            read,
            &[
                &subject,
                &session,
                &request,
                &f.id,
                &expected,
                &f.source.profile,
                &teams
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn host_configuration_does_not_truncate_oversized_reference_only_drafts() -> TestResult {
    let c = connection().await?;
    let f = super::lobbies::lobby(&c).await?;
    let subject: Uuid = c
        .query_one(
            "SELECT auth_subject FROM app.account WHERE id=$1",
            &[&f.host],
        )
        .await?
        .get(0);
    for index in 1..=6_i64 {
        c.execute("INSERT INTO app.room_slot(room_id,seat_index,controller_kind,ready) VALUES($1,$2,'human',false)",&[&f.room,&index]).await?;
    }
    set_auth_claims(&c, subject).await?;
    let value: Value = c
        .query_one("SELECT app.read_host_room_configuration($1)", &[&f.room])
        .await?
        .get(0);
    assert_eq!(
        value,
        serde_json::json!({"status":"fail","data":{"configuration":"unavailable"}})
    );
    c.batch_execute("RESET ROLE; SET ROLE anon").await?;
    assert_eq!(
        c.query_one("SELECT app.read_host_room_configuration($1)", &[&f.room])
            .await
            .unwrap_err()
            .code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
    );
    Ok(())
}
