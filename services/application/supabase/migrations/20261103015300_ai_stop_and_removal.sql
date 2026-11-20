BEGIN;
-- Stop work is durable before I/O. Desired stopped never claims a confirmed provider stop.
CREATE FUNCTION app.request_ai_stop(p_binding uuid,p_cause uuid) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
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
 VALUES('ai-runtime','ai_stop_request_v1',binding.id::text,binding.id::text,app.auth_json_hash(jsonb_build_object('bindingId',binding.id,'epoch',binding.binding_epoch::text,'causeId',p_cause)),'pending','',jsonb_build_object('version','ai-stop-v1','bindingId',binding.id,'epoch',binding.binding_epoch::text,'causeId',p_cause)) RETURNING * INTO receipt;
 UPDATE app.controller_grant SET state='revoked' WHERE binding_id=binding.id AND state='active';
 UPDATE app.ai_runner_binding SET desired_state='stopped',stop_receipt_id=receipt.id WHERE id=binding.id;
 PERFORM app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'disposition','stop requested'));
 RETURN receipt.id;
END $$;
CREATE FUNCTION app.observe_ai_stop(p_binding uuid,p_epoch numeric,p_stop uuid,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot;
BEGIN
 IF p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_stop IS NULL OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid stop observation';END IF;
 receipt:=app.begin_ai_runtime_command('ai_stop_ack_v1',p_binding,p_request,jsonb_build_object('bindingId',p_binding,'epoch',p_epoch::text,'stopId',p_stop,'proofRef',p_proof));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT s.* INTO slot FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE b.id=p_binding;
 PERFORM id FROM app.account WHERE id=slot.sponsor_id FOR NO KEY UPDATE;
 PERFORM id FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding FOR NO KEY UPDATE;
 IF binding.id IS NULL OR binding.binding_epoch<>p_epoch OR binding.desired_state<>'stopped' OR binding.stop_receipt_id IS DISTINCT FROM p_stop THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('source','stale'));END IF;
 UPDATE app.ai_runner_binding SET observed_state='stopped',stop_observation_receipt_id=receipt.id WHERE id=binding.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'disposition','stopped'));
END $$;
CREATE FUNCTION app.remove_ai_slot(p_actor uuid,p_slot uuid,p_expected numeric,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; slot app.room_slot; lobby app.room; original_room uuid; actors uuid[]; binding_id uuid;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_slot IS NULL OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid AI removal envelope';END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI removal requires read committed retry';END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'ai_remove_v1',p_slot::text,jsonb_build_object('basis',p_expected::text));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT * INTO slot FROM app.room_slot WHERE id=p_slot;original_room:=slot.room_id;
 SELECT array_agg(DISTINCT id ORDER BY id) INTO actors FROM(SELECT host_id id FROM app.room WHERE id=original_room UNION SELECT slot.sponsor_id UNION SELECT p_actor) roster;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 PERFORM id FROM app.queue_entry WHERE claimed_room_id=original_room ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=original_room FOR NO KEY UPDATE;
 SELECT * INTO slot FROM app.room_slot WHERE id=p_slot FOR NO KEY UPDATE;
 IF slot.room_id IS DISTINCT FROM original_room OR NOT(slot.sponsor_id=ANY(actors)) OR NOT(lobby.host_id=ANY(actors)) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI removal source changed before fence';END IF;
 IF slot.id IS NULL OR slot.retired_at IS NOT NULL OR slot.controller_kind<>'ai' OR lobby.state NOT IN ('lobby','countdown') OR lobby.revision<>p_expected OR p_actor NOT IN (lobby.host_id,slot.sponsor_id)
 OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending')) OR EXISTS(SELECT 1 FROM app.ai_runner_binding WHERE room_slot_id=p_slot AND match_seat_id IS NOT NULL) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('slot','stale or unavailable'));END IF;
 -- A matched configuration cannot lose one seat; leaving cancels its complete pregame claim.
 IF lobby.queue_claim_receipt_id IS NOT NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','cancel the claimed room before changing its roster'));END IF;
 FOR binding_id IN SELECT id FROM app.ai_runner_binding WHERE room_slot_id=p_slot AND desired_state<>'stopped' ORDER BY id LOOP PERFORM app.request_ai_stop(binding_id,receipt.id);END LOOP;
 UPDATE app.room_slot SET ready=false,retired_at=clock_timestamp() WHERE id=p_slot;
 UPDATE app.room_slot SET ready=false WHERE room_id=lobby.id AND retired_at IS NULL AND controller_kind='human';
 UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=lobby.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_slot));
END $$;
REVOKE ALL ON FUNCTION app.request_ai_stop(uuid,uuid),app.observe_ai_stop(uuid,numeric,uuid,text,uuid),app.remove_ai_slot(uuid,uuid,numeric,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.cancel_matchmaking_claim(p_actor uuid,p_entry uuid,p_request uuid,p_expected numeric,p_room uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; entry app.queue_entry; lobby app.room; claim_room uuid; actors uuid[]; intents uuid[]; binding_id uuid;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid queue cancellation request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='queue cancellation requires read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'queue_cancel',coalesce(p_entry::text,''),jsonb_build_object('expected',p_expected,'room',p_room));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT * INTO entry FROM app.queue_entry WHERE id=p_entry AND account_id=p_actor;
 IF entry.id IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','unavailable')); END IF;
 -- A canceled sibling still identifies the whole consumed intent.
 SELECT claimed_room_id INTO claim_room FROM app.queue_entry WHERE intent_receipt_id=entry.intent_receipt_id AND claimed_room_id IS NOT NULL ORDER BY id LIMIT 1;
 SELECT array_agg(DISTINCT a ORDER BY a) INTO actors FROM(SELECT p_actor a UNION SELECT account_id FROM app.queue_entry WHERE claimed_room_id=claim_room UNION SELECT sponsor_id FROM app.room_slot WHERE room_id=claim_room AND retired_at IS NULL AND sponsor_id IS NOT NULL) ids;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending')) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable'));END IF;
 SELECT array_agg(DISTINCT intent_receipt_id) INTO intents FROM app.queue_entry WHERE claimed_room_id=claim_room;
 PERFORM id FROM app.queue_entry WHERE id=p_entry OR intent_receipt_id=entry.intent_receipt_id OR intent_receipt_id=ANY(intents) ORDER BY id FOR NO KEY UPDATE;
 IF claim_room IS DISTINCT FROM (SELECT claimed_room_id FROM app.queue_entry WHERE intent_receipt_id=entry.intent_receipt_id AND claimed_room_id IS NOT NULL ORDER BY id LIMIT 1) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='queue claim changed before account fence';END IF;
 IF claim_room IS NULL THEN
 IF EXISTS(SELECT 1 FROM app.queue_entry WHERE id=p_entry AND reserved_match_id IS NOT NULL) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','already reserved'));END IF;
 UPDATE app.queue_entry SET state='canceled' WHERE (id=p_entry OR intent_receipt_id=entry.intent_receipt_id) AND state='queued';
 ELSE
 SELECT * INTO STRICT lobby FROM app.room WHERE id=claim_room FOR NO KEY UPDATE;
 IF p_room IS NOT NULL AND (p_room<>claim_room OR p_expected IS NULL OR p_expected<>lobby.revision) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','conflict'));END IF;
 IF lobby.state='started' OR EXISTS(SELECT 1 FROM app.queue_entry WHERE claimed_room_id=claim_room AND reserved_match_id IS NOT NULL) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','already reserved'));END IF;
 UPDATE app.controller_grant g SET state='revoked' FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE g.binding_id=b.id AND s.room_id=claim_room AND g.state='active';
 FOR binding_id IN SELECT b.id FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=claim_room AND b.desired_state<>'stopped' ORDER BY b.id LOOP PERFORM app.request_ai_stop(binding_id,receipt.id);END LOOP;
 UPDATE app.room_slot SET ready=false WHERE room_id=claim_room AND retired_at IS NULL;
 UPDATE app.room SET state='closed',countdown_at=NULL,revision=revision+1 WHERE id=claim_room AND state<>'closed';
 UPDATE app.queue_entry SET state='canceled' WHERE claimed_room_id=claim_room AND state='claimed';
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_entry));
END $$;
COMMIT;
