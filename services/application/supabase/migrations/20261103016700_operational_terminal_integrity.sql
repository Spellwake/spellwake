BEGIN;
CREATE FUNCTION app.match_operational_snapshot(p_match uuid) RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT coalesce(jsonb_agg(jsonb_build_object('gameId',op.game_id,'owner',op.owner_instance_ref,'epoch',op.fence_epoch::text,'revision',op.revision::text,'availability',op.availability,'incidentId',op.incident_receipt_id,'restorationId',op.restoration_receipt_id,'terminalId',op.terminal_receipt_id) ORDER BY op.game_id),'[]'::jsonb) FROM app.operational_state op JOIN app.game g ON g.id=op.game_id WHERE g.match_id=p_match
$$;
CREATE FUNCTION app.check_operational_match_terminal(p_match uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
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
 OR source.normalized_source->>'version' IS DISTINCT FROM 'match-unrecoverable-v1' OR source.normalized_source->>'matchId' IS DISTINCT FROM m.id::text OR source.normalized_source->>'incidentId' IS DISTINCT FROM m.incident_receipt_id::text OR length(btrim(source.normalized_source->>'failureRef')) NOT BETWEEN 1 AND 2048
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
 OR EXISTS(SELECT 1 FROM app.ai_runner_binding b JOIN app.match_seat seat ON seat.id=b.match_seat_id WHERE seat.match_id=p_match AND (b.desired_state<>'stopped' OR EXISTS(SELECT 1 FROM app.controller_grant WHERE binding_id=b.id AND state='active')))
 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal native frontier, bank, decision or outstanding authority differs from its immutable disposition';END IF;
END $$;
CREATE FUNCTION app.guard_operational_terminal_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid;
BEGIN
 IF TG_TABLE_NAME='match' THEN parent:=NEW.id;
 ELSIF TG_TABLE_NAME IN ('match_result','between_game_decision','game') THEN IF TG_OP='DELETE' THEN parent:=OLD.match_id;ELSE parent:=NEW.match_id;END IF;
 ELSIF TG_TABLE_NAME='match_clock' THEN SELECT match_id INTO parent FROM app.match_seat WHERE id=NEW.match_seat_id;
 ELSE SELECT match_id INTO parent FROM app.game WHERE id=NEW.game_id;END IF;
 PERFORM app.check_operational_match_terminal(parent);RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER operational_terminal_match AFTER INSERT OR UPDATE ON app.match DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_operational_terminal_source();
CREATE CONSTRAINT TRIGGER operational_terminal_result AFTER INSERT ON app.match_result DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_operational_terminal_source();
CREATE CONSTRAINT TRIGGER operational_terminal_game AFTER INSERT OR UPDATE OR DELETE ON app.game DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_operational_terminal_source();
CREATE CONSTRAINT TRIGGER operational_terminal_clock AFTER INSERT OR UPDATE ON app.match_clock DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_operational_terminal_source();
CREATE CONSTRAINT TRIGGER operational_terminal_decision AFTER INSERT OR UPDATE ON app.between_game_decision DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_operational_terminal_source();
CREATE CONSTRAINT TRIGGER operational_terminal_host AFTER INSERT OR UPDATE ON app.operational_state DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_operational_terminal_source();
CREATE FUNCTION app.guard_terminal_match_history() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN IF OLD.terminal_receipt_id IS NOT NULL AND NEW IS DISTINCT FROM OLD THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='operational terminal match history cannot be changed';END IF;RETURN NEW;END $$;
CREATE TRIGGER operational_terminal_history BEFORE UPDATE ON app.match FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_match_history();
CREATE FUNCTION app.guard_terminal_native_history() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid; actual_parent uuid; affected uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native terminal history mutation requires read committed retry';END IF;
 IF TG_TABLE_NAME='game' THEN
 IF TG_OP='DELETE' THEN parent:=OLD.match_id;ELSE parent:=NEW.match_id;END IF;
 IF TG_OP='UPDATE' AND NEW.match_id IS DISTINCT FROM OLD.match_id AND EXISTS(SELECT 1 FROM app.match WHERE id=OLD.match_id AND terminal_receipt_id IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='cannot reparent operational terminal game';END IF;
 ELSIF TG_TABLE_NAME='between_game_decision' THEN IF TG_OP='DELETE' THEN parent:=OLD.match_id;ELSE parent:=NEW.match_id;END IF;
 ELSIF TG_TABLE_NAME='match_clock' THEN IF TG_OP='DELETE' THEN SELECT match_id INTO parent FROM app.match_seat WHERE id=OLD.match_seat_id;ELSE SELECT match_id INTO parent FROM app.match_seat WHERE id=NEW.match_seat_id;END IF;
 ELSE affected:=NEW.game_id;SELECT match_id INTO parent FROM app.game WHERE id=affected;END IF;
 PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 IF affected IS NOT NULL THEN SELECT match_id INTO actual_parent FROM app.game WHERE id=affected FOR NO KEY UPDATE;
 IF actual_parent IS DISTINCT FROM parent THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='terminal native source parent changed before fence';END IF;END IF;
 IF EXISTS(SELECT 1 FROM app.match WHERE id=parent AND terminal_receipt_id IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='cannot mutate retained native history after operational terminalization';END IF;
 IF TG_OP='DELETE' THEN RETURN OLD;END IF;RETURN NEW;
END $$;
CREATE TRIGGER terminal_native_game BEFORE INSERT OR UPDATE OR DELETE ON app.game FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_native_history();
CREATE TRIGGER terminal_native_context BEFORE INSERT ON app.game_context FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_native_history();
CREATE TRIGGER terminal_native_checkpoint BEFORE INSERT ON app.game_checkpoint FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_native_history();
CREATE TRIGGER terminal_native_event BEFORE INSERT ON app.game_event FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_native_history();
CREATE TRIGGER terminal_native_result BEFORE INSERT ON app.game_result FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_native_history();
CREATE TRIGGER terminal_native_decision BEFORE INSERT OR UPDATE OR DELETE ON app.between_game_decision FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_native_history();
CREATE TRIGGER terminal_native_clock BEFORE INSERT OR UPDATE OR DELETE ON app.match_clock FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_native_history();
CREATE TRIGGER terminal_native_activity BEFORE INSERT ON app.game_activity_fact FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_native_history();
CREATE FUNCTION app.guard_interrupted_participation() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid; actual_parent uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='participation terminal source requires read committed retry';END IF;
 SELECT match_id INTO parent FROM app.game WHERE id=NEW.game_id;PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 SELECT match_id INTO actual_parent FROM app.game WHERE id=NEW.game_id FOR NO KEY UPDATE;
 IF actual_parent IS DISTINCT FROM parent THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='participation terminal parent changed before fence';END IF;
 IF EXISTS(SELECT 1 FROM app.match m JOIN app.command_receipt r ON r.id=m.terminal_receipt_id WHERE m.id=parent AND r.normalized_source->>'interruptedGameId'=NEW.game_id::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='failed restoration cannot synthesize final participation';END IF;RETURN NEW;
END $$;
CREATE TRIGGER interrupted_participation BEFORE INSERT ON app.game_participation FOR EACH ROW EXECUTE FUNCTION app.guard_interrupted_participation();
REVOKE ALL ON FUNCTION app.match_operational_snapshot(uuid),app.check_operational_match_terminal(uuid),app.guard_operational_terminal_source(),app.guard_terminal_match_history(),app.guard_terminal_native_history(),app.guard_interrupted_participation() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
