BEGIN;
CREATE FUNCTION app.guard_terminal_controller_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE binding_id uuid; parent uuid;
BEGIN
 IF TG_TABLE_NAME='controller_grant' THEN binding_id:=NEW.binding_id;ELSE binding_id:=NEW.id;END IF;
 FOR parent IN SELECT DISTINCT m.id FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id JOIN app.match m ON m.room_id=slot.room_id WHERE b.id=binding_id
 UNION SELECT seat.match_id FROM app.ai_runner_binding b JOIN app.match_seat seat ON seat.id=b.match_seat_id WHERE b.id=binding_id
 LOOP
 PERFORM id FROM app.match WHERE id=parent FOR SHARE;
 PERFORM app.check_operational_match_terminal(parent);END LOOP;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER terminal_runner_source AFTER INSERT OR UPDATE ON app.ai_runner_binding DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_controller_source();
CREATE CONSTRAINT TRIGGER terminal_grant_source AFTER INSERT OR UPDATE ON app.controller_grant DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_controller_source();
CREATE FUNCTION app.guard_stopped_runner_restart() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF OLD.bootstrap_receipt_id IS NOT NULL AND OLD.desired_state='stopped' AND NEW.desired_state='running' AND NEW.binding_epoch<=OLD.binding_epoch THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restarting a stopped typed controller requires a new source epoch';END IF;RETURN NEW;
END $$;
CREATE TRIGGER stopped_runner_restart BEFORE UPDATE ON app.ai_runner_binding FOR EACH ROW EXECUTE FUNCTION app.guard_stopped_runner_restart();
REVOKE ALL ON FUNCTION app.guard_terminal_controller_source(),app.guard_stopped_runner_restart() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE OR REPLACE FUNCTION app.check_operational_match_terminal(p_match uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; source app.command_receipt; incident app.command_receipt; result app.match_result; played uuid[]; canceled uuid[]; terms jsonb;
BEGIN
 SELECT * INTO m FROM app.match WHERE id=p_match;
 IF m.id IS NULL THEN RETURN;END IF;
 SELECT * INTO result FROM app.match_result WHERE match_id=p_match;
 IF m.terminal_receipt_id IS NULL THEN
 IF result.terminal_kind='unrecoverable_incident' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='operational result cannot lose its match terminal lineage';END IF;RETURN;END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='terminal history requires read committed retry';END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=m.terminal_receipt_id;
 SELECT * INTO incident FROM app.command_receipt WHERE id=m.incident_receipt_id;
 terms:=jsonb_build_object('version','match-unrecoverable-v1','matchId',m.id,'basis',source.normalized_source->>'basis','availabilityBasis',source.normalized_source->>'availabilityBasis','incidentId',m.incident_receipt_id,'failureRef',source.normalized_source->>'failureRef');
 IF m.state<>'no_contest' OR m.availability<>'unrecoverable' OR m.current_game_id IS NOT NULL OR m.reserved_game_id IS NOT NULL OR m.restoration_receipt_id IS NOT NULL
 OR source.state IS DISTINCT FROM 'accepted' OR source.scope IS DISTINCT FROM 'match_unrecoverable_v1' OR source.principal_key IS DISTINCT FROM 'match-recovery:'||m.id::text OR source.target_ref IS DISTINCT FROM m.id::text OR source.request_ref IS DISTINCT FROM m.incident_receipt_id::text OR source.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)
 OR source.normalized_source->>'version' IS DISTINCT FROM 'match-unrecoverable-v1' OR source.normalized_source->>'matchId' IS DISTINCT FROM m.id::text OR source.normalized_source->>'incidentId' IS DISTINCT FROM m.incident_receipt_id::text OR source.normalized_source->>'failureRef' IS NULL OR length(btrim(source.normalized_source->>'failureRef')) NOT BETWEEN 1 AND 2048
 OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM m.id::text OR source.response_payload->'data'->>'revision' IS DISTINCT FROM m.revision::text OR source.response_payload->'data'->>'availabilityRevision' IS DISTINCT FROM m.availability_revision::text
 OR incident.state IS DISTINCT FROM 'accepted' OR incident.scope NOT IN ('game_incident_v1','match_incident_v1') OR incident.normalized_source->>'matchId' IS DISTINCT FROM m.id::text
 OR result.id IS NULL OR result.terminal_kind<>'unrecoverable_incident' OR result.terminal_receipt_id IS DISTINCT FROM source.id OR result.outcome<>'no_contest' OR result.winner_seat_ids IS NULL OR cardinality(result.winner_seat_ids)<>0 OR result.source_game_result_id IS NOT NULL OR result.source_ref IS DISTINCT FROM 'incident:'||incident.id::text OR result.winners_ref<>'operational:none'
 OR source.response_payload->'data'->>'terminalResultId' IS DISTINCT FROM result.id::text OR source.normalized_source->>'terminalResultId' IS DISTINCT FROM result.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unrecoverable match lacks exact accepted terminal/incident/result source';END IF;
 IF incident.scope='game_incident_v1' THEN
 IF source.normalized_source->>'interruptedGameId' IS DISTINCT FROM incident.normalized_source->>'currentGameId' OR source.normalized_source->>'neverOpenedGameId' IS DISTINCT FROM incident.normalized_source->>'reservedGameId' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='operational terminal source retargeted paused game frontier';END IF;
 ELSE IF source.normalized_source->>'interruptedGameId' IS NOT NULL OR source.normalized_source->>'neverOpenedGameId' IS DISTINCT FROM incident.normalized_source->>'reservedGameId' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match-only failure cannot invent an interrupted game';END IF;END IF;
 SELECT coalesce(array_agg(g.id ORDER BY g.game_index),'{}'::uuid[]) INTO played FROM app.game g WHERE g.match_id=p_match AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id);
 SELECT coalesce(array_agg(id ORDER BY id),'{}'::uuid[]) INTO canceled FROM app.between_game_decision WHERE match_id=p_match AND state='canceled' AND terminal_receipt_id=source.id;
 IF result.final_game_ids IS DISTINCT FROM played OR source.normalized_source->'finalGameIds' IS DISTINCT FROM to_jsonb(played)
 OR source.normalized_source->'canceledDecisionIds' IS DISTINCT FROM to_jsonb(canceled) OR EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=p_match AND state='pending')
 OR source.normalized_source->'frontiers' IS DISTINCT FROM app.match_retained_frontiers(p_match) OR source.normalized_source->'clocks' IS DISTINCT FROM app.match_clock_snapshot(p_match) OR source.normalized_source->'operational' IS DISTINCT FROM app.match_operational_snapshot(p_match)
 OR EXISTS(SELECT 1 FROM app.game WHERE match_id=p_match AND state NOT IN ('completed','no_contest','canceled'))
 OR EXISTS(SELECT 1 FROM app.game_participation WHERE game_id::text=source.normalized_source->>'interruptedGameId')
 OR EXISTS(SELECT 1 FROM app.ante_commitment WHERE match_id=p_match AND state='offered')
 OR EXISTS(SELECT 1 FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE (slot.room_id=m.room_id OR EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.id=b.match_seat_id AND seat.match_id=p_match)) AND (b.desired_state<>'stopped' OR EXISTS(SELECT 1 FROM app.controller_grant WHERE binding_id=b.id AND state='active')))
 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal native frontier, bank, decision or outstanding authority differs from its immutable disposition';END IF;
END $$;

CREATE OR REPLACE FUNCTION app.issue_ai_grant(p_binding uuid,p_epoch numeric,p_secret_ref text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; lobby app.room; grant_id uuid;
BEGIN
 IF p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_secret_ref IS NULL OR length(btrim(p_secret_ref)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid grant issue envelope';END IF;
 receipt:=app.begin_ai_runtime_command('ai_grant_issue_v1',p_binding,p_request,jsonb_build_object('bindingId',p_binding,'epoch',p_epoch::text,'secretRef',p_secret_ref));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT s.* INTO slot FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE b.id=p_binding;
 PERFORM id FROM app.account WHERE id=slot.sponsor_id AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('sponsor','unavailable'));END IF;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding FOR NO KEY UPDATE;
 IF slot.id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.room_slot current_slot WHERE current_slot.id=slot.id AND current_slot.room_id=slot.room_id AND current_slot.sponsor_id IS NOT DISTINCT FROM slot.sponsor_id AND current_slot.retired_at IS NOT DISTINCT FROM slot.retired_at) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='runtime slot source changed before account/room fence';END IF;
 IF binding.id IS NULL OR binding.bootstrap_receipt_id IS NULL OR binding.binding_epoch<>p_epoch OR binding.desired_state<>'running' OR binding.observed_state NOT IN ('running','ready') OR binding.provider_runner_ref IS NULL OR slot.retired_at IS NOT NULL OR lobby.state='closed'
 OR EXISTS(SELECT 1 FROM app.match WHERE room_id=slot.room_id AND state IN ('completed','no_contest'))
 OR EXISTS(SELECT 1 FROM app.controller_grant WHERE binding_id=binding.id AND binding_epoch=p_epoch AND state='active') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('binding','stale or unavailable'));END IF;
 INSERT INTO app.controller_grant(binding_id,binding_epoch,match_seat_id,grant_secret_ref,state,issue_receipt_id) VALUES(binding.id,p_epoch,binding.match_seat_id,p_secret_ref,'active',receipt.id) RETURNING id INTO grant_id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',grant_id,'delivery','pending'));
END $$;
COMMIT;
