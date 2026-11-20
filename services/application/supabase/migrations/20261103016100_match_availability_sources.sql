BEGIN;
ALTER TABLE app.match ADD COLUMN availability text NOT NULL DEFAULT 'available' CHECK(availability IN ('available','recovering'));
ALTER TABLE app.match ADD COLUMN availability_revision numeric(78,0) NOT NULL DEFAULT 0 CHECK(availability_revision>=0);
ALTER TABLE app.match ADD COLUMN incident_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.match ADD COLUMN restoration_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.match ADD CONSTRAINT match_incident_tuple CHECK((incident_receipt_id IS NULL AND restoration_receipt_id IS NULL AND availability='available') OR incident_receipt_id IS NOT NULL);
CREATE FUNCTION app.stamp_match_incident() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid; next_revision numeric; source uuid; match_revision numeric; phase text; current_game uuid; reserved_game uuid;
BEGIN
 IF NEW.incident_receipt_id IS NULL THEN RETURN NULL;END IF;
 IF TG_OP='UPDATE' AND (NEW.availability,NEW.incident_receipt_id,NEW.restoration_receipt_id) IS NOT DISTINCT FROM (OLD.availability,OLD.incident_receipt_id,OLD.restoration_receipt_id) THEN RETURN NULL;END IF;
 SELECT match_id INTO STRICT parent FROM app.game WHERE id=NEW.game_id;
 UPDATE app.match SET availability=NEW.availability,availability_revision=availability_revision+1,incident_receipt_id=NEW.incident_receipt_id,restoration_receipt_id=NEW.restoration_receipt_id WHERE id=parent RETURNING availability_revision,revision,state,current_game_id,reserved_game_id INTO next_revision,match_revision,phase,current_game,reserved_game;
 source:=CASE WHEN NEW.availability='recovering' THEN NEW.incident_receipt_id ELSE NEW.restoration_receipt_id END;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('matchId',parent,'matchAvailabilityRevision',next_revision::text,'matchRevision',match_revision::text,'matchPhase',phase,'currentGameId',current_game,'reservedGameId',reserved_game) WHERE id=source AND state='pending';
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match incident transition requires pending canonical source';END IF;
 RETURN NULL;
END $$;
CREATE TRIGGER match_incident_from_game AFTER INSERT OR UPDATE ON app.operational_state FOR EACH ROW EXECUTE FUNCTION app.stamp_match_incident();
CREATE FUNCTION app.pause_match_between_games(p_match uuid,p_expected numeric,p_availability numeric,p_incident_at timestamptz,p_kind text,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; receipt app.command_receipt; terms jsonb; fingerprint text;
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
 IF EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=p_match AND state='pending' AND deadline_at IS NOT NULL) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deadline','qualified deadline freeze policy required'));END IF;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('phase',m.state,'reservedGameId',m.reserved_game_id,'pendingDecisionIds',(SELECT coalesce(jsonb_agg(id ORDER BY id),'[]'::jsonb) FROM app.between_game_decision WHERE match_id=p_match AND state='pending'),'clockValues',app.match_clock_snapshot(p_match),'matchAvailabilityRevision',(p_availability+1)::text) WHERE id=receipt.id;
 UPDATE app.match SET availability='recovering',availability_revision=availability_revision+1,incident_receipt_id=receipt.id,restoration_receipt_id=NULL,revision=revision+1 WHERE id=p_match;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_match,'incidentId',receipt.id,'revision',(p_expected+1)::text,'availabilityRevision',(p_availability+1)::text));
END $$;
CREATE FUNCTION app.resume_match_between_games(p_match uuid,p_expected numeric,p_availability numeric,p_incident uuid,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; receipt app.command_receipt; incident app.command_receipt; terms jsonb; fingerprint text;
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
 PERFORM app.check_match_availability(p_match);
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('matchAvailabilityRevision',(p_availability+1)::text) WHERE id=receipt.id;
 UPDATE app.match SET availability='available',availability_revision=availability_revision+1,restoration_receipt_id=receipt.id,revision=revision+1 WHERE id=p_match;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_match,'revision',(p_expected+1)::text,'availabilityRevision',(p_availability+1)::text));
END $$;
REVOKE ALL ON FUNCTION app.stamp_match_incident(),app.pause_match_between_games(uuid,numeric,numeric,timestamptz,text,text,uuid),app.resume_match_between_games(uuid,numeric,numeric,uuid,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
