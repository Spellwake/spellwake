BEGIN;

-- Every hold mutation belongs to its narrow owning function, never raw service
-- DML. Recheck both sides of the domain/hold relationship at transaction commit.
REVOKE INSERT,UPDATE,DELETE ON app.asset_hold,app.hold_line FROM service_role;

CREATE FUNCTION app.validate_trade_commitment(p_trade uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE offer app.trade_offer; hold app.asset_hold;
BEGIN
 SELECT * INTO STRICT offer FROM app.trade_offer WHERE id=p_trade;
 SELECT * INTO STRICT hold FROM app.asset_hold WHERE id=offer.sender_hold_id;
 IF offer.sealed_at IS NULL OR hold.purpose<>'trade' OR hold.commitment_ref<>offer.id::text
 OR NOT EXISTS(SELECT 1 FROM app.asset_owner WHERE id=hold.owner_id AND account_id=offer.sender_id)
 OR hold.state<>(CASE offer.state WHEN 'offered' THEN 'held' WHEN 'accepted' THEN 'consumed' ELSE 'released' END)
 OR NOT EXISTS(SELECT 1 FROM app.trade_line WHERE trade_id=offer.id)
 OR NOT EXISTS(SELECT 1 FROM app.ledger_operation o WHERE o.id=hold.operation_id AND o.kind='trade_offer' AND o.source_scope='trade_offer' AND o.source_ref=offer.id::text AND o.actor_account_id=offer.sender_id AND o.state='committed') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='trade commitment is incomplete or inconsistent';
 END IF;
 IF EXISTS(SELECT 1 FROM
 (SELECT asset_definition_id,quantity FROM app.trade_line WHERE trade_id=offer.id AND side='give') t FULL JOIN
 (SELECT i.asset_definition_id,l.quantity FROM app.hold_line l JOIN app.inventory_asset i ON i.id=l.inventory_asset_id WHERE l.hold_id=hold.id) h
 USING(asset_definition_id) WHERE t.quantity IS DISTINCT FROM h.quantity) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='trade hold must match the exact offered bundle';
 END IF;
 IF offer.state='accepted' AND NOT EXISTS(SELECT 1 FROM app.ledger_operation WHERE id=offer.settlement_operation_id AND kind='trade_settlement'
 AND source_scope='trade_settlement' AND source_ref=offer.id::text AND state='committed') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='trade settlement source is missing';
 END IF;
 RETURN;
END $$;

CREATE OR REPLACE FUNCTION app.check_trade_commitment() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 PERFORM app.validate_trade_commitment(NEW.id);
 RETURN NULL;
END $$;
CREATE FUNCTION app.check_trade_hold_commitment() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE hold_id uuid; purpose text; trade uuid;
BEGIN
 IF TG_TABLE_NAME='asset_hold' THEN hold_id:=NEW.id;
 ELSE hold_id:=NEW.hold_id; END IF;
 SELECT h.purpose INTO STRICT purpose FROM app.asset_hold h WHERE h.id=hold_id;
 IF purpose='trade' THEN
  SELECT id INTO trade FROM app.trade_offer WHERE sender_hold_id=hold_id;
  IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='trade hold requires its source offer'; END IF;
  PERFORM app.validate_trade_commitment(trade);
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER trade_hold_commitment AFTER INSERT OR UPDATE ON app.asset_hold
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_trade_hold_commitment();
CREATE CONSTRAINT TRIGGER trade_hold_line_commitment AFTER INSERT OR UPDATE ON app.hold_line
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_trade_hold_commitment();

COMMIT;
