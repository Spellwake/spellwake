BEGIN;

CREATE FUNCTION app.account_trade_restricted(p_account uuid) RETURNS boolean
LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path='' AS $$
 SELECT app.trade_restricted(a.created_at,clock_timestamp(),count(p.id)) FROM app.account a
 LEFT JOIN app.game_participation p ON p.account_id=a.id AND p.real_game WHERE a.id=p_account GROUP BY a.id
$$;

CREATE FUNCTION app.prepare_exchange_inventory(p_left uuid,p_right uuid,p_assets uuid[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE asset uuid;
BEGIN
 IF p_left=p_right THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='exchange owners must differ'; END IF;
 FOR asset IN SELECT DISTINCT unnest(p_assets) ORDER BY 1 LOOP
  PERFORM app.ensure_inventory(p_left,asset); PERFORM app.ensure_inventory(p_right,asset);
 END LOOP;
 PERFORM id FROM app.inventory_asset WHERE owner_id IN (p_left,p_right) AND asset_definition_id=ANY(p_assets)
 ORDER BY id FOR NO KEY UPDATE;
END $$;

CREATE FUNCTION app.bundle_available(p_owner uuid,p_lines jsonb,p_ignore_hold uuid DEFAULT NULL) RETURNS boolean
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE line jsonb; available numeric;
BEGIN
 FOR line IN SELECT value FROM jsonb_array_elements(p_lines) LOOP
  SELECT i.owned_quantity-coalesce((SELECT sum(l.quantity) FROM app.hold_line l JOIN app.asset_hold h ON h.id=l.hold_id
   WHERE l.inventory_asset_id=i.id AND h.state='held' AND h.id IS DISTINCT FROM p_ignore_hold),0)
  INTO available FROM app.inventory_asset i WHERE i.owner_id=p_owner AND i.asset_definition_id=(line->>'assetId')::uuid;
  IF available IS NULL OR available<(line->>'quantity')::numeric THEN RETURN false; END IF;
 END LOOP;
 RETURN true;
END $$;

CREATE FUNCTION app.apply_exchange(p_left uuid,p_right uuid,p_give jsonb,p_request jsonb,p_operation uuid,p_source text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE item record; inventory uuid;
BEGIN
 -- Both outgoing sides are debited before any credit. Incoming assets cannot
 -- finance an unfunded offer, and valid net balances do not overflow transiently.
 FOR item IN SELECT p_left AS owner,(e->>'assetId')::uuid AS asset,(e->>'quantity')::numeric AS quantity FROM jsonb_array_elements(p_give) e
 UNION ALL SELECT p_right,(e->>'assetId')::uuid,(e->>'quantity')::numeric FROM jsonb_array_elements(p_request) e LOOP
  SELECT id INTO STRICT inventory FROM app.inventory_asset WHERE owner_id=item.owner AND asset_definition_id=item.asset;
  INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
  VALUES(p_operation,inventory,'debit',item.quantity,'exchange',p_source);
 END LOOP;
 FOR item IN SELECT p_right AS owner,(e->>'assetId')::uuid AS asset,(e->>'quantity')::numeric AS quantity FROM jsonb_array_elements(p_give) e
 UNION ALL SELECT p_left,(e->>'assetId')::uuid,(e->>'quantity')::numeric FROM jsonb_array_elements(p_request) e LOOP
  SELECT id INTO STRICT inventory FROM app.inventory_asset WHERE owner_id=item.owner AND asset_definition_id=item.asset;
  INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
  VALUES(p_operation,inventory,'credit',item.quantity,'exchange',p_source);
 END LOOP;
END $$;

CREATE FUNCTION app.check_exchange_conservation() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE kind text; state text;
BEGIN
 SELECT o.kind,o.state INTO kind,state FROM app.ledger_operation o WHERE id=NEW.id;
 IF state='committed' AND ((kind='trade_settlement' AND NOT EXISTS(
 SELECT 1 FROM app.trade_offer t JOIN app.ledger_operation o ON o.id=t.settlement_operation_id
 WHERE o.id=NEW.id AND t.state='accepted' AND o.source_scope='trade_settlement' AND o.source_ref=t.id::text AND o.actor_account_id=t.recipient_id))
 OR (kind='market_purchase' AND NOT EXISTS(
 SELECT 1 FROM app.market_listing l JOIN app.ledger_operation o ON o.id=l.settlement_operation_id
 WHERE o.id=NEW.id AND l.state='sold' AND o.source_scope='market_purchase' AND o.source_ref=l.id::text AND o.actor_account_id=l.buyer_id))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='exchange operation requires its accepted domain source';
 END IF;
 IF kind IN ('trade_settlement','market_purchase') AND state='committed' AND (
 NOT EXISTS(SELECT 1 FROM app.ledger_posting WHERE operation_id=NEW.id)
 OR EXISTS(SELECT 1 FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id
 WHERE p.operation_id=NEW.id GROUP BY i.asset_definition_id
 HAVING sum(CASE WHEN p.direction='debit' THEN -p.quantity ELSE p.quantity END)<>0)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='exchange must conserve every asset definition';
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER exchange_conservation AFTER INSERT OR UPDATE ON app.ledger_operation
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_exchange_conservation();

COMMIT;
