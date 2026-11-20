BEGIN;

-- Existing owned quantities are changed only by immutable source-linked postings.
CREATE FUNCTION app.guard_ledger_operation() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' OR OLD.state='committed' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='immutable ledger operation';
 END IF;
 IF (NEW.id,NEW.kind,NEW.source_ref,NEW.source_scope,NEW.actor_account_id,NEW.created_at)
  IS DISTINCT FROM (OLD.id,OLD.kind,OLD.source_ref,OLD.source_scope,OLD.actor_account_id,OLD.created_at) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ledger source cannot change';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER ledger_operation_immutable BEFORE UPDATE OR DELETE ON app.ledger_operation
 FOR EACH ROW EXECUTE FUNCTION app.guard_ledger_operation();

CREATE FUNCTION app.reject_history_change() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='immutable accepted history';
END $$;
CREATE TRIGGER ledger_posting_immutable BEFORE UPDATE OR DELETE ON app.ledger_posting
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();
CREATE TRIGGER prediction_pair_immutable BEFORE UPDATE OR DELETE ON app.prediction_pair
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

CREATE FUNCTION app.check_available_balance() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE held numeric;
BEGIN
 IF NEW.owner_id<>OLD.owner_id OR NEW.asset_definition_id<>OLD.asset_definition_id THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='inventory identity cannot change';
 END IF;
 SELECT coalesce(sum(l.quantity),0) INTO held FROM app.hold_line l JOIN app.asset_hold h ON h.id=l.hold_id
 WHERE l.inventory_asset_id=NEW.id AND h.state='held';
 IF NEW.owned_quantity<held THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='assets are held or insufficient';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER inventory_available BEFORE UPDATE ON app.inventory_asset
 FOR EACH ROW EXECUTE FUNCTION app.check_available_balance();

CREATE FUNCTION app.apply_ledger_posting() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE operation_state text;
BEGIN
 SELECT state INTO STRICT operation_state FROM app.ledger_operation WHERE id=NEW.operation_id FOR UPDATE;
 IF operation_state<>'pending' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ledger operation is already committed';
 END IF;
 PERFORM id FROM app.inventory_asset WHERE id=NEW.inventory_asset_id FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='inventory asset does not exist'; END IF;
 UPDATE app.inventory_asset SET
  owned_quantity=owned_quantity+CASE WHEN NEW.direction='credit' THEN NEW.quantity ELSE -NEW.quantity END,
  revision=revision+1 WHERE id=NEW.inventory_asset_id;
 RETURN NEW;
END $$;
CREATE TRIGGER ledger_posting_apply BEFORE INSERT ON app.ledger_posting
 FOR EACH ROW EXECUTE FUNCTION app.apply_ledger_posting();

CREATE FUNCTION app.check_hold_line() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE holder uuid; asset_owner uuid; hold_state text; owned numeric; reserved numeric;
BEGIN
 IF TG_OP='UPDATE' AND (NEW.id,NEW.hold_id,NEW.inventory_asset_id) IS DISTINCT FROM (OLD.id,OLD.hold_id,OLD.inventory_asset_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold identity cannot change';
 END IF;
 SELECT owner_id,state INTO STRICT holder,hold_state FROM app.asset_hold WHERE id=NEW.hold_id FOR UPDATE;
 IF hold_state<>'held' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold is terminal'; END IF;
 SELECT owner_id,owned_quantity INTO STRICT asset_owner,owned FROM app.inventory_asset WHERE id=NEW.inventory_asset_id FOR UPDATE;
 IF holder<>asset_owner THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold owner does not own this asset'; END IF;
 SELECT coalesce(sum(l.quantity),0) INTO reserved FROM app.hold_line l JOIN app.asset_hold h ON h.id=l.hold_id
 WHERE l.inventory_asset_id=NEW.inventory_asset_id AND h.state='held' AND l.id<>NEW.id;
 IF reserved+NEW.quantity>owned THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='assets are held or insufficient'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER hold_line_available BEFORE INSERT OR UPDATE ON app.hold_line
 FOR EACH ROW EXECUTE FUNCTION app.check_hold_line();
CREATE TRIGGER hold_line_history BEFORE DELETE ON app.hold_line
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

CREATE FUNCTION app.check_hold_transition() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF OLD.state<>'held' OR (NEW.id,NEW.owner_id,NEW.purpose,NEW.commitment_ref,NEW.operation_id)
  IS DISTINCT FROM (OLD.id,OLD.owner_id,OLD.purpose,OLD.commitment_ref,OLD.operation_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='hold identity or terminal state cannot change';
 END IF;
 -- Acquire the same inventory locks used by debits and competing reservations.
 PERFORM i.id FROM app.inventory_asset i WHERE EXISTS
  (SELECT 1 FROM app.hold_line l WHERE l.hold_id=OLD.id AND l.inventory_asset_id=i.id)
 ORDER BY i.id FOR UPDATE;
 RETURN NEW;
END $$;
CREATE TRIGGER hold_terminal BEFORE UPDATE ON app.asset_hold
 FOR EACH ROW EXECUTE FUNCTION app.check_hold_transition();
CREATE TRIGGER hold_history BEFORE DELETE ON app.asset_hold
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

CREATE FUNCTION app.ensure_inventory(p_owner uuid,p_definition uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE result uuid;
BEGIN
 INSERT INTO app.inventory_asset(owner_id,asset_definition_id,owned_quantity,revision)
 VALUES(p_owner,p_definition,0,0) ON CONFLICT(owner_id,asset_definition_id) DO NOTHING;
 SELECT id INTO STRICT result FROM app.inventory_asset WHERE owner_id=p_owner AND asset_definition_id=p_definition;
 RETURN result;
END $$;
REVOKE INSERT,UPDATE,DELETE ON app.inventory_asset FROM service_role;
GRANT EXECUTE ON FUNCTION app.ensure_inventory(uuid,uuid) TO service_role;

-- Read authority is derived from the verified JWT subject, never a request account ID.
CREATE FUNCTION app.current_account_id() RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT id FROM app.account WHERE auth_subject=auth.uid() AND state IN ('active','deletion_pending')
$$;
CREATE FUNCTION app.owns_asset_owner(p_owner uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.asset_owner WHERE id=p_owner AND account_id=app.current_account_id())
$$;
GRANT USAGE ON SCHEMA app TO authenticated;
GRANT EXECUTE ON FUNCTION app.current_account_id(),app.owns_asset_owner(uuid) TO authenticated;
GRANT SELECT ON app.asset_owner,app.asset_definition,app.inventory_asset,app.asset_hold,app.hold_line,
 app.coin_balance,app.asset_availability TO authenticated;
CREATE POLICY owner_read ON app.asset_owner FOR SELECT TO authenticated USING (account_id=app.current_account_id());
CREATE POLICY owner_read ON app.inventory_asset FOR SELECT TO authenticated USING (app.owns_asset_owner(owner_id));
CREATE POLICY owner_read ON app.asset_hold FOR SELECT TO authenticated USING (app.owns_asset_owner(owner_id));
CREATE POLICY owner_read ON app.hold_line FOR SELECT TO authenticated USING
 (EXISTS(SELECT 1 FROM app.asset_hold h WHERE h.id=hold_id));
CREATE POLICY definition_read ON app.asset_definition FOR SELECT TO authenticated USING (true);

-- Explicitly close function EXECUTE defaults, including SECURITY DEFINER helpers.
REVOKE EXECUTE ON FUNCTION app.guard_ledger_operation(),app.reject_history_change(),app.check_available_balance(),
 app.apply_ledger_posting(),app.check_hold_line(),app.check_hold_transition(),app.ensure_inventory(uuid,uuid),
 app.current_account_id(),app.owns_asset_owner(uuid) FROM PUBLIC,anon;

COMMIT;
