BEGIN;
CREATE UNIQUE INDEX patreon_financial_occurrence ON app.provider_event(effect_key)
 WHERE provider='patreon' AND effect->>'kind'='patreon_financial_v1' AND effect->>'canonical'='true';
CREATE FUNCTION app.financial_instant(p_value text) RETURNS timestamptz
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE stamp timestamptz;
BEGIN
 IF p_value IS NULL OR p_value !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}Z$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='financial time must be canonical UTC'; END IF;
 BEGIN stamp:=p_value::timestamptz; EXCEPTION WHEN invalid_datetime_format OR datetime_field_overflow THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid financial time'; END;
 IF NOT isfinite(stamp) OR to_char(stamp AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')<>p_value THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid canonical financial time'; END IF;
 RETURN stamp;
END $$;

-- Strict normalized financial evidence, distinctly separate from OAuth links,
-- membership snapshots and notifications. External financial truth/signatures
-- must be qualified by the caller before this private port is activated.
CREATE FUNCTION app.commit_patreon_financial(p_delivery text,p_proof jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE membership app.patreon_membership; payment app.patreon_payment; source app.provider_event; canonical app.provider_event;
 tier uuid; owner uuid; lifecycle app.account; started timestamptz; paid timestamptz; occurred timestamptz;
 effect_identity text; source_id uuid; payment_id uuid; disposition text; result jsonb; core jsonb; original app.provider_event;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='financial acceptance requires read committed retry'; END IF;
 IF p_delivery IS NULL OR length(btrim(p_delivery)) NOT BETWEEN 1 AND 200 OR jsonb_typeof(p_proof) IS DISTINCT FROM 'object' OR octet_length(p_proof::text)>65536 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid financial source envelope'; END IF;
 IF p_proof->>'family' IS DISTINCT FROM 'patreon-financial-v1'
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['occurrenceRef','memberRef','paymentRef','tierRef','initiatedAt','paidAt','occurredAt','state','amountMinor','currency','bodyHash','evidenceRef']) field WHERE jsonb_typeof(p_proof->field) IS DISTINCT FROM 'string')
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_proof) field WHERE field NOT IN ('family','occurrenceRef','memberRef','paymentRef','tierRef','initiatedAt','paidAt','occurredAt','state','amountMinor','currency','bodyHash','evidenceRef'))
 OR length(btrim(p_proof->>'occurrenceRef')) NOT BETWEEN 1 AND 256 OR length(btrim(p_proof->>'memberRef')) NOT BETWEEN 1 AND 256
 OR length(btrim(p_proof->>'paymentRef')) NOT BETWEEN 1 AND 256 OR length(btrim(p_proof->>'tierRef')) NOT BETWEEN 1 AND 256
 OR p_proof->>'state' NOT IN ('paid','refunded','reversed') OR p_proof->>'amountMinor' !~ '^[1-9][0-9]{0,29}$' OR p_proof->>'currency' !~ '^[a-z]{3}$'
 OR p_proof->>'bodyHash' !~ '^[0-9a-f]{64}$' OR length(btrim(p_proof->>'evidenceRef')) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified financial source family required'; END IF;
 started:=app.financial_instant(p_proof->>'initiatedAt');paid:=app.financial_instant(p_proof->>'paidAt');occurred:=app.financial_instant(p_proof->>'occurredAt');
 IF started>paid OR paid>occurred OR occurred>clock_timestamp() OR (p_proof->>'state'='paid' AND paid<>occurred) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='financial occurrence times conflict'; END IF;
 SELECT * INTO payment FROM app.patreon_payment WHERE payment_ref=p_proof->>'paymentRef';
 IF payment.id IS NOT NULL THEN SELECT * INTO membership FROM app.patreon_membership WHERE id=payment.membership_id;
 ELSE SELECT * INTO membership FROM app.patreon_membership WHERE patreon_member_ref=p_proof->>'memberRef'; END IF;
 IF membership.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='financial association unavailable'; END IF;
 owner:=membership.account_id;
 SELECT * INTO STRICT lifecycle FROM app.account WHERE id=owner FOR NO KEY UPDATE;
 SELECT * INTO STRICT membership FROM app.patreon_membership WHERE id=membership.id FOR SHARE;
 effect_identity:=encode(sha256(convert_to('patreon-financial-v1:'||(p_proof->>'occurrenceRef'),'UTF8')),'hex');
 PERFORM pg_advisory_xact_lock(hashtextextended('patreon-financial-v1:'||effect_identity,0));
 SELECT * INTO canonical FROM app.provider_event WHERE provider='patreon' AND effect_key=effect_identity AND effect->>'kind'='patreon_financial_v1' AND effect->>'canonical'='true' FOR UPDATE;
 core:=p_proof-ARRAY['bodyHash','evidenceRef'];
 IF canonical.id IS NOT NULL AND (canonical.account_id IS DISTINCT FROM owner OR canonical.capture-ARRAY['bodyHash','evidenceRef'] IS DISTINCT FROM core OR canonical.effect->>'membershipId' IS DISTINCT FROM membership.id::text) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='financial occurrence was retargeted'; END IF;
 SELECT * INTO source FROM app.provider_event WHERE provider='patreon' AND external_event_ref='financial:'||p_delivery FOR UPDATE;
 IF source.id IS NOT NULL THEN
  IF source.capture IS DISTINCT FROM p_proof OR source.account_id IS DISTINCT FROM owner OR source.event_type<>'financial.observation' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='financial delivery terms conflict'; END IF;
  IF source.processing_state='processed' THEN RETURN source.effect->'result'; END IF;
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='financial delivery is not terminal';
 END IF;
 SELECT id INTO tier FROM app.supporter_tier WHERE patreon_tier_ref=p_proof->>'tierRef' FOR SHARE;
 IF tier IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='financial tier unavailable'; END IF;
 INSERT INTO app.provider_event(provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state,effect_key)
 VALUES('patreon','financial:'||p_delivery,owner,clock_timestamp(),'financial.observation','sha256:'||app.auth_json_hash(p_proof),'verified',p_proof,'none',effect_identity) RETURNING id INTO source_id;
 IF canonical.id IS NOT NULL THEN
  result:=canonical.effect->'result';
  UPDATE app.provider_event SET processing_state='processed',effect=jsonb_build_object('kind','patreon_financial_v1','canonical',false,'membershipId',membership.id,'disposition','duplicate','canonicalSourceId',canonical.id,'result',result) WHERE id=source_id;
  RETURN result;
 END IF;
 SELECT * INTO payment FROM app.patreon_payment WHERE payment_ref=p_proof->>'paymentRef' FOR UPDATE;
 IF payment.id IS NOT NULL THEN
  SELECT * INTO STRICT original FROM app.provider_event WHERE id=payment.provider_event_id;
  IF payment.membership_id<>membership.id OR payment.tier_id<>tier OR payment.paid_at<>paid
    OR original.capture->>'memberRef' IS DISTINCT FROM p_proof->>'memberRef'
    OR original.capture->>'initiatedAt' IS DISTINCT FROM p_proof->>'initiatedAt'
    OR original.capture->>'amountMinor' IS DISTINCT FROM p_proof->>'amountMinor' OR original.capture->>'currency' IS DISTINCT FROM p_proof->>'currency' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='financial payment identity differs'; END IF;
 ELSIF membership.patreon_member_ref IS DISTINCT FROM p_proof->>'memberRef' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='financial member differs'; END IF;
 -- A source begun after deletion intent, or first seen after final deletion,
 -- remains an acknowledged financial observation and creates no new benefits.
 IF payment.id IS NULL AND (lifecycle.state='deleted' OR (lifecycle.state='deletion_pending' AND started>lifecycle.deletion_requested_at)) THEN
  disposition:='ineligible_after_deletion';
 ELSIF payment.id IS NULL THEN
  INSERT INTO app.patreon_payment(membership_id,tier_id,provider_event_id,payment_ref,paid_at,state)
  VALUES(membership.id,tier,source_id,p_proof->>'paymentRef',paid,p_proof->>'state') ON CONFLICT(payment_ref) DO NOTHING RETURNING id INTO payment_id;
  IF payment_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='financial payment identity raced; retry'; END IF;
  disposition:='applied';
 ELSE
  payment_id:=payment.id;
  IF payment.state='paid' AND p_proof->>'state' IN ('refunded','reversed') THEN
   UPDATE app.patreon_payment SET state=p_proof->>'state',provider_event_id=source_id WHERE id=payment.id;
   disposition:='applied';
  ELSE disposition:='noop'; END IF;
 END IF;
 result:=jsonb_build_object('paymentId',payment_id,'disposition',disposition,'state',CASE WHEN disposition='noop' THEN payment.state ELSE p_proof->>'state' END);
 UPDATE app.provider_event SET processing_state='processed',effect=jsonb_build_object('kind','patreon_financial_v1','canonical',true,'membershipId',membership.id,'disposition',disposition,'result',result) WHERE id=source_id;
 RETURN result;
END $$;

CREATE OR REPLACE FUNCTION app.guard_patreon_payment() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE owner_account uuid; source app.provider_event; membership app.patreon_membership; tier_ref text;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Patreon payment proof cannot be deleted'; END IF;
 SELECT * INTO STRICT membership FROM app.patreon_membership WHERE id=NEW.membership_id;
 owner_account:=membership.account_id;
 PERFORM id FROM app.account WHERE id=owner_account FOR NO KEY UPDATE;
 IF TG_OP='UPDATE' AND ((NEW.id,NEW.membership_id,NEW.tier_id,NEW.payment_ref,NEW.paid_at) IS DISTINCT FROM (OLD.id,OLD.membership_id,OLD.tier_id,OLD.payment_ref,OLD.paid_at)
  OR (OLD.state IN ('refunded','reversed') AND NEW.state IS DISTINCT FROM OLD.state)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Patreon payment source or reversal cannot change'; END IF;
 PERFORM app.check_provider_source(NEW.provider_event_id,owner_account,'patreon');
 SELECT * INTO STRICT source FROM app.provider_event WHERE id=NEW.provider_event_id FOR SHARE;
 SELECT patreon_tier_ref INTO STRICT tier_ref FROM app.supporter_tier WHERE id=NEW.tier_id FOR SHARE;
 IF source.event_type<>'financial.observation' OR source.capture->>'family' IS DISTINCT FROM 'patreon-financial-v1' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Patreon payment requires financial evidence rather than membership status'; END IF;
 IF source.capture->>'paymentRef' IS DISTINCT FROM NEW.payment_ref OR source.capture->>'tierRef' IS DISTINCT FROM tier_ref
 OR app.financial_instant(source.capture->>'paidAt') IS DISTINCT FROM NEW.paid_at
 OR source.capture->>'state' IS DISTINCT FROM NEW.state OR source.account_id IS DISTINCT FROM owner_account
 OR (TG_OP='INSERT' AND source.capture->>'memberRef' IS DISTINCT FROM membership.patreon_member_ref) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Patreon payment requires its exact financial source'; END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION app.guard_supporter_tier_identity() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF NEW.patreon_tier_ref IS DISTINCT FROM OLD.patreon_tier_ref AND current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='financial tier source changes require read committed retry'; END IF;
 IF NEW.id IS DISTINCT FROM OLD.id OR (NEW.patreon_tier_ref IS DISTINCT FROM OLD.patreon_tier_ref AND EXISTS(SELECT 1 FROM app.patreon_payment WHERE tier_id=OLD.id)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted financial tier identity is immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER supporter_tier_identity BEFORE UPDATE ON app.supporter_tier FOR EACH ROW EXECUTE FUNCTION app.guard_supporter_tier_identity();

REVOKE ALL ON FUNCTION app.guard_supporter_tier_identity(),app.financial_instant(text),app.commit_patreon_financial(text,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
