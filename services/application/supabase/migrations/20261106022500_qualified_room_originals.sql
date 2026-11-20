BEGIN;

CREATE FUNCTION app.native_room_intent_is_valid(p_room uuid,p_original jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF p_room IS NULL OR p_room='00000000-0000-0000-0000-000000000000'::uuid
  OR jsonb_typeof(p_original) IS DISTINCT FROM 'object' OR octet_length(p_original::text)>32768 THEN RETURN false;END IF;
 RETURN coalesce(jsonb_typeof(p_original->'requestId')='string'
  AND p_original->>'requestId' ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
  AND p_original->>'requestId'<>'00000000-0000-0000-0000-000000000000'
  AND jsonb_typeof(p_original->'expectedRevision')='string'
  AND p_original->>'expectedRevision' ~ '^(0|[1-9][0-9]{0,77})$'
  AND NOT EXISTS(SELECT 1 FROM jsonb_object_keys(p_original) k WHERE k NOT IN('requestId','expectedRevision')),false);
END;
$$;

CREATE FUNCTION app.qualify_native_room_command(p_subject uuid,p_session uuid,p_room uuid,p_original jsonb,p_prepared jsonb,p_kind text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;parent app.command_receipt;child app.command_receipt;lobby app.room;
 request_id uuid;basis numeric;release_id uuid;legality_id uuid;revision_id uuid;scope_name text;
 source jsonb;terms jsonb;response jsonb;own_slot jsonb;key text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='qualified room command requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_kind IS NULL OR p_kind NOT IN('ready','countdown') OR NOT app.native_room_intent_is_valid(p_room,p_original) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 request_id:=(p_original->>'requestId')::uuid;basis:=(p_original->>'expectedRevision')::numeric;
 scope_name:=CASE p_kind WHEN 'ready' THEN 'qualified_ready_intent_v1' ELSE 'qualified_countdown_intent_v1' END;
 parent:=app.begin_account_command(player.id,request_id,scope_name,p_room::text,jsonb_build_object('room',p_room,'original',p_original));
 -- Current caller authority, then exact original before new preparation/source.
 IF parent.state<>'pending' THEN RETURN parent.response_payload;END IF;
 IF jsonb_typeof(p_prepared) IS DISTINCT FROM 'object' OR octet_length(p_prepared::text)>2097152
  OR p_prepared->>'version' IS DISTINCT FROM (CASE p_kind WHEN 'ready' THEN 'native-ready-prepared-v1' ELSE 'native-countdown-prepared-v1' END)
  OR EXISTS(SELECT 1 FROM unnest(ARRAY['sourceHash','releaseId','legalityRevisionId','qualificationRef']) k WHERE jsonb_typeof(p_prepared->k) IS DISTINCT FROM 'string')
  OR p_prepared->>'sourceHash' !~ '^[0-9a-f]{64}$'
  OR length(btrim(p_prepared->>'qualificationRef')) NOT BETWEEN 1 AND 2048
  OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_prepared) k WHERE k NOT IN('version','sourceHash','releaseId','legalityRevisionId','qualificationRef','selectedRevisionId'))
  OR(p_kind='countdown' AND p_prepared ? 'selectedRevisionId') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid trusted room preparation';END IF;
 FOREACH key IN ARRAY (CASE p_kind WHEN 'ready' THEN ARRAY['releaseId','legalityRevisionId','selectedRevisionId'] ELSE ARRAY['releaseId','legalityRevisionId'] END) LOOP
  IF jsonb_typeof(p_prepared->key) IS DISTINCT FROM 'string' OR p_prepared->>key !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR p_prepared->>key='00000000-0000-0000-0000-000000000000' THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid trusted room source identity';END IF;
 END LOOP;
 release_id:=(p_prepared->>'releaseId')::uuid;legality_id:=(p_prepared->>'legalityRevisionId')::uuid;
 IF p_kind='ready' THEN
  revision_id:=(p_prepared->>'selectedRevisionId')::uuid;
  child:=app.claim_set_room_ready(player.id,p_room,basis,true,revision_id,p_prepared->>'qualificationRef',parent.id);
 ELSE child:=app.claim_start_room_countdown(player.id,p_room,basis,p_prepared->>'qualificationRef',parent.id);END IF;
 -- Both identities precede the competing room lock. Canonical apply owns writes.
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 player:=app.lock_session_account(p_subject,p_session);
 source:=app.read_native_room_source(p_room,basis,release_id,legality_id);
 IF source->>'status' IS DISTINCT FROM 'success' OR source->'data'->>'sourceHash' IS DISTINCT FROM p_prepared->>'sourceHash' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='trusted room source changed; reprepare original';END IF;
 terms:=source->'data'->'terms';
 IF p_kind='ready' THEN
  SELECT x INTO own_slot FROM jsonb_array_elements(terms->'slots') x WHERE x->>'controllerKind'='human' AND x->>'accountId'=player.id::text;
  IF own_slot IS NULL OR own_slot->>'sourceStatus'<>'available' OR own_slot->>'selectedRevisionId' IS DISTINCT FROM revision_id::text THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='trusted Ready preparation lacks own selected source';END IF;
 ELSE
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(terms->'slots') x WHERE x->>'sourceStatus'<>'available') THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='trusted countdown preparation has incomplete source';END IF;
 END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','qualified-human-v1','operation',p_kind,
  'original',p_original,'targetId',p_room,'sourceTerms',terms,'sourceHash',source->'data'->>'sourceHash',
  'prepared',p_prepared,'childReceiptId',child.id) WHERE id=parent.id;
 IF child.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new room original has preexisting canonical child';END IF;
 IF p_kind='ready' THEN response:=app.apply_set_room_ready(child.id,player.id,p_room,basis,true,revision_id,p_prepared->>'qualificationRef',parent.id);
 ELSE response:=app.apply_start_room_countdown(child.id,player.id,p_room,basis,p_prepared->>'qualificationRef',parent.id);END IF;
 -- A revocation during any canonical room/slot/deck/refresh wait rolls back the
 -- whole function, including parent and child, before returning unauthorized.
 player:=app.lock_session_account(p_subject,p_session);
 RETURN app.finish_account_command(parent.id,response->>'status'='success',response->'data');
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;

CREATE FUNCTION app.qualify_room_ready_command(p_subject uuid,p_session uuid,p_room uuid,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.qualify_native_room_command(p_subject,p_session,p_room,p_original,p_prepared,'ready') $$;
CREATE FUNCTION app.qualify_room_countdown_command(p_subject uuid,p_session uuid,p_room uuid,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.qualify_native_room_command(p_subject,p_session,p_room,p_original,p_prepared,'countdown') $$;

CREATE FUNCTION app.read_qualified_native_room_command(p_subject uuid,p_session uuid,p_room uuid,p_original jsonb,p_kind text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;receipt app.command_receipt;scope_name text;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_kind IS NULL OR p_kind NOT IN('ready','countdown') OR NOT app.native_room_intent_is_valid(p_room,p_original) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 scope_name:=CASE p_kind WHEN 'ready' THEN 'qualified_ready_intent_v1' ELSE 'qualified_countdown_intent_v1' END;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text
  AND scope=scope_name AND request_ref=p_original->>'requestId';
 IF receipt.id IS NULL THEN RETURN jsonb_build_object('status','success','data',jsonb_build_object('state','unknown','response',NULL));END IF;
 IF receipt.account_id IS DISTINCT FROM player.id OR receipt.target_ref IS DISTINCT FROM p_room::text
  OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('room',p_room,'original',p_original)) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='qualified original not terminal';END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('state',receipt.state,'response',receipt.response_payload));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
CREATE FUNCTION app.read_qualified_room_ready_command(p_subject uuid,p_session uuid,p_room uuid,p_original jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.read_qualified_native_room_command(p_subject,p_session,p_room,p_original,'ready') $$;
CREATE FUNCTION app.read_qualified_room_countdown_command(p_subject uuid,p_session uuid,p_room uuid,p_original jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.read_qualified_native_room_command(p_subject,p_session,p_room,p_original,'countdown') $$;

CREATE FUNCTION app.check_qualified_room_original(p_receipt uuid) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE parent app.command_receipt;child app.command_receipt;source jsonb;terms jsonb;kind text;
BEGIN
 SELECT * INTO parent FROM app.command_receipt WHERE id=p_receipt AND scope IN('qualified_ready_intent_v1','qualified_countdown_intent_v1');
 IF parent.id IS NULL THEN RETURN;END IF;source:=parent.normalized_source;
 kind:=CASE parent.scope WHEN 'qualified_ready_intent_v1' THEN 'ready' ELSE 'countdown' END;
 IF parent.state='pending' OR source->>'version' IS DISTINCT FROM 'qualified-human-v1' OR source->>'operation' IS DISTINCT FROM kind
  OR parent.account_id IS NULL OR parent.principal_key IS DISTINCT FROM 'human:'||parent.account_id::text OR parent.controller_grant_id IS NOT NULL
  OR source->>'targetId' IS DISTINCT FROM parent.target_ref OR source->'original'->>'requestId' IS DISTINCT FROM parent.request_ref
  OR parent.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('room',parent.target_ref::uuid,'original',source->'original'))
  OR source->>'sourceHash' IS DISTINCT FROM app.auth_json_hash(source->'sourceTerms')
  OR source->'prepared'->>'sourceHash' IS DISTINCT FROM source->>'sourceHash' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified room original lacks exact terminal source';END IF;
 SELECT * INTO child FROM app.command_receipt WHERE id=(source->>'childReceiptId')::uuid;
 terms:=jsonb_build_object('basis',(source->'original'->>'expectedRevision')::numeric,'qualification',source->'prepared'->>'qualificationRef');
 IF kind='ready' THEN terms:=terms||jsonb_build_object('ready',true,'revision',source->'prepared'->>'selectedRevisionId');END IF;
 IF child.id IS NULL OR child.scope IS DISTINCT FROM (CASE kind WHEN 'ready' THEN 'room_ready' ELSE 'room_countdown' END)
  OR child.principal_key IS DISTINCT FROM parent.principal_key OR child.account_id IS DISTINCT FROM parent.account_id
  OR child.request_ref IS DISTINCT FROM parent.id::text OR child.target_ref IS DISTINCT FROM parent.target_ref
  OR child.payload_hash IS DISTINCT FROM app.auth_json_hash(terms) OR child.state IS DISTINCT FROM parent.state
  OR child.response_payload IS DISTINCT FROM parent.response_payload THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified room original differs from canonical child';END IF;
END $$;
CREATE FUNCTION app.guard_qualified_room_original() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$ BEGIN PERFORM app.check_qualified_room_original(NEW.id);RETURN NULL;END $$;
CREATE CONSTRAINT TRIGGER qualified_room_original_source AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW WHEN(NEW.scope IN('qualified_ready_intent_v1','qualified_countdown_intent_v1')) EXECUTE FUNCTION app.guard_qualified_room_original();

REVOKE ALL ON FUNCTION app.native_room_intent_is_valid(uuid,jsonb),app.qualify_native_room_command(uuid,uuid,uuid,jsonb,jsonb,text),
 app.qualify_room_ready_command(uuid,uuid,uuid,jsonb,jsonb),app.qualify_room_countdown_command(uuid,uuid,uuid,jsonb,jsonb),
 app.read_qualified_native_room_command(uuid,uuid,uuid,jsonb,text),app.read_qualified_room_ready_command(uuid,uuid,uuid,jsonb),
 app.read_qualified_room_countdown_command(uuid,uuid,uuid,jsonb),app.check_qualified_room_original(uuid),app.guard_qualified_room_original()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
