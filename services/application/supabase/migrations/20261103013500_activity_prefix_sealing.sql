BEGIN;
-- All event/fact writers take the game write fence; consumers hold its SHARE
-- fence until committing their cursor. An advanced prefix is therefore sealed:
-- a writer either commits first and is consumed, or observes the committed
-- cursor and rejects. No late fact can silently fall behind a consumer.
CREATE FUNCTION app.guard_activity_stream_insert() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE event_sequence numeric; expected_sequence numeric;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='activity stream insertion requires read committed retry'; END IF;
 PERFORM id FROM app.game WHERE id=NEW.game_id FOR NO KEY UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='activity game source unavailable'; END IF;
 IF TG_TABLE_NAME='game_event' THEN
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
CREATE TRIGGER event_stream_append BEFORE INSERT ON app.game_event FOR EACH ROW EXECUTE FUNCTION app.guard_activity_stream_insert();
CREATE TRIGGER fact_stream_append BEFORE INSERT ON app.game_activity_fact FOR EACH ROW EXECUTE FUNCTION app.guard_activity_stream_insert();
CREATE OR REPLACE FUNCTION app.guard_activity_cursor_history() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='activity cursor source requires read committed retry'; END IF;
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='activity consumption history cannot be deleted'; END IF;
 PERFORM id FROM app.game WHERE id=NEW.game_id FOR SHARE;
 IF TG_OP='INSERT' THEN
 IF NEW.initialized OR NEW.last_sequence<>0 OR NEW.revision<>0 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new activity consumer must start before the first event'; END IF;
 ELSE
 IF (NEW.id,NEW.game_id,NEW.consumer_key) IS DISTINCT FROM (OLD.id,OLD.game_id,OLD.consumer_key) OR NEW.last_sequence<OLD.last_sequence OR (OLD.initialized AND NOT NEW.initialized) OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='activity cursor identity and consumed source frontier cannot rewind'; END IF;
 IF NOT NEW.initialized OR NOT EXISTS(SELECT 1 FROM app.game_event WHERE game_id=NEW.game_id AND sequence=NEW.last_sequence) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='activity frontier must name an accepted event'; END IF;
 END IF;
 RETURN NEW;
END $$;
DROP TRIGGER activity_cursor_history ON app.activity_consumer_cursor;
CREATE TRIGGER activity_cursor_history BEFORE INSERT OR UPDATE OR DELETE ON app.activity_consumer_cursor FOR EACH ROW EXECUTE FUNCTION app.guard_activity_cursor_history();
REVOKE ALL ON FUNCTION app.guard_activity_stream_insert() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
