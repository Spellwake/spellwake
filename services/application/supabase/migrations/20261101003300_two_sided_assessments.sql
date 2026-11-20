BEGIN;

CREATE FUNCTION app.value_outgoing_allowed(p_out app.exact_value,p_in app.exact_value) RETURNS boolean
LANGUAGE sql IMMUTABLE STRICT SET search_path='' AS $$
 SELECT 10*(p_out).numerator*(p_in).denominator<=13*(p_in).numerator*(p_out).denominator
$$;
CREATE OR REPLACE FUNCTION app.assess_exchange(p_outgoing jsonb,p_incoming jsonb) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE outgoing jsonb; incoming jsonb; left_value app.exact_value; right_value app.exact_value;
BEGIN
 PERFORM app.lock_assessment_products(app.bundle_asset_ids(p_outgoing)||app.bundle_asset_ids(p_incoming));
 outgoing:=app.assess_bundle(p_outgoing); incoming:=app.assess_bundle(p_incoming);
 left_value:=app.value_make((outgoing->'total'->>'numerator')::numeric,(outgoing->'total'->>'denominator')::numeric);
 right_value:=app.value_make((incoming->'total'->>'numerator')::numeric,(incoming->'total'->>'denominator')::numeric);
 RETURN jsonb_build_object('outgoing',outgoing,'incoming',incoming,
 'outgoingAllowed',app.value_outgoing_allowed(left_value,right_value),'incomingAllowed',app.value_outgoing_allowed(right_value,left_value));
END $$;

CREATE OR REPLACE FUNCTION app.respond_trade(p_actor uuid,p_trade uuid,p_action text,p_request uuid) RETURNS jsonb
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
 inverse_allowed:=(v_assessment->>'incomingAllowed')::boolean;
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

COMMIT;
