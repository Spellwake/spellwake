BEGIN;

ALTER TABLE app.market_listing ADD COLUMN assessment jsonb;
ALTER TABLE app.market_listing ADD CHECK ((state='sold' AND assessment IS NOT NULL AND paid_coins=total_price_coins AND buyer_id<>seller_id)
 OR (state<>'sold' AND assessment IS NULL));
ALTER TABLE app.market_listing ADD CHECK (revision=1);
CREATE INDEX market_active_asset ON app.market_listing(asset_definition_id,id) WHERE state='listed';
CREATE INDEX market_seller ON app.market_listing(seller_id,id);
CREATE INDEX market_buyer ON app.market_listing(buyer_id,id) WHERE buyer_id IS NOT NULL;

CREATE FUNCTION app.guard_market_listing() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='market history cannot be deleted'; END IF;
 IF TG_OP='UPDATE' AND (OLD.state<>'listed' OR
 (NEW.id,NEW.seller_id,NEW.asset_definition_id,NEW.hold_id,NEW.quantity,NEW.total_price_coins,NEW.revision)
 IS DISTINCT FROM (OLD.id,OLD.seller_id,OLD.asset_definition_id,OLD.hold_id,OLD.quantity,OLD.total_price_coins,OLD.revision)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='market terms and terminal facts cannot change';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.asset_definition WHERE id=NEW.asset_definition_id AND kind IN ('card','pack')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='market lot must be one card printing or pack product';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER market_history BEFORE INSERT OR UPDATE OR DELETE ON app.market_listing
 FOR EACH ROW EXECUTE FUNCTION app.guard_market_listing();

CREATE FUNCTION app.validate_market_commitment(p_listing uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE listing app.market_listing; hold app.asset_hold;
BEGIN
 SELECT * INTO STRICT listing FROM app.market_listing WHERE id=p_listing;
 SELECT * INTO STRICT hold FROM app.asset_hold WHERE id=listing.hold_id;
 IF hold.purpose<>'listing' OR hold.commitment_ref<>listing.id::text
 OR NOT EXISTS(SELECT 1 FROM app.asset_owner WHERE id=hold.owner_id AND account_id=listing.seller_id)
 OR hold.state<>(CASE listing.state WHEN 'listed' THEN 'held' WHEN 'sold' THEN 'consumed' ELSE 'released' END)
 OR NOT EXISTS(SELECT 1 FROM app.ledger_operation WHERE id=hold.operation_id AND kind='market_listing' AND source_scope='market_listing'
 AND source_ref=listing.id::text AND actor_account_id=listing.seller_id AND state='committed')
 OR (SELECT count(*) FROM app.hold_line WHERE hold_id=hold.id)<>1
 OR NOT EXISTS(SELECT 1 FROM app.hold_line l JOIN app.inventory_asset i ON i.id=l.inventory_asset_id WHERE l.hold_id=hold.id
 AND i.asset_definition_id=listing.asset_definition_id AND l.quantity=listing.quantity) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='market hold must match its exact source lot';
 END IF;
 IF listing.state='sold' AND NOT EXISTS(SELECT 1 FROM app.ledger_operation WHERE id=listing.settlement_operation_id AND kind='market_purchase'
 AND source_scope='market_purchase' AND source_ref=listing.id::text AND actor_account_id=listing.buyer_id AND state='committed') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='market settlement source is missing';
 END IF;
END $$;
CREATE FUNCTION app.check_market_commitment() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 PERFORM app.validate_market_commitment(NEW.id); RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER market_commitment AFTER INSERT OR UPDATE ON app.market_listing
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_market_commitment();
CREATE FUNCTION app.check_market_hold_commitment() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source_hold uuid; purpose text; listing uuid;
BEGIN
 IF TG_TABLE_NAME='asset_hold' THEN source_hold:=NEW.id; ELSE source_hold:=NEW.hold_id; END IF;
 SELECT h.purpose INTO STRICT purpose FROM app.asset_hold h WHERE h.id=source_hold;
 IF purpose='listing' THEN
  SELECT id INTO listing FROM app.market_listing WHERE hold_id=source_hold;
  IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='listing hold requires its source lot'; END IF;
  PERFORM app.validate_market_commitment(listing);
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER market_hold_commitment AFTER INSERT OR UPDATE ON app.asset_hold
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_market_hold_commitment();
CREATE CONSTRAINT TRIGGER market_hold_line_commitment AFTER INSERT OR UPDATE ON app.hold_line
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_market_hold_commitment();

CREATE OR REPLACE FUNCTION app.guard_exchange_hold_line() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE hold app.asset_hold;
BEGIN
 SELECT * INTO STRICT hold FROM app.asset_hold WHERE id=NEW.hold_id FOR NO KEY UPDATE;
 IF hold.purpose='trade' AND (TG_OP<>'INSERT' OR EXISTS(SELECT 1 FROM app.trade_offer WHERE sender_hold_id=hold.id AND sealed_at IS NOT NULL)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='offered hold contents are immutable';
 END IF;
 IF hold.purpose='listing' AND (TG_OP<>'INSERT' OR EXISTS(SELECT 1 FROM app.market_listing WHERE hold_id=hold.id)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='listed hold contents are immutable';
 END IF;
 RETURN NEW;
END $$;

CREATE FUNCTION app.create_listing(p_actor uuid,p_asset uuid,p_quantity text,p_price text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; owner uuid; inventory uuid; listing uuid:=gen_random_uuid(); operation uuid; hold uuid; bundle jsonb;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'market_create',p_asset::text,jsonb_build_object('quantity',p_quantity,'price',p_price));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 IF p_quantity IS NULL OR p_price IS NULL OR p_quantity !~ '^[1-9][0-9]{0,77}$' OR p_price !~ '^[1-9][0-9]{0,77}$'
 OR NOT EXISTS(SELECT 1 FROM app.asset_definition WHERE id=p_asset AND kind IN ('card','pack')) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('listing','invalid whole-lot listing'));
 END IF;
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=p_actor AND state='active';
 SELECT id INTO inventory FROM app.inventory_asset WHERE owner_id=owner AND asset_definition_id=p_asset FOR NO KEY UPDATE;
 bundle:=jsonb_build_array(jsonb_build_object('assetId',p_asset,'quantity',p_quantity));
 IF inventory IS NULL OR NOT app.bundle_available(owner,bundle) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('assets','not enough available assets'));
 END IF;
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('market_listing',listing::text,'market_listing',p_actor,clock_timestamp(),'pending') RETURNING id INTO operation;
 INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id)
 VALUES(owner,'listing',listing::text,'held',operation) RETURNING id INTO hold;
 INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES(hold,inventory,p_quantity::numeric);
 INSERT INTO app.market_listing(id,seller_id,asset_definition_id,hold_id,quantity,total_price_coins,state,revision)
 VALUES(listing,p_actor,p_asset,hold,p_quantity::numeric,p_price::numeric,'listed',1);
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',listing));
END $$;

CREATE FUNCTION app.cancel_listing(p_actor uuid,p_listing uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; listing app.market_listing;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'market_cancel',p_listing::text,'{}');
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 SELECT * INTO listing FROM app.market_listing WHERE id=p_listing AND seller_id=p_actor FOR NO KEY UPDATE;
 IF NOT FOUND OR listing.state<>'listed' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('listing','listing unavailable'));
 END IF;
 UPDATE app.asset_hold SET state='released' WHERE id=listing.hold_id;
 UPDATE app.market_listing SET state='canceled' WHERE id=p_listing;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_listing));
END $$;

CREATE FUNCTION app.buy_listing(p_actor uuid,p_listing uuid,p_revision text,p_price text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; listing app.market_listing; seller_owner uuid; buyer_owner uuid; coin uuid;
 give jsonb; want jsonb; v_assessment jsonb; seller_restricted boolean; buyer_restricted boolean; operation uuid;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'market_buy',p_listing::text,jsonb_build_object('revision',p_revision,'price',p_price));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT * INTO listing FROM app.market_listing WHERE id=p_listing;
 IF NOT FOUND OR listing.seller_id=p_actor OR p_revision IS NULL OR p_price IS NULL
 OR p_revision !~ '^[1-9][0-9]{0,77}$' OR p_price !~ '^[1-9][0-9]{0,77}$' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('listing','listing unavailable'));
 END IF;
 -- Reuse stable account locks and revision fencing, without any friend/block gate.
 IF NOT app.lock_contact_pair(p_actor,listing.seller_id,true) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','market accounts unavailable'));
 END IF;
 SELECT * INTO STRICT listing FROM app.market_listing WHERE id=p_listing FOR NO KEY UPDATE;
 IF listing.state<>'listed' OR listing.revision::text<>p_revision OR listing.total_price_coins::text<>p_price THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('listing','listing terms or state changed'));
 END IF;
 SELECT id INTO STRICT coin FROM app.asset_definition WHERE currency_key='coin' AND kind='coin';
 give:=jsonb_build_array(jsonb_build_object('assetId',listing.asset_definition_id,'quantity',listing.quantity::text));
 want:=jsonb_build_array(jsonb_build_object('assetId',coin,'quantity',listing.total_price_coins::text));
 v_assessment:=app.assess_exchange(give,want);
 seller_restricted:=app.account_trade_restricted(listing.seller_id); buyer_restricted:=app.account_trade_restricted(p_actor);
 IF (seller_restricted AND NOT (v_assessment->>'outgoingAllowed')::boolean) OR (buyer_restricted AND NOT (v_assessment->>'incomingAllowed')::boolean) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('value','restricted outgoing value exceeds the allowed limit'));
 END IF;
 SELECT id INTO STRICT seller_owner FROM app.asset_owner WHERE account_id=listing.seller_id AND state='active';
 SELECT id INTO STRICT buyer_owner FROM app.asset_owner WHERE account_id=p_actor AND state='active';
 PERFORM app.prepare_exchange_inventory(seller_owner,buyer_owner,ARRAY[listing.asset_definition_id,coin]);
 IF NOT app.bundle_available(seller_owner,give,listing.hold_id) OR NOT app.bundle_available(buyer_owner,want) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('assets','market assets no longer available'));
 END IF;
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('market_purchase',p_listing::text,'market_purchase',p_actor,clock_timestamp(),'pending') RETURNING id INTO operation;
 UPDATE app.asset_hold SET state='consumed' WHERE id=listing.hold_id;
 PERFORM app.apply_exchange(seller_owner,buyer_owner,give,want,operation,p_listing::text);
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 UPDATE app.market_listing SET state='sold',buyer_id=p_actor,settlement_operation_id=operation,paid_coins=total_price_coins,
 completed_at=clock_timestamp(),assessment=v_assessment||jsonb_build_object('senderRestricted',seller_restricted,'recipientRestricted',buyer_restricted) WHERE id=p_listing;
 UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 INSERT INTO app.notification(recipient_id,source_kind,source_ref,body) VALUES(listing.seller_id,'market_sold',p_listing::text,'Marketplace listing sold') ON CONFLICT DO NOTHING;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_listing));
END $$;
GRANT EXECUTE ON FUNCTION app.create_listing(uuid,uuid,text,text,uuid),app.cancel_listing(uuid,uuid,uuid),app.buy_listing(uuid,uuid,text,text,uuid) TO service_role;
REVOKE INSERT,UPDATE,DELETE ON app.market_listing FROM service_role;
GRANT SELECT(id,seller_id,asset_definition_id,quantity,total_price_coins,state,revision) ON app.market_listing TO authenticated;
CREATE POLICY public_or_own_listing_read ON app.market_listing FOR SELECT TO authenticated USING
 (app.current_account_id() IS NOT NULL AND (state='listed' OR app.current_account_id() IN (seller_id,buyer_id)));

COMMIT;
