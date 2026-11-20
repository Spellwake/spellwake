BEGIN;

-- Deletion-pending accounts can reconcile/cancel an existing association, never
-- start a new one. Retain its identity so deletion cannot bypass reuse checks.
CREATE OR REPLACE FUNCTION app.guard_membership_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership proof cannot be deleted'; END IF;
 IF TG_OP='UPDATE' AND (NEW.id,NEW.account_id,NEW.patreon_member_ref) IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.patreon_member_ref) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership association is immutable';
 END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 PERFORM app.check_provider_source(NEW.source_event_id,NEW.account_id,'patreon');
 IF (TG_OP='INSERT' OR NEW.active) AND NOT EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account cannot activate a membership';
 END IF;
 RETURN NEW;
END $$;
DROP TRIGGER membership_source ON app.patreon_membership;
CREATE TRIGGER membership_source BEFORE INSERT OR UPDATE OR DELETE ON app.patreon_membership
 FOR EACH ROW EXECUTE FUNCTION app.guard_membership_source();

COMMIT;
