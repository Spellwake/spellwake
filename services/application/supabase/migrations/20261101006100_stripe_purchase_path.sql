BEGIN;

ALTER TABLE app.purchase ADD COLUMN checkout_terms jsonb;
ALTER TABLE app.purchase ADD COLUMN revision numeric NOT NULL DEFAULT 0
 CHECK(revision>=0 AND revision<power(10::numeric,78) AND scale(revision)=0);
ALTER TABLE app.purchase ADD COLUMN first_create_attempt_at timestamptz;
ALTER TABLE app.purchase ADD COLUMN create_deadline_at timestamptz;
ALTER TABLE app.purchase ADD COLUMN checkout_url text CHECK(length(checkout_url)<=4096);
ALTER TABLE app.purchase ADD COLUMN checkout_expires_at timestamptz;
ALTER TABLE app.purchase ADD COLUMN current_reversal_event_id uuid REFERENCES app.provider_event(id);
ALTER TABLE app.purchase ADD COLUMN current_reversal_key text;
ALTER TABLE app.purchase ADD COLUMN price_currency text GENERATED ALWAYS AS(checkout_terms->>'currency') STORED;
ALTER TABLE app.purchase ADD COLUMN price_minor_units text GENERATED ALWAYS AS(checkout_terms->>'minorUnits') STORED;
ALTER TABLE app.purchase ADD CHECK((first_create_attempt_at IS NULL AND create_deadline_at IS NULL)
 OR create_deadline_at=first_create_attempt_at+interval '23 hours');

ALTER TABLE app.provider_event ADD COLUMN capture jsonb;
ALTER TABLE app.provider_event ADD COLUMN first_delivery_at timestamptz;
ALTER TABLE app.provider_event ADD COLUMN raw_evidence_state text NOT NULL DEFAULT 'none'
 CHECK(raw_evidence_state IN ('none','pending','retained','expired'));
ALTER TABLE app.provider_event ADD COLUMN effect jsonb;
ALTER TABLE app.provider_event ADD COLUMN effect_key text;
ALTER TABLE app.provider_event ADD COLUMN raw_deleted_at timestamptz;
CREATE UNIQUE INDEX stripe_applied_effect ON app.provider_event(effect_key)
 WHERE provider='stripe' AND effect_key IS NOT NULL AND effect->>'disposition'='applied';
CREATE INDEX stripe_reconciliation_events ON app.provider_event(id) WHERE provider='stripe' AND processing_state='received';
CREATE INDEX stripe_purchase_work ON app.purchase(id);

ALTER TABLE app.cosmetic_grant ADD COLUMN provider_event_id uuid REFERENCES app.provider_event(id);
ALTER TABLE app.cosmetic_grant DROP CONSTRAINT cosmetic_grant_account_id_cosmetic_id_purchase_id_key;
CREATE UNIQUE INDEX stripe_grant_source ON app.cosmetic_grant(purchase_id,provider_event_id) WHERE source_kind='stripe_purchase';
ALTER TABLE app.cosmetic_grant ADD CHECK((source_kind='stripe_purchase' AND provider_event_id IS NOT NULL)
 OR (source_kind<>'stripe_purchase' AND provider_event_id IS NULL));

REVOKE ALL ON app.purchase,app.provider_event,app.cosmetic_grant FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT SELECT ON app.purchase,app.provider_event,app.cosmetic_grant TO service_role;
GRANT SELECT(id,account_id,cosmetic_id,requested_at,state,revision,checkout_url,checkout_expires_at,price_currency,price_minor_units) ON app.purchase TO authenticated;
GRANT SELECT ON app.cosmetic_grant TO authenticated;

CREATE FUNCTION app.stripe_key(p_value text) RETURNS text LANGUAGE sql IMMUTABLE STRICT SET search_path='' AS $$
 SELECT encode(sha256(convert_to(p_value,'UTF8')),'hex')
$$;

CREATE FUNCTION app.stripe_actor(p_subject uuid,p_session uuid) RETURNS app.account
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; authority jsonb;
BEGIN
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT * INTO player FROM app.account WHERE auth_subject=p_subject FOR NO KEY UPDATE;
 authority:=app.read_auth_session_authority(p_subject,p_session);
 IF player.id IS NULL OR authority IS NULL OR player.sign_in_method IS DISTINCT FROM authority->>'method'
 OR player.state NOT IN ('active','deletion_pending') THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified live account required';
 END IF;
 RETURN player;
END $$;

CREATE FUNCTION app.read_stripe_reservation(p_subject uuid,p_session uuid,p_request uuid,p_cosmetic uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt;
BEGIN
 player:=app.stripe_actor(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_cosmetic IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='purchase request identity required'; END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text
 AND scope='stripe_checkout' AND request_ref=p_request::text;
 IF NOT FOUND THEN RETURN NULL; END IF;
 IF receipt.target_ref<>p_cosmetic::text OR receipt.payload_hash<>app.stripe_key(jsonb_build_object('cosmeticId',p_cosmetic)::text) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='purchase request identity differs'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='purchase request still pending'; END IF;
 RETURN receipt.response_payload;
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='purchase request identity differs' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

CREATE FUNCTION app.reserve_stripe_purchase(p_subject uuid,p_session uuid,p_request uuid,p_cosmetic uuid,p_terms jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; prior jsonb; receipt app.command_receipt; purchase_id uuid:=gen_random_uuid();
BEGIN
 prior:=app.read_stripe_reservation(p_subject,p_session,p_request,p_cosmetic);
 IF prior IS NOT NULL THEN RETURN prior; END IF;
 player:=app.stripe_actor(p_subject,p_session);
 receipt:=app.begin_account_command(player.id,p_request,'stripe_checkout',p_cosmetic::text,jsonb_build_object('cosmeticId',p_cosmetic));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF player.state<>'active' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 IF NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=p_cosmetic AND state='active' AND NOT is_free) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('cosmetic','unavailable')); END IF;
 IF (jsonb_typeof(p_terms) IS DISTINCT FROM 'object' OR octet_length(p_terms::text)>16384
 OR p_terms->>'version' IS DISTINCT FROM '1' OR p_terms->>'priceId' !~ '^price_[A-Za-z0-9_]+$'
 OR jsonb_typeof(p_terms->'version') IS DISTINCT FROM 'number'
 OR (SELECT bool_and(jsonb_typeof(p_terms->key)='string') FROM unnest(ARRAY['priceId','currency','minorUnits','apiVersion','successUrl','cancelUrl']) key) IS DISTINCT FROM true
 OR p_terms->>'currency' !~ '^[a-z]{3}$' OR p_terms->>'minorUnits' !~ '^[1-9][0-9]{0,7}$'
 OR jsonb_typeof(p_terms->'live') IS DISTINCT FROM 'boolean' OR length(p_terms->>'apiVersion') NOT BETWEEN 1 AND 64
 OR p_terms->>'successUrl' NOT LIKE 'https://%' OR p_terms->>'cancelUrl' NOT LIKE 'https://%'
 OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_terms) key)
 IS DISTINCT FROM ARRAY['apiVersion','cancelUrl','currency','live','minorUnits','priceId','successUrl','version']::text[]) IS NOT FALSE THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid server checkout terms'; END IF;
 INSERT INTO app.purchase(id,account_id,cosmetic_id,provider_request_ref,requested_at,state,checkout_terms)
 VALUES(purchase_id,player.id,p_cosmetic,'spellwake-purchase-'||purchase_id::text,clock_timestamp(),'creating',p_terms);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',purchase_id));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM IN ('purchase request identity differs','request identity was used for different command terms') THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

CREATE FUNCTION app.read_stripe_purchase(p_purchase uuid) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('id',p.id,'accountId',p.account_id,'cosmeticId',p.cosmetic_id,'state',p.state,
 'revision',p.revision::text,'terms',p.checkout_terms,'termsHash',app.stripe_key(p.checkout_terms::text),
 'accountState',(SELECT state FROM app.account WHERE id=p.account_id),
 'requestKey',p.provider_request_ref,'checkoutRef',p.stripe_checkout_ref,'paymentRef',p.stripe_payment_ref,
 'deadline',p.create_deadline_at,'reversalEventId',p.current_reversal_event_id,'reversalKey',p.current_reversal_key)
 FROM app.purchase p WHERE p.id=p_purchase AND p.checkout_terms IS NOT NULL
$$;

CREATE FUNCTION app.claim_stripe_creation(p_purchase uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player uuid; ticket app.purchase; result jsonb; stamp timestamptz; lifecycle text;
BEGIN
 SELECT account_id INTO STRICT player FROM app.purchase WHERE id=p_purchase;
 SELECT state INTO lifecycle FROM app.account WHERE id=player FOR NO KEY UPDATE;
 SELECT * INTO STRICT ticket FROM app.purchase WHERE id=p_purchase FOR NO KEY UPDATE;
 IF ticket.state<>'creating' OR ticket.stripe_checkout_ref IS NOT NULL THEN RETURN NULL; END IF;
 IF lifecycle='deleted' THEN RETURN NULL; END IF;
 IF lifecycle<>'active' AND ticket.first_create_attempt_at IS NULL THEN
  UPDATE app.purchase SET state='canceled',revision=revision+1 WHERE id=p_purchase;
  RETURN NULL;
 END IF;
 IF ticket.first_create_attempt_at IS NULL THEN
  stamp:=clock_timestamp();
  UPDATE app.purchase SET first_create_attempt_at=stamp,create_deadline_at=stamp+interval '23 hours',revision=revision+1 WHERE id=p_purchase;
 END IF;
 SELECT app.read_stripe_purchase(p_purchase) INTO result;
 IF (result->>'deadline')::timestamptz<=clock_timestamp() THEN RETURN NULL; END IF;
 RETURN result;
END $$;

CREATE FUNCTION app.ack_stripe_checkout(p_purchase uuid,p_checkout text,p_payment text,p_url text,p_expires timestamptz,p_terms_hash text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player uuid; ticket app.purchase;
BEGIN
 SELECT account_id INTO STRICT player FROM app.purchase WHERE id=p_purchase;
 PERFORM id FROM app.account WHERE id=player FOR NO KEY UPDATE;
 SELECT * INTO STRICT ticket FROM app.purchase WHERE id=p_purchase FOR NO KEY UPDATE;
 IF (ticket.checkout_terms IS NULL OR app.stripe_key(ticket.checkout_terms::text) IS DISTINCT FROM p_terms_hash OR p_checkout !~ '^cs_[A-Za-z0-9_]+$'
 OR (p_payment IS NOT NULL AND p_payment !~ '^pi_[A-Za-z0-9_]+$')
 OR (ticket.stripe_checkout_ref IS NOT NULL AND ticket.stripe_checkout_ref<>p_checkout)
 OR (ticket.stripe_payment_ref IS NOT NULL AND ticket.stripe_payment_ref IS DISTINCT FROM p_payment)
 OR (p_url IS NOT NULL AND (p_url NOT LIKE 'https://checkout.stripe.com/%' OR length(p_url)>4096))) IS NOT FALSE THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='checkout acknowledgment differs'; END IF;
 UPDATE app.purchase SET stripe_checkout_ref=p_checkout,stripe_payment_ref=coalesce(stripe_payment_ref,p_payment),
 checkout_url=CASE WHEN state IN ('creating','pending') THEN p_url ELSE NULL END,
 checkout_expires_at=CASE WHEN state IN ('creating','pending') THEN p_expires ELSE NULL END,
 state=CASE WHEN state='creating' THEN 'pending' ELSE state END,revision=revision+1 WHERE id=p_purchase;
 RETURN app.read_stripe_purchase(p_purchase);
END $$;

CREATE FUNCTION app.capture_stripe_event(p_external text,p_type text,p_capture jsonb,p_delivery timestamptz) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source app.provider_event; source_id uuid; native boolean;
BEGIN
 native:=p_capture->>'kind'='webhook';
 IF (length(p_external)>255 OR p_external IS NULL OR p_type IS NULL OR length(p_type)>128
 OR jsonb_typeof(p_capture) IS DISTINCT FROM 'object' OR octet_length(p_capture::text)>16384
 OR p_capture->>'version' IS DISTINCT FROM '1' OR p_capture->>'bodyHash' !~ '^[0-9a-f]{64}$'
 OR p_capture->>'kind' NOT IN ('webhook','api_read')
 OR (native AND (p_external !~ '^evt_[A-Za-z0-9_]+$' OR p_delivery IS NULL))
 OR (NOT native AND (p_external !~ '^api-read:[0-9a-f]{64}$'
  OR jsonb_typeof(p_capture->'purchaseRevision') IS DISTINCT FROM 'string'
  OR p_capture->>'purchaseRevision' !~ '^(0|[1-9][0-9]{0,77})$'))) IS NOT FALSE THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid authenticated source capture'; END IF;
 INSERT INTO app.provider_event(provider,external_event_ref,received_at,event_type,payload_ref,processing_state,capture,first_delivery_at,raw_evidence_state)
 VALUES('stripe',p_external,clock_timestamp(),p_type,'payments/stripe/'||(p_capture->>'bodyHash')||'.json','received',p_capture,p_delivery,CASE WHEN native THEN 'pending' ELSE 'none' END)
 ON CONFLICT(provider,external_event_ref) DO NOTHING RETURNING id INTO source_id;
 SELECT * INTO STRICT source FROM app.provider_event WHERE provider='stripe' AND external_event_ref=p_external FOR UPDATE;
 IF source.capture IS DISTINCT FROM p_capture OR source.event_type<>p_type THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='authenticated event identity differs'; END IF;
 RETURN jsonb_build_object('id',source.id,'evidenceState',source.raw_evidence_state,'payloadRef',source.payload_ref,
 'expiresAt',source.received_at+interval '30 days','processed',source.processing_state='processed');
END $$;

CREATE FUNCTION app.confirm_stripe_evidence(p_event uuid,p_hash text) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 UPDATE app.provider_event SET raw_evidence_state=CASE WHEN received_at+interval '30 days'<=clock_timestamp() THEN 'expired' ELSE 'retained' END
 WHERE id=p_event AND provider='stripe' AND capture->>'bodyHash'=p_hash AND raw_evidence_state='pending';
 RETURN EXISTS(SELECT 1 FROM app.provider_event WHERE id=p_event AND provider='stripe'
 AND capture->>'bodyHash'=p_hash AND raw_evidence_state IN ('retained','expired'));
END $$;

CREATE OR REPLACE FUNCTION app.guard_provider_event() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' OR (NEW.id,NEW.provider,NEW.external_event_ref,NEW.received_at,NEW.event_type,NEW.payload_ref,NEW.capture,NEW.first_delivery_at)
 IS DISTINCT FROM (OLD.id,OLD.provider,OLD.external_event_ref,OLD.received_at,OLD.event_type,OLD.payload_ref,OLD.capture,OLD.first_delivery_at)
 OR (OLD.account_id IS NOT NULL AND NEW.account_id IS DISTINCT FROM OLD.account_id)
 OR (OLD.processing_state='processed' AND (NEW.processing_state,NEW.effect,NEW.effect_key) IS DISTINCT FROM (OLD.processing_state,OLD.effect,OLD.effect_key))
 OR (OLD.processing_state='verified' AND NEW.processing_state NOT IN ('verified','processed'))
 OR (OLD.raw_evidence_state IN ('none','expired') AND NEW.raw_evidence_state<>OLD.raw_evidence_state)
 OR (OLD.raw_evidence_state='retained' AND NEW.raw_evidence_state NOT IN ('retained','expired')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='provider source history is immutable'; END IF;
 RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION app.guard_purchase_proof() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source app.provider_event;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='purchase proof cannot be deleted'; END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 IF TG_OP='INSERT' THEN
  IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state='active') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account cannot start a purchase'; END IF;
 ELSE
  IF (NEW.id,NEW.account_id,NEW.cosmetic_id,NEW.provider_request_ref,NEW.requested_at,NEW.checkout_terms)
   IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.cosmetic_id,OLD.provider_request_ref,OLD.requested_at,OLD.checkout_terms)
  OR (OLD.stripe_checkout_ref IS NOT NULL AND NEW.stripe_checkout_ref IS DISTINCT FROM OLD.stripe_checkout_ref)
  OR (OLD.stripe_payment_ref IS NOT NULL AND NEW.stripe_payment_ref IS DISTINCT FROM OLD.stripe_payment_ref)
  OR (OLD.first_create_attempt_at IS NOT NULL AND (NEW.first_create_attempt_at,NEW.create_deadline_at) IS DISTINCT FROM (OLD.first_create_attempt_at,OLD.create_deadline_at)) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='purchase identity is immutable'; END IF;
  IF OLD.state IN ('refunded','reversed') AND NEW.state='paid' THEN
   SELECT * INTO source FROM app.provider_event WHERE id=NEW.provider_event_id;
   IF source.effect->>'kind' IS DISTINCT FROM 'restored'
    OR source.effect->>'reversalEventId' IS DISTINCT FROM OLD.current_reversal_event_id::text THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restored funds source required'; END IF;
  END IF;
 END IF;
 IF NEW.state IN ('paid','refunded','reversed') THEN PERFORM app.check_provider_source(NEW.provider_event_id,NEW.account_id,'stripe'); END IF;
 RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION app.guard_cosmetic_grant() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE lifecycle text;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='grant history is immutable'; END IF;
 IF TG_OP='UPDATE' AND ((NEW.id,NEW.account_id,NEW.cosmetic_id,NEW.source_kind,NEW.purchase_id,NEW.patreon_payment_id,NEW.provider_event_id)
 IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.cosmetic_id,OLD.source_kind,OLD.purchase_id,OLD.patreon_payment_id,OLD.provider_event_id)
 OR (OLD.state='revoked' AND NEW.state<>'revoked')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='grant provenance is immutable'; END IF;
 SELECT state INTO STRICT lifecycle FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 UPDATE app.account SET appearance_revision=appearance_revision+1 WHERE id=NEW.account_id;
 IF NEW.state='active' AND lifecycle='deleted' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deleted account cannot receive grants'; END IF;
 IF NEW.source_kind='free' THEN
  IF NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=NEW.cosmetic_id AND is_free) OR (NEW.state<>'active' AND lifecycle<>'deleted') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid free grant'; END IF;
 ELSIF NEW.source_kind='stripe_purchase' THEN
  IF NEW.state='active' AND NOT EXISTS(SELECT 1 FROM app.purchase WHERE id=NEW.purchase_id AND account_id=NEW.account_id
   AND cosmetic_id=NEW.cosmetic_id AND state='paid' AND provider_event_id=NEW.provider_event_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='matching paid source required'; END IF;
  IF NEW.state='revoked' AND lifecycle<>'deleted' AND NOT EXISTS(SELECT 1 FROM app.purchase WHERE id=NEW.purchase_id AND state IN ('refunded','reversed')) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reversed purchase required'; END IF;
 ELSIF NEW.source_kind='patreon_payment' THEN
  IF NEW.state='active' AND NOT EXISTS(SELECT 1 FROM app.patreon_payment p JOIN app.patreon_membership m ON m.id=p.membership_id
   JOIN app.tier_cosmetic t ON t.tier_id=p.tier_id AND t.cosmetic_id=NEW.cosmetic_id
   WHERE p.id=NEW.patreon_payment_id AND m.account_id=NEW.account_id AND p.state='paid') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='matching paid tier required'; END IF;
  IF NEW.state='revoked' AND lifecycle<>'deleted' AND NOT EXISTS(SELECT 1 FROM app.patreon_payment WHERE id=NEW.patreon_payment_id AND state IN ('refunded','reversed')) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reversed membership payment required'; END IF;
 END IF;
 RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION app.apply_purchase_cosmetics() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NEW.state='paid' AND (TG_OP='INSERT' OR OLD.state<>'paid') AND EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state<>'deleted') THEN
  INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,purchase_id,provider_event_id,state)
  VALUES(NEW.account_id,NEW.cosmetic_id,'stripe_purchase',NEW.id,NEW.provider_event_id,'active');
 ELSIF NEW.state IN ('refunded','reversed') THEN
  UPDATE app.cosmetic_grant SET state='revoked' WHERE purchase_id=NEW.id AND state='active';
 END IF;
 RETURN NULL;
END $$;

CREATE FUNCTION app.commit_stripe_effect(p_purchase uuid,p_event uuid,p_revision numeric,p_effect jsonb,p_checkout text,p_payment text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player uuid; lifecycle text; ticket app.purchase; source app.provider_event; duplicate app.provider_event;
 kind text:=p_effect->>'kind'; v_effect_key text:=p_effect->>'effectKey'; next_state text; disposition text:='applied'; grant_id uuid; result jsonb;
BEGIN
 SELECT account_id INTO STRICT player FROM app.purchase WHERE id=p_purchase;
 SELECT state INTO STRICT lifecycle FROM app.account WHERE id=player FOR NO KEY UPDATE;
 SELECT * INTO STRICT ticket FROM app.purchase WHERE id=p_purchase FOR NO KEY UPDATE;
 SELECT * INTO STRICT source FROM app.provider_event WHERE id=p_event AND provider='stripe' FOR UPDATE;
 IF source.processing_state='processed' THEN RETURN source.effect->'result'; END IF;
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

CREATE FUNCTION app.read_my_purchase(p_purchase uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); result jsonb;
BEGIN
 IF actor IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized')); END IF;
 SELECT jsonb_build_object('purchaseId',id,'cosmeticId',cosmetic_id,'createdAt',requested_at,'state',state,'revision',revision::text,
 'price',jsonb_build_object('currency',price_currency,'minorUnits',price_minor_units),
 'checkout',CASE WHEN state='pending' AND checkout_url IS NOT NULL AND checkout_expires_at>statement_timestamp()
 THEN jsonb_build_object('status','ready','url',checkout_url,'expiresAt',checkout_expires_at)
 WHEN state IN ('creating','pending') THEN jsonb_build_object('status','waiting') ELSE jsonb_build_object('status','unavailable') END)
 INTO result FROM app.purchase WHERE id=p_purchase AND account_id=actor;
 IF result IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('purchase','unavailable')); END IF;
 RETURN jsonb_build_object('status','success','data',result);
END
$$;
GRANT EXECUTE ON FUNCTION app.read_my_purchase(uuid) TO authenticated;

CREATE FUNCTION app.associate_stripe_notification(p_event uuid,p_purchase uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player uuid; native app.provider_event;
BEGIN
 SELECT account_id INTO STRICT player FROM app.purchase WHERE id=p_purchase;
 PERFORM id FROM app.account WHERE id=player FOR NO KEY UPDATE;
 PERFORM id FROM app.purchase WHERE id=p_purchase FOR NO KEY UPDATE;
 SELECT * INTO STRICT native FROM app.provider_event WHERE id=p_event AND provider='stripe' AND capture->>'kind'='webhook' FOR UPDATE;
 IF native.account_id IS NOT NULL AND native.account_id<>player
 OR (native.effect ? 'purchaseId' AND native.effect->>'purchaseId' IS DISTINCT FROM p_purchase::text) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification purchase differs'; END IF;
 IF native.processing_state<>'processed' THEN
  UPDATE app.provider_event SET account_id=player,effect=jsonb_build_object('purchaseId',p_purchase) WHERE id=p_event;
 END IF;
 RETURN jsonb_build_object('associated',true);
END $$;

-- Diagnostic obligations remain distinct from the final deletion gate.
CREATE OR REPLACE FUNCTION app.read_account_deletion_status(p_actor uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; uncommitted bigint; predictions bigint; purchases bigint;
 provider_events bigint; stripe_events bigint; matches bigint; holds bigint; commands bigint;
BEGIN
 SELECT * INTO player FROM app.account WHERE id=p_actor AND state='deletion_pending';
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='deletion account unavailable'; END IF;
 SELECT count(*) INTO uncommitted FROM (
  SELECT id FROM app.trade_offer WHERE state='offered' AND p_actor IN (sender_id,recipient_id)
  UNION ALL SELECT id FROM app.market_listing WHERE state='listed' AND seller_id=p_actor
  UNION ALL SELECT id FROM app.prediction_order WHERE state='open' AND account_id=p_actor
 ) work;
 SELECT count(DISTINCT m.id) INTO predictions FROM app.prediction_market m JOIN app.prediction_pair p ON p.market_id=m.id
 JOIN app.prediction_order o ON o.id IN (p.yes_order_id,p.no_order_id) WHERE o.account_id=p_actor AND m.state<>'resolved';
 SELECT count(*) INTO purchases FROM app.purchase WHERE account_id=p_actor AND state IN ('creating','pending');
 SELECT count(*) INTO provider_events FROM app.provider_event WHERE account_id=p_actor AND provider='patreon' AND processing_state='verified';
 SELECT count(*) INTO stripe_events FROM app.provider_event e WHERE e.provider='stripe' AND e.processing_state IN ('received','verified')
 AND (e.account_id=p_actor OR EXISTS(SELECT 1 FROM app.purchase p WHERE p.account_id=p_actor AND p.stripe_checkout_ref=e.capture->>'targetId'));
 SELECT count(DISTINCT m.id) INTO matches FROM app.match m JOIN app.match_seat s ON s.match_id=m.id
 WHERE p_actor IN (s.account_id,s.sponsor_id) AND NOT EXISTS(SELECT 1 FROM app.match_result r WHERE r.match_id=m.id);
 SELECT count(*) INTO holds FROM app.asset_hold h JOIN app.asset_owner o ON o.id=h.owner_id
 WHERE o.account_id=p_actor AND h.state='held' AND EXISTS(SELECT 1 FROM app.hold_line l WHERE l.hold_id=h.id AND l.quantity>0);
 SELECT count(*) INTO commands FROM app.command_receipt WHERE account_id=p_actor AND state='pending';
 RETURN jsonb_build_object('state','deletion_pending','requestedAt',
 to_char(player.deletion_requested_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
 'obligations',jsonb_build_object('uncommittedWork',uncommitted::text,'unsettledPredictionMarkets',predictions::text,
 'purchasesInFlight',purchases::text,'verifiedPatreonEvents',provider_events::text,'pendingStripeEvents',stripe_events::text,
 'matchesWithoutResult',matches::text,'heldCommitments',holds::text,'pendingCommands',commands::text));
END $$;

CREATE FUNCTION app.complete_stripe_notification(p_event uuid,p_observation uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE observation app.provider_event; native app.provider_event; player uuid; result jsonb;
BEGIN
 SELECT * INTO STRICT observation FROM app.provider_event WHERE id=p_observation AND provider='stripe' AND capture->>'kind'='api_read'
 AND processing_state='processed' AND effect->>'kind'<>'superseded';
 SELECT account_id INTO STRICT player FROM app.purchase WHERE id=(observation.effect->>'purchaseId')::uuid;
 PERFORM id FROM app.account WHERE id=player FOR NO KEY UPDATE;
 PERFORM id FROM app.purchase WHERE id=(observation.effect->>'purchaseId')::uuid FOR NO KEY UPDATE;
 SELECT * INTO STRICT native FROM app.provider_event WHERE id=p_event AND provider='stripe' AND capture->>'kind'='webhook' FOR UPDATE;
 IF native.processing_state='processed' THEN RETURN native.effect; END IF;
 IF native.account_id IS NOT NULL AND native.account_id<>player
 OR (native.effect ? 'purchaseId' AND native.effect->>'purchaseId' IS DISTINCT FROM observation.effect->>'purchaseId') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification account differs'; END IF;
 result:=jsonb_build_object('kind','observed','observationId',p_observation,'purchaseId',observation.effect->>'purchaseId');
 UPDATE app.provider_event SET account_id=player,processing_state='processed',effect=result WHERE id=p_event;
 RETURN result;
END $$;
CREATE FUNCTION app.ignore_stripe_notification(p_event uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM app.provider_event WHERE id=p_event AND event_type IN
 ('checkout.session.completed','checkout.session.async_payment_succeeded','checkout.session.async_payment_failed','checkout.session.expired',
 'charge.succeeded','charge.refunded','charge.updated','refund.created','refund.updated','refund.failed',
 'charge.dispute.created','charge.dispute.updated','charge.dispute.closed','charge.dispute.funds_withdrawn','charge.dispute.funds_reinstated')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='supported event requires reconciliation'; END IF;
 UPDATE app.provider_event SET processing_state='processed',effect=jsonb_build_object('kind','unsupported') WHERE id=p_event AND provider='stripe' AND processing_state='received';
 RETURN jsonb_build_object('ignored',true);
END $$;
CREATE FUNCTION app.read_stripe_work(p_kind text,p_after uuid,p_limit integer,p_bucket text DEFAULT NULL) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 IF p_kind IS NULL OR p_kind NOT IN ('purchase','event','evidence') OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid stripe work page'; END IF;
 IF p_kind='purchase' THEN
  SELECT coalesce(jsonb_agg(id ORDER BY id),'[]') INTO result FROM (SELECT id FROM app.purchase WHERE checkout_terms IS NOT NULL AND (p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 ELSIF p_kind='event' THEN
  SELECT coalesce(jsonb_agg(id ORDER BY id),'[]') INTO result FROM (SELECT id FROM app.provider_event WHERE provider='stripe' AND capture->>'kind'='webhook' AND processing_state='received' AND (p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 ELSE
  IF p_bucket IS NULL OR p_bucket !~ '^[A-Za-z0-9_-]{1,100}$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='private evidence bucket required'; END IF;
  IF pg_catalog.to_regclass('storage.objects') IS NOT NULL THEN
   -- Storage metadata is read-only here. A late in-flight upload can recreate
   -- an object after a successful deletion, including after a process crash.
   -- Its actual metadata presence makes that source due again. All object
   -- deletion still goes through Storage's API and positive absence readback.
   EXECUTE $query$SELECT coalesce(jsonb_agg(id ORDER BY id),'[]') FROM
    (SELECT e.id FROM app.provider_event e WHERE e.provider='stripe' AND e.capture->>'kind'='webhook'
     AND e.received_at+interval '30 days'<=statement_timestamp() AND ($1 IS NULL OR e.id>$1)
     AND (e.raw_deleted_at IS NULL OR EXISTS(SELECT 1 FROM storage.objects o WHERE o.bucket_id=$3 AND o.name=e.payload_ref))
     ORDER BY e.id LIMIT $2) page$query$ INTO result USING p_after,p_limit,p_bucket;
  ELSE
   SELECT coalesce(jsonb_agg(id ORDER BY id),'[]') INTO result FROM (SELECT id FROM app.provider_event WHERE provider='stripe' AND capture->>'kind'='webhook'
    AND received_at+interval '30 days'<=statement_timestamp() AND raw_deleted_at IS NULL AND (p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
  END IF;
 END IF;
 RETURN result;
END $$;
CREATE FUNCTION app.read_stripe_notification(p_event uuid) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('id',id,'eventType',event_type,'capture',capture,'evidenceState',raw_evidence_state,'expiresAt',received_at+interval '30 days','payloadRef',payload_ref)
 FROM app.provider_event WHERE id=p_event AND provider='stripe' AND capture->>'kind'='webhook' AND processing_state='received'
$$;

CREATE FUNCTION app.begin_stripe_evidence_expiry(p_event uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source app.provider_event;
BEGIN
 SELECT * INTO source FROM app.provider_event WHERE id=p_event AND provider='stripe' AND capture->>'kind'='webhook' FOR UPDATE;
 IF source.id IS NULL OR source.received_at+interval '30 days'>clock_timestamp() THEN RETURN NULL; END IF;
 UPDATE app.provider_event SET raw_evidence_state='expired' WHERE id=p_event;
 RETURN jsonb_build_object('payloadRef',source.payload_ref,'bodyHash',source.capture->>'bodyHash');
END $$;
CREATE FUNCTION app.finish_stripe_evidence_expiry(p_event uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 UPDATE app.provider_event SET raw_deleted_at=coalesce(raw_deleted_at,clock_timestamp()) WHERE id=p_event AND provider='stripe' AND raw_evidence_state='expired'
 AND received_at+interval '30 days'<=clock_timestamp();
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='evidence expiry unavailable'; END IF;
END $$;

REVOKE EXECUTE ON FUNCTION app.stripe_key(text),app.stripe_actor(uuid,uuid),app.guard_purchase_proof(),app.guard_provider_event(),
 app.guard_cosmetic_grant(),app.apply_purchase_cosmetics() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_stripe_reservation(uuid,uuid,uuid,uuid),app.reserve_stripe_purchase(uuid,uuid,uuid,uuid,jsonb),
 app.read_stripe_purchase(uuid),app.claim_stripe_creation(uuid),app.ack_stripe_checkout(uuid,text,text,text,timestamptz,text),
 app.capture_stripe_event(text,text,jsonb,timestamptz),app.confirm_stripe_evidence(uuid,text),
 app.commit_stripe_effect(uuid,uuid,numeric,jsonb,text,text) TO service_role;
GRANT EXECUTE ON FUNCTION app.complete_stripe_notification(uuid,uuid),app.ignore_stripe_notification(uuid),
 app.read_stripe_work(text,uuid,integer,text),app.read_stripe_notification(uuid),app.associate_stripe_notification(uuid,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.begin_stripe_evidence_expiry(uuid),app.finish_stripe_evidence_expiry(uuid) TO service_role;
COMMIT;
