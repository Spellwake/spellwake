BEGIN;
-- Exact checkpoint restoration resumes only already-proven preincident priority.
CREATE OR REPLACE FUNCTION app.pause_game_incident(p_game uuid,p_owner text,p_epoch numeric,p_operational_revision numeric,p_incident_at timestamptz,p_kind text,p_proof text,p_request uuid) RETURNS jsonb
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
 PERFORM app.check_selected_clock_eligibility(c.id) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent AND c.policy_ref='bo3-overall-bank-v1';
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('beforeClocks',app.match_clock_snapshot(parent),'clockSources',(SELECT coalesce(jsonb_agg(jsonb_build_object('seatId',c.match_seat_id,'receiptId',c.eligibility_receipt_id) ORDER BY c.match_seat_id),'[]'::jsonb) FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=parent AND c.policy_ref='bo3-overall-bank-v1'),'eligibilitySources',(SELECT coalesce(jsonb_agg(jsonb_build_object('seatId',c.match_seat_id,'receiptId',c.eligibility_receipt_id) ORDER BY c.match_seat_id),'[]'::jsonb) FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=parent AND c.policy_ref='bo3-overall-bank-v1' AND NOT c.paused)) WHERE id=receipt.id;
 SELECT max(c.clock_revision)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=parent;
 UPDATE app.match_clock c SET remaining_ms=CASE WHEN c.paused THEN c.remaining_ms ELSE greatest(0,c.remaining_ms-CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN app.clock_elapsed_ms(c.last_accounted_at,p_incident_at) ELSE floor(extract(epoch FROM (p_incident_at-c.last_accounted_at))*1000) END) END,
 paused=true,last_accounted_at=p_incident_at,clock_revision=next_clock,eligibility_receipt_id=CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE c.eligibility_receipt_id END FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=parent;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('checkpointId',checkpoint.id,'acceptedRevision',game_row.accepted_revision::text,'clockRevision',next_clock::text,'clockValues',app.match_clock_snapshot(parent)) WHERE id=receipt.id;
 UPDATE app.operational_state SET availability='recovering',fence_epoch=fence_epoch+1,revision=revision+1,incident_ref=p_proof,incident_at=p_incident_at,recovery_checkpoint_id=checkpoint.id,incident_receipt_id=receipt.id,restoration_receipt_id=NULL WHERE id=host.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_game,'checkpointId',checkpoint.id,'epoch',(p_epoch+1)::text,'operationalRevision',(p_operational_revision+1)::text,'clockRevision',next_clock::text));
END $$;
CREATE OR REPLACE FUNCTION app.restore_game_checkpoint(p_game uuid,p_epoch numeric,p_operational_revision numeric,p_checkpoint uuid,p_new_owner text,p_restoration_ref text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; game_row app.game; host app.operational_state; parent uuid; checkpoint app.game_checkpoint; next_clock numeric; terms jsonb; fingerprint text; resumed_at timestamptz; incident app.command_receipt;
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
 SELECT * INTO incident FROM app.command_receipt WHERE id=host.incident_receipt_id;
 resumed_at:=clock_timestamp();
 IF EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=parent AND c.last_accounted_at>resumed_at) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='restoration clock moved before locked incident boundary';END IF;
 UPDATE app.match_clock c SET paused=CASE WHEN c.policy_ref='bo3-overall-bank-v1' AND c.remaining_ms>0 AND EXISTS(SELECT 1 FROM jsonb_array_elements(incident.normalized_source->'beforeClocks') value WHERE value->>'seatId'=c.match_seat_id::text AND value->'paused'='false'::jsonb) THEN false ELSE true END,clock_revision=next_clock,last_accounted_at=resumed_at,eligibility_receipt_id=CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE c.eligibility_receipt_id END FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=parent;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('incidentId',host.incident_receipt_id,'acceptedRevision',game_row.accepted_revision::text,'clockRevision',next_clock::text,'matchId',parent,'resumedAt',to_char(resumed_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'clockValues',app.match_clock_snapshot(parent)) WHERE id=receipt.id;
 UPDATE app.operational_state SET owner_instance_ref=p_new_owner,fence_epoch=fence_epoch+1,revision=revision+1,availability='available',restoration_receipt_id=receipt.id WHERE id=host.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_game,'checkpointId',p_checkpoint,'epoch',(p_epoch+1)::text,'operationalRevision',(p_operational_revision+1)::text,'clockRevision',next_clock::text));
END $$;
CREATE OR REPLACE FUNCTION app.check_selected_clock_eligibility(p_clock uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE c app.match_clock; seat app.match_seat; source app.command_receipt; requested jsonb; current_clock jsonb; values_key text; incident app.command_receipt; prior app.command_receipt; prior_clock jsonb; prior_key text;
BEGIN
 SELECT * INTO c FROM app.match_clock WHERE id=p_clock;
 IF c.id IS NULL OR c.policy_ref<>'bo3-overall-bank-v1' THEN RETURN;END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='selected clock source checks require read committed retry';END IF;
 SELECT * INTO seat FROM app.match_seat WHERE id=c.match_seat_id;
 IF c.clock_revision=0 AND c.paused AND c.remaining_ms=1800000 AND c.eligibility_receipt_id IS NULL THEN RETURN;END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=c.eligibility_receipt_id;
 current_clock:=jsonb_build_object('seatId',seat.id,'remainingMs',c.remaining_ms::text,'clockRevision',c.clock_revision::text,'paused',c.paused,'lastAccountedAt',to_char(c.last_accounted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'));
 values_key:=CASE WHEN source.scope='host_candidate_v1' THEN 'after' WHEN source.scope='match_unrecoverable_v1' THEN 'clocks' ELSE 'clockValues' END;
 IF source.id IS NULL OR source.state<>'accepted' OR source.scope NOT IN('host_candidate_v1','game_opening_v1','game_incident_v1','game_restoration_v1','match_unrecoverable_v1') OR source.normalized_source->>'matchId' IS DISTINCT FROM seat.match_id::text OR NOT coalesce(source.normalized_source->values_key @> jsonb_build_array(current_clock),false) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='BO3 clock requires its exact accepted transition source';END IF;
 IF NOT c.paused THEN
 IF NOT EXISTS(SELECT 1 FROM app.game g JOIN app.match m ON m.id=g.match_id JOIN app.operational_state host ON host.game_id=g.id JOIN app.game_checkpoint checkpoint ON checkpoint.game_id=g.id AND checkpoint.id::text=CASE WHEN source.scope='game_restoration_v1' THEN source.normalized_source->>'checkpointId' ELSE source.response_payload->'data'->>'resourceId' END WHERE g.id::text=source.normalized_source->>'gameId' AND g.match_id=seat.match_id AND m.current_game_id=g.id AND m.state='active' AND m.availability='available' AND g.state='active' AND g.accepted_revision::text=source.normalized_source->>'acceptedRevision' AND checkpoint.accepted_revision=g.accepted_revision AND host.availability='available') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='eligible bank source must retain its actual active game frontier';END IF;
 IF source.scope='game_restoration_v1' THEN
 SELECT * INTO incident FROM app.command_receipt WHERE id::text=source.normalized_source->>'incidentId';
 SELECT value INTO prior_clock FROM jsonb_array_elements(incident.normalized_source->'beforeClocks') value WHERE value->>'seatId'=seat.id::text;
 SELECT * INTO prior FROM app.command_receipt WHERE id::text=(SELECT value->>'receiptId' FROM jsonb_array_elements(incident.normalized_source->'eligibilitySources') value WHERE value->>'seatId'=seat.id::text);
 prior_key:=CASE WHEN prior.scope='host_candidate_v1' THEN 'after' ELSE 'clockValues' END;
 IF incident.id IS NULL OR incident.state<>'accepted' OR incident.scope<>'game_incident_v1' OR incident.target_ref IS DISTINCT FROM source.target_ref OR prior.id IS NULL OR prior.state<>'accepted' OR prior.scope NOT IN('host_candidate_v1','game_restoration_v1') OR prior.target_ref IS DISTINCT FROM source.target_ref OR prior_clock IS NULL OR prior_clock->'paused' IS DISTINCT FROM 'false'::jsonb OR NOT coalesce(prior.normalized_source->prior_key @> jsonb_build_array(prior_clock),false) OR (prior_clock->>'clockRevision')::numeric>=c.clock_revision OR (prior_clock->>'remainingMs')::numeric<c.remaining_ms OR source.normalized_source->>'resumedAt' IS DISTINCT FROM current_clock->>'lastAccountedAt' OR (source.normalized_source->>'resumedAt')::timestamptz<(incident.normalized_source->>'incidentAt')::timestamptz OR (source.normalized_source->>'resumedAt')::timestamptz<(prior_clock->>'lastAccountedAt')::timestamptz THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restored eligibility must continue exact preincident qualified clock';END IF;
 ELSE
 SELECT value INTO requested FROM jsonb_array_elements(source.normalized_source->'requestedClocks') WHERE value->>'seatId'=seat.id::text;
 IF source.scope<>'host_candidate_v1' OR source.normalized_source->>'version' IS DISTINCT FROM 'host-clock-transition-v1' OR source.target_ref IS DISTINCT FROM source.normalized_source->>'gameId' OR source.principal_key IS DISTINCT FROM 'host:'||(source.normalized_source->>'gameId') OR source.normalized_source->>'acceptedAt' IS DISTINCT FROM current_clock->>'lastAccountedAt'
 OR requested IS NULL OR requested->'paused' IS DISTINCT FROM 'false'::jsonb OR requested->'eligibility'->>'reason' IS DISTINCT FROM 'native_priority' OR requested->'eligibility'->>'acceptedRevision' IS DISTINCT FROM source.normalized_source->>'basisRevision' OR jsonb_typeof(requested->'eligibility'->'proofRef') IS DISTINCT FROM 'string' OR length(btrim(requested->'eligibility'->>'proofRef')) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='eligible BO3 bank requires its exact accepted native-priority source';END IF;
 END IF;
 END IF;
END $$;
COMMIT;
