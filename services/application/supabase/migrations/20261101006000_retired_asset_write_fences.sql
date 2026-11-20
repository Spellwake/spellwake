BEGIN;

-- Preserve existing owner RLS reads and source-specific definer APIs. Row guards
-- cannot protect TRUNCATE or a newly installed trigger, so close all nonreads.
REVOKE ALL ON app.asset_owner,app.inventory_asset,app.ledger_operation,app.ledger_posting,app.asset_hold,app.hold_line
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT SELECT ON app.asset_owner,app.inventory_asset,app.asset_hold,app.hold_line TO authenticated;
GRANT SELECT ON app.asset_owner,app.inventory_asset,app.ledger_operation,app.ledger_posting,app.asset_hold,app.hold_line TO service_role;
ALTER TABLE app.asset_owner ADD CONSTRAINT asset_owner_lifecycle CHECK(state IN ('active','retired'));

CREATE FUNCTION app.lock_asset_account_for_write(p_owner uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid;
BEGIN
 SELECT account_id INTO actor FROM app.asset_owner WHERE id=p_owner;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='asset owner unavailable'; END IF;
 PERFORM id FROM app.account WHERE id=actor FOR NO KEY UPDATE;
 IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM app.account a JOIN app.asset_owner o ON o.account_id=a.id
  WHERE a.id=actor AND a.state IN ('active','deletion_pending') AND o.id=p_owner AND o.state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='asset owner lifecycle does not permit writes';
 END IF;
 RETURN actor;
END $$;

CREATE FUNCTION app.guard_asset_owner_lifecycle() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE lifecycle text; operation uuid;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='asset owner history is immutable'; END IF;
 IF TG_OP='UPDATE' AND (NEW.id,NEW.account_id) IS DISTINCT FROM (OLD.id,OLD.account_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='asset owner identity is immutable';
 END IF;
 -- Retirement intentionally has its own deleted-account branch. The general
 -- write fence rejects deleted accounts and must not authorize this transition.
 SELECT state,retirement_operation_id INTO lifecycle,operation FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='asset account unavailable'; END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.state IS DISTINCT FROM 'active' OR lifecycle<>'active' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new asset owner requires an active account';
  END IF;
 ELSE
  IF OLD.state<>'active' OR NEW.state IS DISTINCT FROM 'retired' OR lifecycle<>'deleted' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='asset owner transition unavailable';
  END IF;
  PERFORM app.check_registered_auth_retirement(NEW.account_id,operation);
  IF EXISTS(SELECT 1 FROM app.inventory_asset WHERE owner_id=NEW.id AND owned_quantity<>0)
   OR EXISTS(SELECT 1 FROM app.asset_hold WHERE owner_id=NEW.id AND state='held') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='owned or held assets prevent owner retirement';
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER asset_owner_lifecycle BEFORE INSERT OR UPDATE OR DELETE ON app.asset_owner
 FOR EACH ROW EXECUTE FUNCTION app.guard_asset_owner_lifecycle();

CREATE FUNCTION app.guard_asset_write_lifecycle() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE owner uuid; inventory_owner uuid;
BEGIN
 IF TG_TABLE_NAME='inventory_asset' THEN
  IF TG_OP='UPDATE' AND (NEW.id,NEW.owner_id,NEW.asset_definition_id)
   IS DISTINCT FROM (OLD.id,OLD.owner_id,OLD.asset_definition_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='inventory identity is immutable';
  END IF;
  owner:=NEW.owner_id;
 ELSIF TG_TABLE_NAME='ledger_posting' THEN
  SELECT owner_id INTO owner FROM app.inventory_asset WHERE id=NEW.inventory_asset_id;
 ELSIF TG_TABLE_NAME='asset_hold' THEN
  IF TG_OP='UPDATE' AND (NEW.id,NEW.owner_id,NEW.purpose,NEW.commitment_ref,NEW.operation_id)
   IS DISTINCT FROM (OLD.id,OLD.owner_id,OLD.purpose,OLD.commitment_ref,OLD.operation_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold identity is immutable';
  END IF;
  owner:=NEW.owner_id;
 ELSIF TG_TABLE_NAME='hold_line' THEN
  IF TG_OP='UPDATE' AND (NEW.id,NEW.hold_id,NEW.inventory_asset_id)
   IS DISTINCT FROM (OLD.id,OLD.hold_id,OLD.inventory_asset_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold line identity is immutable';
  END IF;
  SELECT owner_id INTO owner FROM app.asset_hold WHERE id=NEW.hold_id;
  SELECT owner_id INTO inventory_owner FROM app.inventory_asset WHERE id=NEW.inventory_asset_id;
  IF owner IS NULL OR inventory_owner IS NULL OR owner<>inventory_owner THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold and inventory owner must match';
  END IF;
 ELSE
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unsupported asset lifecycle target';
 END IF;
 PERFORM app.lock_asset_account_for_write(owner);
 RETURN NEW;
END $$;

-- These names sort before existing source/availability triggers, including031
-- exchange_hold_lines. UPDATE's own tuple lock still predates BEFORE triggers:
-- supported domain APIs MUST retain their existing account-first prelocks.
CREATE TRIGGER asset_lifecycle_inventory BEFORE INSERT OR UPDATE ON app.inventory_asset
 FOR EACH ROW EXECUTE FUNCTION app.guard_asset_write_lifecycle();
CREATE TRIGGER asset_lifecycle_posting BEFORE INSERT ON app.ledger_posting
 FOR EACH ROW EXECUTE FUNCTION app.guard_asset_write_lifecycle();
CREATE TRIGGER asset_lifecycle_hold BEFORE INSERT OR UPDATE ON app.asset_hold
 FOR EACH ROW EXECUTE FUNCTION app.guard_asset_write_lifecycle();
CREATE TRIGGER asset_lifecycle_hold_line BEFORE INSERT OR UPDATE ON app.hold_line
 FOR EACH ROW EXECUTE FUNCTION app.guard_asset_write_lifecycle();

-- Preserve013 availability/lock modes and007 AFTER INSERT balance application.
CREATE OR REPLACE FUNCTION app.ensure_inventory(p_owner uuid,p_definition uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE result uuid;
BEGIN
 PERFORM app.lock_asset_account_for_write(p_owner);
 INSERT INTO app.inventory_asset(owner_id,asset_definition_id,owned_quantity,revision)
 VALUES(p_owner,p_definition,0,0) ON CONFLICT(owner_id,asset_definition_id) DO NOTHING;
 SELECT id INTO STRICT result FROM app.inventory_asset WHERE owner_id=p_owner AND asset_definition_id=p_definition;
 RETURN result;
END $$;
REVOKE EXECUTE ON FUNCTION app.lock_asset_account_for_write(uuid),app.guard_asset_owner_lifecycle(),
 app.guard_asset_write_lifecycle(),app.ensure_inventory(uuid,uuid),
 app.prepare_exchange_inventory(uuid,uuid,uuid[]),app.apply_exchange(uuid,uuid,jsonb,jsonb,uuid,text),
 app.lock_prediction_market(uuid,uuid),app.refresh_prediction_hold(uuid),app.cancel_prediction_unfilled(uuid,text),
 app.apply_ledger_posting(),app.check_hold_line(),app.check_hold_transition()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
