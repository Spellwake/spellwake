BEGIN;

CREATE INDEX prediction_pending_observation ON app.prediction_market(observation_starts_at,deadline,id) WHERE state IN ('open','closed');
CREATE FUNCTION app.read_prediction_extraction_page(p_slot date,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE result jsonb; started timestamptz;
BEGIN
 IF p_slot IS NULL OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid extraction page bounds';
 END IF;
 started:=app.price_slot_start(p_slot);
 IF clock_timestamp()<started+interval '1 hour' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='source slot acceptance has not closed';
 END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
  SELECT m.id,jsonb_build_object('marketId',m.id,'printingRef',p.external_printing_ref) AS entry
  FROM app.prediction_market m JOIN app.printing p ON p.id=m.printing_id
  WHERE m.state IN ('open','closed') AND m.rules_revision='scryfall-sampled-gte-v1'
  AND m.sampling_policy_ref='scryfall-daily-12-13-utc-v1'
  AND (m.condition='reached_by' OR m.terminal_slot_ref=p_slot::text) AND (p_after IS NULL OR m.id>p_after)
  AND started>=m.observation_starts_at AND started+interval '1 hour'<=m.deadline
  AND NOT EXISTS(SELECT 1 FROM app.price_observation o WHERE o.market_id=m.id AND o.scheduled_slot_ref=p_slot::text)
  ORDER BY m.id LIMIT p_limit) page;
 RETURN jsonb_build_object('items',result,'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'marketId' ELSE NULL END);
END $$;
GRANT EXECUTE ON FUNCTION app.read_prediction_extraction_page(date,uuid,integer) TO service_role;

-- Reuse an already established printing probe for this exact selected source.
-- The durable receipt is the existing authority, not another price cache table.
CREATE FUNCTION app.read_established_price_probe(p_manifest uuid,p_printing uuid) RETURNS uuid
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE manifest jsonb; receipt app.command_receipt; evidence jsonb;
BEGIN
 manifest:=app.read_price_manifest(p_manifest);
 IF manifest->>'state'<>'valid' OR p_printing IS NULL THEN RETURN NULL; END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='collector:scryfall-v1' AND scope='price_probe'
 AND request_ref=p_manifest::text||':'||p_printing::text;
 IF NOT FOUND THEN RETURN NULL; END IF;
 evidence:=receipt.result_ref::jsonb;
 IF receipt.state<>'accepted' OR receipt.account_id IS NOT NULL OR receipt.controller_grant_id IS NOT NULL OR receipt.operation_id IS NOT NULL
 OR receipt.target_ref<>p_printing::text OR (evidence->>'manifestId') IS DISTINCT FROM p_manifest::text OR (evidence->>'printingRef') IS DISTINCT FROM p_printing::text
 OR receipt.payload_hash<>encode(sha256(convert_to(evidence::text,'UTF8')),'hex') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='printing probe source is inconsistent';
 END IF;
 RETURN receipt.id;
END $$;
GRANT EXECUTE ON FUNCTION app.read_established_price_probe(uuid,uuid) TO service_role;

COMMIT;
