-- Current entitlement refresh only; no payment/grant/notification completion.
BEGIN;
CREATE FUNCTION app.patreon_oauth_membership_source(p_new app.patreon_membership,p_old app.patreon_membership,p_op text) RETURNS app.patreon_membership
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source app.provider_event; proof jsonb; expected_tier uuid;
BEGIN
 IF p_op='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership proof cannot be deleted'; END IF;
 IF p_op='UPDATE' AND (p_new.id,p_new.account_id) IS DISTINCT FROM (p_old.id,p_old.account_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership association is immutable'; END IF;
 PERFORM id FROM app.account WHERE id=p_new.account_id FOR NO KEY UPDATE;
 PERFORM app.check_provider_source(p_new.source_event_id,p_new.account_id,'patreon');
 IF (p_op='INSERT' OR p_new.active) AND NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_new.account_id AND state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account cannot activate a membership'; END IF;
 IF p_new.patreon_user_ref IS NULL THEN
  IF p_op='UPDATE' AND (p_old.patreon_user_ref IS NOT NULL OR p_new.patreon_member_ref IS DISTINCT FROM p_old.patreon_member_ref) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership association is immutable'; END IF;
  RETURN p_new;
 END IF;
 IF p_op='UPDATE' AND (p_old.patreon_user_ref IS NULL OR (p_new.patreon_user_ref,p_new.patreon_campaign_ref)
  IS DISTINCT FROM (p_old.patreon_user_ref,p_old.patreon_campaign_ref)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership association is immutable'; END IF;
 SELECT * INTO source FROM app.provider_event WHERE id=p_new.source_event_id; proof:=app.patreon_proof(source.capture);
 expected_tier:=app.patreon_entitled_tier(proof);
 IF proof IS NULL OR source.event_type IS DISTINCT FROM 'oauth.association' OR source.effect->>'kind' IS DISTINCT FROM 'patreon_link_v1'
 OR source.effect->>'associationId' IS DISTINCT FROM p_new.id::text OR proof->>'userId' IS DISTINCT FROM p_new.patreon_user_ref
 OR proof->>'campaignId' IS DISTINCT FROM p_new.patreon_campaign_ref OR proof->>'memberId' IS DISTINCT FROM p_new.patreon_member_ref
 OR expected_tier IS DISTINCT FROM p_new.active_tier_id OR p_new.active IS DISTINCT FROM (expected_tier IS NOT NULL) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership requires qualified association source'; END IF;
 RETURN p_new;
END $$;
CREATE FUNCTION app.patreon_oauth_commit_source(p_new app.patreon_membership) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE e app.provider_event; done app.command_receipt; claim app.command_receipt; start app.command_receipt; data jsonb;
BEGIN
 IF p_new.patreon_user_ref IS NULL THEN RETURN; END IF;
 SELECT * INTO STRICT e FROM app.provider_event WHERE id=p_new.source_event_id;
 SELECT * INTO done FROM app.command_receipt WHERE principal_key='human:'||p_new.account_id::text AND scope='patreon_complete' AND request_ref=e.effect->>'startId';
 SELECT * INTO claim FROM app.command_receipt WHERE principal_key=done.principal_key AND scope='patreon_exchange_claim' AND request_ref=done.request_ref;
 SELECT * INTO start FROM app.command_receipt WHERE principal_key=done.principal_key AND scope='patreon_start' AND request_ref=done.request_ref;
 data:=done.result_ref::jsonb;
 IF done.id IS NULL OR done.state<>'accepted' OR done.account_id IS DISTINCT FROM p_new.account_id OR done.target_ref IS DISTINCT FROM p_new.account_id::text
 OR done.payload_hash IS DISTINCT FROM claim.payload_hash OR done.controller_grant_id IS NOT NULL OR done.operation_id IS NOT NULL
 OR claim.id IS NULL OR claim.state<>'accepted' OR start.id IS NULL OR start.state<>'accepted'
 OR claim.account_id IS DISTINCT FROM p_new.account_id OR start.account_id IS DISTINCT FROM p_new.account_id
 OR e.processing_state<>'processed' OR e.account_id IS DISTINCT FROM p_new.account_id OR e.provider<>'patreon'
 OR e.external_event_ref IS DISTINCT FROM 'oauth:'||p_new.account_id::text||':'||done.request_ref
 OR e.payload_ref IS DISTINCT FROM 'sha256:'||app.auth_json_hash(e.capture) OR data->>'providerEventId' IS DISTINCT FROM e.id::text
 OR data->>'associationId' IS DISTINCT FROM p_new.id::text OR data->>'proofHash' IS DISTINCT FROM app.auth_json_hash(e.capture)
 OR e.effect->>'generation' IS DISTINCT FROM start.result_ref::jsonb->>'generation'
 OR done.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_new.id)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='association requires completed source transaction'; END IF;
 RETURN;
END $$;

CREATE FUNCTION app.patreon_member_fields(p jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE key text; ids jsonb;
BEGIN
 IF jsonb_typeof(p) IS DISTINCT FROM 'object' OR octet_length(p::text)>262144 THEN RETURN NULL; END IF;
 FOREACH key IN ARRAY ARRAY['campaignId','userId','memberId'] LOOP
  IF jsonb_typeof(p->key) IS DISTINCT FROM 'string' OR ((p->>key) COLLATE "C") !~ '^[A-Za-z0-9_-]{1,128}$' THEN RETURN NULL; END IF;
 END LOOP;
 IF jsonb_typeof(p->'memberHash') IS DISTINCT FROM 'string' OR ((p->>'memberHash') COLLATE "C") !~ '^[0-9a-f]{64}$'
 OR jsonb_typeof(p->'entitledTierIds') IS DISTINCT FROM 'array' THEN RETURN NULL; END IF;
 IF jsonb_array_length(p->'entitledTierIds')>1000 OR EXISTS(SELECT 1 FROM jsonb_array_elements(p->'entitledTierIds') v
  WHERE jsonb_typeof(v) IS DISTINCT FROM 'string' OR ((v#>>'{}') COLLATE "C") !~ '^[A-Za-z0-9_-]{1,128}$')
 OR (SELECT count(DISTINCT v) FROM jsonb_array_elements(p->'entitledTierIds') v)<>jsonb_array_length(p->'entitledTierIds') THEN RETURN NULL; END IF;
 SELECT coalesce(jsonb_agg(v ORDER BY (v#>>'{}') COLLATE "C"),'[]'::jsonb) INTO ids FROM jsonb_array_elements(p->'entitledTierIds') v;
 RETURN jsonb_build_object('campaignId',p->>'campaignId','userId',p->>'userId','memberId',p->>'memberId','memberHash',p->>'memberHash','entitledTierIds',ids);
END $$;
CREATE OR REPLACE FUNCTION app.patreon_proof(p jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE fields jsonb;
BEGIN
 fields:=app.patreon_member_fields(p); IF fields IS NULL THEN RETURN NULL; END IF;
 IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p) k) IS DISTINCT FROM ARRAY['campaignId','entitledTierIds','identityHash','memberHash','memberId','source','userId','version']::text[]
 OR p->'version' IS DISTINCT FROM '1'::jsonb OR p->>'source' IS DISTINCT FROM 'patreon-v2-identity-member-v1'
 OR jsonb_typeof(p->'identityHash') IS DISTINCT FROM 'string' OR ((p->>'identityHash') COLLATE "C") !~ '^[0-9a-f]{64}$' THEN RETURN NULL; END IF;
 RETURN fields||jsonb_build_object('version',1,'source','patreon-v2-identity-member-v1','identityHash',p->>'identityHash');
END $$;
CREATE FUNCTION app.patreon_refresh_proof(p jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE fields jsonb;
BEGIN
 fields:=app.patreon_member_fields(p); IF fields IS NULL THEN RETURN NULL; END IF;
 IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p) k) IS DISTINCT FROM ARRAY['campaignId','entitledTierIds','memberHash','memberId','source','userId','version']::text[]
 OR p->'version' IS DISTINCT FROM '1'::jsonb OR p->>'source' IS DISTINCT FROM 'patreon-v2-current-member-v1' THEN RETURN NULL; END IF;
 RETURN fields||jsonb_build_object('version',1,'source','patreon-v2-current-member-v1');
END $$;
CREATE FUNCTION app.patreon_refresh_ticket(p jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE key text;
BEGIN
 IF jsonb_typeof(p) IS DISTINCT FROM 'object' OR octet_length(p::text)>262144 THEN RETURN NULL; END IF;
 IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p) k) IS DISTINCT FROM ARRAY['accountId','anchorEventId','associationId','campaignId','expectedSourceId','generation','memberId','userId','version']::text[]
 OR p->'version' IS DISTINCT FROM '1'::jsonb OR jsonb_typeof(p->'generation') IS DISTINCT FROM 'string'
 OR ((p->>'generation') COLLATE "C" ~ '^[1-9][0-9]{0,18}$') IS DISTINCT FROM true OR (p->>'generation')::bigint<=0 THEN RETURN NULL; END IF;
 FOREACH key IN ARRAY ARRAY['accountId','associationId','expectedSourceId','anchorEventId'] LOOP
  IF jsonb_typeof(p->key) IS DISTINCT FROM 'string' OR (p->>key)::uuid='00000000-0000-0000-0000-000000000000'::uuid
  OR (p->>key)::uuid::text<>p->>key THEN RETURN NULL; END IF;
 END LOOP;
 FOREACH key IN ARRAY ARRAY['campaignId','userId','memberId'] LOOP
  IF jsonb_typeof(p->key) IS DISTINCT FROM 'string' OR ((p->>key) COLLATE "C" ~ '^[A-Za-z0-9_-]{1,128}$') IS DISTINCT FROM true THEN RETURN NULL; END IF;
 END LOOP;
 RETURN p||jsonb_build_object('version',1);
EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN RETURN NULL;
END $$;
CREATE FUNCTION app.patreon_refresh_event_ticket(e app.provider_event) RETURNS jsonb
LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT app.patreon_refresh_ticket(jsonb_build_object('version',1,'accountId',e.account_id,'associationId',e.effect->>'associationId',
  'userId',e.capture->>'userId','campaignId',e.capture->>'campaignId','memberId',e.capture->>'memberId',
  'expectedSourceId',e.effect->>'parentEventId','anchorEventId',e.effect->>'anchorEventId','generation',e.effect->>'generation'))
$$;
CREATE FUNCTION app.patreon_oauth_anchor(m app.patreon_membership,p_anchor uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE e app.provider_event; proof jsonb; root_member app.patreon_membership; generation text;
BEGIN
 SELECT * INTO e FROM app.provider_event WHERE id=p_anchor; proof:=app.patreon_proof(e.capture);
 IF e.id IS NULL OR proof IS NULL OR e.provider<>'patreon' OR e.account_id IS DISTINCT FROM m.account_id
 OR e.event_type<>'oauth.association' OR e.processing_state<>'processed' OR e.raw_evidence_state<>'none'
 OR e.capture IS DISTINCT FROM proof OR e.payload_ref IS DISTINCT FROM 'sha256:'||app.auth_json_hash(proof)
 OR proof->>'userId' IS DISTINCT FROM m.patreon_user_ref OR proof->>'campaignId' IS DISTINCT FROM m.patreon_campaign_ref THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='refresh requires admitted oauth root'; END IF;
 root_member:=m; root_member.source_event_id:=e.id; root_member.patreon_member_ref:=proof->>'memberId';
 PERFORM app.patreon_oauth_commit_source(root_member);
 generation:=e.effect->>'generation';
 IF e.effect IS DISTINCT FROM jsonb_build_object('version',1,'kind','patreon_link_v1','startId',e.effect->>'startId',
  'associationId',m.id,'generation',generation,'proofHash',app.auth_json_hash(proof)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='refresh requires admitted oauth root'; END IF;
 RETURN jsonb_build_object('anchorEventId',e.id,'generation',generation,'memberId',proof->>'memberId');
END $$;
CREATE FUNCTION app.patreon_refresh_source(m app.patreon_membership,e app.provider_event,p_applying boolean DEFAULT false) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE proof jsonb; ticket jsonb; root jsonb; parent app.provider_event; parent_proof jsonb; request uuid;
BEGIN
 proof:=app.patreon_refresh_proof(e.capture); ticket:=app.patreon_refresh_event_ticket(e);
 IF p_applying IS NULL OR proof IS NULL OR ticket IS NULL OR e.id IS NULL OR e.id=(ticket->>'expectedSourceId')::uuid
 OR e.provider<>'patreon' OR e.account_id IS DISTINCT FROM m.account_id OR e.event_type<>'api.membership_refresh'
 OR e.processing_state NOT IN ('verified','processed') OR (NOT p_applying AND e.processing_state<>'processed')
 OR e.raw_evidence_state<>'none' OR e.first_delivery_at IS NOT NULL OR e.raw_deleted_at IS NOT NULL
 OR e.capture IS DISTINCT FROM proof OR e.payload_ref IS DISTINCT FROM 'sha256:'||app.auth_json_hash(proof)
 OR ticket->>'associationId' IS DISTINCT FROM m.id::text OR ticket->>'accountId' IS DISTINCT FROM m.account_id::text
 OR proof->>'userId' IS DISTINCT FROM m.patreon_user_ref OR proof->>'campaignId' IS DISTINCT FROM m.patreon_campaign_ref
 OR proof->>'memberId' IS DISTINCT FROM m.patreon_member_ref THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid current membership source'; END IF;
 request:=(e.effect->>'requestId')::uuid;
 IF request IS NULL OR request='00000000-0000-0000-0000-000000000000'::uuid OR e.effect IS DISTINCT FROM
 jsonb_build_object('version',1,'kind','patreon_refresh_v1','requestId',request,'associationId',m.id,
 'parentEventId',ticket->>'expectedSourceId','anchorEventId',ticket->>'anchorEventId','generation',ticket->>'generation',
 'ticketHash',app.auth_json_hash(ticket),'proofHash',app.auth_json_hash(proof))
 OR e.external_event_ref IS DISTINCT FROM 'refresh:'||m.id::text||':'||request::text
 OR e.effect_key IS DISTINCT FROM 'patreon-refresh:'||m.id::text||':'||request::text THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid current membership source'; END IF;
 root:=app.patreon_oauth_anchor(m,(ticket->>'anchorEventId')::uuid);
 SELECT * INTO parent FROM app.provider_event WHERE id=(ticket->>'expectedSourceId')::uuid;
 parent_proof:=CASE WHEN parent.event_type='oauth.association' THEN app.patreon_proof(parent.capture) ELSE app.patreon_refresh_proof(parent.capture) END;
 IF parent.id IS NULL OR parent.id=e.id OR parent.provider<>'patreon' OR parent.account_id IS DISTINCT FROM m.account_id
 OR parent.processing_state<>'processed' OR parent_proof IS NULL OR parent.capture IS DISTINCT FROM parent_proof
 OR parent.payload_ref IS DISTINCT FROM 'sha256:'||app.auth_json_hash(parent_proof)
 OR parent.effect->>'associationId' IS DISTINCT FROM m.id::text
 OR parent_proof->>'userId' IS DISTINCT FROM proof->>'userId' OR parent_proof->>'campaignId' IS DISTINCT FROM proof->>'campaignId'
 OR parent_proof->>'memberId' IS DISTINCT FROM proof->>'memberId' OR root->>'memberId' IS DISTINCT FROM proof->>'memberId'
 OR (ticket->>'generation')::bigint<(root->>'generation')::bigint
 OR (parent.event_type='oauth.association' AND parent.id IS DISTINCT FROM (ticket->>'anchorEventId')::uuid)
 OR (parent.event_type='api.membership_refresh' AND (parent.effect->>'kind' IS DISTINCT FROM 'patreon_refresh_v1'
  OR parent.effect->>'anchorEventId' IS DISTINCT FROM ticket->>'anchorEventId'))
 OR parent.event_type NOT IN ('oauth.association','api.membership_refresh') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid current membership parent'; END IF;
 RETURN ticket;
END $$;
CREATE FUNCTION app.patreon_refresh_context(m app.patreon_membership) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE e app.provider_event; root jsonb; ticket jsonb; latest app.command_receipt; metadata jsonb; completed boolean;
BEGIN
 IF m.id IS NULL OR m.patreon_user_ref IS NULL OR m.patreon_campaign_ref IS NULL THEN RETURN NULL; END IF;
 SELECT * INTO e FROM app.provider_event WHERE id=m.source_event_id;
 IF e.event_type='oauth.association' THEN
  root:=app.patreon_oauth_anchor(m,e.id);
  IF root->>'memberId' IS DISTINCT FROM m.patreon_member_ref THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='current membership differs from root'; END IF;
 ELSIF e.event_type='api.membership_refresh' THEN
  ticket:=app.patreon_refresh_source(m,e); root:=app.patreon_oauth_anchor(m,(ticket->>'anchorEventId')::uuid);
 ELSE RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified current membership source required'; END IF;
 SELECT * INTO latest FROM app.command_receipt WHERE principal_key='human:'||m.account_id::text AND scope='patreon_start' AND state='accepted'
 ORDER BY (result_ref::jsonb->>'generation')::bigint DESC LIMIT 1;
 IF latest.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='admitted latest start required'; END IF;
 metadata:=latest.result_ref::jsonb;
 completed:=EXISTS(SELECT 1 FROM app.command_receipt WHERE principal_key=latest.principal_key AND scope='patreon_complete' AND request_ref=latest.request_ref AND state='accepted' AND account_id=m.account_id);
 IF NOT completed AND (metadata->>'expiresAt')::timestamptz>clock_timestamp() THEN RETURN NULL; END IF;
 IF completed AND metadata->>'generation' IS DISTINCT FROM root->>'generation' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='current oauth root differs from latest completion'; END IF;
 ticket:=app.patreon_refresh_ticket(jsonb_build_object('version',1,'accountId',m.account_id,'associationId',m.id,
  'userId',m.patreon_user_ref,'campaignId',m.patreon_campaign_ref,'memberId',m.patreon_member_ref,
  'expectedSourceId',m.source_event_id,'anchorEventId',root->>'anchorEventId','generation',metadata->>'generation'));
 IF ticket IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid refresh source context'; END IF;
 RETURN ticket;
END $$;
CREATE FUNCTION app.read_patreon_refresh(p_association uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE m app.patreon_membership; lifecycle text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='patreon requires read committed'; END IF;
 PERFORM app.patreon_notification_id(p_association);
 SELECT * INTO m FROM app.patreon_membership WHERE id=p_association; IF m.id IS NULL THEN RETURN NULL; END IF;
 SELECT state INTO lifecycle FROM app.account WHERE id=m.account_id FOR NO KEY UPDATE;
 IF lifecycle IS DISTINCT FROM 'active' THEN RETURN NULL; END IF;
 SELECT * INTO m FROM app.patreon_membership WHERE id=p_association;
 RETURN app.patreon_refresh_context(m);
END $$;
CREATE FUNCTION app.commit_patreon_refresh(p_request uuid,p_ticket jsonb,p_proof jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE m app.patreon_membership; historical app.patreon_membership; lifecycle text; existing app.provider_event;
 source_id uuid; current_ticket jsonb; tier uuid; result jsonb; external text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='patreon requires read committed'; END IF;
 PERFORM app.patreon_notification_id(p_request);
 p_ticket:=app.patreon_refresh_ticket(p_ticket); p_proof:=app.patreon_refresh_proof(p_proof);
 IF p_ticket IS NULL OR p_proof IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid refresh observation'; END IF;
 IF (p_ticket->>'userId',p_ticket->>'campaignId',p_ticket->>'memberId') IS DISTINCT FROM (p_proof->>'userId',p_proof->>'campaignId',p_proof->>'memberId') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='refresh observation identity differs'; END IF;
 result:=jsonb_build_object('associationId',p_ticket->>'associationId','sourceEventId',NULL,'outcome','unavailable');
 SELECT * INTO m FROM app.patreon_membership WHERE id=(p_ticket->>'associationId')::uuid;
 IF m.id IS NULL THEN RETURN result; END IF;
 SELECT state INTO lifecycle FROM app.account WHERE id=m.account_id FOR NO KEY UPDATE;
 SELECT * INTO m FROM app.patreon_membership WHERE id=m.id FOR UPDATE;
 IF m.account_id::text IS DISTINCT FROM p_ticket->>'accountId' OR m.patreon_user_ref IS DISTINCT FROM p_ticket->>'userId'
 OR m.patreon_campaign_ref IS DISTINCT FROM p_ticket->>'campaignId' THEN RETURN result; END IF;
 external:='refresh:'||m.id::text||':'||p_request::text;
 SELECT * INTO existing FROM app.provider_event WHERE provider='patreon' AND external_event_ref=external;
 IF existing.id IS NOT NULL THEN
  historical:=m; historical.patreon_member_ref:=p_ticket->>'memberId'; historical.source_event_id:=existing.id;
  current_ticket:=app.patreon_refresh_source(historical,existing);
  IF current_ticket IS DISTINCT FROM p_ticket OR existing.capture IS DISTINCT FROM p_proof THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='refresh occurrence terms differ'; END IF;
  RETURN result||jsonb_build_object('sourceEventId',existing.id,'outcome','applied');
 END IF;
 IF lifecycle IS DISTINCT FROM 'active' OR m.patreon_user_ref IS NULL THEN RETURN result; END IF;
 current_ticket:=app.patreon_refresh_context(m);
 IF current_ticket IS DISTINCT FROM p_ticket THEN RETURN result||jsonb_build_object('outcome','superseded'); END IF;
 tier:=app.patreon_entitled_tier(p_proof);
 INSERT INTO app.provider_event(provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state,effect,effect_key)
 VALUES('patreon',external,m.account_id,clock_timestamp(),'api.membership_refresh','sha256:'||app.auth_json_hash(p_proof),'verified',p_proof,'none',
  jsonb_build_object('version',1,'kind','patreon_refresh_v1','requestId',p_request,'associationId',m.id,'parentEventId',p_ticket->>'expectedSourceId',
   'anchorEventId',p_ticket->>'anchorEventId','generation',p_ticket->>'generation','ticketHash',app.auth_json_hash(p_ticket),'proofHash',app.auth_json_hash(p_proof)),
  'patreon-refresh:'||m.id::text||':'||p_request::text) RETURNING id INTO source_id;
 UPDATE app.patreon_membership SET active_tier_id=tier,active=(tier IS NOT NULL),source_event_id=source_id WHERE id=m.id;
 UPDATE app.provider_event SET processing_state='processed' WHERE id=source_id;
 SELECT * INTO m FROM app.patreon_membership WHERE id=m.id;
 current_ticket:=app.patreon_refresh_context(m);
 IF m.source_event_id IS DISTINCT FROM source_id OR current_ticket IS NULL OR current_ticket->>'generation' IS DISTINCT FROM p_ticket->>'generation'
 OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=m.account_id AND state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='refresh context changed before acceptance'; END IF;
 RETURN result||jsonb_build_object('sourceEventId',source_id,'outcome','applied');
END $$;
CREATE OR REPLACE FUNCTION app.guard_membership_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE e app.provider_event; ticket jsonb; tier uuid;
BEGIN
 IF TG_OP='DELETE' THEN RETURN app.patreon_oauth_membership_source(NEW,OLD,TG_OP); END IF;
 SELECT * INTO e FROM app.provider_event WHERE id=NEW.source_event_id;
 IF e.event_type IS DISTINCT FROM 'api.membership_refresh' THEN RETURN app.patreon_oauth_membership_source(NEW,OLD,TG_OP); END IF;
 IF TG_OP<>'UPDATE' OR (NEW.id,NEW.account_id,NEW.patreon_member_ref,NEW.patreon_user_ref,NEW.patreon_campaign_ref)
 IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.patreon_member_ref,OLD.patreon_user_ref,OLD.patreon_campaign_ref) OR OLD.patreon_user_ref IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='refresh cannot change membership identity'; END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 PERFORM app.check_provider_source(e.id,NEW.account_id,'patreon');
 ticket:=app.patreon_refresh_source(NEW,e,true); tier:=app.patreon_entitled_tier(e.capture);
 IF ticket->>'expectedSourceId' IS DISTINCT FROM OLD.source_event_id::text OR NEW.active_tier_id IS DISTINCT FROM tier
 OR NEW.active IS DISTINCT FROM (tier IS NOT NULL) OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='refresh requires exact current parent and tier'; END IF;
 RETURN NEW;
END $$;
CREATE OR REPLACE FUNCTION app.check_patreon_association_commit() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE e app.provider_event;
BEGIN
 SELECT * INTO e FROM app.provider_event WHERE id=NEW.source_event_id;
 IF e.event_type='api.membership_refresh' THEN
  PERFORM app.patreon_refresh_source(NEW,e);
  IF NEW.active_tier_id IS DISTINCT FROM app.patreon_entitled_tier(e.capture)
  OR NEW.active IS DISTINCT FROM (NEW.active_tier_id IS NOT NULL) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='refresh mapped tier changed before commit'; END IF;
 ELSE PERFORM app.patreon_oauth_commit_source(NEW); END IF;
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION app.patreon_oauth_membership_source(app.patreon_membership,app.patreon_membership,text),
 app.patreon_oauth_commit_source(app.patreon_membership),app.patreon_member_fields(jsonb),app.patreon_proof(jsonb),app.patreon_refresh_proof(jsonb),
 app.patreon_refresh_ticket(jsonb),app.patreon_refresh_event_ticket(app.provider_event),app.patreon_oauth_anchor(app.patreon_membership,uuid),
 app.patreon_refresh_source(app.patreon_membership,app.provider_event,boolean),app.patreon_refresh_context(app.patreon_membership),
 app.read_patreon_refresh(uuid),app.commit_patreon_refresh(uuid,jsonb,jsonb),app.guard_membership_source(),app.check_patreon_association_commit()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_patreon_refresh(uuid),app.commit_patreon_refresh(uuid,jsonb,jsonb) TO service_role;
COMMIT;
