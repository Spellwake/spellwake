BEGIN;

CREATE FUNCTION app.native_uuid_is_canonical(p_value text) RETURNS boolean
LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT coalesce(p_value ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
  AND p_value<>'00000000-0000-0000-0000-000000000000',false)
$$;
CREATE FUNCTION app.native_basis_is_valid(p_basis jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF jsonb_typeof(p_basis) IS DISTINCT FROM 'object' THEN RETURN false;END IF;
 RETURN coalesce(NOT EXISTS(SELECT 1 FROM jsonb_object_keys(p_basis) k WHERE k NOT IN('gameId','contextId','epoch','revision'))
  AND NOT EXISTS(SELECT 1 FROM unnest(ARRAY['gameId','contextId','epoch','revision']) k WHERE jsonb_typeof(p_basis->k) IS DISTINCT FROM 'string')
  AND app.native_uuid_is_canonical(p_basis->>'gameId') AND app.native_uuid_is_canonical(p_basis->>'contextId')
  AND app.native_u64_is_canonical(p_basis->>'epoch') AND app.native_u64_is_canonical(p_basis->>'revision'),false);
END $$;
CREATE FUNCTION app.native_player_original_is_valid(p_original jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF jsonb_typeof(p_original) IS DISTINCT FROM 'object' OR octet_length(p_original::text)>1048576 THEN RETURN false;END IF;
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_original) k WHERE k NOT IN('requestId','basis','action'))
  OR jsonb_typeof(p_original->'requestId') IS DISTINCT FROM 'string' OR NOT app.native_uuid_is_canonical(p_original->>'requestId')
  OR NOT app.native_basis_is_valid(p_original->'basis') OR jsonb_typeof(p_original->'action') IS DISTINCT FROM 'object' THEN RETURN false;END IF;
 -- The maintained Rust/shared decoder validates each complete action. SQL does
 -- not duplicate rules/targets/payment/choice validation or trust public handles.
 RETURN coalesce(jsonb_typeof(p_original->'action'->'kind')='string'
  AND p_original->'action'->>'kind' IN('cast-untargeted','cast-targeted','activate-ability','activate-mana','play-land','pass','choose','concede'),false);
END $$;
CREATE FUNCTION app.native_rejection_is_valid(p_errors jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF jsonb_typeof(p_errors) IS DISTINCT FROM 'object' THEN RETURN false;END IF;
 RETURN(SELECT count(*) BETWEEN 1 AND 11 AND bool_and(key IN('request','game','basis','actor','source','ability','target','decision','response','payment','action')
  AND jsonb_typeof(value)='string' AND value#>>'{}' IN('invalid','stale','unavailable','not permitted','unsupported')) FROM jsonb_each(p_errors));
END $$;
CREATE FUNCTION app.native_candidate_refs_are_valid(p_candidate jsonb,p_game uuid,p_context uuid) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE event_ref jsonb;
BEGIN
 IF jsonb_typeof(p_candidate) IS DISTINCT FROM 'object' OR octet_length(p_candidate::text)>1048576 THEN RETURN false;END IF;
 IF jsonb_typeof(p_candidate->'contextId') IS DISTINCT FROM 'string' OR p_candidate->>'contextId' IS DISTINCT FROM p_context::text
  OR NOT app.native_artifact_ref_is_valid(p_candidate->>'stateRef',p_game,p_context,'state')
  OR NOT app.native_artifact_ref_is_valid(p_candidate->>'factsRef',p_game,p_context,'facts')
  OR jsonb_typeof(p_candidate->'events') IS DISTINCT FROM 'array' THEN RETURN false;END IF;
 IF jsonb_array_length(p_candidate->'events') NOT BETWEEN 1 AND 1024 THEN RETURN false;END IF;
 FOR event_ref IN SELECT * FROM jsonb_array_elements(p_candidate->'events') LOOP
  IF jsonb_typeof(event_ref) IS DISTINCT FROM 'string' OR NOT app.native_artifact_ref_is_valid(event_ref#>>'{}',p_game,p_context,'event') THEN RETURN false;END IF;
 END LOOP;
 RETURN true;
END $$;
CREATE FUNCTION app.native_player_prepared_is_valid(p_prepared jsonb,p_original jsonb,p_actor uuid) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE kind text;
BEGIN
 IF NOT app.native_player_original_is_valid(p_original) OR jsonb_typeof(p_prepared) IS DISTINCT FROM 'object'
  OR octet_length(p_prepared::text)>2097152 THEN RETURN false;END IF;
 kind:=p_prepared->>'version';
 IF kind IS NULL OR kind NOT IN('native-player-accept-v1','native-player-reject-v1')
  OR p_prepared->'basis' IS DISTINCT FROM p_original->'basis'
  OR p_prepared->>'actorSeatId' IS DISTINCT FROM p_actor::text
  OR jsonb_typeof(p_prepared->'viewReceiptId') IS DISTINCT FROM 'string' OR NOT app.native_uuid_is_canonical(p_prepared->>'viewReceiptId')
  OR jsonb_typeof(p_prepared->'runtimeId') IS DISTINCT FROM 'string' OR p_prepared->>'runtimeId' !~ '^[0-9a-f]{64}$'
  OR jsonb_typeof(p_prepared->'programSourceId') IS DISTINCT FROM 'string' OR p_prepared->>'programSourceId' !~ '^[0-9a-f]{64}$' THEN RETURN false;END IF;
 IF kind='native-player-reject-v1' THEN
  RETURN NOT EXISTS(SELECT 1 FROM jsonb_object_keys(p_prepared) k WHERE k NOT IN('version','basis','actorSeatId','viewReceiptId','runtimeId','programSourceId','errors'))
   AND app.native_rejection_is_valid(p_prepared->'errors');
 END IF;
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_prepared) k WHERE k NOT IN('version','basis','actorSeatId','viewReceiptId','runtimeId','programSourceId','inputRef','owner','fenceEpoch','candidate'))
  OR jsonb_typeof(p_prepared->'owner') IS DISTINCT FROM 'string' OR length(btrim(p_prepared->>'owner')) NOT BETWEEN 1 AND 256
  OR jsonb_typeof(p_prepared->'fenceEpoch') IS DISTINCT FROM 'string' OR p_prepared->>'fenceEpoch' !~ '^(0|[1-9][0-9]{0,77})$'
  OR NOT app.native_artifact_ref_is_valid(p_prepared->>'inputRef',(p_original->'basis'->>'gameId')::uuid,(p_original->'basis'->>'contextId')::uuid,'input') THEN RETURN false;END IF;
 RETURN app.native_candidate_refs_are_valid(p_prepared->'candidate',(p_original->'basis'->>'gameId')::uuid,(p_original->'basis'->>'contextId')::uuid);
END $$;

-- Both original and canonical child are claimed before entering this shared
-- frontier fence. It introduces no account/room/controller lock after the game.
CREATE FUNCTION app.lock_native_game_source(p_game uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent_id uuid;game_row app.game;host app.operational_state;source jsonb;opening jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native source fence requires read committed retry';END IF;
 SELECT match_id INTO parent_id FROM app.game WHERE id=p_game;
 IF parent_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='SW002',MESSAGE='native game source unavailable';END IF;
 PERFORM id FROM app.match WHERE id=parent_id FOR NO KEY UPDATE;
 SELECT * INTO game_row FROM app.game WHERE id=p_game FOR UPDATE;
 IF game_row.match_id IS DISTINCT FROM parent_id THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native game parent changed before fence';END IF;
 SELECT * INTO host FROM app.operational_state WHERE game_id=p_game FOR NO KEY UPDATE;
 PERFORM clock.id FROM app.match_clock clock JOIN app.match_seat seat ON seat.id=clock.match_seat_id
  WHERE seat.match_id=parent_id ORDER BY clock.match_seat_id FOR NO KEY UPDATE OF clock;
 IF host.id IS NULL OR host.availability<>'available' OR NOT EXISTS(SELECT 1 FROM app.match WHERE id=parent_id AND availability='available') THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native operational source unavailable';END IF;
 opening:=app.read_native_opening_source(p_game);source:=app.read_native_game_source(p_game);
 IF opening->>'status' IS DISTINCT FROM 'success' OR source->>'status' IS DISTINCT FROM 'success' THEN
  RAISE EXCEPTION USING ERRCODE='SW002',MESSAGE='native accepted source unavailable';END IF;
 IF NOT app.native_u64_is_canonical(source->'data'->>'acceptedRevision') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native revision exceeds protocol';END IF;
 RETURN jsonb_build_object('native',source->'data','opening',opening->'data','basis',jsonb_build_object('gameId',p_game,
  'contextId',opening->'data'->>'contextId','epoch',opening->'data'->>'nativeEpoch','revision',source->'data'->>'acceptedRevision'));
END $$;

CREATE FUNCTION app.native_original_basis_state(p_original jsonb,p_source jsonb) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE checkpoint app.game_checkpoint;basis jsonb:=p_original->'basis';terminal boolean;
BEGIN
 IF basis->>'gameId' IS DISTINCT FROM p_source->'basis'->>'gameId'
  OR basis->>'contextId' IS DISTINCT FROM p_source->'basis'->>'contextId'
  OR basis->>'epoch' IS DISTINCT FROM p_source->'basis'->>'epoch'
  OR(basis->>'revision')::numeric>(p_source->'basis'->>'revision')::numeric THEN
  RAISE EXCEPTION USING ERRCODE='SW002',MESSAGE='native original lineage unavailable';END IF;
 SELECT * INTO checkpoint FROM app.game_checkpoint WHERE game_id=(basis->>'gameId')::uuid
  AND context_id=(basis->>'contextId')::uuid AND accepted_revision=(basis->>'revision')::numeric;
 IF checkpoint.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='SW002',MESSAGE='native original checkpoint unavailable';END IF;
 IF checkpoint.accepted_revision=0 THEN
  IF checkpoint.id::text IS DISTINCT FROM p_source->'opening'->>'checkpointId' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original checkpoint differs';END IF;
 ELSIF NOT EXISTS(SELECT 1 FROM app.command_receipt parent WHERE parent.scope IN('native_player_v1','native_host_input_v1') AND parent.state='accepted'
  AND parent.target_ref=basis->>'gameId' AND parent.normalized_source->>'acceptedCheckpointId'=checkpoint.id::text
  AND parent.normalized_source->>'childReceiptId'=checkpoint.source_receipt_id::text) THEN
  RAISE EXCEPTION USING ERRCODE='SW002',MESSAGE='native original protocol checkpoint unavailable';END IF;
 PERFORM app.check_native_creator_frame(checkpoint.source_receipt_id);
 terminal:=p_source->'native'->>'gameState'<>'active';
 IF basis=p_source->'basis' AND NOT terminal THEN RETURN jsonb_build_object('state','current','originalCheckpointId',checkpoint.id);END IF;
 RETURN jsonb_build_object('state','stale','rejectionSource',jsonb_build_object('kind','stale-basis','originalCheckpointId',checkpoint.id,
  'observedCheckpointId',p_source->'native'->'checkpoint'->>'id','observedBasis',p_source->'basis','terminal',terminal));
END $$;

-- A currently verified replacement controller may recover the SAME native-seat
-- original, while its recorded original grant stays immutable. All other scopes
-- retain strict original grant identity. Human ownership cannot be reassigned.
CREATE OR REPLACE FUNCTION app.begin_native_source_receipt(p_principal text,p_request text,p_scope text,p_target uuid,p_terms jsonb,p_account uuid,p_grant uuid)
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
  OR receipt.account_id IS DISTINCT FROM p_account OR(receipt.controller_grant_id IS DISTINCT FROM p_grant AND NOT(
   p_scope='native_player_v1' AND p_grant IS NOT NULL AND receipt.controller_grant_id IS NOT NULL
   AND EXISTS(SELECT 1 FROM app.controller_grant original_grant JOIN app.controller_grant current_grant ON current_grant.id=p_grant
    WHERE original_grant.id=receipt.controller_grant_id AND original_grant.match_seat_id=current_grant.match_seat_id
     AND 'native-seat:'||original_grant.match_seat_id::text=p_principal))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='request identity was used for different command terms';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native original disposition is not terminal';END IF;
 RETURN receipt;
END $$;

REVOKE ALL ON FUNCTION app.native_uuid_is_canonical(text),app.native_basis_is_valid(jsonb),app.native_player_original_is_valid(jsonb),
 app.native_rejection_is_valid(jsonb),app.native_candidate_refs_are_valid(jsonb,uuid,uuid),app.native_player_prepared_is_valid(jsonb,jsonb,uuid),
 app.lock_native_game_source(uuid),app.native_original_basis_state(jsonb,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
