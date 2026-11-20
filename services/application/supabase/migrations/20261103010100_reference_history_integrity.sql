BEGIN;

-- Version rows are content identities. Corrections publish another version, never rewrite one.
CREATE TRIGGER table_profile_history BEFORE UPDATE OR DELETE ON app.table_profile_revision
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();
CREATE TRIGGER qualified_release_history BEFORE UPDATE OR DELETE ON app.qualified_release
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();
CREATE TRIGGER legality_revision_history BEFORE UPDATE OR DELETE ON app.legality_revision
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();
CREATE TRIGGER release_program_history BEFORE UPDATE OR DELETE ON app.release_card_program
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();
CREATE TRIGGER card_legality_history BEFORE UPDATE OR DELETE ON app.card_legality
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

CREATE FUNCTION app.guard_reference_dictionary() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='reference publication requires read committed retry'; END IF;
 IF TG_TABLE_NAME='release_card_program' THEN
  -- Stronger than the context FK's KEY SHARE, including an insertion already in flight.
  PERFORM id FROM app.qualified_release WHERE id=NEW.release_id FOR UPDATE;
  IF EXISTS(SELECT 1 FROM app.game_context WHERE release_id=NEW.release_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted release program set is immutable'; END IF;
 ELSE
  PERFORM id FROM app.legality_revision WHERE id=NEW.legality_revision_id FOR UPDATE;
  IF EXISTS(SELECT 1 FROM app.match WHERE legality_revision_id=NEW.legality_revision_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted legality set is immutable'; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER release_dictionary_freeze BEFORE INSERT ON app.release_card_program
 FOR EACH ROW EXECUTE FUNCTION app.guard_reference_dictionary();
CREATE TRIGGER legality_dictionary_freeze BEFORE INSERT ON app.card_legality
 FOR EACH ROW EXECUTE FUNCTION app.guard_reference_dictionary();
REVOKE ALL ON FUNCTION app.guard_reference_dictionary()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
