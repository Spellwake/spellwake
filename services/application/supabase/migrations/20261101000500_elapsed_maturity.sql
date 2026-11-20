BEGIN;

-- Thirty elapsed days is 720 hours even if the database session uses a DST timezone.
CREATE OR REPLACE VIEW app.account_qualifying_count WITH (security_invoker=true) AS
 SELECT a.id,a.id AS account_id,count(p.id) AS qualifying_games,
 floor(extract(epoch FROM (statement_timestamp()-a.created_at))/86400)::bigint AS age_days,
 NOT (statement_timestamp()>=a.created_at+interval '720 hours' AND count(p.id)>=20) AS restricted
 FROM app.account a LEFT JOIN app.game_participation p ON p.account_id=a.id AND p.real_game GROUP BY a.id;

COMMIT;
