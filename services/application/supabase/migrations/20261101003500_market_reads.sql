BEGIN;

CREATE FUNCTION app.market_listing_json(p_listing app.market_listing,p_actor uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 result:=jsonb_build_object('id',p_listing.id,'sellerId',p_listing.seller_id,'assetId',p_listing.asset_definition_id,
 'quantity',p_listing.quantity::text,'totalPrice',p_listing.total_price_coins::text,'state',p_listing.state,'revision',p_listing.revision::text);
 IF p_actor IN (p_listing.seller_id,p_listing.buyer_id) THEN
  result:=result||jsonb_build_object('buyerId',p_listing.buyer_id,'paidCoins',p_listing.paid_coins::text,
  'completedAt',CASE WHEN p_listing.completed_at IS NULL THEN NULL ELSE to_char(p_listing.completed_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END,
  'assessment',p_listing.assessment);
 END IF;
 RETURN result;
END $$;

CREATE FUNCTION app.read_market_listing(p_actor uuid,p_listing uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE listing app.market_listing;
BEGIN
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending')) THEN
  RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='market account unavailable';
 END IF;
 SELECT * INTO listing FROM app.market_listing WHERE id=p_listing AND
 (state='listed' OR p_actor IN (seller_id,buyer_id));
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='listing unavailable'; END IF;
 RETURN app.market_listing_json(listing,p_actor);
END $$;

CREATE FUNCTION app.read_market_page(p_actor uuid,p_after uuid,p_limit integer,p_owned boolean) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_owned IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid market page bounds';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending')) THEN
  RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='market account unavailable';
 END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
 SELECT l.id,app.market_listing_json(l,p_actor) AS entry FROM app.market_listing l
 WHERE (p_after IS NULL OR l.id>p_after) AND
 ((p_owned AND p_actor IN (l.seller_id,l.buyer_id)) OR (NOT p_owned AND l.state='listed'))
 ORDER BY l.id LIMIT p_limit) page;
 RETURN jsonb_build_object('items',result,'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END);
END $$;
GRANT EXECUTE ON FUNCTION app.read_market_listing(uuid,uuid),app.read_market_page(uuid,uuid,integer,boolean) TO service_role;

COMMIT;
