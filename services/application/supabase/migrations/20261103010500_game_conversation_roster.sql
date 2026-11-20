BEGIN;
CREATE UNIQUE INDEX conversation_game_match ON app.conversation(match_id) WHERE kind='game';

CREATE FUNCTION app.check_game_conversation_roster(p_matches uuid[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='game conversation changes require read committed retry'; END IF;
 PERFORM id FROM app.match WHERE id=ANY(p_matches) ORDER BY id FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM app.conversation c JOIN app.conversation_member member ON member.conversation_id=c.id
  WHERE c.kind='game' AND c.match_id=ANY(p_matches) AND NOT EXISTS(
   SELECT 1 FROM app.match_seat s WHERE s.match_id=c.match_id AND s.controller_kind='human' AND s.account_id=member.account_id)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='game conversation membership must bind its human match roster'; END IF;
END $$;
CREATE FUNCTION app.guard_game_conversation_roster() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE target uuid;
BEGIN
 IF TG_TABLE_NAME='conversation_member' THEN
  SELECT match_id INTO target FROM app.conversation WHERE id=NEW.conversation_id AND kind='game' FOR SHARE;
  IF target IS NULL THEN RETURN NULL; END IF;
  PERFORM app.check_game_conversation_roster(ARRAY[target]);
 ELSIF TG_TABLE_NAME='conversation' THEN
  IF NEW.kind='game' THEN PERFORM app.check_game_conversation_roster(ARRAY[NEW.match_id]); END IF;
 ELSE
  IF TG_OP='UPDATE' AND (NEW.match_id,NEW.account_id,NEW.controller_kind) IS NOT DISTINCT FROM
   (OLD.match_id,OLD.account_id,OLD.controller_kind) THEN RETURN NULL; END IF;
  IF TG_OP='DELETE' THEN PERFORM app.check_game_conversation_roster(ARRAY[OLD.match_id]);
  ELSE PERFORM app.check_game_conversation_roster(ARRAY[OLD.match_id,NEW.match_id]); END IF;
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER game_conversation_source AFTER INSERT ON app.conversation
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_game_conversation_roster();
CREATE CONSTRAINT TRIGGER game_conversation_member_source AFTER INSERT ON app.conversation_member
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_game_conversation_roster();
CREATE CONSTRAINT TRIGGER game_conversation_seat_source AFTER UPDATE OR DELETE ON app.match_seat
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_game_conversation_roster();

-- A trusted admission/lifecycle transaction calls this for its persisted roster.
-- It does not admit a match or qualify native game inputs.
CREATE FUNCTION app.ensure_game_conversation(p_match uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE result_id uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='game conversation creation requires read committed retry'; END IF;
 PERFORM id FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='game conversation match is missing'; END IF;
 IF NOT EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=p_match AND controller_kind='human') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='game conversation requires a human match participant'; END IF;
 INSERT INTO app.conversation(kind,match_id,created_at) VALUES('game',p_match,clock_timestamp())
 ON CONFLICT (match_id) WHERE kind='game' DO NOTHING;
 SELECT id INTO STRICT result_id FROM app.conversation WHERE kind='game' AND match_id=p_match;
 INSERT INTO app.conversation_member(conversation_id,account_id)
 SELECT result_id,s.account_id FROM app.match_seat s WHERE s.match_id=p_match AND s.controller_kind='human'
 ORDER BY s.account_id ON CONFLICT (conversation_id,account_id) DO NOTHING;
 PERFORM app.check_game_conversation_roster(ARRAY[p_match]);
 RETURN result_id;
END $$;
REVOKE ALL ON FUNCTION app.check_game_conversation_roster(uuid[]),app.guard_game_conversation_roster(),app.ensure_game_conversation(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
