BEGIN;
-- Process generation is independent of the authority epoch advanced by adoption.
ALTER TABLE app.ai_runner_binding ADD COLUMN provider_start_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
UPDATE app.ai_runner_binding SET provider_start_receipt_id=bootstrap_receipt_id WHERE bootstrap_receipt_id IS NOT NULL;
ALTER TABLE app.ai_runner_binding ADD CONSTRAINT runner_process_source_tuple CHECK((bootstrap_receipt_id IS NULL)=(provider_start_receipt_id IS NULL));


-- Neither adoption nor repeated provider labels permit rewinding physical generation.
CREATE FUNCTION app.guard_ai_process_generation() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF NEW.provider_start_receipt_id IS DISTINCT FROM OLD.provider_start_receipt_id AND NOT(NEW.binding_epoch>OLD.binding_epoch AND NEW.provider_start_receipt_id=NEW.epoch_receipt_id AND EXISTS(SELECT 1 FROM app.command_receipt r WHERE r.id=NEW.provider_start_receipt_id AND r.scope='ai_rebind_v1' AND r.target_ref=NEW.id::text AND r.normalized_source->>'oldEpoch'=OLD.binding_epoch::text AND r.normalized_source->>'newEpoch'=NEW.binding_epoch::text)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='process generation changes require its new exact rebind epoch';END IF;RETURN NEW;
END $$;
CREATE TRIGGER runner_process_generation BEFORE UPDATE ON app.ai_runner_binding FOR EACH ROW EXECUTE FUNCTION app.guard_ai_process_generation();

CREATE OR REPLACE FUNCTION app.begin_ai_runtime_command(p_scope text,p_target uuid,p_request uuid,p_terms jsonb) RETURNS app.command_receipt
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; fingerprint text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI runtime source requires read committed retry';END IF;
 IF p_scope IS NULL OR p_scope NOT IN ('ai_provider_ack_v1','ai_grant_issue_v1','ai_delivery_ack_v1','ai_stop_ack_v1','ai_adopt_v1','ai_rebind_v1') OR p_target IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_terms IS NULL OR octet_length(p_terms::text)>262144 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid AI runtime source envelope';END IF;
 fingerprint:=encode(sha256(convert_to(p_terms::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('ai-runtime',p_scope,p_target::text,p_request::text,fingerprint,'pending','',p_terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NOT NULL THEN RETURN receipt;END IF;
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='ai-runtime' AND scope=p_scope AND request_ref=p_request::text FOR UPDATE;
 IF receipt.target_ref<>p_target::text OR receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI runtime source request terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI runtime source disposition pending';END IF;
 RETURN receipt;
END $$;

CREATE OR REPLACE FUNCTION app.bootstrap_ai_slot(p_actor uuid,p_slot uuid,p_expected numeric,p_revision uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; slot app.room_slot; lobby app.room; actors uuid[]; binding_id uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI bootstrap requires read committed retry';END IF;
 IF p_slot IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid AI bootstrap envelope';END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'ai_bootstrap_v1',p_slot::text,jsonb_build_object('basis',p_expected::text,'revision',p_revision));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT * INTO slot FROM app.room_slot WHERE id=p_slot;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id;
 actors:=ARRAY[lobby.host_id,slot.sponsor_id,p_actor];
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO slot FROM app.room_slot WHERE id=p_slot FOR NO KEY UPDATE;
 IF slot.room_id IS DISTINCT FROM lobby.id OR (slot.sponsor_id IS NOT NULL AND NOT(slot.sponsor_id=ANY(actors))) OR (lobby.host_id IS NOT NULL AND NOT(lobby.host_id=ANY(actors))) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='bootstrap source changed before account/room fence';END IF;
 IF slot.id IS NULL OR slot.retired_at IS NOT NULL OR slot.controller_kind<>'ai' OR slot.sponsor_id IS NULL OR lobby.state NOT IN ('lobby','countdown') OR lobby.revision<>p_expected
 OR p_actor NOT IN (slot.sponsor_id,lobby.host_id) OR EXISTS(SELECT 1 FROM app.account WHERE id=ANY(actors) AND state<>'active')
 OR EXISTS(SELECT 1 FROM app.ai_runner_binding WHERE room_slot_id=p_slot AND desired_state='running') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('slot','stale or unavailable'));END IF;
 IF p_revision IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id WHERE r.id=p_revision AND r.sealed_at IS NOT NULL AND d.mode=lobby.mode AND d.format_id=lobby.format_id AND (d.source_kind='system_pool' OR (d.source_kind='account' AND d.account_id=slot.sponsor_id)) FOR SHARE OF r,d) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','unavailable'));END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','ai-bootstrap-v1','slotId',slot.id,'sponsorId',slot.sponsor_id,'roomId',lobby.id,'configurationId',lobby.configuration_id,'epoch','1','startKey',receipt.id::text) WHERE id=receipt.id;
 INSERT INTO app.ai_runner_binding(room_slot_id,binding_epoch,desired_state,observed_state,bootstrap_receipt_id,epoch_receipt_id,provider_start_receipt_id)
 VALUES(slot.id,1,'running','pending',receipt.id,receipt.id,receipt.id) RETURNING id INTO binding_id;
 UPDATE app.room_slot SET deck_revision_id=p_revision,ready=false WHERE id=slot.id;
 UPDATE app.room_slot SET ready=false WHERE room_id=lobby.id AND retired_at IS NULL AND controller_kind='human';
 UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=lobby.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding_id,'epoch','1','startKey',receipt.id));
END $$;


CREATE FUNCTION app.ai_stop_confirmed(p_binding uuid,p_stop uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.command_receipt stop JOIN app.command_receipt ack ON ack.normalized_source->>'stopId'=stop.id::text
 WHERE stop.id=p_stop AND stop.state='accepted' AND stop.scope='ai_stop_request_v1' AND stop.principal_key='ai-runtime' AND stop.target_ref=p_binding::text
 AND stop.normalized_source->>'bindingId'=p_binding::text AND ack.state='accepted' AND ack.scope='ai_stop_ack_v1' AND ack.principal_key='ai-runtime' AND ack.target_ref=p_binding::text
 AND ack.normalized_source->>'bindingId'=p_binding::text AND ack.normalized_source->>'epoch'=stop.normalized_source->>'epoch')
$$;
CREATE FUNCTION app.ai_start_dependency_ready(p_binding uuid,p_start uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.id=p_start AND source.state='accepted' AND (
 (source.scope='ai_bootstrap_v1' AND source.normalized_source->>'startKey'=source.id::text)
 OR (source.scope='ai_rebind_v1' AND source.normalized_source->>'bindingId'=p_binding::text AND EXISTS(SELECT 1 FROM app.command_receipt stop WHERE stop.id::text=source.normalized_source->>'previousStopId' AND app.ai_stop_confirmed(p_binding,stop.id)))))
$$;
-- This is the canonical private worker projection; no provider call occurs in SQL.
CREATE FUNCTION app.read_ai_start_work(p_binding uuid,p_epoch numeric) RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('bindingId',b.id,'epoch',b.binding_epoch::text,'startId',b.provider_start_receipt_id,'startKey',b.provider_start_receipt_id::text,'seatId',b.match_seat_id)
 FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id JOIN app.room room ON room.id=slot.room_id
 WHERE b.id=p_binding AND b.binding_epoch=p_epoch AND b.desired_state='running' AND EXISTS(SELECT 1 FROM app.account a WHERE a.id=slot.sponsor_id AND (a.state='active' OR app.ai_sponsor_continuing_match(b.id,a.id))) AND b.provider_runner_ref IS NULL AND slot.retired_at IS NULL AND room.state<>'closed'
 AND NOT EXISTS(SELECT 1 FROM app.match WHERE room_id=room.id AND state IN ('completed','no_contest')) AND app.ai_start_dependency_ready(b.id,b.provider_start_receipt_id)
$$;


-- Historical stop work survives replacement of the binding's current pointers.
CREATE FUNCTION app.read_ai_stop_work(p_binding uuid) RETURNS SETOF jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('bindingId',p_binding,'stopId',r.id,'epoch',r.normalized_source->>'epoch','startId',r.normalized_source->>'providerStartId','processEpoch',r.normalized_source->>'processEpoch','providerRef',r.normalized_source->>'providerRef')
 FROM app.command_receipt r WHERE r.scope='ai_stop_request_v1' AND r.principal_key='ai-runtime' AND r.state='accepted' AND r.target_ref=p_binding::text AND r.normalized_source->>'bindingId'=p_binding::text AND NOT app.ai_stop_confirmed(p_binding,r.id) ORDER BY r.id
$$;

CREATE OR REPLACE FUNCTION app.request_ai_stop(p_binding uuid,p_cause uuid) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot;
BEGIN
 SELECT s.* INTO slot FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE b.id=p_binding;
 PERFORM id FROM app.account WHERE id=slot.sponsor_id FOR NO KEY UPDATE;
 PERFORM id FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO STRICT binding FROM app.ai_runner_binding WHERE id=p_binding FOR NO KEY UPDATE;
 IF binding.desired_state='stopped' AND binding.stop_receipt_id IS NOT NULL THEN RETURN binding.stop_receipt_id;END IF;
 IF p_cause IS NULL OR NOT EXISTS(SELECT 1 FROM app.command_receipt WHERE id=p_cause) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='stop request requires originating command';END IF;
 -- Use the existing immutable command source, with a stable per-binding stop request.
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('ai-runtime','ai_stop_request_v1',binding.id::text,binding.id::text||':'||binding.binding_epoch::text,app.auth_json_hash(jsonb_build_object('bindingId',binding.id,'epoch',binding.binding_epoch::text,'causeId',p_cause)),'pending','',jsonb_build_object('version','ai-stop-v1','bindingId',binding.id,'epoch',binding.binding_epoch::text,'causeId',p_cause,'providerStartId',binding.provider_start_receipt_id,'processEpoch',(SELECT coalesce(normalized_source->>'newEpoch',normalized_source->>'epoch') FROM app.command_receipt WHERE id=binding.provider_start_receipt_id),'providerRef',binding.provider_runner_ref)) RETURNING * INTO receipt;
 UPDATE app.controller_grant SET state='revoked' WHERE binding_id=binding.id AND state='active';
 UPDATE app.ai_runner_binding SET desired_state='stopped',stop_receipt_id=receipt.id WHERE id=binding.id;
 PERFORM app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'disposition','stop requested'));
 RETURN receipt.id;
END $$;


CREATE FUNCTION app.rebind_ai_runner(p_binding uuid,p_epoch numeric,p_proof text,p_request uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; lobby app.room; original_slot uuid; original_seat uuid; parent uuid; stop_id uuid;
BEGIN
 IF p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,76})$' OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid controller rebind envelope';END IF;
 receipt:=app.begin_ai_runtime_command('ai_rebind_v1',p_binding,p_request,jsonb_build_object('bindingId',p_binding,'basis',p_epoch::text,'proofRef',p_proof));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding; original_slot:=binding.room_slot_id;original_seat:=binding.match_seat_id;
 SELECT * INTO slot FROM app.room_slot WHERE id=original_slot;
 PERFORM id FROM app.account WHERE id=slot.sponsor_id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 PERFORM id FROM app.room_slot WHERE id=original_slot FOR NO KEY UPDATE;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding FOR NO KEY UPDATE;
 IF binding.room_slot_id IS DISTINCT FROM original_slot OR binding.match_seat_id IS DISTINCT FROM original_seat OR NOT EXISTS(SELECT 1 FROM app.room_slot s WHERE s.id=slot.id AND s.room_id=slot.room_id AND s.sponsor_id IS NOT DISTINCT FROM slot.sponsor_id AND s.retired_at IS NOT DISTINCT FROM slot.retired_at) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='rebind source changed before controller fence';END IF;
 PERFORM id FROM app.match WHERE room_id=lobby.id OR id IN (SELECT match_id FROM app.match_seat WHERE id=original_seat) ORDER BY id FOR SHARE;
 IF binding.id IS NULL OR binding.bootstrap_receipt_id IS NULL OR binding.binding_epoch<>p_epoch OR slot.retired_at IS NOT NULL OR lobby.state='closed'
 OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=slot.sponsor_id AND (state='active' OR app.ai_sponsor_continuing_match(binding.id,slot.sponsor_id)))
 OR (original_seat IS NULL AND lobby.state NOT IN ('lobby','countdown'))
 OR EXISTS(SELECT 1 FROM app.match WHERE room_id=lobby.id AND state IN ('completed','no_contest'))
 OR (original_seat IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.match_seat seat JOIN app.match m ON m.id=seat.match_id WHERE seat.id=original_seat AND m.state IN ('ready','active','between_games')))
 OR EXISTS(SELECT 1 FROM app.ai_runner_binding WHERE room_slot_id=slot.id AND id<>binding.id AND desired_state='running') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('binding','stale or unavailable'));END IF;
 PERFORM app.check_ai_runtime_source(binding.id);
 IF NOT app.ai_start_dependency_ready(binding.id,binding.provider_start_receipt_id) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='prior controller stop is still unresolved';END IF;
 stop_id:=app.request_ai_stop(binding.id,receipt.id);
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('version','ai-rebind-v1','slotId',slot.id,'roomId',lobby.id,'configurationId',lobby.configuration_id,'seatId',original_seat,'oldEpoch',p_epoch::text,'newEpoch',(p_epoch+1)::text,'previousStartId',binding.provider_start_receipt_id,'previousStopId',stop_id,'startKey',receipt.id::text) WHERE id=receipt.id;
 UPDATE app.ai_runner_binding SET binding_epoch=p_epoch+1,epoch_receipt_id=receipt.id,provider_start_receipt_id=receipt.id,desired_state='running',observed_state='pending',provider_runner_ref=NULL,provider_observation_receipt_id=NULL,stop_receipt_id=NULL,stop_observation_receipt_id=NULL WHERE id=binding.id;
 UPDATE app.room_slot SET ready=false WHERE id=slot.id;
 IF original_seat IS NULL THEN
 UPDATE app.room_slot SET ready=false WHERE room_id=lobby.id AND retired_at IS NULL AND controller_kind='human';
 UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=lobby.id;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'epoch',(p_epoch+1)::text,'startKey',receipt.id,'previousStopId',stop_id));
END $$;


CREATE OR REPLACE FUNCTION app.observe_ai_provider(p_binding uuid,p_epoch numeric,p_start uuid,p_outcome text,p_provider_ref text,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; lobby app.room;
BEGIN
 IF p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_start IS NULL OR p_outcome IS NULL OR p_outcome NOT IN ('running','unavailable','unknown') OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048
 OR (p_outcome='running' AND (p_provider_ref IS NULL OR length(btrim(p_provider_ref)) NOT BETWEEN 1 AND 2048)) OR (p_outcome<>'running' AND p_provider_ref IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid provider observation';END IF;
 receipt:=app.begin_ai_runtime_command('ai_provider_ack_v1',p_binding,p_request,jsonb_build_object('bindingId',p_binding,'epoch',p_epoch::text,'startId',p_start,'outcome',p_outcome,'providerRef',p_provider_ref,'proofRef',p_proof));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT s.* INTO slot FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE b.id=p_binding;
 PERFORM id FROM app.account WHERE id=slot.sponsor_id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding FOR NO KEY UPDATE;
 IF slot.id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.room_slot current_slot WHERE current_slot.id=slot.id AND current_slot.room_id=slot.room_id AND current_slot.sponsor_id IS NOT DISTINCT FROM slot.sponsor_id AND current_slot.retired_at IS NOT DISTINCT FROM slot.retired_at) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='runtime slot source changed before account/room fence';END IF;
 IF binding.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.account a WHERE a.id=slot.sponsor_id AND (a.state='active' OR app.ai_sponsor_continuing_match(binding.id,a.id))) OR binding.binding_epoch<>p_epoch OR binding.provider_start_receipt_id IS DISTINCT FROM p_start OR binding.desired_state<>'running' OR slot.retired_at IS NOT NULL OR lobby.state='closed' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('source','stale'));END IF;
 IF NOT app.ai_start_dependency_ready(binding.id,p_start) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='provider start waits for previous stop acknowledgment';END IF;
 IF binding.provider_runner_ref IS NOT NULL THEN
 IF p_outcome='running' AND binding.provider_runner_ref=p_provider_ref THEN RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'disposition','already observed'));END IF;
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('provider','conflicting observation'));END IF;
 UPDATE app.ai_runner_binding SET provider_runner_ref=p_provider_ref,observed_state=p_outcome,provider_observation_receipt_id=receipt.id WHERE id=binding.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'disposition',p_outcome));
END $$;

CREATE OR REPLACE FUNCTION app.observe_ai_stop(p_binding uuid,p_epoch numeric,p_stop uuid,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; stop_source app.command_receipt;
BEGIN
 IF p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_stop IS NULL OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid stop observation';END IF;
 receipt:=app.begin_ai_runtime_command('ai_stop_ack_v1',p_binding,p_request,jsonb_build_object('bindingId',p_binding,'epoch',p_epoch::text,'stopId',p_stop,'proofRef',p_proof));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT s.* INTO slot FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE b.id=p_binding;
 PERFORM id FROM app.account WHERE id=slot.sponsor_id FOR NO KEY UPDATE;
 PERFORM id FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding FOR NO KEY UPDATE;
 SELECT * INTO stop_source FROM app.command_receipt WHERE id=p_stop;
 IF binding.id IS NULL OR stop_source.state IS DISTINCT FROM 'accepted' OR stop_source.scope IS DISTINCT FROM 'ai_stop_request_v1' OR stop_source.principal_key IS DISTINCT FROM 'ai-runtime' OR stop_source.target_ref IS DISTINCT FROM p_binding::text OR stop_source.normalized_source->>'bindingId' IS DISTINCT FROM p_binding::text OR stop_source.normalized_source->>'epoch' IS DISTINCT FROM p_epoch::text OR p_epoch>binding.binding_epoch THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('source','stale'));END IF;
 IF binding.binding_epoch<>p_epoch OR binding.desired_state<>'stopped' OR binding.stop_receipt_id IS DISTINCT FROM p_stop THEN RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'disposition','historical stop confirmed'));END IF;
 UPDATE app.ai_runner_binding SET observed_state='stopped',stop_observation_receipt_id=receipt.id WHERE id=binding.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'disposition','stopped'));
END $$;

CREATE OR REPLACE FUNCTION app.check_ai_runtime_source(p_binding uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE binding app.ai_runner_binding; source app.command_receipt; observation app.command_receipt; grant_row app.controller_grant;
BEGIN
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding;
 IF binding.id IS NULL OR binding.bootstrap_receipt_id IS NULL THEN RETURN;END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=binding.bootstrap_receipt_id;
 IF source.state IS DISTINCT FROM 'accepted' OR source.scope IS DISTINCT FROM 'ai_bootstrap_v1' OR source.normalized_source->>'version' IS DISTINCT FROM 'ai-bootstrap-v1'
 OR source.normalized_source->>'sponsorId' IS DISTINCT FROM (SELECT sponsor_id::text FROM app.room_slot WHERE id=binding.room_slot_id) OR source.normalized_source->>'roomId' IS DISTINCT FROM (SELECT room_id::text FROM app.room_slot WHERE id=binding.room_slot_id)
 OR source.normalized_source->>'slotId' IS DISTINCT FROM binding.room_slot_id::text OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM binding.id::text
 OR source.normalized_source->>'startKey' IS DISTINCT FROM source.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner lacks original accepted bootstrap source';END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=binding.epoch_receipt_id;
 IF source.state IS DISTINCT FROM 'accepted' OR NOT((source.scope='ai_bootstrap_v1' AND source.normalized_source->>'epoch'=binding.binding_epoch::text AND source.id=binding.bootstrap_receipt_id)
 OR (source.scope='ai_rebind_v1' AND source.normalized_source->>'newEpoch'=binding.binding_epoch::text AND source.normalized_source->>'bindingId'=binding.id::text AND source.normalized_source->>'seatId' IS NOT DISTINCT FROM binding.match_seat_id::text AND source.normalized_source->>'slotId'=binding.room_slot_id::text AND source.id=binding.provider_start_receipt_id)
 OR (source.scope='ai_adopt_v1' AND source.normalized_source->>'newEpoch'=binding.binding_epoch::text AND source.normalized_source->>'bindingId'=binding.id::text AND source.normalized_source->>'seatId'=binding.match_seat_id::text)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner epoch lacks accepted source';END IF;
 IF source.scope='ai_adopt_v1' AND (source.normalized_source->>'providerStartId' IS DISTINCT FROM binding.provider_start_receipt_id::text OR NOT EXISTS(
 SELECT 1 FROM app.command_receipt parent JOIN app.match_seat seat ON seat.id=binding.match_seat_id JOIN app.match m ON m.id=seat.match_id
 WHERE parent.id::text=source.normalized_source->>'startId' AND parent.state='accepted' AND parent.scope='room_start_v1' AND parent.principal_key='room-start:'||m.room_id::text AND parent.target_ref=m.room_id::text
 AND parent.response_payload->'data'->>'resourceId'=m.id::text AND source.normalized_source->>'roomId'=m.room_id::text AND source.normalized_source->>'matchId'=m.id::text
 )) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI adoption lacks its exact accepted room-start source';END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=binding.provider_start_receipt_id;
 IF source.state IS DISTINCT FROM 'accepted' OR NOT((source.id=binding.bootstrap_receipt_id AND source.scope='ai_bootstrap_v1') OR (source.scope='ai_rebind_v1' AND source.principal_key='ai-runtime' AND source.target_ref=binding.id::text AND source.normalized_source->>'bindingId'=binding.id::text AND source.normalized_source->>'slotId'=binding.room_slot_id::text AND (source.normalized_source->>'seatId' IS NOT DISTINCT FROM binding.match_seat_id::text OR (source.normalized_source->>'seatId' IS NULL AND EXISTS(SELECT 1 FROM app.command_receipt adoption WHERE adoption.id=binding.epoch_receipt_id AND adoption.scope='ai_adopt_v1' AND adoption.state='accepted' AND adoption.normalized_source->>'oldEpoch'=source.normalized_source->>'newEpoch' AND adoption.normalized_source->>'seatId'=binding.match_seat_id::text))) AND source.normalized_source->>'startKey'=source.id::text AND source.response_payload->'data'->>'startKey'=source.id::text AND EXISTS(SELECT 1 FROM app.command_receipt stop WHERE stop.id::text=source.normalized_source->>'previousStopId' AND stop.scope='ai_stop_request_v1' AND stop.state='accepted' AND stop.target_ref=binding.id::text AND EXISTS(SELECT 1 FROM app.command_receipt cause WHERE cause.id::text=stop.normalized_source->>'causeId' AND cause.state='accepted') AND stop.normalized_source->>'epoch'=source.normalized_source->>'oldEpoch' AND stop.normalized_source->>'providerStartId'=source.normalized_source->>'previousStartId' AND (stop.normalized_source->>'epoch')::numeric+1=(source.normalized_source->>'newEpoch')::numeric))) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner process lacks exact accepted start dependency source';END IF;
 IF binding.provider_runner_ref IS NOT NULL THEN
 IF NOT app.ai_start_dependency_ready(binding.id,binding.provider_start_receipt_id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='current process bypassed prior stop dependency';END IF;
 SELECT * INTO observation FROM app.command_receipt WHERE id=binding.provider_observation_receipt_id;
 IF observation.state IS DISTINCT FROM 'accepted' OR observation.scope IS DISTINCT FROM 'ai_provider_ack_v1' OR observation.normalized_source->>'bindingId' IS DISTINCT FROM binding.id::text
 OR observation.normalized_source->>'outcome' IS DISTINCT FROM 'running' OR observation.normalized_source->>'providerRef' IS DISTINCT FROM binding.provider_runner_ref
 OR observation.normalized_source->>'startId' IS DISTINCT FROM binding.provider_start_receipt_id::text OR (observation.normalized_source->>'epoch')::numeric>binding.binding_epoch OR (observation.normalized_source->>'epoch')::numeric<coalesce((source.normalized_source->>'newEpoch')::numeric,1) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='provider identity lacks accepted process observation';END IF;
 END IF;
 FOR grant_row IN SELECT * FROM app.controller_grant WHERE binding_id=binding.id LOOP
 SELECT * INTO source FROM app.command_receipt WHERE id=grant_row.issue_receipt_id;
 IF source.state IS DISTINCT FROM 'accepted' OR source.scope NOT IN ('ai_grant_issue_v1','ai_adopt_v1') OR source.normalized_source->>'bindingId' IS DISTINCT FROM binding.id::text
 OR (source.scope='ai_grant_issue_v1' AND (source.normalized_source->>'epoch' IS DISTINCT FROM grant_row.binding_epoch::text OR source.normalized_source->>'secretRef' IS DISTINCT FROM grant_row.grant_secret_ref OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM grant_row.id::text))
 OR (source.scope='ai_adopt_v1' AND (source.normalized_source->>'newEpoch' IS DISTINCT FROM grant_row.binding_epoch::text OR source.normalized_source->>'seatId' IS DISTINCT FROM grant_row.match_seat_id::text OR source.response_payload->'data'->>'grantId' IS DISTINCT FROM grant_row.id::text OR grant_row.grant_secret_ref IS DISTINCT FROM 'controller-secret:'||grant_row.id::text)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='grant lacks accepted issue source';END IF;
 IF grant_row.delivery_state='pending' THEN
 IF grant_row.delivery_receipt_id IS NOT NULL OR grant_row.delivery_revision<>0 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pending grant has invented delivery history';END IF;
 ELSE
 SELECT * INTO observation FROM app.command_receipt WHERE id=grant_row.delivery_receipt_id;
 IF observation.state IS DISTINCT FROM 'accepted' OR observation.scope IS DISTINCT FROM 'ai_delivery_ack_v1' OR observation.normalized_source->>'grantId' IS DISTINCT FROM grant_row.id::text
 OR observation.normalized_source->>'outcome' IS DISTINCT FROM grant_row.delivery_state OR observation.normalized_source->>'basis' IS DISTINCT FROM (grant_row.delivery_revision-1)::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='grant delivery lacks exact accepted observation';END IF;
 END IF;
 END LOOP;
 IF binding.desired_state='stopped' THEN
 SELECT * INTO source FROM app.command_receipt WHERE id=binding.stop_receipt_id;
 IF source.state IS DISTINCT FROM 'accepted' OR source.scope IS DISTINCT FROM 'ai_stop_request_v1' OR NOT EXISTS(SELECT 1 FROM app.command_receipt cause WHERE cause.id::text=source.normalized_source->>'causeId' AND cause.state='accepted') OR source.normalized_source->>'providerStartId' IS DISTINCT FROM binding.provider_start_receipt_id::text OR source.normalized_source->>'version' IS DISTINCT FROM 'ai-stop-v1' OR source.normalized_source->>'bindingId' IS DISTINCT FROM binding.id::text OR source.normalized_source->>'epoch' IS DISTINCT FROM binding.binding_epoch::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner stop requires durable source work';END IF;
 END IF;
 IF binding.observed_state='stopped' THEN
 SELECT * INTO observation FROM app.command_receipt WHERE id=binding.stop_observation_receipt_id;
 IF observation.state IS DISTINCT FROM 'accepted' OR observation.scope IS DISTINCT FROM 'ai_stop_ack_v1' OR observation.normalized_source->>'stopId' IS DISTINCT FROM binding.stop_receipt_id::text
 OR observation.normalized_source->>'epoch' IS DISTINCT FROM binding.binding_epoch::text OR observation.normalized_source->>'bindingId' IS DISTINCT FROM binding.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='observed stop requires exact normalized acknowledgment';END IF;
 END IF;
END $$;


CREATE OR REPLACE FUNCTION app.adopt_room_ai(p_room uuid,p_match uuid,p_start uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE slot app.room_slot; binding app.ai_runner_binding; seat app.match_seat; receipt app.command_receipt; new_grant uuid;
BEGIN
 FOR slot IN SELECT * FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL AND controller_kind='ai' ORDER BY id LOOP
 SELECT * INTO binding FROM app.ai_runner_binding WHERE room_slot_id=slot.id AND desired_state='running' FOR NO KEY UPDATE;
 IF binding.id IS NULL OR binding.bootstrap_receipt_id IS NULL OR binding.match_seat_id IS NOT NULL OR NOT app.ai_slot_ready(slot.id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI match adoption requires qualified current room controller';END IF;
 PERFORM app.check_ai_runtime_source(binding.id);
 IF NOT EXISTS(SELECT 1 FROM app.controller_grant WHERE binding_id=binding.id AND binding_epoch=binding.binding_epoch AND state='active' AND delivery_state='delivered') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI room grant has not been delivered';END IF;
 SELECT * INTO STRICT seat FROM app.match_seat WHERE match_id=p_match AND seat_index=slot.seat_index AND controller_kind='ai' AND sponsor_id=slot.sponsor_id;
 receipt:=app.begin_ai_runtime_command('ai_adopt_v1',seat.id,binding.id,jsonb_build_object('bindingId',binding.id,'seatId',seat.id,'roomId',p_room,'matchId',p_match,'oldEpoch',binding.binding_epoch::text,'newEpoch',(binding.binding_epoch+1)::text,'startId',p_start,'providerStartId',binding.provider_start_receipt_id));
 IF receipt.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI binding already adopted';END IF;
 UPDATE app.controller_grant SET state='revoked' WHERE binding_id=binding.id AND state='active';
 UPDATE app.ai_runner_binding SET binding_epoch=binding_epoch+1,match_seat_id=seat.id,epoch_receipt_id=receipt.id,observed_state='running' WHERE id=binding.id;
 new_grant:=gen_random_uuid();
 -- This allocates a private secret-store reference. Delivery remains pending until its adapter verifies storage/transport.
 INSERT INTO app.controller_grant(id,binding_id,binding_epoch,match_seat_id,grant_secret_ref,state,issue_receipt_id)
 VALUES(new_grant,binding.id,binding.binding_epoch+1,seat.id,'controller-secret:'||new_grant::text,'active',receipt.id);
 PERFORM app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'grantId',new_grant,'epoch',(binding.binding_epoch+1)::text,'delivery','pending'));
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION app.guard_ai_process_generation(),app.ai_stop_confirmed(uuid,uuid),app.ai_start_dependency_ready(uuid,uuid),app.read_ai_start_work(uuid,numeric),app.read_ai_stop_work(uuid),app.rebind_ai_runner(uuid,numeric,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
