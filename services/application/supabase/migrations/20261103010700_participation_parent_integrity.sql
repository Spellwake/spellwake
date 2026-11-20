BEGIN;
-- Insertion already checks completed accepted games in015. Stabilize its seat
-- parent too, then preserve that source when either parent is edited later.
CREATE OR REPLACE FUNCTION app.check_participation_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source_match uuid; source_state text; source_revision numeric; seat app.match_seat;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='participation source changes require read committed retry'; END IF;
 SELECT g.match_id,g.state,g.accepted_revision INTO STRICT source_match,source_state,source_revision
 FROM app.game g WHERE g.id=NEW.game_id FOR SHARE;
 SELECT * INTO seat FROM app.match_seat WHERE id=NEW.match_seat_id FOR SHARE;
 IF source_state NOT IN ('completed','no_contest') OR source_revision<>NEW.accepted_revision
 OR seat.id IS NULL OR seat.match_id<>source_match OR seat.controller_kind<>'human' OR seat.account_id IS DISTINCT FROM NEW.account_id THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='participation does not bind a completed accepted human game'; END IF;
 IF NEW.completed_at>clock_timestamp() THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='participation completion cannot be in the future'; END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION app.guard_participation_parents() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE games uuid[];
BEGIN
 IF TG_TABLE_NAME='game' THEN
  IF (NEW.match_id,NEW.state,NEW.accepted_revision) IS NOT DISTINCT FROM (OLD.match_id,OLD.state,OLD.accepted_revision) THEN RETURN NULL; END IF;
  games:=ARRAY[NEW.id];
 ELSE
  IF TG_OP='UPDATE' AND (NEW.match_id,NEW.account_id,NEW.controller_kind) IS NOT DISTINCT FROM
   (OLD.match_id,OLD.account_id,OLD.controller_kind) THEN RETURN NULL; END IF;
  SELECT array_agg(game_id) INTO games FROM app.game_participation WHERE match_seat_id=OLD.id;
 END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='participation parent changes require read committed retry'; END IF;
 PERFORM id FROM app.game WHERE id=ANY(games) ORDER BY id FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM app.game_participation p JOIN app.game g ON g.id=p.game_id
  LEFT JOIN app.match_seat s ON s.id=p.match_seat_id WHERE g.id=ANY(games) AND
   (g.state NOT IN ('completed','no_contest') OR g.accepted_revision<>p.accepted_revision
    OR s.id IS NULL OR s.match_id<>g.match_id OR s.controller_kind<>'human' OR s.account_id IS DISTINCT FROM p.account_id)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted participation source cannot be invalidated'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER participation_game_source AFTER UPDATE ON app.game
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_participation_parents();
CREATE CONSTRAINT TRIGGER participation_seat_source AFTER UPDATE OR DELETE ON app.match_seat
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_participation_parents();
REVOKE ALL ON FUNCTION app.check_participation_source(),app.guard_participation_parents()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
