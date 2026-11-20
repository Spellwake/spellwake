BEGIN;
ALTER TABLE app.operational_state ADD COLUMN recovery_checkpoint_id uuid REFERENCES app.game_checkpoint(id) ON DELETE RESTRICT;
ALTER TABLE app.operational_state ADD COLUMN incident_at timestamptz CHECK(incident_at IS NULL OR isfinite(incident_at));
ALTER TABLE app.operational_state ADD COLUMN incident_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.operational_state ADD COLUMN restoration_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.operational_state ADD CONSTRAINT recovery_incident_tuple CHECK((incident_receipt_id IS NULL AND recovery_checkpoint_id IS NULL AND incident_at IS NULL) OR (incident_receipt_id IS NOT NULL AND recovery_checkpoint_id IS NOT NULL AND incident_at IS NOT NULL AND incident_ref IS NOT NULL));

-- Only server/storage incidents enter this port. Player/controller disconnect is not a pause source.
CREATE FUNCTION app.pause_game_incident(p_game uuid,p_owner text,p_epoch numeric,p_operational_revision numeric,p_incident_at timestamptz,p_kind text,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; game_row app.game; host app.operational_state; parent uuid; checkpoint app.game_checkpoint; next_clock numeric; terms jsonb; fingerprint text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='recovery requires read committed retry';END IF;
 IF p_game IS NULL OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256 OR p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_operational_revision IS NULL OR p_operational_revision::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_incident_at IS NULL OR NOT isfinite(p_incident_at) OR p_incident_at>clock_timestamp()
 OR p_kind IS NULL OR p_kind NOT IN ('server_process','storage') OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid trusted incident source';END IF;
 terms:=jsonb_build_object('version','game-incident-v1','gameId',p_game,'owner',p_owner,'epoch',p_epoch::text,'basis',p_operational_revision::text,'incidentAt',to_char(p_incident_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'kind',p_kind,'proofRef',p_proof);fingerprint:=app.auth_json_hash(terms);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('recovery:'||p_game::text,'game_incident_v1',p_game::text,p_request::text,fingerprint,'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='recovery:'||p_game::text AND scope='game_incident_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='incident request terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='incident source pending';END IF;RETURN receipt.response_payload;
 END IF;
 SELECT match_id INTO parent FROM app.game WHERE id=p_game;
 PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 SELECT * INTO game_row FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF game_row.match_id IS DISTINCT FROM parent THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='incident game parent changed before fence';END IF;
 SELECT * INTO host FROM app.operational_state WHERE game_id=p_game FOR NO KEY UPDATE;
 IF game_row.id IS NULL OR game_row.state<>'active' OR host.id IS NULL OR host.owner_instance_ref<>p_owner OR host.fence_epoch<>p_epoch OR host.revision<>p_operational_revision OR host.availability<>'available' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or unavailable'));END IF;
 SELECT * INTO checkpoint FROM app.game_checkpoint WHERE game_id=p_game AND accepted_revision=game_row.accepted_revision ORDER BY id LIMIT 1;
 IF checkpoint.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recovery requires latest durable accepted checkpoint';END IF;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
 IF EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent AND c.last_accounted_at>p_incident_at) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('incidentAt','predates durable clock boundary'));END IF;
 SELECT max(c.clock_revision)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent;
 UPDATE app.match_clock c SET remaining_ms=CASE WHEN c.paused THEN c.remaining_ms ELSE greatest(0,c.remaining_ms-floor(extract(epoch FROM (p_incident_at-c.last_accounted_at))*1000)) END,
 paused=true,last_accounted_at=p_incident_at,clock_revision=next_clock FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=parent;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('checkpointId',checkpoint.id,'acceptedRevision',game_row.accepted_revision::text,'clockRevision',next_clock::text,'clockValues',app.match_clock_snapshot(parent)) WHERE id=receipt.id;
 UPDATE app.operational_state SET availability='recovering',fence_epoch=fence_epoch+1,revision=revision+1,incident_ref=p_proof,incident_at=p_incident_at,recovery_checkpoint_id=checkpoint.id,incident_receipt_id=receipt.id,restoration_receipt_id=NULL WHERE id=host.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_game,'checkpointId',checkpoint.id,'epoch',(p_epoch+1)::text,'operationalRevision',(p_operational_revision+1)::text,'clockRevision',next_clock::text));
END $$;

-- The trusted native adapter must prove restoration of this exact retained checkpoint.
-- SQL stores that proof; it does not decode native state or create a replacement game/seed.
CREATE FUNCTION app.restore_game_checkpoint(p_game uuid,p_epoch numeric,p_operational_revision numeric,p_checkpoint uuid,p_new_owner text,p_restoration_ref text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; game_row app.game; host app.operational_state; parent uuid; checkpoint app.game_checkpoint; next_clock numeric; terms jsonb; fingerprint text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='recovery requires read committed retry';END IF;
 IF p_game IS NULL OR p_checkpoint IS NULL OR p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_operational_revision IS NULL OR p_operational_revision::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_new_owner IS NULL OR length(btrim(p_new_owner)) NOT BETWEEN 1 AND 256 OR p_restoration_ref IS NULL OR length(btrim(p_restoration_ref)) NOT BETWEEN 1 AND 2048 OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid checkpoint restoration source';END IF;
 terms:=jsonb_build_object('version','game-restoration-v1','gameId',p_game,'epoch',p_epoch::text,'basis',p_operational_revision::text,'checkpointId',p_checkpoint,'owner',p_new_owner,'restorationRef',p_restoration_ref);fingerprint:=app.auth_json_hash(terms);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('recovery:'||p_game::text,'game_restoration_v1',p_game::text,p_request::text,fingerprint,'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='recovery:'||p_game::text AND scope='game_restoration_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restoration request terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='restoration source pending';END IF;RETURN receipt.response_payload;
 END IF;
 SELECT match_id INTO parent FROM app.game WHERE id=p_game;
 PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 SELECT * INTO game_row FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF game_row.match_id IS DISTINCT FROM parent THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='restoration parent changed before fence';END IF;
 SELECT * INTO host FROM app.operational_state WHERE game_id=p_game FOR NO KEY UPDATE;
 SELECT * INTO checkpoint FROM app.game_checkpoint WHERE id=p_checkpoint AND game_id=p_game;
 IF game_row.id IS NULL OR game_row.state<>'active' OR host.id IS NULL OR host.availability<>'recovering' OR host.fence_epoch<>p_epoch OR host.revision<>p_operational_revision OR host.recovery_checkpoint_id IS DISTINCT FROM p_checkpoint OR checkpoint.accepted_revision IS DISTINCT FROM game_row.accepted_revision THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or unavailable'));END IF;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
 PERFORM app.check_game_recovery_source(p_game);
 IF EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent AND NOT c.paused) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='incident clocks must remain paused during restoration';END IF;
 SELECT max(c.clock_revision)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent;
 UPDATE app.match_clock c SET clock_revision=next_clock,last_accounted_at=clock_timestamp() FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=parent;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('incidentId',host.incident_receipt_id,'acceptedRevision',game_row.accepted_revision::text,'clockRevision',next_clock::text) WHERE id=receipt.id;
 UPDATE app.operational_state SET owner_instance_ref=p_new_owner,fence_epoch=fence_epoch+1,revision=revision+1,availability='available',restoration_receipt_id=receipt.id WHERE id=host.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_game,'checkpointId',p_checkpoint,'epoch',(p_epoch+1)::text,'operationalRevision',(p_operational_revision+1)::text,'clockRevision',next_clock::text));
END $$;
REVOKE ALL ON FUNCTION app.pause_game_incident(uuid,text,numeric,numeric,timestamptz,text,text,uuid),app.restore_game_checkpoint(uuid,numeric,numeric,uuid,text,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
