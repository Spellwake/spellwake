//! Real SQL source observations; fixture references do not qualify native bytes/cards.
use super::*;
use crate::cosmetics;

async fn free_selection(c: &Client, f: &Native) -> FixtureResult<Value> {
    Ok(c.query_one("SELECT jsonb_build_object('backId',p.default_back_id,'sleeveId',p.default_sleeve_id,'diceId',p.default_dice_id,'playmatId',NULL) FROM app.profile p JOIN app.match_seat s ON s.account_id=p.account_id WHERE s.id=$1", &[&f.seat]).await?.get(0))
}
async fn owner_deck(c: &Client, f: &Native) -> FixtureResult<(Uuid, Uuid)> {
    let row=c.query_one("SELECT s.account_id,r.deck_id FROM app.match_seat s JOIN app.deck_revision r ON r.id=s.source_revision_id WHERE s.id=$1", &[&f.seat]).await?;
    Ok((row.get(0), row.get(1)))
}
async fn read_as(
    c: &Client,
    f: &Native,
    subject: Uuid,
    session: Uuid,
    free: &Value,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.read_native_human_presentation($1,$2,$3,$4,$5)",
        &[&subject, &session, &f.game, &f.context, free],
    )
    .await
    .map(|r| r.get(0))
}
async fn read(c: &Client, f: &Native, free: &Value) -> Result<Value, tokio_postgres::Error> {
    read_as(c, f, f.subject, f.session, free).await
}
fn seat_back(value: &Value, seat: Uuid) -> &Value {
    assert_eq!(value["status"], "success", "{value}");
    &value["data"]["presentation"]["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["seatId"] == json!(seat))
        .unwrap()["appearance"]["backId"]
}
async fn choose_back(c: &Client, owner: Uuid, deck: Uuid, back: Uuid) -> TestResult {
    let chosen: Value=c.query_one("SELECT app.change_deck_appearance($1,$2,$3,sleeve_id,dice_id,NULL,$4) FROM app.deck WHERE id=$2", &[&owner,&deck,&back,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(chosen["status"], "success", "{chosen}");
    Ok(())
}
async fn native_counts(c: &Client, f: &Native) -> FixtureResult<Value> {
    Ok(c.query_one("SELECT jsonb_build_object('revision',g.accepted_revision::text,'events',(SELECT count(*) FROM app.game_event WHERE game_id=g.id),'checkpoints',(SELECT count(*) FROM app.game_checkpoint WHERE game_id=g.id),'results',(SELECT count(*) FROM app.game_result WHERE game_id=g.id),'clocks',app.match_clock_snapshot(g.match_id)) FROM app.game g WHERE g.id=$1", &[&f.game]).await?.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_is_read_only_roster_ordered_and_retains_complete_access() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    let before = native_counts(&c, &f).await?;
    let expected:Value=c.query_one("SELECT jsonb_agg(jsonb_build_object('seatId',s.id,'appearance',app.resolve_account_appearance(s.account_id,d.back_id,d.sleeve_id,d.dice_id,d.playmat_id)) ORDER BY s.seat_index) FROM app.match_seat s JOIN app.deck_revision r ON r.id=s.source_revision_id JOIN app.deck d ON d.id=r.deck_id WHERE s.match_id=$1", &[&f.matched]).await?.get(0);
    c.batch_execute("BEGIN READ ONLY; SET LOCAL ROLE service_role")
        .await?;
    let value = read(&c, &f, &free).await?;
    let access: Value = c
        .query_one(
            "SELECT app.read_native_human_access($1,$2,$3,$4)",
            &[&f.subject, &f.session, &f.game, &f.context],
        )
        .await?
        .get(0);
    assert_eq!(
        value,
        json!({"status":"success","data":{"version":"native-presentation-v1","access":access["data"],"presentation":{"seats":expected}}})
    );
    let typed: spellwake_application::player_wire::PlayerPresentation =
        serde_json::from_value(value["data"]["presentation"].clone())?;
    assert_eq!(typed.seats.len(), 2);
    c.batch_execute("COMMIT").await?;
    assert_eq!(native_counts(&c, &f).await?, before);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_uses_mutable_deck_choices_and_union_of_current_grants() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    let (owner, deck) = owner_deck(&c, &f).await?;
    let before = native_counts(&c, &f).await?;
    let paid = cosmetics::cosmetic(&c, "back").await?;
    let first = cosmetics::purchase(&c, owner, paid).await?;
    let second = cosmetics::purchase(&c, owner, paid).await?;
    choose_back(&c, owner, deck, paid).await?;
    assert_eq!(seat_back(&read(&c, &f, &free).await?, f.seat), &json!(paid));
    c.execute(
        "UPDATE app.purchase SET state='refunded' WHERE id=$1",
        &[&first],
    )
    .await?;
    assert_eq!(seat_back(&read(&c, &f, &free).await?, f.seat), &json!(paid));
    c.execute(
        "UPDATE app.purchase SET state='reversed' WHERE id=$1",
        &[&second],
    )
    .await?;
    let replacement: Uuid = c
        .query_one("SELECT app.free_cosmetic($1,'back')", &[&owner])
        .await?
        .get(0);
    assert_ne!(replacement, paid);
    assert_eq!(
        seat_back(&read(&c, &f, &free).await?, f.seat),
        &json!(replacement)
    );
    assert_eq!(native_counts(&c, &f).await?, before);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_scope_session_and_runtime_role_are_independent() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    for role in ["anon", "authenticated", "supabase_auth_admin"] {
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        let error = read(&c, &f, &free).await.unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::INSUFFICIENT_PRIVILEGE));
        c.batch_execute("RESET ROLE").await?;
    }
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(read(&c, &f, &free).await?["status"], "success");
    assert_eq!(
        read_as(&c, &f, f.subject, Uuid::new_v4(), &free).await?,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    let wrong_context = c
        .query_one(
            "SELECT app.read_native_human_presentation($1,$2,$3,$4,$5)",
            &[&f.subject, &f.session, &f.game, &Uuid::new_v4(), &free],
        )
        .await?
        .get::<_, Value>(0);
    assert_eq!(
        wrong_context,
        json!({"status":"fail","data":{"game":"unavailable"}})
    );
    c.batch_execute("RESET ROLE").await?;
    let (_, subject) = crate::account(&c).await?;
    let session: Uuid = c
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0);
    assert_eq!(
        read_as(&c, &f, subject, session, &free).await?,
        json!({"status":"fail","data":{"game":"unavailable"}})
    );
    c.execute("DELETE FROM auth.sessions WHERE id=$1", &[&f.session])
        .await?;
    assert_eq!(
        read(&c, &f, &free).await?,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_rejects_noncanonical_paid_wrong_slot_or_inactive_free_configuration()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    let paid = cosmetics::cosmetic(&c, "back").await?;
    let mut cases = vec![Value::Null, json!([]), json!({})];
    for (key, value) in [
        ("backId", json!(paid)),
        ("backId", free["diceId"].clone()),
        ("diceId", json!(Uuid::nil())),
        ("playmatId", json!(paid)),
        ("extra", json!(true)),
        (
            "backId",
            json!(format!("{{{}}}", free["backId"].as_str().unwrap())),
        ),
    ] {
        let mut bad = free.clone();
        bad[key] = value;
        cases.push(bad);
    }
    for bad in cases {
        let error = read(&c, &f, &bad).await.unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::INVALID_PARAMETER_VALUE));
    }
    let back: Uuid = free["backId"].as_str().unwrap().parse()?;
    c.execute(
        "UPDATE app.cosmetic SET state='inactive' WHERE id=$1",
        &[&back],
    )
    .await?;
    assert_eq!(
        read(&c, &f, &free).await.unwrap_err().code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    c.execute(
        "UPDATE app.cosmetic SET state='active' WHERE id=$1",
        &[&back],
    )
    .await?;
    assert_eq!(read(&c, &f, &free).await?["status"], "success");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_has_one_snapshot_during_real_last_grant_reversal() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    let (owner, deck) = owner_deck(&c, &f).await?;
    let paid = cosmetics::cosmetic(&c, "back").await?;
    let purchase = cosmetics::purchase(&c, owner, paid).await?;
    choose_back(&c, owner, deck, paid).await?;
    let initial = read(&c, &f, &free).await?;
    assert_eq!(seat_back(&initial, f.seat), &json!(paid));
    let pending = blocked_observation(&c, &f, &free).await?;
    c.execute(
        "UPDATE app.purchase SET state='refunded' WHERE id=$1",
        &[&purchase],
    )
    .await?;
    c.batch_execute("COMMIT").await?;
    let old = pending.await??;
    assert_eq!(seat_back(&old, f.seat), &json!(paid));
    let current = read(&c, &f, &free).await?;
    assert_ne!(seat_back(&current, f.seat), &json!(paid));
    assert_eq!(current["data"]["access"], initial["data"]["access"]);
    Ok(())
}

async fn pool_revision(c: &Client, room: &rooms::Room, back: Uuid) -> FixtureResult<Uuid> {
    let deck = Uuid::new_v4();
    let revision = Uuid::new_v4();
    // The canonical deferred invariant requires entries and sealing in one transaction.
    c.batch_execute("BEGIN").await?;
    c.execute("INSERT INTO app.deck(id,source_kind,system_pool_ref,source_manifest_ref,mode,format_id,name,back_id,sleeve_id,dice_id) SELECT $1,'system_pool','fixture:pool:'||$1::uuid::text,'fixture:pool-manifest:'||$1::uuid::text,d.mode,d.format_id,'SQL presentation pool',$3,d.sleeve_id,d.dice_id FROM app.deck d JOIN app.deck_revision r ON r.deck_id=d.id WHERE r.id=$2", &[&deck,&room.source.revision,&back]).await?;
    c.execute("INSERT INTO app.deck_revision(id,deck_id,revision_number,created_at) VALUES($1,$2,1,clock_timestamp())", &[&revision,&deck]).await?;
    c.execute("INSERT INTO app.deck_entry(revision_id,definition_id,printing_id,section,quantity) SELECT $1,definition_id,printing_id,section,quantity FROM app.deck_entry WHERE revision_id=$2", &[&revision,&room.source.revision]).await?;
    c.execute(
        "UPDATE app.deck_revision SET sealed_at=clock_timestamp() WHERE id=$1",
        &[&revision],
    )
    .await?;
    c.execute(
        "UPDATE app.deck SET current_revision_id=$1 WHERE id=$2",
        &[&revision, &deck],
    )
    .await?;
    c.batch_execute("COMMIT").await?;
    Ok(revision)
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_pool_ai_uses_sponsor_entitlements_not_the_viewing_human() -> TestResult {
    let c = connection().await?;
    let paid = cosmetics::cosmetic(&c, "back").await?;
    let room = rooms::room_with_capacity(&c, true, false, 3).await?;
    let pool = pool_revision(&c, &room, paid).await?;
    let slot:Uuid=c.query_one("INSERT INTO app.room_slot(room_id,sponsor_id,deck_revision_id,seat_index,controller_kind,ready) VALUES($1,$2,$3,2,'ai',false) RETURNING id", &[&room.id,&room.source.actor,&pool]).await?.get(0);
    let grant = rooms::bootstrap_ai(&c, slot, room.source.actor, pool).await?;
    let ready:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck-qualified','fixture:authenticated-browser',(SELECT configuration_id FROM app.room WHERE id=$3),$4)", &[&grant,&pool,&room.id,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(ready["status"], "success");
    for (owner, revision) in [
        (room.source.actor, room.source.revision),
        (room.other, room.other_revision),
    ] {
        assert_eq!(
            rooms::ready(&c, &room, owner, revision, true).await?["status"],
            "success"
        );
    }
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&room.id],
    )
    .await?;
    let started = rooms::start(&c, &room, Uuid::new_v4()).await?;
    assert_eq!(started["status"], "success");
    let matched: Uuid = started["data"]["resourceId"].as_str().unwrap().parse()?;
    let f = reserved_native_match(&c, "bo1", &room, matched).await?;
    assert_eq!(
        accept_opening_fixture(&c, &f, &f.opening).await?["status"],
        "success"
    );
    let ai: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND controller_kind='ai'",
            &[&matched],
        )
        .await?
        .get(0);
    let viewer=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1", &[&room.other]).await?;
    let (subject, session) = (viewer.get(0), viewer.get(1));
    let free:Value=c.query_one("SELECT jsonb_build_object('backId',default_back_id,'sleeveId',default_sleeve_id,'diceId',default_dice_id,'playmatId',NULL) FROM app.profile WHERE account_id=$1", &[&room.other]).await?.get(0);
    let sponsor_free: Uuid = c
        .query_one("SELECT app.free_cosmetic($1,'back')", &[&room.source.actor])
        .await?
        .get(0);
    assert_ne!(json!(sponsor_free), free["backId"]);
    cosmetics::purchase(&c, room.other, paid).await?;
    let no_sponsor_grant = read_as(&c, &f, subject, session, &free).await?;
    assert_eq!(
        no_sponsor_grant["data"]["presentation"]["seats"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(seat_back(&no_sponsor_grant, ai), &json!(sponsor_free));
    cosmetics::purchase(&c, room.source.actor, paid).await?;
    assert_eq!(
        seat_back(&read_as(&c, &f, subject, session, &free).await?, ai),
        &json!(paid)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_sponsored_account_deck_resolves_against_its_actual_owner() -> TestResult {
    let c = connection().await?;
    let f = native_with_roster(&c, "bo1", false).await?;
    let free = free_selection(&c, &f).await?;
    let (owner, deck) = owner_deck(&c, &f).await?;
    let ai: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND controller_kind='ai'",
            &[&f.matched],
        )
        .await?
        .get(0);
    let paid = cosmetics::cosmetic(&c, "back").await?;
    cosmetics::purchase(&c, owner, paid).await?;
    choose_back(&c, owner, deck, paid).await?;
    let current = read(&c, &f, &free).await?;
    assert_eq!(seat_back(&current, f.seat), &json!(paid));
    assert_eq!(seat_back(&current, ai), &json!(paid));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_catalogue_changes_do_not_require_a_native_or_grant_revision() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    let (owner, deck) = owner_deck(&c, &f).await?;
    let paid = cosmetics::cosmetic(&c, "back").await?;
    cosmetics::purchase(&c, owner, paid).await?;
    choose_back(&c, owner, deck, paid).await?;
    let before = native_counts(&c, &f).await?;
    let revision: String = c
        .query_one(
            "SELECT appearance_revision::text FROM app.account WHERE id=$1",
            &[&owner],
        )
        .await?
        .get(0);
    assert_eq!(seat_back(&read(&c, &f, &free).await?, f.seat), &json!(paid));
    c.execute(
        "UPDATE app.cosmetic SET state='inactive' WHERE id=$1",
        &[&paid],
    )
    .await?;
    assert_ne!(seat_back(&read(&c, &f, &free).await?, f.seat), &json!(paid));
    c.execute(
        "UPDATE app.cosmetic SET state='active' WHERE id=$1",
        &[&paid],
    )
    .await?;
    assert_eq!(seat_back(&read(&c, &f, &free).await?, f.seat), &json!(paid));
    assert_eq!(
        c.query_one(
            "SELECT appearance_revision::text FROM app.account WHERE id=$1",
            &[&owner]
        )
        .await?
        .get::<_, String>(0),
        revision
    );
    assert_eq!(native_counts(&c, &f).await?, before);
    Ok(())
}

async fn complete_match(c: &Client, f: &Native) -> TestResult {
    let mut frame = candidate(c, f).await?;
    frame["terminal"] = json!({"outcome":"win","winnerSeatIds":[f.seat],"winnersRef":"fixture:SQL-presentation-winner","reason":"fixture:SQL-metadata-only"});
    frame["participation"]=json!(c.query("SELECT id FROM app.match_seat WHERE match_id=$1 AND controller_kind='human' ORDER BY id", &[&f.matched]).await?.iter().map(|r|json!({"seatId":r.get::<_,Uuid>(0),"ownTurns":"0","reason":"fixture:SQL-metadata-only"})).collect::<Vec<_>>());
    let original = json!({"version":"native-host-input-v1","basis":basis(f,"0"),"kind":"native-work","inputRef":artifact(f,"input",90),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
    let accepted: Value = c
        .query_one(
            "SELECT app.commit_native_host_input($1,$2,$3,$4)",
            &[
                &f.game,
                &Uuid::new_v4(),
                &original,
                &json!({"owner":OWNER,"fenceEpoch":"1","candidate":frame}),
            ],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    let row=c.query_one("SELECT r.id,m.revision::text FROM app.game_result r JOIN app.game g ON g.id=r.game_id JOIN app.match m ON m.id=g.match_id WHERE g.id=$1", &[&f.game]).await?;
    let (result, revision): (Uuid, String) = (row.get(0), row.get(1));
    let consumed: Value = c
        .query_one(
            "SELECT app.consume_game_outcome($1,$2::text::numeric)",
            &[&result, &revision],
        )
        .await?
        .get(0);
    assert_eq!(consumed["data"]["phase"], "terminal", "{consumed}");
    // Collection history must drain its event consumer before real retirement.
    let drained: Value = c
        .query_one("SELECT app.consume_quest_game_activity($1,100)", &[&f.game])
        .await?
        .get(0);
    assert_eq!(drained["status"], "success", "{drained}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_deleted_other_participant_gets_only_configured_free_choices() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    let row=c.query_one("SELECT s.id,s.account_id,r.deck_id FROM app.match_seat s JOIN app.deck_revision r ON r.id=s.source_revision_id WHERE s.match_id=$1 AND s.id<>$2", &[&f.matched,&f.seat]).await?;
    let (peer_seat, peer, _deck): (Uuid, Uuid, Uuid) = (row.get(0), row.get(1), row.get(2));
    let original = read(&c, &f, &free).await?;
    assert_ne!(seat_back(&original, peer_seat), &free["backId"]);
    complete_match(&c, &f).await?;
    let requested = crate::deletion::request(&c, peer, Uuid::new_v4()).await?;
    assert_eq!(requested["status"], "success");
    let retired: Value = c
        .query_one("SELECT app.retire_account($1)", &[&peer])
        .await?
        .get(0);
    assert_eq!(retired["data"]["state"], "retired", "{retired}");
    let after = read(&c, &f, &free).await?;
    let selection = &after["data"]["presentation"]["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["seatId"] == json!(peer_seat))
        .unwrap()["appearance"];
    assert_eq!(selection, &free);
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.cosmetic_grant WHERE account_id=$1 AND state='active'",
            &[&peer]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_refuses_foreign_game_and_corrupted_source_or_owner() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let other = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    assert_eq!(
        read_as(&c, &other, f.subject, f.session, &free).await?,
        json!({"status":"fail","data":{"game":"unavailable"}})
    );
    let peer: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND id<>$2",
            &[&f.matched, &f.seat],
        )
        .await?
        .get(0);
    // Administrative corruption controls only. Ordinary writers cannot bypass
    // immutable source/FK guards; every mutation is rolled back before the next.
    for (mutation, refusal) in [
        // Existing native access refuses an unaccepted roster before presentation.
        (
            "UPDATE app.match_seat SET source_revision_id=NULL,deck_snapshot_ref=NULL,deck_accepted_at=NULL WHERE id=$1",
            "game",
        ),
        (
            "DELETE FROM app.deck_revision WHERE id=(SELECT source_revision_id FROM app.match_seat WHERE id=$1)",
            "presentation",
        ),
        (
            "UPDATE app.deck SET account_id=gen_random_uuid() WHERE id=(SELECT r.deck_id FROM app.deck_revision r JOIN app.match_seat s ON s.source_revision_id=r.id WHERE s.id=$1)",
            "presentation",
        ),
        (
            "DELETE FROM app.account WHERE id=(SELECT account_id FROM app.match_seat WHERE id=$1)",
            "presentation",
        ),
    ] {
        c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
            .await?;
        c.execute(mutation, &[&peer]).await?;
        c.batch_execute("SET LOCAL session_replication_role=origin; SET LOCAL ROLE service_role")
            .await?;
        let refused = read(&c, &f, &free).await;
        c.batch_execute("ROLLBACK").await?;
        assert_eq!(
            refused?,
            json!({"status":"fail","data":{(refusal):"unavailable"}}),
            "{mutation}"
        );
        assert_eq!(read(&c, &f, &free).await?["status"], "success");
    }
    Ok(())
}

/// Start the real service reader and prove it is waiting after its statement
/// snapshot exists. Caller owns the lock transaction and must commit/rollback.
async fn blocked_observation(
    c: &Client,
    f: &Native,
    free: &Value,
) -> FixtureResult<tokio::task::JoinHandle<Result<Value, tokio_postgres::Error>>> {
    c.batch_execute("BEGIN; LOCK TABLE app.cosmetic IN ACCESS EXCLUSIVE MODE")
        .await?;
    let reader = connection().await?;
    reader.batch_execute("SET ROLE service_role").await?;
    let pid: i32 = reader
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let (subject, session, game, context, free) =
        (f.subject, f.session, f.game, f.context, free.clone());
    let pending = tokio::spawn(async move {
        reader
            .query_one(
                "SELECT app.read_native_human_presentation($1,$2,$3,$4,$5)",
                &[&subject, &session, &game, &context, &free],
            )
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    crate::wait_for_database_lock(c, pid).await?;
    Ok(pending)
}
async fn both_owners(c: &Client, f: &Native) -> FixtureResult<Vec<(Uuid, Uuid, Uuid)>> {
    Ok(c.query("SELECT s.id,s.account_id,r.deck_id FROM app.match_seat s JOIN app.deck_revision r ON r.id=s.source_revision_id WHERE s.match_id=$1 ORDER BY s.seat_index", &[&f.matched]).await?.iter().map(|r|(r.get(0),r.get(1),r.get(2))).collect())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_header_race_preserves_one_whole_roster_observation() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    let old = cosmetics::cosmetic(&c, "back").await?;
    let new = cosmetics::cosmetic(&c, "back").await?;
    let owners = both_owners(&c, &f).await?;
    for &(_, owner, deck) in &owners {
        cosmetics::purchase(&c, owner, old).await?;
        cosmetics::purchase(&c, owner, new).await?;
        choose_back(&c, owner, deck, old).await?;
    }
    let baseline = read(&c, &f, &free).await?;
    let before = native_counts(&c, &f).await?;
    let pending = blocked_observation(&c, &f, &free).await?;
    for &(_, owner, deck) in &owners {
        choose_back(&c, owner, deck, new).await?;
    }
    c.batch_execute("COMMIT").await?;
    assert_eq!(pending.await??, baseline);
    let current = read(&c, &f, &free).await?;
    for &(seat, _, _) in &owners {
        assert_eq!(seat_back(&current, seat), &json!(new));
    }
    assert_eq!(current["data"]["access"], baseline["data"]["access"]);
    assert_eq!(native_counts(&c, &f).await?, before);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_catalogue_race_refreshes_both_seats_without_a_native_change() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    let paid = cosmetics::cosmetic(&c, "back").await?;
    let owners = both_owners(&c, &f).await?;
    for &(_, owner, deck) in &owners {
        cosmetics::purchase(&c, owner, paid).await?;
        choose_back(&c, owner, deck, paid).await?;
    }
    let baseline = read(&c, &f, &free).await?;
    let before = native_counts(&c, &f).await?;
    let pending = blocked_observation(&c, &f, &free).await?;
    c.execute(
        "UPDATE app.cosmetic SET state='inactive' WHERE id=$1",
        &[&paid],
    )
    .await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(pending.await??, baseline);
    let current = read(&c, &f, &free).await?;
    for &(seat, owner, _) in &owners {
        let replacement: Uuid = c
            .query_one("SELECT app.free_cosmetic($1,'back')", &[&owner])
            .await?
            .get(0);
        assert_eq!(seat_back(&current, seat), &json!(replacement));
        assert_ne!(replacement, paid);
    }
    assert_eq!(current["data"]["access"], baseline["data"]["access"]);
    assert_eq!(native_counts(&c, &f).await?, before);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_other_owner_deletion_race_keeps_old_then_free_observation() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let free = free_selection(&c, &f).await?;
    let (peer_seat, peer, _deck) = both_owners(&c, &f)
        .await?
        .into_iter()
        .find(|(seat, _, _)| *seat != f.seat)
        .unwrap();
    complete_match(&c, &f).await?;
    let baseline = read(&c, &f, &free).await?;
    assert_ne!(seat_back(&baseline, peer_seat), &free["backId"]);
    let before = native_counts(&c, &f).await?;
    let pending = blocked_observation(&c, &f, &free).await?;
    assert_eq!(
        crate::deletion::request(&c, peer, Uuid::new_v4()).await?["status"],
        "success"
    );
    let retired: Value = c
        .query_one("SELECT app.retire_account($1)", &[&peer])
        .await?
        .get(0);
    assert_eq!(retired["data"]["state"], "retired");
    c.batch_execute("COMMIT").await?;
    assert_eq!(pending.await??, baseline);
    let current = read(&c, &f, &free).await?;
    assert_eq!(seat_back(&current, peer_seat), &free["backId"]);
    assert_eq!(current["data"]["access"], baseline["data"]["access"]);
    assert_eq!(native_counts(&c, &f).await?, before);
    Ok(())
}

async fn immutable_match_sources(c: &Client, f: &Native) -> FixtureResult<Value> {
    Ok(c.query_one("SELECT jsonb_build_object('native',app.read_native_game_source($1),'opening',app.read_native_opening_source($1),'match',(SELECT to_jsonb(m) FROM app.match m WHERE m.id=$2),'seats',(SELECT jsonb_agg(to_jsonb(s) ORDER BY s.seat_index) FROM app.match_seat s WHERE s.match_id=$2),'revision',(SELECT to_jsonb(r) FROM app.deck_revision r JOIN app.match_seat s ON s.source_revision_id=r.id WHERE s.id=$3),'cards',(SELECT jsonb_agg(to_jsonb(e) ORDER BY e.id) FROM app.deck_entry e JOIN app.match_seat s ON s.source_revision_id=e.revision_id WHERE s.id=$3))", &[&f.game,&f.matched,&f.seat]).await?.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn presentation_keeps_original_match_across_authenticated_mode_format_and_cosmetic_edits()
-> TestResult {
    let c = connection().await?;
    for case in 0..4 {
        // Independent compatible baselines prove each old mutable-header predicate.
        let f = native(&c, "bo1").await?;
        let free = free_selection(&c, &f).await?;
        let (owner, deck) = owner_deck(&c, &f).await?;
        let old:Value=c.query_one("SELECT jsonb_build_object('name',name,'mode',mode,'formatId',format_id,'appearance',jsonb_build_object('backId',back_id,'sleeveId',sleeve_id,'diceId',dice_id,'playmatId',playmat_id)) FROM app.deck WHERE id=$1", &[&deck]).await?.get(0);
        let format = Uuid::new_v4();
        c.execute("INSERT INTO app.format(id,key,name) VALUES($1,$1::uuid::text,'Presentation metadata fixture')", &[&format]).await?;
        let mut next = old.clone();
        if case != 1 {
            next["mode"] = json!(if old["mode"] == "collection" {
                "open"
            } else {
                "collection"
            });
        }
        if case != 0 {
            next["formatId"] = json!(format);
        }
        if case == 3 {
            let paid = cosmetics::cosmetic(&c, "back").await?;
            cosmetics::purchase(&c, owner, paid).await?;
            next["appearance"]["backId"] = json!(paid);
        }
        let peer=c.query_one("SELECT a.auth_subject,(test_fixture.claims_for_subject(a.auth_subject)->>'session_id')::uuid FROM app.match_seat s JOIN app.account a ON a.id=s.account_id WHERE s.match_id=$1 AND s.id<>$2", &[&f.matched,&f.seat]).await?;
        let (peer_subject, peer_session): (Uuid, Uuid) = (peer.get(0), peer.get(1));
        let baseline = read(&c, &f, &free).await?;
        let peer_baseline = read_as(&c, &f, peer_subject, peer_session, &free).await?;
        let frozen = immutable_match_sources(&c, &f).await?;
        c.batch_execute("BEGIN").await?;
        // Install the exact historical reader transaction-locally. Same function
        // identity retains ACL; a failure/panic rolls the owned transaction back.
        let historical = historical_function(
            include_str!(
                "../../../supabase/migrations/20261106023900_native_current_presentation.sql"
            ),
            "read_native_human_presentation",
        )
        .replacen("CREATE FUNCTION", "CREATE OR REPLACE FUNCTION", 1);
        c.batch_execute(&historical).await?;
        c.batch_execute("SET LOCAL ROLE service_role").await?;
        assert_eq!(read(&c, &f, &free).await?, baseline);
        let changed: Value = c
            .query_one(
                "SELECT app.change_deck_settings_command($1,$2,$3,$4,$5,$6)",
                &[&f.subject, &f.session, &Uuid::new_v4(), &deck, &old, &next],
            )
            .await?
            .get(0);
        assert_eq!(changed["status"], "success", "case{case}: {changed}");
        assert_eq!(
            read(&c, &f, &free).await?,
            json!({"status":"fail","data":{"presentation":"unavailable"}}),
            "old239 case{case}"
        );
        c.batch_execute("RESET ROLE").await?;
        assert_eq!(immutable_match_sources(&c, &f).await?, frozen);
        let current = historical_function(
            include_str!(
                "../../../supabase/migrations/20261107024000_native_presentation_mutable_deck_metadata.sql"
            ),
            "read_native_human_presentation",
        );
        c.batch_execute(current).await?;
        c.batch_execute("SET LOCAL ROLE service_role").await?;
        let after = read(&c, &f, &free).await?;
        let peer_after = read_as(&c, &f, peer_subject, peer_session, &free).await?;
        assert_eq!(after["status"], "success", "new240 case{case}: {after}");
        assert_eq!(after["data"]["access"], baseline["data"]["access"]);
        assert_eq!(
            peer_after["data"]["access"],
            peer_baseline["data"]["access"]
        );
        let mut expected = baseline["data"]["presentation"].clone();
        if case == 3 {
            expected["seats"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|seat| seat["seatId"] == json!(f.seat))
                .unwrap()["appearance"] = next["appearance"].clone();
        }
        assert_eq!(after["data"]["presentation"], expected);
        assert_eq!(peer_after["data"]["presentation"], expected);
        c.batch_execute("RESET ROLE").await?;
        assert_eq!(immutable_match_sources(&c, &f).await?, frozen);
        for role in [
            "anon",
            "authenticated",
            "service_role",
            "supabase_auth_admin",
        ] {
            let allowed:bool=c.query_one("SELECT has_function_privilege($1,'app.read_native_human_presentation(uuid,uuid,uuid,uuid,jsonb)','EXECUTE')", &[&role]).await?.get(0);
            assert_eq!(allowed, role == "service_role");
        }
        c.batch_execute("COMMIT").await?;
    }
    Ok(())
}
