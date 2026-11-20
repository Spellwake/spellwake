//! Calendar metadata checks; these do not qualify rank, ballot or reward producers.
use super::{TestResult, connection};
use tokio_postgres::error::SqlState;

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn all_four_seasons_use_utc_quarters_even_in_a_dst_connection_timezone() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN; SET LOCAL TIME ZONE 'America/New_York'")
        .await?;
    let rows = c.query("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state)
        SELECT 2028,q,make_timestamptz(2028,q*3-2,1,0,0,0,'UTC'),
        (make_timestamp(2028,q*3-2,1,0,0,0)+interval '3 months') AT TIME ZONE 'UTC','fixture','planned'
        FROM generate_series(1,4) AS q
        RETURNING quarter,(start_at AT TIME ZONE 'UTC')::text,(end_at AT TIME ZONE 'UTC')::text", &[]).await?;
    assert_eq!(rows.len(), 4);
    for row in rows {
        let q: i64 = row.get(0);
        assert_eq!(
            row.get::<_, String>(1),
            format!("2028-{:02}-01 00:00:00", q * 3 - 2)
        );
        let (year, month) = if q == 4 { (2029, 1) } else { (2028, q * 3 + 1) };
        assert_eq!(
            row.get::<_, String>(2),
            format!("{year}-{month:02}-01 00:00:00")
        );
    }
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn season_calendar_rejects_wrong_quarter_local_midnight_and_boundary_drift() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    for (start, end) in [
        ("2028-02-01T00:00:00Z", "2028-04-01T00:00:00Z"),
        ("2028-01-01T00:00:00-05:00", "2028-04-01T00:00:00-04:00"),
        ("2028-01-01T00:00:00Z", "2028-04-01T00:00:00.001Z"),
        ("2028-01-01T00:00:00Z", "2028-03-31T23:59:59.999Z"),
        ("2027-01-01T00:00:00Z", "2027-04-01T00:00:00Z"),
    ] {
        c.batch_execute("SAVEPOINT invalid_calendar").await?;
        let error = c.execute("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(2028,1,$1::text::timestamptz,$2::text::timestamptz,'fixture','planned')", &[&start,&end]).await.unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
        assert_eq!(
            error.as_db_error().and_then(|e| e.constraint()),
            Some("season_utc_calendar_quarter")
        );
        c.batch_execute("ROLLBACK TO SAVEPOINT invalid_calendar")
            .await?;
    }
    c.execute("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) VALUES(2028,1,'2028-01-01T00:00:00Z','2028-04-01T00:00:00Z','fixture','planned')", &[]).await?;
    c.batch_execute("SAVEPOINT invalid_update").await?;
    let error = c
        .execute(
            "UPDATE app.season SET quarter=2 WHERE year=2028 AND quarter=1",
            &[],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
    c.batch_execute("ROLLBACK TO SAVEPOINT invalid_update; ROLLBACK")
        .await?;
    Ok(())
}
