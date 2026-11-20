BEGIN;

-- Observation helpers grant no credential possession: the trusted service must
-- first verify the actual controller credential against its private SecretStore.
CREATE FUNCTION app.read_native_controller_actor(p_grant uuid,p_epoch numeric,p_secret_ref text,p_match uuid) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('seatId',seat.id,'kind','ai','accountId',NULL,'grantId',g.id,
  'bindingId',b.id,'bindingEpoch',b.binding_epoch::text)
 FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id
 JOIN app.room_slot slot ON slot.id=b.room_slot_id
 JOIN app.match_seat seat ON seat.id=b.match_seat_id AND seat.id=g.match_seat_id
 JOIN app.match m ON m.id=seat.match_id AND m.room_id=slot.room_id
 JOIN app.room room ON room.id=slot.room_id JOIN app.account sponsor ON sponsor.id=seat.sponsor_id
 WHERE p_epoch IS NOT NULL AND p_epoch::text ~ '^(0|[1-9][0-9]{0,77})$' AND g.id=p_grant AND g.state='active' AND g.delivery_state='delivered' AND g.issue_receipt_id IS NOT NULL
  AND g.binding_epoch=p_epoch AND b.binding_epoch=p_epoch AND g.grant_secret_ref=p_secret_ref
  AND p_secret_ref IS NOT NULL AND b.bootstrap_receipt_id IS NOT NULL
  AND b.desired_state='running' AND b.observed_state='ready' AND b.provider_runner_ref IS NOT NULL
  AND slot.retired_at IS NULL AND slot.controller_kind='ai' AND slot.account_id IS NULL
  AND seat.controller_kind='ai' AND seat.account_id IS NULL AND slot.sponsor_id=seat.sponsor_id
  AND slot.seat_index=seat.seat_index AND m.id=p_match AND m.started_at IS NOT NULL
  AND m.accepted_release_id IS NOT NULL AND m.state IN('ready','active','between_games')
  AND room.state='started' AND app.ai_slot_ready(slot.id)
  AND(sponsor.state='active' OR app.ai_sponsor_continuing_match(b.id,sponsor.id))
$$;

CREATE FUNCTION app.lock_native_controller_actor(p_grant uuid,p_epoch numeric,p_secret_ref text,p_match uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE original_binding uuid;original_sponsor uuid;original_room uuid;actor jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native controller requires read committed retry';END IF;
 SELECT b.id,slot.sponsor_id,slot.room_id INTO original_binding,original_sponsor,original_room
 FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE g.id=p_grant;
 IF original_binding IS NULL THEN RETURN NULL;END IF;
 PERFORM id FROM app.account WHERE id=original_sponsor FOR NO KEY UPDATE;
 PERFORM id FROM app.room WHERE id=original_room FOR NO KEY UPDATE;
 PERFORM id FROM app.ai_runner_binding WHERE id=original_binding FOR NO KEY UPDATE;
 PERFORM id FROM app.controller_grant WHERE id=p_grant FOR SHARE;
 actor:=app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,p_match);
 IF actor IS NOT NULL AND actor->>'bindingId' IS DISTINCT FROM original_binding::text THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='controller lineage changed before authority fence';END IF;
 RETURN actor;
END $$;

CREATE FUNCTION app.native_account_may_continue(p_account app.account,p_match uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT coalesce((p_account).state='active' OR((p_account).state='deletion_pending' AND EXISTS(
  SELECT 1 FROM app.match m WHERE m.id=p_match AND m.state IN('ready','active','between_games')
   AND m.accepted_release_id IS NOT NULL AND m.started_at IS NOT NULL AND m.started_at<=(p_account).deletion_requested_at)),false)
$$;

-- The live permission means ordinary participant-filtered disclosure, including
-- retained historical cuts. It never implies a currently executable game basis.
CREATE FUNCTION app.native_disclosure_source(p_game uuid,p_context uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE game_row app.game;manifest app.replay_manifest;publication app.command_receipt;terms jsonb;
 full_terminal boolean:=false;prefix_terminal boolean:=false;
BEGIN
 SELECT * INTO game_row FROM app.game WHERE id=p_game;
 IF game_row.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.game_context WHERE id=p_context AND game_id=p_game) THEN RETURN NULL;END IF;
 full_terminal:=game_row.state IN('completed','no_contest') AND EXISTS(
  SELECT 1 FROM app.game_result result JOIN app.game_event event ON event.id=result.source_event_id
  WHERE result.game_id=p_game AND result.winner_seat_ids IS NOT NULL AND event.game_id=p_game
   AND event.context_id=p_context AND event.accepted_revision=game_row.accepted_revision);
 SELECT * INTO manifest FROM app.replay_manifest WHERE game_id=p_game AND context_id=p_context AND source_receipt_id IS NOT NULL;
 IF manifest.id IS NOT NULL THEN
  SELECT * INTO publication FROM app.command_receipt WHERE id=manifest.source_receipt_id;
  terms:=jsonb_build_object('version','game-replay-v1','gameId',manifest.game_id,'revision',manifest.through_accepted_revision::text,
   'initialCheckpointId',manifest.initial_checkpoint_id,'throughEventSequence',manifest.through_event_sequence::text,
   'artifactRef',manifest.artifact_ref,'visibilityIndexRef',manifest.visibility_index_ref,'schemaRevision',manifest.schema_revision,'qualificationRef',manifest.qualification_ref);
  IF publication.state IS DISTINCT FROM 'accepted' OR publication.scope IS DISTINCT FROM 'game_replay_v1'
   OR publication.principal_key IS DISTINCT FROM 'replay:'||p_game::text OR publication.target_ref IS DISTINCT FROM p_game::text
   OR publication.request_ref IS DISTINCT FROM p_game::text OR publication.normalized_source IS DISTINCT FROM terms
   OR publication.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)
   OR publication.response_payload->'data'->>'resourceId' IS DISTINCT FROM manifest.id::text
   OR manifest.through_accepted_revision IS DISTINCT FROM game_row.accepted_revision
   OR NOT EXISTS(SELECT 1 FROM app.game_checkpoint WHERE id=manifest.initial_checkpoint_id AND game_id=p_game AND context_id=p_context AND accepted_revision=0)
   OR(SELECT max(sequence) FROM app.game_event WHERE game_id=p_game AND context_id=p_context) IS DISTINCT FROM manifest.through_event_sequence
   OR(SELECT count(*)::numeric FROM app.game_event WHERE game_id=p_game AND context_id=p_context)<>manifest.through_event_sequence+1 THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native replay disclosure lacks exact publication source';END IF;
  prefix_terminal:=NOT full_terminal AND game_row.state='no_contest' AND EXISTS(
   SELECT 1 FROM app.match m JOIN app.match_result result ON result.match_id=m.id
   JOIN app.command_receipt terminal ON terminal.id=m.terminal_receipt_id
   WHERE m.id=game_row.match_id AND result.terminal_kind='unrecoverable_incident'
    AND result.terminal_receipt_id=m.terminal_receipt_id AND terminal.state='accepted'
    AND terminal.normalized_source->>'interruptedGameId'=p_game::text
    AND EXISTS(SELECT 1 FROM app.game_event WHERE game_id=p_game AND context_id=p_context
     AND sequence=manifest.through_event_sequence AND accepted_revision=manifest.through_accepted_revision));
  IF NOT full_terminal AND NOT prefix_terminal THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native publication has no accepted terminal disclosure';END IF;
 END IF;
 RETURN jsonb_build_object('live',true,'completed',full_terminal,'operationalPrefix',prefix_terminal,'replayManifestId',manifest.id);
END $$;

CREATE FUNCTION app.read_native_actor_source(p_actor jsonb,p_game uuid,p_context uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE source jsonb;opening jsonb;seat app.match_seat;disclosure jsonb;
 unavailable constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}'::jsonb;
BEGIN
 IF p_actor IS NULL OR p_game IS NULL OR p_context IS NULL THEN RETURN unavailable;END IF;
 source:=app.read_native_game_source(p_game);opening:=app.read_native_opening_source(p_game);
 IF source->>'status' IS DISTINCT FROM 'success' OR opening->>'status' IS DISTINCT FROM 'success'
  OR source->'data'->>'contextId' IS DISTINCT FROM p_context::text OR opening->'data'->>'contextId' IS DISTINCT FROM p_context::text THEN RETURN unavailable;END IF;
 SELECT * INTO seat FROM app.match_seat WHERE id=(p_actor->>'seatId')::uuid AND match_id=(source->'data'->>'matchId')::uuid;
 IF seat.id IS NULL OR seat.deck_accepted_at IS NULL OR seat.source_revision_id IS NULL OR seat.deck_snapshot_ref IS NULL
  OR seat.controller_kind IS DISTINCT FROM p_actor->>'kind'
  OR(seat.controller_kind='human' AND seat.account_id::text IS DISTINCT FROM p_actor->>'accountId') THEN RETURN unavailable;END IF;
 IF NOT app.native_u64_is_canonical(source->'data'->>'acceptedRevision') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native revision exceeds protocol';END IF;
 disclosure:=app.native_disclosure_source(p_game,p_context);
 IF disclosure IS NULL THEN RETURN unavailable;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('actor',p_actor,'source',jsonb_build_object(
  'gameId',p_game,'matchId',source->'data'->>'matchId','contextId',p_context,
  'nativeEpoch',opening->'data'->>'nativeEpoch','nativeRevision',source->'data'->>'acceptedRevision',
  'runtimeId',opening->'data'->>'runtimeId','programSourceId',opening->'data'->>'programSourceId',
  'initialRef',source->'data'->>'initialRef','stateRef',source->'data'->'checkpoint'->>'stateRef',
  'checkpointId',source->'data'->'checkpoint'->>'id','lastEventSequence',source->'data'->>'lastEventSequence',
  'gameState',source->'data'->>'gameState','matchState',source->'data'->>'matchState','availability',source->'data'->>'matchAvailability'),
  'disclosure',disclosure));
END $$;

CREATE FUNCTION app.read_native_human_access(p_subject uuid,p_session uuid,p_game uuid,p_context uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE authority jsonb;player app.account;seat_id uuid;actor jsonb;response jsonb;
 unauthorized constant jsonb:='{"status":"fail","data":{"session":"unauthorized"}}'::jsonb;
 unavailable constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}'::jsonb;
BEGIN
 authority:=app.read_auth_session_authority(p_subject,p_session);
 SELECT * INTO player FROM app.account WHERE auth_subject=p_subject AND state IN('active','deletion_pending');
 IF authority IS NULL OR player.id IS NULL OR player.sign_in_method IS DISTINCT FROM authority->>'method' THEN RETURN unauthorized;END IF;
 SELECT seat.id INTO seat_id FROM app.match_seat seat JOIN app.game game_row ON game_row.match_id=seat.match_id
  WHERE game_row.id=p_game AND seat.controller_kind='human' AND seat.account_id=player.id;
 IF seat_id IS NULL THEN RETURN unavailable;END IF;
 actor:=jsonb_build_object('seatId',seat_id,'kind','human','accountId',player.id,'grantId',NULL,'bindingId',NULL,'bindingEpoch',NULL);
 response:=app.read_native_actor_source(actor,p_game,p_context);
 -- Fresh delivery observation, not a stream-lifetime lease or held Auth lock.
 authority:=app.read_auth_session_authority(p_subject,p_session);
 IF authority IS NULL OR player.sign_in_method IS DISTINCT FROM authority->>'method'
  OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=player.id AND auth_subject=p_subject AND state IN('active','deletion_pending')) THEN RETURN unauthorized;END IF;
 RETURN response;
END $$;

CREATE FUNCTION app.read_native_controller_access(p_grant uuid,p_epoch numeric,p_secret_ref text,p_game uuid,p_context uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE match_id uuid;actor jsonb;response jsonb;
 unavailable constant jsonb:='{"status":"fail","data":{"actor":"unavailable"}}'::jsonb;
BEGIN
 SELECT game_row.match_id INTO match_id FROM app.game game_row WHERE game_row.id=p_game;
 actor:=app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,match_id);
 IF actor IS NULL THEN RETURN unavailable;END IF;
 response:=app.read_native_actor_source(actor,p_game,p_context);
 IF app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,match_id) IS DISTINCT FROM actor THEN RETURN unavailable;END IF;
 RETURN response;
END $$;

REVOKE ALL ON FUNCTION app.read_native_controller_actor(uuid,numeric,text,uuid),app.lock_native_controller_actor(uuid,numeric,text,uuid),
 app.native_account_may_continue(app.account,uuid),app.native_disclosure_source(uuid,uuid),app.read_native_actor_source(jsonb,uuid,uuid),
 app.read_native_human_access(uuid,uuid,uuid,uuid),app.read_native_controller_access(uuid,numeric,text,uuid,uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
