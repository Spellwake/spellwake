//! Version identity and first-use publication boundaries, not real executable/source qualification.
use super::{TestResult, connection, game_scope, wait_for_database_lock};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

async fn definition(c: &Client) -> Result<(Uuid, Uuid), tokio_postgres::Error> {
    let id = Uuid::new_v4();
    c.execute("INSERT INTO app.card_definition(id,oracle_ref,name) VALUES($1,$1::uuid::text,'Reference fixture')", &[&id]).await?;
    let revision = c.query_one("INSERT INTO app.card_definition_revision(definition_id,revision_ref,rules_text_ref) VALUES($1,'fixture','fixture') RETURNING id", &[&id]).await?.get(0);
    Ok((id, revision))
}

async fn release(c: &Client) -> Result<Uuid, tokio_postgres::Error> {
    let id = Uuid::new_v4();
    c.execute("INSERT INTO app.qualified_release(id,release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref) VALUES($1,$1::uuid::text,'fixture','fixture','fixture','fixture')", &[&id]).await?;
    Ok(id)
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn version_rows_cannot_be_rewritten_and_program_mapping_freezes_when_a_context_uses_it()
-> TestResult {
    let c = connection().await?;
    let f = game_scope::seed(&c).await?;
    let (profile, existing_release, legality): (Uuid, Uuid, Uuid) = {
        let row = c.query_one("SELECT x.profile_id,x.release_id,m.legality_revision_id FROM app.game_context x JOIN app.game g ON g.id=x.game_id JOIN app.match m ON m.id=g.match_id WHERE x.id=$1", &[&f.context]).await?;
        (row.get(0), row.get(1), row.get(2))
    };
    for (sql, id) in [
        (
            "UPDATE app.table_profile_revision SET rules_ref='changed' WHERE id=$1",
            profile,
        ),
        (
            "UPDATE app.qualified_release SET kernel_ref='changed' WHERE id=$1",
            existing_release,
        ),
        (
            "UPDATE app.legality_revision SET source_ref='changed' WHERE id=$1",
            legality,
        ),
    ] {
        assert_eq!(
            c.execute(sql, &[&id]).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    let version = release(&c).await?;
    let (_, program) = definition(&c).await?;
    c.execute("INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref) VALUES($1,$2,'fixture')", &[&version,&program]).await?;
    assert_eq!(
        c.execute(
            "UPDATE app.release_card_program SET program_ref='rewritten' WHERE release_id=$1",
            &[&version]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let game: Uuid = c.query_one("INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES($1,1,'active',0) RETURNING id", &[&f.match_id]).await?.get(0);
    c.execute("INSERT INTO app.game_context(game_id,release_id,profile_id,snapshot_ref,context_revision) VALUES($1,$2,$3,'fixture','fixture')", &[&game,&version,&profile]).await?;
    let (_, added) = definition(&c).await?;
    assert_eq!(c.execute("INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref) VALUES($1,$2,'late')", &[&version,&added]).await.unwrap_err().code(), Some(&SqlState::CHECK_VIOLATION));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn legality_corrections_publish_new_versions_instead_of_changing_an_accepted_match()
-> TestResult {
    let c = connection().await?;
    let f = game_scope::seed(&c).await?;
    let (card, _) = definition(&c).await?;
    let format: Uuid = c
        .query_one(
            "SELECT format_id FROM app.match WHERE id=$1",
            &[&f.match_id],
        )
        .await?
        .get(0);
    let version = Uuid::new_v4();
    c.execute("INSERT INTO app.legality_revision(id,source_ref,effective_at,imported_at) VALUES($1,$1::uuid::text,clock_timestamp(),clock_timestamp())", &[&version]).await?;
    c.execute("INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status) VALUES($1,$2,$3,'legal')", &[&card,&format,&version]).await?;
    let profile: Uuid = c
        .query_one(
            "SELECT profile_id FROM app.match WHERE id=$1",
            &[&f.match_id],
        )
        .await?
        .get(0);
    c.execute("INSERT INTO app.match(format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) VALUES($1,$2,$3,'open',false,'bo1','active',0)", &[&format,&profile,&version]).await?;
    assert_eq!(
        c.execute(
            "UPDATE app.card_legality SET status='banned' WHERE legality_revision_id=$1",
            &[&version]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let (other, _) = definition(&c).await?;
    assert_eq!(c.execute("INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status) VALUES($1,$2,$3,'legal')", &[&other,&format,&version]).await.unwrap_err().code(), Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(c.query_one("SELECT status FROM app.card_legality WHERE definition_id=$1 AND legality_revision_id=$2", &[&card,&version]).await?.get::<_, String>(0), "legal");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn dictionary_insertion_waits_for_an_inflight_context_then_observes_the_accepted_version()
-> TestResult {
    let c = connection().await?;
    let f = game_scope::seed(&c).await?;
    let version = release(&c).await?;
    let (_, program) = definition(&c).await?;
    let profile: Uuid = c
        .query_one(
            "SELECT profile_id FROM app.game_context WHERE id=$1",
            &[&f.context],
        )
        .await?
        .get(0);
    let game: Uuid = c.query_one("INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES($1,1,'active',0) RETURNING id", &[&f.match_id]).await?.get(0);
    c.batch_execute("BEGIN").await?;
    c.execute("INSERT INTO app.game_context(game_id,release_id,profile_id,snapshot_ref,context_revision) VALUES($1,$2,$3,'fixture','fixture')", &[&game,&version,&profile]).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let task = tokio::spawn(async move {
        waiting.execute("INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref) VALUES($1,$2,'late')", &[&version,&program]).await
    });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
