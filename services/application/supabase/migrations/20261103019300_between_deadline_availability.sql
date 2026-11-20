BEGIN;
CREATE OR REPLACE FUNCTION app.pause_match_between_games(p_match uuid,p_expected numeric,p_availability numeric,p_incident_at timestamptz,p_kind text,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; receipt app.command_receipt; terms jsonb; fingerprint text;before_deadlines jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='match incident requires read committed retry';END IF;
 IF p_match IS NULL OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_availability IS NULL OR p_availability::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_incident_at IS NULL OR NOT isfinite(p_incident_at) OR p_incident_at>clock_timestamp() OR p_kind IS NULL OR p_kind NOT IN ('server_process','storage') OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid match incident source';END IF;
 terms:=jsonb_build_object('version','match-incident-v1','matchId',p_match,'basis',p_expected::text,'availabilityBasis',p_availability::text,'incidentAt',to_char(p_incident_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'kind',p_kind,'proofRef',p_proof);fingerprint:=app.auth_json_hash(terms);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('match-recovery:'||p_match::text,'match_incident_v1',p_match::text,p_request::text,fingerprint,'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-recovery:'||p_match::text AND scope='match_incident_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match incident request terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='match incident source pending';END IF;RETURN receipt.response_payload;END IF;
 SELECT * INTO m FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 IF m.id IS NULL OR m.revision<>p_expected OR m.availability_revision<>p_availability OR m.availability<>'available' OR m.state NOT IN ('ready','between_games') OR m.current_game_id IS NOT NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or unavailable match phase'));END IF;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=p_match ORDER BY c.match_seat_id FOR SHARE OF c;
 IF EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=p_match AND (NOT c.paused OR c.last_accounted_at>p_incident_at)) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clock','outside inactive phase boundary'));END IF;
 PERFORM app.check_started_match_frontier(p_match);
 PERFORM d.id FROM app.between_game_decision d WHERE match_id=p_match AND state='pending' ORDER BY d.id FOR NO KEY UPDATE;
 PERFORM app.check_between_deadline(id) FROM app.between_game_decision WHERE match_id=p_match AND state='pending';
 IF EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=p_match AND state='pending' AND deadline_policy_ref IS NOT NULL AND deadline_accounted_at>p_incident_at) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='incident predates an acknowledged deadline boundary';END IF;
 before_deadlines:=app.between_deadline_snapshots(p_match);
 UPDATE app.between_game_decision SET deadline_remaining_us=greatest(0,extract(epoch FROM (deadline_at-p_incident_at))*1000000),deadline_at=NULL,deadline_revision=deadline_revision+1,deadline_source_receipt_id=receipt.id,deadline_availability_revision=p_availability+1,deadline_accounted_at=p_incident_at WHERE match_id=p_match AND state='pending' AND deadline_policy_ref IS NOT NULL;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('beforeDeadlineSnapshots',before_deadlines,'deadlineSnapshots',app.between_deadline_snapshots(p_match),'phase',m.state,'reservedGameId',m.reserved_game_id,'pendingDecisionIds',(SELECT coalesce(jsonb_agg(id ORDER BY id),'[]'::jsonb) FROM app.between_game_decision WHERE match_id=p_match AND state='pending'),'clockValues',app.match_clock_snapshot(p_match),'matchAvailabilityRevision',(p_availability+1)::text) WHERE id=receipt.id;
 UPDATE app.match SET availability='recovering',availability_revision=availability_revision+1,incident_receipt_id=receipt.id,restoration_receipt_id=NULL,revision=revision+1 WHERE id=p_match;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_match,'incidentId',receipt.id,'revision',(p_expected+1)::text,'availabilityRevision',(p_availability+1)::text));
END $$;
CREATE OR REPLACE FUNCTION app.resume_match_between_games(p_match uuid,p_expected numeric,p_availability numeric,p_incident uuid,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; receipt app.command_receipt; incident app.command_receipt; terms jsonb; fingerprint text;before_deadlines jsonb;resumed_at timestamptz;timer app.between_game_decision;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='match resume requires read committed retry';END IF;
 IF p_match IS NULL OR p_incident IS NULL OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_availability IS NULL OR p_availability::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid match resume source';END IF;
 terms:=jsonb_build_object('version','match-restoration-v1','matchId',p_match,'basis',p_expected::text,'availabilityBasis',p_availability::text,'incidentId',p_incident,'proofRef',p_proof);fingerprint:=app.auth_json_hash(terms);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('match-recovery:'||p_match::text,'match_restoration_v1',p_match::text,p_request::text,fingerprint,'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-recovery:'||p_match::text AND scope='match_restoration_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match resume request terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='match resume source pending';END IF;RETURN receipt.response_payload;END IF;
 SELECT * INTO m FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 SELECT * INTO incident FROM app.command_receipt WHERE id=p_incident;
 IF m.id IS NULL OR m.revision<>p_expected OR m.availability_revision<>p_availability OR m.availability<>'recovering' OR m.incident_receipt_id IS DISTINCT FROM p_incident OR m.state NOT IN ('ready','between_games') OR m.current_game_id IS NOT NULL OR incident.scope IS DISTINCT FROM 'match_incident_v1' OR incident.state IS DISTINCT FROM 'accepted' OR incident.normalized_source->>'phase' IS DISTINCT FROM m.state OR incident.normalized_source->>'reservedGameId' IS DISTINCT FROM m.reserved_game_id::text THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or unavailable match incident'));END IF;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=p_match ORDER BY c.match_seat_id FOR SHARE OF c;
 PERFORM app.check_match_availability(p_match);PERFORM app.check_started_match_frontier(p_match);
 PERFORM d.id FROM app.between_game_decision d WHERE match_id=p_match AND state='pending' ORDER BY d.id FOR NO KEY UPDATE;
 PERFORM app.check_between_deadline(id) FROM app.between_game_decision WHERE match_id=p_match AND state='pending';
 before_deadlines:=app.between_deadline_snapshots(p_match);resumed_at:=clock_timestamp();
 IF EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=p_match AND state='pending' AND deadline_accounted_at>resumed_at) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='resume predates acknowledged deadline boundary';END IF;
 UPDATE app.between_game_decision SET deadline_at=resumed_at+(deadline_remaining_us::text||' microseconds')::interval,deadline_remaining_us=NULL,deadline_revision=deadline_revision+1,deadline_source_receipt_id=receipt.id,deadline_availability_revision=p_availability+1,deadline_accounted_at=resumed_at WHERE match_id=p_match AND state='pending' AND deadline_policy_ref IS NOT NULL;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('beforeDeadlineSnapshots',before_deadlines,'deadlineSnapshots',app.between_deadline_snapshots(p_match),'resumedAt',to_char(resumed_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'matchAvailabilityRevision',(p_availability+1)::text) WHERE id=receipt.id;
 UPDATE app.match SET availability='available',availability_revision=availability_revision+1,restoration_receipt_id=receipt.id,revision=revision+1 WHERE id=p_match;
 FOR timer IN SELECT * FROM app.between_game_decision WHERE match_id=p_match AND state='pending' AND deadline_policy_ref IS NOT NULL ORDER BY id LOOP
 INSERT INTO app.durable_handoff(match_id,source_ref,destination,kind,state) VALUES(p_match,receipt.id::text||':'||timer.id::text,'match_lifecycle','between_deadline','pending');
 END LOOP;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_match,'revision',(p_expected+1)::text,'availabilityRevision',(p_availability+1)::text));
END $$;

-- Discovery supplies the current source generation, never an automatic sideboard
-- choice. Every delivery rechecks the common parent fence before accepting it.
CREATE FUNCTION app.read_due_between_game_deadlines(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE observed_at timestamptz;items jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='deadline discovery page must contain one to one hundred items';END IF;
 observed_at:=clock_timestamp();
 SELECT coalesce(jsonb_agg(item ORDER BY id),'[]'::jsonb) INTO items FROM (
 SELECT d.id,app.between_deadline_snapshot(d.id) AS item FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id
 WHERE d.state='pending' AND d.deadline_policy_ref IS NOT NULL AND d.deadline_at<=observed_at AND m.state='between_games' AND m.availability='available' AND d.deadline_availability_revision=m.availability_revision AND (p_after IS NULL OR d.id>p_after) ORDER BY d.id LIMIT p_limit) page;
 RETURN jsonb_build_object('observedAt',to_char(observed_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'items',items);
END $$;
REVOKE ALL ON FUNCTION app.read_due_between_game_deadlines(uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
