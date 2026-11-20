BEGIN;

ALTER TABLE app.trade_offer ADD COLUMN sealed_at timestamptz;
ALTER TABLE app.trade_offer ADD COLUMN assessment jsonb;
ALTER TABLE app.trade_offer ADD CONSTRAINT trade_terminal_group CHECK (
 (state='accepted' AND settlement_operation_id IS NOT NULL AND assessment IS NOT NULL)
 OR (state<>'accepted' AND settlement_operation_id IS NULL AND assessment IS NULL));
ALTER TABLE app.trade_offer ADD CHECK (terms_revision=1);
CREATE UNIQUE INDEX trade_settlement_once ON app.trade_offer(settlement_operation_id);
CREATE INDEX trade_sender_state ON app.trade_offer(sender_id,state,id);
CREATE INDEX trade_recipient_state ON app.trade_offer(recipient_id,state,id);

CREATE FUNCTION app.guard_trade_offer() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='trade history cannot be deleted'; END IF;
 IF (NEW.id,NEW.sender_id,NEW.recipient_id,NEW.sender_hold_id,NEW.terms_revision,NEW.created_at)
 IS DISTINCT FROM (OLD.id,OLD.sender_id,OLD.recipient_id,OLD.sender_hold_id,OLD.terms_revision,OLD.created_at)
 OR OLD.state<>'offered' OR (OLD.sealed_at IS NOT NULL AND NEW.sealed_at IS DISTINCT FROM OLD.sealed_at)
 OR (OLD.sealed_at IS NULL AND NEW.state<>'offered') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='trade terms and terminal disposition are immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER trade_history BEFORE UPDATE OR DELETE ON app.trade_offer FOR EACH ROW EXECUTE FUNCTION app.guard_trade_offer();
CREATE FUNCTION app.guard_trade_line() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE sealed timestamptz;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='trade bundle cannot change'; END IF;
 SELECT sealed_at INTO sealed FROM app.trade_offer WHERE id=NEW.trade_id FOR NO KEY UPDATE;
 IF NOT FOUND OR sealed IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='trade no longer accepts terms'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER trade_line_history BEFORE INSERT OR UPDATE OR DELETE ON app.trade_line FOR EACH ROW EXECUTE FUNCTION app.guard_trade_line();

CREATE FUNCTION app.check_trade_commitment() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE offer app.trade_offer; hold app.asset_hold;
BEGIN
 SELECT * INTO STRICT offer FROM app.trade_offer WHERE id=NEW.id;
 SELECT * INTO STRICT hold FROM app.asset_hold WHERE id=offer.sender_hold_id;
 IF offer.sealed_at IS NULL OR hold.purpose<>'trade' OR hold.commitment_ref<>offer.id::text
 OR NOT EXISTS(SELECT 1 FROM app.asset_owner WHERE id=hold.owner_id AND account_id=offer.sender_id)
 OR hold.state<>(CASE offer.state WHEN 'offered' THEN 'held' WHEN 'accepted' THEN 'consumed' ELSE 'released' END)
 OR NOT EXISTS(SELECT 1 FROM app.trade_line WHERE trade_id=offer.id) THEN
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
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER trade_commitment AFTER INSERT OR UPDATE ON app.trade_offer
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_trade_commitment();

CREATE FUNCTION app.guard_exchange_hold_line() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE hold app.asset_hold;
BEGIN
 SELECT * INTO STRICT hold FROM app.asset_hold WHERE id=NEW.hold_id FOR NO KEY UPDATE;
 IF hold.purpose='trade' AND (TG_OP<>'INSERT' OR EXISTS(SELECT 1 FROM app.trade_offer WHERE sender_hold_id=hold.id AND sealed_at IS NOT NULL)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='offered hold contents are immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER exchange_hold_lines BEFORE INSERT OR UPDATE ON app.hold_line FOR EACH ROW EXECUTE FUNCTION app.guard_exchange_hold_line();

CREATE FUNCTION app.create_trade(p_actor uuid,p_recipient uuid,p_give jsonb,p_want jsonb,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; assets uuid[]; owner uuid; other_owner uuid; offer uuid:=gen_random_uuid(); hold uuid; operation uuid; line jsonb;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'trade_create',p_recipient::text,jsonb_build_object('give',p_give,'request',p_want));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF NOT app.lock_active_contact_accounts(p_actor,p_recipient) OR NOT app.contact_allowed(p_actor,p_recipient) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('contact','trade contact unavailable'));
 END IF;
 BEGIN
  assets:=app.bundle_asset_ids(p_give)||app.bundle_asset_ids(p_want);
 EXCEPTION WHEN invalid_parameter_value OR invalid_text_representation THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('assets','invalid trade bundle'));
 END;
 IF cardinality(assets)=0 OR EXISTS(SELECT 1 FROM unnest(assets) a WHERE NOT EXISTS(SELECT 1 FROM app.asset_definition d WHERE d.id=a)) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('assets','trade assets unavailable'));
 END IF;
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=p_actor AND state='active';
 SELECT id INTO STRICT other_owner FROM app.asset_owner WHERE account_id=p_recipient AND state='active';
 PERFORM app.prepare_exchange_inventory(owner,other_owner,assets);
 IF NOT app.bundle_available(owner,p_give) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('assets','not enough available offered assets'));
 END IF;
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('trade_offer',offer::text,'trade_offer',p_actor,clock_timestamp(),'pending') RETURNING id INTO operation;
 INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES(owner,'trade',offer::text,'held',operation) RETURNING id INTO hold;
 INSERT INTO app.trade_offer(id,sender_id,recipient_id,sender_hold_id,terms_revision,state,created_at)
 VALUES(offer,p_actor,p_recipient,hold,1,'offered',clock_timestamp());
 INSERT INTO app.trade_line(trade_id,asset_definition_id,side,quantity)
 SELECT offer,(e->>'assetId')::uuid,'give',(e->>'quantity')::numeric FROM jsonb_array_elements(p_give) e
 UNION ALL SELECT offer,(e->>'assetId')::uuid,'request',(e->>'quantity')::numeric FROM jsonb_array_elements(p_want) e;
 FOR line IN SELECT value FROM jsonb_array_elements(p_give) LOOP
  INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity)
  SELECT hold,id,(line->>'quantity')::numeric FROM app.inventory_asset WHERE owner_id=owner AND asset_definition_id=(line->>'assetId')::uuid;
 END LOOP;
 UPDATE app.trade_offer SET sealed_at=clock_timestamp() WHERE id=offer;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 INSERT INTO app.notification(recipient_id,source_kind,source_ref,body) VALUES(p_recipient,'trade_offer',offer::text,'Trade offer received') ON CONFLICT DO NOTHING;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',offer));
END $$;

CREATE FUNCTION app.respond_trade(p_actor uuid,p_trade uuid,p_action text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; offer app.trade_offer; peer uuid; left_owner uuid; right_owner uuid;
 give jsonb; want jsonb; v_assessment jsonb; sender_restricted boolean; recipient_restricted boolean; inverse_allowed boolean; operation uuid;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'trade_response',p_trade::text,jsonb_build_object('action',p_action));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT * INTO offer FROM app.trade_offer WHERE id=p_trade AND p_actor IN (sender_id,recipient_id);
 IF NOT FOUND OR p_action IS NULL OR p_action NOT IN ('accept','reject','cancel')
 OR (p_action='cancel' AND offer.sender_id<>p_actor) OR (p_action IN ('accept','reject') AND offer.recipient_id<>p_actor) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('trade','trade action unavailable'));
 END IF;
 peer:=CASE WHEN offer.sender_id=p_actor THEN offer.recipient_id ELSE offer.sender_id END;
 IF NOT app.lock_contact_pair(p_actor,peer,p_action='accept') OR (p_action='accept' AND app.contact_blocked(p_actor,peer)) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('contact','trade contact unavailable'));
 END IF;
 SELECT * INTO STRICT offer FROM app.trade_offer WHERE id=p_trade FOR NO KEY UPDATE;
 IF offer.state<>'offered' OR offer.sealed_at IS NULL THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('trade','trade is already terminal'));
 END IF;
 IF p_action<>'accept' THEN
  UPDATE app.asset_hold SET state='released' WHERE id=offer.sender_hold_id;
  UPDATE app.trade_offer SET state=CASE WHEN p_action='cancel' THEN 'canceled' ELSE 'rejected' END WHERE id=p_trade;
  RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_trade));
 END IF;
 SELECT coalesce(jsonb_agg(jsonb_build_object('assetId',asset_definition_id,'quantity',quantity::text) ORDER BY asset_definition_id),'[]') INTO give FROM app.trade_line WHERE trade_id=p_trade AND side='give';
 SELECT coalesce(jsonb_agg(jsonb_build_object('assetId',asset_definition_id,'quantity',quantity::text) ORDER BY asset_definition_id),'[]') INTO want FROM app.trade_line WHERE trade_id=p_trade AND side='request';
 v_assessment:=app.assess_exchange(give,want);
 sender_restricted:=app.account_trade_restricted(offer.sender_id); recipient_restricted:=app.account_trade_restricted(offer.recipient_id);
 inverse_allowed:=10*(v_assessment->'incoming'->'total'->>'numerator')::numeric*(v_assessment->'outgoing'->'total'->>'denominator')::numeric
 <=13*(v_assessment->'outgoing'->'total'->>'numerator')::numeric*(v_assessment->'incoming'->'total'->>'denominator')::numeric;
 IF (sender_restricted AND NOT (v_assessment->>'outgoingAllowed')::boolean) OR (recipient_restricted AND NOT inverse_allowed) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('value','restricted outgoing value exceeds the allowed limit'));
 END IF;
 SELECT id INTO STRICT left_owner FROM app.asset_owner WHERE account_id=offer.sender_id AND state='active';
 SELECT id INTO STRICT right_owner FROM app.asset_owner WHERE account_id=offer.recipient_id AND state='active';
 PERFORM app.prepare_exchange_inventory(left_owner,right_owner,app.bundle_asset_ids(give)||app.bundle_asset_ids(want));
 IF NOT app.bundle_available(left_owner,give,offer.sender_hold_id) OR NOT app.bundle_available(right_owner,want) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('assets','trade assets no longer available'));
 END IF;
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('trade_settlement',p_trade::text,'trade_settlement',p_actor,clock_timestamp(),'pending') RETURNING id INTO operation;
 UPDATE app.asset_hold SET state='consumed' WHERE id=offer.sender_hold_id;
 PERFORM app.apply_exchange(left_owner,right_owner,give,want,operation,p_trade::text);
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 UPDATE app.trade_offer SET state='accepted',settlement_operation_id=operation,
 assessment=v_assessment||jsonb_build_object('senderRestricted',sender_restricted,'recipientRestricted',recipient_restricted) WHERE id=p_trade;
 UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 INSERT INTO app.notification(recipient_id,source_kind,source_ref,body) VALUES(offer.sender_id,'trade_accepted',p_trade::text,'Trade accepted') ON CONFLICT DO NOTHING;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_trade));
END $$;
GRANT EXECUTE ON FUNCTION app.create_trade(uuid,uuid,jsonb,jsonb,uuid),app.respond_trade(uuid,uuid,text,uuid) TO service_role;
REVOKE INSERT,UPDATE,DELETE ON app.trade_offer,app.trade_line FROM service_role;
GRANT SELECT ON app.trade_offer,app.trade_line TO authenticated;
CREATE POLICY own_trade_read ON app.trade_offer FOR SELECT TO authenticated USING (app.current_account_id() IN (sender_id,recipient_id));
CREATE POLICY own_trade_line_read ON app.trade_line FOR SELECT TO authenticated USING
 (EXISTS(SELECT 1 FROM app.trade_offer t WHERE t.id=trade_id AND app.current_account_id() IN (t.sender_id,t.recipient_id)));

COMMIT;
