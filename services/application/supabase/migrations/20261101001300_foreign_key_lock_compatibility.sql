BEGIN;

-- FK checks hold KEY SHARE locks. Balance/hold state changes do not alter referenced keys;
-- NO KEY UPDATE serializes writers without two children deadlocking on a lock upgrade.
CREATE OR REPLACE FUNCTION app.apply_ledger_posting() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE operation_state text;
BEGIN
 SELECT state INTO STRICT operation_state FROM app.ledger_operation WHERE id=NEW.operation_id FOR NO KEY UPDATE;
 IF operation_state<>'pending' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ledger operation is already committed';
 END IF;
 PERFORM id FROM app.inventory_asset WHERE id=NEW.inventory_asset_id FOR NO KEY UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='inventory asset does not exist'; END IF;
 UPDATE app.inventory_asset SET
  owned_quantity=owned_quantity+CASE WHEN NEW.direction='credit' THEN NEW.quantity ELSE -NEW.quantity END,
  revision=revision+1 WHERE id=NEW.inventory_asset_id;
 RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION app.check_hold_line() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE holder uuid; asset_owner uuid; hold_state text; owned numeric; reserved numeric;
BEGIN
 IF TG_OP='UPDATE' AND (NEW.id,NEW.hold_id,NEW.inventory_asset_id) IS DISTINCT FROM (OLD.id,OLD.hold_id,OLD.inventory_asset_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold identity cannot change';
 END IF;
 SELECT owner_id,state INTO STRICT holder,hold_state FROM app.asset_hold WHERE id=NEW.hold_id FOR NO KEY UPDATE;
 IF hold_state<>'held' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold is terminal'; END IF;
 SELECT owner_id,owned_quantity INTO STRICT asset_owner,owned FROM app.inventory_asset WHERE id=NEW.inventory_asset_id FOR NO KEY UPDATE;
 IF holder<>asset_owner THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold owner does not own this asset'; END IF;
 SELECT coalesce(sum(l.quantity),0) INTO reserved FROM app.hold_line l JOIN app.asset_hold h ON h.id=l.hold_id
 WHERE l.inventory_asset_id=NEW.inventory_asset_id AND h.state='held' AND l.id<>NEW.id;
 IF reserved+NEW.quantity>owned THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='assets are held or insufficient'; END IF;
 RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION app.check_hold_transition() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF OLD.state<>'held' OR (NEW.id,NEW.owner_id,NEW.purpose,NEW.commitment_ref,NEW.operation_id)
  IS DISTINCT FROM (OLD.id,OLD.owner_id,OLD.purpose,OLD.commitment_ref,OLD.operation_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold identity or terminal state cannot change';
 END IF;
 -- Acquire the same inventory locks used by debits and competing reservations.
 PERFORM i.id FROM app.inventory_asset i WHERE EXISTS
  (SELECT 1 FROM app.hold_line l WHERE l.hold_id=OLD.id AND l.inventory_asset_id=i.id)
 ORDER BY i.id FOR NO KEY UPDATE;
 RETURN NEW;
END $$;

COMMIT;
