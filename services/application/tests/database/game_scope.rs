//! Privileged relational fixtures, not native gameplay, admission or host qualification.
use super::{TestResult, account, connection};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

pub(super) struct Seed {
    pub(super) account: Uuid,
    pub(super) match_id: Uuid,
    pub(super) seat: Uuid,
    pub(super) game: Uuid,
    pub(super) context: Uuid,
    pub(super) event: Uuid,
}

pub(super) async fn seed(c: &Client) -> Result<Seed, Box<dyn std::error::Error + Send + Sync>> {
    let (owner, _) = account(c).await?;
    let mut ids: Vec<_> = (0..10).map(|_| Uuid::new_v4()).collect();
    ids[0] = owner;
    let statements = [
        "INSERT INTO app.format(id,key,name) VALUES(($1::uuid[])[10],($1::uuid[])[10]::text,'Relational fixture')",
        "INSERT INTO app.table_profile_revision(id,key,revision_ref,rules_ref) VALUES(($1::uuid[])[7],($1::uuid[])[7]::text,'fixture','fixture')",
        "INSERT INTO app.legality_revision(id,source_ref,effective_at,imported_at) VALUES(($1::uuid[])[8],($1::uuid[])[8]::text,clock_timestamp(),clock_timestamp())",
        "INSERT INTO app.qualified_release(id,release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref) VALUES(($1::uuid[])[9],($1::uuid[])[9]::text,'fixture','fixture','fixture','fixture')",
        "INSERT INTO app.match(id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) VALUES(($1::uuid[])[2],($1::uuid[])[10],($1::uuid[])[7],($1::uuid[])[8],'open',false,'bo1','active',0)",
        "INSERT INTO app.match_seat(id,match_id,account_id,seat_index,controller_kind) VALUES(($1::uuid[])[3],($1::uuid[])[2],($1::uuid[])[1],0,'human')",
        "INSERT INTO app.game(id,match_id,game_index,state,accepted_revision) VALUES(($1::uuid[])[4],($1::uuid[])[2],0,'active',0)",
        "INSERT INTO app.game_context(id,game_id,release_id,profile_id,snapshot_ref,context_revision) VALUES(($1::uuid[])[5],($1::uuid[])[4],($1::uuid[])[9],($1::uuid[])[7],'fixture','fixture')",
        "INSERT INTO app.game_event(id,game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES(($1::uuid[])[6],($1::uuid[])[4],($1::uuid[])[5],0,0,'fixture')",
    ];
    for sql in statements {
        c.execute(sql, &[&ids]).await?;
    }
    Ok(Seed {
        account: owner,
        match_id: ids[1],
        seat: ids[2],
        game: ids[3],
        context: ids[4],
        event: ids[5],
    })
}

struct Link {
    table: &'static str,
    reference_column: &'static str,
    insert: &'static str,
    match_scope: bool,
    reference: fn(&Seed) -> Uuid,
}

// Only internal SQL identifiers occur here; no application input is interpolated.
const LINKS: &[Link] = &[
    Link {
        table: "between_game_decision",
        reference_column: "seat_id",
        match_scope: true,
        reference: |s| s.seat,
        insert: "INSERT INTO app.between_game_decision(id,match_id,seat_id,decision_kind,basis_revision,clock_revision,state) VALUES($1,$2,$3,'ready',0,0,'pending')",
    },
    Link {
        table: "game_checkpoint",
        reference_column: "context_id",
        match_scope: false,
        reference: |s| s.context,
        insert: "INSERT INTO app.game_checkpoint(id,game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision) VALUES($1,$2,$3,1,'fixture','fixture',0)",
    },
    Link {
        table: "game_event",
        reference_column: "context_id",
        match_scope: false,
        reference: |s| s.context,
        insert: "INSERT INTO app.game_event(id,game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES($1,$2,$3,1,1,'fixture')",
    },
    Link {
        table: "game_result",
        reference_column: "source_event_id",
        match_scope: false,
        reference: |s| s.event,
        insert: "INSERT INTO app.game_result(id,game_id,source_event_id,outcome,winners_ref,end_reason) VALUES($1,$2,$3,'draw','fixture','fixture')",
    },
    Link {
        table: "durable_handoff",
        reference_column: "game_id",
        match_scope: true,
        reference: |s| s.game,
        insert: "INSERT INTO app.durable_handoff(id,match_id,game_id,source_ref,destination,kind,state) VALUES($1,$2,$3,$1::uuid::text,'fixture','fixture','pending')",
    },
    Link {
        table: "replay_manifest",
        reference_column: "context_id",
        match_scope: false,
        reference: |s| s.context,
        insert: "INSERT INTO app.replay_manifest(id,game_id,context_id,artifact_ref,visibility_index_ref,schema_revision) VALUES($1,$2,$3,'fixture','fixture','fixture')",
    },
    Link {
        table: "game_activity_fact",
        reference_column: "source_event_id",
        match_scope: false,
        reference: |s| s.event,
        insert: "INSERT INTO app.game_activity_fact(id,game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,$4,'fixture',1,'fixture')",
    },
];

impl Link {
    async fn insert(
        &self,
        c: &Client,
        id: Uuid,
        scope: &Seed,
        parent: &Seed,
    ) -> Result<u64, tokio_postgres::Error> {
        let scope_id = if self.match_scope {
            scope.match_id
        } else {
            scope.game
        };
        let reference = (self.reference)(parent);
        let parameters: &[&(dyn tokio_postgres::types::ToSql + Sync)] =
            &[&id, &scope_id, &reference, &scope.account];
        let count = if self.table == "game_activity_fact" {
            4
        } else {
            3
        };
        c.execute(self.insert, &parameters[..count]).await
    }
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn every_game_scope_link_accepts_its_parent_and_rejects_cross_scope_insert_and_update()
-> TestResult {
    let c = connection().await?;
    let other = seed(&c).await?;
    for link in LINKS {
        let local = seed(&c).await?;
        let id = Uuid::new_v4();
        link.insert(&c, id, &local, &local).await?;
        let error = link
            .insert(&c, Uuid::new_v4(), &other, &local)
            .await
            .unwrap_err();
        assert_eq!(
            error.code(),
            Some(&SqlState::FOREIGN_KEY_VIOLATION),
            "{}",
            link.table
        );
        let sql = format!(
            "UPDATE app.{} SET {}=$2 WHERE id=$1",
            link.table, link.reference_column
        );
        let error = c
            .execute(&sql, &[&id, &(link.reference)(&other)])
            .await
            .unwrap_err();
        // Accepted histories reject rewrites before FK evaluation.
        let expected = if matches!(
            link.table,
            "game_activity_fact"
                | "game_checkpoint"
                | "game_event"
                | "game_result"
                | "replay_manifest"
                | "durable_handoff"
        ) {
            &SqlState::CHECK_VIOLATION
        } else {
            &SqlState::FOREIGN_KEY_VIOLATION
        };
        assert_eq!(error.code(), Some(expected), "{}", link.table);
        let sql = format!(
            "SELECT {} FROM app.{} WHERE id=$1",
            link.reference_column, link.table
        );
        assert_eq!(
            c.query_one(&sql, &[&id]).await?.get::<_, Uuid>(0),
            (link.reference)(&local)
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn all_game_scope_links_reject_deferred_mismatches_at_check_and_commit() -> TestResult {
    let c = connection().await?;
    let local = seed(&c).await?;
    let other = seed(&c).await?;
    for link in LINKS {
        for final_check in ["SET CONSTRAINTS ALL IMMEDIATE", "COMMIT"] {
            let id = Uuid::new_v4();
            c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
                .await?;
            link.insert(&c, id, &other, &local).await?;
            let error = c.batch_execute(final_check).await.unwrap_err();
            assert_eq!(
                error.code(),
                Some(&SqlState::FOREIGN_KEY_VIOLATION),
                "{} {final_check}",
                link.table
            );
            c.batch_execute("ROLLBACK").await?;
            let sql = format!(
                "SELECT count(*)::bigint FROM app.{} WHERE id=$1",
                link.table
            );
            assert_eq!(c.query_one(&sql, &[&id]).await?.get::<_, i64>(0), 0);
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn scope_parent_reassignment_is_rejected_without_weakening_context_history() -> TestResult {
    let c = connection().await?;
    let local = seed(&c).await?;
    let other = seed(&c).await?;
    for index in [0, 3, 4] {
        LINKS[index]
            .insert(&c, Uuid::new_v4(), &local, &local)
            .await?;
    }
    for (sql, id) in [
        (
            "UPDATE app.match_seat SET match_id=$2,seat_index=1 WHERE id=$1",
            local.seat,
        ),
        (
            "UPDATE app.game SET match_id=$2,game_index=1 WHERE id=$1",
            local.game,
        ),
    ] {
        let error = c.execute(sql, &[&id, &other.match_id]).await.unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::FOREIGN_KEY_VIOLATION));
    }
    let error = c
        .execute(
            "UPDATE app.game_event SET game_id=$2,context_id=$3,sequence=2 WHERE id=$1",
            &[&local.event, &other.game, &other.context],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
    let error = c
        .execute(
            "UPDATE app.game_context SET game_id=$2 WHERE id=$1",
            &[&local.context, &other.game],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deferred_scope_repair_and_match_wide_handoff_preserve_their_allowed_forms() -> TestResult {
    let c = connection().await?;
    let local = seed(&c).await?;
    let other = seed(&c).await?;
    c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
        .await?;
    LINKS[0].insert(&c, Uuid::new_v4(), &local, &other).await?;
    c.execute(
        "UPDATE app.match_seat SET match_id=$2,seat_index=1 WHERE id=$1",
        &[&other.seat, &local.match_id],
    )
    .await?;
    c.batch_execute("COMMIT").await?;
    let id = Uuid::new_v4();
    c.execute("INSERT INTO app.durable_handoff(id,match_id,source_ref,destination,kind,state) VALUES($1,$2,$1::uuid::text,'fixture','fixture','pending')", &[&id,&local.match_id]).await?;
    let error = c.execute("INSERT INTO app.durable_handoff(match_id,source_ref,destination,kind,state) VALUES($1,$1::uuid::text,'fixture','fixture','pending')", &[&Uuid::new_v4()]).await.unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::FOREIGN_KEY_VIOLATION));
    Ok(())
}
