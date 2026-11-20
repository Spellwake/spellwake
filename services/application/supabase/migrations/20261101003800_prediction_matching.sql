BEGIN;

CREATE FUNCTION app.guard_prediction_market() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction terms/history cannot be deleted'; END IF;
 IF OLD.state='resolved' OR (OLD.state='closed' AND NEW.state='open')
 OR NEW.next_order_sequence<OLD.next_order_sequence OR NEW.orders_revision<OLD.orders_revision
 OR (NEW.id,NEW.creator_id,NEW.printing_id,NEW.price_metric_ref,NEW.target_usd_decimal,NEW.condition,NEW.deadline,NEW.trading_closes_at,
 NEW.observation_starts_at,NEW.sampling_policy_ref,NEW.terminal_slot_ref,NEW.rules_revision,NEW.payout_coins,NEW.order_capacity)
 IS DISTINCT FROM (OLD.id,OLD.creator_id,OLD.printing_id,OLD.price_metric_ref,OLD.target_usd_decimal,OLD.condition,OLD.deadline,OLD.trading_closes_at,
 OLD.observation_starts_at,OLD.sampling_policy_ref,OLD.terminal_slot_ref,OLD.rules_revision,OLD.payout_coins,OLD.order_capacity) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction terms or terminal state are immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER prediction_market_history BEFORE UPDATE OR DELETE ON app.prediction_market FOR EACH ROW EXECUTE FUNCTION app.guard_prediction_market();
ALTER TABLE app.prediction_market ADD CHECK ((state='resolved' AND outcome IN ('yes','no','void') AND outcome IS NOT NULL
 AND resolution_evidence_ref IS NOT NULL AND resolution_rules_revision IS NOT NULL AND settlement_operation_id IS NOT NULL AND resolved_at IS NOT NULL)
 OR (state<>'resolved' AND outcome IS NULL AND resolution_evidence_ref IS NULL AND resolution_rules_revision IS NULL AND settlement_operation_id IS NULL AND resolved_at IS NULL));
REVOKE INSERT,UPDATE,DELETE ON app.prediction_market FROM service_role;

CREATE FUNCTION app.cancel_prediction_unfilled(p_order uuid,p_state text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_state NOT IN ('canceled','closed') OR p_state IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid prediction cancellation'; END IF;
 UPDATE app.prediction_order SET state=p_state WHERE id=p_order AND state='open';
 IF FOUND THEN PERFORM app.refresh_prediction_hold(p_order); END IF;
END $$;

CREATE FUNCTION app.place_prediction_order(p_actor uuid,p_market uuid,p_side text,p_limit text,p_quantity text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; market app.prediction_market; owner uuid; coin uuid; inventory uuid; available numeric;
 limit_cost numeric; quantity numeric; reserve numeric; operation uuid; hold uuid; created uuid:=gen_random_uuid();
 candidate app.prediction_order; needed numeric; take numeric; pair uuid; match_operation uuid; yes_cost numeric; no_cost numeric;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'prediction_order',p_market::text,jsonb_build_object('side',p_side,'limit',p_limit,'quantity',p_quantity));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF p_side IS NULL OR p_side NOT IN ('yes','no') OR p_limit IS NULL OR p_limit !~ '^[1-9][0-9]?$'
 OR p_quantity IS NULL OR p_quantity !~ '^[1-9][0-9]{0,77}$' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('order','invalid whole-contract bid'));
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.prediction_market WHERE id=p_market) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('market','prediction market unavailable'));
 END IF;
 market:=app.lock_prediction_market(p_market,p_actor);
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable'));
 END IF;
 IF market.state<>'open' OR clock_timestamp()>=market.trading_closes_at THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('market','prediction trading is closed'));
 END IF;
 IF (SELECT count(*) FROM app.prediction_order WHERE market_id=p_market)>=market.order_capacity THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('market','prediction order capacity reached'));
 END IF;
 limit_cost:=p_limit::numeric; quantity:=p_quantity::numeric; reserve:=limit_cost*quantity;
 IF reserve>=power(10::numeric,78) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('quantity','reservation exceeds supported precision'));
 END IF;
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=p_actor AND state='active';
 SELECT id INTO STRICT coin FROM app.asset_definition WHERE kind='coin' AND currency_key='coin';
 inventory:=app.ensure_inventory(owner,coin);
 PERFORM id FROM app.inventory_asset WHERE id=inventory FOR NO KEY UPDATE;
 SELECT a.available INTO available FROM app.asset_availability a WHERE a.id=inventory;
 IF available<reserve THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('coins','not enough available coins')); END IF;
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('prediction_order',created::text,'prediction_order',p_actor,clock_timestamp(),'pending') RETURNING id INTO operation;
 INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES(owner,'prediction',created::text,'held',operation) RETURNING id INTO hold;
 INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES(hold,inventory,reserve);
 INSERT INTO app.prediction_order(id,market_id,account_id,hold_id,side,limit_cost_coins,quantity,filled_quantity,accepted_sequence,state,accepted_at)
 VALUES(created,p_market,p_actor,hold,p_side,limit_cost,quantity,0,market.next_order_sequence,'open',clock_timestamp());
 UPDATE app.prediction_market SET next_order_sequence=next_order_sequence+1,orders_revision=orders_revision+1 WHERE id=p_market;
 needed:=quantity;
 FOR candidate IN SELECT * FROM app.prediction_order WHERE market_id=p_market AND side<>p_side AND state='open'
 AND limit_cost_coins+limit_cost>=100 AND account_id<>p_actor ORDER BY limit_cost_coins DESC,accepted_sequence FOR NO KEY UPDATE LOOP
  EXIT WHEN needed=0;
  IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=candidate.account_id AND state='active') THEN
   PERFORM app.cancel_prediction_unfilled(candidate.id,'closed'); CONTINUE;
  END IF;
  take:=least(needed,candidate.quantity-candidate.filled_quantity);
  IF p_side='yes' THEN no_cost:=take*candidate.limit_cost_coins; yes_cost:=take*100-no_cost;
  ELSE yes_cost:=take*candidate.limit_cost_coins; no_cost:=take*100-yes_cost; END IF;
  pair:=gen_random_uuid();
  INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
  VALUES('prediction_match',pair::text,'prediction_match',p_actor,clock_timestamp(),'pending') RETURNING id INTO match_operation;
  INSERT INTO app.prediction_pair(id,market_id,yes_order_id,no_order_id,quantity,fixed_payout_coins,yes_actual_cost_coins,no_actual_cost_coins,match_operation_id,matched_at)
  VALUES(pair,p_market,CASE WHEN p_side='yes' THEN created ELSE candidate.id END,CASE WHEN p_side='no' THEN created ELSE candidate.id END,take,100,yes_cost,no_cost,match_operation,clock_timestamp());
  UPDATE app.prediction_order SET filled_quantity=filled_quantity+take,state=CASE WHEN filled_quantity+take=app.prediction_order.quantity THEN 'filled' ELSE 'open' END WHERE id=candidate.id;
  UPDATE app.prediction_order SET filled_quantity=filled_quantity+take WHERE id=created;
  PERFORM app.refresh_prediction_hold(candidate.id);
  UPDATE app.ledger_operation SET state='committed' WHERE id=match_operation;
  needed:=needed-take;
 END LOOP;
 IF needed=0 THEN UPDATE app.prediction_order SET state='filled' WHERE id=created; END IF;
 PERFORM app.refresh_prediction_hold(created);
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',created));
END $$;

CREATE FUNCTION app.cancel_prediction_order(p_actor uuid,p_order uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; ticket app.prediction_order; market app.prediction_market;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'prediction_cancel',p_order::text,'{}');
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT * INTO ticket FROM app.prediction_order WHERE id=p_order AND account_id=p_actor;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('order','prediction order unavailable')); END IF;
 market:=app.lock_prediction_market(ticket.market_id,p_actor);
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending')) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable'));
 END IF;
 SELECT * INTO STRICT ticket FROM app.prediction_order WHERE id=p_order FOR NO KEY UPDATE;
 IF ticket.state<>'open' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('order','no unfilled units to cancel')); END IF;
 PERFORM app.cancel_prediction_unfilled(p_order,'canceled');
 UPDATE app.prediction_market SET orders_revision=orders_revision+1 WHERE id=market.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_order));
END $$;

CREATE FUNCTION app.close_prediction_trading(p_market uuid) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE market app.prediction_market; ticket uuid;
BEGIN
 market:=app.lock_prediction_market(p_market,NULL);
 IF market.state<>'open' THEN RETURN false; END IF;
 IF clock_timestamp()<market.trading_closes_at THEN RETURN false; END IF;
 FOR ticket IN SELECT id FROM app.prediction_order WHERE market_id=p_market AND state='open' ORDER BY accepted_sequence LOOP
  PERFORM app.cancel_prediction_unfilled(ticket,'closed');
 END LOOP;
 UPDATE app.prediction_market SET state='closed',orders_revision=orders_revision+1 WHERE id=p_market;
 RETURN true;
END $$;
GRANT EXECUTE ON FUNCTION app.place_prediction_order(uuid,uuid,text,text,text,uuid),app.cancel_prediction_order(uuid,uuid,uuid),app.close_prediction_trading(uuid) TO service_role;

COMMIT;
