BEGIN;

CREATE OR REPLACE FUNCTION app.submit_native_player_command(p_subject uuid, p_session uuid, p_grant uuid, p_epoch numeric, p_secret_ref text, p_original jsonb, p_prepared jsonb)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
DECLARE player app.account;actor jsonb;parent app.command_receipt;child app.command_receipt;
 native_game_id uuid;native_match_id uuid;native_seat_id uuid;source jsonb;basis_state jsonb;view_receipt app.command_receipt;
 expired_choice jsonb;
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
 IF is_accept AND (p_prepared->'candidate'->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb AND p_prepared->'candidate'->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-activity-v1"'::jsonb AND p_prepared->'candidate'->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-land-play-v1"'::jsonb) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='new native player acceptance requires transition event schema';END IF;
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
   IF p_prepared->'candidate'->>'eventSchemaRevision'='native-transition-land-play-v1' THEN
    UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
   END IF;
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
   -- Use the same locked database instant as the tentative checkpoint. An
   -- expired reply rolls back its child and every resulting game mutation.
   IF p_original->'action'->>'kind'='choose' THEN
    SELECT jsonb_build_object('kind','expired-response','decisionRef',w.decision_ref,
      'expiresAt',to_char(w.expires_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
      'observedAt',h.normalized_source->>'acceptedAt','viewReceiptId',view_receipt.id)
    INTO expired_choice
    FROM app.native_response_wait_at(native_game_id,(p_original->'basis'->>'revision')::numeric) w
    JOIN app.operational_state op ON op.game_id=native_game_id AND op.availability='available'
      AND op.revision=0 AND op.fence_epoch=1
    JOIN app.command_receipt h ON h.id=child.id AND h.state='accepted'
    WHERE w.seat_id=native_seat_id AND app.financial_instant(h.normalized_source->>'acceptedAt')>=w.expires_at;
    IF FOUND THEN RAISE EXCEPTION USING ERRCODE='SW004',MESSAGE='native response interval elapsed'; END IF;
   END IF;
   checkpoint_id:=(canonical->'data'->>'resourceId')::uuid;
   normalized:=normalized||jsonb_build_object('childReceiptId',child.id,'acceptedCheckpointId',checkpoint_id);
   UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
   ack:=app.finish_account_command(parent.id,true,jsonb_build_object('requestId',p_original->>'requestId','actorSeatId',native_seat_id,
    'gameId',native_game_id,'contextId',p_original->'basis'->>'contextId','epoch',p_original->'basis'->>'epoch','revision',canonical->'data'->>'revision'));
  END IF;
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
 EXCEPTION WHEN SQLSTATE 'SW004' THEN
  IF expired_choice IS NULL THEN RAISE;END IF;
  source:=app.lock_native_game_source(native_game_id);
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
  UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','native-player-v1',
    'actorSeatId',native_seat_id,'original',p_original,'prepared',NULL,
    'openingReceiptId',source->'opening'->>'receiptId','childReceiptId',NULL,
    'acceptedCheckpointId',NULL,'rejectionSource',expired_choice) WHERE id=parent.id;
  ack:=app.finish_account_command(parent.id,false,jsonb_build_object('decision','stale'));
 WHEN SQLSTATE 'SW003' THEN
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
END $function$;

CREATE OR REPLACE FUNCTION app.check_native_commit_receipt(p_receipt uuid) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE parent app.command_receipt;child app.command_receipt;opening app.command_receipt;checkpoint app.game_checkpoint;
 old_checkpoint app.game_checkpoint;observed_checkpoint app.game_checkpoint;view_receipt app.command_receipt;
 seat app.match_seat;game_row app.game;source jsonb;original jsonb;prepared jsonb;expected jsonb;terms jsonb;
 basis jsonb;rejection jsonb;observed jsonb;actor_id uuid;is_player boolean;native_game_id uuid;native_context_id uuid;next_revision numeric;
BEGIN
 SELECT * INTO parent FROM app.command_receipt WHERE id=p_receipt AND scope IN('native_player_v1','native_host_input_v1');
 IF parent.id IS NULL THEN RETURN;END IF;
 source:=parent.normalized_source;original:=source->'original';prepared:=source->'prepared';is_player:=parent.scope='native_player_v1';
 IF parent.state='pending' OR jsonb_typeof(source) IS DISTINCT FROM 'object'
  OR(is_player AND(NOT app.native_player_original_is_valid(original) OR source->>'version' IS DISTINCT FROM 'native-player-v1'))
  OR(NOT is_player AND(NOT app.native_host_original_is_valid(original) OR source->>'version' IS DISTINCT FROM 'native-host-commit-v1')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original lacks complete terminal source';END IF;
 basis:=original->'basis';native_game_id:=(basis->>'gameId')::uuid;native_context_id:=(basis->>'contextId')::uuid;
 SELECT * INTO game_row FROM app.game WHERE id=native_game_id;
 SELECT * INTO opening FROM app.command_receipt WHERE id=(source->>'openingReceiptId')::uuid AND scope='native_opening_v1' AND state='accepted';
 IF game_row.id IS NULL OR opening.id IS NULL OR opening.target_ref IS DISTINCT FROM native_game_id::text
  OR opening.normalized_source->'opening'->>'contextId' IS DISTINCT FROM native_context_id::text
  OR opening.normalized_source->>'nativeEpoch' IS DISTINCT FROM basis->>'epoch' OR parent.target_ref IS DISTINCT FROM native_game_id::text THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original has different opening lineage';END IF;
 PERFORM app.check_native_opening_receipt(opening.id);
 expected:=jsonb_build_object('version',CASE WHEN is_player THEN 'native-player-v1' ELSE 'native-host-commit-v1' END,
  'original',original,'prepared',prepared,'openingReceiptId',opening.id,'childReceiptId',source->'childReceiptId','acceptedCheckpointId',source->'acceptedCheckpointId');
 IF is_player THEN
  IF (parent.account_id IS NULL)=(parent.controller_grant_id IS NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original requires one retained actor owner';END IF;
  IF NOT app.native_uuid_is_canonical(source->>'actorSeatId') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original actor absent';END IF;
  actor_id:=(source->>'actorSeatId')::uuid;
  SELECT * INTO seat FROM app.match_seat WHERE id=actor_id AND match_id=game_row.match_id;
  IF seat.id IS NULL OR seat.deck_accepted_at IS NULL OR parent.principal_key IS DISTINCT FROM 'native-seat:'||actor_id::text
   OR parent.request_ref IS DISTINCT FROM original->>'requestId'
   OR parent.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('actorSeatId',actor_id,'original',original))
   OR NOT((seat.controller_kind='human' AND parent.account_id=seat.account_id AND parent.controller_grant_id IS NULL)
    OR(seat.controller_kind='ai' AND parent.account_id IS NULL AND parent.controller_grant_id IS NOT NULL
     AND EXISTS(SELECT 1 FROM app.controller_grant WHERE id=parent.controller_grant_id AND match_seat_id=seat.id))) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original has different retained actor';END IF;
  expected:=expected||jsonb_build_object('actorSeatId',actor_id);
 ELSE
  IF parent.principal_key IS DISTINCT FROM 'native-system:'||native_game_id::text OR NOT app.native_uuid_is_canonical(parent.request_ref)
   OR parent.account_id IS NOT NULL OR parent.controller_grant_id IS NOT NULL OR parent.payload_hash IS DISTINCT FROM app.auth_json_hash(original)
   OR original->>'runtimeId' IS DISTINCT FROM opening.normalized_source->>'runtimeId'
   OR original->>'programSourceId' IS DISTINCT FROM opening.normalized_source->>'programSourceId' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native system original has different retained source';END IF;
 END IF;
 IF parent.state='accepted' THEN
  IF jsonb_typeof(prepared) IS DISTINCT FROM 'object'
   OR NOT app.native_uuid_is_canonical(source->>'childReceiptId') OR NOT app.native_uuid_is_canonical(source->>'acceptedCheckpointId') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native original lacks canonical frame';END IF;
  IF is_player THEN
   IF NOT app.native_player_prepared_is_valid(prepared,original,actor_id) OR prepared->>'version' IS DISTINCT FROM 'native-player-accept-v1'
    OR prepared->>'runtimeId' IS DISTINCT FROM opening.normalized_source->>'runtimeId'
    OR prepared->>'programSourceId' IS DISTINCT FROM opening.normalized_source->>'programSourceId' THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native original preparation differs';END IF;
  ELSE
   IF EXISTS(SELECT 1 FROM jsonb_object_keys(prepared) k WHERE k NOT IN('owner','fenceEpoch','candidate'))
    OR jsonb_typeof(prepared->'owner') IS DISTINCT FROM 'string' OR length(btrim(prepared->>'owner')) NOT BETWEEN 1 AND 256
    OR jsonb_typeof(prepared->'fenceEpoch') IS DISTINCT FROM 'string' OR prepared->>'fenceEpoch' !~ '^(0|[1-9][0-9]{0,77})$'
    OR NOT app.native_candidate_refs_are_valid(prepared->'candidate',native_game_id,native_context_id) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native system preparation differs';END IF;
  END IF;
  SELECT * INTO child FROM app.command_receipt WHERE id=(source->>'childReceiptId')::uuid;
  SELECT * INTO checkpoint FROM app.game_checkpoint WHERE id=(source->>'acceptedCheckpointId')::uuid;
  next_revision:=(basis->>'revision')::numeric+1;
  terms:=jsonb_build_object('owner',prepared->>'owner','epoch',prepared->>'fenceEpoch','basis',basis->>'revision','candidate',prepared->'candidate');
  IF NOT app.native_u64_is_canonical(next_revision::text) OR child.id IS NULL OR checkpoint.id IS NULL
   OR child.scope<>'host_candidate_v1' OR child.state<>'accepted' OR child.principal_key IS DISTINCT FROM 'host:'||native_game_id::text
   OR child.target_ref IS DISTINCT FROM native_game_id::text OR child.request_ref IS DISTINCT FROM parent.id::text
   OR child.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)
   OR child.normalized_source->>'version' IS DISTINCT FROM 'host-clock-transition-v1'
   OR child.normalized_source->>'basisRevision' IS DISTINCT FROM basis->>'revision'
   OR child.normalized_source->>'acceptedRevision' IS DISTINCT FROM next_revision::text
   OR child.normalized_source->'candidate' IS DISTINCT FROM prepared->'candidate'
   OR checkpoint.game_id IS DISTINCT FROM native_game_id OR checkpoint.context_id IS DISTINCT FROM native_context_id
   OR checkpoint.accepted_revision IS DISTINCT FROM next_revision OR checkpoint.source_receipt_id IS DISTINCT FROM child.id
   OR checkpoint.private_state_ref IS DISTINCT FROM prepared->'candidate'->>'stateRef'
   OR checkpoint.facts_ref IS DISTINCT FROM prepared->'candidate'->>'factsRef'
   OR child.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',checkpoint.id,'revision',next_revision::text)) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original and canonical frame are not reciprocal';END IF;
  PERFORM app.check_native_creator_frame(child.id);
  IF is_player THEN
   SELECT * INTO view_receipt FROM app.command_receipt WHERE id=(prepared->>'viewReceiptId')::uuid AND scope='native_view_v1' AND state='accepted';
   IF view_receipt.id IS NULL OR view_receipt.target_ref IS DISTINCT FROM native_game_id::text
    OR view_receipt.normalized_source->'basis' IS DISTINCT FROM basis OR view_receipt.normalized_source->>'viewerSeatId' IS DISTINCT FROM actor_id::text
    OR view_receipt.normalized_source->>'disclosure' IS DISTINCT FROM 'live'
    OR parent.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('requestId',original->>'requestId',
     'actorSeatId',actor_id,'gameId',native_game_id,'contextId',native_context_id,'epoch',basis->>'epoch','revision',next_revision::text)) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native player acknowledgment or issued View differs';END IF;
   PERFORM app.check_native_view_receipt(view_receipt.id);
  ELSIF parent.response_payload IS DISTINCT FROM child.response_payload THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native system acknowledgment differs';END IF;
 ELSE
  IF NOT is_player OR parent.state<>'rejected' OR source->'childReceiptId' IS DISTINCT FROM 'null'::jsonb
   OR source->'acceptedCheckpointId' IS DISTINCT FROM 'null'::jsonb OR NOT app.native_rejection_is_valid(parent.response_payload->'data')
   OR EXISTS(SELECT 1 FROM app.command_receipt c WHERE c.scope='host_candidate_v1' AND c.principal_key='host:'||native_game_id::text AND c.request_ref=parent.id::text) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native rejection has a canonical side effect or invalid response';END IF;
  IF prepared='null'::jsonb AND source->'rejectionSource'->>'kind'='expired-response' THEN
   rejection:=source->'rejectionSource';
   SELECT * INTO view_receipt FROM app.command_receipt
    WHERE id=(rejection->>'viewReceiptId')::uuid AND scope='native_view_v1' AND state='accepted';
   IF original->'action'->>'kind' IS DISTINCT FROM 'choose'
    OR view_receipt.id IS NULL OR view_receipt.target_ref IS DISTINCT FROM native_game_id::text
    OR view_receipt.normalized_source->'basis' IS DISTINCT FROM basis
    OR view_receipt.normalized_source->>'viewerSeatId' IS DISTINCT FROM actor_id::text
    OR view_receipt.normalized_source->>'disclosure' IS DISTINCT FROM 'live'
    OR parent.response_payload IS DISTINCT FROM '{"status":"fail","data":{"decision":"stale"}}'::jsonb
    OR NOT EXISTS(SELECT 1 FROM app.native_response_wait_at(native_game_id,(basis->>'revision')::numeric) w
       WHERE w.seat_id=actor_id AND app.financial_instant(rejection->>'observedAt')>=w.expires_at
         AND rejection=jsonb_build_object('kind','expired-response','decisionRef',w.decision_ref,
           'expiresAt',to_char(w.expires_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
           'observedAt',to_char(app.financial_instant(rejection->>'observedAt') AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
           'viewReceiptId',view_receipt.id)) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='expired response lacks the original chooser, View and elapsed deadline';
   END IF;
   PERFORM app.check_native_view_receipt(view_receipt.id);
   expected:=expected||jsonb_build_object('rejectionSource',rejection);
  ELSIF prepared='null'::jsonb THEN
   rejection:=source->'rejectionSource';observed:=rejection->'observedBasis';
   IF jsonb_typeof(rejection) IS DISTINCT FROM 'object' OR rejection->>'kind' IS DISTINCT FROM 'stale-basis'
    OR NOT app.native_basis_is_valid(observed) OR observed->>'gameId' IS DISTINCT FROM basis->>'gameId'
    OR observed->>'contextId' IS DISTINCT FROM basis->>'contextId' OR observed->>'epoch' IS DISTINCT FROM basis->>'epoch'
    OR jsonb_typeof(rejection->'terminal') IS DISTINCT FROM 'boolean'
    OR NOT app.native_uuid_is_canonical(rejection->>'originalCheckpointId') OR NOT app.native_uuid_is_canonical(rejection->>'observedCheckpointId')
    OR parent.response_payload IS DISTINCT FROM jsonb_build_object('status','fail','data',jsonb_build_object('basis','stale')) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native stale rejection lacks exact observed source';END IF;
   SELECT * INTO old_checkpoint FROM app.game_checkpoint cp WHERE cp.id=(rejection->>'originalCheckpointId')::uuid AND cp.game_id=native_game_id
    AND cp.context_id=native_context_id AND accepted_revision=(basis->>'revision')::numeric;
   SELECT * INTO observed_checkpoint FROM app.game_checkpoint cp WHERE cp.id=(rejection->>'observedCheckpointId')::uuid AND cp.game_id=native_game_id
    AND cp.context_id=native_context_id AND accepted_revision=(observed->>'revision')::numeric;
   IF old_checkpoint.id IS NULL OR observed_checkpoint.id IS NULL OR old_checkpoint.accepted_revision>observed_checkpoint.accepted_revision
    OR(old_checkpoint.id=observed_checkpoint.id AND rejection->'terminal'<>'true'::jsonb)
    OR(rejection->'terminal'='true'::jsonb AND game_row.state='active')
    OR rejection IS DISTINCT FROM jsonb_build_object('kind','stale-basis','originalCheckpointId',old_checkpoint.id,
     'observedCheckpointId',observed_checkpoint.id,'observedBasis',observed,'terminal',rejection->'terminal') THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native stale rejection does not prove accepted history';END IF;
   PERFORM app.check_native_creator_frame(old_checkpoint.source_receipt_id);
   PERFORM app.check_native_creator_frame(observed_checkpoint.source_receipt_id);
   expected:=expected||jsonb_build_object('rejectionSource',rejection);
  ELSE
   IF NOT app.native_player_prepared_is_valid(prepared,original,actor_id) OR prepared->>'version' IS DISTINCT FROM 'native-player-reject-v1'
    OR prepared->>'runtimeId' IS DISTINCT FROM opening.normalized_source->>'runtimeId' OR prepared->>'programSourceId' IS DISTINCT FROM opening.normalized_source->>'programSourceId'
    OR parent.response_payload IS DISTINCT FROM jsonb_build_object('status','fail','data',prepared->'errors') THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native rejection differs from native validation source';END IF;
   SELECT * INTO view_receipt FROM app.command_receipt WHERE id=(prepared->>'viewReceiptId')::uuid AND scope='native_view_v1' AND state='accepted';
   IF view_receipt.id IS NULL OR view_receipt.target_ref IS DISTINCT FROM native_game_id::text OR view_receipt.normalized_source->'basis' IS DISTINCT FROM basis
    OR view_receipt.normalized_source->>'viewerSeatId' IS DISTINCT FROM actor_id::text OR view_receipt.normalized_source->>'disclosure' IS DISTINCT FROM 'live' THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native rejection has different issued View';END IF;
   PERFORM app.check_native_view_receipt(view_receipt.id);
  END IF;
 END IF;
 IF source IS DISTINCT FROM expected THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native terminal source has unrecognized fields';END IF;
END $$;

COMMIT;
