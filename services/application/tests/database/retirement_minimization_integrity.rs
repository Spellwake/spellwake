//! Normalized SQL retirement/absence proofs; no real provider, Storage or Auth HTTP is asserted.
use super::{
    TestResult, connection, deletion, patreon_association as patreon,
    patreon_financial_integrity as financial,
};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn retire(c: &Client, actor: Uuid) -> FixtureResult<Value> {
    Ok(c.query_one("SELECT app.retire_account($1)", &[&actor])
        .await?
        .get(0))
}
async fn identity_for(c: &Client, actor: Uuid) -> FixtureResult<patreon::Identity> {
    let subject: Uuid = c
        .query_one(
            "SELECT auth_subject FROM app.account WHERE id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    let session: Uuid = c
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0);
    Ok(patreon::Identity {
        actor,
        subject,
        session,
    })
}
pub(super) async fn link(
    c: &Client,
    i: patreon::Identity,
    active: bool,
) -> FixtureResult<(Uuid, Uuid, Value)> {
    let request = Uuid::new_v4();
    let state = patreon::hash();
    let code = patreon::hash();
    let nonce = patreon::hash();
    let terms = patreon::terms();
    let mut proof = patreon::proof(&terms);
    let tier = Uuid::new_v4();
    c.execute("INSERT INTO app.supporter_tier(id,patreon_tier_ref,name,rank) VALUES($1,$1::uuid::text,'Fixture retirement tier',1)", &[&tier]).await?;
    if active {
        proof["entitledTierIds"] = json!([tier.to_string()]);
    }
    assert_eq!(
        patreon::start(c, i, request, &state, &terms).await?["status"],
        "success"
    );
    assert_eq!(
        patreon::claim(c, i, request, &state, &code, &nonce).await?["data"]["mayExchange"],
        true
    );
    let finished = patreon::finish(c, i, request, &state, &code, &nonce, &proof).await?;
    assert_eq!(finished["status"], "success", "{finished}");
    Ok((
        finished["data"]["resourceId"].as_str().unwrap().parse()?,
        tier,
        proof,
    ))
}
async fn observe(c: &Client, actor: Uuid, request: Uuid) -> FixtureResult<Value> {
    Ok(c.query_one(
        "SELECT app.observe_account_deletion($1,$2,$3)",
        &[&actor, &request, &"a".repeat(64)],
    )
    .await?
    .get(0))
}
async fn acknowledge(
    c: &Client,
    actor: Uuid,
    work: Uuid,
    epoch: &str,
    outcome: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let response: Value = c.query_one("SELECT app.observe_account_resource_cleanup($1,$2,$3::text::numeric,$4,'fixture:qualified-canonical-object-absence',$5)", &[&actor,&work,&epoch,&outcome,&request]).await?.get(0);
    let typed = spellwake_application::decode_wire_object::<
        spellwake_application::ApiResponse<spellwake_application::retirement_resources::CleanupAck>,
    >(response.to_string().as_bytes())
    .expect("actual source acknowledgment must decode");
    let original = spellwake_application::retirement_resources::CleanupObservation {
        account_id: actor,
        work_id: work,
        epoch: epoch.parse().unwrap(),
        outcome: serde_json::from_value(json!(outcome)).unwrap(),
        proof_ref: "fixture:qualified-canonical-object-absence"
            .parse()
            .unwrap(),
        request_id: request,
    };
    assert!(
        matches!(typed, spellwake_application::ApiResponse::Success { data } if data.valid_for(&original))
    );
    Ok(response)
}
async fn terminal_resource_fixture(
    c: &Client,
    with_patreon: bool,
) -> FixtureResult<(patreon::Identity, Uuid, Uuid, Option<Uuid>)> {
    let (f, m) = super::ante_transaction_integrity::reserve(c, false).await?;
    let i = identity_for(c, f.source.actor).await?;
    c.execute(
        "DELETE FROM app.match_seat WHERE match_id=$1",
        &[&f.source.match_id],
    )
    .await?;
    c.execute("DELETE FROM app.match WHERE id=$1", &[&f.source.match_id])
        .await?;
    let member = if with_patreon {
        Some(link(c, i, false).await?.0)
    } else {
        None
    };
    let request = Uuid::new_v4();
    assert_eq!(
        deletion::request(c, i.actor, request).await?["status"],
        "success"
    );
    let paused:Value=c.query_one("SELECT app.pause_match_between_games($1,(SELECT revision FROM app.match WHERE id=$1),(SELECT availability_revision FROM app.match WHERE id=$1),clock_timestamp(),'storage','fixture:incident',$2)", &[&m,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(paused["status"], "success");
    let incident: Uuid = paused["data"]["incidentId"].as_str().unwrap().parse()?;
    let terminal:Value=c.query_one("SELECT app.finalize_unrecoverable_match($1,(SELECT revision FROM app.match WHERE id=$1),(SELECT availability_revision FROM app.match WHERE id=$1),$2,'fixture:failed-restoration')", &[&m,&incident]).await?.get(0);
    assert_eq!(terminal["status"], "success");
    let handoff: Value = c.query_one("SELECT app.prepare_handoff_delivery(destination,id) FROM app.durable_handoff WHERE match_id=$1 AND kind='match_outcome'", &[&m]).await?.get(0);
    let prepared = spellwake_application::decode_wire_object::<
        spellwake_application::handoffs::PreparedHandoff,
    >(handoff["data"].to_string().as_bytes())?;
    assert!(prepared.valid_for(
        prepared.handoff_id,
        spellwake_application::handoffs::Destination::MatchSettlement
    ));
    assert!(prepared.manifest.game_id.is_none());
    let binding=c.query_one("SELECT b.id,b.binding_epoch::text,b.stop_receipt_id FROM app.ai_runner_binding b JOIN app.match_seat s ON s.id=b.match_seat_id WHERE s.match_id=$1", &[&m]).await?;
    let b: Uuid = binding.get(0);
    let epoch: String = binding.get(1);
    let stop: Uuid = binding.get(2);
    let stopped:Value=c.query_one("SELECT app.observe_ai_stop($1,$2::text::numeric,$3,'fixture:verified-generation-stop',$4)", &[&b,&epoch,&stop,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(stopped["status"], "success");
    Ok((i, request, f.source.revision, member))
}
async fn resource_fixture(c: &Client) -> FixtureResult<(Uuid, Uuid, Uuid, Uuid, Vec<Value>)> {
    let (i, request, revision, _) = terminal_resource_fixture(c, false).await?;
    let retired = retire(c, i.actor).await?;
    assert_eq!(retired["data"]["state"], "retired");
    let operation: Uuid = retired["data"]["retirementOperationId"]
        .as_str()
        .unwrap()
        .parse()?;
    let mut jobs = Vec::new();
    let mut cursor: Option<Uuid> = None;
    loop {
        let page: Value = c
            .query_one(
                "SELECT app.read_account_resource_work($1,$2,1)",
                &[&i.actor, &cursor],
            )
            .await?
            .get(0);
        let typed = spellwake_application::decode_wire_object::<
            spellwake_application::retirement_resources::ResourcePage,
        >(page.to_string().as_bytes())?;
        assert!(typed.valid_for(operation, cursor, 1));
        jobs.extend(page["items"].as_array().unwrap().iter().cloned());
        cursor = page["nextCursor"].as_str().map(str::parse).transpose()?;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        jobs.len(),
        2,
        "room and adopted authority must both remain discoverable"
    );
    Ok((i.actor, i.subject, request, revision, jobs))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_history_is_retained_inactive_and_late_financial_sources_cannot_restore_benefits()
-> TestResult {
    let c = connection().await?;
    for active in [false, true] {
        let i = patreon::identity(&c).await?;
        let (member, tier, proof) = link(&c, i, active).await?;
        let cosmetic:Uuid=c.query_one("INSERT INTO app.cosmetic(kind,asset_ref,state) VALUES('dice','fixture:retirement-dice','active') RETURNING id", &[]).await?.get(0);
        c.execute(
            "INSERT INTO app.tier_cosmetic(tier_id,cosmetic_id) VALUES($1,$2)",
            &[&tier, &cosmetic],
        )
        .await?;
        let payment = Uuid::new_v4().to_string();
        let mut paid = financial::proof(
            Uuid::new_v4(),
            tier,
            &payment,
            "paid",
            &format!("paid:{payment}"),
        );
        paid["memberRef"] = proof["memberId"].clone();
        let paid_delivery = Uuid::new_v4().to_string();
        let result = financial::commit(&c, &paid, &paid_delivery).await?;
        assert_eq!(result["disposition"], "applied");
        assert!(
            c.query_one(
                "SELECT app.has_cosmetic($1,$2,'dice')",
                &[&i.actor, &cosmetic]
            )
            .await?
            .get::<_, bool>(0)
        );
        let deletion_request = Uuid::new_v4();
        assert_eq!(
            deletion::request(&c, i.actor, deletion_request).await?["status"],
            "success"
        );
        let retired = retire(&c, i.actor).await?;
        assert_eq!(retired["data"]["state"], "retired", "{retired}");
        let row=c.query_one("SELECT m.active,m.active_tier_id IS NULL,e.event_type,m.patreon_member_ref FROM app.patreon_membership m JOIN app.provider_event e ON e.id=m.source_event_id WHERE m.id=$1", &[&member]).await?;
        assert!(!row.get::<_, bool>(0));
        assert!(row.get::<_, bool>(1));
        assert_eq!(row.get::<_, String>(2), "application.retirement");
        assert_eq!(row.get::<_, String>(3), proof["memberId"].as_str().unwrap());
        assert!(c.query_one("SELECT EXISTS(SELECT 1 FROM app.provider_event WHERE provider='patreon' AND account_id=$1 AND event_type='oauth.association' AND capture=$2)", &[&i.actor,&proof]).await?.get::<_,bool>(0));
        assert_eq!(
            financial::commit(&c, &paid, &Uuid::new_v4().to_string()).await?,
            result
        );
        let mut reversed = financial::proof(
            Uuid::new_v4(),
            tier,
            &payment,
            "reversed",
            &format!("reverse:{payment}"),
        );
        reversed["memberRef"] = proof["memberId"].clone();
        assert_eq!(
            financial::commit(&c, &reversed, &Uuid::new_v4().to_string()).await?["state"],
            "reversed"
        );
        let new_payment = Uuid::new_v4().to_string();
        let mut late = financial::proof(
            Uuid::new_v4(),
            tier,
            &new_payment,
            "paid",
            &format!("late:{new_payment}"),
        );
        late["memberRef"] = proof["memberId"].clone();
        assert_eq!(
            financial::commit(&c, &late, &Uuid::new_v4().to_string()).await?["disposition"],
            "ineligible_after_deletion"
        );
        assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.cosmetic_grant WHERE account_id=$1 AND state='active'", &[&i.actor]).await?.get::<_,i64>(0),0);
        assert_eq!(financial::commit(&c, &paid, &paid_delivery).await?, result);
        assert_eq!(
            observe(&c, i.actor, deletion_request).await?["data"]["phase"],
            "finalizing"
        );
        c.batch_execute("BEGIN").await?;
        super::auth_identity::canonical_delete(&c, i.subject).await?;
        c.batch_execute("COMMIT").await?;
        assert_eq!(
            observe(&c, i.actor, deletion_request).await?["data"]["phase"],
            "deleted"
        );
        assert_eq!(financial::commit(&c, &paid, &paid_delivery).await?, result);
        assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.cosmetic_grant WHERE account_id=$1 AND state='active'",&[&i.actor]).await?.get::<_,i64>(0),0);
        assert_eq!(
            c.execute(
                "UPDATE app.patreon_membership SET active=true WHERE id=$1",
                &[&member]
            )
            .await
            .unwrap_err()
            .code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
        assert_eq!(retire(&c, i.actor).await?, retired);
        let page: Value = c
            .query_one(
                "SELECT app.read_account_resource_work($1,NULL,100)",
                &[&i.actor],
            )
            .await?
            .get(0);
        assert_eq!(page["items"], json!([]));
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn auth_acknowledgment_alone_keeps_all_historical_secret_work_discoverable_until_absent()
-> TestResult {
    let c = connection().await?;
    let (actor, subject, request, revision, jobs) = resource_fixture(&c).await?;
    assert_eq!(
        observe(&c, actor, request).await?["data"]["phase"],
        "finalizing"
    );
    c.batch_execute("BEGIN").await?;
    super::auth_identity::canonical_delete(&c, subject).await?;
    c.batch_execute("COMMIT").await?;
    let wire: String = c
        .query_one(
            "SELECT app.read_account_retirement_work($1)::text",
            &[&actor],
        )
        .await?
        .get(0);
    let work = spellwake_application::decode_wire_object::<
        spellwake_application::deletion_delivery::RetirementWork,
    >(wire.as_bytes())?;
    match work {
        spellwake_application::deletion_delivery::RetirementWork::Resources {
            account_id,
            retirement_operation_id,
            auth_cleanup_receipt_id,
        } => {
            assert_eq!(account_id, actor);
            assert!(!retirement_operation_id.is_nil());
            assert!(!auth_cleanup_receipt_id.is_nil());
        }
        _ => panic!("actual source must retain the resource-pending phase"),
    }
    let mut cursor: Option<Uuid> = None;
    loop {
        let page: Value = c
            .query_one("SELECT app.read_deletion_account_page($1,100)", &[&cursor])
            .await?
            .get(0);
        if page["items"].as_array().unwrap().contains(&json!(actor)) {
            break;
        }
        cursor = page["nextCursor"].as_str().map(str::parse).transpose()?;
        assert!(
            cursor.is_some(),
            "resource-pending account must remain discoverable"
        );
    }
    let work: Uuid = jobs[0]["workId"].as_str().unwrap().parse()?;
    let epoch = jobs[0]["epoch"].as_str().unwrap();
    let ack_request = Uuid::new_v4();
    let unknown = acknowledge(&c, actor, work, epoch, "unknown", ack_request).await?;
    assert_eq!(unknown["data"]["disposition"], "unknown");
    assert_eq!(
        acknowledge(&c, actor, work, epoch, "unknown", ack_request).await?,
        unknown
    );
    assert_eq!(
        acknowledge(&c, actor, work, epoch, "absent", ack_request)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        acknowledge(&c, actor, work, "99", "absent", Uuid::new_v4())
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        observe(&c, actor, request).await?["data"]["phase"],
        "finalizing"
    );
    for job in &jobs {
        let work: Uuid = job["workId"].as_str().unwrap().parse()?;
        assert_eq!(
            acknowledge(
                &c,
                actor,
                work,
                job["epoch"].as_str().unwrap(),
                "absent",
                Uuid::new_v4()
            )
            .await?["data"]["disposition"],
            "absent"
        );
    }
    assert_eq!(
        c.query_one("SELECT app.read_account_retirement_work($1)", &[&actor])
            .await?
            .get::<_, Value>(0)["state"],
        "complete"
    );
    assert_eq!(
        observe(&c, actor, request).await?["data"]["phase"],
        "deleted"
    );
    let page: Value = c
        .query_one("SELECT app.read_deletion_account_page(NULL,100)", &[])
        .await?
        .get(0);
    assert!(!page["items"].as_array().unwrap().contains(&json!(actor)));
    assert_eq!(
        acknowledge(&c, actor, work, epoch, "unknown", ack_request).await?,
        unknown
    );
    assert_eq!(
        observe(&c, actor, request).await?["data"]["phase"],
        "deleted"
    );
    assert_eq!(c.execute("UPDATE app.deck SET name='forbidden private label restoration' WHERE id=(SELECT deck_id FROM app.deck_revision WHERE id=$1)", &[&revision]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(c.execute("INSERT INTO app.notification(recipient_id,source_kind,source_ref,body) VALUES($1,'fixture',$2::uuid::text,'forbidden personal restoration')", &[&actor,&Uuid::new_v4()]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn late_minimization_failure_rolls_back_assets_membership_work_and_auth_proof_together()
-> TestResult {
    let c = connection().await?;
    let (i, _, _, member) = terminal_resource_fixture(&c, true).await?;
    let member = member.unwrap();
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    assert_eq!(retire(&c, i.actor).await?["data"]["state"], "retired");
    c.execute("INSERT INTO app.notification(recipient_id,source_kind,source_ref,body) VALUES($1,'fixture',$2::uuid::text,'late forbidden personal restoration')", &[&i.actor,&Uuid::new_v4()]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert!(c.query_one("SELECT state='deletion_pending' AND auth_subject=$2 AND auth_cleanup_subject IS NULL AND retirement_operation_id IS NULL FROM app.account WHERE id=$1", &[&i.actor,&i.subject]).await?.get::<_,bool>(0));
    assert!(c.query_one("SELECT NOT m.active AND e.event_type='oauth.association' FROM app.patreon_membership m JOIN app.provider_event e ON e.id=m.source_event_id WHERE m.id=$1", &[&member]).await?.get::<_,bool>(0));
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE principal_key='account-resources:'||$1::uuid::text OR account_id=$1 AND scope='account_retirement_v1'", &[&i.actor]).await?.get::<_,i64>(0),0);
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.ledger_operation WHERE actor_account_id=$1 AND kind='retirement'", &[&i.actor]).await?.get::<_,i64>(0),0);
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.provider_event WHERE account_id=$1 AND event_type='application.retirement'", &[&i.actor]).await?.get::<_,i64>(0),0);
    assert_eq!(retire(&c, i.actor).await?["data"]["state"], "retired");
    let source: Uuid = c
        .query_one(
            "SELECT source_event_id FROM app.patreon_membership WHERE id=$1",
            &[&member],
        )
        .await?
        .get(0);
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.patreon_membership SET source_event_id=(SELECT id FROM app.provider_event WHERE account_id=$2 AND event_type='oauth.association') WHERE id=$1", &[&member,&i.actor]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT source_event_id FROM app.patreon_membership WHERE id=$1",
            &[&member]
        )
        .await?
        .get::<_, Uuid>(0),
        source
    );
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source) VALUES('account-resources:'||$1::uuid::text,'account_resource_cleanup_v1',$2::uuid::text,$2::uuid::text,repeat('0',64),'pending','',jsonb_build_object('accountId',$1::uuid))", &[&i.actor,&Uuid::new_v4()]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(retire(&c, i.actor).await?["data"]["state"], "retired");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn auth_and_last_resource_acknowledgment_serialize_in_both_orders_without_premature_completion()
-> TestResult {
    for auth_first in [true, false] {
        let c = connection().await?;
        let (actor, subject, request, _, jobs) = resource_fixture(&c).await?;
        for job in &jobs[..jobs.len() - 1] {
            let work: Uuid = job["workId"].as_str().unwrap().parse()?;
            assert_eq!(
                acknowledge(
                    &c,
                    actor,
                    work,
                    job["epoch"].as_str().unwrap(),
                    "absent",
                    Uuid::new_v4()
                )
                .await?["status"],
                "success"
            );
        }
        let final_job = jobs.last().unwrap();
        let work: Uuid = final_job["workId"].as_str().unwrap().parse()?;
        let epoch = final_job["epoch"].as_str().unwrap().to_owned();
        let waiting = connection().await?;
        let pid: i32 = waiting
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        let observer = connection().await?;
        c.batch_execute("BEGIN").await?;
        let task = if auth_first {
            super::auth_identity::canonical_delete(&c, subject).await?;
            tokio::spawn(async move {
                acknowledge(&waiting, actor, work, &epoch, "absent", Uuid::new_v4())
                    .await
                    .map(|_| ())
            })
        } else {
            assert_eq!(
                acknowledge(&c, actor, work, &epoch, "absent", Uuid::new_v4()).await?["status"],
                "success"
            );
            tokio::spawn(async move {
                waiting.batch_execute("BEGIN").await?;
                super::auth_identity::canonical_delete(&waiting, subject).await?;
                waiting.batch_execute("COMMIT").await
            })
        };
        super::wait_for_database_lock(&observer, pid).await?;
        assert_eq!(
            observe(&observer, actor, request).await?["data"]["phase"],
            "finalizing"
        );
        c.batch_execute("COMMIT").await?;
        task.await??;
        assert_eq!(
            observe(&observer, actor, request).await?["data"]["phase"],
            "deleted"
        );
        assert_eq!(
            observer
                .query_one("SELECT app.read_account_retirement_work($1)", &[&actor])
                .await?
                .get::<_, Value>(0)["state"],
            "complete"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn departing_host_resource_set_includes_credentials_sponsored_by_another_active_account()
-> TestResult {
    let c = connection().await?;
    let f = super::room_start_integrity::room(&c, false).await?;
    c.execute(
        "DELETE FROM app.match_seat WHERE match_id=$1",
        &[&f.source.match_id],
    )
    .await?;
    c.execute("DELETE FROM app.match WHERE id=$1", &[&f.source.match_id])
        .await?;
    let old:Uuid=c.query_one("SELECT id FROM app.room_slot WHERE room_id=$1 AND controller_kind='ai' AND retired_at IS NULL", &[&f.id]).await?.get(0);
    let basis = super::room_start_integrity::basis(&c, f.id).await?;
    let removed: Value = c
        .query_one(
            "SELECT app.remove_ai_slot($1,$2,$3::text::numeric,$4)",
            &[&f.source.actor, &old, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(removed["status"], "success");
    let (peer, _, deck, _, _) = super::decks::library(&c).await?;
    c.execute(
        "UPDATE app.deck SET format_id=(SELECT format_id FROM app.room WHERE id=$2) WHERE id=$1",
        &[&deck, &f.id],
    )
    .await?;
    let saved:Value=c.query_one("SELECT app.save_deck_contents($1,$2,NULL,$3,$4)", &[&peer,&deck,&json!([{"definitionId":f.source.definition,"printingId":f.source.printing,"section":"main","quantity":"2"}]),&Uuid::new_v4()]).await?.get(0);
    assert_eq!(saved["status"], "success");
    let revision: Uuid = saved["data"]["resourceId"].as_str().unwrap().parse()?;
    let slot:Uuid=c.query_one("INSERT INTO app.room_slot(room_id,sponsor_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'ai',false) RETURNING id", &[&f.id,&peer]).await?.get(0);
    let peer_grant = super::room_start_integrity::bootstrap_ai(&c, slot, peer, revision).await?;
    assert_eq!(
        deletion::request(&c, f.source.actor, Uuid::new_v4()).await?["status"],
        "success"
    );
    let left: Value = c
        .query_one(
            "SELECT app.cancel_deletion_work($1,'room',$2)",
            &[&f.source.actor, &f.id],
        )
        .await?
        .get(0);
    assert_eq!(left["status"], "success");
    let binding: Uuid = c
        .query_one(
            "SELECT binding_id FROM app.controller_grant WHERE id=$1",
            &[&peer_grant],
        )
        .await?
        .get(0);
    let discovered: Value = c
        .query_one("SELECT app.read_deletion_work($1,100)", &[&f.source.actor])
        .await?
        .get(0);
    let discovered: Vec<spellwake_application::deletion::CleanupWork> =
        serde_json::from_value(discovered)?;
    spellwake_application::deletion::validate_cleanup_work(&discovered, 100)?;
    assert!(discovered.iter().any(|work| work.kind
        == spellwake_application::deletion::CleanupKind::AiStop
        && work.id == binding));
    let requested: Value = c
        .query_one(
            "SELECT app.cancel_deletion_work($1,'ai_stop',$2)",
            &[&f.source.actor, &binding],
        )
        .await?
        .get(0);
    assert_eq!(requested["status"], "success");
    let parsed: spellwake_application::ApiResponse<spellwake_application::deletion::CleanupAck> =
        spellwake_application::decode_wire_object(&serde_json::to_vec(&requested)?)?;
    let original = spellwake_application::deletion::CleanupWork {
        kind: spellwake_application::deletion::CleanupKind::AiStop,
        id: binding,
    };
    assert!(
        matches!(parsed,spellwake_application::ApiResponse::Success{data} if data.matches(&original))
    );
    assert!(c.query_one("SELECT desired_state='stopped' AND observed_state<>'stopped' FROM app.ai_runner_binding WHERE id=$1",&[&binding]).await?.get::<_,bool>(0));
    let replay: Value = c
        .query_one(
            "SELECT app.cancel_deletion_work($1,'ai_stop',$2)",
            &[&f.source.actor, &binding],
        )
        .await?
        .get(0);
    assert_eq!(requested, replay);
    for row in c.query("SELECT b.id,b.binding_epoch::text,b.stop_receipt_id FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE slot.room_id=$1", &[&f.id]).await? {
        let binding:Uuid=row.get(0);let epoch:String=row.get(1);let stop:Uuid=row.get(2);
        let observed:Value=c.query_one("SELECT app.observe_ai_stop($1,$2::text::numeric,$3,'fixture:verified-stopped-generation',$4)", &[&binding,&epoch,&stop,&Uuid::new_v4()]).await?.get(0);assert_eq!(observed["status"],"success");
    }
    let retired = retire(&c, f.source.actor).await?;
    assert_eq!(retired["data"]["state"], "retired", "{retired}");
    let work: Value = c
        .query_one(
            "SELECT app.read_account_resource_work($1,NULL,100)",
            &[&f.source.actor],
        )
        .await?
        .get(0);
    assert_eq!(work["items"].as_array().unwrap().len(), 2);
    assert!(
        work["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|job| job["grantId"] == peer_grant.to_string())
    );
    assert!(
        c.query_one(
            "SELECT state='active' AND auth_subject IS NOT NULL FROM app.account WHERE id=$1",
            &[&peer]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert!(
        c.query_one(
            "SELECT state='revoked' FROM app.controller_grant WHERE id=$1",
            &[&peer_grant]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}
