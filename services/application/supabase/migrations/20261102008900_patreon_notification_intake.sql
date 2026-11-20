BEGIN;

-- Shared private mechanics retain each domain wrapper's existing source predicates.
CREATE FUNCTION app.provider_evidence_matches(p_provider text,p_capture jsonb,p_type text) RETURNS boolean
LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT p_provider='stripe' OR (p_provider='patreon' AND p_type='member.notification'
  AND p_capture->>'kind'='webhook' AND p_capture->>'source'='patreon-v2-member-notification-v1')
$$;
CREATE FUNCTION app.confirm_provider_evidence(p_provider text,p_event uuid,p_hash text) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_provider IS NULL OR p_provider NOT IN ('stripe','patreon') THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid evidence provider'; END IF;
 -- UPDATE target expressions may be evaluated before a conflicting row lock.
 -- Acquire the same row lock first so expiry is decided after the actual wait.
 PERFORM id FROM app.provider_event WHERE id=p_event AND provider=p_provider
 AND app.provider_evidence_matches(provider,capture,event_type)
 AND capture->>'bodyHash'=p_hash AND raw_evidence_state='pending' FOR NO KEY UPDATE;
 UPDATE app.provider_event SET raw_evidence_state=CASE WHEN received_at+interval '30 days'<=clock_timestamp() THEN 'expired' ELSE 'retained' END
 WHERE id=p_event AND provider=p_provider AND app.provider_evidence_matches(provider,capture,event_type)
 AND capture->>'bodyHash'=p_hash AND raw_evidence_state='pending';
 RETURN EXISTS(SELECT 1 FROM app.provider_event WHERE id=p_event AND provider=p_provider
 AND app.provider_evidence_matches(provider,capture,event_type) AND capture->>'bodyHash'=p_hash AND raw_evidence_state IN ('retained','expired'));
END $$;
CREATE FUNCTION app.begin_provider_evidence_expiry(p_provider text,p_event uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source app.provider_event;
BEGIN
 IF p_provider IS NULL OR p_provider NOT IN ('stripe','patreon') THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid evidence provider'; END IF;
 SELECT * INTO source FROM app.provider_event WHERE id=p_event AND provider=p_provider AND capture->>'kind'='webhook'
 AND app.provider_evidence_matches(provider,capture,event_type) FOR UPDATE;
 IF source.id IS NULL OR source.received_at+interval '30 days'>clock_timestamp() THEN RETURN NULL; END IF;
 UPDATE app.provider_event SET raw_evidence_state='expired' WHERE id=p_event;
 RETURN jsonb_build_object('payloadRef',source.payload_ref,'bodyHash',source.capture->>'bodyHash');
END $$;
CREATE FUNCTION app.finish_provider_evidence_expiry(p_provider text,p_event uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_provider IS NULL OR p_provider NOT IN ('stripe','patreon') THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid evidence provider'; END IF;
 UPDATE app.provider_event SET raw_deleted_at=coalesce(raw_deleted_at,clock_timestamp()) WHERE id=p_event AND provider=p_provider
 AND app.provider_evidence_matches(provider,capture,event_type) AND raw_evidence_state='expired' AND received_at+interval '30 days'<=clock_timestamp();
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='evidence expiry unavailable'; END IF;
END $$;
CREATE FUNCTION app.read_provider_evidence_work(p_provider text,p_after uuid,p_limit integer,p_bucket text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 IF p_provider IS NULL OR p_provider NOT IN ('stripe','patreon') OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100
 OR p_bucket IS NULL OR p_bucket !~ '^[A-Za-z0-9_-]{1,100}$' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid evidence work page'; END IF;
 IF pg_catalog.to_regclass('storage.objects') IS NOT NULL THEN
  -- A late upload can recreate an expired object after deletion acknowledgment.
  -- Metadata is read-only here; deletion and positive absence use Storage's API.
  EXECUTE $query$SELECT coalesce(jsonb_agg(id ORDER BY id),'[]') FROM
   (SELECT e.id FROM app.provider_event e WHERE e.provider=$4 AND e.capture->>'kind'='webhook'
    AND app.provider_evidence_matches(e.provider,e.capture,e.event_type)
    AND e.received_at+interval '30 days'<=statement_timestamp() AND ($1 IS NULL OR e.id>$1)
    AND (e.raw_deleted_at IS NULL OR EXISTS(SELECT 1 FROM storage.objects o WHERE o.bucket_id=$3 AND o.name=e.payload_ref))
    ORDER BY e.id LIMIT $2) page$query$ INTO result USING p_after,p_limit,p_bucket,p_provider;
 ELSE
  SELECT coalesce(jsonb_agg(id ORDER BY id),'[]') INTO result FROM
   (SELECT id FROM app.provider_event WHERE provider=p_provider AND capture->>'kind'='webhook'
    AND app.provider_evidence_matches(provider,capture,event_type)
    AND received_at+interval '30 days'<=statement_timestamp() AND raw_deleted_at IS NULL
    AND (p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 END IF;
 RETURN result;
END $$;

-- Keep Stripe's signatures, non-webhook confirmation/finish, NULLs and inherited timezone.
CREATE OR REPLACE FUNCTION app.confirm_stripe_evidence(p_event uuid,p_hash text) RETURNS boolean
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.confirm_provider_evidence('stripe',p_event,p_hash) $$;
CREATE OR REPLACE FUNCTION app.begin_stripe_evidence_expiry(p_event uuid) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.begin_provider_evidence_expiry('stripe',p_event) $$;
CREATE OR REPLACE FUNCTION app.finish_stripe_evidence_expiry(p_event uuid) RETURNS void
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.finish_provider_evidence_expiry('stripe',p_event) $$;
CREATE OR REPLACE FUNCTION app.read_stripe_work(p_kind text,p_after uuid,p_limit integer,p_bucket text DEFAULT NULL) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 IF p_kind IS NULL OR p_kind NOT IN ('purchase','event','evidence') OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid stripe work page'; END IF;
 IF p_kind='purchase' THEN
  SELECT coalesce(jsonb_agg(id ORDER BY id),'[]') INTO result FROM (SELECT id FROM app.purchase WHERE checkout_terms IS NOT NULL AND (p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 ELSIF p_kind='event' THEN
  SELECT coalesce(jsonb_agg(id ORDER BY id),'[]') INTO result FROM (SELECT id FROM app.provider_event WHERE provider='stripe' AND capture->>'kind'='webhook' AND processing_state='received' AND (p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 ELSE
  result:=app.read_provider_evidence_work('stripe',p_after,p_limit,p_bucket);
 END IF;
 RETURN result;
END $$;

CREATE FUNCTION app.patreon_notification_id(p_id uuid) RETURNS uuid
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF p_id IS NULL OR p_id='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='notification identity required'; END IF;
 RETURN p_id;
END $$;
CREATE FUNCTION app.patreon_notification_capture(p jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF jsonb_typeof(p) IS DISTINCT FROM 'object' OR octet_length(p::text)>16384 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid authenticated notification capture'; END IF;
 IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p) k) IS DISTINCT FROM
 ARRAY['bodyHash','campaignId','kind','memberId','source','userId','version']::text[]
 OR p->'version' IS DISTINCT FROM '1'::jsonb OR p->>'kind' IS DISTINCT FROM 'webhook'
 OR p->>'source' IS DISTINCT FROM 'patreon-v2-member-notification-v1'
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['bodyHash','campaignId','kind','memberId','source','userId']) f WHERE jsonb_typeof(p->f) IS DISTINCT FROM 'string')
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['campaignId','memberId','userId']) f WHERE ((p->>f) COLLATE "C" ~ '^[A-Za-z0-9_-]{1,128}$') IS DISTINCT FROM true)
 OR ((p->>'bodyHash') COLLATE "C" ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid authenticated notification capture'; END IF;
 RETURN p;
END $$;
CREATE INDEX patreon_reconciliation_notifications ON app.provider_event(id)
 WHERE provider='patreon' AND event_type='member.notification' AND processing_state='received';
CREATE FUNCTION app.capture_patreon_notification(p_delivery uuid,p_capture jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE source app.provider_event; payload text;
BEGIN
 PERFORM app.patreon_notification_id(p_delivery); p_capture:=app.patreon_notification_capture(p_capture);
 payload:='payments/patreon/'||p_delivery::text||'/'||(p_capture->>'bodyHash')||'.json';
 INSERT INTO app.provider_event(id,provider,external_event_ref,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state)
 VALUES(p_delivery,'patreon','notification:'||p_delivery::text,clock_timestamp(),'member.notification',payload,'received',p_capture,'pending')
 ON CONFLICT DO NOTHING;
 SELECT * INTO source FROM app.provider_event WHERE id=p_delivery FOR UPDATE;
 IF source.id IS NULL OR source.provider<>'patreon' OR source.external_event_ref IS DISTINCT FROM 'notification:'||p_delivery::text
 OR source.event_type<>'member.notification' OR source.capture IS DISTINCT FROM p_capture OR source.payload_ref IS DISTINCT FROM payload
 OR source.first_delivery_at IS NOT NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification occurrence terms differ'; END IF;
 RETURN jsonb_build_object('id',source.id,'evidenceState',source.raw_evidence_state,'payloadRef',source.payload_ref,
  'expiresAt',source.received_at+interval '30 days','processed',source.processing_state='processed');
END $$;
CREATE FUNCTION app.confirm_patreon_evidence(p_event uuid,p_hash text) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
BEGIN
 PERFORM app.patreon_notification_id(p_event);
 IF (p_hash COLLATE "C" ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid evidence hash'; END IF;
 RETURN app.confirm_provider_evidence('patreon',p_event,p_hash);
END $$;
CREATE FUNCTION app.read_patreon_work(p_kind text,p_after uuid,p_limit integer,p_bucket text DEFAULT NULL) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE result jsonb;
BEGIN
 IF p_kind IS NULL OR p_kind NOT IN ('event','evidence') OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100
 OR (p_bucket IS NOT NULL AND p_bucket COLLATE "C" !~ '^[A-Za-z0-9_-]{1,100}$') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid patreon work page'; END IF;
 IF p_after IS NOT NULL THEN PERFORM app.patreon_notification_id(p_after); END IF;
 IF p_kind='evidence' THEN RETURN app.read_provider_evidence_work('patreon',p_after,p_limit,p_bucket); END IF;
 SELECT coalesce(jsonb_agg(id ORDER BY id),'[]') INTO result FROM
  (SELECT id FROM app.provider_event WHERE provider='patreon' AND event_type='member.notification'
   AND app.provider_evidence_matches(provider,capture,event_type) AND processing_state='received'
   AND (p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 RETURN result;
END $$;
CREATE FUNCTION app.read_patreon_notification(p_event uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE result jsonb;
BEGIN
 PERFORM app.patreon_notification_id(p_event);
 SELECT jsonb_build_object('id',id,'eventType',event_type,'capture',capture,'evidenceState',raw_evidence_state,
  'expiresAt',received_at+interval '30 days','payloadRef',payload_ref) INTO result
 FROM app.provider_event WHERE id=p_event AND provider='patreon' AND app.provider_evidence_matches(provider,capture,event_type)
 AND processing_state='received';
 RETURN result;
END $$;
CREATE FUNCTION app.begin_patreon_evidence_expiry(p_event uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
BEGIN
 PERFORM app.patreon_notification_id(p_event);
 RETURN app.begin_provider_evidence_expiry('patreon',p_event);
END $$;
CREATE FUNCTION app.finish_patreon_evidence_expiry(p_event uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
BEGIN
 PERFORM app.patreon_notification_id(p_event);
 PERFORM app.finish_provider_evidence_expiry('patreon',p_event);
END $$;
REVOKE ALL ON FUNCTION app.provider_evidence_matches(text,jsonb,text),app.confirm_provider_evidence(text,uuid,text),
 app.begin_provider_evidence_expiry(text,uuid),app.finish_provider_evidence_expiry(text,uuid),app.read_provider_evidence_work(text,uuid,integer,text),
 app.patreon_notification_id(uuid),app.patreon_notification_capture(jsonb),app.capture_patreon_notification(uuid,jsonb),
 app.confirm_patreon_evidence(uuid,text),app.read_patreon_work(text,uuid,integer,text),app.read_patreon_notification(uuid),
 app.begin_patreon_evidence_expiry(uuid),app.finish_patreon_evidence_expiry(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.capture_patreon_notification(uuid,jsonb),app.confirm_patreon_evidence(uuid,text),
 app.read_patreon_work(text,uuid,integer,text),app.read_patreon_notification(uuid),
 app.begin_patreon_evidence_expiry(uuid),app.finish_patreon_evidence_expiry(uuid) TO service_role;
COMMIT;
