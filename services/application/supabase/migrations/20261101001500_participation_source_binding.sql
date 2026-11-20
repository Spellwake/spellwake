BEGIN;

ALTER TABLE app.game_participation ADD COLUMN accepted_revision numeric NOT NULL
 CHECK (accepted_revision>=0 AND accepted_revision<power(10::numeric,78) AND scale(accepted_revision)=0);

CREATE FUNCTION app.check_participation_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source_match uuid; source_state text; source_revision numeric;
BEGIN
 SELECT g.match_id,g.state,g.accepted_revision INTO STRICT source_match,source_state,source_revision FROM app.game g WHERE g.id=NEW.game_id FOR SHARE;
 IF source_state NOT IN ('completed','no_contest') OR source_revision<>NEW.accepted_revision
 OR NOT EXISTS(SELECT 1 FROM app.match_seat s WHERE s.id=NEW.match_seat_id AND s.match_id=source_match
 AND s.controller_kind='human' AND s.account_id=NEW.account_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='participation does not bind a completed accepted human game';
 END IF;
 IF NEW.completed_at>clock_timestamp() THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='participation completion cannot be in the future';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER participation_source BEFORE INSERT ON app.game_participation
 FOR EACH ROW EXECUTE FUNCTION app.check_participation_source();
CREATE TRIGGER participation_history BEFORE UPDATE OR DELETE ON app.game_participation
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

COMMIT;
