BEGIN;
CREATE FUNCTION app.guard_recovery_lineage() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF OLD.incident_receipt_id IS NOT NULL AND NEW.incident_receipt_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='source-bound recovery lineage cannot be cleared';END IF;
 IF OLD.incident_receipt_id IS NOT NULL AND NEW.incident_receipt_id IS DISTINCT FROM OLD.incident_receipt_id AND (NEW.availability<>'recovering' OR NEW.restoration_receipt_id IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recovery successor requires a new source-bound incident';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER recovery_lineage BEFORE UPDATE ON app.operational_state FOR EACH ROW EXECUTE FUNCTION app.guard_recovery_lineage();
CREATE FUNCTION app.guard_published_game_frontier() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid; actual_parent uuid;
BEGIN
 IF TG_TABLE_NAME='game' THEN
 IF (NEW.id,NEW.match_id,NEW.game_index,NEW.state,NEW.accepted_revision) IS DISTINCT FROM (OLD.id,OLD.match_id,OLD.game_index,OLD.state,OLD.accepted_revision) AND current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='final replay game mutation requires read committed retry';END IF;
 IF (NEW.id,NEW.match_id,NEW.game_index,NEW.state,NEW.accepted_revision) IS DISTINCT FROM (OLD.id,OLD.match_id,OLD.game_index,OLD.state,OLD.accepted_revision)
 AND EXISTS(SELECT 1 FROM app.replay_manifest WHERE game_id=OLD.id AND source_receipt_id IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='published final replay game frontier is immutable';END IF;
 ELSE
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='published event frontier requires read committed retry';END IF;
 SELECT match_id INTO parent FROM app.game WHERE id=NEW.game_id;
 PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 SELECT match_id INTO actual_parent FROM app.game WHERE id=NEW.game_id FOR NO KEY UPDATE;
 IF actual_parent IS DISTINCT FROM parent THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='replay event parent changed before fence';END IF;
 IF EXISTS(SELECT 1 FROM app.replay_manifest WHERE game_id=NEW.game_id AND source_receipt_id IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='cannot append behind a final published replay frontier';END IF;
 END IF;RETURN NEW;
END $$;
CREATE TRIGGER final_replay_game_frontier BEFORE UPDATE ON app.game FOR EACH ROW EXECUTE FUNCTION app.guard_published_game_frontier();
CREATE TRIGGER final_replay_event_frontier BEFORE INSERT ON app.game_event FOR EACH ROW EXECUTE FUNCTION app.guard_published_game_frontier();
CREATE FUNCTION app.check_replay_host_source(p_game uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM app.replay_manifest replay JOIN app.game g ON g.id=replay.game_id JOIN app.match m ON m.id=g.match_id WHERE g.id=p_game AND replay.source_receipt_id IS NULL AND (m.accepted_release_id IS NOT NULL OR EXISTS(SELECT 1 FROM app.operational_state WHERE game_id=g.id))) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hosted or admitted replay requires typed publication source';END IF;
END $$;
CREATE FUNCTION app.guard_replay_host_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE target uuid; parent uuid;
BEGIN
 IF TG_TABLE_NAME='replay_manifest' THEN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='replay source requires read committed retry';END IF;
 SELECT match_id INTO parent FROM app.game WHERE id=NEW.game_id;
 PERFORM id FROM app.match WHERE id=parent FOR NO KEY UPDATE;
 PERFORM id FROM app.game WHERE id=NEW.game_id FOR NO KEY UPDATE;
 PERFORM app.check_replay_host_source(NEW.game_id);
 ELSIF TG_TABLE_NAME='operational_state' THEN PERFORM app.check_replay_host_source(NEW.game_id);
 ELSIF TG_TABLE_NAME='game' THEN PERFORM app.check_replay_host_source(NEW.id);
 ELSE IF NEW.accepted_release_id IS NOT NULL THEN FOR target IN SELECT id FROM app.game WHERE match_id=NEW.id LOOP PERFORM app.check_replay_host_source(target);END LOOP;END IF;
 END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER replay_host_source AFTER INSERT ON app.replay_manifest DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_replay_host_source();
CREATE CONSTRAINT TRIGGER moved_replay_source AFTER UPDATE ON app.game DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_replay_host_source();
CREATE CONSTRAINT TRIGGER hosted_replay_source AFTER INSERT ON app.operational_state DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_replay_host_source();
CREATE CONSTRAINT TRIGGER admitted_replay_source AFTER INSERT OR UPDATE ON app.match DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_replay_host_source();
REVOKE ALL ON FUNCTION app.guard_recovery_lineage(),app.guard_published_game_frontier(),app.check_replay_host_source(uuid),app.guard_replay_host_source() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
