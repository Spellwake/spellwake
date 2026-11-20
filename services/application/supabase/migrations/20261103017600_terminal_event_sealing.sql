BEGIN;
-- Result acceptance, not delayed archive publication, closes the native stream.
CREATE OR REPLACE FUNCTION app.guard_activity_stream_insert() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE event_sequence numeric; expected_sequence numeric; discovered_match uuid; actual_match uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='activity stream insertion requires read committed retry'; END IF;
 SELECT match_id INTO discovered_match FROM app.game WHERE id=NEW.game_id;
 IF discovered_match IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='activity game source unavailable'; END IF;
 PERFORM id FROM app.match WHERE id=discovered_match FOR NO KEY UPDATE;
 SELECT match_id INTO actual_match FROM app.game WHERE id=NEW.game_id FOR NO KEY UPDATE;
 IF actual_match IS DISTINCT FROM discovered_match THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='activity game parent changed before fence'; END IF;
 IF TG_TABLE_NAME='game_event' THEN
 IF EXISTS(SELECT 1 FROM app.game_result WHERE game_id=NEW.game_id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal native result seals the complete event frontier';END IF;
 SELECT coalesce(max(sequence),-1)+1 INTO expected_sequence FROM app.game_event WHERE game_id=NEW.game_id;
 IF NEW.sequence<>expected_sequence THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted game events must append without holes or backfill'; END IF;
 event_sequence:=NEW.sequence;
 ELSE
 SELECT sequence INTO event_sequence FROM app.game_event WHERE id=NEW.source_event_id FOR SHARE;
 IF event_sequence IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='activity source event unavailable'; END IF;
 END IF;
 IF EXISTS(SELECT 1 FROM app.activity_consumer_cursor WHERE game_id=NEW.game_id AND initialized AND last_sequence>=event_sequence) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='activity facts and events cannot modify a consumed source prefix'; END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION app.guard_terminal_event_boundary() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid; actual_parent uuid; terminal_sequence numeric; last_sequence numeric;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='terminal event boundary requires read committed retry';END IF;
 SELECT match_id INTO parent FROM app.game WHERE id=NEW.game_id;
 PERFORM id FROM app.match WHERE id=parent FOR SHARE;
 SELECT match_id INTO actual_parent FROM app.game WHERE id=NEW.game_id FOR SHARE;
 IF actual_parent IS DISTINCT FROM parent THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='terminal event parent changed before fence';END IF;
 SELECT sequence INTO terminal_sequence FROM app.game_event WHERE id=NEW.source_event_id AND game_id=NEW.game_id;
 SELECT max(sequence) INTO last_sequence FROM app.game_event WHERE game_id=NEW.game_id;
 IF terminal_sequence IS DISTINCT FROM last_sequence THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal source must seal the last accepted native event';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER terminal_event_boundary AFTER INSERT OR UPDATE ON app.game_result DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_terminal_event_boundary();
REVOKE ALL ON FUNCTION app.guard_terminal_event_boundary() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
