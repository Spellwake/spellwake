BEGIN;

CREATE FUNCTION app.recheck_native_player_actor(p_subject uuid,p_session uuid,p_grant uuid,p_epoch numeric,p_secret_ref text,p_actor jsonb,p_match uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 IF p_subject IS NOT NULL THEN
  player:=app.lock_session_account(p_subject,p_session);
  IF player.id::text IS DISTINCT FROM p_actor->>'accountId' OR NOT app.native_account_may_continue(player,p_match)
   OR NOT EXISTS(SELECT 1 FROM app.match_seat WHERE id=(p_actor->>'seatId')::uuid AND match_id=p_match AND controller_kind='human' AND account_id=player.id) THEN
   RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='current admitted player authority required';END IF;
 ELSIF app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,p_match) IS DISTINCT FROM p_actor THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='current adopted controller authority required';END IF;
END $$;

CREATE FUNCTION app.submit_native_player_command(p_subject uuid,p_session uuid,p_grant uuid,p_epoch numeric,p_secret_ref text,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;actor jsonb;parent app.command_receipt;child app.command_receipt;
 native_game_id uuid;native_match_id uuid;native_seat_id uuid;source jsonb;basis_state jsonb;view_receipt app.command_receipt;
 canonical jsonb;ack jsonb;checkpoint_id uuid;normalized jsonb;prepared_valid boolean;is_accept boolean;is_human boolean:=p_subject IS NOT NULL;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native player commit requires read committed retry';END IF;
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('supply-population-cutoff-v1',0));
 IF is_human THEN
  IF p_grant IS NOT NULL OR p_epoch IS NOT NULL OR p_secret_ref IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='mixed native authority';END IF;
  player:=app.lock_session_account(p_subject,p_session);
 ELSIF p_session IS NOT NULL OR p_grant IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 IF NOT app.native_player_original_is_valid(p_original) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 native_game_id:=(p_original->'basis'->>'gameId')::uuid;
 SELECT g.match_id INTO native_match_id FROM app.game g WHERE g.id=native_game_id;
 IF is_human THEN
  SELECT jsonb_build_object('seatId',seat.id,'kind','human','accountId',player.id,'grantId',NULL,'bindingId',NULL,'bindingEpoch',NULL) INTO actor
   FROM app.match_seat seat WHERE seat.match_id=native_match_id AND seat.account_id=player.id AND seat.controller_kind='human';
 ELSE actor:=app.lock_native_controller_actor(p_grant,p_epoch,p_secret_ref,native_match_id);END IF;
 IF actor IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 native_seat_id:=(actor->>'seatId')::uuid;
 parent:=app.begin_native_source_receipt('native-seat:'||native_seat_id::text,p_original->>'requestId','native_player_v1',native_game_id,
  jsonb_build_object('actorSeatId',native_seat_id,'original',p_original),player.id,p_grant);
 -- This lookup precedes current basis, host fence, new handles and preparation.
 IF parent.state<>'pending' THEN RETURN parent.response_payload;END IF;
 prepared_valid:=app.native_player_prepared_is_valid(p_prepared,p_original,native_seat_id);
 is_accept:=prepared_valid AND p_prepared->>'version'='native-player-accept-v1';
 BEGIN
  IF is_accept THEN
   child:=app.claim_accept_host_candidate(native_game_id,p_prepared->>'owner',(p_prepared->>'fenceEpoch')::numeric,
    (p_original->'basis'->>'revision')::numeric,parent.id,p_prepared->'candidate');
   IF child.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new native original has preexisting canonical child';END IF;
  END IF;
  source:=app.lock_native_game_source(native_game_id);
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
  IF is_accept AND(p_prepared->>'owner' IS DISTINCT FROM source->'native'->'operational'->>'ownerRef'
   OR p_prepared->>'fenceEpoch' IS DISTINCT FROM source->'native'->'operational'->>'fenceEpoch') THEN
   RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native prepared operational fence lost';END IF;
  basis_state:=app.native_original_basis_state(p_original,source);
  IF basis_state->>'state'='stale' THEN RAISE EXCEPTION USING ERRCODE='SW003',MESSAGE='verified stale native original';END IF;
  IF NOT prepared_valid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='current native original requires valid trusted preparation';END IF;
  IF p_prepared->>'runtimeId' IS DISTINCT FROM source->'opening'->>'runtimeId'
   OR p_prepared->>'programSourceId' IS DISTINCT FROM source->'opening'->>'programSourceId' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native preparation has different original runtime source';END IF;
  SELECT * INTO view_receipt FROM app.command_receipt WHERE id=(p_prepared->>'viewReceiptId')::uuid AND scope='native_view_v1' AND state='accepted';
  IF view_receipt.id IS NULL OR view_receipt.target_ref IS DISTINCT FROM native_game_id::text
   OR view_receipt.normalized_source->'basis' IS DISTINCT FROM p_original->'basis'
   OR view_receipt.normalized_source->>'viewerSeatId' IS DISTINCT FROM native_seat_id::text
   OR view_receipt.normalized_source->>'disclosure' IS DISTINCT FROM 'live' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native preparation lacks own exact issued View';END IF;
  PERFORM app.check_native_view_receipt(view_receipt.id);
  normalized:=jsonb_build_object('version','native-player-v1','actorSeatId',native_seat_id,'original',p_original,
   'prepared',p_prepared,'openingReceiptId',source->'opening'->>'receiptId','childReceiptId',NULL,'acceptedCheckpointId',NULL);
  IF NOT is_accept THEN
   UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
   ack:=app.finish_account_command(parent.id,false,p_prepared->'errors');
  ELSE
   IF p_original->'basis'->>'revision'='18446744073709551615' THEN RAISE EXCEPTION USING ERRCODE='22003',MESSAGE='native revision exhausted';END IF;
   canonical:=app.apply_accept_host_candidate(child.id,native_game_id,p_prepared->>'owner',(p_prepared->>'fenceEpoch')::numeric,
    (p_original->'basis'->>'revision')::numeric,parent.id,p_prepared->'candidate');
   IF canonical->>'status' IS DISTINCT FROM 'success' THEN
    IF canonical->'data' ? 'basis' THEN
     basis_state:=app.native_original_basis_state(p_original,app.lock_native_game_source(native_game_id));
     IF basis_state->>'state'='stale' THEN RAISE EXCEPTION USING ERRCODE='SW003',MESSAGE='verified stale native original';END IF;
     RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='canonical basis failure disagrees with locked native source';
    ELSIF canonical->'data' ?| ARRAY['game','context','availability','fence','clocks'] THEN
     RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native operational candidate not accepted';
    ELSE RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unexpected canonical native rejection';END IF;
   END IF;
   checkpoint_id:=(canonical->'data'->>'resourceId')::uuid;
   normalized:=normalized||jsonb_build_object('childReceiptId',child.id,'acceptedCheckpointId',checkpoint_id);
   UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
   ack:=app.finish_account_command(parent.id,true,jsonb_build_object('requestId',p_original->>'requestId','actorSeatId',native_seat_id,
    'gameId',native_game_id,'contextId',p_original->'basis'->>'contextId','epoch',p_original->'basis'->>'epoch','revision',canonical->'data'->>'revision'));
  END IF;
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
 EXCEPTION WHEN SQLSTATE 'SW003' THEN
  -- PL/pgSQL variables survive rollback; never reuse a rolled-back child, frame,
  -- response or source. Parent/account/controller scope predates this savepoint.
  child:=NULL;source:=NULL;basis_state:=NULL;view_receipt:=NULL;canonical:=NULL;ack:=NULL;checkpoint_id:=NULL;normalized:=NULL;
  source:=app.lock_native_game_source(native_game_id);
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
  IF is_accept AND(p_prepared->>'owner' IS DISTINCT FROM source->'native'->'operational'->>'ownerRef'
   OR p_prepared->>'fenceEpoch' IS DISTINCT FROM source->'native'->'operational'->>'fenceEpoch') THEN
   RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native prepared operational fence lost';END IF;
  basis_state:=app.native_original_basis_state(p_original,source);
  IF basis_state->>'state' IS DISTINCT FROM 'stale' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native stale observation changed before rejection fence';END IF;
  normalized:=jsonb_build_object('version','native-player-v1','actorSeatId',native_seat_id,'original',p_original,'prepared',NULL,
   'openingReceiptId',source->'opening'->>'receiptId','childReceiptId',NULL,'acceptedCheckpointId',NULL,'rejectionSource',basis_state->'rejectionSource');
  UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
  ack:=app.finish_account_command(parent.id,false,jsonb_build_object('basis','stale'));
 END;
 RETURN ack;
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object(CASE WHEN is_human THEN 'session' ELSE 'actor' END,CASE WHEN is_human THEN 'unauthorized' ELSE 'unavailable' END));
WHEN SQLSTATE 'SW002' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('game','unavailable'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.submit_native_human_command(p_subject uuid,p_session uuid,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.submit_native_player_command(p_subject,p_session,NULL,NULL,NULL,p_original,p_prepared) $$;
CREATE FUNCTION app.submit_native_controller_command(p_grant uuid,p_epoch numeric,p_secret_ref text,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.submit_native_player_command(NULL,NULL,p_grant,p_epoch,p_secret_ref,p_original,p_prepared) $$;

CREATE FUNCTION app.native_host_original_is_valid(p_original jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF jsonb_typeof(p_original) IS DISTINCT FROM 'object' OR octet_length(p_original::text)>1048576 THEN RETURN false;END IF;
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_original) k WHERE k NOT IN('version','basis','kind','inputRef','runtimeId','programSourceId'))
  OR p_original->>'version' IS DISTINCT FROM 'native-host-input-v1' OR NOT app.native_basis_is_valid(p_original->'basis')
  OR jsonb_typeof(p_original->'kind') IS DISTINCT FROM 'string' OR p_original->>'kind' NOT IN('entropy','clock','native-work')
  OR jsonb_typeof(p_original->'runtimeId') IS DISTINCT FROM 'string' OR p_original->>'runtimeId' !~ '^[0-9a-f]{64}$'
  OR jsonb_typeof(p_original->'programSourceId') IS DISTINCT FROM 'string' OR p_original->>'programSourceId' !~ '^[0-9a-f]{64}$' THEN RETURN false;END IF;
 RETURN app.native_artifact_ref_is_valid(p_original->>'inputRef',(p_original->'basis'->>'gameId')::uuid,(p_original->'basis'->>'contextId')::uuid,'input');
END $$;
CREATE FUNCTION app.commit_native_host_input(p_game uuid,p_request uuid,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent app.command_receipt;child app.command_receipt;source jsonb;canonical jsonb;normalized jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native system commit requires read committed retry';END IF;
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('supply-population-cutoff-v1',0));
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR NOT app.native_host_original_is_valid(p_original)
  OR p_original->'basis'->>'gameId' IS DISTINCT FROM p_game::text THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native system original';END IF;
 parent:=app.begin_native_source_receipt('native-system:'||p_game::text,p_request::text,'native_host_input_v1',p_game,p_original,NULL,NULL);
 IF parent.state<>'pending' THEN RETURN parent.response_payload;END IF;
 IF jsonb_typeof(p_prepared) IS DISTINCT FROM 'object' OR octet_length(p_prepared::text)>2097152 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native system preparation';END IF;
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_prepared) k WHERE k NOT IN('owner','fenceEpoch','candidate'))
  OR jsonb_typeof(p_prepared->'owner') IS DISTINCT FROM 'string' OR length(btrim(p_prepared->>'owner')) NOT BETWEEN 1 AND 256
  OR jsonb_typeof(p_prepared->'fenceEpoch') IS DISTINCT FROM 'string' OR p_prepared->>'fenceEpoch' !~ '^(0|[1-9][0-9]{0,77})$'
  OR NOT app.native_candidate_refs_are_valid(p_prepared->'candidate',p_game,(p_original->'basis'->>'contextId')::uuid) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native system candidate source';END IF;
 child:=app.claim_accept_host_candidate(p_game,p_prepared->>'owner',(p_prepared->>'fenceEpoch')::numeric,(p_original->'basis'->>'revision')::numeric,parent.id,p_prepared->'candidate');
 IF child.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new system original has preexisting canonical child';END IF;
 source:=app.lock_native_game_source(p_game);
 IF app.native_original_basis_state(p_original,source)->>'state' IS DISTINCT FROM 'current' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native system basis changed';END IF;
 IF p_original->>'runtimeId' IS DISTINCT FROM source->'opening'->>'runtimeId' OR p_original->>'programSourceId' IS DISTINCT FROM source->'opening'->>'programSourceId' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native system original runtime differs';END IF;
 IF p_original->'basis'->>'revision'='18446744073709551615' THEN RAISE EXCEPTION USING ERRCODE='22003',MESSAGE='native revision exhausted';END IF;
 canonical:=app.apply_accept_host_candidate(child.id,p_game,p_prepared->>'owner',(p_prepared->>'fenceEpoch')::numeric,(p_original->'basis'->>'revision')::numeric,parent.id,p_prepared->'candidate');
 IF canonical->>'status' IS DISTINCT FROM 'success' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native system operational candidate not accepted';END IF;
 normalized:=jsonb_build_object('version','native-host-commit-v1','original',p_original,'prepared',p_prepared,
  'openingReceiptId',source->'opening'->>'receiptId','childReceiptId',child.id,'acceptedCheckpointId',canonical->'data'->>'resourceId');
 UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
 RETURN app.finish_account_command(parent.id,true,canonical->'data');
END $$;

CREATE UNIQUE INDEX native_original_child ON app.command_receipt((normalized_source->>'childReceiptId'))
 WHERE scope IN('native_player_v1','native_host_input_v1') AND state='accepted';
CREATE UNIQUE INDEX native_original_checkpoint ON app.command_receipt((normalized_source->>'acceptedCheckpointId'))
 WHERE scope IN('native_player_v1','native_host_input_v1') AND state='accepted';

REVOKE ALL ON FUNCTION app.recheck_native_player_actor(uuid,uuid,uuid,numeric,text,jsonb,uuid),
 app.submit_native_player_command(uuid,uuid,uuid,numeric,text,jsonb,jsonb),app.submit_native_human_command(uuid,uuid,jsonb,jsonb),
 app.submit_native_controller_command(uuid,numeric,text,jsonb,jsonb),app.native_host_original_is_valid(jsonb),app.commit_native_host_input(uuid,uuid,jsonb,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
