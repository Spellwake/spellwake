BEGIN;

ALTER TABLE app.account ADD CHECK (state IN ('active','deletion_pending','deleted'));
ALTER TABLE app.account ADD CHECK (sign_in_method IN ('google','apple','discord','email_code'));
ALTER TABLE app.account ADD CHECK (username = btrim(username) AND length(username) BETWEEN 3 AND 24);
ALTER TABLE app.account ADD CHECK (state = 'deleted' OR auth_subject IS NOT NULL);
ALTER TABLE app.account ADD CHECK ((state = 'active' AND deletion_requested_at IS NULL AND deletion_finalized_at IS NULL)
 OR (state = 'deletion_pending' AND deletion_requested_at IS NOT NULL AND deletion_finalized_at IS NULL)
 OR (state = 'deleted' AND deletion_requested_at IS NOT NULL AND deletion_finalized_at IS NOT NULL AND retirement_operation_id IS NOT NULL));
CREATE UNIQUE INDEX account_normalized_username ON app.account (lower(username));

ALTER TABLE app.asset_definition ADD CHECK (
 (kind='coin' AND currency_key IS NOT NULL AND printing_id IS NULL AND recipe_id IS NULL)
 OR (kind='card' AND currency_key IS NULL AND printing_id IS NOT NULL AND recipe_id IS NULL)
 OR (kind='pack' AND currency_key IS NULL AND printing_id IS NULL AND recipe_id IS NOT NULL));
ALTER TABLE app.asset_hold ADD CHECK (purpose IN ('trade','listing','prediction','ante'));
ALTER TABLE app.asset_hold ADD CHECK (state IN ('held','consumed','released'));
ALTER TABLE app.hold_line ADD CHECK (quantity > 0);
ALTER TABLE app.ledger_posting ADD CHECK (direction IN ('credit','debit') AND quantity > 0);
ALTER TABLE app.ledger_operation ADD CHECK (state IN ('pending','committed'));
ALTER TABLE app.command_receipt ADD CHECK (state IN ('pending','accepted','rejected'));
ALTER TABLE app.inventory_asset ALTER COLUMN owned_quantity SET DEFAULT 0;
ALTER TABLE app.inventory_asset ALTER COLUMN revision SET DEFAULT 0;

ALTER TABLE app.friendship ADD CHECK (account_low_id < account_high_id);
ALTER TABLE app.friendship ADD CHECK (requested_by_id IN (account_low_id, account_high_id));
ALTER TABLE app.friendship ADD CHECK (state IN ('pending','accepted','removed'));
ALTER TABLE app.friendship ADD CHECK ((state='accepted') = (accepted_at IS NOT NULL));
ALTER TABLE app.trade_offer ADD CHECK (sender_id <> recipient_id AND state IN ('offered','accepted','rejected','canceled'));
ALTER TABLE app.trade_line ADD CHECK (side IN ('give','request') AND quantity > 0);
ALTER TABLE app.market_listing ADD CHECK (quantity > 0 AND total_price_coins > 0 AND state IN ('listed','sold','canceled'));
ALTER TABLE app.market_listing ADD CHECK (
 (state='sold' AND buyer_id IS NOT NULL AND settlement_operation_id IS NOT NULL AND paid_coins IS NOT NULL AND completed_at IS NOT NULL)
 OR (state<>'sold' AND buyer_id IS NULL AND settlement_operation_id IS NULL AND paid_coins IS NULL AND completed_at IS NULL));

ALTER TABLE app.prediction_market ADD CHECK (payout_coins=100);
ALTER TABLE app.prediction_market ADD CHECK (trading_closes_at < observation_starts_at AND observation_starts_at <= deadline);
ALTER TABLE app.prediction_market ADD CHECK (condition IN ('reached_by','at_deadline'));
ALTER TABLE app.prediction_order ADD CHECK (side IN ('yes','no') AND limit_cost_coins BETWEEN 1 AND 99 AND quantity > 0 AND filled_quantity <= quantity AND accepted_sequence > 0);
ALTER TABLE app.prediction_pair ADD CHECK (fixed_payout_coins=100 AND quantity > 0 AND yes_actual_cost_coins > 0 AND no_actual_cost_coins > 0
 AND yes_actual_cost_coins+no_actual_cost_coins=quantity*fixed_payout_coins AND yes_order_id<>no_order_id);
ALTER TABLE app.price_observation ADD CHECK (state IN ('valid','missing','invalid'));
ALTER TABLE app.price_observation ADD CHECK (price_field IN ('usd','usd_foil','usd_etched'));
ALTER TABLE app.price_observation ADD CHECK ((state='valid' AND price_usd_decimal ~ '^(0|[1-9][0-9]*)(\.[0-9]+)?$') OR (state<>'valid' AND price_usd_decimal IS NULL));

ALTER TABLE app.purchase ADD CHECK (state IN ('creating','pending','paid','canceled','refunded','reversed'));
ALTER TABLE app.purchase ADD CHECK (state NOT IN ('paid','refunded','reversed') OR (stripe_payment_ref IS NOT NULL AND provider_event_id IS NOT NULL));
CREATE UNIQUE INDEX free_cosmetic_grant ON app.cosmetic_grant (account_id,cosmetic_id) WHERE source_kind='free';
ALTER TABLE app.cosmetic_grant ADD CHECK (
 (source_kind='free' AND purchase_id IS NULL AND patreon_payment_id IS NULL)
 OR (source_kind='stripe_purchase' AND purchase_id IS NOT NULL AND patreon_payment_id IS NULL)
 OR (source_kind='patreon_payment' AND purchase_id IS NULL AND patreon_payment_id IS NOT NULL));

ALTER TABLE app.match ADD CHECK (mode IN ('open','collection') AND structure IN ('bo1','bo3') AND (NOT competitive OR mode='collection'));
ALTER TABLE app.match_seat ADD CHECK ((source_revision_id IS NULL AND deck_snapshot_ref IS NULL AND deck_accepted_at IS NULL)
 OR (source_revision_id IS NOT NULL AND deck_snapshot_ref IS NOT NULL AND deck_accepted_at IS NOT NULL));
ALTER TABLE app.match_seat ADD CHECK ((controller_kind='human' AND account_id IS NOT NULL)
 OR (controller_kind='ai' AND account_id IS NULL AND sponsor_id IS NOT NULL));
ALTER TABLE app.match_result ADD CHECK ((ranking_format_id IS NULL AND rank_sequence IS NULL) OR (ranking_format_id IS NOT NULL AND rank_sequence > 0));
ALTER TABLE app.ante_stake ADD CHECK ((funding_kind='owned_assets' AND hold_id IS NOT NULL AND ai_exposure_id IS NULL)
 OR (funding_kind='ai_loss_allowance' AND hold_id IS NULL AND ai_exposure_id IS NOT NULL));
ALTER TABLE app.ai_loss_bucket ADD CHECK (lost_coins <= limit_coins AND limit_coins=1000);
ALTER TABLE app.ai_loss_exposure ADD CHECK (reserved_coins <= 300);
ALTER TABLE app.game_participation ADD CHECK (real_game = (own_turns >= 3));

ALTER TABLE app.format ALTER COLUMN next_competitive_sequence SET DEFAULT 1;
ALTER TABLE app.format ALTER COLUMN last_ranked_sequence SET DEFAULT 0;
ALTER TABLE app.format ADD CHECK (last_ranked_sequence < next_competitive_sequence);
ALTER TABLE app.season ADD CHECK (quarter BETWEEN 1 AND 4 AND start_at < end_at);
ALTER TABLE app.supply_pricing_policy ADD CHECK (base_price_coins > 0 AND window_starting_quantity > 0 AND normal_quantity > 0);
ALTER TABLE app.supply_stock ADD CHECK (remaining_quantity <= starting_quantity);
ALTER TABLE app.supply_purchase ADD CHECK (quantity > 0 AND paid_coins > 0);
ALTER TABLE app.quest_assignment ADD CHECK ((completed_at IS NULL AND reward_operation_id IS NULL AND reward_source_fact_id IS NULL)
 OR (completed_at IS NOT NULL AND reward_operation_id IS NOT NULL AND reward_source_fact_id IS NOT NULL));
ALTER TABLE app.daily_quest_state ADD CHECK (rerolls_used <= 1);

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

REVOKE ALL ON ALL TABLES IN SCHEMA app FROM PUBLIC,anon,authenticated;
GRANT SELECT ON app.coin_balance,app.asset_availability,app.prediction_position,app.friend_request,
 app.shared_real_game,app.account_qualifying_count,app.quest_progress,app.season_rank_snapshot,
 app.match_decision_deadline,app.historical_context TO service_role;

COMMIT;
