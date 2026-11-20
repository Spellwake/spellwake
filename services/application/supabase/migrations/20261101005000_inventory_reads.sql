BEGIN;

-- Quantities come from the existing hold-aware view, never another balance store.
CREATE FUNCTION app.read_inventory_page(p_actor uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid inventory page bounds';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending')) THEN
  RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='inventory account unavailable';
 END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
  SELECT i.id,jsonb_build_object('id',i.id,'assetId',d.id,'revision',i.revision::text,
   'owned',a.owned::text,'held',a.held::text,'available',a.available::text,'asset',
   CASE d.kind WHEN 'coin' THEN jsonb_build_object('kind','coin','currencyKey',d.currency_key)
    WHEN 'card' THEN jsonb_build_object('kind','card','printingId',d.printing_id)
    WHEN 'pack' THEN jsonb_build_object('kind','pack','recipeId',d.recipe_id) END) AS entry
  FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id
  JOIN app.asset_definition d ON d.id=i.asset_definition_id JOIN app.asset_availability a ON a.id=i.id
  WHERE o.account_id=p_actor AND i.owned_quantity>0 AND (p_after IS NULL OR i.id>p_after)
  ORDER BY i.id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('items',result,'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END);
END $$;
GRANT EXECUTE ON FUNCTION app.read_inventory_page(uuid,uuid,integer) TO service_role;

COMMIT;
