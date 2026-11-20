BEGIN;

CREATE FUNCTION app.check_native_commit_receipt(p_receipt uuid) RETURNS void
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
  IF prepared='null'::jsonb THEN
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

CREATE FUNCTION app.guard_native_commit_receipt() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$ BEGIN PERFORM app.check_native_commit_receipt(NEW.id);RETURN NULL;END $$;
CREATE CONSTRAINT TRIGGER native_commit_original_source AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW WHEN(NEW.scope IN('native_player_v1','native_host_input_v1')) EXECUTE FUNCTION app.guard_native_commit_receipt();

-- Legacy owner-only frames remain legacy. A game admitted to the new native
-- opening protocol cannot acquire an accepted raw host frame without its original.
CREATE FUNCTION app.guard_native_child_original() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE child app.command_receipt;parent_id uuid;
BEGIN
 SELECT * INTO child FROM app.command_receipt WHERE id=NEW.id AND scope='host_candidate_v1' AND state='accepted';
 IF child.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.command_receipt opening WHERE opening.scope='native_opening_v1'
  AND opening.state='accepted' AND opening.target_ref=child.target_ref) THEN RETURN NULL;END IF;
 SELECT id INTO parent_id FROM app.command_receipt parent WHERE parent.scope IN('native_player_v1','native_host_input_v1')
  AND parent.state='accepted' AND parent.target_ref=child.target_ref AND parent.id::text=child.request_ref
  AND parent.normalized_source->>'childReceiptId'=child.id::text;
 IF parent_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native canonical frame lacks reciprocal original';END IF;
 PERFORM app.check_native_commit_receipt(parent_id);RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER native_canonical_original_source AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW WHEN(NEW.scope='host_candidate_v1') EXECUTE FUNCTION app.guard_native_child_original();

REVOKE ALL ON FUNCTION app.check_native_commit_receipt(uuid),app.guard_native_commit_receipt(),app.guard_native_child_original()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
