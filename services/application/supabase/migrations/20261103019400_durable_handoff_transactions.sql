BEGIN;
ALTER TABLE app.durable_handoff ADD COLUMN acknowledgement_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.durable_handoff ADD CONSTRAINT handoff_state CHECK(state IN('pending','acknowledged') AND ((state='acknowledged')=(acknowledgement_receipt_id IS NOT NULL)));
CREATE INDEX handoff_pending_delivery ON app.durable_handoff(destination,id) WHERE state='pending';

CREATE FUNCTION app.handoff_source_manifest(p_handoff uuid) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE h app.durable_handoff;m app.match;source app.command_receipt;timer jsonb;source_id uuid;decision_id uuid;
BEGIN
 SELECT * INTO h FROM app.durable_handoff WHERE id=p_handoff;
 IF h.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='handoff unavailable';END IF;
 SELECT * INTO m FROM app.match WHERE id=h.match_id;
 IF m.room_id IS NULL OR m.started_at IS NULL OR m.accepted_release_id IS NULL OR NOT EXISTS(SELECT 1 FROM app.command_receipt WHERE scope='room_start_v1' AND principal_key='room-start:'||m.room_id::text AND target_ref=m.room_id::text AND state='accepted' AND response_payload->'data'->>'resourceId'=m.id::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff publication requires an admitted started room source';END IF;
 IF h.kind='ready_match' AND h.destination='game_host' AND h.source_ref=m.id::text THEN
 SELECT * INTO source FROM app.command_receipt WHERE scope='room_start_v1' AND principal_key='room-start:'||m.room_id::text AND target_ref=m.room_id::text AND state='accepted' AND response_payload->'data'->>'resourceId'=m.id::text AND response_payload->'data'->>'gameId'=h.game_id::text;
 ELSIF h.kind='reserved_game' AND h.destination='game_host' AND h.source_ref=h.game_id::text THEN
 SELECT * INTO source FROM app.command_receipt WHERE scope='between_game_reply_v1' AND state='accepted' AND response_payload->'data'->>'reservedGameId'=h.game_id::text AND EXISTS(SELECT 1 FROM app.between_game_decision WHERE id::text=target_ref AND match_id=h.match_id);
 ELSIF h.kind='game_outcome' AND h.destination='match_lifecycle' THEN
 SELECT * INTO source FROM app.command_receipt WHERE scope='host_candidate_v1' AND principal_key='host:'||h.game_id::text AND state='accepted' AND normalized_source->>'gameResultId'=h.source_ref AND EXISTS(SELECT 1 FROM app.game_result WHERE id::text=h.source_ref AND game_id=h.game_id);
 ELSIF h.kind='match_outcome' AND h.destination='match_settlement' THEN
 SELECT producer.* INTO source FROM app.command_receipt producer WHERE producer.state='accepted' AND ((producer.scope='game_outcome_v1' AND producer.principal_key='match-lifecycle:'||m.id::text AND producer.request_ref=producer.target_ref AND producer.payload_hash=app.auth_json_hash(jsonb_build_object('resultId',producer.target_ref::uuid)) AND producer.normalized_source->>'version'='game-outcome-consumption-v1' AND producer.response_payload->'data'->>'phase'='terminal' AND producer.normalized_source->>'matchId'=h.match_id::text AND producer.response_payload->'data'->>'terminalResultId'=h.source_ref) OR (producer.scope='match_unrecoverable_v1' AND producer.id=m.terminal_receipt_id AND producer.principal_key='match-recovery:'||m.id::text AND producer.target_ref=m.id::text AND producer.normalized_source->>'matchId'=h.match_id::text AND producer.response_payload->'data'->>'resourceId'=h.match_id::text)) AND EXISTS(SELECT 1 FROM app.match_result r WHERE r.id::text=h.source_ref AND r.match_id=h.match_id AND ((r.source_game_result_id IS NULL AND h.game_id IS NULL AND r.terminal_receipt_id=producer.id) OR (r.source_game_result_id::text=producer.target_ref AND EXISTS(SELECT 1 FROM app.game_result g WHERE g.id=r.source_game_result_id AND g.game_id=h.game_id))));
 ELSIF h.kind='between_deadline' AND h.destination='match_lifecycle' AND h.game_id IS NULL THEN
 IF h.source_ref ~ '^[0-9a-f-]{36}$' THEN
 source_id:=h.source_ref::uuid;SELECT * INTO source FROM app.command_receipt WHERE id=source_id AND scope='between_deadline_schedule_v1' AND state='accepted';timer:=source.normalized_source->'snapshot';
 ELSIF h.source_ref ~ '^[0-9a-f-]{36}:[0-9a-f-]{36}$' THEN
 source_id:=split_part(h.source_ref,':',1)::uuid;decision_id:=split_part(h.source_ref,':',2)::uuid;
 SELECT * INTO source FROM app.command_receipt WHERE id=source_id AND scope='match_restoration_v1' AND state='accepted';
 SELECT value INTO timer FROM jsonb_array_elements(source.normalized_source->'deadlineSnapshots') value WHERE value->>'decisionId'=decision_id::text;
 END IF;
 IF timer IS NULL OR timer->>'matchId' IS DISTINCT FROM h.match_id::text OR timer->>'sourceReceiptId' IS DISTINCT FROM source.id::text OR timer->>'dueAt' IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deadline handoff requires its exact armed snapshot';END IF;
 PERFORM app.check_between_deadline_snapshot(timer);
 END IF;
 IF source.id IS NULL OR source.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff requires its exact accepted producer source';END IF;
 RETURN jsonb_build_object('version','durable-handoff-source-v1','handoffId',h.id,'matchId',h.match_id,'gameId',h.game_id,'sourceRef',h.source_ref,'destination',h.destination,'kind',h.kind,'producerReceiptId',source.id,'timer',timer);
END $$;

CREATE FUNCTION app.guard_handoff_history() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='handoff mutation requires read committed retry';END IF;
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='durable handoff history is retained';END IF;
 IF TG_OP='INSERT' THEN
 IF NEW.state<>'pending' OR NEW.acknowledgement_receipt_id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff starts pending without an acknowledgement';END IF;
 ELSE
 IF (NEW.id,NEW.match_id,NEW.game_id,NEW.source_ref,NEW.destination,NEW.kind) IS DISTINCT FROM (OLD.id,OLD.match_id,OLD.game_id,OLD.source_ref,OLD.destination,OLD.kind) OR OLD.state='acknowledged' AND NEW IS DISTINCT FROM OLD THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff identity and terminal disposition are immutable';END IF;
 END IF;
 PERFORM id FROM app.match WHERE id=NEW.match_id FOR NO KEY UPDATE;
 RETURN NEW;
END $$;
CREATE TRIGGER handoff_history BEFORE INSERT OR UPDATE OR DELETE ON app.durable_handoff FOR EACH ROW EXECUTE FUNCTION app.guard_handoff_history();

CREATE FUNCTION app.begin_handoff_command(p_handoff uuid,p_scope text,p_request uuid,p_input jsonb) RETURNS app.command_receipt LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 IF p_handoff IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_scope IS NULL OR p_scope NOT IN('handoff_prepare_v1','handoff_ack_v1') OR p_input IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid handoff command identity';END IF;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref) VALUES('handoff:'||p_handoff::text,p_scope,p_handoff::text,p_request::text,app.auth_json_hash(p_input),'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='handoff:'||p_handoff::text AND scope=p_scope AND request_ref=p_request::text FOR UPDATE;
 IF receipt.target_ref IS DISTINCT FROM p_handoff::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(p_input) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff request reused with different terms';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='handoff command is pending';END IF;END IF;RETURN receipt;
END $$;
CREATE FUNCTION app.prepare_handoff(p_handoff uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;h app.durable_handoff;parent uuid;input jsonb;manifest jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='handoff preparation requires read committed retry';END IF;
 input:=jsonb_build_object('handoffId',p_handoff);
 receipt:=app.begin_handoff_command(p_handoff,'handoff_prepare_v1',p_handoff,input);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT match_id INTO parent FROM app.durable_handoff WHERE id=p_handoff;
 PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 SELECT * INTO h FROM app.durable_handoff WHERE id=p_handoff FOR NO KEY UPDATE;
 IF h.id IS NULL OR h.match_id IS DISTINCT FROM parent THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='handoff unavailable';END IF;
 manifest:=app.handoff_source_manifest(h.id);
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','handoff-prepare-v1','input',input,'manifest',manifest) WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',h.id,'dispatchKey',h.id,'destination',h.destination,'sourceHash',app.auth_json_hash(manifest)));
END $$;
CREATE FUNCTION app.acknowledge_handoff(p_handoff uuid,p_preparation uuid,p_outcome text,p_proof text,p_request uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;preparation app.command_receipt;h app.durable_handoff;parent uuid;input jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='handoff acknowledgement requires read committed retry';END IF;
 IF p_preparation IS NULL OR p_outcome IS NULL OR p_outcome NOT IN('unknown','published') OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid handoff publication observation';END IF;
 input:=jsonb_build_object('handoffId',p_handoff,'preparationId',p_preparation,'outcome',p_outcome,'proofRef',p_proof);
 receipt:=app.begin_handoff_command(p_handoff,'handoff_ack_v1',p_request,input);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT match_id INTO parent FROM app.durable_handoff WHERE id=p_handoff;
 PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 SELECT * INTO h FROM app.durable_handoff WHERE id=p_handoff FOR NO KEY UPDATE;
 SELECT * INTO preparation FROM app.command_receipt WHERE id=p_preparation;
 IF h.id IS NULL OR h.match_id IS DISTINCT FROM parent OR preparation.state IS DISTINCT FROM 'accepted' OR preparation.scope IS DISTINCT FROM 'handoff_prepare_v1' OR preparation.target_ref IS DISTINCT FROM h.id::text OR preparation.principal_key IS DISTINCT FROM 'handoff:'||h.id::text OR preparation.normalized_source->'manifest' IS DISTINCT FROM app.handoff_source_manifest(h.id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff acknowledgement requires its exact prepared source';END IF;
 IF h.state='acknowledged' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('handoff','already acknowledged'));END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','handoff-ack-v1','input',input,'manifest',preparation.normalized_source->'manifest') WHERE id=receipt.id;
 IF p_outcome='published' THEN UPDATE app.durable_handoff SET state='acknowledged',acknowledgement_receipt_id=receipt.id WHERE id=h.id;END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',h.id,'preparationId',preparation.id,'outcome',p_outcome));
END $$;

CREATE FUNCTION app.check_handoff_receipt(p_receipt uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;h app.durable_handoff;preparation app.command_receipt;input jsonb;manifest jsonb;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_receipt;
 IF receipt.state IS DISTINCT FROM 'accepted' OR receipt.scope NOT IN('handoff_prepare_v1','handoff_ack_v1') THEN RETURN;END IF;
 SELECT * INTO h FROM app.durable_handoff WHERE id::text=receipt.target_ref;
 IF h.id IS NULL OR receipt.principal_key IS DISTINCT FROM 'handoff:'||h.id::text OR receipt.response_payload->'data'->>'resourceId' IS DISTINCT FROM h.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff receipt requires its exact retained target';END IF;
 manifest:=app.handoff_source_manifest(h.id);input:=receipt.normalized_source->'input';
 IF receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(input) OR receipt.normalized_source->'manifest' IS DISTINCT FROM manifest THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff receipt differs from its immutable producer manifest';END IF;
 IF receipt.scope='handoff_prepare_v1' THEN
 IF receipt.request_ref IS DISTINCT FROM h.id::text OR receipt.normalized_source->>'version' IS DISTINCT FROM 'handoff-prepare-v1' OR input IS DISTINCT FROM jsonb_build_object('handoffId',h.id) OR receipt.response_payload->'data'->>'dispatchKey' IS DISTINCT FROM h.id::text OR receipt.response_payload->'data'->>'destination' IS DISTINCT FROM h.destination OR receipt.response_payload->'data'->>'sourceHash' IS DISTINCT FROM app.auth_json_hash(manifest) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff preparation has a different publication identity';END IF;
 ELSE
 SELECT * INTO preparation FROM app.command_receipt WHERE id::text=input->>'preparationId';
 IF receipt.normalized_source->>'version' IS DISTINCT FROM 'handoff-ack-v1' OR input->>'handoffId' IS DISTINCT FROM h.id::text OR input->>'outcome' IS NULL OR input->>'outcome' NOT IN('unknown','published') OR input->>'proofRef' IS NULL OR length(btrim(input->>'proofRef')) NOT BETWEEN 1 AND 2048 OR preparation.state IS DISTINCT FROM 'accepted' OR preparation.scope IS DISTINCT FROM 'handoff_prepare_v1' OR preparation.target_ref IS DISTINCT FROM h.id::text OR preparation.principal_key IS DISTINCT FROM receipt.principal_key OR preparation.normalized_source->'manifest' IS DISTINCT FROM manifest OR receipt.response_payload->'data'->>'preparationId' IS DISTINCT FROM preparation.id::text OR receipt.response_payload->'data'->>'outcome' IS DISTINCT FROM input->>'outcome' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff observation differs from its prepared publication';END IF;
 PERFORM app.check_handoff_receipt(preparation.id);
 IF input->>'outcome'='published' AND (h.state<>'acknowledged' OR h.acknowledgement_receipt_id IS DISTINCT FROM receipt.id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='published handoff observation must own the unique completion';END IF;
 END IF;
END $$;
CREATE FUNCTION app.check_handoff(p_handoff uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE h app.durable_handoff;ack app.command_receipt;
BEGIN
 SELECT * INTO h FROM app.durable_handoff WHERE id=p_handoff;
 IF h.id IS NULL THEN RETURN;END IF;
 IF EXISTS(SELECT 1 FROM app.match WHERE id=h.match_id AND room_id IS NOT NULL AND started_at IS NOT NULL AND accepted_release_id IS NOT NULL) THEN PERFORM app.handoff_source_manifest(h.id);END IF;
 IF h.state='acknowledged' THEN
 SELECT * INTO ack FROM app.command_receipt WHERE id=h.acknowledgement_receipt_id;
 IF ack.state IS DISTINCT FROM 'accepted' OR ack.scope IS DISTINCT FROM 'handoff_ack_v1' OR ack.target_ref IS DISTINCT FROM h.id::text OR ack.normalized_source->'input'->>'outcome' IS DISTINCT FROM 'published' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='acknowledged handoff requires its exact verified publication receipt';END IF;
 PERFORM app.check_handoff_receipt(ack.id);
 END IF;
END $$;
CREATE FUNCTION app.guard_handoff_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_TABLE_NAME='durable_handoff' THEN PERFORM app.check_handoff(NEW.id);ELSE PERFORM app.check_handoff_receipt(NEW.id);END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER handoff_source AFTER INSERT OR UPDATE ON app.durable_handoff DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_handoff_source();
CREATE CONSTRAINT TRIGGER handoff_receipt_source AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_handoff_source();
CREATE FUNCTION app.read_pending_handoffs(p_destination text,p_after uuid,p_limit integer) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE items jsonb;
BEGIN
 IF p_destination IS NULL OR p_destination NOT IN('game_host','match_lifecycle','match_settlement') OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid handoff destination or page size';END IF;
 SELECT coalesce(jsonb_agg(manifest ORDER BY id),'[]'::jsonb) INTO items FROM (SELECT h.id,app.handoff_source_manifest(h.id) manifest FROM app.durable_handoff h JOIN app.match m ON m.id=h.match_id WHERE m.room_id IS NOT NULL AND m.started_at IS NOT NULL AND m.accepted_release_id IS NOT NULL AND EXISTS(SELECT 1 FROM app.command_receipt producer WHERE producer.scope='room_start_v1' AND producer.principal_key='room-start:'||m.room_id::text AND producer.target_ref=m.room_id::text AND producer.state='accepted' AND producer.response_payload->'data'->>'resourceId'=m.id::text) AND h.destination=p_destination AND h.state='pending' AND h.kind IN('ready_match','reserved_game','game_outcome','match_outcome','between_deadline') AND (p_after IS NULL OR h.id>p_after) ORDER BY h.id LIMIT p_limit) page;
 RETURN items;
END $$;
REVOKE ALL ON FUNCTION app.handoff_source_manifest(uuid),app.guard_handoff_history(),app.begin_handoff_command(uuid,text,uuid,jsonb),app.prepare_handoff(uuid),app.acknowledge_handoff(uuid,uuid,text,text,uuid),app.check_handoff_receipt(uuid),app.check_handoff(uuid),app.guard_handoff_source(),app.read_pending_handoffs(text,uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
