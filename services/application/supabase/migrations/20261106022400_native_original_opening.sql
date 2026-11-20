BEGIN;

-- Narrow native protocol helper; the general account helper retains its64KiB
-- original envelope. Only closed native families can use the larger allowance.
CREATE FUNCTION app.begin_native_source_receipt(p_principal text,p_request text,p_scope text,p_target uuid,p_terms jsonb,p_account uuid,p_grant uuid)
RETURNS app.command_receipt LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;fingerprint text;
BEGIN
 IF p_scope IS NULL OR p_scope NOT IN('native_opening_v1','native_player_v1','native_host_input_v1','native_view_v1','native_cursor_v1','qualified_between_intent_v1')
  OR p_target IS NULL OR p_target='00000000-0000-0000-0000-000000000000'::uuid
  OR p_principal IS NULL OR length(p_principal)>128 OR p_request IS NULL OR length(p_request) NOT BETWEEN 1 AND 256
  OR jsonb_typeof(p_terms) IS DISTINCT FROM 'object' OR octet_length(p_terms::text)>2097152
  OR(p_account IS NOT NULL AND p_grant IS NOT NULL) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native original receipt terms';END IF;
 IF (p_scope='native_opening_v1' AND(p_principal IS DISTINCT FROM 'native-opening:'||p_target::text OR p_request IS DISTINCT FROM p_target::text OR p_account IS NOT NULL OR p_grant IS NOT NULL))
  OR(p_scope='native_host_input_v1' AND(p_principal IS DISTINCT FROM 'native-system:'||p_target::text OR p_account IS NOT NULL OR p_grant IS NOT NULL))
  OR(p_scope='native_player_v1' AND(p_principal !~ '^native-seat:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' OR(p_account IS NULL)=(p_grant IS NULL)))
  OR(p_scope IN('native_view_v1','native_cursor_v1') AND(p_principal !~ '^native-viewer:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' OR p_account IS NOT NULL OR p_grant IS NOT NULL))
  OR(p_scope='qualified_between_intent_v1' AND(p_grant IS NULL OR p_account IS NOT NULL OR p_principal IS DISTINCT FROM 'controller:'||p_grant::text)) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native receipt principal';END IF;
 fingerprint:=app.auth_json_hash(p_terms);
 INSERT INTO app.command_receipt(account_id,controller_grant_id,principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES(p_account,p_grant,p_principal,p_request,p_scope,p_target::text,fingerprint,'pending','')
 ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NOT NULL THEN RETURN receipt;END IF;
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key=p_principal AND scope=p_scope AND request_ref=p_request FOR UPDATE;
 IF receipt.target_ref IS DISTINCT FROM p_target::text OR receipt.payload_hash IS DISTINCT FROM fingerprint
  OR receipt.account_id IS DISTINCT FROM p_account OR receipt.controller_grant_id IS DISTINCT FROM p_grant THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='request identity was used for different command terms';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native original disposition is not terminal';END IF;
 RETURN receipt;
END $$;

CREATE FUNCTION app.check_native_opening_receipt(p_receipt uuid) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE parent app.command_receipt;child app.command_receipt;source jsonb;native_game_id uuid;native_context_id uuid;
BEGIN
 SELECT * INTO parent FROM app.command_receipt WHERE id=p_receipt AND scope='native_opening_v1';
 IF parent.id IS NULL THEN RETURN;END IF;
 source:=parent.normalized_source;
 IF jsonb_typeof(source) IS DISTINCT FROM 'object'
  OR EXISTS(SELECT 1 FROM unnest(ARRAY['version','matchId','gameId','owner','originalBasis','nativeEpoch','runtimeId','programSourceId','childReceiptId']) k WHERE jsonb_typeof(source->k) IS DISTINCT FROM 'string')
  OR jsonb_typeof(source->'opening') IS DISTINCT FROM 'object'
  OR EXISTS(SELECT 1 FROM jsonb_object_keys(source) k WHERE k NOT IN('version','matchId','gameId','owner','originalBasis','opening','nativeEpoch','runtimeId','programSourceId','childReceiptId'))
  OR source->>'originalBasis' !~ '^(0|[1-9][0-9]{0,77})$'
  OR parent.state<>'accepted' OR source->>'version' IS DISTINCT FROM 'native-opening-v1'
  OR source->>'gameId' IS DISTINCT FROM parent.target_ref
  OR parent.principal_key IS DISTINCT FROM 'native-opening:'||parent.target_ref
  OR parent.request_ref IS DISTINCT FROM parent.target_ref OR parent.account_id IS NOT NULL OR parent.controller_grant_id IS NOT NULL
  OR parent.payload_hash IS DISTINCT FROM app.auth_json_hash(source-'childReceiptId')
  OR NOT app.native_u64_is_canonical(source->>'nativeEpoch')
  OR source->>'runtimeId' !~ '^[0-9a-f]{64}$' OR source->>'programSourceId' !~ '^[0-9a-f]{64}$' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native opening lacks exact original source';END IF;
 native_game_id:=(source->>'gameId')::uuid;native_context_id:=(source->'opening'->>'contextId')::uuid;
 IF NOT app.native_artifact_ref_is_valid(source->'opening'->>'snapshotRef',native_game_id,native_context_id,'initial')
  OR NOT app.native_artifact_ref_is_valid(source->'opening'->>'stateRef',native_game_id,native_context_id,'state')
  OR NOT app.native_artifact_ref_is_valid(source->'opening'->>'factsRef',native_game_id,native_context_id,'facts')
  OR NOT app.native_artifact_ref_is_valid(source->'opening'->>'eventRef',native_game_id,native_context_id,'event') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native opening source artifacts have wrong scope';END IF;
 SELECT * INTO child FROM app.command_receipt WHERE id=(source->>'childReceiptId')::uuid;
 IF child.id IS NULL OR child.scope<>'game_opening_v1' OR child.state<>'accepted'
  OR child.principal_key IS DISTINCT FROM 'match-opening:'||(source->>'matchId')
  OR child.target_ref IS DISTINCT FROM native_game_id::text OR child.request_ref IS DISTINCT FROM native_game_id::text
  OR child.response_payload IS DISTINCT FROM parent.response_payload
  OR child.normalized_source->>'version' IS DISTINCT FROM 'game-opening-v1'
  OR child.normalized_source->>'matchId' IS DISTINCT FROM source->>'matchId'
  OR child.normalized_source->>'gameId' IS DISTINCT FROM native_game_id::text
  OR child.normalized_source->>'owner' IS DISTINCT FROM source->>'owner'
  OR(child.normalized_source-ARRAY['version','matchId','gameId','owner','clockValues']) IS DISTINCT FROM source->'opening'
  OR NOT EXISTS(SELECT 1 FROM app.game_context c JOIN app.game_checkpoint k ON k.context_id=c.id AND k.game_id=c.game_id
   WHERE c.id=native_context_id AND c.game_id=native_game_id AND k.accepted_revision=0 AND k.source_receipt_id=child.id
    AND c.snapshot_ref=source->'opening'->>'snapshotRef' AND c.context_revision=source->'opening'->>'contextRevision'
    AND k.private_state_ref=source->'opening'->>'stateRef' AND k.facts_ref=source->'opening'->>'factsRef') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native opening differs from canonical accepted child';END IF;
 PERFORM app.check_native_creator_frame(child.id);
END $$;

CREATE FUNCTION app.guard_native_opening_receipt() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN PERFORM app.check_native_opening_receipt(NEW.id);RETURN NULL;END $$;
CREATE CONSTRAINT TRIGGER native_opening_original_source AFTER INSERT OR UPDATE ON app.command_receipt
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN(NEW.scope='native_opening_v1') EXECUTE FUNCTION app.guard_native_opening_receipt();

CREATE FUNCTION app.accept_native_opening(p_match uuid,p_basis numeric,p_game uuid,p_owner text,p_opening jsonb,p_native_epoch text,p_runtime_id text,p_program_source_id text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;child app.command_receipt;terms jsonb;reservation jsonb;response jsonb;native_context_id uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native opening requires read committed retry';END IF;
 IF p_match IS NULL OR p_game IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_match,p_game)
  OR p_basis IS NULL OR p_basis::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256
  OR NOT app.native_u64_is_canonical(p_native_epoch) OR p_runtime_id IS NULL OR p_runtime_id !~ '^[0-9a-f]{64}$'
  OR p_program_source_id IS NULL OR p_program_source_id !~ '^[0-9a-f]{64}$'
  OR jsonb_typeof(p_opening) IS DISTINCT FROM 'object' OR octet_length(p_opening::text)>16384 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native opening original';END IF;
 terms:=jsonb_build_object('version','native-opening-v1','matchId',p_match,'gameId',p_game,'owner',p_owner,
  'originalBasis',p_basis::text,'opening',p_opening,'nativeEpoch',p_native_epoch,'runtimeId',p_runtime_id,'programSourceId',p_program_source_id);
 receipt:=app.begin_native_source_receipt('native-opening:'||p_game::text,p_game::text,'native_opening_v1',p_game,terms,NULL,NULL);
 -- Exact retained original wins before reservation lookup: it is now active.
 IF receipt.state<>'pending' THEN PERFORM app.check_native_opening_receipt(receipt.id);RETURN receipt.response_payload;END IF;
 IF jsonb_typeof(p_opening->'contextId') IS DISTINCT FROM 'string' OR p_opening->>'contextId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native opening context';END IF;
 native_context_id:=(p_opening->>'contextId')::uuid;
 IF NOT app.native_artifact_ref_is_valid(p_opening->>'snapshotRef',p_game,native_context_id,'initial')
  OR NOT app.native_artifact_ref_is_valid(p_opening->>'stateRef',p_game,native_context_id,'state')
  OR NOT app.native_artifact_ref_is_valid(p_opening->>'factsRef',p_game,native_context_id,'facts')
  OR NOT app.native_artifact_ref_is_valid(p_opening->>'eventRef',p_game,native_context_id,'event') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='native opening artifacts have wrong scope';END IF;
 reservation:=app.read_native_admission_source(p_game);
 IF reservation->>'status' IS DISTINCT FROM 'success' OR reservation->'data'->>'matchId' IS DISTINCT FROM p_match::text
  OR reservation->'data'->>'matchRevision' IS DISTINCT FROM p_basis::text THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native opening reservation unavailable; retry current source';END IF;
 response:=app.accept_reserved_game(p_match,p_basis,p_game,p_owner,p_opening);
 SELECT * INTO STRICT child FROM app.command_receipt WHERE principal_key='match-opening:'||p_match::text
  AND scope='game_opening_v1' AND request_ref=p_game::text;
 UPDATE app.command_receipt SET normalized_source=terms||jsonb_build_object('childReceiptId',child.id) WHERE id=receipt.id;
 response:=app.finish_account_command(receipt.id,true,response->'data');
 PERFORM app.check_native_opening_receipt(receipt.id);
 RETURN response;
END $$;

CREATE FUNCTION app.read_native_opening_source(p_game uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;checkpoint_id uuid;native_context_id uuid;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='native-opening:'||p_game::text
  AND scope='native_opening_v1' AND target_ref=p_game::text AND request_ref=p_game::text AND state='accepted';
 IF receipt.id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('game','unavailable'));END IF;
 PERFORM app.check_native_opening_receipt(receipt.id);
 native_context_id:=(receipt.normalized_source->'opening'->>'contextId')::uuid;
 SELECT id INTO STRICT checkpoint_id FROM app.game_checkpoint cp WHERE cp.game_id=p_game AND cp.context_id=native_context_id AND accepted_revision=0;
 RETURN jsonb_build_object('status','success','data',receipt.normalized_source||jsonb_build_object('receiptId',receipt.id,'contextId',native_context_id,'checkpointId',checkpoint_id));
END $$;

REVOKE ALL ON FUNCTION app.begin_native_source_receipt(text,text,text,uuid,jsonb,uuid,uuid),app.check_native_opening_receipt(uuid),
 app.guard_native_opening_receipt(),app.accept_native_opening(uuid,numeric,uuid,text,jsonb,text,text,text),app.read_native_opening_source(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
