BEGIN;
-- Unqualified historical/reference-only profiles remain valid drafts. Admission
-- requires these source-qualified fields together; no guessed capacity is seeded.
ALTER TABLE app.table_profile_revision ADD COLUMN minimum_seats bigint;
ALTER TABLE app.table_profile_revision ADD COLUMN seat_capacity bigint;
ALTER TABLE app.table_profile_revision ADD COLUMN admission_qualification_ref text;
ALTER TABLE app.table_profile_revision ADD CONSTRAINT qualified_profile_capacity CHECK(
 (minimum_seats IS NULL AND seat_capacity IS NULL AND admission_qualification_ref IS NULL)
 OR (minimum_seats IS NOT NULL AND seat_capacity IS NOT NULL AND admission_qualification_ref IS NOT NULL
  AND minimum_seats>=2 AND seat_capacity>=minimum_seats AND seat_capacity<=6 AND length(btrim(admission_qualification_ref))>0));
-- This is a qualified play permission, never a collectible asset or a rule that
-- suppresses a real pack output. Existing program rows do not gain permission.
ALTER TABLE app.release_card_program ADD COLUMN unlimited_basic_land boolean NOT NULL DEFAULT false;

CREATE FUNCTION app.guard_room_capacity() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE targets uuid[];
BEGIN
 IF TG_TABLE_NAME='room' THEN targets:=ARRAY[NEW.id];
 ELSE
  IF TG_OP='INSERT' THEN targets:=ARRAY[NEW.room_id]; ELSE targets:=ARRAY[OLD.room_id,NEW.room_id]; END IF;
 END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room capacity changes require read committed retry'; END IF;
 PERFORM id FROM app.room WHERE id=ANY(targets) ORDER BY id FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM app.room r JOIN app.table_profile_revision p ON p.id=r.profile_id
  JOIN app.room_slot s ON s.room_id=r.id WHERE r.id=ANY(targets) AND
   (s.seat_index<0 OR (p.seat_capacity IS NOT NULL AND s.seat_index>=p.seat_capacity)
    OR (s.controller_kind='human' AND s.sponsor_id IS NOT NULL)
    OR (s.controller_kind='ai' AND (s.account_id IS NOT NULL OR s.sponsor_id IS NULL))
    OR s.controller_kind NOT IN ('human','ai') OR (r.competitive AND s.controller_kind='ai'))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='room slot exceeds its profile or controller configuration'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER room_capacity_parent AFTER INSERT OR UPDATE ON app.room
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_room_capacity();
CREATE CONSTRAINT TRIGGER room_capacity_slot AFTER INSERT OR UPDATE ON app.room_slot
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_room_capacity();
REVOKE ALL ON FUNCTION app.guard_room_capacity() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
