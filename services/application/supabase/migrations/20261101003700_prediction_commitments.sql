BEGIN;

ALTER TABLE app.prediction_market ADD COLUMN orders_revision numeric NOT NULL DEFAULT 0
 CHECK (orders_revision>=0 AND orders_revision<power(10::numeric,78) AND scale(orders_revision)=0);
ALTER TABLE app.prediction_market ADD COLUMN order_capacity integer NOT NULL DEFAULT 10000 CHECK (order_capacity BETWEEN 1 AND 100000);
ALTER TABLE app.prediction_market ADD CHECK (state IN ('open','closed','resolved'));
ALTER TABLE app.prediction_market ADD CHECK (next_order_sequence>0);
ALTER TABLE app.prediction_order ADD COLUMN accepted_at timestamptz NOT NULL;
ALTER TABLE app.prediction_order ADD CHECK (state IN ('open','filled','canceled','closed','settled'));
ALTER TABLE app.prediction_order ADD CHECK (state<>'filled' OR filled_quantity=quantity);
ALTER TABLE app.prediction_pair ADD COLUMN matched_at timestamptz NOT NULL;
CREATE INDEX prediction_book ON app.prediction_order(market_id,side,limit_cost_coins DESC,accepted_sequence) WHERE state='open';
CREATE INDEX prediction_owner ON app.prediction_order(account_id,market_id,id);

CREATE FUNCTION app.lock_prediction_market(p_market uuid,p_actor uuid) RETURNS app.prediction_market
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE before_revision numeric; market app.prediction_market;
BEGIN
 SELECT orders_revision INTO before_revision FROM app.prediction_market WHERE id=p_market;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='prediction market unavailable'; END IF;
 -- All affected accounts precede the market/hold/inventory locks, including the
 -- scheduler path. A changed participant set retries instead of reversing locks.
 PERFORM a.id FROM app.account a WHERE a.id=p_actor OR EXISTS
 (SELECT 1 FROM app.prediction_order o WHERE o.market_id=p_market AND o.account_id=a.id)
 ORDER BY a.id FOR NO KEY UPDATE;
 SELECT * INTO STRICT market FROM app.prediction_market WHERE id=p_market FOR NO KEY UPDATE;
 IF market.orders_revision<>before_revision THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='prediction participant snapshot changed'; END IF;
 RETURN market;
END $$;

CREATE FUNCTION app.prediction_filled_cost(p_order uuid) RETURNS numeric
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT coalesce(sum(CASE WHEN yes_order_id=p_order THEN yes_actual_cost_coins ELSE no_actual_cost_coins END),0)
 FROM app.prediction_pair WHERE yes_order_id=p_order OR no_order_id=p_order
$$;
CREATE FUNCTION app.refresh_prediction_hold(p_order uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE ticket app.prediction_order; backing numeric;
BEGIN
 SELECT * INTO STRICT ticket FROM app.prediction_order WHERE id=p_order;
 backing:=app.prediction_filled_cost(p_order)+CASE WHEN ticket.state='open' THEN (ticket.quantity-ticket.filled_quantity)*ticket.limit_cost_coins ELSE 0 END;
 IF backing>0 THEN UPDATE app.hold_line SET quantity=backing WHERE hold_id=ticket.hold_id;
 ELSE UPDATE app.asset_hold SET state='released' WHERE id=ticket.hold_id AND state='held'; END IF;
END $$;

CREATE FUNCTION app.guard_prediction_order() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction order history cannot be deleted'; END IF;
 IF TG_OP='UPDATE' AND ((NEW.id,NEW.market_id,NEW.account_id,NEW.hold_id,NEW.side,NEW.limit_cost_coins,NEW.quantity,NEW.accepted_sequence,NEW.accepted_at)
 IS DISTINCT FROM (OLD.id,OLD.market_id,OLD.account_id,OLD.hold_id,OLD.side,OLD.limit_cost_coins,OLD.quantity,OLD.accepted_sequence,OLD.accepted_at)
 OR NEW.filled_quantity<OLD.filled_quantity OR OLD.state='settled'
 OR (OLD.state IN ('canceled','closed') AND NEW.state NOT IN (OLD.state,'settled'))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction terms or terminal fills cannot change';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER prediction_order_history BEFORE UPDATE OR DELETE ON app.prediction_order FOR EACH ROW EXECUTE FUNCTION app.guard_prediction_order();

CREATE FUNCTION app.guard_prediction_pair() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE yes_order app.prediction_order; no_order app.prediction_order; earlier app.prediction_order; operation app.ledger_operation;
BEGIN
 SELECT * INTO STRICT yes_order FROM app.prediction_order WHERE id=NEW.yes_order_id;
 SELECT * INTO STRICT no_order FROM app.prediction_order WHERE id=NEW.no_order_id;
 SELECT * INTO STRICT operation FROM app.ledger_operation WHERE id=NEW.match_operation_id;
 IF yes_order.market_id<>NEW.market_id OR no_order.market_id<>NEW.market_id OR yes_order.side<>'yes' OR no_order.side<>'no'
 OR yes_order.account_id=no_order.account_id OR yes_order.limit_cost_coins+no_order.limit_cost_coins<100
 OR operation.kind<>'prediction_match' OR operation.source_scope<>'prediction_match' OR operation.source_ref<>NEW.id::text OR operation.state<>'pending' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction pair source or opposite funded sides do not match';
 END IF;
 earlier:=CASE WHEN yes_order.accepted_sequence<no_order.accepted_sequence THEN yes_order ELSE no_order END;
 IF (earlier.side='yes' AND NEW.yes_actual_cost_coins<>NEW.quantity*earlier.limit_cost_coins)
 OR (earlier.side='no' AND NEW.no_actual_cost_coins<>NEW.quantity*earlier.limit_cost_coins) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction execution must use the resting price';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER prediction_pair_source BEFORE INSERT ON app.prediction_pair FOR EACH ROW EXECUTE FUNCTION app.guard_prediction_pair();

CREATE FUNCTION app.validate_prediction_order(p_order uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE ticket app.prediction_order; hold app.asset_hold; filled numeric; cost numeric; backing numeric; line_quantity numeric;
BEGIN
 SELECT * INTO STRICT ticket FROM app.prediction_order WHERE id=p_order;
 SELECT * INTO STRICT hold FROM app.asset_hold WHERE id=ticket.hold_id;
 SELECT coalesce(sum(quantity),0) INTO filled FROM app.prediction_pair WHERE yes_order_id=p_order OR no_order_id=p_order;
 cost:=app.prediction_filled_cost(p_order);
 backing:=cost+CASE WHEN ticket.state='open' THEN (ticket.quantity-ticket.filled_quantity)*ticket.limit_cost_coins ELSE 0 END;
 SELECT l.quantity INTO line_quantity FROM app.hold_line l JOIN app.inventory_asset i ON i.id=l.inventory_asset_id
 JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE l.hold_id=hold.id AND d.kind='coin' AND d.currency_key='coin';
 IF ticket.filled_quantity<>filled OR hold.purpose<>'prediction' OR hold.commitment_ref<>ticket.id::text
 OR NOT EXISTS(SELECT 1 FROM app.asset_owner WHERE id=hold.owner_id AND account_id=ticket.account_id)
 OR (SELECT count(*) FROM app.hold_line WHERE hold_id=hold.id)<>1 OR line_quantity IS NULL
 OR (ticket.state<>'settled' AND ((backing>0 AND (hold.state<>'held' OR line_quantity<>backing)) OR (backing=0 AND hold.state<>'released')))
 OR (ticket.state='settled' AND hold.state<>(CASE WHEN cost>0 THEN 'consumed' ELSE 'released' END))
 OR NOT EXISTS(SELECT 1 FROM app.ledger_operation WHERE id=hold.operation_id AND kind='prediction_order' AND source_scope='prediction_order'
 AND source_ref=ticket.id::text AND actor_account_id=ticket.account_id AND state='committed') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction fill/backing/source commitment is inconsistent';
 END IF;
END $$;
CREATE FUNCTION app.check_prediction_commitment() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE order_id uuid; v_hold_id uuid; purpose text;
BEGIN
 IF TG_TABLE_NAME='prediction_order' THEN PERFORM app.validate_prediction_order(NEW.id);
 ELSIF TG_TABLE_NAME='prediction_pair' THEN
  PERFORM app.validate_prediction_order(NEW.yes_order_id); PERFORM app.validate_prediction_order(NEW.no_order_id);
  IF NOT EXISTS(SELECT 1 FROM app.ledger_operation WHERE id=NEW.match_operation_id AND kind='prediction_match' AND source_scope='prediction_match' AND source_ref=NEW.id::text AND state='committed') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction pair operation is not committed';
  END IF;
 ELSE
  IF TG_TABLE_NAME='asset_hold' THEN v_hold_id:=NEW.id; ELSE v_hold_id:=NEW.hold_id; END IF;
  SELECT h.purpose INTO STRICT purpose FROM app.asset_hold h WHERE h.id=v_hold_id;
  IF purpose='prediction' THEN
   SELECT id INTO order_id FROM app.prediction_order WHERE app.prediction_order.hold_id=v_hold_id;
   IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction hold requires its source order'; END IF;
   PERFORM app.validate_prediction_order(order_id);
  END IF;
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER prediction_order_commitment AFTER INSERT OR UPDATE ON app.prediction_order DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_prediction_commitment();
CREATE CONSTRAINT TRIGGER prediction_pair_commitment AFTER INSERT ON app.prediction_pair DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_prediction_commitment();
CREATE CONSTRAINT TRIGGER prediction_hold_commitment AFTER INSERT OR UPDATE ON app.asset_hold DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_prediction_commitment();
CREATE CONSTRAINT TRIGGER prediction_hold_line_commitment AFTER INSERT OR UPDATE ON app.hold_line DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_prediction_commitment();

CREATE FUNCTION app.check_prediction_cutoff() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM app.prediction_market WHERE id=NEW.market_id AND state='open' AND clock_timestamp()<trading_closes_at) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction trading cutoff has passed';
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER prediction_order_cutoff AFTER INSERT ON app.prediction_order DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_prediction_cutoff();
CREATE CONSTRAINT TRIGGER prediction_pair_cutoff AFTER INSERT ON app.prediction_pair DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_prediction_cutoff();
REVOKE INSERT,UPDATE,DELETE ON app.prediction_order,app.prediction_pair FROM service_role;

COMMIT;
