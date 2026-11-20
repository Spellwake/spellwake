BEGIN;

-- Keep raw numeric input until integer/precision checks run; numeric(78,0) rounds first.
DROP VIEW app.coin_balance;
DROP VIEW app.asset_availability;
DROP VIEW app.prediction_position;
DROP VIEW app.friend_request;
DROP VIEW app.shared_real_game;
DROP VIEW app.account_qualifying_count;
DROP VIEW app.quest_progress;
DROP VIEW app.season_rank_snapshot;
DROP VIEW app.match_decision_deadline;
DROP VIEW app.historical_context;
DO $$ DECLARE item record; BEGIN
 FOR item IN SELECT table_name,column_name FROM information_schema.columns
  WHERE table_schema='app' AND data_type='numeric' AND numeric_precision=78 AND numeric_scale=0
 LOOP
  EXECUTE format('ALTER TABLE app.%I ALTER COLUMN %I TYPE numeric',item.table_name,item.column_name);
  EXECUTE format('ALTER TABLE app.%I ADD CONSTRAINT %I CHECK (%I IS NULL OR (%I >= 0 AND %I < power(10::numeric,78) AND scale(%I)=0))',
   item.table_name,item.column_name||'_whole_value',item.column_name,item.column_name,item.column_name,item.column_name);
 END LOOP;
END $$;

CREATE VIEW app.coin_balance WITH (security_invoker=true) AS
 SELECT i.id, i.owner_id, i.id AS inventory_asset_id, i.owned_quantity AS quantity
 FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE d.kind='coin';

CREATE VIEW app.asset_availability WITH (security_invoker=true) AS
 SELECT i.id,i.id AS inventory_asset_id,i.owned_quantity AS owned,coalesce(h.held,0)::numeric AS held,
 i.owned_quantity-coalesce(h.held,0) AS available
 FROM app.inventory_asset i LEFT JOIN (
 SELECT l.inventory_asset_id,sum(l.quantity) AS held FROM app.hold_line l JOIN app.asset_hold h ON h.id=l.hold_id
 WHERE h.state='held' GROUP BY l.inventory_asset_id) h ON h.inventory_asset_id=i.id;

CREATE VIEW app.prediction_position WITH (security_invoker=true) AS
 SELECT md5(p.id::text||'-yes')::uuid AS id,p.id AS pair_id,p.yes_order_id AS order_id,o.account_id,'yes'::text AS side,
 p.quantity,p.yes_actual_cost_coins AS actual_cost_coins FROM app.prediction_pair p JOIN app.prediction_order o ON o.id=p.yes_order_id
 UNION ALL
 SELECT md5(p.id::text||'-no')::uuid,p.id,p.no_order_id,o.account_id,'no',p.quantity,p.no_actual_cost_coins
 FROM app.prediction_pair p JOIN app.prediction_order o ON o.id=p.no_order_id;

CREATE VIEW app.friend_request WITH (security_invoker=true) AS
 SELECT id,id AS friendship_id,requested_by_id FROM app.friendship WHERE state='pending';

CREATE VIEW app.shared_real_game WITH (security_invoker=true) AS
 SELECT md5(a.id::text||b.id::text)::uuid AS id,a.id AS left_participation_id,b.id AS right_participation_id,
 a.game_id,greatest(a.completed_at,b.completed_at) AS shared_at
 FROM app.game_participation a JOIN app.game_participation b ON a.game_id=b.game_id AND a.account_id<b.account_id
 WHERE a.real_game AND b.real_game;

CREATE VIEW app.account_qualifying_count WITH (security_invoker=true) AS
 SELECT a.id,a.id AS account_id,count(p.id) AS qualifying_games,
 floor(extract(epoch FROM (statement_timestamp()-a.created_at))/86400)::bigint AS age_days,
 NOT (statement_timestamp()>=a.created_at+interval '30 days' AND count(p.id)>=20) AS restricted
 FROM app.account a LEFT JOIN app.game_participation p ON p.account_id=a.id AND p.real_game GROUP BY a.id;

CREATE VIEW app.quest_progress WITH (security_invoker=true) AS
 SELECT a.id,a.id AS assignment_id,a.progress,a.completed_at IS NOT NULL AS complete FROM app.quest_assignment a;

CREATE VIEW app.season_rank_snapshot WITH (security_invoker=true) AS
 SELECT id,id AS rank_id,season_id FROM app.rank WHERE finalized;

CREATE VIEW app.match_decision_deadline WITH (security_invoker=true) AS
 SELECT id,id AS decision_id,deadline_at,clock_revision FROM app.between_game_decision WHERE state='pending';

CREATE VIEW app.historical_context WITH (security_invoker=true) AS
 SELECT id,id AS context_id,release_id FROM app.game_context;

GRANT SELECT ON app.coin_balance,app.asset_availability,app.prediction_position,app.friend_request,app.shared_real_game,app.account_qualifying_count,app.quest_progress,app.season_rank_snapshot,app.match_decision_deadline,app.historical_context TO service_role;
GRANT SELECT ON app.coin_balance,app.asset_availability TO authenticated;

COMMIT;
