BEGIN;
ALTER TABLE app.operational_state ADD CONSTRAINT recovering_requires_source CHECK(availability<>'recovering' OR incident_receipt_id IS NOT NULL);
CREATE FUNCTION app.check_game_recovery_source(p_game uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE host app.operational_state; source app.command_receipt; restored app.command_receipt; checkpoint app.game_checkpoint; current_game app.game;
BEGIN
 SELECT * INTO host FROM app.operational_state WHERE game_id=p_game;
 IF host.id IS NULL OR host.incident_receipt_id IS NULL THEN RETURN;END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=host.incident_receipt_id;
 SELECT * INTO checkpoint FROM app.game_checkpoint WHERE id=host.recovery_checkpoint_id;
 SELECT * INTO current_game FROM app.game WHERE id=p_game;
 IF source.state IS DISTINCT FROM 'accepted' OR source.scope IS DISTINCT FROM 'game_incident_v1' OR source.principal_key IS DISTINCT FROM 'recovery:'||p_game::text OR source.target_ref IS DISTINCT FROM p_game::text
 OR source.normalized_source->>'version' IS DISTINCT FROM 'game-incident-v1' OR source.normalized_source->>'gameId' IS DISTINCT FROM p_game::text OR source.normalized_source->>'proofRef' IS DISTINCT FROM host.incident_ref
 OR source.normalized_source->>'incidentAt' IS DISTINCT FROM to_char(host.incident_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') OR source.normalized_source->>'checkpointId' IS DISTINCT FROM host.recovery_checkpoint_id::text
 OR checkpoint.game_id IS DISTINCT FROM p_game OR source.normalized_source->>'acceptedRevision' IS DISTINCT FROM checkpoint.accepted_revision::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='operational incident lacks exact accepted checkpoint source';END IF;
 IF host.availability='recovering' THEN
 IF host.restoration_receipt_id IS NOT NULL OR host.fence_epoch::text IS DISTINCT FROM source.response_payload->'data'->>'epoch' OR host.revision::text IS DISTINCT FROM source.response_payload->'data'->>'operationalRevision'
 OR current_game.accepted_revision IS DISTINCT FROM checkpoint.accepted_revision OR host.owner_instance_ref IS DISTINCT FROM source.normalized_source->>'owner'
 OR EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=current_game.match_id AND (NOT c.paused OR c.last_accounted_at<>host.incident_at OR c.clock_revision::text IS DISTINCT FROM source.normalized_source->>'clockRevision'))
 OR app.match_clock_snapshot(current_game.match_id) IS DISTINCT FROM source.normalized_source->'clockValues' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recovering host or paused bank differs from incident disposition';END IF;
 ELSIF host.availability='available' THEN
 SELECT * INTO restored FROM app.command_receipt WHERE id=host.restoration_receipt_id;
 IF restored.state IS DISTINCT FROM 'accepted' OR restored.scope IS DISTINCT FROM 'game_restoration_v1' OR restored.target_ref IS DISTINCT FROM p_game::text OR restored.principal_key IS DISTINCT FROM 'recovery:'||p_game::text
 OR restored.normalized_source->>'incidentId' IS DISTINCT FROM source.id::text OR restored.normalized_source->>'checkpointId' IS DISTINCT FROM checkpoint.id::text
 OR restored.normalized_source->>'acceptedRevision' IS DISTINCT FROM checkpoint.accepted_revision::text OR restored.normalized_source->>'owner' IS DISTINCT FROM host.owner_instance_ref
 OR restored.response_payload->'data'->>'epoch' IS DISTINCT FROM host.fence_epoch::text OR restored.response_payload->'data'->>'operationalRevision' IS DISTINCT FROM host.revision::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restored host lacks exact checkpoint/fence disposition';END IF;
 ELSE RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unknown source-bound recovery state';END IF;
END $$;
CREATE FUNCTION app.guard_game_recovery_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE affected uuid; parent uuid;
BEGIN
 IF TG_TABLE_NAME='operational_state' THEN PERFORM app.check_game_recovery_source(NEW.game_id);
 ELSIF TG_TABLE_NAME='game' THEN PERFORM app.check_game_recovery_source(NEW.id);
 ELSE SELECT match_id INTO parent FROM app.match_seat WHERE id=NEW.match_seat_id;
 FOR affected IN SELECT id FROM app.game WHERE match_id=parent AND state='active' LOOP PERFORM app.check_game_recovery_source(affected);END LOOP;
 END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER operational_recovery_source AFTER INSERT OR UPDATE ON app.operational_state DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_game_recovery_source();
CREATE CONSTRAINT TRIGGER game_recovery_source AFTER UPDATE ON app.game DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_game_recovery_source();
CREATE CONSTRAINT TRIGGER clock_recovery_source AFTER INSERT OR UPDATE ON app.match_clock DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_game_recovery_source();
REVOKE ALL ON FUNCTION app.check_game_recovery_source(uuid),app.guard_game_recovery_source() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
