//! Normalized historical outcome fixtures qualify arithmetic/order/accounting, not native result truth.
use super::{TestResult, account, connection, game_scope, wait_for_database_lock};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) async fn source(
    c: &Client,
    shared: Option<(Uuid, Uuid, Uuid)>,
    sequence: i64,
    when: &str,
) -> FixtureResult<(Uuid, Uuid, Uuid, Uuid)> {
    let f = game_scope::seed(c).await?;
    let (format, actor, peer) = if let Some(v) = shared {
        v
    } else {
        (
            c.query_one(
                "SELECT format_id FROM app.match WHERE id=$1",
                &[&f.match_id],
            )
            .await?
            .get(0),
            f.account,
            account(c).await?.0,
        )
    };
    c.execute(
        "UPDATE app.match_seat SET account_id=$1 WHERE id=$2",
        &[&actor, &f.seat],
    )
    .await?;
    c.execute("INSERT INTO app.match_seat(match_id,account_id,seat_index,controller_kind) VALUES($1,$2,1,'human')", &[&f.match_id,&peer]).await?;
    c.execute("UPDATE app.match SET format_id=$2,competitive=true,mode='collection',state='completed' WHERE id=$1", &[&f.match_id,&format]).await?;
    c.execute(
        "UPDATE app.game SET state='completed' WHERE id=$1",
        &[&f.game],
    )
    .await?;
    let game_result:Uuid=c.query_one("INSERT INTO app.game_result(game_id,source_event_id,outcome,winners_ref,end_reason,winner_seat_ids) VALUES($1,$2,'win','fixture:normalized','fixture',$3) RETURNING id", &[&f.game,&f.event,&vec![f.seat]]).await?.get(0);
    let result:Uuid=c.query_one("INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at,ranking_format_id,rank_sequence,winner_seat_ids,final_game_ids,source_game_result_id) VALUES($1,$2,'win','fixture:normalized',$8::text::timestamptz,$3,$4::bigint,$5,$6,$7) RETURNING id", &[&f.match_id,&Uuid::new_v4().to_string(),&format,&sequence,&vec![f.seat],&vec![f.game],&game_result,&when]).await?.get(0);
    c.execute(
        "UPDATE app.format SET next_competitive_sequence=$2::bigint+1 WHERE id=$1",
        &[&format, &sequence],
    )
    .await?;
    Ok((result, format, actor, peer))
}
pub(super) async fn consume(c: &Client, result: Uuid) -> Result<Value, tokio_postgres::Error> {
    let response: Value = c
        .query_one("SELECT app.consume_ranked_match_result($1)", &[&result])
        .await?
        .get(0);
    let context = c.query_one("SELECT s.id,r.rank_sequence::text,(SELECT count(*)::text FROM app.match_seat WHERE match_id=r.match_id AND controller_kind='human') FROM app.match_result r JOIN app.season s ON r.committed_at>=s.start_at AND r.committed_at<s.end_at WHERE r.id=$1", &[&result]).await?;
    let context = spellwake_application::terminal_consumers::RankedContext {
        season_id: context.get(0),
        sequence: context.get::<_, String>(1).parse().unwrap(),
        players: context.get::<_, String>(2).parse().unwrap(),
    };
    assert!(
        spellwake_application::terminal_consumers::RankedResult { result_id: result }
            .decode_response(response.to_string().as_bytes(), Some(&context))
            .is_ok()
    );
    Ok(response)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn ordered_rank_rating_rewards_and_drained_season_finalize_as_one_source_once_path()
-> TestResult {
    let c = connection().await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(2003,1,'2003-01-01T00:00:00Z','2003-04-01T00:00:00Z','fixture:elo-v1','active') RETURNING id", &[]).await?.get(0);
    let (first, format, actor, peer) = source(&c, None, 1, "2003-02-01T00:00:00Z").await?;
    let (second, _, _, _) =
        source(&c, Some((format, actor, peer)), 2, "2003-02-01T00:00:00Z").await?;
    assert_eq!(
        consume(&c, second).await.unwrap_err().code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    let one = consume(&c, first).await?;
    let pending = tokio::spawn(async move { consume(&waiting, first).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(pending.await??, one);
    assert_eq!(one["status"], "success");
    assert_eq!(consume(&c, first).await?, one);
    let first_ratings=c.query("SELECT rating_before::text,rating_delta::text,rating_after::text FROM app.competitive_result WHERE match_result_id=$1 ORDER BY win DESC", &[&first]).await?;
    assert_eq!(first_ratings[0].get::<_, String>(0), "1000");
    assert_eq!(first_ratings[0].get::<_, String>(1), "16");
    assert_eq!(first_ratings[0].get::<_, String>(2), "1016");
    assert_eq!(first_ratings[1].get::<_, String>(1), "-16");
    assert_eq!(
        c.query_one("SELECT app.finalize_rank_season($1)", &[&season])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert_eq!(consume(&c, second).await?["status"], "success");
    assert_eq!(
        c.query_one(
            "SELECT rating::text FROM app.rank WHERE account_id=$1 AND season_id=$2",
            &[&actor, &season]
        )
        .await?
        .get::<_, String>(0),
        "1031"
    );
    assert_eq!(c.query_one("SELECT issued_coins::text,wins::text FROM app.daily_win_bucket WHERE account_id=$1 AND utc_day='2003-02-01'", &[&actor]).await?.get::<_,String>(0),"350");
    for sequence in 3..=9 {
        let (next, _, _, _) = source(
            &c,
            Some((format, actor, peer)),
            sequence,
            "2003-02-01T00:00:00Z",
        )
        .await?;
        assert_eq!(consume(&c, next).await?["status"], "success");
    }
    let finalized: Value = c
        .query_one("SELECT app.finalize_rank_season($1)", &[&season])
        .await?
        .get(0);
    assert_eq!(finalized["status"], "success");
    assert!(
        spellwake_application::terminal_consumers::FinalizeRankSeason { season_id: season }
            .decode_response(finalized.to_string().as_bytes())
            .is_ok()
    );
    assert_eq!(
        c.query_one("SELECT app.finalize_rank_season($1)", &[&season])
            .await?
            .get::<_, Value>(0),
        finalized
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.rank WHERE season_id=$1 AND finalized",
            &[&season]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    assert_eq!(
        c.query_one(
            "SELECT result_cutoff_manifest->'formats'->0->>'count' FROM app.season WHERE id=$1",
            &[&season]
        )
        .await?
        .get::<_, String>(0),
        "9"
    );
    assert_eq!(c.query_one("SELECT sum(i.owned_quantity)::text FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE o.account_id=$1 AND d.currency_key='coin'", &[&actor]).await?.get::<_,String>(0),"900");
    assert_eq!(consume(&c, second).await?["status"], "success");
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.ledger_operation WHERE source_scope='rank_season_v1' AND actor_account_id=ANY($1)", &[&vec![actor,peer]]).await?.get::<_,i64>(0),2);
    assert_eq!(c.execute("UPDATE app.rank SET rating=1000,revision=revision+1 WHERE account_id=$1 AND season_id=$2", &[&actor,&season]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.execute("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(2003,2,'2003-04-01T00:00:00Z','2003-07-01T00:00:00Z','fixture:elo-v1','active')", &[]).await?;
    let (next, _, _, _) =
        source(&c, Some((format, actor, peer)), 10, "2003-05-01T00:00:00Z").await?;
    assert_eq!(consume(&c, next).await?["status"], "success");
    assert_eq!(c.query_one("SELECT rating_before::text FROM app.competitive_result WHERE account_id=$1 AND match_result_id=$2", &[&actor,&next]).await?.get::<_,String>(0),"1051");
    assert_eq!(c.query_one("SELECT rating_before::text FROM app.competitive_result WHERE account_id=$1 AND match_result_id=$2", &[&peer,&next]).await?.get::<_,String>(0),"948");

    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn team_free_for_all_shared_winner_draw_and_no_contest_use_one_locked_side_basis()
-> TestResult {
    let c = connection().await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    c.execute("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(2007,1,'2007-01-01T00:00:00Z','2007-04-01T00:00:00Z','fixture:elo-v1','active')", &[]).await?;
    for case in ["teams", "ffa", "shared", "draw", "no_contest"] {
        let f = game_scope::seed(&c).await?;
        let format: Uuid = c
            .query_one(
                "SELECT format_id FROM app.match WHERE id=$1",
                &[&f.match_id],
            )
            .await?
            .get(0);
        c.execute(
            "UPDATE app.match SET competitive=true,mode='collection',state='completed' WHERE id=$1",
            &[&f.match_id],
        )
        .await?;
        let mut seats = vec![f.seat];
        let mut actors = vec![f.account];
        for index in 1..4i64 {
            let actor = account(&c).await?.0;
            actors.push(actor);
            seats.push(c.query_one("INSERT INTO app.match_seat(match_id,account_id,seat_index,controller_kind) VALUES($1,$2,$3,'human') RETURNING id", &[&f.match_id,&actor,&index]).await?.get(0));
        }
        if case == "teams" {
            for (index, seat) in seats.iter().enumerate() {
                c.execute(
                    "UPDATE app.match_seat SET team_ref=$2 WHERE id=$1",
                    &[seat, &if index < 2 { "red" } else { "blue" }],
                )
                .await?;
            }
        }
        let winners = match case {
            "teams" | "shared" => seats[..2].to_vec(),
            "ffa" => vec![seats[0]],
            _ => vec![],
        };
        let outcome = if case == "draw" {
            "draw"
        } else if case == "no_contest" {
            "no_contest"
        } else {
            "win"
        };
        let game_result:Uuid=c.query_one("INSERT INTO app.game_result(game_id,source_event_id,outcome,winners_ref,end_reason,winner_seat_ids) VALUES($1,$2,$3,'fixture:normalized','fixture',$4) RETURNING id", &[&f.game,&f.event,&outcome,&winners]).await?.get(0);
        let result:Uuid=c.query_one("INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at,ranking_format_id,rank_sequence,winner_seat_ids,final_game_ids,source_game_result_id) VALUES($1,$2,$3,'fixture:normalized','2007-02-01T00:00:00Z',$4,1,$5,$6,$7) RETURNING id", &[&f.match_id,&Uuid::new_v4().to_string(),&outcome,&format,&winners,&vec![f.game],&game_result]).await?.get(0);
        c.execute(
            "UPDATE app.format SET next_competitive_sequence=2 WHERE id=$1",
            &[&format],
        )
        .await?;
        assert_eq!(consume(&c, result).await?["status"], "success");
        let expected = match case {
            "teams" => [16, 16, -16, -16],
            "ffa" => [16, -5, -5, -5],
            "shared" => [11, 11, -11, -11],
            _ => [0, 0, 0, 0],
        };
        for (index, actor) in actors.iter().enumerate() {
            assert_eq!(c.query_one("SELECT rating_delta::text FROM app.competitive_result WHERE account_id=$1 AND match_result_id=$2", &[actor,&result]).await?.get::<_,String>(0),expected[index].to_string(),"{case} seat {index}");
        }
    }
    Ok(())
}

/// Historical normalized fixtures cover delayed reward disposition, not post-deletion admission.
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn mixed_active_and_deleted_season_records_unissued_reward_without_resurrecting_inventory()
-> TestResult {
    let c = connection().await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    for (year, reward) in [(1988_i32, "250"), (1989_i32, "0")] {
        let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES($1::integer,1,make_date($1::integer,1,1)::timestamp AT TIME ZONE 'UTC',make_date($1::integer,4,1)::timestamp AT TIME ZONE 'UTC','fixture:retired-reward','active') RETURNING id", &[&year]).await?.get(0);
        c.execute("UPDATE app.season SET ranking_policy=jsonb_set(ranking_policy,'{tiers,0,reward}',to_jsonb($2::text)) WHERE id=$1", &[&season,&reward]).await?;
        let (retired, owner) = account(&c).await?;
        assert_eq!(
            super::deletion::request(&c, retired, Uuid::new_v4()).await?["status"],
            "success"
        );
        let retirement: Value = c
            .query_one("SELECT app.retire_account($1)", &[&retired])
            .await?
            .get(0);
        assert_eq!(retirement["data"]["state"], "retired", "{retirement}");
        let f = game_scope::seed(&c).await?;
        let format: Uuid = c
            .query_one(
                "SELECT format_id FROM app.match WHERE id=$1",
                &[&f.match_id],
            )
            .await?
            .get(0);
        let when = format!("{year}-02-01T00:00:00Z");
        let (result, _, actor, _) =
            source(&c, Some((format, f.account, retired)), 1, &when).await?;
        assert_eq!(consume(&c, result).await?["status"], "success");
        c.batch_execute("BEGIN").await?;
        let finalized: Value = c
            .query_one("SELECT app.finalize_rank_season($1)", &[&season])
            .await?
            .get(0);
        assert_eq!(finalized["status"], "success");
        assert!(
            spellwake_application::terminal_consumers::FinalizeRankSeason { season_id: season }
                .decode_response(finalized.to_string().as_bytes())
                .is_ok()
        );
        c.batch_execute("ROLLBACK").await?;
        assert!(c.query_opt("SELECT id FROM app.command_receipt WHERE scope='rank_season_retired_v1' AND normalized_source->>'accountId'=$1", &[&retired.to_string()]).await?.is_none());
        let finalized: Value = c
            .query_one("SELECT app.finalize_rank_season($1)", &[&season])
            .await?
            .get(0);
        assert_eq!(finalized["status"], "success", "{finalized}");
        assert!(
            spellwake_application::terminal_consumers::FinalizeRankSeason { season_id: season }
                .decode_response(finalized.to_string().as_bytes())
                .is_ok()
        );
        assert_eq!(
            c.query_one("SELECT app.finalize_rank_season($1)", &[&season])
                .await?
                .get::<_, Value>(0),
            finalized
        );
        let row=c.query_one("SELECT normalized_source->>'quantity',normalized_source->>'retirementOperationId',operation_id IS NULL FROM app.command_receipt WHERE scope='rank_season_retired_v1' AND normalized_source->>'accountId'=$1", &[&retired.to_string()]).await?;
        assert_eq!(row.get::<_, String>(0), reward);
        assert_eq!(
            row.get::<_, String>(1),
            retirement["data"]["retirementOperationId"]
                .as_str()
                .unwrap()
        );
        assert!(row.get::<_, bool>(2));
        assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.ledger_operation WHERE source_scope='rank_season_v1' AND actor_account_id=$1", &[&retired]).await?.get::<_,i64>(0),0);
        assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.inventory_asset WHERE owner_id=$1 AND owned_quantity<>0", &[&owner]).await?.get::<_,i64>(0),0);
        assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.ledger_operation WHERE source_scope='rank_season_v1' AND actor_account_id=$1", &[&actor]).await?.get::<_,i64>(0),1);
        let retired_rank: Uuid = c
            .query_one(
                "SELECT id FROM app.rank WHERE account_id=$1 AND season_id=$2",
                &[&retired, &season],
            )
            .await?
            .get(0);
        for deferred in [false, true] {
            c.batch_execute("BEGIN").await?;
            if deferred {
                c.batch_execute("SET CONSTRAINTS ALL DEFERRED").await?;
            }
            let inserted=c.execute("INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES('rank_season',$1::uuid::text,'rank_season_v1',$2,clock_timestamp(),'pending')", &[&retired_rank,&retired]).await;
            let failure = if deferred {
                inserted?;
                c.batch_execute("COMMIT").await.unwrap_err()
            } else {
                let err = inserted.unwrap_err();
                c.batch_execute("ROLLBACK").await?;
                err
            };
            assert_eq!(failure.code(), Some(&SqlState::CHECK_VIOLATION));
        }
        assert!(
            c.query_one(
                "SELECT state='deleted' FROM app.account WHERE id=$1",
                &[&retired]
            )
            .await?
            .get::<_, bool>(0)
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn empty_season_ack_preserves_zero_rank_count_and_exact_cutoff() -> TestResult {
    let c = connection().await?;
    let season:Uuid=c.query_one("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(1991,1,'1991-01-01T00:00:00Z','1991-04-01T00:00:00Z','fixture:empty','active') RETURNING id",&[]).await?.get(0);
    let response: Value = c
        .query_one("SELECT app.finalize_rank_season($1)", &[&season])
        .await?
        .get(0);
    let ack = spellwake_application::terminal_consumers::FinalizeRankSeason { season_id: season }
        .decode_response(response.to_string().as_bytes())?;
    assert!(ack.ranks.is_zero());
    assert_eq!(
        response,
        c.query_one("SELECT app.finalize_rank_season($1)", &[&season])
            .await?
            .get::<_, Value>(0)
    );
    Ok(())
}
