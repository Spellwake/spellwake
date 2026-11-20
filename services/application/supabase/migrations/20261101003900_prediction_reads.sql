BEGIN;

CREATE FUNCTION app.require_prediction_reader(p_actor uuid) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending')) THEN
  RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='prediction account unavailable';
 END IF;
END $$;

CREATE FUNCTION app.prediction_market_json(p_market app.prediction_market) RETURNS jsonb
LANGUAGE sql STABLE SET search_path='' AS $$
 SELECT jsonb_build_object('id',p_market.id,'printingId',p_market.printing_id,'priceMetric',p_market.price_metric_ref,
 'targetUsd',p_market.target_usd_decimal,'condition',p_market.condition,'deadline',p_market.deadline,
 'tradingClosesAt',p_market.trading_closes_at,'observationStartsAt',p_market.observation_starts_at,
 'samplingPolicy',p_market.sampling_policy_ref,'terminalSlot',p_market.terminal_slot_ref,'rulesRevision',p_market.rules_revision,
 'payoutCoins',p_market.payout_coins::text,'state',p_market.state,
 'acceptingOrders',p_market.state='open' AND statement_timestamp()<p_market.trading_closes_at,
 'outcome',p_market.outcome,'resolvedAt',p_market.resolved_at)
$$;

CREATE FUNCTION app.read_prediction_market(p_actor uuid,p_market uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE market app.prediction_market; book jsonb;
BEGIN
 PERFORM app.require_prediction_reader(p_actor);
 SELECT * INTO market FROM app.prediction_market WHERE id=p_market;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='prediction market unavailable'; END IF;
 -- A public depth level reveals neither an account nor a fill/hold identity.
 -- Expired and deleting counterparties are not advertised as executable liquidity.
 SELECT coalesce(jsonb_agg(jsonb_build_object('side',depth.side,'limitCost',depth.limit_cost_coins::text,
 'quantity',depth.quantity::text) ORDER BY depth.side,depth.limit_cost_coins DESC),'[]') INTO book FROM (
 SELECT o.side,o.limit_cost_coins,sum(o.quantity-o.filled_quantity) AS quantity
 FROM app.prediction_order o JOIN app.account a ON a.id=o.account_id
 WHERE o.market_id=p_market AND o.state='open' AND a.state='active' AND market.state='open'
 AND statement_timestamp()<market.trading_closes_at GROUP BY o.side,o.limit_cost_coins) depth;
 RETURN app.prediction_market_json(market)||jsonb_build_object('book',book);
END $$;

CREATE FUNCTION app.read_prediction_market_page(p_actor uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 PERFORM app.require_prediction_reader(p_actor);
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid prediction page bounds';
 END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
 SELECT m.id,app.prediction_market_json(m) AS entry FROM app.prediction_market m
 WHERE p_after IS NULL OR m.id>p_after ORDER BY m.id LIMIT p_limit) page;
 RETURN jsonb_build_object('items',result,'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END);
END $$;

CREATE FUNCTION app.read_prediction_orders(p_actor uuid,p_market uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 PERFORM app.require_prediction_reader(p_actor);
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid prediction page bounds';
 END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
 SELECT o.id,jsonb_build_object('id',o.id,'marketId',o.market_id,'side',o.side,'limitCost',o.limit_cost_coins::text,
 'quantity',o.quantity::text,'filledQuantity',o.filled_quantity::text,'actualFilledCost',app.prediction_filled_cost(o.id)::text,
 'unfilledReservation',CASE WHEN o.state='open' THEN ((o.quantity-o.filled_quantity)*o.limit_cost_coins)::text ELSE '0' END,
 'state',o.state,'acceptedSequence',o.accepted_sequence::text,'acceptedAt',o.accepted_at) AS entry
 FROM app.prediction_order o WHERE o.account_id=p_actor AND (p_market IS NULL OR o.market_id=p_market)
 AND (p_after IS NULL OR o.id>p_after) ORDER BY o.id LIMIT p_limit) page;
 RETURN jsonb_build_object('items',result,'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END);
END $$;

CREATE FUNCTION app.read_prediction_positions(p_actor uuid,p_market uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 PERFORM app.require_prediction_reader(p_actor);
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid prediction page bounds';
 END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
 SELECT p.id,jsonb_build_object('id',p.id,'marketId',p.market_id,'orderId',o.id,'side',o.side,'quantity',p.quantity::text,
 'actualCost',CASE WHEN o.side='yes' THEN p.yes_actual_cost_coins::text ELSE p.no_actual_cost_coins::text END,
 'fixedPayout',p.fixed_payout_coins::text,'matchedAt',p.matched_at) AS entry
 FROM app.prediction_pair p JOIN app.prediction_order o ON o.id IN (p.yes_order_id,p.no_order_id)
 WHERE o.account_id=p_actor AND (p_market IS NULL OR p.market_id=p_market) AND (p_after IS NULL OR p.id>p_after)
 ORDER BY p.id LIMIT p_limit) page;
 RETURN jsonb_build_object('items',result,'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END);
END $$;
GRANT EXECUTE ON FUNCTION app.read_prediction_market(uuid,uuid),app.read_prediction_market_page(uuid,uuid,integer),
 app.read_prediction_orders(uuid,uuid,uuid,integer),app.read_prediction_positions(uuid,uuid,uuid,integer) TO service_role;

COMMIT;
