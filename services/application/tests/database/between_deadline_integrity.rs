//! Qualified-shape timing sources, not a selected production duration or native sideboard default.
use super::{
    TestResult, ante_transaction_integrity as ante, connection,
    match_lifecycle_integrity as lifecycle, room_start_integrity::Room,
};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) struct Fixture {
    pub(super) room: Room,
    pub(super) matched: Uuid,
    pub(super) decision: Uuid,
    pub(super) clock: String,
    pub(super) basis: String,
    pub(super) availability: String,
    pub(super) profile: Uuid,
}
pub(super) async fn fixture(c: &Client) -> FixtureResult<Fixture> {
    let (room, matched) = ante::reserve_match(c, true, "bo3").await?;
    let game: Uuid = c
        .query_one(
            "SELECT reserved_game_id FROM app.match WHERE id=$1",
            &[&matched],
        )
        .await?
        .get(0);
    lifecycle::open(c, matched, game).await?;
    let outcome = lifecycle::complete(c, matched, game, vec![]).await?;
    assert_eq!(
        lifecycle::consume(c, matched, outcome).await?["data"]["phase"],
        "between_games"
    );
    let r=c.query_one("SELECT d.id,d.clock_revision::text,m.revision::text,m.availability_revision::text,m.profile_id FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id WHERE m.id=$1 ORDER BY d.id LIMIT 1",&[&matched]).await?;
    Ok(Fixture {
        room,
        matched,
        decision: r.get(0),
        clock: r.get(1),
        basis: r.get(2),
        availability: r.get(3),
        profile: r.get(4),
    })
}
pub(super) async fn due(c: &Client, offset: &str) -> FixtureResult<String> {
    Ok(c.query_one("SELECT to_char((clock_timestamp()+$1::text::interval) AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",&[&offset]).await?.get(0))
}
pub(super) async fn arm(
    c: &Client,
    f: &Fixture,
    due: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let value:Value=c.query_one("SELECT app.schedule_between_game_deadline($1,$2::text::numeric,0,$3::text::numeric,$4::text::numeric,$5,'fixture:qualified-policy',$6::text::timestamptz,'fixture:timing-source',$7)",&[&f.decision,&f.basis,&f.clock,&f.availability,&f.profile,&due,&request]).await?.get(0);
    let original = spellwake_application::deadline_ports::ScheduleDeadline {
        decision_id: f.decision,
        match_revision: f.basis.parse().unwrap(),
        deadline_revision: 0.into(),
        clock_revision: f.clock.parse().unwrap(),
        availability_revision: f.availability.parse().unwrap(),
        profile_id: f.profile,
        policy_ref: "fixture:qualified-policy".parse().unwrap(),
        due_at: due.parse().unwrap(),
        qualification_ref: "fixture:timing-source".parse().unwrap(),
        request_id: request,
    };
    assert!(
        original
            .decode_response(&serde_json::to_vec(&value).unwrap())
            .is_ok()
    );
    Ok(value)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn between_deadlines_start_inactive_and_a_qualified_schedule_is_source_once() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let snapshot: Value = c
        .query_one("SELECT app.between_deadline_snapshot($1)", &[&f.decision])
        .await?
        .get(0);
    assert_eq!(snapshot["revision"], "0");
    for key in [
        "policyRef",
        "dueAt",
        "remainingUs",
        "originReceiptId",
        "sourceReceiptId",
        "availabilityRevision",
        "accountedAt",
    ] {
        assert!(snapshot[key].is_null());
    }
    let request = Uuid::new_v4();
    let time = due(&c, "1 hour").await?;
    let first = arm(&c, &f, &time, request).await?;
    assert_eq!(first["status"], "success");
    assert_eq!(first["data"]["deadlineRevision"], "1");
    assert_eq!(arm(&c, &f, &time, request).await?, first);
    assert_eq!(
        arm(&c, &f, &due(&c, "2 hours").await?, request)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.query_one("SELECT app.check_between_deadline($1)", &[&f.decision])
        .await?;
    assert!(c.query_one("SELECT deadline_revision=1 AND deadline_origin_receipt_id=deadline_source_receipt_id AND deadline_at IS NOT NULL AND deadline_remaining_us IS NULL FROM app.between_game_decision WHERE id=$1",&[&f.decision]).await?.get::<_,bool>(0));
    assert!(arm(&c, &f, "infinity", Uuid::new_v4()).await.is_err());
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deadline_history_rejects_raw_rebase_origin_change_and_orphan_schedule() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let time = due(&c, "1 hour").await?;
    let pending:Uuid=c.query_one("SELECT (app.begin_between_deadline_command('between_deadline_schedule_v1',$1,$2,'{}'::jsonb)).id",&[&f.decision,&Uuid::new_v4()]).await?.get(0);
    // A pending private identity is not an accepted schedule or timer authority.
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.query_one(
        "SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid))",
        &[&pending, &f.decision],
    )
    .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let inserted=c.execute("INSERT INTO app.between_game_decision(match_id,seat_id,decision_kind,basis_revision,clock_revision,state,after_game_id,deadline_at,deadline_policy_ref,deadline_revision,deadline_origin_receipt_id,deadline_source_receipt_id,deadline_availability_revision,deadline_accounted_at) SELECT match_id,seat_id,decision_kind,basis_revision,clock_revision,'pending',after_game_id,clock_timestamp(),'fixture:prearmed',1,$2,$2,0,clock_timestamp() FROM app.between_game_decision WHERE id=$1",&[&f.decision,&pending]).await.unwrap_err();
    assert_eq!(inserted.code(), Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        inserted.as_db_error().unwrap().message(),
        "new between-game decisions start with inactive deadlines"
    );
    let result = arm(&c, &f, &time, Uuid::new_v4()).await?;
    assert_eq!(result["status"], "success");
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.between_game_decision SET deadline_at=deadline_at+interval '1 microsecond',deadline_revision=deadline_revision+1 WHERE id=$1",&[&f.decision]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(c.execute("UPDATE app.between_game_decision SET deadline_policy_ref='fixture:changed',deadline_revision=deadline_revision+1 WHERE id=$1",&[&f.decision]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(c.execute("UPDATE app.between_game_decision SET opened_at=opened_at+interval '1 microsecond' WHERE id=$1",&[&f.decision]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.query_one("SELECT app.check_between_deadline($1)", &[&f.decision])
        .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn answered_deadline_origin_remains_valid_through_peer_reply_and_next_game_opening()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let armed = arm(&c, &f, &due(&c, "1 hour").await?, Uuid::new_v4()).await?;
    assert_eq!(armed["status"], "success");
    let original: Value = c
        .query_one("SELECT app.between_deadline_snapshot($1)", &[&f.decision])
        .await?
        .get(0);
    let decisions=c.query("SELECT d.id,s.source_revision_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.match_id=$1 ORDER BY d.id",&[&f.matched]).await?;
    let mut retained_public = Vec::new();
    for row in decisions {
        let decision: Uuid = row.get(0);
        let revision: Uuid = row.get(1);
        let basis: String = c
            .query_one(
                "SELECT revision::text FROM app.match WHERE id=$1",
                &[&f.matched],
            )
            .await?
            .get(0);
        let request = Uuid::new_v4();
        let reply:Value=c.query_one("SELECT app.reply_between_games($1,$2::text::numeric,$3,'fixture:selection','fixture:qualified',$4)",&[&decision,&basis,&revision,&request]).await?.get(0);
        let original = spellwake_application::deadline_ports::BetweenGameReply {
            decision_id: decision,
            basis: basis.parse()?,
            selected_revision_id: revision,
            reply_ref: "fixture:selection".parse()?,
            qualification_ref: "fixture:qualified".parse()?,
            request_id: request,
        };
        assert!(
            original
                .decode_response(&serde_json::to_vec(&reply)?)
                .is_ok()
        );
        use spellwake_application::qualified_intents::{
            BetweenDecisionOwner, BetweenGameIntent, PreparedBetweenGame,
        };
        let actor: Uuid = c.query_one("SELECT s.account_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.id=$1", &[&decision]).await?.get(0);
        let intent = BetweenGameIntent {
            request_id: request,
            decision_id: decision,
            expected_revision: original.basis.clone(),
            selected_revision_id: revision,
        };
        let owner = BetweenDecisionOwner {
            decision_id: decision,
            account_id: actor,
        };
        let prepared = PreparedBetweenGame::new(&intent, actor, &owner, original)?;
        let public = prepared.decode_response(&serde_json::to_vec(&reply)?)?;
        let disposition = json!({"state":"accepted","response":public});
        prepared.decode_disposition(&intent, actor, &serde_json::to_vec(&disposition)?)?;
        assert!(
            disposition["response"]["data"]
                .get("qualificationRef")
                .is_none()
        );
        retained_public.push((intent, actor, prepared, disposition));
        assert_eq!(reply["status"], "success");
        c.query_one("SELECT app.check_between_deadline($1)", &[&f.decision])
            .await?;
    }
    let game: Uuid = c
        .query_one(
            "SELECT reserved_game_id FROM app.match WHERE id=$1",
            &[&f.matched],
        )
        .await?
        .get(0);
    assert_eq!(
        lifecycle::open(&c, f.matched, game).await?["status"],
        "success"
    );
    for (intent, actor, prepared, original_public) in retained_public {
        let source = prepared.source();
        let replay: Value = c
            .query_one(
                "SELECT app.reply_between_games($1,$2::text::numeric,$3,$4,$5,$6)",
                &[
                    &source.decision_id,
                    &source.basis.to_string(),
                    &source.selected_revision_id,
                    &source.reply_ref.as_str(),
                    &source.qualification_ref.as_str(),
                    &source.request_id,
                ],
            )
            .await?
            .get(0);
        let public = prepared.decode_response(&serde_json::to_vec(&replay)?)?;
        let disposition = json!({"state":"accepted","response":public});
        assert_eq!(disposition, original_public);
        prepared.decode_disposition(&intent, actor, &serde_json::to_vec(&disposition)?)?;
    }
    assert_eq!(
        c.query_one("SELECT app.between_deadline_snapshot($1)", &[&f.decision])
            .await?
            .get::<_, Value>(0),
        original
    );
    c.query_one(
        "SELECT app.check_between_deadline_snapshot($1)",
        &[&original],
    )
    .await?;
    assert!(f.room.source.actor != Uuid::nil());
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deadline_original_jsonb_budgets_match_postgres_at_both_exact_boundaries() -> TestResult {
    use spellwake_application::{
        ApiResponse,
        deadline_ports::{ExpireDeadline, MAX_DEADLINE_TERMS_BYTES, ScheduleDeadline},
    };
    let c = connection().await?;
    let f = fixture(&c).await?;
    let time = due(&c, "0 seconds").await?;
    let request = Uuid::new_v4();
    let mut original = ScheduleDeadline {
        decision_id: f.decision,
        match_revision: f.basis.parse()?,
        deadline_revision: 0.into(),
        clock_revision: f.clock.parse()?,
        availability_revision: f.availability.parse()?,
        profile_id: f.profile,
        policy_ref: "fixture:🂡\n\"\\".parse()?,
        due_at: time.parse()?,
        qualification_ref: "fixture:timing-source".parse()?,
        request_id: request,
    };
    let initial = original.original_terms_octets().unwrap();
    let oracle:i64=c.query_one("SELECT octet_length(jsonb_build_object('decisionId',$1::uuid,'matchRevision',$2::text,'deadlineRevision','0','clockRevision',$3::text,'availabilityRevision',$4::text,'profileId',$5::uuid,'policyRef',$6::text,'dueAt',to_char($7::text::timestamptz AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),'proofRef','fixture:timing-source')::text)::bigint",&[&f.decision,&f.basis,&f.clock,&f.availability,&f.profile,&original.policy_ref.as_str(),&time]).await?.get(0);
    assert_eq!(initial, usize::try_from(oracle)?);
    original.policy_ref = format!(
        "{}{}",
        " ".repeat(MAX_DEADLINE_TERMS_BYTES - initial),
        original.policy_ref.as_str()
    )
    .parse()?;
    assert_eq!(
        original.original_terms_octets(),
        Some(MAX_DEADLINE_TERMS_BYTES)
    );
    let value:Value=c.query_one("SELECT app.schedule_between_game_deadline($1,$2::text::numeric,0,$3::text::numeric,$4::text::numeric,$5,$6,$7::text::timestamptz,'fixture:timing-source',$8)",&[&f.decision,&f.basis,&f.clock,&f.availability,&f.profile,&original.policy_ref.as_str(),&time,&request]).await?.get(0);
    assert!(matches!(
        original.decode_response(&serde_json::to_vec(&value)?)?,
        ApiResponse::Success { .. }
    ));
    let over = format!(" {}", original.policy_ref.as_str());
    assert_eq!(c.query_one("SELECT app.schedule_between_game_deadline($1,$2::text::numeric,0,$3::text::numeric,$4::text::numeric,$5,$6,$7::text::timestamptz,'fixture:timing-source',$8)",&[&f.decision,&f.basis,&f.clock,&f.availability,&f.profile,&over,&time,&Uuid::new_v4()]).await.unwrap_err().code(),Some(&SqlState::INVALID_PARAMETER_VALUE));
    let chosen:Uuid=c.query_one("SELECT s.source_revision_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.id=$1",&[&f.decision]).await?.get(0);
    let mut original = ExpireDeadline {
        decision_id: f.decision,
        deadline_revision: 1.into(),
        selected_revision_id: chosen,
        reply_ref: "fixture:🂡\t\"\\".parse()?,
        qualification_ref: "fixture:expiry-proof".parse()?,
        request_id: Uuid::new_v4(),
    };
    let initial = original.original_terms_octets().unwrap();
    let oracle:i64=c.query_one("SELECT octet_length(jsonb_build_object('decisionId',$1::uuid,'deadlineRevision','1','selectedRevisionId',$2::uuid,'replyRef',$3::text,'proofRef','fixture:expiry-proof')::text)::bigint",&[&f.decision,&chosen,&original.reply_ref.as_str()]).await?.get(0);
    assert_eq!(initial, usize::try_from(oracle)?);
    original.reply_ref = format!(
        "{}{}",
        " ".repeat(MAX_DEADLINE_TERMS_BYTES - initial),
        original.reply_ref.as_str()
    )
    .parse()?;
    assert_eq!(
        original.original_terms_octets(),
        Some(MAX_DEADLINE_TERMS_BYTES)
    );
    let value: Value = c
        .query_one(
            "SELECT app.expire_between_game_deadline($1,1,$2,$3,'fixture:expiry-proof',$4)",
            &[
                &f.decision,
                &chosen,
                &original.reply_ref.as_str(),
                &original.request_id,
            ],
        )
        .await?
        .get(0);
    assert!(matches!(
        original.decode_response(&serde_json::to_vec(&value)?)?,
        ApiResponse::Success { .. }
    ));
    let over = format!(" {}", original.reply_ref.as_str());
    assert_eq!(
        c.query_one(
            "SELECT app.expire_between_game_deadline($1,1,$2,$3,'fixture:expiry-proof',$4)",
            &[&f.decision, &chosen, &over, &Uuid::new_v4()]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    Ok(())
}
