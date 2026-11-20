-- Caller-bound exchange records; private holds and full valuation evidence stay server-side.
BEGIN;

CREATE FUNCTION app.account_identity_json(p_account app.account) RETURNS jsonb
LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT jsonb_build_object('id',p_account.id,'username',CASE WHEN p_account.state='deleted' THEN 'Deleted player' ELSE p_account.username END,
  'isDeleted',p_account.state='deleted')
$$;
CREATE FUNCTION app.asset_identity_json(p_asset app.asset_definition) RETURNS jsonb
LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT CASE p_asset.kind WHEN 'coin' THEN jsonb_build_object('kind','coin','currencyKey',p_asset.currency_key)
 WHEN 'card' THEN jsonb_build_object('kind','card','printingId',p_asset.printing_id)
 WHEN 'pack' THEN jsonb_build_object('kind','pack','recipeId',p_asset.recipe_id) END
$$;
CREATE FUNCTION app.settlement_assessment_json(p_assessment jsonb) RETURNS jsonb
LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT CASE WHEN p_assessment IS NULL OR p_assessment='null'::jsonb THEN NULL ELSE jsonb_build_object(
  'outgoing',jsonb_build_object('policyId',p_assessment->'outgoing'->'policyId','catalogueRevision',p_assessment->'outgoing'->'catalogueRevision','total',p_assessment->'outgoing'->'total'),
  'incoming',jsonb_build_object('policyId',p_assessment->'incoming'->'policyId','catalogueRevision',p_assessment->'incoming'->'catalogueRevision','total',p_assessment->'incoming'->'total'),
  'outgoingAllowed',p_assessment->'outgoingAllowed','incomingAllowed',p_assessment->'incomingAllowed',
  'senderRestricted',p_assessment->'senderRestricted','recipientRestricted',p_assessment->'recipientRestricted') END
$$;

CREATE FUNCTION app.trade_offer_json(p_offer app.trade_offer) RETURNS jsonb
LANGUAGE sql STABLE SET search_path='' AS $$
 SELECT jsonb_build_object('id',p_offer.id,'sender',app.account_identity_json(sender),'recipient',app.account_identity_json(recipient),
  'revision',p_offer.terms_revision::text,'state',p_offer.state,
  'createdAt',to_char(p_offer.created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'give',(SELECT coalesce(jsonb_agg(jsonb_build_object('assetId',d.id,'quantity',l.quantity::text,'asset',app.asset_identity_json(d)) ORDER BY d.id),'[]')
   FROM app.trade_line l JOIN app.asset_definition d ON d.id=l.asset_definition_id WHERE l.trade_id=p_offer.id AND l.side='give'),
  'request',(SELECT coalesce(jsonb_agg(jsonb_build_object('assetId',d.id,'quantity',l.quantity::text,'asset',app.asset_identity_json(d)) ORDER BY d.id),'[]')
   FROM app.trade_line l JOIN app.asset_definition d ON d.id=l.asset_definition_id WHERE l.trade_id=p_offer.id AND l.side='request'),
  'settlement',app.settlement_assessment_json(p_offer.assessment))
 FROM app.account sender JOIN app.account recipient ON recipient.id=p_offer.recipient_id WHERE sender.id=p_offer.sender_id
$$;
CREATE FUNCTION app.market_listing_view(p_listing app.market_listing,p_actor uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SET search_path='' AS $$
DECLARE original jsonb:=app.market_listing_json(p_listing,p_actor); seller jsonb; buyer jsonb; asset jsonb;
BEGIN
 SELECT app.account_identity_json(a) INTO seller FROM app.account a WHERE id=p_listing.seller_id;
 SELECT app.account_identity_json(a) INTO buyer FROM app.account a WHERE id=p_listing.buyer_id;
 SELECT app.asset_identity_json(d) INTO asset FROM app.asset_definition d WHERE id=p_listing.asset_definition_id;
 RETURN (original-ARRAY['sellerId','buyerId','paidCoins','completedAt','assessment'])||jsonb_build_object('seller',seller,'asset',asset,
  'participant',CASE WHEN original ? 'buyerId' THEN jsonb_build_object('buyer',buyer,'paidCoins',original->'paidCoins',
   'completedAt',original->'completedAt','settlement',app.settlement_assessment_json(original->'assessment')) END);
END $$;

CREATE FUNCTION app.read_my_trade(p_trade uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; item jsonb;
BEGIN
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 SELECT app.trade_offer_json(t) INTO item FROM app.trade_offer t
 WHERE t.id=p_trade AND actor IN (t.sender_id,t.recipient_id) AND t.sealed_at IS NOT NULL;
 IF item IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('trade','unavailable')); END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'trade',item));
END $$;
CREATE FUNCTION app.read_my_trades(p_after uuid,p_limit integer,p_state text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; items jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 8 OR p_after='00000000-0000-0000-0000-000000000000'::uuid
 OR (p_state IS NOT NULL AND p_state NOT IN ('offered','accepted','rejected','canceled')) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('page','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 SELECT coalesce(jsonb_agg(page.item ORDER BY page.id),'[]') INTO items FROM (
  SELECT t.id,app.trade_offer_json(t) AS item FROM app.trade_offer t
  WHERE actor IN (t.sender_id,t.recipient_id) AND t.sealed_at IS NOT NULL AND (p_after IS NULL OR t.id>p_after)
   AND (p_state IS NULL OR t.state=p_state) ORDER BY t.id LIMIT p_limit) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,
  'nextCursor',CASE WHEN jsonb_array_length(items)=p_limit THEN items->-1->>'id' ELSE NULL END));
END $$;

CREATE FUNCTION app.read_my_market_listing(p_listing uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; original jsonb; item jsonb;
BEGIN
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 BEGIN original:=app.read_market_listing(actor,p_listing);
 EXCEPTION WHEN insufficient_privilege THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('listing','unavailable')); END;
 SELECT app.market_listing_view(l,actor) INTO item FROM app.market_listing l WHERE l.id=(original->>'id')::uuid;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'listing',item));
END $$;
CREATE FUNCTION app.read_my_market(p_after uuid,p_limit integer,p_owned boolean) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; original jsonb; items jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 8 OR p_owned IS NULL
 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('page','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 original:=app.read_market_page(actor,p_after,p_limit,p_owned);
 SELECT coalesce(jsonb_agg(app.market_listing_view(l,actor) ORDER BY l.id),'[]') INTO items
 FROM jsonb_array_elements(original->'items') i JOIN app.market_listing l ON l.id=(i->>'id')::uuid;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',original->'nextCursor'));
END $$;

REVOKE ALL ON FUNCTION app.account_identity_json(app.account),app.asset_identity_json(app.asset_definition),
 app.settlement_assessment_json(jsonb),app.trade_offer_json(app.trade_offer),app.market_listing_view(app.market_listing,uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
REVOKE ALL ON FUNCTION app.read_my_trade(uuid),app.read_my_trades(uuid,integer,text),app.read_my_market_listing(uuid),app.read_my_market(uuid,integer,boolean)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_trade(uuid),app.read_my_trades(uuid,integer,text),app.read_my_market_listing(uuid),app.read_my_market(uuid,integer,boolean) TO authenticated;
REVOKE EXECUTE ON FUNCTION app.read_market_listing(uuid,uuid),app.read_market_page(uuid,uuid,integer,boolean) FROM service_role;
-- The caller projection is also the boundary for private hold/assessment data.
-- Keep direct participant RLS reads to the safe immutable terms only.
REVOKE SELECT ON app.trade_offer FROM authenticated;
GRANT SELECT(id,sender_id,recipient_id,terms_revision,state,created_at) ON app.trade_offer TO authenticated;

-- Existing social reads share the anonymous identity encoder. The invoker/RLS
-- inventory projection stays unchanged rather than restoring its removed service reader.
CREATE OR REPLACE FUNCTION app.social_person_snapshot(p_actor uuid,p_peer uuid,p_at timestamptz) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT app.account_identity_json(a)||jsonb_build_object('relationship',CASE WHEN a.state='deleted' THEN 'none' WHEN f.state='accepted' THEN 'friend'
   WHEN f.state='pending' AND f.requested_by_id=p_actor THEN 'outgoing_request'
   WHEN f.state='pending' THEN 'incoming_request' ELSE 'none' END,
  'friendshipId',CASE WHEN a.state<>'deleted' AND f.state IN ('pending','accepted') THEN f.id END,
  'isBlocked',EXISTS(SELECT 1 FROM app.block b WHERE b.blocker_id=p_actor AND b.blocked_id=a.id),
  'canContact',app.contact_allowed_at(p_actor,a.id,p_at),
  'conversationId',c.id,
  'unreadMessages',(SELECT count(*)::text FROM app.message m JOIN app.notification n
   ON n.source_kind='dm_message' AND n.source_ref=m.id::text AND n.recipient_id=p_actor AND n.read_at IS NULL
   WHERE m.conversation_id=c.id AND m.sender_id<>p_actor),
  'supporter',app.active_supporter_tier(a.id) IS NOT NULL)
 FROM app.account a
 LEFT JOIN app.friendship f ON f.account_low_id=least(p_actor,a.id) AND f.account_high_id=greatest(p_actor,a.id)
 LEFT JOIN app.conversation c ON c.kind='dm' AND c.dm_low_account_id=least(p_actor,a.id)
  AND c.dm_high_account_id=greatest(p_actor,a.id)
  AND EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=c.id AND cm.account_id=p_actor)
 WHERE a.id=p_peer AND a.id<>p_actor
$$;

COMMIT;
