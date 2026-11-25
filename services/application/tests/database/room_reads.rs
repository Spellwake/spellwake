//! Safe room projections enforce current visibility even when the helper bypasses raw grants.
use super::{TestResult, account, connection, lobbies, set_auth_claims};
use serde_json::Value;
use tokio_postgres::error::SqlState;
use uuid::Uuid;
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_reads_keep_foreign_decks_private_and_retired_slots_out_of_current_pages() -> TestResult
{
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let host: Uuid = c
        .query_one(
            "SELECT auth_subject FROM app.account WHERE id=$1",
            &[&f.host],
        )
        .await?
        .get(0);
    let (other, subject) = account(&c).await?;
    let selected: Value = c
        .query_one(
            "SELECT app.select_room_deck($1,$2,0,$3,$4)",
            &[&f.host, &f.room, &f.revision, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(selected["status"], "success");
    let retired:Uuid=c.query_one("INSERT INTO app.room_slot(room_id,sponsor_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'ai',false) RETURNING id",&[&f.room,&f.host]).await?.get(0);
    c.execute(
        "UPDATE app.room_slot SET retired_at=clock_timestamp() WHERE id=$1",
        &[&retired],
    )
    .await?;
    c.execute("INSERT INTO app.room_slot(room_id,sponsor_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'ai',false)",&[&f.room,&f.host]).await?;
    c.execute("INSERT INTO app.room_slot(room_id,seat_index,controller_kind,ready) VALUES($1,2,'human',false)",&[&f.room]).await?;
    let sql = "SELECT app.read_visible_room($1,NULL,100)";
    set_auth_claims(&c, host).await?;
    let own: Value = c.query_one(sql, &[&f.room]).await?.get(0);
    assert_eq!(own["data"]["accountId"], f.host.to_string());
    assert!(own["data"]["room"]["targetSeatCount"].is_null());
    let own_items = own["data"]["slots"]["items"].as_array().unwrap();
    assert_eq!(own_items.len(), 3);
    assert!(own_items.iter().all(|v| v["id"] != retired.to_string()));
    assert!(
        own_items
            .iter()
            .any(|v| v["accountId"] == f.host.to_string()
                && v["deckRevisionId"] == f.revision.to_string())
    );
    for v in own_items {
        assert_eq!(v.as_object().unwrap().len(), 7);
    }
    set_auth_claims(&c, subject).await?;
    for function in ["read_visible_room", "read_current_room_slots"] {
        let value: Value = c
            .query_one(&format!("SELECT app.{function}($1,NULL,100)"), &[&f.room])
            .await?
            .get(0);
        assert_eq!(value["data"]["room"], "unavailable");
    }
    c.batch_execute("RESET ROLE").await?;
    c.execute(
        "UPDATE app.room SET visibility='public' WHERE id=$1",
        &[&f.room],
    )
    .await?;
    set_auth_claims(&c, subject).await?;
    let public: Value = c.query_one(sql, &[&f.room]).await?.get(0);
    assert_eq!(public["data"]["accountId"], other.to_string());
    for v in public["data"]["slots"]["items"].as_array().unwrap() {
        assert!(v["deckRevisionId"].is_null());
    }
    let direct: Value = c
        .query_one(
            "SELECT app.read_current_room_slots($1,NULL,100)",
            &[&f.room],
        )
        .await?
        .get(0);
    assert_eq!(direct["data"], public["data"]["slots"]);
    let first: Value = c
        .query_one("SELECT app.read_visible_room($1,NULL,1)", &[&f.room])
        .await?
        .get(0);
    let cursor: Uuid = first["data"]["slots"]["nextCursor"]
        .as_str()
        .unwrap()
        .parse()?;
    let later: Value = c
        .query_one(
            "SELECT app.read_visible_room($1,$2,100)",
            &[&f.room, &cursor],
        )
        .await?
        .get(0);
    assert_eq!(later["data"]["slots"]["items"].as_array().unwrap().len(), 2);
    let page: Value = c
        .query_one("SELECT app.read_visible_rooms(NULL,100)", &[])
        .await?
        .get(0);
    assert!(
        page["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == f.room.to_string())
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.room SET state='closed' WHERE id=$1", &[&f.room])
        .await?;
    set_auth_claims(&c, subject).await?;
    assert_eq!(
        c.query_one(sql, &[&f.room]).await?.get::<_, Value>(0)["data"]["room"],
        "unavailable"
    );
    set_auth_claims(&c, host).await?;
    assert_eq!(
        c.query_one(sql, &[&f.room]).await?.get::<_, Value>(0)["status"],
        "success"
    );
    c.batch_execute("RESET ROLE; SET ROLE anon").await?;
    assert_eq!(
        c.query_one(
            "SELECT app.read_current_room_slots($1,NULL,100)",
            &[&f.room]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    Ok(())
}
