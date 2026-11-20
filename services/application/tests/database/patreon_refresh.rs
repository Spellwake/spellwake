//! Actual SQL authority over canonical synthetic OAuth admission, not live provider observations.
use super::wait_for_database_lock as blocked;
use super::{TestResult, connection, patreon_association as oauth};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
#[path = "patreon_current_work.rs"]
mod current_work;
struct Linked {
    identity: oauth::Identity,
    association: Uuid,
    terms: Value,
    proof: Value,
    request: Uuid,
    state: String,
    code: String,
    nonce: String,
}
async fn linked(c: &Client) -> Result<Linked, Box<dyn std::error::Error + Send + Sync>> {
    let identity = oauth::identity(c).await?;
    let terms = oauth::terms();
    let proof = oauth::proof(&terms);
    let request = Uuid::new_v4();
    let state = oauth::hash();
    let code = oauth::hash();
    let nonce = oauth::hash();
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        oauth::start(c, identity, request, &state, &terms).await?["status"],
        "success"
    );
    assert_eq!(
        oauth::claim(c, identity, request, &state, &code, &nonce).await?["data"]["mayExchange"],
        true
    );
    let result = oauth::finish(c, identity, request, &state, &code, &nonce, &proof).await?;
    assert_eq!(result["status"], "success");
    let association = serde_json::from_value(result["data"]["resourceId"].clone())?;
    c.batch_execute("RESET ROLE").await?;
    Ok(Linked {
        identity,
        association,
        terms,
        proof,
        request,
        state,
        code,
        nonce,
    })
}
async fn prepare(c: &Client, id: Uuid) -> Result<Option<Value>, tokio_postgres::Error> {
    c.query_one("SELECT app.read_patreon_refresh($1)", &[&id])
        .await
        .map(|r| r.get(0))
}
async fn commit(
    c: &Client,
    id: Uuid,
    t: &Value,
    p: &Value,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one("SELECT app.commit_patreon_refresh($1,$2,$3)", &[&id, t, p])
        .await
        .map(|r| r.get(0))
}
fn proof(l: &Linked, tiers: Value) -> Value {
    json!({"version":1,"source":"patreon-v2-current-member-v1","campaignId":l.proof["campaignId"],"userId":l.proof["userId"],"memberId":l.proof["memberId"],"memberHash":oauth::hash(),"entitledTierIds":tiers})
}
async fn tier(c: &Client, rank: i64) -> Result<(Uuid, String), tokio_postgres::Error> {
    let id = Uuid::new_v4();
    let external = format!("tier-{id}");
    c.execute("INSERT INTO app.supporter_tier(id,patreon_tier_ref,name,rank) VALUES($1,$2,'Synthetic configured tier',$3)",&[&id,&external,&rank]).await?;
    Ok((id, external))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_chains_tier_only_sources_and_historical_replay_without_financial_effects()
-> TestResult {
    let c = connection().await?;
    let l = linked(&c).await?;
    let (low, low_ref) = tier(&c, i64::MIN).await?;
    let (a, ar) = tier(&c, i64::MAX).await?;
    let (b, br) = tier(&c, i64::MAX).await?;
    let p = proof(&l, json!([br, ar]));
    let request = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    let first = prepare(&c, l.association).await?.unwrap();
    assert_eq!(first.as_object().unwrap().len(), 9);
    let accepted = commit(&c, request, &first, &p).await?;
    assert_eq!(accepted["outcome"], "applied");
    let source: Uuid = serde_json::from_value(accepted["sourceEventId"].clone())?;
    let row=c.query_one("SELECT active,active_tier_id,patreon_member_ref,source_event_id FROM app.patreon_membership WHERE id=$1",&[&l.association]).await?;
    assert!(row.get::<_, bool>(0));
    assert_eq!(row.get::<_, Uuid>(1), a.min(b));
    assert_ne!(row.get::<_, Uuid>(1), low);
    assert_eq!(
        row.get::<_, String>(2),
        l.proof["memberId"].as_str().unwrap()
    );
    assert_eq!(row.get::<_, Uuid>(3), source);
    let second = prepare(&c, l.association).await?.unwrap();
    assert_eq!(second["expectedSourceId"], json!(source));
    assert_eq!(second["anchorEventId"], first["anchorEventId"]);
    assert_eq!(
        commit(&c, Uuid::new_v4(), &second, &proof(&l, json!([low_ref]))).await?["outcome"],
        "applied"
    );
    assert_eq!(
        c.query_one(
            "SELECT active_tier_id FROM app.patreon_membership WHERE id=$1",
            &[&l.association]
        )
        .await?
        .get::<_, Uuid>(0),
        low
    );
    let third = prepare(&c, l.association).await?.unwrap();
    assert_eq!(
        commit(&c, Uuid::new_v4(), &third, &proof(&l, json!([]))).await?["outcome"],
        "applied"
    );
    assert_eq!(commit(&c, request, &first, &p).await?, accepted);
    assert!(
        !c.query_one(
            "SELECT active FROM app.patreon_membership WHERE id=$1",
            &[&l.association]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(
        oauth::view(&c, l.identity, l.request, &l.state, Some(&l.code)).await?["data"]["phase"],
        "linked"
    );
    assert_eq!(
        oauth::finish(
            &c, l.identity, l.request, &l.state, &l.code, &l.nonce, &l.proof
        )
        .await?["data"]["resourceId"],
        json!(l.association)
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.provider_event WHERE account_id=$1 AND event_type='api.membership_refresh'",&[&l.identity.actor]).await?.get::<_,i64>(0),3);
    assert_eq!(c.query_one("SELECT count(*) FROM app.patreon_payment p JOIN app.patreon_membership m ON m.id=p.membership_id WHERE m.account_id=$1",&[&l.identity.actor]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.cosmetic_grant WHERE account_id=$1",
            &[&l.identity.actor]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_parent_cas_and_pending_oauth_gate_fence_both_delayed_observation_directions()
-> TestResult {
    let c = connection().await?;
    let l = linked(&c).await?;
    let p = proof(&l, json!([]));
    c.batch_execute("SET ROLE service_role").await?;
    let old = prepare(&c, l.association).await?.unwrap();
    let first = commit(&c, Uuid::new_v4(), &old, &p).await?;
    assert_eq!(first["outcome"], "applied");
    assert_eq!(
        commit(&c, Uuid::new_v4(), &old, &p).await?["outcome"],
        "superseded"
    );
    let before = prepare(&c, l.association).await?.unwrap();
    let r = Uuid::new_v4();
    let state = oauth::hash();
    let code = oauth::hash();
    let nonce = oauth::hash();
    oauth::start(&c, l.identity, r, &state, &l.terms).await?;
    assert!(prepare(&c, l.association).await?.is_none());
    assert_eq!(
        commit(&c, Uuid::new_v4(), &before, &p).await?["outcome"],
        "superseded"
    );
    oauth::claim(&c, l.identity, r, &state, &code, &nonce).await?;
    assert!(prepare(&c, l.association).await?.is_none());
    assert_eq!(
        oauth::finish(&c, l.identity, r, &state, &code, &nonce, &l.proof).await?["status"],
        "success"
    );
    let after = prepare(&c, l.association).await?.unwrap();
    assert_ne!(after["generation"], before["generation"]);
    assert_eq!(
        commit(&c, Uuid::new_v4(), &before, &p).await?["outcome"],
        "superseded"
    );
    c.batch_execute("RESET ROLE").await?;
    oauth::timed_start(
        &c,
        l.identity,
        Uuid::new_v4(),
        &oauth::hash(),
        &l.terms,
        -60,
    )
    .await?;
    c.batch_execute("SET ROLE service_role").await?;
    let expired = prepare(&c, l.association).await?.unwrap();
    assert_ne!(expired["generation"], after["generation"]);
    assert_eq!(
        commit(&c, Uuid::new_v4(), &expired, &p).await?["outcome"],
        "applied"
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_old_accepted_source_replays_after_member_reauthorization_and_pending_deletion()
-> TestResult {
    let c = connection().await?;
    let l = linked(&c).await?;
    let request = Uuid::new_v4();
    let p = proof(&l, json!([]));
    c.batch_execute("SET ROLE service_role").await?;
    let t = prepare(&c, l.association).await?.unwrap();
    let accepted = commit(&c, request, &t, &p).await?;
    let mut changed = l.proof.clone();
    changed["memberId"] = json!(format!("new-member-{}", Uuid::new_v4()));
    let r = Uuid::new_v4();
    let s = oauth::hash();
    let code = oauth::hash();
    let nonce = oauth::hash();
    oauth::start(&c, l.identity, r, &s, &l.terms).await?;
    oauth::claim(&c, l.identity, r, &s, &code, &nonce).await?;
    assert_eq!(
        oauth::finish(&c, l.identity, r, &s, &code, &nonce, &changed).await?["status"],
        "success"
    );
    let current = prepare(&c, l.association).await?.unwrap();
    assert_eq!(current["memberId"], changed["memberId"]);
    assert_eq!(
        oauth::view(&c, l.identity, l.request, &l.state, Some(&l.code)).await?["data"]["phase"],
        "linked"
    );
    assert_eq!(
        oauth::finish(
            &c, l.identity, l.request, &l.state, &l.code, &l.nonce, &l.proof
        )
        .await?["data"]["resourceId"],
        json!(l.association)
    );
    assert_eq!(commit(&c, request, &t, &p).await?, accepted);
    let mut conflict = p.clone();
    conflict["memberHash"] = json!(oauth::hash());
    assert_eq!(
        commit(&c, request, &t, &conflict).await.unwrap_err().code(),
        Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&l.identity.actor]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert!(prepare(&c, l.association).await?.is_none());
    assert_eq!(commit(&c, request, &t, &p).await?, accepted);
    let mut cp = p.clone();
    cp["memberId"] = changed["memberId"].clone();
    assert_eq!(
        commit(&c, Uuid::new_v4(), &current, &cp).await?["outcome"],
        "unavailable"
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_strict_ticket_proof_and_unknown_identity_rejections_leave_no_source_rows()
-> TestResult {
    let c = connection().await?;
    let l = linked(&c).await?;
    let p = proof(&l, json!([]));
    c.batch_execute("SET ROLE service_role").await?;
    let t = prepare(&c, l.association).await?.unwrap();
    for (field, bad) in [
        ("generation", json!("0")),
        ("generation", json!("9223372036854775808")),
        ("generation", json!("01")),
        ("associationId", json!(Uuid::nil())),
        ("userId", json!("é")),
        ("expectedSourceId", Value::Null),
    ] {
        let mut bad_t = t.clone();
        bad_t[field] = bad;
        assert_eq!(
            commit(&c, Uuid::new_v4(), &bad_t, &p)
                .await
                .unwrap_err()
                .code(),
            Some(&tokio_postgres::error::SqlState::INVALID_PARAMETER_VALUE)
        );
    }
    let mut extra = t.clone();
    extra["extra"] = json!(true);
    assert!(commit(&c, Uuid::new_v4(), &extra, &p).await.is_err());
    for (field, bad) in [
        ("source", json!("patreon-v2-identity-member-v1")),
        ("version", json!("1")),
        ("memberHash", json!("A".repeat(64))),
        ("entitledTierIds", json!(["same", "same"])),
        ("entitledTierIds", json!(vec!["tier"; 1001])),
        ("userId", Value::Null),
    ] {
        let mut bad_p = p.clone();
        bad_p[field] = bad;
        assert_eq!(
            commit(&c, Uuid::new_v4(), &t, &bad_p)
                .await
                .unwrap_err()
                .code(),
            Some(&tokio_postgres::error::SqlState::INVALID_PARAMETER_VALUE)
        );
    }
    let mut missing = p.clone();
    missing.as_object_mut().unwrap().remove("memberId");
    assert!(commit(&c, Uuid::new_v4(), &t, &missing).await.is_err());
    assert!(commit(&c, Uuid::nil(), &t, &p).await.is_err());
    assert!(commit(&c, Uuid::new_v4(), &Value::Null, &p).await.is_err());
    let mut unknown = t.clone();
    unknown["associationId"] = json!(Uuid::new_v4());
    let result = commit(&c, Uuid::new_v4(), &unknown, &p).await?;
    assert_eq!(result["outcome"], "unavailable");
    assert!(result["sourceEventId"].is_null());
    let mut stale = t.clone();
    stale["anchorEventId"] = json!(Uuid::new_v4());
    assert_eq!(
        commit(&c, Uuid::new_v4(), &stale, &p).await?["outcome"],
        "superseded"
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.provider_event WHERE account_id=$1 AND event_type='api.membership_refresh'",&[&l.identity.actor]).await?.get::<_,i64>(0),0);
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_runtime_capabilities_are_private_and_two_service_rpcs_do_not_restore_raw_writes()
-> TestResult {
    let c = connection().await?;
    let public = [
        "read_patreon_refresh(uuid)",
        "commit_patreon_refresh(uuid,jsonb,jsonb)",
    ];
    let private = [
        "patreon_oauth_membership_source(app.patreon_membership,app.patreon_membership,text)",
        "patreon_oauth_commit_source(app.patreon_membership)",
        "patreon_member_fields(jsonb)",
        "patreon_refresh_proof(jsonb)",
        "patreon_refresh_ticket(jsonb)",
        "patreon_refresh_event_ticket(app.provider_event)",
        "patreon_oauth_anchor(app.patreon_membership,uuid)",
        "patreon_refresh_source(app.patreon_membership,app.provider_event,boolean)",
        "patreon_refresh_context(app.patreon_membership)",
    ];
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for name in public.iter().chain(private.iter()) {
            assert_eq!(
                c.query_one(
                    "SELECT has_function_privilege($1,$2,'EXECUTE')",
                    &[&role, &format!("app.{name}")]
                )
                .await?
                .get::<_, bool>(0),
                role == "service_role" && public.contains(name),
                "{role} {name}"
            );
        }
    }
    c.batch_execute("SET ROLE service_role").await?;
    assert!(prepare(&c, Uuid::new_v4()).await?.is_none());
    assert!(prepare(&c, Uuid::nil()).await.is_err());
    assert!(
        c.execute(
            "UPDATE app.patreon_membership SET active=false WHERE false",
            &[]
        )
        .await
        .is_err()
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM pg_class WHERE relnamespace='app'::regnamespace AND relkind='r'",
            &[]
        )
        .await?
        .get::<_, i64>(0),
        96
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_competing_observations_wait_and_replay_or_supersede_without_duplicate_sources()
-> TestResult {
    for mode in ["same", "changed", "different"] {
        let a = connection().await?;
        let b = connection().await?;
        let observer = connection().await?;
        let l = linked(&a).await?;
        let p = proof(&l, json!([]));
        let request = Uuid::new_v4();
        a.batch_execute("SET ROLE service_role").await?;
        let t = prepare(&a, l.association).await?.unwrap();
        a.batch_execute("BEGIN").await?;
        let accepted = commit(&a, request, &t, &p).await?;
        let mut other = p.clone();
        if mode == "changed" {
            other["memberHash"] = json!(oauth::hash());
        }
        let other_request = if mode == "different" {
            Uuid::new_v4()
        } else {
            request
        };
        let second_ticket = t.clone();
        let pid: i32 = b.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
        b.batch_execute("SET ROLE service_role").await?;
        let task =
            tokio::spawn(async move { commit(&b, other_request, &second_ticket, &other).await });
        blocked(&observer, pid).await?;
        a.batch_execute("COMMIT").await?;
        let result = task.await?;
        match mode {
            "same" => assert_eq!(result?, accepted),
            "changed" => assert!(result.is_err()),
            _ => {
                let v = result?;
                assert_eq!(v["outcome"], "superseded");
                assert!(v["sourceEventId"].is_null());
            }
        }
        assert_eq!(observer.query_one("SELECT count(*) FROM app.provider_event WHERE account_id=$1 AND event_type='api.membership_refresh'",&[&l.identity.actor]).await?.get::<_,i64>(0),1);
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_tier_wait_serializes_deletion_and_changed_mapping_rolls_back_the_entire_source()
-> TestResult {
    for change_mapping in [false, true] {
        let owner = connection().await?;
        let worker = connection().await?;
        let observer = connection().await?;
        let deletion = connection().await?;
        let l = linked(&owner).await?;
        let (id, external) = tier(&owner, 5).await?;
        worker.batch_execute("SET ROLE service_role").await?;
        let t = prepare(&worker, l.association).await?.unwrap();
        let p = proof(&l, json!([external]));
        let pid: i32 = worker
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        owner.batch_execute("BEGIN").await?;
        owner
            .query_one(
                "SELECT id FROM app.supporter_tier WHERE id=$1 FOR UPDATE",
                &[&id],
            )
            .await?;
        if change_mapping {
            owner
                .execute(
                    "UPDATE app.supporter_tier SET patreon_tier_ref=$2 WHERE id=$1",
                    &[&id, &format!("changed-{id}")],
                )
                .await?;
        }
        let task = tokio::spawn(async move { commit(&worker, Uuid::new_v4(), &t, &p).await });
        blocked(&observer, pid).await?;
        if change_mapping {
            owner.batch_execute("COMMIT").await?;
            assert!(task.await?.is_err());
            assert_eq!(observer.query_one("SELECT count(*) FROM app.provider_event WHERE account_id=$1 AND event_type='api.membership_refresh'",&[&l.identity.actor]).await?.get::<_,i64>(0),0);
            assert!(
                !observer
                    .query_one(
                        "SELECT active FROM app.patreon_membership WHERE id=$1",
                        &[&l.association]
                    )
                    .await?
                    .get::<_, bool>(0)
            );
        } else {
            let deletion_pid: i32 = deletion
                .query_one("SELECT pg_backend_pid()", &[])
                .await?
                .get(0);
            let account = l.identity.actor;
            let pending = tokio::spawn(async move {
                deletion.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&account]).await
            });
            blocked(&observer, deletion_pid).await?;
            owner.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["outcome"], "applied");
            assert_eq!(pending.await??, 1);
            assert!(prepare(&observer, l.association).await?.is_none());
        }
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_does_not_adopt_legacy_rows_and_rejects_unsupported_isolation() -> TestResult {
    let c = connection().await?;
    let (account, _) = super::pricing::buyer(&c).await?;
    let member = Uuid::new_v4();
    let source:Uuid=c.query_one("INSERT INTO app.provider_event(provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state) VALUES('patreon',$1,$2,clock_timestamp(),'synthetic-unqualified','synthetic','verified') RETURNING id",&[&Uuid::new_v4().to_string(),&account]).await?.get(0);
    c.execute("INSERT INTO app.patreon_membership(id,account_id,patreon_member_ref,active,source_event_id) VALUES($1,$2,$3,false,$4)",&[&member,&account,&format!("legacy-{member}"),&source]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert!(prepare(&c, member).await?.is_none());
    c.batch_execute("RESET ROLE").await?;
    let l = linked(&c).await?;
    c.batch_execute("SET ROLE service_role").await?;
    let t = prepare(&c, l.association).await?.unwrap();
    let p = proof(&l, json!([]));
    c.batch_execute(
        "RESET ROLE; BEGIN ISOLATION LEVEL REPEATABLE READ; SET LOCAL ROLE service_role",
    )
    .await?;
    assert!(prepare(&c, l.association).await.is_err());
    c.batch_execute("ROLLBACK; BEGIN ISOLATION LEVEL REPEATABLE READ; SET LOCAL ROLE service_role")
        .await?;
    assert!(commit(&c, Uuid::new_v4(), &t, &p).await.is_err());
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_incomplete_source_lineage_or_unprocessed_commit_cannot_replace_current_membership()
-> TestResult {
    let c = connection().await?;
    let l = linked(&c).await?;
    let p = proof(&l, json!([]));
    c.batch_execute("SET ROLE service_role").await?;
    let t = prepare(&c, l.association).await?.unwrap();
    c.batch_execute("RESET ROLE").await?;
    for mode in ["anchor", "parent", "missing_root_receipt", "unprocessed"] {
        let mut ticket = t.clone();
        if mode == "anchor" {
            ticket["anchorEventId"] = json!(Uuid::new_v4());
        }
        if mode == "parent" {
            ticket["expectedSourceId"] = json!(Uuid::new_v4());
        }
        let event = Uuid::new_v4();
        let request = Uuid::new_v4();
        c.batch_execute("BEGIN").await?;
        if mode == "missing_root_receipt" {
            let root = Uuid::new_v4();
            let missing_start = Uuid::new_v4();
            c.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state,effect,effect_key) VALUES($1,'patreon',$2,$3,clock_timestamp(),'oauth.association','sha256:'||app.auth_json_hash($4::jsonb),'processed',$4,'none',jsonb_build_object('version',1,'kind','patreon_link_v1','startId',$5::uuid,'associationId',$6::uuid,'generation',$7::text,'proofHash',app.auth_json_hash($4::jsonb)),$8)",&[&root,&format!("oauth:{}:{missing_start}",l.identity.actor),&l.identity.actor,&l.proof,&missing_start,&l.association,&t["generation"].as_str().unwrap(),&format!("patreon-link:{}:{missing_start}",l.identity.actor)]).await?;
            ticket["anchorEventId"] = json!(root);
        }

        c.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state,effect,effect_key) VALUES($1,'patreon',$2,$3,clock_timestamp(),'api.membership_refresh','sha256:'||app.auth_json_hash($4::jsonb),'verified',$4,'none',jsonb_build_object('version',1,'kind','patreon_refresh_v1','requestId',$5::uuid,'associationId',$6::uuid,'parentEventId',$7::jsonb->>'expectedSourceId','anchorEventId',$7::jsonb->>'anchorEventId','generation',$7::jsonb->>'generation','ticketHash',app.auth_json_hash($7::jsonb),'proofHash',app.auth_json_hash($4::jsonb)),$8)",&[&event,&format!("refresh:{}:{request}",l.association),&l.identity.actor,&p,&request,&l.association,&ticket,&format!("patreon-refresh:{}:{request}",l.association)]).await?;
        let updated = c
            .execute(
                "UPDATE app.patreon_membership SET source_event_id=$2 WHERE id=$1",
                &[&l.association, &event],
            )
            .await;
        if mode == "unprocessed" {
            assert_eq!(updated?, 1);
            assert!(c.batch_execute("COMMIT").await.is_err());
        } else {
            assert!(updated.is_err());
        }
        c.batch_execute("ROLLBACK").await?;
        assert_eq!(
            c.query_one(
                "SELECT source_event_id::text FROM app.patreon_membership WHERE id=$1",
                &[&l.association]
            )
            .await?
            .get::<_, String>(0),
            t["expectedSourceId"].as_str().unwrap()
        );
        assert_eq!(
            c.query_one(
                "SELECT count(*) FROM app.provider_event WHERE id=$1",
                &[&event]
            )
            .await?
            .get::<_, i64>(0),
            0
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_tier_wait_serializes_a_new_oauth_start_and_fences_later_old_ticket_work()
-> TestResult {
    let owner = connection().await?;
    let worker = connection().await?;
    let starter = connection().await?;
    let observer = connection().await?;
    let l = linked(&owner).await?;
    let (id, external) = tier(&owner, 5).await?;
    worker.batch_execute("SET ROLE service_role").await?;
    let t = prepare(&worker, l.association).await?.unwrap();
    let p = proof(&l, json!([external]));
    let request = Uuid::new_v4();
    let replay_ticket = t.clone();
    let replay_proof = p.clone();
    let worker_pid: i32 = worker
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    owner.batch_execute("BEGIN").await?;
    owner
        .query_one(
            "SELECT id FROM app.supporter_tier WHERE id=$1 FOR UPDATE",
            &[&id],
        )
        .await?;
    let refresh = tokio::spawn(async move { commit(&worker, request, &t, &p).await });
    blocked(&observer, worker_pid).await?;
    starter.batch_execute("SET ROLE service_role").await?;
    let starter_pid: i32 = starter
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let identity = l.identity;
    let terms = l.terms.clone();
    let start = tokio::spawn(async move {
        oauth::start(&starter, identity, Uuid::new_v4(), &oauth::hash(), &terms).await
    });
    blocked(&observer, starter_pid).await?;
    owner.batch_execute("COMMIT").await?;
    let accepted = refresh.await??;
    assert_eq!(accepted["outcome"], "applied");
    assert_eq!(start.await??["status"], "success");
    observer.batch_execute("SET ROLE service_role").await?;
    assert!(prepare(&observer, l.association).await?.is_none());
    assert_eq!(
        commit(&observer, request, &replay_ticket, &replay_proof).await?,
        accepted
    );
    assert_eq!(
        commit(&observer, Uuid::new_v4(), &replay_ticket, &replay_proof).await?["outcome"],
        "superseded"
    );
    observer.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn new_oauth_start_winning_the_account_lock_supersedes_a_waiting_refresh_without_writes()
-> TestResult {
    let owner = connection().await?;
    let starter = connection().await?;
    let worker = connection().await?;
    let observer = connection().await?;
    let l = linked(&owner).await?;
    worker.batch_execute("SET ROLE service_role").await?;
    let t = prepare(&worker, l.association).await?.unwrap();
    let p = proof(&l, json!([]));
    let pid: i32 = worker
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    starter
        .batch_execute("BEGIN; SET LOCAL ROLE service_role")
        .await?;
    assert_eq!(
        oauth::start(
            &starter,
            l.identity,
            Uuid::new_v4(),
            &oauth::hash(),
            &l.terms
        )
        .await?["status"],
        "success"
    );
    let refresh = tokio::spawn(async move { commit(&worker, Uuid::new_v4(), &t, &p).await });
    blocked(&observer, pid).await?;
    starter.batch_execute("COMMIT").await?;
    let result = refresh.await??;
    assert_eq!(result["outcome"], "superseded");
    assert!(result["sourceEventId"].is_null());
    assert_eq!(observer.query_one("SELECT count(*) FROM app.provider_event WHERE account_id=$1 AND event_type='api.membership_refresh'",&[&l.identity.actor]).await?.get::<_,i64>(0),0);
    Ok(())
}
