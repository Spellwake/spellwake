BEGIN;
ALTER TABLE app.controller_grant ADD CONSTRAINT controller_grant_state CHECK(state IN ('active','revoked'));

CREATE FUNCTION app.guard_controller_history() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='controller history cannot be erased'; END IF;
 IF TG_TABLE_NAME='controller_grant' THEN
  IF TG_OP='INSERT' THEN
   IF NEW.state<>'active' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='controller grant requires original active authority'; END IF;
  ELSIF (NEW.id,NEW.binding_id,NEW.binding_epoch,NEW.match_seat_id,NEW.grant_secret_ref) IS DISTINCT FROM
       (OLD.id,OLD.binding_id,OLD.binding_epoch,OLD.match_seat_id,OLD.grant_secret_ref)
       OR (NEW.state IS DISTINCT FROM OLD.state AND NOT (OLD.state='active' AND NEW.state='revoked')) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='controller grant history cannot be rewritten'; END IF;
 ELSE
  IF (NEW.id,NEW.room_slot_id) IS DISTINCT FROM (OLD.id,OLD.room_slot_id) OR NEW.binding_epoch<OLD.binding_epoch
   OR (OLD.match_seat_id IS NOT NULL AND NEW.match_seat_id IS DISTINCT FROM OLD.match_seat_id)
   OR ((NEW.match_seat_id,NEW.provider_runner_ref) IS DISTINCT FROM (OLD.match_seat_id,OLD.provider_runner_ref) AND NEW.binding_epoch<=OLD.binding_epoch) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='controller binding identity and epoch cannot be rewritten'; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER controller_grant_history BEFORE INSERT OR UPDATE OR DELETE ON app.controller_grant
 FOR EACH ROW EXECUTE FUNCTION app.guard_controller_history();
CREATE TRIGGER runner_binding_history BEFORE UPDATE OR DELETE ON app.ai_runner_binding
 FOR EACH ROW EXECUTE FUNCTION app.guard_controller_history();

CREATE FUNCTION app.check_controller_lineage(p_bindings uuid[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE binding app.ai_runner_binding; slot app.room_slot; seat app.match_seat; source_room uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='controller lineage changes require read committed retry'; END IF;
 FOR binding IN SELECT * FROM app.ai_runner_binding WHERE id=ANY(p_bindings) ORDER BY id FOR NO KEY UPDATE LOOP
  SELECT * INTO slot FROM app.room_slot WHERE id=binding.room_slot_id FOR SHARE;
  IF slot.id IS NULL OR slot.controller_kind<>'ai' OR slot.account_id IS NOT NULL OR slot.sponsor_id IS NULL THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner binding requires a sponsored AI room slot'; END IF;
  IF binding.match_seat_id IS NOT NULL THEN
   SELECT * INTO seat FROM app.match_seat WHERE id=binding.match_seat_id FOR SHARE;
   SELECT room_id INTO source_room FROM app.match WHERE id=seat.match_id FOR SHARE;
   IF seat.id IS NULL OR seat.controller_kind<>'ai' OR seat.account_id IS NOT NULL
    OR seat.sponsor_id IS DISTINCT FROM slot.sponsor_id OR seat.seat_index<>slot.seat_index
    OR source_room IS DISTINCT FROM slot.room_id THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner seat must preserve its room slot and sponsor'; END IF;
  END IF;
  IF EXISTS(SELECT 1 FROM app.controller_grant g WHERE g.binding_id=binding.id AND g.state='active'
   AND (g.binding_epoch<>binding.binding_epoch OR g.match_seat_id IS DISTINCT FROM binding.match_seat_id)) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='active controller grants must match the current binding epoch and seat'; END IF;
 END LOOP;
END $$;
CREATE FUNCTION app.guard_controller_lineage() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE bindings uuid[];
BEGIN
 IF TG_TABLE_NAME='ai_runner_binding' THEN bindings:=ARRAY[NEW.id];
 ELSIF TG_TABLE_NAME='controller_grant' THEN bindings:=ARRAY[NEW.binding_id];
 ELSIF TG_TABLE_NAME='room_slot' THEN
  IF (NEW.room_id,NEW.sponsor_id,NEW.account_id,NEW.seat_index,NEW.controller_kind) IS NOT DISTINCT FROM
    (OLD.room_id,OLD.sponsor_id,OLD.account_id,OLD.seat_index,OLD.controller_kind) THEN RETURN NULL; END IF;
  SELECT array_agg(id) INTO bindings FROM app.ai_runner_binding WHERE room_slot_id=NEW.id;
 ELSIF TG_TABLE_NAME='match_seat' THEN
  IF (NEW.match_id,NEW.sponsor_id,NEW.account_id,NEW.seat_index,NEW.controller_kind) IS NOT DISTINCT FROM
    (OLD.match_id,OLD.sponsor_id,OLD.account_id,OLD.seat_index,OLD.controller_kind) THEN RETURN NULL; END IF;
  SELECT array_agg(id) INTO bindings FROM app.ai_runner_binding WHERE match_seat_id=NEW.id;
 ELSE
  IF NEW.room_id IS NOT DISTINCT FROM OLD.room_id THEN RETURN NULL; END IF;
  SELECT array_agg(b.id) INTO bindings FROM app.ai_runner_binding b JOIN app.match_seat s ON s.id=b.match_seat_id WHERE s.match_id=NEW.id;
 END IF;
 PERFORM app.check_controller_lineage(bindings);
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER runner_binding_lineage AFTER INSERT OR UPDATE ON app.ai_runner_binding
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_controller_lineage();
CREATE CONSTRAINT TRIGGER controller_grant_lineage AFTER INSERT OR UPDATE ON app.controller_grant
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_controller_lineage();
CREATE CONSTRAINT TRIGGER runner_slot_lineage AFTER UPDATE ON app.room_slot
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_controller_lineage();
CREATE CONSTRAINT TRIGGER runner_seat_lineage AFTER UPDATE ON app.match_seat
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_controller_lineage();
CREATE CONSTRAINT TRIGGER runner_match_lineage AFTER UPDATE ON app.match
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_controller_lineage();
REVOKE ALL ON FUNCTION app.guard_controller_history(),app.check_controller_lineage(uuid[]),app.guard_controller_lineage()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
