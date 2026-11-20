BEGIN;

-- Keep source identity/lifecycle writes behind the existing registration and
-- deletion commands. Provider reconciliation remains a separate trusted path.
REVOKE INSERT,UPDATE,DELETE ON app.account FROM service_role;

-- One pair-lock implementation. Pending permission is private and used only by
-- cancel/reject; existing social/create/accept/buy calls retain active actors.
CREATE FUNCTION app.lock_contact_pair(p_actor uuid,p_other uuid,p_active_peer boolean,p_allow_pending_actor boolean) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE account_count bigint;
BEGIN
 IF p_actor IS NULL OR p_other IS NULL OR p_actor=p_other OR p_active_peer IS NULL OR p_allow_pending_actor IS NULL THEN RETURN false; END IF;
 PERFORM id FROM app.account WHERE id IN (p_actor,p_other) ORDER BY id FOR NO KEY UPDATE;
 SELECT count(*) INTO account_count FROM app.account WHERE id IN (p_actor,p_other);
 IF account_count<>2 OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND (state='active' OR (p_allow_pending_actor AND state='deletion_pending')))
 OR (p_active_peer AND NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_other AND state='active')) THEN RETURN false; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('contact:'||least(p_actor,p_other)::text||':'||greatest(p_actor,p_other)::text,0));
 -- Invalidate stale REPEATABLE READ snapshots of friendship/block/participation policy.
 UPDATE app.account SET social_revision=social_revision+1 WHERE id IN (p_actor,p_other);
 RETURN true;
END $$;

CREATE OR REPLACE FUNCTION app.lock_contact_pair(p_actor uuid,p_other uuid,p_active_peer boolean) RETURNS boolean
LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path='' AS $$
 SELECT app.lock_contact_pair(p_actor,p_other,p_active_peer,false)
$$;

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
 IF NOT app.lock_contact_pair(p_actor,peer,p_action='accept',p_action<>'accept') OR (p_action='accept' AND app.contact_blocked(p_actor,peer)) THEN
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

CREATE OR REPLACE FUNCTION app.cancel_listing(p_actor uuid,p_listing uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; listing app.market_listing;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'market_cancel',p_listing::text,'{}');
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending') FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 SELECT * INTO listing FROM app.market_listing WHERE id=p_listing AND seller_id=p_actor FOR NO KEY UPDATE;
 IF NOT FOUND OR listing.state<>'listed' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('listing','listing unavailable'));
 END IF;
 UPDATE app.asset_hold SET state='released' WHERE id=listing.hold_id;
 UPDATE app.market_listing SET state='canceled' WHERE id=p_listing;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_listing));
END $$;


CREATE FUNCTION app.request_account_deletion(p_actor uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; player app.account;
BEGIN
 IF p_actor IS NULL OR p_actor='00000000-0000-0000-0000-000000000000'::uuid OR p_request IS NULL
 OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='deletion request identity required';
 END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'account_delete',p_actor::text,'{}');
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT * INTO STRICT player FROM app.account WHERE id=p_actor FOR NO KEY UPDATE;
 IF player.state='deleted' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 IF player.state='active' THEN
  UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp(),social_revision=social_revision+1 WHERE id=p_actor;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_actor));
END $$;

CREATE INDEX account_deletion_pending ON app.account(id) WHERE state='deletion_pending';
CREATE FUNCTION app.read_deletion_account_page(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid deletion account page bounds';
 END IF;
 SELECT coalesce(jsonb_agg(page.id ORDER BY page.id),'[]') INTO result FROM (
  SELECT id FROM app.account WHERE state='deletion_pending' AND (p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('items',result,'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->>-1 ELSE NULL END);
END $$;

-- Re-read from the beginning after each bounded batch: successfully canceled
-- items leave this workset. Filled positions and payment/match obligations do not.
CREATE FUNCTION app.read_deletion_work(p_actor uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid deletion work bounds';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='deletion_pending') THEN
  RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='deletion account unavailable';
 END IF;
 SELECT coalesce(jsonb_agg(jsonb_build_object('kind',work.kind,'id',work.id) ORDER BY work.kind,work.id),'[]') INTO result
 FROM (SELECT * FROM (
  SELECT 'trade'::text AS kind,id FROM app.trade_offer WHERE state='offered' AND p_actor IN (sender_id,recipient_id)
  UNION ALL SELECT 'listing',id FROM app.market_listing WHERE state='listed' AND seller_id=p_actor
  UNION ALL SELECT 'prediction',id FROM app.prediction_order WHERE state='open' AND account_id=p_actor
 ) eligible ORDER BY kind,id LIMIT p_limit) work;
 RETURN result;
END $$;

-- Call this in its own transaction, without pre-locking the actor. Each original
-- domain writer acquires its full account set in the canonical UUID order.
CREATE FUNCTION app.cancel_deletion_work(p_actor uuid,p_kind text,p_target uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE request uuid; action text;
BEGIN
 IF p_kind IS NULL OR p_kind NOT IN ('trade','listing','prediction') OR p_target IS NULL
 OR p_target='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid deletion cleanup identity';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='deletion_pending') THEN
  RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='deletion account unavailable';
 END IF;
 request:=substr(encode(sha256(convert_to('spellwake-deletion-cleanup-v1:'||p_actor::text||':'||p_kind||':'||p_target::text,'UTF8')),'hex'),1,32)::uuid;
 IF p_kind='trade' THEN
  SELECT CASE WHEN sender_id=p_actor THEN 'cancel' ELSE 'reject' END INTO action FROM app.trade_offer
  WHERE id=p_target AND p_actor IN (sender_id,recipient_id);
  IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='deletion work unavailable'; END IF;
  RETURN app.respond_trade(p_actor,p_target,action,request);
 ELSIF p_kind='listing' THEN
  IF NOT EXISTS(SELECT 1 FROM app.market_listing WHERE id=p_target AND seller_id=p_actor) THEN
   RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='deletion work unavailable';
  END IF;
  RETURN app.cancel_listing(p_actor,p_target,request);
 ELSE
  IF NOT EXISTS(SELECT 1 FROM app.prediction_order WHERE id=p_target AND account_id=p_actor) THEN
   RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='deletion work unavailable';
  END IF;
  RETURN app.cancel_prediction_order(p_actor,p_target,request);
 END IF;
END $$;
GRANT EXECUTE ON FUNCTION app.request_account_deletion(uuid,uuid),app.read_deletion_account_page(uuid,integer),
 app.read_deletion_work(uuid,integer),app.cancel_deletion_work(uuid,text,uuid) TO service_role;

COMMIT;
