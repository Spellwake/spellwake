//! Actual caller RLS and verified-session facades over normalized fixtures, not native/provider truth.
use super::{TestResult, account, connection, set_auth_claims};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
fn success<T: serde::de::DeserializeOwned>(wire: &Value) -> T {
    match serde_json::from_value::<spellwake_application::ApiResponse<T>>(wire.clone()).unwrap() {
        spellwake_application::ApiResponse::Success { data } => data,
        _ => panic!("expected qualified fixture projection"),
    }
}
async fn identity(c: &Client, actor: Uuid) -> Result<(Uuid, Uuid), tokio_postgres::Error> {
    let row=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1",&[&actor]).await?;
    Ok((row.get(0), row.get(1)))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn human_foundation_match_history_has_authorized_results_clocks_and_only_own_decisions()
-> TestResult {
    let c = connection().await?;
    let f = super::between_deadline_integrity::fixture(&c).await?;
    let owner:Uuid=c.query_one("SELECT s.account_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.id=$1",&[&f.decision]).await?.get(0);
    let (subject, _) = identity(&c, owner).await?;
    let (_, stranger) = account(&c).await?;
    set_auth_claims(&c, subject).await?;
    let detail: Value = c
        .query_one("SELECT app.read_match_metadata($1)", &[&f.matched])
        .await?
        .get(0);
    success::<spellwake_application::match_history::MatchDetail>(&detail)
        .validate(owner, f.matched)?;
    assert_eq!(detail["data"]["accountId"], owner.to_string());
    assert_eq!(detail["data"]["match"]["id"], f.matched.to_string());
    assert!(detail["data"]["ownSeatId"].is_string());
    assert!(detail["data"]["result"].is_null());
    let list: Value = c
        .query_one("SELECT app.read_my_matches(NULL,100)", &[])
        .await?
        .get(0);
    assert!(
        list["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == f.matched.to_string())
    );
    for function in [
        "read_match_seats",
        "read_match_games",
        "read_my_match_decisions",
    ] {
        let page: Value = c
            .query_one(&format!("SELECT app.{function}($1,NULL,1)"), &[&f.matched])
            .await?
            .get(0);
        match function {
            "read_match_seats" => success::<spellwake_application::match_history::SeatPage>(&page)
                .validate(owner, f.matched, None, 1)?,
            "read_match_games" => success::<spellwake_application::match_history::GamePage>(&page)
                .validate(owner, f.matched, None, 1)?,
            "read_my_match_decisions" => {
                success::<spellwake_application::match_history::DecisionPage>(&page)
                    .validate(owner, f.matched, None, 1)?
            }
            _ => unreachable!(),
        }
        assert_eq!(page["status"], "success", "{page}");
        assert_eq!(page["data"]["matchId"], f.matched.to_string());
        assert_eq!(page["data"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["data"]["nextCursor"], page["data"]["items"][0]["id"]);
        let cursor: Uuid = page["data"]["nextCursor"].as_str().unwrap().parse()?;
        let later: Value = c
            .query_one(
                &format!("SELECT app.{function}($1,$2,100)"),
                &[&f.matched, &cursor],
            )
            .await?
            .get(0);
        assert!(
            later["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["id"].as_str().unwrap() > cursor.to_string().as_str())
        );
        for hidden in [
            "snapshotRef",
            "sourceEventId",
            "winnersRef",
            "providerRef",
            "sponsorId",
            "deckSnapshotRef",
            "replyRef",
            "qualificationRef",
            "originReceiptId",
        ] {
            assert!(!page.to_string().contains(hidden), "{function}: {hidden}");
        }
        assert_eq!(
            c.query_one(
                &format!("SELECT app.{function}($1,NULL,101)"),
                &[&f.matched]
            )
            .await?
            .get::<_, Value>(0)["data"]["query"],
            "invalid"
        );
    }
    let games: Value = c
        .query_one("SELECT app.read_match_games($1,NULL,100)", &[&f.matched])
        .await?
        .get(0);
    assert_eq!(games["data"]["items"][0]["result"]["outcome"], "draw");
    assert_eq!(
        games["data"]["items"][0]["result"]["winnerSeatIds"],
        json!([])
    );
    let own: Value = c
        .query_one(
            "SELECT app.read_my_match_decisions($1,NULL,100)",
            &[&f.matched],
        )
        .await?
        .get(0);
    assert_eq!(own["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(own["data"]["items"][0]["id"], f.decision.to_string());
    assert!(own["data"]["items"][0]["deadlineAt"].is_null());
    set_auth_claims(&c, stranger).await?;
    for function in ["read_match_metadata", "read_match_ante"] {
        assert_eq!(
            c.query_one(&format!("SELECT app.{function}($1)"), &[&f.matched])
                .await?
                .get::<_, Value>(0)["data"]["match"],
            "unavailable"
        );
    }
    for function in [
        "read_match_seats",
        "read_match_games",
        "read_my_match_decisions",
    ] {
        assert_eq!(
            c.query_one(
                &format!("SELECT app.{function}($1,NULL,100)"),
                &[&f.matched]
            )
            .await?
            .get::<_, Value>(0)["data"]["match"],
            "unavailable"
        );
    }
    assert_eq!(
        c.query_one("SELECT app.read_my_matches(NULL,100)", &[])
            .await?
            .get::<_, Value>(0)["data"]["items"],
        json!([])
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&owner]).await?;
    set_auth_claims(&c, subject).await?;
    assert_eq!(
        c.query_one("SELECT app.read_match_metadata($1)", &[&f.matched])
            .await?
            .get::<_, Value>(0)["status"],
        "success"
    );
    c.query_one("SELECT set_config('request.jwt.claims','{}',false)", &[])
        .await?;
    assert_eq!(
        c.query_one("SELECT app.read_match_metadata($1)", &[&f.matched])
            .await?
            .get::<_, Value>(0)["data"]["session"],
        "unauthorized"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn human_foundation_competitive_history_uses_owner_rls_and_exact_reward_facts() -> TestResult
{
    let c = connection().await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING",&[]).await?;
    let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(1982,1,'1982-01-01T00:00:00Z','1982-04-01T00:00:00Z','fixture','closed') RETURNING id",&[]).await?.get(0);
    let (result, format, owner, _) =
        super::rank_source_integrity::source(&c, None, 1, "1982-02-01T00:00:00Z").await?;
    assert_eq!(
        super::rank_source_integrity::consume(&c, result).await?["status"],
        "success"
    );
    let (subject, _) = identity(&c, owner).await?;
    let (_, stranger) = account(&c).await?;
    set_auth_claims(&c, subject).await?;
    let credit: Value = c
        .query_one(
            "SELECT app.read_my_competitive_results(NULL,1,$1,$2)",
            &[&format, &season],
        )
        .await?
        .get(0);
    success::<spellwake_application::competitive_history::CreditPage>(&credit).validate(
        owner,
        None,
        1,
        Some(format),
        Some(season),
    )?;
    assert_eq!(credit["data"]["accountId"], owner.to_string());
    assert_eq!(
        credit["data"]["items"][0]["matchResultId"],
        result.to_string()
    );
    assert_eq!(credit["data"]["items"][0]["win"], true);
    assert!(credit["data"]["items"][0]["ratingDelta"].is_string());
    let wins: Value = c
        .query_one("SELECT app.read_my_daily_wins(NULL,100)", &[])
        .await?
        .get(0);
    success::<spellwake_application::competitive_history::DailyWinPage>(&wins)
        .validate(owner, None, 100)?;
    assert_eq!(wins["data"]["items"][0]["utcDay"], "1982-02-01");
    assert_eq!(wins["data"]["items"][0]["wins"], "1");
    assert!(wins["data"]["items"][0]["issuedCoins"].is_string());
    set_auth_claims(&c, stranger).await?;
    assert_eq!(
        c.query_one(
            "SELECT app.read_my_competitive_results(NULL,100,NULL,NULL)",
            &[]
        )
        .await?
        .get::<_, Value>(0)["data"]["items"],
        json!([])
    );
    assert_eq!(
        c.query_one("SELECT app.read_my_daily_wins(NULL,100)", &[])
            .await?
            .get::<_, Value>(0)["data"]["items"],
        json!([])
    );
    Ok(())
}
// This fixture mirrors the exact eight SQL parameters plus connection/function selection.
#[allow(clippy::too_many_arguments)]
async fn ante(
    c: &Client,
    function: &str,
    subject: Uuid,
    session: Uuid,
    request: Uuid,
    kind: &str,
    target: Uuid,
    basis: &str,
    lines: Option<&Value>,
    hash: Option<&str>,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        &format!("SELECT app.{function}($1,$2,$3,$4,$5,$6::text::numeric,$7,$8)"),
        &[
            &subject, &session, &request, &kind, &target, &basis, &lines, &hash,
        ],
    )
    .await
    .map(|r| r.get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn human_foundation_ante_derives_seat_keeps_special_acks_and_recovers_exact_held_terms()
-> TestResult {
    use super::ante_transaction_integrity as source;
    let c = connection().await?;
    let (room, matched) = source::reserve(&c, true).await?;
    let coin = source::fund(&c, room.source.actor).await?;
    source::fund(&c, room.other).await?;
    let (s1, k1) = identity(&c, room.source.actor).await?;
    let (s2, k2) = identity(&c, room.other).await?;
    let (outsider, os) = account(&c).await?;
    let (_, ok) = identity(&c, outsider).await?;
    let lines = json!([{"assetId":coin,"quantity":"100"}]);
    let request = Uuid::new_v4();
    let write = "human_ante_command";
    let read = "read_human_ante_command";
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        ante(
            &c,
            write,
            os,
            ok,
            Uuid::new_v4(),
            "offer",
            matched,
            "0",
            Some(&lines),
            None
        )
        .await?["data"]["match"],
        "unavailable"
    );
    assert_eq!(
        ante(
            &c,
            read,
            s1,
            k1,
            request,
            "offer",
            matched,
            "0",
            Some(&lines),
            None
        )
        .await?["data"]["state"],
        "unknown"
    );
    let unavailable_request = Uuid::new_v4();
    let too_much = json!([{"assetId":coin,"quantity":"1001"}]);
    assert_eq!(
        ante(
            &c,
            write,
            s1,
            k1,
            unavailable_request,
            "offer",
            matched,
            "0",
            Some(&too_much),
            None
        )
        .await?["data"]["assets"],
        "ante bundle unavailable"
    );
    assert_eq!(
        ante(
            &c,
            read,
            s1,
            k1,
            unavailable_request,
            "offer",
            matched,
            "0",
            Some(&too_much),
            None
        )
        .await?["data"]["state"],
        "unknown"
    );
    let offered = ante(
        &c,
        write,
        s1,
        k1,
        request,
        "offer",
        matched,
        "0",
        Some(&lines),
        None,
    )
    .await?;
    assert_eq!(offered["status"], "success", "{offered}");
    assert_eq!(offered["data"]["termsRevision"], "1");
    assert_eq!(offered["data"]["termsHash"].as_str().unwrap().len(), 64);
    let commitment: Uuid = offered["data"]["resourceId"].as_str().unwrap().parse()?;
    assert_eq!(
        ante(
            &c,
            read,
            s1,
            k1,
            request,
            "offer",
            matched,
            "0",
            Some(&lines),
            None
        )
        .await?["data"]["response"],
        offered
    );
    assert_eq!(
        ante(
            &c,
            read,
            s1,
            k1,
            request,
            "offer",
            matched,
            "1",
            Some(&lines),
            None
        )
        .await?["data"]["request"],
        "conflict"
    );
    assert_eq!(
        ante(
            &c,
            read,
            s2,
            k2,
            request,
            "offer",
            matched,
            "0",
            Some(&lines),
            None
        )
        .await?["data"]["state"],
        "unknown"
    );
    let second = ante(
        &c,
        write,
        s2,
        k2,
        Uuid::new_v4(),
        "offer",
        matched,
        "1",
        Some(&lines),
        None,
    )
    .await?;
    assert_eq!(second["data"]["termsRevision"], "2");
    c.batch_execute("RESET ROLE").await?;
    set_auth_claims(&c, s1).await?;
    let projection: Value = c
        .query_one("SELECT app.read_match_ante($1)", &[&matched])
        .await?
        .get(0);
    success::<spellwake_application::ante::AnteDetail>(&projection)
        .validate(room.source.actor, matched)?;
    assert_eq!(
        projection["data"]["agreement"]["termsHash"],
        second["data"]["termsHash"]
    );
    assert_eq!(
        projection["data"]["agreement"]["stakes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        projection["data"]["agreement"]["stakes"][0]["value"]["numerator"],
        "100"
    );
    for hidden in [
        "holdId",
        "exposureId",
        "assessment",
        "qualification",
        "seed",
        "sourceStakeId",
        "operationId",
    ] {
        assert!(!projection.to_string().contains(hidden));
    }
    c.batch_execute("RESET ROLE;SET ROLE service_role").await?;
    let hash = second["data"]["termsHash"].as_str().unwrap();
    let accepted_request = Uuid::new_v4();
    let partial = ante(
        &c,
        write,
        s1,
        k1,
        accepted_request,
        "accept",
        commitment,
        "2",
        None,
        Some(hash),
    )
    .await?;
    assert_eq!(partial["data"]["state"], "offered");
    assert_eq!(partial["data"]["termsRevision"], "2");
    let full = ante(
        &c,
        write,
        s2,
        k2,
        Uuid::new_v4(),
        "accept",
        commitment,
        "2",
        None,
        Some(hash),
    )
    .await?;
    assert_eq!(full["data"]["state"], "committed", "{full}");
    assert_eq!(
        ante(
            &c,
            read,
            s1,
            k1,
            accepted_request,
            "accept",
            commitment,
            "2",
            None,
            Some(hash)
        )
        .await?["data"]["response"],
        partial
    );
    assert_eq!(
        ante(
            &c,
            write,
            s1,
            k1,
            Uuid::new_v4(),
            "cancel",
            commitment,
            "2",
            None,
            None
        )
        .await?["data"]["terms"],
        "ante cannot be canceled at this basis"
    );
    c.batch_execute("RESET ROLE").await?;
    let unchanged: Value = c
        .query_one("SELECT app.ante_terms_manifest($1)", &[&commitment])
        .await?
        .get(0);
    assert!(unchanged.to_string().contains("holdId"));
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.asset_hold WHERE ante_commitment_id=$1 AND state='held'",&[&commitment]).await?.get::<_,i64>(0),2);
    // The normalized existing lifecycle fixture checks terminal display integration only.
    let game: Uuid = c
        .query_one(
            "SELECT reserved_game_id FROM app.match WHERE id=$1",
            &[&matched],
        )
        .await?
        .get(0);
    let winning_seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&matched, &room.source.actor],
        )
        .await?
        .get(0);
    super::match_lifecycle_integrity::open(&c, matched, game).await?;
    let outcome =
        super::match_lifecycle_integrity::complete(&c, matched, game, vec![winning_seat]).await?;
    super::match_lifecycle_integrity::consume(&c, matched, outcome).await?;
    let terminal: Uuid = c
        .query_one(
            "SELECT id FROM app.match_result WHERE match_id=$1",
            &[&matched],
        )
        .await?
        .get(0);
    c.query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &terminal])
        .await?;
    set_auth_claims(&c, s1).await?;
    let settled: Value = c
        .query_one("SELECT app.read_match_ante($1)", &[&matched])
        .await?
        .get(0);
    success::<spellwake_application::ante::AnteDetail>(&settled)
        .validate(room.source.actor, matched)?;
    assert_eq!(settled["data"]["agreement"]["state"], "settled");
    assert!(!settled.to_string().contains("sourceStakeId"));
    let history: Value = c
        .query_one("SELECT app.read_match_metadata($1)", &[&matched])
        .await?
        .get(0);
    success::<spellwake_application::match_history::MatchDetail>(&history)
        .validate(room.source.actor, matched)?;
    assert!(history["data"]["result"].is_object());
    assert!(history["data"]["result"].get("finalGameIds").is_none());
    let games: Value = c
        .query_one("SELECT app.read_match_games($1,NULL,100)", &[&matched])
        .await?
        .get(0);
    success::<spellwake_application::match_history::GamePage>(&games).validate(
        room.source.actor,
        matched,
        None,
        100,
    )?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn human_foundation_ai_intent_redacts_private_start_and_observes_pending_stop_and_unready()
-> TestResult {
    let c = connection().await?;
    let f = super::lobbies::lobby(&c).await?;
    let profile:Uuid=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref) VALUES($1,'fixture','fixture',2,3,'fixture') RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    c.execute(
        "UPDATE app.room SET profile_id=$1 WHERE id=$2",
        &[&profile, &f.room],
    )
    .await?;
    let (subject, session) = identity(&c, f.host).await?;
    let (_, stranger) = account(&c).await?;
    c.batch_execute("SET ROLE service_role").await?;
    let added: Value = c
        .query_one(
            "SELECT app.add_room_ai_slot_command($1,$2,$3,$4,0)",
            &[&subject, &session, &Uuid::new_v4(), &f.room],
        )
        .await?
        .get(0);
    let slot: Uuid = added["data"]["resourceId"].as_str().unwrap().parse()?;
    let request = Uuid::new_v4();
    let lookup = "SELECT app.read_ai_slot_command($1,$2,$3,'bootstrap',$4,1,NULL)";
    let accepted: Value = c
        .query_one(
            "SELECT app.manage_ai_slot_command($1,$2,$3,'bootstrap',$4,1,NULL)",
            &[&subject, &session, &request, &slot],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success", "{accepted}");
    assert_eq!(accepted["data"].as_object().unwrap().len(), 2);
    assert_eq!(accepted["data"]["epoch"], "1");
    assert!(!accepted.to_string().contains("startKey"));
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &slot])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    c.batch_execute("RESET ROLE").await?;
    set_auth_claims(&c, subject).await?;
    let state: Value = c
        .query_one("SELECT app.read_room_ai_status($1,NULL,100)", &[&f.room])
        .await?
        .get(0);
    assert_eq!(
        state["data"]["items"][0]["binding"]["observedState"],
        "pending"
    );
    success::<spellwake_application::room_controls::AiStatusPage>(&state)
        .validate(f.host, f.room, None, 100)?;
    assert_eq!(state["data"]["items"][0]["stopPending"], false);
    set_auth_claims(&c, stranger).await?;
    assert_eq!(
        c.query_one("SELECT app.read_room_ai_status($1,NULL,100)", &[&f.room])
            .await?
            .get::<_, Value>(0)["data"]["room"],
        "unavailable"
    );
    c.batch_execute("RESET ROLE;SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            "SELECT app.manage_ai_slot_command($1,$2,$3,'remove',$4,2,NULL)",
            &[&subject, &session, &Uuid::new_v4(), &slot]
        )
        .await?
        .get::<_, Value>(0)["status"],
        "success"
    );
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &slot])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    c.batch_execute("RESET ROLE").await?;
    set_auth_claims(&c, subject).await?;
    let stopped: Value = c
        .query_one("SELECT app.read_room_ai_status($1,NULL,100)", &[&f.room])
        .await?
        .get(0);
    success::<spellwake_application::room_controls::AiStatusPage>(&stopped)
        .validate(f.host, f.room, None, 100)?;
    assert_eq!(stopped["data"]["items"][0]["isRetired"], true);
    assert_eq!(stopped["data"]["items"][0]["stopPending"], true);
    assert!(stopped["data"]["items"][0]["binding"].is_null());
    for hidden in [
        "startKey",
        "providerRef",
        "authenticationRef",
        "secret",
        "ReceiptId",
    ] {
        assert!(!stopped.to_string().contains(hidden));
    }
    c.batch_execute("RESET ROLE").await?;
    c.execute(
        "UPDATE app.room_slot SET ready=true WHERE room_id=$1 AND account_id=$2",
        &[&f.room, &f.host],
    )
    .await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&f.host]).await?;
    let request = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    let unready: Value = c
        .query_one(
            "SELECT app.unready_room_command($1,$2,$3,$4,3)",
            &[&subject, &session, &request, &f.room],
        )
        .await?
        .get(0);
    assert_eq!(unready["status"], "success", "{unready}");
    assert_eq!(unready["data"]["revision"], "4");
    assert_eq!(
        c.query_one(
            "SELECT app.read_unready_room_command($1,$2,$3,$4,3)",
            &[&subject, &session, &request, &f.room]
        )
        .await?
        .get::<_, Value>(0)["data"]["response"],
        unready
    );
    Ok(())
}
/// Extend the existing current-quarter source fixture; creating another season would collide.
pub(super) async fn verify_supply_ballot_boundaries(
    c: &Client,
    season: Uuid,
    sets: &[Uuid],
) -> TestResult {
    let f = super::game_scope::seed(c).await?;
    let (subject, session) = identity(c, f.account).await?;
    let (stranger, os) = account(c).await?;
    let (_, ok) = identity(c, stranger).await?;
    let approvals = vec![sets[1], sets[0]];
    let request = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            "SELECT app.replace_supply_ballot_command($1,$2,$3,$4,$5)",
            &[&os, &ok, &Uuid::new_v4(), &season, &approvals]
        )
        .await?
        .get::<_, Value>(0)["data"]["account"],
        "qualifying real game required"
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute(
        "UPDATE app.game SET state='completed' WHERE id=$1",
        &[&f.game],
    )
    .await?;
    c.execute("INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision) VALUES($1,$2,$3,3,true,'fixture',clock_timestamp(),0)",&[&f.game,&f.seat,&f.account]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    let accepted: Value = c
        .query_one(
            "SELECT app.replace_supply_ballot_command($1,$2,$3,$4,$5)",
            &[&subject, &session, &request, &season, &approvals],
        )
        .await?
        .get(0);
    assert_eq!(
        accepted,
        json!({"status":"success","data":{"resourceId":season,"approvals":"2"}})
    );
    let lookup = "SELECT app.read_supply_ballot_command($1,$2,$3,$4,$5)";
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &season, &approvals])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    let mut reversed = approvals.clone();
    reversed.reverse();
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &season, &reversed])
            .await?
            .get::<_, Value>(0)["data"]["request"],
        "conflict"
    );
    c.batch_execute("RESET ROLE").await?;
    set_auth_claims(c, subject).await?;
    let page: Value = c
        .query_one("SELECT app.read_supply_ballot($1,NULL,2)", &[&season])
        .await?
        .get(0);
    success::<spellwake_application::supply_ballot::BallotPage>(&page)
        .validate(f.account, season, None, 2)?;
    assert_eq!(page["data"]["accountId"], f.account.to_string());
    assert_eq!(page["data"]["canVote"], true);
    assert_eq!(page["data"]["approvedSetIds"].as_array().unwrap().len(), 2);
    assert!(page["data"]["publication"].is_null());
    for row in page["data"]["items"].as_array().unwrap() {
        assert!(row["finalApprovals"].is_null());
        assert!(row["winner"].is_null());
        assert!(row["dropQuantity"].is_null());
    }
    let cursor: Uuid = page["data"]["nextCursor"].as_str().unwrap().parse()?;
    let more: Value = c
        .query_one(
            "SELECT app.read_supply_ballot($1,$2,100)",
            &[&season, &cursor],
        )
        .await?
        .get(0);
    assert_eq!(more["data"]["items"].as_array().unwrap().len(), 4);
    for hidden in [
        "tieSeed",
        "draw",
        "sourceRef",
        "accountSetHash",
        "products",
        "modifierRef",
        "receipt",
    ] {
        assert!(!page.to_string().contains(hidden));
    }
    set_auth_claims(c, os).await?;
    assert_eq!(
        c.query_one("SELECT app.read_supply_ballot($1,NULL,100)", &[&season])
            .await?
            .get::<_, Value>(0)["data"]["approvedSetIds"],
        json!([])
    );
    c.batch_execute("RESET ROLE;SET ROLE service_role").await?;
    let empty: Vec<Uuid> = vec![];
    assert_eq!(
        c.query_one(
            "SELECT app.replace_supply_ballot_command($1,$2,$3,$4,$5)",
            &[&subject, &session, &Uuid::new_v4(), &season, &empty]
        )
        .await?
        .get::<_, Value>(0)["data"]["approvals"],
        "0"
    );
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn human_foundation_supply_final_projection_strips_private_population_and_tie_sources()
-> TestResult {
    let c = connection().await?;
    let (manifest, _) = super::supply_ballot_integrity::manifest(&c).await?;
    let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(1969,2,'1969-04-01T00:00:00Z','1969-07-01T00:00:00Z','fixture','planned') RETURNING id",&[]).await?.get(0);
    // Privileged historical opening fixture, not evidence of production source selection.
    let receipt:Uuid=c.query_one("INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source) VALUES('supply-season:'||$1::uuid::text,'supply_ballot_open_v1',$1::uuid::text,'initial',app.auth_json_hash($2),'pending','',$2::jsonb||jsonb_build_object('openedAt','1969-01-01T00:00:00.000000Z')) RETURNING id",&[&season,&manifest]).await?.get(0);
    c.query_one(
        "SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid))",
        &[&receipt, &season],
    )
    .await?;
    let finalized: Value = c
        .query_one("SELECT app.finalize_quarterly_supply($1)", &[&season])
        .await?
        .get(0);
    assert_eq!(finalized["data"]["quantity"], "0");
    let (_, subject) = account(&c).await?;
    set_auth_claims(&c, subject).await?;
    let result: Value = c
        .query_one("SELECT app.read_supply_ballot($1,NULL,100)", &[&season])
        .await?
        .get(0);
    let owner: Uuid = result["data"]["accountId"].as_str().unwrap().parse()?;
    success::<spellwake_application::supply_ballot::BallotPage>(&result)
        .validate(owner, season, None, 100)?;
    assert_eq!(result["data"]["isOpen"], false);
    assert_eq!(result["data"]["canVote"], false);
    assert_eq!(result["data"]["publication"]["quantity"], "0");
    assert_eq!(
        result["data"]["publication"]["population"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        result["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["winner"] == true)
            .count(),
        5
    );
    for row in result["data"]["items"].as_array().unwrap() {
        assert_eq!(row["finalApprovals"], "0");
        if row["winner"] == true {
            assert_eq!(row["dropQuantity"], "0");
        } else {
            assert!(row["dropQuantity"].is_null());
        }
    }
    for hidden in [
        "tieSeed",
        "draw",
        "accountSetHash",
        "sourceRef",
        "openingHash",
        "openingReceiptId",
        "allocationRef",
    ] {
        assert!(!result.to_string().contains(hidden));
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn human_foundation_ante_concurrent_participants_preserve_one_terms_basis_and_pending_cancel()
-> TestResult {
    use super::ante_transaction_integrity as source;
    let c = connection().await?;
    let (room, matched) = source::reserve(&c, true).await?;
    let coin = source::fund(&c, room.source.actor).await?;
    source::fund(&c, room.other).await?;
    let (s1, k1) = identity(&c, room.source.actor).await?;
    let (s2, k2) = identity(&c, room.other).await?;
    let left = connection().await?;
    let right = connection().await?;
    left.batch_execute("SET ROLE service_role").await?;
    right.batch_execute("SET ROLE service_role").await?;
    let lines = json!([{"assetId":coin,"quantity":"100"}]);
    let (a, b) = tokio::join!(
        ante(
            &left,
            "human_ante_command",
            s1,
            k1,
            Uuid::new_v4(),
            "offer",
            matched,
            "0",
            Some(&lines),
            None
        ),
        ante(
            &right,
            "human_ante_command",
            s2,
            k2,
            Uuid::new_v4(),
            "offer",
            matched,
            "0",
            Some(&lines),
            None
        )
    );
    let a = a?;
    let b = b?;
    assert_ne!(a["status"], b["status"], "{a} {b}");
    let accepted = if a["status"] == "success" { a } else { b };
    let commitment: Uuid = accepted["data"]["resourceId"].as_str().unwrap().parse()?;
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.asset_hold WHERE ante_commitment_id=$1 AND state='held'",&[&commitment]).await?.get::<_,i64>(0),1);
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&room.source.actor]).await?;
    let request = Uuid::new_v4();
    let canceled = ante(
        &left,
        "human_ante_command",
        s1,
        k1,
        request,
        "cancel",
        commitment,
        "1",
        None,
        None,
    )
    .await?;
    assert_eq!(canceled["data"]["state"], "canceled", "{canceled}");
    assert_eq!(
        ante(
            &left,
            "read_human_ante_command",
            s1,
            k1,
            request,
            "cancel",
            commitment,
            "1",
            None,
            None
        )
        .await?["data"]["response"],
        canceled
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.asset_hold WHERE ante_commitment_id=$1 AND state='held'",&[&commitment]).await?.get::<_,i64>(0),0);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn human_foundation_ante_checks_live_session_after_complete_other_participant_lock_wait()
-> TestResult {
    use super::ante_transaction_integrity as source;
    let c = connection().await?;
    let (room, matched) = source::reserve(&c, true).await?;
    let (subject, session) = identity(&c, room.source.actor).await?;
    let request = Uuid::new_v4();
    c.execute(
        "UPDATE auth.sessions SET not_after=clock_timestamp()+interval '2 seconds' WHERE id=$1",
        &[&session],
    )
    .await?;
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
        &[&room.other],
    )
    .await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    waiting.batch_execute("SET ROLE service_role").await?;
    let task = tokio::spawn(async move {
        ante(
            &waiting,
            "human_ante_command",
            subject,
            session,
            request,
            "offer",
            matched,
            "0",
            Some(&json!([])),
            None,
        )
        .await
    });
    super::wait_for_database_lock(&c, pid).await?;
    assert!(c.query_one("SELECT a.query_start<s.not_after FROM pg_stat_activity a CROSS JOIN auth.sessions s WHERE a.pid=$1 AND s.id=$2",&[&pid,&session]).await?.get::<_,bool>(0));
    c.query_one("SELECT pg_sleep(greatest(0,extract(epoch FROM(not_after-clock_timestamp())))::double precision+0.025) FROM auth.sessions WHERE id=$1",&[&session]).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await??,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.ante_commitment WHERE match_id=$1",
            &[&matched]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE scope='ante_offer_v1' AND request_ref=$1",&[&request.to_string()]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres; metadata access, not native source readiness"]
async fn game_context_locator_is_required_nullable_and_participant_scoped_without_private_source_rows()
-> TestResult {
    use tokio_postgres::error::SqlState;
    let c = connection().await?;
    let (_, matched) = super::ante_transaction_integrity::reserve_match(&c, true, "bo3").await?;
    let game: Uuid = c
        .query_one(
            "SELECT reserved_game_id FROM app.match WHERE id=$1",
            &[&matched],
        )
        .await?
        .get(0);
    let rows=c.query("SELECT s.account_id,a.auth_subject FROM app.match_seat s JOIN app.account a ON a.id=s.account_id WHERE s.match_id=$1 AND s.controller_kind='human' ORDER BY s.seat_index",&[&matched]).await?;
    let participants: Vec<(Uuid, Uuid)> = rows.iter().map(|row| (row.get(0), row.get(1))).collect();
    assert_eq!(participants.len(), 2);
    let (_, stranger) = account(&c).await?;
    set_auth_claims(&c, participants[0].1).await?;
    let before: Value = c
        .query_one("SELECT app.read_match_games($1,NULL,100)", &[&matched])
        .await?
        .get(0);
    assert_eq!(before["status"], "success");
    assert_eq!(before["data"]["items"][0]["id"], json!(game));
    assert!(before["data"]["items"][0].get("contextId").is_some());
    assert!(before["data"]["items"][0]["contextId"].is_null());
    success::<spellwake_application::match_history::GamePage>(&before).validate(
        participants[0].0,
        matched,
        None,
        100,
    )?;
    // Exercise the RPC's query guard independently of HTTP parsing, including
    // nullable SQL inputs. Query rejection precedes match-scope rejection.
    for limit in [None, Some(-1_i32), Some(0), Some(101)] {
        let invalid: Value = c
            .query_one(
                "SELECT app.read_match_games($1,NULL,$2)",
                &[&matched, &limit],
            )
            .await?
            .get(0);
        assert_eq!(invalid, json!({"status":"fail","data":{"query":"invalid"}}));
    }
    let nil = Uuid::nil();
    let invalid_cursor: Value = c
        .query_one("SELECT app.read_match_games($1,$2,100)", &[&matched, &nil])
        .await?
        .get(0);
    assert_eq!(
        invalid_cursor,
        json!({"status":"fail","data":{"query":"invalid"}})
    );
    for scope in [None, Some(nil), Some(Uuid::new_v4())] {
        let unavailable: Value = c
            .query_one("SELECT app.read_match_games($1,NULL,100)", &[&scope])
            .await?
            .get(0);
        assert_eq!(
            unavailable,
            json!({"status":"fail","data":{"match":"unavailable"}})
        );
    }
    let query_first: Value = c
        .query_one("SELECT app.read_match_games(NULL,NULL,0)", &[])
        .await?
        .get(0);
    assert_eq!(
        query_first,
        json!({"status":"fail","data":{"query":"invalid"}})
    );
    c.query_one("SELECT set_config('request.jwt.claims','{}',false)", &[])
        .await?;
    let session_first: Value = c
        .query_one("SELECT app.read_match_games(NULL,NULL,0)", &[])
        .await?
        .get(0);
    assert_eq!(
        session_first,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    c.batch_execute("RESET ROLE").await?;
    let context = Uuid::new_v4();
    assert_ne!(context, game);
    super::match_lifecycle_integrity::open_with_context(&c, matched, game, context).await?;
    let snapshot = async {
        c.query_one("SELECT jsonb_build_object('game',(SELECT to_jsonb(g) FROM app.game g WHERE id=$1),'context',(SELECT to_jsonb(ctx) FROM app.game_context ctx WHERE game_id=$1),'receipts',(SELECT count(*) FROM app.command_receipt WHERE target_ref=($1::uuid)::text))",&[&game]).await.map(|row|row.get::<_,Value>(0))
    };
    let authoritative = snapshot.await?;
    for (owner, subject) in &participants {
        set_auth_claims(&c, *subject).await?;
        let page: Value = c
            .query_one("SELECT app.read_match_games($1,NULL,100)", &[&matched])
            .await?
            .get(0);
        success::<spellwake_application::match_history::GamePage>(&page)
            .validate(*owner, matched, None, 100)?;
        assert_eq!(page["data"]["items"][0]["contextId"], json!(context));
        for hidden in [
            "snapshotRef",
            "snapshot_ref",
            "contextRevision",
            "releaseId",
            "profileId",
            "stateRef",
            "factsRef",
            "programSourceId",
        ] {
            assert!(!page.to_string().contains(hidden), "{hidden}");
        }
        assert_eq!(
            c.query("SELECT id FROM app.game_context", &[])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        c.batch_execute("RESET ROLE").await?;
    }
    set_auth_claims(&c, stranger).await?;
    let denied: Value = c
        .query_one("SELECT app.read_match_games($1,NULL,100)", &[&matched])
        .await?
        .get(0);
    assert_eq!(
        denied,
        json!({"status":"fail","data":{"match":"unavailable"}})
    );
    c.batch_execute("RESET ROLE; SET ROLE anon").await?;
    assert_eq!(
        c.query("SELECT app.read_match_games($1,NULL,100)", &[&matched])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&participants[0].0]).await?;
    set_auth_claims(&c, participants[0].1).await?;
    let closing: Value = c
        .query_one("SELECT app.read_match_games($1,NULL,100)", &[&matched])
        .await?
        .get(0);
    assert_eq!(closing["data"]["items"][0]["contextId"], json!(context));
    c.batch_execute("RESET ROLE").await?;
    let after:Value=c.query_one("SELECT jsonb_build_object('game',(SELECT to_jsonb(g) FROM app.game g WHERE id=$1),'context',(SELECT to_jsonb(ctx) FROM app.game_context ctx WHERE game_id=$1),'receipts',(SELECT count(*) FROM app.command_receipt WHERE target_ref=($1::uuid)::text))",&[&game]).await?.get(0);
    assert_eq!(authoritative, after);
    Ok(())
}
