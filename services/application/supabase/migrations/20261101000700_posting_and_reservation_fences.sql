BEGIN;

-- Schema-local revokes cannot subtract the built-in global PUBLIC function default.
-- Existing Auth/other functions keep their current ACLs; new functions require grants.
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;

-- BEFORE INSERT runs even for ON CONFLICT DO NOTHING. Apply only an actually inserted row.
DROP TRIGGER ledger_posting_apply ON app.ledger_posting;
CREATE TRIGGER ledger_posting_apply AFTER INSERT ON app.ledger_posting
 FOR EACH ROW EXECUTE FUNCTION app.apply_ledger_posting();

-- Row locks alone do not invalidate a REPEATABLE READ snapshot of other hold rows.
-- An actual shared inventory revision change forces stale transactions to serialize/retry.
CREATE FUNCTION app.fence_hold_line_change() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 UPDATE app.inventory_asset SET revision=revision+1 WHERE id=NEW.inventory_asset_id;
 RETURN NEW;
END $$;
CREATE TRIGGER hold_line_revision AFTER INSERT OR UPDATE ON app.hold_line
 FOR EACH ROW EXECUTE FUNCTION app.fence_hold_line_change();

CREATE FUNCTION app.fence_hold_state_change() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE inventory uuid;
BEGIN
 IF OLD.state IS DISTINCT FROM NEW.state THEN
  FOR inventory IN SELECT l.inventory_asset_id FROM app.hold_line l WHERE l.hold_id=NEW.id ORDER BY l.inventory_asset_id LOOP
   UPDATE app.inventory_asset SET revision=revision+1 WHERE id=inventory;
  END LOOP;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER hold_state_revision AFTER UPDATE ON app.asset_hold
 FOR EACH ROW EXECUTE FUNCTION app.fence_hold_state_change();

REVOKE EXECUTE ON FUNCTION app.fence_hold_line_change(),app.fence_hold_state_change() FROM PUBLIC,anon,authenticated;

COMMIT;
