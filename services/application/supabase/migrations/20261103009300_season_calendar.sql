BEGIN;

-- Ranked and supply seasons share exact calendar-quarter boundaries in UTC.
-- Explicit UTC conversion keeps the rule independent of a connection's timezone and DST.
ALTER TABLE app.season ADD CONSTRAINT season_utc_calendar_quarter CHECK (
 start_at = make_timestamptz(year::integer,quarter::integer * 3 - 2,1,0,0,0,'UTC')
 AND end_at = ((start_at AT TIME ZONE 'UTC') + interval '3 months') AT TIME ZONE 'UTC'
);

COMMIT;
