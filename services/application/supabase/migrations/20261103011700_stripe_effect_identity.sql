BEGIN;

-- Preserve061 financial lineage and its existing privileges; tighten only the
-- mandatory revision and the target binding of cached processed dispositions.
CREATE OR REPLACE FUNCTION app.commit_stripe_effect(p_purchase uuid,p_event uuid,p_revision numeric,p_effect jsonb,p_checkout text,p_payment text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player uuid; lifecycle text; ticket app.purchase; source app.provider_event; duplicate app.provider_event;
 kind text:=p_effect->>'kind'; v_effect_key text:=p_effect->>'effectKey'; next_state text; disposition text:='applied'; grant_id uuid; result jsonb;
BEGIN
 IF p_revision IS NULL OR p_revision::text !~ '^(0|[1-9][0-9]{0,77})$' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='purchase revision is required and must be a canonical finite integer'; END IF;
 SELECT account_id INTO STRICT player FROM app.purchase WHERE id=p_purchase;
 SELECT state INTO STRICT lifecycle FROM app.account WHERE id=player FOR NO KEY UPDATE;
 SELECT * INTO STRICT ticket FROM app.purchase WHERE id=p_purchase FOR NO KEY UPDATE;
 SELECT * INTO STRICT source FROM app.provider_event WHERE id=p_event AND provider='stripe' FOR UPDATE;
 IF source.processing_state='processed' THEN
  IF source.effect->>'purchaseId' IS DISTINCT FROM p_purchase::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='processed source belongs to another purchase'; END IF;
  RETURN source.effect->'result';
 END IF;
 IF ticket.revision<>p_revision THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='purchase revision changed'; END IF;
 IF source.account_id IS NOT NULL AND source.account_id<>player THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='source account differs'; END IF;
 IF (ticket.checkout_terms IS NULL OR source.capture IS NULL OR jsonb_typeof(p_effect) IS DISTINCT FROM 'object' OR octet_length(p_effect::text)>16384
 OR p_effect->>'version' IS DISTINCT FROM '1' OR kind NOT IN ('pending','canceled','paid','refunded','reversed','restored','ignored')
 OR jsonb_typeof(p_effect->'version') IS DISTINCT FROM 'number'
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['kind','effectKey','termsHash','sourceObjectKey','checkoutKey','chargeKey','disputeKey','movementKey','reversalKey','refundMinorUnits','amountMinor','currency','settledReversalKey']) field
  WHERE p_effect ? field AND jsonb_typeof(p_effect->field) IS DISTINCT FROM 'string')
 OR v_effect_key !~ '^[0-9a-f]{64}$' OR p_effect->>'termsHash' IS DISTINCT FROM app.stripe_key(ticket.checkout_terms::text)
 OR v_effect_key IS DISTINCT FROM app.stripe_key(CASE
  WHEN kind='paid' THEN 'paid:'||(p_effect->>'paymentKey')
  WHEN kind IN ('refunded','reversed','restored') THEN kind||':'||(p_effect->>'paymentKey')||':'||(p_effect->>'movementKey')
  ELSE kind||':'||(p_effect->>'checkoutKey') END)
 OR source.capture->>'apiVersion' IS DISTINCT FROM ticket.checkout_terms->>'apiVersion'
 OR (source.capture->>'kind'='api_read' AND source.capture->>'purchaseRevision' IS DISTINCT FROM p_revision::text)
 OR source.capture->'live' IS DISTINCT FROM ticket.checkout_terms->'live'
 OR source.raw_evidence_state='pending' OR (source.capture->>'kind'='webhook' AND source.raw_evidence_state<>'retained')
 OR (p_checkout IS NOT NULL AND (p_checkout !~ '^cs_[A-Za-z0-9_]+$' OR (ticket.stripe_checkout_ref IS NOT NULL AND ticket.stripe_checkout_ref<>p_checkout)))
 OR (p_payment IS NOT NULL AND (p_payment !~ '^pi_[A-Za-z0-9_]+$' OR (ticket.stripe_payment_ref IS NOT NULL AND ticket.stripe_payment_ref<>p_payment)))
 OR p_effect->>'sourceObjectKey' IS DISTINCT FROM app.stripe_key(source.capture->>'targetId')
 OR p_effect->>'checkoutKey' IS DISTINCT FROM app.stripe_key(coalesce(ticket.stripe_checkout_ref,p_checkout))
 OR p_effect->>'paymentKey' IS DISTINCT FROM app.stripe_key(coalesce(ticket.stripe_payment_ref,p_payment))
 OR (p_effect-ARRAY['version','kind','effectKey','termsHash','sourceObjectKey','checkoutKey','paymentKey','chargeKey','disputeKey','movementKey','reversalKey','reversalEventId','refundMinorUnits','fundsReturned','paid','amountMinor','currency','settledReversalKey'])<>'{}'::jsonb
 OR (kind IN ('paid','refunded','reversed','restored') AND (p_effect->'paid' IS DISTINCT FROM 'true'::jsonb OR p_effect->>'amountMinor' IS DISTINCT FROM ticket.checkout_terms->>'minorUnits'
  OR p_effect->>'currency' IS DISTINCT FROM ticket.checkout_terms->>'currency'))
 OR (kind IN ('paid','refunded','reversed','restored') AND p_effect->>'chargeKey' !~ '^[0-9a-f]{64}$')
 OR (kind='refunded' AND CASE WHEN p_effect->>'refundMinorUnits' ~ '^[1-9][0-9]{0,7}$'
  THEN (p_effect->>'refundMinorUnits')::numeric>(ticket.checkout_terms->>'minorUnits')::numeric ELSE true END)
 OR (kind='reversed' AND p_effect->>'disputeKey' !~ '^[0-9a-f]{64}$')
 OR (kind IN ('refunded','reversed','restored') AND p_effect->>'movementKey' !~ '^[0-9a-f]{64}$')
 OR (p_effect ? 'settledReversalKey' AND (kind NOT IN ('paid','restored')
  OR p_effect->>'settledReversalKey' !~ '^[0-9a-f]{64}$'
  OR p_effect->>'movementKey' !~ '^[0-9a-f]{64}$'
  OR p_effect->>'reversalKey' IS DISTINCT FROM p_effect->>'settledReversalKey'
  OR p_effect->>'refundMinorUnits' IS DISTINCT FROM '0' OR p_effect->'fundsReturned' IS DISTINCT FROM 'true'::jsonb))
 OR (kind='restored' AND (p_effect->>'refundMinorUnits' IS DISTINCT FROM '0' OR p_effect->'fundsReturned' IS DISTINCT FROM 'true'::jsonb))) IS NOT FALSE THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='source effect contract differs'; END IF;
 IF kind IN ('paid','refunded','reversed','restored') AND EXISTS(SELECT 1 FROM app.provider_event e
  WHERE e.provider='stripe' AND e.processing_state='processed' AND e.effect->>'purchaseId'=p_purchase::text
  AND e.effect ? 'chargeKey' AND e.effect->>'chargeKey' IS DISTINCT FROM p_effect->>'chargeKey') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='charge lineage differs'; END IF;
 SELECT * INTO duplicate FROM app.provider_event e WHERE e.provider='stripe' AND e.effect_key=v_effect_key AND e.effect->>'disposition'='applied';
 IF FOUND THEN
  IF duplicate.effect->>'purchaseId' IS DISTINCT FROM p_purchase::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='effect purchase differs'; END IF;
  disposition:='duplicate';
 END IF;
 next_state:=CASE kind WHEN 'restored' THEN 'paid' WHEN 'ignored' THEN ticket.state ELSE kind END;
 IF kind='restored' AND disposition='applied' THEN
  IF ticket.state NOT IN ('refunded','reversed') OR p_effect->>'reversalEventId' IS DISTINCT FROM ticket.current_reversal_event_id::text
   OR p_effect->>'reversalKey' IS DISTINCT FROM ticket.current_reversal_key
   OR p_effect->>'refundMinorUnits' IS DISTINCT FROM '0' OR p_effect->'fundsReturned' IS DISTINCT FROM 'true'::jsonb THEN
   disposition:='noop'; END IF;
 ELSIF (kind='paid' AND ticket.state IN ('paid','refunded','reversed'))
 OR (kind='pending' AND ticket.state NOT IN ('creating','pending'))
 OR (kind='canceled' AND ticket.state NOT IN ('creating','pending'))
 OR (kind='reversed' AND ticket.state='refunded') OR kind='ignored' THEN disposition:='noop';
 END IF;
 IF kind IN ('paid','refunded','reversed','restored') AND coalesce(ticket.stripe_payment_ref,p_payment) IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='payment lineage required'; END IF;
 IF kind='reversed' AND EXISTS(SELECT 1 FROM app.provider_event e WHERE e.provider='stripe'
  AND e.effect->>'purchaseId'=p_purchase::text AND e.effect->>'settledReversalKey'=p_effect->>'movementKey'
  AND e.effect->>'kind' IN ('paid','restored') AND e.effect->'fundsReturned'='true'::jsonb
  AND e.effect->>'refundMinorUnits'='0' AND e.processing_state='processed') THEN disposition:='noop'; END IF;
 UPDATE app.provider_event SET account_id=player,processing_state='verified',effect=p_effect||jsonb_build_object('purchaseId',p_purchase,'disposition',disposition),
 effect_key=v_effect_key WHERE id=p_event;
 IF disposition='applied' THEN
  UPDATE app.purchase SET state=next_state,provider_event_id=p_event,
   stripe_checkout_ref=coalesce(stripe_checkout_ref,p_checkout),stripe_payment_ref=coalesce(stripe_payment_ref,p_payment),
   current_reversal_event_id=CASE WHEN kind IN ('reversed','refunded') THEN p_event ELSE current_reversal_event_id END,
   current_reversal_key=CASE WHEN kind IN ('reversed','refunded') THEN p_effect->>'movementKey' ELSE current_reversal_key END,
   checkout_url=CASE WHEN next_state='pending' THEN checkout_url ELSE NULL END,
   revision=revision+1 WHERE id=p_purchase RETURNING revision INTO p_revision;
  SELECT id INTO grant_id FROM app.cosmetic_grant WHERE purchase_id=p_purchase AND provider_event_id=p_event;
 END IF;
 result:=jsonb_build_object('disposition',disposition,'purchaseId',p_purchase,'revision',p_revision::text,'state',CASE WHEN disposition='applied' THEN next_state ELSE ticket.state END,'grantId',grant_id);
 UPDATE app.provider_event SET processing_state='processed',effect=effect||jsonb_build_object('result',result) WHERE id=p_event;
 -- A failed optimistic commit can leave an API-read capture without an effect.
 -- A later committed current observation supersedes those earlier captures;
 -- it does not pretend their unsigned historical hashes were applied facts.
 UPDATE app.provider_event SET account_id=player,processing_state='processed',
  effect=jsonb_build_object('kind','superseded','purchaseId',p_purchase,'observationId',p_event,
   'result',result||jsonb_build_object('disposition','stale'))
 WHERE provider='stripe' AND processing_state='received' AND capture->>'kind'='api_read'
 AND capture->>'targetId'=coalesce(ticket.stripe_checkout_ref,p_checkout)
 AND (account_id IS NULL OR account_id=player) AND id<>p_event
 AND CASE WHEN capture->>'purchaseRevision' ~ '^(0|[1-9][0-9]{0,77})$'
  THEN (capture->>'purchaseRevision')::numeric<p_revision ELSE false END;
 RETURN result;
END $$;

COMMIT;
