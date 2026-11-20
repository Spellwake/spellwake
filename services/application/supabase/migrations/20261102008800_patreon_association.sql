-- Original-session OAuth association. No payment/grant writer or provider activation.
BEGIN;
ALTER TABLE app.patreon_membership ADD COLUMN patreon_user_ref text;
ALTER TABLE app.patreon_membership ADD COLUMN patreon_campaign_ref text;
ALTER TABLE app.patreon_membership ADD CHECK ((patreon_user_ref IS NULL AND patreon_campaign_ref IS NULL)
 OR ((patreon_user_ref COLLATE "C") ~ '^[A-Za-z0-9_-]{1,128}$' AND (patreon_campaign_ref COLLATE "C") ~ '^[A-Za-z0-9_-]{1,128}$'
 AND patreon_user_ref IS NOT NULL AND patreon_campaign_ref IS NOT NULL));
CREATE UNIQUE INDEX patreon_stable_user ON app.patreon_membership(patreon_user_ref) WHERE patreon_user_ref IS NOT NULL;
CREATE SEQUENCE app.patreon_start_generation;
REVOKE ALL ON SEQUENCE app.patreon_start_generation FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE INDEX patreon_latest_start ON app.command_receipt(principal_key,((result_ref::jsonb->>'generation')::bigint) DESC)
 WHERE scope='patreon_start' AND state='accepted';
CREATE UNIQUE INDEX patreon_spent_code ON app.command_receipt((result_ref::jsonb->>'clientId'),(result_ref::jsonb->>'codeHash'))
 WHERE scope='patreon_exchange_claim' AND state='accepted';
REVOKE SELECT ON app.patreon_membership FROM authenticated;
GRANT SELECT(id,account_id,active_tier_id,active) ON app.patreon_membership TO authenticated;

CREATE FUNCTION app.patreon_terms(p jsonb) RETURNS jsonb LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF jsonb_typeof(p) IS DISTINCT FROM 'object' OR octet_length(p::text)>4096 THEN RETURN NULL; END IF;
 IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p) k) IS DISTINCT FROM ARRAY['campaignId','clientId','redirectUri','version']::text[]
 OR p->'version' IS DISTINCT FROM '1'::jsonb OR jsonb_typeof(p->'version') IS DISTINCT FROM 'number'
 OR jsonb_typeof(p->'campaignId') IS DISTINCT FROM 'string' OR (p->>'campaignId' COLLATE "C") !~ '^[A-Za-z0-9_-]{1,128}$'
 OR jsonb_typeof(p->'clientId') IS DISTINCT FROM 'string' OR (p->>'clientId' COLLATE "C") !~ '^[A-Za-z0-9_-]{1,128}$'
 OR jsonb_typeof(p->'redirectUri') IS DISTINCT FROM 'string' OR length(p->>'redirectUri') NOT BETWEEN 9 AND 2048
 OR p->>'redirectUri' NOT LIKE 'https://%' OR p->>'redirectUri' ~ '[[:space:]]' THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('version',1,'campaignId',p->>'campaignId','clientId',p->>'clientId','redirectUri',p->>'redirectUri');
END $$;
CREATE FUNCTION app.patreon_proof(p jsonb) RETURNS jsonb LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE key text; ids jsonb;
BEGIN
 IF jsonb_typeof(p) IS DISTINCT FROM 'object' OR octet_length(p::text)>262144 THEN RETURN NULL; END IF;
 IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p) k) IS DISTINCT FROM ARRAY['campaignId','entitledTierIds','identityHash','memberHash','memberId','source','userId','version']::text[]
 OR p->'version' IS DISTINCT FROM '1'::jsonb OR jsonb_typeof(p->'version') IS DISTINCT FROM 'number'
 OR p->>'source' IS DISTINCT FROM 'patreon-v2-identity-member-v1' OR jsonb_typeof(p->'source') IS DISTINCT FROM 'string'
 OR jsonb_typeof(p->'entitledTierIds') IS DISTINCT FROM 'array' THEN RETURN NULL; END IF;
 FOREACH key IN ARRAY ARRAY['campaignId','userId','memberId'] LOOP
  IF jsonb_typeof(p->key) IS DISTINCT FROM 'string' OR ((p->>key) COLLATE "C") !~ '^[A-Za-z0-9_-]{1,128}$' THEN RETURN NULL; END IF;
 END LOOP;
 FOREACH key IN ARRAY ARRAY['identityHash','memberHash'] LOOP
  IF jsonb_typeof(p->key) IS DISTINCT FROM 'string' OR ((p->>key) COLLATE "C") !~ '^[0-9a-f]{64}$' THEN RETURN NULL; END IF;
 END LOOP;
 IF jsonb_array_length(p->'entitledTierIds')>1000 OR EXISTS(SELECT 1 FROM jsonb_array_elements(p->'entitledTierIds') v
  WHERE jsonb_typeof(v) IS DISTINCT FROM 'string' OR ((v#>>'{}') COLLATE "C") !~ '^[A-Za-z0-9_-]{1,128}$')
 OR (SELECT count(DISTINCT v) FROM jsonb_array_elements(p->'entitledTierIds') v)<>jsonb_array_length(p->'entitledTierIds') THEN RETURN NULL; END IF;
 SELECT coalesce(jsonb_agg(v ORDER BY (v#>>'{}') COLLATE "C"),'[]'::jsonb) INTO ids FROM jsonb_array_elements(p->'entitledTierIds') v;
 RETURN jsonb_build_object('version',1,'source','patreon-v2-identity-member-v1','campaignId',p->>'campaignId','userId',p->>'userId',
  'memberId',p->>'memberId','identityHash',p->>'identityHash','memberHash',p->>'memberHash','entitledTierIds',ids);
END $$;
CREATE FUNCTION app.patreon_start_record(p_actor uuid,p_session uuid,p_request uuid,p_state_hash text) RETURNS app.command_receipt
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE r app.command_receipt; metadata jsonb; provider jsonb;
BEGIN
 SELECT * INTO r FROM app.command_receipt WHERE principal_key='human:'||p_actor::text AND scope='patreon_start' AND request_ref=p_request::text;
 IF NOT FOUND THEN RETURN NULL; END IF;
 metadata:=r.result_ref::jsonb; provider:=app.patreon_terms(metadata->'provider');
 IF r.account_id IS DISTINCT FROM p_actor OR r.target_ref IS DISTINCT FROM p_actor::text OR r.state<>'accepted'
 OR r.controller_grant_id IS NOT NULL OR r.operation_id IS NOT NULL OR provider IS NULL
 OR r.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('sessionId',p_session,'stateHash',p_state_hash,'provider',provider)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='patreon context conflict'; END IF;
 IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(metadata) k) IS DISTINCT FROM ARRAY['createdAt','expiresAt','generation','provider','version']::text[]
 OR metadata->'version' IS DISTINCT FROM '1'::jsonb OR jsonb_typeof(metadata->'generation') IS DISTINCT FROM 'string'
 OR (metadata->>'generation' ~ '^[1-9][0-9]{0,18}$') IS DISTINCT FROM true
 OR jsonb_typeof(metadata->'createdAt') IS DISTINCT FROM 'string' OR jsonb_typeof(metadata->'expiresAt') IS DISTINCT FROM 'string'
 OR (metadata->>'generation')::bigint<=0 OR (metadata->>'expiresAt')::timestamptz IS DISTINCT FROM (metadata->>'createdAt')::timestamptz+interval '10 minutes'
 OR r.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_request,'expiresAt',metadata->>'expiresAt')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid patreon start source'; END IF;
 RETURN r;
END $$;
CREATE FUNCTION app.patreon_attempt_terms(p_session uuid,p_request uuid,p_state_hash text,p_code_hash text) RETURNS jsonb
LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT jsonb_build_object('startId',p_request,'stateHash',p_state_hash,'codeHash',p_code_hash,'sessionId',p_session)
$$;
CREATE FUNCTION app.patreon_link_view(p_player app.account,p_session uuid,p_request uuid,p_state_hash text,p_code_hash text) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE start app.command_receipt; claim app.command_receipt; done app.command_receipt; source app.provider_event; member app.patreon_membership;
 metadata jsonb; claim_data jsonb; final_data jsonb; proof jsonb; intent jsonb; phase text; association uuid; result jsonb;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR (p_state_hash ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true
 OR (p_code_hash IS NOT NULL AND (p_code_hash ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 result:=jsonb_build_object('accountId',p_player.id,'requestId',p_request,'phase','unknown','expiresAt',NULL,'generation',NULL,'serverTerms',NULL,'associationId',NULL);
 start:=app.patreon_start_record(p_player.id,p_session,p_request,p_state_hash);
 IF start.id IS NULL THEN
  IF p_player.state<>'active' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
  RETURN jsonb_build_object('status','success','data',result); END IF;
 metadata:=start.result_ref::jsonb;
 SELECT * INTO claim FROM app.command_receipt WHERE principal_key=start.principal_key AND scope='patreon_exchange_claim' AND request_ref=start.request_ref;
 IF FOUND THEN
  claim_data:=claim.result_ref::jsonb;
  intent:=app.patreon_attempt_terms(p_session,p_request,p_state_hash,claim_data->>'codeHash');
  IF claim.state<>'accepted' OR claim.account_id IS DISTINCT FROM p_player.id OR claim.target_ref IS DISTINCT FROM p_player.id::text
  OR claim.controller_grant_id IS NOT NULL OR claim.operation_id IS NOT NULL OR claim.payload_hash IS DISTINCT FROM app.auth_json_hash(intent)
  OR (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(claim_data) k) IS DISTINCT FROM ARRAY['claimHash','clientId','codeHash','generation','version']::text[]
  OR claim_data->'version' IS DISTINCT FROM '1'::jsonb OR (claim_data->>'claimHash' ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true
  OR (claim_data->>'codeHash' ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true OR claim_data->>'clientId' IS DISTINCT FROM metadata->'provider'->>'clientId'
  OR claim_data->>'generation' IS DISTINCT FROM metadata->>'generation'
  OR claim.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_request)) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid patreon exchange source'; END IF;
  IF p_code_hash IS NOT NULL AND p_code_hash IS DISTINCT FROM claim_data->>'codeHash' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='patreon context conflict'; END IF;
 END IF;
 SELECT * INTO done FROM app.command_receipt WHERE principal_key=start.principal_key AND scope='patreon_complete' AND request_ref=start.request_ref;
 IF FOUND THEN
  final_data:=done.result_ref::jsonb;
  SELECT * INTO source FROM app.provider_event WHERE id=(final_data->>'providerEventId')::uuid;
  SELECT * INTO member FROM app.patreon_membership WHERE id=(final_data->>'associationId')::uuid;
  proof:=app.patreon_proof(source.capture);
  IF claim.id IS NULL OR done.state<>'accepted' OR done.account_id IS DISTINCT FROM p_player.id OR done.target_ref IS DISTINCT FROM p_player.id::text
  OR done.controller_grant_id IS NOT NULL OR done.operation_id IS NOT NULL OR done.payload_hash IS DISTINCT FROM claim.payload_hash
  OR (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(final_data) k) IS DISTINCT FROM ARRAY['associationId','proofHash','providerEventId','version']::text[]
  OR final_data->'version' IS DISTINCT FROM '1'::jsonb OR source.id IS NULL OR member.id IS NULL OR proof IS NULL
  OR source.provider<>'patreon' OR source.account_id IS DISTINCT FROM p_player.id OR source.external_event_ref IS DISTINCT FROM 'oauth:'||p_player.id::text||':'||p_request::text
  OR source.event_type<>'oauth.association' OR source.processing_state<>'processed' OR source.raw_evidence_state<>'none'
  OR source.payload_ref IS DISTINCT FROM 'sha256:'||app.auth_json_hash(proof) OR final_data->>'proofHash' IS DISTINCT FROM app.auth_json_hash(proof)
  OR member.account_id IS DISTINCT FROM p_player.id OR member.patreon_user_ref IS DISTINCT FROM proof->>'userId'
  OR member.patreon_campaign_ref IS DISTINCT FROM proof->>'campaignId' OR proof->>'campaignId' IS DISTINCT FROM metadata->'provider'->>'campaignId'
  OR source.effect IS DISTINCT FROM jsonb_build_object('version',1,'kind','patreon_link_v1','startId',p_request,'associationId',member.id,'generation',metadata->>'generation','proofHash',app.auth_json_hash(proof))
  OR source.effect_key IS DISTINCT FROM 'patreon-link:'||p_player.id::text||':'||p_request::text
  OR done.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',member.id)) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid patreon completion source'; END IF;
  association:=member.id; phase:='linked';
 ELSE
  IF p_player.state<>'active' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
  IF EXISTS(SELECT 1 FROM app.command_receipt r WHERE r.principal_key=start.principal_key AND r.scope='patreon_start' AND r.state='accepted'
   AND (r.result_ref::jsonb->>'generation')::bigint>(metadata->>'generation')::bigint) THEN phase:='superseded';
  ELSIF (metadata->>'expiresAt')::timestamptz<=clock_timestamp() THEN phase:='expired';
  ELSIF claim.id IS NOT NULL THEN phase:='claimed'; ELSE phase:='issued'; END IF;
 END IF;
 RETURN jsonb_build_object('status','success','data',result||jsonb_build_object('phase',phase,'expiresAt',metadata->>'expiresAt',
  'generation',metadata->>'generation','serverTerms',metadata->'provider','associationId',association));
END $$;
CREATE FUNCTION app.start_patreon_link(p_subject uuid,p_session uuid,p_request uuid,p_state_hash text,p_terms jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE player app.account; receipt app.command_receipt; terms jsonb; created timestamptz; metadata jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='patreon requires read committed'; END IF;
 player:=app.lock_session_account(p_subject,p_session); terms:=app.patreon_terms(p_terms);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR (p_state_hash ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true OR terms IS NULL THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 IF player.state<>'active' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 receipt:=app.begin_account_command(player.id,p_request,'patreon_start',player.id::text,jsonb_build_object('sessionId',p_session,'stateHash',p_state_hash,'provider',terms));
 IF receipt.state<>'pending' THEN PERFORM app.lock_session_account(p_subject,p_session); RETURN receipt.response_payload; END IF;
 created:=clock_timestamp(); metadata:=jsonb_build_object('version',1,'provider',terms,'createdAt',created,'expiresAt',created+interval '10 minutes',
  'generation',nextval('app.patreon_start_generation')::text);
 UPDATE app.command_receipt SET result_ref=metadata::text WHERE id=receipt.id;
 PERFORM app.lock_session_account(p_subject,p_session);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_request,'expiresAt',metadata->>'expiresAt'));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;
CREATE FUNCTION app.read_patreon_link(p_subject uuid,p_session uuid,p_request uuid,p_state_hash text,p_code_hash text DEFAULT NULL) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE player app.account;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='patreon requires read committed'; END IF;
 player:=app.lock_session_account(p_subject,p_session);
 RETURN app.patreon_link_view(player,p_session,p_request,p_state_hash,p_code_hash);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='patreon context conflict' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;
CREATE FUNCTION app.claim_patreon_link(p_subject uuid,p_session uuid,p_request uuid,p_state_hash text,p_code_hash text,p_claim_hash text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE player app.account; view jsonb; receipt app.command_receipt; metadata jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='patreon requires read committed'; END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF (p_code_hash ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true OR (p_claim_hash ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 view:=app.patreon_link_view(player,p_session,p_request,p_state_hash,p_code_hash);
 IF view->>'status'<>'success' THEN RETURN view; END IF;
 IF view->'data'->>'phase' IN ('claimed','linked') THEN RETURN jsonb_build_object('status','success','data',jsonb_build_object('mayExchange',false)); END IF;
 IF view->'data'->>'phase'<>'issued' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request',view->'data'->>'phase')); END IF;
 metadata:=view->'data';
 receipt:=app.begin_account_command(player.id,p_request,'patreon_exchange_claim',player.id::text,app.patreon_attempt_terms(p_session,p_request,p_state_hash,p_code_hash));
 UPDATE app.command_receipt SET result_ref=jsonb_build_object('version',1,'claimHash',p_claim_hash,'codeHash',p_code_hash,
  'clientId',metadata->'serverTerms'->>'clientId','generation',metadata->>'generation')::text WHERE id=receipt.id;
 PERFORM app.lock_session_account(p_subject,p_session);
 IF (metadata->>'expiresAt')::timestamptz<=clock_timestamp() THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='patreon flow expired'; END IF;
 PERFORM app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_request));
 -- The spent-code unique index can wait behind another account after the first authority check.
 PERFORM app.lock_session_account(p_subject,p_session);
 IF (metadata->>'expiresAt')::timestamptz<=clock_timestamp() THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='patreon flow expired'; END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('mayExchange',true));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN unique_violation THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','spent'));
WHEN check_violation THEN
 IF SQLERRM IN ('patreon context conflict','request identity was used for different command terms') THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));
 ELSIF SQLERRM='patreon flow expired' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','expired')); END IF;
 RAISE;
END $$;
CREATE FUNCTION app.patreon_entitled_tier(p_proof jsonb) RETURNS uuid LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT t.id FROM app.supporter_tier t WHERE t.patreon_tier_ref IN (SELECT jsonb_array_elements_text(p_proof->'entitledTierIds'))
 ORDER BY t.rank DESC,t.id ASC LIMIT 1
$$;
CREATE OR REPLACE FUNCTION app.guard_membership_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source app.provider_event; proof jsonb; expected_tier uuid;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership proof cannot be deleted'; END IF;
 IF TG_OP='UPDATE' AND (NEW.id,NEW.account_id) IS DISTINCT FROM (OLD.id,OLD.account_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership association is immutable'; END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 PERFORM app.check_provider_source(NEW.source_event_id,NEW.account_id,'patreon');
 IF (TG_OP='INSERT' OR NEW.active) AND NOT EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account cannot activate a membership'; END IF;
 IF NEW.patreon_user_ref IS NULL THEN
  IF TG_OP='UPDATE' AND (OLD.patreon_user_ref IS NOT NULL OR NEW.patreon_member_ref IS DISTINCT FROM OLD.patreon_member_ref) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership association is immutable'; END IF;
  RETURN NEW;
 END IF;
 IF TG_OP='UPDATE' AND (OLD.patreon_user_ref IS NULL OR (NEW.patreon_user_ref,NEW.patreon_campaign_ref)
  IS DISTINCT FROM (OLD.patreon_user_ref,OLD.patreon_campaign_ref)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership association is immutable'; END IF;
 SELECT * INTO source FROM app.provider_event WHERE id=NEW.source_event_id; proof:=app.patreon_proof(source.capture);
 expected_tier:=app.patreon_entitled_tier(proof);
 IF proof IS NULL OR source.event_type IS DISTINCT FROM 'oauth.association' OR source.effect->>'kind' IS DISTINCT FROM 'patreon_link_v1'
 OR source.effect->>'associationId' IS DISTINCT FROM NEW.id::text OR proof->>'userId' IS DISTINCT FROM NEW.patreon_user_ref
 OR proof->>'campaignId' IS DISTINCT FROM NEW.patreon_campaign_ref OR proof->>'memberId' IS DISTINCT FROM NEW.patreon_member_ref
 OR expected_tier IS DISTINCT FROM NEW.active_tier_id OR NEW.active IS DISTINCT FROM (expected_tier IS NOT NULL) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership requires qualified association source'; END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION app.complete_patreon_link(p_subject uuid,p_session uuid,p_request uuid,p_state_hash text,p_code_hash text,p_claim_hash text,p_proof jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE player app.account; view jsonb; metadata jsonb; proof jsonb; claim app.command_receipt; receipt app.command_receipt;
 member app.patreon_membership; association uuid; source_id uuid; source_hash text; tier uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='patreon requires read committed'; END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF (p_code_hash ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true OR (p_claim_hash ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 view:=app.patreon_link_view(player,p_session,p_request,p_state_hash,p_code_hash);
 IF view->>'status'<>'success' THEN RETURN view; END IF;
 IF view->'data'->>'phase'='linked' THEN
  SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='patreon_complete' AND request_ref=p_request::text;
  RETURN receipt.response_payload; END IF;
 IF view->'data'->>'phase'<>'claimed' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request',view->'data'->>'phase')); END IF;
 metadata:=view->'data';
 SELECT * INTO STRICT claim FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='patreon_exchange_claim' AND request_ref=p_request::text;
 IF claim.result_ref::jsonb->>'claimHash' IS DISTINCT FROM p_claim_hash THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 proof:=app.patreon_proof(p_proof);
 IF proof IS NULL OR proof->>'campaignId' IS DISTINCT FROM metadata->'serverTerms'->>'campaignId' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('proof','invalid')); END IF;
 -- No network request occurs while these account/external-identity locks are held.
 PERFORM pg_advisory_xact_lock(hashtextextended('patreon-user:'||(proof->>'userId'),0));
 PERFORM app.lock_session_account(p_subject,p_session);
 IF EXISTS(SELECT 1 FROM app.patreon_membership WHERE account_id<>player.id
  AND (patreon_user_ref=proof->>'userId' OR patreon_member_ref=proof->>'memberId')) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('membership','unavailable')); END IF;
 SELECT * INTO member FROM app.patreon_membership WHERE account_id=player.id FOR UPDATE;
 IF FOUND AND (member.patreon_user_ref IS DISTINCT FROM proof->>'userId' OR member.patreon_campaign_ref IS DISTINCT FROM proof->>'campaignId') THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('membership','unavailable')); END IF;
 association:=coalesce(member.id,gen_random_uuid()); tier:=app.patreon_entitled_tier(proof); source_hash:=app.auth_json_hash(proof);
 receipt:=app.begin_account_command(player.id,p_request,'patreon_complete',player.id::text,app.patreon_attempt_terms(p_session,p_request,p_state_hash,p_code_hash));
 INSERT INTO app.provider_event(provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state,effect,effect_key)
 VALUES('patreon','oauth:'||player.id::text||':'||p_request::text,player.id,clock_timestamp(),'oauth.association','sha256:'||source_hash,'verified',proof,'none',
  jsonb_build_object('version',1,'kind','patreon_link_v1','startId',p_request,'associationId',association,'generation',metadata->>'generation','proofHash',source_hash),
  'patreon-link:'||player.id::text||':'||p_request::text) RETURNING id INTO source_id;
 IF member.id IS NULL THEN
  INSERT INTO app.patreon_membership(id,account_id,patreon_member_ref,patreon_user_ref,patreon_campaign_ref,active_tier_id,active,source_event_id)
  VALUES(association,player.id,proof->>'memberId',proof->>'userId',proof->>'campaignId',tier,tier IS NOT NULL,source_id);
 ELSE
  UPDATE app.patreon_membership SET patreon_member_ref=proof->>'memberId',active_tier_id=tier,active=(tier IS NOT NULL),source_event_id=source_id WHERE id=association;
 END IF;
 UPDATE app.provider_event SET processing_state='processed' WHERE id=source_id;
 UPDATE app.command_receipt SET result_ref=jsonb_build_object('version',1,'providerEventId',source_id,'associationId',association,'proofHash',source_hash)::text WHERE id=receipt.id;
 PERFORM app.lock_session_account(p_subject,p_session);
 IF (metadata->>'expiresAt')::timestamptz<=clock_timestamp() THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='patreon flow expired'; END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',association));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN unique_violation THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('membership','unavailable'));
WHEN check_violation THEN
 IF SQLERRM IN ('patreon context conflict','request identity was used for different command terms') THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));
 ELSIF SQLERRM='patreon flow expired' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','expired')); END IF;
 RAISE;
END $$;
CREATE FUNCTION app.check_patreon_association_commit() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE e app.provider_event; done app.command_receipt; claim app.command_receipt; start app.command_receipt; data jsonb;
BEGIN
 IF NEW.patreon_user_ref IS NULL THEN RETURN NULL; END IF;
 SELECT * INTO STRICT e FROM app.provider_event WHERE id=NEW.source_event_id;
 SELECT * INTO done FROM app.command_receipt WHERE principal_key='human:'||NEW.account_id::text AND scope='patreon_complete' AND request_ref=e.effect->>'startId';
 SELECT * INTO claim FROM app.command_receipt WHERE principal_key=done.principal_key AND scope='patreon_exchange_claim' AND request_ref=done.request_ref;
 SELECT * INTO start FROM app.command_receipt WHERE principal_key=done.principal_key AND scope='patreon_start' AND request_ref=done.request_ref;
 data:=done.result_ref::jsonb;
 IF done.id IS NULL OR done.state<>'accepted' OR done.account_id IS DISTINCT FROM NEW.account_id OR done.target_ref IS DISTINCT FROM NEW.account_id::text
 OR done.payload_hash IS DISTINCT FROM claim.payload_hash OR done.controller_grant_id IS NOT NULL OR done.operation_id IS NOT NULL
 OR claim.id IS NULL OR claim.state<>'accepted' OR start.id IS NULL OR start.state<>'accepted'
 OR claim.account_id IS DISTINCT FROM NEW.account_id OR start.account_id IS DISTINCT FROM NEW.account_id
 OR e.processing_state<>'processed' OR e.account_id IS DISTINCT FROM NEW.account_id OR e.provider<>'patreon'
 OR e.external_event_ref IS DISTINCT FROM 'oauth:'||NEW.account_id::text||':'||done.request_ref
 OR e.payload_ref IS DISTINCT FROM 'sha256:'||app.auth_json_hash(e.capture) OR data->>'providerEventId' IS DISTINCT FROM e.id::text
 OR data->>'associationId' IS DISTINCT FROM NEW.id::text OR data->>'proofHash' IS DISTINCT FROM app.auth_json_hash(e.capture)
 OR e.effect->>'generation' IS DISTINCT FROM start.result_ref::jsonb->>'generation'
 OR done.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',NEW.id)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='association requires completed source transaction'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER patreon_association_commit AFTER INSERT OR UPDATE ON app.patreon_membership
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_patreon_association_commit();
REVOKE ALL ON FUNCTION app.patreon_terms(jsonb),app.patreon_proof(jsonb),app.patreon_start_record(uuid,uuid,uuid,text),
 app.patreon_attempt_terms(uuid,uuid,text,text),app.patreon_link_view(app.account,uuid,uuid,text,text),app.patreon_entitled_tier(jsonb),
 app.guard_membership_source(),app.check_patreon_association_commit(),app.start_patreon_link(uuid,uuid,uuid,text,jsonb),
 app.read_patreon_link(uuid,uuid,uuid,text,text),app.claim_patreon_link(uuid,uuid,uuid,text,text,text),
 app.complete_patreon_link(uuid,uuid,uuid,text,text,text,jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.start_patreon_link(uuid,uuid,uuid,text,jsonb),app.read_patreon_link(uuid,uuid,uuid,text,text),
 app.claim_patreon_link(uuid,uuid,uuid,text,text,text),app.complete_patreon_link(uuid,uuid,uuid,text,text,text,jsonb) TO service_role;
COMMIT;
