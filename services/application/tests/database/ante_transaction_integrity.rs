//! Normalized match fixtures exercise ante holds/exposure/consent, not native play or provider qualification.
use super::{TestResult, connection, operation, room_start_integrity, wait_for_database_lock};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) async fn reserve(
    c: &Client,
    humans: bool,
) -> FixtureResult<(room_start_integrity::Room, Uuid)> {
    reserve_match(c, humans, "bo1").await
}
pub(super) async fn reserve_match(
    c: &Client,
    humans: bool,
    structure: &str,
) -> FixtureResult<(room_start_integrity::Room, Uuid)> {
    let f = room_start_integrity::room(c, humans).await?;
    if structure != "bo1" {
        let teams: Vec<Value> = c
            .query(
                "SELECT id FROM app.room_slot WHERE room_id=$1 AND retired_at IS NULL",
                &[&f.id],
            )
            .await?
            .iter()
            .map(|r| json!({"slotId":r.get::<_,Uuid>(0),"teamRef":null}))
            .collect();
        let basis = room_start_integrity::basis(c, f.id).await?;
        let configured: Value = c
            .query_one(
                "SELECT app.configure_room($1,$2,$3::text::numeric,$4,$5,$6,$7)",
                &[
                    &f.source.actor,
                    &f.id,
                    &basis,
                    &f.source.profile,
                    &structure,
                    &json!(teams),
                    &Uuid::new_v4(),
                ],
            )
            .await?
            .get(0);
        assert_eq!(configured["status"], "success");
        // Configuration changes invalidate the AI's earlier Ready confirmation.
        // Reconfirm the selected deck against the new configuration before start.
        for row in c.query("SELECT g.id,b.binding_epoch::text,s.deck_revision_id,r.configuration_id FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.room r ON r.id=s.room_id WHERE r.id=$1 AND g.state='active' AND g.binding_epoch=b.binding_epoch", &[&f.id]).await? {
            let grant: Uuid = row.get(0);
            let epoch: String = row.get(1);
            let revision: Uuid = row.get(2);
            let configuration: Uuid = row.get(3);
            let confirmed: Value = c.query_one("SELECT app.commit_ai_ready_confirmation($1,$2::text::numeric,$3,'fixture:deck-qualified','fixture:authenticated-browser',$4,$5)", &[&grant,&epoch,&revision,&configuration,&Uuid::new_v4()]).await?.get(0);
            assert_eq!(confirmed["status"], "success", "{confirmed}");
        }
    }

    let basis = room_start_integrity::basis(c, f.id).await?;
    let countdown: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:decks',$4)",
            &[&f.source.actor, &f.id, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(countdown["status"], "success", "{countdown}");
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&f.id],
    )
    .await?;
    let started = room_start_integrity::start(c, &f, Uuid::new_v4()).await?;
    assert_eq!(started["status"], "success");
    let m = started["data"]["resourceId"].as_str().unwrap().parse()?;
    Ok((f, m))
}
pub(super) async fn fund(c: &Client, actor: Uuid) -> FixtureResult<Uuid> {
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    let coin: Uuid = c
        .query_one(
            "SELECT id FROM app.asset_definition WHERE currency_key='coin'",
            &[],
        )
        .await?
        .get(0);
    let owner: Uuid = c
        .query_one(
            "SELECT id FROM app.asset_owner WHERE account_id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    let op = operation(c, actor).await?;
    c.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,app.ensure_inventory($2,$3),'credit',1000,'fixture',$1::uuid::text)", &[&op,&owner,&coin]).await?;
    c.execute(
        "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
        &[&op],
    )
    .await?;
    Ok(coin)
}
pub(super) async fn offer(
    c: &Client,
    m: Uuid,
    seat: Uuid,
    basis: &str,
    lines: Value,
    ai: Option<&str>,
) -> FixtureResult<Value> {
    Ok(c.query_one(
        "SELECT app.set_ante_stake($1,$2,$3::text::numeric,$4,$5::text::numeric,$6)",
        &[&m, &seat, &basis, &lines, &ai, &Uuid::new_v4()],
    )
    .await?
    .get(0))
}
pub(super) async fn accept(
    c: &Client,
    commitment: Uuid,
    seat: Uuid,
    basis: &str,
    hash: &str,
) -> FixtureResult<Value> {
    Ok(c.query_one(
        "SELECT app.accept_ante_terms($1,$2,$3::text::numeric,$4,$5)",
        &[&commitment, &seat, &basis, &hash, &Uuid::new_v4()],
    )
    .await?
    .get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn offered_assets_are_held_immediately_changes_clear_consent_and_final_terms_commit_once()
-> TestResult {
    let c = connection().await?;
    let (f, m) = reserve(&c, true).await?;
    let coin = fund(&c, f.source.actor).await?;
    fund(&c, f.other).await?;
    let seats = c
        .query(
            "SELECT id,account_id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index",
            &[&m],
        )
        .await?;
    let a: Uuid = seats[0].get(0);
    let b: Uuid = seats[1].get(0);
    let first = offer(
        &c,
        m,
        a,
        "0",
        json!([{"assetId":coin,"quantity":"100"}]),
        None,
    )
    .await?;
    assert_eq!(first["status"], "success");
    let commitment: Uuid = first["data"]["resourceId"].as_str().unwrap().parse()?;
    let second = offer(
        &c,
        m,
        b,
        "1",
        json!([{"assetId":coin,"quantity":"100"}]),
        None,
    )
    .await?;
    assert_eq!(second["status"], "success");
    let old_hash = second["data"]["termsHash"].as_str().unwrap();
    assert!(
        c.execute(
            "UPDATE app.ante_commitment SET accepted_seat_ids=$2 WHERE id=$1",
            &[&commitment, &vec![a]]
        )
        .await
        .is_err()
    );
    assert_eq!(
        c.query_one(
            "SELECT cardinality(accepted_seat_ids) FROM app.ante_commitment WHERE id=$1",
            &[&commitment]
        )
        .await?
        .get::<_, i32>(0),
        0
    );
    let legacy:Uuid=c.query_one("INSERT INTO app.ante_commitment(match_id,terms_revision,state) VALUES($1,0,'offered') RETURNING id", &[&f.source.match_id]).await?.get(0);
    let owner: Uuid = c
        .query_one(
            "SELECT id FROM app.asset_owner WHERE account_id=$1",
            &[&f.source.actor],
        )
        .await?
        .get(0);
    let op = operation(&c, f.source.actor).await?;
    let other_hold:Uuid=c.query_one("INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,'ante',$2::uuid::text,'held',$3) RETURNING id", &[&owner,&legacy,&op]).await?.get(0);
    c.execute("INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) SELECT $1,id,100 FROM app.inventory_asset WHERE owner_id=$2 AND asset_definition_id=$3", &[&other_hold,&owner,&coin]).await?;
    c.execute(
        "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
        &[&op],
    )
    .await?;
    assert_eq!(c.execute("UPDATE app.ante_stake SET commitment_id=$2,match_seat_id=$3,hold_id=$4 WHERE commitment_id=$1 AND match_seat_id=$5", &[&commitment,&legacy,&f.source.human,&other_hold,&a]).await.unwrap_err().code(),Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION));
    c.execute(
        "UPDATE app.asset_hold SET state='released' WHERE id=$1",
        &[&other_hold],
    )
    .await?;

    assert_eq!(
        accept(&c, commitment, a, "2", old_hash).await?["data"]["state"],
        "offered"
    );
    let edited = offer(
        &c,
        m,
        a,
        "2",
        json!([{"assetId":coin,"quantity":"120"}]),
        None,
    )
    .await?;
    assert_eq!(edited["status"], "success");
    assert_eq!(
        c.query_one(
            "SELECT cardinality(accepted_seat_ids) FROM app.ante_commitment WHERE id=$1",
            &[&commitment]
        )
        .await?
        .get::<_, i32>(0),
        0
    );
    assert_eq!(
        accept(&c, commitment, b, "2", old_hash).await?["status"],
        "fail"
    );
    let hash = edited["data"]["termsHash"].as_str().unwrap();
    accept(&c, commitment, a, "3", hash).await?;
    let committed = accept(&c, commitment, b, "3", hash).await?;
    assert_eq!(committed["data"]["state"], "committed");
    assert_eq!(c.query_one("SELECT sum(line.quantity)::text FROM app.ante_stake s JOIN app.asset_hold hold ON hold.id=s.hold_id JOIN app.hold_line line ON line.hold_id=hold.id WHERE s.commitment_id=$1 AND hold.state='held'", &[&commitment]).await?.get::<_,String>(0),"220");
    assert_eq!(
        offer(&c, m, a, "3", json!([]), None).await?["status"],
        "fail"
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.ledger_operation WHERE source_scope='ante_commit_v1' AND source_ref=$1", &[&commitment.to_string()]).await?.get::<_,i64>(0),1);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn ai_offers_reuse_one_start_day_exposure_when_decreased_withdrawn_and_increased()
-> TestResult {
    let c = connection().await?;
    let (f, m) = reserve(&c, false).await?;
    let coin = fund(&c, f.source.actor).await?;
    let human: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND controller_kind='human'",
            &[&m],
        )
        .await?
        .get(0);
    let ai: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND controller_kind='ai'",
            &[&m],
        )
        .await?
        .get(0);
    assert_eq!(
        offer(
            &c,
            m,
            human,
            "0",
            json!([{"assetId":coin,"quantity":"100"}]),
            None
        )
        .await?["status"],
        "success"
    );
    for (basis, amount) in [("1", "100"), ("2", "50"), ("3", "0"), ("4", "300")] {
        assert_eq!(
            offer(&c, m, ai, basis, json!([]), Some(amount)).await?["status"],
            "success"
        );
    }
    let row=c.query_one("SELECT c.id,c.terms_hash,b.utc_day=to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD'),x.reserved_coins::text,b.lost_coins::text FROM app.ante_commitment c JOIN app.match m ON m.id=c.match_id JOIN app.ai_loss_exposure x ON x.commitment_id=c.id JOIN app.ai_loss_bucket b ON b.id=x.bucket_id WHERE c.match_id=$1", &[&m]).await?;
    let commitment: Uuid = row.get(0);
    let hash: String = row.get(1);
    assert!(row.get::<_, bool>(2));
    assert_eq!(row.get::<_, String>(3), "300");
    assert_eq!(row.get::<_, String>(4), "0");
    accept(&c, commitment, human, "5", &hash).await?;
    assert_eq!(
        accept(&c, commitment, ai, "5", &hash).await?["data"]["state"],
        "committed"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn terminal_human_transfers_ai_issuance_ai_retirement_and_draw_refunds_never_settle_twice()
-> TestResult {
    for case in ["human_transfer", "ai_loss", "ai_win", "draw"] {
        let c = connection().await?;
        let humans = case == "human_transfer";
        let (f, m) = reserve(&c, humans).await?;
        let coin = fund(&c, f.source.actor).await?;
        if humans {
            fund(&c, f.other).await?;
        }
        let rows=c.query("SELECT id,controller_kind FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index", &[&m]).await?;
        let a: Uuid = rows[0].get(0);
        let b: Uuid = rows[1].get(0);
        offer(
            &c,
            m,
            a,
            "0",
            json!([{"assetId":coin,"quantity":"100"}]),
            None,
        )
        .await?;
        let offered = if humans {
            offer(
                &c,
                m,
                b,
                "1",
                json!([{"assetId":coin,"quantity":"100"}]),
                None,
            )
            .await?
        } else {
            offer(&c, m, b, "1", json!([]), Some("300")).await?
        };
        let commitment: Uuid = offered["data"]["resourceId"].as_str().unwrap().parse()?;
        let hash = offered["data"]["termsHash"].as_str().unwrap();
        accept(&c, commitment, a, "2", hash).await?;
        assert_eq!(
            accept(&c, commitment, b, "2", hash).await?["data"]["state"],
            "committed"
        );
        let game: Uuid = c
            .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
            .await?
            .get(0);
        assert_eq!(
            super::match_lifecycle_integrity::open(&c, m, game).await?["status"],
            "success"
        );
        let outcome = super::match_lifecycle_integrity::complete(
            &c,
            m,
            game,
            if case == "draw" {
                vec![]
            } else if case == "ai_win" {
                vec![b]
            } else {
                vec![a]
            },
        )
        .await?;
        let consumed = super::match_lifecycle_integrity::consume(&c, m, outcome).await?;
        let result: Uuid = consumed["data"]["terminalResultId"]
            .as_str()
            .unwrap()
            .parse()?;
        if case == "ai_loss" {
            c.batch_execute("BEGIN").await?;
            c.query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &result])
                .await?;
            c.batch_execute("ROLLBACK").await?;
            assert_eq!(
                c.query_one(
                    "SELECT state FROM app.ante_commitment WHERE id=$1",
                    &[&commitment]
                )
                .await?
                .get::<_, String>(0),
                "committed"
            );
            assert_eq!(c.query_one("SELECT b.lost_coins::text FROM app.ai_loss_bucket b JOIN app.ai_loss_exposure x ON x.bucket_id=b.id WHERE x.commitment_id=$1", &[&commitment]).await?.get::<_,String>(0),"0");
        }
        let settled: Value = if case == "human_transfer" {
            let waiting = connection().await?;
            let pid: i32 = waiting
                .query_one("SELECT pg_backend_pid()", &[])
                .await?
                .get(0);
            c.batch_execute("BEGIN").await?;
            let first: Value = c
                .query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &result])
                .await?
                .get(0);
            let task = tokio::spawn(async move {
                waiting
                    .query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &result])
                    .await
                    .map(|row| row.get::<_, Value>(0))
            });
            wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??, first);
            first
        } else {
            c.query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &result])
                .await?
                .get(0)
        };
        assert_eq!(settled["status"], "success");
        let changed_result = Uuid::new_v4();
        let error = c
            .query_one(
                "SELECT app.settle_ante($1,$2)",
                &[&commitment, &changed_result],
            )
            .await
            .unwrap_err();
        let cause = error
            .as_db_error()
            .expect("canonical settlement retarget refusal");
        assert_eq!(
            cause.code(),
            &tokio_postgres::error::SqlState::CHECK_VIOLATION
        );
        assert_eq!(cause.message(), "ante settlement source was retargeted");
        let expected_kind = if case == "draw" {
            spellwake_application::ante::AnteState::Refunded
        } else {
            spellwake_application::ante::AnteState::Settled
        };
        assert!(
            spellwake_application::terminal_consumers::AnteSettlement {
                commitment_id: commitment,
                result_id: result
            }
            .decode_response(settled.to_string().as_bytes(), expected_kind)
            .is_ok()
        );
        assert_eq!(
            c.query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &result])
                .await?
                .get::<_, Value>(0),
            settled
        );
        let expected = match case {
            "human_transfer" => "1100",
            "ai_loss" => "1300",
            "ai_win" => "900",
            _ => "1000",
        };
        assert_eq!(c.query_one("SELECT i.owned_quantity::text FROM app.inventory_asset i JOIN app.asset_owner owner ON owner.id=i.owner_id WHERE owner.account_id=$1 AND i.asset_definition_id=$2", &[&f.source.actor,&coin]).await?.get::<_,String>(0),expected,"{case}");
        if !humans {
            let row=c.query_one("SELECT b.lost_coins::text,x.loss_coins::text,x.state FROM app.ai_loss_exposure x JOIN app.ai_loss_bucket b ON b.id=x.bucket_id WHERE x.commitment_id=$1", &[&commitment]).await?;
            assert_eq!(
                row.get::<_, String>(0),
                if case == "ai_loss" { "300" } else { "0" }
            );
            assert_eq!(
                row.get::<_, String>(1),
                if case == "ai_loss" { "300" } else { "0" }
            );
        }
        assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.ledger_operation WHERE source_scope='ante_settlement_v1' AND source_ref=$1", &[&commitment.to_string()]).await?.get::<_,i64>(0),1);
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn restricted_ante_compares_exact_pack_values_without_rounding_a_fractional_card()
-> TestResult {
    let c = connection().await?;
    let (f, m) = reserve(&c, true).await?;
    let coin = fund(&c, f.source.actor).await?;
    fund(&c, f.other).await?;
    let product = super::pricing::product(&c, true).await?;
    super::pricing::publish(
        &c,
        product.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let owner: Uuid = c
        .query_one(
            "SELECT id FROM app.asset_owner WHERE account_id=$1",
            &[&f.source.actor],
        )
        .await?
        .get(0);
    let op = operation(&c, f.source.actor).await?;
    c.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,app.ensure_inventory($2,$3),'credit',1,'fixture',$1::uuid::text)", &[&op,&owner,&product.cards[0]]).await?;
    c.execute(
        "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
        &[&op],
    )
    .await?;
    let rows = c
        .query(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index",
            &[&m],
        )
        .await?;
    let a: Uuid = rows[0].get(0);
    let b: Uuid = rows[1].get(0);
    offer(
        &c,
        m,
        a,
        "0",
        json!([{"assetId":product.cards[0],"quantity":"1"}]),
        None,
    )
    .await?;
    let offered = offer(
        &c,
        m,
        b,
        "1",
        json!([{"assetId":coin,"quantity":"512"}]),
        None,
    )
    .await?;
    let commitment: Uuid = offered["data"]["resourceId"].as_str().unwrap().parse()?;
    let hash = offered["data"]["termsHash"].as_str().unwrap();
    accept(&c, commitment, a, "2", hash).await?;
    assert_eq!(
        accept(&c, commitment, b, "2", hash).await?["status"],
        "fail"
    );
    assert_eq!(
        c.query_one(
            "SELECT cardinality(accepted_seat_ids) FROM app.ante_commitment WHERE id=$1",
            &[&commitment]
        )
        .await?
        .get::<_, i32>(0),
        1
    );
    let updated = offer(
        &c,
        m,
        b,
        "2",
        json!([{"assetId":coin,"quantity":"513"}]),
        None,
    )
    .await?;
    let hash = updated["data"]["termsHash"].as_str().unwrap();
    accept(&c, commitment, a, "3", hash).await?;
    assert_eq!(
        accept(&c, commitment, b, "3", hash).await?["data"]["state"],
        "committed"
    );
    let value=c.query_one("SELECT value_numerator::text,value_denominator::text,estimated_value_coins::text FROM app.ante_stake WHERE commitment_id=$1 AND match_seat_id=$2", &[&commitment,&a]).await?;
    assert_eq!(value.get::<_, String>(0), "2000");
    assert_eq!(value.get::<_, String>(1), "3");
    assert_eq!(value.get::<_, String>(2), "667");
    Ok(())
}

struct ThreeRoom {
    room: room_start_integrity::Room,
    m: Uuid,
    game: Uuid,
    seats: [Uuid; 3],
    coin: Uuid,
}
async fn reserve_three(
    c: &Client,
    human_team: &str,
    first_ai_team: &str,
    second_ai_team: &str,
) -> FixtureResult<ThreeRoom> {
    let f = room_start_integrity::room(c, false).await?;
    let coin = fund(c, f.source.actor).await?;
    let profile:Uuid=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref) VALUES($1,'fixture','fixture',2,3,'fixture:team3') RETURNING id", &[&Uuid::new_v4().to_string()]).await?.get(0);
    let teams:Vec<Value>=c.query("SELECT id,controller_kind FROM app.room_slot WHERE room_id=$1", &[&f.id]).await?.iter().map(|r|json!({"slotId":r.get::<_,Uuid>(0),"teamRef":if r.get::<_,String>(1)=="human"{human_team}else{first_ai_team}})).collect();
    let basis = room_start_integrity::basis(c, f.id).await?;
    let configured: Value = c
        .query_one(
            "SELECT app.configure_room($1,$2,$3::text::numeric,$4,'bo1',$5,$6)",
            &[
                &f.source.actor,
                &f.id,
                &basis,
                &profile,
                &json!(teams),
                &Uuid::new_v4(),
            ],
        )
        .await?
        .get(0);
    assert_eq!(configured["status"], "success");
    let slot:Uuid=c.query_one("INSERT INTO app.room_slot(room_id,sponsor_id,seat_index,controller_kind,deck_revision_id,team_ref,ready) VALUES($1,$2,2,'ai',$3,$4,false) RETURNING id", &[&f.id,&f.source.actor,&f.source.revision,&second_ai_team]).await?.get(0);
    let _grant =
        room_start_integrity::bootstrap_ai(c, slot, f.source.actor, f.source.revision).await?;
    for row in c.query("SELECT g.id,s.deck_revision_id FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1 AND g.state='active'", &[&f.id]).await? {
 let grant:Uuid=row.get(0);let revision:Uuid=row.get(1);let confirmed:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck','fixture:authenticated',(SELECT configuration_id FROM app.room WHERE id=$3),$4)", &[&grant,&revision,&f.id,&Uuid::new_v4()]).await?.get(0);assert_eq!(confirmed["status"],"success");
 }
    let basis = room_start_integrity::basis(c, f.id).await?;
    let countdown: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:decks',$4)",
            &[&f.source.actor, &f.id, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(countdown["status"], "success");
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&f.id],
    )
    .await?;
    let started = room_start_integrity::start(c, &f, Uuid::new_v4()).await?;
    assert_eq!(started["status"], "success");
    let m: Uuid = started["data"]["resourceId"].as_str().unwrap().parse()?;
    let game: Uuid = started["data"]["gameId"].as_str().unwrap().parse()?;
    let rows = c
        .query(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index",
            &[&m],
        )
        .await?;
    let human: Uuid = rows[0].get(0);
    let losing: Uuid = rows[1].get(0);
    let winning: Uuid = rows[2].get(0);
    Ok(ThreeRoom {
        room: f,
        m,
        game,
        seats: [human, losing, winning],
        coin,
    })
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn losing_ai_300_splits_150_to_each_human_ai_teammate_and_charges_the_full_allowance()
-> TestResult {
    let c = connection().await?;
    let ThreeRoom {
        room: f,
        m,
        game,
        seats: [human, losing, winning],
        coin,
    } = reserve_three(&c, "red", "blue", "red").await?;
    offer(
        &c,
        m,
        human,
        "0",
        json!([{"assetId":coin,"quantity":"100"}]),
        None,
    )
    .await?;
    let offered = offer(&c, m, losing, "1", json!([]), Some("300")).await?;
    let commitment: Uuid = offered["data"]["resourceId"].as_str().unwrap().parse()?;
    let hash = offered["data"]["termsHash"].as_str().unwrap();
    for seat in [human, losing, winning] {
        assert_eq!(
            accept(&c, commitment, seat, "2", hash).await?["status"],
            "success"
        );
    }
    assert_eq!(
        super::match_lifecycle_integrity::open(&c, m, game).await?["status"],
        "success"
    );
    let outcome =
        super::match_lifecycle_integrity::complete(&c, m, game, vec![human, winning]).await?;
    let consumed = super::match_lifecycle_integrity::consume(&c, m, outcome).await?;
    let result: Uuid = consumed["data"]["terminalResultId"]
        .as_str()
        .unwrap()
        .parse()?;
    let settled: Value = c
        .query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &result])
        .await?
        .get(0);
    assert_eq!(settled["status"], "success");
    assert!(
        spellwake_application::terminal_consumers::AnteSettlement {
            commitment_id: commitment,
            result_id: result
        }
        .decode_response(
            settled.to_string().as_bytes(),
            spellwake_application::ante::AnteState::Settled
        )
        .is_ok()
    );
    assert_eq!(c.query_one("SELECT b.lost_coins::text FROM app.ai_loss_bucket b JOIN app.ai_loss_exposure x ON x.bucket_id=b.id WHERE x.commitment_id=$1", &[&commitment]).await?.get::<_,String>(0),"300");
    assert_eq!(c.query_one("SELECT i.owned_quantity::text FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id WHERE o.account_id=$1 AND i.asset_definition_id=$2", &[&f.source.actor,&coin]).await?.get::<_,String>(0),"1150");
    let shares=c.query("SELECT value->>'classification',sum((value->>'quantity')::numeric)::text FROM app.ante_commitment c CROSS JOIN LATERAL jsonb_array_elements(c.settlement->'allocations') WHERE c.id=$1 AND value->>'classification' IN ('ai_issue','ai_unissued') GROUP BY value->>'classification' ORDER BY value->>'classification'", &[&commitment]).await?;
    assert_eq!(shares.len(), 2);
    for row in shares {
        assert_eq!(row.get::<_, String>(1), "150");
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn whole_unit_allocation_offsets_existing_card_value_and_batches_huge_coin_quantities()
-> TestResult {
    let c = connection().await?;
    c.batch_execute("SET statement_timeout='5s'").await?;
    let winners = vec![Uuid::new_v4(), Uuid::new_v4()];
    let seed = "a".repeat(64);
    let stake = Uuid::new_v4();
    let asset = Uuid::new_v4();
    let balances =
        json!([{"numerator":"100","denominator":"1"},{"numerator":"0","denominator":"1"}]);
    let result: Value = c
        .query_one(
            "SELECT app.split_ante_asset($1,$2,100,1,1,$3,$4,$5)",
            &[&winners, &balances, &seed, &stake, &asset],
        )
        .await?
        .get(0);
    assert_eq!(
        result["balances"],
        json!([{"numerator":"100","denominator":"1"},{"numerator":"100","denominator":"1"}])
    );
    assert_eq!(
        result["allocations"],
        json!([{"recipientSeatId":winners[1],"quantity":"100"}])
    );
    let zero = json!([{"numerator":"0","denominator":"1"},{"numerator":"0","denominator":"1"}]);
    let large:Value=c.query_one("SELECT app.split_ante_asset($1,$2,100000000000000000000000000000000000000000000000000,1,1,$3,$4,$5)", &[&winners,&zero,&seed,&stake,&asset]).await?.get(0);
    assert_eq!(
        large["allocations"][0]["quantity"],
        "50000000000000000000000000000000000000000000000000"
    );
    assert_eq!(
        large["allocations"][1]["quantity"],
        large["allocations"][0]["quantity"]
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn mixed_opposing_card_and_coins_balance_by_value_regardless_of_asset_uuid_order()
-> TestResult {
    let c = connection().await?;
    let ThreeRoom {
        room: f,
        m,
        game,
        seats: [human, a, b],
        coin,
    } = reserve_three(&c, "blue", "red", "red").await?;
    let product = super::pricing::product(&c, true).await?;
    super::pricing::publish(
        &c,
        product.recipe,
        (4, 4),
        (3, 20),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let card = product.cards[0];
    let owner: Uuid = c
        .query_one(
            "SELECT id FROM app.asset_owner WHERE account_id=$1",
            &[&f.source.actor],
        )
        .await?
        .get(0);
    let op = operation(&c, f.source.actor).await?;
    c.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,app.ensure_inventory($2,$3),'credit',1,'fixture',$1::uuid::text)", &[&op,&owner,&card]).await?;
    c.execute(
        "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
        &[&op],
    )
    .await?;
    offer(
        &c,
        m,
        human,
        "0",
        json!([{"assetId":coin,"quantity":"100"},{"assetId":card,"quantity":"1"}]),
        None,
    )
    .await?;
    offer(&c, m, a, "1", json!([]), Some("200")).await?;
    let offered = offer(&c, m, b, "2", json!([]), Some("100")).await?;
    let commitment: Uuid = offered["data"]["resourceId"].as_str().unwrap().parse()?;
    let hash = offered["data"]["termsHash"].as_str().unwrap();
    for seat in [human, a, b] {
        assert_eq!(
            accept(&c, commitment, seat, "3", hash).await?["status"],
            "success"
        );
    }
    super::match_lifecycle_integrity::open(&c, m, game).await?;
    let outcome = super::match_lifecycle_integrity::complete(&c, m, game, vec![a, b]).await?;
    let terminal = super::match_lifecycle_integrity::consume(&c, m, outcome).await?;
    let result: Uuid = terminal["data"]["terminalResultId"]
        .as_str()
        .unwrap()
        .parse()?;
    c.query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &result])
        .await?;
    let values=c.query("SELECT value->>'recipientSeatId',sum((value->>'quantity')::numeric*CASE WHEN value->>'sourceAssetId'=$2::uuid::text THEN 100 ELSE 1 END)::text FROM app.ante_commitment c CROSS JOIN LATERAL jsonb_array_elements(c.settlement->'allocations') WHERE c.id=$1 AND value->>'classification'='retire' GROUP BY value->>'recipientSeatId'", &[&commitment,&card]).await?;
    assert_eq!(values.len(), 2);
    for row in values {
        assert_eq!(row.get::<_, String>(1), "100");
    }
    assert_eq!(c.query_one("SELECT b.lost_coins::text FROM app.ai_loss_bucket b JOIN app.ai_loss_exposure x ON x.bucket_id=b.id WHERE x.commitment_id=$1", &[&commitment]).await?.get::<_,String>(0),"0");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pending_account_can_cancel_existing_ante_and_release_holds_and_loss_exposure_once()
-> TestResult {
    let c = connection().await?;
    let (f, m) = reserve(&c, false).await?;
    let coin = fund(&c, f.source.actor).await?;
    let seats = c
        .query(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index",
            &[&m],
        )
        .await?;
    let a: Uuid = seats[0].get(0);
    let b: Uuid = seats[1].get(0);
    offer(
        &c,
        m,
        a,
        "0",
        json!([{"assetId":coin,"quantity":"100"}]),
        None,
    )
    .await?;
    let offered = offer(&c, m, b, "1", json!([]), Some("300")).await?;
    let commitment: Uuid = offered["data"]["resourceId"].as_str().unwrap().parse()?;
    accept(
        &c,
        commitment,
        a,
        "2",
        offered["data"]["termsHash"].as_str().unwrap(),
    )
    .await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.source.actor]).await?;
    let request = Uuid::new_v4();
    let first: Value = c
        .query_one(
            "SELECT app.cancel_ante($1,$2,2,$3)",
            &[&commitment, &a, &request],
        )
        .await?
        .get(0);
    assert_eq!(first["data"]["state"], "canceled");
    assert_eq!(
        c.query_one(
            "SELECT app.cancel_ante($1,$2,2,$3)",
            &[&commitment, &a, &request]
        )
        .await?
        .get::<_, Value>(0),
        first
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.asset_hold WHERE ante_commitment_id=$1 AND state='held'", &[&commitment]).await?.get::<_,i64>(0),0);
    let row=c.query_one("SELECT x.state,x.loss_coins::text,b.lost_coins::text FROM app.ai_loss_exposure x JOIN app.ai_loss_bucket b ON b.id=x.bucket_id WHERE x.commitment_id=$1", &[&commitment]).await?;
    assert_eq!(row.get::<_, String>(0), "released");
    assert_eq!(row.get::<_, String>(1), "0");
    assert_eq!(row.get::<_, String>(2), "0");
    assert_eq!(
        offer(
            &c,
            m,
            a,
            "2",
            json!([{"assetId":coin,"quantity":"100"}]),
            None
        )
        .await?["status"],
        "fail"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn tied_opening_rerolls_only_leaders_and_the_adopted_controller_chooses_once() -> TestResult {
    let c = connection().await?;
    let fixture = reserve_three(&c, "red", "blue", "red").await?;
    room_start_integrity::confirm_adopted_ai(&c, fixture.m).await?;
    let [human, first_ai, second_ai] = fixture.seats;
    // Only the dice stream is controlled. The production election, seat order, grant
    // checks and immutable selection all run against the migrated database.
    c.batch_execute("BEGIN;
        CREATE TEMP SEQUENCE roll_position;
        CREATE OR REPLACE FUNCTION app.native_opening_d20() RETURNS smallint LANGUAGE sql VOLATILE SET search_path='' AS $$
        SELECT (ARRAY[6,19,19,3,12]::smallint[])[nextval('pg_temp.roll_position')::integer] $$;").await?;
    let observe = "SELECT app.native_opening_human_choice(a.auth_subject,(test_fixture.claims_for_subject(a.auth_subject)->>'session_id')::uuid,$2,NULL,NULL,$3) FROM app.account a WHERE a.id=$1";
    let actor = fixture.room.source.actor;
    let game = fixture.game;
    let get = |first: Option<Uuid>| {
        let c = &c;
        async move {
            c.query_one(observe, &[&actor, &game, &first])
                .await
                .map(|r| r.get::<_, Value>(0))
        }
    };
    let before = get(None).await?;
    let expected = json!({"gameId":fixture.game,"chooserSeatId":second_ai,"firstSeatId":null,"yourChoice":false,
        "rolls":[6,19,19],"seatIds":fixture.seats,"rollRounds":[[6,19,19],[3,12]]});
    assert_eq!(before["data"], expected);
    assert_eq!(get(Some(human)).await?["status"], "fail");
    let current = c.query_one("SELECT id,binding_epoch::text,grant_secret_ref FROM app.controller_grant WHERE match_seat_id=$1 AND state='active'", &[&second_ai]).await?;
    let grant: Uuid = current.get(0);
    let epoch: String = current.get(1);
    let secret: String = current.get(2);
    let choose =
        "SELECT app.native_opening_controller_choice($1,$2::text::numeric,$3,$4,NULL,NULL,$5)";
    let request = |grant: Uuid, first: Uuid| {
        let c = &c;
        let epoch = &epoch;
        let secret = &secret;
        let game = &fixture.game;
        async move {
            c.query_one(choose, &[&grant, epoch, secret, game, &first])
                .await
                .map(|r| r.get::<_, Value>(0))
        }
    };
    assert_eq!(request(Uuid::new_v4(), human).await?["status"], "fail");
    let original = request(grant, first_ai).await?;
    assert_eq!(original["data"]["firstSeatId"], json!(first_ai));
    assert_eq!(original["data"]["yourChoice"], true);
    assert_eq!(request(grant, first_ai).await?, original);
    assert_eq!(
        request(grant, human).await?["data"]["choice"],
        "already selected"
    );
    assert_eq!(
        c.query_one("SELECT last_value FROM pg_temp.roll_position", &[])
            .await?
            .get::<_, i64>(0),
        5
    );
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}
