BEGIN;
ALTER TABLE app.between_game_decision ADD COLUMN opened_at timestamptz NOT NULL DEFAULT clock_timestamp() CHECK(isfinite(opened_at));
ALTER TABLE app.between_game_decision ADD COLUMN deadline_policy_ref text;
ALTER TABLE app.between_game_decision ADD COLUMN deadline_revision numeric(78,0) NOT NULL DEFAULT 0 CHECK(deadline_revision::text ~ '^(0|[1-9][0-9]{0,77})$');
ALTER TABLE app.between_game_decision ADD COLUMN deadline_origin_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.between_game_decision ADD COLUMN deadline_source_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.between_game_decision ADD COLUMN deadline_availability_revision numeric(78,0) CHECK(deadline_availability_revision IS NULL OR deadline_availability_revision::text ~ '^(0|[1-9][0-9]{0,77})$');
ALTER TABLE app.between_game_decision ADD COLUMN deadline_remaining_us numeric(78,0) CHECK(deadline_remaining_us IS NULL OR deadline_remaining_us::text ~ '^(0|[1-9][0-9]{0,77})$');
ALTER TABLE app.between_game_decision ADD COLUMN deadline_accounted_at timestamptz CHECK(deadline_accounted_at IS NULL OR isfinite(deadline_accounted_at));
ALTER TABLE app.between_game_decision ADD CONSTRAINT between_deadline_tuple CHECK(
 (deadline_policy_ref IS NULL AND deadline_revision=0 AND deadline_origin_receipt_id IS NULL AND deadline_source_receipt_id IS NULL AND deadline_availability_revision IS NULL AND deadline_remaining_us IS NULL AND deadline_accounted_at IS NULL AND deadline_at IS NULL)
 OR (deadline_policy_ref IS NOT NULL AND length(btrim(deadline_policy_ref)) BETWEEN 1 AND 2048 AND deadline_revision>0 AND deadline_origin_receipt_id IS NOT NULL AND deadline_source_receipt_id IS NOT NULL AND deadline_availability_revision IS NOT NULL AND deadline_accounted_at IS NOT NULL AND ((deadline_at IS NOT NULL AND deadline_remaining_us IS NULL) OR (deadline_at IS NULL AND deadline_remaining_us IS NOT NULL))));
CREATE FUNCTION app.guard_between_deadline_history() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
 IF NEW.deadline_revision<>0 OR NEW.deadline_policy_ref IS NOT NULL OR NEW.deadline_origin_receipt_id IS NOT NULL OR NEW.deadline_source_receipt_id IS NOT NULL OR NEW.deadline_at IS NOT NULL OR NEW.deadline_remaining_us IS NOT NULL OR NEW.deadline_accounted_at IS NOT NULL OR NEW.deadline_availability_revision IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new between-game decisions start with inactive deadlines';END IF;RETURN NEW;END IF;
 IF NEW.opened_at IS DISTINCT FROM OLD.opened_at OR OLD.deadline_policy_ref IS NOT NULL AND (NEW.deadline_policy_ref,NEW.deadline_origin_receipt_id) IS DISTINCT FROM (OLD.deadline_policy_ref,OLD.deadline_origin_receipt_id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game deadline origin and opening time are immutable';END IF;
 IF (NEW.deadline_at,NEW.deadline_policy_ref,NEW.deadline_source_receipt_id,NEW.deadline_availability_revision,NEW.deadline_remaining_us,NEW.deadline_accounted_at) IS DISTINCT FROM (OLD.deadline_at,OLD.deadline_policy_ref,OLD.deadline_source_receipt_id,OLD.deadline_availability_revision,OLD.deadline_remaining_us,OLD.deadline_accounted_at) THEN
 IF OLD.state<>'pending' OR NEW.deadline_revision<>OLD.deadline_revision+1 OR NEW.deadline_accounted_at<OLD.deadline_accounted_at THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pending deadline changes require one monotone source revision';END IF;
 ELSIF NEW.deadline_revision<>OLD.deadline_revision THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline revision cannot change without its source metadata';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER between_deadline_history BEFORE INSERT OR UPDATE ON app.between_game_decision FOR EACH ROW EXECUTE FUNCTION app.guard_between_deadline_history();
CREATE FUNCTION app.between_deadline_snapshot(p_decision uuid) RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('version','between-deadline-snapshot-v1','decisionId',id,'matchId',match_id,'seatId',seat_id,'afterGameId',after_game_id,'openedAt',to_char(opened_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'basisRevision',basis_revision::text,'clockRevision',clock_revision::text,'policyRef',deadline_policy_ref,'revision',deadline_revision::text,'originReceiptId',deadline_origin_receipt_id,'sourceReceiptId',deadline_source_receipt_id,'availabilityRevision',deadline_availability_revision::text,'accountedAt',CASE WHEN deadline_accounted_at IS NOT NULL THEN to_char(deadline_accounted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END,'dueAt',CASE WHEN deadline_at IS NOT NULL THEN to_char(deadline_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END,'remainingUs',deadline_remaining_us::text) FROM app.between_game_decision WHERE id=p_decision
$$;
CREATE FUNCTION app.between_deadline_snapshots(p_match uuid) RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT coalesce(jsonb_agg(app.between_deadline_snapshot(id) ORDER BY id),'[]'::jsonb) FROM app.between_game_decision WHERE match_id=p_match AND state='pending' AND deadline_policy_ref IS NOT NULL
$$;

-- A strictly decreasing metadata chain reproduces every pause/resume boundary,
-- while answered historical decisions retain the same immutable timer proof.
CREATE FUNCTION app.check_between_deadline_snapshot(p_snapshot jsonb) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE current_snapshot jsonb:=p_snapshot; before_snapshot jsonb; source app.command_receipt; input jsonb; revision numeric; prior_revision numeric; account_time timestamptz; prior_time timestamptz; due timestamptz; expected_remaining numeric; origin uuid; immutable_keys text[]:=ARRAY['sourceReceiptId','revision','availabilityRevision','accountedAt','dueAt','remainingUs'];
BEGIN
 IF current_snapshot->>'policyRef' IS NULL THEN
 IF current_snapshot->>'revision' IS DISTINCT FROM '0' OR current_snapshot->>'sourceReceiptId' IS NOT NULL OR current_snapshot->>'originReceiptId' IS NOT NULL OR current_snapshot->>'dueAt' IS NOT NULL OR current_snapshot->>'remainingUs' IS NOT NULL OR current_snapshot->>'accountedAt' IS NOT NULL OR current_snapshot->>'availabilityRevision' IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='inactive deadline cannot claim timer authority';END IF;RETURN;END IF;
 origin:=(current_snapshot->>'originReceiptId')::uuid;
 LOOP
 IF current_snapshot->>'version' IS DISTINCT FROM 'between-deadline-snapshot-v1' OR current_snapshot->>'revision' IS NULL OR current_snapshot->>'revision' !~ '^[1-9][0-9]{0,77}$' OR current_snapshot->>'originReceiptId' IS DISTINCT FROM origin::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline source chain has invalid identity or revision';END IF;
 revision:=(current_snapshot->>'revision')::numeric;account_time:=app.financial_instant(current_snapshot->>'accountedAt');
 IF account_time>clock_timestamp() THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline cannot acknowledge a future observation';END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id::text=current_snapshot->>'sourceReceiptId';
 IF source.id IS NULL OR source.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline requires an accepted source';END IF;
 IF source.scope='between_deadline_schedule_v1' THEN
 input:=source.normalized_source->'input';
 IF source.id<>origin OR revision<>1 OR source.principal_key IS DISTINCT FROM 'between-deadline:'||(current_snapshot->>'decisionId') OR source.target_ref IS DISTINCT FROM current_snapshot->>'decisionId' OR source.normalized_source->>'version' IS DISTINCT FROM 'between-deadline-arm-v1' OR source.normalized_source->'snapshot' IS DISTINCT FROM current_snapshot OR source.payload_hash IS DISTINCT FROM app.auth_json_hash(input) OR input->>'decisionId' IS DISTINCT FROM current_snapshot->>'decisionId' OR input->>'clockRevision' IS DISTINCT FROM current_snapshot->>'clockRevision' OR input->>'deadlineRevision' IS DISTINCT FROM '0' OR input->>'availabilityRevision' IS DISTINCT FROM current_snapshot->>'availabilityRevision' OR input->>'policyRef' IS DISTINCT FROM current_snapshot->>'policyRef' OR input->>'dueAt' IS DISTINCT FROM current_snapshot->>'dueAt' OR input->>'proofRef' IS NULL OR length(btrim(input->>'proofRef')) NOT BETWEEN 1 AND 2048 OR current_snapshot->>'dueAt' IS NULL OR current_snapshot->>'remainingUs' IS NOT NULL OR app.financial_instant(current_snapshot->>'dueAt')<app.financial_instant(current_snapshot->>'openedAt') OR account_time<app.financial_instant(current_snapshot->>'openedAt')
 OR NOT EXISTS(SELECT 1 FROM app.match WHERE id::text=current_snapshot->>'matchId' AND profile_id::text=input->>'profileId') OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM current_snapshot->>'decisionId' OR source.response_payload->'data'->>'deadlineRevision' IS DISTINCT FROM '1' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline origin differs from its exact qualified schedule';END IF;RETURN;
 END IF;
 IF source.scope NOT IN('match_incident_v1','match_restoration_v1') OR source.principal_key IS DISTINCT FROM 'match-recovery:'||(current_snapshot->>'matchId') OR source.target_ref IS DISTINCT FROM current_snapshot->>'matchId' OR source.normalized_source->>'matchId' IS DISTINCT FROM current_snapshot->>'matchId' OR source.normalized_source->>'matchAvailabilityRevision' IS DISTINCT FROM current_snapshot->>'availabilityRevision' OR NOT coalesce(source.normalized_source->'deadlineSnapshots' @> jsonb_build_array(current_snapshot),false) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline revision requires its exact incident or restoration source';END IF;
 SELECT value INTO before_snapshot FROM jsonb_array_elements(source.normalized_source->'beforeDeadlineSnapshots') value WHERE value->>'decisionId'=current_snapshot->>'decisionId';
 IF before_snapshot IS NULL OR current_snapshot-immutable_keys IS DISTINCT FROM before_snapshot-immutable_keys OR before_snapshot->>'revision' IS NULL OR before_snapshot->>'revision' !~ '^[1-9][0-9]{0,77}$' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline rebase cannot change its original decision identity';END IF;
 prior_revision:=(before_snapshot->>'revision')::numeric;prior_time:=app.financial_instant(before_snapshot->>'accountedAt');
 IF before_snapshot->>'availabilityRevision' IS NULL OR before_snapshot->>'availabilityRevision' !~ '^(0|[1-9][0-9]{0,77})$' OR current_snapshot->>'availabilityRevision' IS DISTINCT FROM ((before_snapshot->>'availabilityRevision')::numeric+1)::text OR source.normalized_source->>'availabilityBasis' IS DISTINCT FROM before_snapshot->>'availabilityRevision' OR source.normalized_source->>'version' IS DISTINCT FROM (CASE WHEN source.scope='match_incident_v1' THEN 'match-incident-v1' ELSE 'match-restoration-v1' END) OR source.payload_hash IS DISTINCT FROM app.auth_json_hash(CASE WHEN source.scope='match_incident_v1' THEN jsonb_build_object('version','match-incident-v1','matchId',source.target_ref::uuid,'basis',source.normalized_source->>'basis','availabilityBasis',source.normalized_source->>'availabilityBasis','incidentAt',source.normalized_source->>'incidentAt','kind',source.normalized_source->>'kind','proofRef',source.normalized_source->>'proofRef') ELSE jsonb_build_object('version','match-restoration-v1','matchId',source.target_ref::uuid,'basis',source.normalized_source->>'basis','availabilityBasis',source.normalized_source->>'availabilityBasis','incidentId',(source.normalized_source->>'incidentId')::uuid,'proofRef',source.normalized_source->>'proofRef') END) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline recovery must advance its exact availability source';END IF;
 IF prior_revision+1<>revision OR account_time<prior_time THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline source must advance once without reversing acknowledged time';END IF;
 IF source.scope='match_incident_v1' THEN
 due:=app.financial_instant(before_snapshot->>'dueAt');
 expected_remaining:=greatest(0,extract(epoch FROM (due-account_time))*1000000);
 IF current_snapshot->>'dueAt' IS NOT NULL OR before_snapshot->>'remainingUs' IS NOT NULL OR current_snapshot->>'remainingUs' IS DISTINCT FROM expected_remaining::numeric(78,0)::text OR source.normalized_source->>'incidentAt' IS DISTINCT FROM current_snapshot->>'accountedAt' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='frozen deadline must preserve exact remaining microseconds';END IF;
 ELSE
 IF before_snapshot->>'dueAt' IS NOT NULL OR before_snapshot->>'remainingUs' IS NULL OR before_snapshot->>'remainingUs' !~ '^(0|[1-9][0-9]{0,77})$' OR current_snapshot->>'remainingUs' IS NOT NULL OR current_snapshot->>'dueAt' IS NULL OR app.financial_instant(current_snapshot->>'dueAt') IS DISTINCT FROM account_time+((before_snapshot->>'remainingUs')||' microseconds')::interval OR source.normalized_source->>'resumedAt' IS DISTINCT FROM current_snapshot->>'accountedAt' OR source.normalized_source->>'incidentId' IS DISTINCT FROM before_snapshot->>'sourceReceiptId' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restored deadline must rebase its exact frozen duration';END IF;
 END IF;
 current_snapshot:=before_snapshot;
 END LOOP;
END $$;

CREATE FUNCTION app.check_between_deadline(p_decision uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE d app.between_game_decision; m app.match;
BEGIN
 SELECT * INTO d FROM app.between_game_decision WHERE id=p_decision;
 IF d.id IS NULL THEN RETURN;END IF;
 PERFORM app.check_between_deadline_snapshot(app.between_deadline_snapshot(d.id));
 IF d.deadline_policy_ref IS NULL OR d.state<>'pending' THEN RETURN;END IF;
 SELECT * INTO m FROM app.match WHERE id=d.match_id;
 IF m.state<>'between_games' OR d.deadline_availability_revision IS DISTINCT FROM m.availability_revision OR (m.availability='available' AND (d.deadline_at IS NULL OR d.deadline_remaining_us IS NOT NULL)) OR (m.availability='recovering' AND (d.deadline_at IS NOT NULL OR d.deadline_remaining_us IS NULL OR d.deadline_source_receipt_id IS DISTINCT FROM m.incident_receipt_id)) OR m.availability NOT IN('available','recovering') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pending deadline must match its actual current availability';END IF;
END $$;
CREATE FUNCTION app.guard_between_deadline_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE target uuid; source app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='deadline mutations require read committed retry';END IF;
 IF TG_TABLE_NAME='between_game_decision' THEN PERFORM app.check_between_deadline(NEW.id);
 ELSIF TG_TABLE_NAME='match' THEN FOR target IN SELECT id FROM app.between_game_decision WHERE match_id=NEW.id AND deadline_policy_ref IS NOT NULL LOOP PERFORM app.check_between_deadline(target);END LOOP;
 ELSE
 SELECT * INTO source FROM app.command_receipt WHERE id=NEW.id;
 IF source.scope='between_deadline_schedule_v1' AND source.state='accepted' THEN
 IF NOT EXISTS(SELECT 1 FROM app.between_game_decision WHERE id::text=source.target_ref AND deadline_origin_receipt_id=source.id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted deadline schedule requires its actual retained decision';END IF;
 PERFORM app.check_between_deadline((source.target_ref)::uuid);
 END IF;
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER between_deadline_source AFTER INSERT OR UPDATE ON app.between_game_decision DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_between_deadline_source();
CREATE CONSTRAINT TRIGGER between_deadline_parent AFTER UPDATE ON app.match DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_between_deadline_source();
CREATE CONSTRAINT TRIGGER between_deadline_origin AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_between_deadline_source();

CREATE FUNCTION app.begin_between_deadline_command(p_scope text,p_decision uuid,p_request uuid,p_input jsonb) RETURNS app.command_receipt LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; principal text;
BEGIN
 IF p_scope IS NULL OR p_scope NOT IN('between_deadline_schedule_v1','between_deadline_expiry_v1') OR p_decision IS NULL OR p_decision='00000000-0000-0000-0000-000000000000'::uuid OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_input IS NULL OR octet_length(p_input::text)>65536 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid deadline command identity';END IF;
 principal:=CASE WHEN p_scope='between_deadline_schedule_v1' THEN 'between-deadline:' ELSE 'between-expiry:' END||p_decision::text;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES(principal,p_scope,p_decision::text,p_request::text,app.auth_json_hash(p_input),'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key=principal AND scope=p_scope AND request_ref=p_request::text FOR UPDATE;
 IF receipt.target_ref IS DISTINCT FROM p_decision::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(p_input) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline request identity was used for different terms';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='deadline command disposition is pending';END IF;END IF;
 RETURN receipt;
END $$;

CREATE FUNCTION app.schedule_between_game_deadline(p_decision uuid,p_match_revision numeric,p_deadline_revision numeric,p_clock_revision numeric,p_availability_revision numeric,p_profile uuid,p_policy text,p_due timestamptz,p_qualification text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE receipt app.command_receipt; d app.between_game_decision; m app.match; discovered uuid; input jsonb; at_time timestamptz;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='deadline scheduling requires read committed retry';END IF;
 IF p_match_revision IS NULL OR p_match_revision::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_deadline_revision IS NULL OR p_deadline_revision::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_clock_revision IS NULL OR p_clock_revision::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_availability_revision IS NULL OR p_availability_revision::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_profile IS NULL OR p_policy IS NULL OR length(btrim(p_policy)) NOT BETWEEN 1 AND 2048 OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048 OR p_due IS NULL OR NOT isfinite(p_due) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='qualified deadline policy, time and generations are required';END IF;
 input:=jsonb_build_object('decisionId',p_decision,'matchRevision',p_match_revision::text,'deadlineRevision',p_deadline_revision::text,'clockRevision',p_clock_revision::text,'availabilityRevision',p_availability_revision::text,'profileId',p_profile,'policyRef',p_policy,'dueAt',to_char(p_due AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'proofRef',p_qualification);
 receipt:=app.begin_between_deadline_command('between_deadline_schedule_v1',p_decision,p_request,input);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT match_id INTO discovered FROM app.between_game_decision WHERE id=p_decision;
 SELECT * INTO m FROM app.match WHERE id=discovered FOR NO KEY UPDATE;
 SELECT * INTO d FROM app.between_game_decision WHERE id=p_decision FOR NO KEY UPDATE;
 IF d.id IS NULL OR m.id IS NULL OR d.match_id IS DISTINCT FROM discovered THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('decision','unavailable'));END IF;
 IF m.revision<>p_match_revision OR d.deadline_revision<>p_deadline_revision OR d.clock_revision<>p_clock_revision OR m.availability_revision<>p_availability_revision THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='deadline basis changed before scheduling';END IF;
 IF d.state<>'pending' OR d.after_game_id IS NULL OR m.structure<>'bo3' OR m.state<>'between_games' OR m.availability<>'available' OR m.profile_id<>p_profile OR m.room_id IS NULL OR m.started_at IS NULL OR m.accepted_release_id IS NULL OR d.deadline_policy_ref IS NOT NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('decision','unavailable for a new qualified deadline'));END IF;
 PERFORM app.check_started_match_frontier(m.id);PERFORM app.check_between_deadline(d.id);
 at_time:=clock_timestamp();IF at_time<d.opened_at OR p_due<d.opened_at THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='deadline time predates its acknowledged decision opening';END IF;
 UPDATE app.between_game_decision SET deadline_policy_ref=p_policy,deadline_revision=deadline_revision+1,deadline_origin_receipt_id=receipt.id,deadline_source_receipt_id=receipt.id,deadline_availability_revision=m.availability_revision,deadline_accounted_at=at_time,deadline_at=p_due WHERE id=d.id;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','between-deadline-arm-v1','input',input,'snapshot',app.between_deadline_snapshot(d.id)) WHERE id=receipt.id;
 INSERT INTO app.durable_handoff(match_id,source_ref,destination,kind,state) VALUES(m.id,receipt.id::text,'match_lifecycle','between_deadline','pending');
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',d.id,'deadlineRevision',(p_deadline_revision+1)::text,'dueAt',to_char(p_due AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')));
END $$;
REVOKE ALL ON FUNCTION app.guard_between_deadline_history(),app.between_deadline_snapshot(uuid),app.between_deadline_snapshots(uuid),app.check_between_deadline_snapshot(jsonb),app.check_between_deadline(uuid),app.guard_between_deadline_source(),app.begin_between_deadline_command(text,uuid,uuid,jsonb),app.schedule_between_game_deadline(uuid,numeric,numeric,numeric,numeric,uuid,text,timestamptz,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
