BEGIN;
ALTER TABLE app.replay_manifest ADD COLUMN initial_checkpoint_id uuid REFERENCES app.game_checkpoint(id) ON DELETE RESTRICT;
ALTER TABLE app.replay_manifest ADD COLUMN through_event_sequence numeric(78,0) CHECK(through_event_sequence>=0);
ALTER TABLE app.replay_manifest ADD COLUMN through_accepted_revision numeric(78,0) CHECK(through_accepted_revision>=0);
ALTER TABLE app.replay_manifest ADD COLUMN source_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.replay_manifest ADD COLUMN qualification_ref text;
ALTER TABLE app.replay_manifest ADD CONSTRAINT replay_publication_tuple CHECK(
 (initial_checkpoint_id IS NULL AND through_event_sequence IS NULL AND through_accepted_revision IS NULL AND source_receipt_id IS NULL AND qualification_ref IS NULL)
 OR(initial_checkpoint_id IS NOT NULL AND through_event_sequence IS NOT NULL AND through_accepted_revision IS NOT NULL AND source_receipt_id IS NOT NULL AND qualification_ref IS NOT NULL AND length(btrim(qualification_ref)) BETWEEN 1 AND 2048));
CREATE UNIQUE INDEX replay_publication_source ON app.replay_manifest(source_receipt_id) WHERE source_receipt_id IS NOT NULL;
CREATE FUNCTION app.publish_game_replay(p_game uuid,p_revision numeric,p_checkpoint uuid,p_through numeric,p_artifact text,p_visibility text,p_schema text,p_qualification text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; game_row app.game; checkpoint app.game_checkpoint; source_result app.game_result; parent uuid; manifest_id uuid; terms jsonb; fingerprint text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='replay publication requires read committed retry';END IF;
 IF p_game IS NULL OR p_checkpoint IS NULL OR p_revision IS NULL OR p_revision::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_through IS NULL OR p_through::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_artifact IS NULL OR length(btrim(p_artifact)) NOT BETWEEN 1 AND 2048 OR p_visibility IS NULL OR length(btrim(p_visibility)) NOT BETWEEN 1 AND 2048 OR p_schema IS NULL OR length(btrim(p_schema)) NOT BETWEEN 1 AND 2048 OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid replay source envelope';END IF;
 terms:=jsonb_build_object('version','game-replay-v1','gameId',p_game,'revision',p_revision::text,'initialCheckpointId',p_checkpoint,'throughEventSequence',p_through::text,'artifactRef',p_artifact,'visibilityIndexRef',p_visibility,'schemaRevision',p_schema,'qualificationRef',p_qualification);fingerprint:=app.auth_json_hash(terms);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('replay:'||p_game::text,'game_replay_v1',p_game::text,p_game::text,fingerprint,'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='replay:'||p_game::text AND scope='game_replay_v1' AND request_ref=p_game::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='published replay source cannot be replaced';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='replay publication pending';END IF;RETURN receipt.response_payload;
 END IF;
 SELECT match_id INTO parent FROM app.game WHERE id=p_game;
 PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 SELECT * INTO game_row FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF game_row.match_id IS DISTINCT FROM parent THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='replay game parent changed before source fence';END IF;
 SELECT * INTO checkpoint FROM app.game_checkpoint WHERE id=p_checkpoint;
 SELECT * INTO source_result FROM app.game_result WHERE game_id=p_game;
 IF game_row.id IS NULL OR game_row.state NOT IN ('completed','no_contest') OR game_row.accepted_revision<>p_revision OR source_result.id IS NULL OR source_result.winner_seat_ids IS NULL
 OR checkpoint.game_id IS DISTINCT FROM p_game OR checkpoint.accepted_revision IS DISTINCT FROM 0::numeric
 OR EXISTS(SELECT 1 FROM app.replay_manifest WHERE game_id=p_game) OR (SELECT max(sequence) FROM app.game_event WHERE game_id=p_game) IS DISTINCT FROM p_through
 OR (SELECT count(*)::numeric FROM app.game_event WHERE game_id=p_game)<>p_through+1
 OR NOT EXISTS(SELECT 1 FROM app.game_event WHERE id=source_result.source_event_id AND game_id=p_game AND sequence=p_through AND context_id=checkpoint.context_id AND accepted_revision=p_revision)
 OR EXISTS(SELECT 1 FROM app.game_event WHERE game_id=p_game AND (context_id<>checkpoint.context_id OR accepted_revision>p_revision)) THEN
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal game, original checkpoint and complete accepted frontier required';END IF;
 INSERT INTO app.replay_manifest(game_id,context_id,artifact_ref,visibility_index_ref,schema_revision,initial_checkpoint_id,through_event_sequence,through_accepted_revision,source_receipt_id,qualification_ref)
 VALUES(p_game,checkpoint.context_id,p_artifact,p_visibility,p_schema,p_checkpoint,p_through,p_revision,receipt.id,p_qualification) RETURNING id INTO manifest_id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',manifest_id,'gameId',p_game,'throughSequence',p_through::text,'revision',p_revision::text));
END $$;
CREATE FUNCTION app.guard_replay_publication() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE manifest app.replay_manifest; receipt app.command_receipt; checkpoint app.game_checkpoint; terms jsonb; game_row app.game;
BEGIN
 SELECT * INTO manifest FROM app.replay_manifest WHERE id=NEW.id;
 IF manifest.source_receipt_id IS NULL THEN RETURN NULL;END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE id=manifest.source_receipt_id;
 SELECT * INTO checkpoint FROM app.game_checkpoint WHERE id=manifest.initial_checkpoint_id;
 SELECT * INTO game_row FROM app.game WHERE id=manifest.game_id;
 terms:=jsonb_build_object('version','game-replay-v1','gameId',manifest.game_id,'revision',manifest.through_accepted_revision::text,'initialCheckpointId',manifest.initial_checkpoint_id,'throughEventSequence',manifest.through_event_sequence::text,'artifactRef',manifest.artifact_ref,'visibilityIndexRef',manifest.visibility_index_ref,'schemaRevision',manifest.schema_revision,'qualificationRef',manifest.qualification_ref);
 IF receipt.state IS DISTINCT FROM 'accepted' OR receipt.scope IS DISTINCT FROM 'game_replay_v1' OR receipt.principal_key IS DISTINCT FROM 'replay:'||manifest.game_id::text OR receipt.target_ref IS DISTINCT FROM manifest.game_id::text OR receipt.request_ref IS DISTINCT FROM manifest.game_id::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(terms) OR receipt.normalized_source IS DISTINCT FROM terms OR receipt.response_payload->'data'->>'resourceId' IS DISTINCT FROM manifest.id::text
 OR checkpoint.game_id IS DISTINCT FROM manifest.game_id OR checkpoint.context_id IS DISTINCT FROM manifest.context_id OR checkpoint.accepted_revision IS DISTINCT FROM 0::numeric
 OR game_row.state NOT IN ('completed','no_contest') OR game_row.accepted_revision IS DISTINCT FROM manifest.through_accepted_revision OR (SELECT count(*)::numeric FROM app.game_event WHERE game_id=manifest.game_id)<>manifest.through_event_sequence+1
 OR NOT EXISTS(SELECT 1 FROM app.game_result result JOIN app.game_event event ON event.id=result.source_event_id WHERE result.game_id=manifest.game_id AND result.winner_seat_ids IS NOT NULL AND event.accepted_revision=manifest.through_accepted_revision AND event.sequence=manifest.through_event_sequence AND event.context_id=manifest.context_id)
 OR EXISTS(SELECT 1 FROM app.game_event WHERE game_id=manifest.game_id AND (context_id<>manifest.context_id OR sequence>manifest.through_event_sequence OR accepted_revision>manifest.through_accepted_revision)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='replay publication lacks exact accepted source/frontier';END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER replay_publication_source AFTER INSERT ON app.replay_manifest DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_replay_publication();
REVOKE ALL ON FUNCTION app.publish_game_replay(uuid,numeric,uuid,numeric,text,text,text,text),app.guard_replay_publication() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
