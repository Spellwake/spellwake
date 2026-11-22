//! Whole-relation role capabilities plus caller-scoped match/progression reads.
use super::{
    TestResult, account, between_deadline_integrity as deadlines, connection, set_auth_claims,
};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
async fn subject(c: &Client, actor: Uuid) -> Result<Uuid, tokio_postgres::Error> {
    Ok(c.query_one(
        "SELECT auth_subject FROM app.account WHERE id=$1",
        &[&actor],
    )
    .await?
    .get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn every_relation_preserves_rls_invoker_views_and_closed_runtime_mutations() -> TestResult {
    let c = connection().await?;
    let relations=c.query("SELECT c.oid,c.relname,c.relkind::text,c.relrowsecurity,c.relforcerowsecurity,coalesce(c.reloptions,'{}'), (SELECT quote_ident(a.attname) FROM pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped AND a.attgenerated='' ORDER BY a.attnum LIMIT 1) FROM pg_class c WHERE c.relnamespace='app'::regnamespace AND c.relkind IN('r','v') ORDER BY c.relname",&[]).await?;
    assert_eq!(relations.len(), 121);
    for row in &relations {
        let name: String = row.get(1);
        let kind: String = row.get(2);
        if kind == "r" {
            assert!(row.get::<_, bool>(3), "{name}");
            assert!(row.get::<_, bool>(4), "{name}");
        } else {
            assert!(
                row.get::<_, Vec<String>>(5)
                    .iter()
                    .any(|v| v == "security_invoker=true"),
                "{name}"
            );
        }
    }
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        let count:i64=c.query_one("SELECT count(*) FROM pg_class c WHERE c.relnamespace='app'::regnamespace AND c.relkind IN('r','v') AND (has_table_privilege($1,c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER,MAINTAIN') OR EXISTS(SELECT 1 FROM pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped AND (has_column_privilege($1,c.oid,a.attnum,'INSERT') OR has_column_privilege($1,c.oid,a.attnum,'UPDATE') OR has_column_privilege($1,c.oid,a.attnum,'REFERENCES'))))",&[&role]).await?.get(0);
        assert_eq!(count, 0, "{role} raw table/column write authority");
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        for row in &relations {
            let name: String = row.get(1);
            // Pick an actual column, including on join tables with no id column.
            let column: String = row.get(6);
            for statement in [
                format!("UPDATE app.\"{name}\" SET {column}={column} WHERE false"),
                format!("DELETE FROM app.\"{name}\" WHERE false"),
                format!(
                    "INSERT INTO app.\"{name}\" ({column}) SELECT {column} FROM app.\"{name}\" WHERE false"
                ),
            ] {
                let error = c.execute(&statement, &[]).await.unwrap_err();
                // PostgreSQL rejects some view mutations during rewriting before
                // ACL evaluation. The exhaustive privilege assertion above proves
                // those views have no mutation authority independently of errors.
                assert!(
                    row.get::<_, String>(2) == "v"
                        || error.code() == Some(&SqlState::INSUFFICIENT_PRIVILEGE),
                    "{role} {name}: {error:?}"
                );
            }
        }
        c.batch_execute("RESET ROLE").await?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn participant_metadata_is_shared_but_private_artifacts_and_peer_decisions_are_not()
-> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    let humans=c.query("SELECT account_id FROM app.match_seat WHERE match_id=$1 AND controller_kind='human' ORDER BY seat_index",&[&f.matched]).await?;
    assert_eq!(humans.len(), 2);
    for human in humans {
        let actor: Uuid = human.get(0);
        let auth = subject(&c, actor).await?;
        set_auth_claims(&c, auth).await?;
        assert_eq!(
            c.query(
                "SELECT id,state,availability FROM app.match WHERE id=$1",
                &[&f.matched]
            )
            .await?
            .len(),
            1
        );
        assert_eq!(
            c.query(
                "SELECT id,account_id,controller_kind FROM app.match_seat WHERE match_id=$1",
                &[&f.matched]
            )
            .await?
            .len(),
            2
        );
        assert_eq!(c.query("SELECT c.id,c.remaining_ms FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1",&[&f.matched]).await?.len(),2);
        assert_eq!(
            c.query(
                "SELECT id,state,deadline_at FROM app.between_game_decision WHERE match_id=$1",
                &[&f.matched]
            )
            .await?
            .len(),
            1
        );
        assert_eq!(c.query("SELECT v.id FROM app.match_decision_deadline v JOIN app.between_game_decision d ON d.id=v.decision_id WHERE d.match_id=$1",&[&f.matched]).await?.len(),1);
        assert_eq!(
            c.query("SELECT id FROM app.game WHERE match_id=$1", &[&f.matched])
                .await?
                .len(),
            1
        );
        for sql in [
            "SELECT deck_snapshot_ref FROM app.match_seat",
            "SELECT sponsor_id FROM app.match_seat",
            "SELECT team_ref FROM app.match_seat",
            "SELECT source_revision_id FROM app.match_seat",
            "SELECT reply_ref FROM app.between_game_decision",
            "SELECT deadline_source_receipt_id FROM app.between_game_decision",
            "SELECT admission_qualification_ref FROM app.match",
            "SELECT private_state_ref FROM app.game_checkpoint",
            "SELECT private_payload_ref FROM app.game_event",
            "SELECT artifact_ref FROM app.replay_manifest",
            "SELECT grant_secret_ref FROM app.controller_grant",
        ] {
            assert_eq!(
                c.query(sql, &[]).await.unwrap_err().code(),
                Some(&SqlState::INSUFFICIENT_PRIVILEGE),
                "{sql}"
            );
        }
        c.batch_execute("RESET ROLE").await?;
    }
    let (_, stranger) = account(&c).await?;
    set_auth_claims(&c, stranger).await?;
    for sql in [
        "SELECT id FROM app.match WHERE id=$1",
        "SELECT id FROM app.match_seat WHERE match_id=$1",
        "SELECT id FROM app.between_game_decision WHERE match_id=$1",
        "SELECT id FROM app.game WHERE match_id=$1",
    ] {
        assert!(c.query(sql, &[&f.matched]).await?.is_empty(), "{sql}");
    }
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn missing_forged_and_expired_identity_never_opens_match_or_progression_rows() -> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    let owner = f.room.source.actor;
    let auth = subject(&c, owner).await?;
    let def:Uuid=c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref) VALUES($1,'fixture','fixture:objective',3,500,'fixture:casts','fixture:v1','fixture:qualified') RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    let populated:Value=c.query_one("SELECT app.generate_daily_quest($1,$2,to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD'),$3)",&[&owner,&def,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(populated["status"], "success");
    let claims: Value = c
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&auth])
        .await?
        .get(0);
    let (_, other) = account(&c).await?;
    let mut forged = claims.clone();
    forged["sub"] = json!(other);
    for invalid in [json!({}), forged] {
        c.batch_execute("SET ROLE authenticated").await?;
        c.query_one(
            "SELECT set_config('request.jwt.claims',$1,false)",
            &[&invalid.to_string()],
        )
        .await?;
        assert!(
            c.query("SELECT id FROM app.match WHERE id=$1", &[&f.matched])
                .await?
                .is_empty()
        );
        assert!(
            c.query(
                "SELECT id FROM app.quest_assignment WHERE account_id=$1",
                &[&owner]
            )
            .await?
            .is_empty()
        );
        c.batch_execute("RESET ROLE").await?;
    }
    c.execute(
        "UPDATE auth.sessions SET not_after=clock_timestamp()-interval '1 second' WHERE user_id=$1",
        &[&auth],
    )
    .await?;
    set_auth_claims(&c, auth).await?;
    assert!(
        c.query("SELECT id FROM app.match WHERE id=$1", &[&f.matched])
            .await?
            .is_empty()
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn owners_read_quest_state_and_calendar_while_strangers_cannot_read_assignments() -> TestResult
{
    let c = connection().await?;
    let (owner, auth) = account(&c).await?;
    let (_, other) = account(&c).await?;
    let def:Uuid=c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref) VALUES($1,'fixture','fixture:objective',3,500,'fixture:casts','fixture:v1','fixture:qualified') RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    let reply:Value=c.query_one("SELECT app.generate_daily_quest($1,$2,to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD'),$3)",&[&owner,&def,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(reply["status"], "success");
    let assignment: Uuid = reply["data"]["resourceId"].as_str().unwrap().parse()?;
    set_auth_claims(&c, auth).await?;
    assert_eq!(
        c.query(
            "SELECT id,definition_id,progress FROM app.quest_assignment WHERE id=$1",
            &[&assignment]
        )
        .await?
        .len(),
        1
    );
    assert_eq!(
        c.query(
            "SELECT id FROM app.quest_progress WHERE assignment_id=$1",
            &[&assignment]
        )
        .await?
        .len(),
        1
    );
    assert_eq!(
        c.query(
            "SELECT id FROM app.daily_quest_state WHERE account_id=$1",
            &[&owner]
        )
        .await?
        .len(),
        1
    );
    assert_eq!(
        c.query(
            "SELECT id,target,reward_coins FROM app.quest_definition_revision WHERE id=$1",
            &[&def]
        )
        .await?
        .len(),
        1
    );
    assert_eq!(
        c.query(
            "SELECT objective_qualification_ref FROM app.quest_definition_revision",
            &[]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    set_auth_claims(&c, other).await?;
    assert!(
        c.query(
            "SELECT id FROM app.quest_assignment WHERE id=$1",
            &[&assignment]
        )
        .await?
        .is_empty()
    );
    assert!(
        c.query(
            "SELECT id FROM app.quest_progress WHERE assignment_id=$1",
            &[&assignment]
        )
        .await?
        .is_empty()
    );
    assert!(
        c.query(
            "SELECT id FROM app.daily_quest_state WHERE account_id=$1",
            &[&owner]
        )
        .await?
        .is_empty()
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn every_function_capability_and_privileged_owner_is_explicit_and_search_path_is_closed()
-> TestResult {
    let c = connection().await?;
    let approved: Value =
        serde_json::from_str(include_str!("../fixtures/rls-function-capabilities.json"))?;
    let rows=c.query("SELECT p.proname,pg_get_function_identity_arguments(p.oid),p.prosecdef,p.proconfig,r.rolname,r.rolsuper,r.rolbypassrls,ARRAY(SELECT name::text FROM unnest(ARRAY['anon','authenticated','service_role','supabase_auth_admin']::name[]) name WHERE has_function_privilege(name,p.oid,'EXECUTE') ORDER BY name) FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE p.pronamespace='app'::regnamespace ORDER BY p.proname,pg_get_function_identity_arguments(p.oid)",&[]).await?;
    assert!(rows.len() > 500);
    let mut exposed = 0;
    for row in rows {
        let name: String = row.get(0);
        let arguments: String = row.get(1);
        let definer: bool = row.get(2);
        let config: Option<Vec<String>> = row.get(3);
        let owner: String = row.get(4);
        let superuser: bool = row.get(5);
        let bypass: bool = row.get(6);
        let roles: Vec<String> = row.get(7);
        if definer {
            assert!(
                config
                    .unwrap_or_default()
                    .iter()
                    .any(|value| value == "search_path=\"\"" || value == "search_path="),
                "{name} search_path"
            );
            assert_eq!(owner, "postgres", "{name} unexpected elevated owner");
            assert!(
                superuser,
                "{name} local definer's owner bypass must be explicit"
            );
        }
        assert!(
            ![
                "anon",
                "authenticated",
                "service_role",
                "supabase_auth_admin"
            ]
            .contains(&owner.as_str()),
            "{name} runtime-owned definer"
        );
        // Superuser and BYPASSRLS are separate flags; the local postgres fixture owns
        // these elevated boundaries. Runtime-role behavior is tested without it.
        assert!(!bypass || superuser, "{name} undeclared owner bypass");
        if roles.is_empty() {
            continue;
        }
        exposed += 1;
        let expected = approved
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == name && f["arguments"] == arguments)
            .unwrap_or_else(|| {
                panic!("unexpected executable function {name}({arguments}) for {roles:?}")
            });
        let actual: Vec<&str> = roles.iter().map(String::as_str).collect();
        let expected_roles: Vec<&str> = expected["roles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(actual, expected_roles, "{name}({arguments})");
    }
    assert_eq!(exposed, approved.as_array().unwrap().len());
    assert_eq!(c.query_one("SELECT count(*) FROM pg_roles WHERE rolname IN('anon','authenticated','service_role','supabase_auth_admin') AND rolsuper",&[]).await?.get::<_,i64>(0),0);
    assert!(
        c.query_one(
            "SELECT rolbypassrls FROM pg_roles WHERE rolname='service_role'",
            &[]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn queue_and_invitation_metadata_are_owner_scoped_and_tokens_stay_private() -> TestResult {
    let c = connection().await?;
    let f = super::queue_intent_integrity::fixture(&c).await?;
    let body =
        super::queue_intent_integrity::alternatives(f.profile, f.room.source.revision, "duel", "0");
    let queued =
        super::queue_intent_integrity::enqueue(&c, f.room.source.actor, &body, Uuid::new_v4())
            .await?;
    assert_eq!(queued["status"], "success");
    let invitation:Value=c.query_one("SELECT app.issue_room_invitation($1,$2,(SELECT revision FROM app.room WHERE id=$2),$3,$4)",&[&f.room.source.actor,&f.room.id,&format!("{}{}",Uuid::new_v4().simple(),Uuid::new_v4().simple()),&Uuid::new_v4()]).await?.get(0);
    assert_eq!(invitation["status"], "success");
    let auth = subject(&c, f.room.source.actor).await?;
    set_auth_claims(&c, auth).await?;
    assert_eq!(
        c.query(
            "SELECT id,state FROM app.queue_entry WHERE account_id=$1",
            &[&f.room.source.actor]
        )
        .await?
        .len(),
        1
    );
    assert_eq!(
        c.query(
            "SELECT id,state FROM app.invitation WHERE room_id=$1",
            &[&f.room.id]
        )
        .await?
        .len(),
        1
    );
    assert_eq!(
        c.query("SELECT token_hash FROM app.invitation", &[])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    assert_eq!(
        c.query("SELECT preferences FROM app.queue_entry", &[])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.queue_entry SET account_id=$1 WHERE account_id=$2",
            &[&f.room.other, &f.room.source.actor]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    c.batch_execute("RESET ROLE").await?;
    let peer = subject(&c, f.room.other).await?;
    set_auth_claims(&c, peer).await?;
    assert!(
        c.query(
            "SELECT id FROM app.queue_entry WHERE account_id=$1",
            &[&f.room.source.actor]
        )
        .await?
        .is_empty()
    );
    assert!(
        c.query(
            "SELECT id FROM app.invitation WHERE room_id=$1",
            &[&f.room.id]
        )
        .await?
        .is_empty()
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn rank_credit_season_and_finalized_snapshot_reads_follow_the_credited_owner() -> TestResult {
    let c = connection().await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING",&[]).await?;
    let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(1983,1,'1983-01-01T00:00:00Z','1983-04-01T00:00:00Z','fixture:elo','active') RETURNING id",&[]).await?.get(0);
    let (result, _format, owner, _peer) =
        super::rank_source_integrity::source(&c, None, 1, "1983-02-01T00:00:00Z").await?;
    assert_eq!(
        super::rank_source_integrity::consume(&c, result).await?["status"],
        "success"
    );
    let finalized: Value = c
        .query_one("SELECT app.finalize_rank_season($1)", &[&season])
        .await?
        .get(0);
    assert_eq!(finalized["status"], "success");
    let auth = subject(&c, owner).await?;
    let (_, other) = account(&c).await?;
    set_auth_claims(&c, auth).await?;
    assert_eq!(
        c.query(
            "SELECT id,tier,rating FROM app.rank WHERE account_id=$1 AND season_id=$2",
            &[&owner, &season]
        )
        .await?
        .len(),
        1
    );
    assert_eq!(c.query("SELECT id,win,rating_delta FROM app.competitive_result WHERE account_id=$1 AND match_result_id=$2",&[&owner,&result]).await?.len(),1);
    assert_eq!(
        c.query(
            "SELECT id,wins,issued_coins FROM app.daily_win_bucket WHERE account_id=$1",
            &[&owner]
        )
        .await?
        .len(),
        1
    );
    assert_eq!(
        c.query(
            "SELECT id,start_at,end_at FROM app.season WHERE id=$1",
            &[&season]
        )
        .await?
        .len(),
        1
    );
    assert_eq!(
        c.query("SELECT rating_evidence FROM app.competitive_result", &[])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    assert_eq!(c.query("SELECT v.id FROM app.season_rank_snapshot v JOIN app.rank r ON r.id=v.rank_id WHERE r.account_id=$1 AND r.season_id=$2",&[&owner,&season]).await?.len(),1);
    set_auth_claims(&c, other).await?;
    for sql in [
        "SELECT id FROM app.rank WHERE account_id=$1",
        "SELECT id FROM app.competitive_result WHERE account_id=$1",
        "SELECT id FROM app.daily_win_bucket WHERE account_id=$1",
    ] {
        assert!(c.query(sql, &[&owner]).await?.is_empty());
    }
    assert!(c.query("SELECT v.id FROM app.season_rank_snapshot v JOIN app.rank r ON r.id=v.rank_id WHERE r.account_id=$1",&[&owner]).await?.is_empty());
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pending_participant_keeps_match_history_while_unadmitted_and_ai_only_sponsorship_do_not_grant_access()
-> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    let owner = f.room.source.actor;
    let auth = subject(&c, owner).await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&owner]).await?;
    set_auth_claims(&c, auth).await?;
    assert_eq!(
        c.query("SELECT id FROM app.match WHERE id=$1", &[&f.matched])
            .await?
            .len(),
        1
    );
    c.batch_execute("RESET ROLE").await?;
    let source = super::game_scope::seed(&c).await?;
    let sponsor = account(&c).await?;
    c.execute("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,1,'ai')",&[&source.match_id,&sponsor.0]).await?;
    set_auth_claims(&c, sponsor.1).await?;
    let games: serde_json::Value = c
        .query_one(
            "SELECT app.read_match_games($1,NULL,100)",
            &[&source.match_id],
        )
        .await?
        .get(0);
    assert_eq!(
        games,
        serde_json::json!({"status":"fail","data":{"match":"unavailable"}})
    );
    assert!(
        c.query("SELECT id FROM app.match WHERE id=$1", &[&source.match_id])
            .await?
            .is_empty()
    );
    set_auth_claims(&c, subject_after_reset(&c, source.account).await?).await?;
    assert!(
        c.query("SELECT id FROM app.match WHERE id=$1", &[&source.match_id])
            .await?
            .is_empty()
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
async fn subject_after_reset(c: &Client, actor: Uuid) -> Result<Uuid, tokio_postgres::Error> {
    c.batch_execute("RESET ROLE").await?;
    subject(c, actor).await
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn canonical_deletion_removes_history_read_authority_without_erasing_quest_rows() -> TestResult
{
    let c = connection().await?;
    let (owner, auth) = account(&c).await?;
    let def:Uuid=c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref) VALUES($1,'fixture','fixture:objective',3,500,'fixture:casts','fixture:v1','fixture:qualified') RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    let generated:Value=c.query_one("SELECT app.generate_daily_quest($1,$2,to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD'),$3)",&[&owner,&def,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(generated["status"], "success");
    let assignment: Uuid = generated["data"]["resourceId"].as_str().unwrap().parse()?;
    let claims: Value = c
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&auth])
        .await?
        .get(0);
    assert_eq!(
        super::deletion::request(&c, owner, Uuid::new_v4()).await?["status"],
        "success"
    );
    let retired: Value = c
        .query_one("SELECT app.retire_account($1)", &[&owner])
        .await?
        .get(0);
    assert_eq!(retired["status"], "success", "{retired}");
    assert_eq!(
        c.query(
            "SELECT id FROM app.quest_assignment WHERE id=$1",
            &[&assignment]
        )
        .await?
        .len(),
        1
    );
    c.batch_execute("SET ROLE authenticated").await?;
    c.query_one(
        "SELECT set_config('request.jwt.claims',$1,false)",
        &[&claims.to_string()],
    )
    .await?;
    assert!(
        c.query(
            "SELECT id FROM app.quest_assignment WHERE id=$1",
            &[&assignment]
        )
        .await?
        .is_empty()
    );
    assert!(
        c.query(
            "SELECT id FROM app.quest_progress WHERE id=$1",
            &[&assignment]
        )
        .await?
        .is_empty()
    );
    assert!(
        c.query("SELECT id FROM app.cosmetic", &[])
            .await?
            .is_empty()
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn every_selectable_column_matches_the_reviewed_whole_relation_capability_matrix()
-> TestResult {
    let c = connection().await?;
    let approved: Value =
        serde_json::from_str(include_str!("../fixtures/rls-relation-capabilities.json"))?;
    assert_eq!(approved.as_array().unwrap().len(), 121);
    for relation in approved.as_array().unwrap() {
        let name = relation["relation"].as_str().unwrap();
        for role in [
            "anon",
            "authenticated",
            "service_role",
            "supabase_auth_admin",
        ] {
            let actual:Vec<String>=c.query("SELECT a.attname FROM pg_attribute a WHERE a.attrelid=('app.'||$1)::regclass AND a.attnum>0 AND NOT a.attisdropped AND has_column_privilege($2,a.attrelid,a.attnum,'SELECT') ORDER BY a.attnum",&[&name,&role]).await?.into_iter().map(|r|r.get(0)).collect();
            let expected: Vec<&str> = relation["reads"][role]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(
                actual.iter().map(String::as_str).collect::<Vec<_>>(),
                expected,
                "{role} {name}"
            );
        }
    }
    // New objects may not inherit prototype runtime capabilities. Explicit grants
    // are tested above and in the per-function approved signatures.
    assert_eq!(c.query_one("SELECT count(*) FROM pg_default_acl d CROSS JOIN LATERAL aclexplode(d.defaclacl) x WHERE d.defaclnamespace='app'::regnamespace AND x.grantee IN(0,'anon'::regrole::oid,'authenticated'::regrole::oid,'service_role'::regrole::oid,'supabase_auth_admin'::regrole::oid)",&[]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn completed_match_result_is_participant_only_and_private_terminal_sources_are_hidden()
-> TestResult {
    let c = connection().await?;
    let (room, m) = super::ante_transaction_integrity::reserve_match(&c, true, "bo1").await?;
    let game: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    super::match_lifecycle_integrity::open(&c, m, game).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&m, &room.source.actor],
        )
        .await?
        .get(0);
    let outcome = super::match_lifecycle_integrity::complete(&c, m, game, vec![seat]).await?;
    assert_eq!(
        super::match_lifecycle_integrity::consume(&c, m, outcome).await?["data"]["phase"],
        "terminal"
    );
    let actor_subject = subject(&c, room.source.actor).await?;
    set_auth_claims(&c, actor_subject).await?;
    assert_eq!(c.query("SELECT id,outcome,winner_seat_ids,final_game_ids FROM app.match_result WHERE match_id=$1",&[&m]).await?.len(),1);
    assert_eq!(
        c.query("SELECT winners_ref FROM app.match_result", &[])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    c.batch_execute("RESET ROLE").await?;
    let (_, stranger) = account(&c).await?;
    set_auth_claims(&c, stranger).await?;
    assert!(
        c.query("SELECT id FROM app.match_result WHERE match_id=$1", &[&m])
            .await?
            .is_empty()
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn admitted_ai_sponsor_reads_through_its_human_seat_and_cannot_reassign_the_accepted_sponsor()
-> TestResult {
    let c = connection().await?;
    let f = super::native_elimination_integrity::fixture(&c, true).await?;
    let sponsor: Uuid = c
        .query_one(
            "SELECT sponsor_id FROM app.match_seat WHERE match_id=$1 AND controller_kind='ai'",
            &[&f.matched],
        )
        .await?
        .get(0);
    assert!(c.query_one("SELECT EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=$1 AND account_id=$2 AND controller_kind='human')",&[&f.matched,&sponsor]).await?.get::<_,bool>(0));
    let (outsider, outsider_subject) = account(&c).await?;
    assert_eq!(
        c.execute(
            "UPDATE app.match_seat SET sponsor_id=$2 WHERE match_id=$1 AND controller_kind='ai'",
            &[&f.matched, &outsider]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let auth = subject(&c, sponsor).await?;
    set_auth_claims(&c, auth).await?;
    assert_eq!(
        c.query("SELECT id FROM app.match WHERE id=$1", &[&f.matched])
            .await?
            .len(),
        1
    );
    assert_eq!(
        c.query(
            "SELECT id,controller_kind FROM app.match_seat WHERE match_id=$1",
            &[&f.matched]
        )
        .await?
        .len(),
        3
    );
    set_auth_claims(&c, outsider_subject).await?;
    assert!(
        c.query("SELECT id FROM app.match WHERE id=$1", &[&f.matched])
            .await?
            .is_empty()
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
