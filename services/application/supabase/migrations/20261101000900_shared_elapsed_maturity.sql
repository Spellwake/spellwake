BEGIN;

-- A view rebuild must not reintroduce calendar-day arithmetic. Share the tested rule.
CREATE FUNCTION app.trade_restricted(p_created timestamptz,p_evaluated timestamptz,p_games numeric)
RETURNS boolean LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT NOT coalesce(p_evaluated>=p_created
  AND extract(epoch FROM (p_evaluated-p_created))>=2592000
  AND p_games>=20,false)
$$;
GRANT EXECUTE ON FUNCTION app.trade_restricted(timestamptz,timestamptz,numeric) TO service_role,authenticated;

CREATE OR REPLACE VIEW app.account_qualifying_count WITH (security_invoker=true) AS
 SELECT a.id,a.id AS account_id,count(p.id) AS qualifying_games,
 floor(extract(epoch FROM (statement_timestamp()-a.created_at))/86400)::bigint AS age_days,
 app.trade_restricted(a.created_at,statement_timestamp(),count(p.id)) AS restricted
 FROM app.account a LEFT JOIN app.game_participation p ON p.account_id=a.id AND p.real_game GROUP BY a.id;

COMMIT;
