BEGIN;

-- Unresolved rows are the durable work source. UUID keysets let a sweep move
-- past pending markets; a later sweep revisits them and discovers new rows.
CREATE INDEX prediction_maintenance_due ON app.prediction_market(id)
 INCLUDE(trading_closes_at) WHERE state IN ('open','closed')
 AND rules_revision='scryfall-sampled-gte-v1' AND sampling_policy_ref='scryfall-daily-12-13-utc-v1';

CREATE FUNCTION app.read_prediction_maintenance_page(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE entries jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid prediction maintenance page bounds';
 END IF;
 SELECT coalesce(jsonb_agg(page.id ORDER BY page.id),'[]'::jsonb) INTO entries FROM (
  SELECT id FROM app.prediction_market WHERE state IN ('open','closed')
  AND rules_revision='scryfall-sampled-gte-v1' AND sampling_policy_ref='scryfall-daily-12-13-utc-v1'
  AND trading_closes_at<=statement_timestamp() AND (p_after IS NULL OR id>p_after)
  ORDER BY id LIMIT p_limit) page;
 RETURN jsonb_build_object('items',entries,'nextCursor',CASE WHEN jsonb_array_length(entries)=p_limit THEN entries->>-1 ELSE NULL END);
END $$;

-- The activity supplies only identity. The existing locked SQL writers decide
-- the clock, filled backing, source evidence, outcome and exact postings.
CREATE FUNCTION app.maintain_prediction_market(p_market uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE market app.prediction_market;
BEGIN
 IF p_market IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='prediction identity required'; END IF;
 market:=app.lock_prediction_market(p_market,NULL);
 IF market.rules_revision<>'scryfall-sampled-gte-v1' OR market.sampling_policy_ref<>'scryfall-daily-12-13-utc-v1' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction maintenance requires qualified sampled rules';
 END IF;
 IF market.state='resolved' THEN RETURN app.resolve_prediction(p_market); END IF;
 IF clock_timestamp()<market.trading_closes_at THEN RETURN jsonb_build_object('state','not_due'); END IF;
 PERFORM app.close_prediction_trading(p_market);
 RETURN app.resolve_prediction(p_market);
END $$;
GRANT EXECUTE ON FUNCTION app.read_prediction_maintenance_page(uuid,integer),app.maintain_prediction_market(uuid) TO service_role;

COMMIT;
