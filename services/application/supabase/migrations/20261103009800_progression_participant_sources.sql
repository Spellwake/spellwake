BEGIN;

-- Both child creation and parent reassignment participate in the same match fence.
-- These relational checks do not interpret native payloads or establish winner identities.
CREATE FUNCTION app.check_match_progression(p_matches uuid[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='progression source changes require read committed retry'; END IF;
 PERFORM id FROM app.match WHERE id=ANY(p_matches) ORDER BY id FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM app.match_result r JOIN app.match m ON m.id=r.match_id
  WHERE m.id=ANY(p_matches) AND
   ((m.competitive AND (m.mode<>'collection' OR r.ranking_format_id IS DISTINCT FROM m.format_id OR r.rank_sequence IS NULL))
    OR (NOT m.competitive AND r.ranking_format_id IS NOT NULL))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ranked result must bind its competitive match format'; END IF;
 IF EXISTS(SELECT 1 FROM app.game_activity_fact f JOIN app.game g ON g.id=f.game_id
  WHERE g.match_id=ANY(p_matches) AND NOT EXISTS(SELECT 1 FROM app.match_seat s
   WHERE s.match_id=g.match_id AND s.account_id=f.account_id AND s.controller_kind='human')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='activity source must bind a human participant'; END IF;
 IF EXISTS(SELECT 1 FROM app.competitive_result c JOIN app.match_result r ON r.id=c.match_result_id
  JOIN app.match m ON m.id=r.match_id WHERE m.id=ANY(p_matches) AND
   (NOT m.competitive OR m.mode<>'collection' OR (c.win AND r.outcome IN ('draw','no_contest'))
    OR NOT EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND s.account_id=c.account_id AND s.controller_kind='human'))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='competitive credit must bind an eligible human result'; END IF;
END $$;

CREATE FUNCTION app.guard_progression_participants() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE before_match uuid; after_match uuid;
BEGIN
 IF TG_TABLE_NAME='match' THEN
  IF TG_OP='UPDATE' AND (NEW.mode,NEW.competitive,NEW.format_id) IS NOT DISTINCT FROM (OLD.mode,OLD.competitive,OLD.format_id) THEN RETURN NULL; END IF;
  IF TG_OP<>'INSERT' THEN before_match:=OLD.id; END IF;
  IF TG_OP<>'DELETE' THEN after_match:=NEW.id; END IF;
 ELSIF TG_TABLE_NAME='match_seat' THEN
  IF TG_OP='UPDATE' AND (NEW.match_id,NEW.account_id,NEW.controller_kind) IS NOT DISTINCT FROM (OLD.match_id,OLD.account_id,OLD.controller_kind) THEN RETURN NULL; END IF;
  IF TG_OP<>'INSERT' THEN before_match:=OLD.match_id; END IF;
  IF TG_OP<>'DELETE' THEN after_match:=NEW.match_id; END IF;
 ELSIF TG_TABLE_NAME='game' THEN
  IF TG_OP='UPDATE' AND NEW.match_id IS NOT DISTINCT FROM OLD.match_id THEN RETURN NULL; END IF;
  IF TG_OP<>'INSERT' THEN before_match:=OLD.match_id; END IF;
  IF TG_OP<>'DELETE' THEN after_match:=NEW.match_id; END IF;
 ELSIF TG_TABLE_NAME='match_result' THEN after_match:=NEW.match_id;
 ELSIF TG_TABLE_NAME='game_activity_fact' THEN
  SELECT match_id INTO after_match FROM app.game WHERE id=NEW.game_id FOR SHARE;
 ELSIF TG_TABLE_NAME='competitive_result' THEN
  SELECT match_id INTO after_match FROM app.match_result WHERE id=NEW.match_result_id;
 END IF;
 PERFORM app.check_match_progression(array_remove(ARRAY[before_match,after_match],NULL));
 RETURN NULL;
END $$;

CREATE CONSTRAINT TRIGGER progression_match_source AFTER INSERT OR UPDATE OR DELETE ON app.match
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_progression_participants();
CREATE CONSTRAINT TRIGGER progression_seat_source AFTER INSERT OR UPDATE OR DELETE ON app.match_seat
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_progression_participants();
CREATE CONSTRAINT TRIGGER progression_game_source AFTER INSERT OR UPDATE OR DELETE ON app.game
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_progression_participants();
CREATE CONSTRAINT TRIGGER progression_result_source AFTER INSERT ON app.match_result
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_progression_participants();
CREATE CONSTRAINT TRIGGER progression_activity_source AFTER INSERT ON app.game_activity_fact
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_progression_participants();
CREATE CONSTRAINT TRIGGER progression_credit_source AFTER INSERT ON app.competitive_result
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_progression_participants();
CREATE TRIGGER competitive_result_history BEFORE UPDATE OR DELETE ON app.competitive_result
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

REVOKE ALL ON FUNCTION app.check_match_progression(uuid[]),app.guard_progression_participants()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
